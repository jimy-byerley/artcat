use tokio::io::{AsyncReadExt, AsyncWriteExt, AsyncRead, AsyncWrite};
use tokio_serial::{SerialStream, SerialPortBuilder};
use packbytes::{FromBytes, ToBytes, ByteArray};
use std::{
    task::{Poll, Waker},
    future::poll_fn,
    time::Duration,
    borrow::Cow,
    collections::HashMap,
    mem::transmute,
    marker::Unpin,
    };

use crate::{
    mutex::*,
    command::*,
    registers::{Register, CommandError},
    };


#[derive(Copy, Clone)]
pub enum Host {
    Topological(u16),
    Fixed(u16),
    Virtual,
}
#[derive(Copy, Clone)]
pub enum Address {
    Topological(u16, u16),
    Fixed(u16, u16),
    Virtual(u32),
}
#[derive(Copy, Clone)]
pub enum ArtcatError {
    Bus(&'static str),
    Slave(CommandError),
    Master(&'static str),
}


/// artcat master implementation
pub struct Master<B> {
    /// uart RX/TX stream
    bus: BusyMutex<B>,
    /// command answers currently waited for
    pending: BusyMutex<HashMap<Token, Pending>>,
}
/// hold data for receiving command's results
struct Pending {
    /// initial command header, executed is set to MAX until actual answer received
    command: Command,
    /// buffer for data reception
    buffer: &'static mut [u8],
    /// for waking up the async task waiting for the answer
    waker: Option<Waker>,
    /// result set after last reception
    result: Option<Result<u8, ArtcatError>>,
}
/// internal token type for pending commands
type Token = u16;


impl<B: AsyncRead + AsyncWrite + Unpin> Master<B> {
//     pub fn new<'a>(path: impl Into<Cow<'a, str>>, rate: u32, timeout: Duration) -> Self {
//         todo!()
//     }
//     pub fn from_filename(path: impl Into<Cow<'a, str>>, rate: u32, timeout: Duration) -> Self {
//         td
//     }
    pub async fn read_bytes<'d>(&self, address: Address, data: &'d mut [u8]) -> ArtcatResult<&'d mut [u8]> {
        let len = data.len().try_into().expect("data is longer than what u16 can address");
        self.command(Command::new(address, true, false, len), data).await
    }
    pub async fn write_bytes(&self, address: Address, data: &mut [u8]) -> ArtcatResult<()> {
        let len = data.len().try_into().expect("data is longer than what u16 can address");
        self.command(Command::new(address, false, true, len), data).await .map(|_| ())
    }
    pub async fn exchange_bytes<'d>(&self, address: Address, data: &'d mut [u8]) -> ArtcatResult<&'d mut [u8]> {
        let len = data.len().try_into().expect("data is longer than what u16 can address");
        self.command(Command::new(address, true, true, len), data).await
    }
    
    pub async fn read<T: FromBytes>(&self, host: Host, register: Register<T>) -> ArtcatResult<T> {todo!()}
    pub async fn write<T: ToBytes>(&self, host: Host, register: Register<T>, value: T) -> ArtcatResult<()> {todo!()}
    pub async fn exchange<T: ToBytes + FromBytes>(&self, host: Host, register: Register<T>, value: T) -> ArtcatResult<T> {todo!()}
    
    
    async fn command<'d>(&self, command: Command, data: &'d mut [u8]) -> ArtcatResult<&'d mut [u8]> {
        // TODO unregister command from pending if this future is canceled
        let topic = Topic::new(self, command, unsafe{ transmute::<&mut [u8], &'static mut [u8]>(data) }).await?;
        topic.send().await?;
        let executed = topic.receive().await?;
        ArtcatResult {
            result: Ok(data),
            executed,
        }
    }
    
    pub async fn run(&self) {
        let mut header = <Command as FromBytes>::Bytes::zeroed();
        let mut receive = [0u8; MAX_COMMAND];
        loop {
            let (header, data) = {
                let mut bus = self.bus.lock().await;
                let size = bus.read(&mut header).await;
                let header = Command::from_be_bytes(header);
                let data = &mut receive[.. usize::from(header.size)];
                let size = bus.read(data).await;
                (header, data)
            };
            
            let mut pending = self.pending.lock().await;
            if let Some(buffer) = pending.get_mut(&header.token) {
                buffer.result = 
                    if buffer.command.token == header.token
                    && buffer.command.access == header.access
                    && buffer.command.address == header.address
                    && buffer.command.size == header.size
                {
                    buffer.buffer.copy_from_slice(data);
                    Some(Ok(header.executed))
                }
                else 
                    {Some(Err(ArtcatError::Master("reponse header mismatch")))};
                
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
                command.address = SlaveLocal::new(slave, local).into();
            },
            Address::Fixed(slave, local) => {
                command.access.set_fixed(true);
                command.address = SlaveLocal::new(slave, local).into();
            },
            Address::Virtual(global) => {
                command.address = global;
            },
        }
        command
    }
}

struct Topic<'m, B> {
    master: &'m Master<B>,
    token: Token,
}
impl<'m, B: AsyncRead + AsyncWrite + Unpin> Topic<'m, B> {
    async fn new(master: &'m Master<B>, command: Command, data: &'static mut [u8]) -> Result<Self, ArtcatError> {
        let token;
        {
            let mut pending = master.pending.lock().await;
            token = loop {
                if let Some(token) = (0 ..= pending.len()) .filter(pending.contains).first()
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
        }
        Ok(Self{master, token})
    }
    async fn send(&self) -> Result<(), ArtcatError> {
        let mut pending = self.master.pending.lock().await;
        let buffer = pending.get_mut(&self.token).unwrap();
        {
            let mut bus = self.master.bus.lock().await;
            bus.write(&buffer.command.to_be_bytes()).await;
            bus.write(buffer.buffer).await;
        }
        Ok(())
    }
    async fn receive(&self) -> Result<u8, ArtcatError> {
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
impl<B> Drop for Topic<'_, B> {
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


/** arcat command result
    
    basically contains a success/error flag with associated data, and the number of slaves who executed the command
*/
pub struct ArtcatResult<T> {
    result: Result<T, ArtcatError>,
    executed: u8,
}
impl<T> ArtcatResult<T> {
    pub fn executed(&self) -> u8 {
        self.executed
    }
    pub fn any(self) -> Result<T, ArtcatError> {
        self.result
    }
    pub fn exact(self, executed: u8) -> Result<T, ArtcatError> {
        if self.executed != executed {
            if self.executed == 0
                {return Err(ArtcatError::Master("no slave answered"))}
            else
                {return Err(ArtcatError::Master("incorrect number of answers"))}
        }
        self.result
    }
    pub fn once(self) -> Result<T, ArtcatError>  {
        self.exact(1)
    }
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> ArtcatResult<U> {
        match self.result {
            Ok(value) => ArtcatResult {
                result: Ok(f(value)),
                executed: self.executed,
            },
            Err(error) => ArtcatResult {
                result: Err(self.result),
                executed: self.executed,
            }
        }
    }
}
