#![no_std]
#![no_main]

use anemone_rs::{
    abi::{
        fs::linux::open::O_NONBLOCK,
        net::{AF_INET, SOCK_DGRAM, SockAddrIn, socklen_t},
    },
    env::args,
    os::linux::{
        fs::{Fd, close, dup, fcntl_getfd, fcntl_getfl},
        net::{
            SocketFlags, bind_ipv4, bind_raw, getsockname_ipv4, getsockname_raw, socket_raw,
            udp_socket,
        },
        process::{WStatus, WStatusRaw, WaitFor, WaitOptions, execve, exit, fork, wait4},
    },
    prelude::*,
};

fn ensure(condition: bool) -> Result<(), Errno> {
    if condition { Ok(()) } else { Err(EIO) }
}

fn expect_errno<T>(result: Result<T, Errno>, expected: Errno) -> Result<(), Errno> {
    match result {
        Err(actual) if actual == expected => Ok(()),
        _ => Err(EIO),
    }
}

fn wait_child(pid: u32) -> Result<(), Errno> {
    let mut status = WStatusRaw::EMPTY;
    ensure(
        wait4(
            WaitFor::ChildWithTgid(pid),
            Some(&mut status),
            WaitOptions::empty(),
        )? == Some(pid),
    )?;
    ensure(matches!(status.read(), WStatus::Exited(0)))
}

fn test_create_errno_and_flags() -> Result<(), Errno> {
    expect_errno(unsafe { socket_raw(0, SOCK_DGRAM, 0) }, EAFNOSUPPORT)?;
    expect_errno(unsafe { socket_raw(AF_INET, 1, 0) }, ESOCKTNOSUPPORT)?;
    expect_errno(
        unsafe { socket_raw(AF_INET, SOCK_DGRAM, 6) },
        EPROTONOSUPPORT,
    )?;
    expect_errno(
        unsafe { socket_raw(AF_INET, SOCK_DGRAM | 0x4000_0000, 0) },
        EINVAL,
    )?;

    let protocol_zero = unsafe { socket_raw(AF_INET, SOCK_DGRAM, 0) }?;
    close(protocol_zero)?;
    let flagged = udp_socket(SocketFlags::NONBLOCK | SocketFlags::CLOEXEC)?;
    ensure(fcntl_getfd(flagged)? == 1)?;
    ensure(fcntl_getfl(flagged)? & O_NONBLOCK != 0)?;
    close(flagged)
}

fn test_unbound_and_port_zero_name() -> Result<(), Errno> {
    let fd = udp_socket(SocketFlags::empty())?;
    let unbound = getsockname_ipv4(fd)?;
    ensure(unbound.address() == [0; 4] && unbound.port() == 0)?;
    bind_ipv4(fd, SockAddrIn::new([0; 4], 0))?;
    let bound = getsockname_ipv4(fd)?;
    ensure(bound.address() == [0; 4])?;
    ensure((32768..=60999).contains(&bound.port()))?;
    expect_errno(bind_ipv4(fd, SockAddrIn::new([0; 4], 0)), EINVAL)?;
    close(fd)
}

fn test_binding_conflict_matrix_and_address_validation() -> Result<(), Errno> {
    let first = udp_socket(SocketFlags::empty())?;
    let second = udp_socket(SocketFlags::empty())?;
    let wildcard = udp_socket(SocketFlags::empty())?;
    bind_ipv4(first, SockAddrIn::new([127, 0, 0, 1], 45100))?;
    bind_ipv4(second, SockAddrIn::new([127, 0, 0, 2], 45100))?;
    expect_errno(
        bind_ipv4(wildcard, SockAddrIn::new([0; 4], 45100)),
        EADDRINUSE,
    )?;
    close(first)?;
    close(second)?;
    bind_ipv4(wildcard, SockAddrIn::new([0; 4], 45100))?;
    close(wildcard)?;

    let nonlocal = udp_socket(SocketFlags::empty())?;
    expect_errno(
        bind_ipv4(nonlocal, SockAddrIn::new([192, 0, 2, 1], 45101)),
        EADDRNOTAVAIL,
    )?;
    close(nonlocal)
}

fn sockaddr_bytes(address: SockAddrIn) -> [u8; 16] {
    let mut bytes = [0u8; 16];
    let source = unsafe {
        core::slice::from_raw_parts(
            (&address as *const SockAddrIn).cast::<u8>(),
            core::mem::size_of::<SockAddrIn>(),
        )
    };
    bytes.copy_from_slice(source);
    bytes
}

