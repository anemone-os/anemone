use core::arch::naked_asm;

use la_insc::reg::csr::{CR_PGD, CR_TLBREHI, CR_TLBRELO0, CR_TLBRELO1, CR_TLBRSAVE};

use crate::prelude::{PagingArch, PagingArchTrait};

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
            beqz $t0, 1f

            lddir $t0, $t0, 1
            beqz $t0, 1f
            
            ldpte $t0, 0
            ldpte $t0, 1
            b 2f

        1:
            # A sparse page table may have no intermediate directory yet. Do
            # not interpret its zero entry as physical address zero. Fill a
            # base-page invalid pair so the retry raises PIL/PIS/PIF and lets
            # the regular page-fault path allocate the missing hierarchy.
            csrrd $t0, {tlbrehi}
            bstrins.d $t0, $zero, 5, 0
            ori $t0, $t0, {base_page_size}
            csrwr $t0, {tlbrehi}
            csrwr $zero, {tlbrelo0}
            csrwr $zero, {tlbrelo1}

        2:
            tlbfill
            csrrd $t0, {tlbrsave}
            ertn
        ",
        tlbrsave = const CR_TLBRSAVE,
        pgd = const CR_PGD,
        tlbrehi = const CR_TLBREHI,
        tlbrelo0 = const CR_TLBRELO0,
        tlbrelo1 = const CR_TLBRELO1,
        base_page_size = const PagingArch::PAGE_SIZE_BITS,
    );
}
