use crate::prelude::*;

fn normalize_umask(mask: u32) -> InodePerm {
    InodePerm::from_bits_retain((mask & InodePerm::all_rwx().bits() as u32) as u16)
}

#[syscall(SYS_UMASK)]
fn sys_umask(mask: u32) -> Result<u64, SysError> {
    let mask = normalize_umask(mask);
    let old = get_current_task().replace_umask(mask);
    kdebugln!("sys_umask: new={:#o}, old={:#o}", mask.bits(), old.bits());

    Ok(old.bits() as u64)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn test_normalize_umask_keeps_only_rwx_bits() {
        assert_eq!(normalize_umask(0o71022).bits(), 0o022);
        assert_eq!(normalize_umask(u32::MAX), InodePerm::all_rwx());
    }
}
