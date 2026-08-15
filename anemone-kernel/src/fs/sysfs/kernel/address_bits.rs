use crate::{arch, fs::sysfs::entry::StaticEntry, prelude::*};

fn address_bits() -> String {
    format!("{}\n", arch::address_bits())
}

pub(super) static ADDRESS_BITS_ENTRY: StaticEntry = StaticEntry::text("address_bits", address_bits);
