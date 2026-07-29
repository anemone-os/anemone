//! Decoder for scalar integer load/store instructions handled in software.

use crate::prelude::*;

const REG2I12_OPCODE_SHIFT: u32 = 22;
const REG2I14_OPCODE_SHIFT: u32 = 24;
const REG3_OPCODE_SHIFT: u32 = 15;
const RD_MASK: u32 = 0x1f;

/// Width of a decoded memory transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AccessWidth {
    Half,
    Word,
    Double,
}

impl AccessWidth {
    pub(super) const fn bytes(self) -> usize {
        match self {
            Self::Half => 2,
            Self::Word => 4,
            Self::Double => 8,
        }
    }
}

/// Memory effect of a decoded instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AccessKind {
    SignedLoad,
    UnsignedLoad,
    Store,
}

impl AccessKind {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::SignedLoad => "signed-load",
            Self::UnsignedLoad => "unsigned-load",
            Self::Store => "store",
        }
    }
}

/// A decoded memory effect whose address comes from CSR.BADV.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DecodedAccess {
    address: VirtAddr,
    width: AccessWidth,
    kind: AccessKind,
    register: u8,
}

impl DecodedAccess {
    const fn new(
        address: VirtAddr,
        width: AccessWidth,
        kind: AccessKind,
        register: u8,
    ) -> Self {
        Self {
            address,
            width,
            kind,
            register,
        }
    }

    pub(super) const fn address(self) -> VirtAddr {
        self.address
    }

    pub(super) const fn width(self) -> AccessWidth {
        self.width
    }

    pub(super) const fn kind(self) -> AccessKind {
        self.kind
    }

    pub(super) const fn register(self) -> u8 {
        self.register
    }

    pub(super) const fn length(self) -> usize {
        self.width.bytes()
    }
}

/// The instruction is outside the supported scalar integer subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct UnsupportedInstruction;

/// Decode a supported scalar integer load/store at the supplied fault address.
pub(super) fn decode(
    instruction: u32,
    address: VirtAddr,
) -> Result<DecodedAccess, UnsupportedInstruction> {
    let register = (instruction & RD_MASK) as u8;

    let decoded = match instruction >> REG2I12_OPCODE_SHIFT {
        0xa1 => Some((AccessWidth::Half, AccessKind::SignedLoad)),  // ld.h
        0xa2 => Some((AccessWidth::Word, AccessKind::SignedLoad)),  // ld.w
        0xa3 => Some((AccessWidth::Double, AccessKind::SignedLoad)), // ld.d
        0xa5 => Some((AccessWidth::Half, AccessKind::Store)),       // st.h
        0xa6 => Some((AccessWidth::Word, AccessKind::Store)),       // st.w
        0xa7 => Some((AccessWidth::Double, AccessKind::Store)),     // st.d
        0xa9 => Some((AccessWidth::Half, AccessKind::UnsignedLoad)), // ld.hu
        0xaa => Some((AccessWidth::Word, AccessKind::UnsignedLoad)), // ld.wu
        _ => None,
    };
    if let Some((width, kind)) = decoded {
        return Ok(DecodedAccess::new(address, width, kind, register));
    }

    let decoded = match instruction >> REG2I14_OPCODE_SHIFT {
        0x24 => Some((AccessWidth::Word, AccessKind::SignedLoad)), // ldptr.w
        0x25 => Some((AccessWidth::Word, AccessKind::Store)),      // stptr.w
        0x26 => Some((AccessWidth::Double, AccessKind::SignedLoad)), // ldptr.d
        0x27 => Some((AccessWidth::Double, AccessKind::Store)),    // stptr.d
        _ => None,
    };
    if let Some((width, kind)) = decoded {
        return Ok(DecodedAccess::new(address, width, kind, register));
    }

    let decoded = match instruction >> REG3_OPCODE_SHIFT {
        0x7008 => Some((AccessWidth::Half, AccessKind::SignedLoad)), // ldx.h
        0x7010 => Some((AccessWidth::Word, AccessKind::SignedLoad)), // ldx.w
        0x7018 => Some((AccessWidth::Double, AccessKind::SignedLoad)), // ldx.d
        0x7028 => Some((AccessWidth::Half, AccessKind::Store)),      // stx.h
        0x7030 => Some((AccessWidth::Word, AccessKind::Store)),      // stx.w
        0x7038 => Some((AccessWidth::Double, AccessKind::Store)),    // stx.d
        0x7048 => Some((AccessWidth::Half, AccessKind::UnsignedLoad)), // ldx.hu
        0x7050 => Some((AccessWidth::Word, AccessKind::UnsignedLoad)), // ldx.wu
        _ => None,
    };
    decoded
        .map(|(width, kind)| DecodedAccess::new(address, width, kind, register))
        .ok_or(UnsupportedInstruction)
}

#[kunit]
fn decodes_integer_instruction_formats() {
    let address = VirtAddr::new(0x1234_5678);
    let cases = [
        (0xa1 << REG2I12_OPCODE_SHIFT, 2, AccessKind::SignedLoad),
        (0xa2 << REG2I12_OPCODE_SHIFT, 4, AccessKind::SignedLoad),
        (0xa3 << REG2I12_OPCODE_SHIFT, 8, AccessKind::SignedLoad),
        (0xa5 << REG2I12_OPCODE_SHIFT, 2, AccessKind::Store),
        (0xa6 << REG2I12_OPCODE_SHIFT, 4, AccessKind::Store),
        (0xa7 << REG2I12_OPCODE_SHIFT, 8, AccessKind::Store),
        (0xa9 << REG2I12_OPCODE_SHIFT, 2, AccessKind::UnsignedLoad),
        (0xaa << REG2I12_OPCODE_SHIFT, 4, AccessKind::UnsignedLoad),
        (0x24 << REG2I14_OPCODE_SHIFT, 4, AccessKind::SignedLoad),
        (0x25 << REG2I14_OPCODE_SHIFT, 4, AccessKind::Store),
        (0x26 << REG2I14_OPCODE_SHIFT, 8, AccessKind::SignedLoad),
        (0x27 << REG2I14_OPCODE_SHIFT, 8, AccessKind::Store),
        (0x7008 << REG3_OPCODE_SHIFT, 2, AccessKind::SignedLoad),
        (0x7010 << REG3_OPCODE_SHIFT, 4, AccessKind::SignedLoad),
        (0x7018 << REG3_OPCODE_SHIFT, 8, AccessKind::SignedLoad),
        (0x7028 << REG3_OPCODE_SHIFT, 2, AccessKind::Store),
        (0x7030 << REG3_OPCODE_SHIFT, 4, AccessKind::Store),
        (0x7038 << REG3_OPCODE_SHIFT, 8, AccessKind::Store),
        (0x7048 << REG3_OPCODE_SHIFT, 2, AccessKind::UnsignedLoad),
        (0x7050 << REG3_OPCODE_SHIFT, 4, AccessKind::UnsignedLoad),
    ];

    for (instruction, length, kind) in cases {
        let decoded = decode(instruction | 31, address).unwrap();
        assert_eq!(decoded.address(), address);
        assert_eq!(decoded.length(), length);
        assert_eq!(decoded.kind(), kind);
        assert_eq!(decoded.register(), 31);
    }
}

#[kunit]
fn rejects_non_emulated_instructions() {
    let address = VirtAddr::new(0x1234_5678);
    assert!(decode(0x20 << REG2I14_OPCODE_SHIFT, address).is_err()); // ll.w
    assert!(decode(0x70c0 << REG3_OPCODE_SHIFT, address).is_err()); // amswap.w
    assert!(decode(0, address).is_err());
}
