/*!
    implement a asynchronous uartcat slave in a ` no-std`  and ` no-alloc` environment.
*/
use core::ops::{Deref, DerefMut, Range};
use packbytes::{FromBytes, ToBytes, ByteArray};
use embedded_io_async::{Read, Write, ReadExactError};
use log::*;

use crate::{
    mutex::*,
    command::*,
    registers::{SlaveRegister, self},
    };


/**
    uartcat slave async implementation for bare-metal `no-std` and `no-alloc` environment
    
    A slave owns a local data buffer of `MEM` bytes, that is shared between bus coroutine and user task using a sync mutex.
    This buffer stores communication config of the slave as well as user data the slave wants to share with the master
*/
pub struct Slave<B, const MEM: usize> {
    buffer: BusyMutex<SlaveBuffer<MEM>>,
    control: BusyMutex<SlaveControl<B>>,
}
/// buffer of `MEM` bytes data shared between slave tasks an the bus communication
pub struct SlaveBuffer<const MEM: usize> {
    buffer: [u8; MEM],
}
struct SlaveControl<B> {
    bus: B,
    mapping: heapless::Vec<registers::Mapping, 128>,
    address: u16,
    receive: [u8; MAX_COMMAND],
    send: [u8; MAX_COMMAND],
    send_header: Command,
}

// TODO: implement separated TX and RX
impl<B: Read + Write, const MEM: usize> Slave<B, MEM> {
    /// initialize the slave on the given UART bus, with the given slave identification infos
    pub fn new(bus: B, device: registers::Device) -> Self {
        assert!(MEM >= registers::USER, "buffer is too small for standard registers");
    
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
                send_header: Command::default(),
            }),
        };
        new
    }
    
    /// wait until getting access to the slave's buffer
    pub async fn lock(&self) -> BusyMutexGuard<'_, SlaveBuffer<MEM>> {self.buffer.lock().await}
    /// try to get access to the slave's buffer, immediately abort if the buffer is being used by other tasks
    pub fn try_lock(&self) -> Option<BusyMutexGuard<'_, SlaveBuffer<MEM>>> {self.buffer.try_lock()}
    
    /** 
        coroutine reacting to uartcat commands received on the bus. it is responsible of all communications with the master.
        
        It **must** run in order to communicate with the master
    */
    pub async fn run(&self) {
        let Some(mut control) = self.control.try_lock() 
            else {return};
        loop {
//             if control.receive_command(self).await.is_err() {
            if let Err(err) = control.receive_command(self).await {
                warn!("uartcat error {:?}", err);
                self.buffer.lock().await.add_loss();
            }
        }
    }
}

