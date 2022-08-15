use super::messages::{
    AtomicBroadcastDeser, AtomicBroadcastMsg, Proposal, StopMsg as NetStopMsg, StopMsgDeser,
    RECONFIG_ID,
};
use hashbrown::HashMap;
use kompact::{prelude::*, messaging::NetData};
#[cfg(feature = "track_timestamps")]
use quanta::{Clock, Instant};
use std::{
    sync::Arc,
    time::{Duration, SystemTime}, borrow::BorrowMut,
};
use synchronoise::{event::CountdownError, CountdownEvent};
use super::serialiser_ids;

pub struct PartitioningActorSer;
pub const PARTITIONING_ACTOR_SER: PartitioningActorSer = PartitioningActorSer {};
const INIT_ID: u8 = 1;
const INITACK_ID: u8 = 2;
const RUN_ID: u8 = 3;
const DONE_ID: u8 = 4;
const TESTDONE_ID: u8 = 5;
const STOP_ID: u8 = 6;
const STOP_ACK_ID: u8 = 13;
/* bytes to differentiate KVOperations*/
const READ_INV: u8 = 8;
const READ_RESP: u8 = 9;
const WRITE_INV: u8 = 10;
const WRITE_RESP: u8 = 11;

impl Serialisable for PartitioningActorMsg {
    fn ser_id(&self) -> u64 {
        serialiser_ids::PARTITIONING_ID
    }

    fn size_hint(&self) -> Option<usize> {
        match self {
            PartitioningActorMsg::Init(_) => Some(1000),
            PartitioningActorMsg::InitAck(_) => Some(5),
            PartitioningActorMsg::Run => Some(1),
            PartitioningActorMsg::Done => Some(1),
            PartitioningActorMsg::Stop => Some(1),
            PartitioningActorMsg::StopAck => Some(1),
            PartitioningActorMsg::TestDone(_) => Some(100000),
        }
    }

    fn serialise(&self, buf: &mut dyn BufMut) -> Result<(), SerError> {
        match self {
            PartitioningActorMsg::Init(i) => {
                buf.put_u8(INIT_ID);
                buf.put_u32(i.pid);
                buf.put_u32(i.init_id);
                match &i.init_data {
                    Some(data) => {
                        buf.put_u64(data.len() as u64);
                        for byte in data {
                            buf.put_u8(*byte);
                        }
                    }
                    None => buf.put_u64(0),
                }
                buf.put_u32(i.nodes.len() as u32);
                for node in i.nodes.iter() {
                    node.serialise(buf)?;
                }
            }
            PartitioningActorMsg::InitAck(id) => {
                buf.put_u8(INITACK_ID);
                buf.put_u32(*id);
            }
            PartitioningActorMsg::Run => buf.put_u8(RUN_ID),
            PartitioningActorMsg::Done => buf.put_u8(DONE_ID),
            PartitioningActorMsg::TestDone(timestamps) => {
                buf.put_u8(TESTDONE_ID);
                buf.put_u32(timestamps.len() as u32);
                for ts in timestamps {
                    buf.put_u64(ts.key);
                    match ts.operation {
                        KVOperation::ReadInvokation => buf.put_u8(READ_INV),
                        KVOperation::ReadResponse => {
                            buf.put_u8(READ_RESP);
                            buf.put_u32(ts.value.unwrap());
                        }
                        KVOperation::WriteInvokation => {
                            buf.put_u8(WRITE_INV);
                            buf.put_u32(ts.value.unwrap());
                        }
                        KVOperation::WriteResponse => {
                            buf.put_u8(WRITE_RESP);
                            buf.put_u32(ts.value.unwrap());
                        }
                    }
                    buf.put_i64(ts.time);
                    buf.put_u32(ts.sender);
                }
            }
            PartitioningActorMsg::Stop => buf.put_u8(STOP_ID),
            PartitioningActorMsg::StopAck => buf.put_u8(STOP_ACK_ID),
        }
        Ok(())
    }

    fn local(self: Box<Self>) -> Result<Box<dyn Any + Send>, Box<dyn Serialisable>> {
        Ok(self)
    }
}

#[derive(Debug, PartialEq)]
enum ExperimentState {
    LeaderElection,
    Running,
    ReconfigurationElection,
    Finished,
}

