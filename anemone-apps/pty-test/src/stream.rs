use anemone_rs::{
    abi::{
        fs::linux::{
            epoll::{EPOLLIN, EPOLLOUT},
            ioctl::FIONREAD,
            open::{O_NOCTTY, O_NONBLOCK, O_RDWR},
            poll::{POLLIN, POLLOUT, PollFd},
            select::FdSet,
        },
        syscall::{linux::SYS_IOCTL, syscall},
        time::linux::TimeSpec,
        tty::linux::{
            ECHO, ICANON, ISIG, ONLCR, OPOST, TCFLSH, TCIFLUSH, TCIOFLUSH, TCOFLUSH, TIOCINQ, VEOF,
            Winsize,
        },
    },
    os::linux::{
        fs::{
            EpollCreateFlags, EpollCtlOp, Fd, PipeFlags, close, epoll_create1, epoll_ctl,
            epoll_wait, fcntl_getfl, fcntl_setfl, ioctl_readable_bytes, pipe2, ppoll, pselect,
            read, write,
        },
        process::{exit, fork},
        tty::{SetTermiosWhen, get_winsize, set_winsize, tcgetattr, tcsetattr},
    },
    prelude::*,
};

use crate::support::{Pair, ensure, expect_errno, raw_termios, read_exact, wait_child, write_all};

const ZERO_TIMEOUT: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 0,
};

fn tcflush(fd: Fd, selector: u64) -> Result<(), Errno> {
    unsafe { syscall(SYS_IOCTL, fd as u64, TCFLSH as u64, selector, 0, 0, 0) }.map(|_| ())
}

fn fdset_with(fd: Fd) -> FdSet {
    let mut set = FdSet::default();
    set.fds_bits[fd as usize / 64] |= 1u64 << (fd as usize % 64);
    set
}

fn fdset_contains(set: &FdSet, fd: Fd) -> bool {
    set.fds_bits[fd as usize / 64] & (1u64 << (fd as usize % 64)) != 0
}

fn fill_until_eagain(fd: Fd, byte: u8) -> Result<usize, Errno> {
    const FAILURE_BOUND_BYTES: usize = 1024 * 1024;

    let chunk = [byte; 1024];
    let mut committed = 0usize;
    loop {
        match write(fd, &chunk) {
            Ok(0) => return Err(EIO),
            Ok(count) => {
                committed = committed.checked_add(count).ok_or(EOVERFLOW)?;
                // This is only a failure bound for an unexpectedly unbounded
                // queue; the first EAGAIN remains the capacity observation.
                if committed > FAILURE_BOUND_BYTES {
                    return Err(EOVERFLOW);
                }
            },
            Err(EAGAIN) if committed != 0 => return Ok(committed),
            Err(errno) => return Err(errno),
        }
    }
}

fn assert_partial_retry(
    writer: Fd,
    reader: Fd,
    fill: u8,
    first: u8,
    suffix: u8,
) -> Result<(), Errno> {
    let filled = fill_until_eagain(writer, fill)?;
    let mut released = [0u8; 1];
    read_exact(reader, &mut released)?;
    ensure(released == [fill])?;

    ensure(write(writer, &[first, suffix])? == 1)?;
    let mut queued = vec![0u8; filled];
    read_exact(reader, &mut queued)?;
    ensure(queued[..filled - 1].iter().all(|byte| *byte == fill))?;
    ensure(queued[filled - 1] == first)?;

    ensure(write(writer, &[suffix])? == 1)?;
    let mut retried = [0u8; 1];
    read_exact(reader, &mut retried)?;
    ensure(retried == [suffix])
}

pub fn test_stream_and_terminal_state() -> Result<(), Errno> {
    let pair = Pair::allocate()?;
    pair.unlock()?;
    let slave = pair.open_path(O_RDWR | O_NOCTTY)?;
    let raw = raw_termios(slave.raw())?;
    ensure(tcgetattr(pair.master.raw())? == raw)?;
    let mut alternate = raw;
    alternate.c_lflag ^= anemone_rs::abi::tty::linux::ECHO;
    tcsetattr(pair.master.raw(), SetTermiosWhen::Drain, &alternate)?;
    ensure(tcgetattr(slave.raw())? == alternate)?;
    tcsetattr(pair.master.raw(), SetTermiosWhen::DrainFlush, &raw)?;
    ensure(tcgetattr(slave.raw())? == raw)?;

    let size = Winsize {
        ws_row: 37,
        ws_col: 91,
        ws_xpixel: 3,
        ws_ypixel: 7,
    };
    set_winsize(pair.master.raw(), &size)?;
    ensure(get_winsize(slave.raw())? == size)?;

    write_all(pair.master.raw(), b"master-to-slave")?;
    let mut input = [0u8; 15];
    read_exact(slave.raw(), &mut input)?;
    ensure(&input == b"master-to-slave")?;

    write_all(slave.raw(), b"slave-to-master")?;
    let mut output = [0u8; 15];
    read_exact(pair.master.raw(), &mut output)?;
    ensure(&output == b"slave-to-master")
}

