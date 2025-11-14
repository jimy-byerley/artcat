use packbytes::{FromBytes, ToBytes, ByteArray};
use std::{
    marker::PhantomData,
    collections::HashMap,
    vec::Vec,
    };
use crate::registers::{self, Register};


#[derive(Clone, Debug)]
pub struct Mapping {
    registers: HashMap<u16, Vec<registers::Mapping>>,
    end: u32,
}
impl Mapping {
    pub fn new() -> Self {
        Self {
            registers: HashMap::new(),
            end: 0,
        }
    }
    pub fn buffer<T: FromBytes + ToBytes>(&mut self) -> Option<BufferMapping<'_, T>> {
        let start = self.end;
        self.end = self.end.checked_add(T::Bytes::SIZE)?;
        BufferMapping {
            mapping: self,
            start,
            end: self.end,
            ty: PhantomData,
            }
    }
}
#[derive(Debug)]
pub struct BufferMapping<'m, T> {
    start: u32,
    end: u32,
    mapping: &'m mut Mapping,
    ty: PhantomData<T>,
}
impl BufferMapping<'_, U> {
    pub fn padding(self, size: u16) -> Self {
        self.end += size;
    }
    pub fn register<R: FromBytes>(self, slave: Host, register: Register<R>) -> Self {
        let start = self.end;
        assert!(end <= self.start + T::Bytes::SIZE, "mapping set is bigger than packed type");
        self.end += u32::from(register.size());
        self.mapping.registers.entry(slave).push(registers::Mapping {
            slave_start: register.address(), 
            virtual_start: start,
            size: register.size(),
            });
        self
    }
    pub fn build(self) -> Register<T> {
        assert_eq!(self.end, self.start + T::Bytes::SIZE, "mapping set has different size than packed type");
        Buffer {
            address: address,
            data: PhantomData,
        }
    }
}

