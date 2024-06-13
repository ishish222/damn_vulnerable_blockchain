

use revm::{
    db::{
        CacheDB, 
        EmptyDB
    },
    Evm,
};

use crate::settlement::DvbTransaction;
use crate::consensus::{
    DvbBlockchain,
    DvbBlock,
    DvbCommand,
};
use crate::data::DvbClientBehavior;

use alloy::signers::wallet::LocalWallet;

use tokio::sync::mpsc;

use libp2p::gossipsub::IdentTopic;

pub struct Config<'a> {
    pub difficulty: usize,
    pub evm: Evm<'a, (), CacheDB<EmptyDB>>,
    pub transactions: Vec<DvbTransaction>,
    pub blockchain: DvbBlockchain,
    pub current_signer: Option<LocalWallet>,
    pub command_tx: mpsc::Sender<DvbCommand>,
    pub block_rx: mpsc::Receiver<DvbBlock>,
    pub swarm: libp2p::Swarm<DvbClientBehavior>,
    pub topic: IdentTopic,
}
