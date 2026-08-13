use anemone_abi::tty::linux as abi;

use crate::prelude::*;

use super::{
    super::{
        port::{TtyLineSnapshot, TtyParity},
        terminal::{TtyCompatibility, TtyControlProfile, TtyTabMode, TtyTermios},
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
        let updated = validate_termios(candidate, current)?;
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
            observe_no_behavior_compatibility_change(current, updated);
            tty.wake.wake();
            return Ok(());
        }
    }
}

pub(super) fn project_termios(termios: TtyTermios) -> Result<abi::Termios, SysError> {
    let mut result = abi::Termios {
        c_iflag: 0,
        c_oflag: (if termios.opost { abi::OPOST } else { 0 })
            | (if termios.onlcr { abi::ONLCR } else { 0 })
            | match termios.tab_mode {
                TtyTabMode::Literal => abi::TAB0,
                TtyTabMode::Delay1 => abi::TAB1,
                TtyTabMode::Delay2 => abi::TAB2,
                TtyTabMode::Expand => abi::TAB3,
            }
            | termios.compatibility.output,
        c_cflag: project_control(termios.control)?,
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
        (termios.iutf8, abi::IUTF8),
    ] {
        if enabled {
            result.c_iflag |= flag;
        }
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
    result.c_lflag |= termios.compatibility.local;
    for (index, value) in control_chars(termios) {
        result.c_cc[index] = value;
    }
    Ok(result)
}

pub(super) fn validate_termios(
    candidate: abi::Termios,
    current: TtyTermios,
) -> Result<TtyTermios, SysError> {
    let projected = project_termios(current)?;
    let control = validate_control(candidate.c_cflag, current.control, projected.c_cflag)?;
    let allowed_iflag = abi::IGNBRK
        | abi::BRKINT
        | abi::IGNPAR
        | abi::PARMRK
        | abi::INPCK
        | abi::ISTRIP
        | abi::INLCR
        | abi::IGNCR
        | abi::ICRNL
        | abi::IUTF8;
    let tab_mode = candidate.c_oflag & abi::TABDLY;
    let compatibility_oflag =
        abi::OFILL | abi::OFDEL | abi::NLDLY | abi::CRDLY | abi::BSDLY | abi::VTDLY | abi::FFDLY;
    let compatibility_lflag = abi::XCASE | abi::FLUSHO | abi::PENDIN;
    let allowed_oflag = abi::OPOST | abi::ONLCR | abi::TABDLY | compatibility_oflag;
    let allowed_lflag = abi::ISIG
        | abi::ICANON
        | abi::ECHO
        | abi::ECHOE
        | abi::ECHOK
        | abi::ECHONL
        | compatibility_lflag;
    if candidate.c_iflag & !allowed_iflag != projected.c_iflag & !allowed_iflag
        || candidate.c_oflag & !allowed_oflag != projected.c_oflag & !allowed_oflag
        || candidate.c_lflag & !allowed_lflag != projected.c_lflag & !allowed_lflag
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
        control,
        ignbrk: candidate.c_iflag & abi::IGNBRK != 0,
        brkint: candidate.c_iflag & abi::BRKINT != 0,
        ignpar: candidate.c_iflag & abi::IGNPAR != 0,
        parmrk: candidate.c_iflag & abi::PARMRK != 0,
        inpck: candidate.c_iflag & abi::INPCK != 0,
        istrip: candidate.c_iflag & abi::ISTRIP != 0,
        inlcr: candidate.c_iflag & abi::INLCR != 0,
        igncr: candidate.c_iflag & abi::IGNCR != 0,
        icrnl: candidate.c_iflag & abi::ICRNL != 0,
        iutf8: candidate.c_iflag & abi::IUTF8 != 0,
        opost: candidate.c_oflag & abi::OPOST != 0,
        onlcr: candidate.c_oflag & abi::ONLCR != 0,
        tab_mode: match tab_mode {
            abi::TAB0 => TtyTabMode::Literal,
            abi::TAB1 => TtyTabMode::Delay1,
            abi::TAB2 => TtyTabMode::Delay2,
            abi::TAB3 => TtyTabMode::Expand,
            _ => unreachable!("TABDLY mask produced an invalid mode"),
        },
        compatibility: TtyCompatibility {
            output: candidate.c_oflag & compatibility_oflag,
            local: candidate.c_lflag & compatibility_lflag,
        },
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

fn observe_no_behavior_compatibility_change(old: TtyTermios, new: TtyTermios) {
    let old_output = no_behavior_output_flags(old);
    let new_output = no_behavior_output_flags(new);
    if old_output == new_output && old.compatibility.local == new.compatibility.local {
        return;
    }
    // These bits are a deliberate Linux 6.6.32 compatibility surface:
    // TCGETS preserves them but N_TTY assigns no behavior. Keep the notice
    // until the accepted ABI is explicitly revised; it makes silent
    // compatibility distinguishable from accidentally ignored semantics.
    knoticeln!(
        "tty: no-behavior termios compatibility changed oflag={:#x}->{:#x} lflag={:#x}->{:#x}",
        old_output,
        new_output,
        old.compatibility.local,
        new.compatibility.local,
    );
}

fn no_behavior_output_flags(termios: TtyTermios) -> u32 {
    termios.compatibility.output
        | match termios.tab_mode {
            TtyTabMode::Delay1 => abi::TAB1,
            TtyTabMode::Delay2 => abi::TAB2,
            TtyTabMode::Literal | TtyTabMode::Expand => 0,
        }
}

fn project_control(control: TtyControlProfile) -> Result<u32, SysError> {
    match control {
        TtyControlProfile::Physical(line) => {
            let mut cflag = baud_flag(line.baud).ok_or(SysError::InvalidArgument)?
                | data_bits_flag(line.data_bits).ok_or(SysError::InvalidArgument)?
                | abi::CREAD
                | abi::CLOCAL;
            match line.parity {
                TtyParity::None => {},
                TtyParity::Even => cflag |= abi::PARENB,
                TtyParity::Odd => cflag |= abi::PARENB | abi::PARODD,
            }
            Ok(cflag)
        },
        TtyControlProfile::Pty(cflag) => Ok(cflag),
    }
}

fn validate_control(
    candidate: u32,
    current: TtyControlProfile,
    projected: u32,
) -> Result<TtyControlProfile, SysError> {
    match current {
        TtyControlProfile::Physical(line) => {
            if candidate != projected {
                return Err(SysError::InvalidArgument);
            }
            Ok(TtyControlProfile::Physical(line))
        },
        TtyControlProfile::Pty(_) => {
            if candidate & abi::CBAUD == abi::BOTHER
                || (candidate & abi::CIBAUD) >> 16 == abi::BOTHER
            {
                // Legacy TCSETS has no c_ispeed/c_ospeed payload, so accepting
                // BOTHER would commit a selector whose requested speed is lost.
                // Arbitrary speeds remain outside the ABI until TCSETS2 exists.
                return Err(SysError::InvalidArgument);
            }
            let supported = abi::CBAUD
                | abi::CSIZE
                | abi::CSTOPB
                | abi::CREAD
                | abi::PARENB
                | abi::PARODD
                | abi::HUPCL
                | abi::CLOCAL
                | abi::CIBAUD
                | abi::CMSPAR
                | abi::CRTSCTS;
            if candidate & !supported != projected & !supported {
                return Err(SysError::InvalidArgument);
            }
            // A PTY has no baud generator, framing, receiver-enable, or parity
            // hardware. Linux exposes those fields as logical termios state but
            // normalizes the impossible framing request to CS8 | CREAD with
            // parity disabled; preserving the remaining supported bits keeps
            // ordinary read-modify-write callers ABI-visible without inventing
            // a physical-line effect.
            let normalized = (candidate & !(abi::CSIZE | abi::PARENB)) | abi::CS8 | abi::CREAD;
            Ok(TtyControlProfile::Pty(normalized))
        },
    }
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

    fn physical() -> TtyTermios {
        TtyTermios::with_control(TtyControlProfile::Physical(line()))
    }

    fn pty() -> TtyTermios {
        TtyTermios::with_control(TtyControlProfile::Pty(abi::B38400 | abi::CS8 | abi::CREAD))
    }

    #[kunit]
    fn asm_generic_projection_and_validation_are_atomic() {
        let current = physical();
        let raw = project_termios(current).unwrap();
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
        let input_modes = validate_termios(input_modes, current).unwrap();
        assert_eq!(
            project_termios(input_modes).unwrap().c_iflag,
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
        let updated = validate_termios(candidate, current).unwrap();
        assert!(!updated.icanon);
        assert!(!updated.echo);

        // GNU less 668 enables XTABS while entering its noncanonical input
        // mode. TAB3 is a real output transform; legacy TAB1/TAB2 delay modes
        // remain unsupported and must not become success-no-op flags.
        let mut less = raw;
        less.c_oflag |= abi::XTABS;
        less.c_lflag = abi::ISIG;
        let less = validate_termios(less, current).unwrap();
        assert!(less.expands_tabs());
        assert_eq!(
            project_termios(less).unwrap().c_oflag & abi::TABDLY,
            abi::TAB3
        );
        for compatibility_tab_mode in [abi::TAB1, abi::TAB2] {
            let mut compatibility = raw;
            compatibility.c_oflag |= compatibility_tab_mode;
            let compatibility = validate_termios(compatibility, current).unwrap();
            assert_eq!(
                project_termios(compatibility).unwrap().c_oflag & abi::TABDLY,
                compatibility_tab_mode
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
        let disabled = validate_termios(disabled, current).unwrap();
        let projected_disabled = project_termios(disabled).unwrap();
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
            validate_termios(candidate, current),
            Err(SysError::InvalidArgument)
        );
        assert!(current.icanon);

        let mut canonical_cc = raw;
        canonical_cc.c_cc[abi::VMIN] = 7;
        canonical_cc.c_cc[abi::VTIME] = 9;
        let canonical = validate_termios(canonical_cc, current).unwrap();
        assert_eq!(canonical.vmin, 7);
        assert_eq!(canonical.vtime, 9);
        canonical_cc.c_lflag &= !abi::ICANON;
        assert_eq!(
            validate_termios(canonical_cc, current),
            Err(SysError::InvalidArgument)
        );
    }

    #[kunit]
    fn physical_cflag_is_immutable_but_pty_cflag_is_logical() {
        let physical = physical();
        let physical_snapshot = project_termios(physical).unwrap();
        let mut changed_physical = physical_snapshot;
        changed_physical.c_cflag ^= abi::CLOCAL;
        assert_eq!(
            validate_termios(changed_physical, physical),
            Err(SysError::InvalidArgument)
        );

        let pty = pty();
        let mut candidate = project_termios(pty).unwrap();
        candidate.c_cflag = abi::B115200
            | (abi::B9600 << 16)
            | abi::CS7
            | abi::CSTOPB
            | abi::PARENB
            | abi::PARODD
            | abi::HUPCL
            | abi::CLOCAL
            | abi::CMSPAR
            | abi::CRTSCTS;
        let updated = validate_termios(candidate, pty).unwrap();
        assert_eq!(
            project_termios(updated).unwrap().c_cflag,
            abi::B115200
                | (abi::B9600 << 16)
                | abi::CS8
                | abi::CSTOPB
                | abi::CREAD
                | abi::PARODD
                | abi::HUPCL
                | abi::CLOCAL
                | abi::CMSPAR
                | abi::CRTSCTS
        );

        let mut unsupported = project_termios(updated).unwrap();
        unsupported.c_cflag |= abi::ADDRB;
        assert_eq!(
            validate_termios(unsupported, updated),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(project_termios(updated).unwrap().c_cflag & abi::ADDRB, 0);

        let committed = project_termios(updated).unwrap();
        for (mask, unsupported_baud) in
            [(abi::CBAUD, abi::BOTHER), (abi::CIBAUD, abi::BOTHER << 16)]
        {
            let mut unsupported = committed;
            unsupported.c_cflag = (unsupported.c_cflag & !mask) | unsupported_baud;
            assert_eq!(
                validate_termios(unsupported, updated),
                Err(SysError::InvalidArgument)
            );
            assert_eq!(project_termios(updated).unwrap(), committed);
        }
    }

    #[kunit]
    fn iutf8_and_linux_no_behavior_flags_round_trip_atomically() {
        let current = pty();
        let before = project_termios(current).unwrap();
        let mut candidate = before;
        candidate.c_iflag |= abi::IUTF8;
        candidate.c_oflag |= abi::OFILL
            | abi::OFDEL
            | abi::NL1
            | abi::CR3
            | abi::TAB2
            | abi::BS1
            | abi::VT1
            | abi::FF1;
        candidate.c_lflag |= abi::XCASE | abi::FLUSHO | abi::PENDIN;
        let updated = validate_termios(candidate, current).unwrap();
        assert!(updated.iutf8);
        assert_eq!(project_termios(updated).unwrap(), candidate);

        for unsupported_iflag in [abi::IMAXBEL, 0x8000] {
            let committed = project_termios(updated).unwrap();
            let mut unsupported = committed;
            unsupported.c_iflag |= unsupported_iflag;
            assert_eq!(
                validate_termios(unsupported, updated),
                Err(SysError::InvalidArgument)
            );
            assert_eq!(project_termios(updated).unwrap(), committed);
        }
    }
}
