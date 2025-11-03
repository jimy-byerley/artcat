use core::cell::UnsafeCell;
use packbytes::{FromBytes, ToBytes, ByteArray};
use embedded_io_async::{Read, Write};

use crate::{
    mutex::*,
    command::*,
    registers::{Register, self},
    pack_bilge,
    };



struct Slave<'d, B, const MEM: usize> {
    buffers: UnsafeCell<[[u8; MEM]; 3]>,
    control: BusyMutex<SlaveControl<B>>,
    receive: BusyMutex<[u8; MAX_COMMAND]>,
    send: BusyMutex<[u8; MAX_COMMAND]>,
}
struct SlaveControl<B> {
    bus: B,
    mapping: heapless::Vec<registers::Mapping, 128>,
    address: u16,
}
impl<'d, B: Read + Write, const MEM: usize> Slave<'d, B, MEM> {
    pub fn new(bus: B, device: registers::Device) -> Self {
        todo!()
    }
    pub async fn get<T: FromBytes>(&self, register: Register<T>) -> T {
        let mut dst = T::Bytes::zeroed();
        let src = self.buffer.lock().await;
        dst.as_mut().copy_from_slice(&src[usize::from(register.address) ..][.. T::Bytes::SIZE]);
        T::from_be_bytes(dst)
    }
    pub async fn set<T: ToBytes>(&self, register: Register<T>, value: T) {
        let src = value.to_be_bytes();
        let mut dst = self.buffer.lock().await;
        dst[usize::from(register.address) ..][.. T::Bytes::SIZE].copy_from_slice(src.as_ref());
    }
    pub async fn run(&self) {
        let mut control = self.control.lock().await;
        let mut receive = self.receive.lock().await;
        let mut send = self.send.lock().await;
        
        let mut address;
        loop {
            // read header
            let size = control.bus.read(&mut receive[.. <Command as FromBytes>::Bytes::SIZE]).await;
            let header = Command::from_be_bytes(&receive);
            let local = SlaveRegister::from(header.address);
            
            // check command consistency
            if header.size > MAX_COMMAND {
                self.set(registers::error, registers::CommandError::InvalidAccess).await;
                continue;
            }
            if header.access.slave_fixed() && header.access.slave_topological() {
                self.set(registers::error, registers::CommandError::InvalidCommand).await;
                continue;
            }
            
            // logic for topologial addresses
            if header.access.slave_topological() {
                local.set_slave(local.slave().wrapping_sub(1));
                header.address = local.into();
            }
            // direct access to slave buffer
            if header.access.slave_fixed() && local.slave() == control.address
            || header.access.slave_topological() && local.slave() == 0 
            {
                // exchange requested chunk of data
                // mark the command executed
                header.executed += 1;
                control.bus.write(&header.to_be_bytes()).await;
                
                let size = control.bus.read(&mut receive[.. header.size]).await;
                assert_eq!(size, header.size);
                
                if header.access.read() {
                    send[.. header.size].copy_from_slice(
                        &self.buffer.lock().await
                        [usize::from(local.register()) ..][.. header.size]
                        );
                    control.bus.write(&send[.. header.size]).await;
                }
                else {
                    control.bus.write(&receive[.. header.size]).await;
                }
                if header.access.write() {
                    self.buffer.lock().await
                        [usize::from(local.register()) ..][.. header.size]
                        .copy_from_slice(&receive[.. header.size]);
                }
                
                // special actions for special registers
                let address = local.register();
                if address == registers::address.address {
                    control.address = self.get(registers::address).await;
                }
                else if address == registers::mapping.address {
                    let table = self.get(registers::mapping).await;
                    control.mapping.clear();
                    control.mapping.extend_from_slice(table.map[.. table.size]);
                    control.mapping.sort_by_key(|item| item.virtual_start);
                }
            }
            // access to bus virtual memory
            else if !header.access.slave_fixed() && !header.access.slave_topological() {
                // exchange data according to local mapping
                header.executed += 1;
                control.bus.write(&header.to_be_bytes()).await;
                
                todo!("iterate over mappings inside the requested area and exchange with registers");
            }
            // any other command
            else {
                // simply pass data
                control.bus.write(&header.to_be_bytes()).await;
                
                let size = control.bus.read(&mut receive[.. header.size]).await;
                assert_eq!(size, header.size);
                
                control.bus.write(receive[.. size]).await;
            }
        }
    }
}
