extern crate raft as tikv_raft;

use std::path::PathBuf;
use std::time::Duration;
use hocon::HoconLoader;
use leaderpaxos::storage::SequenceTraits;

use super::{
    client::{Client, LocalClientMessage},
    raft::{RaftComp, ReconfigurationPolicy as RaftReconfigurationPolicy},
};
use crate::{atomic_broadcast::atomic_broadcast_request::*, GetSequence};
use crate::atomic_broadcast::partitioning_actor::IterationControlMsg;
//use benchmark_suite_shared::kompics_benchmarks::benchmarks::AtomicBroadcastRequest;
use hashbrown::HashMap;
use hdrhistogram::Histogram;
use kompact::prelude::*;
use crate::atomic_broadcast::partitioning_actor::PartitioningActor;
use std::{str::FromStr, sync::Arc};
use synchronoise::CountdownEvent;

#[allow(unused_imports)]
use super::storage::raft::DiskStorage;
use crate::atomic_broadcast::{raft::RaftCompMsg};

use kompact::net::buffers::BufferConfig;
use std::{
    fs::{create_dir_all, OpenOptions},
    io::Write,
};
use tikv_raft::storage::MemStorage;

#[derive(Debug)]
pub enum BenchmarkError {
    RSETargetNotMet(String),
    Panic,
    InvalidMessage(String),
    InvalidTest(String),
    InvalidDeployment(String),
}

#[derive(Clone, Debug)]
pub struct SequenceResp {
    pub node_id: u64,
    pub sequence: Vec<u64>,
}

impl SequenceResp {
    pub fn with(node_id: u64, sequence: Vec<u64>) -> SequenceResp {
        SequenceResp { node_id, sequence }
    }
}

#[derive(Debug, Clone)]
pub struct Done;

pub const MAX_INFLIGHT: usize = 100000;
pub const INITIAL_ELECTION_FACTOR: u64 = 10;
pub const ELECTION_TIMEOUT: u64 = 5000;
pub const OUTGOING_PERIOD: Duration = Duration::from_millis(1);

pub const CONFIG_PATH: &str = "./src/atomic_broadcast.conf";
pub const PAXOS_PATH: &str = "paxos_replica";
pub const RAFT_PATH: &str = "raft_replica";
pub const REGISTER_TIMEOUT: Duration = Duration::from_secs(5);

/*

pub struct ExperimentParams {
    pub election_timeout: u64,
    pub outgoing_period: Duration,
    pub max_inflight: usize,
    pub initial_election_factor: u64,
}

impl ExperimentParams {
    pub fn new(
        election_timeout: u64,
        outgoing_period: Duration,
        max_inflight: usize,
        initial_election_factor: u64,
    ) -> ExperimentParams {
        ExperimentParams {
            election_timeout,
            outgoing_period,
            max_inflight,
            initial_election_factor,
        }
    }

    pub fn load_from_file<P>(path: P) -> ExperimentParams
    where
        P: Into<PathBuf>,
    {
        let p: PathBuf = path.into();
        let config = HoconLoader::new()
            .load_file(p)
            .expect("Failed to load file")
            .hocon()
            .expect("Failed to load as HOCON");
        let election_timeout = config["experiment"]["election_timeout"]
            .as_i64()
            .expect("Failed to load election_timeout") as u64;
        let outgoing_period = config["experiment"]["outgoing_period"]
            .as_duration()
            .expect("Failed to load outgoing_period");
        let max_inflight = config["experiment"]["max_inflight"]
            .as_i64()
            .expect("Failed to load max_inflight") as usize;
        let initial_election_factor = config["experiment"]["initial_election_factor"]
            .as_i64()
            .expect("Failed to load initial_election_factor")
            as u64;
        ExperimentParams::new(
            election_timeout,
            outgoing_period,
            max_inflight,
            initial_election_factor,
        )
    }
}
*/

#[derive(Debug, Clone)]
pub struct ClientParams {
    algorithm: String,
    last_node_id: u64,
    reconfig_policy: String,
}

impl ClientParams {
    fn with(algorithm: String, last_node_id: u64, reconfig_policy: String) -> ClientParams {
        ClientParams {
            algorithm,
            last_node_id,
            reconfig_policy,
        }
    }
}


#[derive(Debug, Clone)]
pub struct DeploymentMetaData {
    number_of_clients: u32,
}
impl DeploymentMetaData {
    pub fn new(number_of_clients: u32) -> DeploymentMetaData {
        DeploymentMetaData { number_of_clients }
    }

    pub fn number_of_clients(&self) -> u32 { self.number_of_clients }
}

