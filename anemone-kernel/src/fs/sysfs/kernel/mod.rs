use super::entry::StaticEntry;

use self::{address_bits::ADDRESS_BITS_ENTRY, cpu_byteorder::CPU_BYTEORDER_ENTRY};

static KERNEL_CHILDREN: &[&StaticEntry] = &[&ADDRESS_BITS_ENTRY, &CPU_BYTEORDER_ENTRY];

pub(super) static KERNEL_ENTRY: StaticEntry = StaticEntry::dir("kernel", KERNEL_CHILDREN);

mod address_bits;
mod cpu_byteorder;
