
use futures::stream::StreamExt;

use std::error::Error;

use tokio::{
    io, 
    io::AsyncBufReadExt, 
    select,
    sync::mpsc
};

use tracing_subscriber::EnvFilter;

use ishishnet::{
    consensus::IshIshBlockchain,
    config::Config,
    common::{
        ensure_ishish_home,
        DEFAULT_DIFFICULTY
    },
    data::build_swarm,
    settlement::IshIshTransaction
};

use revm::{
    db::{
        CacheDB, 
        EmptyDB, 
    },
    Evm,
};
use ishishnet::command::process_command;
use ishishnet::consensus::{
    process_block,
    mining_task
};
use ishishnet::data::process_event;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {

    
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    ensure_ishish_home().await?;

    /* Setting up the data availability layer */
    let (
        mut swarm, 
        topic
    ) = build_swarm()?;

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
