use std::vec::Vec;
use packbytes::{FromBytes, ToBytes, ByteArray};
use crate::registers::{Register, VirtualRegister};
use super::{
    Error,
    networking::{Master, Topic, Address, PinnedBuffer},
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
    pub async fn slave(&self, host: Host) -> Slave<'_>   {Slave{master: self, host}}
    
    pub async fn stream<T: FromBytes + ToBytes>(&self, buffer: VirtualRegister<T>) -> Result<Stream<'_, T>, Error> {
        Stream::<T, u32>::new(self, buffer).await
    }
    pub async fn stream_bytes(&self, _address: u32, _size: u16) -> StreamBytes<'_>   {todo!()}
    
    
    pub async fn read<T: FromBytes>(&self, register: VirtualRegister<T>) -> ArtcatResult<T> {
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
    
    pub async fn write<T: ToBytes>(&self, register: VirtualRegister<T>, value: T) -> ArtcatResult<()> {
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
    
    pub async fn exchange<C,T>(&self, register: VirtualRegister<T>, value: T) -> ArtcatResult<T> 
    where 
        C: ByteArray, 
        T: ToBytes<Bytes=C> + FromBytes<Bytes=C> 
    {
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
        let executed = {
            let topic = Topic::new(
                self, 
                Address::Virtual(address),
                PinnedBuffer::Borrowed(data),
                ).await?;
            topic.send(read, write, None).await?;
            topic.receive(None).await?
            };
        Ok(Answer {data, executed})
    }
}


pub struct Slave<'m> {
    master: &'m Master,
    host: Host,
}
#[derive(Copy, Clone, Eq, Hash, PartialEq, Debug)]
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
    
    pub async fn stream<T: FromBytes + ToBytes>(&self, buffer: Register<T>) -> Result<Stream<'m, T, u16>, Error> {
        Stream::<T, u16>::new(self.master, self.host, buffer).await
    }
    pub async fn stream_bytes(&self, _address: u16, _size: u16) -> StreamBytes<'m>   {todo!()}
    
    
    pub async fn read<T: FromBytes>(&self, register: Register<T>) -> ArtcatResult<T> {
        let mut buffer = T::Bytes::zeroed();
        let executed = self.read_bytes(register.address(), buffer.as_mut()).await?.executed;
        Ok(Answer{
            data: T::from_be_bytes(buffer),
            executed,
            })
    }
    pub async fn read_bytes<'d>(&self, address: u16, data: &'d mut [u8]) -> ArtcatResult<&'d mut [u8]> {
        self.command(address, true, false, data).await
    }
    
    pub async fn write<T: ToBytes>(&self, register: Register<T>, value: T) -> ArtcatResult<()> {
        let executed = self.write_bytes(register.address(), value.to_be_bytes().as_mut()).await?.executed;
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
        let executed = self.exchange_bytes(register.address(), buffer.as_mut()).await?.executed;
        Ok(Answer{
            data: T::from_be_bytes(buffer),
            executed,
            })
    }
    pub async fn exchange_bytes<'d>(&self, address: u16, data: &'d mut [u8]) -> ArtcatResult<&'d mut [u8]> {
        self.command(address, true, true, data).await
    }
    
    
    async fn command<'d>(&self, address: u16, read: bool, write: bool, data: &'d mut [u8]) -> ArtcatResult<&'d mut [u8]> {
        let executed = {
            let topic = Topic::new(
                self.master, 
                self.host.at(address.into()), 
                PinnedBuffer::Borrowed(data),
                ).await?;
            topic.send(read, write, None).await?;
            topic.receive(None).await?
            };
        Ok(Answer {data, executed})
    }
}




pub struct Stream<'m, T, S=u32> {
    register: Register<T,S>,
    topic: Topic<'m>,
}
impl<'m, T> Stream<'m, T, u16>
where T: FromBytes {
    pub async fn new(master: &'m Master, host: Host, register: Register<T>) -> Result<Self, Error> {
        Ok(Self {
            topic: Topic::new(
                master, 
                host.at(register.address()), 
                PinnedBuffer::Owned(Vec::from(T::Bytes::zeroed().as_ref())),
                ).await?,
            register,
            })
    }
}
impl<'m, T> Stream<'m, T, u32> 
where T: FromBytes {
    pub async fn new(master: &'m Master, register: VirtualRegister<T>) -> Result<Self, Error> {
        Ok(Self {
            topic: Topic::new(
                master, 
                Address::Virtual(register.address()), 
                PinnedBuffer::Owned(Vec::from(T::Bytes::zeroed().as_ref())),
                ).await?,
            register,
            })
    }
}
impl<'m, T,S> Stream<'m, T,S>
where 
    T: FromBytes,
    S: Copy,
{
    pub fn register(&self) -> Register<T,S>  {self.register.clone()}
    
    pub async fn receive(&mut self) -> T  {todo!()}
    pub async fn try_receive(&mut self) -> Option<T>  {todo!()}
}
impl<'m, T,S> Stream<'m, T,S>
where T: ToBytes
{
    pub async fn send_write(&self, value: T) -> Result<(), Error>  {
        self.topic.send(true, false, Some(value.to_be_bytes().as_ref())).await
    }
    pub async fn send_read(&self) -> Result<(), Error> {
        self.topic.send(false, true, Some(T::Bytes::zeroed().as_ref())).await
    }
    pub async fn send_exchange(&self, value: T) -> Result<(), Error> {
        self.topic.send(true, true, Some(value.to_be_bytes().as_ref())).await
    }
}


#[allow(unused)]  // TODO
pub struct StreamBytes<'m> {
    host: Host,
    address: u32,
    topic: Topic<'m>,
}
impl<'m> StreamBytes<'m> {
    // TODO
}
