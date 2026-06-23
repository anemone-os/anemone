//! LTP root/group coordinator.
//!
//! `LtpRunner` owns only runtime coordination: typed components plus immutable
//! policy. Static roots/groups live in `config`, and the current
//! root/group/case is passed through hook calls instead of cached here, so
//! there is no second truth source for heartbeat or output diagnostics.

use anemone_rs::{os::linux::fs::AtFd, prelude::*};

use super::{
    case::{case_executable_exists, run_ltp_case},
    component::LtpComponents,
    config::{LTP_ROOTS, LtpGroup, LtpRoot, LtpRunPolicy},
    profile::{parse_case_line, select_ltp_groups},
    result::{LtpCaseOutcome, LtpSummary},
};

pub(super) fn run_ltp_tests() {
    let groups = select_ltp_groups();
    let mut runner = LtpRunner::new();
    runner.run(groups.as_slice());
}

struct LtpRunner {
    // `components` owns heartbeat lifecycle. Runner methods may publish phase
    // transitions through hooks, but must not retain heartbeat child/pipe state.
    components: LtpComponents,
    policy: LtpRunPolicy,
}

impl LtpRunner {
    fn new() -> Self {
        let policy = LtpRunPolicy::DEFAULT;
        Self {
            components: LtpComponents::start(&policy),
            policy,
        }
    }

    fn run(&mut self, groups: &[&'static LtpGroup]) {
        self.components.on_profile_start(groups);

        let mut overall = LtpSummary::default();
        for root in LTP_ROOTS {
            let summary = self.run_root(root, groups);
            overall.merge(summary);
        }

        self.components.on_profile_finished(overall, &self.policy);
    }

    fn run_root(&mut self, root: &LtpRoot, groups: &[&'static LtpGroup]) -> LtpSummary {
        crate::runtime::switch_runtime(root.family);
        crate::runtime::clear_tmp();
        self.components.on_root_start(root);

        let mut summary = LtpSummary::default();
        if anemone_rs::os::linux::fs::fstatat(AtFd::Cwd, Path::new(root.workdir)).is_err() {
            self.components.on_root_missing(root);
            return summary;
        }

        for group in groups {
            let group_summary = self.run_group(root, group);
            summary.merge(group_summary);
        }

        self.components.on_root_finished(root, summary);
        summary
    }

    fn run_group(&mut self, root: &LtpRoot, group: &LtpGroup) -> LtpSummary {
        self.components.on_group_start(root, group);

        let mut summary = LtpSummary::default();
        for line in group.cases.lines() {
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

            let case_path = format!("/{}/ltp/testcases/bin/{}", root.family, case.executable);
            if !case_executable_exists(case_path.as_str()) {
                self.components
                    .on_case_missing(root, case.name, case.executable);
                summary.skipped += 1;
                continue;
            }

            summary.attempted += 1;
            match run_ltp_case(
                root,
                group,
                &case,
                case_path.as_str(),
                &mut self.components,
                &self.policy,
            ) {
                LtpCaseOutcome::Passed => summary.passed += 1,
                LtpCaseOutcome::Failed => summary.failed += 1,
                LtpCaseOutcome::InfraFailed => summary.infra_failed += 1,
                LtpCaseOutcome::Skipped => summary.skipped += 1,
            }
        }

        self.components.on_group_finished(root, group, summary);
        summary
    }
}
