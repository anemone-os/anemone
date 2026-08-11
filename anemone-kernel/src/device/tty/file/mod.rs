use anemone_abi::tty::linux as abi;

use crate::{
    fs::FileMode,
    prelude::*,
    user_access::{UserReadPtr, UserWritePtr},
    utils::any_opaque::AnyOpaque,
};

use super::{
    TtyEndpoint, TtyWakeHandle, discipline::InputRead, pty::PtySlaveDescription, relation,
    terminal::TtyWinsize,
};

mod relation_ioctl;
mod termios;

#[derive(Opaque)]
pub(super) struct TtyFile {
    pub(super) endpoint: Arc<TtyEndpoint>,
    pub(super) wake: TtyWakeHandle,
    pty_description: Option<Arc<PtySlaveDescription>>,
}

/// Orders only a bounded Terminal snapshot or commit against a PTY episode's
/// final release. User memory access, drain waits, relation operations and
/// notification stay outside this window.
pub(super) trait TtyOperation {
    fn run_if_live(&self, operation: &mut dyn FnMut()) -> Result<(), SysError>;
}

fn run_terminal_operation<R>(
    operation: Option<&dyn TtyOperation>,
    commit: impl FnOnce() -> R,
) -> Result<R, SysError> {
    let mut commit = Some(commit);
    let mut result = None;
    let mut run = || {
        result = Some(commit
            .take()
            .expect("TTY bounded operation executed more than once")(
        ));
    };
    if let Some(operation) = operation {
        operation.run_if_live(&mut run)?;
    } else {
        run();
    }
    Ok(result.expect("TTY bounded operation was not executed"))
}

pub(super) fn terminal_file(endpoint: Arc<TtyEndpoint>, wake: TtyWakeHandle) -> TtyFile {
    TtyFile {
        endpoint,
        wake,
        pty_description: None,
    }
}

pub(super) fn opened_file(endpoint: Arc<TtyEndpoint>, wake: TtyWakeHandle) -> OpenedFile {
    OpenedFile::with_mode(
        &TTY_FILE_OPS,
        FileMode::STREAM,
        AnyOpaque::new(terminal_file(endpoint, wake)),
    )
}

pub(super) fn opened_pty_slave_file(
    endpoint: Arc<TtyEndpoint>,
    wake: TtyWakeHandle,
    description: Arc<PtySlaveDescription>,
) -> OpenedFile {
    OpenedFile::with_mode(
        &TTY_FILE_OPS,
        FileMode::STREAM,
        AnyOpaque::new(TtyFile {
            endpoint,
            wake,
            pty_description: Some(description),
        }),
    )
}

pub(super) fn pty_slave_description(file: &File) -> &PtySlaveDescription {
    tty_file(file)
        .pty_description
        .as_deref()
        .expect("PTY slave final release received a serial TTY file")
}

fn tty_file(file: &File) -> &TtyFile {
    file.private::<TtyFile>()
        .expect("TTY FileOps received a file without TTY private state")
}

fn tty_read(
    file: &File,
    _pos: &mut usize,
    buf: &mut [u8],
    ctx: FileIoCtx,
) -> Result<usize, SysError> {
    if buf.is_empty() {
        return Ok(0);
    }
    let tty = tty_file(file);
    loop {
        check_read_access(tty)?;
        if let Some(description) = &tty.pty_description {
            return description.read(&tty.endpoint.terminal, buf, ctx);
        }
        match tty.endpoint.terminal.read_input(buf) {
            InputRead::Bytes(count) => {
                if count != 0 {
                    tty.wake.wake();
                }
                return Ok(count);
            },
            InputRead::Eof => return Ok(0),
            InputRead::Empty => {
                if ctx.status_flags().contains(FileOpStatusFlags::NONBLOCK) {
                    return Err(SysError::Again);
                }
                tty.endpoint.terminal.wait_readable()?;
            },
        }
    }
}

