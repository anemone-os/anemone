use anemone_abi::tty::linux as abi;

use crate::prelude::*;

use super::{
    super::{
        port::{TtyLineSnapshot, TtyParity},
        terminal::TtyTermios,
    },
    TtyFile, TtyOperation, run_terminal_operation,
};

#[derive(Clone, Copy)]
pub(super) enum SetMode {
    Now,
    Drain,
    DrainFlush,
}

pub(super) fn set_termios(
    tty: &TtyFile,
    operation: Option<&dyn TtyOperation>,
    candidate: abi::Termios,
    mode: SetMode,
) -> Result<(), SysError> {
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
        if run_terminal_operation(operation, || {
            if operation.is_some() {
                tty.endpoint.terminal.commit_pty_termios_if_generation(
                    generation,
                    drained_output_generation,
                    updated,
                    matches!(mode, SetMode::DrainFlush),
                )
            } else {
                tty.endpoint.terminal.commit_termios_if_generation(
                    generation,
                    drained_output_generation,
                    updated,
                    matches!(mode, SetMode::DrainFlush),
                )
            }
        })? {
            tty.wake.wake();
            return Ok(());
        }
    }
}

pub(super) fn project_termios(
    termios: TtyTermios,
    line: TtyLineSnapshot,
) -> Result<abi::Termios, SysError> {
    let mut result = abi::Termios {
        c_iflag: 0,
        c_oflag: (if termios.opost { abi::OPOST } else { 0 })
            | (if termios.onlcr { abi::ONLCR } else { 0 })
            | (if termios.tab3 { abi::TAB3 } else { abi::TAB0 }),
        c_cflag: baud_flag(line.baud).ok_or(SysError::InvalidArgument)?
            | data_bits_flag(line.data_bits).ok_or(SysError::InvalidArgument)?
            | abi::CREAD
            | abi::CLOCAL,
        c_lflag: 0,
        c_line: 0,
        c_cc: [0; abi::NCCS],
    };
    for (enabled, flag) in [
        (termios.ignbrk, abi::IGNBRK),
        (termios.brkint, abi::BRKINT),
        (termios.ignpar, abi::IGNPAR),
        (termios.parmrk, abi::PARMRK),
        (termios.inpck, abi::INPCK),
        (termios.istrip, abi::ISTRIP),
        (termios.inlcr, abi::INLCR),
        (termios.igncr, abi::IGNCR),
        (termios.icrnl, abi::ICRNL),
    ] {
        if enabled {
            result.c_iflag |= flag;
        }
    }
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

pub(super) fn validate_termios(
    candidate: abi::Termios,
    current: TtyTermios,
    line: TtyLineSnapshot,
) -> Result<TtyTermios, SysError> {
    let projected = project_termios(current, line)?;
    let allowed_iflag = abi::IGNBRK
        | abi::BRKINT
        | abi::IGNPAR
        | abi::PARMRK
        | abi::INPCK
        | abi::ISTRIP
        | abi::INLCR
        | abi::IGNCR
        | abi::ICRNL;
    let tab_mode = candidate.c_oflag & abi::TABDLY;
    if !matches!(tab_mode, abi::TAB0 | abi::TAB3) {
        return Err(SysError::InvalidArgument);
    }
    let allowed_oflag = abi::OPOST | abi::ONLCR | abi::TABDLY;
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
        ignbrk: candidate.c_iflag & abi::IGNBRK != 0,
        brkint: candidate.c_iflag & abi::BRKINT != 0,
        ignpar: candidate.c_iflag & abi::IGNPAR != 0,
        parmrk: candidate.c_iflag & abi::PARMRK != 0,
        inpck: candidate.c_iflag & abi::INPCK != 0,
        istrip: candidate.c_iflag & abi::ISTRIP != 0,
        inlcr: candidate.c_iflag & abi::INLCR != 0,
        igncr: candidate.c_iflag & abi::IGNCR != 0,
        icrnl: candidate.c_iflag & abi::ICRNL != 0,
        opost: candidate.c_oflag & abi::OPOST != 0,
        onlcr: candidate.c_oflag & abi::ONLCR != 0,
        tab3: tab_mode == abi::TAB3,
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

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    fn line() -> TtyLineSnapshot {
        TtyLineSnapshot {
            baud: 115200,
            parity: TtyParity::None,
            data_bits: 8,
        }
    }

    #[kunit]
    fn asm_generic_projection_and_validation_are_atomic() {
        let current = TtyTermios::default();
        let raw = project_termios(current, line()).unwrap();
        assert_eq!(raw.c_cflag & abi::CBAUD, abi::B115200);
        assert_eq!(raw.c_cflag & abi::CSIZE, abi::CS8);
        assert_eq!(raw.c_cc[abi::VMIN], 1);
        assert_eq!(raw.c_cc[abi::VTIME], 0);
        assert_eq!(raw.c_iflag, abi::ICRNL);
        assert_eq!(raw.c_oflag & abi::TABDLY, abi::TAB0);

        let mut input_modes = raw;
        input_modes.c_iflag = abi::IGNBRK
            | abi::BRKINT
            | abi::IGNPAR
            | abi::PARMRK
            | abi::INPCK
            | abi::ISTRIP
            | abi::INLCR
            | abi::IGNCR
            | abi::ICRNL;
        let input_modes = validate_termios(input_modes, current, line()).unwrap();
        assert_eq!(
            project_termios(input_modes, line()).unwrap().c_iflag,
            abi::IGNBRK
                | abi::BRKINT
                | abi::IGNPAR
                | abi::PARMRK
                | abi::INPCK
                | abi::ISTRIP
                | abi::INLCR
                | abi::IGNCR
                | abi::ICRNL
        );

        let mut candidate = raw;
        candidate.c_lflag &= !(abi::ICANON | abi::ECHO);
        let updated = validate_termios(candidate, current, line()).unwrap();
        assert!(!updated.icanon);
        assert!(!updated.echo);

        // GNU less 668 enables XTABS while entering its noncanonical input
        // mode. TAB3 is a real output transform; legacy TAB1/TAB2 delay modes
        // remain unsupported and must not become success-no-op flags.
        let mut less = raw;
        less.c_oflag |= abi::XTABS;
        less.c_lflag = abi::ISIG;
        let less = validate_termios(less, current, line()).unwrap();
        assert!(less.tab3);
        assert_eq!(
            project_termios(less, line()).unwrap().c_oflag & abi::TABDLY,
            abi::TAB3
        );
        for unsupported_tab_mode in [abi::TAB1, abi::TAB2] {
            let mut unsupported = raw;
            unsupported.c_oflag |= unsupported_tab_mode;
            assert_eq!(
                validate_termios(unsupported, current, line()),
                Err(SysError::InvalidArgument)
            );
        }

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
}
