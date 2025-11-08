use core::ops::{BitXor, Deref, DerefMut};
use packbytes::{FromBytes, ToBytes, ByteArray};
use embedded_io_async::{Read, Write, ReadExactError};
use log::*;

use crate::{
    mutex::*,
    command::*,
    registers::{Register, self},
    };



pub struct Slave<B, const MEM: usize> {
    buffer: BusyMutex<SlaveBuffer<MEM>>,
    control: BusyMutex<SlaveControl<B>>,
}
pub struct SlaveBuffer<const MEM: usize> {
    buffer: [u8; MEM],
}
struct SlaveControl<B> {
    bus: B,
    mapping: heapless::Vec<registers::Mapping, 128>,
    address: u16,
    receive: [u8; MAX_COMMAND],
    send: [u8; MAX_COMMAND],
}

// TODO implement loss recovery
impl<B: Read + Write, const MEM: usize> Slave<B, MEM> {
    pub fn new(bus: B, device: registers::Device) -> Self {
        let mut buffer = SlaveBuffer {buffer: [0; MEM]};
        buffer.set(registers::VERSION, 1);
        buffer.set(registers::DEVICE, device);
        
        let new = Self {
            buffer: BusyMutex::from(buffer),
            control: BusyMutex::from(SlaveControl {
                bus,
                address: 0,
                mapping: heapless::Vec::new(),
                receive: [0; MAX_COMMAND],
                send: [0; MAX_COMMAND],
            }),
        };
        new
    }
    pub async fn lock(&self) -> BusyMutexGuard<'_, SlaveBuffer<MEM>> {self.buffer.lock().await}
    pub fn try_lock(&self) -> Option<BusyMutexGuard<'_, SlaveBuffer<MEM>>> {self.buffer.try_lock()}
    
    pub async fn run(&self) {
        let Some(mut control) = self.control.try_lock() 
            else {return};
        loop {
            if control.receive_command(self).await.is_err() {
                let mut buffer = self.lock().await;
                let count = buffer.get(registers::LOSS);
                buffer.set(registers::LOSS, count.saturating_add(1));
            }
        }
    }
}
impl<B: Read + Write> SlaveControl<B> {
    async fn receive_command<const MEM: usize>(&mut self, slave: &Slave<B, MEM>) -> Result<(), B::Error> {
        let mut buff_header = <Command as FromBytes>::Bytes::zeroed();
//         let mut buff_checksum = <Checksum as FromBytes>::Bytes::zeroed();
        // read header
        debug!("waiting header");
        no_eof(self.bus.read_exact(&mut buff_header).await)?;
//         debug!("waiting checksum");
//         no_eof(self.bus.read_exact(&mut checksum).await)?;
        debug!("received header {:?}", buff_header);
        
//         let checksum = Checksum::from_be_bytes(checksum);
        
//         if checksum.header != header.iter().cloned().reduce(BitXor::bitxor).unwrap() {
//             {
//                 let mut buffer = slave.lock().await;
//                 let count = buffer.get(registers::LOSS);
//                 buffer.set(registers::LOSS, count.saturating_add(1));
//             }
//             self.bus.write(&header.to_be_bytes()).await?;
//             return Ok(());
//         }
        
        let recv_header = Command::from_be_bytes(buff_header);
        let recv_register = SlaveRegister::from(recv_header.address);
        let mut send_header = recv_header.clone();
        
        debug!("receive header {:?} {:#?}", recv_register, recv_header);
        
        // check command consistency
        if usize::from(recv_header.size) > MAX_COMMAND {
            slave.lock().await.set_error(registers::CommandError::InvalidAccess);
            self.bus.write(&send_header.to_be_bytes()).await?;
            return Ok(());
        }
        if recv_header.access.fixed() && recv_header.access.topological() {
            slave.lock().await.set_error(registers::CommandError::InvalidCommand);
            self.bus.write(&send_header.to_be_bytes()).await?;
            return Ok(());
        }
        
        // logic for topologial addresses
        if recv_header.access.topological() {
            let mut send_register = recv_register.clone();
            send_register.set_slave(recv_register.slave().wrapping_sub(1));
            send_header.address = send_register.into();
        }
        // direct access to slave buffer
        if recv_header.access.fixed() && recv_register.slave() == self.address
        || recv_header.access.topological() && recv_register.slave() == 0 
        {
            debug!("read slave buffer");
            // exchange requested chunk of data
            // mark the command executed
            send_header.executed += 1;
            self.bus.write(&send_header.to_be_bytes()).await?;
            self.transceive_slave_data(slave, recv_header).await?;
        }
        // access to bus virtual memory
        else if !recv_header.access.fixed() && !recv_header.access.topological() {
            debug!("read virtual memory");
            // exchange data according to local mapping
            send_header.executed += 1;
            self.bus.write(&send_header.to_be_bytes()).await?;
            self.transceive_virtual_data(slave, recv_header).await?;
        }
        // any other command
        else {
            debug!("ignore command");
            // simply pass data
            self.bus.write(&send_header.to_be_bytes()).await?;
            
            debug!("waiting data");
            no_eof(self.bus.read_exact(&mut self.receive[.. usize::from(recv_header.size)]).await)?;
            self.bus.write(&self.receive[.. usize::from(recv_header.size)]).await?;
        }
        Ok(())
    }
    
