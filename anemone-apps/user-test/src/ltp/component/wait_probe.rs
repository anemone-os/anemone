//! Wait-loop probe for stalled-case diagnosis.
//!
//! The probe is logging-only. It records coarse wait-loop progress around
//! `gettimeofday`, `wait4`, and `sched_yield`, but must not change timeout,
//! reaping, or retry semantics.

use anemone_rs::{os::linux::process::sched_yield, prelude::*};

use crate::ltp::config::LtpRunPolicy;

pub(in crate::ltp) struct LtpWaitLoopProbe<'a> {
    // Snapshot labels are diagnostic-only. They are copied from the caller at
    // probe creation and must not become a source of runner state decisions.
    root_label: &'a str,
    group_name: &'a str,
    case_name: &'a str,
    tid: u32,
    phase: &'static str,
    seq: u64,
    last_now_us: i64,
    next_probe_us: i64,
    log_iteration: bool,
    probe_interval_us: i64,
}

impl<'a> LtpWaitLoopProbe<'a> {
    pub(in crate::ltp) fn new(
        root_label: &'a str,
        group_name: &'a str,
        case_name: &'a str,
        tid: u32,
        policy: &LtpRunPolicy,
    ) -> Self {
        Self {
            root_label,
            group_name,
            case_name,
            tid,
            phase: "wait",
            seq: 0,
            last_now_us: 0,
            next_probe_us: 0,
            log_iteration: true,
            probe_interval_us: policy.wait_loop_probe_interval_us,
        }
    }

    pub(in crate::ltp) fn set_phase(&mut self, phase: &'static str) {
        self.phase = phase;
        self.next_probe_us = 0;
        self.log_iteration = true;
    }

    pub(in crate::ltp) fn begin_iteration(&mut self) {
        self.log_iteration = self.next_probe_us == 0 || self.last_now_us >= self.next_probe_us;
        if self.log_iteration {
            self.seq += 1;
        }
    }

    pub(in crate::ltp) fn finish_iteration(&mut self) {
        if !self.log_iteration {
            return;
        }

        let mut next = if self.next_probe_us == 0 {
            self.last_now_us.saturating_add(self.probe_interval_us)
        } else {
            self.next_probe_us
        };
        while self.last_now_us >= next {
            next = next.saturating_add(self.probe_interval_us);
        }
        self.next_probe_us = next;
        self.log_iteration = false;
    }

    pub(in crate::ltp) fn before(&self, op: &str) {
        if self.log_iteration {
            println!(
                "user-test: LTP wait-loop probe seq={} phase={} root={} group={} case={} case_pgrp={} before {}",
                self.seq,
                self.phase,
                self.root_label,
                self.group_name,
                self.case_name,
                self.tid,
                op,
            );
        }
    }

    pub(in crate::ltp) fn after(&self, op: &str, detail: core::fmt::Arguments<'_>) {
        if self.log_iteration {
            println!(
                "user-test: LTP wait-loop probe seq={} phase={} root={} group={} case={} case_pgrp={} after {} {}",
                self.seq,
                self.phase,
                self.root_label,
                self.group_name,
                self.case_name,
                self.tid,
                op,
                detail,
            );
        }
    }

    fn observe_now(&mut self, now_us: i64) {
        self.last_now_us = now_us;
    }
}

pub(in crate::ltp) fn now_us_with_probe(probe: &mut LtpWaitLoopProbe<'_>) -> Result<i64, Errno> {
    probe.before("gettimeofday");
    match crate::ltp::time::now_us() {
        Ok(now) => {
            probe.observe_now(now);
            probe.after("gettimeofday", format_args!("now_us={}", now));
            Ok(now)
        },
        Err(errno) => {
            probe.after("gettimeofday", format_args!("err={errno:?}"));
            Err(errno)
        },
    }
}

pub(in crate::ltp) fn elapsed_us_since_with_probe(
    start_us: i64,
    probe: &mut LtpWaitLoopProbe<'_>,
) -> Result<i64, Errno> {
    Ok(now_us_with_probe(probe)?.saturating_sub(start_us))
}

pub(in crate::ltp) fn sched_yield_with_probe(probe: &LtpWaitLoopProbe<'_>) -> Result<(), Errno> {
    probe.before("sched_yield");
    match sched_yield() {
        Ok(()) => {
            probe.after("sched_yield", format_args!("ok"));
            Ok(())
        },
        Err(errno) => {
            probe.after("sched_yield", format_args!("err={errno:?}"));
            Err(errno)
        },
    }
}
