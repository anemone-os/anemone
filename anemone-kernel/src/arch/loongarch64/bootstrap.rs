//! Bootstrap for Loongarch64 architecture.

use core::{arch::naked_asm, mem::ManuallyDrop};

use la_insc::{
    reg::{
        csr::{
            CR_CPUID, CR_CRMD, CR_DMW0, CR_DMW1, CR_DMW2, CR_PGDH, CR_PGDL, CR_PWCH, CR_PWCL,
            CR_TLBRENTRY, dmw2,
        },
        dmw::Dmw,
    },
    utils::{mem::MemAccessType, privl::PrivilegeFlags},
};
use loongArch64::ipi::csr_mail_send;

use crate::{
    arch::{
        clear_bss,
        loongarch64::{
            exception::install_ktrap_handler,
            mm::{BOOT_DMW0_DM, BOOTSTRAP_PTABLE, PWCH, PWCL, refill::__tlb_rfill},
        },
    },
    device::discovery::open_firmware::{
        EarlyMemoryScanner, early_scan_clock_freq, early_scan_cpu_count,
    },
    mm::{kptable::kmap, layout::KernelLayoutTrait, stack::RawKernelStack},
    prelude::*,
    sync::counter::CpuSync,
    task::clone::CloneFlags,
};

#[unsafe(no_mangle)]
#[unsafe(link_section = ".bss.stack0")]
/// Per-CPU bootstrap stacks used before the regular per-CPU stack remap is in
/// place.
static mut STACK0: [RawKernelStack; MAX_CPUS] = [RawKernelStack::ZEROED; MAX_CPUS];

/// Temporary I/O space base address used during early boot before the full
/// memory manager is online.
const TEMP_IO_SPACE: u64 = 0x8000_0000_0000_0000;

/// Flattened device tree blob embedded by the build.
static DTB_BYTES: &[u8] = include_bytes_aligned_as!(PhantomAligned8, "generated.dtb");

/// # Note
/// LoongArch64 boots without SBI, so the entry point is fixed and [`__nun`]
/// would otherwise be considered unused by the compiler.
///
/// This static keeps the entry point referenced.
#[used]
static __NUN_KEEPER: unsafe extern "C" fn() -> ! = __nun;

/// Kernel entry point.
///
/// The first CPU uses this path directly, while secondary CPUs reach it through
/// [wake_up_aps].
#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.bootstrap")]
pub unsafe extern "C" fn __nun() -> ! {
    naked_asm!(
        // Enable address mapping
        "
            li.d    $t0, {boot_dmw0}
            csrwr   $t0, {cr_dmw0}
            
            li.d    $t0, {boot_dmw1}
            csrwr   $t0, {cr_dmw1}

            li.d    $t0, {boot_dmw2}
            csrwr   $t0, {cr_dmw2}

            li.w    $t0, 0xb8   // IE=0, PLV=0, DA=0, PG=1
            csrwr   $t0, {cr_crmd}
        ",
        // Set up page table configuration
        "
            li.d    $t0, {pwcl}
            csrwr   $t0, {cr_pwcl}
            li.d    $t0, {pwch}
            csrwr   $t0, {cr_pwch}
            li.d    $t2, {k_offset}
            la.global   $t0, {bootstrap_ptable}
            #sub.d       $t0, $t0, $t2
            csrwr   $t0, {cr_pgdh}
            la.global   $t0, {bootstrap_ptable}
            #sub.d       $t0, $t0, $t2
            csrwr   $t0, {cr_pgdl}
        ",
        // Set up stack and refill handler
        "

            li.d        $t2, {k_offset}
            la.global   $sp, {boot_stack}
            csrrd       $t0, {cr_cpuid}
            li.d        $t1, {stack_size}
            addi.d      $t0, $t0, 0x1
            mul.d       $t0, $t0, $t1
            add.d       $sp, $sp, $t0
            or          $sp, $sp, $t2


            la.global   $t0, {tlb_rfill}
            #sub.d       $t0, $t0, $t2
            csrwr       $t0, {tlbr_entry}
        ",
        // Jump
        "
            // remove dmw1
            li.d    $t0, 0
            csrwr   $t0, {cr_dmw1}

            csrrd       $a0, {cr_cpuid} // arg0: hart_id
            la.global   $t0, {rusty_nun}
            or          $t0, $t0, $t2
            jirl        $zero,$t0,0
        ",
        boot_dmw0 = const BOOT_DMW0_DM.to_u64(),
        // temporary
        boot_dmw1 = const Dmw::new(
                PrivilegeFlags::PLV0,
                MemAccessType::Cache,
                Dmw::vseg_from_addr(0),
            ).to_u64(),
        // temporary IO space
        boot_dmw2 = const Dmw::new(
                PrivilegeFlags::PLV0,
                MemAccessType::StrongNonCache,
                Dmw::vseg_from_addr(TEMP_IO_SPACE),
            ).to_u64(),
        cr_dmw0 = const CR_DMW0,
        cr_dmw1 = const CR_DMW1,
        cr_dmw2 = const CR_DMW2,

        tlbr_entry = const CR_TLBRENTRY,

        cr_crmd = const CR_CRMD,
        // cr_prmd = const CR_PRMD,
        cr_cpuid = const CR_CPUID,

        pwcl = const PWCL.to_u32(),
        cr_pwcl = const CR_PWCL,
        pwch = const PWCH.to_u32(),
        cr_pwch = const CR_PWCH,

        boot_stack = sym STACK0,
        stack_size = const (1 << KSTACK_SHIFT_KB) as usize * 1024,

        k_offset = const KernelLayout::KERNEL_MAPPING_OFFSET,

        rusty_nun = sym rusty_nun,

        bootstrap_ptable = sym BOOTSTRAP_PTABLE,
        cr_pgdh = const CR_PGDH,
        cr_pgdl = const CR_PGDL,

        tlb_rfill = sym __tlb_rfill
    )
}

