use packbytes::{FromBytes, ToBytes, ByteArray};
use tokio::io::AsyncReadExt;
// use tokio_serial::{SerialStream, SerialPort, DataBits, Parity, StopBits};
use serial2_tokio::{SerialPort, CharSize, StopBits, Parity};
use std::{
    path::Path,
    task::{Poll, Waker},
    future::poll_fn,
    collections::HashMap,
    mem::transmute,
    vec::Vec,
    ops::{Deref, DerefMut},
    time::Duration,
    };

use crate::{
    mutex::*,
    command::{Command, MAX_COMMAND, checksum, self},
    registers::{CommandError, SlaveSize, VirtualSize},
    };
use super::{Error, usize_to_message};




/** 
    artcat master async implementation
    
    all methods here are addressing the virtual memory which is shared by all slaves
*/
pub struct Master {
    /// uart RX/TX stream
    receive: BusyMutex<SerialPort>,
    transmit: BusyMutex<SerialPort>,
    /// command answers currently waited for
    pending: BusyMutex<HashMap<Token, Pending>>,
    timeout: Duration,
    
    // TODO reimplement pending with an atomic queue
}
/// internal struct holding data for receiving command's results
struct Pending {
    /// initial command header, executed is set to MAX until actual answer received
    command: Command,
    /// buffer for data reception
    buffer: &'static mut [u8],
    /// for waking up the async task waiting for the answer
    waker: Option<Waker>,
    /// result set after last reception
    result: Option<Result<u8, Error>>,
}
/// internal token type for pending commands
type Token = u16;


// TODO implement per-command timeout
impl Master {
    /// initialize a master on the given serial port file and with the given baud rate
    pub fn new(path: impl AsRef<Path>, rate: u32) -> Result<Self, std::io::Error> {
        let bus1 = SerialPort::open(path, |mut settings: serial2_tokio::Settings| {
                settings.set_raw();
                settings.set_baud_rate(rate)?;
                settings.set_char_size(CharSize::Bits8);
                settings.set_stop_bits(StopBits::One);
                settings.set_parity(Parity::Even);
                Ok(settings)
                })?;
        let bus2 = bus1.try_clone()?;
        Ok(Self {
            receive: BusyMutex::from(bus1),
            transmit: BusyMutex::from(bus2),
            pending: BusyMutex::from(HashMap::new()),
            timeout: Duration::from_millis(100),
        })
    }
    
    /**
        coroutine responsible of receving all responses from the bus
        
        it **must** be running in order to receive answers
    */
    pub async fn run(&self) -> Result<(), std::io::Error> {
        let mut bus = self.receive.try_lock().expect("run function called twice");
        let mut receive = [0u8; MAX_COMMAND];
        loop {
            const HEADER: usize = <Command as FromBytes>::Bytes::SIZE;
            // receive an amount that can be a header and its checksum
            bus.read_exact(&mut receive[.. HEADER+1]).await?;
            // loop until checksum is good to catch up new command
            while checksum(&receive[.. HEADER]) != receive[HEADER] {
                receive[.. HEADER+1].rotate_left(1);
                bus.read_exact(&mut receive[HEADER .. HEADER+1]).await?;
            }
            let header = Command::from_be_bytes(receive[.. HEADER].try_into().unwrap());
            
            let data = &mut receive[.. usize::from(header.size)];
            bus.read_exact(data).await?;
            
            let mut pending = self.pending.lock().await;
            if let Some(buffer) = pending.get_mut(&header.token) {
                if !(  buffer.command.token == header.token
                    && buffer.command.access.fixed() == header.access.fixed()
                    && buffer.command.access.topological() == header.access.topological()
                    && buffer.command.access.read() == header.access.read()
                    && (buffer.command.address == header.address 
                        || header.access.topological() 
                        && buffer.command.address.register() == header.address.register())
                    && buffer.command.size == header.size )
                {
                    buffer.result = Some(Err(Error::Master("reponse header mismatch")));
                }
                else if header.access.error() {
                    buffer.result = Some(Err(Error::Slave(CommandError::Unknown)));
                }
                else if header.checksum != checksum(data) {
                    buffer.result = Some(Err(Error::Master("data checksum mismatch")));
                }
                else {
                    buffer.buffer.copy_from_slice(data);
                    buffer.result = Some(Ok(header.executed));
                }
                
                if let Some(waker) = buffer.waker.take() {
                    waker.wake();
                }
            }
        }
    }
}