fn check_read_access(tty: &TtyFile) -> Result<(), SysError> {
    loop {
        let caller = match crate::task::jobctl::TtyCaller::current_user_or_kernel()? {
            Some(caller) => caller,
            None => {
                // Kernel-internal FileOps users and KUnit workers have no user
                // session, so this terminal cannot be their controlling terminal.
                return Ok(());
            },
        };
        let Some(snapshot) = relation::endpoint_snapshot(&tty.endpoint) else {
            if caller.revalidate() {
                return Ok(());
            }
            continue;
        };
        if !snapshot.session().same_identity(caller.session()) {
            if caller.revalidate() && snapshot.is_current() {
                return Ok(());
            }
            continue;
        }
        let decision = caller.read_decision(snapshot.foreground());
        if !caller.revalidate() || !snapshot.is_current() {
            continue;
        }
        match decision {
            crate::task::jobctl::TtyReadDecision::Continue => return Ok(()),
            crate::task::jobctl::TtyReadDecision::Signal => {
                if caller.signal_process_group(crate::task::jobctl::TtyTerminalSignal::Input) {
                    return Err(SysError::RestartSyscall(RestartSyscall::Idempotent));
                }
            },
            crate::task::jobctl::TtyReadDecision::Eio => {
                tty.endpoint.terminal.record_background_read_eio();
                return Err(SysError::IO);
            },
        }
    }
}

fn tty_write(file: &File, _pos: &mut usize, buf: &[u8], ctx: FileIoCtx) -> Result<usize, SysError> {
    if buf.is_empty() {
        return Ok(0);
    }
    let tty = tty_file(file);
    if let Some(description) = &tty.pty_description {
        return description.write(&tty.endpoint.terminal, buf, ctx);
    }
    loop {
        let written = tty.endpoint.terminal.enqueue_output(buf);
        if written != 0 {
            tty.wake.wake();
            return Ok(written);
        }
        if ctx.status_flags().contains(FileOpStatusFlags::NONBLOCK) {
            return Err(SysError::Again);
        }
        tty.endpoint.terminal.wait_writable()?;
    }
}

fn tty_check_status_flags(_file: &File, flags: FileOpStatusFlags) -> Result<(), SysError> {
    if !(flags - FileOpStatusFlags::NONBLOCK).is_empty() {
        return Err(SysError::InvalidArgument);
    }
    Ok(())
}

fn tty_poll(file: &File, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
    let tty = tty_file(file);
    if let Some(description) = &tty.pty_description {
        Ok(description.poll(&tty.endpoint.terminal, request))
    } else {
        Ok(tty.endpoint.terminal.poll(request))
    }
}

pub(super) fn read_ioctl_value<T: zerocopy::FromBytes>(ctx: &IoctlCtx<'_>) -> Result<T, SysError> {
    ctx.uspace()
        .with_usp(|usp| UserReadPtr::<T>::try_new(VirtAddr::new(ctx.arg()), usp)?.read())
}

pub(super) fn write_ioctl_value<T: zerocopy::IntoBytes + zerocopy::Immutable>(
    ctx: &IoctlCtx<'_>,
    value: T,
) -> Result<(), SysError> {
    ctx.uspace().with_usp(|usp| {
        UserWritePtr::<T>::try_new(VirtAddr::new(ctx.arg()), usp)?.write(value)?;
        Ok(())
    })
}

fn tty_ioctl(file: &File, ctx: IoctlCtx<'_>) -> Result<u64, SysError> {
    let tty = tty_file(file);
    if let Some(description) = &tty.pty_description {
        if description.is_released_or_hung_up() {
            return if ctx.cmd() == abi::TIOCSPGRP {
                Err(SysError::UnsupportedIoctl)
            } else {
                Err(SysError::IO)
            };
        }
    }
    terminal_ioctl(
        tty,
        true,
        tty.pty_description
            .as_deref()
            .map(|description| description as &dyn TtyOperation),
        ctx,
    )
}

