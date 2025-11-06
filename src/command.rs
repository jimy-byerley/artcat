use bilge::prelude::*;
use packbytes::{FromBytes, ToBytes};

use crate::pack_bilge;


pub const MAX_COMMAND: usize = 1024;

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
    pub address: u32,
    /// number of bytes to read/write, following this header
    pub size: u16,
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
pub struct SlaveRegister {
    /// slave we are adressing the request to
    pub slave: u16,
    /// register we want to access
    pub register: u16,
}
pack_bilge!(SlaveRegister);

/// checksums for command
#[derive(Copy, Clone, FromBytes, ToBytes, Debug, Default)]
pub struct Checksum {
    /// bitwise xor of the header
    pub header: u8,
    /// bitwise xor of the data
    pub data: u8,
}
