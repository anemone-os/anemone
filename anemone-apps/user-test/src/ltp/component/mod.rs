//! Typed component aggregation for the LTP runner.
//!
//! This is intentionally a hard-coded set of components, not a plugin/event-bus
//! framework. The hooks define the few runner phases that need coordinated
//! output plus heartbeat updates while keeping heartbeat ownership in one
//! place.

pub(super) mod output_filter;
pub(super) mod wait_probe;

mod heartbeat;
mod output;

use crate::ltp::{
    config::{LtpGroup, LtpRoot, LtpRunPolicy},
    result::{LtpCaseOutcome, LtpSummary},
};

use self::{heartbeat::LtpHeartbeat, output::LtpOutput};

pub(super) struct LtpComponents {
    // `output` owns runner/judge-visible text. `heartbeat` owns the diagnostic
    // child and control pipe. Keeping both behind this type prevents runner/case
    // code from splitting ownership of console and heartbeat side effects.
    output: LtpOutput,
    heartbeat: LtpHeartbeat,
}

impl LtpComponents {
    pub(super) fn start(policy: &LtpRunPolicy) -> Self {
        Self {
            output: LtpOutput,
            heartbeat: LtpHeartbeat::start_or_disabled(policy),
        }
    }

    pub(super) fn on_profile_start(&mut self, groups: &[&'static LtpGroup]) {
        self.output.profile_start(groups);
        self.heartbeat.publish("profile_start", "-", "-", "-", 0);
    }

    pub(super) fn on_profile_finished(&mut self, summary: LtpSummary, policy: &LtpRunPolicy) {
        self.heartbeat.publish("profile_finished", "-", "-", "-", 0);
        self.output.profile_finished(summary);
        self.heartbeat.finish(policy);
    }

    pub(super) fn on_root_start(&mut self, root: &LtpRoot) {
        self.heartbeat
            .publish("root_start", root.label, "-", "-", 0);
    }

    pub(super) fn on_root_missing(&mut self, root: &LtpRoot) {
        self.output.root_missing(root);
        self.heartbeat
            .publish("root_skipped", root.label, "-", "-", 0);
    }

    pub(super) fn on_root_finished(&mut self, root: &LtpRoot, summary: LtpSummary) {
        self.heartbeat
            .publish("root_finished", root.label, "-", "-", 0);
        self.output.root_summary(root, summary);
    }

    pub(super) fn on_group_start(&mut self, root: &LtpRoot, group: &LtpGroup) {
        self.output.group_start(root, group);
        self.heartbeat
            .publish("group_start", root.label, group.name, "-", 0);
    }

    pub(super) fn on_group_finished(
        &mut self,
        root: &LtpRoot,
        group: &LtpGroup,
        summary: LtpSummary,
    ) {
        self.output.group_end(root, group);
        self.heartbeat
            .publish("group_finished", root.label, group.name, "-", 0);
        self.output.group_summary(root, group, summary);
    }

    pub(super) fn on_case_start(&mut self, root: &LtpRoot, group: &LtpGroup, case_name: &str) {
        self.output.case_start(case_name);
        self.heartbeat
            .publish("case_start", root.label, group.name, case_name, 0);
    }

    pub(super) fn on_case_missing(&self, root: &LtpRoot, case_name: &str, executable: &str) {
        self.output.case_missing(root, case_name, executable);
    }

    pub(super) fn on_case_waiting(
        &mut self,
        root: &LtpRoot,
        group: &LtpGroup,
        case_name: &str,
        tid: u32,
    ) {
        self.heartbeat
            .publish("case_waiting", root.label, group.name, case_name, tid);
    }

    pub(super) fn on_case_timeout_kill(
        &mut self,
        root_label: &str,
        group_name: &str,
        case_name: &str,
        tid: u32,
    ) {
        self.heartbeat
            .publish("case_timeout_kill", root_label, group_name, case_name, tid);
    }

    pub(super) fn on_case_timeout_unreaped(
        &mut self,
        root_label: &str,
        group_name: &str,
        case_name: &str,
        tid: u32,
    ) {
        self.heartbeat.publish(
            "case_timeout_unreaped",
            root_label,
            group_name,
            case_name,
            tid,
        );
    }

    pub(super) fn on_case_finished(
        &mut self,
        root: &LtpRoot,
        group: &LtpGroup,
        case_name: &str,
        tid: u32,
        outcome: LtpCaseOutcome,
        exit_code: i32,
    ) {
        self.output.case_result(case_name, outcome, exit_code);
        let phase = match outcome {
            LtpCaseOutcome::Passed => "case_passed",
            LtpCaseOutcome::Failed => "case_failed",
            LtpCaseOutcome::InfraFailed => "case_wait_failed",
            LtpCaseOutcome::Skipped => "case_skipped",
        };
        self.heartbeat
            .publish(phase, root.label, group.name, case_name, tid);
    }

    pub(super) fn on_case_timeout(
        &mut self,
        root: &LtpRoot,
        group: &LtpGroup,
        case_name: &str,
        tid: u32,
    ) {
        self.output.case_timeout_result(case_name);
        self.heartbeat
            .publish("case_timeout", root.label, group.name, case_name, tid);
    }

    pub(super) fn on_case_wait_failed(
        &mut self,
        root: &LtpRoot,
        group: &LtpGroup,
        case_name: &str,
        tid: u32,
        errno: anemone_rs::prelude::Errno,
    ) {
        self.output.case_wait_failed(case_name, errno);
        self.output.case_infra_result(case_name);
        self.heartbeat
            .publish("case_wait_failed", root.label, group.name, case_name, tid);
    }

    pub(super) fn on_case_fork_failed(
        &mut self,
        root: &LtpRoot,
        group: &LtpGroup,
        case_name: &str,
        errno: anemone_rs::prelude::Errno,
    ) {
        self.output.case_fork_failed(case_name, errno);
        self.output.case_infra_result(case_name);
        self.heartbeat
            .publish("case_fork_failed", root.label, group.name, case_name, 0);
    }

    pub(super) fn case_timeout(&self, case_name: &str, timeout_seconds: i64) {
        self.output.case_timeout(case_name, timeout_seconds);
    }

    pub(super) fn case_timeout_unreaped(&self, case_name: &str, kill_grace_seconds: i64) {
        self.output
            .case_timeout_unreaped(case_name, kill_grace_seconds);
    }

    pub(super) fn case_pgrp_absent(&self, case_name: &str, tid: u32) {
        self.output.case_pgrp_absent(case_name, tid);
    }

    pub(super) fn case_pid_mismatch(&self, case_name: &str) {
        self.output.case_pid_mismatch(case_name);
    }

    pub(super) fn child_attach_filter_failed(
        &self,
        case_name: &str,
        errno: anemone_rs::prelude::Errno,
    ) {
        self.output.child_attach_filter_failed(case_name, errno);
    }

    pub(super) fn child_setpgid_failed(&self, case_name: &str, errno: anemone_rs::prelude::Errno) {
        self.output.child_setpgid_failed(case_name, errno);
    }

    pub(super) fn child_chdir_failed(
        &self,
        case_name: &str,
        workdir: &str,
        errno: anemone_rs::prelude::Errno,
    ) {
        self.output.child_chdir_failed(case_name, workdir, errno);
    }

    pub(super) fn child_exec_failed(
        &self,
        case_name: &str,
        case_path: &str,
        errno: anemone_rs::prelude::Errno,
    ) {
        self.output.child_exec_failed(case_name, case_path, errno);
    }
}
