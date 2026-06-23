//! Console output boundary for LTP.
//!
//! Runner control lines, timeout diagnostics, and judge-visible case result
//! lines are kept here so cleanup work cannot accidentally rewrite scoring
//! strings while changing case lifecycle code. This is not a dynamic logger.

use anemone_rs::prelude::*;

use crate::ltp::{
    config::{LtpGroup, LtpRoot},
    result::{LTP_CASE_TIMEOUT_EXIT_CODE, LtpCaseOutcome, LtpSummary},
};

pub(super) struct LtpOutput;

impl LtpOutput {
    pub(super) fn profile_start(&self, groups: &[&'static LtpGroup]) {
        print!("user-test: running LTP profile groups:");
        for group in groups {
            print!(" {}", group.name);
        }
        println!();
    }

    pub(super) fn profile_finished(&self, summary: LtpSummary) {
        println!(
            "user-test: LTP whitelist finished: attempted={} passed={} failed={} infra_failed={} skipped={}",
            summary.attempted,
            summary.passed,
            summary.failed,
            summary.infra_failed,
            summary.skipped,
        );
    }

    pub(super) fn root_missing(&self, root: &LtpRoot) {
        println!(
            "user-test: skipping {} because {} is missing",
            root.label, root.workdir,
        );
    }

    pub(super) fn root_start(&self, root: &LtpRoot) {
        println!("#### OS COMP TEST GROUP START {} ####", root.label);
    }

    pub(super) fn root_end(&self, root: &LtpRoot) {
        println!("#### OS COMP TEST GROUP END {} ####", root.label);
    }

    pub(super) fn root_summary(&self, root: &LtpRoot, summary: LtpSummary) {
        println!(
            "user-test: {} summary attempted={} passed={} failed={} infra_failed={} skipped={}",
            root.label,
            summary.attempted,
            summary.passed,
            summary.failed,
            summary.infra_failed,
            summary.skipped,
        );
    }

    pub(super) fn group_start(&self, root: &LtpRoot, group: &LtpGroup) {
        println!("user-test: LTP group start {}/{}", root.label, group.name);
    }

    pub(super) fn group_end(&self, root: &LtpRoot, group: &LtpGroup, summary: LtpSummary) {
        println!(
            "user-test: LTP group end {}/{} attempted={} passed={} failed={} infra_failed={} skipped={}",
            root.label,
            group.name,
            summary.attempted,
            summary.passed,
            summary.failed,
            summary.infra_failed,
            summary.skipped,
        );
    }

    pub(super) fn case_missing(&self, root: &LtpRoot, case_name: &str, executable: &str) {
        println!(
            "user-test: skipping {} missing case {} executable {}",
            root.label, case_name, executable,
        );
    }

    pub(super) fn case_start(&self, case_name: &str) {
        println!("\nRUN LTP CASE {case_name}");
    }

    pub(super) fn case_timeout(&self, case_name: &str, timeout_seconds: i64) {
        println!(
            "user-test: TIMEOUT LTP CASE {case_name}: exceeded {timeout_seconds}s; killing case process group",
        );
    }

    pub(super) fn case_timeout_unreaped(&self, case_name: &str, kill_grace_seconds: i64) {
        println!(
            "user-test: TIMEOUT LTP CASE {case_name}: child not reaped after {kill_grace_seconds}s kill grace; continuing",
        );
    }

    pub(super) fn case_pgrp_absent(&self, case_name: &str, tid: u32) {
        println!(
            "user-test: TIMEOUT LTP CASE {case_name}: process group -{tid} is absent; killing child pid {tid}",
        );
    }

    pub(super) fn case_wait_failed(&self, case_name: &str, errno: Errno) {
        println!("user-test: {case_name} wait failed: {errno:?}");
    }

    pub(super) fn case_pid_mismatch(&self, case_name: &str) {
        println!("user-test: {case_name} waited pid mismatch");
    }

    pub(super) fn case_fork_failed(&self, case_name: &str, errno: Errno) {
        println!("user-test: {case_name} fork failed: {errno:?}");
    }

    pub(super) fn child_attach_filter_failed(&self, case_name: &str, errno: Errno) {
        println!(
            "user-test: INFRA {case_name} failed to attach LTP output filter: {errno:?}; not running case",
        );
    }

    pub(super) fn child_setpgid_failed(&self, case_name: &str, errno: Errno) {
        println!(
            "user-test: INFRA {case_name} setpgid(0, 0) failed before execve: {errno:?}; not running case",
        );
    }

    pub(super) fn child_chdir_failed(&self, case_name: &str, workdir: &str, errno: Errno) {
        println!("user-test: {case_name} chdir({workdir}) failed: {errno:?}");
    }

    pub(super) fn child_exec_failed(&self, case_name: &str, case_path: &str, errno: Errno) {
        println!("user-test: {case_name} execve({case_path}) failed: {errno:?}");
    }

    pub(super) fn case_timeout_result(&self, case_name: &str) {
        self.case_result(
            case_name,
            LtpCaseOutcome::InfraFailed,
            LTP_CASE_TIMEOUT_EXIT_CODE,
        );
    }

    pub(super) fn case_infra_result(&self, case_name: &str) {
        self.case_result(case_name, LtpCaseOutcome::InfraFailed, 127);
    }

    pub(super) fn case_result(&self, case_name: &str, _outcome: LtpCaseOutcome, exit_code: i32) {
        // Judge-visible compatibility: the competition scripts key off this
        // "FAIL" line even when exit_code 0 maps to LtpCaseOutcome::Passed.
        println!("FAIL LTP CASE {case_name} : {exit_code}");
    }
}
