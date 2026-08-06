//! Focused competition-asset runner for the net-tcp Stage 5 SystemTargets.
//!
//! The mode is explicit so ordinary user-test runs keep their existing profile.
//! Repository-owned static oracles run from the boot root before chroot; this
//! module consumes only the fixed competition CAgent assets after chroot.

use anemone_rs::{
    abi::{
        net::linux::{AF_INET, IPPROTO_TCP, SOCK_STREAM, SockAddrIn},
        time::linux::TimeSpec,
    },
    os::linux::{
        fs::{chdir, close},
        net::{connect_ipv4, socket_raw},
        process::{
            WStatus, WStatusRaw, WaitFor, WaitOptions, execve, exit, fork, setpgid,
            signal::{SigNo, kill},
            wait4,
        },
        time::{gettimeofday, nanosleep},
    },
    prelude::*,
};

use crate::ARCH_LABEL;

const CAGENT_PORT: u16 = 8080;
const CHILD_TIMEOUT_US: i64 = 60_000_000;
const SERVER_STOP_TIMEOUT_US: i64 = 2_000_000;
const SERVER_READY_ATTEMPTS: usize = 200;
const MARKER_DRAIN: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 100_000_000,
};
const WAIT_TICK: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 10_000_000,
};
const CAGENT_ENV: &[&str] = &[
    "PATH=/glibc/bin:/glibc/usr/bin:/bin:/usr/bin:/sbin:/usr/sbin",
    "LD_LIBRARY_PATH=/lib",
];

fn now_us() -> Result<i64, Errno> {
    let time = gettimeofday()?;
    Ok(time
        .tv_sec
        .saturating_mul(1_000_000)
        .saturating_add(time.tv_usec))
}

fn spawn_in_group(
    workdir: &str,
    command: &str,
    argv: &[&str],
    envp: &[&str],
) -> Result<u32, Errno> {
    match fork()? {
        Some(pid) => Ok(pid),
        None => {
            if setpgid(0, 0).is_err() || chdir(workdir).is_err() {
                exit(127);
            }
            let _ = execve(command, argv, envp);
            exit(127)
        },
    }
}

fn poll_child(pid: u32) -> Result<Option<WStatus>, Errno> {
    let mut status = WStatusRaw::EMPTY;
    match wait4(
        WaitFor::ChildWithTgid(pid),
        Some(&mut status),
        WaitOptions::NOHANG,
    ) {
        Ok(Some(waited)) if waited == pid => Ok(Some(status.read())),
        Ok(Some(_)) => Err(ECHILD),
        Ok(None) | Err(EINTR) => Ok(None),
        Err(errno) => Err(errno),
    }
}

fn kill_group(pid: u32, signal: SigNo) -> Result<(), Errno> {
    let pid = i32::try_from(pid).map_err(|_| EINVAL)?;
    match kill(-pid, signal) {
        Ok(()) => Ok(()),
        Err(ESRCH) => match kill(pid, signal) {
            Ok(()) | Err(ESRCH) => Ok(()),
            Err(errno) => Err(errno),
        },
        Err(errno) => Err(errno),
    }
}

fn wait_child_bounded(pid: u32, timeout_us: i64) -> Result<WStatus, Errno> {
    let start = now_us()?;
    loop {
        if let Some(status) = poll_child(pid)? {
            return Ok(status);
        }
        if now_us()?.saturating_sub(start) >= timeout_us {
            kill_group(pid, SigNo::SIGKILL)?;
            loop {
                if let Some(status) = poll_child(pid)? {
                    return Ok(status);
                }
                nanosleep(WAIT_TICK)?;
            }
        }
        nanosleep(WAIT_TICK)?;
    }
}

