use std::{net::SocketAddr, time::Duration, sync::Arc, borrow::BorrowMut, fmt::Display, thread, fs::OpenOptions};

use crate::atomic_broadcast::{atomic_broadcast::{run_experiment, check_quorum, check_validity, check_uniform_agreement}, partitioning_actor::IterationControlMsg, client::LocalClientMessage};

mod atomic_broadcast;

extern crate raft as tikv_raft;
use hashbrown::{HashMap, HashSet};
use kompact::prelude::{KompactSystem, ActorPath, Recipient, KompactConfig, BufferConfig, Ask, promise, FutureCollection, NetworkConfig, DeadletterBox, Serialisable, SimulationScenario, SimulatedScheduling, GetState, Component, SimulationError, Invariant, BufMut};
use rand::{rngs::StdRng, Rng, SeedableRng};
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
use slog::{error, info, o, Logger, Level};
use slog::Drain;
use tikv_raft::eraftpb::Entry;
use synchronoise::CountdownEvent;
use tikv_raft::{storage::MemStorage, StateRole, RaftLog, Storage as TikvStorage};
use kompact::prelude::ActorRefFactory;
type Storage = MemStorage;
use std::io::Write;

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

#[derive(Debug, PartialEq)]
enum SimulationState {
    ClientState(ClientState),
    RaftState(RaftState)
}

impl Display for SimulationState{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug)]
pub struct ClientState{
    id: u64
}

impl Display for ClientState{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone)]
pub struct RaftState{
    id: u64,
    term: u64,
    vote: u64,
    state: StateRole,
    leader_id: u64,
    log_unstable: u64,
    log_committed: u64,
    log_applied: u64,
    //on_ready_count: u64,
    //tick_count: u64,
    log_last_index: u64,
    became_leader_count: u64,
    randomized_election_timeout: u64
}

impl Display for RaftState{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Clone, Copy)]
pub struct SimulationConfig {
    num_nodes: u64,
    num_proposals: u64,
    client_timeout: Duration
}

impl GetState<SimulationState> for Component<RaftComp<Storage>> {
    fn get_state(&self) -> SimulationState {
        let def = &self.mutable_core.lock().unwrap().definition;
        let log = def.raft_replica.raw_raft.raft.raft_log.get_store();
        let raft_state = RaftState {
            id: def.raft_replica.raw_raft.raft.id,
            term: def.raft_replica.raw_raft.raft.term,
            vote: def.raft_replica.raw_raft.raft.vote,
            state: def.raft_replica.raw_raft.raft.state,
            leader_id: def.raft_replica.raw_raft.raft.leader_id,
            log_unstable: def.raft_replica.raw_raft.raft.raft_log.unstable.entries.len() as u64,
            log_committed: def.raft_replica.raw_raft.raft.raft_log.committed,
            log_applied: def.raft_replica.raw_raft.raft.raft_log.applied,
            //on_ready_count: def.timer_on_ready_count,
            //tick_count: def.timer_tick_count,
            log_last_index: def.raft_replica.raw_raft.raft.raft_log.last_index(),
            became_leader_count: def.became_leader_count,
            randomized_election_timeout: def.raft_replica.raw_raft.raft.get_randomized_election_timeout() as u64
            
        };
        SimulationState::RaftState(raft_state)
    }
}

fn print_all_raft_states(components: Vec<Component<RaftComp<MemStorage>>>){
    for component in components {
        println!("{:?}", component.get_state());
    }
}

fn todo_raft_normal_test(mut simulation_scenario: SimulationScenario<RaftState>, simulation_config: SimulationConfig, logger: Logger) {
    //Simulation config 5
    //Create nodes function invocation 1
    let (systems, actor_paths, actor_refs, simulation_scenario) = create_nodes(simulation_scenario, simulation_config, logger.clone());
    let (client_comp, client_path, simulation_scenario) = create_client(simulation_scenario, actor_paths, simulation_config, logger.clone());
    //Create state monitors/inspections invariants 3
    //Register state monitors (implicitly checked for every step) 3
    //Simulation Sequence 1

    //Output/Log to console or file final state 1
    todo!()
}

