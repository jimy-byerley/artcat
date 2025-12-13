use log::*;
use packbytes::{FromBytes, ByteArray};
use std::{
    marker::PhantomData,
    collections::HashMap,
    vec::Vec,
    };
use crate::registers::{self, SlaveRegister, VirtualRegister};
use super::accessing::{Host, Slave};
use super::{Error, usize_to_message};


/// helper to build a global config of slaves mappings to the common virtual memory. it follows the builder pattern
#[derive(Clone, Debug)]
pub struct Mapping {
    map: HashMap<Host, Vec<registers::Mapping>>,
    end: u32,
}
impl Mapping {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            end: 0,
        }
    }
    pub fn buffer<T: FromBytes>(&mut self) -> Result<BufferMapping<'_, T>, Error> {
        let start = self.end;
        self.end = self.end.checked_add(usize_to_message(T::Bytes::SIZE)?.into())
            .ok_or(Error::Master("no more virtual memory available"))?;
        Ok(BufferMapping {
            start,
            end: start,
            mapping: self,
            ty: PhantomData,
            })
    }
    pub fn map(&self) -> &HashMap<Host, Vec<registers::Mapping>> {
        &self.map
    }
    pub async fn configure(&self, slave: &Slave<'_>) -> Result<(), Error> {
        let mut mapping = registers::MappingTable::default();
        if let Some(table) = self.map.get(&slave.address()) {
            if table.len() > mapping.map.len() {
                return Err(Error::Master("too many items in mapping table"));
            }
            mapping.size = u8::try_from(table.len()).unwrap();
            for (i, item) in table.iter().enumerate() {
                mapping.map[i] = *item;
            }
        }
        slave.write(registers::MAPPING, mapping).await?.one()
    }
}

/// helper to map multiple slave registers into a packed struct in the virtual memory. it follows the builder pattern
#[derive(Debug)]
pub struct BufferMapping<'m, T> {
    start: u32,
    end: u32,
    mapping: &'m mut Mapping,
    ty: PhantomData<T>,
}
impl<T: FromBytes> BufferMapping<'_, T> {
    pub fn padding(mut self, size: u16) -> Self {
        self.end += u32::from(size);
        self
    }
    pub fn register<R: FromBytes>(mut self, slave: Host, register: SlaveRegister<R>) -> Self {
        let start = self.end;
        self.end += u32::from(register.size());
        debug!("mapping {:?} {:#x} {}    {}", slave, register.address(), register.size(), self.end - self.start);
        assert!(self.end <= self.start + T::Bytes::SIZE as u32, "mapping set is bigger than packed type");
        let table = self.mapping.map.entry(slave).or_insert_with(Vec::new);
        table.push(registers::Mapping {
                slave_start: register.address(), 
                virtual_start: start,
                size: register.size(),
                });
        self
    }
    pub fn build(self) -> VirtualRegister<T> {
        assert_eq!(self.end, self.start + T::Bytes::SIZE as u32, "mapping set has different size than packed type");
        VirtualRegister::new(self.start)
    }
}