impl<const MEM: usize> SlaveBuffer<MEM> {
    /// get the current register's value
    pub fn get<T: FromBytes>(&self, register: SlaveRegister<T>) -> T {
        let mut dst = T::Bytes::zeroed();
        dst.as_mut().copy_from_slice(&self.buffer[usize::try_from(register.address()).unwrap() ..][.. T::Bytes::SIZE]);
        T::from_be_bytes(dst)
    }
    /// set the given register's value
    pub fn set<T: ToBytes>(&mut self, register: SlaveRegister<T>, value: T) {
        let src = value.to_be_bytes();
        self.buffer[usize::try_from(register.address()).unwrap() ..][.. T::Bytes::SIZE].copy_from_slice(src.as_ref());
    }
    /// set current command error, if not already set
    fn set_error(&mut self, error: registers::CommandError) {
        if self.get(registers::ERROR) == registers::CommandError::None {
            self.set(registers::ERROR, error);
        }
    }
    fn add_loss(&mut self) {
        let count = self.get(registers::LOSS);
        self.set(registers::LOSS, count.saturating_add(1));
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

impl<B: Read + Write> SlaveControl<B> {
    /// process one command on the bus, block until a command is found and executed
    async fn receive_command<const MEM: usize>(&mut self, slave: &Slave<B, MEM>) -> Result<(), B::Error> {
        let recv_header = self.catch_header().await?;
        let size = usize::from(recv_header.size);
        if size > MAX_COMMAND {
            return Ok(());
        }
        // receive data
        no_eof(self.bus.read_exact(&mut self.receive[..size]).await)?;
        // try to process it
        self.send_header = recv_header.clone();
        if let Err(err) = self.process_command(slave, recv_header).await {
            slave.lock().await.set_error(err);
            self.send_header.access.set_error(true);
        }
        // transmit anyway
        let header = self.send_header.to_be_bytes();
        self.bus.write_all(&header).await?;
        self.bus.write_all(&checksum(&header).to_be_bytes()).await?;
        self.bus.write_all(&self.send[.. size]).await?;
        Ok(())
    }
    /// wait until a command header is found
    async fn catch_header(&mut self) -> Result<Command, B::Error> {
        const HEADER: usize = <Command as FromBytes>::Bytes::SIZE;
        // receive an amount that can be a header and its checksum
        no_eof(self.bus.read_exact(&mut self.receive[.. HEADER+1]).await)?;
        // loop until checksum is good to catch up new command
        while checksum(&self.receive[.. HEADER]) != self.receive[HEADER] {
            self.receive[.. HEADER+1].rotate_left(1);
            no_eof(self.bus.read_exact(&mut self.receive[HEADER .. HEADER+1]).await)?;
        }
        Ok(Command::from_be_bytes(self.receive[.. HEADER].try_into().unwrap()))
    }
    /// execute a given command is this slaved is concerned
    async fn process_command<const MEM: usize>(&mut self, slave: &Slave<B, MEM>, recv_header: Command) -> Result<(), registers::CommandError> {
        let size = usize::from(recv_header.size);
        
        // check command consistency
        if recv_header.access.fixed() && recv_header.access.topological() {
            return Err(registers::CommandError::InvalidCommand);
        }
        // logic for topologial addresses
        if recv_header.access.topological() {
            let slave = recv_header.address.slave();
            self.send_header.address.set_slave(slave.wrapping_sub(1));
        }
        // direct access to slave buffer
        if recv_header.access.fixed() && recv_header.address.slave() == self.address
        || recv_header.access.topological() && recv_header.address.slave() == 0 
        {
            // check data integrity, only useful if data was expected
            if recv_header.access.write() && recv_header.checksum != checksum(&self.receive[..size]) {
                slave.buffer.lock().await.add_loss();
                return Ok(());
            }
            // exchange requested chunk of data
            // mark the command executed
            self.send_header.executed += 1;
            return self.exchange_slave(slave, recv_header).await;
        }
        // access to bus virtual memory
        else if !recv_header.access.fixed() && !recv_header.access.topological() {
            // check data integrity, only useful if data was expected
            if recv_header.access.write() && recv_header.checksum != checksum(&self.receive[..size]) {
                slave.buffer.lock().await.add_loss();
                return Ok(());
            }
            // exchange data according to local mapping
            // mark the command executed
            self.send_header.executed += 1;
            self.exchange_virtual(slave, recv_header).await;
            return Ok(());
        }
        // any other command
        else {
            // simply pass data
            self.send[..size] .copy_from_slice(&self.receive[..size]);
            return Ok(());
        }
    }
    /// exchange directly with slave buffer, executing special operations on reading and writing special registers
    async fn exchange_slave<const MEM: usize>(&mut self, slave: &Slave<B, MEM>, header: Command) -> Result<(), registers::CommandError> {
        // get memory range in slave buffer
        let size = usize::from(header.size);
        let register = header.address.register();
        
        // request specifically addressed to this slave is always locking its buffer
        {
            // lock slave's buffer only once
            let mut buffer = slave.buffer.lock().await;
            
            if usize::from(register).saturating_add(size) > buffer.len() {
                warn!("invalid size");
                return Err(registers::CommandError::InvalidRegister);
            }
            
            // read buffer before writing it
            if header.access.read() {
                self.on_read(&mut buffer, register);
                self.send[..size] .copy_from_slice(&buffer[usize::from(register) ..][.. size]);
                self.send_header.checksum = checksum(&self.send[..size]);
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
        // lower bound os the first that ends in the requested area
        let start = bisect_slice(&self.mapping, |item| item.virtual_start + u32::from(item.size) > u32::from(header.address));
        // upper bound is the first that starts after requested area
        let stop = bisect_slice(&self.mapping[start ..], |item| item.virtual_start > u32::from(header.address) + u32::from(header.size));
        
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
                self.send_header.checksum = checksum(&self.send[..size]);
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
        if address == registers::ADDRESS.address() {
            self.address = buffer.get(registers::ADDRESS);
        }
        else if address == registers::MAPPING.address() {
            let table = buffer.get(registers::MAPPING);
            self.mapping.clear();
            self.mapping.extend(
                table.map[.. usize::from(table.size)]
                .iter().cloned().filter(|mapping|  mapping.size != 0)
                );
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


/// simple helper unwrapping eof because they should not appear in bare metal uart, at least in esp32 hal
fn no_eof<T, E>(result: Result<T, ReadExactError<E>>) -> Result<T, E> {
    result.map_err(|e| match e {
        ReadExactError::UnexpectedEof => panic!("end of file is not supposed to happend on peripheral"),
        ReadExactError::Other(io) => io,
        })
}
/// bisect a slice to find the first `i` at which `threshold(slice[i])` is True
fn bisect_slice<T>(slice: &[T], threshold: impl Fn(&T) -> bool) -> usize {
    let (mut start, mut end) = (0, slice.len());
    while start < end {
        let mid = (start + end)/2;
        if threshold(&slice[mid]) {
            end = mid;
        }
        else {
            start = mid+1;
        }
    }
    end
}
/** 
    return matching ranges in frame data buffer and slave buffer according to the given mapping
    
    result is a couple (in frame, in slave)
*/
fn map_frame_slave(mapped: registers::Mapping, frame: Command) -> Option<(Range<usize>, Range<usize>)> {
    let address = u32::from(frame.address);
    let virtual_range = Range {
        start: mapped.virtual_start,
        end: mapped.virtual_start + u32::from(mapped.size),
        };
    let requested_range = Range {
        start: address,
        end: address + u32::from(frame.size),
        };
    let intersection = Range {
        start: virtual_range.start.max(requested_range.start),
        end: virtual_range.end.min(requested_range.end),
        };
    if intersection.end <= intersection.start
        {return None}
    
    Some((
        Range {
            start: usize::try_from(intersection.start - address).unwrap(),
            end: usize::try_from(intersection.end - address).unwrap(),
        },
        Range {
            start: usize::try_from(intersection.start - mapped.virtual_start).unwrap() + usize::from(mapped.slave_start),
            end: usize::try_from(intersection.end - mapped.virtual_start).unwrap() + usize::from(mapped.slave_start),
        },
    ))
}