fn create_nodes(
    mut simulation_scenario: SimulationScenario<RaftState>, 
    sim_conf: SimulationConfig,
    logger: Logger,
) -> (Vec<KompactSystem>, Vec<ActorPath>, Vec<Recipient<GetSequence>>, SimulationScenario<RaftState>) {
    //CREATE NODES TDOD: extract to separate method
    let mut systems = Vec::with_capacity(sim_conf.num_nodes as usize);
    let mut actor_paths = Vec::with_capacity(sim_conf.num_nodes as usize);
    let mut actor_refs = Vec::with_capacity(sim_conf.num_nodes as usize);
    //let mut comp_defs = Vec::with_capacity(num_nodes as usize);

    let mut conf = KompactConfig::default();
    conf.load_config_file(CONFIG_PATH);
    for i in 1..= sim_conf.num_nodes {
        let system = simulation_scenario.spawn_system(conf.clone());
        let voters = get_experiment_configs(sim_conf.num_nodes).0;
        let (raft_comp, unique_reg_f) = simulation_scenario.create_and_register(&system, || {
            RaftComp::<Storage>::with(
                i,
                voters,
                RaftReconfigurationPolicy::ReplaceFollower,
                1,
                logger.clone(),
            )
        }, REGISTER_TIMEOUT);


        let get_state = raft_comp.clone() as Arc<dyn GetState<SimulationState>>;
        simulation_scenario.monitor_actor(get_state);

        let actor_path = simulation_scenario.register_by_alias(&system, &raft_comp, RAFT_PATH, REGISTER_TIMEOUT);
        simulation_scenario.start_notify(&system, &raft_comp, REGISTER_TIMEOUT);
        let actor_ref: Recipient<GetSequence> = raft_comp.actor_ref().recipient();
        systems.push(system);
        actor_paths.push(actor_path);
        actor_refs.push(actor_ref);
    }
    (systems, actor_paths, actor_refs, simulation_scenario)
}

fn create_client (
    mut simulation_scenario: SimulationScenario<RaftState>, 
    actor_paths: Vec<ActorPath>, 
    sim_conf: SimulationConfig,
    logger: Logger
) -> (Arc<Component<Client>>, ActorPath, SimulationScenario<RaftState>) {
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
    let prepare_latch = Arc::new(CountdownEvent::new(1));

    // CREATE CLIENT
    let initial_config: Vec<_> = (1..=sim_conf.num_nodes).map(|x| x as u64).collect();
    let (client_comp, unique_reg_f) = simulation_scenario.create_and_register(&system, || {
        Client::with(
            initial_config,
            sim_conf.num_proposals,
            nodes_id,
            None,
            sim_conf.client_timeout,
            leader_election_latch.clone(),
            finished_latch.clone(),
            prepare_latch.clone(),
            actor_paths,
            logger
        )
    }, REGISTER_TIMEOUT);

    let client_comp_f = simulation_scenario.start_notify(&system, &client_comp, REGISTER_TIMEOUT);
    let client_path = simulation_scenario
        .register_by_alias(&system, &client_comp, format!("client"), REGISTER_TIMEOUT);
    
    (client_comp, client_path, simulation_scenario)
}


























