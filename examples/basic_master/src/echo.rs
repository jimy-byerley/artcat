use std::time::{Duration, Instant};
use serial2_tokio::{SerialPort, CharSize, StopBits, Parity};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    // initialize a master on some uart port
    println!("creating master");
    
    // possible baud rates
    // 115_200
    // 1_000_000
    // 1_500_000
    // 2_000_000
    
    let mut bus = SerialPort::open("/dev/ttyUSB1", |mut settings: serial2_tokio::Settings| {
        settings.set_raw();
        settings.set_baud_rate(1_500_000)?;
        settings.set_char_size(CharSize::Bits8);
        settings.set_stop_bits(StopBits::One);
        settings.set_parity(Parity::Even);
        Ok(settings)
        }).unwrap();
        
    println!("running");
    const size: usize = 150;
    let mut send = [0; size];
    let timeout = Duration::from_millis(100);
    
    let result = tokio::time::timeout(timeout, bus.read_exact(&mut send)).await;
    
    for j in 0 .. 10 {
        send[0] = j as u8;
        for i in 1 .. send.len() {send[i] = (i as u8).wrapping_mul(2);}
        
        let mut receive = [0; size];
        
        let start = Instant::now();
        bus.write(&send).await.unwrap();
//         let result = tokio::time::timeout(timeout, bus.read_exact(&mut receive)).await;

        let result = async {
            let mut remain = &mut receive[..];
            while !remain.is_empty() {
                let Ok(received) = tokio::time::timeout(timeout, bus.read(remain)).await
                    else {return Err(std::io::Error::other("tokio timeout"))};
                remain = &mut remain[received? ..];
                dbg!(remain.len());
            }
            Result::<(), std::io::Error>::Ok(())
        }.await;
        
        let complete_read = start.elapsed();
        
        if result.is_ok() {
            println!("    send {:?}", &send);
            println!("    receive {:?}", &receive);
        }
        
        println!("elapsed {:?}  result {:?}  equal: {}", complete_read, result.map(|_| ()), send == receive);
        tokio::time::sleep(Duration::from_millis(100));
    }
}
