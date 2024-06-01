use std::convert::TryFrom;
use std::error::Error;
use std::fmt::Display;
use std::fmt::Formatter;
use std::str;
use std::str::Utf8Error;

#[derive(Debug)]
pub enum IshIshError {
    ParseError,
    InvalidMessageHeader,
    EmptyMessage,
    InvalidEvent
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

pub enum IshIshBlockchainEvent<'a> {
    NewBlockMined(&'a str),
    SthElse((&'a str, &'a str))
}

impl<'a> TryFrom<&'a Vec<u8>> for IshIshBlockchainEvent<'a> {
    type Error = IshIshError;

    fn try_from(value: &'a Vec<u8>) -> Result<Self, IshIshError> 
    {
        let value_str = str::from_utf8(value)?;
        
        let (header, value_str) = explode(value_str).ok_or(IshIshError::InvalidMessageHeader)?;
        let (message, _) = explode(value_str).ok_or(IshIshError::EmptyMessage)?;

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
            None
        }
    })
}
