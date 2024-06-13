use std::fmt::{Display, Formatter};
use std::str::Utf8Error;

use std::{
    path::PathBuf,
    error::Error,
    env,
    fs
};

pub const ISHISH_HOME: &str = ".ishish";
pub const ISHISH_TOPIC: &str = "test-net";
pub const DEFAULT_DIFFICULTY: u32 = 2;

pub async fn ensure_ishish_home(
) -> Result<PathBuf, Box<dyn Error>> {
    /* setup wallet dir path */
    let mut path = PathBuf::new();
    let home_dir = env::var_os("HOME").expect("HOME is not set in env.");
    path.push(home_dir);
    path.push(ISHISH_HOME);

    if !path.exists() {
        println!("Creating ishish home dir");
        fs::create_dir_all(&path).expect("Failed to create ishish home dir");
    }
    Ok(path)
}


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