#[unsafe(no_mangle)]
extern "C" fn rusty_nun(hart_id: usize) -> ! {
    #[unsafe(link_section = ".bss.nonzero_init")]
    static mut BSP_ARRIVED: bool = false;
    unsafe {
        if !BSP_ARRIVED {
            BSP_ARRIVED = true;
            bsp_setup(hart_id, VirtAddr::new(DTB_BYTES.as_ptr() as u64))
        } else {
            // ap
            ap_setup(hart_id)
        }
    }
}

/// Register the early debug console that is backed by the temporary I/O DMW.
pub fn register_debugcon() {
    use crate::{
        device::console::{Console, ConsoleFlags, register_console},
        driver::Ns16550ARegisters,
    };

    const DEBUG_CON_REG: u64 = 0x1fe0_01e0;
    let con = unsafe {
        Ns16550ARegisters::from_raw(
            (TEMP_IO_SPACE + DEBUG_CON_REG) as *const u8 as *mut u8,
            0,
            1,
        )
    };
    pub struct DebugCon {
        con: SpinLock<Ns16550ARegisters>,
    }
    unsafe impl Send for DebugCon {}
    unsafe impl Sync for DebugCon {}
    impl Console for DebugCon {
        fn output(&self, s: &str) {
            use core::fmt::Write;
            let _ = self.con.lock_irqsave().write_str(s);
        }
    }
    impl Drop for DebugCon {
        fn drop(&mut self) {
            unsafe {
                // remove temporary IO space
                dmw2::csr_write(Dmw::from_u64(0));
            }
        }
    }
    let debug_con = DebugCon {
        con: SpinLock::new(con),
    };
    register_console(
        Arc::new(debug_con),
        ConsoleFlags::EARLY | ConsoleFlags::REPLAY,
    );
}

unsafe fn bsp_setup(bsp_id: usize, fdt_va: VirtAddr) -> ! {
    unsafe {
        clear_bss();
    }
    //register_basic_power_handlers();
    // set up kernel trap handler
    install_ktrap_handler();

    register_debugcon();

    unsafe {
        // needed by percpu initialization.
        let ncpus = early_scan_cpu_count(fdt_va);
        super::cpu::set_ncpus(ncpus);
        kinfoln!("anemone kernel booting on bsp #{}", bsp_id);

        wake_up_aps(bsp_id, ncpus);

        // needed by timer initialization.
        if let Some(freq_hz) = early_scan_clock_freq(fdt_va) {
            super::time::set_hw_clock_freq(freq_hz);
        } else {
            kwarningln!("failed to scan clock frequency from device tree.");
        };

        let mut scanner = EarlyMemoryScanner::new(fdt_va);

        mm::percpu::bsp_init(bsp_id, |npages| scanner.early_alloc_folio(npages as u64));
        kinfoln!("percpu data initialized");

        scanner.commit_to_pmm();
        mm::frame::memmap_init(|npages| {
            kdebugln!("memmap init: allocating {} pages", npages);
            sys_mem_zones()
                .leak(npages)
                .expect("no enough memory to initialize memmap")
        });
        mm::frame::pmm_init();
        kinfoln!("physical memory management initialized");

        mm::kptable::init_kernel_mapping();
        kinfoln!("kernel mapping initialized");
        mm::kptable::activate_kernel_mapping();
        kinfoln!("kernel mapping activated");

        remap_boot_stack();
        BOOT_SYNC_COUNTER.sync_with_counter();

        knoticeln!("stage 1 bootstrap finished, switching to stage 2...");
        let kinit_task = Task::new_kernel(
            "bsp-kinit",
            bsp_kinit as *const (),
            ParameterList::new(&[bsp_id as u64, fdt_va.get()]),
            IntrArch::DISABLED_IRQ_FLAGS,
            TaskFlags::NONE,
            CloneFlags::empty(),
        )
        .unwrap_or_else(|e| panic!("failed to create bsp kinit task: {:?}", e));
        register_root_task(kinit_task.clone());
        add_to_ready(kinit_task);
        switch_to_guarded(VirtAddr::new(run_tasks as *const () as u64))
    }
}

