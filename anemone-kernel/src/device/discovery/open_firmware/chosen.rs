use super::DEVICE_TREE;
use crate::{device::discovery::fwnode::StdoutConfig, prelude::*, sync::mono::MonoOnce};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StdoutPathError {
    InvalidString,
    DevicePath {
        raw: Box<str>,
        source: device_tree::DevicePathError,
    },
}

#[derive(Debug)]
struct OpenFirmwareStdout {
    /// Stable boot-time snapshot resolved from `/chosen/stdout-path`.
    ///
    /// The DeviceTree is immutable, and this selection is initialized once by
    /// the BSP before platform discovery. It is the sole behavioral source for
    /// explicit firmware stdout selection; the options remain device-specific.
    node: device_tree::DeviceNodeHandle,
    options: Option<Box<str>>,
}

/// Initialized exactly once after unflattening the DeviceTree and before any
/// platform device can query its stdout configuration.
static OF_STDOUT: MonoOnce<Option<OpenFirmwareStdout>> = unsafe { MonoOnce::new() };

pub fn of_init_stdout() -> Result<(), StdoutPathError> {
    let device_tree = DEVICE_TREE.get();
    let stdout = if let Some(chosen) = device_tree.handle.find_node_by_full_name_path("/chosen") {
        if let Some(property) = chosen.property("stdout-path") {
            let raw = property
                .value_as_string()
                .ok_or(StdoutPathError::InvalidString)?;
            let resolved = device_tree
                .handle
                .resolve_device_path(raw)
                .map_err(|source| StdoutPathError::DevicePath {
                    raw: Box::from(raw),
                    source,
                })?;
            kinfoln!("stdout-path: {} -> {}", raw, resolved.node().path());
            Some(OpenFirmwareStdout {
                node: resolved.node().handle(),
                options: resolved.options().map(Box::from),
            })
        } else {
            None
        }
    } else {
        None
    };

    OF_STDOUT.init(|slot| {
        slot.write(stdout);
    });
    Ok(())
}

pub(super) fn stdout_config(node: device_tree::DeviceNodeHandle) -> Option<StdoutConfig<'static>> {
    let stdout = OF_STDOUT.get().as_ref()?;
    if stdout.node != node {
        return None;
    }
    Some(StdoutConfig::new(stdout.options.as_deref()))
}
