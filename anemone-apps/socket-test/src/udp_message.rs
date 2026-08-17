use core::{ffi::c_void, mem::offset_of};

use anemone_rs::{
    abi::{
        fs::linux::{IOV_MAX, IoVec},
        net::linux::{MSG_DONTWAIT, MSG_PEEK, MSG_TRUNC, MsgHdr, SockAddrIn},
    },
    os::linux::{
        fs::{Fd, PipeFlags, close, pipe2},
        net::{
            SocketFlags, bind_ipv4, getpeername_ipv4, getsockname_ipv4, icmp_raw_socket,
            recvmsg_raw, sendmsg_raw, udp_socket, unix_stream_pair,
        },
        process::{MmapFlags, MmapProt, mmap, mprotect, munmap, sched_yield},
    },
    prelude::*,
};

const PAGE_SIZE: usize = 4096;
const DELIVERY_RETRIES: usize = 16_384;

#[track_caller]
fn ensure(condition: bool) -> Result<(), Errno> {
    if condition {
        Ok(())
    } else {
        let caller = core::panic::Location::caller();
        println!("UDPMSGTST:ASSERT:{}:{}", caller.line(), caller.column());
        Err(EIO)
    }
}

#[track_caller]
fn expect_errno<T>(result: Result<T, Errno>, expected: Errno) -> Result<(), Errno> {
    match result {
        Err(actual) if actual == expected => Ok(()),
        Err(actual) => {
            let caller = core::panic::Location::caller();
            println!(
                "UDPMSGTST:ERRNO:{}:{}:expected={}:actual={}",
                caller.line(),
                caller.column(),
                expected,
                actual
            );
            Err(EIO)
        },
        Ok(_) => {
            let caller = core::panic::Location::caller();
            println!(
                "UDPMSGTST:ERRNO:{}:{}:expected={}:actual=success",
                caller.line(),
                caller.column(),
                expected
            );
            Err(EIO)
        },
    }
}

fn read_iovec(bytes: &[u8]) -> IoVec {
    IoVec {
        iov_base: (bytes.as_ptr() as *mut c_void).into(),
        iov_len: bytes.len() as u64,
    }
}

fn write_iovec(bytes: &mut [u8]) -> IoVec {
    IoVec {
        iov_base: bytes.as_mut_ptr().cast::<c_void>().into(),
        iov_len: bytes.len() as u64,
    }
}

fn message(iovecs: &mut [IoVec]) -> MsgHdr {
    MsgHdr {
        msg_iov: iovecs.as_mut_ptr().into(),
        msg_iovlen: iovecs.len() as u64,
        ..MsgHdr::default()
    }
}

fn bound_socket() -> Result<(Fd, SockAddrIn), Errno> {
    let fd = udp_socket(SocketFlags::NONBLOCK)?;
    bind_ipv4(fd, SockAddrIn::new([127, 0, 0, 1], 0))?;
    Ok((fd, getsockname_ipv4(fd)?))
}

fn connected_pair() -> Result<(Fd, SockAddrIn, Fd), Errno> {
    let (server, server_name) = bound_socket()?;
    let client = udp_socket(SocketFlags::NONBLOCK)?;
    anemone_rs::os::linux::net::connect_ipv4(client, server_name)?;
    Ok((server, server_name, client))
}

fn loopback_peer(fd: Fd) -> Result<SockAddrIn, Errno> {
    Ok(SockAddrIn::new(
        [127, 0, 0, 1],
        getsockname_ipv4(fd)?.port(),
    ))
}

fn recvmsg_retry(fd: Fd, header: *mut MsgHdr, flags: i32) -> Result<usize, Errno> {
    for _ in 0..DELIVERY_RETRIES {
        match unsafe { recvmsg_raw(fd as i32, header, flags) } {
            Err(EAGAIN) => sched_yield()?,
            result => return result,
        }
    }
    Err(ETIMEDOUT)
}

fn expect_empty(fd: Fd) -> Result<(), Errno> {
    let mut byte = [0u8; 1];
    let mut iovecs = [write_iovec(&mut byte)];
    let mut header = message(&mut iovecs);
    expect_errno(
        unsafe { recvmsg_raw(fd as i32, &mut header, MSG_DONTWAIT) },
        EAGAIN,
    )
}

