//! Generic reset-controller resource resolution.
//!
//! The reset core owns DT resource parsing and provider lookup.  Register
//! layouts and reset-id namespaces remain in the machine/provider owner.

use crate::{
    device::discovery::{
        fwnode::FwNode,
        open_firmware::{get_of_node, of_with_node_by_phandle},
    },
    prelude::*,
};

#[derive(Debug, Clone, Copy)]
pub struct ResetSpecifier<'a> {
    cells: &'a [u32],
}

impl<'a> ResetSpecifier<'a> {
    pub const fn cells(self) -> &'a [u32] {
        self.cells
    }

    pub const fn first(self) -> Option<u32> {
        self.cells.first().copied()
    }
}

/// A machine-specific reset provider.  The provider receives only the cells
/// following its DT phandle; it owns the meaning of those cells and its MMIO
/// register layout.
pub trait ResetController: Send + Sync {
    fn reset(&self, specifier: ResetSpecifier<'_>) -> Result<(), SysError>;

    /// Leave the selected reset line deasserted without first asserting it.
    fn deassert(&self, specifier: ResetSpecifier<'_>) -> Result<(), SysError>;
}

pub struct ResetDomain {
    fwnode: Arc<dyn FwNode>,
    controller: Box<dyn ResetController>,
}

impl ResetDomain {
    pub fn new(fwnode: Arc<dyn FwNode>, controller: Box<dyn ResetController>) -> Self {
        Self { fwnode, controller }
    }
}

static RESET_DOMAINS: Lazy<RwLock<Vec<Arc<ResetDomain>>>> = Lazy::new(|| RwLock::new(Vec::new()));

pub fn register_reset_controller(domain: ResetDomain) -> Result<(), SysError> {
    let mut domains = RESET_DOMAINS.write_irqsave();
    if domains
        .iter()
        .any(|registered| registered.fwnode.as_ref().equals(domain.fwnode.as_ref()))
    {
        kerrln!("reset: controller already registered");
        return Err(SysError::AlreadyExists);
    }
    domains.push(Arc::new(domain));
    kinfoln!("reset: controller registered");
    Ok(())
}

fn find_reset_controller(fwnode: &dyn FwNode) -> Option<Arc<ResetDomain>> {
    RESET_DOMAINS
        .read_irqsave()
        .iter()
        .find(|domain| domain.fwnode.as_ref().equals(fwnode))
        .cloned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResetResourceError {
    MissingResets,
    InvalidResets,
    MissingNames,
    InvalidNames,
    NameCountMismatch,
    DuplicateName,
    NameNotFound,
    MissingProvider,
    MissingResetCells,
    InvalidResetCells,
    ControllerNotRegistered,
}

struct ResetEntry {
    provider: Arc<dyn FwNode>,
    cells: Vec<u32>,
}

fn parse_reset_entries(raw: &[u8]) -> Result<Vec<ResetEntry>, ResetResourceError> {
    if raw.is_empty() || !raw.len().is_multiple_of(core::mem::size_of::<u32>()) {
        return Err(ResetResourceError::InvalidResets);
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
            .ok_or(ResetResourceError::MissingProvider)?;
        let cell_count = provider
            .prop_read_u32("#reset-cells")
            .ok_or(ResetResourceError::MissingResetCells)?;
        let cell_count =
            usize::try_from(cell_count).map_err(|_| ResetResourceError::InvalidResetCells)?;
        let end = offset
            .checked_add(cell_count)
            .ok_or(ResetResourceError::InvalidResets)?;
        if end > cells.len() {
            return Err(ResetResourceError::InvalidResets);
        }
        entries.push(ResetEntry {
            provider,
            cells: cells[offset..end].to_vec(),
        });
        offset = end;
    }
    Ok(entries)
}

fn select_reset_name(
    raw: &[u8],
    expected_count: usize,
    requested: &str,
) -> Result<usize, ResetResourceError> {
    if raw.last() != Some(&0) {
        return Err(ResetResourceError::InvalidNames);
    }
    let mut offset = 0;
    let mut count = 0;
    let mut selected = None;
    while offset < raw.len() {
        let relative_end = raw[offset..]
            .iter()
            .position(|byte| *byte == 0)
            .ok_or(ResetResourceError::InvalidNames)?;
        if relative_end == 0 {
            return Err(ResetResourceError::InvalidNames);
        }
        let end = offset + relative_end;
        let name = core::str::from_utf8(&raw[offset..end])
            .map_err(|_| ResetResourceError::InvalidNames)?;
        if name == requested {
            if selected.is_some() {
                return Err(ResetResourceError::DuplicateName);
            }
            selected = Some(count);
        }
        count += 1;
        offset = end + 1;
    }
    if count != expected_count {
        return Err(ResetResourceError::NameCountMismatch);
    }
    selected.ok_or(ResetResourceError::NameNotFound)
}

fn resolve_reset_resource(
    fwnode: &dyn FwNode,
    name: &str,
) -> Result<(Arc<ResetDomain>, Vec<u32>), ResetResourceError> {
    let raw = fwnode
        .prop_read_raw("resets")
        .ok_or(ResetResourceError::MissingResets)?;
    let entries = parse_reset_entries(raw)?;
    let names = fwnode
        .prop_read_raw("reset-names")
        .ok_or(ResetResourceError::MissingNames)?;
    let index = select_reset_name(names, entries.len(), name)?;
    let entry = entries
        .into_iter()
        .nth(index)
        .ok_or(ResetResourceError::InvalidResets)?;
    let domain = find_reset_controller(entry.provider.as_ref())
        .ok_or(ResetResourceError::ControllerNotRegistered)?;
    Ok((domain, entry.cells))
}

fn map_reset_error(error: ResetResourceError) -> SysError {
    match error {
        ResetResourceError::MissingResets
        | ResetResourceError::MissingNames
        | ResetResourceError::MissingProvider
        | ResetResourceError::MissingResetCells
        | ResetResourceError::InvalidResetCells
        | ResetResourceError::InvalidResets
        | ResetResourceError::InvalidNames
        | ResetResourceError::NameCountMismatch
        | ResetResourceError::DuplicateName
        | ResetResourceError::NameNotFound
        | ResetResourceError::ControllerNotRegistered => SysError::DriverIncompatible,
    }
}

/// Perform one synchronous assert/deassert transaction for a named reset.
pub fn require_reset(dev: &dyn Device, name: &str) -> Result<(), SysError> {
    let (domain, cells) = resolve_named_reset(dev, name)?;
    if let Err(error) = domain.controller.reset(ResetSpecifier { cells: &cells }) {
        kerrln!(
            "reset: {} reset {} transaction failed: {:?}",
            dev.name(),
            name,
            error
        );
        return Err(error);
    }
    kdebugln!("reset: {} reset {} complete", dev.name(), name);
    Ok(())
}

/// Ensure a named reset is deasserted without issuing an assert pulse.
pub fn require_reset_deasserted(dev: &dyn Device, name: &str) -> Result<(), SysError> {
    let (domain, cells) = resolve_named_reset(dev, name)?;
    if let Err(error) = domain.controller.deassert(ResetSpecifier { cells: &cells }) {
        kerrln!(
            "reset: {} reset {} deassert failed: {:?}",
            dev.name(),
            name,
            error
        );
        return Err(error);
    }
    kdebugln!("reset: {} reset {} deasserted", dev.name(), name);
    Ok(())
}

fn resolve_named_reset(
    dev: &dyn Device,
    name: &str,
) -> Result<(Arc<ResetDomain>, Vec<u32>), SysError> {
    let fwnode = match dev.fwnode() {
        Some(fwnode) => fwnode,
        None => {
            kerrln!("reset: {} has no firmware node for {}", dev.name(), name);
            return Err(SysError::MissingFwNode);
        },
    };
    let (domain, cells) = match resolve_reset_resource(fwnode.as_ref(), name) {
        Ok(resource) => resource,
        Err(error) => {
            kerrln!(
                "reset: {} failed to resolve reset {}: {:?}",
                dev.name(),
                name,
                error
            );
            return Err(map_reset_error(error));
        },
    };
    Ok((domain, cells))
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn reset_names_select_unique_entry() {
        assert_eq!(
            select_reset_name(b"ahb\0stmmaceth\0", 2, "stmmaceth").unwrap(),
            1
        );
        assert_eq!(
            select_reset_name(b"ahb\0stmmaceth\0", 2, "missing"),
            Err(ResetResourceError::NameNotFound)
        );
        assert_eq!(
            select_reset_name(b"ahb\0ahb\0", 2, "ahb"),
            Err(ResetResourceError::DuplicateName)
        );
    }

    #[kunit]
    fn reset_names_reject_malformed_or_mismatched_properties() {
        assert_eq!(
            select_reset_name(b"ahb", 1, "ahb"),
            Err(ResetResourceError::InvalidNames)
        );
        assert_eq!(
            select_reset_name(b"ahb\0", 2, "ahb"),
            Err(ResetResourceError::NameCountMismatch)
        );
    }
}
