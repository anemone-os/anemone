use core::{arch::naked_asm, mem::ManuallyDrop};

use riscv::register::sstatus;

use crate::{
    align_down_power_of_2, align_up_power_of_2,
    arch::{
        clear_bss,
        riscv64::{
            exception::install_ktrap_handler,
            mm::{RiscV64PgDir, RiscV64Pte, RiscV64PteFlags, sv39},
        },
    },
    device::{
        console::{Console, ConsoleFlags},
        discovery::open_firmware::{
            EarlyMemoryScanner, early_scan_clock_freq, early_scan_cpu_count, early_scan_fdt_size,
        },
    },
    mm::{kptable::kmap, layout::KernelLayoutTrait, stack::RawKernelStack},
    prelude::*,
    sched::class::{SchedClassPrv, SchedEntity},
    task::tid::Tid,
};

#[unsafe(no_mangle)]
#[unsafe(link_section = ".bss.stack0")]
static mut STACK0: [RawKernelStack; MAX_CPUS] = [RawKernelStack::ZEROED; MAX_CPUS];

#[unsafe(no_mangle)]
static BOOTSTRAP_PGDIR: RiscV64PgDir = {
    // set up kernel mapping here.
    // we use sv39. Hardcoding here, which is a bit ugly. When we support sv48, we
    // should refactor this code to be more flexible.
    //
    // method in trait cannot be called in const context, so we have to manually
    // construct the page table here.

    let mut raw_ptes: [RiscV64Pte; 512] = [RiscV64Pte::ZEROED; 512];

    let k_phys_align_down = align_down_power_of_2!(KERNEL_LA_BASE, 1 << 30);
    let k_phys_ppn = k_phys_align_down as u64 >> 12;
    let k_virt_idx = (KERNEL_VA_BASE >> 30) as usize & 0x1ff;

    // 1. map kernel image to -2gb ~ 0
    assert!(k_virt_idx == 510);
    raw_ptes[k_virt_idx] = RiscV64Pte::arch_new(
        PhysPageNum::new(k_phys_ppn),
        RiscV64PteFlags::BOOTSTRAP_KERNEL,
    );
    raw_ptes[k_virt_idx + 1] = RiscV64Pte::arch_new(
        PhysPageNum::new(k_phys_ppn + 512 * 512),
        RiscV64PteFlags::BOOTSTRAP_KERNEL,
    );

    // 2. direct mapping for code running without page fault
    let direct_idx = k_phys_align_down as usize >> 30;
    raw_ptes[direct_idx] = RiscV64Pte::arch_new(
        PhysPageNum::new(k_phys_ppn),
        RiscV64PteFlags::BOOTSTRAP_KERNEL,
    );
    raw_ptes[direct_idx + 1] = RiscV64Pte::arch_new(
        PhysPageNum::new(k_phys_ppn + 512 * 512),
        RiscV64PteFlags::BOOTSTRAP_KERNEL,
    );

    // 3. HHDM optimistic mapping for later use in physical memory management
    //    initialization. We probably map more physical memory than actually exists,
    //    but it's fine because the kernel will only access the physical memory that
    //    actually exists, and the extra mappings won't cause any harm.
    let s_ram_ppn = align_down_power_of_2!(PHYS_RAM_START, 1 << 30) as u64 >> 12;
    let hhdm_start_idx =
        (((<sv39::Sv39KernelLayout as KernelLayoutTrait<sv39::Sv39PagingArch>>::DIRECT_MAPPING_ADDR
            as usize)
            >> 30)
            & 0x1ff)
            + s_ram_ppn as usize / (512 * 512);
    let hhdm_end_idx = hhdm_start_idx + (align_up_power_of_2!(MAX_PHYS_RAM_SIZE, 1 << 30) >> 30);
    let mut i = hhdm_start_idx;
    while i < hhdm_end_idx {
        let ppn = s_ram_ppn + ((i - hhdm_start_idx) as u64 * 512 * 512);
        raw_ptes[i] = RiscV64Pte::arch_new(PhysPageNum::new(ppn), RiscV64PteFlags::BOOTSTRAP_RAM);
        i += 1;
    }

    unsafe { core::mem::transmute(raw_ptes) }
};

