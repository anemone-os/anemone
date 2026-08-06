use std::{ffi::OsStr, io::ErrorKind, process::Command};

fn fail(message: &str) -> ! {
    eprintln!("RUSTCOMMAND:FAIL:{message}");
    std::process::exit(1)
}

fn main() {
    if std::env::args_os().nth(1).as_deref() == Some(OsStr::new("--child")) {
        return;
    }

    println!("RUSTCOMMAND:START");

    let success = Command::new("/bin/rust-command-test")
        .arg("--child")
        .status()
        .unwrap_or_else(|error| {
            fail(&format!("exec-success:{error:?}"));
        });
    if !success.success() {
        fail(&format!("exec-success-status:{success:?}"));
    }
    println!("RUSTCOMMAND:PASS:exec-success-cloexec-eof");

    let error = match Command::new("/definitely-not-an-anemone-program").status() {
        Ok(status) => fail(&format!("exec-failure-status:{status:?}")),
        Err(error) => error,
    };
    if error.kind() != ErrorKind::NotFound || error.raw_os_error() != Some(2) {
        fail(&format!("exec-failure-error:{error:?}"));
    }
    println!("RUSTCOMMAND:PASS:exec-failure-errno-record");
    println!("RUSTCOMMAND:SUMMARY:PASS:2");
}
