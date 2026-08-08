//! Generic clock-controller resource resolution.
//!
//! The clock core owns DT resource parsing and provider lookup. Register
//! layouts, clock-id namespaces, and the meaning of each clock cell remain in
//! the machine/provider owner.

use crate::{
    device::discovery::{
        fwnode::FwNode,
        open_firmware::{get_of_node, of_with_node_by_phandle},
    },
    prelude::*,
};

#[derive(Debug, Clone, Copy)]
pub struct ClockSpecifier<'a> {
    cells: &'a [u32],
}

impl<'a> ClockSpecifier<'a> {
    pub const fn cells(self) -> &'a [u32] {
        self.cells
    }

    pub const fn first(self) -> Option<u32> {
        self.cells.first().copied()
    }
}

/// A machine-specific clock provider. The provider receives only the cells
/// following its DT phandle; it owns the clock-id namespace and MMIO layout.
pub trait ClockController: Send + Sync {
    fn enable(&self, specifier: ClockSpecifier<'_>) -> Result<(), SysError>;
}

pub struct ClockDomain {
    fwnode: Arc<dyn FwNode>,
    controller: Box<dyn ClockController>,
}

impl ClockDomain {
    pub fn new(fwnode: Arc<dyn FwNode>, controller: Box<dyn ClockController>) -> Self {
        Self { fwnode, controller }
    }
}

static CLOCK_DOMAINS: Lazy<RwLock<Vec<Arc<ClockDomain>>>> = Lazy::new(|| RwLock::new(Vec::new()));

pub fn register_clock_controller(domain: ClockDomain) -> Result<(), SysError> {
    let mut domains = CLOCK_DOMAINS.write_irqsave();
    if domains
        .iter()
        .any(|registered| registered.fwnode.as_ref().equals(domain.fwnode.as_ref()))
    {
        kerrln!("clock: controller already registered");
        return Err(SysError::AlreadyExists);
    }
    domains.push(Arc::new(domain));
    kinfoln!("clock: controller registered");
    Ok(())
}

fn find_clock_controller(fwnode: &dyn FwNode) -> Option<Arc<ClockDomain>> {
    CLOCK_DOMAINS
        .read_irqsave()
        .iter()
        .find(|domain| domain.fwnode.as_ref().equals(fwnode))
        .cloned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClockResourceError {
    MissingClocks,
    InvalidClocks,
    MissingNames,
    InvalidNames,
    NameCountMismatch,
    DuplicateName,
    NameNotFound,
    MissingProvider,
    MissingClockCells,
    InvalidClockCells,
    ControllerNotRegistered,
}

struct ClockEntry {
    provider: Arc<dyn FwNode>,
    cells: Vec<u32>,
}

fn parse_clock_entries(raw: &[u8]) -> Result<Vec<ClockEntry>, ClockResourceError> {
    if raw.is_empty() || !raw.len().is_multiple_of(core::mem::size_of::<u32>()) {
        return Err(ClockResourceError::InvalidClocks);
    }

    let cells: Vec<u32> = raw
        .chunks_exact(core::mem::size_of::<u32>())
        .map(|cell| u32::from_be_bytes(cell.try_into().unwrap()))
        .collect();
    let mut entries = Vec::new();
    let mut offset = 0;
    while offset < cells.len() {
        let phandle = cells[offset];
        offset += 1;
        let provider = of_with_node_by_phandle(phandle, |node| get_of_node(node.handle()))
            .ok()
            .ok_or(ClockResourceError::MissingProvider)?;
        let cell_count = provider
            .prop_read_u32("#clock-cells")
            .ok_or(ClockResourceError::MissingClockCells)?;
        let cell_count =
            usize::try_from(cell_count).map_err(|_| ClockResourceError::InvalidClockCells)?;
        let end = offset
            .checked_add(cell_count)
            .ok_or(ClockResourceError::InvalidClocks)?;
        if end > cells.len() {
            return Err(ClockResourceError::InvalidClocks);
        }
        entries.push(ClockEntry {
            provider,
            cells: cells[offset..end].to_vec(),
        });
        offset = end;
    }
    Ok(entries)
}

fn select_clock_name(
    raw: &[u8],
    expected_count: usize,
    requested: &str,
) -> Result<usize, ClockResourceError> {
    if raw.last() != Some(&0) {
        return Err(ClockResourceError::InvalidNames);
    }
    let mut offset = 0;
    let mut count = 0;
    let mut selected = None;
    while offset < raw.len() {
        let relative_end = raw[offset..]
            .iter()
            .position(|byte| *byte == 0)
            .ok_or(ClockResourceError::InvalidNames)?;
        if relative_end == 0 {
            return Err(ClockResourceError::InvalidNames);
        }
        let end = offset + relative_end;
        let name = core::str::from_utf8(&raw[offset..end])
            .map_err(|_| ClockResourceError::InvalidNames)?;
        if name == requested {
            if selected.is_some() {
                return Err(ClockResourceError::DuplicateName);
            }
            selected = Some(count);
        }
        count += 1;
        offset = end + 1;
    }
    if count != expected_count {
        return Err(ClockResourceError::NameCountMismatch);
    }
    selected.ok_or(ClockResourceError::NameNotFound)
}

fn resolve_clock_resource(
    fwnode: &dyn FwNode,
    name: &str,
) -> Result<(Arc<ClockDomain>, Vec<u32>), ClockResourceError> {
    let raw = fwnode
        .prop_read_raw("clocks")
        .ok_or(ClockResourceError::MissingClocks)?;
    let entries = parse_clock_entries(raw)?;
    let names = fwnode
        .prop_read_raw("clock-names")
        .ok_or(ClockResourceError::MissingNames)?;
    let index = select_clock_name(names, entries.len(), name)?;
    let entry = entries
        .into_iter()
        .nth(index)
        .ok_or(ClockResourceError::InvalidClocks)?;
    let domain = find_clock_controller(entry.provider.as_ref())
        .ok_or(ClockResourceError::ControllerNotRegistered)?;
    Ok((domain, entry.cells))
}

fn map_clock_error(error: ClockResourceError) -> SysError {
    match error {
        ClockResourceError::MissingClocks
        | ClockResourceError::InvalidClocks
        | ClockResourceError::MissingNames
        | ClockResourceError::InvalidNames
        | ClockResourceError::NameCountMismatch
        | ClockResourceError::DuplicateName
        | ClockResourceError::NameNotFound
        | ClockResourceError::MissingProvider
        | ClockResourceError::MissingClockCells
        | ClockResourceError::InvalidClockCells
        | ClockResourceError::ControllerNotRegistered => SysError::DriverIncompatible,
    }
}

/// Enable one named clock synchronously. Clock lifetime is intentionally
/// monotonic for this first framework slice; consumers do not disable clocks.
pub fn require_clock(dev: &dyn Device, name: &str) -> Result<(), SysError> {
    let fwnode = match dev.fwnode() {
        Some(fwnode) => fwnode,
        None => {
            kerrln!("clock: {} has no firmware node for {}", dev.name(), name);
            return Err(SysError::MissingFwNode);
        },
    };
    let (domain, cells) = match resolve_clock_resource(fwnode.as_ref(), name) {
        Ok(resource) => resource,
        Err(error) => {
            kerrln!(
                "clock: {} failed to resolve clock {}: {:?}",
                dev.name(),
                name,
                error
            );
            return Err(map_clock_error(error));
        },
    };
    if let Err(error) = domain.controller.enable(ClockSpecifier { cells: &cells }) {
        kerrln!(
            "clock: {} clock {} enable failed: {:?}",
            dev.name(),
            name,
            error
        );
        return Err(error);
    }
    kdebugln!("clock: {} clock {} enabled", dev.name(), name);
    Ok(())
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn clock_names_select_unique_entry() {
        assert_eq!(
            select_clock_name(b"ahb\0stmmaceth\0", 2, "stmmaceth").unwrap(),
            1
        );
        assert_eq!(
            select_clock_name(b"ahb\0stmmaceth\0", 2, "missing"),
            Err(ClockResourceError::NameNotFound)
        );
        assert_eq!(
            select_clock_name(b"ahb\0ahb\0", 2, "ahb"),
            Err(ClockResourceError::DuplicateName)
        );
    }

    #[kunit]
    fn clock_names_reject_malformed_or_mismatched_properties() {
        assert_eq!(
            select_clock_name(b"ahb", 1, "ahb"),
            Err(ClockResourceError::InvalidNames)
        );
        assert_eq!(
            select_clock_name(b"ahb\0", 2, "ahb"),
            Err(ClockResourceError::NameCountMismatch)
        );
    }
}
