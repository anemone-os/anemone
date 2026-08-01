use super::*;
use sys::anemone::power;

pub fn shutdown(magic: u64) -> Result<(), Errno> {
    power::shutdown(magic)
}
