//! Kernel-private role capability for the initial-domain ICMP raw owner.
//!
//! Checkpoint 1 intentionally has no Socket consumer. Every item in this
//! module remains crate-private and unreachable from syscalls; Checkpoint 2
//! must either consume this final-shape capability or remove it.

#![allow(dead_code)]

use anemone_net_api::{
    Ipv4Address, Ipv4EgressSelection,
    icmp_raw::{
        IcmpRawAssociation, IcmpRawCreateError, IcmpRawDropDiagnostics, IcmpRawEgressPolicy,
        IcmpRawEndpointConfig, IcmpRawEndpointFacts, IcmpRawEndpointId, IcmpRawEndpointLimits,
        IcmpRawMutationError, IcmpRawNamespacePolicy, IcmpRawQueryError, IcmpRawReceiveError,
        IcmpRawReceivedPacket, IcmpRawRetireError, IcmpRawSendError, IcmpRawTypeFilter,
    },
};

use crate::{kconfig_defs::*, prelude::*};

use super::{ACTIVE_PATHS, domain::DomainStack};

pub(crate) use super::EventRegistrationError;

pub(in crate::net) const ICMP_RAW_NAMESPACE_POLICY: IcmpRawNamespacePolicy =
    IcmpRawNamespacePolicy::new(NET_ICMP_RAW_ENDPOINT_CAPACITY);
const ICMP_RAW_ENDPOINT_LIMITS: IcmpRawEndpointLimits = IcmpRawEndpointLimits::new(
    NET_ICMP_RAW_TX_PACKET_CAPACITY,
    NET_ICMP_RAW_TX_BYTE_CAPACITY,
    NET_ICMP_RAW_RX_PACKET_CAPACITY,
    NET_ICMP_RAW_RX_BYTE_CAPACITY,
);

static_assert!(NET_ICMP_RAW_ENDPOINT_CAPACITY > 0);
static_assert!(NET_ICMP_RAW_TX_PACKET_CAPACITY > 0);
static_assert!(NET_ICMP_RAW_TX_BYTE_CAPACITY >= 20);
static_assert!(NET_ICMP_RAW_RX_PACKET_CAPACITY > 0);
static_assert!(NET_ICMP_RAW_RX_BYTE_CAPACITY >= 20);

#[derive(Clone)]
pub(crate) struct IcmpRawEndpointPort {
    stack: Arc<DomainStack>,
    endpoint: IcmpRawEndpointId,
}

impl core::fmt::Debug for IcmpRawEndpointPort {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("IcmpRawEndpointPort")
            .finish_non_exhaustive()
    }
}

pub(crate) trait IcmpRawEndpointInvalidationObserver: Send + Sync {
    fn invalidate(&self);
}

pub(crate) struct IcmpRawEndpointEventRegistration {
    stack: Arc<DomainStack>,
    endpoint: IcmpRawEndpointId,
    active: bool,
}

impl IcmpRawEndpointEventRegistration {
    pub(crate) fn unregister(mut self) {
        self.stack
            .unregister_icmp_raw_endpoint_observer(self.endpoint);
        self.active = false;
    }
}

impl Drop for IcmpRawEndpointEventRegistration {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        self.stack
            .unregister_icmp_raw_endpoint_observer(self.endpoint);
        self.active = false;
        assert!(
            false,
            "ICMP raw endpoint event registration dropped while active"
        );
    }
}

pub(crate) fn create_endpoint() -> Result<IcmpRawEndpointPort, IcmpRawCreateError> {
    let stack = {
        let authority = ACTIVE_PATHS.lock();
        assert!(
            !authority.shutdown_started,
            "ICMP raw endpoint creation raced terminal network shutdown"
        );
        assert!(
            authority.domain.control_plane().is_some(),
            "ICMP raw endpoint creation preceded control-plane publication"
        );
        authority.domain.stack()
    };
    let endpoint = stack.create_icmp_raw_endpoint(ICMP_RAW_ENDPOINT_LIMITS)?;
    Ok(IcmpRawEndpointPort { stack, endpoint })
}