pub fn test_input_queue_queries() -> Result<(), Errno> {
    let pair = Pair::allocate()?;
    pair.unlock()?;
    let slave = pair.open_path(O_RDWR | O_NOCTTY)?;
    ensure(TIOCINQ == FIONREAD)?;

    let mut canonical = tcgetattr(slave.raw())?;
    canonical.c_lflag |= ICANON;
    canonical.c_lflag &= !(ECHO | ISIG);
    tcsetattr(slave.raw(), SetTermiosWhen::DrainFlush, &canonical)?;

    write_all(pair.master.raw(), b"abc")?;
    ensure(ioctl_readable_bytes(slave.raw())? == 0)?;
    write_all(pair.master.raw(), b"\nsecond\n")?;
    ensure(ioctl_readable_bytes(slave.raw())? == 11)?;
    ensure(ioctl_readable_bytes(slave.raw())? == 11)?;
    expect_errno(
        unsafe { syscall(SYS_IOCTL, slave.raw() as u64, FIONREAD as u64, 1, 0, 0, 0) },
        EFAULT,
    )?;
    ensure(ioctl_readable_bytes(slave.raw())? == 11)?;

    let mut prefix = [0u8; 2];
    ensure(read(slave.raw(), &mut prefix)? == 2 && &prefix == b"ab")?;
    ensure(ioctl_readable_bytes(slave.raw())? == 9)?;
    let mut delimiter = [0u8; 2];
    ensure(read(slave.raw(), &mut delimiter)? == 2 && &delimiter == b"c\n")?;
    ensure(ioctl_readable_bytes(slave.raw())? == 7)?;
    let mut second = [0u8; 7];
    read_exact(slave.raw(), &mut second)?;
    ensure(&second == b"second\n" && ioctl_readable_bytes(slave.raw())? == 0)?;

    write_all(pair.master.raw(), &[canonical.c_cc[VEOF]])?;
    ensure(ioctl_readable_bytes(slave.raw())? == 0)?;
    let mut readable = [PollFd {
        fd: slave.raw() as i32,
        events: POLLIN,
        revents: 0,
    }];
    ensure(ppoll(&mut readable, Some(&ZERO_TIMEOUT))? == 1)?;
    ensure(readable[0].revents & POLLIN != 0)?;
    let mut empty = [0u8; 1];
    ensure(read(slave.raw(), &mut empty)? == 0)?;

    raw_termios(slave.raw())?;
    write_all(pair.master.raw(), b"raw")?;
    ensure(ioctl_readable_bytes(slave.raw())? == 3)?;
    let mut byte = [0u8; 1];
    ensure(read(slave.raw(), &mut byte)? == 1 && byte == [b'r'])?;
    ensure(ioctl_readable_bytes(slave.raw())? == 2)?;
    let mut suffix = [0u8; 2];
    read_exact(slave.raw(), &mut suffix)?;
    ensure(&suffix == b"aw")?;

    let mut output = tcgetattr(slave.raw())?;
    output.c_lflag &= !ECHO;
    output.c_oflag |= OPOST | ONLCR;
    tcsetattr(slave.raw(), SetTermiosWhen::Now, &output)?;
    write_all(slave.raw(), b"\nX")?;
    ensure(ioctl_readable_bytes(pair.master.raw())? == 3)?;
    ensure(ioctl_readable_bytes(pair.master.raw())? == 3)?;
    expect_errno(
        unsafe {
            syscall(
                SYS_IOCTL,
                pair.master.raw() as u64,
                TIOCINQ as u64,
                1,
                0,
                0,
                0,
            )
        },
        EFAULT,
    )?;
    ensure(ioctl_readable_bytes(pair.master.raw())? == 3)?;
    ensure(read(pair.master.raw(), &mut byte)? == 1 && byte == [b'\r'])?;
    ensure(ioctl_readable_bytes(pair.master.raw())? == 2)?;
    read_exact(pair.master.raw(), &mut suffix)?;
    ensure(&suffix == b"\nX" && ioctl_readable_bytes(pair.master.raw())? == 0)
}

