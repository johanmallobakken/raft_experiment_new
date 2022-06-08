extern crate raft as tikv_raft;

use crate::atomic_broadcast::{partitioning_actor::{PartitioningActorSer, PartitioningActorMsg}, messages::raft::RawRaftSer};

use super::{
    messages::{StopMsg as NetStopMsg, StopMsgDeser, *, raft::RaftMsg},
    storage::raft::*, partitioning_actor::KVOperation, atomic_broadcast::{SequenceResp, Done},
    serialiser_ids::ATOMICBCAST_ID,
};
use hashbrown::{HashMap, HashSet};
use kompact::prelude::*;
use protobuf::Message as PbMessage;
use rand::Rng;
use std::{borrow::Borrow, clone::Clone, marker::Send, ops::DerefMut, sync::Arc, time::Duration, os::unix::prelude::CommandExt, cell::RefCell, rc::Rc};
use tikv_raft::{
    prelude::{Message as TikvRaftMsg, *},
    StateRole,
};

const COMMUNICATOR: &str = "communicator";
const DELAY: Duration = Duration::from_millis(0);


fn create_rawraft_config(pid: u64) -> Config {
    let max_inflight_msgs: usize = 100000;//config["experiment"]["max_inflight"]
        //.as_i64()
        //.expect("Failed to load max_inflight") as usize;
    let election_timeout: usize = 5000; //config["experiment"]["election_timeout"]
        //.as_i64()
        //.expect("Failed to load election_timeout") as usize;
    let tick_period: usize = 100;//config["raft"]["tick_period"]
        //.as_i64()
        //.expect("Failed to load tick_period") as usize;
    let leader_hb_period: usize = 1000;//config["raft"]["leader_hb_period"]
        //.as_i64()
        //.expect("Failed to load leader_hb_period") as usize;
    let max_batch_size: u64 = 50000000;//config["raft"]["max_batch_size"]
        //.as_i64()
        //.expect("Failed to load max_batch_size") as u64;
    let pre_vote = false; //config["raft"]["pre_vote"]
        //.as_bool()
        //.expect("Failed to load pre_vote");
    let check_quorum = false; //config["raft"]["check_quorum"]
        //.as_bool()
        //.expect("Failed to load check_quorum");
    // convert from ms to logical clock ticks
    let election_tick = election_timeout / tick_period;
    let heartbeat_tick = leader_hb_period / tick_period;
    // info!(self.ctx.log(), "RawRaft config: election_tick={}, heartbeat_tick={}", election_tick, heartbeat_tick);
    let max_size_per_msg = max_batch_size;
    let c = Config {
        id: pid,
        election_tick,  // number of ticks without HB before starting election
        heartbeat_tick, // leader sends HB every heartbeat_tick
        max_inflight_msgs,
        max_size_per_msg,
        pre_vote,
        check_quorum,
        ..Default::default()
    };
    assert!(c.validate().is_ok(), "Invalid RawRaft config");
    c
}

#[derive(Debug)]
pub enum RaftCompMsg {
    Leader(bool, u64),
    ForwardReconfig(u64, (Vec<u64>, Vec<u64>)),
    KillComponents(Ask<(), Done>),
    GetSequence(Ask<(), SequenceResp>),
}

#[derive(Clone)]
pub struct RaftReplicaStruct<S>(Rc<RefCell<Option<RaftReplica<S>>>>)
where
    S: RaftStorage + Send + Clone + 'static,;

unsafe impl<S> Sync for RaftReplicaStruct<S> 
where
    S: RaftStorage + Send + Clone + 'static,
{}
unsafe impl<S> Send for RaftReplicaStruct<S>
where
    S: RaftStorage + Send + Clone + 'static,
{}

struct RaftReplica<S>
where
    S: RaftStorage + Send + Clone + 'static,
{
    //RaftReplica
    //ctx: ComponentContext<Self>,
    //supervisor: ActorRef<RaftCompMsg>,
    state: State,
    raw_raft: RawNode<S>,
    //communication_port: RequiredPort<CommunicationPort>,
    //timers: Option<(ScheduledTimer, ScheduledTimer)>,
    reconfig_state: ReconfigurationState,
    //current_leader: u64,
    //reconfig_policy: ReconfigurationPolicy,
    //num_peers: usize,
    //stopped: bool,
    //stopped_peers: HashSet<u64>,
    hb_proposals: Vec<Proposal>,
    max_inflight: usize,
    //sstop_ask: Option<Ask<(), ()>>,
}

impl<S> RaftReplica<S>
where
    S: RaftStorage + Send + Clone + 'static,
{
    pub fn with(
        raw_raft: RawNode<S>,
        //replica: ActorRef<RaftCompMsg>,
        reconfig_policy: ReconfigurationPolicy,
        num_peers: usize,
        max_inflight: usize,
    ) -> RaftReplica<S> {
        RaftReplica {
            //supervisor: replica,
            state: State::Election,
            raw_raft,
            //timers: None,
            reconfig_state: ReconfigurationState::None,
            //stopped_peers: HashSet::new(),
            //num_peers,
            hb_proposals: vec![],
            max_inflight,
            //stop_ask: None,
        }
    }
}

#[derive(ComponentDefinition)]
pub struct RaftComp<S>
where
    S: RaftStorage + Send + Clone + 'static,
{
    //Original RaftComp
    ctx: ComponentContext<Self>,
    pid: u64,
    initial_config: Vec<u64>,
    peers: HashMap<u64, ActorPath>,
    iteration_id: u32,
    stopped: bool,
    partitioning_actor: Option<ActorPath>,
    cached_client: Option<ActorPath>,
    current_leader: u64,
    reconfig_policy: ReconfigurationPolicy,
    raft_replica: RaftReplica<S>,
    timers: Option<(ScheduledTimer, ScheduledTimer)>,
}

