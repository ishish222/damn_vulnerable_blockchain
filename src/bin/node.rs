
use futures::stream::StreamExt;
use libp2p::{
    mdns, 
    swarm::SwarmEvent, 
    gossipsub::IdentTopic,
};

use std::{
    error::Error,
    mem
};

use tokio::{
    io, 
    io::AsyncBufReadExt, 
    select,
    sync::mpsc
};

use tracing_subscriber::EnvFilter;

use ishishnet::{
    blockchain::{
        IshIshBlock, 
        IshIshBlockchain, 
        IshIshBlockchainEvent, 
        IshIshCommand, 
        IshIshTransaction
    },
    utils::{
        ensure_ishish_home,
        DEFAULT_DIFFICULTY
    },
    data_layer::{
        build_swarm,
        broadcast_new_blockchain,
        broadcast_new_transaction,
        IshIshClientBehavior,
        IshIshClientBehaviorEvent
    },
    mining::{
        propose_block,
        mining_task
    }
};

use alloy::{
    signers::wallet::{
        Wallet, 
        LocalWallet
    },
    primitives::{
        Address,
        U256,   
    }
};

use revm::{
    db::{
        CacheDB, 
        EmptyDB, 
        InMemoryDB
    },
    primitives::AccountInfo,
    Evm,
};

