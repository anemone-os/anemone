//! LTP result accounting and judge tag constants.
//!
//! Summary counters and exit-code classification live here, but
//! judge-visible result line text does not. That text is centralized in
//! `component::output::LtpOutput` so compatibility formatting has one owner.

use anemone_rs::os::linux::process::WStatus;

pub(super) const LTP_RESULT_RESET: &[u8] = b"\x1b[0m";
pub(super) const LTP_JUDGE_TPASS: &[u8] = b"\x1b[1;32mTPASS: \x1b[0m";
pub(super) const LTP_JUDGE_TFAIL: &[u8] = b"\x1b[1;31mTFAIL: \x1b[0m";
pub(super) const LTP_JUDGE_TBROK: &[u8] = b"\x1b[1;31mTBROK: \x1b[0m";
pub(super) const LTP_JUDGE_TCONF: &[u8] = b"\x1b[1;33mTCONF: \x1b[0m";
pub(super) const LTP_JUDGE_TWARN: &[u8] = b"\x1b[1;35mTWARN: \x1b[0m";

// LTP result bits use TCONF=32 for "unsupported configuration". Only a pure
// TCONF exit is a skip; mixed TCONF|TFAIL/TBROK still represents a real
// failure.
pub(super) const LTP_TCONF_EXIT_CODE: i32 = 32;

pub(super) const LTP_CASE_TIMEOUT_EXIT_CODE: i32 = 124;

#[derive(Clone, Copy, Default)]
pub(super) struct LtpSummary {
    pub(super) attempted: usize,
    pub(super) passed: usize,
    pub(super) failed: usize,
    pub(super) infra_failed: usize,
    pub(super) skipped: usize,
}

impl LtpSummary {
    pub(super) fn merge(&mut self, other: Self) {
        self.attempted += other.attempted;
        self.passed += other.passed;
        self.failed += other.failed;
        self.infra_failed += other.infra_failed;
        self.skipped += other.skipped;
    }
}

#[derive(Clone, Copy)]
pub(super) enum LtpCaseOutcome {
    Passed,
    Failed,
    InfraFailed,
    Skipped,
}

pub(super) fn ltp_exit_code(wstatus: WStatus) -> i32 {
    match wstatus {
        WStatus::Exited(code) => i32::from(code),
        WStatus::Signal(sig) | WStatus::Stopped(sig) => 128 + i32::from(sig),
        WStatus::Continued => 128,
    }
}
