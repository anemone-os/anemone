//! StarFive JH7110 reset provider used by VisionFive 2.

use crate::{
    device::{
        discovery::open_firmware::OpenFirmwareNode,
        reset::{ResetController, ResetSpecifier},
    },
    mm::remap::{IoRemap, ioremap},
    prelude::*,
};

const SYS: usize = 0;
const STG: usize = 1;
const AON: usize = 2;
const ISP: usize = 3;
const VOUT: usize = 4;
const WINDOW_COUNT: usize = 5;

#[derive(Debug, Clone, Copy)]
struct ResetLayout {
    global_base: u32,
    count: u32,
    assert_offset: usize,
    status_offset: usize,
    window: usize,
}

const RESET_LAYOUTS: [ResetLayout; WINDOW_COUNT] = [
    ResetLayout {
        global_base: 0,
        count: 126,
        assert_offset: 0x2f8,
        status_offset: 0x308,
        window: SYS,
    },
    ResetLayout {
        global_base: 128,
        count: 23,
        assert_offset: 0x74,
        status_offset: 0x78,
        window: STG,
    },
    ResetLayout {
        global_base: 160,
        count: 8,
        assert_offset: 0x38,
        status_offset: 0x3c,
        window: AON,
    },
    ResetLayout {
        global_base: 192,
        count: 12,
        assert_offset: 0x38,
        status_offset: 0x3c,
        window: ISP,
    },
    ResetLayout {
        global_base: 224,
        count: 12,
        assert_offset: 0x48,
        status_offset: 0x4c,
        window: VOUT,
    },
];

#[derive(Debug)]
pub struct Jh7110ResetController {
    windows: Vec<IoRemap>,
    lock: SpinLock<()>,
}

impl Jh7110ResetController {
    /// Build a provider from the DT resources.  `reg-names`, rather than
    /// physical addresses or DT ordering, selects each CRG window.
    pub fn from_of_node(node: &OpenFirmwareNode) -> Result<Self, SysError> {
        let names = parse_names(
            node.node()
                .property("reg-names")
                .ok_or(SysError::MissingResource)?
                .value_as_bytes(),
        )?;
        let mut name_to_index = [None; WINDOW_COUNT];
        for (index, name) in names.iter().enumerate() {
            let slot = match *name {
                "syscrg" => SYS,
                "stgcrg" => STG,
                "aoncrg" => AON,
                "ispcrg" => ISP,
                "voutcrg" => VOUT,
                _ => return Err(SysError::DriverIncompatible),
            };
            if name_to_index[slot].replace(index).is_some() {
                return Err(SysError::DriverIncompatible);
            }
        }
        if names.len() != WINDOW_COUNT || name_to_index.iter().any(Option::is_none) {
            return Err(SysError::DriverIncompatible);
        }

        let mut resources = Vec::new();
        let reg = node.node().reg().ok_or(SysError::MissingResource)?;
        for (base, len) in reg.iter() {
            let len = usize::try_from(len).map_err(|_| SysError::InvalidArgument)?;
            if len == 0 {
                return Err(SysError::InvalidArgument);
            }
            resources.push((PhysAddr::new(base), len));
        }
        if resources.len() != WINDOW_COUNT {
            return Err(SysError::MissingResource);
        }

        let mut windows = Vec::new();
        for slot in 0..WINDOW_COUNT {
            let resource_index = name_to_index[slot].ok_or(SysError::DriverIncompatible)?;
            let (base, len) = resources[resource_index];
            let remap = unsafe { ioremap(base, len) }?;
            let layout = RESET_LAYOUTS[slot];
            let required = layout
                .status_offset
                .checked_add(((layout.count - 1) / 32) as usize * 4 + 4)
                .ok_or(SysError::InvalidArgument)?;
            if len < required {
                return Err(SysError::MissingResource);
            }
            windows.push(remap);
        }
        Ok(Self {
            windows,
            lock: SpinLock::new(()),
        })
    }

    fn layout_for(id: u32) -> Option<(ResetLayout, u32)> {
        RESET_LAYOUTS.iter().copied().find_map(|layout| {
            let end = layout.global_base.checked_add(layout.count)?;
            (id >= layout.global_base && id < end).then_some((layout, id - layout.global_base))
        })
    }

    fn ptr(window: &IoRemap, offset: usize) -> *mut u32 {
        assert!(
            offset
                .checked_add(4)
                .is_some_and(|end| end <= window.size() as usize)
        );
        unsafe { window.as_ptr().as_ptr().cast::<u8>().add(offset).cast() }
    }

    fn read(window: &IoRemap, offset: usize) -> u32 {
        unsafe { core::ptr::read_volatile(Self::ptr(window, offset)) }
    }

    fn write(window: &IoRemap, offset: usize, value: u32) {
        unsafe { core::ptr::write_volatile(Self::ptr(window, offset), value) }
    }

    fn wait_status(window: &IoRemap, offset: usize, mask: u32, asserted: bool) -> bool {
        // The JH7110 CRG status bit is clear while reset is asserted and set
        // after deassertion.  Bound the poll so a gated clock cannot hang boot.
        for _ in 0..1000 {
            let value = Self::read(window, offset);
            if (value & mask != 0) == !asserted {
                return true;
            }
            core::hint::spin_loop();
        }
        false
    }
}

impl ResetController for Jh7110ResetController {
    fn reset(&self, specifier: ResetSpecifier<'_>) -> Result<(), SysError> {
        let cells = specifier.cells();
        if cells.len() != 1 {
            kerrln!("jh7110-reset: expected one reset cell, got {}", cells.len());
            return Err(SysError::InvalidArgument);
        }
        let id = cells[0];
        let (layout, local_id) = match Self::layout_for(id) {
            Some(layout) => layout,
            None => {
                kerrln!("jh7110-reset: unsupported reset id {:#x}", id);
                return Err(SysError::InvalidArgument);
            },
        };
        let word = (local_id / 32) as usize;
        let bit = 1u32 << (local_id % 32);
        let assert_offset = layout.assert_offset + word * 4;
        let status_offset = layout.status_offset + word * 4;
        let window = &self.windows[layout.window];
        let _guard = self.lock.lock_irqsave();

        let value = Self::read(window, assert_offset);
        Self::write(window, assert_offset, value | bit);
        if !Self::wait_status(window, status_offset, bit, true) {
            kerrln!(
                "jh7110-reset: reset id {:#x} assert timeout base={:#x}",
                id,
                window.phys_base().get()
            );
            return Err(SysError::Timeout);
        }

        let value = Self::read(window, assert_offset);
        Self::write(window, assert_offset, value & !bit);
        if !Self::wait_status(window, status_offset, bit, false) {
            kerrln!(
                "jh7110-reset: reset id {:#x} deassert timeout base={:#x}",
                id,
                window.phys_base().get()
            );
            return Err(SysError::Timeout);
        }
        Ok(())
    }
}

fn parse_names(raw: &[u8]) -> Result<Vec<&str>, SysError> {
    if raw.last() != Some(&0) {
        return Err(SysError::DriverIncompatible);
    }
    let mut names = Vec::new();
    let mut offset = 0;
    while offset < raw.len() {
        let end = offset
            + raw[offset..]
                .iter()
                .position(|byte| *byte == 0)
                .ok_or(SysError::DriverIncompatible)?;
        if end == offset {
            return Err(SysError::DriverIncompatible);
        }
        names.push(
            core::str::from_utf8(&raw[offset..end]).map_err(|_| SysError::DriverIncompatible)?,
        );
        offset = end + 1;
    }
    Ok(names)
}
