//! Initial-domain static IPv4 route and source-selection owner.

use anemone_net_api::{InterfaceId, Ipv4Address, Ipv4Cidr};

use crate::{network_defs::StaticIpv4Deployment, prelude::*};

use super::LogicalInterfaceSnapshot;
use crate::net::worker::PumpWake;

pub(in crate::net) struct ExternalControlInput {
    logical: LogicalInterfaceSnapshot,
    interface: InterfaceId,
    wake: PumpWake,
}

impl ExternalControlInput {
    pub(in crate::net) fn new(
        logical: LogicalInterfaceSnapshot,
        interface: InterfaceId,
        wake: PumpWake,
    ) -> Self {
        Self {
            logical,
            interface,
            wake,
        }
    }

    pub(in crate::net) fn name(&self) -> &str {
        self.logical.name()
    }
}

#[derive(Clone)]
struct ExternalIpv4 {
    logical: LogicalInterfaceSnapshot,
    interface: InterfaceId,
    cidr: Ipv4Cidr,
    default_gateway: Option<Ipv4Address>,
    wake: PumpWake,
}

pub(in crate::net) struct Ipv4ControlPlane {
    local_interface: InterfaceId,
    local_wake: PumpWake,
    external: Option<ExternalIpv4>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::net) enum ControlPlaneActivationError {
    AlreadyPublished,
    MissingInterface,
    DuplicateInterface,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::net) enum SelectionError {
    NoRoute,
    SourceUnavailable,
    InterfaceUnavailable,
}

pub(in crate::net) struct Ipv4Selection {
    interface: InterfaceId,
    source: Ipv4Address,
    wake: PumpWake,
}

impl Ipv4Selection {
    pub(in crate::net) const fn interface(&self) -> InterfaceId {
        self.interface
    }

    pub(in crate::net) const fn source(&self) -> Ipv4Address {
        self.source
    }

    pub(in crate::net) fn request_pump(&self) {
        self.wake.request_work();
    }
}

impl Ipv4ControlPlane {
    pub(in crate::net) fn publish(
        slot: &mut Option<Self>,
        control_plane: Self,
    ) -> Result<(), ControlPlaneActivationError> {
        if slot.is_some() {
            return Err(ControlPlaneActivationError::AlreadyPublished);
        }
        *slot = Some(control_plane);
        Ok(())
    }

    pub(in crate::net) fn prepare(
        deployment: Option<StaticIpv4Deployment>,
        local_interface: InterfaceId,
        local_wake: PumpWake,
        inputs: &[ExternalControlInput],
    ) -> Result<Self, ControlPlaneActivationError> {
        let external = match deployment {
            None => None,
            Some(deployment) => {
                let mut matches = inputs
                    .iter()
                    .filter(|input| input.logical.name() == deployment.interface);
                let Some(input) = matches.next() else {
                    return Err(ControlPlaneActivationError::MissingInterface);
                };
                if matches.next().is_some() {
                    return Err(ControlPlaneActivationError::DuplicateInterface);
                }
                let cidr = Ipv4Cidr::new(Ipv4Address::new(deployment.address), deployment.prefix)
                    .expect("SystemTarget validation guarantees the generated prefix");
                Some(ExternalIpv4 {
                    logical: input.logical.clone(),
                    interface: input.interface,
                    cidr,
                    default_gateway: deployment.default_gateway.map(Ipv4Address::new),
                    wake: input.wake.clone(),
                })
            },
        };

        Ok(Self {
            local_interface,
            local_wake,
            external,
        })
    }

    pub(in crate::net) fn external_projection(
        &self,
    ) -> Option<(InterfaceId, Ipv4Cidr, Option<Ipv4Address>)> {
        self.external
            .as_ref()
            .map(|external| (external.interface, external.cidr, external.default_gateway))
    }

    pub(in crate::net) fn external_logical(&self) -> Option<&LogicalInterfaceSnapshot> {
        self.external.as_ref().map(|external| &external.logical)
    }

    pub(in crate::net) fn owns_local_address(&self, address: Ipv4Address) -> bool {
        address.is_loopback()
            || self
                .external
                .as_ref()
                .is_some_and(|external| external.cidr.address() == address)
    }

