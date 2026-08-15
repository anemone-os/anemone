use super::*;

pub fn load(request: &anemone_abi::nemophila::LoadRequest) -> Result<u64, Errno> {
    unsafe {
        syscall(
            SYS_NEMOPHILA_LOAD,
            request as *const anemone_abi::nemophila::LoadRequest as u64,
            0,
            0,
            0,
            0,
            0,
        )
    }
}

pub fn try_unload(identity: u64, flags: u64) -> Result<(), Errno> {
    unsafe { syscall(SYS_NEMOPHILA_TRY_UNLOAD, identity, flags, 0, 0, 0, 0).map(|_| ()) }
}