pub fn test_tcflush_direction_matrix() -> Result<(), Errno> {
    let pair = Pair::allocate()?;
    pair.unlock()?;
    let slave = pair.open_path(O_RDWR | O_NOCTTY)?;
    raw_termios(slave.raw())?;

    write_all(pair.master.raw(), b"slave-input-drop")?;
    ensure(ioctl_readable_bytes(slave.raw())? == 16)?;
    tcflush(slave.raw(), TCIFLUSH)?;
    ensure(ioctl_readable_bytes(slave.raw())? == 0)?;
    write_all(pair.master.raw(), b"slave-input-keep")?;
    let mut slave_input = [0_u8; 16];
    read_exact(slave.raw(), &mut slave_input)?;
    ensure(&slave_input == b"slave-input-keep")?;

    write_all(slave.raw(), b"slave-output-drop")?;
    ensure(ioctl_readable_bytes(pair.master.raw())? == 17)?;
    tcflush(slave.raw(), TCOFLUSH)?;
    ensure(ioctl_readable_bytes(pair.master.raw())? == 0)?;
    write_all(slave.raw(), b"slave-output-keep")?;
    let mut master_input = [0_u8; 17];
    read_exact(pair.master.raw(), &mut master_input)?;
    ensure(&master_input == b"slave-output-keep")?;

    write_all(slave.raw(), b"master-input-drop")?;
    ensure(ioctl_readable_bytes(pair.master.raw())? == 17)?;
    tcflush(pair.master.raw(), TCIFLUSH)?;
    ensure(ioctl_readable_bytes(pair.master.raw())? == 0)?;
    write_all(slave.raw(), b"master-input-keep")?;
    read_exact(pair.master.raw(), &mut master_input)?;
    ensure(&master_input == b"master-input-keep")?;

    write_all(pair.master.raw(), b"master-output-drop")?;
    ensure(ioctl_readable_bytes(slave.raw())? == 18)?;
    tcflush(pair.master.raw(), TCOFLUSH)?;
    ensure(ioctl_readable_bytes(slave.raw())? == 0)?;
    write_all(pair.master.raw(), b"master-output-keep")?;
    let mut slave_output = [0_u8; 18];
    read_exact(slave.raw(), &mut slave_output)?;
    ensure(&slave_output == b"master-output-keep")?;

    write_all(pair.master.raw(), b"both-input-drop")?;
    write_all(slave.raw(), b"both-output-drop")?;
    ensure(ioctl_readable_bytes(slave.raw())? == 15)?;
    ensure(ioctl_readable_bytes(pair.master.raw())? == 16)?;
    tcflush(slave.raw(), TCIOFLUSH)?;
    ensure(ioctl_readable_bytes(slave.raw())? == 0)?;
    ensure(ioctl_readable_bytes(pair.master.raw())? == 0)?;

    expect_errno(tcflush(slave.raw(), TCIOFLUSH + 1), EINVAL)?;
    write_all(pair.master.raw(), b"both-input-keep")?;
    write_all(slave.raw(), b"both-output-keep")?;
    let mut both_input = [0_u8; 15];
    let mut both_output = [0_u8; 16];
    read_exact(slave.raw(), &mut both_input)?;
    read_exact(pair.master.raw(), &mut both_output)?;
    ensure(&both_input == b"both-input-keep")?;
    ensure(&both_output == b"both-output-keep")?;

    drop(slave);
    tcflush(pair.master.raw(), TCIFLUSH)?;
    tcflush(pair.master.raw(), TCOFLUSH)?;
    tcflush(pair.master.raw(), TCIOFLUSH)?;
    expect_errno(tcflush(pair.master.raw(), TCIOFLUSH + 1), EINVAL)
}

fn test_blocking_read(pair: &Pair, slave: Fd) -> Result<(), Errno> {
    let (ready_read, ready_write) = pipe2(PipeFlags::empty())?;
    let child = match fork()? {
        None => {
            let outcome = (|| {
                close(ready_read)?;
                write_all(ready_write, &[1])?;
                close(ready_write)?;
                let mut byte = [0u8; 1];
                read_exact(slave, &mut byte)?;
                ensure(byte == [0x5a])
            })();
            exit(if outcome.is_ok() { 0 } else { 1 })
        },
        Some(child) => child,
    };
    close(ready_write)?;
    let mut ready = [0u8; 1];
    read_exact(ready_read, &mut ready)?;
    close(ready_read)?;
    write_all(pair.master.raw(), &[0x5a])?;
    wait_child(child)
}

