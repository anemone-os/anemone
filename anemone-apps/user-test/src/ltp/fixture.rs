//! LTP text fixtures installed into the test root.
//!
//! Keep this limited to files that LTP cases need to observe in the chroot.
//! Staged tools and guest-root preparation stay in `guest`, shell/runtime links
//! stay in `runtime`, and BusyBox execution stays in `busybox` so LTP cleanup
//! does not absorb non-LTP setup.

use anemone_rs::prelude::*;

const ETC_PASSWD: &str = include_str!("../../fixtures/passwd");
const ETC_GROUP: &str = include_str!("../../fixtures/group");
const LTP_KCONFIG: &str = include_str!("../../fixtures/ltp-kconfig");
const LTP_MODULES_BUILTIN: &str = include_str!("../../fixtures/modules.builtin");
const LTP_MODULES_DEP: &str = include_str!("../../fixtures/modules.dep");

struct LtpFixture {
    path: &'static str,
    content: &'static str,
}

const LTP_FIXTURES: &[LtpFixture] = &[
    LtpFixture {
        path: "/etc/passwd",
        content: ETC_PASSWD,
    },
    LtpFixture {
        path: "/etc/group",
        content: ETC_GROUP,
    },
    LtpFixture {
        path: "/etc/ltp/anemone-kconfig",
        content: LTP_KCONFIG,
    },
    // rv64 switches /lib between runtime lib directories before running LTP.
    // Keep the module metadata visible through that active /lib symlink.
    LtpFixture {
        path: "/glibc/lib/modules/6.6.32/modules.dep",
        content: LTP_MODULES_DEP,
    },
    LtpFixture {
        path: "/glibc/lib/modules/6.6.32/modules.builtin",
        content: LTP_MODULES_BUILTIN,
    },
    LtpFixture {
        path: "/musl/lib/modules/6.6.32/modules.dep",
        content: LTP_MODULES_DEP,
    },
    LtpFixture {
        path: "/musl/lib/modules/6.6.32/modules.builtin",
        content: LTP_MODULES_BUILTIN,
    },
    LtpFixture {
        path: "/lib/modules/6.6.32/modules.dep",
        content: LTP_MODULES_DEP,
    },
    LtpFixture {
        path: "/lib/modules/6.6.32/modules.builtin",
        content: LTP_MODULES_BUILTIN,
    },
];

pub(super) fn install_ltp_fixtures() {
    println!("user-test: installing LTP fixtures...");

    for fixture in LTP_FIXTURES {
        install_ltp_fixture(fixture);
        println!("user-test: ensured LTP fixture {}", fixture.path);
    }
}

fn install_ltp_fixture(fixture: &LtpFixture) {
    let parent = fixture.path.rsplit_once('/').map(|(parent, _)| parent);
    let parent = parent.filter(|parent| !parent.is_empty()).unwrap_or("/");
    let script = format!(
        "mkdir -p {parent} && cat > {path} <<'EOF'\n{content}\nEOF",
        path = fixture.path,
        parent = parent,
        content = fixture.content,
    );

    crate::busybox::run_busybox(
        &["busybox", "sh", "-c", script.as_str()],
        "install LTP fixture",
    );
}