impl<S> RaftComp<S>
where
    S: RaftStorage + Send + Clone + 'static,
{
    pub fn with(pid: u64, initial_config: Vec<u64>, reconfig_policy: ReconfigurationPolicy) -> Self {
        //FIXED:: raft replica struct here
        //TODO: check for inconsistency in pid, update pid and peers len in create components???
        let dir = &format!("./diskstorage_node{}", pid);
        let conf_state: (Vec<u64>, Vec<u64>) = (initial_config.clone(), vec![]);
        let store = S::new_with_conf_state(Some(dir), conf_state);
        let raw_raft_config = create_rawraft_config(pid);
        let raw_raft =
            RawNode::new(&raw_raft_config, store).expect("Failed to create tikv Raft");

        let max_inflight = raw_raft_config.max_inflight_msgs;//self.ctx.config()["experiment"]["max_inflight"]
            //.as_i64()
            //.expect("Failed to load max_inflight") as usize;
        
        let raft_replica = RaftReplica::with(
            raw_raft,
            reconfig_policy.clone(),
            0,
            max_inflight
        );

        RaftComp {
            ctx: ComponentContext::uninitialised(),
            pid,
            initial_config,
            peers: HashMap::new(),
            iteration_id: 0,
            stopped: false,
            partitioning_actor: None,
            cached_client: None,
            current_leader: 0,
            reconfig_policy,
            raft_replica,
            timers: None
        }
    }

    fn get_raft_replica(&self){
        /*let raft_replica_struct = self.raft_replica_struct.clone();
        let raft_option = raft_replica_struct.0.as_ref().take();
        raft_option.unwrap();*/ 
    }
    /* 
    fn create_rawraft_config(&self) -> Config {
        let config = self.ctx.config();
        let max_inflight_msgs = config["experiment"]["max_inflight"]
            .as_i64()
            .expect("Failed to load max_inflight") as usize;
        let election_timeout = config["experiment"]["election_timeout"]
            .as_i64()
            .expect("Failed to load election_timeout") as usize;
        let tick_period = config["raft"]["tick_period"]
            .as_i64()
            .expect("Failed to load tick_period") as usize;
        let leader_hb_period = config["raft"]["leader_hb_period"]
            .as_i64()
            .expect("Failed to load leader_hb_period") as usize;
        let max_batch_size = config["raft"]["max_batch_size"]
            .as_i64()
            .expect("Failed to load max_batch_size") as u64;
        let pre_vote = config["raft"]["pre_vote"]
            .as_bool()
            .expect("Failed to load pre_vote");
        let check_quorum = config["raft"]["check_quorum"]
            .as_bool()
            .expect("Failed to load check_quorum");
        // convert from ms to logical clock ticks
        let election_tick = election_timeout / tick_period;
        let heartbeat_tick = leader_hb_period / tick_period;
        // info!(self.ctx.log(), "RawRaft config: election_tick={}, heartbeat_tick={}", election_tick, heartbeat_tick);
        let max_size_per_msg = max_batch_size;
        let c = Config {
            id: self.pid,
            election_tick,  // number of ticks without HB before starting election
            heartbeat_tick, // leader sends HB every heartbeat_tick
            max_inflight_msgs,
            max_size_per_msg,
            pre_vote,
            check_quorum,
            ..Default::default()
        };
        assert!(c.validate().is_ok(), "Invalid RawRaft config");
        c
    }*/

    fn create_components(&mut self) -> Handled {
        /* 
        let dir = &format!("./diskstorage_node{}", self.pid);
        let conf_state: (Vec<u64>, Vec<u64>) = (self.initial_config.clone(), vec![]);
        let store = S::new_with_conf_state(Some(dir), conf_state);
        let raw_raft =
            RawNode::new(&self.create_rawraft_config(), store).expect("Failed to create tikv Raft");
        let max_inflight = self.ctx.config()["experiment"]["max_inflight"]
            .as_i64()
            .expect("Failed to load max_inflight") as usize;
        
        println!("!!!!raft_replica_struct!!!!");
        self.raft_replica_struct = Some(RaftReplica::with(
            raw_raft,
            self.actor_ref(),
            self.reconfig_policy.clone(),
            self.peers.len(),
            max_inflight
        ));*/

        self.partitioning_actor
        .take()
        .expect("No partitioning actor found!")
        .tell_serialised(
            PartitioningActorMsg::InitAck(self.iteration_id),
            self,
        )
        .expect("Should serialise InitAck");
        //println!("CREATED COMPONENTS");
        Handled::Ok
    }

    fn start_components(&mut self) {
        //println!("RUN Starting components and timerssssssss!!");
        /*let raft = self.raft_replica.as_ref().expect("No raft comp to start!");
        let communicator = self
            .communicator
            .as_ref()
            .expect("No communicator to start!");
        self.ctx.system().start(raft);
        self.ctx.system().start(communicator);*/

        //Done in RaftReplica on_start
        let bc = BufferConfig::default();
        self.ctx.borrow().init_buffers(Some(bc), None);
        self.start_timers();
    }

    fn start_timers(&mut self) {
        let config = self.ctx.config();
        let outgoing_period = config["experiment"]["outgoing_period"]
            .as_duration()
            .expect("Failed to load outgoing_period");
        let tick_period = config["raft"]["tick_period"]
            .as_i64()
            .expect("Failed to load tick_period") as u64;
        //println!("STARTING TIMERRRRRRRRRRRRRRRRRRRR");
        let ready_timer = self.schedule_periodic(DELAY, outgoing_period, move |c, _| c.on_ready());
        let tick_timer =
            self.schedule_periodic(DELAY, Duration::from_millis(tick_period), move |rc, _| {
                rc.tick()
            });
        self.timers = Some((ready_timer, tick_timer));
    }

    fn on_ready(&mut self) -> Handled {
        //println!("ON READYYYY");

        //println!("ON READYYYY INSIDE");
        
        if !self.raft_replica.raw_raft.has_ready() {
            return Handled::Ok;
        }
        let mut store = self.raft_replica.raw_raft.raft.raft_log.store.clone();

        // Get the `Ready` with `RawNode::ready` interface.
        let mut ready = self.raft_replica.raw_raft.ready();

        // Persistent raft logs. It's necessary because in `RawNode::advance` we stabilize
        // raft logs to the latest position.
        if let Err(e) = store.append_log(ready.entries()) {
            error!(
                self.ctx.log(),
                "{}",
                format!("persist raft log fail: {:?}, need to retry or panic", e)
            );
            return Handled::Ok;
        }

        // Apply the snapshot. It's necessary because in `RawNode::advance` we stabilize the snapshot.
        if *ready.snapshot() != Snapshot::default() {
            unimplemented!("Should not be any snapshots to handle!");
        }

        //FIXED: send messages from here, replacing communicator logic TRIGGER RAFT REPLICA LOGIC WHEN RECIEVING CommunicatorMsg::RawRaftMsg
        // Send out the messages come from the node.
        let mut ready_msgs = Vec::with_capacity(self.raft_replica.max_inflight);
        std::mem::swap(&mut ready.messages, &mut ready_msgs);
        for msg in ready_msgs {
            //TODO: Send messages over the network to the destination RaftComp

            let receiver = self.peers.get(&msg.get_to()).unwrap_or_else(|| {
                panic!(
                    "Could not find actorpath for id={}. Known peers: {:?}. RaftMsg: {:?}",
                    &msg.get_to(),
                    self.peers.keys(),
                    msg
                )
            });
            receiver
                .tell_serialised(RaftMsg(msg), self)
                .expect("Should serialise RaftMsg");

            //self.raft_replica_handle_atomicbroadcastcompmsg_rawraftmsg(msg);
            /*if !self.stopped && self.raft_replica.reconfig_state != ReconfigurationState::Removed
            {
                //FIXED: pass rm, call this functioon upon recieving message
                //self.step(rm);
                println!("STYEPPPINGGGGGGGGGGGGG");
                let _ = self.raft_replica.raw_raft.step(msg);
            }*/
        }
        // let mut next_conf_change: Option<ConfChangeType> = None;
        // Apply all committed proposals.
        if let Some(committed_entries) = ready.committed_entries.take() {
            //println!("?????? committed_entries");

            for entry in &committed_entries {
                if entry.data.is_empty() {
                    // From new elected leaders.
                    continue;
                }
                if let EntryType::EntryConfChange = entry.get_entry_type() {
                    if self.raft_replica.reconfig_state == ReconfigurationState::Removed
                        || self.raft_replica.reconfig_state == ReconfigurationState::Finished
                    {
                        continue;
                    }
                    // For conf change messages, make them effective.
                    let mut cc = ConfChange::default();
                    cc.merge_from_bytes(&entry.data).unwrap();
                    let change_type = cc.get_change_type();
                    match &change_type {
                        ConfChangeType::BeginMembershipChange => {
                            let reconfig = cc.get_configuration();
                            let start_index = cc.get_start_index();
                            debug!(
                                self.ctx.log(),
                                "{}",
                                format!(
                                    "Beginning reconfiguration to: {:?}, start_index: {}",
                                    reconfig, start_index
                                )
                            );
                            self.raft_replica.raw_raft
                                .raft
                                .begin_membership_change(&cc)
                                .expect("Failed to begin reconfiguration");

                            assert!(self.raft_replica.raw_raft.raft.is_in_membership_change());
                        }
                        ConfChangeType::FinalizeMembershipChange => {
                            self.raft_replica.raw_raft
                                .raft
                                .finalize_membership_change(&cc)
                                .expect("Failed to finalize reconfiguration");

                            let current_conf = self.raft_replica.raw_raft.raft.prs().configuration().clone();
                            let current_voters = current_conf.voters();
                            if !current_voters.contains(&self.raft_replica.raw_raft.raft.id) {
                                self.stop_timers();
                                self.raft_replica.reconfig_state = ReconfigurationState::Removed;
                            } else {
                                self.raft_replica.reconfig_state = ReconfigurationState::Finished;
                            }
                            let leader = self.raft_replica.raw_raft.raft.leader_id;
                            if leader == 0 {
                                // leader was removed
                                self.raft_replica.state = State::Election; // reset leader so it can notify client when new leader emerges
                                if self.raft_replica.reconfig_state != ReconfigurationState::Removed {
                                    // campaign later if we are not removed
                                    let mut rng = rand::thread_rng();
                                    let config = self.ctx.config();
                                    let tick_period = config["raft"]["tick_period"]
                                        .as_i64()
                                        .expect("Failed to load tick_period")
                                        as usize;
                                    let election_timeout = config["experiment"]["election_timeout"]
                                        .as_i64()
                                        .expect("Failed to load election_timeout")
                                        as usize;
                                    let initial_election_factor = config["experiment"]
                                        ["initial_election_factor"]
                                        .as_i64()
                                        .expect("Failed to load initial_election_factor")
                                        as usize;
                                    // randomize with ticks to ensure at least one tick difference in timeout
                                    let intial_timeout_ticks =
                                        (election_timeout / initial_election_factor) / tick_period;
                                    let rnd = rng
                                        .gen_range(intial_timeout_ticks, 2 * intial_timeout_ticks);
                                    let timeout = rnd * tick_period;
                                    self.schedule_once(
                                        Duration::from_millis(timeout as u64),
                                        move |c, _| c.try_campaign_leader(),
                                    );
                                }
                            }
                            let conf_len = current_voters.len();
                            let mut data: Vec<u8> = Vec::with_capacity(8 + 4 + 8 * conf_len);
                            data.put_u64(RECONFIG_ID);
                            data.put_u32(conf_len as u32);
                            for pid in current_voters {
                                data.put_u64(*pid);
                            }
                            let cs = ConfState::from(current_conf);
                            store.set_conf_state(cs, None);

                            let pr = ProposalResp::with(data, leader);
                            //FIXED: Send messages from here replacing communicator logic SEND TO CLIENT
                            /*self.communication_port
                                .trigger(CommunicatorMsg::ProposalResponse(pr));*/
                            self.cached_client
                                .as_ref()
                                .expect("No cached client!")
                                .tell_serialised(AtomicBroadcastMsg::ProposalResp(pr), self)
                                .expect("Should serialise ProposalResp");
                        }
                        _ => unimplemented!(),
                    }
                } else {
                    // normal proposals
                    if self.raft_replica.raw_raft.raft.state == StateRole::Leader {
                        let pr =
                            ProposalResp::with(entry.get_data().to_vec(), self.raft_replica.raw_raft.raft.id);
                        //FIXED: Send messages from here replacing communicator logic, SEND TO CLIENT
                        /*self.communication_port
                            .trigger(CommunicatorMsg::ProposalResponse(pr));*/
                        self.cached_client
                            .as_ref()
                            .expect("No cached client!")
                            .tell_serialised(AtomicBroadcastMsg::ProposalResp(pr), self)
                            .expect("Should serialise ProposalResp");
                    }
                }
            }
            if let Some(last_committed) = committed_entries.last() {
                store
                    .set_hard_state(last_committed.index, last_committed.term)
                    .expect("Failed to set hardstate");
            }
        }
        // Call `RawNode::advance` interface to update position flags in the raft.
        self.raft_replica.raw_raft.advance(ready);
        Handled::Ok
    }

    fn try_campaign_leader(&mut self) -> Handled {
        //println!("try_campaign_leader!!!!!!!!!");
        // start campaign to become leader if none has been elected yet
        let leader = self.raft_replica.raw_raft.raft.leader_id;
        if leader == 0 && self.raft_replica.state == State::Election {
            let _ = self.raft_replica.raw_raft.campaign();
        }

        Handled::Ok
    }

    fn stop_timers(&mut self) {
        if let Some(timers) = self.timers.take() {
            self.cancel_timer(timers.0);
            self.cancel_timer(timers.1);
        }
    }

    fn stop_components(&mut self) -> Handled {
        self.stopped = true;
        //TODO: trigger stopping logic of raft replica RaftReplicaMsg::Stop(Ask::new(p, ())))
        // info!(self.ctx.log(), "Stopping components");
        /*let raft = self
            .raft_replica
            .as_ref()
            .expect("Got stop but no Raft replica")
        let stop_f = raft
            .actor_ref()
            .ask_with(|p| RaftReplicaMsg::Stop(Ask::new(p, ())));;
        Handled::block_on(self, move |_| async move {
            stop_f.await.expect("Failed to stop RaftReplica");
        })*/

        self.raft_replica_receive_local_raftreplicamsg_stop();
        Handled::Ok
    }

    fn tick(&mut self) -> Handled {
        self.raft_replica.raw_raft.tick();
        let leader = self.raft_replica.raw_raft.raft.leader_id;
        //println!("leader id: {}", leader);
        if leader != 0 {
            //println!("LEADER NOT EQUAL TO 0");
            if !self.raft_replica.hb_proposals.is_empty() {
                let proposals = std::mem::take(&mut self.raft_replica.hb_proposals);
                for proposal in proposals {
                    self.propose(proposal);
                }
            }
            if leader != self.current_leader {
                //info!(self.ctx.log(), "New leader: {}, old: {}", leader, self.current_leader);
                self.current_leader = leader;
                let notify_client = if self.raft_replica.state == State::Election {
                    self.raft_replica.state = State::Running;
                    true
                } else {
                    false
                };

                //FIXED: Simply trigger logic for RaftCompMsg::Leader here
                /* 
                self.supervisor
                    .tell(RaftCompMsg::Leader(notify_client, leader));
                */

                self.raft_comp_receive_local_raftcompmsg_leader(notify_client, leader);
            }
        }
        Handled::Ok
    }

    fn propose(&mut self, proposal: Proposal) {
        if self.raft_replica.raw_raft.raft.leader_id == 0 {
            self.raft_replica.hb_proposals.push(proposal);
            return;
        }
        match proposal.reconfig {
            Some(mut reconfig) => {
                if let ReconfigurationState::None = self.raft_replica.reconfig_state {
                    let leader_pid = self.raft_replica.raw_raft.raft.leader_id;
                    if leader_pid != self.raft_replica.raw_raft.raft.id {
                        //FIXED: Simply trigger RaftCompMsg::ForwardReconfig logic from here
                        /*self.supervisor
                            .tell(RaftCompMsg::ForwardReconfig(leader_pid, reconfig));*/
                        self.raft_comp_receive_local_raftcompmsg_forwardreconfig(leader_pid, reconfig);
                        return;
                    }
                    let mut current_config = self.raft_replica.raw_raft.raft.prs().configuration().voters().clone();
                    match self.reconfig_policy {
                        ReconfigurationPolicy::ReplaceLeader => {
                            let mut add_nodes: Vec<u64> = reconfig
                                .0
                                .drain(..)
                                .filter(|pid| !current_config.contains(pid))
                                .collect();
                            current_config.remove(&leader_pid);
                            let mut new_voters = current_config.into_iter().collect::<Vec<u64>>();
                            new_voters.append(&mut add_nodes);
                            let new_config = (new_voters, vec![]);
                            // info!(self.ctx.log(), "Joint consensus remove leader: my pid: {}, reconfig: {:?}", leader_pid, new_config);
                            self.raft_replica.raw_raft
                                .raft
                                .propose_membership_change(new_config)
                                .expect(
                                "Failed to propose joint consensus reconfiguration (remove leader)",
                            );
                        }
                        ReconfigurationPolicy::ReplaceFollower => {
                            if !reconfig.0.contains(&leader_pid) {
                                let my_pid = self.raft_replica.raw_raft.raft.id;
                                let mut add_nodes: Vec<u64> = reconfig
                                    .0
                                    .drain(..)
                                    .filter(|pid| !current_config.contains(pid))
                                    .collect();
                                let follower_pid: u64 = **(current_config
                                    .iter()
                                    .filter(|pid| *pid != &my_pid)
                                    .collect::<Vec<&u64>>()
                                    .first()
                                    .expect("No followers found"));
                                current_config.remove(&follower_pid);
                                let mut new_voters =
                                    current_config.into_iter().collect::<Vec<u64>>();
                                new_voters.append(&mut add_nodes);
                                let new_config = (new_voters, vec![]);
                                // info!(self.ctx.log(), "Joint consensus remove follower: my pid: {}, reconfig: {:?}", leader_pid, new_config);
                                self.raft_replica.raw_raft
                                    .raft
                                    .propose_membership_change(new_config)
                                    .expect("Failed to propose joint consensus reconfiguration");
                            } else {
                                self.raft_replica.raw_raft.raft.propose_membership_change(reconfig).expect("Failed to propose joint consensus reconfiguration (remove follower)");
                            }
                        }
                    }
                    self.raft_replica.reconfig_state = ReconfigurationState::Pending;
                }
            }
            None => {
                // i.e normal operation
                let data = proposal.data;
                self.raft_replica.raw_raft.propose(vec![], data).unwrap_or_else(|_| {
                    panic!(
                        "Failed to propose. leader: {}, lead_transferee: {:?}",
                        self.raft_replica.raw_raft.raft.leader_id, self.raft_replica.raw_raft.raft.lead_transferee
                    )
                });
            }
        }
    }

    fn raft_replica_receive_local_raftreplicamsg_sequencereq(&mut self, sr: Ask<(), SequenceResp>){
        //FIXED: sr sent with RaftReplicaMsg::SequenceReq(sr) must be sent with this function
        //println!("pre raw_raft.raft.raft_log.all_entries()?????");
        let raft_entries: Vec<Entry> = self.raft_replica.raw_raft.raft.raft_log.all_entries();
        let mut sequence: Vec<u64> = Vec::with_capacity(raft_entries.len());
        let mut unique = HashSet::new();
        //println!("pre for entry in raft entries?????");
        for entry in raft_entries {
            if entry.get_entry_type() == EntryType::EntryNormal && !&entry.data.is_empty() {
                let id = entry.data.as_slice().get_u64();
                if id != 0 {
                    sequence.push(id);
                }
                unique.insert(id);
            }
        }
        info!(
            self.ctx.log(),
            "Got SequenceReq: my seq_len={}. Unique={}",
            sequence.len(),
            unique.len()
        );
        sr.reply(SequenceResp::with(self.pid, sequence));
    }

    fn raft_replica_receive_local_raftreplicamsg_stop(&mut self) {
        //TODO: Send stop massage to peers? 
        /*self.communication_port
            .trigger(CommunicatorMsg::SendStop(self.raw_raft.raft.id, true));*/
        
        //COMMUNICATOR LOGIC
        debug!(self.ctx.log(), "Sending stop to {:?}", self.peers.keys());
        for ap in self.peers.values() {
            ap.tell_serialised(NetStopMsg::Peer(self.pid), self)
                .expect("Should serialise StopMsg")
        }
        
        //if ack_client {
        self.cached_client
            .as_ref()
            .expect("No cached client!")
            .tell_serialised(NetStopMsg::Peer(self.pid), self)
            .expect("Should serialise StopMsg");
        //}

        //COMMUNICATOR LOGIC END

        self.stop_timers();
        /*if self.stopped_peers.len() == self.num_peers {
            ask.reply(()).expect("Failed to reply Stop ask");
        } else {
            self.stop_ask = Some(ask);
        }*/
    }

    fn raft_replica_receive_local_raftreplicamsg_propose(&mut self, p: Proposal){
        //FIXED: Get p from RaftReplicaMsg::Propose through function
        if self.raft_replica.reconfig_state != ReconfigurationState::Removed {
            self.propose(p);
        }
    }

    fn raft_replica_handle_atomicbroadcastcompmsg_rawraftmsg(&mut self, rm: Message) {
        if !self.stopped && self.raft_replica.reconfig_state != ReconfigurationState::Removed
        {
            //FIXED: pass rm, call this functioon upon recieving message
            self.step(rm);
        }
    }

    fn step(&mut self, msg: TikvRaftMsg) {
        let _ = self.raft_replica.raw_raft.step(msg);
    }

    fn raft_comp_receive_local_raftcompmsg_forwardreconfig(&mut self, leader_pid: u64, reconfig: (Vec<u64>, Vec<u64>)){
        //FIXED: pass leader_pid, reconfig to function
        self.current_leader = leader_pid;
        let mut data: Vec<u8> = Vec::with_capacity(8);
        data.put_u64(RECONFIG_ID);
        let p = Proposal::reconfiguration(data, reconfig);
        self.peers
            .get(&leader_pid)
            .expect("No actorpath to current leader")
            .tell_serialised(AtomicBroadcastMsg::Proposal(p), self)
            .expect("Should serialise")
    }

    fn raft_comp_receive_local_raftcompmsg_leader(&mut self, notify_client: bool, pid: u64) {
        //FIXED: call fn and send in notify_client, pid
        debug!(self.ctx.log(), "Node {} became leader", pid);
        if notify_client {
            self.cached_client
                .as_ref()
                .expect("No cached client!")
                .tell_serialised(AtomicBroadcastMsg::FirstLeader(pid), self)
                .expect("Should serialise FirstLeader");
        }
        self.current_leader = pid
    }

    /*fn create_components(&mut self) -> Handled {
        println!("created raft replica!!!!!!!!!!!!!!!!!");
        println!("Need to spawn replica communicator etc through simulationscenario");
        let mut communicator_peers: HashMap<u64, ActorPath> =
            HashMap::with_capacity(self.peers.len());
        for (pid, ap) in &self.peers {
            match ap {
                ActorPath::Named(n) => {
                    let sys_path = n.system();
                    let protocol = sys_path.protocol();
                    let port = sys_path.port();
                    let addr = sys_path.address();
                    let named_communicator = NamedPath::new(
                        protocol,
                        *addr,
                        port,
                        vec![format!("{}{}-{}", COMMUNICATOR, pid, self.iteration_id)],
                    );
                    communicator_peers.insert(*pid, ActorPath::Named(named_communicator));
                }
                _ => unimplemented!(),
            }
        }

        let system = self.ctx.system();
        let dir = &format!("./diskstorage_node{}", self.pid);
        let conf_state: (Vec<u64>, Vec<u64>) = (self.initial_config.clone(), vec![]);
        let store = S::new_with_conf_state(Some(dir), conf_state);
        let raw_raft =
            RawNode::new(&self.create_rawraft_config(), store).expect("Failed to create tikv Raft");
        let max_inflight = self.ctx.config()["experiment"]["max_inflight"]
            .as_i64()
            .expect("Failed to load max_inflight") as usize;
        let (raft_replica, raft_f) = system.create_and_register(|| {
            RaftReplica::with(
                raw_raft,
                self.actor_ref(),
                self.reconfig_policy.clone(),
                self.peers.len(),
                max_inflight,
            )
        });
        let (communicator, comm_f) = system.create_and_register(|| {
            Communicator::with(
                communicator_peers,
                self.cached_client
                    .as_ref()
                    .expect("No cached client")
                    .clone(),
            )
        });
        let communicator_alias = format!("{}{}-{}", COMMUNICATOR, self.pid, self.iteration_id);
        let comm_alias_f = system.register_by_alias(&communicator, communicator_alias);
        biconnect_components::<CommunicationPort, _, _>(&communicator, &raft_replica)
            .expect("Could not connect components!");
        self.raft_replica = Some(raft_replica);
        self.communicator = Some(communicator);
        Handled::block_on(self, move |mut async_self| async move {
            raft_f
                .await
                .unwrap()
                .expect("Timed out registering raft component");
            comm_f
                .await
                .unwrap()
                .expect("Timed out registering communicator");
            comm_alias_f
                .await
                .unwrap()
                .expect("Timed out registering communicator alias");
            async_self
                .partitioning_actor
                .take()
                .expect("No partitioning actor found!")
                .tell_serialised(
                    PartitioningActorMsg::InitAck(async_self.iteration_id),
                    async_self.deref_mut(),
                )
                .expect("Should serialise InitAck");
        })
    }*/
    /* 
    fn start_components(&self) {
        let raft = self.raft_replica.as_ref().expect("No raft comp to start!");
        let communicator = self
            .communicator
            .as_ref()
            .expect("No communicator to start!");
        self.ctx.system().start(raft);
        self.ctx.system().start(communicator);
    }

    fn stop_components(&mut self) -> Handled {
        self.stopped = true;
        // info!(self.ctx.log(), "Stopping components");
        let raft = self
            .raft_replica
            .as_ref()
            .expect("Got stop but no Raft replica");
        let stop_f = raft
            .actor_ref()
            .ask_with(|p| RaftReplicaMsg::Stop(Ask::new(p, ())));
        Handled::block_on(self, move |_| async move {
            stop_f.await.expect("Failed to stop RaftReplica");
        })
    }

    fn kill_components(&mut self, ask: Ask<(), Done>) -> Handled {
        let system = self.ctx.system();
        let mut kill_futures = Vec::with_capacity(2);

        if let Some(raft) = self.raft_replica.take() {
            let kill_raft = system.kill_notify(raft);
            kill_futures.push(kill_raft);
        }
        if let Some(communicator) = self.communicator.take() {
            let kill_comm = system.kill_notify(communicator);
            kill_futures.push(kill_comm);
        }

        Handled::block_on(self, move |_| async move {
            for f in kill_futures {
                f.await.expect("Failed to kill");
            }
            ask.reply(Done).expect("Failed to reply done");
        })
    }*/
}

