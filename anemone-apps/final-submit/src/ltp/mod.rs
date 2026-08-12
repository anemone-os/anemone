//! Submission-local facade over the mature `user-test` LTP runner.
//!
//! Only profile selection differs: the final submission runs every registered
//! group except low-value fanotify. The runner and its containment behavior are
//! reused directly so this one-shot app does not fork a second implementation.

#[path = "../../../user-test/src/ltp/case.rs"]
mod case;
#[path = "../../../user-test/src/ltp/component/mod.rs"]
mod component;
#[path = "../../../user-test/src/ltp/config.rs"]
mod config;
#[path = "../../../user-test/src/ltp/fixture.rs"]
mod fixture;
mod profile;
#[path = "../../../user-test/src/ltp/result.rs"]
mod result;
#[path = "../../../user-test/src/ltp/runner.rs"]
mod runner;
#[path = "../../../user-test/src/ltp/time.rs"]
mod time;

pub(crate) fn install_ltp_fixtures() {
    fixture::install_ltp_fixtures();
}

pub(crate) fn run_ltp_tests() {
    runner::run_ltp_tests();
}
