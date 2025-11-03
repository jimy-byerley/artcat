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