impl<S> ComponentLifecycle for RaftComp<S> where S: RaftStorage + Send + Clone + 'static {}

impl<S> Actor for RaftComp<S>
where
    S: RaftStorage + Send + Clone + 'static,
{
    type Message = RaftCompMsg;

    fn receive_local(&mut self, msg: Self::Message) -> Handled {
        //println!("RECIEVE LOCAL? Shoould never happen");
        match msg {
            RaftCompMsg::Leader(notify_client, pid) => {

                self.raft_comp_receive_local_raftcompmsg_leader(notify_client, pid);

                //FIXED: move logic to function
                /*debug!(self.ctx.log(), "Node {} became leader", pid);
                if notify_client {
                    self.cached_client
                        .as_ref()
                        .expect("No cached client!")
                        .tell_serialised(AtomicBroadcastMsg::FirstLeader(pid), self)
                        .expect("Should serialise FirstLeader");
                }
                self.current_leader = pid*/
            }
            RaftCompMsg::ForwardReconfig(leader_pid, reconfig) => {

                self.raft_comp_receive_local_raftcompmsg_forwardreconfig(leader_pid, reconfig);

                //FIXED: move logic to function
                /* 
                self.current_leader = leader_pid;
                let mut data: Vec<u8> = Vec::with_capacity(8);
                data.put_u64(RECONFIG_ID);
                let p = Proposal::reconfiguration(data, reconfig);
                self.peers
                    .get(&leader_pid)
                    .expect("No actorpath to current leader")
                    .tell_serialised(AtomicBroadcastMsg::Proposal(p), self)
                    .expect("Should serialise")*/
            }
            RaftCompMsg::KillComponents(ask) => {
                //return self.kill_components(ask);
            }
            RaftCompMsg::GetSequence(ask) => {

                println!("GET SEQUENCE REQQQQQQ");
                self.raft_replica_receive_local_raftreplicamsg_sequencereq(ask);



                /*return Handled::block_on(self, move | async_self| async move {
                    let raft_replica = async_self.raft_replica.as_ref().expect("No raft replica");
                    let seq = raft_replica
                    .actor_ref()
                    .ask_with(|promise| {
                        RaftReplicaMsg::SequenceReq(Ask::new(promise, ()))
                    }).await.expect("failed to ask");
                    let sr = SequenceResp::with(async_self.pid, seq);
                    ask.reply(sr).expect("Failed to reply SequenceResp");
                });*/
            }
        }
        Handled::Ok
    }

    fn receive_network(&mut self, m: NetMessage) -> Handled {
        //println!("RECIEVE NETWORK");
        match m.data.ser_id {
            ATOMICBCAST_ID => {
                //println!("ATOMICBCASTMSGGGGGG");
                if !self.stopped {
                    if self.current_leader == self.pid || self.current_leader == 0 {
                        // if no leader, let raftcomp hold back
                        if let AtomicBroadcastMsg::Proposal(p) = m
                            .try_deserialise_unchecked::<AtomicBroadcastMsg, AtomicBroadcastDeser>()
                            .expect("Should be AtomicBroadcastMsg!")
                        {
                            self.raft_replica_receive_local_raftreplicamsg_propose(p);
                            /*self.raft_replica
                                .as_ref()
                                .expect("No active RaftComp")
                                .actor_ref()
                                .tell(RaftReplicaMsg::Propose(p));*/
                        }
                    } else if self.current_leader > 0 {
                        let leader = self.peers.get(&self.current_leader).unwrap_or_else(|| {
                            panic!(
                                "Could not get leader's actorpath. Pid: {}",
                                self.current_leader
                            )
                        });
                        leader.forward_with_original_sender(m, self);
                    }
                }
            }
            _ => {
                let NetMessage { sender, data, .. } = m;
                match_deser! {data {
                    msg(p): PartitioningActorMsg [using PartitioningActorSer] => {
                        match p {
                            PartitioningActorMsg::Init(init) => {
                                //println!("PARTITIONINGACTORMSG::INIT");
                                info!(self.ctx.log(), "Raft got init, pid: {}", init.pid);
                                self.current_leader = 0;
                                self.iteration_id = init.init_id;
                                let my_pid = init.pid as u64;
                                let ser_client = init.init_data.expect("Init should include ClientComp's actorpath");
                                let client = ActorPath::deserialise(&mut ser_client.as_slice()).expect("Failed to deserialise Client's actorpath");
                                self.cached_client = Some(client);
                                
                                self.peers = init.nodes.into_iter().enumerate().map(|(idx, ap)| (idx as u64 + 1, ap)).filter(|(pid, _)| pid != &my_pid).collect();
                                //println!("ASSERT: self.pid: {} my_pid: {}", self.pid, my_pid);
                                assert_eq!(self.pid, my_pid);
                                self.pid = my_pid;
                                self.partitioning_actor = Some(sender);
                                self.stopped = false;
                                let handled = self.create_components();
                                return handled;
                            },
                            PartitioningActorMsg::Run => {
                                //println!("PARTITIONINGACTORMSG::RUN");
                                self.start_components();
                                //FIXED: on_start logic for RaftReplica and Communicator(?)
                            },
                            _ => {},
                        }
                    },
                    msg(r): Message [using RawRaftSer] => {
                        //TODO: trigger rawraft on recieve logic
                        //println!("RECEIVED RAWRAFTSER WOHOOOOOOOOOO");
                        self.raft_replica_handle_atomicbroadcastcompmsg_rawraftmsg(r);
                    },
                    msg(stop): NetStopMsg [using StopMsgDeser] => {
                        if let NetStopMsg::Client = stop {
                            println!("NETSTOPMSG::CLIENT");
                            // info!(self.ctx.log(), "Got client stop");
                            //assert!(!self.stopped);
                            //return self.stop_components();
                            //stop logic for raft_replica and communicator 
                            assert!(!self.stopped);
                            return self.stop_components();
                        } else if let NetStopMsg::Peer(pid) = stop {
                            println!("RECEIVED NETSTOP PEEER WOHOOOOOOOOOOOOO");
                            /*assert!(
                                self.stopped_peers.insert(from_pid),
                                "Got duplicate stop from {}",
                                pid
                            );
                            // info!(self.ctx.log(), "Got stop from {}. received: {}, num_peers: {}", from_pid, self.stopped_peers.len(), self.num_peers);
                            if self.stopped_peers.len() == self.num_peers && self.stopped {
                                self.stop_ask
                                    .take()
                                    .expect("No stop ask")
                                    .reply(())
                                    .expect("Failed to reply Stop ask");
                            }*/
                        }
                    },
                    err(e) => error!(self.ctx.log(), "Error deserialising msg: {:?}", e),
                    default(_) => unimplemented!("Expected either PartitioningActorMsg or NetStopMsg!"),
                }
                }
            }
        }
        Handled::Ok
    }
}

