//! `invtlb` instruction wrapper

/// Make specific tlb items invalid, loongarch `invtlb` instruction wrapper.
pub unsafe fn invtlb(invt_type: InvtlbType) {
    unsafe {
        match invt_type {
            InvtlbType::All => {
                core::arch::asm!("invtlb 0,$r0,$r0");
            },
            InvtlbType::Global => {
                core::arch::asm!("invtlb 2,$r0,$r0");
            },
            InvtlbType::NonGlobal => {
                core::arch::asm!("invtlb 3,$r0,$r0");
            },
            InvtlbType::NonGlobalWithAsid { asid } => {
                core::arch::asm!("invtlb 4,{0},$r0", in(reg) asid);
            },
            InvtlbType::NonGlobalWithAsidAndVaddr { asid, vaddr } => {
                core::arch::asm!("invtlb 5,{0},{1}", in(reg) asid, in(reg) vaddr);
            },
        }
    }
}

/// Type of `invtlb` instruction
#[derive(Debug, Clone, Copy)]
pub enum InvtlbType {
    /// All TLB entries
    All,
    /// Only global TLB entries
    Global,
    /// All non-global TLB entries
    NonGlobal,
    /// Non-global TLB entries with specific ASID
    NonGlobalWithAsid {
        /// ASID of the entries to invalidate
        asid: u16,
    },
    /// Non-global TLB entries with specific ASID and vaddr
    NonGlobalWithAsidAndVaddr {
        /// ASID of the entries to invalidate
        asid: u16,
        /// Vaddr of the entries to invalidate
        vaddr: u64,
    },
}
