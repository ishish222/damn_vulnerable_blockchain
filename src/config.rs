

use revm::{
    db::{
        CacheDB, 
        EmptyDB
    },
    Evm,
};

use crate::settlement::IshIshTransaction;
use crate::consensus::{
    IshIshBlockchain,
    IshIshBlock,
    IshIshCommand,
};
use crate::data::IshIshClientBehavior;

use alloy::signers::wallet::LocalWallet;

use tokio::sync::mpsc;

use libp2p::gossipsub::IdentTopic;

pub struct Config<'a> {
    pub difficulty: usize,
    pub evm: Evm<'a, (), CacheDB<EmptyDB>>,
    pub transactions: Vec<IshIshTransaction>,
    pub blockchain: IshIshBlockchain,
    pub current_signer: Option<LocalWallet>,
    pub command_tx: mpsc::Sender<IshIshCommand>,
    pub block_rx: mpsc::Receiver<IshIshBlock>,
    pub swarm: libp2p::Swarm<IshIshClientBehavior>,
    pub topic: IdentTopic,
}