fn livelock_scenario_ready(simulation_scenario: &SimulationScenario<SimulationState>) -> Option<(usize, usize)>{
    let states = simulation_scenario.get_all_actor_states();

    //let raft_states = states.into_iter().filter(|state| *state. //howww SimulationState::RaftState);

    let state = Some(states.iter().min_by_key(|e| e.log_last_index).unwrap()).unwrap();
    if state.log_last_index < 200 {
        println!("less than 20");
        return None;
    }

    println!("more than 50");
    println!("LOG LENGTHS: {}, {}, {}", states[0].log_last_index, states[1].log_last_index, states[2].log_last_index);

    let leader = states.iter().filter(|e| e.state == StateRole::Leader).last().unwrap().id;
    let min = states.iter().min_by_key(|e| e.log_last_index).unwrap().id;
    let other = states.iter().filter(|e| e.id != min && e.id != leader).last().unwrap().id  as usize;

    let leader = leader as usize;
    let min = min as usize;

    if min == other || other == leader {
        return None
    }

    if states[min-1].log_last_index == states[other-1].log_last_index || states[other-1].log_last_index == states[leader-1].log_last_index{
        return None
    }

    let break_link_with_shortest_leader = true;

    if break_link_with_shortest_leader {
        return Some(((leader-1), (min-1)))
    } 
    //&& (states[other-1].randomized_election_timeout > states[min-1].randomized_election_timeout && states[other-1].randomized_election_timeout > states[leader-1].randomized_election_timeout)
    
    /* 
    if !break_link_with_shortest_leader && (states[min-1].randomized_election_timeout > states[min-1].randomized_election_timeout && states[other-1].randomized_election_timeout > states[leader-1].randomized_election_timeout) {
        return Some(((leader-1), (other-1)))
    }*/

    return Some((states.len() as usize, states.len() as usize))

    /* 
    if (states[1].log_committed >= states[0].log_committed && states[0].log_committed > states[2].log_committed) {
        println!("breaking link betweeen 2, 3");
        return Some((1, 0))

        //return Some((1, 2))
    }

    if (states[1].log_committed >= states[2].log_committed && states[2].log_committed > states[0].log_committed) {
        println!("breaking link betweeen 2, 1");
        //return Some((1, 0))
        return Some((1, 2))
    }*/

    //return None
}