/// Nun. The primordial watery abyss in Egyptian myth, where all things were
/// born.
///
/// Both BSP and APs start executing from here, and will jump to Rust
/// entry point after some basic setup.
#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.bootstrap")]
pub unsafe extern "C" fn __nun() -> ! {
    naked_asm!(

        // We don't want to use gp-relative addressing, so we clear gp
        // to ensure gp-relative accesses will fault.
        "mv  gp, zero",
        // Per-cpu data hasn't been set up yet, so we also clear tp to avoid
        // accidental usage.
        "mv  tp, zero",

        // It is possible that Sbi has already cleared those gprs, but we can't trust that,
        // so we clear them again to be safe.

        // Following code is guaranteed to be position-independent.

        // Note that we're still in lower half.
        "li  t6, {KERNEL_MAPPING_OFFSET}",


        // Use STACK0 as the initial stack.
        "la  t0, {stack0_lower_bound}",
        "li  t1, {KSTACK_KB}",
        "slli    t1, t1, 10",
        // Calculate offset for current hart.
        "addi    t2, a0, 1",

        // Without following line, rustc will emit an error saying 'instruction requires
        // the following: 'Zmmul' (Integer Multiplication)',
        // even though we have specified the 'm' extension in the target spec.
        // This is a known issue in rustc, and we take this workaround to make
        // rustc recognize that we do have the 'm' extension.
        //
        // Refer to https://github.com/rust-lang/rust/issues/80608#issuecomment-2674772735
        // for more details.
        ".attribute arch, \"rv64gc\"",

        "mul     t2, t2, t1",
        "add     sp, t0, t2",
        "add     sp, sp, t6",

        // Enable bootstrap paging.
        "la  t0, {BOOTSTRAP_PGDIR}",
        "li  t1, {bootstrap_satp_mode}",
        "slli    t1, t1, 60",
        "srli    t0, t0, 12",
        "or      t0, t0, t1",
        "csrw    satp, t0",
        "sfence.vma",

        // Clear used temporaries.
        "li  t0, 0",
        "li  t1, 0",
        "li  t2, 0",

        // Jump to Rust entry point.
        "la  t0, {rusty_nun}",
        "add t0, t0, t6",
        "li  t6, 0",
        "jr  t0",
        KERNEL_MAPPING_OFFSET = const sv39::Sv39KernelLayout::KERNEL_MAPPING_OFFSET,
        stack0_lower_bound = sym STACK0,
        KSTACK_KB = const { 1 << KSTACK_SHIFT_KB },
        BOOTSTRAP_PGDIR = sym BOOTSTRAP_PGDIR,
        bootstrap_satp_mode = const 8,
        rusty_nun = sym rusty_nun,
    );
}

// On RiscV architectures, we always register an sbi-based early console.
fn register_earlycon() {
    struct SbiEarlyCon;

    impl Console for SbiEarlyCon {
        fn output(&self, s: &str) {
            // After bsp_primary remaps the boot stack into the REMAP region and
            // switches to it, any stack-allocated data (e.g. LogRecord.msg) has
            // a virtual address in the REMAP region, not the kernel-image region.
            // kvirt_to_phys() only subtracts KERNEL_MAPPING_OFFSET, so it gives
            // a garbage physical address for REMAP-region pointers, causing SBI
            // to read from invalid memory and hang.
            #[unsafe(link_section = ".bss.nonzero_init")]
            static mut SBI_EARLYCON_BUF: [u8; 512] = [0u8; 512];

            let bytes = s.as_bytes();
            let mut remaining = bytes;

            // SAFETY: when we reached here, we have already taken the lock of system
            // console, which ensures that only one CPU can execute this code at the same
            // time.
            #[allow(static_mut_refs)]
            while !remaining.is_empty() {
                let buf = unsafe { &mut SBI_EARLYCON_BUF };
                let chunk_len = remaining.len().min(buf.len());
                buf[..chunk_len].copy_from_slice(&remaining[..chunk_len]);

                let buf_pa = unsafe { VirtAddr::new(buf.as_ptr() as u64).kvirt_to_phys() };
                let pa = sbi_rt::Physical::new(
                    chunk_len,
                    buf_pa.lower_32_bits() as usize,
                    buf_pa.upper_32_bits() as usize,
                );
                let _ = sbi_rt::console_write(pa);

                remaining = &remaining[chunk_len..];
            }
        }
    }

    device::console::register_console(
        Arc::new(SbiEarlyCon),
        ConsoleFlags::EARLY | ConsoleFlags::REPLAY,
    );
}

