use std::{net::SocketAddr, time::Duration, sync::Arc, borrow::BorrowMut, fmt::Display};

use crate::atomic_broadcast::{atomic_broadcast::{run_experiment, check_quorum, check_validity, check_uniform_agreement}, partitioning_actor::IterationControlMsg, client::LocalClientMessage};

mod atomic_broadcast;

extern crate raft as tikv_raft;
use hashbrown::HashMap;
use kompact::prelude::{KompactSystem, ActorPath, Recipient, KompactConfig, BufferConfig, Ask, promise, FutureCollection, NetworkConfig, DeadletterBox, Serialisable, SimulationScenario, SimulatedScheduling, GetState, Component, SimulationError, Invariant, BufMut};

use atomic_broadcast::{
    raft::{
        RaftCompMsg, RaftComp, ReconfigurationPolicy as RaftReconfigurationPolicy
    }, 
    atomic_broadcast::{
        REGISTER_TIMEOUT, RAFT_PATH, CONFIG_PATH, SequenceResp
    },
    client::{
        Client
    },
    partitioning_actor::{
        PartitioningActor
    },
    messages::Proposal
};

use tikv_raft::eraftpb::Entry;
use synchronoise::CountdownEvent;
use tikv_raft::{storage::MemStorage, StateRole, RaftLog, Storage as TikvStorage};
use kompact::prelude::ActorRefFactory;
type Storage = MemStorage;

fn get_experiment_configs(last_node_id: u64) -> (Vec<u64>, Vec<u64>) {
    if last_node_id % 2 == 0 {
        let initial_config: Vec<_> = (1..last_node_id).collect();
        let reconfig: Vec<_> = (2..=last_node_id).collect();
        (initial_config, reconfig)
    } else {
        let initial_config: Vec<_> = (1..=last_node_id).collect();
        (initial_config, vec![])
    }
}

#[derive(Debug)]
pub struct GetSequence(Ask<(), SequenceResp>);

impl Into<RaftCompMsg> for GetSequence {
    fn into(self) -> RaftCompMsg {
        RaftCompMsg::GetSequence(self.0)
    }
}

#[derive(Debug)]
pub struct RaftState{
    id: u64,
    term: u64,
    vote: u64, //?
    state: StateRole,
    leader_id: u64,
    //RAFT LOG
    //log_entries: Vec<Entry>,
    log_entries_len: usize,
    //log_unstable: Unstable,
    log_committed: u64,
    log_applied: u64
}