/* consumes both blockchains */
fn process_new_blockchain(
    new_blockchain: IshIshBlockchain, 
    current_blockchain: IshIshBlockchain, 
) -> Result<IshIshBlockchain, Box<dyn Error>> {

    println!("Got new blockchain: {new_blockchain:?}, verifying");

    if new_blockchain.blocks.len() > current_blockchain.blocks.len()
    {
        println!("Received blockchain is heavier, verifying hashes");
        match IshIshBlockchain::verify_chain(&new_blockchain) {
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

fn progress_state(
    db: &mut InMemoryDB, 
    block: &IshIshBlock, 
    transactions: &mut Vec<IshIshTransaction>
) -> Result<(), Box<dyn Error>> {
    /* reward coinbase */
    let coinbase = block.header.coinbase;

    let mut acc_info = AccountInfo::default();
    {
        let db_acc = db.load_account(coinbase).unwrap();
        acc_info = db_acc.info.clone();
    } // drop db_acc so that mut ref can be taken again
    
    let mut new_acc_info = acc_info.clone();
    new_acc_info.balance = acc_info.balance + U256::from(1);
    println!("Updated balance for {}: {:?}", coinbase, new_acc_info);
    
    db.insert_account_info(coinbase, new_acc_info);

    /* process transactions */
    for tx in block.content.iter() {
        let from = tx.from;
        let to = tx.to;
        let amount = tx.amount;

        let mut from_acc_info = AccountInfo::default();
        {
            let db_acc = db.load_account(from).unwrap();
            from_acc_info = db_acc.info.clone();
        } // drop db_acc so that mut ref can be taken again

        let mut to_acc_info = AccountInfo::default();
        {
            let db_acc = db.load_account(to).unwrap();
            to_acc_info = db_acc.info.clone();
        } // drop db_acc so that mut ref can be taken again

        let mut new_from_acc_info = from_acc_info.clone();
        new_from_acc_info.balance = from_acc_info.balance - U256::from(amount);
        println!("Updated balance for {}: {:?}", from, new_from_acc_info);
        
        let mut new_to_acc_info = to_acc_info.clone();
        new_to_acc_info.balance = to_acc_info.balance + U256::from(amount);
        println!("Updated balance for {}: {:?}", to, new_to_acc_info);

        db.insert_account_info(from, new_from_acc_info);
        db.insert_account_info(to, new_to_acc_info);

        /* Remove the transaction from the pool */
        for (i, my_txs) in transactions.iter().enumerate() {
            if my_txs == tx {
                transactions.remove(i);
                println!("Removed transaction {:?} local pool", tx);
                println!("Current pool: {:?}", transactions);
                break;
            }
        }
    }

    Ok(())
}

fn get_balance(
    db: &mut InMemoryDB, 
    address: Address
) -> u64 {
    let db_acc = db.load_account(address).unwrap();
    let acc_info = &db_acc.info;
    let balance = acc_info.balance;
    balance.to::<u64>()
}

fn refresh_state(
    db: &mut InMemoryDB, 
    chain: &IshIshBlockchain, 
    transactions: &mut Vec<IshIshTransaction>
) -> Result<(), Box<dyn Error>> {

    /* Progress the state for each block in the blockchain */
    for block in chain.blocks.iter() {
        progress_state(db, block, transactions)?;
    }
    Ok(())
}

async fn process_command(
    command: &str,
    config: &mut Config<'_>
) -> Result<(), Box<dyn Error>> {

    println!("Processing command: {command}");

    match command {
        "start" => {
            match &config.current_signer {
                Some(signer) => {

                    /* Get block proposition */
                    let new_block = propose_block(
                        signer.address(), 
                        &config.blockchain, 
                        config.difficulty, 
                        &mut config.transactions
                    ).await?;                                

                    /* Send the new block to the mining thread */
                    config.command_tx.send(IshIshCommand::MineBlock(new_block)).await?;
                    config.command_tx.send(IshIshCommand::Start).await?;
                },
                None => {
                    println!("Please open a wallet first");
                }
            };
        },
        "stop" => {
            config.command_tx.send(IshIshCommand::Stop).await?;
        },
        "open" => {
            println!("Enter the name of the wallet [default]");
            let mut wallet_name = String::new();

            std::io::stdin().read_line(&mut wallet_name)
                .expect("Failed to read line");
            if wallet_name.trim().is_empty() {
                wallet_name = "default".to_string();
            }
            
            println!("Please enter a password for the wallet");
            let mut password = String::new();

            std::io::stdin().read_line(&mut password)
                .expect("Failed to read line");

            let mut full_path = ensure_ishish_home().await
                .expect("Failed to ensure ishish home dir");

            full_path.push(&wallet_name.trim());

            println!("Opening wallet: {}", full_path.display());
            config.current_signer = match Wallet::decrypt_keystore(
                full_path, 
                password
            ) {
                Ok(wallet) => {
                    println!("Opened wallet: {}", wallet.address());
                    Some(wallet)
                },
                Err(e) => {
                    println!("Failed to open wallet: {e:?}");
                    None
                }
            }

        },
        "get_balance" => {
            println!("Enter the name of the wallet [coinbase]");

            let mut address = String::new();
            
            std::io::stdin().read_line(&mut address)
                .expect("Failed to read line");

            if address.trim().is_empty() {
                match &config.current_signer {
                    Some(signer) => {
                        let address = signer.address();
                        println!("Balance of {address}: {}", get_balance(config.evm.db_mut(), address));
                    },
                    None => {
                        println!("Please open a wallet first");
                    }
                }
            } else {
                let checksummed = address.trim();
                let address = Address::parse_checksummed(checksummed, None)
                    .expect("valid checksum");
                
                println!("Balance of {address}: {}", get_balance(config.evm.db_mut(), address));
            }
        },                    
        "print_pool" => {
            println!("Current pool: {:?}", config.transactions);
        },
        "send_ish" => {
            let mut read_str = String::new();
            let mut src = Address::new([0x0; 20]);
            let mut dst = Address::new([0x0; 20]);

            println!("Enter the name of the source wallet [coinbase]");
            std::io::stdin().read_line(&mut read_str).expect("Failed to read line");
            if read_str.trim().is_empty() {
                src = match &config.current_signer {
                    Some(signer) => signer.address(),
                    None => {
                        println!("Please open a wallet first");
                        return Ok(());
                    }
                };

            } else {
                let checksummed = read_str.trim();
                src = Address::parse_checksummed(checksummed, None)
                    .expect("valid checksum");
            }

            println!("Enter the target wallet");
            read_str.clear();
            std::io::stdin().read_line(&mut read_str).expect("Failed to read line");
            if read_str.trim().is_empty() {
                dst = match &config.current_signer {
                    Some(signer) => signer.address(),
                    None => {
                        println!("Please open a wallet first");
                        return Ok(());
                    }
                };

            } else {
                let checksummed = read_str.trim();
                dst = Address::parse_checksummed(checksummed, None).expect("valid checksum");
            }

            println!("How much ish to send?");
            read_str.clear();
            std::io::stdin().read_line(&mut read_str).expect("Failed to read line");
            let read_str = read_str.trim();
            println!("Trying to cenvert {read_str} to u64");
            let amount = read_str.trim().parse::<u64>().unwrap();

            println!("Sending {amount} ish from {src} to {dst}");
            
            /* Prepare the transaction */
            let transaction = IshIshTransaction {
                from: src,
                to: dst,
                amount: amount,
            };

            /* Broadcast the transaction */
            broadcast_new_transaction(
                &mut config.swarm, 
                &config.topic, 
                &transaction
            ).await?;

            /* Add to local pool */
            config.transactions.push(transaction);
            println!("Transaction added to local pool");
            println!("Current pool: {:?}", config.transactions);
        },
        _ => {
            println!("Unknown command: {command}");
        }
    }    
    Ok(())
}

async fn process_block(
    block: IshIshBlock, 
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
    cfg.command_tx.send(IshIshCommand::MineBlock(new_block)).await?;

    /* We broadcast info about the new blockchain via data availability layer */
    broadcast_new_blockchain(
        &mut cfg.swarm, 
        &cfg.topic, 
        &cfg.blockchain
    ).await?;

    Ok(())
}

async fn process_blockchain_event(
    event: libp2p::gossipsub::Event,
    cfg: &mut Config<'_>
) -> Result<(), Box<dyn Error>> {

    match event {
        libp2p::gossipsub::Event::Message { 
            propagation_source: _, 
            message_id: _, 
            message } => {
                match IshIshBlockchainEvent::try_from(&message.data)? {
                    IshIshBlockchainEvent::NewBlockMined(serialized) => {
                        /* Deserializing */
                        let new_blockchain: IshIshBlockchain = serde_json::from_str(&serialized)?;
            
                        /* Processing, consume both and return selected */
                        cfg.blockchain = process_new_blockchain(
                            new_blockchain, 
                            mem::take(&mut cfg.blockchain)
                        )?;
            
                        /* We need to recreate the internal state */
                        let new_state = CacheDB::new(EmptyDB::default());
                        cfg.evm = Evm::builder().with_db(new_state).build();
                        refresh_state(&mut cfg.evm.db_mut(), &cfg.blockchain, &mut cfg.transactions)?;
            
                        /* Get block proposition */
                        let signer = cfg.current_signer.clone();
                        match signer {
                            Some(signer) => {
            
                                /* Get block proposition */
                                let new_block = propose_block(signer.address(), &cfg.blockchain, cfg.difficulty, &mut cfg.transactions).await?;                                    
                                cfg.command_tx.send(IshIshCommand::MineBlock(new_block)).await?;
                                /* We just update the block, we don't start because we don't know the mining status */
                            },
                            None => {
                                println!("No wallet opened, can't propose block");
                            }
                        };
                    },
                    IshIshBlockchainEvent::NewSignedTransaction(transaction_str) => {
                        let transaction: IshIshTransaction = serde_json::from_str(&transaction_str)?;
            
                        println!("Got new transaction: {:?}", transaction);
            
                        /* Add to local pool */
                        cfg.transactions.push(transaction);
                        println!("Transaction added to local pool");
                        println!("Current pool: {:?}", cfg.transactions);
            
                    },
                    IshIshBlockchainEvent::SthElse((msg,re)) => {
                        println!("Something else: {msg} {re}");
                    }
            
            }
        },
        _ => { }
    }

    Ok(())
}

async fn process_event(
    event: SwarmEvent<IshIshClientBehaviorEvent>, //why not dyn here?
    cfg: &mut Config<'_>
) -> Result<(), Box<dyn Error>> {
    
    match event {
        SwarmEvent::Behaviour(IshIshClientBehaviorEvent::Mdns(mdns::Event::Discovered(list))) => {
            for (peer_id, _multiaddr) in list {
                println!("mDNS discovered a new peer: {peer_id}");
                cfg.swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
            }
        },
        SwarmEvent::Behaviour(IshIshClientBehaviorEvent::Mdns(mdns::Event::Expired(list))) => {
            for (peer_id, _multiaddr) in list {
                println!("mDNS discover peer has expired: {peer_id}");
                cfg.swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
            }
        },
        SwarmEvent::Behaviour(IshIshClientBehaviorEvent::Gossipsub(event)) => {
            process_blockchain_event(event, cfg).await?;
        },
        SwarmEvent::NewListenAddr { address, .. } => {
            println!("Local node is listening on {address}");
        }
        _ => {}
    }
    Ok(())

}

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {

    
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    ensure_ishish_home().await.expect("Failed to ensure ishish home dir");

    /* Setting up the data availability layer */
    let (
        mut swarm, 
        topic
    ) = build_swarm().expect("Failed to build swarm");

    // Read full lines from stdin
    println!("Reading commands from stdin");
    let mut stdin = io::BufReader::new(io::stdin()).lines();

    // Listen on all interfaces and whatever port the OS assigns
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    /* Local representation of the blockchain */

    /* Local state */
    let my_state = CacheDB::new(EmptyDB::default());

    /* Local EVM */
    let my_evm = Evm::builder().with_db(my_state).build();

    /* Local transaction pool */
    let my_transactions: Vec<IshIshTransaction> = Vec::new();

    /* Local blockchain */
    let my_blockchain = IshIshBlockchain::new();

    
    /* Prepare local mining task */
    
    /* Channels for commands and blocks */
    let (command_tx, command_rx) = mpsc::channel(10);
    let (block_tx, block_rx) = mpsc::channel(10);

    /* Set the difficulty */
    let difficulty: usize = match std::env::args().nth(1)
    {
        Some(v) => v.parse::<usize>().unwrap(),
        None => DEFAULT_DIFFICULTY as usize
    };

    println!("Starting the local mining task");
    tokio::spawn(mining_task(command_rx, block_tx));

    /* Currently selected wallet for signing */
    //let mut current_signer: Option<LocalWallet> = None;
    // moving everything into new struct

    let mut cfg = Config {
        difficulty: difficulty,
        evm: my_evm,
        transactions: my_transactions,
        blockchain: my_blockchain,
        current_signer: None,
        command_tx: command_tx,
        block_rx: block_rx,
        swarm: swarm,
        topic: topic,
    };

    // Kick it off
    loop {
        select! {
            Ok(Some(line)) = stdin.next_line() => {
                /* Here we process the commands from stdin */
                let line = line.trim();
                process_command(line, &mut cfg).await?;

            },
            Some(mined_block) = cfg.block_rx.recv() => {
                /* Here we process the newly mined block */
                process_block(mined_block, &mut cfg).await?;
            },
            event = cfg.swarm.select_next_some() =>  {
                /* Here we process the events from the data availability layer */
                process_event(event, &mut cfg).await?;
            }
        }
    }
}