pub(super) fn terminal_ioctl(
    tty: &TtyFile,
    relation_operations: bool,
    operation: Option<&dyn TtyOperation>,
    ctx: IoctlCtx<'_>,
) -> Result<u64, SysError> {
    match ctx.cmd() {
        abi::TCGETS => {
            let (termios, line) = run_terminal_operation(operation, || {
                let (termios, _) = tty.endpoint.terminal.termios_snapshot();
                (termios, tty.endpoint.terminal.line_snapshot())
            })?;
            write_ioctl_value(&ctx, termios::project_termios(termios, line)?)?;
        },
        abi::TCSETS | abi::TCSETSW | abi::TCSETSF => {
            let candidate = read_ioctl_value::<abi::Termios>(&ctx)?;
            let mode = match ctx.cmd() {
                abi::TCSETS => termios::SetMode::Now,
                abi::TCSETSW => termios::SetMode::Drain,
                abi::TCSETSF => termios::SetMode::DrainFlush,
                _ => unreachable!(),
            };
            termios::set_termios(tty, operation, candidate, mode)?;
        },
        abi::TIOCGWINSZ => {
            let winsize = run_terminal_operation(operation, || tty.endpoint.terminal.winsize())?;
            write_ioctl_value(
                &ctx,
                abi::Winsize {
                    ws_row: winsize.rows,
                    ws_col: winsize.cols,
                    ws_xpixel: winsize.xpixel,
                    ws_ypixel: winsize.ypixel,
                },
            )?;
        },
        abi::TIOCSWINSZ => {
            let winsize = read_ioctl_value::<abi::Winsize>(&ctx)?;
            let changed = run_terminal_operation(operation, || {
                let winsize = TtyWinsize {
                    rows: winsize.ws_row,
                    cols: winsize.ws_col,
                    xpixel: winsize.ws_xpixel,
                    ypixel: winsize.ws_ypixel,
                };
                if operation.is_some() {
                    tty.endpoint.terminal.set_pty_winsize(winsize)
                } else {
                    tty.endpoint.terminal.set_winsize(winsize)
                }
            })?;
            if changed
                && !relation::signal_foreground(
                    &tty.endpoint,
                    crate::task::jobctl::TtyTerminalSignal::WindowChanged,
                )
            {
                tty.endpoint.terminal.record_no_foreground_winsize();
            }
            if changed {
                tty.wake.wake();
            }
        },
        abi::TIOCSCTTY if relation_operations => relation_ioctl::set_controlling_tty(tty, &ctx)?,
        abi::TIOCNOTTY if relation_operations => relation_ioctl::detach_controlling_tty(tty)?,
        abi::TIOCGSID if relation_operations => relation_ioctl::get_controlling_sid(tty, &ctx)?,
        abi::TIOCGPGRP if relation_operations => relation_ioctl::get_foreground_pgid(tty, &ctx)?,
        abi::TIOCSPGRP if relation_operations => relation_ioctl::set_foreground_pgid(tty, &ctx)?,
        _ => return Err(SysError::UnsupportedIoctl),
    }
    Ok(0)
}

static TTY_FILE_OPS: FileOps = FileOps {
    read: tty_read,
    write: tty_write,
    read_at: |_, _, _, _| Err(SysError::IllegalSeek),
    write_at: |_, _, _, _| Err(SysError::IllegalSeek),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: tty_check_status_flags,
    seek: |_, _, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: tty_poll,
    fcntl: None,
    ioctl: tty_ioctl,
};