// Register basic power off and reboot handlers that use Sbi calls. This ensures
// that the system can be powered off or rebooted even if no other power
// management drivers are available.
fn register_basic_power_handlers() {
    struct SbiPower;

    impl PowerOffHandler for SbiPower {
        unsafe fn poweroff(&self) {
            sbi_rt::system_reset(sbi_rt::Shutdown, sbi_rt::NoReason);
            unreachable!()
        }
    }

    impl RebootHandler for SbiPower {
        unsafe fn reboot(&self) {
            sbi_rt::system_reset(sbi_rt::ColdReboot, sbi_rt::NoReason);
            unreachable!()
        }
    }

    register_power_off_handler(Box::new(SbiPower));
    register_reboot_handler(Box::new(SbiPower));
}

/// The Rust entry point of the kernel.
///
/// Note that when we reached here, the paging is not fully set up yet.
///
/// As Sbi specifies, the BSP will start executing first, with all APs parked
/// and waiting for being woken up.
///
/// The 'fdt_pa' argument is only valid for BSP, and APs should ignore it.
#[unsafe(no_mangle)]
extern "C" fn rusty_nun(hart_id: usize, fdt_pa: PhysAddr) -> ! {
    #[unsafe(link_section = ".bss.nonzero_init")]
    static mut BSP_ARRIVED: bool = false;
    unsafe {
        sstatus::set_sum();
    }
    unsafe {
        if !BSP_ARRIVED {
            // bsp
            BSP_ARRIVED = true;
            bsp_setup(hart_id, fdt_pa)
        } else {
            // ap
            ap_setup(hart_id)
        }
    }
}

/// percpu guarded stack top addresses, filled by [`remap_boot_stacks`].
static GUARDED_STACK_TOPS: MonoOnce<[VirtAddr; MAX_CPUS]> = unsafe { MonoOnce::new() };

/// Remap every CPU's boot stack ([`STACK0`]) into the remap region with a
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
        // The first page is the guard – we simply leave it unmapped. The underlying pte
        // should be empty.
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
            "cpu #{}: boot stack remapped with guard page at [{:#x}, {:#x}), stack [{:#x}, {:#x})",
            cpu,
            vrange.start().to_virt_addr().get(),
            stack_vpn.to_virt_addr().get(),
            stack_vpn.to_virt_addr().get(),
            stack_top.get(),
        );
    }

    GUARDED_STACK_TOPS.init(|g| {
        g.write(tops);
    });
}

#[inline(always)]
unsafe fn switch_to_guarded(dest_entry: VirtAddr) -> ! {
    let cpu_id = cur_cpu_id().get();
    let new_stack_top = GUARDED_STACK_TOPS.get()[cpu_id];

    unsafe {
        core::arch::asm!(
            "mv  sp, {new_top}",
            "mv  fp, zero",
            "jr  {entry}",
            new_top = in(reg) new_stack_top.get(),
            entry = in(reg) dest_entry.get(),
            options(noreturn),
        )
    }
}

static INIT_SYNC_COUNTER: CpuSync = CpuSync::new("registering init task");

