//! LTP runner facade.
//!
//! Keep the public surface narrow for `main.rs`: competition/rootfs
//! orchestration can install LTP fixtures and run the selected profile, but
//! heartbeat, output filtering, case parsing, timeout policy, and per-case
//! lifecycle stay internal to this module.

mod case;
mod component;
mod config;
mod fixture;
mod profile;
mod result;
mod runner;
mod time;

pub fn install_ltp_fixtures() {
    fixture::install_ltp_fixtures();
}

pub fn run_ltp_tests() {
    runner::run_ltp_tests();
}