/// Synchronization point that keeps all CPUs aligned during bootstrap.
static BOOT_SYNC_COUNTER: CpuSync = CpuSync::new("boot");

unsafe fn ap_setup(ap_id: usize) -> ! {
    unsafe {
        BOOT_SYNC_COUNTER.sync_with_counter();
        kdebugln!("anemone kernel booting on ap #{}", ap_id);
        install_ktrap_handler();
        mm::percpu::ap_init(ap_id);
        mm::kptable::activate_kernel_mapping();
        let ap_kinit = Task::new_kernel(
            "ap-kinit",
            ap_kinit as *const (),
            ParameterList::new(&[ap_id as u64]),
            IntrArch::DISABLED_IRQ_FLAGS,
            TaskFlags::NONE,
            CloneFlags::empty(),
        )
        .unwrap_or_else(|e| panic!("failed to create ap kinit task: {:?}", e));
        ap_kinit.add_as_child(wait_for_root_task());
        add_to_ready(ap_kinit);
        switch_to_guarded(VirtAddr::new(run_tasks as *const () as u64));
    }
}

/// Send the bootstrap entry address to every secondary CPU and trigger its
/// IPI.
pub fn wake_up_aps(bsp_id: usize, ncpus: usize) {
    unsafe {
        let cur_cpu = bsp_id;
        let st_addr = VirtAddr::new(__nun as *const () as u64)
            .kvirt_to_phys()
            .get();
        for cpu in 0..ncpus {
            if cpu == cur_cpu {
                continue;
            }
            csr_mail_send(st_addr, cpu, 0);
            IntrArch::send_ipi(cpu);
        }
    }
}

#[inline(always)]
unsafe fn switch_to_guarded(dest_entry: VirtAddr) -> ! {
    let cpu_id = CpuArch::cur_cpu_id().get();
    let new_stack_top = GUARDED_STACK_TOPS.get()[cpu_id];
    unsafe {
        core::arch::asm!(
            "move  $sp, {new_top}",
            "jirl  $zero, {entry}, 0 ",
            new_top = in(reg) new_stack_top.get(),
            entry = in(reg) dest_entry.get(),
            options(noreturn),
        )
    }
}

/// Per-CPU guarded stack tops, filled by [remap_boot_stack].
static GUARDED_STACK_TOPS: MonoOnce<[VirtAddr; MAX_CPUS]> = unsafe { MonoOnce::new() };

/// Remap every CPU's bootstrap stack ([`STACK0`]) into the remap region with a
/// guard page at the bottom.
///
/// # Safety
///
/// Must be called by bsp **after** `init_kernel_mapping()` +
/// `activate_kernel_mapping()` and **before** `wake_up_aps()`.
unsafe fn remap_boot_stack() {
    const KSTACK_PAGES: usize = (1 << KSTACK_SHIFT_KB) * 1024 / PagingArch::PAGE_SIZE_BYTES;

    let total_vpages = 1 + KSTACK_PAGES;
    let stack0_sppn =
        unsafe { VirtAddr::new(core::ptr::addr_of!(STACK0) as u64).kvirt_to_phys() }.page_down();

    let mut tops: [VirtAddr; MAX_CPUS] = [VirtAddr::new(0); MAX_CPUS];

    for cpu in 0..MAX_CPUS {
        let cpu_stack_ppn = stack0_sppn + (cpu as u64 * KSTACK_PAGES as u64);

        let vrange = unsafe { mm::remap::alloc_virt_range(total_vpages) }
            .expect("failed to allocate virtual range for boot stack guard page");
        // The first page is the guard page, so it stays unmapped.
        let stack_vpn = vrange.start() + 1;
        unsafe {
            let _guard = kmap(Mapping {
                vpn: stack_vpn,
                ppn: cpu_stack_ppn,
                flags: PteFlags::READ | PteFlags::WRITE | PteFlags::GLOBAL,
                npages: KSTACK_PAGES,
                huge_pages: false,
            })
            .expect("failed to map boot stack with guard page");

            // no need to send ipi right now cz aps haven't activate kernel mapping.
            let _ = ManuallyDrop::new(_guard);
        }

        let stack_top = (stack_vpn + KSTACK_PAGES as u64).to_virt_addr();

        tops[cpu] = stack_top;

        kinfoln!(
            "cpu #{}: boot stack remapped with guard page at [{:#x}, {:#x}), stack [{:#x}, {:#x}), from [{:#x}, {:#x}]",
            cpu,
            vrange.start().to_virt_addr().get(),
            stack_vpn.to_virt_addr().get(),
            stack_vpn.to_virt_addr().get(),
            stack_top.get(),
            cpu_stack_ppn.to_kvirt().to_virt_addr().get(),
            cpu_stack_ppn.to_kvirt().to_virt_addr().get() + (KSTACK_PAGES as u64 * 4096),
        );
    }

    GUARDED_STACK_TOPS.init(|g| {
        g.write(tops);
    });
}