    pub(in crate::net) fn select(
        &self,
        destination: Ipv4Address,
        explicit_source: Option<Ipv4Address>,
    ) -> Result<Ipv4Selection, SelectionError> {
        let external_address = self
            .external
            .as_ref()
            .map(|external| external.cidr.address());
        let source_is_local =
            |source| source == Ipv4Address::LOOPBACK || external_address == Some(source);

        if external_address == Some(destination) || destination.is_loopback() {
            let default_source = if external_address == Some(destination) {
                destination
            } else {
                Ipv4Address::LOOPBACK
            };
            let source = explicit_source.unwrap_or(default_source);
            if !source_is_local(source) {
                return Err(SelectionError::SourceUnavailable);
            }
            return Ok(Ipv4Selection {
                interface: self.local_interface,
                source,
                wake: self.local_wake.clone(),
            });
        }

        let Some(external) = &self.external else {
            return Err(SelectionError::NoRoute);
        };
        if !external.cidr.contains(destination) && external.default_gateway.is_none() {
            return Err(SelectionError::NoRoute);
        }
        let source = explicit_source.unwrap_or(external.cidr.address());
        if source != external.cidr.address() {
            return Err(SelectionError::SourceUnavailable);
        }
        Ok(Ipv4Selection {
            interface: external.interface,
            source,
            wake: external.wake.clone(),
        })
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::{device::net::NetdevId, net::worker::PumpControl};

    fn external_snapshot(name: &str) -> LogicalInterfaceSnapshot {
        LogicalInterfaceSnapshot::for_control_plane_kunit(name, NetdevId::for_kunit(9))
    }

    fn wake() -> PumpWake {
        Arc::new(PumpControl::new()).pump_wake()
    }

    fn deployment(interface: &'static str) -> StaticIpv4Deployment {
        StaticIpv4Deployment {
            interface,
            address: [10, 0, 2, 15],
            prefix: 24,
            default_gateway: Some([10, 0, 2, 2]),
        }
    }

    #[kunit]
    fn ipv4_control_plane_table_and_source_matrix() {
        let external = ExternalControlInput::new(
            external_snapshot("eth0"),
            InterfaceId::from_index(1),
            wake(),
        );
        let control = Ipv4ControlPlane::prepare(
            Some(deployment("eth0")),
            InterfaceId::from_index(0),
            wake(),
            &[external],
        )
        .unwrap();

        let loopback = control
            .select(Ipv4Address::new([127, 7, 6, 5]), None)
            .unwrap();
        assert_eq!(loopback.interface(), InterfaceId::from_index(0));
        assert_eq!(loopback.source(), Ipv4Address::LOOPBACK);

        let self_external = control
            .select(Ipv4Address::new([10, 0, 2, 15]), None)
            .unwrap();
        assert_eq!(self_external.interface(), InterfaceId::from_index(0));
        assert_eq!(self_external.source(), Ipv4Address::new([10, 0, 2, 15]));

        assert!(matches!(
            control.select(
                Ipv4Address::new([10, 0, 2, 90]),
                Some(Ipv4Address::LOOPBACK),
            ),
            Err(SelectionError::SourceUnavailable)
        ));
        let default = control
            .select(Ipv4Address::new([203, 0, 113, 7]), None)
            .unwrap();
        assert_eq!(default.interface(), InterfaceId::from_index(1));
    }

    #[kunit]
    fn missing_interface_fails_before_control_publication() {
        let result = Ipv4ControlPlane::prepare(
            Some(deployment("eth0")),
            InterfaceId::from_index(0),
            wake(),
            &[],
        );
        assert!(matches!(
            result,
            Err(ControlPlaneActivationError::MissingInterface)
        ));

        let duplicate = [
            ExternalControlInput::new(
                external_snapshot("eth0"),
                InterfaceId::from_index(1),
                wake(),
            ),
            ExternalControlInput::new(
                external_snapshot("eth0"),
                InterfaceId::from_index(2),
                wake(),
            ),
        ];
        assert!(matches!(
            Ipv4ControlPlane::prepare(
                Some(deployment("eth0")),
                InterfaceId::from_index(0),
                wake(),
                &duplicate,
            ),
            Err(ControlPlaneActivationError::DuplicateInterface)
        ));
    }

    #[kunit]
    fn control_plane_publication_is_one_time() {
        let mut slot = None;
        let first =
            Ipv4ControlPlane::prepare(None, InterfaceId::from_index(0), wake(), &[]).unwrap();
        Ipv4ControlPlane::publish(&mut slot, first).unwrap();
        let second =
            Ipv4ControlPlane::prepare(None, InterfaceId::from_index(0), wake(), &[]).unwrap();
        assert!(matches!(
            Ipv4ControlPlane::publish(&mut slot, second),
            Err(ControlPlaneActivationError::AlreadyPublished)
        ));
    }
}
