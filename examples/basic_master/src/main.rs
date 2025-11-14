use std::time::Duration;
use futures_concurrency::future::Race;
use artcat::{
    registers::{Register, Mapping, MappingTable},
    master::{Master, Host, Address},
    };

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    // declare some application-specific registers expected on the slave
    let counter = Register::<u32>::new(0x300);
    let discounter = Register::<u16>::new(0x304);
    // initialize a master on some uart port
    println!("creating master");
    // 4_147_200
    // 921_600
    // 1_792_000
    // 1_843_200
    // 3_584_000
    let master = Master::new("/dev/ttyUSB1", 2_000_000).unwrap();
    
    let task = async {
        println!("running task");
        // read and write registers
        let slave = master.slave(Host::Topological(0));
        let device = slave.read(artcat::registers::DEVICE).await.unwrap().any().unwrap();
        println!("standard device info: model: {}  soft: {}  hard: {}", 
                device.model.as_str().unwrap(), 
                device.software_version.as_str().unwrap(),
                device.hardware_version.as_str().unwrap(),
                );
        for i in 0 .. 10 {
            println!("specific counter register: {}, {:?}", i, slave.read(counter).await.unwrap().any().unwrap());
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        
        slave.write(artcat::registers::ADDRESS, Host::Fixed(1)).await.unwrap();
        for i in 0 .. 10 {
            println!("specific counter register: {}, {:?}", i, master.read(slave, counter).await.unwrap().any().unwrap());
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        
        let mut mapping = Mapping::new();
        let buffer = mapping.buffer::<MyBuffer>().unwrap();
            .register(slave.address(), registers::POSITION)
            .register(slave.address(), registers::FORCE)
            .build();
        
        let mut stream = master.stream(buffer);
        stream.send_exchange().await?;
        loop {
            let pack = stream.receive().await?;
            stream.send_exchange(pack).await?;
            pack.force;
            pack.position;
        }
        
        Ok(())
    };
    (task, master.run()).race().await.unwrap();
}

#[derive(FromBytes, ToBytes, Clone, Debug)]
pub struct MyBuffer {
    pub position: u32,
    pub force: u16,
}
