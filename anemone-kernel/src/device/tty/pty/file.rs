use crate::{
    fs::{PollRegisterResult, PollRequest},
    prelude::*,
    task::files::OpenedFileFinalReleaseCtx,
};

use super::{
    super::{
        discipline::TtySignalControl,
        file::{self as tty_file, TtyFile},
        relation,
    },
    PtyMasterDescription,
};

#[derive(Opaque)]
pub(super) struct PtyMasterFile {
    pub(super) terminal_file: TtyFile,
    pub(super) description: Arc<PtyMasterDescription>,
}

pub(super) fn master_file(file: &File) -> &PtyMasterFile {
    file.private::<PtyMasterFile>()
        .expect("PTY master FileOps received non-master private state")
}

fn master_read(
    file: &File,
    _pos: &mut usize,
    dst: &mut [u8],
    ctx: FileIoCtx,
) -> Result<usize, SysError> {
    master_file(file).description.read(dst, ctx)
}

fn master_write(
    file: &File,
    _pos: &mut usize,
    source: &[u8],
    ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let master = master_file(file);
    master.description.write(source, ctx, |signal| {
        let signal = match signal {
            TtySignalControl::Interrupt => crate::task::jobctl::TtyTerminalSignal::Interrupt,
            TtySignalControl::Quit => crate::task::jobctl::TtyTerminalSignal::Quit,
            TtySignalControl::Suspend => crate::task::jobctl::TtyTerminalSignal::Suspend,
        };
        relation::signal_foreground(&master.terminal_file.endpoint, signal)
    })
}

fn master_poll(file: &File, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
    master_file(file).description.poll(request)
}

fn master_ioctl(file: &File, ctx: IoctlCtx<'_>) -> Result<u64, SysError> {
    let master = master_file(file);
    if !master.description.is_live() {
        return Err(SysError::IO);
    }
    match ctx.cmd() {
        anemone_abi::tty::linux::TIOCGPTN => {
            tty_file::write_ioctl_value(&ctx, master.description.index())?;
            return Ok(0);
        },
        anemone_abi::tty::linux::TIOCSPTLCK => {
            let locked = tty_file::read_ioctl_value::<i32>(&ctx)? != 0;
            master.description.set_slave_locked(locked)?;
            return Ok(0);
        },
        anemone_abi::tty::linux::TIOCGPTPEER => {
            return super::open_peer(
                &master.description,
                master.terminal_file.endpoint.clone(),
                &ctx,
            );
        },
        _ => {},
    }
    // Master and slave observe the same Terminal truth, but the master never
    // forwards controlling-terminal operations to the relation owner.
    let result = tty_file::terminal_ioctl(
        &master.terminal_file,
        false,
        Some(master.description.as_ref()),
        ctx,
    );
    result
}

pub(super) fn master_final_release(ctx: OpenedFileFinalReleaseCtx<'_>) {
    let master = master_file(ctx.file);
    master.description.release();
    let base = *master
        .description
        .base_final_release
        .lock()
        .as_ref()
        .expect("live PTY master missing static final-release composition");
    if let Some(base) = base {
        base(ctx);
    }
}

pub(super) fn slave_final_release(ctx: OpenedFileFinalReleaseCtx<'_>) {
    let description = tty_file::pty_slave_description(ctx.file);
    description.release();
    let base = *description
        .base_final_release
        .lock()
        .as_ref()
        .expect("live PTY slave missing static final-release composition");
    if let Some(base) = base {
        base(ctx);
    }
}

fn check_status_flags(_file: &File, flags: FileOpStatusFlags) -> Result<(), SysError> {
    if !(flags - FileOpStatusFlags::NONBLOCK).is_empty() {
        return Err(SysError::InvalidArgument);
    }
    Ok(())
}

pub(super) static PTY_MASTER_FILE_OPS: FileOps = FileOps {
    read: master_read,
    write: master_write,
    read_at: |_, _, _, _| Err(SysError::IllegalSeek),
    write_at: |_, _, _, _| Err(SysError::IllegalSeek),
    read_user_at: None,
    write_user_at: None,
    check_status_flags,
    seek: |_, _, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: master_poll,
    fcntl: None,
    ioctl: master_ioctl,
};
