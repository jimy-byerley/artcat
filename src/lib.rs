/*!
UartCAT is a realtime memory bus protocol implemented on a UART daisy chain. Its concept are inspired from EtherCAT but with most of its complexity removed. UartCAT is a caterpillar propagating protocol using UART just like Ethercat is a caterpillar propagating protocol using Ethernet.

The main advantages of this protocol are

- fully open-source
- light weight
- no need for specific hardware for the protocol, any microcontroller has at least one UART

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

for a complete example see [`master/examples/basic.rs`](https://github.com/jimy-byerley/uartcat/blob/master/master/examples/basic.rs) and matching [`slave/examples/basic.rs`](https://github.com/jimy-byerley/uartcat/blob/master/slave/examples/basic.rs)

*/

#![no_std]
#[cfg(feature = "std")]
extern crate std;

mod command;
mod mutex;
mod utils;


pub mod registers;
#[cfg(feature = "master")]
pub mod master;
#[cfg(feature = "slave")]
pub mod slave;