fn wait_server_ready(server: u32) -> Result<(), Errno> {
    for _ in 0..SERVER_READY_ATTEMPTS {
        if let Some(status) = poll_child(server)? {
            println!("TCPSTAGE5:CAGENT:SERVER-EARLY-EXIT:{status:?}");
            return Err(EIO);
        }
        let socket = unsafe { socket_raw(AF_INET, SOCK_STREAM, IPPROTO_TCP) }?;
        match connect_ipv4(socket, SockAddrIn::new([127, 0, 0, 1], CAGENT_PORT)) {
            Ok(()) => {
                close(socket)?;
                return Ok(());
            },
            Err(ECONNREFUSED) | Err(EINPROGRESS) => {
                close(socket)?;
                nanosleep(WAIT_TICK)?;
            },
            Err(errno) => {
                close(socket)?;
                return Err(errno);
            },
        }
    }
    Err(ETIMEDOUT)
}

fn stop_server(server: u32) -> Result<(), Errno> {
    let pid = i32::try_from(server).map_err(|_| EINVAL)?;
    match kill(pid, SigNo::SIGTERM) {
        Ok(()) | Err(ESRCH) => {},
        Err(errno) => return Err(errno),
    }
    let status = wait_child_bounded(server, SERVER_STOP_TIMEOUT_US)?;
    if matches!(status, WStatus::Exited(0)) {
        Ok(())
    } else {
        println!("TCPSTAGE5:CAGENT:SERVER-CLEANUP-STATUS:{status:?}");
        Err(EIO)
    }
}

fn run_cagent() -> Result<(), Errno> {
    crate::runtime::switch_runtime("glibc");
    // The contest CAgent binaries are static, but their popen() tool resolves
    // /bin/sh. Use the contest image's static BusyBox shell so the tool does
    // not inherit the selectable LTP libc directory through /lib or /lib64.
    crate::runtime::install_bin_sh_ash_wrapper();
    let server = spawn_in_group(
        "/glibc",
        "/glibc/simple_llm_server",
        &["simple_llm_server", "8080"],
        CAGENT_ENV,
    )?;

    let result = (|| {
        wait_server_ready(server)?;
        println!("TCPSTAGE5:CAGENT:READY:arch={ARCH_LABEL}:libc=glibc:pid={server}");

        let argv = [
            "agent_lite",
            "--workspace",
            "/tmp",
            "--host",
            "127.0.0.1",
            "--port",
            "8080",
            "Calculate factorial of 10 using bash",
        ];
        let first = spawn_in_group("/glibc", "/glibc/agent_lite", &argv, CAGENT_ENV)?;
        let second = match spawn_in_group("/glibc", "/glibc/agent_lite", &argv, CAGENT_ENV) {
            Ok(pid) => pid,
            Err(errno) => {
                let _ = kill_group(first, SigNo::SIGKILL);
                let _ = wait_child_bounded(first, SERVER_STOP_TIMEOUT_US);
                return Err(errno);
            },
        };

        let first_status = wait_child_bounded(first, CHILD_TIMEOUT_US)?;
        let second_status = wait_child_bounded(second, CHILD_TIMEOUT_US)?;
        if !matches!(first_status, WStatus::Exited(0))
            || !matches!(second_status, WStatus::Exited(0))
        {
            println!(
                "TCPSTAGE5:CAGENT:CLIENT-FAIL:first={first_status:?}:second={second_status:?}"
            );
            return Err(EIO);
        }
        println!("TCPSTAGE5:CAGENT:CLIENTS:PASS:2");
        Ok(())
    })();

    let cleanup = stop_server(server);
    if cleanup.is_ok() {
        println!("TCPSTAGE5:CAGENT:CLEANUP:PASS:pid={server}");
    }
    result.and(cleanup)?;
    println!("TCPSTAGE5:CAGENT:SUMMARY:PASS:arch={ARCH_LABEL}:libc=glibc");
    Ok(())
}

pub(crate) fn run() -> Result<(), Errno> {
    run_cagent()?;
    println!("TCPSTAGE5:SUMMARY:PASS:arch={ARCH_LABEL}");
    Ok(())
}

pub(crate) fn drain_terminal() -> Result<(), Errno> {
    nanosleep(MARKER_DRAIN)
}
