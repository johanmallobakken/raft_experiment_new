use crate::atomic_broadcast::atomic_broadcast::run_experiment;

mod atomic_broadcast;



extern crate raft as tikv_raft;
use kompact::prelude::{KompactSystem, ActorPath, Recipient, KompactConfig, BufferConfig, Ask, promise, FutureCollection};


use atomic_broadcast::{raft::{RaftCompMsg, RaftComp, ReconfigurationPolicy as RaftReconfigurationPolicy}, atomic_broadcast::{REGISTER_TIMEOUT, RAFT_PATH, CONFIG_PATH, SequenceResp}};
use tikv_raft::storage::MemStorage;
use kompact::prelude::ActorRefFactory;
type Storage = MemStorage;

#[derive(Debug)]
pub struct GetSequence(Ask<(), SequenceResp>);

impl Into<RaftCompMsg> for GetSequence {
    fn into(self) -> RaftCompMsg {
        RaftCompMsg::GetSequence(self.0)
    }
}
/*
fn create_nodes(
    n: u64,
    algorithm: &str,
    reconfig_policy: &str,
    last_node_id: u64,
) -> (
    Vec<KompactSystem>,
    Vec<ActorPath>,
    Vec<Recipient<GetSequence>>,
) {
    let mut systems = Vec::with_capacity(n as usize);
    let mut actor_paths = Vec::with_capacity(n as usize);
    let mut actor_refs = Vec::with_capacity(n as usize);
    let mut conf = KompactConfig::default();
    conf.load_config_file(CONFIG_PATH);
    let bc = BufferConfig::from_config_file(CONFIG_PATH);
    bc.validate();
    let tcp_no_delay = true;
    for i in 1..=n {
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
                let voters = atomic_broadcast::atomic_broadcast::get_experiment_configs(last_node_id).0;
                let reconfig_policy = match reconfig_policy {
                    "none" => None,
                    "replace-leader" => Some(RaftReconfigurationPolicy::ReplaceLeader),
                    "replace-follower" => Some(RaftReconfigurationPolicy::ReplaceFollower),
                    unknown => panic!("Got unknown Raft transfer policy: {}", unknown),
                };
                /*** Setup RaftComp ***/
                let (raft_comp, unique_reg_f) = system.create_and_register(|| {
                    RaftComp::<Storage>::with(
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
    (systems, actor_paths, actor_refs)
}

fn run_experiment(
    algorithm: &str,
    num_nodes: u64,
    num_proposals: u64,
    concurrent_proposals: u64,
    reconfiguration: &str,
    reconfig_policy: &str,
) {
    let mut master = AtomicBroadcastMaster::new();
    let mut experiment = AtomicBroadcastRequest::new();
    experiment.algorithm = String::from(algorithm);
    experiment.number_of_nodes = num_nodes;
    experiment.number_of_proposals = num_proposals;
    experiment.concurrent_proposals = concurrent_proposals;
    experiment.reconfiguration = String::from(reconfiguration);
    experiment.reconfig_policy = String::from(reconfig_policy);
    let num_nodes_needed = match reconfiguration {
        "off" => num_nodes,
        "single" => num_nodes + 1,
        _ => unimplemented!(),
    };
    let d = DeploymentMetaData::new(num_nodes_needed as u32);
    let (client_systems, clients, client_refs) = create_nodes(
        num_nodes_needed,
        experiment.get_algorithm(),
        experiment.get_reconfig_policy(),
        num_nodes_needed,
    );
    master
        .setup(experiment, &d)
        .expect("Failed to setup master");
    master.prepare_iteration(clients);
    master.run_iteration();

    let mut futures = vec![];
    for client in client_refs {
        let (kprom, kfuture) = promise::<SequenceResp>();
        let ask = Ask::new(kprom, ());
        client.tell(GetSequence(ask));
        futures.push(kfuture);
    }
    let sequence_responses: Vec<_> = FutureCollection::collect_results::<Vec<_>>(futures);
    let quorum_size = num_nodes as usize / 2 + 1;
    /*check_quorum(&sequence_responses, quorum_size, num_proposals);
    check_validity(&sequence_responses, num_proposals);
    check_uniform_agreement(&sequence_responses);*/

    master.cleanup_iteration(true, 0.0);
    for system in client_systems {
        system.shutdown().expect("Failed to shutdown system");
    }
}
*/
fn raft_normal_test() {
    println!("running test");
    let num_nodes = 1;
    let num_proposals = 1000;
    let concurrent_proposals = 200;
    let reconfiguration = "off";
    let reconfig_policy = "none";
    run_experiment(
        "raft",
        num_nodes,
        num_proposals,
        concurrent_proposals,
        reconfiguration,
        reconfig_policy,
    );
}
fn main() {
    raft_normal_test()
}