#[cfg(feature = "kunit")]
use super::{
    port::{TtyLineSnapshot, TtyParity},
    terminal::{Terminal, TtyTermios},
};
#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::{device::tty::TtyWakeSource, fs::anony_open_with};

    fn line() -> TtyLineSnapshot {
        TtyLineSnapshot {
            baud: 115200,
            parity: TtyParity::None,
            data_bits: 8,
        }
    }

    fn no_worker_file(terminal: Arc<Terminal>) -> File {
        let source = Arc::new(TtyWakeSource {
            worker: SpinLock::new(None),
        });
        let endpoint = Arc::new(TtyEndpoint {
            terminal,
            wake_source: {
                let progress: Arc<dyn super::super::TtyProgress> = source.clone();
                Arc::downgrade(&progress)
            },
        });
        let wake = TtyWakeHandle { source };
        let placeholder = crate::device::console::open_console_stdin();
        anony_open_with(placeholder.path(), opened_file(endpoint, wake)).unwrap()
    }

    #[kunit]
    fn file_read_preserves_records_eof_nonblock_and_zero_length() {
        let terminal = Terminal::try_new(line()).unwrap();
        let file = no_worker_file(terminal.clone());
        let nonblocking = FileIoCtx::new(FileOpStatusFlags::NONBLOCK);
        let mut pos = 0;

        assert_eq!(tty_read(&file, &mut pos, &mut [], nonblocking), Ok(0));
        assert_eq!(
            tty_read(&file, &mut pos, &mut [0_u8; 1], nonblocking),
            Err(SysError::Again)
        );

        for byte in b"ab\ncd\n" {
            assert!(terminal.receive_rx_byte(*byte));
        }
        let mut first = [0_u8; 2];
        assert_eq!(tty_read(&file, &mut pos, &mut first, nonblocking), Ok(2));
        assert_eq!(&first, b"ab");
        let mut delimiter = [0_u8; 8];
        assert_eq!(
            tty_read(&file, &mut pos, &mut delimiter, nonblocking),
            Ok(1)
        );
        assert_eq!(&delimiter[..1], b"\n");
        assert_eq!(
            tty_read(&file, &mut pos, &mut delimiter, nonblocking),
            Ok(3)
        );
        assert_eq!(&delimiter[..3], b"cd\n");

        assert!(terminal.receive_rx_byte(TtyTermios::default().eof));
        assert_eq!(
            tty_read(&file, &mut pos, &mut delimiter, nonblocking),
            Ok(0)
        );
        assert_eq!(
            tty_read(&file, &mut pos, &mut delimiter, nonblocking),
            Err(SysError::Again)
        );
    }

    #[kunit]
    fn file_write_is_binary_reports_short_progress_and_zero_length() {
        let terminal = Terminal::try_new(line()).unwrap();
        let file = no_worker_file(terminal.clone());
        let nonblocking = FileIoCtx::new(FileOpStatusFlags::NONBLOCK);
        let mut pos = 0;

        assert_eq!(tty_write(&file, &mut pos, &[], nonblocking), Ok(0));
        assert_eq!(
            tty_write(&file, &mut pos, &[0xff, 0, b'\n'], nonblocking),
            Ok(3)
        );
        let mut binary = [0_u8; 4];
        assert_eq!(terminal.peek_output(&mut binary), 4);
        assert_eq!(binary, [0xff, 0, b'\r', b'\n']);
        terminal.consume_output(&binary);

        let fill = vec![b'x'; TTY_OUTPUT_CAPACITY_BYTES - 1];
        assert_eq!(terminal.enqueue_output(&fill), fill.len());
        assert_eq!(tty_write(&file, &mut pos, b"a\n", nonblocking), Ok(1));
        assert_eq!(
            tty_write(&file, &mut pos, b"\n", nonblocking),
            Err(SysError::Again)
        );
    }

    #[kunit]
    fn winsize_defaults_and_updates_without_foreground_relation() {
        let terminal = Terminal::try_new(line()).unwrap();
        assert_eq!(terminal.winsize(), TtyWinsize::default());
        terminal.set_winsize(TtyWinsize {
            rows: 40,
            cols: 100,
            xpixel: 1,
            ypixel: 2,
        });
        assert_eq!(terminal.winsize().rows, 40);
    }
}
