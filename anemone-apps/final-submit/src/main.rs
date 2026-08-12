#![no_std]
#![no_main]

//! One-shot entry point for the final submission artifact.
//!
//! The organizer reuses one `make all` result with the frozen preliminary and
//! final test disks. Keep their compatibility paths isolated here; this app is
//! not a general boot selector and should be removed after the submission.

mod embedded_tools;
mod environment;
mod final_stage;
mod ltp;
mod preliminary;
mod process;

#[path = "../../user-test/src/busybox.rs"]
mod busybox;
#[path = "../../user-test/src/file.rs"]
mod file;
#[path = "../../user-test/src/runtime.rs"]
mod runtime;

use anemone_rs::{
    abi::system::native::power::SHUTDOWN_MAGIC, os::anemone::power::shutdown, prelude::*,
};

enum ContestStage {
    Preliminary,
    Final,
}

fn detect_stage() -> Result<ContestStage, Errno> {
    let final_disk = environment::path_exists("/usr")
        && environment::path_exists("/glibc/cagent_testcode.sh")
        && environment::path_exists("/glibc/buildstorm_testcode.sh");
    let preliminary_disk = environment::path_exists("/glibc/ltp_testcode.sh")
        && environment::path_exists("/musl/ltp_testcode.sh");

    match (preliminary_disk, final_disk) {
        (true, false) => Ok(ContestStage::Preliminary),
        (false, true) => Ok(ContestStage::Final),
        _ => {
            eprintln!(
                "final-submit: unrecognized or ambiguous test disk: preliminary={preliminary_disk} final={final_disk}",
            );
            Err(EINVAL)
        },
    }
}

fn run_submission() -> Result<(), Errno> {
    environment::prepare_common_filesystems()?;
    match detect_stage()? {
        ContestStage::Preliminary => {
            println!("final-submit: detected frozen preliminary test disk");
            preliminary::run()
        },
        ContestStage::Final => {
            println!("final-submit: detected frozen final test disk");
            final_stage::run();
            Ok(())
        },
    }
}

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    if let Err(errno) = run_submission() {
        eprintln!("final-submit: submission runner failed: {errno}");
    }

    shutdown(SHUTDOWN_MAGIC)?;
    unreachable!("final-submit: shutdown returned unexpectedly");
}
