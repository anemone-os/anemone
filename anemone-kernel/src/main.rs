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
pub mod power;
pub mod sched;
pub mod sync;
pub mod syscall;
pub mod syserror;
pub mod task;
pub mod time;
pub mod utils;

use crate::{
    device::discovery::open_firmware::{
        get_of_node, of_platform_discovery, of_with_node_by_full_name_path, of_with_root,
        unflatten_device_tree,
    },
    fs::vfs_mount,
    mm::layout::KernelLayoutTrait,
    prelude::{dt::user_pointer, image::load_image_from_elf, *},
    sync::{counter::CpuSync, mono::MonoOnce},
};

static INIT_SYNC_COUNTER: CpuSync = CpuSync::new("init");
static FINISH_SYNC_COUNTER: CpuSync = CpuSync::new("finish");
static KUNIT_SYNC_COUNTER: CpuSync = CpuSync::new("kunit");

unsafe extern "C" fn bsp_kinit(bsp_id: usize, fdt_va: VirtAddr) {
    unsafe {
        kinfoln!("bsp #{} kinit running on {}...", bsp_id, current_task_id());
        syscall::register_syscall_handlers();
        // register filesystem drivers
        fs::init();
        // register drivers to bus types
        driver::init();
        unflatten_device_tree(fdt_va);
        parse_bootargs();
        machine_init();
        of_platform_discovery();

        IntrArch::init_local_irq();
        unsafe {
            device::console::on_system_boot();
        }
        INIT_SYNC_COUNTER.sync_with_counter();

        FINISH_SYNC_COUNTER.sync_with_counter();
        kinfoln!("bsp #{} kinit finished", bsp_id);

        // mount a ramfs as temporary root filesystem.
        vfs_mount("ramfs", MountSource::Pseudo, MountFlags::empty(), None).unwrap();

        #[cfg(feature = "kunit")]
        {
            kinfoln!("running kunit tests");
            crate::debug::kunit::kunit_runner();
            KUNIT_SYNC_COUNTER.sync_with_counter();
            kinfoln!("kunit tests finished");
        }
    }
    let image = load_image_from_elf(APP0, &["command0"]).unwrap();
    add_to_ready(Arc::new(
        Task::new_user(
            "user",
            image.entry as *const (),
            Arc::new(image.memsp),
            VirtAddr::new(KernelLayout::USPACE_TOP_ADDR),
        )
        .unwrap(),
    ));
}

unsafe extern "C" fn ap_kinit(ap_id: usize) {
    unsafe {
        INIT_SYNC_COUNTER.sync_with_counter();
        kinfoln!("ap #{} kinit running on {}...", ap_id, current_task_id());
        IntrArch::init_local_irq();

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

static APP0: &[u8] = include_bytes!("../../build/apps/user-test.elf");