fn receive_payload(fd: Fd, expected: &[u8]) -> Result<SockAddrIn, Errno> {
    let mut payload = [0u8; 64];
    let mut iovecs = [write_iovec(&mut payload)];
    let mut peer = SockAddrIn::default();
    let mut header = message(&mut iovecs);
    header.msg_name = (&mut peer as *mut SockAddrIn).cast::<c_void>().into();
    header.msg_namelen = core::mem::size_of::<SockAddrIn>() as i32;
    let received = recvmsg_retry(fd, &mut header, MSG_DONTWAIT)?;
    ensure(&payload[..received] == expected)?;
    ensure(header.msg_namelen as usize == core::mem::size_of::<SockAddrIn>())?;
    Ok(peer)
}

fn send_explicit(fd: Fd, peer: SockAddrIn, payload: &[u8]) -> Result<usize, Errno> {
    let mut iovecs = [read_iovec(payload)];
    let mut header = message(&mut iovecs);
    header.msg_name = (&peer as *const SockAddrIn)
        .cast_mut()
        .cast::<c_void>()
        .into();
    header.msg_namelen = core::mem::size_of::<SockAddrIn>() as i32;
    unsafe { sendmsg_raw(fd as i32, &header, MSG_DONTWAIT) }
}

fn map_pages(count: usize) -> Result<*mut u8, Errno> {
    mmap(
        0,
        PAGE_SIZE * count,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )
    .map(|mapping| mapping.as_ptr())
}

fn test_layout_fd_and_family_boundary() -> Result<(), Errno> {
    ensure(core::mem::size_of::<MsgHdr>() == 56)?;
    ensure(core::mem::align_of::<MsgHdr>() == 8)?;
    ensure(offset_of!(MsgHdr, msg_name) == 0)?;
    ensure(offset_of!(MsgHdr, msg_namelen) == 8)?;
    ensure(offset_of!(MsgHdr, msg_iov) == 16)?;
    ensure(offset_of!(MsgHdr, msg_iovlen) == 24)?;
    ensure(offset_of!(MsgHdr, msg_control) == 32)?;
    ensure(offset_of!(MsgHdr, msg_controllen) == 40)?;
    ensure(offset_of!(MsgHdr, msg_flags) == 48)?;

    let bad_header = 1usize as *const MsgHdr;
    expect_errno(unsafe { sendmsg_raw(-1, bad_header, 0) }, EBADF)?;
    expect_errno(unsafe { recvmsg_raw(-1, bad_header.cast_mut(), 0) }, EBADF)?;

    let pipe = pipe2(PipeFlags::empty())?;
    expect_errno(
        unsafe { sendmsg_raw(pipe.1 as i32, bad_header, 0) },
        ENOTSOCK,
    )?;
    expect_errno(
        unsafe { recvmsg_raw(pipe.0 as i32, bad_header.cast_mut(), 0) },
        ENOTSOCK,
    )?;
    close(pipe.1)?;
    close(pipe.0)?;

    let udp = udp_socket(SocketFlags::NONBLOCK)?;
    expect_errno(unsafe { sendmsg_raw(udp as i32, bad_header, 0) }, EFAULT)?;
    expect_errno(
        unsafe { recvmsg_raw(udp as i32, bad_header.cast_mut(), MSG_DONTWAIT) },
        EFAULT,
    )?;
    close(udp)?;

    let header = MsgHdr::default();
    let unix = unix_stream_pair(SocketFlags::NONBLOCK)?;
    ensure(unsafe { sendmsg_raw(unix.0 as i32, &header, 0) }? == 0)?;
    let mut recv_header = MsgHdr::default();
    expect_errno(
        unsafe { recvmsg_raw(unix.0 as i32, &mut recv_header, MSG_DONTWAIT) },
        EAGAIN,
    )?;
    close(unix.1)?;
    close(unix.0)?;

    let raw = icmp_raw_socket(SocketFlags::NONBLOCK)?;
    expect_errno(unsafe { sendmsg_raw(raw as i32, &header, 0) }, EOPNOTSUPP)?;
    expect_errno(
        unsafe { recvmsg_raw(raw as i32, &mut recv_header, MSG_DONTWAIT) },
        EOPNOTSUPP,
    )?;
    close(raw)
}

