# UartCAT

UartCAT is a realtime memory bus protocol implemented on a [UART](https://en.wikipedia.org/wiki/Universal_asynchronous_receiver-transmitter) [daisy chain](https://en.wikipedia.org/wiki/Daisy_chain_\(electrical_engineering\)). Its concept are inspired from [EtherCAT](https://en.wikipedia.org/wiki/EtherCAT) but with most of its complexity removed. UartCAT is a caterpillar propagating protocol using UART just like Ethercat is a caterpillar propagating protocol using [Ethernet](https://en.wikipedia.org/wiki/Ethernet).

The main advantages of this protocol are

- fully [open-source](https://en.wikipedia.org/wiki/Open_source)
- light weight
- no need for specific hardware for the protocol, any microcontroller has at least one UART

[![crate](https://img.shields.io/crates/v/uartcat.svg)](https://crates.io/crates/uartcat)
[![doc](https://img.shields.io/docsrs/uartcat)]()
[![ci](https://github.com/jimy-byerley/artcat/actions/workflows/ci.yml/badge.svg)](https://github.com/jimy-byerley/artcat/actions/workflows/ci.yml)

## differences with EtherCAT

- no more communication state machine (INIT, PREOP, SAFE-OP, OP) all features are working all the time
- no more mailbox nor canopen, just registers including user made ones
- no more EEPROM interface for slave informations, its registers too
- exchanges of data mapped to virtual (aka logical) memory are always bidirectional (no more sync manager directions)
- no distributed clock (for now, can be added in the future)

also differences due to UART instead of Ethernet:

- hotplug of devices need to be manually done, this is unspecified by UART
- no detection of whether the bus is connected or not to other devices
- much lower bandwidth
- no automatic negociation of bus frequency

## repo organization

- root is the `uartcat` crate, that implements master and slave parties of the protocol
- `master` is a collection of binaries running the uartcat master, for testing and examples
- `slave` is a collection of binaries running a uartcat test slave, and other example slaves



*master* and *slave* suites are implemented for *esp32* in this repo, but the uartcat protocol also works for any microcontroller with a UART bus and supported by rust.

### running tests

hardware config

- the UART0 interface of esp32 is on `/dev/ttyUSB0`
- the UART1 interface of esp32 is connected to your PC via a serial to USB converter on `/dev/ttyUSB1`

in one shell run the testing slave

```shell
cd uartcat/slave
ESP_LOG=debug cargo run
```

in a second shell run the tests at the master level

```shell
cd uartcat/master
RUST_LOG=debug cargo test
```

### running examples

in one shell run the slave implementation of the example

```shell
cd uartcat/slave
cargo run --example basic
```

in a second shell run the example code with the same name

```shell
cd uartcat/master
cargo run --example basic
```

## getting started

### example

on your master (can be your PC):

```rust
const CUSTOM_REGISTER: SlaveRegister<u32> = Register::new(0x500);

let master = Master::new("/dev/ttyUSB1", 1_500_000).unwrap();
let custom = master.read(CUSTOM_REGISTER).await?.any()?;
master.write(CUSTOM_REGISTER, custom+1).await?.any()?;

assert_eq!(custom, 42);
```

on your slave (any microcontroller)

```rust
const CUSTOM_REGISTER: SlaveRegister<u32> = Register::new(0x500);
const BUFFER: usize = 0x504;  // size of slave buffer accessible by master

let slave = Slave::<_, BUFFER>::new(
    I2c(UART1, 1_500_000, GPIO1, GPIO2), 
    Device {...},
	);
slave.lock().await.set(CUSTOM_REGISTER, 42);
slave.run().await;
```



for a complete example see [`master/examples/basic.rs`](master/examples/basic.rs) and matching [`slave/examples/basic.rs`](slave/examples/basic.rs)

