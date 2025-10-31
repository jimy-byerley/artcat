use tokio_serial::{SerialStream, SerialPortBuilder};
use packbytes::{FromBytes, ToBytes, ByteArray};
use std::{
    task::{Poll, Waker},
    future::poll_fn,
    time::Duration,
    borrow::Cow,
    collections::HashMap,
    mem::transmute,
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
    Command(CommandError),
}

/// artcat master implementation
pub struct Master {
    /// uart RX/TX stream
    port: SerialStream,
    /// command answers currently waited for
    pending: BusyMutex<HashMap<Token, Pending>>,
}
/// hold data for receiving command's results
struct Pending {
    /// initial command header, executed is set to MAX until actual answer received
    command: Command,
    /// buffer for data reception
    buffer: &'static [u8],
    /// for waking up the async task waiting for the answer
    waker: Option<Waker>,
}
/// internal token type for pending commands
type Token = u16;


impl Master {
    pub fn new(path: impl Into<Cow<'_, str>>, rate: u32, timeout: Duration) -> Self {
        todo!()
    }
    pub async fn run(&self) {
        todo!();
    }
    async fn send(&self, command: Command, data: &'static mut [u8]) -> Result<Token, ArtcatError> {
        let token;
        {
            let mut pending = self.pending.lock().await;
            token = loop {
                if let Some(token) = (0 ..= pending.len()) .filter(pending.contains).first()
                    {break token}
                };
            let command = command.clone();
            command.set_executed(u8::MAX);
            pending.insert(token, Pending {
                command: command,
                buffer: data,
                waker: None,
                });
        }
        self.port.write(&command.to_ge_bytes()).await;
        self.port.write(data).await;
        Ok(token)
    }
    async fn receive(&self, token: Token) {
        poll_fn(|context| {
            if let Some(pending) = self.pending.try_lock() {
                if pending[token].executed != u8::MAX
                    {return Poll::Ready(())}
                pending[token].waker = context.waker();
            }
            // TODO check wether it is ok to return pending without changing waker in the pending task
            Poll::Pending
        }).await;
    }
    async fn command(&self, command: Command, data: &mut [u8]) -> Result<(), ArtcatError> {
        // TODO unregister command from pending if this future is canceled
        self.receive(
            self.send(command, unsafe{ transmute::<&mut [u8], &'static mut [u8]>(data) }).await?
            ).await?;
    }
    
    pub async fn read_bytes<'d>(&self, address: Address, data: &'d mut [u8]) -> Result<&'d mut [u8], ArtcatError> {
        self.command(Command::new(address, true, false, data.len()), data).await?;
        Ok(data)
    }
    pub async fn write_bytes(&self, address: Address, data: &[u8]) -> Result<(), ArtcatError> {
        self.command(Command::new(address, false, true, data.len()), data).await?;
    }
    pub async fn exchange_bytes<'d>(&self, address: Address, data: &'d mut [u8]) -> Result<&'d mut [u8], ArtcatError> {
        self.command(Command::new(address, true, true, data.len()), data).await?;
        Ok(data)
    }
    
    pub async fn read<T: FromBytes>(&self, host: Host, register: Register<T>) -> Result<T, ArtcatError> {todo!()}
    pub async fn write<T: ToBytes>(&self, host: Host, register: Register<T>, value: T) -> Result<(), ArtcatError> {todo!()}
    pub async fn exchange<T: ToBytes + FromBytes>(&self, host: Host, register: Register<T>, value: T) -> Result<T, ArtcatError> {todo!()}
}


impl Command {
    fn new(address: Address, read: bool, write: bool, size: u16) -> Self {
        let mut command = Self::default();
        command.access.set_read(read);
        command.access.set_write(write);
        command.size = size;
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
