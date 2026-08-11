#![no_std]
#![no_main]

mod allocation;
mod lifecycle;
mod mount;
mod relation;
mod stream;
mod support;

use anemone_rs::{
    abi::system::native::power::SHUTDOWN_MAGIC,
    os::{
        anemone::power::shutdown,
        linux::{
            fs::{AtFd, STDOUT_FILENO, mkdirat, mount},
            tty::{SetTermiosWhen, tcgetattr, tcsetattr},
        },
    },
    prelude::*,
};

use support::{ADDITIONAL_VIEW, CANONICAL_VIEW};

const OUTPUT_FD: u32 = STDOUT_FILENO as u32;

struct Results {
    passed: usize,
    failed: usize,
}

impl Results {
    const fn new() -> Self {
        Self {
            passed: 0,
            failed: 0,
        }
    }

    fn case(&mut self, name: &str, test: fn() -> Result<(), Errno>) {
        match test() {
            Ok(()) => {
                self.passed += 1;
                println!("PTYTEST:PASS:{name}");
            },
            Err(errno) => {
                self.failed += 1;
                println!("PTYTEST:FAIL:{name}:{errno}");
            },
        }
    }
}

fn ensure_dir(path: &str) -> Result<(), Errno> {
    match mkdirat(AtFd::Cwd, Path::new(path), 0o755) {
        Ok(()) | Err(EEXIST) => Ok(()),
        Err(errno) => Err(errno),
    }
}

fn prepare_mounts() -> Result<(), Errno> {
    ensure_dir("/dev")?;
    mount(Path::new("devfs"), Path::new("/dev"), "devfs")?;
    ensure_dir("/proc")?;
    mount(Path::new("proc"), Path::new("/proc"), "proc")?;
    ensure_dir("/tmp")?;
    ensure_dir(ADDITIONAL_VIEW)?;
    mount(Path::new("devpts"), Path::new(CANONICAL_VIEW), "devpts")?;
    mount(Path::new("devpts"), Path::new(ADDITIONAL_VIEW), "devpts")
}

fn run_cases(results: &mut Results) {
    results.case("identity-reuse", lifecycle::test_identity_reuse);
    results.case(
        "allocation-admission",
        allocation::test_allocation_admission,
    );
    results.case("mount-views", mount::test_mount_views);
    results.case("stream-termios", stream::test_stream_and_terminal_state);
    results.case("nonblock-readiness", stream::test_nonblocking_and_readiness);
    results.case(
        "description-lifecycle",
        lifecycle::test_description_lifecycle,
    );
    results.case("fork-final-release", lifecycle::test_fork_final_release);
    results.case("implicit-path", relation::test_path_implicit_acquire);
    results.case("implicit-peer", relation::test_peer_implicit_acquire);
    results.case("implicit-negative", relation::test_implicit_negative_matrix);
    results.case("hangup-relation", relation::test_master_hangup_relation);
    results.case("capacity-reuse", allocation::test_capacity_reuse);
}

fn finish(mut results: Results) -> Result<(), Errno> {
    let output_termios = match tcgetattr(OUTPUT_FD) {
        Ok(termios) => Some(termios),
        Err(errno) => {
            println!("PTYTEST:FAIL:final-termios:{errno}");
            results.failed += 1;
            None
        },
    };
    if let Some(termios) = output_termios.as_ref()
        && let Err(errno) = tcsetattr(OUTPUT_FD, SetTermiosWhen::Drain, termios)
    {
        println!("PTYTEST:FAIL:final-drain:{errno}");
        results.failed += 1;
    }

    if results.failed == 0 {
        println!("PTYTEST:SUMMARY:PASS:{}", results.passed);
    } else {
        println!(
            "PTYTEST:SUMMARY:FAIL:passed={}:failed={}",
            results.passed, results.failed
        );
    }
    // Writes complete when queued. Drain the summary before the init task
    // powers off so the acceptance oracle cannot be lost on a fast platform.
    if let Some(termios) = output_termios.as_ref() {
        let _ = tcsetattr(OUTPUT_FD, SetTermiosWhen::Drain, termios);
    }
    shutdown(SHUTDOWN_MAGIC)?;
    unreachable!("pty-test: shutdown returned unexpectedly")
}

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    println!("PTYTEST:START");
    let mut results = Results::new();
    match prepare_mounts() {
        Ok(()) => run_cases(&mut results),
        Err(errno) => {
            println!("PTYTEST:FAIL:setup:{errno}");
            results.failed += 1;
        },
    }
    finish(results)
}
