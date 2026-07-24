//! Anemone kernel.

#![no_std]
#![no_main]
#![allow(unused)]
#![warn(unused_imports)]
// **IMPORTANT**
// **UNSTABLE FEATURES SHOULD BE AVOIDED WHENEVER POSSIBLE, SINCE THEY MAY CAUSE
// COMPATIBILITY ISSUES IN THE FUTURE.**
// **EVERY TIME A NEW UNSTABLE FEATURE IS ADDED, IT SHOULD BE DOCUMENTED.**

// This feature must be enabled for zero-cost downcasting of trait objects to get the same
// efficiency as C's void* and manual casts, which is crucial for the performance of the kernel.
#![feature(downcast_unchecked)]
// This feature is required for fallible heap allocation. Yes, Rust's default global allocator is
// infallible, which is unacceptable for kernel environments...
#![feature(allocator_api)]

extern crate alloc;

pub mod kconfig_defs;
pub mod platform_defs;

pub mod prelude;

pub mod arch;
pub mod debug;
pub mod device;
pub mod driver;
pub mod exception;
pub mod fs;
pub mod initcall;
pub mod mm;
pub mod panic;
pub mod percpu;
pub mod power;
pub mod sched;
pub mod sync;
pub mod syscall;
pub mod syserror;
pub mod task;
pub mod time;
pub mod utils;
pub mod uts;

use crate::{
    device::discovery::{
        open_firmware::{of_init_stdout, of_platform_discovery, unflatten_device_tree},
        probe_virtual_devices,
    },
    initcall::{InitCallLevel, run_initcalls},
    mm::layout::KernelLayoutTrait,
    percpu::percpu_login,
    prelude::*,
    sync::{counter::CpuSync, mono::MonoOnce},
    task::{
        execve::kernel::kernel_execve,
        files::{FdFlags, FileStatusFlags, LinuxOpenCompat, OpenAccessMode},
        task_fs::FsState,
    },
};

static INIT_SYNC_COUNTER: CpuSync = CpuSync::new("init");
static FINISH_SYNC_COUNTER: CpuSync = CpuSync::new("finish");
#[cfg(feature = "kunit")]
static KUNIT_SYNC_COUNTER: CpuSync = CpuSync::new("kunit");

fn mount_rootfs() {
    match ROOTFS_SOURCE_KIND {
        "pseudo" => {
            mount_root("ramfs", MountSource::Pseudo, MountAttrFlags::empty())
                .expect("root mount failed");
        },
        "block" => {
            let rootfs_path = ROOTFS_SOURCE_PATH
                .expect("rootfs source path must be configured for block-backed rootfs");
            let root_dev = device::block::get_block_dev_by_name(rootfs_path)
                .unwrap_or_else(|| panic!("rootfs block device not found: {}", rootfs_path));
            mount_root(
                ROOTFS_FS_TYPE,
                MountSource::Block(root_dev),
                MountAttrFlags::empty(),
            )
            .expect("root mount failed");
        },
        other => panic!("unsupported rootfs source kind: {}", other),
    }
    ls_dir(Path::new("/"));
}

// recursively ls
fn ls_dir(path: &Path) {
    const MAX_ENTRIES: usize = 256;

    let mut sink = FixedSizeDirSink::<MAX_ENTRIES>::new();

    let Ok(dir) = vfs_open(PathResolution::normal(path)) else {
        return;
    };

    loop {
        sink.clear();
        match dir.read_dir(&mut sink) {
            Ok(ReadDirResult::Progressed) => {
                for DirEntry { name, ino, ty } in sink.entries() {
                    if name == "." || name == ".." {
                        continue;
                    }

                    let path = path.join(name);
                    kdebugln!("{} ({:?})", path.display(), ty);
                    if *ty == InodeType::Dir {
                        ls_dir(&path);
                    }
                }
            },
            Ok(ReadDirResult::Eof) => break,
            Err(e) => panic!("failed to read dir {}: {:?}", path.display(), e),
        }
    }
}

