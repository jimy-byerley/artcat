use std::time::{Instant, Duration};
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
    // 4_147_200
    // 921_600
    // 1_792_000
    // 1_843_200
    // 3_584_000
    // 2_000_000
    let master = Master::new("/dev/ttyUSB1", 2_000_000).unwrap();
    
    let task = async {
        println!("running task");
        // read and write registers
        let slave = master.slave(Host::Topological(0));
        for size in 2 .. 1024 {
            let mut data = vec![0; size];
            
            let start = Instant::now();
            let result = slave.read_bytes(0, &mut data).await;
            let complete_read = start.elapsed();
            
//             let stream = slave.stream_bytes(0, data.clone()).await;
//             let start = Instant::now();
//             stream.send_read().await;
//             let send_read = start.elapsed();
//             
//             let start = Instant::now();
//             stream.receive().await;
//             let receive_read = start.elapsed();

            println!(" size {}:  elapsed {:?}  result {:?}", size, complete_read, result.map(|_| ()));
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
