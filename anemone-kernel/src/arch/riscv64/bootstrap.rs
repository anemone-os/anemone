use core::{arch::naked_asm, sync::atomic::AtomicUsize};

use crate::{
    align_down_power_of_2, align_up_power_of_2,
    arch::{
        clear_bss,
        riscv64::{
            exception::{enable_local_irq, install_ktrap_handler},
            machine::machine_init,
            mm::{RiscV64PgDir, RiscV64Pte, RiscV64PteFlags, sv39},
        },
    },
    device::{
        console::{Console, ConsoleFlags},
        discovery::open_firmware::{
            EarlyMemoryScanner, early_scan_clock_freq, early_scan_cpu_count, early_scan_fdt_size,
            get_of_node, of_platform_discovery, of_with_node_by_full_name_path, of_with_root,
            unflatten_device_tree,
        },
    },
    mm::layout::KernelLayoutTrait,
    prelude::*,
    utils::align::{AlignedBytes, PhantomAligned4096},
};

#[unsafe(no_mangle)]
#[unsafe(link_section = ".bss.stack0")]
static mut STACK0: [AlignedBytes<
    PhantomAligned4096,
    [u8; (1 << KSTACK_SHIFT_KB) as usize * 1024],
>; MAX_CPUS] = [AlignedBytes::ZEROED; MAX_CPUS];

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
            let str_pa = unsafe { VirtAddr::new(s.as_ptr() as u64).kvirt_to_phys() };
            let pa = sbi_rt::Physical::new(
                s.bytes().len(),
                str_pa.lower_32_bits() as usize,
                str_pa.upper_32_bits() as usize,
            );
            let _ = sbi_rt::console_write(pa);
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
        if !BSP_ARRIVED {
            // bsp
            BSP_ARRIVED = true;
            bsp_entry(hart_id, fdt_pa);
        } else {
            // ap
            ap_entry(hart_id)
        }
    }
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

unsafe fn bsp_entry(bsp_id: usize, fdt_pa: PhysAddr) -> ! {
    unsafe {
        clear_bss();
    }
    register_basic_power_handlers();
    // set up kernel trap handler
    install_ktrap_handler();

    register_earlycon();
    kinfoln!("anemone kernel booting...");
    kinfoln!("bsp id: {}", bsp_id);

    let fdt_va = sv39::Sv39KernelLayout::phys_to_dm(fdt_pa);

    unsafe {
        // needed by percpu initialization.
        let ncpus = early_scan_cpu_count(fdt_va);
        super::cpu::set_ncpus(ncpus);

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

        wake_up_aps(bsp_id);
        sync_with_counter("boot", &BOOT_SYNC_COUNTER);

        // register drivers to bus types
        driver::init();

        unflatten_device_tree(fdt_va);
        parse_bootargs();
        machine_init();
        of_platform_discovery();

        enable_local_irq();
        bsp_pre_kernel_main();

        sync_with_counter("init", &INIT_SYNC_COUNTER);
        sync_with_counter("finish", &FINISH_SYNC_COUNTER);
    }

    kernel_main()
}

unsafe fn wake_up_aps(bsp_id: usize) {
    unsafe {
        for ap_id in 0..CpuArch::ncpus() {
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

static BOOT_SYNC_COUNTER: AtomicUsize = AtomicUsize::new(0);
static INIT_SYNC_COUNTER: AtomicUsize = AtomicUsize::new(0);
static FINISH_SYNC_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Synchronize all CPUs to ensure they have all executed the code up to this
/// point.
#[inline(never)]
unsafe fn sync_with_counter(name: &str, counter: &'static AtomicUsize) {
    let ncpus = CpuArch::ncpus();
    _ = counter.fetch_add(1, Ordering::SeqCst);
    knoticeln!("{} sync: +1", name);
    while counter.load(Ordering::SeqCst) < ncpus {
        core::hint::spin_loop();
    }
}

unsafe fn ap_entry(ap_id: usize) -> ! {
    unsafe {
        install_ktrap_handler();
        mm::percpu::ap_init(ap_id);
        mm::kptable::activate_kernel_mapping();

        sync_with_counter("boot", &BOOT_SYNC_COUNTER);
        kdebugln!("ap {} booting...", ap_id);

        enable_local_irq();
        // collect previous IPIs sent by bsp before ap starts to run.
        // the main reason for this is to clear IPI buffers of bsp such that it can send
        // IPIs to other APs again.
        riscv::register::sip::set_ssoft();

        // synchronize with BSP
        sync_with_counter("init", &INIT_SYNC_COUNTER);

        sync_with_counter("finish", &FINISH_SYNC_COUNTER);
        kinfoln!("ap {} running...", ap_id);
    }
    kernel_main()
}
