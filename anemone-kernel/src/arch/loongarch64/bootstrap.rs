//! Bootstrap for Loongarch64 architecture.

use core::arch::{global_asm, naked_asm};

use la_insc::{
    reg::{
        csr::{
            CR_CPUID, CR_CRMD, CR_DMW0, CR_DMW1, CR_PGDH, CR_PGDL, CR_PRMD, CR_PWCH, CR_PWCL,
            CR_TLBRENTRY,
        },
        dmw::Dmw,
        pwc::{PteWidth, Pwch, Pwcl},
    },
    utils::{mem::MemAccessType, privl::PrivilegeFlags},
};

use crate::{
    arch::{
        clear_bss,
        loongarch64::{
            exception::{enable_local_irq, install_ktrap_handler},
            machine::machine_init,
            mm::{
                BOOT_DMW0, BOOT_DMW1, BOOTSTRAP_PTABLE, PWCH, PWCL,
                paging::{LA64PageDirectory, create_bootstrap_ptable},
                refill::__tlb_rfill,
            },
        },
    },
    device::discovery::open_firmware::{
        EarlyMemoryScanner, early_scan_clock_freq, early_scan_cpu_count, early_scan_fdt_size,
        of_platform_discovery, unflatten_device_tree,
    },
    mm::layout::KernelLayoutTrait,
    prelude::*,
    utils::align::{AlignedBytes, PhantomAligned8, PhantomAligned4096, PhantomAligned16384},
};

#[unsafe(no_mangle)]
#[unsafe(link_section = ".bss.stack0")]
static mut STACK0: [AlignedBytes<
    PhantomAligned4096,
    [u8; (1 << KSTACK_SHIFT_KB) as usize * 1024],
>; MAX_CPUS] = [AlignedBytes::ZEROED; MAX_CPUS];

static DTB_BYTES: &[u8] = include_bytes_aligned_as!(PhantomAligned8, "generated.dtb");

/// # Note
/// Because Loongarch64 is booted without SBI,
///     so the entry point is hard_coded, and the entry point [__nun]
///     will be seen as not-used by the compiler and ignored.
///
/// This static value is used to keep the entry point.
#[used]
static __NUN_KEEPER: unsafe extern "C" fn() -> ! = __nun;

/// Entry point of the kernel
///
/// TODO: Wake up aps.
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
        // Set up stack
        "

            li.d        $t2, {k_offset}
            la.global   $sp, {boot_stack}
            csrrd       $t0, {cr_cpuid}
            li.d        $t1, {stack_size}
            addi.d      $t0, $t0, 0x1
            mul.d       $t0, $t0, $t1
            // add.d       $sp, $sp, $t0
            or          $sp, $sp, $t2
        ",
        // Jump
        "
            la.global   $t0, {tlb_rfill}
            #sub.d       $t0, $t0, $t2
            csrwr       $t0, {tlbr_entry}

            csrrd       $a0, {cr_cpuid} // arg0: hart_id
            la.global   $t0, {rusty_nun}
            or          $t0, $t0, $t2
            jirl        $zero,$t0,0
        ",
        boot_dmw0 = const BOOT_DMW0.to_u64(),
        boot_dmw1 = const BOOT_DMW1.to_u64(),
        cr_dmw0 = const CR_DMW0,
        cr_dmw1 = const CR_DMW1,

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
            bsp_entry(hart_id, VirtAddr::new(DTB_BYTES.as_ptr() as u64))
        } else {
            // ap
            ap_entry(hart_id)
        }
    }
}

#[cfg(debug_assertions)]
pub fn register_debugcon() {
    use crate::{
        device::console::{Console, ConsoleFlags, register_console},
        driver::Ns16550ARegisters,
    };

    let con = unsafe { Ns16550ARegisters::from_raw(0x1fe0_01e0 as *const u8 as *mut u8, 0, 1) };
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
    let debug_con = DebugCon {
        con: SpinLock::new(con),
    };
    register_console(
        Arc::new(debug_con),
        ConsoleFlags::EARLY | ConsoleFlags::REPLAY,
    );
}

unsafe fn bsp_entry(bsp_id: usize, fdt_va: VirtAddr) -> ! {
    unsafe {
        clear_bss();
    }
    install_ktrap_handler();
    kinfoln!("anemone kernel booting...");
    kinfoln!("bsp id : {}", bsp_id);
    register_debugcon();
    unsafe {
        ///
        let ncpus = early_scan_cpu_count(fdt_va);
        super::cpu::set_ncpus(ncpus);

        // needed by timer initialization.
        if let Some(freq_hz) = early_scan_clock_freq(fdt_va) {
            super::time::set_hw_clock_freq(freq_hz);
        } else {
            kwarningln!("failed to scan clock frequency from device tree.");
        }

        let mut scanner = EarlyMemoryScanner::new(fdt_va);

        // mark fdt as reserved memory so that it won't be allocated by frame allocator.
        let fdt_npages = (early_scan_fdt_size(fdt_va) + PagingArch::PAGE_SIZE_BYTES - 1)
            / PagingArch::PAGE_SIZE_BYTES;
        let fdt_ppn = PhysPageNum::new(fdt_va.kvirt_to_phys().get() >> PagingArch::PAGE_SIZE_BITS);
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

        mm::kptable::init_kernel_mapping();
        kinfoln!("kernel mapping initialized");
        mm::kptable::activate_kernel_mapping();
        kinfoln!("kernel mapping activated");

        // register drivers to bus types
        driver::init();

        unflatten_device_tree(fdt_va);
        //parse_bootargs();
        machine_init();
        of_platform_discovery();

        enable_local_irq();
        bsp_pre_kernel_main();
    }
    // bsp
    loop {}
}

unsafe fn ap_entry(hart_id: usize) -> ! {
    loop {}
}