unsafe fn bsp_setup(bsp_id: usize, fdt_pa: PhysAddr) -> ! {
    unsafe {
        clear_bss();
    }
    register_basic_power_handlers();
    // set up kernel trap handler
    install_ktrap_handler();

    register_earlycon();

    let fdt_va = sv39::Sv39KernelLayout::phys_to_dm(fdt_pa);

    unsafe {
        // needed by percpu initialization.
        let ncpus = early_scan_cpu_count(fdt_va);

        kinfoln!("anemone kernel booting on bsp #{}", bsp_id);

        // needed by timer initialization.
        if let Some(freq_hz) = early_scan_clock_freq(fdt_va) {
            super::time::set_hw_clock_freq(freq_hz);
        } else {
            kwarningln!("failed to scan clock frequency from device tree.");
        };
        let mut scanner = EarlyMemoryScanner::new(fdt_va);

        // mark fdt as reserved memory so that it won't be allocated by frame allocator.
        let fdt_npages = (early_scan_fdt_size(fdt_va) + PagingArch::PAGE_SIZE_BYTES - 1)
            / PagingArch::PAGE_SIZE_BYTES;
        let fdt_ppn = PhysPageNum::new(fdt_pa.get() >> PagingArch::PAGE_SIZE_BITS);
        scanner.mark_as_reserved(fdt_ppn, fdt_npages as u64, RsvMemFlags::FDT);

        percpu::bsp_init(bsp_id, ncpus, |npages| {
            scanner.early_alloc_folio(npages as u64)
        });
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

        wake_up_aps(bsp_id);

        knoticeln!("stage 1 bootstrap finished, switching to stage 2...");
        set_boot_mono(true);
        let (bsp_kinit, guard) = unsafe {
            Task::new_kernel(
                "kinit-bsp",
                bsp_kinit as *const (),
                ParameterList::new(&[bsp_id as u64, fdt_va.get()]),
                SchedEntity::new(SchedClassPrv::RoundRobin(())),
                TaskFlags::NONE,
                Some(cur_cpu_id()),
            )
        }
        .unwrap_or_else(|e| panic!("failed to create bsp kinit task: {:?}", e));

        let bsp_kinit = RegisterGuard::register_root(guard, bsp_kinit);
        INIT_SYNC_COUNTER.sync_with_counter();

        sched::init_routines::local_enqueue_first(bsp_kinit);
        switch_to_guarded(VirtAddr::new(scheduler as *const () as u64))
    }
}

unsafe fn wake_up_aps(bsp_id: usize) {
    unsafe {
        for ap_id in 0..ncpus() {
            if ap_id == bsp_id {
                continue;
            }
            kdebugln!("waking up ap {}", ap_id);
            let sbiret = sbi_rt::hart_start(
                ap_id,
                VirtAddr::new(__nun as *const () as u64)
                    .kvirt_to_phys()
                    .get() as usize,
                0,
            );
            if sbiret.is_err() {
                panic!("failed to start hart {}: {:?}", ap_id, sbiret);
            }
        }
    }
}

unsafe fn ap_setup(ap_id: usize) -> ! {
    unsafe {
        install_ktrap_handler();
        percpu::ap_init(ap_id);
        mm::kptable::activate_kernel_mapping();
        kdebugln!("anemone kernel booting on ap #{}", ap_id);
        set_boot_mono(false);

        INIT_SYNC_COUNTER.sync_with_counter();
        // now init task has been registered.
        let (ap_kinit, guard) = Task::new_kernel(
            "kinit-ap",
            ap_kinit as *const (),
            ParameterList::new(&[ap_id as u64]),
            SchedEntity::new(SchedClassPrv::RoundRobin(())),
            TaskFlags::NONE,
            Some(cur_cpu_id()),
        )
        .unwrap_or_else(|e| panic!("failed to create ap kinit task: {:?}", e));
        let ap_kinit = guard.register(ap_kinit, TaskBinding { parent: Tid::INIT });

        sched::init_routines::local_enqueue_first(ap_kinit);
        switch_to_guarded(VirtAddr::new(scheduler as *const () as u64))
    }
}
