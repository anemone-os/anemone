use crate::{arch, fs::sysfs::entry::StaticEntry, prelude::*};

fn cpu_byteorder() -> String {
    format!("{}\n", arch::cpu_byteorder())
}

pub(super) static CPU_BYTEORDER_ENTRY: StaticEntry =
    StaticEntry::text("cpu_byteorder", cpu_byteorder);