impl IcmpRawEndpointPort {
    pub(crate) fn register_invalidation_observer(
        &self,
        observer: &Arc<dyn IcmpRawEndpointInvalidationObserver>,
    ) -> Result<IcmpRawEndpointEventRegistration, EventRegistrationError> {
        self.stack
            .register_icmp_raw_endpoint_observer(self.endpoint, observer)?;
        Ok(IcmpRawEndpointEventRegistration {
            stack: self.stack.clone(),
            endpoint: self.endpoint,
            active: true,
        })
    }

    pub(crate) fn set_association(
        &self,
        association: IcmpRawAssociation,
    ) -> Result<(), IcmpRawMutationError> {
        if let Some(local) = association.local() {
            let owned = {
                let authority = ACTIVE_PATHS.lock();
                authority
                    .domain
                    .control_plane()
                    .expect("published ICMP raw capability lost its control plane")
                    .owns_local_address(local)
            };
            if !owned {
                return Err(IcmpRawMutationError::InvalidAssociation);
            }
        }
        self.stack
            .set_icmp_raw_association(self.endpoint, association)
    }

    pub(crate) fn set_filter(&self, filter: IcmpRawTypeFilter) -> Result<(), IcmpRawMutationError> {
        self.stack.set_icmp_raw_filter(self.endpoint, filter)
    }

    pub(crate) fn config(&self) -> Result<IcmpRawEndpointConfig, IcmpRawQueryError> {
        self.stack.icmp_raw_endpoint_config(self.endpoint)
    }

    pub(crate) fn facts(&self) -> Result<IcmpRawEndpointFacts, IcmpRawQueryError> {
        self.stack.icmp_raw_endpoint_facts(self.endpoint)
    }

    pub(crate) fn diagnostics(&self) -> Result<IcmpRawDropDiagnostics, IcmpRawQueryError> {
        self.stack.icmp_raw_endpoint_diagnostics(self.endpoint)
    }

    pub(crate) fn send(
        &self,
        destination: Ipv4Address,
        policy: IcmpRawEgressPolicy,
        message: &[u8],
    ) -> Result<(), SendError> {
        let association = self.config().map_err(|error| match error {
            IcmpRawQueryError::UnknownEndpoint => {
                SendError::Stack(IcmpRawSendError::UnknownEndpoint)
            },
        })?;
        let selection = {
            let authority = ACTIVE_PATHS.lock();
            authority
                .domain
                .control_plane()
                .expect("published ICMP raw capability lost its control plane")
                .select(destination, association.association().local())
                .map_err(|error| match error {
                    super::domain::SelectionError::NoRoute => SendError::NoRoute,
                    super::domain::SelectionError::SourceUnavailable => {
                        SendError::SourceUnavailable
                    },
                    super::domain::SelectionError::InterfaceUnavailable => {
                        SendError::InterfaceUnavailable
                    },
                })?
        };
        self.stack
            .send_icmp_raw_endpoint(
                self.endpoint,
                Ipv4EgressSelection::new(selection.interface(), selection.source()),
                destination,
                policy,
                message,
            )
            .map_err(SendError::Stack)?;
        selection.request_pump();
        Ok(())
    }

    pub(crate) fn receive(&self, peek: bool) -> Result<IcmpRawReceivedPacket, IcmpRawReceiveError> {
        self.stack.receive_icmp_raw_endpoint(self.endpoint, peek)
    }

    pub(crate) fn retire(&self) -> Result<(), IcmpRawRetireError> {
        self.stack.retire_icmp_raw_endpoint(self.endpoint)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SendError {
    NoRoute,
    SourceUnavailable,
    InterfaceUnavailable,
    Stack(IcmpRawSendError),
}
