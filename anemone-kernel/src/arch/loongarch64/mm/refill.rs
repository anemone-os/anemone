use core::arch::naked_asm;

use la_insc::reg::csr::{CR_PGD, CR_TLBRSAVE};

use crate::{arch::loongarch64::mm::paging::LA64PteFlags, prelude::*};

/// TLB refill handler
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __tlb_rfill() -> ! {
    naked_asm!(
        "
            .align 12
            csrwr $t0, {tlbrsave} 
            csrrd $t0, {pgd} 
            lddir $t0, $t0, 2
            # bstrins.d   $t0, $zero, 0, 0

            lddir $t0, $t0, 1
            # bstrins.d   $t0, $zero, 0, 0
            
            ldpte $t0, 0
            ldpte $t0, 1

            tlbfill
            csrrd $t0, {tlbrsave}
            #jr $ra
            ertn
        ",
        tlbrsave = const CR_TLBRSAVE,
        pgd = const CR_PGD,
    );
}