/* 
#[derive(Default)]
pub struct AtomicBroadcast;

impl DistributedBenchmark for AtomicBroadcast {
    type MasterConf = AtomicBroadcastRequest;
    type ClientConf = ClientParams;
    type ClientData = ActorPath;
    type Master = AtomicBroadcastMaster;
    type Client = AtomicBroadcastClient;
    const LABEL: &'static str = "AtomicBroadcast";

    fn new_master() -> Self::Master {
        AtomicBroadcastMaster::new()
    }

    fn msg_to_master_conf(
        msg: Box<dyn (::protobuf::Message)>,
    ) -> Result<Self::MasterConf, BenchmarkError> {
        downcast_msg!(msg; AtomicBroadcastRequest)
    }

    fn new_client() -> Self::Client {
        AtomicBroadcastClient::new()
    }

    fn str_to_client_conf(s: String) -> Result<Self::ClientConf, BenchmarkError> {
        let split: Vec<_> = s.split(',').collect();
        if split.len() != 3 {
            Err(BenchmarkError::InvalidMessage(format!(
                "String '{}' does not represent a client conf! Split length should be 3",
                s
            )))
        } else {
            let algorithm = split[0].to_lowercase();
            let last_node_id = split[1].parse::<u64>().map_err(|e| {
                BenchmarkError::InvalidMessage(format!(
                    "String to ClientConf error: '{}' does not represent a node id: {:?}",
                    split[1], e
                ))
            })?;
            let reconfig_policy = split[2].to_lowercase();
            Ok(ClientParams::with(algorithm, last_node_id, reconfig_policy))
        }
    }

    fn str_to_client_data(str: String) -> Result<Self::ClientData, BenchmarkError> {
        let res = ActorPath::from_str(&str);
        res.map_err(|e| {
            BenchmarkError::InvalidMessage(format!("Could not read client data: {}", e))
        })
    }

    fn client_conf_to_str(c: Self::ClientConf) -> String {
        format!("{},{},{}", c.algorithm, c.last_node_id, c.reconfig_policy)
    }

    fn client_data_to_str(d: Self::ClientData) -> String {
        d.to_string()
    }
}*/

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

fn get_reconfig_data(
    s: &str,
    n: u64,
) -> Result<(u64, Option<(Vec<u64>, Vec<u64>)>), BenchmarkError> {
    match s.to_lowercase().as_ref() {
        "off" => Ok((0, None)),
        "single" => {
            let reconfig: Vec<u64> = (2..=n + 1).collect();
            let new_followers: Vec<u64> = vec![];
            let reconfiguration = Some((reconfig, new_followers));
            Ok((1, reconfiguration))
        }
        "majority" => {
            let majority = n / 2 + 1;
            let mut reconfig: Vec<u64> = (1..majority).collect(); // minority i.e. continued node ids
            for i in 1..=majority {
                let new_node_id = n + i;
                reconfig.push(new_node_id);
            }
            assert_eq!(n, reconfig.len() as u64);
            let new_followers: Vec<u64> = vec![];
            let reconfiguration = Some((reconfig, new_followers));
            Ok((majority, reconfiguration))
        }
        _ => Err(BenchmarkError::InvalidMessage(String::from(
            "Got unknown reconfiguration parameter",
        ))),
    }
}
type Storage = MemStorage;

pub struct AtomicBroadcastMaster {
    num_nodes: Option<u64>,
    num_proposals: Option<u64>,
    concurrent_proposals: Option<u64>,
    reconfiguration: Option<(Vec<u64>, Vec<u64>)>,
    system: Option<KompactSystem>,
    finished_latch: Option<Arc<CountdownEvent>>,
    iteration_id: u32,
    client_comp: Option<Arc<Component<Client>>>,
    partitioning_actor: Option<Arc<Component<PartitioningActor>>>,
    latency_hist: Option<Histogram<u64>>,
    num_timed_out: Vec<u64>,
    experiment_str: Option<String>,
    meta_results_path: Option<String>,
}

impl AtomicBroadcastMaster {

    fn new() -> AtomicBroadcastMaster {
        AtomicBroadcastMaster {
            num_nodes: None,
            num_proposals: None,
            concurrent_proposals: None,
            reconfiguration: None,
            system: None,
            finished_latch: None,
            iteration_id: 0,
            client_comp: None,
            partitioning_actor: None,
            latency_hist: None,
            num_timed_out: vec![],
            experiment_str: None,
            meta_results_path: None,
        }
    }

