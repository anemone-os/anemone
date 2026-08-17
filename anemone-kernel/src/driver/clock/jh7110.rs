//! StarFive JH7110 clock provider used by VisionFive 2.
//!
//! The Linux provider exposes SYS, STG and AON clocks through one DT node on
//! this board.  The DT IDs are therefore the merged global namespace below,
//! even though each ID is written to a different CRG window.

use crate::{
    device::{
        clock_controller::{ClockController, ClockSpecifier},
        discovery::open_firmware::OpenFirmwareNode,
    },
    mm::remap::{IoRemap, ioremap},
    prelude::*,
};

const SYS: usize = 0;
const STG: usize = 1;
const AON: usize = 2;
const WINDOW_COUNT: usize = 3;

const CLOCK_ENABLE: u32 = 1 << 31;

// These masks are generated from the JH7110 Linux clock descriptors.  A bit
// is set only for GATE, GDIV or GMUX clocks; pure divider/mux/inverter clocks
// must not be treated as gates because bit 31 is not part of their register
// value.
const SYS_GATE_MASK: [u32; 6] = [
    0x7fc8_1600,
    0xf73f_d0ff,
    0x7dff_dfdc,
    0xffff_fa47,
    0xffff_ffff,
    0x07c1_8307,
];
const STG_GATE_MASK: [u32; 1] = [0x1fff_ff7f];
const AON_GATE_MASK: [u32; 1] = [0x0000_262c];

#[derive(Debug, Clone, Copy)]
struct ClockLayout {
    global_base: u32,
    count: u32,
    window: usize,
    gate_mask: &'static [u32],
}

const CLOCK_LAYOUTS: [ClockLayout; WINDOW_COUNT] = [
    ClockLayout {
        global_base: 0,
        count: 190,
        window: SYS,
        gate_mask: &SYS_GATE_MASK,
    },
    ClockLayout {
        global_base: 190,
        count: 29,
        window: STG,
        gate_mask: &STG_GATE_MASK,
    },
    ClockLayout {
        global_base: 219,
        count: 14,
        window: AON,
        gate_mask: &AON_GATE_MASK,
    },
];

#[derive(Debug)]
pub struct Jh7110Crg {
    windows: Vec<IoRemap>,
    lock: SpinLock<()>,
}

static SHARED_CRG: Lazy<RwLock<Option<Arc<Jh7110Crg>>>> = Lazy::new(|| RwLock::new(None));

impl Jh7110Crg {
    /// Build a provider from DT resources. `reg-names`, rather than physical
    /// addresses or DT ordering, selects each CRG window.
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
                "sys" => SYS,
                "stg" => STG,
                "aon" => AON,
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
            let required = usize::try_from(CLOCK_LAYOUTS[slot].count)
                .ok()
                .and_then(|count| count.checked_mul(4))
                .ok_or(SysError::InvalidArgument)?;
            if len < required {
                return Err(SysError::MissingResource);
            }
            windows.push(unsafe { ioremap(base, len) }?);
        }

        Ok(Self {
            windows,
            lock: SpinLock::new(()),
        })
    }

    fn ptr(window: &IoRemap, offset: usize) -> *mut u32 {
        assert!(
            offset
                .checked_add(core::mem::size_of::<u32>())
                .is_some_and(|end| end <= window.size() as usize)
        );
        unsafe { window.as_ptr().as_ptr().cast::<u8>().add(offset).cast() }
    }

    pub fn read(&self, window: usize, offset: usize) -> u32 {
        let window = &self.windows[window];
        unsafe { core::ptr::read_volatile(Self::ptr(window, offset)) }
    }

    pub fn write(&self, window: usize, offset: usize, value: u32) {
        let window = &self.windows[window];
        unsafe { core::ptr::write_volatile(Self::ptr(window, offset), value) }
    }

    pub fn modify(&self, window: usize, offset: usize, f: impl FnOnce(u32) -> u32) {
        let _guard = self.lock.lock_irqsave();
        let value = self.read(window, offset);
        self.write(window, offset, f(value));
    }

    pub fn matches_window(&self, window: usize, base: PhysAddr, len: usize) -> bool {
        self.windows
            .get(window)
            .is_some_and(|mapped| mapped.phys_base() == base && mapped.size() == len as u64)
    }

    pub fn phys_base(&self, window: usize) -> PhysAddr {
        self.windows[window].phys_base()
    }
}

