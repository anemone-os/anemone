use core::{any::Any, fmt::Debug};

use crate::{device::discovery::open_firmware::OpenFirmwareNode, prelude::*};

#[derive(Debug, Clone, Copy)]
pub struct StdoutConfig<'a> {
    options: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InterruptSelector<'a> {
    Index(usize),
    Name(&'a str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InterruptResourceError {
    MissingInterrupts,
    MissingParentCells,
    InvalidCellCount,
    SpecifierLengthMismatch,
    IndexOutOfRange,
    MissingNames,
    InvalidNames,
    NameCountMismatch,
    DuplicateName,
    NameNotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InterruptResource<'a> {
    pub(crate) index: usize,
    pub(crate) specifier: &'a [u8],
}

impl<'a> InterruptResource<'a> {
    pub(crate) const fn index(self) -> usize {
        self.index
    }

    pub(crate) const fn specifier(self) -> &'a [u8] {
        self.specifier
    }
}

impl<'a> StdoutConfig<'a> {
    pub(crate) fn new(options: Option<&'a str>) -> Self {
        Self { options }
    }

    pub fn options(&self) -> Option<&'a str> {
        self.options
    }
}

/// Firmware node, used by drivers to read device information based on their own
/// needs.
///
/// This is actually an abstraction layer over hardware description nodes
/// provided by firmware interfaces such as ACPI and Device Tree, providing a
/// uniform interface for reading properties from firmware nodes, allowing the
/// driver code to be agnostic of the underlying firmware mechanism.
///
/// Refer to https://elixir.bootlin.com/linux/v6.6.32/source/include/linux/fwnode.h for more details.
///
/// **One Important Invariant**: each physical device should have exactly one
/// corresponding firmware node. For multiple firmware nodes referring to the
/// same device, [Arc] is used.
pub trait FwNode: Sync + Send + Any {
    /// Check whether two firmware nodes refer to the same hardware entity.
    ///
    /// This can be implemented by pointer comparison. But it is always
    /// preferred to be implemented by a more semantic way, which is less
    /// error-prone and more robust.
    fn equals(&self, other: &dyn FwNode) -> bool;

    fn prop_read_u32(&self, prop_name: &str) -> Option<u32>;
    fn prop_read_u64(&self, prop_name: &str) -> Option<u64>;
    fn prop_read_str(&self, prop_name: &str) -> Option<String>;
    fn prop_read_present(&self, prop_name: &str) -> bool;
    fn prop_read_raw(&self, prop_name: &str) -> Option<&[u8]>;

    fn interrupt_parent(&self) -> Option<Arc<dyn FwNode>>;
    fn interrupt_info(&self) -> Option<&[u8]>;

    fn stdout_config(&self) -> Option<StdoutConfig<'_>>;

    // TODO: add more methods for retrieving information about the hardware, on
    // demand.
}

/// Resolve one firmware interrupt resource without exposing DT cell parsing to
/// device drivers. The public FwNode trait still exposes the raw property for
/// legacy callers; new callers must use this crate-local resource owner.
pub(crate) fn select_interrupt_resource<'a>(
    fwnode: &'a dyn FwNode,
    selector: InterruptSelector<'_>,
) -> Result<InterruptResource<'a>, InterruptResourceError> {
    let raw = fwnode
        .interrupt_info()
        .ok_or(InterruptResourceError::MissingInterrupts)?;

    let Some(of_parent) = fwnode.interrupt_parent().and_then(|parent| {
        parent
            .as_of_node()
            .map(|node| node.node().interrupt_cells())
    }) else {
        // PCIe interrupt routing already carries one fully translated parent
        // specifier. It has no DT interrupt-names property to select from.
        return match selector {
            InterruptSelector::Index(0) => Ok(InterruptResource {
                index: 0,
                specifier: raw,
            }),
            InterruptSelector::Index(_) => Err(InterruptResourceError::IndexOutOfRange),
            InterruptSelector::Name(_) => Err(InterruptResourceError::MissingNames),
        };
    };

    let cells = of_parent.ok_or(InterruptResourceError::MissingParentCells)?;
    select_interrupt_specifier(
        raw,
        cells,
        fwnode.prop_read_raw("interrupt-names"),
        selector,
    )
}

pub(crate) fn select_interrupt_specifier<'a>(
    raw: &'a [u8],
    cells: u32,
    names_raw: Option<&[u8]>,
    selector: InterruptSelector<'_>,
) -> Result<InterruptResource<'a>, InterruptResourceError> {
    let cells = usize::try_from(cells).map_err(|_| InterruptResourceError::InvalidCellCount)?;
    if cells == 0 {
        return Err(InterruptResourceError::InvalidCellCount);
    }
    let specifier_len = cells
        .checked_mul(core::mem::size_of::<u32>())
        .ok_or(InterruptResourceError::InvalidCellCount)?;
    if raw.len() % specifier_len != 0 {
        return Err(InterruptResourceError::SpecifierLengthMismatch);
    }
    let count = raw.len() / specifier_len;
    let index = match selector {
        InterruptSelector::Index(index) => {
            if index >= count {
                return Err(InterruptResourceError::IndexOutOfRange);
            }
            index
        },
        InterruptSelector::Name(name) => {
            let names = names_raw.ok_or(InterruptResourceError::MissingNames)?;
            resolve_interrupt_name(names, count, name)?
        },
    };
    let start = index * specifier_len;
    Ok(InterruptResource {
        index,
        specifier: &raw[start..start + specifier_len],
    })
}

