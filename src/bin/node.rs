
use futures::stream::StreamExt;
use libp2p::{
    gossipsub, 
    mdns, 
    noise, 
    swarm::NetworkBehaviour, 
    swarm::SwarmEvent, 
    tcp, 
    yamux
};
use std::collections::hash_map::DefaultHasher;
use std::error::Error;
use std::hash::{
    Hash, 
    Hasher
};
use std::time::Duration;
use tokio::{
    io, 
    io::AsyncBufReadExt, 
    select
};
use tracing_subscriber::EnvFilter;

use tokio::sync::mpsc;

use ishishnet::blockchain::{
    IshIshBlock, IshIshBlockchain, IshIshBlockchainEvent, IshIshCommand, IshIshError, IshIshTransaction
};

use alloy::signers::wallet::{Wallet, LocalWallet};
use alloy::primitives::{
    Address,
    address,
    U256,
    utils::format_units,
    
};

use ishishnet::mining::{
    propose_block,
    mining_task
};

use revm::{
    db::{CacheDB, EmptyDB, InMemoryDB, DbAccount, },
    primitives::{AccountInfo},
    Evm,
};

const DEFAULT_DIFFICULTY : usize = 3;

// We create a custom network behaviour that combines Gossipsub and Mdns.
#[derive(NetworkBehaviour)]
struct IshIshClientBehavior {
    gossipsub: gossipsub::Behaviour,
    mdns: mdns::tokio::Behaviour,
}

async fn broadcast_new_blockchain(
    swarm: &mut libp2p::Swarm<IshIshClientBehavior>, 
    topic: &gossipsub::IdentTopic, 
    blockchain: &IshIshBlockchain
) -> Result<(), Box<dyn Error>> {
    /* Broadcast info about the new blockchain via data availability layer */
    let mut line = String::from("NBM");
    let blockchain_serialized = serde_json::to_string(&blockchain)?;
    line.push_str(&blockchain_serialized);

    println!("Sending line: {line:?}");
    
    if let Err(e) = swarm
        .behaviour_mut().gossipsub
        .publish(topic.clone(), line.as_bytes()) {
            println!("Publish error: {e:?}");
        }
    Ok(())
}

async fn broadcast_new_transaction(
    swarm: &mut libp2p::Swarm<IshIshClientBehavior>, 
    topic: &gossipsub::IdentTopic, 
    transaction: &IshIshTransaction
) -> Result<(), Box<dyn Error>> {
    /* Broadcast info about the new blockchain via data availability layer */
    let mut line = String::from("TRA");
    let transaction_serialized = serde_json::to_string(&transaction)?;
    line.push_str(&transaction_serialized);

    println!("Sending line: {line:?}");
    
    if let Err(e) = swarm
        .behaviour_mut().gossipsub
        .publish(topic.clone(), line.as_bytes()) {
            println!("Publish error: {e:?}");
        }
    Ok(())
}

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

fn progress_state(db: &mut InMemoryDB, block: &IshIshBlock, my_transactions: &mut Vec<IshIshTransaction>) {
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
        for (i, my_txs) in my_transactions.iter().enumerate() {
            if my_txs == tx {
                my_transactions.remove(i);
                println!("Removed transaction {:?} local pool", tx);
                println!("Current pool: {:?}", my_transactions);
                break;
            }
        }
    }
}

fn get_balance(db: &mut InMemoryDB, address: Address) -> u64 {
    let db_acc = db.load_account(address).unwrap();
    let acc_info = &db_acc.info;
    let balance = acc_info.balance;
    balance.to::<u64>()
}

fn refresh_state(db: &mut InMemoryDB, chain: &IshIshBlockchain, my_transactions: &mut Vec<IshIshTransaction>) {
    for block in chain.blocks.iter() {
        progress_state(db, block, my_transactions);
    }
}


use ishishnet::utils::{
    ensure_ishish_home,
    ISHISH_HOME
};

