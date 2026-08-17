//! DWMAC compatible dispatch, frame adaptation, and netdev publication.

mod dwmac1000;
mod dwmac4;
mod frame;

use anemone_net_api::FrameProvider;

use crate::{
    device::{
        bus::platform::PlatformDevice,
        discovery::fwnode::{InterruptResource, InterruptSelector, select_interrupt_resource},
        net::{ReadyNetdev, publish},
    },
    exception::intr::{IrqHandler, IrqSense, request_irq_selected},
    prelude::*,
    utils::{any_opaque::AnyOpaque, identity::AnyIdentity},
};

use frame::{DwmacFrameProvider, DwmacFrameQueue};

pub(super) trait DwmacDeviceControl: Send + Sync {
    fn suppress_device(&self);
    fn start_device(&self);
}

#[derive(Opaque)]
struct DwmacState {
    /// Non-owning shutdown capability. The published provider and registered
    /// IRQ private data retain the concrete context until reset or power-off.
    control: Weak<dyn DwmacDeviceControl>,
}

pub(in crate::driver::net::dwmac) fn compatible_matches(
    device: &PlatformDevice,
    table: &[&str],
) -> bool {
    device
        .compatibles()
        .any(|compatible| table.contains(&compatible))
}

pub(in crate::driver::net::dwmac) fn common_probe_inputs<'a>(
    device: &'a PlatformDevice,
    table: &[&str],
) -> Result<(String, AnyIdentity, InterruptResource<'a>), SysError> {
    if !compatible_matches(device, table) {
        return Err(SysError::DriverIncompatible);
    }
    let fwnode = device.fwnode().ok_or(SysError::MissingFwNode)?;
    let node = fwnode.as_of_node().ok_or(SysError::FwNodeLookupFailed)?;
    let origin = AnyIdentity::try_from(node.node().path().as_str())
        .map_err(|_| SysError::DriverIncompatible)?;
    let interrupt = select_interrupt_resource(fwnode.as_ref(), InterruptSelector::Name("macirq"))
        .map_err(|_| SysError::InvalidInterruptInfo)?;
    Ok((node.node().path(), origin, interrupt))
}

pub(in crate::driver::net::dwmac) fn shutdown(device: &dyn Device) {
    let Some(state) = device.drv_state().cast::<DwmacState>() else {
        return;
    };
    if let Some(control) = state.control.upgrade() {
        control.suppress_device();
    }
}

pub(in crate::driver::net::dwmac) fn publish_node<Q>(
    device: Arc<dyn Device>,
    origin: AnyIdentity,
    context: Arc<Q>,
    mac: [u8; 6],
    handler: &'static IrqHandler,
    private: AnyOpaque,
) -> Result<(), SysError>
where
    Q: DwmacFrameQueue + DwmacDeviceControl,
{
    if let Err(error) = request_irq_selected(
        device.as_ref(),
        InterruptSelector::Name("macirq"),
        None,
        handler,
        Some(private),
    ) {
        kerrln!(
            "dwmac {}: macirq registration failed: {:?}",
            device.name(),
            error
        );
        return Err(error);
    }

    context.suppress_device();
    let control: Arc<dyn DwmacDeviceControl> = context.clone();
    device.set_drv_state(AnyOpaque::new(DwmacState {
        control: Arc::downgrade(&control),
    }));
    let provider = DwmacFrameProvider::new(context, mac);
    let ready = ReadyNetdev::new(
        origin,
        Some(provider.ethernet_address()),
        provider.capabilities(),
        provider.link_state(),
        provider,
    );
    let snapshot = match publish(ready) {
        Ok(snapshot) => snapshot,
        Err((error, ready)) => {
            shutdown(device.as_ref());
            // IRQ registration is not removable. Retain the ready provider so
            // its DMA backing stays valid until reset or power-off.
            core::mem::forget(ready);
            kerrln!(
                "dwmac {}: netdev publication failed: {:?}",
                device.name(),
                error
            );
            return Err(SysError::ProbeFailed);
        },
    };
    let state = device
        .drv_state()
        .cast::<DwmacState>()
        .expect("published DWMAC must retain shutdown state");
    state
        .control
        .upgrade()
        .expect("published DWMAC lost its hardware context")
        .start_device();
    kinfoln!(
        "dwmac {} published as netdev {} (MAC {:?}, frame capacity {}); device causes enabled; DMA started",
        device.name(),
        snapshot.id().index(),
        snapshot.facts().ethernet_address,
        snapshot.facts().max_frame_len,
    );
    Ok(())
}

/// Adopt a Gate 2 owner that already owns the device `drv_state`. The IRQ
/// request is the first non-retiring boundary; the private context therefore
/// receives the same strong owner before commit, while publication still uses
/// the existing registry/attach authority.
pub(in crate::driver::net::dwmac) fn publish_adopted_node<Q>(
    device: Arc<dyn Device>,
    origin: AnyIdentity,
    context: Arc<Q>,
    mac: [u8; 6],
    link_state: anemone_net_api::LinkState,
    handler: &'static IrqHandler,
    private: AnyOpaque,
    expected: IrqSense,
) -> Result<(), SysError>
where
    Q: DwmacFrameQueue + DwmacDeviceControl,
{
    request_irq_selected(
        device.as_ref(),
        InterruptSelector::Name("macirq"),
        Some(expected),
        handler,
        Some(private),
    )?;

    context.suppress_device();
    let provider = DwmacFrameProvider::with_link_state(context.clone(), mac, link_state);
    let ready = ReadyNetdev::new(
        origin,
        Some(provider.ethernet_address()),
        provider.capabilities(),
        provider.link_state(),
        provider,
    );
    let snapshot = match publish(ready) {
        Ok(snapshot) => snapshot,
        Err((error, ready)) => {
            context.suppress_device();
            // There is no free_irq path. The descriptor's private context is
            // the retained lifetime carrier when publication is rejected.
            core::mem::forget(ready);
            kerrln!(
                "dwmac {}: adopted netdev publication failed: {:?}",
                device.name(),
                error
            );
            return Err(SysError::ProbeFailed);
        },
    };
    context.start_device();
    kinfoln!(
        "dwmac {} adopted as netdev {} (MAC {:?}, frame capacity {}); device causes enabled; DMA started",
        device.name(),
        snapshot.id().index(),
        snapshot.facts().ethernet_address,
        snapshot.facts().max_frame_len,
    );
    Ok(())
}
