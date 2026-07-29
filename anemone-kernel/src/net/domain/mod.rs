//! Initial network-domain identity and protocol-Stack owners.

mod interfaces;
mod stack;

use interfaces::LogicalInterfaces;
pub(super) use interfaces::{LogicalInterfaceReservation, LogicalInterfaceSnapshot};
pub(super) use stack::{DomainStack, ExternalMapping, ExternalPumpPort};

use crate::prelude::*;

/// Boot-persistent composition of the two distinct initial-domain owners.
pub(super) struct InitialDomain {
    logical: LogicalInterfaces,
    stack: Arc<DomainStack>,
}

impl InitialDomain {
    pub(super) fn new() -> Self {
        let domain = Self {
            logical: LogicalInterfaces::new(),
            stack: Arc::new(DomainStack::new()),
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

    pub(super) fn stack(&self) -> Arc<DomainStack> {
        self.stack.clone()
    }
}
