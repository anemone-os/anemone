use crate::{
    device::char::{CharDev, register_char_device},
    prelude::*,
};

#[derive(Debug)]
struct Full;

const FULL_DEVNUM: CharDevNum = CharDevNum::new(
    MajorNum::new(devnum::char::major::MEMORY),
    MinorNum::new(devnum::char::minor::FULL),
);

impl CharDev for Full {
    fn devnum(&self) -> CharDevNum {
        FULL_DEVNUM
    }

    fn read(&self, buf: &mut [u8]) -> Result<usize, SysError> {
        buf.fill(0x00);
        Ok(buf.len())
    }

    fn write(&self, buf: &[u8]) -> Result<usize, SysError> {
        Err(SysError::NoSpace)
    }
}

#[initcall(probe)]
fn init() {
    match register_char_device(FULL_DEVNUM, "full".to_string(), Arc::new(Full)) {
        Ok(()) => {
            knoticeln!("full device registered");
        },
        Err(e) => {
            knoticeln!("failed to register full device: {:?}", e);
        },
    }
}