fn test_header_and_iovec_admission() -> Result<(), Errno> {
    let (server, server_name, client) = connected_pair()?;

    let mut negative_name = MsgHdr {
        msg_name: anemone_rs::abi::RawUserAddr64::from_bits(1),
        msg_namelen: -1,
        ..MsgHdr::default()
    };
    expect_errno(
        unsafe { sendmsg_raw(client as i32, &negative_name, MSG_DONTWAIT) },
        EINVAL,
    )?;
    negative_name.msg_name = anemone_rs::abi::RawUserAddr64::NULL;
    ensure(unsafe { sendmsg_raw(client as i32, &negative_name, MSG_DONTWAIT) }? == 0)?;
    let mut zero_header = MsgHdr::default();
    ensure(recvmsg_retry(server, &mut zero_header, MSG_DONTWAIT)? == 0)?;

    let mut oversized_name = [0u8; 128];
    unsafe {
        oversized_name
            .as_mut_ptr()
            .cast::<SockAddrIn>()
            .write_unaligned(server_name)
    };
    let oversized_name_header = MsgHdr {
        msg_name: oversized_name.as_mut_ptr().cast::<c_void>().into(),
        msg_namelen: i32::MAX,
        ..MsgHdr::default()
    };
    ensure(unsafe { sendmsg_raw(client as i32, &oversized_name_header, MSG_DONTWAIT) }? == 0)?;
    ensure(recvmsg_retry(server, &mut zero_header, MSG_DONTWAIT)? == 0)?;

    let bad_vector = MsgHdr {
        msg_iov: anemone_rs::abi::RawUserAddr64::from_bits(1),
        msg_iovlen: 1,
        ..MsgHdr::default()
    };
    expect_errno(
        unsafe { sendmsg_raw(client as i32, &bad_vector, MSG_DONTWAIT) },
        EFAULT,
    )?;

    let too_many = MsgHdr {
        msg_iov: anemone_rs::abi::RawUserAddr64::NULL,
        msg_iovlen: (IOV_MAX + 1) as u64,
        ..MsgHdr::default()
    };
    expect_errno(
        unsafe { sendmsg_raw(client as i32, &too_many, MSG_DONTWAIT) },
        EMSGSIZE,
    )?;

    let mut overflow_iovecs = [
        IoVec {
            iov_base: anemone_rs::abi::RawUserAddr64::NULL,
            iov_len: u64::MAX,
        },
        IoVec {
            iov_base: anemone_rs::abi::RawUserAddr64::NULL,
            iov_len: 1,
        },
    ];
    let overflow = message(&mut overflow_iovecs);
    expect_errno(
        unsafe { sendmsg_raw(client as i32, &overflow, MSG_DONTWAIT) },
        EFAULT,
    )?;

    let mut clipped_iovec = [IoVec {
        iov_base: anemone_rs::abi::RawUserAddr64::from_bits(1),
        iov_len: u64::MAX,
    }];
    let clipped = message(&mut clipped_iovec);
    expect_errno(
        unsafe { sendmsg_raw(client as i32, &clipped, MSG_DONTWAIT) },
        EMSGSIZE,
    )?;
    expect_empty(server)?;

    close(client)?;
    close(server)
}

