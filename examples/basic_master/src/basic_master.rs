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
    // 115_200
    // 1_000_000
    // 1_500_000
    // 2_000_000
    let master = Master::new("/dev/ttyUSB1", 1_500_000).unwrap();
    
    let task = async {
        println!("running task");
        // read and write registers
        let slave = master.slave(Host::Topological(0));
        for i in 0 .. 10 {
            if let Err(err) = slave.read(registers::DEVICE).await {
                println!("err {:?}", err);
            }
            else {
                println!("ok");
            }
        }
        let device = slave.read(registers::DEVICE).await.unwrap().any().unwrap();
        println!("standard device info: model: {}  soft: {}  hard: {}", 
                device.model.as_str().unwrap(), 
                device.software_version.as_str().unwrap(),
                device.hardware_version.as_str().unwrap(),
                );
//         for i in 0 .. 10 {
//             println!("specific counter register: {}, {:?}", i, slave.read(COUNTER).await.unwrap().any().unwrap());
//             tokio::time::sleep(Duration::from_millis(100)).await;
//         }
        
        let address = 1;
        slave.write(registers::ADDRESS, address).await.unwrap();
        let slave = master.slave(Host::Fixed(address));
//         for i in 0 .. 10 {
//             println!("specific counter register: {}, {:?}", i, slave.read(COUNTER).await.unwrap().any().unwrap());
//             tokio::time::sleep(Duration::from_millis(100)).await;
//         }
        
        let mut mapping = Mapping::new();
        let buffer = mapping.buffer::<MyBuffer>().unwrap()
            .register(slave.address(), COUNTER)
            .register(slave.address(), OFFSETED)
            .register(slave.address(), OFFSET)
            .build();
        
        const BUFFER_254: SlaveRegister<[u8; 254]> = Register::new(0);
        const BUFFER_255: SlaveRegister<[u8; 255]> = Register::new(0);
        const BUFFER_256: SlaveRegister<[u8; 256]> = Register::new(0);

        const BUFFER: SlaveRegister<[u8; 120]> = Register::new(0);
        
        slave.read(BUFFER).await.unwrap();
        slave.read(BUFFER_254).await.unwrap();
        slave.read(BUFFER_255).await.unwrap();
        slave.read(BUFFER_256).await.unwrap();
            
        mapping.configure(&slave).await.unwrap();
        
        let mut previous;
        let mut current = MyBuffer::default();
        let stream = master.stream(buffer).await?;
        stream.send_exchange(current.clone()).await.unwrap();
        for i in 0 .. 10 {
            (current, previous) = (stream.receive().await.unwrap().data, current);
            stream.send_exchange(previous.clone()).await.unwrap();
            println!("{}:  counter {} offset {} offseted {}", 
                i,
                current.counter,
                current.offset,
                current.offseted,
                );
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
    pub counter: u32,
    pub offseted: u32,
    pub offset: u16,
}