fn raft_normal_test(mut simulation_scenario: SimulationScenario<SimulationState>, seed: u64, logger: Logger) -> Vec<RaftState> {
    let mut rng = StdRng::seed_from_u64(seed);

    let num_nodes = 3;
    let num_proposals = 4000;
    let concurrent_proposals = 3000;
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
                rng.gen::<u64>(),
                logger.clone()
            )
        }, REGISTER_TIMEOUT);


        let get_state = raft_comp.clone() as Arc<dyn GetState<SimulationState>>;
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
    let prepare_latch = Arc::new(CountdownEvent::new(1));

    // CREATE CLIENT
    let initial_config: Vec<_> = (1..=num_nodes).map(|x| x as u64).collect();
    let (client_comp, unique_reg_f) = simulation_scenario.create_and_register(&system, || {
        Client::with(
            initial_config,
            num_proposals,
            nodes_id,
            None,
            Duration::from_millis(2000),
            leader_election_latch.clone(),
            finished_latch.clone(),
            prepare_latch.clone(),
            actor_paths,
            logger.clone()
        )
    }, REGISTER_TIMEOUT);

    let client_comp_f = simulation_scenario.start_notify(&system, &client_comp, REGISTER_TIMEOUT);
    let client_path = simulation_scenario
        .register_by_alias(&system, &client_comp, format!("client{}", &iteration_id), REGISTER_TIMEOUT);

    //PARTITIONING ACTOR
    
    let mut ser_client = Vec::<u8>::new();
    client_path
        .serialise(&mut ser_client)
        .expect("Failed to serialise ClientComp actorpath");

    client_comp.actor_ref().tell(LocalClientMessage::Prepare(Some(ser_client)));

    while prepare_latch.count() > 0 {
        simulation_scenario.simulate_step();
    }
    
    let post_prepare_count = simulation_scenario.get_simulation_step_count();

    client_comp.actor_ref().tell(LocalClientMessage::Start);

    while leader_election_latch.count() > 0 {
        simulation_scenario.simulate_step();
    }

    let post_leader_election_count = simulation_scenario.get_simulation_step_count();

    for i in 1..(num_proposals+1) {
        client_comp.actor_ref().tell(LocalClientMessage::Propose(i));
    }

    let mut post_break_link = 0;

    loop {
        if let Some(idxs) = livelock_scenario_ready(&simulation_scenario).take() {

            if idxs.0 == num_nodes as usize && idxs.1 == num_nodes as usize{
                return Vec::new()
            }

            simulation_scenario.break_link(&systems[idxs.0], &systems[idxs.1]);
            simulation_scenario.break_link(&systems[idxs.1], &systems[idxs.0]);
            break;
        }

        println!("!!!waiting for livelock scenariooooooo!!!");
        simulation_scenario.simulate_step();
    }

    println!("LIVELOCK SCENARIO IS HERE, BREAKING LINKKKKK");
    //simulation_scenario.clog_system(&systems[0]);

    let post_break_link = simulation_scenario.get_simulation_step_count();
 
    
    while finished_latch.count() > 0 {
        simulation_scenario.simulate_step();
    }

    let post_finished_latch = simulation_scenario.get_simulation_step_count();

    /*for i in 0..20000 {simulation_scenario.simulate_step()
    }*/
    /* 
    println!("post finished latch");
    
    let mut futures = vec![];
    for node in actor_refs {
        let (kprom, kfuture) = promise::<SequenceResp>();
        let ask = Ask::new(kprom, ());
        println!("node tell get sequence");
        node.tell(GetSequence(ask));
        futures.push(kfuture);
    }*/

    println!("post finished latch");

    for i in 0..1000 {
        simulation_scenario.simulate_step()
    }

    /* 
    let sequence_responses: Vec<_> = FutureCollection::collect_results::<Vec<_>>(futures);

    let quorum_size = num_nodes as usize / 2 + 1;
    check_quorum(&sequence_responses, quorum_size, num_proposals);
    check_validity(&sequence_responses, num_proposals);
    check_uniform_agreement(&sequence_responses);
    */

    // CLEANUOP ITERATION

    println!(
        "Cleaning up Atomic Broadcast (master) iteration {}. Exec_time: {}",
        iteration_id, 0.0
    );

    simulation_scenario.set_scheduling_choice(SimulatedScheduling::Now);

    let kill_client_f = system.kill_notify(client_comp);
    kill_client_f
        .wait_timeout(REGISTER_TIMEOUT)
        .expect("Client never died");
    

    println!("Post prepare: {}", post_prepare_count);
    println!("Post leader: {}", post_leader_election_count);
    println!("Post finished: {}", post_finished_latch);
    println!("Leader finished diff: {}", post_finished_latch - post_leader_election_count);

    system
        .shutdown()
        .expect("Kompact didn't shut down properly");


    let mut file = OpenOptions::new()
        .write(true)
        .append(true)
        .open("state.txt")
        .unwrap();

    if let Err(e) = writeln!(file, "BreakLink {}", post_break_link.to_string()) {
        eprintln!("Couldn't write to file: {}", e);
    }
    return simulation_scenario.get_all_actor_states();
}

struct RaftInvariantChecker{}

impl RaftInvariantChecker {
    fn log_length_more_or_eq_500(states: Vec<RaftState>) -> bool{
        for state in states {
            if state.log_unstable < 500 {
                return false;
            }
        }
        return true;
    }
}

impl Invariant<RaftState> for RaftInvariantChecker {
    fn check(&self, state: Vec<RaftState>) -> Result<(), SimulationError> {
        if RaftInvariantChecker::log_length_more_or_eq_500(state){
            Ok(())
        } else {
            Err(SimulationError{})
        }
    }
}

