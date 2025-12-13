use std::time::Duration;
use futures_concurrency::future::Race;
use packbytes::{FromBytes, ToBytes};
use artcat::{
    registers::{self, Register, SlaveRegister},
    master::*,
    };

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    // initialize a master on some uart port
    println!("creating master");
    let master = Master::new("/dev/ttyUSB1", 1_500_000).unwrap();
    
    let task = async {
        println!("running task");
        let slave = master.slave(Host::Topological(0));
        // read standard registers
        let device = slave.read(registers::DEVICE).await.unwrap().any().unwrap();
        println!("standard device info: model: {}  soft: {}  hard: {}  serial: {}", 
                device.model.as_str().unwrap(), 
                device.software_version.as_str().unwrap(),
                device.hardware_version.as_str().unwrap(),
                device.serial.as_str().unwrap(),
                );
        // read non standard registers
        for i in 0 .. 10 {
            println!("specific counter register: {}, {:?}", i, slave.read(COUNTER).await.unwrap().any().unwrap());
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // change address to fixed
        let address = 1;
        slave.write(registers::ADDRESS, address).await.unwrap();
        
        // read non standard registers with fixed address
        let slave = master.slave(Host::Fixed(address));
        for i in 0 .. 10 {
            println!("specific counter register: {}, {:?}", i, slave.read(COUNTER).await.unwrap().any().unwrap());
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        
        // create a mapping to gather many registers
        let mut mapping = Mapping::new();
        let buffer = mapping.buffer::<MyBuffer>().unwrap()
            .register(slave.address(), OFFSETED)
            .register(slave.address(), OFFSET)
            .build();
        
        mapping.configure(&slave).await.unwrap();
        
        // stream our custom packet of data
        let mut previous = MyBuffer::default();
        let mut current = MyBuffer::default();
        let stream = master.stream(buffer).await?;
        stream.send_exchange(current.clone()).await.unwrap();
        for i in 0 .. 10 {
            tokio::time::sleep(Duration::from_millis(10)).await;
            stream.send_exchange(previous.clone()).await.unwrap();
            (current, previous) = (stream.receive().await.unwrap().data, current);
            println!("{}:  offset {} offseted {}", 
                i,
                current.offset,
                current.offseted,
                );
            current.offset = (i%2)*100;
        }
        
        Ok::<(), artcat::master::Error>(())
    };
    let com = async {
        Ok(master.run().await?)
    };
    (task, com).race().await.unwrap();
}


// declare some application-specific registers expected on the slave
const COUNTER: SlaveRegister<u32> = Register::new(0x500);
const OFFSET: SlaveRegister<u16> = Register::new(0x504);
const OFFSETED: SlaveRegister<u32> = Register::new(0x512);

// buffer with a different layout
#[derive(FromBytes, ToBytes, Default, Clone, Debug)]
pub struct MyBuffer {
    pub offseted: u32,
    pub offset: u16,
}