pub fn test_nonblocking_and_readiness() -> Result<(), Errno> {
    let pair = Pair::allocate()?;
    pair.unlock()?;
    let slave = pair.open_peer(O_RDWR | O_NOCTTY)?;
    raw_termios(slave.raw())?;
    test_blocking_read(&pair, slave.raw())?;

    let original_flags = fcntl_getfl(slave.raw())?;
    fcntl_setfl(slave.raw(), original_flags | O_NONBLOCK)?;
    let mut empty = [0u8; 1];
    expect_errno(read(slave.raw(), &mut empty), EAGAIN)?;

    let mut initial = [
        PollFd {
            fd: slave.raw() as i32,
            events: POLLIN,
            revents: 0,
        },
        PollFd {
            fd: pair.master.raw() as i32,
            events: POLLOUT,
            revents: 0,
        },
    ];
    ensure(ppoll(&mut initial, Some(&ZERO_TIMEOUT))? == 1)?;
    ensure(initial[0].revents == 0 && initial[1].revents & POLLOUT != 0)?;

    let epfd = epoll_create1(EpollCreateFlags::empty())?;
    epoll_ctl(
        epfd,
        EpollCtlOp::Add,
        slave.raw(),
        Some(&anemone_rs::os::linux::fs::EpollEvent::new(
            EPOLLIN | EPOLLOUT,
            0x5054_5901,
        )),
    )?;
    let mut events = [anemone_rs::os::linux::fs::EpollEvent::default(); 1];
    ensure(epoll_wait(epfd, &mut events, 0)? == 1)?;
    ensure(events[0].events & EPOLLOUT != 0 && events[0].events & EPOLLIN == 0)?;

    write_all(pair.master.raw(), b"ready")?;
    initial[0].revents = 0;
    initial[1].revents = 0;
    ensure(ppoll(&mut initial[..1], Some(&ZERO_TIMEOUT))? == 1)?;
    ensure(initial[0].revents & POLLIN != 0)?;

    let mut readfds = fdset_with(slave.raw());
    ensure(
        pselect(
            slave.raw() as usize + 1,
            Some(&mut readfds),
            None,
            None,
            Some(&ZERO_TIMEOUT),
        )? == 1,
    )?;
    ensure(fdset_contains(&readfds, slave.raw()))?;
    ensure(epoll_wait(epfd, &mut events, 0)? == 1)?;
    ensure(events[0].events & EPOLLIN != 0)?;
    close(epfd)?;

    let mut ready = [0u8; 5];
    read_exact(slave.raw(), &mut ready)?;
    ensure(&ready == b"ready")?;

    let master_epfd = epoll_create1(EpollCreateFlags::empty())?;
    epoll_ctl(
        master_epfd,
        EpollCtlOp::Add,
        pair.master.raw(),
        Some(&anemone_rs::os::linux::fs::EpollEvent::new(
            EPOLLIN,
            0x5054_5902,
        )),
    )?;
    ensure(epoll_wait(master_epfd, &mut events, 0)? == 0)?;
    write_all(slave.raw(), b"master-ready")?;
    let mut master_poll = [PollFd {
        fd: pair.master.raw() as i32,
        events: POLLIN,
        revents: 0,
    }];
    ensure(ppoll(&mut master_poll, Some(&ZERO_TIMEOUT))? == 1)?;
    ensure(master_poll[0].revents & POLLIN != 0)?;
    let mut master_readfds = fdset_with(pair.master.raw());
    ensure(
        pselect(
            pair.master.raw() as usize + 1,
            Some(&mut master_readfds),
            None,
            None,
            Some(&ZERO_TIMEOUT),
        )? == 1,
    )?;
    ensure(fdset_contains(&master_readfds, pair.master.raw()))?;
    ensure(epoll_wait(master_epfd, &mut events, 0)? == 1)?;
    ensure(events[0].events & EPOLLIN != 0)?;
    let mut master_ready = [0u8; 12];
    read_exact(pair.master.raw(), &mut master_ready)?;
    ensure(&master_ready == b"master-ready")?;
    close(master_epfd)?;

    // Each bounded queue is filled to its own observed EAGAIN. Releasing one
    // byte then writing two proves committed-prefix return and suffix retry
    // without assuming either Kconfig capacity.
    let master_flags = fcntl_getfl(pair.master.raw())?;
    fcntl_setfl(pair.master.raw(), master_flags | O_NONBLOCK)?;
    assert_partial_retry(pair.master.raw(), slave.raw(), b'x', b'y', b'z')?;
    assert_partial_retry(slave.raw(), pair.master.raw(), b'o', b'p', b'q')
}
