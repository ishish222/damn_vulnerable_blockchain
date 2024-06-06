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
    SthElse((&'a str, &'a str))
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
            _ => return Ok(IshIshBlockchainEvent::SthElse((header, message)))
        }
    }
}

fn explode(message: &str) -> Option<(&str, &str)> {
    message.chars().enumerate().find_map(|(i, c)| { 
        if c == ' ' {
            Some((&message[..i], &message[i+1..]))
        }
        else {
            Some((&message[..], ""))
        }
    })
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IshIshBlockHeader {
    pub nonce: u64,
    pub difficulty: usize,
    pub cur_hash: [u8; 32],
    prev_hash: [u8; 32],
}

impl IshIshBlockHeader {
    pub fn empty(difficulty: usize) -> Self {
        Self {
            nonce: 0,
            difficulty: difficulty,
            cur_hash: [0; 32],
            prev_hash: [0; 32]
        }
    }

    pub fn from_prev_hash(prev_hash: [u8; 32], difficulty: usize) -> Self {
        Self {
            nonce: 0,
            difficulty: difficulty,            
            cur_hash: [0; 32],
            prev_hash: prev_hash
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IshIshBlock {
    pub header: IshIshBlockHeader,
    content: String,
}

impl IshIshBlock {
    pub fn empty_from_content(content: String, difficulty: usize) -> Self {
        Self {
            header: IshIshBlockHeader::empty(difficulty),
            content: content
        }
    }

    pub fn linked_from_content(content: String, prev_hash: [u8; 32], difficulty: usize) -> Self {
        Self {
            header: IshIshBlockHeader::from_prev_hash(prev_hash, difficulty),
            content: content
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct IshIshBlockchain {
    pub blocks: Vec<IshIshBlock>,
}

impl IshIshBlockchain {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new()
        }
    }

    pub fn append(&mut self, mut block: IshIshBlock) -> Result<(), IshIshError> {
        self.verify_block(block.clone())?;
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

    pub fn verify_chain(&self) -> Result<(), IshIshError> {
        
        /* First check the pow of each block */
        for block in self.blocks.iter() {
            self.verify_block(block.clone())?;
        }

        /* Then check the links */
        for i in 1..self.blocks.len() {
            if self.blocks[i].header.prev_hash != self.blocks[i-1].header.cur_hash {
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