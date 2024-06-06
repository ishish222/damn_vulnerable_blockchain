use std::convert::TryInto;

use sha2::{
    Sha256, 
    Digest
};

use rand::Rng;
use tokio::sync::{
    mpsc,
    watch
};

use crate::blockchain::{
    IshIshBlock, 
    IshIshBlockchain, 
    IshIshError,
    IshIshCommand
};

pub fn proof_of_work(
    mut block: IshIshBlock, 
    difficulty: usize
    ) -> Result<IshIshBlock, IshIshError> {
    println!("proof_of_work::start");

    let mut nonce = rand::thread_rng().gen();
    loop {
        let mut hasher = Sha256::new();
        block.header.nonce = nonce;

        let data = serde_json::to_string(&block)?;

        hasher.update(data);

        let hash: [u8; 32] = match hasher.finalize().try_into() {
            Ok(arr) => arr,
            Err(_) => return Err(IshIshError::HashConversionFailed), 
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
    mut command_rx: mpsc::Receiver<IshIshCommand>,
    mut block_tx: mpsc::Sender<IshIshBlock>
    ) -> Result<(), IshIshError> {

    let mut current_block: Option<IshIshBlock> = None;
    let mut running = false;
    
    loop {
        tokio::select! {
            cmd = command_rx.recv() => {
                match cmd {
                    Some(IshIshCommand::MineBlock(block)) => {
                        println!("mining_task::Updating current_block");
                        current_block = Some(block);
                    },
                    Some(IshIshCommand::Stop) => {
                        println!("mining_task::Stopping mining");
                        running = false;
                    },
                    Some(IshIshCommand::Start) => {
                        println!("mining_task::Starting mining");
                        running = true;
                    },
                    Some(IshIshCommand::Restart) => {
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
                        block_tx.send(mined_block).await;
                        current_block = None;
                    },
                    None => {}
                }
            }
        }
    }
}

pub async fn propose_block(
    blockchain: &IshIshBlockchain,
    difficulty: usize
) -> Result<IshIshBlock, Box<dyn std::error::Error>> {
    
    println!("Building a block proposal");
    if blockchain.blocks.len() == 0 {
        Ok(IshIshBlock::empty_from_content("Genesis".into(), difficulty))
    }
    else {
        let mined_block = blockchain.blocks.last().unwrap();
        let new_content = format!("Block number: {}", blockchain.blocks.len());
        let next = IshIshBlock::linked_from_content(
            new_content, 
            mined_block.header.cur_hash,
            difficulty
        );
        Ok(next)
    }
}