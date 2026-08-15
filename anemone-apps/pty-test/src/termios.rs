use anemone_rs::{
    abi::{
        fs::linux::open::{O_NOCTTY, O_RDWR},
        tty::linux::{
            ADDRB, B9600, B38400, B115200, BOTHER, BS1, CBAUD, CIBAUD, CLOCAL, CMSPAR, CR3, CREAD,
            CRTSCTS, CS6, CS7, CS8, CSIZE, CSTOPB, ECHO, ECHOE, FF1, FLUSHO, HUPCL, ICANON,
            IMAXBEL, ISIG, IUTF8, NL1, OFDEL, OFILL, OPOST, PARENB, PARODD, PENDIN, TAB2, TAB3,
            VMIN, VT1, VTIME, XCASE,
        },
    },
    os::linux::tty::{SetTermiosWhen, tcgetattr, tcsetattr},
    prelude::*,
};

use crate::support::{Pair, ensure, expect_errno, read_exact, write_all};

const NORMALIZED_LINE_BITS: u32 = CS8 | CREAD;

fn expected_logical_cflag(requested: u32) -> u32 {
    (requested & !(CSIZE | PARENB)) | NORMALIZED_LINE_BITS
}

pub fn test_cflag_profile() -> Result<(), Errno> {
    let pair = Pair::allocate()?;
    pair.unlock()?;
    let slave = pair.open_path(O_RDWR | O_NOCTTY)?;

    let initial = tcgetattr(slave.raw())?;
    ensure(initial.c_cflag == B38400 | NORMALIZED_LINE_BITS)?;
    ensure(tcgetattr(pair.master.raw())? == initial)?;

    let mut immediate = initial;
    immediate.c_cflag = B115200
        | (B9600 << 16)
        | CS7
        | CSTOPB
        | PARENB
        | PARODD
        | HUPCL
        | CLOCAL
        | CMSPAR
        | CRTSCTS;
    let expected_immediate = expected_logical_cflag(immediate.c_cflag);
    tcsetattr(slave.raw(), SetTermiosWhen::Now, &immediate)?;
    let immediate = tcgetattr(pair.master.raw())?;
    ensure(immediate.c_cflag == expected_immediate)?;
    ensure(immediate.c_cflag & CBAUD == B115200)?;
    ensure(immediate.c_cflag & CIBAUD == B9600 << 16)?;
    ensure(
        immediate.c_cflag & (CSTOPB | PARODD | HUPCL | CLOCAL | CMSPAR | CRTSCTS)
            == CSTOPB | PARODD | HUPCL | CLOCAL | CMSPAR | CRTSCTS,
    )?;

    let mut drained = immediate;
    drained.c_cflag = B9600 | CS6 | CLOCAL;
    tcsetattr(pair.master.raw(), SetTermiosWhen::Drain, &drained)?;
    let drained = tcgetattr(slave.raw())?;
    ensure(drained.c_cflag == B9600 | CLOCAL | NORMALIZED_LINE_BITS)?;

    tcsetattr(slave.raw(), SetTermiosWhen::DrainFlush, &initial)?;
    ensure(tcgetattr(pair.master.raw())? == initial)?;

    let before_reject = tcgetattr(slave.raw())?;
    let mut unsupported = before_reject;
    unsupported.c_cflag |= ADDRB;
    expect_errno(
        tcsetattr(slave.raw(), SetTermiosWhen::Now, &unsupported),
        EINVAL,
    )?;
    ensure(tcgetattr(pair.master.raw())? == before_reject)?;

    for (mask, unsupported_baud) in [(CBAUD, BOTHER), (CIBAUD, BOTHER << 16)] {
        let mut unsupported = before_reject;
        unsupported.c_cflag = (unsupported.c_cflag & !mask) | unsupported_baud;
        expect_errno(
            tcsetattr(slave.raw(), SetTermiosWhen::Now, &unsupported),
            EINVAL,
        )?;
        ensure(tcgetattr(pair.master.raw())? == before_reject)?;
    }
    Ok(())
}