fn raft_five_node_livelock_test(mut simulation_scenario: SimulationScenario<RaftState>, seed: u64, logger: Logger) -> Vec<RaftState> {
    /*let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).chan_size(4096).build().fuse();

    let logger = slog::Logger::root(drain, o!());*/


    let mut rng = StdRng::seed_from_u64(seed);

    let num_nodes = 5;
    let num_proposals = 3000;
    let concurrent_proposals = 3000;
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
                rng.gen::<u64>(),
                logger.clone()
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
    let prepare_latch = Arc::new(CountdownEvent::new(1));

    // CREATE CLIENT
    let initial_config: Vec<_> = (1..=num_nodes).map(|x| x as u64).collect();
    let (client_comp, unique_reg_f) = simulation_scenario.create_and_register(&system, || {
        Client::with(
            initial_config,
            num_proposals,
            nodes_id,
            None,
            Duration::from_millis(20000),
            leader_election_latch.clone(),
            finished_latch.clone(),
            prepare_latch.clone(),
            actor_paths,
            logger.clone()
        )
    }, REGISTER_TIMEOUT);

    let client_comp_f = simulation_scenario.start_notify(&system, &client_comp, REGISTER_TIMEOUT);
    let client_path = simulation_scenario
        .register_by_alias(&system, &client_comp, format!("client{}", &iteration_id), REGISTER_TIMEOUT);

    //PARTITIONING ACTOR
    
    let mut ser_client = Vec::<u8>::new();
    client_path
        .serialise(&mut ser_client)
        .expect("Failed to serialise ClientComp actorpath");

    /* 
    simulation_scenario.break_link(&systems[4], &systems[0]);
    simulation_scenario.break_link(&systems[4], &systems[1]);
    simulation_scenario.break_link(&systems[4], &systems[3]);

    simulation_scenario.break_link(&systems[0], &systems[4]);
    simulation_scenario.break_link(&systems[1], &systems[4]);
    simulation_scenario.break_link(&systems[3], &systems[4]);

    //break link between 4 and everyone else except 2
    simulation_scenario.break_link(&systems[3], &systems[0]);
    simulation_scenario.break_link(&systems[3], &systems[2]);
    simulation_scenario.break_link(&systems[3], &systems[4]);

    simulation_scenario.break_link(&systems[0], &systems[3]);
    simulation_scenario.break_link(&systems[2], &systems[3]);
    simulation_scenario.break_link(&systems[4], &systems[3]);*/

    client_comp.actor_ref().tell(LocalClientMessage::Prepare(Some(ser_client)));

    while prepare_latch.count() > 0 {
        simulation_scenario.simulate_step();
    }
    
    let post_prepare_count = simulation_scenario.get_simulation_step_count();

    client_comp.actor_ref().tell(LocalClientMessage::Start);

    while leader_election_latch.count() > 0 {
        simulation_scenario.simulate_step();
    }

    let post_leader_election_count = simulation_scenario.get_simulation_step_count();

    /* */
    //break link between 5 and everyone else except 3


    for i in 1..(num_proposals+1) {
        client_comp.actor_ref().tell(LocalClientMessage::Propose(i));
    }

    /*
    loop {
        if let Some(idxs) = livelock_scenario_ready(&simulation_scenario).take() {
            simulation_scenario.break_link(&systems[idxs.0], &systems[idxs.1]);
            simulation_scenario.break_link(&systems[idxs.1], &systems[idxs.0]);
            break;
        }

        println!("!!!waiting for livelock scenariooooooo!!!");
        simulation_scenario.simulate_step();
    }
    */

    println!("LIVELOCK SCENARIO IS HERE, BREAKING LINKKKKK");
    //simulation_scenario.clog_system(&systems[0]);


    
    while finished_latch.count() > 0 {
        simulation_scenario.simulate_step();
    }

    let post_finished_latch = simulation_scenario.get_simulation_step_count();

    /*for i in 0..20000 {simulation_scenario.simulate_step()
    }*/
    /* 
    println!("post finished latch");
    
    let mut futures = vec![];
    for node in actor_refs {
        let (kprom, kfuture) = promise::<SequenceResp>();
        let ask = Ask::new(kprom, ());
        println!("node tell get sequence");
        node.tell(GetSequence(ask));
        futures.push(kfuture);
    }*/

    println!("post finished latch");

    for i in 0..5000 {
        simulation_scenario.simulate_step()
    }

    /* 
    let sequence_responses: Vec<_> = FutureCollection::collect_results::<Vec<_>>(futures);

    let quorum_size = num_nodes as usize / 2 + 1;
    check_quorum(&sequence_responses, quorum_size, num_proposals);
    check_validity(&sequence_responses, num_proposals);
    check_uniform_agreement(&sequence_responses);
    */

    // CLEANUOP ITERATION

    println!(
        "Cleaning up Atomic Broadcast (master) iteration {}. Exec_time: {}",
        iteration_id, 0.0
    );

    simulation_scenario.set_scheduling_choice(SimulatedScheduling::Now);

    let kill_client_f = system.kill_notify(client_comp);
    kill_client_f
        .wait_timeout(REGISTER_TIMEOUT)
        .expect("Client never died");
    

    println!("Post prepare: {}", post_prepare_count);
    println!("Post leader: {}", post_leader_election_count);
    println!("Post finished: {}", post_finished_latch);
    println!("Leader finished diff: {}", post_finished_latch - post_leader_election_count);

    system
        .shutdown()
        .expect("Kompact didn't shut down properly");

    /* 
    let mut file = OpenOptions::new()
        .write(true)
        .append(true)
        .open("state.txt")
        .unwrap();

    if let Err(e) = writeln!(file, "A new line!") {
        eprintln!("Couldn't write to file: {}", e);
    }*/

    return simulation_scenario.get_all_actor_states();
}

