//! Reference:
//! - https://xkcd.com/221/
//!
//! "chosen by fair dice roll. guaranteed to be random." :P

use crate::{
    device::char::{CharDev, register_char_device},
    prelude::*,
};

#[derive(Debug)]
struct URandom;

const URANDOM_DEVNUM: CharDevNum = CharDevNum::new(
    MajorNum::new(devnum::char::major::MEMORY),
    MinorNum::new(devnum::char::minor::URANDOM),
);

impl CharDev for URandom {
    fn devnum(&self) -> CharDevNum {
        URANDOM_DEVNUM
    }

    fn read(&self, buf: &mut [u8]) -> Result<usize, FsError> {
        buf.fill(4);
        Ok(buf.len())
    }

    fn write(&self, buf: &[u8]) -> Result<usize, FsError> {
        Ok(buf.len())
    }
}

#[initcall(probe)]
fn init() {
    match register_char_device(URANDOM_DEVNUM, "urandom".to_string(), Arc::new(URandom)) {
        Ok(()) => {
            knoticeln!("urandom device registered");
        },
        Err(e) => {
            knoticeln!("failed to register urandom device: {:?}", e);
        },
    }
}
