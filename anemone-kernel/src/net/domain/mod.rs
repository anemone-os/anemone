//! Initial network-domain identity and protocol-Stack owners.

mod control_plane;
mod interfaces;
mod stack;

pub(super) use control_plane::{
    ControlPlaneActivationError, ExternalControlInput, Ipv4ControlPlane, Ipv4Selection,
    SelectionError,
};
use interfaces::LogicalInterfaces;
pub(super) use interfaces::{
    LogicalInterfaceKind, LogicalInterfaceReservation, LogicalInterfaceSnapshot,
};
pub(super) use stack::{DomainStack, ExternalMapping, ExternalPumpPort, LocalPumpPort};

use crate::prelude::*;

/// Boot-persistent composition of the two distinct initial-domain owners.
pub(super) struct InitialDomain {
    logical: LogicalInterfaces,
    stack: Arc<DomainStack>,
    local_path: crate::net::worker::PreparedLocalPath,
    control_plane: Option<Ipv4ControlPlane>,
}

impl InitialDomain {
    pub(super) fn new() -> Self {
        let stack = Arc::new(DomainStack::new(
            crate::net::udp::UDP_NAMESPACE_POLICY,
            crate::net::icmp_raw::ICMP_RAW_NAMESPACE_POLICY,
            crate::net::tcp::TCP_POLICY,
        ));
        let local_port = stack
            .attach_local(crate::net::worker::network_now())
            .expect("the initial domain must create exactly one local mapping");
        let local_path = crate::net::worker::prepare_local(local_port);
        let domain = Self {
            logical: LogicalInterfaces::new(),
            stack,
            local_path,
            control_plane: None,
        };
        let loopback = domain.logical.loopback();
        kinfoln!(
            "initial network domain: {} (ifindex {}, {:?}) committed; global protocol Stack initialized",
            loopback.name(),
            loopback.ifindex(),
            loopback.kind(),
        );
        domain
    }

    pub(super) fn logical_mut(&mut self) -> &mut LogicalInterfaces {
        &mut self.logical
    }

    pub(in crate::net) fn logical_diagnostic_members(&self) -> Vec<LogicalInterfaceSnapshot> {
        self.logical.diagnostic_members()
    }

    pub(super) fn stack(&self) -> Arc<DomainStack> {
        self.stack.clone()
    }

    pub(super) fn activate_control_plane(
        &mut self,
        deployment: Option<crate::network_defs::StaticIpv4Deployment>,
        external: &[control_plane::ExternalControlInput],
    ) -> Result<(), ControlPlaneActivationError> {
        if self.control_plane.is_some() {
            return Err(ControlPlaneActivationError::AlreadyPublished);
        }
        let control_plane =
            Ipv4ControlPlane::prepare(deployment, self.local_path.interface(), external)?;
        self.stack
            .install_ipv4_projection(control_plane.external_projection())
            .expect("validated IPv4 projection must fit the global Stack");
        Ipv4ControlPlane::publish(&mut self.control_plane, control_plane)?;
        // Publication is complete before the local worker gains pump admission.
        self.local_path.activate();
        Ok(())
    }

    pub(super) fn control_plane(&self) -> Option<&Ipv4ControlPlane> {
        self.control_plane.as_ref()
    }

    pub(super) fn local_progression_control(
        &self,
        interface: anemone_net_api::InterfaceId,
    ) -> Option<Arc<crate::net::worker::PumpControl>> {
        (self.local_path.interface() == interface).then(|| self.local_path.control())
    }

    pub(super) fn withdraw_control_plane(
        &mut self,
    ) -> Option<Arc<crate::net::worker::PumpControl>> {
        self.control_plane.take()?;
        Some(self.local_path.control())
    }
}
