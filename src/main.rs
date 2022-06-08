use std::{net::SocketAddr, time::Duration, sync::Arc};

use crate::atomic_broadcast::{atomic_broadcast::{run_experiment, check_quorum, check_validity, check_uniform_agreement}, partitioning_actor::IterationControlMsg, client::LocalClientMessage};

mod atomic_broadcast;

extern crate raft as tikv_raft;
use hashbrown::HashMap;
use kompact::prelude::{KompactSystem, ActorPath, Recipient, KompactConfig, BufferConfig, Ask, promise, FutureCollection, NetworkConfig, DeadletterBox, Serialisable};

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
use synchronoise::CountdownEvent;
use tikv_raft::storage::MemStorage;
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

fn raft_normal_test() {
    println!("running test");
    let num_nodes = 3;
    let num_proposals = 1000;
    let concurrent_proposals = 200;
    let reconfiguration = "off";
    let reconfig_policy = "none";
    let algorithm = "raft";
    let last_node_id = num_nodes;
    let iteration_id = 1;
    /*run_experiment(
        "raft",
        num_nodes,
        num_proposals,
        concurrent_proposals,
        reconfiguration,
        reconfig_policy,
    );*/

    //CREATE NODES
    let mut systems = Vec::with_capacity(num_nodes as usize);
    let mut actor_paths = Vec::with_capacity(num_nodes as usize);
    let mut actor_refs = Vec::with_capacity(num_nodes as usize);
    let mut conf = KompactConfig::default();
    conf.load_config_file(CONFIG_PATH);
    let bc = BufferConfig::from_config_file(CONFIG_PATH);
    bc.validate();
    let tcp_no_delay = true;
    for i in 1..=num_nodes {
        let system = atomic_broadcast::kompact_system_provider::global()
            .new_remote_system_with_threads_config(
                format!("node{}", i),
                1,
                conf.clone(),
                bc.clone(),
                tcp_no_delay,
            );
        let (actor_path, actor_ref) = match algorithm {
            "raft" => {
                let voters = get_experiment_configs(last_node_id).0;
                let reconfig_policy = match reconfig_policy {
                    "none" => None,
                    "replace-leader" => Some(RaftReconfigurationPolicy::ReplaceLeader),
                    "replace-follower" => Some(RaftReconfigurationPolicy::ReplaceFollower),
                    unknown => panic!("Got unknown Raft transfer policy: {}", unknown),
                };
                /*** Setup RaftComp ***/
                let (raft_comp, unique_reg_f) = system.create_and_register(|| {
                    RaftComp::<Storage>::with(
                        i,
                        voters,
                        reconfig_policy.unwrap_or(RaftReconfigurationPolicy::ReplaceFollower),
                    )
                });
                unique_reg_f.wait_expect(REGISTER_TIMEOUT, "RaftComp failed to register!");
                let self_path = system
                    .register_by_alias(&raft_comp, RAFT_PATH)
                    .wait_expect(REGISTER_TIMEOUT, "Communicator failed to register!");
                let raft_comp_f = system.start_notify(&raft_comp);
                raft_comp_f
                    .wait_timeout(REGISTER_TIMEOUT)
                    .expect("RaftComp never started!");

                let r: Recipient<GetSequence> = raft_comp.actor_ref().recipient();
                (self_path, r)
            }
            unknown => panic!("Got unknown algorithm: {}", unknown),
        };
        systems.push(system);
        actor_paths.push(actor_path);
        actor_refs.push(actor_ref);
    }

    //CREATE CLIENT SYSTEM

    let mut conf = KompactConfig::default();
    conf.load_config_file(CONFIG_PATH);
    let bc = BufferConfig::from_config_file(CONFIG_PATH);
    bc.validate();
    let tcp_no_delay = true;
    let addr = SocketAddr::new("127.0.0.1".parse().unwrap(), 0);
    conf.threads(1);
    //Self::set_executor_for_threads(threads, &mut conf);
    conf.throughput(50);
    let mut nc = NetworkConfig::with_buffer_config(addr, bc);
    nc.set_tcp_nodelay(tcp_no_delay);
    conf.system_components(DeadletterBox::new, nc.build());
    let system = conf.build().expect("KompactSystem");

    let mut nodes_id: HashMap<u64, ActorPath> = HashMap::new();

    for (id, ap) in actor_paths.iter().enumerate() {
        nodes_id.insert(id as u64 + 1, ap.clone());
    }

    //CREATE CLIENT LATCHES
    let finished_latch = Arc::new(CountdownEvent::new(1));
    let leader_election_latch = Arc::new(CountdownEvent::new(1));
    // CREATE CLIENT
    /*** Setup client ***/
    let initial_config: Vec<_> = (1..=num_nodes).map(|x| x as u64).collect();
    let (client_comp, unique_reg_f) = system.create_and_register(|| {
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
    });
    unique_reg_f.wait_expect(REGISTER_TIMEOUT, "Client failed to register!");
    let client_comp_f = system.start_notify(&client_comp);
    client_comp_f
        .wait_timeout(REGISTER_TIMEOUT)
        .expect("ClientComp never started!");
    let client_path = system
        .register_by_alias(&client_comp, format!("client{}", &iteration_id))
        .wait_expect(REGISTER_TIMEOUT, "Failed to register alias for ClientComp");

    //PARTITIONING ACTOR
    let prepare_latch = Arc::new(CountdownEvent::new(1));
    /*** Setup partitioning actor ***/
    let (partitioning_actor, unique_reg_f) = system.create_and_register(|| {
        PartitioningActor::with(prepare_latch.clone(), None, iteration_id, actor_paths, None)
    });
    unique_reg_f.wait_expect(
        Duration::from_millis(1000),
        "PartitioningComp failed to register!",
    );

    let partitioning_actor_f = system.start_notify(&partitioning_actor);
    partitioning_actor_f
        .wait_timeout(Duration::from_millis(1000))
        .expect("PartitioningComp never started!");
    let mut ser_client = Vec::<u8>::new();
    client_path
        .serialise(&mut ser_client)
        .expect("Failed to serialise ClientComp actorpath");
    partitioning_actor
        .actor_ref()
        .tell(IterationControlMsg::Prepare(Some(ser_client)));
    prepare_latch.wait();
    partitioning_actor.actor_ref().tell(IterationControlMsg::Run);
    leader_election_latch.wait(); 

    client_comp.actor_ref().tell(LocalClientMessage::Run);
    println!("11111111111111111 PRE FINISHED LATCH 1111111111111111111111");
    finished_latch.wait();
    println!("11111111111111111 POST FINISHED LATCH 1111111111111111111111");

    let mut futures = vec![];
    for node in actor_refs {
        let (kprom, kfuture) = promise::<SequenceResp>();
        let ask = Ask::new(kprom, ());
        println!("SENDING OUT GET SEQUENCE HELLOOOOOOOOOOOOO");
        node.tell(GetSequence(ask));
        futures.push(kfuture);
    }

    let sequence_responses: Vec<_> = FutureCollection::collect_results::<Vec<_>>(futures);
    let quorum_size = num_nodes as usize / 2 + 1;
    check_quorum(&sequence_responses, quorum_size, num_proposals);
    check_validity(&sequence_responses, num_proposals);
    check_uniform_agreement(&sequence_responses);

    // CLEANUOP ITERATION

    println!(
        "Cleaning up Atomic Broadcast (master) iteration {}. Exec_time: {}",
        iteration_id, 0.0
    );
    let meta_results = client_comp
        .actor_ref()
        .ask_with(|promise| LocalClientMessage::Stop(Ask::new(promise, ())))
        .wait();

    let mut num_timed_out = vec![];
    num_timed_out.push(meta_results.num_timed_out);
    if concurrent_proposals == 1 || cfg!(feature = "track_latency") {
        //persist_latency_results(&meta_results.latencies);
    }
    #[cfg(feature = "track_timestamps")]
    {
        let (timestamps, leader_changes_t) = meta_results
            .timestamps_leader_changes
            .expect("No timestamps results!");
        self.persist_timestamp_results(&timestamps, &leader_changes_t);
    }

    let kill_client_f = system.kill_notify(client_comp);
    kill_client_f
        .wait_timeout(REGISTER_TIMEOUT)
        .expect("Client never died");

    let kill_pactor_f = system.kill_notify(partitioning_actor);
    kill_pactor_f
        .wait_timeout(REGISTER_TIMEOUT)
        .expect("Partitioning Actor never died!");

    println!("Cleaning up last iteration");
    //persist_timeouts_summary();
    if concurrent_proposals == 1 || cfg!(feature = "track_latency") {
        //persist_latency_summary();
    }
    /* 
    self.num_nodes = None;
    self.reconfiguration = None;
    self.concurrent_proposals = None;
    self.num_proposals = None;
    self.experiment_str = None;
    self.num_timed_out.clear();
    self.iteration_id = 0;*/
    system
        .shutdown()
        .expect("Kompact didn't shut down properly");
}

fn main() {
    raft_normal_test()
}
//Executing task: cargo run --package kompact_benchmarks --bin kompact_benchmarks 