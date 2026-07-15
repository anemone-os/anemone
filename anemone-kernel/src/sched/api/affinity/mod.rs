//! Linux-compatible fixed-owner CPU affinity syscalls.

use anemone_abi::process::linux::sched::{CPU_SET_WORD_BITS, CPU_SET_WORD_BYTES};

use crate::prelude::*;

mod sched_getaffinity;
mod sched_setaffinity;

const KERNEL_CPU_MASK_WORDS: usize = (MAX_LOGICAL_CPUS + CPU_SET_WORD_BITS - 1) / CPU_SET_WORD_BITS;
const KERNEL_CPU_MASK_BYTES: usize = KERNEL_CPU_MASK_WORDS * CPU_SET_WORD_BYTES;

fn resolve_affinity_target(pid: i32) -> Result<Arc<Task>, SysError> {
    match pid {
        0 => Ok(get_current_task()),
        pid if pid > 0 => get_task(&Tid::new(pid as u32)).ok_or(SysError::NoSuchProcess),
        _ => Err(SysError::NoSuchProcess),
    }
}
