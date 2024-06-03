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

use crate::ishishnet::{
    IshIshBlock, 
    IshIshBlockchain, 
    IshIshError
};

pub async fn proof_of_work(
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
    mut rx: mpsc::Receiver<IshIshBlock>, 
    mut tx: mpsc::Sender<IshIshBlock>, 
    mut control_rx: watch::Receiver<bool>
    ) 
    {

    loop {
        while let Ok(_) = rx.try_recv() {}
        println!("mining_task: Waiting for start signal.");
        control_rx.changed().await.unwrap();
    
        if let Some(block) = rx.recv().await {
            println!("mining_task: Received block proposition {block:?}");
            println!("mining_task: Received start signal, commencing mining");

            tokio::select! {
                _ = control_rx.changed() => {
                    println!("Mining interrupted.");
                }
                mined_block = async {
                    let difficulty = block.header.difficulty;
                    proof_of_work(block, difficulty).await
                } => {
                    match mined_block {
                        Ok(mined_block) => {
                            println!("mining_task: Mined block");
                            if tx.send(mined_block).await.is_err() {
                                eprintln!("Failed to send mined block");
                                break;
                            }
                        }
                        Err(e) => {
                            eprintln!("Failed to mine block: {}", e);
                        }
                    }
                }
            }
        } else {
            // If `None` is received, it means all senders have been dropped and no more messages will be sent.
            println!("No more blocks to receive, terminating mining task.");
            break;
        }
    }
}

pub async fn stop_mining(
    control_tx: &watch::Sender<bool>
) -> Result<(), Box<dyn std::error::Error>> {
    
    println!("Stopping mining");

    control_tx.send(false).unwrap();
    Ok(())
}

pub async fn mine_new_block(
    blockchain: &IshIshBlockchain,
    block_tx: &mpsc::Sender<IshIshBlock>,
    control_tx: &watch::Sender<bool>,
    difficulty: usize
) -> Result<(), Box<dyn std::error::Error>> {
    
    println!("Building & sending block proposal");
    if blockchain.blocks.len() == 0 {
        let first = IshIshBlock::empty_from_content("First".into(), difficulty);
        block_tx.send(first).await?;
    }
    else {
        let mined_block = blockchain.blocks.last().unwrap();
        let new_content = format!("Block number: {}", blockchain.blocks.len());
        let next = IshIshBlock::linked_from_content(
            new_content, 
            mined_block.header.cur_hash,
            difficulty
        );
        block_tx.send(next).await?;
    }

    println!("Signalling mining start");
    control_tx.send(true).unwrap();
    Ok(())
}