use std::{
    sync::Arc,
    time::Duration,
    };
use futures_concurrency::future::Race;
use packbytes::{FromBytes, ToBytes};
use serial_test::serial;

use uartcat::{
    registers::{self, Register, SlaveRegister, VirtualSize},
    master::*,
    };


fn test<T, F>(test: T)
where 
    T: FnOnce(Arc<Master>) -> F,
    F: Future,
{
    tokio::runtime::Runtime::new() 
    .expect("failed to create runtime")
    .block_on(async move {
        let master = Arc::new(Master::new("/dev/ttyUSB1", 1_500_000) .expect("failed to initialize master"));
        (
            async {
                tokio::time::timeout(Duration::from_secs(10), test(master.clone()))
                .await.expect("aborted test because took too long");
            }, 
            async {
                master.run()
                .await.expect("master communication failed");
            },
        ).race().await;
    });
}

// declare some application-specific registers expected on the slave
const COUNTER: SlaveRegister<u32> = Register::new(0x500);
const OFFSET: SlaveRegister<u16> = Register::new(0x504);
const OFFSETED: SlaveRegister<u32> = Register::new(0x512);

// buffer with a different layout
#[derive(FromBytes, ToBytes, Default, Clone, Debug)]
pub struct MyBuffer {
    pub offseted: u32,
    pub offset: u16,
}
// buffer with a different layout
#[derive(FromBytes, ToBytes, Default, Clone, Debug)]
pub struct MyBuffer2 {
    pub offset: u16,
    pub counter: u32,
    pub offseted: u32,
}


#[test]
#[serial]
fn addresses_topological_fixed() {
    test(|master| async move {
        for fixed in 1 .. 4 {
            master.slave(Host::Topological(0)).write(registers::ADDRESS, fixed).await.unwrap().one().unwrap();
            
            let slave = master.slave(Host::Topological(0));
            assert_eq!(slave.read(registers::VERSION).await.unwrap().one().unwrap(), 1);
            
            let slave = master.slave(Host::Fixed(fixed));
            assert_eq!(slave.read(registers::VERSION).await.unwrap().one().unwrap(), 1);
        }
    });
}

#[test]
#[serial]
fn standard_registers() {
    test(|master| async move {
        let slave = master.slave(Host::Topological(0));
        
        slave.write(registers::LOSS, 0).await.unwrap().one().unwrap();
        slave.write(registers::ERROR, registers::CommandError::None).await.unwrap().one().unwrap();
        
        let device = slave.read(registers::DEVICE).await.unwrap().one().unwrap();
        assert_eq!(device.model.as_str().unwrap(), "esp32-test"); 
        assert_eq!(device.software_version.as_str().unwrap(), "0.2");
        assert_eq!(device.hardware_version.as_str().unwrap(), "0.1");
        
        let error = slave.read(registers::ERROR).await.unwrap().one().unwrap();
        assert_eq!(error, registers::CommandError::None);
    });
}

#[test]
#[serial]
fn read_write_while_updating() {
    test(|master| async move {
        let slave = master.slave(Host::Topological(0));
        
        let start = slave.read(COUNTER).await.unwrap().one().unwrap();
        
        // check that slave counts as expected
        let mut last = start;
        for _ in 0 .. 10 {
            let value = slave.read(COUNTER).await.unwrap().one().unwrap();
            assert!(value.wrapping_sub(last) <= 2, "counter dephasing");
            tokio::time::sleep(Duration::from_millis(10)).await;
            last = value;
        }
        
        // set counter to specific value, slave should count starting there
        let new = 1042;
        slave.write(COUNTER, new).await.unwrap().one().unwrap();
        
        // check that we restarted
        let mut changed = false;
        for _ in 0 .. 10 {
            let value = slave.read(COUNTER).await.unwrap().one().unwrap();
            if value.wrapping_sub(new) <= 1 {
                changed = true;
                break
            }
        }
        assert!(changed, "failed to set counter");
    });
}

#[test]
fn offline_mapping() {
    // create a mapping to gather many registers
    let slave = Host::Topological(42);
    let mut mapping = Mapping::new();
    let a = mapping.buffer::<MyBuffer>().unwrap()
        .register(slave, OFFSETED)
        .register(slave, OFFSET)
        .build();
    let b = mapping.buffer::<MyBuffer2>().unwrap()
        .register(slave, OFFSET)
        .register(slave, COUNTER)
        .register(slave, OFFSETED)
        .build();
    
    assert!(a.address() == 0);
    assert!(a.size() == 6);
    assert!(b.address() == 6);
    assert!(b.size() == 10);
    
    assert_eq!(mapping.map()[&slave], &[
        registers::Mapping {
            virtual_start: 0,
            slave_start: OFFSETED.address(),
            size: OFFSETED.size(),
        },
        registers::Mapping {
            virtual_start: VirtualSize::from(OFFSETED.size()),
            slave_start: OFFSET.address(),
            size: OFFSET.size(),
        },
        registers::Mapping {
            virtual_start: VirtualSize::from(OFFSETED.size() + OFFSET.size()),
            slave_start: OFFSET.address(),
            size: OFFSET.size(),
        },
        registers::Mapping {
            virtual_start: VirtualSize::from(OFFSETED.size() + OFFSET.size() + OFFSET.size()),
            slave_start: COUNTER.address(),
            size: COUNTER.size(),
        },
        registers::Mapping {
            virtual_start: VirtualSize::from(OFFSETED.size() + OFFSET.size() + OFFSET.size() + COUNTER.size()),
            slave_start: OFFSETED.address(),
            size: OFFSETED.size(),
        },
    ]);
}

#[test]
#[serial]
fn streaming_virtual() {
    test(|master| async move {
        let slave = master.slave(Host::Topological(0));
    
        let mut mapping = Mapping::new();
        let buffer = mapping.buffer::<MyBuffer>().unwrap()
            .register(slave.address(), OFFSETED)
            .register(slave.address(), OFFSET)
            .build();
            
        mapping.configure(&slave).await.unwrap();
        
        // stream our custom packet of data
        let mut previous = MyBuffer::default();
        let mut current = MyBuffer::default();
        let stream = master.stream(buffer).await.unwrap();
        stream.send_exchange(current.clone()).await.unwrap();
        for i in 0 .. 10 {
            tokio::time::sleep(Duration::from_millis(10)).await;
            stream.send_exchange(previous.clone()).await.unwrap();
            (current, previous) = (stream.receive().await.unwrap().data, current);
            current.offset = (i%2)*100;
        }
        
        // TODO improve to actually check counter values and interaction with direct slave access
    });
}