const COMPAT_OFLAG: u32 = OFILL | OFDEL | NL1 | CR3 | TAB2 | BS1 | VT1 | FF1;
const COMPAT_LFLAG: u32 = XCASE | FLUSHO | PENDIN;

pub fn test_iutf8_compatibility() -> Result<(), Errno> {
    let pair = Pair::allocate()?;
    pair.unlock()?;
    let slave = pair.open_path(O_RDWR | O_NOCTTY)?;
    let initial = tcgetattr(slave.raw())?;

    for mode in [
        SetTermiosWhen::Now,
        SetTermiosWhen::Drain,
        SetTermiosWhen::DrainFlush,
    ] {
        let mut candidate = initial;
        candidate.c_iflag |= IUTF8;
        candidate.c_oflag |= COMPAT_OFLAG;
        candidate.c_lflag |= COMPAT_LFLAG;
        tcsetattr(slave.raw(), mode, &candidate)?;
        ensure(tcgetattr(pair.master.raw())? == candidate)?;
        tcsetattr(pair.master.raw(), mode, &initial)?;
        ensure(tcgetattr(slave.raw())? == initial)?;
    }

    let mut committed = initial;
    committed.c_iflag |= IUTF8;
    committed.c_oflag |= COMPAT_OFLAG;
    committed.c_lflag |= COMPAT_LFLAG;
    tcsetattr(slave.raw(), SetTermiosWhen::Now, &committed)?;
    for unsupported in [IMAXBEL, 0x8000] {
        let mut candidate = committed;
        candidate.c_iflag |= unsupported;
        expect_errno(
            tcsetattr(slave.raw(), SetTermiosWhen::Now, &candidate),
            EINVAL,
        )?;
        ensure(tcgetattr(pair.master.raw())? == committed)?;
    }

    // Linux 6.6.32 preserves these obsolete flags but N_TTY does not read
    // them. Exercise a representative byte stream to prevent a future
    // compatibility-field branch from silently acquiring behavior.
    let mut raw = committed;
    raw.c_lflag &= !(ICANON | ECHO | ISIG);
    raw.c_cc[VMIN] = 1;
    raw.c_cc[VTIME] = 0;
    tcsetattr(slave.raw(), SetTermiosWhen::Now, &raw)?;
    write_all(pair.master.raw(), b"compat")?;
    let mut observed = [0_u8; 6];
    read_exact(slave.raw(), &mut observed)?;
    ensure(&observed == b"compat")
}

fn canonical_noecho(slave: u32, iutf8: bool) -> Result<(), Errno> {
    let mut termios = tcgetattr(slave)?;
    termios.c_iflag = if iutf8 { IUTF8 } else { 0 };
    termios.c_lflag |= ICANON;
    termios.c_lflag &= !(ECHO | ISIG);
    termios.c_cc[2] = 0x7f;
    tcsetattr(slave, SetTermiosWhen::DrainFlush, &termios)
}

fn canonical_erase_case(input: &[u8], expected: &[u8], iutf8: bool) -> Result<(), Errno> {
    let pair = Pair::allocate()?;
    pair.unlock()?;
    let slave = pair.open_path(O_RDWR | O_NOCTTY)?;
    canonical_noecho(slave.raw(), iutf8)?;
    write_all(pair.master.raw(), input)?;
    let mut observed = vec![0_u8; expected.len()];
    read_exact(slave.raw(), &mut observed)?;
    ensure(observed == expected)
}

fn output_column_case(iutf8: bool, spaces: usize) -> Result<(), Errno> {
    let pair = Pair::allocate()?;
    pair.unlock()?;
    let slave = pair.open_path(O_RDWR | O_NOCTTY)?;
    let mut termios = tcgetattr(slave.raw())?;
    termios.c_iflag = if iutf8 { IUTF8 } else { 0 };
    termios.c_oflag = OPOST | TAB3;
    termios.c_lflag &= !(ICANON | ECHO | ISIG);
    termios.c_cc[VMIN] = 1;
    termios.c_cc[VTIME] = 0;
    tcsetattr(slave.raw(), SetTermiosWhen::Now, &termios)?;
    write_all(slave.raw(), &[0xe4, 0xb8, 0xad, b'\t'])?;
    let mut observed = vec![0_u8; 3 + spaces];
    read_exact(pair.master.raw(), &mut observed)?;
    ensure(&observed[..3] == &[0xe4, 0xb8, 0xad])?;
    ensure(observed[3..].iter().all(|byte| *byte == b' '))
}