impl Display for RaftState{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl GetState<RaftState> for Component<RaftComp<Storage>> {
    fn get_state(&self) -> RaftState {

        let def = &self.mutable_core.lock().unwrap().definition;

        let log = def.raft_replica.raw_raft.raft.raft_log.store.clone();

        let mut entries:Vec<Entry> = vec![];

        if log.last_index().unwrap() > 1 {
            //println!("{}", log.last_index().unwrap());
            entries = log.entries(log.first_index().unwrap(), log.last_index().unwrap(), 9999).unwrap();
        }

        RaftState{
            id: def.raft_replica.raw_raft.raft.id,
            term: def.raft_replica.raw_raft.raft.term,
            vote: def.raft_replica.raw_raft.raft.vote,
            state: def.raft_replica.raw_raft.raft.state,
            leader_id: def.raft_replica.raw_raft.raft.leader_id,
            //RAFT LOG
            log_entries_len: entries.len(),
            //log_unstable: self.raft_replica.raw_raft.raft.raft_log.unstable.copy(),
            log_committed: def.raft_replica.raw_raft.raft.raft_log.committed,
            log_applied: def.raft_replica.raw_raft.raft.raft_log.applied,
        }
    }
}



fn print_all_raft_states(components: Vec<Component<RaftComp<MemStorage>>>){
    for component in components {
        println!("{:?}", component.get_state());
    }
}

fn todo_raft_normal_test(mut simulation_scenario: SimulationScenario<RaftState>) {
    //Simulation config 5
    let num_nodes = 3;
    //Create nodes function invocation 1
    let (systems, actor_paths, actor_refs) = create_nodes(simulation_scenario, num_nodes);
    //Create state monitors/inspections invariants 3
    //Register state monitors (implicitly checked for every step) 3
    //Simulation Sequence 1
    //Output/Log to console or file final state 1
    todo!()
}

fn create_nodes(mut simulation_scenario: SimulationScenario<RaftState>, num_nodes: u64) -> (Vec<KompactSystem>, Vec<ActorPath>, Vec<Recipient<GetSequence>>) {
    //CREATE NODES TDOD: extract to separate method
    let mut systems = Vec::with_capacity(num_nodes as usize);
    let mut actor_paths = Vec::with_capacity(num_nodes as usize);
    let mut actor_refs = Vec::with_capacity(num_nodes as usize);
    //let mut comp_defs = Vec::with_capacity(num_nodes as usize);

    let mut conf = KompactConfig::default();
    conf.load_config_file(CONFIG_PATH);
    for i in 1..= num_nodes {
        let system = simulation_scenario.spawn_system(conf.clone());
        let voters = get_experiment_configs(num_nodes).0;
        let (raft_comp, unique_reg_f) = simulation_scenario.create_and_register(&system, || {
            RaftComp::<Storage>::with(
                i,
                voters,
                RaftReconfigurationPolicy::ReplaceFollower,
            )
        }, REGISTER_TIMEOUT);


        let get_state = raft_comp.clone() as Arc<dyn GetState<RaftState>>;
        simulation_scenario.monitor_actor(get_state);

        let actor_path = simulation_scenario.register_by_alias(&system, &raft_comp, RAFT_PATH, REGISTER_TIMEOUT);
        simulation_scenario.start_notify(&system, &raft_comp, REGISTER_TIMEOUT);
        let actor_ref: Recipient<GetSequence> = raft_comp.actor_ref().recipient();
        systems.push(system);
        actor_paths.push(actor_path);
        actor_refs.push(actor_ref);
    }
    (systems, actor_paths, actor_refs)
}

fn raft_normal_test(mut simulation_scenario: SimulationScenario<RaftState>) {
    let num_nodes = 3;
    let num_proposals = 10;
    let concurrent_proposals = 10;
    let last_node_id = num_nodes;
    let iteration_id = 1;

    //CREATE NODES TDOD: extract to separate method
    let mut systems = Vec::with_capacity(num_nodes as usize);
    let mut actor_paths = Vec::with_capacity(num_nodes as usize);
    let mut actor_refs = Vec::with_capacity(num_nodes as usize);
    //let mut comp_defs = Vec::with_capacity(num_nodes as usize);

    let mut conf = KompactConfig::default();
    conf.load_config_file(CONFIG_PATH);
    for i in 1..=num_nodes {
        let system = simulation_scenario.spawn_system(conf.clone());
        let voters = get_experiment_configs(last_node_id).0;
        let (raft_comp, unique_reg_f) = simulation_scenario.create_and_register(&system, || {
            RaftComp::<Storage>::with(
                i,
                voters,
                RaftReconfigurationPolicy::ReplaceFollower,
            )
        }, REGISTER_TIMEOUT);


        let get_state = raft_comp.clone() as Arc<dyn GetState<RaftState>>;
        simulation_scenario.monitor_actor(get_state);

        let actor_path = simulation_scenario.register_by_alias(&system, &raft_comp, RAFT_PATH, REGISTER_TIMEOUT);
        simulation_scenario.start_notify(&system, &raft_comp, REGISTER_TIMEOUT);
        let actor_ref: Recipient<GetSequence> = raft_comp.actor_ref().recipient();
        systems.push(system);
        actor_paths.push(actor_path);
        actor_refs.push(actor_ref);
    }

    //CREATE CLIENT SYSTEM 
    let mut conf = KompactConfig::default();
    conf.load_config_file(CONFIG_PATH);
    let system = simulation_scenario.spawn_system(conf); 

    let mut nodes_id: HashMap<u64, ActorPath> = HashMap::new();

    for (id, ap) in actor_paths.iter().enumerate() {
        nodes_id.insert(id as u64 + 1, ap.clone());
    }

    //CREATE CLIENT LATCHES
    let finished_latch = Arc::new(CountdownEvent::new(1));
    let leader_election_latch = Arc::new(CountdownEvent::new(1));

    // CREATE CLIENT
    let initial_config: Vec<_> = (1..=num_nodes).map(|x| x as u64).collect();
    let (client_comp, unique_reg_f) = simulation_scenario.create_and_register(&system, || {
        Client::with(
            initial_config,
            num_proposals,
            concurrent_proposals,
            nodes_id,
            None,
            Duration::from_millis(20000),
            leader_election_latch.clone(),
            finished_latch.clone(),
        )
    }, REGISTER_TIMEOUT);

    let client_comp_f = simulation_scenario.start_notify(&system, &client_comp, REGISTER_TIMEOUT);
    let client_path = simulation_scenario
        .register_by_alias(&system, &client_comp, format!("client{}", &iteration_id), REGISTER_TIMEOUT);

    //TODO: remove need for paritioning actor and trigger this codeblock through the client instead, maybe(?) make it part of the simulation sequence
    /*
        for node in &self.nodes {
            node.tell_serialised(PartitioningActorMsg::Run, self)
                .expect("Should serialise");
        }
    */

    //PARTITIONING ACTOR
    let prepare_latch = Arc::new(CountdownEvent::new(1));
    let (partitioning_actor, _) = simulation_scenario.create_and_register(&system, || {
        PartitioningActor::with(prepare_latch.clone(), None, iteration_id, actor_paths, None)
    }, Duration::from_millis(1000));

    let partitioning_actor_f = simulation_scenario.start_notify(&system, &partitioning_actor, Duration::from_millis(1000));

    let mut ser_client = Vec::<u8>::new();
    client_path
        .serialise(&mut ser_client)
        .expect("Failed to serialise ClientComp actorpath");

    partitioning_actor
        .actor_ref()
        .tell(IterationControlMsg::Prepare(Some(ser_client)));

    while prepare_latch.count() > 0 {
        simulation_scenario.simulate_step();
    }
    
    let post_prepare_count = simulation_scenario.get_simulation_step_count();

    partitioning_actor.actor_ref().tell(IterationControlMsg::Run);

    while leader_election_latch.count() > 0 {
        simulation_scenario.simulate_step();
    }

    //client_comp.actor_ref().tell(LocalClientMessage::Run);

    let post_leader_election_count = simulation_scenario.get_simulation_step_count();
    println!("POST LEADER ELECTION: {}", post_leader_election_count);

    client_comp.actor_ref().tell(LocalClientMessage::Propose(1));
    client_comp.actor_ref().tell(LocalClientMessage::Propose(2));
    client_comp.actor_ref().tell(LocalClientMessage::Propose(3));
    client_comp.actor_ref().tell(LocalClientMessage::Propose(4));
    client_comp.actor_ref().tell(LocalClientMessage::Propose(5));
    client_comp.actor_ref().tell(LocalClientMessage::Propose(6));
    client_comp.actor_ref().tell(LocalClientMessage::Propose(7));
    client_comp.actor_ref().tell(LocalClientMessage::Propose(8));
    client_comp.actor_ref().tell(LocalClientMessage::Propose(9));
    client_comp.actor_ref().tell(LocalClientMessage::Propose(10));

    println!("POST PROPOSALS SENT");


    /*println!("CLOGGING SYSTEMMMMMMMMMMMM NOOOOOOWWW");
    println!("CLOGGING SYSTEMMMMMMMMMMMM NOOOOOOWWW");
    println!("CLOGGING SYSTEMMMMMMMMMMMM NOOOOOOWWW");
    println!("CLOGGING SYSTEMMMMMMMMMMMM NOOOOOOWWW");
    println!("CLOGGING SYSTEMMMMMMMMMMMM NOOOOOOWWW");*/
    //simulation_scenario.clog_system(systems[0].clone());


    /* 

    let invariant = RaftInvariantChecker{};
    let invariant_idx = simulation_scenario.monitor_invariant(Arc::new(invariant));
    while simulation_scenario.check_invariant_on_index(invariant_idx).is_err() {
        simulation_scenario.simulate_step();
    }

    //simulation_scenario.print_simulation_step_count();
    simulation_scenario.clog_system(systems[0].clone());
    */


    
    while finished_latch.count() > 0 {
        simulation_scenario.simulate_step();
    }

    let post_finished_latch = simulation_scenario.get_simulation_step_count();


    //let mut sequence_responses: Vec<SequenceResp> = vec![];

    //simulation_scenario.set_scheduling_choice(SimulatedScheduling::Now);


    let mut futures = vec![];
    for node in actor_refs {
        let (kprom, kfuture) = promise::<SequenceResp>();
        let ask = Ask::new(kprom, ());
        println!("TELLING NODE GET SEQ");
        node.tell(GetSequence(ask));

        //println!("pre wait future");
        //let seq = simulation_scenario.wait_future(kfuture);
        futures.push(kfuture);
        //println!("post wait future");
        //sequence_responses.push(seq);
    }


    for i in 0..100 {
        simulation_scenario.simulate_step()
    }

    println!("PRE SEQUENCE RESPONSES");
    let sequence_responses: Vec<_> = FutureCollection::collect_results::<Vec<_>>(futures);
    println!("POST SEQUENCE RESPONSES");

    let quorum_size = num_nodes as usize / 2 + 1;
    check_quorum(&sequence_responses, quorum_size, num_proposals);
    check_validity(&sequence_responses, num_proposals);
    check_uniform_agreement(&sequence_responses);

    // CLEANUOP ITERATION

    println!(
        "Cleaning up Atomic Broadcast (master) iteration {}. Exec_time: {}",
        iteration_id, 0.0
    );

    println!("wait meta results");

    simulation_scenario.set_scheduling_choice(SimulatedScheduling::Now);

    //let meta_results = client_comp.actor_ref().ask_with(|promise| LocalClientMessage::Stop(Ask::new(promise, ()))).wait();

    /*for i in 0..100 {
        simulation_scenario.simulate_step()
    }

    let meta_results = ask.wait();*/

    //let mut num_timed_out = vec![];
    //num_timed_out.push(meta_results.num_timed_out);

    println!("kill client");

    let kill_client_f = system.kill_notify(client_comp);
    kill_client_f
        .wait_timeout(REGISTER_TIMEOUT)
        .expect("Client never died");
    
    println!("kill pactor");

    let kill_pactor_f = system.kill_notify(partitioning_actor);
    kill_pactor_f
        .wait_timeout(REGISTER_TIMEOUT)
        .expect("Partitioning Actor never died!");

    println!("system shutdown");

    println!("Post prepare: {}", post_prepare_count);
    println!("Post leader: {}", post_leader_election_count);
    println!("Post finished: {}", post_finished_latch);


    system
        .shutdown()
        .expect("Kompact didn't shut down properly");
}

struct RaftInvariantChecker{}

impl RaftInvariantChecker {
    fn log_length_more_or_eq_500(states: Vec<RaftState>) -> bool{
        for state in states {
            if state.log_entries_len < 500 {
                return false;
            }
        }
        return true;
    }
}

impl Invariant<RaftState> for RaftInvariantChecker{
    fn check(&self, state: Vec<RaftState>) -> Result<(), SimulationError> {
        if RaftInvariantChecker::log_length_more_or_eq_500(state){
            Ok(())
        } else {
            Err(SimulationError{})
        }
    }
}


fn main() {
    let mut simulation_scenario: SimulationScenario<RaftState> = SimulationScenario::new();
    raft_normal_test(simulation_scenario)
}
//Executing task: cargo run --package kompact_benchmarks --bin kompact_benchmarks 