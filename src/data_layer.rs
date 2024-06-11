use std::{
    io,
    time::Duration,
    collections::hash_map::DefaultHasher,
    error::Error,
    hash::{
        Hash,
        Hasher
    }
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

use crate::blockchain::{
    IshIshBlockchain, 
    IshIshTransaction
};

use crate::utils::{
    ISHISH_TOPIC
};

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