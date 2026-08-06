//! Repository-owned focused C oracle orchestration.
//!
//! The app/rootfs pipeline owns building and installing these static binaries.
//! User-test only decides when an optional local pair runs and requires the TCP
//! pair when the explicit Stage 5 mode needs its external topology cases.

use anemone_rs::{
    abi::{fs::linux::open::O_RDONLY, time::linux::TimeSpec},
    os::linux::{fs, time::nanosleep},
    prelude::*,
};

struct OracleBinary {
    family: &'static str,
    path: &'static str,
}

const SOCKET_R1: &[OracleBinary] = &[
    OracleBinary {
        family: "glibc",
        path: "/bin/socket-r1-glibc",
    },
    OracleBinary {
        family: "musl",
        path: "/bin/socket-r1-musl",
    },
];

const TCP_R0: &[OracleBinary] = &[
    OracleBinary {
        family: "glibc",
        path: "/bin/tcp-r0-glibc",
    },
    OracleBinary {
        family: "musl",
        path: "/bin/tcp-r0-musl",
    },
];

const TCP_ORACLE_RECLAIM_GRACE: TimeSpec = TimeSpec {
    tv_sec: 11,
    tv_nsec: 0,
};

fn path_exists(path: &str) -> bool {
    let Ok(fd) = fs::openat(fs::AtFd::Cwd, Path::new(path), O_RDONLY, 0) else {
        return false;
    };
    let _ = fs::close(fd);
    true
}

fn installed_pair(name: &str, binaries: &[OracleBinary]) -> bool {
    let installed = binaries
        .iter()
        .filter(|binary| path_exists(binary.path))
        .count();
    if installed == 0 {
        return false;
    }
    if installed != binaries.len() {
        panic!("user-test: incomplete {name} oracle app installation");
    }
    true
}

fn run(binary: &OracleBinary, args: &[&str], label: &str) {
    crate::process::run_execve(binary.path, args, &[], label);
}

fn run_local_pair(name: &str, binaries: &[OracleBinary]) {
    if !installed_pair(name, binaries) {
        return;
    }

    println!("user-test: running {name} dual-libc oracle...");
    for binary in binaries {
        let label = format!("{name} {} oracle", binary.family);
        run(binary, &[name], label.as_str());
    }
    println!("user-test: {name} dual-libc oracle finished.");
}

fn run_tcp_local_pair() {
    if !installed_pair("tcp-r0", TCP_R0) {
        return;
    }

    println!("user-test: running tcp-r0 dual-libc oracle...");
    for (index, binary) in TCP_R0.iter().enumerate() {
        let label = format!("tcp-r0 {} oracle", binary.family);
        run(binary, &["tcp-r0"], label.as_str());
        if index + 1 != TCP_R0.len() {
            // A complete pass deliberately creates several graceful-close
            // episodes. Keep the next libc pass outside the stack's current
            // TIME-WAIT retention window so finite endpoint capacity cannot
            // turn cross-libc sequencing into an oracle result. Remove this
            // wall-clock bridge when the network owner exposes a test-visible
            // quiescence boundary.
            nanosleep(TCP_ORACLE_RECLAIM_GRACE)
                .expect("user-test: failed to wait between tcp-r0 libc passes");
        }
    }
    println!("user-test: tcp-r0 dual-libc oracle finished.");
}

pub(crate) fn run_local() {
    run_local_pair("socket-r1", SOCKET_R1);
    run_tcp_local_pair();
}

pub(crate) fn run_tcp_stage5(peer: &str, port: &str) {
    println!(
        "TCPSTAGE5:START:arch={}:peer={peer}:port={port}",
        crate::ARCH_LABEL
    );
    if !installed_pair("tcp-r0", TCP_R0) {
        panic!("user-test: TCP Stage 5 requires the tcp-r0 oracle app");
    }

    for binary in TCP_R0 {
        for (mode, args) in [
            ("self-external", vec!["tcp-r0", "--self-external"]),
            (
                "remote-external",
                vec!["tcp-r0", "--remote-external", peer, port],
            ),
            ("remote-reset", vec!["tcp-r0", "--remote-reset", peer, port]),
        ] {
            let label = format!("TCP Stage 5 {} {mode}", binary.family);
            run(binary, args.as_slice(), label.as_str());
            println!(
                "TCPSTAGE5:TOPOLOGY:PASS:arch={}:libc={}:mode={mode}",
                crate::ARCH_LABEL,
                binary.family
            );
        }
    }
}
