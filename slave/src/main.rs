/// slave program for unit tests run on master side

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
    uart::{DataBits, Parity, StopBits, RxConfig},
};
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use embassy_futures::join::join;
use esp_println as _;
use log::*;

use uartcat::{
    registers::{Register, SlaveRegister, Device},
    slave::*,
    };


esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(_spawner: Spawner) {
    // init hardware
    esp_println::logger::init_logger_from_env();
    
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);
    
    // declare some application-specific registers, with custom alignments and order
    const MEMORY: usize = 0x516;
    const COUNTER: SlaveRegister<u32> = Register::new(0x500);
    const OFFSET: SlaveRegister<u16> = Register::new(0x504);
    const OFFSETED: SlaveRegister<u32> = Register::new(0x512);
    
    // initialize slave
    info!("setting up slave");
    let config = esp_hal::uart::Config::default()
        .with_baudrate(1_500_000)
        .with_data_bits(DataBits::_8)
        .with_stop_bits(StopBits::_1)
        .with_parity(Parity::Even)
        .with_rx(RxConfig::default() .with_fifo_full_threshold(1))
        ;
    let bus = esp_hal::uart::Uart::new(peripherals.UART1, config).unwrap()
        .with_rx(peripherals.GPIO16)
        .with_tx(peripherals.GPIO17)
        .into_async();
    let slave = Slave::<_, MEMORY>::new(bus, Device {
        serial: "".try_into().unwrap(),
        model: "esp32-test".try_into().unwrap(),
        hardware_version: "0.1".try_into().unwrap(),
        software_version: "0.2".try_into().unwrap(),
        });
    info!("init done");
    // refresh registers periodically
    let task = async {
        info!("running task");
        loop {
            Timer::after(Duration::from_millis(10)).await;
            let mut buffer = slave.lock().await;
            let count = buffer.get(COUNTER) + 1;
            let offset = buffer.get(OFFSET);
            buffer.set(COUNTER, count);
            buffer.set(OFFSETED, count + u32::from(offset));
        }
    };
    // run application-specific task and slave concurrently
    join(task, slave.run()).await;
}
