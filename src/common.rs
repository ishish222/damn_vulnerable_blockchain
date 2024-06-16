use std::fmt::{Display, Formatter};
use std::str::Utf8Error;

use std::{
    path::PathBuf,
    error::Error,
    env,
    fs
};

use tokio::sync::mpsc;

use crate::consensus::DvbBlock;

pub const DVB_HOME: &str = ".dvb";
pub const DVB_TOPIC: &str = "test-net";
pub const DEFAULT_DIFFICULTY: u32 = 2;

pub async fn ensure_dvb_home(
) -> Result<PathBuf, Box<dyn Error>> {
    /* setup wallet dir path */
    let mut path = PathBuf::new();
    let home_dir = env::var_os("HOME").unwrap();
    path.push(home_dir);
    path.push(DVB_HOME);

    if !path.exists() {
        println!("Creating dvb home dir");
        fs::create_dir_all(&path)?;
    }
    Ok(path)
}


#[derive(Debug)]
pub enum DvbError {
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

impl Display for DvbError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> 
    { 
        write!(f, "Error")?;
        Ok(())
    }
}

impl Error for DvbError {}

impl From<Utf8Error> for DvbError {
    fn from(_: Utf8Error) -> Self {
        DvbError::ParseError
    }
}

impl From<serde_json::Error> for DvbError {
    fn from(_: serde_json::Error) -> Self {
        DvbError::ParseError
    }
}

impl From<mpsc::error::SendError<DvbBlock>> for DvbError {
    fn from(_: mpsc::error::SendError<DvbBlock>) -> Self {
        DvbError::ParseError
    }
}