    fn initialise_iteration(
        &self,
        nodes: Vec<ActorPath>,
        client: ActorPath,
    ) -> Arc<Component<PartitioningActor>> {
        let system = self.system.as_ref().unwrap();
        let prepare_latch = Arc::new(CountdownEvent::new(1));
        /*** Setup partitioning actor ***/
        let (partitioning_actor, unique_reg_f) = system.create_and_register(|| {
            PartitioningActor::with(prepare_latch.clone(), None, self.iteration_id, nodes, None)
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
        client
            .serialise(&mut ser_client)
            .expect("Failed to serialise ClientComp actorpath");
        partitioning_actor
            .actor_ref()
            .tell(IterationControlMsg::Prepare(Some(ser_client)));
        prepare_latch.wait();
        partitioning_actor
    }

    fn create_client(
        &self,
        nodes_id: HashMap<u64, ActorPath>,
        client_timeout: Duration,
        reconfig: Option<(Vec<u64>, Vec<u64>)>,
        leader_election_latch: Arc<CountdownEvent>,
    ) -> (Arc<Component<Client>>, ActorPath) {
        let system = self.system.as_ref().unwrap();
        let finished_latch = self.finished_latch.clone().unwrap();
        /*** Setup client ***/
        let initial_config: Vec<_> = (1..=self.num_nodes.unwrap()).map(|x| x as u64).collect();
        let (client_comp, unique_reg_f) = system.create_and_register(|| {
            Client::with(
                initial_config,
                self.num_proposals.unwrap(),
                self.concurrent_proposals.unwrap(),
                nodes_id,
                reconfig,
                client_timeout,
                leader_election_latch,
                finished_latch,
            )
        });
        unique_reg_f.wait_expect(REGISTER_TIMEOUT, "Client failed to register!");
        let client_comp_f = system.start_notify(&client_comp);
        client_comp_f
            .wait_timeout(REGISTER_TIMEOUT)
            .expect("ClientComp never started!");
        let client_path = system
            .register_by_alias(&client_comp, format!("client{}", &self.iteration_id))
            .wait_expect(REGISTER_TIMEOUT, "Failed to register alias for ClientComp");
        (client_comp, client_path)
    }

    fn validate_experiment_params(
        &mut self,
        c: &AtomicBroadcastRequest,
        num_clients: u32,
    ) -> Result<(), BenchmarkError> {
        // TODO reconfiguration
        if c.concurrent_proposals > c.number_of_proposals {
            return Err(BenchmarkError::InvalidTest(format!(
                "Concurrent proposals: {} should be less or equal to number of proposals: {}",
                c.concurrent_proposals, c.number_of_proposals
            )));
        }
        match &c.algorithm.to_lowercase() {
            paxos if paxos == "paxos" => match c.reconfiguration.to_lowercase().as_ref() {
                "off" => {
                    if c.reconfig_policy.to_lowercase() != "none" {
                        return Err(BenchmarkError::InvalidTest(format!(
                            "Reconfiguration is off, transfer policy should be none, but found: {}",
                            &c.reconfig_policy
                        )));
                    }
                }
                s if s == "single" || s == "majority" => {
                    let reconfig_policy: &str = &c.reconfig_policy.to_lowercase();
                    if reconfig_policy != "eager" && reconfig_policy != "pull" {
                        return Err(BenchmarkError::InvalidTest(format!(
                            "Unimplemented Paxos transfer policy: {}",
                            &c.reconfig_policy
                        )));
                    }
                }
                _ => {
                    return Err(BenchmarkError::InvalidTest(format!(
                        "Unimplemented Paxos reconfiguration: {}",
                        &c.reconfiguration
                    )));
                }
            },
            raft if raft == "raft" => match c.reconfiguration.to_lowercase().as_ref() {
                "off" => {
                    if c.reconfig_policy.to_lowercase() != "none" {
                        return Err(BenchmarkError::InvalidTest(format!(
                            "Reconfiguration is off, transfer policy should be none, but found: {}",
                            &c.reconfig_policy
                        )));
                    }
                }
                s if s == "single" || s == "majority" => {
                    let reconfig_policy: &str = &c.reconfig_policy.to_lowercase();
                    if reconfig_policy != "replace-leader" && reconfig_policy != "replace-follower"
                    {
                        return Err(BenchmarkError::InvalidTest(format!(
                            "Unimplemented Raft transfer policy: {}",
                            &c.reconfig_policy
                        )));
                    }
                }
                _ => {
                    return Err(BenchmarkError::InvalidTest(format!(
                        "Unimplemented Raft reconfiguration: {}",
                        &c.reconfiguration
                    )));
                }
            },
            _ => {
                return Err(BenchmarkError::InvalidTest(format!(
                    "Unimplemented atomic broadcast algorithm: {}",
                    &c.algorithm
                )));
            }
        };
        match get_reconfig_data(&c.reconfiguration, c.number_of_nodes) {
            Ok((additional_n, reconfig)) => {
                let n = c.number_of_nodes + additional_n;
                if (num_clients as u64) < n {
                    return Err(BenchmarkError::InvalidTest(format!(
                        "Not enough clients: {}, Required: {}",
                        num_clients, n
                    )));
                }
                self.reconfiguration = reconfig;
                self.num_nodes = Some(n);
            }
            Err(e) => return Err(e),
        }
        Ok(())
    }

    pub fn load_benchmark_config<P>(path: P) -> (Duration, Option<String>)
    where
        P: Into<PathBuf>,
    {
        let p: PathBuf = path.into();
        let config = HoconLoader::new()
            .load_file(p)
            .expect("Failed to load file")
            .hocon()
            .expect("Failed to load as HOCON");
        let client_timeout = config["experiment"]["client_timeout"]
            .as_duration()
            .expect("Failed to load client timeout");
        let meta_results_path = config["experiment"]["meta_results_path"].as_string();
        (client_timeout, meta_results_path)
    }

    fn cleanup_iteration(&mut self, last_iteration: bool, exec_time_millis: f64) -> () {
        println!(
            "Cleaning up Atomic Broadcast (master) iteration {}. Exec_time: {}",
            self.iteration_id, exec_time_millis
        );
        let system = self.system.take().unwrap();
        let client = self.client_comp.take().unwrap();
        let meta_results = client
            .actor_ref()
            .ask_with(|promise| LocalClientMessage::Stop(Ask::new(promise, ())))
            .wait();
        self.num_timed_out.push(meta_results.num_timed_out);
        if self.concurrent_proposals == Some(1) || cfg!(feature = "track_latency") {
            self.persist_latency_results(&meta_results.latencies);
        }
        #[cfg(feature = "track_timestamps")]
        {
            let (timestamps, leader_changes_t) = meta_results
                .timestamps_leader_changes
                .expect("No timestamps results!");
            self.persist_timestamp_results(&timestamps, &leader_changes_t);
        }

        let kill_client_f = system.kill_notify(client);
        kill_client_f
            .wait_timeout(REGISTER_TIMEOUT)
            .expect("Client never died");

        if let Some(partitioning_actor) = self.partitioning_actor.take() {
            let kill_pactor_f = system.kill_notify(partitioning_actor);
            kill_pactor_f
                .wait_timeout(REGISTER_TIMEOUT)
                .expect("Partitioning Actor never died!");
        }

        if last_iteration {
            println!("Cleaning up last iteration");
            self.persist_timeouts_summary();
            if self.concurrent_proposals == Some(1) || cfg!(feature = "track_latency") {
                self.persist_latency_summary();
            }
            self.num_nodes = None;
            self.reconfiguration = None;
            self.concurrent_proposals = None;
            self.num_proposals = None;
            self.experiment_str = None;
            self.num_timed_out.clear();
            self.iteration_id = 0;
            system
                .shutdown()
                .expect("Kompact didn't shut down properly");
        } else {
            self.system = Some(system);
        }
    }

    #[cfg(feature = "track_timestamps")]
    fn persist_timestamp_results(
        &mut self,
        timestamps: &[Duration],
        leader_changes_t: &[(u64, Duration)],
    ) {
        let meta_path = self
            .meta_results_path
            .as_ref()
            .expect("No meta results path!");
        let timestamps_dir = format!("{}/timestamps/", meta_path);
        create_dir_all(&timestamps_dir)
            .unwrap_or_else(|_| panic!("Failed to create given directory: {}", &timestamps_dir));
        let mut timestamps_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(format!(
                "{}raw_{}.data",
                &timestamps_dir,
                self.experiment_str.as_ref().unwrap()
            ))
            .expect("Failed to open timestamps file");
        for (pid, leader_change_ts) in leader_changes_t {
            let ts = leader_change_ts.as_micros() as u64;
            write!(timestamps_file, "{},{} ", pid, ts)
                .expect("Failed to write leader changes to timestamps file");
        }
        writeln!(timestamps_file, "").expect("Failed to write raw timestamps file");
        for ts in timestamps {
            let timestamp = ts.as_micros() as u64;
            writeln!(timestamps_file, "{}", timestamp)
                .expect("Failed to write raw timestamps file");
        }
        timestamps_file
            .flush()
            .expect("Failed to flush raw timestamps file");
    }

    fn persist_latency_results(&mut self, latencies: &[Duration]) {
        let meta_path = self
            .meta_results_path
            .as_ref()
            .expect("No meta results path!");
        let latency_dir = format!("{}/latency/", meta_path);
        create_dir_all(&latency_dir)
            .unwrap_or_else(|_| panic!("Failed to create given directory: {}", &latency_dir));
        let mut latency_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(format!(
                "{}raw_{}.data",
                &latency_dir,
                self.experiment_str.as_ref().unwrap()
            ))
            .expect("Failed to open latency file");

        let histo = self.latency_hist.as_mut().unwrap();
        for l in latencies {
            let latency = l.as_nanos() as u64;
            writeln!(latency_file, "{}", latency).expect("Failed to write raw latency");
            histo.record(latency).expect("Failed to record histogram");
        }
        latency_file
            .flush()
            .expect("Failed to flush raw latency file");
    }

    fn persist_latency_summary(&mut self) {
        let meta_path = self
            .meta_results_path
            .as_ref()
            .expect("No meta results path!");
        let dir = format!("{}/latency/", meta_path);
        create_dir_all(&dir)
            .unwrap_or_else(|_| panic!("Failed to create given directory: {}", dir));
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(format!(
                "{}summary_{}.out",
                &dir,
                self.experiment_str.as_ref().unwrap()
            ))
            .expect("Failed to open latency file");
        let hist = std::mem::take(&mut self.latency_hist).unwrap();
        let quantiles = [
            0.001, 0.01, 0.005, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 0.95, 0.99, 0.999,
        ];
        for q in &quantiles {
            writeln!(
                file,
                "Value at quantile {}: {} micro s",
                q,
                hist.value_at_quantile(*q)
            )
            .expect("Failed to write summary latency file");
        }
        let max = hist.max();
        writeln!(
            file,
            "Min: {} micro s, Max: {} micro s, Average: {} micro s",
            hist.min(),
            max,
            hist.mean()
        )
        .expect("Failed to write histogram summary");
        writeln!(file, "Total elements: {}", hist.len())
            .expect("Failed to write histogram summary");
        file.flush().expect("Failed to flush histogram file");
    }

    fn persist_timeouts_summary(&mut self) {
        let sum: u64 = self.num_timed_out.iter().sum();
        if sum > 0 {
            let meta_path = self
                .meta_results_path
                .as_ref()
                .expect("No meta results path!");
            let summary_file_path = format!("{}/summary.out", meta_path);
            create_dir_all(meta_path).unwrap_or_else(|_| {
                panic!("Failed to create given directory: {}", summary_file_path)
            });
            let mut summary_file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(summary_file_path)
                .expect("Failed to open meta summary file");
            let len = self.num_timed_out.len();
            let timed_out_len = self.num_timed_out.iter().filter(|x| **x > 0).count();
            self.num_timed_out.sort_unstable();
            let min = self.num_timed_out.first().unwrap();
            let max = self.num_timed_out.last().unwrap();
            let avg = sum / (self.iteration_id as u64);
            let median = self.num_timed_out[len / 2];
            let summary_str = format!(
                "{}/{} runs had timeouts. sum: {}, avg: {}, med: {}, min: {}, max: {}",
                timed_out_len, len, sum, avg, median, min, max
            );
            writeln!(summary_file, "{}", self.experiment_str.as_ref().unwrap())
                .expect("Failed to write meta summary file");
            writeln!(summary_file, "{}", summary_str)
                .unwrap_or_else(|_| panic!("Failed to write meta summary file: {}", summary_str));
            summary_file.flush().expect("Failed to flush meta file");
        }
    }
    
    fn setup(
        &mut self,
        c: AtomicBroadcastRequest,
        m: &DeploymentMetaData,
    ) -> Result<ClientParams, BenchmarkError> {
        println!("Setting up Atomic Broadcast (Master)");
        self.validate_experiment_params(&c, m.number_of_clients())?;
        let experiment_str = format!(
            "{},{},{},{},{},{}",
            c.algorithm,
            c.number_of_nodes,
            c.concurrent_proposals,
            c.number_of_proposals,
            c.reconfiguration,
            c.reconfig_policy
        );
        self.experiment_str = Some(experiment_str);
        self.num_proposals = Some(c.number_of_proposals);
        self.concurrent_proposals = Some(c.concurrent_proposals);
        if c.concurrent_proposals == 1
            || (self.reconfiguration.is_some() && cfg!(feature = "track_latency"))
        {
            self.latency_hist =
                Some(Histogram::<u64>::new(4).expect("Failed to create latency histogram"));
        }
        let mut conf = KompactConfig::default();
        conf.load_config_file(CONFIG_PATH);
        let bc = BufferConfig::from_config_file(CONFIG_PATH);
        bc.validate();
        let tcp_no_delay = true;
        let system = atomic_broadcast::kompact_system_provider::global()
            .new_remote_system_with_threads_config("atomicbroadcast", 1, conf, bc, tcp_no_delay);
        self.system = Some(system);
        let last_node_id = if self.reconfiguration.is_some() {
            c.number_of_nodes + 1
        } else {
            c.number_of_nodes
        };
        let params = ClientParams::with(c.algorithm, last_node_id, c.reconfig_policy);
        Ok(params)
    }


    fn prepare_iteration(&mut self, d: Vec<ActorPath>) -> () {
        println!("Preparing iteration");
        if self.system.is_none() {
            panic!("No KompactSystem found!")
        }
        let finished_latch = Arc::new(CountdownEvent::new(1));
        self.finished_latch = Some(finished_latch);
        self.iteration_id += 1;
        let mut nodes_id: HashMap<u64, ActorPath> = HashMap::new();
        let num_nodes_needed = self.num_nodes.expect("No cached num_nodes") as usize;
        let mut nodes = d;
        nodes.truncate(num_nodes_needed);
        for (id, ap) in nodes.iter().enumerate() {
            nodes_id.insert(id as u64 + 1, ap.clone());
        }
        let (client_timeout, meta_path) = Self::load_benchmark_config(CONFIG_PATH);
        self.meta_results_path = meta_path;
        let leader_election_latch = Arc::new(CountdownEvent::new(1));
        let (client_comp, client_path) = self.create_client(
            nodes_id,
            client_timeout,
            self.reconfiguration.clone(),
            leader_election_latch.clone(),
        );
        let partitioning_actor = self.initialise_iteration(nodes, client_path);
        partitioning_actor
            .actor_ref()
            .tell(IterationControlMsg::Run);
        leader_election_latch.wait(); // wait until leader is established
        println!("FIRST LEADER ELECTED");
        self.partitioning_actor = Some(partitioning_actor);
        self.client_comp = Some(client_comp);
    }

    fn run_iteration(&mut self) -> () {
        println!("Running Atomic Broadcast experiment!");
        match self.client_comp {
            Some(ref client_comp) => {
                client_comp.actor_ref().tell(LocalClientMessage::Run);
                let finished_latch = self.finished_latch.take().unwrap();
                finished_latch.wait();
            }
            _ => panic!("No client found!"),
        }
    }
}
/*
impl DistributedBenchmarkMaster for AtomicBroadcastMaster {
    type MasterConf = AtomicBroadcastRequest;
    type ClientConf = ClientParams;
    type ClientData = ActorPath;

    fn setup(
        &mut self,
        c: Self::MasterConf,
        m: &DeploymentMetaData,
    ) -> Result<Self::ClientConf, BenchmarkError> {
        println!("Setting up Atomic Broadcast (Master)");
        self.validate_experiment_params(&c, m.number_of_clients())?;
        let experiment_str = format!(
            "{},{},{},{},{},{}",
            c.algorithm,
            c.number_of_nodes,
            c.concurrent_proposals,
            c.number_of_proposals,
            c.reconfiguration,
            c.reconfig_policy
        );
        self.experiment_str = Some(experiment_str);
        self.num_proposals = Some(c.number_of_proposals);
        self.concurrent_proposals = Some(c.concurrent_proposals);
        if c.concurrent_proposals == 1
            || (self.reconfiguration.is_some() && cfg!(feature = "track_latency"))
        {
            self.latency_hist =
                Some(Histogram::<u64>::new(4).expect("Failed to create latency histogram"));
        }
        let mut conf = KompactConfig::default();
        conf.load_config_file(CONFIG_PATH);
        let bc = BufferConfig::from_config_file(CONFIG_PATH);
        bc.validate();
        let tcp_no_delay = true;
        let system = crate::kompact_system_provider::global()
            .new_remote_system_with_threads_config("atomicbroadcast", 4, conf, bc, tcp_no_delay);
        self.system = Some(system);
        let last_node_id = if self.reconfiguration.is_some() {
            c.number_of_nodes + 1
        } else {
            c.number_of_nodes
        };
        let params = ClientParams::with(c.algorithm, last_node_id, c.reconfig_policy);
        Ok(params)
    }

    fn prepare_iteration(&mut self, d: Vec<Self::ClientData>) -> () {
        println!("Preparing iteration");
        if self.system.is_none() {
            panic!("No KompactSystem found!")
        }
        let finished_latch = Arc::new(CountdownEvent::new(1));
        self.finished_latch = Some(finished_latch);
        self.iteration_id += 1;
        let mut nodes_id: HashMap<u64, ActorPath> = HashMap::new();
        let num_nodes_needed = self.num_nodes.expect("No cached num_nodes") as usize;
        let mut nodes = d;
        nodes.truncate(num_nodes_needed);
        for (id, ap) in nodes.iter().enumerate() {
            nodes_id.insert(id as u64 + 1, ap.clone());
        }
        let (client_timeout, meta_path) = Self::load_benchmark_config(CONFIG_PATH);
        self.meta_results_path = meta_path;
        let leader_election_latch = Arc::new(CountdownEvent::new(1));
        let (client_comp, client_path) = self.create_client(
            nodes_id,
            client_timeout,
            self.reconfiguration.clone(),
            leader_election_latch.clone(),
        );
        let partitioning_actor = self.initialise_iteration(nodes, client_path);
        partitioning_actor
            .actor_ref()
            .tell(IterationControlMsg::Run);
        leader_election_latch.wait(); // wait until leader is established
        self.partitioning_actor = Some(partitioning_actor);
        self.client_comp = Some(client_comp);
    }

    fn run_iteration(&mut self) -> () {
        println!("Running Atomic Broadcast experiment!");
        match self.client_comp {
            Some(ref client_comp) => {
                client_comp.actor_ref().tell(LocalClientMessage::Run);
                let finished_latch = self.finished_latch.take().unwrap();
                finished_latch.wait();
            }
            _ => panic!("No client found!"),
        }
    }

    fn cleanup_iteration(&mut self, last_iteration: bool, exec_time_millis: f64) -> () {
        println!(
            "Cleaning up Atomic Broadcast (master) iteration {}. Exec_time: {}",
            self.iteration_id, exec_time_millis
        );
        let system = self.system.take().unwrap();
        let client = self.client_comp.take().unwrap();
        let meta_results = client
            .actor_ref()
            .ask_with(|promise| LocalClientMessage::Stop(Ask::new(promise, ())))
            .wait();
        self.num_timed_out.push(meta_results.num_timed_out);
        if self.concurrent_proposals == Some(1) || cfg!(feature = "track_latency") {
            self.persist_latency_results(&meta_results.latencies);
        }
        #[cfg(feature = "track_timestamps")]
        {
            let (timestamps, leader_changes_t) = meta_results
                .timestamps_leader_changes
                .expect("No timestamps results!");
            self.persist_timestamp_results(&timestamps, &leader_changes_t);
        }

        let kill_client_f = system.kill_notify(client);
        kill_client_f
            .wait_timeout(REGISTER_TIMEOUT)
            .expect("Client never died");

        if let Some(partitioning_actor) = self.partitioning_actor.take() {
            let kill_pactor_f = system.kill_notify(partitioning_actor);
            kill_pactor_f
                .wait_timeout(REGISTER_TIMEOUT)
                .expect("Partitioning Actor never died!");
        }

        if last_iteration {
            println!("Cleaning up last iteration");
            self.persist_timeouts_summary();
            if self.concurrent_proposals == Some(1) || cfg!(feature = "track_latency") {
                self.persist_latency_summary();
            }
            self.num_nodes = None;
            self.reconfiguration = None;
            self.concurrent_proposals = None;
            self.num_proposals = None;
            self.experiment_str = None;
            self.num_timed_out.clear();
            self.iteration_id = 0;
            system
                .shutdown()
                .expect("Kompact didn't shut down properly");
        } else {
            self.system = Some(system);
        }
    }
} */

pub struct AtomicBroadcastClient {
    system: Option<KompactSystem>,
    raft_comp: Option<Arc<Component<RaftComp<Storage>>>>,
}

impl AtomicBroadcastClient {
    fn new() -> AtomicBroadcastClient {
        AtomicBroadcastClient {
            system: None,
            raft_comp: None,
        }
    }
}
/*
impl DistributedBenchmarkClient for AtomicBroadcastClient {
    type ClientConf = ClientParams;
    type ClientData = ActorPath;

    fn setup(&mut self, c: Self::ClientConf) -> Self::ClientData {
        println!("Setting up Atomic Broadcast (client)");
        let mut conf = KompactConfig::default();
        conf.load_config_file(CONFIG_PATH);
        let bc = BufferConfig::from_config_file(CONFIG_PATH);
        bc.validate();
        let tcp_no_delay = true;
        let system = crate::kompact_system_provider::global()
            .new_remote_system_with_threads_config("atomicbroadcast", 4, conf, bc, tcp_no_delay);
        let named_path = match c.algorithm.as_ref() {
            "paxos" => {
                let experiment_configs = get_experiment_configs(c.last_node_id);
                let reconfig_policy = match c.reconfig_policy.as_ref() {
                    "none" => None,
                    "eager" => Some(PaxosReconfigurationPolicy::Eager),
                    "pull" => Some(PaxosReconfigurationPolicy::Pull),
                    unknown => panic!("Got unknown Paxos transfer policy: {}", unknown),
                };
                let experiment_params = ExperimentParams::load_from_file(CONFIG_PATH);
                let (paxos_comp, unique_reg_f) = system.create_and_register(|| {
                    PaxosComp::with(
                        experiment_configs,
                        reconfig_policy.unwrap_or(PaxosReconfigurationPolicy::Pull),
                        experiment_params,
                    )
                });
                unique_reg_f.wait_expect(REGISTER_TIMEOUT, "ReplicaComp failed to register!");
                let self_path = system
                    .register_by_alias(&paxos_comp, PAXOS_PATH)
                    .wait_expect(REGISTER_TIMEOUT, "Failed to register alias for ReplicaComp");
                let paxos_comp_f = system.start_notify(&paxos_comp);
                paxos_comp_f
                    .wait_timeout(REGISTER_TIMEOUT)
                    .expect("ReplicaComp never started!");
                self.paxos_comp = Some(paxos_comp);
                self_path
            }
            "raft" => {
                let voters = get_experiment_configs(c.last_node_id).0;
                let reconfig_policy = match c.reconfig_policy.as_ref() {
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

                self.raft_comp = Some(raft_comp);
                self_path
            }
            unknown => panic!("Got unknown algorithm: {}", unknown),
        };
        self.system = Some(system);
        println!("Got path for Atomic Broadcast actor: {}", named_path);
        named_path
    }

    fn prepare_iteration(&mut self) -> () {
        println!("Preparing Atomic Broadcast (client)");
    }

    fn cleanup_iteration(&mut self, last_iteration: bool) -> () {
        println!("Cleaning up Atomic Broadcast (client)");
        if let Some(paxos) = &self.paxos_comp {
            let kill_comps_f = paxos
                .actor_ref()
                .ask_with(|p| PaxosCompMsg::KillComponents(Ask::new(p, ())));
            kill_comps_f.wait();
        }
        if let Some(raft) = &self.raft_comp {
            let kill_comps_f = raft
                .actor_ref()
                .ask_with(|p| RaftCompMsg::KillComponents(Ask::new(p, ())));
            kill_comps_f.wait();
        }
        if last_iteration {
            let system = self.system.take().unwrap();
            if let Some(replica) = self.paxos_comp.take() {
                let kill_replica_f = system.kill_notify(replica);
                kill_replica_f
                    .wait_timeout(REGISTER_TIMEOUT)
                    .expect("Paxos Replica never died!");
            }
            if let Some(raft_replica) = self.raft_comp.take() {
                let kill_raft_f = system.kill_notify(raft_replica);
                kill_raft_f
                    .wait_timeout(REGISTER_TIMEOUT)
                    .expect("Raft Replica never died!");
            }
            system
                .shutdown()
                .expect("Kompact didn't shut down properly");
        }
    }
}*/


use super::*;
use crate::{atomic_broadcast::{self}};

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
    (systems, actor_paths, actor_refs)
}

fn check_quorum(sequence_responses: &[SequenceResp], quorum_size: usize, num_proposals: u64) {
    for i in 1..=num_proposals {
        let nodes: Vec<_> = sequence_responses
            .iter()
            .filter(|sr| sr.sequence.contains(&i))
            .map(|sr| sr.node_id)
            .collect();
        let timed_out_proposal = nodes.len() == 0;
        if !timed_out_proposal {
            assert!(nodes.len() >= quorum_size, "Decided value did NOT have majority quorum! proposal_id: {}, contained: {:?}, quorum: {}", i, nodes, quorum_size);
        }
    }
}

fn check_validity(sequence_responses: &[SequenceResp], num_proposals: u64) {
    let invalid_nodes: Vec<_> = sequence_responses
        .iter()
        .map(|sr| (sr.node_id, sr.sequence.iter().max().unwrap_or(&0)))
        .filter(|(_, max)| *max > &num_proposals)
        .collect();
    assert!(
        invalid_nodes.len() < 1,
        "Nodes decided unproposed values. Num_proposals: {}, invalied_nodes: {:?}",
        num_proposals,
        invalid_nodes
    );
}

fn check_uniform_agreement(sequence_responses: &[SequenceResp]) {
    let longest_seq = sequence_responses
        .iter()
        .max_by(|sr, other_sr| sr.sequence.len().cmp(&other_sr.sequence.len()))
        .expect("Empty SequenceResp from nodes!");
    for sr in sequence_responses {
        assert!(longest_seq.sequence.starts_with(sr.sequence.as_slice()));
    }
}

pub fn run_experiment(
    algorithm: &str,
    num_nodes: u64,
    num_proposals: u64,
    concurrent_proposals: u64,
    reconfiguration: &str,
    reconfig_policy: &str,
) {
    println!("SETUP");
    let mut master = AtomicBroadcastMaster::new();
    let mut experiment = AtomicBroadcastRequest::new();
    experiment.algorithm = String::from(algorithm);
    experiment.number_of_nodes = num_nodes;
    experiment.number_of_proposals = num_proposals;
    experiment.concurrent_proposals = concurrent_proposals;
    experiment.reconfiguration = String::from(reconfiguration);
    experiment.reconfig_policy = String::from(reconfig_policy);
    println!("NUM NODES");
    let num_nodes_needed = match reconfiguration {
        "off" => num_nodes,
        "single" => num_nodes + 1,
        _ => unimplemented!(),
    };
    println!("DEPLOYMENT META DATA");
    let d = DeploymentMetaData::new(num_nodes_needed as u32);
    println!("CREATE NODES");
    let (client_systems, clients, client_refs) = create_nodes(
        num_nodes_needed,
        experiment.get_algorithm(),
        experiment.get_reconfig_policy(),
        num_nodes_needed,
    );
    println!("MASTER SETUP");
    master
        .setup(experiment, &d)
        .expect("Failed to setup master");
    master.prepare_iteration(clients);
    println!("RUN ITERATION");
    master.run_iteration();
    println!("FUTURES AND CLIENT");
    let mut futures = vec![];
    for client in client_refs {
        let (kprom, kfuture) = promise::<SequenceResp>();
        let ask = Ask::new(kprom, ());
        //println!("receive getsequence??????");
        client.tell(GetSequence(ask));
        futures.push(kfuture);
    }
    println!("RESPONSES AND SEQUENCE");
    println!("futures len: {}", futures.len());
    let sequence_responses: Vec<_> = FutureCollection::collect_results::<Vec<_>>(futures);
    let quorum_size = num_nodes as usize / 2 + 1;
    check_quorum(&sequence_responses, quorum_size, num_proposals);
    check_validity(&sequence_responses, num_proposals);
    check_uniform_agreement(&sequence_responses);

    println!("CLEANUP");
    master.cleanup_iteration(true, 0.0);
    for system in client_systems {
        system.shutdown().expect("Failed to shutdown system");
    }
}
/*
#[test]
#[ignore]
fn paxos_normal_test() {
    let num_nodes = 3;
    let num_proposals = 1000;
    let concurrent_proposals = 200;
    let reconfiguration = "off";
    let reconfig_policy = "none";
    run_experiment(
        "paxos",
        num_nodes,
        num_proposals,
        concurrent_proposals,
        reconfiguration,
        reconfig_policy,
    );
}

#[test]
#[ignore]
fn paxos_reconfig_test() {
    let num_nodes = 3;
    let num_proposals = 1000;
    let concurrent_proposals = 200;
    let reconfiguration = "single";
    let reconfig_policy = "pull";
    run_experiment(
        "paxos",
        num_nodes,
        num_proposals,
        concurrent_proposals,
        reconfiguration,
        reconfig_policy,
    );
}

#[test]
fn raft_normal_test() {
    println!("running test");
    let num_nodes = 3;
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

fn raft_reconfig_follower_test() {
    let num_nodes = 3;
    let num_proposals = 1000;
    let concurrent_proposals = 200;
    let reconfiguration = "single";
    run_experiment(
        "raft",
        num_nodes,
        num_proposals,
        concurrent_proposals,
        reconfiguration,
        "replace-follower",
    );
}

#[test]
#[ignore]
fn raft_reconfig_leader_test() {
    let num_nodes = 3;
    let num_proposals = 1000;
    let concurrent_proposals = 200;
    let reconfiguration = "single";
    run_experiment(
        "raft",
        num_nodes,
        num_proposals,
        concurrent_proposals,
        reconfiguration,
        "replace-leader",
    );
}*/
