use std::error::Error;
use std::convert::TryInto;
use serde::{Serialize, Deserialize};

use sha2::{
    Sha256, 
    Digest
};

use rand::Rng;
use tokio::sync::mpsc;

use alloy::primitives::Address;

use crate::data::broadcast_new_blockchain;
use crate::settlement::{
    progress_state,
    DvbTransaction
};
use crate::config::Config;
use crate::common::DvbError;

pub fn process_new_blockchain(
    new_blockchain: DvbBlockchain, 
    current_blockchain: DvbBlockchain, 
) -> Result<DvbBlockchain, Box<dyn Error>> {

    println!("Got new blockchain: {new_blockchain:?}, verifying");

    if new_blockchain.blocks.len() > current_blockchain.blocks.len()
    {
        println!("Received blockchain is heavier, verifying hashes");
        match DvbBlockchain::verify_chain(&new_blockchain) {
            Ok(()) => {
                println!("Verification passed, accepting new blockchain as local");
                Ok(new_blockchain)
            }
            Err(e) => {
                println!("Blockchain verification failed {e:?}, ignoring");
                Ok(current_blockchain)
            }
        }
    } else {
        println!("Received blockchain is lighter, ignoring");
        Ok(current_blockchain)
    }
}

pub async fn process_block(
    block: DvbBlock, 
    cfg: &mut Config<'_>
) -> Result<(), Box<dyn Error>> {
    println!("Successfuly mined block: {:?}", block);

    progress_state(
        cfg.evm.db_mut(), 
        &block, 
        &mut cfg.transactions
    )?;

    /* Add the new block to the blockchain */
    cfg.blockchain.append(block.clone())?;

    /* Get block proposition */
    let signer = cfg.current_signer.clone().unwrap();
    let new_block = propose_block(
        signer.address(), 
        &cfg.blockchain, 
        cfg.difficulty, 
        &mut cfg.transactions
    ).await?;      

    /* Send the command w/ new proposition */
    cfg.command_tx.send(DvbCommand::MineBlock(new_block)).await?;

    /* We broadcast info about the new blockchain via data availability layer */
    broadcast_new_blockchain(
        &mut cfg.swarm, 
        &cfg.topic, 
        &cfg.blockchain
    ).await?;

    Ok(())
}

fn validate_pow(mut block: DvbBlock, difficulty: usize) -> Result<bool, DvbError> {
    let mut hasher = Sha256::new();

    block.header.cur_hash = [0; 32];

    let data = serde_json::to_string(&block)?;
    hasher.update(data);

    let hash: [u8; 32] = match hasher.finalize().try_into() {
        Ok(arr) => arr,
        Err(_) => return Err(DvbError::HashConversionFailed), 
    };

    if hash.iter().take(difficulty).all(|&b| b == 0) {
        block.header.cur_hash = hash;
        Ok(true)
    }
    else {
        Ok(false)
    }
}

pub enum DvbCommand {
    MineBlock(DvbBlock),
    Start,
    Restart,
    Stop
}


#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DvbBlockHeader {
    pub coinbase: Address,
    pub number: u64,
    pub nonce: u64,
    pub difficulty: usize,
    pub cur_hash: [u8; 32],
    prev_hash: [u8; 32],
}

impl DvbBlockHeader {
    pub fn no_prev(coinbase: Address, difficulty: usize) -> Self {
        Self {
            coinbase: coinbase,
            number: 0,
            nonce: 0,
            difficulty: difficulty,
            cur_hash: [0; 32],
            prev_hash: [0; 32]
        }
    }

