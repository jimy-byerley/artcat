#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use esp_backtrace as _;
use esp_hal::{
    clock::CpuClock,
    timer::timg::TimerGroup,
    uart::{DataBits, Parity, StopBits},
};
use embassy_executor::Spawner;
use esp_println as _;
use log::*;
// use embedded_io_async::{Read, Write};


esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(_spawner: Spawner) {
    // init hardware
    esp_println::logger::init_logger_from_env();
    
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);
    
    // initialize slave
    info!("setting up slave");
    let config = esp_hal::uart::Config::default()
        .with_baudrate(1_500_000)  // same for 115_200
        .with_data_bits(DataBits::_8)
        .with_stop_bits(StopBits::_1)
        .with_parity(Parity::Even)
        ;
    let mut bus = esp_hal::uart::Uart::new(peripherals.UART1, config).unwrap()
        .with_rx(peripherals.GPIO16)
        .with_tx(peripherals.GPIO17)
        .into_async();
    
    info!("start echoing");
    let mut buffer = [0; 129];  // works with 128, but not after
    loop {
        if let Err(err) = bus.read_exact_async(&mut buffer).await {
            debug!("read error: {:?}", err);
            continue
        }
        bus.write_async(&buffer).await.unwrap();
    }
}
