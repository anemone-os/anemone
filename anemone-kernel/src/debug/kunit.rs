//! In-kernel unit testing framework.
//!
//! Registered cases run once and serially on the BSP `kinit` task after all
//! configured CPUs have completed local initialization, Late initcalls and
//! device attachment have finished, and the root filesystem has been mounted.
//! Interrupts, the scheduler, timers, allocation and `kthreadd` are available.
//! The initial userspace task has not been prepared, so the current task must
//! not be treated as an ordinary process with user memory, files, filesystem
//! state, credentials, signals or a userspace trap frame. Scheduler and wait
//! tests may use its explicitly available kernel-task properties.
//!
//! Cases share the live kernel, global registries and root filesystem. There is
//! no per-case isolation or execution-order contract. A case must withdraw its
//! publications, restore changed global state and stop or join every kthread it
//! creates before returning. Root-filesystem fixtures must also be removed;
//! the runner syncs mounted filesystems after the suite so successful cleanup
//! is durable.
//!
//! An unpublished `Task` may be constructed only as an owner-local data fixture
//! for APIs that explicitly accept a detached task. It must not be published
//! into the global task topology, enqueued onto a live processor or executed,
//! and proves no task-topology or execution lifecycle.
//! Tests that need running kernel concurrency must use the production
//! `KThreadBuilder` lifecycle; spawning a userspace task is unsupported here.
//! Multi-CPU tests must build an explicit topology with production kthread,
//! scheduler or IPI interfaces; KUnit does not execute arbitrary test functions
//! in per-CPU interrupt context. A passing case proves only the architecture,
//! CPU count, devices and filesystem selected for that run.
//!
//! Semantic negative cases should assert the expected `Err`, `None` or rejected
//! transition. Any panic is terminal for the kernel and the current test run:
//! the runner deliberately does not unwind or continue through possibly
//! partially mutated global state. Expected-panic tests therefore require a
//! separate single-case boot and host-side terminal oracle, not this runner.
use crate::prelude::*;

#[repr(C)]
pub struct KUnit {
    pub name: &'static str,
    pub test_fn: fn(),
}

fn sync_filesystem_mutations() {
    for sb in crate::fs::mounted_superblocks() {
        sb.fs().sync_fs(&sb).unwrap_or_else(|err| {
            panic!(
                "failed to sync filesystem {} after KUnit cleanup: {:?}",
                sb.fs().name(),
                err
            )
        });
    }
}

pub fn kunit_runner() {
    // yansi doesn't work well in macros, so we manually print the ANSI codes here
    const GREEN_BOLD: &str = "\x1b[32;1m";
    const BOLD: &str = "\x1b[1m";
    const RESET: &str = "\x1b[0m";

    let kunits = unsafe {
        use link_symbols::{__ekunit, __skunit};

        let (start, end) = (
            __skunit as *const () as usize,
            __ekunit as *const () as usize,
        );
        assert!(
            start.is_multiple_of(align_of::<KUnit>()),
            "KUnit start address({:#x}) is not properly aligned",
            start
        );
        assert!(
            (end - start).is_multiple_of(size_of::<KUnit>()),
            "KUnit end address({:#x}) is not properly aligned",
            end
        );
        let kunit_count = (end - start) / size_of::<KUnit>();
        core::slice::from_raw_parts(start as *const KUnit, kunit_count)
    };

    kprintln!("{}-- KUnit Test Runner --{}", BOLD, RESET);
    kprintln!("{}Running {} tests...{}", BOLD, kunits.len(), RESET);
    for kunit in kunits {
        kprint!("{}...", kunit.name);
        (kunit.test_fn)();
        kprintln!("{}ok{}", GREEN_BOLD, RESET);
    }

    // VFS KUnits remove their fixtures before returning, but block-backed
    // filesystems may retain those namespace updates in a writeback cache.
    // Make the cleanup durable before a host-side test gate can stop the VM.
    sync_filesystem_mutations();

    kprintln!("{}All tests passed!{}", BOLD, RESET);
}