fn test_send_transaction_and_rejection() -> Result<(), Errno> {
    let (first, first_name) = bound_socket()?;
    let (second, second_name) = bound_socket()?;
    let client = udp_socket(SocketFlags::NONBLOCK)?;

    let left = *b"ex";
    let right = *b"plicit";
    let mut explicit_iovecs = [read_iovec(&left), read_iovec(&right)];
    let mut explicit = message(&mut explicit_iovecs);
    explicit.msg_name = (&first_name as *const SockAddrIn)
        .cast_mut()
        .cast::<c_void>()
        .into();
    explicit.msg_namelen = core::mem::size_of::<SockAddrIn>() as i32;
    ensure(unsafe { sendmsg_raw(client as i32, &explicit, MSG_DONTWAIT) }? == 8)?;
    receive_payload(first, b"explicit")?;

    anemone_rs::os::linux::net::connect_ipv4(client, first_name)?;
    let default_payload = *b"default";
    let mut default_iovecs = [read_iovec(&default_payload)];
    let default = message(&mut default_iovecs);
    ensure(unsafe { sendmsg_raw(client as i32, &default, MSG_DONTWAIT) }? == 7)?;
    receive_payload(first, b"default")?;

    let override_payload = *b"override";
    let mut override_iovecs = [read_iovec(&override_payload)];
    let mut override_message = message(&mut override_iovecs);
    override_message.msg_name = (&second_name as *const SockAddrIn)
        .cast_mut()
        .cast::<c_void>()
        .into();
    override_message.msg_namelen = core::mem::size_of::<SockAddrIn>() as i32;
    ensure(unsafe { sendmsg_raw(client as i32, &override_message, MSG_DONTWAIT) }? == 8)?;
    receive_payload(second, b"override")?;
    ensure(getpeername_ipv4(client)? == first_name)?;

    let unconnected = udp_socket(SocketFlags::NONBLOCK)?;
    expect_errno(
        unsafe { sendmsg_raw(unconnected as i32, &default, MSG_DONTWAIT) },
        EDESTADDRREQ,
    )?;
    close(unconnected)?;

    let control = MsgHdr {
        msg_iov: default_iovecs.as_mut_ptr().into(),
        msg_iovlen: 1,
        msg_control: anemone_rs::abi::RawUserAddr64::from_bits(1),
        msg_controllen: 1,
        ..MsgHdr::default()
    };
    expect_errno(
        unsafe { sendmsg_raw(client as i32, &control, MSG_DONTWAIT) },
        EOPNOTSUPP,
    )?;
    expect_empty(first)?;
    expect_errno(
        unsafe { sendmsg_raw(client as i32, &default, MSG_DONTWAIT | 1) },
        EOPNOTSUPP,
    )?;
    expect_empty(first)?;
    let mut receive_header = MsgHdr::default();
    expect_errno(
        unsafe { recvmsg_raw(first as i32, &mut receive_header, MSG_DONTWAIT | 1) },
        EOPNOTSUPP,
    )?;

    let mut ignored_header_flags = default;
    ignored_header_flags.msg_flags = u32::MAX;
    ensure(unsafe { sendmsg_raw(client as i32, &ignored_header_flags, MSG_DONTWAIT) }? == 7)?;
    receive_payload(first, b"default")?;

    let prefix = *b"prefix";
    let protected = map_pages(1)?;
    mprotect(protected, PAGE_SIZE, MmapProt::empty())?;
    let mut fault_iovecs = [
        read_iovec(&prefix),
        IoVec {
            iov_base: protected.cast::<c_void>().into(),
            iov_len: 1,
        },
    ];
    let fault = message(&mut fault_iovecs);
    expect_errno(
        unsafe { sendmsg_raw(client as i32, &fault, MSG_DONTWAIT) },
        EFAULT,
    )?;
    expect_empty(first)?;
    mprotect(
        protected,
        PAGE_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
    )?;
    munmap(protected, PAGE_SIZE)?;

    close(client)?;
    close(second)?;
    close(first)
}

