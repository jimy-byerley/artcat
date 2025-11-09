use core::ops::{BitXor, Deref, DerefMut, Range};
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
        buffer.set(registers::LOSS, 0);
        buffer.set(registers::ADDRESS, 0);
        
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
        let register = SlaveRegister::from(recv_header.address);
        let size = usize::from(recv_header.size);
        let mut send_header = recv_header.clone();
        
        debug!("receive header {:?} {:#?}", register, recv_header);
        
        no_eof(self.bus.read_exact(&mut self.receive[..size]).await)?;
        
        // check command consistency
        if size > MAX_COMMAND {
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
            let mut send_register = register.clone();
            send_register.set_slave(register.slave().wrapping_sub(1));
            send_header.address = send_register.into();
        }
        // direct access to slave buffer
        if recv_header.access.fixed() && register.slave() == self.address
        || recv_header.access.topological() && register.slave() == 0 
        {
            debug!("access slave buffer");
            // exchange requested chunk of data
            // mark the command executed
            send_header.executed += 1;
            if self.exchange_slave(slave, recv_header).await.is_err() {
                send_header.access.set_error(true);
            }
            self.bus.write(&send_header.to_be_bytes()).await?;
            self.bus.write(&self.send[.. usize::from(recv_header.size)]).await?;
        }
        // access to bus virtual memory
        else if !recv_header.access.fixed() && !recv_header.access.topological() {
            debug!("access virtual memory");
            // exchange data according to local mapping
            // mark the command executed
            send_header.executed += 1;
            self.bus.write(&send_header.to_be_bytes()).await?;
            self.exchange_virtual(slave, recv_header).await;
            self.bus.write(&self.send[.. usize::from(recv_header.size)]).await?;
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
    /// exchange directly with slave buffer, executing special operations on reading and writing special registers
    async fn exchange_slave<const MEM: usize>(&mut self, slave: &Slave<B, MEM>, header: Command) -> Result<(), registers::CommandError> {
        // get memory range in slave buffer
        let size = usize::from(header.size);
        let register = SlaveRegister::from(header.address).register();
        
        // request specifically addressed to this slave is always locking its buffer
        {
            // lock slave's buffer only once
            let mut buffer = slave.buffer.lock().await;
            
            // read buffer before writing it
            if header.access.read() {
                self.on_read(&mut buffer, register);
                self.send[..size] .copy_from_slice(&buffer[usize::from(register) ..][.. size]);
            }
            else {
                self.send[..size] .copy_from_slice(&self.receive[..size]);
            }
            if header.access.write() {
                buffer[usize::from(register) ..][.. size] .copy_from_slice(&self.receive[..size]);
                self.on_write(&mut buffer, register);
            }
        }
        Ok(())
    }
    /// iterate over mappings inside the requested area and exchange with registers
    async fn exchange_virtual<const MEM: usize>(&mut self, slave: &Slave<B, MEM>, header: Command) {
        // get concerned mapping
        let size = usize::from(header.size);
        let start = bisect_slice(&self.mapping, |item| item.virtual_start <= header.address);
        let stop = bisect_slice(&self.mapping[start ..], |item| item.virtual_start <= header.address + u32::from(header.size));
        
        // transmit all unless altered by mapping
        self.send[..size] .copy_from_slice(&self.receive[..size]);
        
        // only lock if concerned by this frame (frames not concerning this slave at all will never lock the slave task)
        if stop > start {
            // lock slave's buffer only once
            let mut buffer = slave.buffer.lock().await;
            
            // read buffer before writing it
            if header.access.read() {
                for &mapped in &self.mapping[start .. stop] {
                    if let Some((dst, src)) = map_frame_slave(mapped, header) {
                        self.send[dst].copy_from_slice(&buffer[src]);
                    }
                }
            }
            if header.access.write() {
                for &mapped in &self.mapping[start .. stop] {
                    if let Some((src, dst)) = map_frame_slave(mapped, header) {
                        buffer[dst].copy_from_slice(&self.receive[src]);
                    }
                }
            }
        }
    }
    
    /// special actions when reading special registers
    fn on_read<const MEM: usize>(&mut self, _buffer: &mut SlaveBuffer<MEM>, _address: u16) {
        // TODO clock interrogation
    }
    
    /// special actions when writing special registers
    fn on_write<const MEM: usize>(&mut self, buffer: &mut SlaveBuffer<MEM>, address: u16) {
        let address = u32::from(address);
        if address == registers::ADDRESS.address {
            self.address = buffer.get(registers::ADDRESS);
        }
        else if address == registers::MAPPING.address {
            let table = buffer.get(registers::MAPPING);
            self.mapping.clear();
            self.mapping.extend_from_slice(&table.map[.. usize::from(table.size)]).unwrap();
            self.mapping.sort_unstable_by_key(|item| item.virtual_start);
            for mapped in &self.mapping {
                if usize::from(mapped.slave_start + mapped.size) > buffer.len()
                || usize::from(mapped.slave_start) > buffer.len()
                || u32::MAX - mapped.virtual_start < u32::from(mapped.size) {
                    buffer.set_error(registers::CommandError::InvalidMapping);
                    // TODO set the error flag in the header
                }
            }
        }
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
            self.set(registers::ERROR, error);
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
        ReadExactError::UnexpectedEof => panic!("end of file is not supposed to happend on peripheral"),
        ReadExactError::Other(io) => io,
        })
}
/// bisect a slice to find the first index at which `threshold(slice[i])` is True
fn bisect_slice<T>(slice: &[T], threshold: impl Fn(&T) -> bool) -> usize {
    let (mut start, mut end) = (0, slice.len());
    while end > start {
        let mid = start/2 + end/2;
        if threshold(&slice[mid]) {
            start = mid;
        }
        else {
            end = mid;
        }
    }
    start
}
/** 
    return matching ranges in frame data buffer and slave buffer according to the given mapping
    
    result is a couple (in frame, in slave)
*/
fn map_frame_slave(mapped: registers::Mapping, frame: Command) -> Option<(Range<usize>, Range<usize>)> {
    let virtual_range = Range {
        start: mapped.virtual_start,
        end: mapped.virtual_start + u32::from(mapped.size),
        };
    let requested_range = Range {
        start: frame.address,
        end: frame.address + u32::from(frame.size),
        };
    let intersection = Range {
        start: virtual_range.start.max(requested_range.start),
        end: virtual_range.end.min(requested_range.end),
        };
    if intersection.end <= intersection.start
        {return None}
    
    Some((
        Range {
            start: usize::try_from(intersection.start - frame.address).unwrap(),
            end: usize::try_from(intersection.end - frame.address).unwrap(),
        },
        Range {
            start: usize::try_from(intersection.start - mapped.virtual_start).unwrap() + usize::from(mapped.slave_start),
            end: usize::try_from(intersection.end - mapped.virtual_start).unwrap() + usize::from(mapped.slave_start),
        },
    ))
}
