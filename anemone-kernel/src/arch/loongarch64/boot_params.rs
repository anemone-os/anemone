//! LoongArch boot-parameter decoding used before platform discovery.

use crate::prelude::*;

#[repr(C)]
#[derive(Clone, Copy)]
struct EfiTableHeader {
    signature: u64,
    revision: u32,
    header_size: u32,
    crc32: u32,
    reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EfiSystemTable {
    header: EfiTableHeader,
    firmware_vendor: u64,
    firmware_revision: u32,
    _padding: u32,
    console_in_handle: u64,
    console_in: u64,
    console_out_handle: u64,
    console_out: u64,
    stderr_handle: u64,
    stderr: u64,
    runtime_services: u64,
    boot_services: u64,
    configuration_table_count: u64,
    configuration_tables: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EfiConfigurationTable {
    guid: [u8; 16],
    table: u64,
}

static_assert!(core::mem::size_of::<EfiSystemTable>() == 120);
static_assert!(core::mem::size_of::<EfiConfigurationTable>() == 24);

const EFI_SYSTEM_TABLE_SIGNATURE: u64 = 0x5453_5953_2049_4249;
const DEVICE_TREE_GUID: [u8; 16] = [
    0xd5, 0x21, 0xb6, 0xb1, 0x9c, 0xf1, 0xa5, 0x41, 0x83, 0x0b, 0xd9, 0x15, 0x2c, 0x69, 0xaa, 0xe0,
];

/// Finds the FDT delivered through the LoongArch boot parameters.
///
/// LoongArch passes an EFI-system-table-shaped envelope in `a2`. QEMU direct
/// boot leaves boot/runtime services null and uses only its configuration
/// table to publish boot facts, so this decoder deliberately does not expose
/// an EFI service abstraction.
///
/// # Safety
///
/// `system_table_pa` and every physical pointer reachable from it must follow
/// the LoongArch boot ABI and remain readable through the early direct map.
pub(super) unsafe fn find_fdt(system_table_pa: PhysAddr) -> PhysAddr {
    assert!(
        system_table_pa.get() != 0,
        "LoongArch firmware did not provide a system table"
    );

    let system_table = unsafe { read_phys::<EfiSystemTable>(system_table_pa) };
    assert!(
        system_table.header.signature == EFI_SYSTEM_TABLE_SIGNATURE,
        "LoongArch firmware provided an invalid EFI system-table signature"
    );

    let table_count = usize::try_from(system_table.configuration_table_count)
        .expect("LoongArch EFI configuration-table count does not fit usize");
    assert!(
        table_count == 0 || system_table.configuration_tables != 0,
        "LoongArch EFI configuration-table pointer is null"
    );

    for index in 0..table_count {
        let offset = index
            .checked_mul(core::mem::size_of::<EfiConfigurationTable>())
            .expect("LoongArch EFI configuration-table offset overflowed");
        let entry_pa = system_table
            .configuration_tables
            .checked_add(offset as u64)
            .map(PhysAddr::new)
            .expect("LoongArch EFI configuration-table address overflowed");
        let entry = unsafe { read_phys::<EfiConfigurationTable>(entry_pa) };
        if entry.guid == DEVICE_TREE_GUID {
            assert!(entry.table != 0, "LoongArch firmware provided a null FDT");
            return PhysAddr::new(entry.table);
        }
    }

    panic!("LoongArch firmware system table does not contain a device tree")
}

unsafe fn read_phys<T: Copy>(address: PhysAddr) -> T {
    unsafe { core::ptr::read_unaligned(address.to_hhdm().as_ptr()) }
}
