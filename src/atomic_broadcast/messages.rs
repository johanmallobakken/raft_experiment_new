extern crate raft as tikv_raft;

use crate::atomic_broadcast::serialiser_ids;
use kompact::prelude::*;
use protobuf::{parse_from_bytes, Message};

pub const DATA_SIZE_HINT: usize = 8; // TODO

pub mod raft {
    extern crate raft as tikv_raft;
    use super::*;
    use kompact::prelude::{Buf, BufMut, Deserialiser, SerError};
    use tikv_raft::prelude::Message as TikvRaftMsg;

    pub struct RawRaftSer;

    #[derive(Debug)]
    pub struct RaftMsg(pub TikvRaftMsg); // wrapper to implement eager serialisation

    impl Serialisable for RaftMsg {
        fn ser_id(&self) -> u64 {
            serialiser_ids::RAFT_ID
        }

        fn size_hint(&self) -> Option<usize> {
            let num_entries = self.0.entries.len();
            Some(60 + num_entries * DATA_SIZE_HINT) // TODO
        }

        fn serialise(&self, buf: &mut dyn BufMut) -> Result<(), SerError> {
            let bytes = self
                .0
                .write_to_bytes()
                .expect("Protobuf failed to serialise TikvRaftMsg");
            buf.put_slice(&bytes);
            Ok(())
        }

        fn local(self: Box<Self>) -> Result<Box<dyn Any + Send>, Box<dyn Serialisable>> {
            Ok(self)
        }
    }

    impl Deserialiser<TikvRaftMsg> for RawRaftSer {
        const SER_ID: u64 = serialiser_ids::RAFT_ID;

        fn deserialise(buf: &mut dyn Buf) -> Result<TikvRaftMsg, SerError> {
            let bytes = buf.chunk();
            let remaining = buf.remaining();
            let rm: TikvRaftMsg = if bytes.len() < remaining {
                let mut dst = Vec::with_capacity(remaining);
                buf.copy_to_slice(dst.as_mut_slice());
                parse_from_bytes::<TikvRaftMsg>(dst.as_slice())
                    .expect("Protobuf failed to deserialise TikvRaftMsg")
            } else {
                parse_from_bytes::<TikvRaftMsg>(bytes)
                    .expect("Protobuf failed to deserialise TikvRaftMsg")
            };
            Ok(rm)
        }
    }
}

/*** Shared Messages***/
#[derive(Clone, Debug)]
pub struct Run;

pub const RECONFIG_ID: u64 = 0;

#[derive(Clone, Debug)]
pub struct Proposal {
    pub data: Vec<u8>,
    pub reconfig: Option<(Vec<u64>, Vec<u64>)>,
}

impl Proposal {
    pub fn reconfiguration(data: Vec<u8>, reconfig: (Vec<u64>, Vec<u64>)) -> Proposal {
        Proposal {
            data,
            reconfig: Some(reconfig),
        }
    }

