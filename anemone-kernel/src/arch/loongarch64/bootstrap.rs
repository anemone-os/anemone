//! Bootstrap for Loongarch64 architecture.

use core::arch::naked_asm;

use la_insc::reg::csr::{
    CR_CPUID, CR_CRMD, CR_DMW0, CR_DMW1, CR_PGDH, CR_PGDL, CR_PWCH, CR_PWCL, CR_TLBRENTRY,
};
use loongArch64::ipi::csr_mail_send;

use crate::{
    arch::{
        clear_bss,
        loongarch64::{
            exception::install_ktrap_handler,
            mm::{BOOT_DMW0, BOOT_DMW1, BOOTSTRAP_PTABLE, PWCH, PWCL, refill::__tlb_rfill},
        },
    },
    device::discovery::open_firmware::{
        EarlyMemoryScanner, early_scan_clock_freq, early_scan_cpu_count,
    },
    mm::{kptable::kmap, layout::KernelLayoutTrait, stack::RawKernelStack},
    prelude::*,
    sync::counter::CpuSync,
};

#[unsafe(no_mangle)]
#[unsafe(link_section = ".bss.stack0")]
static mut STACK0: [RawKernelStack; MAX_CPUS] = [RawKernelStack::ZEROED; MAX_CPUS];

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
            add.d       $sp, $sp, $t0
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
            bsp_setup(hart_id, VirtAddr::new(DTB_BYTES.as_ptr() as u64))
        } else {
            // ap
            ap_setup(hart_id)
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
        add_to_ready(Arc::new(
            Task::new_kernel(
                bsp_kinit as *const (),
                ParameterList::new(&[bsp_id as u64, fdt_va.get()]),
                IntrArch::DISABLED_IRQ_FLAGS,
                TaskFlags::NONE,
            )
            .unwrap_or_else(|e| panic!("failed to create bsp kinit task: {:?}", e)),
        ));
        switch_to_guarded(VirtAddr::new(run_tasks as *const () as u64))
    }
}

static BOOT_SYNC_COUNTER: CpuSync = CpuSync::new("boot");

unsafe fn ap_setup(ap_id: usize) -> ! {
    unsafe {
        BOOT_SYNC_COUNTER.sync_with_counter();
        kdebugln!("anemone kernel booting on ap #{}", ap_id);
        install_ktrap_handler();
        mm::percpu::ap_init(ap_id);
        mm::kptable::activate_kernel_mapping();
        add_to_ready(Arc::new(
            Task::new_kernel(
                ap_kinit as *const (),
                ParameterList::new(&[ap_id as u64]),
                IntrArch::DISABLED_IRQ_FLAGS,
                TaskFlags::NONE,
            )
            .unwrap_or_else(|e| panic!("failed to create ap kinit task: {:?}", e)),
        ));
        switch_to_guarded(VirtAddr::new(run_tasks as *const () as u64));
    }
}

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
            "move  $fp, $zero",
            "jirl  $zero, {entry}, 0 ",
            new_top = in(reg) new_stack_top.get(),
            entry = in(reg) dest_entry.get(),
            options(noreturn),
        )
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
            kmap(Mapping {
                vpn: stack_vpn,
                ppn: cpu_stack_ppn,
                flags: PteFlags::READ | PteFlags::WRITE | PteFlags::GLOBAL,
                npages: KSTACK_PAGES,
                huge_pages: false,
            })
            .expect("failed to map boot stack with guard page");
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
