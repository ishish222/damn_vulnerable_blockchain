
use std::error::Error;
use revm::db::InMemoryDB;
use alloy::primitives::{Address, U256};
use serde::{Serialize, Deserialize};

use crate::consensus::{
    DvbBlockchain,
    DvbBlock,
};


fn increase_account(
    db: &mut InMemoryDB,
    address: Address,
    amount: i64,
) -> Result<(), Box<dyn Error>> {
    if let Ok(db_acc) = db.load_account(address)
    {
        let mut new_acc_info = db_acc.info.clone();
        new_acc_info.balance += U256::from(amount);
        println!("Updated balance for {}: {:?}", address, new_acc_info);
        db.insert_account_info(address, new_acc_info);
        
    }
    Ok(())
}

fn decrease_account(
    db: &mut InMemoryDB,
    address: Address,
    amount: i64,
) -> Result<(), Box<dyn Error>> {
    if let Ok(db_acc) = db.load_account(address)
    {
        let mut new_acc_info = db_acc.info.clone();
        new_acc_info.balance -= U256::from(amount);
        println!("Updated balance for {}: {:?}", address, new_acc_info);
        db.insert_account_info(address, new_acc_info);
        
    }
    Ok(())
}

fn process_transaction(
    db: &mut InMemoryDB,
    tx: &DvbTransaction,
) -> Result<(), Box<dyn Error>>
{
    let from = tx.from;
    let to = tx.to;
    let amount = tx.amount;

    decrease_account(db, from, amount)?;
    increase_account(db, to, amount)?;
    println!("Updated balance for {}: {:?}", from, get_address_balance(db, from));
    println!("Updated balance for {}: {:?}", to, get_address_balance(db, to));
    Ok(())
}

fn remove_transaction_from_pool(
    tx: &DvbTransaction,
    transactions: &mut Vec<DvbTransaction>,
) -> Result<(), Box<dyn Error>>
{
    for (i, my_txs) in transactions.iter().enumerate() {
        if my_txs == tx {
            transactions.remove(i);
            println!("Removed transaction {:?} local pool", tx);
            println!("Current pool: {:?}", transactions);
            break;
        }
    }
    Ok(())
}


pub fn progress_state(
    db: &mut InMemoryDB, 
    block: &DvbBlock, 
    transactions: &mut Vec<DvbTransaction>
) -> Result<(), Box<dyn Error>> {
    /* reward coinbase */
    let coinbase = block.header.coinbase;

    increase_account(db, coinbase, 1)?;

    /* process transactions */
    for tx in block.content.iter() {
        process_transaction(db, tx)?;
        remove_transaction_from_pool(tx, transactions)?;
    }

    Ok(())
}

pub fn get_address_balance(
    db: &mut InMemoryDB, 
    address: Address
) -> i64 {
    let db_acc = db.load_account(address).unwrap();
    let acc_info = &db_acc.info;
    let balance = acc_info.balance;
    balance.to::<i64>()
}

pub fn refresh_state(
    db: &mut InMemoryDB, 
    chain: &DvbBlockchain, 
    transactions: &mut Vec<DvbTransaction>
) -> Result<(), Box<dyn Error>> {

    /* Progress the state for each block in the blockchain */
    for block in chain.blocks.iter() {
        progress_state(db, block, transactions)?;
    }
    Ok(())
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DvbTransaction {
    pub from: Address,
    pub to: Address,
    pub amount: i64,
}