#[derive(Debug)]
pub struct Jh7110ClockController {
    crg: Arc<Jh7110Crg>,
}

impl Jh7110ClockController {
    pub fn from_of_node(node: &OpenFirmwareNode) -> Result<Self, SysError> {
        let crg = Arc::new(Jh7110Crg::from_of_node(node)?);
        let mut shared = SHARED_CRG.write_irqsave();
        if shared.is_some() {
            return Err(SysError::AlreadyExists);
        }
        *shared = Some(crg.clone());
        Ok(Self { crg })
    }

    pub fn shared_crg() -> Option<Arc<Jh7110Crg>> {
        SHARED_CRG.read_irqsave().clone()
    }

    fn layout_for(id: u32) -> Option<(ClockLayout, u32)> {
        CLOCK_LAYOUTS.iter().copied().find_map(|layout| {
            let end = layout.global_base.checked_add(layout.count)?;
            (id >= layout.global_base && id < end).then_some((layout, id - layout.global_base))
        })
    }

    fn gate_capable(layout: ClockLayout, local_id: u32) -> bool {
        let word = usize::try_from(local_id / 32).ok();
        let bit = local_id % 32;
        word.and_then(|word| layout.gate_mask.get(word))
            .is_some_and(|mask| mask & (1 << bit) != 0)
    }
}

impl ClockController for Jh7110ClockController {
    fn enable(&self, specifier: ClockSpecifier<'_>) -> Result<(), SysError> {
        let cells = specifier.cells();
        if cells.len() != 1 {
            kerrln!("jh7110-clock: expected one clock cell, got {}", cells.len());
            return Err(SysError::InvalidArgument);
        }
        let id = cells[0];
        let (layout, local_id) = match Self::layout_for(id) {
            Some(layout) => layout,
            None => {
                kerrln!("jh7110-clock: unsupported clock id {:#x}", id);
                return Err(SysError::InvalidArgument);
            },
        };
        if !Self::gate_capable(layout, local_id) {
            // Linux clk_prepare_enable() succeeds for a divider/mux that has
            // no enable op; enabling its parent is the provider's job. This
            // first slice keeps firmware-selected rate/mux and treats the
            // leaf as an already-on handoff rather than writing bit 31.
            kdebugln!(
                "jh7110-clock: clock id {:#x} has no gate bit; preserving firmware handoff",
                id
            );
            return Ok(());
        }

        let offset = usize::try_from(local_id)
            .ok()
            .and_then(|local_id| local_id.checked_mul(4))
            .ok_or(SysError::InvalidArgument)?;
        self.crg
            .modify(layout.window, offset, |value| value | CLOCK_ENABLE);
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

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn jh7110_clock_namespace_uses_merged_dt_ids() {
        assert_eq!(Jh7110ClockController::layout_for(108).unwrap().1, 108);
        assert_eq!(Jh7110ClockController::layout_for(190).unwrap().1, 0);
        assert_eq!(Jh7110ClockController::layout_for(224).unwrap().1, 5);
        assert!(Jh7110ClockController::gate_capable(CLOCK_LAYOUTS[SYS], 108));
        assert!(Jh7110ClockController::gate_capable(CLOCK_LAYOUTS[AON], 5));
        assert!(!Jh7110ClockController::gate_capable(CLOCK_LAYOUTS[AON], 4));
        assert!(Jh7110ClockController::layout_for(233).is_none());
    }
}