fn resolve_interrupt_name(
    raw: &[u8],
    expected_count: usize,
    requested: &str,
) -> Result<usize, InterruptResourceError> {
    if raw.last() != Some(&0) {
        return Err(InterruptResourceError::InvalidNames);
    }
    let mut offset = 0;
    let mut count = 0;
    let mut selected = None;
    while offset < raw.len() {
        let relative_end = raw[offset..]
            .iter()
            .position(|byte| *byte == 0)
            .ok_or(InterruptResourceError::InvalidNames)?;
        if relative_end == 0 {
            return Err(InterruptResourceError::InvalidNames);
        }
        let end = offset + relative_end;
        let name = core::str::from_utf8(&raw[offset..end])
            .map_err(|_| InterruptResourceError::InvalidNames)?;
        if name == requested {
            if selected.is_some() {
                return Err(InterruptResourceError::DuplicateName);
            }
            selected = Some(count);
        }
        count += 1;
        offset = end + 1;
    }
    if count != expected_count {
        return Err(InterruptResourceError::NameCountMismatch);
    }
    selected.ok_or(InterruptResourceError::NameNotFound)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    fn cells(values: &[u32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_be_bytes())
            .collect()
    }

    #[kunit]
    fn interrupt_selector_handles_zero_one_and_multiple_specs() {
        let raw = cells(&[1, 2, 3, 4, 5, 6]);
        assert_eq!(
            select_interrupt_specifier(&[], 1, None, InterruptSelector::Index(0)),
            Err(InterruptResourceError::IndexOutOfRange)
        );
        assert_eq!(
            select_interrupt_specifier(&raw[..4], 1, None, InterruptSelector::Index(0))
                .unwrap()
                .specifier(),
            &raw[..4]
        );
        assert_eq!(
            select_interrupt_specifier(&raw, 1, None, InterruptSelector::Index(2))
                .unwrap()
                .specifier(),
            &raw[8..12]
        );
    }

    #[kunit]
    fn interrupt_name_selector_rejects_malformed_and_duplicate_names() {
        let raw = cells(&[7, 8]);
        assert_eq!(
            select_interrupt_specifier(
                &raw,
                1,
                Some(b"macirq\0wake\0"),
                InterruptSelector::Name("macirq"),
            )
            .unwrap()
            .index(),
            0
        );
        assert_eq!(
            select_interrupt_specifier(
                &raw,
                1,
                Some(b"macirq\0macirq\0"),
                InterruptSelector::Name("macirq"),
            ),
            Err(InterruptResourceError::DuplicateName)
        );
        assert_eq!(
            select_interrupt_specifier(&raw, 1, Some(b"macirq"), InterruptSelector::Name("macirq"),),
            Err(InterruptResourceError::InvalidNames)
        );
    }

    #[kunit]
    fn interrupt_selector_checks_cells_names_and_bounds() {
        let raw = cells(&[1, 2, 3]);
        assert_eq!(
            select_interrupt_specifier(&raw, 2, None, InterruptSelector::Index(0)),
            Err(InterruptResourceError::SpecifierLengthMismatch)
        );
        assert_eq!(
            select_interrupt_specifier(
                &raw,
                1,
                Some(b"only\0"),
                InterruptSelector::Name("missing"),
            ),
            Err(InterruptResourceError::NameCountMismatch)
        );
        assert_eq!(
            select_interrupt_specifier(&raw, 1, None, InterruptSelector::Name("macirq")),
            Err(InterruptResourceError::MissingNames)
        );
        assert_eq!(
            select_interrupt_specifier(&raw, 1, None, InterruptSelector::Index(3)),
            Err(InterruptResourceError::IndexOutOfRange)
        );
    }

    #[kunit]
    fn variable_cell_name_and_index_select_the_same_complete_specifier() {
        let raw = cells(&[1, 2, 3, 4]);
        let by_name = select_interrupt_specifier(
            &raw,
            2,
            Some(b"first\0macirq\0"),
            InterruptSelector::Name("macirq"),
        )
        .unwrap();
        let by_index =
            select_interrupt_specifier(&raw, 2, None, InterruptSelector::Index(1)).unwrap();
        assert_eq!(by_name, by_index);
        assert_eq!(by_name.specifier(), &raw[8..16]);
        assert_eq!(
            select_interrupt_specifier(
                &raw,
                2,
                Some(b"first\0second\0"),
                InterruptSelector::Name("macirq"),
            ),
            Err(InterruptResourceError::NameNotFound)
        );
    }
}

impl dyn FwNode {
    // If some additional information is indeed hard to be abstracted by the above
    // methods, we have following methods as a plan B:

    /// Try to downcast this firmware node to an OpenFirmwareNode.
    pub fn as_of_node(&self) -> Option<&OpenFirmwareNode> {
        (self as &dyn Any).downcast_ref::<OpenFirmwareNode>()
    }

    // this is not that unreasonable, since there are only a very limited number of
    // such FwNode implementations.
}

impl Debug for dyn FwNode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("dyn FwNode").finish()
    }
}
