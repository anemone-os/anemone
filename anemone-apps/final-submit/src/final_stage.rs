use anemone_rs::{os::linux::process::WStatus, prelude::*};

const TEST_DIR: &str = "/glibc";
const TEST_ENV: &[&str] = &[
    "HOME=/root",
    "PATH=/root/.cargo/bin:/usr/local/bin:/usr/bin:/bin:/sbin:/usr/sbin",
    "TERM=linux",
];
const TEST_SCRIPTS: &[&str] = &["./cagent_testcode.sh", "./buildstorm_testcode.sh"];

pub(crate) fn run() {
    for script in TEST_SCRIPTS {
        println!("final-submit: starting final script {script}");
        match crate::process::run_status_in_dir(
            Some(TEST_DIR),
            script,
            &[*script],
            TEST_ENV,
            script,
        ) {
            Ok(WStatus::Exited(0)) => println!("final-submit: final script {script} passed"),
            Ok(status) => {
                eprintln!("final-submit: final script {script} finished with {status:?}")
            },
            Err(errno) => eprintln!("final-submit: final script {script} failed: {errno}"),
        }
    }
}
