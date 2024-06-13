use std::error::Error;

use crate::config::Config;
use crate::consensus::{
    IshIshCommand,
    propose_block
};
use crate::common::ensure_ishish_home;
use crate::data::broadcast_new_transaction;
use crate::settlement::{
    get_address_balance,
    IshIshTransaction
};

use alloy::signers::wallet::Wallet;
use alloy::primitives::Address;


async fn start_command(
    cfg: &mut Config<'_>
) -> Result<(), Box<dyn Error>> {
    match &cfg.current_signer {
        Some(signer) => {

            /* Get block proposition */
            let new_block = propose_block(
                signer.address(), 
                &cfg.blockchain, 
                cfg.difficulty, 
                &mut cfg.transactions
            ).await?;                                

            /* Send the new block to the mining thread */
            cfg.command_tx.send(IshIshCommand::MineBlock(new_block)).await?;
            cfg.command_tx.send(IshIshCommand::Start).await?;
        },
        None => {
            println!("Please open a wallet first");
        }
    };

    Ok(())
}

async fn stop_command(
    cfg: &mut Config<'_>
) -> Result<(), Box<dyn Error>> {
    cfg.command_tx.send(IshIshCommand::Stop).await?;
    Ok(())
}

async fn open_command(
    cfg: &mut Config<'_>
) -> Result<(), Box<dyn Error>> {

    println!("Enter the name of the wallet [default]");
    let mut wallet_name = String::new();

    std::io::stdin().read_line(&mut wallet_name)?;
    if wallet_name.trim().is_empty() {
        wallet_name = "default".to_string();
    }
    
    println!("Please enter a password for the wallet");
    let mut password = String::new();

    std::io::stdin().read_line(&mut password)?;

    let mut full_path = ensure_ishish_home().await?;

    full_path.push(&wallet_name.trim());

    println!("Opening wallet: {}", full_path.display());
    cfg.current_signer = match Wallet::decrypt_keystore(
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
    };
    Ok(())
}

async fn get_balance(
    cfg: &mut Config<'_>
) -> Result<(), Box<dyn Error>> {

    println!("Enter the name of the wallet [coinbase]");

    let mut address = String::new();
    std::io::stdin().read_line(&mut address)?;

    if address.trim().is_empty() {
        match &cfg.current_signer {
            Some(signer) => {
                let address = signer.address();
                println!("Balance of {address}: {}", get_address_balance(cfg.evm.db_mut(), address));
            },
            None => {
                println!("Please open a wallet first");
            }
        }
    } else {
        let checksummed = address.trim();
        let address = Address::parse_checksummed(checksummed, None)?;
        
        println!("Balance of {address}: {}", get_address_balance(cfg.evm.db_mut(), address));
    }
    Ok(())
}

async fn print_pool(
    cfg: &mut Config<'_>
) -> Result<(), Box<dyn Error>> {
    println!("Current pool: {:?}", cfg.transactions);
    Ok(())
}

async fn get_line(
    prompt: &str,
) -> Result<String, Box<dyn Error>> {
    println!("{prompt}");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

async fn get_address(
    prompt: &str,
    cfg: &mut Config<'_>
) -> Result<Address, Box<dyn Error>> {
    let input = get_line(prompt).await?;

    if input.trim().is_empty() {
        match &cfg.current_signer {
            Some(signer) => {
                return Ok(signer.address());
            },
            None => {
                println!("Please open a wallet first");
                Err("No wallet opened".into())
            }
        }
    }
    else {
        Ok(Address::parse_checksummed(input, None)?)
    }
}

async fn get_amount(
    prompt: &str
) -> Result<i64, Box<dyn Error>> {
    let input = get_line(prompt).await?;
    Ok(input.trim().parse::<i64>().unwrap())
}

async fn send_ish(
    cfg: &mut Config<'_>
) -> Result<(), Box<dyn Error>> {

    let src = get_address("Enter the name of the source wallet [coinbase]", cfg).await?;
    let dst = get_address("Enter the target wallet", cfg).await?;
    let amount = get_amount("How much ish to send?").await?;

    println!("Sending {amount} ish from {src} to {dst}");

    /* Prepare the transaction */
    let transaction = IshIshTransaction {
        from: src,
        to: dst,
        amount,
    };

    /* Broadcast the transaction */
    broadcast_new_transaction(
        &mut cfg.swarm, 
        &cfg.topic, 
        &transaction
    ).await?;

    /* Add to local pool */
    cfg.transactions.push(transaction);
    println!("Transaction added to local pool");
    println!("Current pool: {:?}", cfg.transactions);
    Ok(())
}

pub async fn process_command(
    command: &str,
    cfg: &mut Config<'_>
) -> Result<(), Box<dyn Error>> {

    println!("Processing command: {command}");

    match command {
        "start" => start_command(cfg).await?,
        "stop" => stop_command(cfg).await?,
        "open" => open_command(cfg).await?,
        "get_balance" => get_balance(cfg).await?,
        "print_pool" => print_pool(cfg).await?,
        "send_ish" => send_ish(cfg).await?,
        _ => {
            println!("Unknown command: {command}");
        }
    }    
    Ok(())
}
