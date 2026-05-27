//! /dev/null character device.

use crate::{
    device::char::{CharDev, devfs::publish_char_device, register_char_device},
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

    fn read(&self, buf: &mut [u8]) -> Result<usize, SysError> {
        Ok(0)
    }

    fn write(&self, buf: &[u8]) -> Result<usize, SysError> {
        Ok(buf.len())
    }
}

#[initcall(probe)]
fn init() {
    match register_char_device(NULL_DEVNUM, "null".to_string(), Arc::new(Null)) {
        Ok(()) => {
            if let Err(err) = publish_char_device(NULL_DEVNUM) {
                knoticeln!(
                    "null device registered, but devfs publish failed: {:?}",
                    err
                );
            } else {
                knoticeln!("null device registered");
            }
        },
        Err(e) => {
            knoticeln!("failed to register null device: {:?}", e);
        },
    }
}
