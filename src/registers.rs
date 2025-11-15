use core::marker::PhantomData;
use packbytes::{FromBytes, ToBytes, ByteArray};


#[derive(PartialEq, Hash)]
pub struct Register<T, Size=u16> {
    addr: Size,
    ty: PhantomData<T>,
}
impl<T, Size:Copy> Register<T, Size> {
    pub const fn new(address: Size) -> Self {
        Self{addr: address, ty: PhantomData}
    }
    pub const fn address(&self) -> Size {self.addr}
}
impl<T: FromBytes, S> Register<T, S> {
    pub const fn size(&self) -> u16 {T::Bytes::SIZE as u16}
}
impl<T, S:Copy> Clone for Register<T, S> {
    fn clone(&self) -> Self {
        Self::new(self.address())
    }
}

pub type SlaveRegister<T> = Register<T, u16>;
pub type VirtualRegister<T> = Register<T, u32>;



/// slave fixed address
pub const ADDRESS: Register<u16> = Register::new(0x0);
/// first communication error raise by slave, write to 0 to reset
pub const ERROR: Register<CommandError> = Register::new(0x2);
/// count the number of loss sequences detected since last reset, write to 0 to reset
pub const LOSS: Register<u16> = Register::new(0x3);
/// protocol version
pub const VERSION: Register<u8> = Register::new(0x5);
/// slave standard informations
pub const DEVICE: Register::<Device> = Register::new(0x20);
/// slave clock value when reading
pub const CLOCK: Register::<u64> = Register::new(0x100);
/// mapping between registers and virtual memory
pub const MAPPING: Register::<MappingTable> = Register::new(0x200);


/// slave standard informations
#[derive(Clone, FromBytes, ToBytes, Debug)]
pub struct Device {
    /// model name
    pub model: StringArray,
    /// version of the slave's hardware
    pub hardware_version: StringArray,
    /// version of the slave's software
    pub software_version: StringArray,
}

#[derive(Clone, FromBytes, ToBytes, Debug)]
pub struct MappingTable {
    pub size: u8,
    pub map: [Mapping; 128],
}

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

use bilge::prelude::*;
use crate::pack_enum;
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
    /// requested register doesn't exist
    InvalidRegister = 3,
    /// register set in mapping doesn't exist
    InvalidMapping = 4,
}
pack_enum!(CommandError);


#[derive(Clone, Debug, Default, FromBytes, ToBytes)]
pub struct StringArray {
    pub size: u16,
    pub buffer: [u8; 30],
}
impl TryFrom<&str> for StringArray {
    type Error = &'static str;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let value = value.as_bytes();
        let size = u16::try_from(value.len()) .map_err(|_|  "input string exceeds maximum size")?;
        let mut dst = Self {size, .. Default::default()};
        if dst.buffer.len() >= 32
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
