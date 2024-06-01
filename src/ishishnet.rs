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
    PrevHashMismatch
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


pub async fn proof_of_work(
    mut block: IshIshBlock, 
    difficulty: usize
    ) -> Result<IshIshBlock, IshIshError> {
    println!("proof_of_work::start");

    let mut nonce = rand::thread_rng().gen();
    loop {
        let mut hasher = Sha256::new();
        block.header.nonce = nonce;

        let data = serde_json::to_string(&block)?;

        hasher.update(data);

        let hash: [u8; 32] = match hasher.finalize().try_into() {
            Ok(arr) => arr,
            Err(_) => return Err(IshIshError::HashConversionFailed), 
        };

        if hash.iter().take(difficulty).all(|&b| b == 0) {
            block.header.cur_hash = hash;
            println!("proof_of_work::finish");
            return Ok(block);
        }
        nonce += 1;
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


#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IshIshBlockHeader {
    nonce: u64,
    pub cur_hash: [u8; 32],
    prev_hash: [u8; 32],
}

impl IshIshBlockHeader {
    pub fn empty() -> Self {
        Self {
            nonce: 0,
            cur_hash: [0; 32],
            prev_hash: [0; 32]
        }
    }

    pub fn from_prev_hash(prev_hash: [u8; 32]) -> Self {
        Self {
            nonce: 0,
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
    pub fn empty_from_content(content: String) -> Self {
        Self {
            header: IshIshBlockHeader::empty(),
            content: content
        }
    }

    pub fn linked_from_content(content: String, prev_hash: [u8; 32]) -> Self {
        Self {
            header: IshIshBlockHeader::from_prev_hash(prev_hash),
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

    pub fn append(&mut self, mut block: IshIshBlock, difficulty: usize) -> Result<(), IshIshError> {
        self.verify_block(block.clone(), difficulty)?;
        self.blocks.push(block);
        Ok(())
    }

    fn verify_block(&self, block: IshIshBlock, difficulty: usize) -> Result<(), IshIshError> {
        let pow_ok = validate_pow(block.clone(), difficulty)?;
        
        // check POW
        if !pow_ok {
            return Err(IshIshError::InvalidProofOfWork);
        }

        // check link to previous block
        match self.blocks.last() {
            Some(last_block) => {
                if block.header.prev_hash != last_block.header.cur_hash
                {
                    return Err(IshIshError::PrevHashMismatch);
                }
            },
            None => {
                // first block, always good
            }
        }
        
        Ok(())
    }

    pub fn verify_chain(&self) -> Result<(), IshIshError> {
        // For now I assume its good xD
        Ok(())
    }
}