fn main() {
    //Three node partition
    //tested 0..26 not successful election_timeout 500
    //81, 115, 152, 215, 284 (client problems?) with 1 5
    //80-140 covere with no problem 1 10
    for i in 0..1 {
        println!("starting simulation {}", i);
        //5 gives interesting result?=??
        let decorator = slog_term::TermDecorator::new().build();
        let drain = slog_term::FullFormat::new(decorator).build().fuse();
        let drain = slog_async::Async::new(drain).chan_size(16384).build().filter(|r| r.level() == Level::Info).fuse();

        //let filter = slog::Filter(drain, |r| r.level() == Level::Info);
    
        let logger = slog::Logger::root(drain, o!());
        let mut simulation_scenario: SimulationScenario<SimulationState> = SimulationScenario::new();
        let states =  raft_normal_test(simulation_scenario,i, logger);

        //std::thread::sleep(Duration::from_millis(10000));

        println!("ending simulation {}", i);
        
        
        let ss: Vec<u64> = states.clone().into_iter()
            .map(|s| s.log_applied)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();


        if states.iter().any(|e| e.became_leader_count > 2) && ss.len() > 1 {
            println!("");
            println!("{}", i);
            println!("{:?}", states);
            println!("");
            break
        }

        println!("{:?}", states);

    }

    //5 node stuff

    /*//14 gives interesting results
    let i = 1;
    //println!("starting simulation {}", i);
    //5 gives interesting result?=??
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).chan_size(16384).build().fuse();
    //let logger = slog::Logger::root(slog::Discard, o!());
    let logger = slog::Logger::root(drain, o!());
    //println!("starting simulation {}", i);
    let mut simulation_scenario: SimulationScenario<RaftState> = SimulationScenario::new();
    let states =  raft_five_node_livelock_test(simulation_scenario,i, logger);
    //println!("ending simulation {}", i);


    if states.iter().any(|e| e.became_leader_count > 2){
        println!("");
        println!("{}", i);
        println!("{:?}", states);
        println!("");
    }*/
}
//Executing task: cargo run --package kompact_benchmarks --bin kompact_benchmarks 