fn test_unaligned_and_length_copy_rules() -> Result<(), Errno> {
    let fd = udp_socket(SocketFlags::empty())?;
    let bytes = sockaddr_bytes(SockAddrIn::new([127, 0, 0, 9], 45110));
    let mut unaligned = [0u8; 17];
    unaligned[1..].copy_from_slice(&bytes);
    unsafe { bind_raw(fd as i32, unaligned.as_ptr().add(1), 16) }?;

    let mut truncated = [0xa5u8; 9];
    let mut truncated_len: socklen_t = 8;
    unsafe { getsockname_raw(fd as i32, truncated.as_mut_ptr().add(1), &mut truncated_len) }?;
    ensure(truncated_len == 16)?;
    ensure(&truncated[5..9] == &[127, 0, 0, 9])?;

    let mut negative_len = socklen_t::MAX;
    expect_errno(
        unsafe { getsockname_raw(fd as i32, truncated.as_mut_ptr(), &mut negative_len) },
        EINVAL,
    )?;
    ensure(negative_len == socklen_t::MAX)?;

    let mut zero_len: socklen_t = 0;
    unsafe { getsockname_raw(fd as i32, 1usize as *mut u8, &mut zero_len) }?;
    ensure(zero_len == 16)?;

    let invalid = udp_socket(SocketFlags::empty())?;
    expect_errno(
        unsafe { bind_raw(invalid as i32, bytes.as_ptr(), 15) },
        EINVAL,
    )?;
    expect_errno(
        unsafe { bind_raw(invalid as i32, bytes.as_ptr(), 129) },
        EINVAL,
    )?;
    expect_errno(
        unsafe { bind_raw(invalid as i32, 1usize as *const u8, 16) },
        EFAULT,
    )?;
    close(invalid)?;
    close(fd)
}

fn test_dup_fork_and_final_release() -> Result<(), Errno> {
    let fd = udp_socket(SocketFlags::NONBLOCK | SocketFlags::CLOEXEC)?;
    bind_ipv4(fd, SockAddrIn::new([127, 0, 0, 1], 45120))?;
    let alias = dup(fd)?;
    ensure(fcntl_getfd(alias)? == 0)?;
    ensure(fcntl_getfl(alias)? & O_NONBLOCK != 0)?;
    close(fd)?;
    ensure(getsockname_ipv4(alias)?.port() == 45120)?;

    let child = match fork()? {
        None => {
            let ok = getsockname_ipv4(alias)
                .map(|address| address.port() == 45120)
                .unwrap_or(false);
            let _ = close(alias);
            exit(if ok { 0 } else { 1 })
        },
        Some(pid) => pid,
    };
    wait_child(child)?;
    ensure(getsockname_ipv4(alias)?.port() == 45120)?;
    close(alias)?;

    let replacement = udp_socket(SocketFlags::empty())?;
    bind_ipv4(replacement, SockAddrIn::new([127, 0, 0, 1], 45120))?;
    close(replacement)
}

fn cloexec_child(fd: Fd) -> Result<(), Errno> {
    expect_errno(getsockname_ipv4(fd), EBADF)
}

fn test_cloexec_exec_projection() -> Result<(), Errno> {
    let fd = udp_socket(SocketFlags::CLOEXEC)?;
    let fd_text = format!("{fd}");
    let child = match fork()? {
        None => {
            let result = execve(
                "/bin/udp-test",
                &["udp-test", "--cloexec-child", fd_text.as_str()],
                &[],
            );
            exit(if result.is_err() { 1 } else { 0 })
        },
        Some(pid) => pid,
    };
    wait_child(child)?;
    close(fd)
}

struct Results {
    passed: usize,
    failed: usize,
}

impl Results {
    fn case(&mut self, name: &str, test: fn() -> Result<(), Errno>) {
        match test() {
            Ok(()) => {
                self.passed += 1;
                println!("UDPTEST:PASS:{name}");
            },
            Err(errno) => {
                self.failed += 1;
                println!("UDPTEST:FAIL:{name}:{errno}");
            },
        }
    }
}

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    let mut argv = args();
    let _ = argv.next();
    if argv.next() == Some("--cloexec-child") {
        let fd = argv
            .next()
            .ok_or(EINVAL)?
            .parse::<Fd>()
            .map_err(|_| EINVAL)?;
        return cloexec_child(fd);
    }

    println!("UDPTEST:START");
    let mut results = Results {
        passed: 0,
        failed: 0,
    };
    results.case("create-errno-flags", test_create_errno_and_flags);
    results.case("unbound-port0-name", test_unbound_and_port_zero_name);
    results.case(
        "binding-matrix-address",
        test_binding_conflict_matrix_and_address_validation,
    );
    results.case(
        "unaligned-length-copy",
        test_unaligned_and_length_copy_rules,
    );
    results.case("dup-fork-final-release", test_dup_fork_and_final_release);
    results.case("cloexec-exec", test_cloexec_exec_projection);

    if results.failed == 0 {
        println!("UDPTEST:SUMMARY:PASS:{}", results.passed);
        Ok(())
    } else {
        println!(
            "UDPTEST:SUMMARY:FAIL:passed={}:failed={}",
            results.passed, results.failed
        );
        Err(EIO)
    }
}
