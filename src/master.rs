use thiserror::Error;
use log::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt, AsyncRead, AsyncWrite};
// use tokio_serial::{SerialStream, SerialPort, DataBits, Parity, StopBits};
use serial2_tokio::{SerialPort, CharSize, StopBits, Parity};
use packbytes::{FromBytes, ToBytes, ByteArray};
use std::{
    path::Path,
    task::{Poll, Waker},
    future::poll_fn,
    collections::HashMap,
    mem::transmute,
    marker::Unpin,
    boxed::Box,
    println, dbg,
    };

use crate::{
    mutex::*,
    command::Command,
    registers::{Register, CommandError},
    };


#[derive(Copy, Clone)]
pub enum Address {
    Topological(u16, u16),
    Fixed(u16, u16),
    Virtual(u32),
}
impl Address {
    pub fn host(self) -> Host {
        match self {
            Address::Topological(slave, _) => Host::Topological(slave),
            Address::Fixed(slave, _) => Host::Fixed(slave),
            Address::Virtual(_) => Host::Virtual,
        }
    }
}


#[derive(Copy, Clone)]
pub enum Host {
    Topological(u16),
    Fixed(u16),
    Virtual,
}
impl Host {
    pub fn at(self, memory: u32) -> Address {
        match self {
            Host::Topological(slave) => Address::Topological(slave, memory.try_into().expect("register address doesn't fit in u16")),
            Host::Fixed(slave) => Address::Fixed(slave, memory.try_into().expect("register address doesn't fit in u16")),
            Host::Virtual => Address::Virtual(memory),
        }
    }
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("problem with uart bus")]
    Bus(std::io::Error),
    #[error("problem detected on slave side")]
    Slave(CommandError),
    #[error("problem detected on master side")]
    Master(&'static str),
}
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Bus(error)
    }
}

type ArtcatResult<T> = Result<Answer<T>, Error>;


