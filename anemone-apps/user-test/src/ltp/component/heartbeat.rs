//! Diagnostic heartbeat child for long LTP profiles.
//!
//! Heartbeat state is diagnostic-only: snapshots help identify which
//! root/group/case was active if the runner stalls, but they never drive runner
//! decisions. `LtpComponents` is the only owner of this type.

use anemone_rs::{
    abi::time::linux::TimeSpec,
    os::linux::{
        fs::{Fd, PipeFlags, close, pipe2, read, write},
        process::{
            WStatusRaw, WaitFor, WaitOptions, exit, fork, sched_yield,
            signal::{SigNo, kill},
            wait4,
        },
        time::nanosleep,
    },
    prelude::*,
};

use crate::ltp::{config::LtpRunPolicy, time::now_us};

pub(super) struct LtpHeartbeat {
    // Diagnostic-only child identity and control pipe. They are not protocol
    // state for case execution; the runner must keep working if heartbeat setup
    // fails or the pipe closes.
    child: Option<u32>,
    write_fd: Option<Fd>,
    snapshot_seq: u64,
}

impl LtpHeartbeat {
    pub(super) fn start_or_disabled(policy: &LtpRunPolicy) -> Self {
        match Self::start(policy) {
            Ok(heartbeat) => heartbeat,
            Err(errno) => {
                println!("user-test: LTP heartbeat disabled: {errno:?}");
                Self::disabled()
            },
        }
    }

    fn start(policy: &LtpRunPolicy) -> Result<Self, Errno> {
        let (read_fd, write_fd) = pipe2(PipeFlags::CLOEXEC | PipeFlags::NONBLOCK)?;
        // The heartbeat channel is diagnostic-only. CLOEXEC keeps the long-lived
        // LTP case image from inheriting the writer and hiding parent shutdown.
        let fork_result = match fork() {
            Ok(result) => result,
            Err(errno) => {
                let _ = close(read_fd);
                let _ = close(write_fd);
                return Err(errno);
            },
        };

        match fork_result {
            Some(child) => {
                let _ = close(read_fd);
                let mut heartbeat = Self {
                    child: Some(child),
                    write_fd: Some(write_fd),
                    snapshot_seq: 0,
                };
                heartbeat.publish("started", "-", "-", "-", 0);
                Ok(heartbeat)
            },
            None => {
                let _ = close(write_fd);
                run_ltp_heartbeat_child(
                    read_fd,
                    policy.heartbeat_print_interval_us,
                    policy.heartbeat_sleep_tick_seconds,
                );
            },
        }
    }

    fn disabled() -> Self {
        Self {
            child: None,
            write_fd: None,
            snapshot_seq: 0,
        }
    }

    pub(super) fn publish(
        &mut self,
        phase: &str,
        root: &str,
        group: &str,
        case: &str,
        case_pgrp: u32,
    ) {
        let Some(fd) = self.write_fd else {
            return;
        };

        self.snapshot_seq += 1;
        let now = now_us().unwrap_or(-1);
        let message = format!(
            "snapshot_seq={} now_us={} phase={} root={} group={} case={} case_pgrp={}\n",
            self.snapshot_seq, now, phase, root, group, case, case_pgrp,
        );

        match write(fd, message.as_bytes()) {
            Ok(_) | Err(EAGAIN) | Err(EINTR) => {},
            Err(EPIPE) => {
                println!("user-test: LTP heartbeat pipe closed; disabling updates");
                self.close_write_fd();
            },
            Err(errno) => {
                println!("user-test: LTP heartbeat update failed: {errno:?}");
                self.close_write_fd();
            },
        }
    }

    pub(super) fn finish(&mut self, policy: &LtpRunPolicy) {
        self.publish("finished", "-", "-", "-", 0);
        self.close_write_fd();
        self.wait_child_exit(policy);
    }

    fn close_write_fd(&mut self) {
        if let Some(fd) = self.write_fd.take() {
            let _ = close(fd);
        }
    }

