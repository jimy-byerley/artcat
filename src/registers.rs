/*!
    define standard arcat registers
    
    each standard is described by a serializable data type and a constant of type [SlaveRegister] defining its standard position in slaves' memory.
*/

use core::marker::PhantomData;
use packbytes::{FromBytes, ToBytes, ByteArray};
use bilge::prelude::*;
use crate::pack_enum;


/**
    a register is a typed pointer in bus memory. 
    
    it only hols the memory address of the starting byte of the referened value, hence can be created, copied or destroyed at no cost
    
    depending on the target memory, address size can vary. See [SlaveRegister]  and [VirtualRegister]
*/
#[derive(PartialEq, Hash)]
pub struct Register<T, A> {
    addr: A,
    ty: PhantomData<T>,
}
impl<T, A:Copy> Register<T, A> {
    /// create a register from its starting byte
    pub const fn new(address: A) -> Self {
        Self{addr: address, ty: PhantomData}
    }
    /// starting byte in memory
    pub const fn address(&self) -> A {self.addr}
}
impl<T: FromBytes, A> Register<T, A> {
    pub const fn size(&self) -> SlaveSize {T::Bytes::SIZE as SlaveSize}
}
impl<T, A:Copy> Clone for Register<T, A> {
    fn clone(&self) -> Self {
        Self::new(self.address())
    }
}
impl<T, A:Copy> Copy for Register<T, A> {}


/// integer used for addressing slave memory
pub type SlaveSize = u16;
/// integer used for addressing virtual memory
pub type VirtualSize = u32;

/// register in slave's memory, which is using 16bit addresses
pub type SlaveRegister<T> = Register<T, SlaveSize>;
/// register in virtual memory, which is using 32bit addresses
pub type VirtualRegister<T> = Register<T, VirtualSize>;



/// slave fixed address
pub const ADDRESS: SlaveRegister<SlaveSize> = Register::new(0x0);
/// first communication error raise by slave, write to 0 to reset
pub const ERROR: SlaveRegister<CommandError> = Register::new(0x2);
/// count the number of loss sequences detected since last reset, write to 0 to reset
pub const LOSS: SlaveRegister<u16> = Register::new(0x3);
/// protocol version
pub const VERSION: SlaveRegister<u8> = Register::new(0x5);
/// slave standard informations
pub const DEVICE: SlaveRegister<Device> = Register::new(0x20);
/// slave clock value when reading
pub const CLOCK: SlaveRegister<u64> = Register::new(0x86);
/// mapping between registers and virtual memory
pub const MAPPING: SlaveRegister<MappingTable> = Register::new(0xff);

/// end of standard mendatory section of slave buffer
pub const USER: usize = 0x500;


/// slave standard informations
#[derive(Clone, FromBytes, ToBytes, Debug)]
pub struct Device {
    /// model name
    pub model: StringArray,
    /// version of the slave's hardware
    pub hardware_version: StringArray,
    /// version of the slave's software
    pub software_version: StringArray,
    /// serial number of this specific hardware item
    pub serial: StringArray,
}
/// slave config for mapping between slave and virtual memory
#[derive(Clone, FromBytes, ToBytes, Debug)]
pub struct MappingTable {
    pub size: u8,
    pub map: [Mapping; 128],
}
/// setting for mapping a range of memory between slave and virtual memory
#[derive(Copy, Clone, Default, FromBytes, ToBytes, Debug)]
pub struct Mapping {
    pub virtual_start: u32,
    pub slave_start: u16,
    pub size: u16,
}
impl Default for MappingTable {
    fn default() -> Self {
        Self {
            size: 0,
            map: [Default::default(); 128],
            }
    }
}
impl MappingTable {
    pub fn from_iter(iterable: impl IntoIterator<Item=Mapping>) -> Result<Self, &'static str> {
        let mut table = Self::default();
        for (i, item) in iterable.into_iter().enumerate() {
            if i >= table.map.len() {
                return Err("too many items for table");
            }
            table.map[i] = item;
            table.size = u8::try_from(i).unwrap();
        }
        Ok(table)
    }
}

/// error code set after an refused command
#[bitsize(8)]
#[derive(Copy, Clone, Default, FromBits, Debug, PartialEq)]
pub enum CommandError {
    #[default]
    None = 0,
    #[fallback]
    Unknown = 255,
    
    /// received command doesn't exist
    InvalidCommand = 1,
    /// requested read/write is not allowed for given register
    InvalidAccess = 2,
    /// data size is too big for slave
    InvalidSize = 3,
    /// requested register doesn't exist
    InvalidRegister = 4,
    /// register set in mapping doesn't exist
    InvalidMapping = 5,
}
pack_enum!(CommandError);

/// register format for strings
#[derive(Clone, Debug, Default, FromBytes, ToBytes)]
pub struct StringArray {
    pub size: u8,
    pub buffer: [u8; 31],
}
impl TryFrom<&str> for StringArray {
    type Error = &'static str;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let value = value.as_bytes();
        let size = u8::try_from(value.len()) .map_err(|_|  "input string exceeds maximum size")?;
        let mut dst = Self {size, .. Default::default()};
        if value.len() > dst.buffer.len()
            {return Err("input string too long");}
        dst.buffer[..value.len()] .copy_from_slice(value);
        Ok(dst)
    }
}
impl StringArray {
    pub fn as_str(&self) -> Result<&'_ str, core::str::Utf8Error> {
        str::from_utf8(&self.buffer[.. usize::from(self.size)])
    }
}