/// According to Anemone Boot Protocol, /.anemone/init is a file containing a
/// absolute path pointing to the init process executable.
fn exec_init_proc(init_stdio: device::boot_io::InitStdio) {
    const INIT_PATH: &str = "/.anemone/init";

    let init_path = vfs_read_to_string(PathResolution::normal(&Path::new(INIT_PATH)))
        .unwrap_or_else(|e| panic!("failed to read init path from {}: {:?}", INIT_PATH, e));

    // open initial stdio fds so that they can be inherited.
    {
        let kinit = get_current_task();
        let [stdin, stdout, stderr] = init_stdio.into_files();
        let open_stdio = |file: File, access| {
            let status = FileStatusFlags::empty();
            // Boot stdio uses three normal files backed by one shared Terminal;
            // no Linux open flags are accepted here, but keep the status hook
            // boundary explicit.
            file.check_status_flags(status.to_file_op_status_flags())
                .expect("initial stdio status rejected");
            kinit
                .open_fd(
                    file,
                    access,
                    status,
                    LinuxOpenCompat::empty(),
                    FdFlags::empty(),
                )
                .expect("failed to open initial stdio fd");
        };
        open_stdio(stdin, OpenAccessMode::Read);
        open_stdio(stdout, OpenAccessMode::Write);
        open_stdio(stderr, OpenAccessMode::Write);
    }

    // set up initial root and cwd for inheritance.
    {
        let kinit = get_current_task();
        kinit.set_fs_state(FsState::new_root());
    }

    kernel_execve(
        &init_path,
        &[&init_path],
        &["OS=anemone", "one=1", "two=2", "three=3", "MIKU=39"],
    )
    .unwrap_or_else(|e| {
        panic!(
            "failed to execve init process at path specified by {}: {:?}",
            INIT_PATH, e
        );
    });
}

/// **System Invariant**
///
/// - When bootstrap processor reaches [bsp_kinit], interrupts are disabled in
///   terms of effect. (e.g. on RiscV, sstatus::sie can be set, but sie::ssoft,
///   sie::stimer and sie::sext interrupts should be disabled.)
unsafe extern "C" fn bsp_kinit(bsp_id: usize, fdt_va: VirtAddr) {
    let bsp_id = CpuId::new(bsp_id);
    let init_stdio = unsafe {
        kinfoln!("BSP {} kinit running on {}...", bsp_id, current_task_id());
        syscall::register_syscall_handlers();
        fs::register_filesystem_drivers();
        driver::register_builtin_drivers();
        unflatten_device_tree(fdt_va);
        parse_bootargs();
        machine_init();
        of_platform_discovery();
        probe_virtual_devices();

        program_first_timer();
        percpu_login();
        IntrArch::init_local_irq();
        task::kthread::init_kthreadd();

        let console_selection = device::console::finish_boot_selection();
        INIT_SYNC_COUNTER.sync_with_counter();

        FINISH_SYNC_COUNTER.sync_with_counter();
        // Ordinary kthreads may round-robin onto any CPU, so wait until every CPU
        // has completed local init and marked itself online before late services
        // publish their workers. `kthreadd` remains a hand-built boot invariant.
        run_initcalls(InitCallLevel::Late);
        let init_stdio = device::boot_io::finalize(console_selection)
            .expect("failed to finalize boot console and TTY endpoints");
        kinfoln!("BSP {} kinit finished", bsp_id);
        init_stdio
    };

    mount_rootfs();

    #[cfg(feature = "kunit")]
    {
        crate::debug::kunit::kunit_runner();
        unsafe {
            KUNIT_SYNC_COUNTER.sync_with_counter();
        }
    }

    exec_init_proc(init_stdio);
}

fn parse_bootargs() {
    of_init_stdout()
        .unwrap_or_else(|error| panic!("failed to initialize stdout-path: {:?}", error));
}

/// **System Invariant**
///
/// - When application processors reach [ap_kinit], interrupts are disabled in
///   terms of effect. (e.g. on RiscV, sstatus::sie can be set, but sie::ssoft,
///   sie::stimer and sie::sext interrupts should be disabled.)
///
/// TODO: do we really need a separate kinit function for APs? maybe a single
/// bsp_kinit is enough.
unsafe extern "C" fn ap_kinit(ap_id: usize) {
    let ap_id = CpuId::new(ap_id);
    unsafe {
        INIT_SYNC_COUNTER.sync_with_counter();
        kinfoln!("AP {} kinit running on {}...", ap_id, current_task_id());

        program_first_timer();
        percpu_login();
        IntrArch::init_local_irq();

        // collect previous IPIs sent by bsp before ap starts to run.
        // the main reason for this is to clear IPI buffers of bsp such that it can send
        // IPIs to other APs again.
        IntrArch::claim_ipi();

        // synchronize with BSP

        FINISH_SYNC_COUNTER.sync_with_counter();
        kinfoln!("AP {} kinit finished", ap_id);

        #[cfg(feature = "kunit")]
        KUNIT_SYNC_COUNTER.sync_with_counter();
    }
    // exit
}
