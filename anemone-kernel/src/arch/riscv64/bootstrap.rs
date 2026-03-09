use core::{arch::naked_asm, sync::atomic::AtomicUsize};

use crate::{
    align_down_power_of_2, align_up_power_of_2,
    arch::{
        clear_bss,
        riscv64::{
            exception::on_enter_kernel,
            mm::{RiscV64PgDir, RiscV64Pte, RiscV64PteFlags, sv39},
        },
    },
    debug::printk::{Console, ConsoleFlags, register_console},
    device::discovery::open_firmware::{
        EarlyMemoryScanner, early_scan_clock_freq, early_scan_cpu_count, early_scan_fdt_size,
        of_platform_discovery, unflatten_device_tree,
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
            for byte in s.bytes() {
                let _ = sbi_rt::console_write_byte(byte);
            }
        }
    }

    register_console(
        Arc::new(SbiEarlyCon),
        ConsoleFlags::EARLY | ConsoleFlags::REPLAY,
    );
}

fn dump_kernel_image_info() {
    use link_symbols::*;
    kinfoln!("kernel image layout:");
    kinfoln!(
        "  .text: {:#x} - {:#x}",
        __stext as *const () as usize,
        __etext as *const () as usize
    );
    kinfoln!(
        "  .rodata: {:#x} - {:#x}",
        __srodata as *const () as usize,
        __erodata as *const () as usize
    );
    kinfoln!(
        "  .data: {:#x} - {:#x}",
        __sdata as *const () as usize,
        __edata as *const () as usize
    );
    kinfoln!(
        "  .bss: {:#x} - {:#x}",
        __sbss as *const () as usize,
        __ebss as *const () as usize
    );
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

unsafe fn bsp_entry(bsp_id: usize, fdt_pa: PhysAddr) -> ! {
    unsafe {
        clear_bss();

        // set up kernel trap handler
        on_enter_kernel();
        riscv::register::sstatus::set_sie();
        // enable ipi
        riscv::register::sie::set_ssoft();
    }
    register_earlycon();

    kinfoln!("anemone kernel booting...");
    kinfoln!("bsp id: {}", bsp_id);

    let fdt_va = sv39::Sv39KernelLayout::phys_to_hhdm(fdt_pa);

    unsafe {
        // needed by percpu initialization.
        let ncpus = early_scan_cpu_count(fdt_va);
        super::cpu::set_ncpus(ncpus);
        // needed by timer initialization.
        let freq_hz = early_scan_clock_freq(fdt_va);
        super::time::set_hw_clock_freq(freq_hz);

        let mut scanner = EarlyMemoryScanner::new(fdt_va);

        // mark fdt as reserved memory so that it won't be allocated by frame allocator.
        let fdt_npages = (early_scan_fdt_size(fdt_va) + PagingArch::PAGE_SIZE_BYTES - 1)
            / PagingArch::PAGE_SIZE_BYTES;
        let fdt_ppn = PhysPageNum::new(fdt_pa.get() >> PagingArch::PAGE_SIZE_BITS);
        scanner.mark_as_reserved(fdt_ppn, fdt_npages as u64, RsvMemFlags::FDT);

        mm::percpu::bsp_init(bsp_id, |npages| scanner.early_alloc_folio(npages as u64));
        kinfoln!("percpu data initialized");

        scanner.commit_to_pmm();
        let mut memmap_pages = 0;
        mm::frame::memmap_init(|npages| {
            memmap_pages += npages;
            kdebugln!("memmap init: allocating {} pages", npages);
            sys_mem_zones()
                .leak(npages)
                .expect("no enough memory to initialize memmap")
        });
        kdebugln!("memmap initialized, total {} pages", memmap_pages);
        mm::frame::pmm_init();
        kinfoln!("physical memory management initialized");

        mm::kpgdir::init_kernel_mapping();
        kinfoln!("kernel mapping initialized");
        mm::kpgdir::activate_kernel_mapping();
        kinfoln!("kernel mapping activated");

        // register drivers to bus types
        driver::init();

        unflatten_device_tree(fdt_va);
        of_platform_discovery();

        // okay, we can wake up APs now.
        {
            for ap_id in 0..CpuArch::ncpus() {
                if ap_id == bsp_id {
                    continue;
                }
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
            sync_all_cpus();
        }
        kinfoln!("bsp {} running...", bsp_id);
    }

    kernel_main(true)
}

/// Synchronize all CPUs to ensure they have all executed the code up to this
/// point.
unsafe fn sync_all_cpus() {
    static SYNC_COUNTER: AtomicUsize = AtomicUsize::new(0);

    let ncpus = CpuArch::ncpus();
    _ = SYNC_COUNTER.fetch_add(1, Ordering::SeqCst);
    while SYNC_COUNTER.load(Ordering::SeqCst) < ncpus {
        core::hint::spin_loop();
    }

    // all right, all CPUs have reached this point. We can safely proceed.
}

unsafe fn ap_entry(ap_id: usize) -> ! {
    unsafe {
        kinfoln!("ap {} is starting up...", ap_id);
        on_enter_kernel();
        mm::percpu::ap_init(ap_id);
        riscv::register::sstatus::set_sie();
        riscv::register::sie::set_ssoft();

        mm::kpgdir::activate_kernel_mapping();

        // synchronize with BSP
        sync_all_cpus();
        kinfoln!("ap {} running...", ap_id);
    }

    kernel_main(false)
}
