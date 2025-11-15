/// implementation of the bus exchanges, this is the tricky part of the code
mod networking;
/// convenient methods to read/write/exchange data on the bus
mod accessing;
/// helpers to map slave registers to virtual memory
mod mapping;


pub use networking::Master;
pub use accessing::*;
pub use mapping::*;


use crate::{
    registers::CommandError,
    command::MAX_COMMAND,
    };
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("problem with uart bus")]
    Bus(std::io::Error),
    #[error("problem detected on slave side")]
    Slave(CommandError),
    #[error("problem detected on master side")]
    Master(&'static str),
}
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Bus(error)
    }
}


fn usize_to_message(size: usize) -> Result<u16, Error> {
    if size < MAX_COMMAND  {Ok(size as u16)}
    else {Err(Error::Master("data is longer than maximum allowed message"))}
}
