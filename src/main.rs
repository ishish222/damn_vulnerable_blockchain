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
    IshIshBlock,
    IshIshCommand
};

mod mining;

use mining::{
    propose_block,
    mining_task
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

/* consumes both blockchains */
fn process_new_blockchain(
    new_blockchain: IshIshBlockchain, 
    current_blockchain: IshIshBlockchain, 
) -> Result<IshIshBlockchain, Box<dyn Error>> {

    println!("Got new blockchain: {new_blockchain:?}, verifying");

    if new_blockchain.blocks.len() > current_blockchain.blocks.len()
    {
        println!("Received blockchain is heavier, verifying hashes");
        match new_blockchain.verify_chain() {
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

    println!("Starting the local mining task");
    let (command_tx, command_rx) = mpsc::channel(100);
    let (block_tx, mut block_rx) = mpsc::channel(100);

    let difficulty: usize = match std::env::args().nth(1)
    {
        Some(v) => v.parse::<usize>().unwrap(),
        None => DEFAULT_DIFFICULTY as usize
    };

    tokio::spawn(mining_task(command_rx, block_tx));

    let genesis = propose_block(&my_blockchain, difficulty).await?;
    command_tx.send(IshIshCommand::MineBlock(genesis)).await?;

    // Kick it off
    loop {
        select! {
            Ok(Some(line)) = stdin.next_line() => {

                /* Here we process commands from stdin */

                match line.as_str() {
                    "start" => {
                        command_tx.send(IshIshCommand::Start).await?;
                    },
                    "stop" => {
                        command_tx.send(IshIshCommand::Stop).await?;
                    },
                    _ => {
                        println!("Unknown command: {line}");
                    }
                }
            },
            Some(mined_block) = block_rx.recv() => {

                /* Event - we successfuly mined requested block */

                println!("Successfuly mined block: {:?}", mined_block);

                /* Add the new block to my_blockchain */
                if let Err(e) = my_blockchain.append(mined_block.clone()) {
                    println!("Append error: {e:?}");
                }

                /* Get block proposition */
                let new_block = propose_block(&my_blockchain, difficulty).await?;

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

                            /* Get block proposition */
                            let new_block = propose_block(&my_blockchain, difficulty).await?;

                            /* Send the command w/ new proposition */
                            command_tx.send(IshIshCommand::MineBlock(new_block)).await?;
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
