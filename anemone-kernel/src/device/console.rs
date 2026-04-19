//! Console subsystem.
//!
//! Here lies /dev/console.

use crate::{debug::printk::KERNEL_LOG, prelude::*, utils::any_opaque::NilOpaque};

use core::fmt::{Debug, Write};

pub trait Console: Send + Sync {
    fn output(&self, s: &str);
}

impl dyn Console {
    pub fn writer(&self) -> ConsoleWriter<'_> {
        ConsoleWriter { console: self }
    }
}

pub struct ConsoleWriter<'a> {
    console: &'a dyn Console,
}

impl Write for ConsoleWriter<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.console.output(s);
        Ok(())
    }
}

struct ConsoleDesc {
    ops: Arc<dyn Console>,
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

impl Debug for ConsoleDesc {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ConsoleDesc").finish()
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
        /// Enable the console during [on_system_boot()]
        const ENABLE_ON_BOOT = 0b1000;
    }
}

struct ConsoleSubSys {
    consoles: SpinLock<Vec<ConsoleDesc>>,
}

impl ConsoleSubSys {
    fn new() -> Self {
        Self {
            consoles: SpinLock::new(Vec::new()),
        }
    }
}

static SUBSYS: Lazy<ConsoleSubSys> = Lazy::new(|| ConsoleSubSys::new());

/// Register a console.
pub fn register_console(ops: Arc<dyn Console>, mut flags: ConsoleFlags) {
    if flags.contains(ConsoleFlags::EARLY) {
        // If the console is registered with EARLY flag, it will be automatically
        // enabled during early boot.
        flags |= ConsoleFlags::ENABLED;
    }

    if flags.contains(ConsoleFlags::REPLAY) {
        let it = KERNEL_LOG.iter_weak();
        for record in it {
            if !record.level.emits_to_console() {
                continue;
            }

            let full_msg_str =
                core::str::from_utf8(&record.msg[..record.len]).unwrap_or("[Invalid UTF-8]");
            ops.output(full_msg_str);
        }
    }

    SUBSYS
        .consoles
        .lock_irqsave()
        .push(ConsoleDesc { ops, flags });
}

/// Output a message to all enabled consoles.
///
/// **This function must not cause any reentrancy deadlocks, otherwise the
/// observability of the system will be compromised.**
///
/// TODO: Implement the above requirement.(maybe through lock-free data
/// structures).The above requirement also applies to kernel log buffer.
pub fn output(msg: &str) {
    SUBSYS
        .consoles
        .lock_irqsave()
        .iter()
        .filter(|desc| desc.enabled())
        .for_each(|desc| desc.ops.output(msg));
}

/// If there are any non-early consoles registered but none of them is enabled,
/// enable the first one. If there are only early consoles registered, keep them
/// as is and print a warning message, as this might indicate a problem with the
/// console registration order.
///
/// # Safety
///
/// Timing.
pub unsafe fn on_system_boot() {
    let mut consoles = SUBSYS.consoles.lock_irqsave();

    let mut has_normal_con = false;

    if consoles
        .iter()
        .any(|desc| !desc.flags.contains(ConsoleFlags::EARLY))
    {
        has_normal_con = true;
        consoles.retain(|desc| !desc.flags.contains(ConsoleFlags::EARLY));
    }

    consoles
        .iter_mut()
        .filter(|desc| desc.flags.contains(ConsoleFlags::ENABLE_ON_BOOT))
        .for_each(|desc| desc.enable());
    
    if !consoles.iter().any(|desc| desc.enabled()) {
        let desc = consoles.first_mut().unwrap();
        desc.enable();
    }
    drop(consoles);

    if !has_normal_con {
        kwarningln!("no normal console registered, only early consoles are available");
    } else {
        kinfoln!("normal console(s) registered, early consoles have been unregistered");
    }
}

fn console_read(_file: &File, _buf: &mut [u8]) -> Result<usize, SysError> {
    // currently no-op. always return EOF.
    Ok(0)
}

fn console_write(_file: &File, buf: &[u8]) -> Result<usize, SysError> {
    let s = core::str::from_utf8(buf).map_err(|_| SysError::InvalidArgument)?;
    output(s);
    Ok(buf.len())
}

fn console_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: InodeMode::new(InodeType::Char, inode.perm()),
        nlink: inode.nlink(),
        uid: 0,
        gid: 0,
        rdev: DeviceId::None,
        size: inode.size(),
        atime: inode.atime(),
        mtime: inode.mtime(),
        ctime: inode.ctime(),
    })
}

static CONSOLE_STDIN_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotSupported),
    touch: |_, _, _| Err(SysError::NotSupported),
    mkdir: |_, _, _| Err(SysError::NotSupported),
    symlink: |_, _, _| Err(SysError::NotSupported),
    unlink: |_, _| Err(SysError::NotSupported),
    rmdir: |_, _| Err(SysError::NotSupported),
    link: |_, _, _| Err(SysError::NotSupported),
    get_attr: console_get_attr,
    read_link: |_| Err(SysError::NotSymlink),
    open: |_| {
        Ok(OpenedFile {
            file_ops: &CONSOLE_STDIN_FILE_OPS,
            prv: NilOpaque::new(),
        })
    },
};

static CONSOLE_STDOUT_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotSupported),
    touch: |_, _, _| Err(SysError::NotSupported),
    mkdir: |_, _, _| Err(SysError::NotSupported),
    symlink: |_, _, _| Err(SysError::NotSupported),
    unlink: |_, _| Err(SysError::NotSupported),
    rmdir: |_, _| Err(SysError::NotSupported),
    link: |_, _, _| Err(SysError::NotSupported),
    get_attr: console_get_attr,
    read_link: |_| Err(SysError::NotSymlink),
    open: |_| {
        Ok(OpenedFile {
            file_ops: &CONSOLE_STDOUT_FILE_OPS,
            prv: NilOpaque::new(),
        })
    },
};

static CONSOLE_STDIN_FILE_OPS: FileOps = FileOps {
    read: console_read,
    write: |_file, _buf| Err(SysError::NotSupported),
    seek: |_file, _pos| Err(SysError::NotSupported),
    iterate: |_file, _ctx| Err(SysError::NotSupported),
};

static CONSOLE_STDOUT_FILE_OPS: FileOps = FileOps {
    read: |_file, _buf| Err(SysError::NotSupported),
    write: console_write,
    seek: |_file, _pos| Err(SysError::NotSupported),
    iterate: |_file, _ctx| Err(SysError::NotSupported),
};

static CONSOLE_STDIN_PATHREF: Lazy<PathRef> = Lazy::new(|| {
    anony_new_inode(InodeType::Char, &CONSOLE_STDIN_INODE_OPS, NilOpaque::new())
        .expect("failed to create console stdin inode")
});

static CONSOLE_STDOUT_PATHREF: Lazy<PathRef> = Lazy::new(|| {
    anony_new_inode(InodeType::Char, &CONSOLE_STDOUT_INODE_OPS, NilOpaque::new())
        .expect("failed to create console stdout inode")
});

pub fn open_console_stdin() -> File {
    anony_open(&CONSOLE_STDIN_PATHREF).expect("failed to open console stdin")
}

pub fn open_console_stdout() -> File {
    anony_open(&CONSOLE_STDOUT_PATHREF).expect("failed to open console stdout")
}
