//! Bootstrap for Loongarch64 architecture.

use core::arch::naked_asm;

use la_insc::{
    reg::{
        csr::{CR_CPUID, CR_CRMD, CR_DMW0, CR_DMW1, CR_PRMD},
        dmw::Dmw,
    },
    utils::{mem::MemAccessType, privl::PrivilegeFlags},
};

use crate::{
    arch::clear_bss, mm::layout::KernelLayoutTrait, prelude::*, utils::align::{AlignedBytes, PhantomAligned4096}
};

#[unsafe(no_mangle)]
#[unsafe(link_section = ".bss.stack0")]
static mut STACK0: [AlignedBytes<
    PhantomAligned4096,
    [u8; (1 << KSTACK_SHIFT_KB) as usize * 1024],
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

#[unsafe(no_mangle)]
#[unsafe(naked)]
#[unsafe(link_section = ".text.entry")]
unsafe extern "C" fn __nun(hart_id: usize, dtb_addr: usize) {
    naked_asm!(
        // Enable address mapping
        "
            li.d    $t0, {boot_dmw0}
            csrwr   $t0, {cr_dmw0}
            
            li.d    $t0, {boot_dmw1}
            csrwr   $t0, {cr_dmw1}

            li.w    $t0, 0xb0   // IE=0, PLV=0, DA=0, PG=1
            csrwr   $t0, {cr_crmd}
            csrwr   $t0, {cr_prmd}
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
        cr_prmd = const CR_PRMD,
        cr_cpuid = const CR_CPUID,
        boot_stack = sym STACK0,
        stack_size = const (1 << KSTACK_SHIFT_KB) as usize * 1024,
        k_offset = const 0x9000_0000_0000_0000,
        rusty_nun = sym rusty_nun

    )
}

#[unsafe(no_mangle)]
extern "C" fn rusty_nun(hart_id: usize) -> ! {
    #[unsafe(link_section = ".bss.nonzero_init")]
    static mut BSP_ARRIVED: bool = false;
    unsafe {
        if !BSP_ARRIVED {
            // bsp
            BSP_ARRIVED = true;
            bsp_entry(hart_id, 0x0)
        } else {
            // ap
            ap_entry(hart_id)
        }
    }
}

unsafe fn bsp_entry(hart_id: usize, dtb_pa: usize) -> ! {
    unsafe{
        clear_bss();
    }
    loop{}
}

unsafe fn ap_entry(hart_id: usize) -> ! {
    loop{}
}