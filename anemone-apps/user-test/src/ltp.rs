use anemone_rs::{
    os::linux::{
        fs::{chdir, fstatat, AtFd},
        process::{execve, exit, fork, WStatus},
    },
    prelude::*,
};

const STARTER_WHITELIST: &str = include_str!("../ltp/whitelist-starter.txt");

const GLIBC_LTP_ENV: &[&str] = &[
    "PATH=/glibc/ltp/testcases/bin:/glibc/bin:/glibc/usr/bin:/bin:/usr/bin:/sbin:/usr/sbin",
    "LTPROOT=/glibc/ltp",
    "LTP_VIRT_OVERRIDE=kvm",
    "LTP_COLORIZE_OUTPUT=1",
    "TMPDIR=/tmp",
];

const MUSL_LTP_ENV: &[&str] = &[
    "PATH=/musl/ltp/testcases/bin:/musl/bin:/musl/usr/bin:/bin:/usr/bin:/sbin:/usr/sbin",
    "LTPROOT=/musl/ltp",
    "LTP_VIRT_OVERRIDE=kvm",
    "LTP_COLORIZE_OUTPUT=1",
    "TMPDIR=/tmp",
];

struct LtpRoot {
    family: &'static str,
    label: &'static str,
    workdir: &'static str,
    envp: &'static [&'static str],
    whitelist: &'static str,
    disabled_cases: &'static [&'static str],
}

const LTP_ROOTS: &[LtpRoot] = &[
    LtpRoot {
        family: "glibc",
        label: "ltp-glibc",
        workdir: "/glibc",
        envp: GLIBC_LTP_ENV,
        whitelist: STARTER_WHITELIST,
        disabled_cases: &[],
    },
    LtpRoot {
        family: "musl",
        label: "ltp-musl",
        workdir: "/musl",
        envp: MUSL_LTP_ENV,
        whitelist: STARTER_WHITELIST,
        disabled_cases: &["sbrk01"],
    },
];

struct LtpCaseSpec<'a> {
    name: &'a str,
    args: Vec<&'a str>,
}

#[derive(Clone, Copy, Default)]
struct LtpSummary {
    attempted: usize,
    passed: usize,
    failed: usize,
    infra_failed: usize,
    skipped: usize,
}

impl LtpSummary {
    fn merge(&mut self, other: Self) {
        self.attempted += other.attempted;
        self.passed += other.passed;
        self.failed += other.failed;
        self.infra_failed += other.infra_failed;
        self.skipped += other.skipped;
    }
}

enum LtpCaseOutcome {
    Passed,
    Failed,
    InfraFailed,
}

pub fn run_ltp_tests() {
    println!("user-test: running LTP whitelist...");

    let mut overall = LtpSummary::default();
    for root in LTP_ROOTS {
        let summary = run_ltp_root(root);
        overall.merge(summary);
    }

    println!(
		"user-test: LTP whitelist finished: attempted={} passed={} failed={} infra_failed={} skipped={}",
		overall.attempted,
		overall.passed,
		overall.failed,
		overall.infra_failed,
		overall.skipped,
	);
}

fn run_ltp_root(root: &LtpRoot) -> LtpSummary {
    crate::switch_runtime(root.family);
    crate::clear_tmp();

    println!("#### OS COMP TEST GROUP START {} ####", root.label);

    let mut summary = LtpSummary::default();
    if fstatat(AtFd::Cwd, Path::new(root.workdir)).is_err() {
        println!(
            "user-test: skipping {} because {} is missing",
            root.label, root.workdir,
        );
        println!("#### OS COMP TEST GROUP END {} ####", root.label);
        return summary;
    }

    for line in root.whitelist.lines() {
        let Some(case) = parse_case_line(line) else {
            continue;
        };

        if root
            .disabled_cases
            .iter()
            .any(|disabled| *disabled == case.name)
        {
            summary.skipped += 1;
            continue;
        }

        let case_path = format!("/{}/ltp/testcases/bin/{}", root.family, case.name);
        if fstatat(AtFd::Cwd, Path::new(case_path.as_str())).is_err() {
            println!(
                "user-test: skipping {} missing case {}",
                root.label, case.name,
            );
            summary.skipped += 1;
            continue;
        }

        summary.attempted += 1;
        match run_ltp_case(root, &case, case_path.as_str()) {
            LtpCaseOutcome::Passed => summary.passed += 1,
            LtpCaseOutcome::Failed => summary.failed += 1,
            LtpCaseOutcome::InfraFailed => summary.infra_failed += 1,
        }
    }

    println!("#### OS COMP TEST GROUP END {} ####", root.label);
    println!(
        "user-test: {} summary attempted={} passed={} failed={} infra_failed={} skipped={}",
        root.label,
        summary.attempted,
        summary.passed,
        summary.failed,
        summary.infra_failed,
        summary.skipped,
    );
    summary
}

fn run_ltp_case(root: &LtpRoot, case: &LtpCaseSpec<'_>, case_path: &str) -> LtpCaseOutcome {
    println!("RUN LTP CASE {}", case.name);

    match fork() {
        Ok(Some(tid)) => match crate::wait_child_status(tid, case.name) {
            Ok(wstatus) => {
                let exit_code = ltp_exit_code(wstatus);
                println!("FAIL LTP CASE {} : {}", case.name, exit_code);
                if exit_code == 0 {
                    LtpCaseOutcome::Passed
                } else {
                    LtpCaseOutcome::Failed
                }
            },
            Err(errno) => {
                println!("user-test: {} wait failed: {errno:?}", case.name);
                println!("FAIL LTP CASE {} : 127", case.name);
                LtpCaseOutcome::InfraFailed
            },
        },
        Ok(None) => {
            if let Err(errno) = chdir(root.workdir) {
                println!(
                    "user-test: {} chdir({}) failed: {errno:?}",
                    case.name, root.workdir,
                );
                exit(127);
            }

            let mut argv = Vec::with_capacity(case.args.len() + 1);
            argv.push(case_path);
            argv.extend(case.args.iter().copied());

            if let Err(errno) = execve(case_path, argv.as_slice(), root.envp) {
                println!(
                    "user-test: {} execve({}) failed: {errno:?}",
                    case.name, case_path,
                );
            }
            exit(127);
        },
        Err(errno) => {
            println!("user-test: {} fork failed: {errno:?}", case.name);
            println!("FAIL LTP CASE {} : 127", case.name);
            LtpCaseOutcome::InfraFailed
        },
    }
}

fn parse_case_line(line: &str) -> Option<LtpCaseSpec<'_>> {
    let line = line.split('#').next().unwrap_or("").trim();
    if line.is_empty() {
        return None;
    }

    let mut parts = line.split_ascii_whitespace();
    let name = parts.next()?;
    let args = parts.collect();
    Some(LtpCaseSpec { name, args })
}

fn ltp_exit_code(wstatus: WStatus) -> i32 {
    match wstatus {
        WStatus::Exited(code) => i32::from(code),
        WStatus::Signal(sig) | WStatus::Stopped(sig) => 128 + i32::from(sig),
        WStatus::Continued => 128,
    }
}
