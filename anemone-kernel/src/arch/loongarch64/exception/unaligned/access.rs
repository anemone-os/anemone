//! Bytewise user-memory access for emulated LoongArch unaligned instructions.

use crate::{
    prelude::*,
    syscall::user_access::{UserReadPtr, UserWritePtr},
};

use super::decode::{AccessKind, AccessWidth, DecodedAccess};
use crate::arch::loongarch64::exception::trap::LA64TrapFrame;

fn extend_load(bytes: [u8; 8], width: AccessWidth, kind: AccessKind) -> u64 {
    match (width, kind) {
        (AccessWidth::Half, AccessKind::SignedLoad) => {
            i16::from_le_bytes(bytes[..2].try_into().unwrap()) as i64 as u64
        },
        (AccessWidth::Half, AccessKind::UnsignedLoad) => {
            u16::from_le_bytes(bytes[..2].try_into().unwrap()) as u64
        },
        (AccessWidth::Word, AccessKind::SignedLoad) => {
            i32::from_le_bytes(bytes[..4].try_into().unwrap()) as i64 as u64
        },
        (AccessWidth::Word, AccessKind::UnsignedLoad) => {
            u32::from_le_bytes(bytes[..4].try_into().unwrap()) as u64
        },
        (AccessWidth::Double, AccessKind::SignedLoad) => u64::from_le_bytes(bytes),
        (_, AccessKind::Store) | (AccessWidth::Double, AccessKind::UnsignedLoad) => unreachable!(),
    }
}

/// Emulate one decoded access and advance ERA only after its effects succeed.
pub(super) fn emulate_user(
    access: DecodedAccess,
    trapframe: &mut LA64TrapFrame,
    uspace: &mut UserSpace,
) -> Result<(), SysError> {
    assert!(
        IntrArch::local_intr_enabled(),
        "user-memory validation may broadcast a synchronous TLB IPI"
    );
    let length = access.length();

    match access.kind() {
        AccessKind::SignedLoad | AccessKind::UnsignedLoad => {
            let source = UserReadPtr::<[u8]>::try_new(access.address(), length, uspace)?;
            let mut bytes = [0u8; 8];

            // UserPtr validation stays outside this diagnostic IRQ-off window:
            // page-fault completion may synchronously broadcast a TLB IPI. The
            // UserSpace lock keeps the validated mapping stable while the copy
            // and trapframe commit run without hardware-interrupt interleaving.
            with_intr_disabled(|| {
                source.copy_to_slice(&mut bytes[..length]);
                let value = extend_load(bytes, access.width(), access.kind());
                trapframe.write_gpr(access.register(), value);
                trapframe.advance_era_after_emulated_instruction();
            });
        },
        AccessKind::Store => {
            let mut destination =
                UserWritePtr::<[u8]>::try_new(access.address(), length, uspace)?;

            // Keep this paired with the load path until the 2K1000 ramdisk.c
            // stress run confirms or rejects hardware-interrupt interleaving as
            // the corruption source.
            with_intr_disabled(|| {
                let bytes = trapframe.read_gpr(access.register()).to_le_bytes();
                destination.copy_from_slice(&bytes[..length]);
                trapframe.advance_era_after_emulated_instruction();
            });
        },
    }
    Ok(())
}

#[kunit]
fn extends_signed_and_unsigned_loads() {
    let mut bytes = [0u8; 8];
    bytes[..4].copy_from_slice(&0x8000_0001u32.to_le_bytes());
    assert_eq!(
        extend_load(bytes, AccessWidth::Word, AccessKind::SignedLoad),
        0xffff_ffff_8000_0001
    );
    assert_eq!(
        extend_load(bytes, AccessWidth::Word, AccessKind::UnsignedLoad),
        0x8000_0001
    );
}
