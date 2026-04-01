/// In-kernel unit testing framework, inspired by Linux's KUnit but simplified
/// for Anemone's needs.
///
/// Since we don't support stack unwinding now, a panic in a kunit test will
/// crash the kernel.
use crate::prelude::*;

struct PerCpuKUnitBarrier {
    participants: AtomicUsize,
    ready: AtomicUsize,
    done: AtomicUsize,
    start: AtomicBool,
}

impl PerCpuKUnitBarrier {
    const fn new() -> Self {
        Self {
            participants: AtomicUsize::new(0),
            ready: AtomicUsize::new(0),
            done: AtomicUsize::new(0),
            start: AtomicBool::new(false),
        }
    }

    fn reset(&self) {
        self.participants.store(CpuArch::ncpus(), Ordering::Release);
        self.ready.store(0, Ordering::Release);
        self.done.store(0, Ordering::Release);
        self.start.store(false, Ordering::Release);
    }

    fn mark_ready(&self) {
        self.ready.fetch_add(1, Ordering::AcqRel);
    }

    fn wait_all_ready(&self) {
        while self.ready.load(Ordering::Acquire) < self.participants.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
    }

    fn release_start(&self) {
        self.start.store(true, Ordering::Release);
    }

    fn wait_start(&self) {
        while !self.start.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
    }

    fn mark_done(&self) {
        self.done.fetch_add(1, Ordering::AcqRel);
    }

    fn wait_all_done(&self) {
        while self.done.load(Ordering::Acquire) < self.participants.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
    }
}

struct PerCpuRunGuard;

impl PerCpuRunGuard {
    fn acquire() -> Self {
        while PERCPU_KUNIT_RUNNING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            core::hint::spin_loop();
        }
        Self
    }
}

impl Drop for PerCpuRunGuard {
    fn drop(&mut self) {
        PERCPU_KUNIT_RUNNING.store(false, Ordering::Release);
    }
}

static PERCPU_KUNIT_RUNNING: AtomicBool = AtomicBool::new(false);
static PERCPU_KUNIT_BARRIER: PerCpuKUnitBarrier = PerCpuKUnitBarrier::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum KUnitKind {
    Plain,
    PerCpu,
}

#[repr(C)]
pub struct KUnit {
    pub name: &'static str,
    pub test_fn: fn(),
    pub kind: KUnitKind,
}

pub fn handle_percpu_ipi_test(test_fn: fn()) {
    with_intr_enabled(|_| {
        PERCPU_KUNIT_BARRIER.mark_ready();
        PERCPU_KUNIT_BARRIER.wait_start();
        test_fn();
        PERCPU_KUNIT_BARRIER.mark_done();
    });
}

pub fn run_percpu_test(test_fn: fn()) {
    let _guard = PerCpuRunGuard::acquire();
    let sync = PERCPU_KUNIT_BARRIER.reset();

    broadcast_ipi_async(IpiPayload::RunKUnitPerCpu { test_fn })
        .expect("failed to dispatch percpu kunit");

    PERCPU_KUNIT_BARRIER.mark_ready();
    PERCPU_KUNIT_BARRIER.wait_all_ready();
    PERCPU_KUNIT_BARRIER.release_start();

    test_fn();
    PERCPU_KUNIT_BARRIER.mark_done();
    PERCPU_KUNIT_BARRIER.wait_all_done();
}

/// Since we don't support stack unwinding, there is no need to count failed
/// tests separately - if a test panics, the kernel will crash and we won't
/// reach the end of the test runner.
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
        // TODO: catch panics and count them as failures
        match kunit.kind {
            KUnitKind::Plain => (kunit.test_fn)(),
            KUnitKind::PerCpu => run_percpu_test(kunit.test_fn),
        }
        kprintln!("{}ok{}", GREEN_BOLD, RESET);
    }

    kprintln!("{}All tests passed!{}", BOLD, RESET);
}