fn test_receive_name_control_and_scatter() -> Result<(), Errno> {
    let (server, server_name) = bound_socket()?;
    let client = udp_socket(SocketFlags::NONBLOCK)?;

    send_explicit(client, server_name, b"scatter")?;
    let mut left = [0u8; 3];
    let mut right = [0u8; 4];
    let mut iovecs = [write_iovec(&mut left), write_iovec(&mut right)];
    let mut peer = SockAddrIn::default();
    let mut header = message(&mut iovecs);
    header.msg_name = (&mut peer as *mut SockAddrIn).cast::<c_void>().into();
    header.msg_namelen = core::mem::size_of::<SockAddrIn>() as i32;
    header.msg_control = anemone_rs::abi::RawUserAddr64::from_bits(1);
    header.msg_controllen = 64;
    header.msg_flags = u32::MAX;
    ensure(recvmsg_retry(server, &mut header, MSG_DONTWAIT)? == 7)?;
    ensure(&left == b"sca" && &right == b"tter")?;
    ensure(peer == loopback_peer(client)?)?;
    ensure(header.msg_namelen as usize == core::mem::size_of::<SockAddrIn>())?;
    ensure(header.msg_flags == 0 && header.msg_controllen == 0)?;

    send_explicit(client, server_name, b"empty-name")?;
    let mut payload = [0u8; 16];
    let mut iovecs = [write_iovec(&mut payload)];
    let mut short_name = [0xa5u8; core::mem::size_of::<SockAddrIn>()];
    let mut header = message(&mut iovecs);
    header.msg_name = short_name.as_mut_ptr().cast::<c_void>().into();
    header.msg_namelen = 0;
    ensure(recvmsg_retry(server, &mut header, MSG_DONTWAIT)? == 10)?;
    ensure(short_name == [0xa5; core::mem::size_of::<SockAddrIn>()])?;
    ensure(header.msg_namelen as usize == core::mem::size_of::<SockAddrIn>())?;

    send_explicit(client, server_name, b"short-name")?;
    let mut short_name = [0xa5u8; 4];
    let mut header = message(&mut iovecs);
    header.msg_name = short_name.as_mut_ptr().cast::<c_void>().into();
    header.msg_namelen = short_name.len() as i32;
    ensure(recvmsg_retry(server, &mut header, MSG_DONTWAIT)? == 10)?;
    let client_name = getsockname_ipv4(client)?;
    let peer_bytes = unsafe {
        core::slice::from_raw_parts(
            (&client_name as *const SockAddrIn).cast::<u8>(),
            core::mem::size_of::<SockAddrIn>(),
        )
    };
    ensure(short_name == peer_bytes[..4])?;
    ensure(header.msg_namelen as usize == core::mem::size_of::<SockAddrIn>())?;

    send_explicit(client, server_name, b"null-name")?;
    let mut header = message(&mut iovecs);
    header.msg_namelen = -1;
    ensure(recvmsg_retry(server, &mut header, MSG_DONTWAIT)? == 9)?;
    ensure(header.msg_namelen == -1)?;

    close(client)?;
    close(server)
}

fn test_truncate_peek_and_zero_capacity() -> Result<(), Errno> {
    let (server, server_name) = bound_socket()?;
    let client = udp_socket(SocketFlags::NONBLOCK)?;
    send_explicit(client, server_name, b"truncate")?;

    let mut short = [0u8; 2];
    let mut iovecs = [write_iovec(&mut short)];
    let mut header = message(&mut iovecs);
    ensure(recvmsg_retry(server, &mut header, MSG_DONTWAIT | MSG_PEEK)? == 2)?;
    ensure(&short == b"tr" && header.msg_flags == MSG_TRUNC as u32)?;

    short.fill(0);
    header.msg_flags = 0;
    ensure(recvmsg_retry(server, &mut header, MSG_DONTWAIT | MSG_TRUNC)? == 8)?;
    ensure(&short == b"tr" && header.msg_flags == MSG_TRUNC as u32)?;
    expect_empty(server)?;

    send_explicit(client, server_name, b"ok")?;
    header.msg_flags = u32::MAX;
    ensure(recvmsg_retry(server, &mut header, MSG_DONTWAIT | MSG_TRUNC)? == 2)?;
    ensure(header.msg_flags == 0)?;

    send_explicit(client, server_name, b"zero-capacity")?;
    let mut zero = MsgHdr::default();
    ensure(recvmsg_retry(server, &mut zero, MSG_DONTWAIT | MSG_TRUNC)? == 13)?;
    ensure(zero.msg_flags == MSG_TRUNC as u32)?;
    expect_empty(server)?;

    ensure(send_explicit(client, server_name, b"")? == 0)?;
    zero.msg_flags = u32::MAX;
    ensure(recvmsg_retry(server, &mut zero, MSG_DONTWAIT)? == 0)?;
    ensure(zero.msg_flags == 0)?;
    expect_empty(server)?;

    close(client)?;
    close(server)
}

