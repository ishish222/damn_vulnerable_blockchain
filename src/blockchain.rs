use std::{
    convert::TryFrom,
    error::Error,
    fmt::{
        Display,
        Formatter
    },
    str::{
        self,
        Utf8Error
    }
};

use sha2::{Sha256, Digest};
use serde::{Serialize, Deserialize};
use rand::Rng;

#[derive(Debug)]
pub enum IshIshError {
    ParseError,
    InvalidMessageHeader,
    EmptyMessage,
    InvalidEvent,
    MiningError,
    HashConversionFailed,
    InvalidProofOfWork,
    PrevHashMismatch,
    EmptyBlockchain,
    RequestedBlockIsNone
}

use alloy::primitives::{
    Address,
};

pub enum IshIshCommand {
    MineBlock(IshIshBlock),
    Start,
    Restart,
    Stop
}

impl Display for IshIshError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> 
    { 
        write!(f, "Error");
        Ok(())
    }
}

impl Error for IshIshError {}

impl From<Utf8Error> for IshIshError {
    fn from(_: Utf8Error) -> Self {
        IshIshError::ParseError
    }
}

impl From<serde_json::Error> for IshIshError {
    fn from(_: serde_json::Error) -> Self {
        IshIshError::ParseError
    }
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
        let value_str = str::from_utf8(value)?;
    
        let (header, message) = (&value_str[..3], &value_str[3..]); // good example for threat modelling

        //let (header, value_str) = explode(value_str).ok_or(IshIshError::InvalidMessageHeader)?;
        //let (message, _) = explode(value_str).ok_or(IshIshError::EmptyMessage)?; //needs corrections

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

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct IshIshTransaction {
    pub from: Address,
    pub to: Address,
    pub amount: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IshIshBlockHeader {
    pub coinbase: Address,
    pub number: u64,
    pub nonce: u64,
    pub difficulty: usize,
    pub cur_hash: [u8; 32],
    prev_hash: [u8; 32],
}

impl IshIshBlockHeader {
    pub fn no_prev(coinbase: Address, difficulty: usize) -> Self {
        Self {
            coinbase: coinbase,
            number: 0,
            nonce: 0,
            difficulty: difficulty,
            cur_hash: [0; 32],
            prev_hash: [0; 32]
        }
    }

    pub fn from_prev_block(coinbase: Address, prev_block: &IshIshBlock, difficulty: usize) -> Self {
        Self {
            coinbase: coinbase,
            number: prev_block.header.number + 1,
            nonce: 0,
            difficulty: difficulty,            
            cur_hash: [0; 32],
            prev_hash: prev_block.header.cur_hash
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IshIshBlock {
    pub header: IshIshBlockHeader,
    pub content: Vec<IshIshTransaction>,
}

impl IshIshBlock {
    pub fn no_prev(coinbase: Address, transactions: &mut Vec<IshIshTransaction>, difficulty: usize) -> Self {
        let mut content = Vec::new();

        /* We include at most top 3 transactions */
        let num_transactions = transactions.len().min(3);
        for i in 0..num_transactions {
            content.push(transactions[i].clone());
        }
        
        Self {
            header: IshIshBlockHeader::no_prev(coinbase, difficulty),
            content: content
        }
    }

    pub fn from_prev_block(coinbase: Address, transactions: &mut Vec<IshIshTransaction>, prev_block: &IshIshBlock, difficulty: usize) -> Self {
        let mut content = Vec::new();
        
        /* We include at most top 3 transactions */
        let num_transactions = transactions.len().min(3);
        for i in 0..num_transactions {
            content.push(transactions[i].clone());
        }
        
        Self {
            header: IshIshBlockHeader::from_prev_block(coinbase, prev_block, difficulty),
            content: content
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct IshIshBlockchain {
    pub blocks: Vec<IshIshBlock>,
}

impl IshIshBlockchain {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new()
        }
    }

    pub fn append(&mut self, block: IshIshBlock) -> Result<(), IshIshError> {
        self.verify_block(block.clone())?;
        /* update internal state */
        self.blocks.push(block);
        Ok(())
    }
    
    fn verify_block(&self, block: IshIshBlock) -> Result<(), IshIshError> {
        let pow_ok = validate_pow(block.clone(), block.header.difficulty)?;
        
        // check POW
        if !pow_ok {
            return Err(IshIshError::InvalidProofOfWork);
        }        
        Ok(())
    }

    pub fn verify_chain(chain: &IshIshBlockchain) -> Result<(), IshIshError> {
        
        /* First check the pow of each block */
        for block in chain.blocks.iter() {
            chain.verify_block(block.clone())?;
        }

        /* Then check the links */
        for i in 1..chain.blocks.len() {
            if chain.blocks[i].header.prev_hash != chain.blocks[i-1].header.cur_hash {
                return Err(IshIshError::PrevHashMismatch);
            }
        }

        Ok(())
    }
}

fn validate_pow(mut block: IshIshBlock, difficulty: usize) -> Result<bool, IshIshError> {
    let mut hasher = Sha256::new();

    block.header.cur_hash = [0; 32];

    let data = serde_json::to_string(&block)?;
    hasher.update(data);

    let hash: [u8; 32] = match hasher.finalize().try_into() {
        Ok(arr) => arr,
        Err(_) => return Err(IshIshError::HashConversionFailed), 
    };

    if hash.iter().take(difficulty).all(|&b| b == 0) {
        block.header.cur_hash = hash;
        Ok(true)
    }
    else {
        Ok(false)
    }
}