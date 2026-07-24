use anemone_rs::{
    abi::fs::linux::open::{O_CREAT, O_RDONLY, O_TRUNC, O_WRONLY},
    os::linux::fs::{AtFd, chdir, chroot, close, fstatat, mkdirat, mount, openat, read},
    prelude::*,
};

const COMPETITION_DISK: &str = "/dev/vdb";

cfg_select! {
    target_arch = "riscv64" => {
        const STAGED_COMPETITION_FIXTURES: &[StagedCompetitionFixture] = &[
            StagedCompetitionFixture {
                source: "/fixtures/user-test/tools/mke2fs",
                dest: "/bin/mkfs.ext4",
            },
            StagedCompetitionFixture {
                source: "/fixtures/user-test/tools/mke2fs",
                dest: "/bin/mkfs.ext3",
            },
        ];
    },
    target_arch = "loongarch64" => {
        const STAGED_COMPETITION_FIXTURES: &[StagedCompetitionFixture] = &[
            StagedCompetitionFixture {
                source: "/fixtures/user-test/tools/mke2fs",
                dest: "/bin/mkfs.ext4",
            },
            StagedCompetitionFixture {
                source: "/fixtures/user-test/tools/mke2fs",
                dest: "/bin/mkfs.ext3",
            },
        ];
    }
}

struct StagedCompetitionFixture {
    source: &'static str,
    dest: &'static str,
}

pub(crate) fn enter_competition_root() {
    mount(Path::new("devfs"), Path::new("/dev"), "devfs")
        .expect("user-test: failed to mount devfs on /dev");
    mount(Path::new(COMPETITION_DISK), Path::new("/mnt"), "ext4")
        .expect("user-test: failed to mount /dev/vdb on /mnt with ext4");
    // Staged tools live on the boot rootfs and disappear after chroot, so copy
    // them into the mounted competition image before entering it.
    install_staged_competition_fixtures("/mnt");

    println!("user-test: entering environment...");
    chroot("/mnt").expect("user-test: failed to chroot to /mnt");
    chdir("/").expect("user-test: failed to change directory to / after chroot");
}

pub(crate) fn init_competition_environment() {
    ensure_dir("/dev");
    mount(Path::new("devfs"), Path::new("/dev"), "devfs")
        .expect("user-test: failed to mount devfs on /dev");
    mount(Path::new("ramfs"), Path::new("/dev/shm"), "ramfs")
        .expect("user-test: failed to mount ramfs on /dev/shm");

    ensure_dir("/tmp");
    mount(Path::new("ramfs"), Path::new("/tmp"), "ramfs")
        .expect("user-test: failed to mount ramfs on /tmp");

    ensure_dir("/proc");
    mount(Path::new("proc"), Path::new("/proc"), "proc")
        .expect("user-test: failed to mount procfs on /proc");

    ensure_dir("/bin");
    ensure_dir("/usr");

    crate::busybox::run_bootstrap_busybox(&["busybox", "rm", "-f", "/bin/busybox"], "/bin/busybox");
    crate::busybox::run_bootstrap_busybox(
        &[
            "busybox",
            "ln",
            "-s",
            crate::busybox::bootstrap_busybox(),
            "/bin/busybox",
        ],
        "/bin/busybox",
    );
    crate::busybox::run_busybox(&["busybox", "--install", "-s", "/bin"], "busybox --install");
    crate::runtime::install_bin_sh_ash_wrapper_if_needed();

    crate::busybox::ensure_symlink("/usr/bin", "/bin");
    crate::busybox::ensure_symlink("/usr/sbin", "/bin");
    crate::busybox::ensure_symlink("/sbin", "/bin");
    crate::runtime::ensure_runtime_loader_links();

    println!("user-test: competition environment initialized.");
}

fn ensure_dir(path: &str) {
    if fstatat(AtFd::Cwd, Path::new(path)).is_err() {
        mkdirat(AtFd::Cwd, Path::new(path), 0o755)
            .unwrap_or_else(|_| panic!("user-test: failed to create {path}"));
    }
}

fn ensure_dir_tree(path: &str) {
    let mut current = String::new();
    for component in path.split('/') {
        if component.is_empty() {
            if current.is_empty() {
                current.push('/');
            }
            continue;
        }

        if current.len() > 1 {
            current.push('/');
        }
        current.push_str(component);
        ensure_dir(current.as_str());
    }
}

fn copy_staged_fixture(source: &str, dest: &str) {
    let source_fd = openat(AtFd::Cwd, Path::new(source), O_RDONLY, 0).unwrap_or_else(|errno| {
        panic!("user-test: failed to open staged fixture source {source}: {errno:?}")
    });
    let dest_fd = openat(
        AtFd::Cwd,
        Path::new(dest),
        O_WRONLY | O_CREAT | O_TRUNC,
        0o755,
    )
    .unwrap_or_else(|errno| {
        panic!("user-test: failed to create staged fixture dest {dest}: {errno:?}")
    });

    let mut buf = [0u8; 4096];
    loop {
        let count = read(source_fd, &mut buf).unwrap_or_else(|errno| {
            panic!("user-test: failed to read staged fixture source {source}: {errno:?}")
        });
        if count == 0 {
            break;
        }
        crate::file::write_all(dest_fd, &buf[..count], dest);
    }

    close(source_fd).unwrap_or_else(|errno| {
        panic!("user-test: failed to close staged fixture source {source}: {errno:?}")
    });
    close(dest_fd).unwrap_or_else(|errno| {
        panic!("user-test: failed to close staged fixture dest {dest}: {errno:?}")
    });
}

fn parent_dir(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some(("", _)) => "/",
        Some((parent, _)) => parent,
        None => ".",
    }
}

fn install_staged_competition_fixtures(mountpoint: &str) {
    for fixture in STAGED_COMPETITION_FIXTURES {
        if let Err(errno) = fstatat(AtFd::Cwd, Path::new(fixture.source)) {
            println!(
                "user-test: missing staged competition fixture: source {} -> dest {} ({errno:?})",
                fixture.source, fixture.dest
            );
            panic!("user-test: staged competition fixture source missing");
        }

        let dest = format!("{mountpoint}{}", fixture.dest);
        ensure_dir_tree(parent_dir(dest.as_str()));
        copy_staged_fixture(fixture.source, dest.as_str());
        println!(
            "user-test: installed staged competition fixture: {} -> {}",
            fixture.source, fixture.dest
        );
    }
}
