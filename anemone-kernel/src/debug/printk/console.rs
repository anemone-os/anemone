//! Console abstraction module.
use core::fmt::Debug;

use crate::prelude::*;

/// A trait for kernel consoles. Implementations of this trait can represent
/// various types of consoles, such as serial consoles or graphical consoles.
pub trait Console: Send + Sync {
    fn output(&self, s: &str);
}

impl Debug for dyn Console {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "dyn Console")
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct ConsoleFlags: u32 {
        /// Console is used for early boot messages.
        ///
        /// If a console is registered with this flag, it will be
        /// automatically set to enabled during early boot.
        ///
        /// Such consoles will be unregistered after the system is fully booted,
        /// and will not receive log messages emitted after that point.
        const EARLY = 0b0001;
        /// When registering a console, also replay all previous log messages to it.
        const REPLAY = 0b0010;
        /// Whether the console is enabled.
        const ENABLED = 0b0100;
    }
}

/// Console descriptor, containing the console instance and its associated
/// flags.
#[derive(Debug)]
struct ConsoleDesc {
    console: Box<dyn Console>,
    flags: ConsoleFlags,
}

impl ConsoleDesc {
    fn enabled(&self) -> bool {
        self.flags.contains(ConsoleFlags::ENABLED)
    }

    fn enable(&mut self) {
        self.flags |= ConsoleFlags::ENABLED;
    }
}

/// System console manager, managing multiple console instances and dispatches
/// log messages to all registered consoles.
///
/// This struct should be used as a global singleton.
#[derive(Debug)]
pub struct SysConsole {
    consoles: SpinLock<Vec<ConsoleDesc>>,
}

impl SysConsole {
    /// Creates a new `SysConsole` instance.
    pub fn new() -> Self {
        Self {
            consoles: SpinLock::new(Vec::new()),
        }
    }

    /// Registers a new console with the system console manager.
    pub fn register_console(&self, console: Box<dyn Console>, flags: ConsoleFlags) {
        let mut flags = flags;
        if flags.contains(ConsoleFlags::EARLY) {
            flags |= ConsoleFlags::ENABLED;
        }
        let desc = ConsoleDesc { console, flags };
        let mut consoles = self.consoles.lock_irqsave();
        consoles.push(desc);
    }

    /// Outputs a string to all registered consoles.
    pub fn output(&self, s: &str) {
        for desc in self.consoles.lock_irqsave().iter() {
            if desc.enabled() {
                desc.console.output(s);
            }
        }
    }

    /// Return false if there are only early consoles registered, true
    /// otherwise.
    pub unsafe fn on_system_boot(&self) -> bool {
        let mut consoles = self.consoles.lock_irqsave();

        let mut has_normal_con = false;

        if consoles
            .iter()
            .any(|desc| !desc.flags.contains(ConsoleFlags::EARLY))
        {
            has_normal_con = true;
            consoles.retain(|desc| !desc.flags.contains(ConsoleFlags::EARLY));
            if !consoles.iter().any(|desc| desc.enabled()) {
                let desc = consoles.first_mut().unwrap();
                desc.enable();
            }
        }

        has_normal_con
    }
}