fn test_payload_fault_consume_and_peek() -> Result<(), Errno> {
    let (server, server_name) = bound_socket()?;
    let client = udp_socket(SocketFlags::NONBLOCK)?;

    send_explicit(client, server_name, b"fault")?;
    let mut prefix = [0u8; 2];
    let protected = map_pages(1)?;
    mprotect(protected, PAGE_SIZE, MmapProt::empty())?;
    let mut iovecs = [
        write_iovec(&mut prefix),
        IoVec {
            iov_base: protected.cast::<c_void>().into(),
            iov_len: 3,
        },
    ];
    let mut header = message(&mut iovecs);
    header.msg_flags = 0xa5a5_a5a5;
    header.msg_controllen = 7;
    expect_errno(recvmsg_retry(server, &mut header, MSG_DONTWAIT), EFAULT)?;
    ensure(&prefix == b"fa")?;
    ensure(header.msg_flags == 0xa5a5_a5a5 && header.msg_controllen == 7)?;
    expect_empty(server)?;

    send_explicit(client, server_name, b"peek-fault")?;
    prefix.fill(0);
    expect_errno(
        recvmsg_retry(server, &mut header, MSG_DONTWAIT | MSG_PEEK),
        EFAULT,
    )?;
    ensure(&prefix == b"pe")?;
    receive_payload(server, b"peek-fault")?;
    expect_empty(server)?;
    mprotect(
        protected,
        PAGE_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
    )?;
    munmap(protected, PAGE_SIZE)?;

    close(client)?;
    close(server)
}

