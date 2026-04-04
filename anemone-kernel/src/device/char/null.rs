//! /dev/null character device.

use crate::{
    device::char::{CharDev, register_char_device},
    prelude::*,
};

const NULL_DEVNUM: CharDevNum = CharDevNum::new(
    MajorNum::new(devnum::char::major::MEMORY),
    MinorNum::new(devnum::char::minor::NULL),
);

#[derive(Debug)]
struct Null;

impl CharDev for Null {
    fn devnum(&self) -> CharDevNum {
        NULL_DEVNUM
    }

    fn read(&self, buf: &mut [u8]) -> Result<usize, FsError> {
        Ok(0)
    }

    fn write(&self, buf: &[u8]) -> Result<usize, FsError> {
        Ok(buf.len())
    }
}

#[initcall(probe)]
fn init() {
    match register_char_device(NULL_DEVNUM, "null".to_string(), Arc::new(Null)) {
        Ok(()) => {
            knoticeln!("null device registered");
        },
        Err(e) => {
            knoticeln!("failed to register null device: {:?}", e);
        },
    }
}
