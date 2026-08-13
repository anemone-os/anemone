use anemone_rs::{
    abi::{
        fs::linux::open::{O_NOCTTY, O_RDWR},
        tty::linux::{
            ADDRB, B9600, B38400, B115200, BOTHER, CBAUD, CIBAUD, CLOCAL, CMSPAR, CREAD, CRTSCTS,
            CS6, CS7, CS8, CSIZE, CSTOPB, HUPCL, PARENB, PARODD,
        },
    },
    os::linux::tty::{SetTermiosWhen, tcgetattr, tcsetattr},
    prelude::*,
};

use crate::support::{Pair, ensure, expect_errno};

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
