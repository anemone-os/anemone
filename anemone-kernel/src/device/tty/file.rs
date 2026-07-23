use anemone_abi::tty::linux as abi;

use crate::{
    fs::FileMode,
    prelude::*,
    user_access::{UserReadPtr, UserWritePtr},
    utils::any_opaque::AnyOpaque,
};

use super::{
    TtyEndpoint, TtyWakeHandle,
    discipline::InputRead,
    port::{TtyLineSnapshot, TtyParity},
    relation,
    terminal::{Terminal, TtyTermios, TtyWinsize},
};

#[derive(Opaque)]
struct TtyFile {
    endpoint: Arc<TtyEndpoint>,
    wake: TtyWakeHandle,
}

pub(super) fn opened_file(endpoint: Arc<TtyEndpoint>, wake: TtyWakeHandle) -> OpenedFile {
    OpenedFile::with_mode(
        &TTY_FILE_OPS,
        FileMode::STREAM,
        AnyOpaque::new(TtyFile { endpoint, wake }),
    )
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

fn tty_write(file: &File, _pos: &mut usize, buf: &[u8], ctx: FileIoCtx) -> Result<usize, SysError> {
    if buf.is_empty() {
        return Ok(0);
    }
    let tty = tty_file(file);
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
    Ok(tty_file(file).endpoint.terminal.poll(request))
}

fn read_ioctl_value<T: Copy>(ctx: &IoctlCtx<'_>) -> Result<T, SysError> {
    ctx.uspace()
        .with_usp(|usp| Ok(UserReadPtr::<T>::try_new(VirtAddr::new(ctx.arg()), usp)?.read()))
}

fn write_ioctl_value<T: Copy>(ctx: &IoctlCtx<'_>, value: T) -> Result<(), SysError> {
    ctx.uspace().with_usp(|usp| {
        UserWritePtr::<T>::try_new(VirtAddr::new(ctx.arg()), usp)?.write(value);
        Ok(())
    })
}

#[derive(Clone, Copy)]
enum SetMode {
    Now,
    Drain,
    DrainFlush,
}

fn tty_ioctl(file: &File, ctx: IoctlCtx<'_>) -> Result<u64, SysError> {
    let tty = tty_file(file);
    match ctx.cmd() {
        abi::TCGETS => {
            let (termios, _) = tty.endpoint.terminal.termios_snapshot();
            write_ioctl_value(
                &ctx,
                project_termios(termios, tty.endpoint.terminal.line_snapshot())?,
            )?;
        },
        abi::TCSETS | abi::TCSETSW | abi::TCSETSF => {
            let candidate = read_ioctl_value::<abi::Termios>(&ctx)?;
            let mode = match ctx.cmd() {
                abi::TCSETS => SetMode::Now,
                abi::TCSETSW => SetMode::Drain,
                abi::TCSETSF => SetMode::DrainFlush,
                _ => unreachable!(),
            };
            set_termios(tty, candidate, mode)?;
        },
        abi::TIOCGWINSZ => {
            let winsize = tty.endpoint.terminal.winsize();
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
            tty.endpoint.terminal.set_winsize(TtyWinsize {
                rows: winsize.ws_row,
                cols: winsize.ws_col,
                xpixel: winsize.ws_xpixel,
                ypixel: winsize.ws_ypixel,
            });
        },
        abi::TIOCSCTTY => set_controlling_tty(tty, &ctx)?,
        abi::TIOCNOTTY => detach_controlling_tty(tty)?,
        abi::TIOCGSID => get_controlling_sid(tty, &ctx)?,
        abi::TIOCGPGRP => get_foreground_pgid(tty, &ctx)?,
        abi::TIOCSPGRP => set_foreground_pgid(tty, &ctx)?,
        _ => return Err(SysError::UnsupportedIoctl),
    }
    Ok(0)
}

fn current_tty_caller() -> Result<crate::task::jobctl::TtyCaller, SysError> {
    crate::task::jobctl::TtyCaller::current().map_err(|_| SysError::UnsupportedIoctl)
}

fn controlling_snapshot(
    tty: &TtyFile,
    caller: &crate::task::jobctl::TtyCaller,
) -> Result<relation::RelationSnapshot, SysError> {
    let snapshot = relation::endpoint_snapshot(&tty.endpoint).ok_or(SysError::UnsupportedIoctl)?;
    if !snapshot.session().same_identity(caller.session()) {
        return Err(SysError::UnsupportedIoctl);
    }
    Ok(snapshot)
}

fn set_controlling_tty(tty: &TtyFile, ctx: &IoctlCtx<'_>) -> Result<(), SysError> {
    // Privileged stealing (`arg=1`) is outside the accepted first-version ABI;
    // rejecting every nonzero value avoids a silent success with weaker effect.
    if ctx.arg() != 0 {
        knoticeln!(
            "TTY: rejecting unsupported TIOCSCTTY steal argument {}",
            ctx.arg()
        );
        return Err(SysError::PermissionDenied);
    }
    let caller = current_tty_caller()?;
    relation::acquire(&tty.endpoint, &caller, ctx.target_access().can_read())
}

fn detach_controlling_tty(tty: &TtyFile) -> Result<(), SysError> {
    relation::detach(&tty.endpoint, &current_tty_caller()?)
}

fn get_controlling_sid(tty: &TtyFile, ctx: &IoctlCtx<'_>) -> Result<(), SysError> {
    loop {
        let caller = current_tty_caller()?;
        let snapshot = controlling_snapshot(tty, &caller)?;
        if caller.revalidate() && snapshot.is_current() {
            return write_ioctl_value(ctx, snapshot.session().sid().get() as i32);
        }
    }
}

fn get_foreground_pgid(tty: &TtyFile, ctx: &IoctlCtx<'_>) -> Result<(), SysError> {
    loop {
        let caller = current_tty_caller()?;
        let snapshot = controlling_snapshot(tty, &caller)?;
        let pgid = snapshot
            .foreground()
            .map_or(0, |foreground| foreground.pgid().get() as i32);
        if caller.revalidate() && snapshot.is_current() {
            return write_ioctl_value(ctx, pgid);
        }
    }
}

fn set_foreground_pgid(tty: &TtyFile, ctx: &IoctlCtx<'_>) -> Result<(), SysError> {
    loop {
        let caller = current_tty_caller()?;
        let snapshot = controlling_snapshot(tty, &caller)?;

        // POSIX terminal access checks precede touching the user candidate. An
        // actionable background SIGTTOU therefore has no foreground mutation
        // and restarts this idempotent ioctl only after signal handling.
        if matches!(
            caller.sigttou_decision(snapshot.foreground()),
            crate::task::jobctl::TtySigttouDecision::Signal
        ) {
            if !caller.revalidate() || !snapshot.is_current() {
                continue;
            }
            if caller.signal_process_group_sigttou() {
                return Err(SysError::RestartSyscall(RestartSyscall::Idempotent));
            }
            continue;
        }

        let raw_pgid = read_ioctl_value::<i32>(ctx)?;
        if raw_pgid < 0 {
            return Err(SysError::InvalidArgument);
        }
        let foreground = caller.resolve_process_group(Tid::new(raw_pgid as u32))?;
        if !caller.revalidate()
            || !foreground.is_live_in(caller.session())
            || !snapshot.is_current()
        {
            continue;
        }
        let pgid = foreground.pgid();
        if relation::commit_foreground(&snapshot, foreground) {
            kinfoln!(
                "TTY: foreground commit sid={} pgid={}",
                caller.session().sid(),
                pgid
            );
            return Ok(());
        }
    }
}

fn set_termios(tty: &TtyFile, candidate: abi::Termios, mode: SetMode) -> Result<(), SysError> {
    loop {
        let (current, generation) = tty.endpoint.terminal.termios_snapshot();
        let updated = validate_termios(candidate, current, tty.endpoint.terminal.line_snapshot())?;
        let drained_output_generation = if !matches!(mode, SetMode::Now) {
            tty.endpoint.terminal.request_drain_check();
            tty.wake.wake();
            Some(tty.endpoint.terminal.wait_drain_complete()?)
        } else {
            None
        };
        if tty.endpoint.terminal.commit_termios_if_generation(
            generation,
            drained_output_generation,
            updated,
            matches!(mode, SetMode::DrainFlush),
        ) {
            tty.wake.wake();
            return Ok(());
        }
    }
}

fn project_termios(termios: TtyTermios, line: TtyLineSnapshot) -> Result<abi::Termios, SysError> {
    let mut result = abi::Termios {
        c_iflag: if termios.icrnl { abi::ICRNL } else { 0 },
        c_oflag: (if termios.opost { abi::OPOST } else { 0 })
            | (if termios.onlcr { abi::ONLCR } else { 0 }),
        c_cflag: baud_flag(line.baud).ok_or(SysError::InvalidArgument)?
            | data_bits_flag(line.data_bits).ok_or(SysError::InvalidArgument)?
            | abi::CREAD
            | abi::CLOCAL,
        c_lflag: 0,
        c_line: 0,
        c_cc: [0; abi::NCCS],
    };
    match line.parity {
        TtyParity::None => {},
        TtyParity::Even => result.c_cflag |= abi::PARENB,
        TtyParity::Odd => result.c_cflag |= abi::PARENB | abi::PARODD,
    }
    for (enabled, flag) in [
        (termios.isig, abi::ISIG),
        (termios.icanon, abi::ICANON),
        (termios.echo, abi::ECHO),
        (termios.echoe, abi::ECHOE),
        (termios.echok, abi::ECHOK),
        (termios.echonl, abi::ECHONL),
    ] {
        if enabled {
            result.c_lflag |= flag;
        }
    }
    for (index, value) in control_chars(termios) {
        result.c_cc[index] = value;
    }
    Ok(result)
}

fn validate_termios(
    candidate: abi::Termios,
    current: TtyTermios,
    line: TtyLineSnapshot,
) -> Result<TtyTermios, SysError> {
    let projected = project_termios(current, line)?;
    let allowed_iflag = abi::ICRNL;
    let allowed_oflag = abi::OPOST | abi::ONLCR;
    let allowed_lflag = abi::ISIG | abi::ICANON | abi::ECHO | abi::ECHOE | abi::ECHOK | abi::ECHONL;
    if candidate.c_iflag & !allowed_iflag != projected.c_iflag & !allowed_iflag
        || candidate.c_oflag & !allowed_oflag != projected.c_oflag & !allowed_oflag
        || candidate.c_lflag & !allowed_lflag != projected.c_lflag & !allowed_lflag
        || candidate.c_cflag != projected.c_cflag
        || candidate.c_line != projected.c_line
    {
        return Err(SysError::InvalidArgument);
    }
    let allowed_cc = [
        abi::VINTR,
        abi::VQUIT,
        abi::VERASE,
        abi::VKILL,
        abi::VEOF,
        abi::VTIME,
        abi::VMIN,
        abi::VSTART,
        abi::VSTOP,
        abi::VSUSP,
        abi::VREPRINT,
        abi::VDISCARD,
        abi::VWERASE,
        abi::VLNEXT,
    ];
    for index in 0..abi::NCCS {
        if !allowed_cc.contains(&index) && candidate.c_cc[index] != projected.c_cc[index] {
            return Err(SysError::InvalidArgument);
        }
    }
    let canonical = candidate.c_lflag & abi::ICANON != 0;
    if !canonical && (candidate.c_cc[abi::VMIN] != 1 || candidate.c_cc[abi::VTIME] != 0) {
        return Err(SysError::InvalidArgument);
    }
    Ok(TtyTermios {
        icrnl: candidate.c_iflag & abi::ICRNL != 0,
        opost: candidate.c_oflag & abi::OPOST != 0,
        onlcr: candidate.c_oflag & abi::ONLCR != 0,
        icanon: candidate.c_lflag & abi::ICANON != 0,
        isig: candidate.c_lflag & abi::ISIG != 0,
        echo: candidate.c_lflag & abi::ECHO != 0,
        echoe: candidate.c_lflag & abi::ECHOE != 0,
        echok: candidate.c_lflag & abi::ECHOK != 0,
        echonl: candidate.c_lflag & abi::ECHONL != 0,
        intr: candidate.c_cc[abi::VINTR],
        quit: candidate.c_cc[abi::VQUIT],
        erase: candidate.c_cc[abi::VERASE],
        kill: candidate.c_cc[abi::VKILL],
        eof: candidate.c_cc[abi::VEOF],
        susp: candidate.c_cc[abi::VSUSP],
        start: candidate.c_cc[abi::VSTART],
        stop: candidate.c_cc[abi::VSTOP],
        reprint: candidate.c_cc[abi::VREPRINT],
        discard: candidate.c_cc[abi::VDISCARD],
        werase: candidate.c_cc[abi::VWERASE],
        lnext: candidate.c_cc[abi::VLNEXT],
        vmin: candidate.c_cc[abi::VMIN],
        vtime: candidate.c_cc[abi::VTIME],
    })
}

fn control_chars(termios: TtyTermios) -> [(usize, u8); 14] {
    [
        (abi::VINTR, termios.intr),
        (abi::VQUIT, termios.quit),
        (abi::VERASE, termios.erase),
        (abi::VKILL, termios.kill),
        (abi::VEOF, termios.eof),
        (abi::VTIME, termios.vtime),
        (abi::VMIN, termios.vmin),
        (abi::VSTART, termios.start),
        (abi::VSTOP, termios.stop),
        (abi::VSUSP, termios.susp),
        (abi::VREPRINT, termios.reprint),
        (abi::VDISCARD, termios.discard),
        (abi::VWERASE, termios.werase),
        (abi::VLNEXT, termios.lnext),
    ]
}

fn data_bits_flag(bits: u8) -> Option<u32> {
    Some(match bits {
        5 => abi::CS5,
        6 => abi::CS6,
        7 => abi::CS7,
        8 => abi::CS8,
        _ => return None,
    })
}

fn baud_flag(baud: u32) -> Option<u32> {
    Some(match baud {
        0 => abi::B0,
        50 => abi::B50,
        75 => abi::B75,
        110 => abi::B110,
        134 => abi::B134,
        150 => abi::B150,
        200 => abi::B200,
        300 => abi::B300,
        600 => abi::B600,
        1200 => abi::B1200,
        1800 => abi::B1800,
        2400 => abi::B2400,
        4800 => abi::B4800,
        9600 => abi::B9600,
        19200 => abi::B19200,
        38400 => abi::B38400,
        57600 => abi::B57600,
        115200 => abi::B115200,
        230400 => abi::B230400,
        460800 => abi::B460800,
        500000 => abi::B500000,
        576000 => abi::B576000,
        921600 => abi::B921600,
        1_000_000 => abi::B1000000,
        1_152_000 => abi::B1152000,
        1_500_000 => abi::B1500000,
        2_000_000 => abi::B2000000,
        2_500_000 => abi::B2500000,
        3_000_000 => abi::B3000000,
        3_500_000 => abi::B3500000,
        4_000_000 => abi::B4000000,
        _ => return None,
    })
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
mod kunits {
    use super::*;
    use crate::{
        device::tty::{
            TtyPort, TtyPortAttachment, TtyPortId, TtyWakeSource, attach_unpublished_port,
        },
        fs::anony_open_with,
    };

    struct DrainPort {
        id: TtyPortId,
        submitted: AtomicUsize,
    }

    impl DrainPort {
        fn new(id: &str) -> Arc<Self> {
            Arc::new(Self {
                id: TtyPortId::try_from(id).unwrap(),
                submitted: AtomicUsize::new(0),
            })
        }
    }

    impl TtyPort for DrainPort {
        fn id(&self) -> &TtyPortId {
            &self.id
        }

        fn line_snapshot(&self) -> TtyLineSnapshot {
            line()
        }

        fn rx_pending(&self) -> bool {
            false
        }

        fn dequeue_rx(&self, _dst: &mut [u8]) -> usize {
            0
        }

        fn submit_tx(&self, src: &[u8]) -> usize {
            self.submitted.fetch_add(src.len(), Ordering::Relaxed);
            src.len()
        }

        fn tx_idle(&self) -> bool {
            true
        }
    }

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
            port: DrainPort::new("/kunit/tty/file-no-worker"),
            terminal,
            wake_source: Arc::downgrade(&source),
        });
        let wake = TtyWakeHandle { source };
        let placeholder = crate::device::console::open_console_stdin();
        anony_open_with(placeholder.path(), opened_file(endpoint, wake)).unwrap()
    }

    fn attachment_file(attachment: &TtyPortAttachment) -> TtyFile {
        TtyFile {
            endpoint: attachment.endpoint.clone(),
            wake: TtyWakeHandle {
                source: attachment.wake_source.as_ref().unwrap().clone(),
            },
        }
    }

    #[kunit]
    fn file_read_preserves_records_eof_nonblock_and_zero_length() {
        let terminal = Terminal::try_new(line()).unwrap();
        let file = no_worker_file(terminal.clone());
        let blocking = FileIoCtx::new(FileOpStatusFlags::empty());
        let nonblocking = FileIoCtx::new(FileOpStatusFlags::NONBLOCK);
        let mut pos = 0;

        assert_eq!(tty_read(&file, &mut pos, &mut [], blocking), Ok(0));
        assert_eq!(
            tty_read(&file, &mut pos, &mut [0_u8; 1], nonblocking),
            Err(SysError::Again)
        );

        for byte in b"ab\ncd\n" {
            assert!(terminal.receive_rx_byte(*byte));
        }
        let mut first = [0_u8; 2];
        assert_eq!(tty_read(&file, &mut pos, &mut first, blocking), Ok(2));
        assert_eq!(&first, b"ab");
        let mut delimiter = [0_u8; 8];
        assert_eq!(tty_read(&file, &mut pos, &mut delimiter, blocking), Ok(1));
        assert_eq!(&delimiter[..1], b"\n");
        assert_eq!(tty_read(&file, &mut pos, &mut delimiter, blocking), Ok(3));
        assert_eq!(&delimiter[..3], b"cd\n");

        assert!(terminal.receive_rx_byte(TtyTermios::default().eof));
        assert_eq!(tty_read(&file, &mut pos, &mut delimiter, blocking), Ok(0));
        assert_eq!(
            tty_read(&file, &mut pos, &mut delimiter, nonblocking),
            Err(SysError::Again)
        );
    }

    #[kunit]
    fn file_write_is_binary_reports_short_progress_and_zero_length() {
        let terminal = Terminal::try_new(line()).unwrap();
        let file = no_worker_file(terminal.clone());
        let blocking = FileIoCtx::new(FileOpStatusFlags::empty());
        let nonblocking = FileIoCtx::new(FileOpStatusFlags::NONBLOCK);
        let mut pos = 0;

        assert_eq!(tty_write(&file, &mut pos, &[], blocking), Ok(0));
        assert_eq!(
            tty_write(&file, &mut pos, &[0xff, 0, b'\n'], blocking),
            Ok(3)
        );
        let mut binary = [0_u8; 4];
        assert_eq!(terminal.peek_output(&mut binary), 4);
        assert_eq!(binary, [0xff, 0, b'\r', b'\n']);
        terminal.consume_output(&binary);

        let fill = vec![b'x'; TTY_OUTPUT_CAPACITY_BYTES - 1];
        assert_eq!(terminal.enqueue_output(&fill), fill.len());
        assert_eq!(tty_write(&file, &mut pos, b"a\n", blocking), Ok(1));
        assert_eq!(
            tty_write(&file, &mut pos, b"\n", nonblocking),
            Err(SysError::Again)
        );
    }

    #[kunit]
    fn asm_generic_projection_and_validation_are_atomic() {
        let current = TtyTermios::default();
        let raw = project_termios(current, line()).unwrap();
        assert_eq!(raw.c_cflag & abi::CBAUD, abi::B115200);
        assert_eq!(raw.c_cflag & abi::CSIZE, abi::CS8);
        assert_eq!(raw.c_cc[abi::VMIN], 1);
        assert_eq!(raw.c_cc[abi::VTIME], 0);

        let mut candidate = raw;
        candidate.c_lflag &= !(abi::ICANON | abi::ECHO);
        let updated = validate_termios(candidate, current, line()).unwrap();
        assert!(!updated.icanon);
        assert!(!updated.echo);

        let mut disabled = raw;
        for index in [
            abi::VINTR,
            abi::VQUIT,
            abi::VERASE,
            abi::VKILL,
            abi::VEOF,
            abi::VSUSP,
        ] {
            disabled.c_cc[index] = 0;
        }
        let disabled = validate_termios(disabled, current, line()).unwrap();
        let projected_disabled = project_termios(disabled, line()).unwrap();
        for index in [
            abi::VINTR,
            abi::VQUIT,
            abi::VERASE,
            abi::VKILL,
            abi::VEOF,
            abi::VSUSP,
        ] {
            assert_eq!(projected_disabled.c_cc[index], 0);
        }

        candidate.c_iflag |= 0x400;
        assert_eq!(
            validate_termios(candidate, current, line()),
            Err(SysError::InvalidArgument)
        );
        assert!(current.icanon);

        let mut canonical_cc = raw;
        canonical_cc.c_cc[abi::VMIN] = 7;
        canonical_cc.c_cc[abi::VTIME] = 9;
        let canonical = validate_termios(canonical_cc, current, line()).unwrap();
        assert_eq!(canonical.vmin, 7);
        assert_eq!(canonical.vtime, 9);
        canonical_cc.c_lflag &= !abi::ICANON;
        assert_eq!(
            validate_termios(canonical_cc, current, line()),
            Err(SysError::InvalidArgument)
        );
    }

    #[kunit]
    fn set_modes_commit_after_drain_and_flush_only_for_tcsetsf() {
        let port = DrainPort::new("/kunit/tty/file-set-modes");
        let (attachment, _) = attach_unpublished_port(port.clone()).unwrap();
        let tty = attachment_file(&attachment);
        let mut candidate = project_termios(TtyTermios::default(), line()).unwrap();
        candidate.c_lflag &= !abi::ECHO;

        assert_eq!(tty.endpoint.terminal.enqueue_output(b"queued\n"), 7);
        set_termios(&tty, candidate, SetMode::Now).unwrap();
        assert!(!tty.endpoint.terminal.termios_snapshot().0.echo);
        assert!(tty.endpoint.terminal.output_pending());
        assert_eq!(port.submitted.load(Ordering::Relaxed), 0);

        candidate.c_lflag |= abi::ECHO;
        set_termios(&tty, candidate, SetMode::Drain).unwrap();
        assert!(tty.endpoint.terminal.termios_snapshot().0.echo);
        assert_eq!(port.submitted.load(Ordering::Relaxed), 8);

        for byte in b"unread\n" {
            assert!(tty.endpoint.terminal.receive_rx_byte(*byte));
        }
        candidate.c_lflag &= !abi::ECHO;
        set_termios(&tty, candidate, SetMode::DrainFlush).unwrap();
        assert!(!tty.endpoint.terminal.termios_snapshot().0.echo);
        assert_eq!(
            tty.endpoint.terminal.read_input(&mut [0_u8; 8]),
            InputRead::Empty
        );
        attachment.abort();
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