/// object allowing to send commands and wait and receive responses using master pending buffers
pub struct Topic<'m> {
    master: &'m Master,
    token: Token,
    #[allow(unused)]  // this field needs to be owned here, despite its ref is being used by Master
    buffer: PinnedBuffer<'m>,
}
/// data address on this bus
#[derive(Copy, Clone)]
pub enum Address {
    /// slave topological address (rank in bus, register address)
    Topological(u16, SlaveSize),
    /// slave fixed address (fixed address, register address)
    Fixed(u16, SlaveSize),
    /// mapped address in the virtual memory
    Virtual(VirtualSize),
}
impl<'m> Topic<'m> {
    pub async fn new(master: &'m Master, address: Address, mut buffer: PinnedBuffer<'m>) -> Result<Self, Error> {
        // reserve space in the master for the answer
        let mut pending = master.pending.lock().await;
        // reserve a free token, preferably random to increase the chance of getting one that was not used by previus communication (useful at start) and to decrease the chance of good checksum for bad packet
        let first = rand::random::<u16>();
        let token = loop {
            if let Some(token) = (0 ..= u16::try_from(pending.len()).unwrap())
                .map(|i|  i.wrapping_add(first))
                .filter(|k| ! pending.contains_key(&k))
                .next()
                {break token}
            };
        
        // set that part of the command that is not gonna change
        let mut command = Command::default();
        command.token = token;
        command.size = usize_to_message(buffer.len())?;

        match address {
            Address::Topological(slave, local) => {
                command.access.set_topological(true);
                command.address = command::Address::new(slave, local).into();
            },
            Address::Fixed(slave, local) => {
                command.access.set_fixed(true);
                command.address = command::Address::new(slave, local).into();
            },
            Address::Virtual(global) => {
                command.address = command::Address::from(global);
            },
        }
        
        pending.insert(token, Pending {
            command: command,
            // SAFETY: we will remove this reference when self is dropped, self guarantees that this buffer lives until then
            buffer: unsafe {transmute::<&mut [u8], &mut [u8]>(buffer.deref_mut())},
            waker: None,
            result: None,
            });
        Ok(Self{master, token, buffer})
    }
    /// send the current content of the buffer
    pub async fn send(&self, read: bool, write: bool, data: Option<&[u8]>) -> Result<(), Error> {
        let mut pending = self.master.pending.lock().await;
        let buffer = pending.get_mut(&self.token).unwrap();
        let data = data.unwrap_or(buffer.buffer);
        // update command for new buffer
        buffer.command.checksum = checksum(data);
        buffer.command.access.set_read(read);
        buffer.command.access.set_write(write);
        {
            let bus = self.master.transmit.lock().await;
            let header = buffer.command.to_be_bytes();
            bus.write_all(&header).await?;
            bus.write_all(&checksum(&header).to_be_bytes()).await?;
            bus.write_all(data).await?;
        }
        Ok(())
    }
    /// wait for answer to be ready in the current buffer
    pub async fn receive(&self, mut copy: Option<&mut [u8]>) -> Result<u8, Error> {
        let polling = poll_fn(|context| {
            if let Some(mut pending) = self.master.pending.try_lock() {
                let buffer = pending.get_mut(&self.token).unwrap();
                if let Some(result) = buffer.result.take() {
                    if let Some(dst) = copy.take() {
                        dst.copy_from_slice(buffer.buffer);
                    }
                    return Poll::Ready(result)
                }
                buffer.waker.replace(context.waker().clone());
            }
            // TODO check wether it is ok to return pending without changing waker in the pending task
            // nothing else to do, leave resources to the runtime
            Poll::Pending
        });
        tokio::time::timeout(self.master.timeout, polling).await
            .map_err(|_| Error::Timeout)?
    }
    /// copy the current data in the buffer, received or not, already read or not
    pub async fn get(&self, dst: &mut [u8]) {
        let pending = self.master.pending.lock().await;
        let buffer = pending.get(&self.token).unwrap();
        dst.copy_from_slice(buffer.buffer);
    }
}
impl Drop for Topic<'_> {
    fn drop(&mut self) {
        loop {
            if let Some(mut pending) = self.master.pending.try_lock() {
                pending.remove(&self.token);
                break
            }
            // nothing else to do, leave resources to the kernel
            std::thread::yield_now();
        }
    }
}



pub enum PinnedBuffer<'s> {
    Borrowed(&'s mut [u8]),
    Owned(Vec<u8>),
}
impl Deref for PinnedBuffer<'_> {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(slice) => slice,
            Self::Owned(vec) => vec.deref(),
        }
    }
}
impl DerefMut for PinnedBuffer<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Borrowed(slice) => slice,
            Self::Owned(vec) => vec.deref_mut(),
        }
    }
}


