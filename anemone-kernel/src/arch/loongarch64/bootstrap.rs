//! Bootstrap for Loongarch64 architecture.

use core::arch::naked_asm;

use la_insc::{
    reg::{
        csr::{CR_CPUID, CR_CRMD, CR_DMW0, CR_DMW1, CR_PGDH, CR_PGDL, CR_PRMD, CR_PWCH, CR_PWCL},
        dmw::Dmw,
        pwc::{PteWidth, Pwch, Pwcl},
    },
    utils::{mem::MemAccessType, privl::PrivilegeFlags},
};

use crate::{
    arch::{
        clear_bss,
        loongarch64::mm::paging::{LA64PageDirectory, create_bootstrap_ptable},
    },
    mm::layout::KernelLayoutTrait,
    prelude::*,
    utils::align::{AlignedBytes, PhantomAligned4096, PhantomAligned16384},
};

#[unsafe(no_mangle)]
#[unsafe(link_section = ".bss.stack0")]
#[used]
static mut STACK0: [AlignedBytes<
    PhantomAligned4096,
    [u8; (1 << 10) as usize * 1024],
>; MAX_CPUS] = [AlignedBytes::ZEROED; MAX_CPUS];

/// Initial user space
const BOOT_DMW0: Dmw = Dmw::new(
    PrivilegeFlags::PLV0,
    MemAccessType::Cache,
    Dmw::vseg_from_addr(0),
);

/// DM space
const BOOT_DMW1: Dmw = Dmw::new(
    PrivilegeFlags::PLV0,
    MemAccessType::Cache,
    Dmw::vseg_from_addr(KernelLayout::DIRECT_MAPPING_ADDR),
);

const PWCL: Pwcl = Pwcl::new(
    PagingArch::PAGE_SIZE_BITS as u8,
    PagingArch::PGDIR_IDX_BITS as u8,
    (PagingArch::PAGE_SIZE_BITS + PagingArch::PGDIR_IDX_BITS) as u8,
    PagingArch::PGDIR_IDX_BITS as u8,
    (PagingArch::PAGE_SIZE_BITS + 2 * PagingArch::PGDIR_IDX_BITS) as u8,
    PagingArch::PGDIR_IDX_BITS as u8,
    PteWidth::WIDTH_64,
);

const PWCH: Pwch = Pwch::new(
    0,
    0,
    0,
    0,
    true,
);

static BOOTSTRAP_PTABLE: LA64PageDirectory = create_bootstrap_ptable();

#[used]
static __NUN_KEEPER: unsafe extern "C" fn() -> ! = __nun;

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

            li.w    $t0, 0x18   // IE=0, PLV=0, DA=0, PG=1
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
            sub.d       $t0, $t0, $t2
            csrwr   $t0, {cr_pgdh}
            la.global   $t0, {bootstrap_ptable}
            sub.d       $t0, $t0, $t2
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
            csrrd       $a0, {cr_cpuid} // arg0: hart_id
            la.global   $t0, {rusty_nun}
            or          $t0, $t0, $t2
            jirl        $zero,$t0,0

        ",
        boot_dmw0 = const BOOT_DMW0.to_u64(),
        boot_dmw1 = const BOOT_DMW1.to_u64(),
        cr_dmw0 = const CR_DMW0,
        cr_dmw1 = const CR_DMW1,
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
    )
}

#[unsafe(no_mangle)]
extern "C" fn rusty_nun(hart_id: usize) -> ! {
    #[unsafe(link_section = ".bss.nonzero_init")]
    static mut BSP_ARRIVED: bool = false;
    unsafe {
        if !BSP_ARRIVED {
            BSP_ARRIVED = true;
            bsp_entry(hart_id, 0x0)
        } else {
            // ap
            ap_entry(hart_id)
        }
    }
}

#[cfg(debug_assertions)]
pub fn register_debugcon() {
    use crate::debug::printk::{Console, ConsoleFlags, register_console};

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
        Box::new(debug_con),
        ConsoleFlags::EARLY | ConsoleFlags::REPLAY,
    );
}

unsafe fn bsp_entry(hart_id: usize, dtb_pa: usize) -> ! {
    unsafe {
        clear_bss();
    }
    register_debugcon();
    kdebugln!("Test..");
    // bsp
    loop {}
}

unsafe fn ap_entry(hart_id: usize) -> ! {
    loop {}
}
