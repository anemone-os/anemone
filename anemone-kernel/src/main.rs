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
pub mod net;
pub mod panic;
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
        open_firmware::{
            get_of_node, of_platform_discovery, of_with_node_by_full_name_path, of_with_root,
            unflatten_device_tree,
        },
        probe_virtual_devices,
    },
    mm::layout::KernelLayoutTrait,
    prelude::*,
    sync::{counter::CpuSync, mono::MonoOnce},
    task::{execve::kernel_execve, task_fs::FsState},
};

static INIT_SYNC_COUNTER: CpuSync = CpuSync::new("init");
static FINISH_SYNC_COUNTER: CpuSync = CpuSync::new("finish");
#[cfg(feature = "kunit")]
static KUNIT_SYNC_COUNTER: CpuSync = CpuSync::new("kunit");

fn mount_rootfs() {
    match ROOTFS_SOURCE_KIND {
        "pseudo" => {
            mount_root("ramfs", MountSource::Pseudo, MountFlags::empty())
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
                MountFlags::empty(),
            )
            .expect("root mount failed");
        },
        other => panic!("unsupported rootfs source kind: {}", other),
    }

    ls_dir(Path::new("/"));
}

// recursively ls
fn ls_dir(path: &Path) {
    let mut ctx = DirContext::new();

    let Ok(dir) = vfs_open(path) else {
        return;
    };

    while let Ok(dirent) = dir.iterate(&mut ctx) {
        if dirent.name == "." || dirent.name == ".." {
            continue;
        }

        let name = dirent.name;
        let path = path.join(name);
        kdebugln!("{} ({:?})", path.display(), dirent.ty);
        if dirent.ty == InodeType::Dir {
            ls_dir(&path);
        }
    }
}

/// According to Anemone Boot Protocol, /.anemone/init is a file containing a
/// absolute path pointing to the init process executable.
fn exec_init_proc() {
    const INIT_PATH: &str = "/.anemone/init";

    let init_path = vfs_read_to_string(Path::new(INIT_PATH))
        .unwrap_or_else(|e| panic!("failed to read init path from {}: {:?}", INIT_PATH, e));

    kernel_execve(&init_path, &[&init_path, &"1".to_string()]).unwrap_or_else(|e| {
        panic!(
            "failed to execve init process at path specified by {}: {:?}",
            INIT_PATH, e
        );
    });
}

unsafe extern "C" fn bsp_kinit(bsp_id: usize, fdt_va: VirtAddr) {
    unsafe {
        kinfoln!("bsp #{} kinit running on {}...", bsp_id, current_task_id());
        syscall::register_syscall_handlers();
        fs::register_filesystem_drivers();
        driver::register_builtin_drivers();
        unflatten_device_tree(fdt_va);
        parse_bootargs();
        machine_init();
        of_platform_discovery();
        probe_virtual_devices();

        IntrArch::init_local_irq();
        percpu_login();

        unsafe {
            device::console::on_system_boot();
        }
        INIT_SYNC_COUNTER.sync_with_counter();

        FINISH_SYNC_COUNTER.sync_with_counter();
        kinfoln!("bsp #{} kinit finished", bsp_id);
    }
    mount_rootfs();

    #[cfg(feature = "kunit")]
    {
        kinfoln!("running kunit tests");
        crate::debug::kunit::kunit_runner();
        unsafe {
            KUNIT_SYNC_COUNTER.sync_with_counter();
        }
        kinfoln!("kunit tests finished");
    }

    with_current_task(|kinit| {
        kinit.set_fs_state(FsState::new_root());
    });

    exec_init_proc();
}

unsafe extern "C" fn ap_kinit(ap_id: usize) {
    unsafe {
        INIT_SYNC_COUNTER.sync_with_counter();
        kinfoln!("ap #{} kinit running on {}...", ap_id, current_task_id());
        IntrArch::init_local_irq();
        percpu_login();

        // collect previous IPIs sent by bsp before ap starts to run.
        // the main reason for this is to clear IPI buffers of bsp such that it can send
        // IPIs to other APs again.
        IntrArch::claim_ipi();

        // synchronize with BSP

        FINISH_SYNC_COUNTER.sync_with_counter();
        kinfoln!("ap #{} kinit finished", ap_id);

        #[cfg(feature = "kunit")]
        KUNIT_SYNC_COUNTER.sync_with_counter();
    }
    // exit
}

fn parse_bootargs() {
    of_with_root(|root| {
        root.children().for_each(|child| {
            if child.name() == "chosen" {
                if let Some(stdout_path) = child.property("stdout-path") {
                    if let Some(stdout_path) = stdout_path.value_as_string() {
                        kinfoln!("stdout-path: {}", stdout_path);

                        if of_with_node_by_full_name_path(stdout_path, |node| {
                            get_of_node(node.handle()).mark_as_stdout();
                        })
                        .is_err()
                        {
                            panic!(
                                "device tree node specified by stdout-path not found: {}",
                                stdout_path
                            );
                        }
                    }
                }
            }
        })
    });
}
