use bilge::prelude::*;
use packbytes::{FromBytes, ToBytes};

use crate::pack_bilge;


pub const MAX_COMMAND: usize = 4096;

/// memory bus command header
#[derive(Copy, Clone, FromBytes, ToBytes, Debug, Default)]
pub struct Command {
    /// identifier of command
    pub token: u16,
    /// type of memory access
    pub access: Access,
    /// counte the number of times this command has been executed by consecutive slaves
    pub executed: u8,
    /// address, its value depends on whether accessing a particular slave or the bus virtual memory
    pub address: Address,
    /// number of bytes to read/write, following this header
    pub size: u16,
    /// checksum of data
    pub checksum: u8,
}

/// type of memory access
#[bitsize(8)]
#[derive(Copy, Clone, FromBits, DebugBits, PartialEq, Default)]
pub struct Access {
    /// want to read memory
    pub read: bool,
    /// want to write memory, can be enabled along read
    pub write: bool,
    /** which memory to address
        - if False, the bus virtual memory is addressed, all slaves mixed, and a 32bit address is expected
        - if True, an individual slave's registers are addresses, the 32 bit addres concatenates 16bit address of slave and 16bit address of register in this slave
    */
    pub fixed: bool,
    /// if set, the slave address is topological
    pub topological: bool,
    _reserved: u3,
    /// set to True for a command that could not be executed, the error code is instantly set in register `error`
    pub error: bool,
}
pack_bilge!(Access);

#[bitsize(32)]
#[derive(Copy, Clone, FromBits, DebugBits, PartialEq, Default)]
pub struct Address {
    /// slave we are adressing the request to
    pub slave: u16,
    /// register we want to access
    pub register: u16,
}
pack_bilge!(Address);

/// checksum method used for command header and data
pub fn checksum(slice: &[u8]) -> u8 {
    let initial = 0b010110111; // standard neutral value of checksum
    slice.iter().cloned().fold(initial, |a, b|  a.wrapping_add(b)<<1)
}
