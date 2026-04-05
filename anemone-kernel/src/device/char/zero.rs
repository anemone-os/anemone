use crate::{
    device::char::{CharDev, register_char_device},
    prelude::*,
};

#[derive(Debug)]
struct Zero;

const ZERO_DEVNUM: CharDevNum = CharDevNum::new(
    MajorNum::new(devnum::char::major::MEMORY),
    MinorNum::new(devnum::char::minor::ZERO),
);

impl CharDev for Zero {
    fn devnum(&self) -> CharDevNum {
        ZERO_DEVNUM
    }

    fn read(&self, buf: &mut [u8]) -> Result<usize, FsError> {
        buf.fill(0x00);
        Ok(buf.len())
    }

    fn write(&self, buf: &[u8]) -> Result<usize, FsError> {
        Ok(buf.len())
    }
}

#[initcall(probe)]
fn init() {
    match register_char_device(ZERO_DEVNUM, "zero".to_string(), Arc::new(Zero)) {
        Ok(()) => {
            knoticeln!("zero device registered");
        },
        Err(e) => {
            knoticeln!("failed to register zero device: {:?}", e);
        },
    }
}