fn test_name_and_header_output_faults() -> Result<(), Errno> {
    let (server, server_name) = bound_socket()?;
    let client = udp_socket(SocketFlags::NONBLOCK)?;

    send_explicit(client, server_name, b"name-fault")?;
    let mut payload = [0u8; 16];
    let mut iovecs = [write_iovec(&mut payload)];
    let mut header = message(&mut iovecs);
    header.msg_name = anemone_rs::abi::RawUserAddr64::from_bits(1);
    header.msg_namelen = core::mem::size_of::<SockAddrIn>() as i32;
    header.msg_flags = u32::MAX;
    header.msg_controllen = 9;
    expect_errno(recvmsg_retry(server, &mut header, MSG_DONTWAIT), EFAULT)?;
    ensure(&payload[..10] == b"name-fault")?;
    ensure(header.msg_flags == u32::MAX && header.msg_controllen == 9)?;
    expect_empty(server)?;

    send_explicit(client, server_name, b"peek-name")?;
    payload.fill(0);
    expect_errno(
        recvmsg_retry(server, &mut header, MSG_DONTWAIT | MSG_PEEK),
        EFAULT,
    )?;
    ensure(&payload[..9] == b"peek-name")?;
    receive_payload(server, b"peek-name")?;
    expect_empty(server)?;

    send_explicit(client, server_name, b"name-length")?;
    payload.fill(0);
    let mut peer = [0u8; core::mem::size_of::<SockAddrIn>()];
    let read_only = map_pages(1)?;
    let read_only_header = read_only.cast::<MsgHdr>();
    let mut staged = message(&mut iovecs);
    staged.msg_name = peer.as_mut_ptr().cast::<c_void>().into();
    staged.msg_namelen = peer.len() as i32;
    staged.msg_flags = u32::MAX;
    staged.msg_controllen = 11;
    unsafe { read_only_header.write(staged) };
    mprotect(read_only, PAGE_SIZE, MmapProt::PROT_READ)?;
    expect_errno(
        recvmsg_retry(server, read_only_header, MSG_DONTWAIT),
        EFAULT,
    )?;
    ensure(&payload[..11] == b"name-length")?;
    let expected_peer = loopback_peer(client)?;
    let expected_peer = unsafe {
        core::slice::from_raw_parts(
            (&expected_peer as *const SockAddrIn).cast::<u8>(),
            core::mem::size_of::<SockAddrIn>(),
        )
    };
    ensure(peer == expected_peer)?;
    let after = unsafe { read_only_header.read() };
    ensure(after.msg_namelen == staged.msg_namelen)?;
    ensure(after.msg_flags == u32::MAX && after.msg_controllen == 11)?;
    mprotect(
        read_only,
        PAGE_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
    )?;
    munmap(read_only, PAGE_SIZE)?;
    expect_empty(server)?;

    send_explicit(client, server_name, b"flags-fault")?;
    payload.fill(0);
    let split = map_pages(2)?;
    let split_header = unsafe { split.add(PAGE_SIZE - offset_of!(MsgHdr, msg_flags)) };
    let mut staged = message(&mut iovecs);
    staged.msg_flags = u32::MAX;
    staged.msg_controllen = 12;
    unsafe { split_header.cast::<MsgHdr>().write_unaligned(staged) };
    mprotect(
        unsafe { split.add(PAGE_SIZE) },
        PAGE_SIZE,
        MmapProt::PROT_READ,
    )?;
    expect_errno(
        recvmsg_retry(server, split_header.cast(), MSG_DONTWAIT | MSG_PEEK),
        EFAULT,
    )?;
    ensure(&payload[..11] == b"flags-fault")?;
    let after = unsafe { split_header.cast::<MsgHdr>().read_unaligned() };
    ensure(after.msg_flags == u32::MAX && after.msg_controllen == 12)?;
    expect_errno(
        recvmsg_retry(server, split_header.cast(), MSG_DONTWAIT),
        EFAULT,
    )?;
    mprotect(
        unsafe { split.add(PAGE_SIZE) },
        PAGE_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
    )?;
    munmap(split, PAGE_SIZE * 2)?;
    expect_empty(server)?;

    send_explicit(client, server_name, b"control-fault")?;
    payload.fill(0);
    let split = map_pages(2)?;
    let split_header = unsafe { split.add(PAGE_SIZE - offset_of!(MsgHdr, msg_flags)) };
    let mut staged = message(&mut iovecs);
    staged.msg_flags = u32::MAX;
    staged.msg_controllen = 13;
    unsafe { split_header.cast::<MsgHdr>().write_unaligned(staged) };
    mprotect(split, PAGE_SIZE, MmapProt::PROT_READ)?;
    expect_errno(
        recvmsg_retry(server, split_header.cast(), MSG_DONTWAIT | MSG_PEEK),
        EFAULT,
    )?;
    ensure(&payload[..13] == b"control-fault")?;
    let after = unsafe { split_header.cast::<MsgHdr>().read_unaligned() };
    ensure(after.msg_flags == 0 && after.msg_controllen == 13)?;
    expect_errno(
        recvmsg_retry(server, split_header.cast(), MSG_DONTWAIT),
        EFAULT,
    )?;
    mprotect(split, PAGE_SIZE, MmapProt::PROT_READ | MmapProt::PROT_WRITE)?;
    munmap(split, PAGE_SIZE * 2)?;
    expect_empty(server)?;

    close(client)?;
    close(server)
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
                println!("UDPMSGTST:PASS:{name}");
            },
            Err(errno) => {
                self.failed += 1;
                println!("UDPMSGTST:FAIL:{name}:{errno}");
            },
        }
    }
}

pub(crate) fn run() -> Result<(), Errno> {
    println!("UDPMSGTST:START");
    let mut results = Results {
        passed: 0,
        failed: 0,
    };
    results.case("layout-fd-family", test_layout_fd_and_family_boundary);
    results.case("header-iovec-admission", test_header_and_iovec_admission);
    results.case(
        "send-transaction-rejection",
        test_send_transaction_and_rejection,
    );
    results.case(
        "recv-name-control-scatter",
        test_receive_name_control_and_scatter,
    );
    results.case("truncate-peek-zero", test_truncate_peek_and_zero_capacity);
    results.case(
        "payload-fault-consume-peek",
        test_payload_fault_consume_and_peek,
    );
    results.case("output-fault-order", test_name_and_header_output_faults);
    if results.failed == 0 {
        println!("UDPMSGTST:SUMMARY:PASS:{}", results.passed);
        Ok(())
    } else {
        println!(
            "UDPMSGTST:SUMMARY:FAIL:passed={}:failed={}",
            results.passed, results.failed
        );
        Err(EIO)
    }
}
