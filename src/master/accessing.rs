use std::vec::Vec;
use packbytes::{FromBytes, ToBytes, ByteArray};
use crate::registers::{Register, SlaveRegister, VirtualRegister, SlaveSize, VirtualSize};
use super::{
    Error,
    networking::{Master, Topic, Address, PinnedBuffer},
    };


type UartcatResult<T> = Result<Answer<T>, Error>;


/// received data and number of slaves who executed the command
pub struct Answer<T> {
    /// data received
    pub data: T,
    /// number of slaves that executed the command, if 0 then the data is supposed to be untouched
    pub executed: u8,
}
impl<T> Answer<T> {
    /// ok if at least one slave executed the command
    pub fn any(self) -> Result<T, Error> {
        if self.executed == 0 
            {return Err(Error::Master("no slave answered"))}
        Ok(self.data)
    }
    /// ok if the exact given number of slave executed the command
    pub fn exact(self, executed: u8) -> Result<T, Error> {
        if self.executed != executed {
            if self.executed == 0
                {return Err(Error::Master("no slave answered"))}
            else
                {return Err(Error::Master("incorrect number of answers"))}
        }
        Ok(self.data)
    }
    /// ok if the command was executed by by one slave only
    pub fn one(self) -> Result<T, Error>  {
        self.exact(1)
    }
}



impl Master {
    pub fn slave(&self, host: Host) -> Slave<'_>   {Slave{master: self, host}}
    
    pub async fn stream<T: FromBytes + ToBytes>(&self, buffer: VirtualRegister<T>) -> Result<Stream<'_, T>, Error> {
        Stream::<T, VirtualSize>::new(self, buffer).await
    }
    pub async fn read<T: FromBytes>(&self, register: VirtualRegister<T>) -> UartcatResult<T> {
        let mut buffer = T::Bytes::zeroed();
        let executed = self.read_bytes(register.address(), buffer.as_mut()).await?.executed;
        Ok(Answer{
            data: T::from_be_bytes(buffer),
            executed,
            })
    }
    pub async fn write<T: ToBytes>(&self, register: VirtualRegister<T>, value: T) -> UartcatResult<()> {
        let executed = self.write_bytes(register.address(), value.to_be_bytes().as_mut()).await?.executed;
        Ok(Answer{
            data: (),
            executed,
            })
    }
    pub async fn exchange<C,T>(&self, register: VirtualRegister<T>, value: T) -> UartcatResult<T> 
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
    
    pub async fn stream_bytes(&self, _address: VirtualSize, _size: SlaveSize) -> StreamBytes<'_>   {todo!()}
    pub async fn read_bytes<'d>(&self, address: VirtualSize, data: &'d mut [u8]) -> UartcatResult<&'d mut [u8]> {
        self.command(address, true, false, data).await
    }
    pub async fn write_bytes(&self, address: VirtualSize, data: &mut [u8]) -> UartcatResult<()> {
        self.command(address, false, true, data).await 
            .map(|a| Answer {data: (), executed: a.executed})
    }
    pub async fn exchange_bytes<'d>(&self, address: VirtualSize, data: &'d mut [u8]) -> UartcatResult<&'d mut [u8]> {
        self.command(address, true, true, data).await
    }
    
    async fn command<'d>(&self, address: VirtualSize, read: bool, write: bool, data: &'d mut [u8]) -> UartcatResult<&'d mut [u8]> {
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

/** 
    represent a specific slave on the bus

    this struct is a simple reference and address and can be created and destroyed whenever with no effect on the bus
*/
pub struct Slave<'m> {
    master: &'m Master,
    host: Host,
}
/// address of a slave on the bus
#[derive(Copy, Clone, Eq, Hash, PartialEq, Debug)]
pub enum Host {
    Topological(SlaveSize),
    Fixed(SlaveSize),
}
impl Host {
    pub fn at(self, memory: SlaveSize) -> Address {
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
    pub fn address(&self) -> Host {
        self.host
    }
    
    pub async fn stream<T: FromBytes + ToBytes>(&self, buffer: SlaveRegister<T>) -> Result<Stream<'m, T, SlaveSize>, Error> {
        Stream::<T, SlaveSize>::new(self.master, self.host, buffer).await
    }
    pub async fn read<T: FromBytes>(&self, register: SlaveRegister<T>) -> UartcatResult<T> {
        let mut buffer = T::Bytes::zeroed();
        let executed = self.read_bytes(register.address(), buffer.as_mut()).await?.executed;
        Ok(Answer{
            data: T::from_be_bytes(buffer),
            executed,
            })
    }
    pub async fn write<T: ToBytes>(&self, register: SlaveRegister<T>, value: T) -> UartcatResult<()> {
        let executed = self.write_bytes(register.address(), value.to_be_bytes().as_mut()).await?.executed;
        Ok(Answer{
            data: (),
            executed,
            })
    }
    /// read-then-write the given register on current slave
    pub async fn exchange<C: ByteArray, T: ToBytes<Bytes=C> + FromBytes<Bytes=C>>(&self, register: SlaveRegister<T>, value: T) -> UartcatResult<T> {
        let mut buffer = value.to_be_bytes();
        let executed = self.exchange_bytes(register.address(), buffer.as_mut()).await?.executed;
        Ok(Answer{
            data: T::from_be_bytes(buffer),
            executed,
            })
    }
    
