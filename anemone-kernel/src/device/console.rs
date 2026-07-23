//! Console subsystem.
//!
//! Here lies /dev/console.

use crate::{
    debug::printk::KERNEL_LOG,
    fs::devfs::{DevfsNodeAttr, DevfsNodeOps, DevfsPublish, publish as devfs_publish},
    prelude::*,
    utils::{any_opaque::NilOpaque, identity::AnyIdentity},
};

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
    terminal_identity: Option<ConsoleTerminalIdentity>,
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
    /// Authoritative stable boot-selection snapshot. This is not a diagnostic
    /// cache: TTY consumes it after Late init without copying console policy.
    selected_terminal: MonoOnce<Option<ConsoleTerminalIdentity>>,
}

impl ConsoleSubSys {
    fn new() -> Self {
        Self {
            consoles: SpinLock::new(Vec::new()),
            selected_terminal: unsafe { MonoOnce::new() },
        }
    }
}

static SUBSYS: Lazy<ConsoleSubSys> = Lazy::new(|| ConsoleSubSys::new());

/// Register a console.
pub fn register_console(ops: Arc<dyn Console>, flags: ConsoleFlags) {
    register_console_with_terminal_identity(ops, flags, None);
}

/// Immutable serial-terminal identity carried by a console registration.
///
/// The value does not make console depend on TTY internals. Console owns only
/// the selected registration; TTY later revalidates this opaque identity
/// against its own port registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConsoleTerminalIdentity(AnyIdentity);

impl ConsoleTerminalIdentity {
    pub(crate) fn try_from_str(value: &str) -> Result<Self, SysError> {
        AnyIdentity::try_from(value)
            .map(Self)
            .map_err(|_| SysError::NameTooLong)
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

pub(crate) fn register_console_with_terminal_identity(
    ops: Arc<dyn Console>,
    mut flags: ConsoleFlags,
    terminal_identity: Option<ConsoleTerminalIdentity>,
) {
    if flags.contains(ConsoleFlags::EARLY) {
        // If the console is registered with EARLY flag, it will be automatically
        // enabled during early boot.
        flags |= ConsoleFlags::ENABLED;
    }

    if flags.contains(ConsoleFlags::REPLAY) {
        let it = KERNEL_LOG.iter_weak();
        for record in it {
            if !record.level.should_print() {
                continue;
            }

            let full_msg_str =
                core::str::from_utf8(&record.msg[..record.len]).unwrap_or("[Invalid UTF-8]");
            ops.output(full_msg_str);
        }
    }

    SUBSYS.consoles.lock_irqsave().push(ConsoleDesc {
        ops,
        flags,
        terminal_identity,
    });
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
    let selected_terminal = consoles
        .iter()
        .find(|desc| desc.enabled())
        .and_then(|desc| desc.terminal_identity.clone());
    SUBSYS.selected_terminal.init(|slot| {
        slot.write(selected_terminal);
    });
    drop(consoles);

    if !has_normal_con {
        kwarningln!("no normal console registered, only early consoles are available");
    } else {
        kinfoln!("normal console(s) registered, early consoles have been unregistered");
    }
}

pub(crate) fn selected_terminal_identity() -> Option<ConsoleTerminalIdentity> {
    SUBSYS.selected_terminal.get().clone()
}

fn console_read(
    _file: &File,
    _pos: &mut usize,
    _buf: &mut [u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    // currently no-op. always return EOF.
    Ok(0)
}

fn console_write(
    _file: &File,
    _pos: &mut usize,
    buf: &[u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
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
        uid: inode.uid(),
        gid: inode.gid(),
        rdev: DeviceId::None,
        size: inode.size(),
        atime: inode.atime(),
        mtime: inode.mtime(),
        ctime: inode.ctime(),
    })
}

fn console_devnum() -> CharDevNum {
    CharDevNum::new(
        MajorNum::new(devnum::char::major::TTY_AUX),
        MinorNum::new(devnum::char::minor::CONSOLE),
    )
}

fn console_devfs_get_attr(inode: &InodeRef, attr: DevfsNodeAttr) -> Result<InodeStat, SysError> {
    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: InodeMode::new(attr.ty, inode.perm()),
        nlink: inode.nlink(),
        uid: inode.uid(),
        gid: inode.gid(),
        rdev: attr.rdev,
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
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    link: |_, _, _| Err(SysError::NotSupported),
    truncate: |_, _| Err(SysError::NotSupported),
    get_attr: console_get_attr,
    read_link: |_| Err(SysError::NotSymlink),
    open: |_| Ok(OpenedFile::new(&CONSOLE_STDIN_FILE_OPS, NilOpaque::new())),
};

static CONSOLE_STDOUT_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotSupported),
    touch: |_, _, _| Err(SysError::NotSupported),
    mkdir: |_, _, _| Err(SysError::NotSupported),
    symlink: |_, _, _| Err(SysError::NotSupported),
    unlink: |_, _| Err(SysError::NotSupported),
    rmdir: |_, _| Err(SysError::NotSupported),
    link: |_, _, _| Err(SysError::NotSupported),
    truncate: |_, _| Err(SysError::NotSupported),
    get_attr: console_get_attr,
    read_link: |_| Err(SysError::NotSymlink),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: |_| Ok(OpenedFile::new(&CONSOLE_STDOUT_FILE_OPS, NilOpaque::new())),
};

