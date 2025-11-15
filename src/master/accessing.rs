use core::{
    mem::transmute,
    marker::PhantomData,
    };
use packbytes::{FromBytes, ToBytes, ByteArray};
use crate::registers::{self, Register};
use super::{
    Error,
    networking::{Master, Topic, Address},
    };


type ArtcatResult<T> = Result<Answer<T>, Error>;


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



impl Master {
    pub async fn slave(&self, host: Host) -> Slave<'m>   {todo!()}
    
    pub async fn stream<T>(&self, buffer: Register<T>) -> Stream<'m, T>   {todo!()}
    pub async fn stream_bytes<T>(&self, address: u32, size: u16) -> StreamBytes<'m>   {todo!()}
    
    
    pub async fn read<T: FromBytes>(&self, register: Register<T>) -> ArtcatResult<T> {
        let mut buffer = T::Bytes::zeroed();
        let executed = self.read_bytes(register.address(), buffer.as_mut()).await?.executed;
        Ok(Answer{
            data: T::from_be_bytes(buffer),
            executed,
            })
    }
    pub async fn read_bytes<'d>(&self, address: u32, data: &'d mut [u8]) -> ArtcatResult<&'d mut [u8]> {
        self.command(address, true, false, data).await
    }
    
    pub async fn write<T: ToBytes>(&self, register: Register<T>, value: T) -> ArtcatResult<()> {
        let executed = self.write_bytes(register.address(), value.to_be_bytes().as_mut()).await?.executed;
        Ok(Answer{
            data: (),
            executed,
            })
    }
    pub async fn write_bytes(&self, address: u32, data: &mut [u8]) -> ArtcatResult<()> {
        self.command(address, false, true, data).await 
            .map(|a| Answer {data: (), executed: a.executed})
    }
    
    pub async fn exchange<C: ByteArray, T: ToBytes<Bytes=C> + FromBytes<Bytes=C>>(&self, register: Register<T>, value: T) -> ArtcatResult<T> {
        let mut buffer = value.to_be_bytes();
        let executed = self.exchange_bytes(register.address(), buffer.as_mut()).await?.executed;
        Ok(Answer{
            data: T::from_be_bytes(buffer),
            executed,
            })
    }    
    pub async fn exchange_bytes<'d>(&self, address: u32, data: &'d mut [u8]) -> ArtcatResult<&'d mut [u8]> {
        self.command(address, true, true, data).await
    }
    
    async fn command<'d>(&self, address: u32, read: bool, write: bool, data: &'d mut [u8]) -> ArtcatResult<&'d mut [u8]> {
        let topic = Topic::new(
            self, address, read, write, 
            unsafe{ transmute::<&mut [u8], &'static mut [u8]>(data) }
            ).await?;
        topic.send().await?;
        let executed = topic.receive().await?;
        Ok(Answer {
            data: data,
            executed,
        })
    }
}


pub struct Slave<'m> {
    master: &'m Master,
    host: Host,
}
#[derive(Copy, Clone)]
pub enum Host {
    Topological(u16),
    Fixed(u16),
}
impl Host {
    pub fn at(self, memory: u16) -> Address {
        match self {
            Host::Topological(slave) => Address::Topological(slave, memory),
            Host::Fixed(slave) => Address::Fixed(slave, memory),
        }
    }
}
impl<'m> Slave<'m> {
    pub fn new(master: &'m Master, host: Host) -> Self {
        Self {master, host}
    }
    
    pub async fn stream<T>(&self, buffer: Register<T>) -> Stream<'m, T>   {todo!()}
    pub async fn stream_bytes<T>(&self, address: u16, size: u16) -> StreamBytes<'m>   {todo!()}
    
    
    pub async fn read<T: FromBytes>(&self, register: Register<T>) -> ArtcatResult<T> {
        let mut buffer = T::Bytes::zeroed();
        let executed = self.read_bytes(buffer.as_mut()).await?.executed;
        Ok(Answer{
            data: T::from_be_bytes(buffer),
            executed,
            })
    }
    pub async fn read_bytes<'d>(&self, address: u16, data: &'d mut [u8]) -> ArtcatResult<&'d mut [u8]> {
        self.command(address, true, false, data).await
    }
    
    pub async fn write<T: ToBytes>(&self, register: Register<T>, value: T) -> ArtcatResult<()> {
        let executed = self.write_bytes(value.to_be_bytes().as_mut()).await?.executed;
        Ok(Answer{
            data: (),
            executed,
            })
    }
    pub async fn write_bytes(&self, address: u16, data: &mut [u8]) -> ArtcatResult<()> {
        self.command(address, false, true, data).await 
            .map(|a| Answer {data: (), executed: a.executed})
    }
    
    pub async fn exchange<C: ByteArray, T: ToBytes<Bytes=C> + FromBytes<Bytes=C>>(&self, register: Register<T>, value: T) -> ArtcatResult<T> {
        let mut buffer = value.to_be_bytes();
        let executed = self.exchange_bytes(buffer.as_mut()).await?.executed;
        Ok(Answer{
            data: T::from_be_bytes(buffer),
            executed,
            })
    }
    pub async fn exchange_bytes<'d>(&self, address: u16, data: &'d mut [u8]) -> ArtcatResult<&'d mut [u8]> {
        self.command(address, true, true, data).await
    }
    
    
    async fn command<'d>(&self, address: u16, read: bool, write: bool, data: &'d mut [u8]) -> ArtcatResult<&'d mut [u8]> {
        let topic = Topic::new(
            self.master, 
            self.host.at(address.into()), 
            read, 
            write, 
            unsafe{ transmute::<&mut [u8], &'static mut [u8]>(data) }
            ).await?;
        topic.send().await?;
        let executed = topic.receive().await?;
        Ok(Answer {
            data: data,
            executed,
        })
    }
}




pub struct Stream<'m, T: ByteArray> {
    host: Host,
    register: Register<T>,
    topic: Topic<'m>,
}
impl<'m, T: FromBytes + ToBytes> Stream<'m, T> {
    pub async fn new(master: &'m Master, host: Host, register: Register<T>) -> Self {
        Self {
            host,
            register,
            topic: Topic::new(host.at(register.address()), T::Bytes::zeroed()).await,
            }
    }

    pub fn host(&self) -> Host  {self.host}
    pub fn register(&self) -> Register<T>  {self.register.clone()}
    
    pub async fn receive(&mut self) -> T  {todo!()}
    pub async fn try_receive(&mut self) -> Option<T>  {todo!()}
    
    pub async fn send_write(&self, value: T) -> Result<(), Error>  {
        self.topic.send(true, false, &value.to_be_bytes()).await
    }
    pub async fn send_read(&self) -> Result<(), Error> {
        self.topic.send(false, true, &T::Bytes::zeroed()).await
    }
    pub async fn send_exchange(&self, value: T) -> Result<(), Error> {
        self.topic.send(true, true, &value.to_be_bytes()).await
    }
}

pub struct StreamBytes<'m> {
    host: Host,
    address: u32,
    topic: Topic<'m>,
}
impl<'m> StreamBytes {
    // TODO
}
