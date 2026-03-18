pub unsafe fn invtlb(invt_type: InvtlbType) {
    unsafe {
        match invt_type {
            InvtlbType::All => {
                // Invalidate all TLB entries
                core::arch::asm!("invtlb 0,$r0,$r0");
            },
            InvtlbType::Global => {
                // Invalidate global TLB entries
                core::arch::asm!("invtlb 2,$r0,$r0");
            },
            InvtlbType::NonGlobal => {
                // Invalidate non-global TLB entries
                core::arch::asm!("invtlb 3,$r0,$r0");
            },
            InvtlbType::NonGlobalWithAsid { asid } => {
                // Invalidate non-global TLB entries with ASID
                core::arch::asm!("invtlb 4,{0},$r0", in(reg) asid);
            },
            InvtlbType::NonGlobalWithAsidAndVaddr { asid, vaddr } => {
                // Invalidate non-global TLB entry with ASID and virtual address
                core::arch::asm!("invtlb 5,{0},{1}", in(reg) asid, in(reg) vaddr);
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum InvtlbType {
    All,
    Global,
    NonGlobal,
    NonGlobalWithAsid { asid: u16 },
    NonGlobalWithAsidAndVaddr { asid: u16, vaddr: u64 },
}
