//! Request-local projections for read-only network diagnostics.

use anemone_net_api::{InterfaceId, Ipv4Address, LinkState, tcp::TcpDiagnosticRecord};

use crate::{kconfig_defs::NET_LOCAL_LINK_MTU_BYTES, prelude::*};

use super::{ACTIVE_PATHS, domain::LogicalInterfaceKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LinkDiagnosticKind {
    Loopback,
    Ethernet,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LinkDiagnostic {
    pub(crate) ifindex: u32,
    pub(crate) name: String,
    pub(crate) kind: LinkDiagnosticKind,
    pub(crate) active: bool,
    pub(crate) mtu: usize,
    pub(crate) ethernet_address: Option<[u8; 6]>,
    pub(crate) link_state: LinkState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ipv4AddressDiagnostic {
    pub(crate) ifindex: u32,
    pub(crate) label: String,
    pub(crate) address: Ipv4Address,
    pub(crate) prefix_len: u8,
    pub(crate) host_scope: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Ipv4RouteDiagnostic {
    pub(crate) destination: Option<(Ipv4Address, u8)>,
    pub(crate) gateway: Option<Ipv4Address>,
    pub(crate) preferred_source: Ipv4Address,
    pub(crate) output_ifindex: u32,
}

pub(crate) struct NetworkRouteDiagnostics {
    pub(crate) links: Vec<LinkDiagnostic>,
    pub(crate) addresses: Vec<Ipv4AddressDiagnostic>,
    pub(crate) routes: Vec<Ipv4RouteDiagnostic>,
}

pub(crate) struct TcpDiagnostic {
    pub(crate) record: TcpDiagnosticRecord,
    pub(crate) ifindex: u32,
}

pub(crate) fn route_diagnostics() -> NetworkRouteDiagnostics {
    let authority = ACTIVE_PATHS.lock();
    let members = authority.domain.logical_diagnostic_members();
    let control = authority
        .domain
        .control_plane()
        .map(|control| control.diagnostic_snapshot());

    let mut links = Vec::with_capacity(members.len());
    for member in &members {
        match member.kind() {
            LogicalInterfaceKind::Loopback => links.push(LinkDiagnostic {
                ifindex: member.ifindex(),
                name: member.name().to_string(),
                kind: LinkDiagnosticKind::Loopback,
                active: control.is_some() && !authority.shutdown_started,
                mtu: NET_LOCAL_LINK_MTU_BYTES,
                ethernet_address: None,
                link_state: LinkState::Up,
            }),
            LogicalInterfaceKind::External => {
                let path = authority
                    .active_paths
                    .iter()
                    .find(|path| path.logical.id() == member.id())
                    .expect("published external logical interface lost its active path");
                let facts = path.netdev.facts();
                links.push(LinkDiagnostic {
                    ifindex: member.ifindex(),
                    name: member.name().to_string(),
                    kind: LinkDiagnosticKind::Ethernet,
                    active: !authority.shutdown_started,
                    mtu: facts
                        .max_frame_len
                        .checked_sub(14)
                        .expect("published Ethernet frame capacity must include its header"),
                    ethernet_address: facts.ethernet_address.map(|address| address.octets()),
                    link_state: facts.link_state,
                });
            },
        }
    }

    let loopback = members
        .iter()
        .find(|member| member.kind() == LogicalInterfaceKind::Loopback)
        .expect("initial domain must retain loopback");
    let mut addresses = vec![Ipv4AddressDiagnostic {
        ifindex: loopback.ifindex(),
        label: loopback.name().to_string(),
        address: Ipv4Address::LOOPBACK,
        prefix_len: 8,
        host_scope: true,
    }];
    let mut routes = Vec::new();
    if let Some(control) = control {
        if let Some((logical, _, cidr, gateway)) = control.external {
            addresses.push(Ipv4AddressDiagnostic {
                ifindex: logical.ifindex(),
                label: logical.name().to_string(),
                address: cidr.address(),
                prefix_len: cidr.prefix_len(),
                host_scope: false,
            });
            routes.push(Ipv4RouteDiagnostic {
                destination: Some((
                    Ipv4Address::new(cidr.network_bits().to_be_bytes()),
                    cidr.prefix_len(),
                )),
                gateway: None,
                preferred_source: cidr.address(),
                output_ifindex: logical.ifindex(),
            });
            if let Some(gateway) = gateway {
                routes.push(Ipv4RouteDiagnostic {
                    destination: None,
                    gateway: Some(gateway),
                    preferred_source: cidr.address(),
                    output_ifindex: logical.ifindex(),
                });
            }
        }
    }
    NetworkRouteDiagnostics {
        links,
        addresses,
        routes,
    }
}

pub(crate) fn tcp_diagnostics() -> Vec<TcpDiagnostic> {
    let (stack, mappings) = {
        let authority = ACTIVE_PATHS.lock();
        let Some(control) = authority.domain.control_plane() else {
            return Vec::new();
        };
        let control = control.diagnostic_snapshot();
        let loopback = authority
            .domain
            .logical_diagnostic_members()
            .into_iter()
            .find(|member| member.kind() == LogicalInterfaceKind::Loopback)
            .expect("initial domain must retain loopback");
        let mut mappings = vec![(control.local_interface, loopback.ifindex())];
        mappings.extend(
            authority
                .active_paths
                .iter()
                .map(|path| (path.interface, path.logical.ifindex())),
        );
        (authority.domain.stack(), mappings)
    };

    stack
        .tcp_diagnostic_records()
        .into_iter()
        .map(|record| TcpDiagnostic {
            ifindex: logical_ifindex(record.interface(), &mappings),
            record,
        })
        .collect()
}

fn logical_ifindex(interface: InterfaceId, mappings: &[(InterfaceId, u32)]) -> u32 {
    mappings
        .iter()
        .find_map(|(candidate, ifindex)| (*candidate == interface).then_some(*ifindex))
        .expect("TCP diagnostic record references an unpublished interface")
}
