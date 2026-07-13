use crate::prelude::*;

use super::target::{PriorityWhich, collect_priority_targets};

/// Return the highest priority among selected tasks.
///
/// The raw syscall ABI returns `20 - nice` so successful negative nice values
/// do not collide with negative errno returns.
///
/// Reference: <https://man7.org/linux/man-pages/man2/getpriority.2.html>.
#[syscall(SYS_GETPRIORITY)]
fn sys_getpriority(which: PriorityWhich, who: i32) -> Result<u64, SysError> {
    let current = get_current_task();
    let targets = collect_priority_targets(&current, which, who)?;
    let highest = targets
        .into_iter()
        .map(|target| nice_to_syscall_return(target.nice()))
        .max()
        .expect("priority target collector returned an empty snapshot");
    Ok(highest)
}

const fn nice_to_syscall_return(nice: Nice) -> u64 {
    (20 - nice.get() as i32) as u64
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn test_getpriority_raw_return_encoding() {
        assert_eq!(nice_to_syscall_return(Nice::MIN), 40);
        assert_eq!(nice_to_syscall_return(Nice::ZERO), 20);
        assert_eq!(nice_to_syscall_return(Nice::MAX), 1);
    }
}
