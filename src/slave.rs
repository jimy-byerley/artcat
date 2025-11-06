use core::ops::BitXor;
use packbytes::{FromBytes, ToBytes, ByteArray};
use embedded_io_async::{Read, Write, ReadExactError};

use crate::{
    mutex::*,
    command::*,
    registers::{Register, self},
    };



pub struct Slave<B, const MEM: usize> {
    buffer: BusyMutex<[u8; MEM]>,
    control: BusyMutex<SlaveControl<B>>,
}
struct SlaveControl<B> {
    bus: B,
    mapping: heapless::Vec<registers::Mapping, 128>,
    address: u16,
    receive: [u8; MAX_COMMAND],
    send: [u8; MAX_COMMAND],
}
impl<B: Read + Write, const MEM: usize> Slave<B, MEM> {
    pub fn new(bus: B, device: registers::Device) -> Self {
        todo!()
    }
    pub async fn get<T: FromBytes>(&self, register: Register<T>) -> T {
        let mut dst = T::Bytes::zeroed();
        let src = self.buffer.lock().await;
        dst.as_mut().copy_from_slice(&src[usize::try_from(register.address).unwrap() ..][.. T::Bytes::SIZE]);
        T::from_be_bytes(dst)
    }
    pub async fn set<T: ToBytes>(&self, register: Register<T>, value: T) {
        let src = value.to_be_bytes();
        let mut dst = self.buffer.lock().await;
        dst[usize::try_from(register.address).unwrap() ..][.. T::Bytes::SIZE].copy_from_slice(src.as_ref());
    }
    pub async fn run(&self) {
        let Some(mut control) = self.control.try_lock() 
            else {return};
        loop {
            if control.receive_command(self).await.is_err() {
                let count = self.get(registers::LOSS).await;
                self.set(registers::LOSS, count.saturating_add(1)).await;
            }
        }
    }
}
impl<B: Read + Write> SlaveControl<B> {
    async fn receive_command<const MEM: usize>(&mut self, slave: &Slave<B, MEM>) -> Result<(), B::Error> {
        let mut header = <Command as FromBytes>::Bytes::zeroed();
        let mut checksum = <Checksum as FromBytes>::Bytes::zeroed();
        // read header
        no_eof(self.bus.read_exact(&mut header).await)?;
        no_eof(self.bus.read_exact(&mut checksum).await)?;
        
        let checksum = Checksum::from_be_bytes(checksum);
        
        if checksum.header != header.iter().cloned().reduce(BitXor::bitxor).unwrap() {
            let count = slave.get(registers::LOSS).await;
            slave.set(registers::LOSS, count.saturating_add(1)).await;
            self.bus.write(&header.to_be_bytes()).await?;
            return Ok(());
        }
        
        let mut header = Command::from_be_bytes(header);
        let mut local = SlaveRegister::from(header.address);
        
        // check command consistency
        if usize::from(header.size) > MAX_COMMAND {
            if slave.get(registers::ERROR).await == registers::CommandError::None {
                slave.set(registers::ERROR, registers::CommandError::InvalidAccess).await;
            }
            self.bus.write(&header.to_be_bytes()).await?;
            return Ok(());
        }
        if header.access.fixed() && header.access.topological() {
            if slave.get(registers::ERROR).await == registers::CommandError::None {
                slave.set(registers::ERROR, registers::CommandError::InvalidCommand).await;
            }
            self.bus.write(&header.to_be_bytes()).await?;
            return Ok(());
        }
        
        // logic for topologial addresses
        if header.access.topological() {
            local.set_slave(local.slave().wrapping_sub(1));
            header.address = local.into();
        }
        // direct access to slave buffer
        if header.access.fixed() && local.slave() == self.address
        || header.access.topological() && local.slave() == 0 
        {
            // exchange requested chunk of data
            // mark the command executed
            header.executed += 1;
            self.bus.write(&header.to_be_bytes()).await?;
            self.receive_slave_data(slave, header).await?;
        }
        // access to bus virtual memory
        else if !header.access.fixed() && !header.access.topological() {
            // exchange data according to local mapping
            header.executed += 1;
            self.bus.write(&header.to_be_bytes()).await?;
            self.receive_virtual_data(slave, header).await?;
        }
        // any other command
        else {
            // simply pass data
            self.bus.write(&header.to_be_bytes()).await?;
            
            no_eof(self.bus.read_exact(&mut self.receive[.. usize::from(header.size)]).await)?;
            self.bus.write(&self.receive[.. usize::from(header.size)]).await?;
        }
        Ok(())
    }
    
    async fn receive_slave_data<const MEM: usize>(&mut self, slave: &Slave<B, MEM>, header: Command) -> Result<(), B::Error> {
        let data = &mut self.receive[.. usize::from(header.size)];
        no_eof(self.bus.read_exact(data).await)?;
        let local = SlaveRegister::from(header.address);
        
        if header.access.read() {
            self.send[.. data.len()].copy_from_slice(
                &slave.buffer.lock().await
                [usize::from(local.register()) ..][.. data.len()]
                );
            self.bus.write(data).await?;
        }
        else {
            self.bus.write(data).await?;
        }
        if header.access.write() {
            slave.buffer.lock().await
                [usize::from(local.register()) ..][.. data.len()]
                .copy_from_slice(data);
        }
        
        // special actions for special registers
        let address = local.register();
        if u32::from(address) == registers::ADDRESS.address {
            self.address = slave.get(registers::ADDRESS).await;
        }
        else if u32::from(address) == registers::MAPPING.address {
            let table = slave.get(registers::MAPPING).await;
//             self.mapping.clear();
//             self.mapping.extend_from_slice(&table.map[.. usize::from(table.size)]);
//             self.mapping.sort_by_key(|item| item.virtual_start);
        }
        Ok(())
    }
    
    async fn receive_virtual_data<const MEM: usize>(&mut self, slave: &Slave<B, MEM>, header: Command) -> Result<(), B::Error> {
        todo!("iterate over mappings inside the requested area and exchange with registers")
    }
}

/// simple helper unwrapping eof because they should not appear in bare metal uart, at least in esp32 hal
fn no_eof<T, E>(result: Result<T, ReadExactError<E>>) -> Result<T, E> {
    result.map_err(|e| match e {
        ReadExactError::UnexpectedEof => panic!(),
        ReadExactError::Other(io) => io,
        })
}
