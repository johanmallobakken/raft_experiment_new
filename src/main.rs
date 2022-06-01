use std::{cell::RefCell, rc::Rc};

use crate::atomic_broadcast::atomic_broadcast::run_experiment;

mod atomic_broadcast;

extern crate raft as tikv_raft;
use kompact::prelude::{KompactSystem, ActorPath, Recipient, KompactConfig, BufferConfig, Ask, promise, FutureCollection, SimulationScenario, GetState};

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

fn raft_normal_test(simulation_ref: &mut SimulationScenario<RaftState>) {
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
        simulation_ref
    );
}

pub struct RaftState{}

fn main() {
    let mut simulation: SimulationScenario<RaftState> = SimulationScenario::new();
    raft_normal_test(&mut simulation)
}