    pub fn from_prev_block(coinbase: Address, prev_block: &DvbBlock, difficulty: usize) -> Self {
        Self {
            coinbase: coinbase,
            number: prev_block.header.number + 1,
            nonce: 0,
            difficulty: difficulty,            
            cur_hash: [0; 32],
            prev_hash: prev_block.header.cur_hash
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DvbBlock {
    pub header: DvbBlockHeader,
    pub content: Vec<DvbTransaction>,
}

impl DvbBlock {
    pub fn no_prev(coinbase: Address, transactions: &mut Vec<DvbTransaction>, difficulty: usize) -> Self {
        let mut content = Vec::new();

        /* We include at most top 3 transactions */
        let num_transactions = transactions.len().min(3);
        for i in 0..num_transactions {
            content.push(transactions[i].clone());
        }
        
        Self {
            header: DvbBlockHeader::no_prev(coinbase, difficulty),
            content: content
        }
    }

    pub fn from_prev_block(coinbase: Address, transactions: &mut Vec<DvbTransaction>, prev_block: &DvbBlock, difficulty: usize) -> Self {
        let mut content = Vec::new();
        
        /* We include at most top 3 transactions */
        let num_transactions = transactions.len().min(3);
        for i in 0..num_transactions {
            content.push(transactions[i].clone());
        }
        
        Self {
            header: DvbBlockHeader::from_prev_block(coinbase, prev_block, difficulty),
            content: content
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct DvbBlockchain {
    pub blocks: Vec<DvbBlock>,
}

impl DvbBlockchain {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new()
        }
    }

    pub fn append(&mut self, block: DvbBlock) -> Result<(), DvbError> {
        self.verify_block(block.clone())?;
        /* update internal state */
        self.blocks.push(block);
        Ok(())
    }
    
    fn verify_block(&self, block: DvbBlock) -> Result<(), DvbError> {
        let pow_ok = validate_pow(block.clone(), block.header.difficulty)?;
        
        // check POW
        if !pow_ok {
            return Err(DvbError::InvalidProofOfWork);
        }        
        Ok(())
    }

    pub fn verify_chain(chain: &DvbBlockchain) -> Result<(), DvbError> {
        
        /* First check the pow of each block */
        for block in chain.blocks.iter() {
            chain.verify_block(block.clone())?;
        }

        /* Then check the links */
        for i in 1..chain.blocks.len() {
            if chain.blocks[i].header.prev_hash != chain.blocks[i-1].header.cur_hash {
                return Err(DvbError::PrevHashMismatch);
            }
        }

        Ok(())
    }
}


pub fn proof_of_work(
    mut block: DvbBlock, 
    difficulty: usize
    ) -> Result<DvbBlock, DvbError> {
    println!("proof_of_work::start");

    let mut nonce = rand::thread_rng().gen();
    loop {
        let mut hasher = Sha256::new();
        block.header.nonce = nonce;

        let data = serde_json::to_string(&block)?;

        hasher.update(data);

        let hash: [u8; 32] = match hasher.finalize().try_into() {
            Ok(arr) => arr,
            Err(_) => return Err(DvbError::HashConversionFailed), 
        };

        if hash.iter().take(difficulty).all(|&b| b == 0) {
            block.header.cur_hash = hash;
            println!("proof_of_work::finish");
            return Ok(block);
        }
        nonce += 1;
    }
}

pub async fn mining_task(
    mut command_rx: mpsc::Receiver<DvbCommand>,
    block_tx: mpsc::Sender<DvbBlock>
    ) -> Result<(), DvbError> {

    let mut current_block: Option<DvbBlock> = None;
    let mut running = false;
    
    loop {
        tokio::select! {
            cmd = command_rx.recv() => {
                match cmd {
                    Some(DvbCommand::MineBlock(block)) => {
                        println!("mining_task::Updating current_block");
                        current_block = Some(block);
                    },
                    Some(DvbCommand::Stop) => {
                        println!("mining_task::Stopping mining");
                        running = false;
                    },
                    Some(DvbCommand::Start) => {
                        println!("mining_task::Starting mining");
                        running = true;
                    },
                    Some(DvbCommand::Restart) => {
                        println!("mining_task::Restarting mining");
                        running = true;
                    },
                    None => {}
                }
            },
            mined_block = async {
                if !running || current_block.is_none() {
                    return None // kill this async
                }

                println!("Starting the mining for a new block");
                let block = current_block.clone().unwrap();
                let difficulty = block.header.difficulty;
                Some(proof_of_work(block, difficulty).ok()?)
            } => {
                match mined_block {
                    Some(mined_block) => {
                        println!("mining_task: Mined block");
                        block_tx.send(mined_block).await?;
                        current_block = None;
                    },
                    None => {}
                }
            }
        }
    }
}

pub async fn propose_block(
    coinbase: Address, 
    blockchain: &DvbBlockchain,
    difficulty: usize,
    transactions: &mut Vec<DvbTransaction>
) -> Result<DvbBlock, Box<dyn std::error::Error>> {
    
    println!("Building a block proposal");
    if blockchain.blocks.len() == 0 {
        Ok(DvbBlock::no_prev(coinbase, transactions, difficulty))
    }
    else {
        let mined_block = blockchain.blocks.last().unwrap();
        
        let next = DvbBlock::from_prev_block(
            coinbase,
            transactions, 
            &mined_block,
            difficulty
        );
        Ok(next)
    }
}