    async fn transceive_slave_data<const MEM: usize>(&mut self, slave: &Slave<B, MEM>, header: Command) -> Result<(), B::Error> {
        let size = usize::from(header.size);
        
        debug!("waiting data");
        no_eof(self.bus.read_exact(&mut self.receive[..size]).await)?;
        let local = SlaveRegister::from(header.address);
        
        if header.access.read() {
            let mut buffer = slave.buffer.lock().await;
            self.on_read(&mut buffer, local.register());
            self.send[..size] .copy_from_slice(&buffer[usize::from(local.register()) ..][.. size]);
        }
        else {
            self.send[..size] .copy_from_slice(&self.receive[..size]);
        }
        
        self.bus.write(&self.send[..size]).await?;
        
        if header.access.write() {
            let mut buffer = slave.buffer.lock().await;
            buffer[usize::from(local.register()) ..][.. size] .copy_from_slice(&self.receive[..size]);
            self.on_write(&mut buffer, local.register());
        }
        Ok(())
    }
    
    /// special actions when reading special registers
    fn on_read<const MEM: usize>(&mut self, buffer: &mut SlaveBuffer<MEM>, address: u16) {
    }
    
    /// special actions when writing special registers
    fn on_write<const MEM: usize>(&mut self, buffer: &mut SlaveBuffer<MEM>, address: u16) {
        let address = u32::from(address);
        if address == registers::ADDRESS.address {
            self.address = buffer.get(registers::ADDRESS);
        }
        else if address == registers::MAPPING.address {
            let table = buffer.get(registers::MAPPING);
//             self.mapping.clear();
//             self.mapping.extend_from_slice(&table.map[.. usize::from(table.size)]);
//             self.mapping.sort_by_key(|item| item.virtual_start);
        }
    }
    
    async fn transceive_virtual_data<const MEM: usize>(&mut self, slave: &Slave<B, MEM>, header: Command) -> Result<(), B::Error> {
        todo!("iterate over mappings inside the requested area and exchange with registers")
    }
}

impl<const MEM: usize> SlaveBuffer<MEM> {
    /// get the current register's value
    pub fn get<T: FromBytes>(&self, register: Register<T>) -> T {
        let mut dst = T::Bytes::zeroed();
        dst.as_mut().copy_from_slice(&self.buffer[usize::try_from(register.address).unwrap() ..][.. T::Bytes::SIZE]);
        T::from_be_bytes(dst)
    }
    /// set the given register's value
    pub fn set<T: ToBytes>(&mut self, register: Register<T>, value: T) {
        let src = value.to_be_bytes();
        self.buffer[usize::try_from(register.address).unwrap() ..][.. T::Bytes::SIZE].copy_from_slice(src.as_ref());
    }
    /// set current command error, if not already set
    fn set_error(&mut self, error: registers::CommandError) {
        if self.get(registers::ERROR) == registers::CommandError::None {
            self.set(registers::ERROR, registers::CommandError::InvalidAccess);
        }
    }
}
impl<const MEM: usize> Deref for SlaveBuffer<MEM> {
    type Target = [u8; MEM];
    fn deref(&self) -> &Self::Target {
        &self.buffer
    }
}
impl<const MEM: usize> DerefMut for SlaveBuffer<MEM> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.buffer
    }
}

/// simple helper unwrapping eof because they should not appear in bare metal uart, at least in esp32 hal
fn no_eof<T, E>(result: Result<T, ReadExactError<E>>) -> Result<T, E> {
    result.map_err(|e| match e {
        ReadExactError::UnexpectedEof => panic!(),
        ReadExactError::Other(io) => io,
        })
}
