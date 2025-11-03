use core::marker::PhantomData;
use packbytes::{FromBytes, ToBytes};


#[derive(PartialEq, Hash)]
pub struct Register<T> {
    pub address: u16,
    ty: PhantomData<T>,
}
impl<T> Register<T> {
    pub const fn new(_address: u16) -> Self {
        Self{address: _address, ty: PhantomData}
    }
}

/// slave fixed address
pub const ADDRESS: Register<u16> = Register::new(0x0);
/// first communication error raise by slave, write to to 0 to reset
pub const ERROR: Register<CommandError> = Register::new(0x2);
/// protocol version
pub const VERSION: Register<u8> = Register::new(0x3);
//         /// error message, must be a UTF8 zero-terminated string
//         pub const message: Register<[u8; 32]> = Register::new(0x4);
/// slave standard informations
pub const DEVICE: Register::<Device> = Register::new(0x20);
/// slave clock value when reading
pub const CLOCK: Register::<u64> = Register::new(0x100);
/// mapping between registers and virtual memory
pub const MAPPING: Register::<MappingTable> = Register::new(0x200);


/// slave standard informations
#[derive(Copy, Clone, FromBytes, ToBytes)]
pub struct Device {
    /// model name, must be a UTF8 zero-terminated string
    model: [u8; 32],
    /// version of the slave's hardware, arbitrary format, must be a UTF8 zero-terminated string
    hardware_version: [u8; 32],
    /// version of the slave's software, arbitrary format, must be a UTF8 zero-terminated string
    software_version: [u8; 32],
}
#[derive(Copy, Clone, FromBytes, ToBytes)]
pub struct MappingTable {
    size: u8,
    map: [Mapping; 128],
}
#[derive(Copy, Clone, FromBytes, ToBytes)]
pub struct Mapping {
    mapped_start: u32,
    slave_start: u16,
    size: u16,
}
#[repr(u8)]
#[derive(Copy, Clone, Default)]
pub enum CommandError {
    #[default]
    None = 0,
    Unknown = 255,
    
    /// received command doesn't exist
    InvalidCommand = 1,
    /// requested read/write is not allowed for given register
    InvalidAccess = 2,
    /// requested register doesn't exist
    InvalidRegister = 3,
    /// register set in mapping doesn't exist
    InvalidMapping = 4,
}