fn echo_tab_erase_case(iutf8: bool, spaces: usize) -> Result<(), Errno> {
    let pair = Pair::allocate()?;
    pair.unlock()?;
    let slave = pair.open_path(O_RDWR | O_NOCTTY)?;
    let mut termios = tcgetattr(slave.raw())?;
    termios.c_iflag = if iutf8 { IUTF8 } else { 0 };
    termios.c_oflag = OPOST | TAB3;
    termios.c_lflag = ICANON | ECHO | ECHOE;
    termios.c_cc[2] = 0x7f;
    tcsetattr(slave.raw(), SetTermiosWhen::DrainFlush, &termios)?;
    write_all(pair.master.raw(), &[0xe4, 0xb8, 0xad, b'\t', 0x7f])?;
    let mut observed = vec![0_u8; 3 + spaces * 2];
    read_exact(pair.master.raw(), &mut observed)?;
    ensure(&observed[..3] == &[0xe4, 0xb8, 0xad])?;
    ensure(observed[3..3 + spaces].iter().all(|byte| *byte == b' '))?;
    ensure(observed[3 + spaces..].iter().all(|byte| *byte == 0x08))
}

fn tab_erase_without_opost_case() -> Result<(), Errno> {
    let pair = Pair::allocate()?;
    pair.unlock()?;
    let slave = pair.open_path(O_RDWR | O_NOCTTY)?;
    let mut termios = tcgetattr(slave.raw())?;
    termios.c_iflag = IUTF8;
    termios.c_oflag = OPOST | TAB3;
    termios.c_lflag = ICANON | ECHO | ECHOE;
    termios.c_cc[2] = 0x7f;
    tcsetattr(slave.raw(), SetTermiosWhen::Now, &termios)?;

    write_all(slave.raw(), b"abc")?;
    let mut prefix = [0_u8; 3];
    read_exact(pair.master.raw(), &mut prefix)?;
    ensure(&prefix == b"abc")?;

    termios.c_oflag = 0;
    tcsetattr(slave.raw(), SetTermiosWhen::Now, &termios)?;
    write_all(pair.master.raw(), b"\t\x7f")?;
    let mut erased = [0_u8; 6];
    read_exact(pair.master.raw(), &mut erased)?;
    ensure(&erased == b"\t\x08\x08\x08\x08\x08")?;

    termios.c_oflag = OPOST | TAB3;
    tcsetattr(slave.raw(), SetTermiosWhen::Now, &termios)?;
    write_all(slave.raw(), b"\t")?;
    let mut expanded = [0_u8; 8];
    read_exact(pair.master.raw(), &mut expanded)?;
    ensure(expanded == [b' '; 8])
}

pub fn test_iutf8_discipline_column() -> Result<(), Errno> {
    for sequence in [
        &[b'A'][..],
        &[0xc2, 0xa2],
        &[0xe4, 0xb8, 0xad],
        &[0xf0, 0x9f, 0x98, 0x80],
    ] {
        let mut input = sequence.to_vec();
        input.extend_from_slice(&[0x7f, b'\n']);
        canonical_erase_case(&input, b"\n", true)?;
    }
    canonical_erase_case(
        &[0xe4, 0xb8, 0xad, 0x7f, b'\n'],
        &[0xe4, 0xb8, b'\n'],
        false,
    )?;
    canonical_erase_case(&[0x80, 0x81, 0x7f, b'\n'], &[0x80, 0x81, b'\n'], true)?;
    canonical_erase_case(&[0xc2, 0x80, 0x81, 0x7f, b'\n'], b"\n", true)?;

    for (iutf8, spaces) in [(true, 7), (false, 5)] {
        output_column_case(iutf8, spaces)?;
        echo_tab_erase_case(iutf8, spaces)?;
    }
    tab_erase_without_opost_case()?;
    Ok(())
}
