use crate::prelude::*;

/// Bytes in the register host-to-device FIS defined by Serial ATA.
pub(super) const COMMAND_FIS_BYTES: usize = 20;

/// FIS type values used by the command path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum FisType {
    RegisterHostToDevice = 0x27,
}

/// Byte offsets within a register host-to-device FIS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
enum RegisterFisField {
    Type = 0,
    Flags = 1,
    Command = 2,
    Lba0 = 4,
    Lba1 = 5,
    Lba2 = 6,
    Device = 7,
    Lba3 = 8,
    Lba4 = 9,
    Lba5 = 10,
    SectorCountLow = 12,
    SectorCountHigh = 13,
}

bitflags! {
    /// Control bits in byte one of a register host-to-device FIS.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct RegisterFisFlags: u8 {
        /// Marks the FIS payload as an ATA command rather than control update.
        const COMMAND = 1 << 7;
    }

    /// ATA device-register bits used by LBA-addressed commands.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct AtaDeviceFlags: u8 {
        /// Selects logical-block addressing.
        const LBA = 1 << 6;
    }
}

/// ATA command opcodes emitted by the current AHCI command path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum AtaOpcode {
    ReadDmaExt = 0x25,
    WriteDmaExt = 0x35,
    IdentifyDevice = 0xec,
}

/// Builds an IDENTIFY DEVICE command FIS.
pub(super) fn identify_fis() -> [u8; COMMAND_FIS_BYTES] {
    command_fis(AtaOpcode::IdentifyDevice, 0, 0, false)
}

/// Builds a 48-bit DMA read command FIS.
pub(super) fn read_dma_ext_fis(lba: u64, sectors: u16) -> [u8; COMMAND_FIS_BYTES] {
    command_fis(AtaOpcode::ReadDmaExt, lba, sectors, true)
}

/// Builds a 48-bit DMA write command FIS.
pub(super) fn write_dma_ext_fis(lba: u64, sectors: u16) -> [u8; COMMAND_FIS_BYTES] {
    command_fis(AtaOpcode::WriteDmaExt, lba, sectors, true)
}

/// Encodes a typed ATA opcode and optional LBA48 payload into a register FIS.
fn command_fis(
    opcode: AtaOpcode,
    lba: u64,
    sectors: u16,
    lba_mode: bool,
) -> [u8; COMMAND_FIS_BYTES] {
    assert!(lba < (1u64 << 48));
    if matches!(opcode, AtaOpcode::ReadDmaExt | AtaOpcode::WriteDmaExt) {
        assert_ne!(sectors, 0);
    }

    let mut fis = [0u8; COMMAND_FIS_BYTES];
    fis[RegisterFisField::Type as usize] = FisType::RegisterHostToDevice as u8;
    fis[RegisterFisField::Flags as usize] = RegisterFisFlags::COMMAND.bits();
    fis[RegisterFisField::Command as usize] = opcode as u8;
    fis[RegisterFisField::Lba0 as usize] = lba as u8;
    fis[RegisterFisField::Lba1 as usize] = (lba >> 8) as u8;
    fis[RegisterFisField::Lba2 as usize] = (lba >> 16) as u8;
    fis[RegisterFisField::Device as usize] = if lba_mode {
        AtaDeviceFlags::LBA.bits()
    } else {
        AtaDeviceFlags::empty().bits()
    };
    fis[RegisterFisField::Lba3 as usize] = (lba >> 24) as u8;
    fis[RegisterFisField::Lba4 as usize] = (lba >> 32) as u8;
    fis[RegisterFisField::Lba5 as usize] = (lba >> 40) as u8;
    fis[RegisterFisField::SectorCountLow as usize] = sectors as u8;
    fis[RegisterFisField::SectorCountHigh as usize] = (sectors >> 8) as u8;
    fis
}

#[kunit]
/// Checks byte order and field placement for an LBA48 command.
fn lba48_fis_encodes_address_and_count() {
    let fis = read_dma_ext_fis(0x1234_5678_9abc, 0x3456);
    assert_eq!(fis[0], 0x27);
    assert_eq!(fis[1], 0x80);
    assert_eq!(fis[2], AtaOpcode::ReadDmaExt as u8);
    assert_eq!(&fis[4..=10], &[0xbc, 0x9a, 0x78, 0x40, 0x56, 0x34, 0x12]);
    assert_eq!(&fis[12..=13], &[0x56, 0x34]);
}

#[kunit]
/// Checks that IDENTIFY leaves all address and count fields clear.
fn identify_has_no_data_address_fields() {
    let fis = identify_fis();
    assert_eq!(fis[2], AtaOpcode::IdentifyDevice as u8);
    assert!(fis[4..].iter().all(|byte| *byte == 0));
}
