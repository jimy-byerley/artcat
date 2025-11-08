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
use embassy_time::{Duration, Timer};
use embassy_futures::join::join;
// use esp_println::{println, dbg};

use artcat::{
    registers::*, 
    slave::Slave,
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
    
    // declare some application-specific registers
    const MEMORY: usize = 0x304;
    const COUNTER: Register<u32> = Register::new(0x300);
    // initialize slave
    let config = esp_hal::uart::Config::default()
        .with_baudrate(9600)
        .with_data_bits(DataBits::_8)
        .with_parity(Parity::Even)
        .with_stop_bits(StopBits::_2)
        ;
    let bus = esp_hal::uart::Uart::new(peripherals.UART1, config).unwrap()
        .with_rx(peripherals.GPIO1)
        .with_tx(peripherals.GPIO2)
        .into_async();
    let slave = Slave::<_, MEMORY>::new(bus, Device {
        model: "esp32-example".try_into().unwrap(),
        hardware_version: "0.1".try_into().unwrap(),
        software_version: "0.1".try_into().unwrap(),
        });
    // refresh registers periodically
    let task = async {
        loop {
            Timer::after(Duration::from_millis(200)).await;
            let mut buffer = slave.lock().await;
            let count = buffer.get(COUNTER);
            buffer.set(COUNTER, count + 1);
        }
    };
    // run application-specific task and slave concurrently
    join(task, slave.run()).await;
}
