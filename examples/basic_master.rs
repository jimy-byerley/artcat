use std::time::Duration;
use futures_concurrency::future::Join;
use artcat::{
    registers::Register,
    master::{Master, Host},
    };

#[tokio::main]
async fn main() {
    // declare some application-specific registers expected on the slave
    let counter = Register::<u32>::new(0x300);
    // initialize a master on some uart port
    let master = Master::new("/dev/ttyUSB0", 9600).unwrap();
    
    let task = async {
        println!("created master");
        // read and write registers
        let slave = Host::Topological(0);
        println!("standard device info: {:?}", master.read(slave, artcat::registers::DEVICE).await.unwrap().any().unwrap());
        for _ in 0 .. 10 {
            println!("specific counter register: {:?}", master.read(slave, counter).await.unwrap().any().unwrap());
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    let (_, run) = (task, master.run()).join().await;
    run.unwrap();
}
