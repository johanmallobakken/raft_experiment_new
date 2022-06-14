use std::{net::SocketAddr, time::Duration, sync::Arc, borrow::BorrowMut};

use crate::atomic_broadcast::{atomic_broadcast::{run_experiment, check_quorum, check_validity, check_uniform_agreement}, partitioning_actor::IterationControlMsg, client::LocalClientMessage};

mod atomic_broadcast;

extern crate raft as tikv_raft;
use hashbrown::HashMap;
use kompact::prelude::{KompactSystem, ActorPath, Recipient, KompactConfig, BufferConfig, Ask, promise, FutureCollection, NetworkConfig, DeadletterBox, Serialisable, SimulationScenario, SimulatedScheduling, GetState, Invariant};

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
    }
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
    term: u64,
    vote: u64, //?
    id: u64,
    state: StateRole,
    leader_id: u64,
    //RAFT LOG
    log_entries: Vec<Entry>,
    //log_unstable: Unstable,
    log_committed: u64,
    log_applied: u64
}

impl GetState<RaftState> for RaftComp<Storage> {
    fn get_state(&self) -> RaftState {

        let log = self.raft_replica.raw_raft.raft.raft_log.store.clone();
        let entries = log.entries(log.first_index().unwrap(), log.last_index().unwrap(), 9999).unwrap();

        RaftState{
            term: self.raft_replica.raw_raft.raft.term,
            vote: self.raft_replica.raw_raft.raft.vote,
            id: self.raft_replica.raw_raft.raft.id,
            state: self.raft_replica.raw_raft.raft.state,
            leader_id: self.raft_replica.raw_raft.raft.leader_id,
            //RAFT LOG
            log_entries: entries,
            //log_unstable: self.raft_replica.raw_raft.raft.raft_log.unstable.copy(),
            log_committed: self.raft_replica.raw_raft.raft.raft_log.committed,
            log_applied: self.raft_replica.raw_raft.raft.raft_log.applied,
        }
    }
}

fn print_all_raft_states(components: Vec<RaftComp<MemStorage>>){
    for component in components {
        println!("{:?}", component.get_state());
    }
}
fn todo_raft_normal_test(mut simulation_scenario: SimulationScenario<RaftState>) {
    todo!()
    //Simulation config 5
    //Create nodes function invocation 1
    //Create state monitors/inspections invariants 3
    //Register state monitors (implicitly checked for every step) 3
    //Simulation Sequence 1
    //Output/Log to console or file final state 1
}

fn raft_normal_test(mut simulation_scenario: SimulationScenario<RaftState>) {
    let num_nodes = 3;
    let num_proposals = 1000;
    let concurrent_proposals = 200;
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
            Duration::from_secs(20),
            leader_election_latch.clone(),
            finished_latch.clone(),
        )
    }, REGISTER_TIMEOUT);

    let client_comp_f = simulation_scenario.start_notify(&system, &client_comp, REGISTER_TIMEOUT);
    let client_path = simulation_scenario
        .register_by_alias(&system, &client_comp, format!("client{}", &iteration_id), REGISTER_TIMEOUT);

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

    partitioning_actor.actor_ref().tell(IterationControlMsg::Run);

    while leader_election_latch.count() > 0 {
        simulation_scenario.simulate_step();
    }

    client_comp.actor_ref().tell(LocalClientMessage::Run);
    while finished_latch.count() > 0 {
        simulation_scenario.simulate_step();
    }

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

    //println!("PRE SEQUENCE RESPONSES len: {} ", sequence_responses.len());
    let sequence_responses: Vec<_> = FutureCollection::collect_results::<Vec<_>>(futures);
    //println!("POST SEQUENCE RESPONSES");

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

    system
        .shutdown()
        .expect("Kompact didn't shut down properly");
}

struct RaftInvariantChecker{}

impl<RaftState> Invariant<RaftState> for RaftInvariantChecker{
 fn check(){

 }

}

fn main() {
    let mut simulation_scenario: SimulationScenario<RaftState> = SimulationScenario::new();
    raft_normal_test(simulation_scenario)
}
//Executing task: cargo run --package kompact_benchmarks --bin kompact_benchmarks 