use std::env;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {

    /* setup wallet dir path */
    let mut path = PathBuf::new();
    let home_dir = env::var_os("HOME").expect("HOME is not set in env.");
    path.push(home_dir);
    ensure_ishish_home(&path).await?;
    
    path.push(ISHISH_HOME);
    println!("Home dir: {}", path.display());
    
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    /* Setting up the data availability layer */

    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|key| {
            // To content-address message, we can take the hash of message and use it as an ID.
            let message_id_fn = |message: &gossipsub::Message| {
                let mut s = DefaultHasher::new();
                message.data.hash(&mut s);
                gossipsub::MessageId::from(s.finish().to_string())
            };

            // Set a custom gossipsub configuration
            let gossipsub_config = gossipsub::ConfigBuilder::default()
                .heartbeat_interval(Duration::from_secs(10)) // This is set to aid debugging by not cluttering the log space
                .validation_mode(gossipsub::ValidationMode::Strict) // This sets the kind of message validation. The default is Strict (enforce message signing)
                .message_id_fn(message_id_fn) // content-address messages. No two messages of the same content will be propagated.
                .build()
                .map_err(|msg| io::Error::new(io::ErrorKind::Other, msg))?; // Temporary hack because `build` does not return a proper `std::error::Error`.

            // build a gossipsub network behaviour
            let gossipsub = gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Signed(key.clone()),
                gossipsub_config,
            )?;

            let mdns =
                mdns::tokio::Behaviour::new(mdns::Config::default(), key.public().to_peer_id())?;
            Ok(IshIshClientBehavior { gossipsub, mdns })
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    // Create a Gossipsub topic
    let topic = gossipsub::IdentTopic::new("test-net");
    // subscribes to our topic
    swarm.behaviour_mut().gossipsub.subscribe(&topic)?;

    // Read full lines from stdin
    println!("Enter messages via STDIN and they will be sent to connected peers using Gossipsub");
    let mut stdin = io::BufReader::new(io::stdin()).lines();

    // Listen on all interfaces and whatever port the OS assigns
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    /* Local state */
    let mut my_state = CacheDB::new(EmptyDB::default());

    /* Local EVM */
    let mut my_evm = Evm::builder().with_db(my_state).build();
    let mut my_transactions: Vec<IshIshTransaction> = Vec::new();

    /* Local blockchain */
    let mut my_blockchain = IshIshBlockchain::new();

    println!("Starting the local mining task");
    let (command_tx, command_rx) = mpsc::channel(100);
    let (block_tx, mut block_rx) = mpsc::channel(100);

    let difficulty: usize = match std::env::args().nth(1)
    {
        Some(v) => v.parse::<usize>().unwrap(),
        None => DEFAULT_DIFFICULTY as usize
    };

    tokio::spawn(mining_task(command_rx, block_tx));

    let mut current_signer: Option<LocalWallet> = None;

    // Kick it off
    loop {
        select! {
            Ok(Some(line)) = stdin.next_line() => {
                let line = line.trim();
                /* Here we process commands from stdin */

                println!("Received line: {line}");

                match line {
                    "start" => {
                        let signer = current_signer.clone();
                        match signer {
                            Some(signer) => {

                                /* Get block proposition */
                                let new_block = propose_block(signer.address(), &my_blockchain, difficulty, &mut my_transactions).await?;                                
                                command_tx.send(IshIshCommand::MineBlock(new_block)).await?;
                                command_tx.send(IshIshCommand::Start).await?;
                            },
                            None => {
                                println!("Please open a wallet first");
                            }
                        };
                        
                    },
                    "stop" => {
                        command_tx.send(IshIshCommand::Stop).await?;
                    },
                    "open" => {
                        println!("Enter the name of the wallet [default]");
                        let mut wallet_name = String::new();
                        //let mut wallet_name = stdin.next_line().await.unwrap().unwrap();
                        std::io::stdin().read_line(&mut wallet_name).expect("Failed to read line");
                        if wallet_name.trim().is_empty() {
                            wallet_name = "default".to_string();
                        }
                        
                        println!("Please enter a password for the wallet");
                        let mut password = String::new();
                        //let mut password = stdin.next_line().await.unwrap().unwrap();
                        std::io::stdin().read_line(&mut password).expect("Failed to read line");

                        let mut full_path = path.clone();
                        full_path.push(&wallet_name.trim());

                        println!("Opening wallet: {}", full_path.display());
                        current_signer = match Wallet::decrypt_keystore(full_path, password) {
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
                        //let mut wallet_name = stdin.next_line().await.unwrap().unwrap();
                        std::io::stdin().read_line(&mut address).expect("Failed to read line");
                        if address.trim().is_empty() {
                            let address = current_signer.clone().unwrap().address();
                            println!("Balance of {address}: {}", get_balance(my_evm.db_mut(), address));
                        } else {
                            let checksummed = address.trim();
                            let address = Address::parse_checksummed(checksummed, None).expect("valid checksum");
                            
                            println!("Balance of {address}: {}", get_balance(my_evm.db_mut(), address));
                        }
                    },                    
                    "print_pool" => {
                        println!("Current pool: {:?}", my_transactions);
                    },
                    "send_ish" => {
                        let mut read_str = String::new();
                        let mut src = Address::new([0x0; 20]);
                        let mut dst = Address::new([0x0; 20]);

                        println!("Enter the name of the source wallet [coinbase]");
                        std::io::stdin().read_line(&mut read_str).expect("Failed to read line");
                        if read_str.trim().is_empty() {
                            src = current_signer.clone().unwrap().address();

                        } else {
                            let checksummed = read_str.trim();
                            src = Address::parse_checksummed(checksummed, None).expect("valid checksum");
                        }

                        println!("Enter the target wallet");
                        read_str.clear();
                        std::io::stdin().read_line(&mut read_str).expect("Failed to read line");
                        if read_str.trim().is_empty() {
                            dst = current_signer.clone().unwrap().address();

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
                        broadcast_new_transaction(&mut swarm, &topic, &transaction).await?;

                        /* Add to local pool */
                        my_transactions.push(transaction);
                        println!("Transaction added to local pool");
                        println!("Current pool: {:?}", my_transactions);
                    },
                    _ => {
                        println!("Unknown command: {line}");
                    }
                }
            },
            Some(mined_block) = block_rx.recv() => {

                /* Event - we successfuly mined requested block */

                println!("Successfuly mined block: {:?}", mined_block);
                progress_state(my_evm.db_mut(), &mined_block, &mut my_transactions);

                /* Add the new block to my_blockchain */
                if let Err(e) = my_blockchain.append(mined_block.clone()) {
                    println!("Append error: {e:?}");
                }

                /* Get block proposition */
                let signer = current_signer.clone().unwrap();
                let new_block = propose_block(signer.address(), &my_blockchain, difficulty, &mut my_transactions).await?;      

                /* Send the command w/ new proposition */
                command_tx.send(IshIshCommand::MineBlock(new_block)).await?;

                /* We broadcast info about the new blockchain via data availability layer */
                broadcast_new_blockchain(&mut swarm, &topic, &my_blockchain).await?;
            },

            /* Processing events from the data availability layer */
            event = swarm.select_next_some() => match event {
                SwarmEvent::Behaviour(IshIshClientBehaviorEvent::Mdns(mdns::Event::Discovered(list))) => {
                    for (peer_id, _multiaddr) in list {
                        println!("mDNS discovered a new peer: {peer_id}");
                        swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                    }
                },
                SwarmEvent::Behaviour(IshIshClientBehaviorEvent::Mdns(mdns::Event::Expired(list))) => {
                    for (peer_id, _multiaddr) in list {
                        println!("mDNS discover peer has expired: {peer_id}");
                        swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                    }
                },
                SwarmEvent::Behaviour(IshIshClientBehaviorEvent::Gossipsub(gossipsub::Event::Message {
                    propagation_source: peer_id,
                    message_id: id,
                    message,
                })) => {
                    match IshIshBlockchainEvent::try_from(&message.data)? {
                        IshIshBlockchainEvent::NewBlockMined(serialized) => {
                            /* Deserializing */
                            let new_blockchain: IshIshBlockchain = serde_json::from_str(&serialized)?;

                            /* Processing, consume both and return selected */
                            my_blockchain = process_new_blockchain(
                                new_blockchain, 
                                my_blockchain
                            )?;

                            /* We need to recreate the internal state */
                            my_state = CacheDB::new(EmptyDB::default());
                            my_evm = Evm::builder().with_db(my_state).build();
                            refresh_state(&mut my_evm.db_mut(), &my_blockchain, &mut my_transactions);

                            /* Get block proposition */
                            let signer = current_signer.clone();
                            match signer {
                                Some(signer) => {
    
                                    /* Get block proposition */
                                    let new_block = propose_block(signer.address(), &my_blockchain, difficulty, &mut my_transactions).await?;                                    
                                    command_tx.send(IshIshCommand::MineBlock(new_block)).await?;
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
                            my_transactions.push(transaction);
                            println!("Transaction added to local pool");
                            println!("Current pool: {:?}", my_transactions);

                        },
                        IshIshBlockchainEvent::SthElse((msg,re)) => {
                            println!("Something else: {msg} {re}");
                        }
                    }
                },
                SwarmEvent::NewListenAddr { address, .. } => {
                    println!("Local node is listening on {address}");
                }
                _ => {}
            }
        }
    }
}
