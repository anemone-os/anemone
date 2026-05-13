//! Initcall mechanism, used to run initialization code at various stages of the
//! kernel boot process in a automatic and modular manner.

use crate::prelude::*;

/// Initcall levels, which determine when the initcall functions will be called
/// during initialization.
#[derive(Debug)]
#[repr(i8)]
pub enum InitCallLevel {
    /// Register filesystem drivers.
    Fs = 0,
    /// Register generic device drivers.
    Driver = 1,
    /// Physical devices are probed when devices are discovered. This state
    /// happens right after that, and is for those virtual devices that should
    /// be registered to various subsystems. (e.g. null, zero, random for char
    /// subsystem, ramdisk for block subsystem, etc.)
    ///
    /// In that sense, this can be considered as virtual device probing.
    ///
    /// Additionally, those pseudo filesystems may also create their inodes in
    /// this stage.
    Probe = 2,
}

#[derive(Debug)]
#[repr(C)]
pub struct InitCall {
    pub name: &'static str,
    pub level: InitCallLevel,
    pub init_fn: fn(),
}

fn collect_initcalls(level: InitCallLevel) -> &'static [InitCall] {
    use link_symbols::*;

    let (start, end) = match level {
        InitCallLevel::Fs => (
            __sinitcall_fs as *const () as usize,
            __einitcall_fs as *const () as usize,
        ),
        InitCallLevel::Driver => (
            __sinitcall_driver as *const () as usize,
            __einitcall_driver as *const () as usize,
        ),
        InitCallLevel::Probe => (
            __sinitcall_probe as *const () as usize,
            __einitcall_probe as *const () as usize,
        ),
    };

    assert!(start.is_multiple_of(align_of::<InitCall>()));
    assert!((end - start).is_multiple_of(size_of::<InitCall>()));
    let initcall_size = core::mem::size_of::<InitCall>();
    let initcall_count = (end - start) / initcall_size;
    unsafe { core::slice::from_raw_parts(start as *const InitCall, initcall_count) }
}

/// Runs all initcalls of the specified level.
///
/// # Safety
///
/// Calling initcall functions may have arbitrary side effects, and may not be
/// safe to call at certain points during initialization. The caller must ensure
/// that it is safe to call the initcall functions at the time.
pub unsafe fn run_initcalls(level: InitCallLevel) {
    let initcalls = collect_initcalls(level);
    for initcall in initcalls {
        (initcall.init_fn)();
    }
}