static CONSOLE_STDIN_FILE_OPS: FileOps = FileOps {
    read: console_read,
    write: |_, _, _, _| Err(SysError::NotSupported),
    read_at: |_, _, _, _| Err(SysError::IllegalSeek),
    write_at: |_, _, _, _| Err(SysError::NotSupported),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: |_, _, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, _| {
        kerrln!("console stdin poll is not implemented yet");
        Err(SysError::NotYetImplemented)
    },
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

static CONSOLE_STDOUT_FILE_OPS: FileOps = FileOps {
    read: |_, _, _, _| Err(SysError::NotSupported),
    write: console_write,
    read_at: |_, _, _, _| Err(SysError::NotSupported),
    write_at: |_, _, _, _| Err(SysError::IllegalSeek),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: |_, _, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, _| {
        kerrln!("console stdout poll is not implemented yet");
        Err(SysError::NotYetImplemented)
    },
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

static CONSOLE_DEVFS_FILE_OPS: FileOps = FileOps {
    read: console_read,
    write: console_write,
    read_at: |_, _, _, _| Err(SysError::IllegalSeek),
    write_at: |_, _, _, _| Err(SysError::IllegalSeek),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: |_, _, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, _| Err(SysError::NotYetImplemented),
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

struct ConsoleDevfsNodeOps;

impl DevfsNodeOps for ConsoleDevfsNodeOps {
    fn open(&self, _inode: &InodeRef) -> Result<OpenedFile, SysError> {
        Ok(OpenedFile::new(&CONSOLE_DEVFS_FILE_OPS, NilOpaque::new()))
    }

    fn get_attr(&self, inode: &InodeRef, attr: DevfsNodeAttr) -> Result<InodeStat, SysError> {
        console_devfs_get_attr(inode, attr)
    }
}

pub(crate) struct ConsoleDevfsPublication(DevfsPublish);

impl ConsoleDevfsPublication {
    pub(crate) fn publish(self) -> Result<Ino, SysError> {
        devfs_publish(self.0)
    }
}

/// Prepare the permanent console-owned `/dev/console` node after all Late
/// device activation has completed. The descriptor retains console's EOF-input,
/// UTF-8 output, and non-TTY ioctl behavior; publication happens only after
/// the boot coordinator has also prepared every TTY endpoint and stdio file.
pub(crate) fn prepare_devfs() -> Result<ConsoleDevfsPublication, SysError> {
    let ops: Arc<dyn DevfsNodeOps> =
        Arc::try_new(ConsoleDevfsNodeOps).map_err(|_| SysError::OutOfMemory)?;
    let mut name = String::new();
    name.try_reserve_exact("console".len())
        .map_err(|_| SysError::OutOfMemory)?;
    name.push_str("console");
    Ok(ConsoleDevfsPublication(DevfsPublish {
        name,
        attr: DevfsNodeAttr {
            ty: InodeType::Char,
            perm: InodePerm::all_rw(),
            rdev: DeviceId::Char(console_devnum()),
        },
        ops,
    }))
}

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