    fn wait_child_exit(&mut self, policy: &LtpRunPolicy) {
        let Some(child) = self.child.take() else {
            return;
        };

        let start_us = now_us().unwrap_or(0);
        let mut killed = false;
        loop {
            let mut wstatus = WStatusRaw::EMPTY;
            match wait4(
                WaitFor::ChildWithTgid(child),
                Some(&mut wstatus),
                WaitOptions::NOHANG,
            ) {
                Ok(Some(_)) | Err(ECHILD) => return,
                Ok(None) | Err(EINTR) => {},
                Err(errno) => {
                    println!("user-test: LTP heartbeat wait failed: {errno:?}");
                    return;
                },
            }

            let elapsed_us = crate::ltp::time::elapsed_us_since(start_us)
                .unwrap_or(policy.heartbeat_stop_grace_us);
            if !killed && elapsed_us >= policy.heartbeat_stop_grace_us {
                println!("user-test: LTP heartbeat did not exit after control pipe close; killing");
                let _ = kill(child as i32, SigNo::SIGKILL);
                killed = true;
            }
            if killed
                && elapsed_us
                    >= policy
                        .heartbeat_stop_grace_us
                        .saturating_add(crate::ltp::time::MICROS_PER_SECOND)
            {
                println!("user-test: LTP heartbeat child not reaped after kill; continuing");
                return;
            }

            let _ = sched_yield();
        }
    }
}

impl Drop for LtpHeartbeat {
    fn drop(&mut self) {
        self.close_write_fd();
    }
}

enum LtpHeartbeatPipe {
    Open,
    Closed,
}

fn run_ltp_heartbeat_child(
    read_fd: Fd,
    heartbeat_print_interval_us: i64,
    heartbeat_sleep_tick_seconds: i64,
) -> ! {
    let mut heartbeat_seq = 0u64;
    let mut snapshot =
        String::from("snapshot_seq=0 now_us=-1 phase=starting root=- group=- case=- case_pgrp=0");
    let mut pending = String::new();
    let mut last_snapshot_us = now_us().unwrap_or(0);
    let mut next_print_us = last_snapshot_us;

    loop {
        match drain_ltp_heartbeat_pipe(read_fd, &mut snapshot, &mut pending, &mut last_snapshot_us)
        {
            LtpHeartbeatPipe::Open => {},
            LtpHeartbeatPipe::Closed => {
                println!("user-test-heartbeat: control_pipe_closed");
                let _ = close(read_fd);
                exit(0);
            },
        }

        let now = now_us().unwrap_or(last_snapshot_us);
        if now >= next_print_us {
            heartbeat_seq += 1;
            println!(
                "user-test-heartbeat: seq={} now_us={} {} stale_for_ms={}",
                heartbeat_seq,
                now,
                snapshot,
                now.saturating_sub(last_snapshot_us) / 1000,
            );
            next_print_us = now.saturating_add(heartbeat_print_interval_us);
        }

        sleep_ltp_heartbeat_tick(heartbeat_sleep_tick_seconds);
    }
}

fn drain_ltp_heartbeat_pipe(
    fd: Fd,
    snapshot: &mut String,
    pending: &mut String,
    last_snapshot_us: &mut i64,
) -> LtpHeartbeatPipe {
    let mut buf = [0u8; 256];
    loop {
        match read(fd, &mut buf) {
            Ok(0) => return LtpHeartbeatPipe::Closed,
            Ok(count) => {
                for &byte in &buf[..count] {
                    if byte == b'\n' {
                        snapshot.clear();
                        snapshot.push_str(pending.as_str());
                        pending.clear();
                        *last_snapshot_us = now_us().unwrap_or(*last_snapshot_us);
                    } else if byte.is_ascii_graphic() || byte == b' ' {
                        pending.push(byte as char);
                    } else {
                        pending.push('?');
                    }
                }
            },
            Err(EAGAIN) => return LtpHeartbeatPipe::Open,
            Err(EINTR) => {},
            Err(errno) => {
                println!("user-test-heartbeat: read failed: {errno:?}");
                return LtpHeartbeatPipe::Closed;
            },
        }
    }
}

fn sleep_ltp_heartbeat_tick(heartbeat_sleep_tick_seconds: i64) {
    let duration = TimeSpec {
        tv_sec: heartbeat_sleep_tick_seconds,
        tv_nsec: 0,
    };

    loop {
        match nanosleep(duration) {
            Ok(()) => return,
            Err(EINTR) => {},
            Err(errno) => {
                println!("user-test-heartbeat: nanosleep failed: {errno:?}");
                let _ = sched_yield();
                return;
            },
        }
    }
}
