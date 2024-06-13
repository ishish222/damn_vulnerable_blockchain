use std::{
    io,
    time::Duration,
    collections::hash_map::DefaultHasher,
    error::Error,
    hash::{
        Hash,
        Hasher
    },
    mem
};

use revm::{
    db::{
        CacheDB, 
        EmptyDB, 
    },
    Evm,
};

use libp2p::{
    gossipsub, 
    mdns, 
    noise, 
    swarm::NetworkBehaviour, 
    swarm::SwarmEvent, 
    gossipsub::IdentTopic,
    tcp, 
    yamux
};

use crate::consensus::{
    IshIshBlockchain, 
    IshIshCommand
};

use crate::config::Config;
use crate::common::IshIshError;
use crate::consensus::{
    process_new_blockchain,
    propose_block
};
use crate::settlement::{
    IshIshTransaction,
    refresh_state
};

use crate::common::ISHISH_TOPIC;

// We create a custom network behaviour that combines Gossipsub and Mdns.
#[derive(NetworkBehaviour)]
pub struct IshIshClientBehavior {
    pub gossipsub: gossipsub::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
}

pub async fn swarm_publish(
    swarm: &mut libp2p::Swarm<IshIshClientBehavior>, 
    topic: &IdentTopic,
    message: &str
) -> Result<(), Box<dyn Error>> {
    if let Err(e) = swarm
        .behaviour_mut().gossipsub
        .publish(topic.clone(), message.as_bytes()) {
            println!("Publish error: {e:?}");
        }
    Ok(())
}

pub async fn broadcast_new_blockchain(
    swarm: &mut libp2p::Swarm<IshIshClientBehavior>, 
    topic: &IdentTopic, 
    blockchain: &IshIshBlockchain
) -> Result<(), Box<dyn Error>> {
    /* Broadcast info about the new blockchain via data availability layer */
    let mut line = String::from("NBM");
    let blockchain_serialized = serde_json::to_string(&blockchain)?;
    line.push_str(&blockchain_serialized);

    println!("Sending line: {line:?}");
    
    swarm_publish(swarm, topic, &line).await?;
    Ok(())
}

pub async fn broadcast_new_transaction(
    swarm: &mut libp2p::Swarm<IshIshClientBehavior>, 
    topic: &IdentTopic, 
    transaction: &IshIshTransaction
) -> Result<(), Box<dyn Error>> {
    /* Broadcast info about the new blockchain via data availability layer */
    let mut line = String::from("TRA");
    let transaction_serialized = serde_json::to_string(&transaction)?;
    line.push_str(&transaction_serialized);

    println!("Sending line: {line:?}");
    swarm_publish(swarm, topic, &line).await?;
    Ok(())
}

pub fn build_swarm(
) -> Result<(libp2p::Swarm<IshIshClientBehavior>, IdentTopic), Box<dyn Error>> {
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
    let topic = gossipsub::IdentTopic::new(ISHISH_TOPIC);

    // subscribes to our topic
    swarm.behaviour_mut().gossipsub.subscribe(&topic)?;
    Ok((swarm, topic))
}

async fn process_blockchain_event(
    event: libp2p::gossipsub::Event,
    cfg: &mut Config<'_>
) -> Result<(), Box<dyn Error>> {

    if let libp2p::gossipsub::Event::Message { message, ..} = event {
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
                refresh_state(
                    &mut cfg.evm.db_mut(), 
                    &cfg.blockchain, 
                    &mut cfg.transactions
                )?;
    
                /* Get block proposition */
                if let Some(signer) = &cfg.current_signer
                {
                    let new_block = propose_block(
                        signer.address(), 
                        &cfg.blockchain, 
                        cfg.difficulty, 
                        &mut cfg.transactions
                    ).await?;                                    
                    cfg.command_tx.send(IshIshCommand::MineBlock(new_block)).await?;

                } else {
                    println!("No wallet opened, can't propose block");
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
    } // else ignore because it's probably data layer event

    Ok(())
}

pub async fn process_event(
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

pub enum IshIshBlockchainEvent<'a> {
    NewBlockMined(&'a str),
    SthElse((&'a str, &'a str)),
    NewSignedTransaction(&'a str),
}

impl<'a> TryFrom<&'a Vec<u8>> for IshIshBlockchainEvent<'a> {
    type Error = IshIshError;

    fn try_from(value: &'a Vec<u8>) -> Result<Self, IshIshError> 
    {
        let value_str = std::str::from_utf8(value)?;
    
        let (header, message) = (&value_str[..3], &value_str[3..]); // good example for threat modelling

        match header {
            "NBM" => {
                return Ok(IshIshBlockchainEvent::NewBlockMined(message))
            },
            "TRA" => {
                return Ok(IshIshBlockchainEvent::NewSignedTransaction(message))
            },
            _ => return Ok(IshIshBlockchainEvent::SthElse((header, message)))
        }
    }
}

