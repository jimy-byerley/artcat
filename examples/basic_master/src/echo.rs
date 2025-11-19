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
    let mut send = [0; 129];
    let mut receive = send.clone();
    let timeout = Duration::from_millis(100);
    
    while tokio::time::timeout(timeout, bus.read_exact(&mut receive)).await.is_ok() {}
    
    for j in 0 .. 20 {
        send[0] = j as u8;
        for i in 1 .. send.len() {send[i] = i as u8;}
        
        let start = Instant::now();
        bus.write(&send).await.unwrap();
        let result = tokio::time::timeout(timeout, bus.read_exact(&mut receive)).await;
        let complete_read = start.elapsed();
        
//         if result.is_ok() {
//             println!("    send {:?}", &send);
//             println!("    receive {:?}", &receive);
//         }
        
        println!("elapsed {:?}  result {:?}  equal: {}", complete_read, result.map(|_| ()), send == receive);
    }
}