/// artcat master implementation
pub struct Master {
    /// uart RX/TX stream
    receive: BusyMutex<SerialPort>,
    transmit: BusyMutex<SerialPort>,
    /// command answers currently waited for
    pending: BusyMutex<HashMap<Token, Pending>>,
    
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
// TODO implement loss recovery
impl Master {
    pub fn new(path: impl AsRef<Path>, rate: u32) -> Result<Self, std::io::Error> {
        let mut bus1 = SerialPort::open(path, |mut settings: serial2_tokio::Settings| {
                settings.set_raw();
                settings.set_baud_rate(rate)?;
                settings.set_char_size(CharSize::Bits8);
                settings.set_stop_bits(StopBits::Two);
                settings.set_parity(Parity::Even);
                Ok(settings)
                })?;
        let bus2 = bus1.try_clone()?;
        Ok(Self {
            receive: BusyMutex::from(bus1),
            transmit: BusyMutex::from(bus2),
            pending: BusyMutex::from(HashMap::new()),
        })
    }
}
impl Master {
    pub async fn read_bytes<'d>(&self, address: Address, data: &'d mut [u8]) -> ArtcatResult<&'d mut [u8]> {
        self.command(Command::new(address, true, false, usize_to_message(data.len())?), data).await
    }
    pub async fn write_bytes(&self, address: Address, data: &mut [u8]) -> ArtcatResult<()> {
        self.command(Command::new(address, false, true, usize_to_message(data.len())?), data).await 
            .map(|a| Answer {data: (), executed: a.executed})
    }
    pub async fn exchange_bytes<'d>(&self, address: Address, data: &'d mut [u8]) -> ArtcatResult<&'d mut [u8]> {
        self.command(Command::new(address, true, true, usize_to_message(data.len())?), data).await
    }
    
    pub async fn read<T: FromBytes>(&self, host: Host, register: Register<T>) -> ArtcatResult<T> {
//         let answers = self.read_bytes(host.at(register.address), T::Bytes::zeroed().as_mut()).await?.answers;
//             .map(|buffer| T::from_be_bytes(buffer.try_into().unwrap())) )
        let mut buffer = T::Bytes::zeroed();
        let executed = self.read_bytes(host.at(register.address), buffer.as_mut()).await?.executed;
        Ok(Answer{
            data: T::from_be_bytes(buffer),
            executed,
            })
    }
    pub async fn write<T: ToBytes>(&self, host: Host, register: Register<T>, value: T) -> ArtcatResult<()> {
        let executed = self.write_bytes(host.at(register.address), value.to_be_bytes().as_mut()).await?.executed;
        Ok(Answer{
            data: (),
            executed,
            })
    }
    pub async fn exchange<C: ByteArray, T: ToBytes<Bytes=C> + FromBytes<Bytes=C>>(&self, host: Host, register: Register<T>, value: T) -> ArtcatResult<T> {
//         Ok( self.write_bytes(host.at(register.address), value.to_be_bytes().as_mut()).await
//             .map(|buffer| T::from_be_bytes(buffer.try_into().unwrap())) )
        let mut buffer = value.to_be_bytes();
        let executed = self.write_bytes(host.at(register.address), buffer.as_mut()).await?.executed;
        Ok(Answer{
            data: T::from_be_bytes(buffer),
            executed,
            })
    }
    
    
    async fn command<'d>(&self, command: Command, data: &'d mut [u8]) -> ArtcatResult<&'d mut [u8]> {
        debug!("start command {:#?}", command);
        let topic = Topic::new(self, command, unsafe{ transmute::<&mut [u8], &'static mut [u8]>(data) }).await?;
        topic.send().await?;
        let executed = topic.receive().await?;
        Ok(Answer {
            data: data,
            executed,
        })
    }
    
    pub async fn run(&self) -> Result<(), std::io::Error> {
        let mut header = <Command as FromBytes>::Bytes::zeroed();
        let mut receive = [0u8; MAX_COMMAND];
        loop {
            let (header, data) = {
                let mut bus = self.receive.lock().await;
                bus.read_exact(&mut header).await?;
                let header = Command::from_be_bytes(header);
                debug!("header {:#?}", header);
                let data = &mut receive[.. usize::from(header.size)];
                bus.read_exact(data).await?;
                (header, data)
            };
            
            let mut pending = self.pending.lock().await;
            if let Some(buffer) = pending.get_mut(&header.token) {
                if !(  buffer.command.token == header.token
                    && buffer.command.access == header.access
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


impl Command {
    fn new(address: Address, read: bool, write: bool, size: u16) -> Self {
        let mut command = Self::default();
        command.access.set_read(read);
        command.access.set_write(write);
        command.size = size;
        command.executed = 0;
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
        command
    }
}

struct Topic<'m> {
    master: &'m Master,
    token: Token,
}
impl<'m> Topic<'m> {
    async fn new(master: &'m Master, command: Command, data: &'static mut [u8]) -> Result<Self, Error> {
        let mut pending = master.pending.lock().await;
        let token = loop {
            if let Some(token) = (0 ..= u16::try_from(pending.len()).unwrap()) 
                .filter(|k| ! pending.contains_key(&k))
                .next()
                {break token}
            };
        let mut command = command;
        command.token = token;
        pending.insert(token, Pending {
            command: command,
            buffer: data,
            waker: None,
            result: None,
            });
        Ok(Self{master, token})
    }
    /// send the current content of the buffer
    async fn send(&self) -> Result<(), Error> {
        let mut pending = self.master.pending.lock().await;
        let buffer = pending.get_mut(&self.token).unwrap();
        {
            let mut bus = self.master.transmit.lock().await;
            debug!("send {:#?}", buffer.command);
            bus.write(&buffer.command.to_be_bytes()).await?;
            bus.write(buffer.buffer).await?;
        }
        Ok(())
    }
    /// wait for answer to be ready in the current buffer
    async fn receive(&self) -> Result<u8, Error> {
        poll_fn(|context| {
            if let Some(mut pending) = self.master.pending.try_lock() {
                let buffer = pending.get_mut(&self.token).unwrap();
                if let Some(result) = buffer.result.take() {
                    return Poll::Ready(result)
                }
                buffer.waker.replace(context.waker().clone());
            }
            // TODO check wether it is ok to return pending without changing waker in the pending task
            // nothing else to do, leave resources to the runtime
            Poll::Pending
        }).await
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


/// received data and number of slaves who executed the command
pub struct Answer<T> {
    pub data: T,
    pub executed: u8,
}
impl<T> Answer<T> {
    pub fn any(self) -> Result<T, Error> {
        if self.executed == 0 
            {return Err(Error::Master("no slave answered"))}
        Ok(self.data)
    }
    pub fn exact(self, executed: u8) -> Result<T, Error> {
        if self.executed != executed {
            if self.executed == 0
                {return Err(Error::Master("no slave answered"))}
            else
                {return Err(Error::Master("incorrect number of answers"))}
        }
        Ok(self.data)
    }
    pub fn once(self) -> Result<T, Error>  {
        self.exact(1)
    }
}

fn usize_to_message(size: usize) -> Result<u16, Error> {
    if size < MAX_COMMAND  {Ok(size as u16)}
    else {Err(Error::Master("data is longer than maximum allowed message"))}
}