    pub async fn read_bytes<'d>(&self, address: SlaveSize, data: &'d mut [u8]) -> UartcatResult<&'d mut [u8]> {
        self.command(address, true, false, data).await
    }
    pub async fn write_bytes(&self, address: SlaveSize, data: &mut [u8]) -> UartcatResult<()> {
        self.command(address, false, true, data).await 
            .map(|a| Answer {data: (), executed: a.executed})
    }
    pub async fn exchange_bytes<'d>(&self, address: SlaveSize, data: &'d mut [u8]) -> UartcatResult<&'d mut [u8]> {
        self.command(address, true, true, data).await
    }
    pub async fn stream_bytes(&self, _address: SlaveSize, _size: SlaveSize) -> StreamBytes<'m>   {todo!()}
    
    
    async fn command<'d>(&self, address: SlaveSize, read: bool, write: bool, data: &'d mut [u8]) -> UartcatResult<&'d mut [u8]> {
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



/** 
    Custom sequence access to bus memory
  
    It basically reserve a topic token on the bus, and allows repeated sending/receval using the same topic and memory area.
    The consequence is that any answer concerning that topic and region are received indistinctly. It allows custom exchange sequences, like artcat commands without waiting for answers, and receving answers in a separate coroutine.
*/
pub struct Stream<'m, T, A=VirtualSize> {
    register: Register<T,A>,
    topic: Topic<'m>,
}
impl<'m, T> Stream<'m, T, SlaveSize>
where T: FromBytes {
    pub async fn new(master: &'m Master, host: Host, register: SlaveRegister<T>) -> Result<Self, Error> {
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
impl<'m, T> Stream<'m, T, VirtualSize> 
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
impl<'m, T,A> Stream<'m, T,A>
where 
    T: FromBytes,
    A: Copy,
{
    /// return the register we are streaming
    pub fn register(&self) -> Register<T,A>  {self.register.clone()}
    
    /// wait for a answer to be received, and unpack the received value
    pub async fn receive(&self) -> UartcatResult<T>  {
        let mut buffer = T::Bytes::zeroed();
        let executed = self.topic.receive(Some(&mut buffer.as_mut())).await?;
        Ok(Answer{
            data: T::from_be_bytes(buffer),
            executed,
            })
    }
    /// check whether a answer has been received, and unpack the current value in the buffer whenever nothing has been received
    pub async fn get(&self) -> T  {
        let mut buffer = T::Bytes::zeroed();
        self.topic.get(&mut buffer.as_mut()).await;
        T::from_be_bytes(buffer)
    }
}
impl<'m, T,A> Stream<'m, T,A>
where T: ToBytes
{
    /// send a write command with the given value, this has not effect on the current value in the buffer
    pub async fn send_write(&self, value: T) -> Result<(), Error>  {
        self.topic.send(false, true, Some(value.to_be_bytes().as_ref())).await
    }
    /// send a read command , this has not effect on the current value in the buffer
    pub async fn send_read(&self) -> Result<(), Error> {
        self.topic.send(true, false, Some(T::Bytes::zeroed().as_ref())).await
    }
    /// send a read-then-write command writing the given value, this has not effect on the current value in the buffer
    pub async fn send_exchange(&self, value: T) -> Result<(), Error> {
        self.topic.send(true, true, Some(value.to_be_bytes().as_ref())).await
    }
}


/// TODO
#[allow(unused)]
pub struct StreamBytes<'m> {
    host: Host,
    address: VirtualSize,
    topic: Topic<'m>,
}
impl<'m> StreamBytes<'m> {
    // TODO
}