    pub fn normal(data: Vec<u8>) -> Proposal {
        Proposal {
            data,
            reconfig: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProposalResp {
    pub data: Vec<u8>,
    pub latest_leader: u64,
}

impl ProposalResp {
    pub fn with(data: Vec<u8>, latest_leader: u64) -> ProposalResp {
        ProposalResp {
            data,
            latest_leader,
        }
    }
}

#[derive(Clone, Debug)]
pub enum AtomicBroadcastMsg {
    Proposal(Proposal),
    ProposalResp(ProposalResp),
    FirstLeader(u64),
    PendingReconfiguration(Vec<u8>),
    PActorInitAck(u32)
}

const PROPOSAL_ID: u8 = 1;
const PROPOSALRESP_ID: u8 = 2;
const FIRSTLEADER_ID: u8 = 3;
const PENDINGRECONFIG_ID: u8 = 4;
const INITACK_ID: u8 = 5;

impl Serialisable for AtomicBroadcastMsg {
    fn ser_id(&self) -> u64 {
        serialiser_ids::ATOMICBCAST_ID
    }

    fn size_hint(&self) -> Option<usize> {
        let msg_size = match &self {
            AtomicBroadcastMsg::Proposal(p) => {
                let reconfig_len = match p.reconfig.as_ref() {
                    Some((v, f)) => v.len() + f.len(),
                    _ => 0,
                };
                13 + DATA_SIZE_HINT + reconfig_len * 8
            }
            AtomicBroadcastMsg::ProposalResp(_) => 13 + DATA_SIZE_HINT,
            AtomicBroadcastMsg::PendingReconfiguration(_) => 5 + DATA_SIZE_HINT,
            AtomicBroadcastMsg::FirstLeader(_) => 9,
            AtomicBroadcastMsg::PActorInitAck(_) => 9,
        };
        Some(msg_size)
    }

    fn serialise(&self, buf: &mut dyn BufMut) -> Result<(), SerError> {
        match self {
            AtomicBroadcastMsg::Proposal(p) => {
                buf.put_u8(PROPOSAL_ID);
                let data = p.data.as_slice();
                let data_len = data.len() as u32;
                buf.put_u32(data_len);
                buf.put_slice(data);
                match &p.reconfig {
                    Some((voters, followers)) => {
                        let voters_len: u32 = voters.len() as u32;
                        buf.put_u32(voters_len);
                        for voter in voters.to_owned() {
                            buf.put_u64(voter);
                        }
                        let followers_len: u32 = followers.len() as u32;
                        buf.put_u32(followers_len);
                        for follower in followers.to_owned() {
                            buf.put_u64(follower);
                        }
                    }
                    None => {
                        buf.put_u32(0); // 0 voters len
                        buf.put_u32(0); // 0 followers len
                    }
                }
            }
            AtomicBroadcastMsg::ProposalResp(pr) => {
                buf.put_u8(PROPOSALRESP_ID);
                buf.put_u64(pr.latest_leader);
                let data = pr.data.as_slice();
                let data_len = data.len() as u32;
                buf.put_u32(data_len);
                buf.put_slice(data);
            }
            AtomicBroadcastMsg::FirstLeader(pid) => {
                buf.put_u8(FIRSTLEADER_ID);
                buf.put_u64(*pid);
            }
            AtomicBroadcastMsg::PendingReconfiguration(data) => {
                buf.put_u8(PENDINGRECONFIG_ID);
                let d = data.as_slice();
                buf.put_u32(d.len() as u32);
                buf.put_slice(d);
            }
            AtomicBroadcastMsg::PActorInitAck(id) => {
                buf.put_u8(INITACK_ID);
                buf.put_u32(*id);
            },
        }
        Ok(())
    }

    fn local(self: Box<Self>) -> Result<Box<dyn Any + Send>, Box<dyn Serialisable>> {
        Ok(self)
    }
}

pub struct AtomicBroadcastDeser;

impl Deserialiser<AtomicBroadcastMsg> for AtomicBroadcastDeser {
    const SER_ID: u64 = serialiser_ids::ATOMICBCAST_ID;

    fn deserialise(buf: &mut dyn Buf) -> Result<AtomicBroadcastMsg, SerError> {
        match buf.get_u8() {
            PROPOSAL_ID => {
                let data_len = buf.get_u32() as usize;
                let mut data = vec![0; data_len];
                buf.copy_to_slice(&mut data);
                let voters_len = buf.get_u32() as usize;
                let mut voters = Vec::with_capacity(voters_len);
                for _ in 0..voters_len {
                    voters.push(buf.get_u64());
                }
                let followers_len = buf.get_u32() as usize;
                let mut followers = Vec::with_capacity(followers_len);
                for _ in 0..followers_len {
                    followers.push(buf.get_u64());
                }
                let reconfig = if voters_len == 0 && followers_len == 0 {
                    None
                } else {
                    Some((voters, followers))
                };
                let proposal = Proposal { data, reconfig };
                Ok(AtomicBroadcastMsg::Proposal(proposal))
            }
            PROPOSALRESP_ID => {
                let latest_leader = buf.get_u64();
                let data_len = buf.get_u32() as usize;
                // println!("latest_leader: {}, data_len: {}, buf remaining: {}", latest_leader, data_len, buf.remaining());
                let mut data = vec![0; data_len];
                buf.copy_to_slice(&mut data);
                let pr = ProposalResp {
                    data,
                    latest_leader,
                };
                // print!(" deser ok");
                Ok(AtomicBroadcastMsg::ProposalResp(pr))
            }
            FIRSTLEADER_ID => {
                let pid = buf.get_u64();
                Ok(AtomicBroadcastMsg::FirstLeader(pid))
            }
            PENDINGRECONFIG_ID => {
                let data_len = buf.get_u32() as usize;
                // println!("latest_leader: {}, data_len: {}, buf remaining: {}", latest_leader, data_len, buf.remaining());
                let mut data = vec![0; data_len];
                buf.copy_to_slice(&mut data);
                Ok(AtomicBroadcastMsg::PendingReconfiguration(data))
            }
            INITACK_ID => {
                let id = buf.get_u32();
                Ok(AtomicBroadcastMsg::PActorInitAck(id))
            }
            _ => Err(SerError::InvalidType(
                "Found unkown id but expected RaftMsg, Proposal or ProposalResp".into(),
            )),
        }
    }
}

#[derive(Clone, Debug)]
pub enum StopMsg {
    Peer(u64),
    Client,
}

const PEER_STOP_ID: u8 = 1;
const CLIENT_STOP_ID: u8 = 2;

impl Serialisable for StopMsg {
    fn ser_id(&self) -> u64 {
        serialiser_ids::STOP_ID
    }

    fn size_hint(&self) -> Option<usize> {
        Some(9)
    }

    fn serialise(&self, buf: &mut dyn BufMut) -> Result<(), SerError> {
        match self {
            StopMsg::Peer(pid) => {
                buf.put_u8(PEER_STOP_ID);
                buf.put_u64(*pid);
            }
            StopMsg::Client => buf.put_u8(CLIENT_STOP_ID),
        }
        Ok(())
    }

    fn local(self: Box<Self>) -> Result<Box<dyn Any + Send>, Box<dyn Serialisable>> {
        Ok(self)
    }
}

pub struct StopMsgDeser;

impl Deserialiser<StopMsg> for StopMsgDeser {
    const SER_ID: u64 = serialiser_ids::STOP_ID;

    fn deserialise(buf: &mut dyn Buf) -> Result<StopMsg, SerError> {
        match buf.get_u8() {
            PEER_STOP_ID => {
                let pid = buf.get_u64();
                Ok(StopMsg::Peer(pid))
            }
            CLIENT_STOP_ID => Ok(StopMsg::Client),
            _ => Err(SerError::InvalidType(
                "Found unkown id but expected Peer stop or client stop".into(),
            )),
        }
    }
}