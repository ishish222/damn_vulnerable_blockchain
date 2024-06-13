use alloy::signers::wallet::{
    Wallet, 
    LocalWallet
};

use tokio::{
    self
};
use std::error::Error;

use rand::thread_rng;
use clap::{
    Parser,
    Subcommand
};

#[derive(Parser, Debug)]
struct Args {
    #[command(subcommand)]
    command: Commands,

    #[arg(short, long, default_value_t=String::from("default"))]
    wallet: String,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Create { wallet: Option<String> },
    Print { wallet: Option<String> },
    PrintPrivateKey { wallet: Option<String> },
}

use std::path::PathBuf;

use ishishnet::common::{
    ensure_ishish_home,
    ISHISH_HOME
};

async fn create_new_wallet(
    path: &PathBuf,
    full_path: &mut PathBuf
) -> Result<LocalWallet, Box<dyn Error>> {
    let mut rng = thread_rng();
    println!("Please enter a password for the wallet");
    let mut password = String::new();
    std::io::stdin().read_line(&mut password).expect("Failed to read line");

    let (wallet, _) =
        Wallet::new_keystore(&path, &mut rng, &password, Some(full_path.to_str().expect("fail")))?;

    Ok(wallet)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {

    /* setup wallet dir path */
    let mut path = ensure_ishish_home().await?;
    
    path.push(ISHISH_HOME);
    println!("Home dir: {}", path.display());


    let args = Args::parse();
    println!("Args: {:?}", args);

    match &args.command {
        Commands::Create { wallet} => {
            println!("Creating new wallet");

            let fname = match wallet {
                Some(wallet) => {
                    wallet
                },
                None => {
                    "default"
                }
            };

            let mut full_path = path.clone();
            full_path.push(&fname);
            
            if full_path.exists() {
                println!("Keystore {} already exists, do you want to overwrite it?", full_path.display());
                let mut response = String::new();
                std::io::stdin().read_line(&mut response).expect("Failed to read line");
                if response.trim() == "yes" {
                    println!("Overwriting wallet");
                    let wallet = create_new_wallet(&path, &mut full_path).await?;
                    println!("Created wallet: {} in keystore {}/{}", wallet.address(), path.display(), full_path.display());
                } else {
                    println!("Not overwriting wallet, quitting");
                }
            } else {
                let wallet = create_new_wallet(&path, &mut full_path).await?;
                println!("Created wallet: {} in keystore {}/{}", wallet.address(), path.display(), full_path.display());

            }
        },
        Commands::Print { wallet } => {
            let fname = match wallet {
                Some(wallet) => {
                    wallet
                },
                None => {
                    "default"
                }
            };

            let mut full_path = path.clone();
            full_path.push(&fname);

            println!("Please enter a password for the wallet");
            let mut password = String::new();
            std::io::stdin().read_line(&mut password).expect("Failed to read line");

            let signer = Wallet::decrypt_keystore(full_path, password)?;
            println!("Wallet: {}", signer.address());
        },
        Commands::PrintPrivateKey { wallet } => {
            let fname = match wallet {
                Some(wallet) => {
                    wallet
                },
                None => {
                    "default"
                }
            };

            let mut full_path = path.clone();
            full_path.push(&fname);

            println!("Please enter a password for the wallet");
            let mut password = String::new();
            std::io::stdin().read_line(&mut password).expect("Failed to read line");

            let signer = Wallet::decrypt_keystore(full_path, password)?;
            println!("Wallet: {}", signer.to_bytes());
        },
        _ => { 
            println!("Not implemented");
        }
    }

    Ok(())
}