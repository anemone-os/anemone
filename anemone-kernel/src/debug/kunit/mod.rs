/// In-kernel unit testing framework, inspired by Linux's KUnit but simplified
/// for Anemone's needs.
///
/// Since we don't support stack unwinding now, a panic in a kunit test will
/// crash the kernel.
use crate::prelude::*;

#[repr(C)]
pub struct KUnit {
    pub name: &'static str,
    pub test_fn: fn(),
}

/// Since we don't support stack unwinding, there is no need to count failed
/// tests separately - if a test panics, the kernel will crash and we won't
/// reach the end of the test runner.
#[cfg(feature = "kunit")]
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
        (kunit.test_fn)();
        kprintln!("{}ok{}", GREEN_BOLD, RESET);
    }

    kprintln!("{}All tests passed!{}", BOLD, RESET);
}