#[derive(Debug)]
pub enum RaftReplicaMsg {
    Propose(Proposal),
    Stop(Ask<(), ()>),
    SequenceReq(Ask<(), Vec<u64>>),
}

#[derive(Clone, Debug)]
pub enum ReconfigurationPolicy {
    ReplaceLeader,
    ReplaceFollower,
}

#[derive(Debug, PartialEq)]
enum ReconfigurationState {
    None,
    Pending,
    Finished,
    Removed,
}

#[derive(Debug, PartialEq)]
enum State {
    Election,
    Running,
}

/*
#[derive(ComponentDefinition)]
pub struct RaftReplica<S>
where
    S: RaftStorage + Send + Clone + 'static,
{
    ctx: ComponentContext<Self>,
    supervisor: ActorRef<RaftCompMsg>,
    state: State,
    raw_raft: RawNode<S>,
    communication_port: RequiredPort<CommunicationPort>,
    timers: Option<(ScheduledTimer, ScheduledTimer)>,
    reconfig_state: ReconfigurationState,
    current_leader: u64,
    reconfig_policy: ReconfigurationPolicy,
    num_peers: usize,
    stopped: bool,
    stopped_peers: HashSet<u64>,
    hb_proposals: Vec<Proposal>,
    max_inflight: usize,
    stop_ask: Option<Ask<(), ()>>,
}

impl<S> ComponentLifecycle for RaftReplica<S>
where
    S: RaftStorage + Send + Clone + 'static,
{
    fn on_start(&mut self) -> Handled {
        println!("???????????????????????RAFTREPLICA STARTED???????????????????????");
        let bc = BufferConfig::default();
        self.ctx.borrow().init_buffers(Some(bc), None);
        self.start_timers();
        Handled::Ok
    }

    fn on_kill(&mut self) -> Handled {
        // info!(self.ctx.log(), "Got kill! RaftLog commited: {}", self.raw_raft.raft.raft_log.committed);
        self.stop_timers();
        self.raw_raft
            .mut_store()
            .clear()
            .expect("Failed to clear storage!");
        Handled::Ok
    }
}

impl<S> Actor for RaftReplica<S>
where
    S: RaftStorage + Send + Clone + 'static,
{
    type Message = RaftReplicaMsg;

    fn receive_local(&mut self, msg: Self::Message) -> Handled {
        match msg {
            RaftReplicaMsg::Propose(p) => {
                if self.reconfig_state != ReconfigurationState::Removed {
                    self.propose(p);
                }
            }
            RaftReplicaMsg::Stop(ask) => {
                println!("STOP REPLICA");
                self.communication_port
                    .trigger(CommunicatorMsg::SendStop(self.raw_raft.raft.id, true));
                self.stop_timers();
                self.stopped = true;
                if self.stopped_peers.len() == self.num_peers {
                    ask.reply(()).expect("Failed to reply Stop ask");
                } else {
                    self.stop_ask = Some(ask);
                }
            }
            RaftReplicaMsg::SequenceReq(sr) => {
                println!("pre raw_raft.raft.raft_log.all_entries()?????");
                let raft_entries: Vec<Entry> = self.raw_raft.raft.raft_log.all_entries();
                let mut sequence: Vec<u64> = Vec::with_capacity(raft_entries.len());
                let mut unique = HashSet::new();
                println!("pre for entry in raft entries?????");
                for entry in raft_entries {
                    if entry.get_entry_type() == EntryType::EntryNormal && !&entry.data.is_empty() {
                        let id = entry.data.as_slice().get_u64();
                        if id != 0 {
                            sequence.push(id);
                        }
                        unique.insert(id);
                    }
                }
                info!(
                    self.ctx.log(),
                    "Got SequenceReq: my seq_len={}. Unique={}",
                    sequence.len(),
                    unique.len()
                );
                sr.reply(sequence)
                    .expect("Failed to respond SequenceReq ask");
            }
        }
        Handled::Ok
    }

    fn receive_network(&mut self, _msg: NetMessage) -> Handled {
        // ignore
        Handled::Ok
    }
}

impl<S> Require<CommunicationPort> for RaftReplica<S>
where
    S: RaftStorage + Send + Clone + 'static,
{
    fn handle(&mut self, msg: AtomicBroadcastCompMsg) -> Handled {
        match msg {
            AtomicBroadcastCompMsg::RawRaftMsg(rm)
                if !self.stopped && self.reconfig_state != ReconfigurationState::Removed =>
            {
                self.step(rm);
            }
            AtomicBroadcastCompMsg::StopMsg(from_pid) => {
                assert!(
                    self.stopped_peers.insert(from_pid),
                    "Got duplicate stop from {}",
                    from_pid
                );
                // info!(self.ctx.log(), "Got stop from {}. received: {}, num_peers: {}", from_pid, self.stopped_peers.len(), self.num_peers);
                if self.stopped_peers.len() == self.num_peers && self.stopped {
                    self.stop_ask
                        .take()
                        .expect("No stop ask")
                        .reply(())
                        .expect("Failed to reply Stop ask");
                }
            }
            _ => {}
        }
        Handled::Ok
    }
}

impl<S> RaftReplica<S>
where
    S: RaftStorage + Send + Clone + 'static,
{
    pub fn with(
        raw_raft: RawNode<S>,
        replica: ActorRef<RaftCompMsg>,
        reconfig_policy: ReconfigurationPolicy,
        num_peers: usize,
        max_inflight: usize,
    ) -> RaftReplica<S> {
        RaftReplica {
            ctx: ComponentContext::uninitialised(),
            supervisor: replica,
            state: State::Election,
            raw_raft,
            communication_port: RequiredPort::uninitialised(),
            timers: None,
            reconfig_state: ReconfigurationState::None,
            current_leader: 0,
            reconfig_policy,
            stopped: false,
            stopped_peers: HashSet::new(),
            num_peers,
            hb_proposals: vec![],
            max_inflight,
            stop_ask: None,
        }
    }

    fn start_timers(&mut self) {
        let config = self.ctx.config();
        let outgoing_period = config["experiment"]["outgoing_period"]
            .as_duration()
            .expect("Failed to load outgoing_period");
        let tick_period = config["raft"]["tick_period"]
            .as_i64()
            .expect("Failed to load tick_period") as u64;
        println!("STARTING TIMERRRRRRRRRRRRRRRRRRRR");
        let ready_timer = self.schedule_periodic(DELAY, outgoing_period, move |c, _| c.on_ready());
        let tick_timer =
            self.schedule_periodic(DELAY, Duration::from_millis(tick_period), move |rc, _| {
                rc.tick()
            });
        self.timers = Some((ready_timer, tick_timer));
    }

    fn stop_timers(&mut self) {
        if let Some(timers) = self.timers.take() {
            self.cancel_timer(timers.0);
            self.cancel_timer(timers.1);
        }
    }

    fn try_campaign_leader(&mut self) -> Handled {
        // start campaign to become leader if none has been elected yet
        let leader = self.raw_raft.raft.leader_id;
        if leader == 0 && self.state == State::Election {
            let _ = self.raw_raft.campaign();
        }
        Handled::Ok
    }

    fn tick(&mut self) -> Handled {
        println!("TICKKKKKKKKKKKK");
        self.raw_raft.tick();
        let leader = self.raw_raft.raft.leader_id;
        if leader != 0 {
            if !self.hb_proposals.is_empty() {
                let proposals = std::mem::take(&mut self.hb_proposals);
                for proposal in proposals {
                    self.propose(proposal);
                }
            }
            if leader != self.current_leader {
                // info!(self.ctx.log(), "New leader: {}, old: {}", leader, self.current_leader);
                self.current_leader = leader;
                let notify_client = if self.state == State::Election {
                    self.state = State::Running;
                    true
                } else {
                    false
                };
                self.supervisor
                    .tell(RaftCompMsg::Leader(notify_client, leader));
            }
        }
        Handled::Ok
    }

    fn step(&mut self, msg: TikvRaftMsg) {
        let _ = self.raw_raft.step(msg);
    }

    fn propose(&mut self, proposal: Proposal) {
        if self.raw_raft.raft.leader_id == 0 {
            self.hb_proposals.push(proposal);
            return;
        }
        match proposal.reconfig {
            Some(mut reconfig) => {
                if let ReconfigurationState::None = self.reconfig_state {
                    let leader_pid = self.raw_raft.raft.leader_id;
                    if leader_pid != self.raw_raft.raft.id {
                        self.supervisor
                            .tell(RaftCompMsg::ForwardReconfig(leader_pid, reconfig));
                        return;
                    }
                    let mut current_config =
                        self.raw_raft.raft.prs().configuration().voters().clone();
                    match self.reconfig_policy {
                        ReconfigurationPolicy::ReplaceLeader => {
                            let mut add_nodes: Vec<u64> = reconfig
                                .0
                                .drain(..)
                                .filter(|pid| !current_config.contains(pid))
                                .collect();
                            current_config.remove(&leader_pid);
                            let mut new_voters = current_config.into_iter().collect::<Vec<u64>>();
                            new_voters.append(&mut add_nodes);
                            let new_config = (new_voters, vec![]);
                            // info!(self.ctx.log(), "Joint consensus remove leader: my pid: {}, reconfig: {:?}", leader_pid, new_config);
                            self.raw_raft
                                .raft
                                .propose_membership_change(new_config)
                                .expect(
                                "Failed to propose joint consensus reconfiguration (remove leader)",
                            );
                        }
                        ReconfigurationPolicy::ReplaceFollower => {
                            if !reconfig.0.contains(&leader_pid) {
                                let my_pid = self.raw_raft.raft.id;
                                let mut add_nodes: Vec<u64> = reconfig
                                    .0
                                    .drain(..)
                                    .filter(|pid| !current_config.contains(pid))
                                    .collect();
                                let follower_pid: u64 = **(current_config
                                    .iter()
                                    .filter(|pid| *pid != &my_pid)
                                    .collect::<Vec<&u64>>()
                                    .first()
                                    .expect("No followers found"));
                                current_config.remove(&follower_pid);
                                let mut new_voters =
                                    current_config.into_iter().collect::<Vec<u64>>();
                                new_voters.append(&mut add_nodes);
                                let new_config = (new_voters, vec![]);
                                // info!(self.ctx.log(), "Joint consensus remove follower: my pid: {}, reconfig: {:?}", leader_pid, new_config);
                                self.raw_raft
                                    .raft
                                    .propose_membership_change(new_config)
                                    .expect("Failed to propose joint consensus reconfiguration");
                            } else {
                                self.raw_raft.raft.propose_membership_change(reconfig).expect("Failed to propose joint consensus reconfiguration (remove follower)");
                            }
                        }
                    }
                    self.reconfig_state = ReconfigurationState::Pending;
                }
            }
            None => {
                // i.e normal operation
                let data = proposal.data;
                self.raw_raft.propose(vec![], data).unwrap_or_else(|_| {
                    panic!(
                        "Failed to propose. leader: {}, lead_transferee: {:?}",
                        self.raw_raft.raft.leader_id, self.raw_raft.raft.lead_transferee
                    )
                });
            }
        }
    }

    fn on_ready(&mut self) -> Handled {
        if !self.raw_raft.has_ready() {
            return Handled::Ok;
        }
        let mut store = self.raw_raft.raft.raft_log.store.clone();

        // Get the `Ready` with `RawNode::ready` interface.
        let mut ready = self.raw_raft.ready();

        // Persistent raft logs. It's necessary because in `RawNode::advance` we stabilize
        // raft logs to the latest position.
        if let Err(e) = store.append_log(ready.entries()) {
            error!(
                self.ctx.log(),
                "{}",
                format!("persist raft log fail: {:?}, need to retry or panic", e)
            );
            return Handled::Ok;
        }

        // Apply the snapshot. It's necessary because in `RawNode::advance` we stabilize the snapshot.
        if *ready.snapshot() != Snapshot::default() {
            unimplemented!("Should not be any snapshots to handle!");
        }

        // Send out the messages come from the node.
        let mut ready_msgs = Vec::with_capacity(self.max_inflight);
        std::mem::swap(&mut ready.messages, &mut ready_msgs);
        for msg in ready_msgs {
            self.communication_port
                .trigger(CommunicatorMsg::RawRaftMsg(msg));
        }
        // let mut next_conf_change: Option<ConfChangeType> = None;
        // Apply all committed proposals.
        if let Some(committed_entries) = ready.committed_entries.take() {
            for entry in &committed_entries {
                if entry.data.is_empty() {
                    // From new elected leaders.
                    continue;
                }
                if let EntryType::EntryConfChange = entry.get_entry_type() {
                    if self.reconfig_state == ReconfigurationState::Removed
                        || self.reconfig_state == ReconfigurationState::Finished
                    {
                        continue;
                    }
                    // For conf change messages, make them effective.
                    let mut cc = ConfChange::default();
                    cc.merge_from_bytes(&entry.data).unwrap();
                    let change_type = cc.get_change_type();
                    match &change_type {
                        ConfChangeType::BeginMembershipChange => {
                            let reconfig = cc.get_configuration();
                            let start_index = cc.get_start_index();
                            debug!(
                                self.ctx.log(),
                                "{}",
                                format!(
                                    "Beginning reconfiguration to: {:?}, start_index: {}",
                                    reconfig, start_index
                                )
                            );
                            self.raw_raft
                                .raft
                                .begin_membership_change(&cc)
                                .expect("Failed to begin reconfiguration");

                            assert!(self.raw_raft.raft.is_in_membership_change());
                        }
                        ConfChangeType::FinalizeMembershipChange => {
                            self.raw_raft
                                .raft
                                .finalize_membership_change(&cc)
                                .expect("Failed to finalize reconfiguration");

                            let current_conf = self.raw_raft.raft.prs().configuration().clone();
                            let current_voters = current_conf.voters();
                            if !current_voters.contains(&self.raw_raft.raft.id) {
                                self.stop_timers();
                                self.reconfig_state = ReconfigurationState::Removed;
                            } else {
                                self.reconfig_state = ReconfigurationState::Finished;
                            }
                            let leader = self.raw_raft.raft.leader_id;
                            if leader == 0 {
                                // leader was removed
                                self.state = State::Election; // reset leader so it can notify client when new leader emerges
                                if self.reconfig_state != ReconfigurationState::Removed {
                                    // campaign later if we are not removed
                                    let mut rng = rand::thread_rng();
                                    let config = self.ctx.config();
                                    let tick_period = config["raft"]["tick_period"]
                                        .as_i64()
                                        .expect("Failed to load tick_period")
                                        as usize;
                                    let election_timeout = config["experiment"]["election_timeout"]
                                        .as_i64()
                                        .expect("Failed to load election_timeout")
                                        as usize;
                                    let initial_election_factor = config["experiment"]
                                        ["initial_election_factor"]
                                        .as_i64()
                                        .expect("Failed to load initial_election_factor")
                                        as usize;
                                    // randomize with ticks to ensure at least one tick difference in timeout
                                    let intial_timeout_ticks =
                                        (election_timeout / initial_election_factor) / tick_period;
                                    let rnd = rng
                                        .gen_range(intial_timeout_ticks, 2 * intial_timeout_ticks);
                                    let timeout = rnd * tick_period;
                                    self.schedule_once(
                                        Duration::from_millis(timeout as u64),
                                        move |c, _| c.try_campaign_leader(),
                                    );
                                }
                            }
                            let conf_len = current_voters.len();
                            let mut data: Vec<u8> = Vec::with_capacity(8 + 4 + 8 * conf_len);
                            data.put_u64(RECONFIG_ID);
                            data.put_u32(conf_len as u32);
                            for pid in current_voters {
                                data.put_u64(*pid);
                            }
                            let cs = ConfState::from(current_conf);
                            store.set_conf_state(cs, None);

                            let pr = ProposalResp::with(data, leader);
                            self.communication_port
                                .trigger(CommunicatorMsg::ProposalResponse(pr));
                        }
                        _ => unimplemented!(),
                    }
                } else {
                    // normal proposals
                    if self.raw_raft.raft.state == StateRole::Leader {
                        let pr =
                            ProposalResp::with(entry.get_data().to_vec(), self.raw_raft.raft.id);
                        self.communication_port
                            .trigger(CommunicatorMsg::ProposalResponse(pr));
                    }
                }
            }
            if let Some(last_committed) = committed_entries.last() {
                store
                    .set_hard_state(last_committed.index, last_committed.term)
                    .expect("Failed to set hardstate");
            }
        }
        // Call `RawNode::advance` interface to update position flags in the raft.
        self.raw_raft.advance(ready);
        Handled::Ok
    }
}*/