#[derive(Debug)]
pub enum LocalClientMessage {
    Prepare(Option<Vec<u8>>), 
    Start,
    Run,
    Propose(u64),
    Stop(Ask<(), MetaResults>), // (num_timed_out, latency)
}

enum Response {
    Normal(u64),
    Reconfiguration(Vec<u64>),
}

#[derive(Debug)]
struct ProposalMetaData {
    start_time: Option<SystemTime>,
    timer: ScheduledTimer,
}

impl ProposalMetaData {
    fn with(start_time: Option<SystemTime>, timer: ScheduledTimer) -> ProposalMetaData {
        ProposalMetaData { start_time, timer }
    }

    fn set_timer(&mut self, timer: ScheduledTimer) {
        self.timer = timer;
    }
}

#[derive(Debug)]
pub struct MetaResults {
    pub num_timed_out: u64,
    pub latencies: Vec<Duration>,
    pub timestamps_leader_changes: Option<(Vec<Duration>, Vec<(u64, Duration)>)>,
}

impl MetaResults {
    pub fn with(
        num_timed_out: u64,
        latencies: Vec<Duration>,
        timestamps_leader_changes: Option<(Vec<Duration>, Vec<(u64, Duration)>)>,
    ) -> Self {
        MetaResults {
            num_timed_out,
            latencies,
            timestamps_leader_changes,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KVOperation {
    ReadInvokation,
    ReadResponse,
    WriteInvokation,
    WriteResponse,
}

#[derive(Clone, Copy, Debug)]
pub struct KVTimestamp {
    pub key:       u64,
    pub operation: KVOperation,
    pub value:     Option<u32>,
    pub time:      i64,
    pub sender:    u32,
}

#[derive(Debug, Clone)]
pub enum PartitioningActorMsg {
    Init(Init),
    InitAck(u32),
    Run,
    Done,
    TestDone(Vec<KVTimestamp>),
    Stop,
    StopAck,
}

#[derive(Debug, Clone)]
pub struct Init {
    pub pid: u32,
    pub init_id: u32,
    pub nodes: Vec<ActorPath>,
    pub init_data: Option<Vec<u8>>,
}

impl Deserialiser<PartitioningActorMsg> for PartitioningActorSer {
    const SER_ID: u64 = serialiser_ids::PARTITIONING_ID;

    fn deserialise(buf: &mut dyn Buf) -> Result<PartitioningActorMsg, SerError> {
        match buf.get_u8() {
            INIT_ID => {
                let pid: u32 = buf.get_u32();
                let init_id: u32 = buf.get_u32();
                let data_len: u64 = buf.get_u64();
                let init_data = match data_len {
                    0 => None,
                    _ => {
                        let mut data = vec![];
                        for _ in 0..data_len {
                            data.push(buf.get_u8());
                        }
                        Some(data)
                    }
                };
                let nodes_len: u32 = buf.get_u32();
                let mut nodes: Vec<ActorPath> = Vec::new();
                for _ in 0..nodes_len {
                    let actorpath = ActorPath::deserialise(buf)?;
                    nodes.push(actorpath);
                }
                let init = Init {
                    pid,
                    init_id,
                    nodes,
                    init_data,
                };
                Ok(PartitioningActorMsg::Init(init))
            }
            INITACK_ID => {
                let init_id = buf.get_u32();
                Ok(PartitioningActorMsg::InitAck(init_id))
            }
            RUN_ID => Ok(PartitioningActorMsg::Run),
            DONE_ID => Ok(PartitioningActorMsg::Done),
            TESTDONE_ID => {
                let n: u32 = buf.get_u32();
                let mut timestamps: Vec<KVTimestamp> = Vec::new();
                for _ in 0..n {
                    let key = buf.get_u64();
                    let (operation, value) = match buf.get_u8() {
                        READ_INV => (KVOperation::ReadInvokation, None),
                        READ_RESP => (KVOperation::ReadResponse, Some(buf.get_u32())),
                        WRITE_INV => (KVOperation::WriteInvokation, Some(buf.get_u32())),
                        WRITE_RESP => (KVOperation::WriteResponse, Some(buf.get_u32())),
                        _ => panic!("Found unknown KVOperation id"),
                    };
                    let time = buf.get_i64();
                    let sender = buf.get_u32();
                    let ts = KVTimestamp {
                        key,
                        operation,
                        value,
                        time,
                        sender,
                    };
                    timestamps.push(ts);
                }
                Ok(PartitioningActorMsg::TestDone(timestamps))
            }
            STOP_ID => Ok(PartitioningActorMsg::Stop),
            STOP_ACK_ID => Ok(PartitioningActorMsg::StopAck),
            _ => Err(SerError::InvalidType(
                "Found unkown id, but expected PartitioningActorMsg.".into(),
            )),
        }
    }
}

#[derive(ComponentDefinition)]
pub struct Client {
    ctx: ComponentContext<Self>,
    num_proposals: u64,
    num_concurrent_proposals: u64,
    nodes: HashMap<u64, ActorPath>,
    node_vec: Vec<ActorPath>,
    reconfig: Option<(Vec<u64>, Vec<u64>)>,
    leader_election_latch: Arc<CountdownEvent>,
    finished_latch: Arc<CountdownEvent>,
    latest_proposal_id: u64,
    responses: HashMap<u64, Option<Duration>>,
    pending_proposals: HashMap<u64, ProposalMetaData>,
    timeout: Duration,
    current_leader: u64,
    state: ExperimentState,
    current_config: Vec<u64>,
    num_timed_out: u64,
    leader_changes: Vec<u64>,
    first_proposal_after_reconfig: Option<u64>,
    retry_proposals: Vec<(u64, Option<SystemTime>)>,
    stop_ask: Option<Ask<(), MetaResults>>,
    //P Actor vars
    n: u32,
    init_id: u32,
    init_ack_count: u32,
    prepare_latch: Arc<CountdownEvent>,
    #[cfg(feature = "track_timeouts")]
    timeouts: Vec<u64>,
    #[cfg(feature = "track_timeouts")]
    late_responses: Vec<u64>,
    #[cfg(feature = "track_timestamps")]
    clock: Clock,
    #[cfg(feature = "track_timestamps")]
    start: Option<Instant>,
    #[cfg(feature = "track_timestamps")]
    timestamps: HashMap<u64, Instant>,
    #[cfg(feature = "track_timestamps")]
    leader_changes_t: Vec<Instant>,
}

impl Client {
    pub fn with(
        initial_config: Vec<u64>,
        num_proposals: u64,
        num_concurrent_proposals: u64,
        nodes: HashMap<u64, ActorPath>,
        reconfig: Option<(Vec<u64>, Vec<u64>)>,
        timeout: Duration,
        leader_election_latch: Arc<CountdownEvent>,
        finished_latch: Arc<CountdownEvent>,
        prepare_latch: Arc<CountdownEvent>,
        node_vec: Vec<ActorPath>
    ) -> Client {
        println!("CREATING CLIENT");
        Client {
            ctx: ComponentContext::uninitialised(),
            num_proposals,
            num_concurrent_proposals,
            nodes,
            reconfig,
            leader_election_latch,
            finished_latch,
            latest_proposal_id: 0,
            responses: HashMap::with_capacity(num_proposals as usize),
            pending_proposals: HashMap::with_capacity(num_concurrent_proposals as usize),
            timeout,
            current_leader: 0,
            state: ExperimentState::LeaderElection,
            current_config: initial_config,
            num_timed_out: 0,
            leader_changes: vec![],
            first_proposal_after_reconfig: None,
            retry_proposals: Vec::with_capacity(num_concurrent_proposals as usize),
            stop_ask: None,
            n: 0,
            init_id: 0,
            node_vec,
            init_ack_count: 0,
            prepare_latch,
            #[cfg(feature = "track_timeouts")]
            timeouts: vec![],
            #[cfg(feature = "track_timeouts")]
            late_responses: vec![],
            #[cfg(feature = "track_timestamps")]
            clock: Clock::new(),
            #[cfg(feature = "track_timestamps")]
            start: None,
            #[cfg(feature = "track_timestamps")]
            timestamps: HashMap::with_capacity(num_proposals as usize),
            #[cfg(feature = "track_timestamps")]
            leader_changes_t: vec![],
        }
    }

    fn propose_normal(&self, id: u64, node: &ActorPath) {
        println!("!!!!!!!!!!!!!!!!!!!!!!!! PROPOSE NORMAL ID: {}", id);
        let mut data: Vec<u8> = Vec::with_capacity(8);
        data.put_u64(id);
        let p = Proposal::normal(data);
        //println!("propose to addr: {}", node.address());
        node.tell_serialised(AtomicBroadcastMsg::Proposal(p), self)
            .expect("Should serialise Proposal");
    }

    fn propose_reconfiguration(&self, node: &ActorPath) {
        println!("PROPOSING RECONFIG!!!!!!!!");
        let reconfig = self.reconfig.as_ref().unwrap();
        debug!(
            self.ctx.log(),
            "{}",
            format!("Sending reconfiguration: {:?}", reconfig)
        );
        let mut data: Vec<u8> = Vec::with_capacity(8);
        data.put_u64(RECONFIG_ID);
        let p = Proposal::reconfiguration(data, reconfig.clone());
        node.tell_serialised(AtomicBroadcastMsg::Proposal(p), self)
            .expect("Should serialise reconfig Proposal");
        #[cfg(feature = "track_timeouts")]
        {
            info!(self.ctx.log(), "Proposed reconfiguration. latest_proposal_id: {}, timed_out: {}, pending proposals: {}, min: {:?}, max: {:?}",
                self.latest_proposal_id, self.num_timed_out, self.pending_proposals.len(), self.pending_proposals.keys().min(), self.pending_proposals.keys().max());
        }
    }

    fn send_concurrent_proposals(&mut self) {
        println!("send concurrent");
        let num_inflight = self.pending_proposals.len() as u64;
        assert!(num_inflight <= self.num_concurrent_proposals);
        let available_n = self.num_concurrent_proposals - num_inflight;
        if num_inflight == self.num_concurrent_proposals || self.current_leader == 0 {
            return;
        }
        let leader = self.nodes.get(&self.current_leader).unwrap().clone();
        if self.retry_proposals.is_empty() {
            // normal case
            let from = self.latest_proposal_id + 1;
            let i = self.latest_proposal_id + available_n;
            let to = if i > self.num_proposals {
                self.num_proposals
            } else {
                i
            };
            if from > to {
                return;
            }
            let cache_start_time =
                self.num_concurrent_proposals == 1 || cfg!(feature = "track_latency");
            for id in from..=to {
                let current_time = match cache_start_time {
                    true => Some(SystemTime::now()),
                    _ => None,
                };
                self.propose_normal(id, &leader);
                let timer = self.schedule_once(self.timeout, move |c, _| c.proposal_timeout(id));
                let proposal_meta = ProposalMetaData::with(current_time, timer);
                self.pending_proposals.insert(id, proposal_meta);
            }
            self.latest_proposal_id = to;
        } else {
            let num_retry_proposals = self.retry_proposals.len();
            let n = if num_retry_proposals > available_n as usize {
                available_n as usize
            } else {
                num_retry_proposals
            };
            let retry_proposals: Vec<_> = self.retry_proposals.drain(0..n).collect();
            #[cfg(feature = "track_timeouts")]
            {
                let min = retry_proposals.iter().min();
                let max = retry_proposals.iter().max();
                let count = retry_proposals.len();
                let num_pending = self.pending_proposals.len();
                info!(self.ctx.log(), "Retrying proposals to node {}. Count: {}, min: {:?}, max: {:?}, num_pending: {}", self.current_leader, count, min, max, num_pending);
            }
            for (id, start_time) in retry_proposals {
                self.propose_normal(id, &leader);
                println!("RETRY PROPOSAL TIMEOUT");
                let timer = self.schedule_once(self.timeout, move |c, _| c.proposal_timeout(id));
                let meta = ProposalMetaData::with(start_time, timer);
                self.pending_proposals.insert(id, meta);
            }
        }
        println!("End concurrent!");
    }

    fn handle_normal_response(&mut self, id: u64, latency_res: Option<Duration>) {
        println!("Got response {}!!!!!", id);
        #[cfg(feature = "track_timestamps")]
        {
            let timestamp = self.clock.now();
            self.timestamps.insert(id, timestamp);
        }
        self.responses.insert(id, latency_res);
        let received_count = self.responses.len() as u64;
        if received_count == self.num_proposals && self.reconfig.is_none() {
            self.state = ExperimentState::Finished;
            //println!("decrement finished latch 253 handle normal response");
            self.finished_latch
                .decrement()
                .expect("Failed to countdown finished latch");
            if self.num_timed_out > 0 {
                info!(self.ctx.log(), "Got all responses with {} timeouts, Number of leader changes: {}, {:?}, Last leader was: {}", self.num_timed_out, self.leader_changes.len(), self.leader_changes, self.current_leader);
                #[cfg(feature = "track_timeouts")]
                {
                    let min = self.timeouts.iter().min();
                    let max = self.timeouts.iter().max();
                    let late_min = self.late_responses.iter().min();
                    let late_max = self.late_responses.iter().max();
                    info!(
                        self.ctx.log(),
                        "Timed out: Min: {:?}, Max: {:?}. Late responses: {}, min: {:?}, max: {:?}",
                        min,
                        max,
                        self.late_responses.len(),
                        late_min,
                        late_max
                    );
                }
            } else {
                info!(
                    self.ctx.log(),
                    "Got all responses. Number of leader changes: {}, {:?}, Last leader was: {}",
                    self.leader_changes.len(),
                    self.leader_changes,
                    self.current_leader
                );
            }
        } else if received_count == self.num_proposals / 2 && self.reconfig.is_some() {
            if let Some(leader) = self.nodes.get(&self.current_leader) {
                self.propose_reconfiguration(&leader);
            }
            println!("HANDLE NORMAL RESPONSE PROPOSAL TIMEOUT");
            let timer =
                self.schedule_once(self.timeout, move |c, _| c.proposal_timeout(RECONFIG_ID));
            let proposal_meta = ProposalMetaData::with(None, timer);
            self.pending_proposals.insert(RECONFIG_ID, proposal_meta);
        }
    }

    fn proposal_timeout(&mut self, id: u64) -> Handled {
        if self.responses.contains_key(&id)
            || self.state == ExperimentState::ReconfigurationElection
        {
            println!("PROPOSAL DID NOT TIME OUT");
            return Handled::Ok;
        }
        // info!(self.ctx.log(), "Timed out proposal {}", id);
        if id == RECONFIG_ID {
            if let Some(leader) = self.nodes.get(&self.current_leader) {
                self.propose_reconfiguration(leader);
            }
            let timer = self.schedule_once(self.timeout, move |c, _| c.proposal_timeout(id));
            let proposal_meta = self.pending_proposals.get_mut(&id).expect(&format!(
                "Could not find pending proposal id {}, latest_proposal_id: {}",
                id, self.latest_proposal_id
            ));
            proposal_meta.set_timer(timer);
        } else {
            println!("proposal timed out");
            self.num_timed_out += 1;
            let proposal_meta = self
                .pending_proposals
                .remove(&id)
                .expect("Timed out on proposal not in pending proposals");
            let latency = match proposal_meta.start_time {
                Some(start_time) => Some(
                    start_time
                        .elapsed()
                        .expect("Failed to get elapsed duration"),
                ),
                _ => None,
            };
            self.handle_normal_response(id, latency);
            self.send_concurrent_proposals();
            #[cfg(feature = "track_timeouts")]
            {
                self.timeouts.push(id);
            }
        }
        Handled::Ok
    }

    fn hold_back_proposals(&mut self, from: u64, to: u64) {
        for i in from..=to {
            match self.pending_proposals.remove(&i) {
                Some(ProposalMetaData { start_time, timer }) => {
                    self.cancel_timer(timer);
                    self.retry_proposals.push((i, start_time));
                }
                None => {
                    assert!(
                        self.responses.contains_key(&i),
                        "Hold back proposal not in pending and responses: {}. State {:?}, has_reconfig: {}", i, self.state, self.reconfig.is_some()
                    )
                }
            }
        }
    }

    fn deserialise_response(data: &mut dyn Buf) -> Response {
        match data.get_u64() {
            RECONFIG_ID => {
                let len = data.get_u32();
                let mut config = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    config.push(data.get_u64());
                }
                Response::Reconfiguration(config)
            }
            n => Response::Normal(n),
        }
    }

    fn send_stop(&self) {
        for ap in self.nodes.values() {
            ap.tell_serialised(NetStopMsg::Client, self)
                .expect("Failed to send Client stop");
        }
    }

    fn reply_stop_ask(&mut self) {
        let l = std::mem::take(&mut self.responses);
        let mut v: Vec<_> = l
            .into_iter()
            .filter(|(_, latency)| latency.is_some())
            .collect();
        v.sort();
        let latencies: Vec<Duration> = v.into_iter().map(|(_, latency)| latency.unwrap()).collect();
        let mut meta_results = MetaResults::with(self.num_timed_out, latencies, None);
        #[cfg(feature = "track_timestamps")]
        {
            let mut ts: Vec<_> = std::mem::take(&mut self.timestamps).into_iter().collect();
            ts.sort();
            let start = self.start.expect("No cached start point");
            let timestamps = ts
                .into_iter()
                .map(|(_, timestamp)| timestamp.duration_since(start))
                .collect();
            let leader_changes_t = std::mem::take(&mut self.leader_changes_t);
            let pid_ts = self
                .leader_changes
                .iter()
                .zip(leader_changes_t)
                .map(|(pid, ts)| (*pid, ts.duration_since(start)))
                .collect();
            meta_results.timestamps_leader_changes = Some((timestamps, pid_ts));
        }
        self.stop_ask
            .take()
            .expect("No stop promise!")
            .reply(meta_results)
            .expect("Failed to reply write latency file!");
    }
}

ignore_lifecycle!(Client);

impl Actor for Client {
    type Message = LocalClientMessage;

    fn receive_local(&mut self, msg: Self::Message) -> Handled {
        match msg {
            LocalClientMessage::Prepare(init_data) => {
                self.n = self.node_vec.len() as u32;
                for (r, node) in (&self.node_vec).iter().enumerate() {
                    let pid = r as u32 + 1;
                    let init = Init {
                        pid,
                        init_id: self.init_id,
                        nodes: self.node_vec.clone(),
                        init_data: init_data.clone(),
                    };
                    node.tell_serialised(PartitioningActorMsg::Init(init), self)
                        .expect("Should serialise");
                }
            }
            LocalClientMessage::Start => {
                for node in &self.node_vec {
                    node.tell_serialised(PartitioningActorMsg::Run, self)
                        .expect("Should serialise");
                }
            }
            //TODO: deleting LocalClientMessage::AllInitAcks, make sure this is nothing we actually need
            LocalClientMessage::Run => {
                self.state = ExperimentState::Running;
                assert_ne!(self.current_leader, 0);
                #[cfg(feature = "track_timestamps")]
                {
                    let now = self.clock.now();
                    self.start = Some(now);
                    self.leader_changes.push(self.current_leader);
                    self.leader_changes_t.push(now);
                }
                self.send_concurrent_proposals();
            }
            LocalClientMessage::Stop(a) => {
                println!("Stopping client!");
                let pending_proposals = std::mem::take(&mut self.pending_proposals);
                for proposal_meta in pending_proposals {
                    self.cancel_timer(proposal_meta.1.timer);
                }
                self.send_stop();
                self.stop_ask = Some(a);
            }
            LocalClientMessage::Propose(id) => {
                let mut data: Vec<u8> = Vec::with_capacity(8);
                data.put_u64(id);
                let p = Proposal::normal(data);
                let leader = self.nodes.get(&self.current_leader).unwrap().clone();
                println!("TELLING LEADER PROPOSE HEHEHE");
                leader.tell_serialised(AtomicBroadcastMsg::Proposal(p), self).expect("Should serialise Proposal");
                println!("sending propose prooposal timeout");
                let timer = self.schedule_once(self.timeout, move |c, _| c.proposal_timeout(id));
                let proposal_meta = ProposalMetaData::with(Some(SystemTime::now()), timer);
                self.pending_proposals.insert(id, proposal_meta);
            }
        }
        Handled::Ok
    }

    fn receive_network(&mut self, m: NetMessage) -> Handled {
        let NetMessage {
            sender: _,
            receiver: _,
            data,
            session: _,
        } = m;

        println!("CLIENT RECIEVE NETWORK");

        match_deser! {data {
            msg(am): AtomicBroadcastMsg [using AtomicBroadcastDeser] => {
                // info!(self.ctx.log(), "Handling {:?}", am);
                match am {
                    AtomicBroadcastMsg::FirstLeader(pid) => {
                        match self.state {
                            ExperimentState::LeaderElection=>{
                                self.current_leader=pid;
                                self.state=ExperimentState::Running;
                                assert_ne!(self.current_leader,0);
                            }
                            ExperimentState::Running => println!("Recieve network Running"),
                            ExperimentState::ReconfigurationElection => println!("Recieve network ReconfigurationElection"),
                            ExperimentState::Finished => println!("Recieve network Finished"), 
                        }
                        /*println!("GOT LEADER ALERT");
                        println!("self.state: {:?}", self.state);
                        if !self.current_config.contains(&pid) { return Handled::Ok; }
                        //match self.state {
                        //    ExperimentState::LeaderElection => {
                        self.current_leader = pid;
                        
                        self.state = ExperimentState::Running;
                        assert_ne!(self.current_leader, 0);
                        #[cfg(feature = "track_timestamps")]
                        {
                            let now = self.clock.now();
                            self.start = Some(now);
                            self.leader_changes.push(self.current_leader);
                            self.leader_changes_t.push(now);
                        }
                        self.send_concurrent_proposals();*/

                        //TODO: send proposals one by one through the main function instead. 
                        self.current_leader = pid;
                        if self.leader_election_latch.count() > 0{
                            match self.leader_election_latch.decrement() {
                                Ok(_) => info!(self.ctx.log(), "Got first leader: {}", pid),
                                Err(e) => if e != CountdownError::AlreadySet {
                                    panic!("Failed to decrement election latch: {:?}", e);
                                }
                            }
                        }
                        //    },
                            /*ExperimentState::ReconfigurationElection => {
                                if self.current_leader != pid {
                                    // info!(self.ctx.log(), "Got leader in ReconfigElection: {}. old: {}", pid, self.current_leader);
                                    self.current_leader = pid;
                                    self.leader_changes.push(pid);
                                    #[cfg(feature = "track_timestamps")] {
                                        self.leader_changes_t.push(self.clock.now());
                                    }
                                }
                                self.state = ExperimentState::Running;
                                if self.retry_proposals.is_empty() {
                                    self.first_proposal_after_reconfig = Some(self.latest_proposal_id);
                                }
                                self.send_concurrent_proposals();
                            },*/
                        //    _ => {},
                        //}
                    },
                    AtomicBroadcastMsg::ProposalResp(pr) => {
                        println!("PROPOSAL RESP RECEIVED");
                        if self.state == ExperimentState::Finished || self.state == ExperimentState::LeaderElection { 
                            //println!("self.state finished?: {}", self.state == ExperimentState::Finished);
                            //println!("self.state LeaderElection?: {}", self.state == ExperimentState::LeaderElection);
                            return Handled::Ok; 
                        }
                        let data = pr.data;
                        let response = Self::deserialise_response(&mut data.as_slice());

                        match response {
                            Response::Normal(id) => {
                                println!("normal response");
                                println!("RESPONSE IS {}", id);
                                if let Some(proposal_meta) = self.pending_proposals.remove(&id) {
                                    let latency = match proposal_meta.start_time {
                                        Some(start_time) => Some(start_time.elapsed().expect("Failed to get elapsed duration")),
                                        _ => None,
                                    };
                                    self.cancel_timer(proposal_meta.timer);
                                    if self.current_config.contains(&pr.latest_leader) && self.current_leader != pr.latest_leader && self.state != ExperimentState::ReconfigurationElection {
                                        // info!(self.ctx.log(), "Got leader in normal response: {}. old: {}", pr.latest_leader, self.current_leader);
                                        self.current_leader = pr.latest_leader;
                                        self.leader_changes.push(pr.latest_leader);
                                        #[cfg(feature = "track_timestamps")] {
                                            self.leader_changes_t.push(self.clock.now());
                                        }
                                    }
                                    self.handle_normal_response(id, latency);
                                    if self.state != ExperimentState::ReconfigurationElection {
                                        println!("send concurrent?");
                                        self.send_concurrent_proposals();
                                    }
                                }
                                #[cfg(feature = "track_timeouts")] {
                                    if self.timeouts.contains(&id) {
                                        /*if self.late_responses.is_empty() {
                                            info!(self.ctx.log(), "Got first late response: {}", id);
                                        }*/
                                        self.late_responses.push(id);
                                    }
                                }
                            }
                            Response::Reconfiguration(new_config) => {
                                println!("reconfig response");
                                println!("RESPONSE len IS {}", new_config.len());
                                if let Some(proposal_meta) = self.pending_proposals.remove(&RECONFIG_ID) {
                                    self.cancel_timer(proposal_meta.timer);
                                    if self.responses.len() as u64 == self.num_proposals {
                                        self.state = ExperimentState::Finished;
                                        println!("decrement finished latch 564 response reconfig");
                                        self.finished_latch.decrement().expect("Failed to countdown finished latch");
                                        info!(self.ctx.log(), "Got reconfig at last. {} proposals timed out. Leader changes: {}, {:?}, Last leader was: {}", self.num_timed_out, self.leader_changes.len(), self.leader_changes, self.current_leader);
                                    } else {
                                        self.reconfig = None;
                                        self.current_config = new_config;
                                        let leader_changed = self.current_leader != pr.latest_leader;
                                        info!(self.ctx.log(), "Reconfig OK, leader: {}, old: {}, current_config: {:?}", pr.latest_leader, self.current_leader, self.current_config);
                                        self.current_leader = pr.latest_leader;
                                        if self.current_leader == 0 {   // Paxos or Raft-remove-leader: wait for leader in new config
                                            self.state = ExperimentState::ReconfigurationElection;
                                        } else {    // Raft: continue if there is a leader
                                            self.state = ExperimentState::Running;
                                            self.send_concurrent_proposals();
                                            if leader_changed {
                                                self.leader_changes.push(pr.latest_leader);
                                                #[cfg(feature = "track_timestamps")] {
                                                    self.leader_changes_t.push(self.clock.now());
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    AtomicBroadcastMsg::PendingReconfiguration(data) => {
                        let dropped_proposal = data.as_slice().get_u64();
                        match self.state {
                            ExperimentState::Running => {
                                if self.reconfig.is_some() {    // still running in old config
                                    self.hold_back_proposals(dropped_proposal, self.latest_proposal_id);
                                    self.state = ExperimentState::ReconfigurationElection;  // wait for FirstLeader in new configuration before proposing more
                                } else {    // already running in new configuration
                                    match self.first_proposal_after_reconfig {
                                        Some(first_id) => {
                                            self.hold_back_proposals(dropped_proposal, first_id);
                                            self.send_concurrent_proposals();
                                        }
                                        _ => unreachable!("Running in new configuration but never cached first proposal in new config!?"),
                                    }
                                }
                            }
                            ExperimentState::ReconfigurationElection => {
                                self.hold_back_proposals(dropped_proposal, self.latest_proposal_id);
                            }
                            _ => {}
                        }
                    },
                    AtomicBroadcastMsg::PActorInitAck(id) => {
                        println!("PARTITIONING ACTOR INIT ACK");
                        self.init_ack_count += 1;
                        debug!(self.ctx.log(), "Got init ack {}/{}", &self.init_ack_count, &self.n);
                        if self.init_ack_count == self.n {
                            self.prepare_latch
                                .decrement()
                                .expect("Latch didn't decrement!");
                        }
                    },
                    _ => error!(self.ctx.log(), "Client received unexpected msg"),
                }
            },
            msg(stop): NetStopMsg [using StopMsgDeser] => {
                if let NetStopMsg::Peer(pid) = stop {
                    self.nodes.remove(&pid).unwrap_or_else(|| panic!("Got stop from unknown pid {}", pid));
                    if self.nodes.is_empty() {
                        self.reply_stop_ask();
                    }
                }
            },
            err(e) => error!(self.ctx.log(), "{}", &format!("Client failed to deserialise msg: {:?}", e)),
            default(_) => unimplemented!("Should be either AtomicBroadcastMsg or NetStopMsg"),
        }
        }

        Handled::Ok
    }
}