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
use tokio::sync::watch;

mod ishishnet;

use ishishnet::{
    IshIshBlockchainEvent,
    IshIshBlockchain,
    proof_of_work,
    IshIshBlock,
};

const DEFAULT_DIFFICULTY : usize = 3;

// We create a custom network behaviour that combines Gossipsub and Mdns.
#[derive(NetworkBehaviour)]
struct IshIshClientBehavior {
    gossipsub: gossipsub::Behaviour,
    mdns: mdns::tokio::Behaviour,
}

// routine for mining a block

async fn mining_task(
    mut rx: mpsc::Receiver<IshIshBlock>, 
    mut tx: mpsc::Sender<IshIshBlock>, 
    mut control_rx: watch::Receiver<bool>, 
    difficulty: usize) 
    {

    control_rx.changed().await.unwrap();

    while let Some(block) = rx.recv().await {
        println!("mining_task: Received block proposition {block:?}");

        tokio::select! {
            _ = control_rx.changed() => {
                println!("Received stop signal, terminating mining task.");
                continue;
            }
            mined_block = async {
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
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
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


    /* Local blockchain */
    let mut my_blockchain = IshIshBlockchain::new();

    println!("Starting the local mining thread");
    let (block_tx, block_rx) = mpsc::channel(10);
    let (mined_block_tx, mut mined_block_rx) = mpsc::channel(10);
    let (control_tx, control_rx) = watch::channel(false);

    let difficulty: usize = match std::env::args().nth(1)
    {
        Some(v) => v.parse::<usize>().unwrap(),
        None => DEFAULT_DIFFICULTY as usize
    };
    
    tokio::spawn(mining_task(
        block_rx, 
        mined_block_tx, 
        control_rx, 
        difficulty));

    /* Request mining of the first block */
    let first = IshIshBlock::empty_from_content("First".into());
    block_tx.send(first).await?;

    // Kick it off
    loop {
        select! {
            Ok(Some(line)) = stdin.next_line() => {
                /* 
                if let Err(e) = swarm
                    .behaviour_mut().gossipsub
                    .publish(topic.clone(), line.as_bytes()) {
                    println!("Publish error: {e:?}");
                }*/
                match line.as_str() {
                    "start" => {
                        println!("Starting the local mining thread");
                        control_tx.send(true).unwrap();
                    },
                    _ => {
                        println!("Unknown command: {line}");
                    }
                }
            },
            Some(mined_block) = mined_block_rx.recv() => {
                println!("Successfuly mined block: {:?}", mined_block);

                /* Add the new block to my_blockchain */
                if let Err(e) = my_blockchain.append(mined_block.clone(), difficulty) {
                    println!("Append error: {e:?}");
                }

                /* Send info about the new blockchain */
                let mut line = String::from("NBM");
                let blockchain_serialized = serde_json::to_string(&my_blockchain)?;
                line.push_str(&blockchain_serialized);

                println!("Sending line: {line:?}");
                
                if let Err(e) = swarm
                    .behaviour_mut().gossipsub
                    .publish(topic.clone(), line.as_bytes()) {
                        println!("Publish error: {e:?}");
                    }

                // Requestng mining new block
                let new_content = format!("Block number: {}", my_blockchain.blocks.len());
                let mut next = IshIshBlock::linked_from_content(
                    new_content, 
                    mined_block.header.cur_hash
                );
                block_tx.send(next).await?;

            },
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
                            
                            let deserialized: IshIshBlockchain = serde_json::from_str(&serialized)?;
                            println!("Got new blockchain: {deserialized:?}, verifying");

                            if deserialized.blocks.len() > my_blockchain.blocks.len()
                            {
                                println!("Received blockchain is heavier, verifying hashes");
                                match deserialized.verify_chain() {
                                    Ok(()) => {
                                        println!("Verification passed, need to restart mining");
                                        my_blockchain = deserialized;

                                        /*  need to restart the mining thread */
                                        control_tx.send(true).unwrap();

                                        // Requestng mining new block
                                        let mined_block = my_blockchain.blocks.last().unwrap();
                                        let new_content = format!("Block number: {}", my_blockchain.blocks.len());

                                        let mut next = IshIshBlock::linked_from_content(
                                            new_content, 
                                            mined_block.header.cur_hash
                                        );

                                        println!("Requesting mining of new block: {next:?}");
                                        block_tx.send(next).await?;
                                    }
                                    Err(e) => {
                                        println!("Blockchain verification failed, ignoring");
                                    }
                                }
                            }
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
