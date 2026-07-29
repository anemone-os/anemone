//! Temporary UDP capability used to validate the production local path.
//!
//! This module is independent of the kernel test harness. Stage 3 must delete
//! it when the real Socket owner consumes the accepted Endpoint capability.

use alloc::vec::Vec;

use anemone_net_api::{InterfaceId, Ipv4Address as ApiIpv4Address};
use smoltcp::wire::{IpAddress, IpEndpoint, Ipv4Address};

use crate::udp::{EndpointCreateError, EndpointId as OwnedEndpointId, SendError as OwnedSendError};

use super::Stack;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EndpointId(OwnedEndpointId);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CreateError {
    InvalidPort,
    PortInUse,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SendError {
    UnknownEndpoint,
    UnknownInterface,
    UnsupportedSource,
    InvalidDestination,
    Oversize { maximum: usize },
    TxFull,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetireError {
    UnknownEndpoint,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceivedDatagram {
    pub payload: Vec<u8>,
    pub source_address: ApiIpv4Address,
    pub source_port: u16,
}

pub fn create(
    stack: &mut Stack,
    port: u16,
    receive_packet_capacity: usize,
    engine_payload_capacity: usize,
) -> Result<EndpointId, CreateError> {
    stack
        .create_udp_endpoint(port, receive_packet_capacity, engine_payload_capacity)
        .map(EndpointId)
        .map_err(|error| match error {
            EndpointCreateError::InvalidPort => CreateError::InvalidPort,
            EndpointCreateError::PortInUse => CreateError::PortInUse,
        })
}

/// Route and source selection remain outside Stack and are supplied by the
/// production control-plane owner.
pub fn send(
    stack: &mut Stack,
    endpoint: EndpointId,
    interface: InterfaceId,
    source: ApiIpv4Address,
    destination: ApiIpv4Address,
    destination_port: u16,
    payload: &[u8],
) -> Result<(), SendError> {
    stack
        .send_udp(
            endpoint.0,
            Some(interface),
            Ipv4Address::from_octets(source.octets()),
            IpEndpoint::new(
                IpAddress::Ipv4(Ipv4Address::from_octets(destination.octets())),
                destination_port,
            ),
            payload,
        )
        .map_err(|error| match error {
            OwnedSendError::UnknownEndpoint => SendError::UnknownEndpoint,
            OwnedSendError::MissingSelection => {
                unreachable!("the UDP probe always supplies a selection")
            },
            OwnedSendError::UnknownInterface => SendError::UnknownInterface,
            OwnedSendError::UnsupportedSource => SendError::UnsupportedSource,
            OwnedSendError::InvalidDestination => SendError::InvalidDestination,
            OwnedSendError::Oversize { maximum } => SendError::Oversize { maximum },
            OwnedSendError::TxFull => SendError::TxFull,
        })
}

pub fn receive(stack: &mut Stack, endpoint: EndpointId) -> Option<ReceivedDatagram> {
    let datagram = stack.udp.receive(endpoint.0)?;
    let IpAddress::Ipv4(source_address) = datagram.source.addr;
    Some(ReceivedDatagram {
        payload: datagram.payload,
        source_address: ApiIpv4Address::new(source_address.octets()),
        source_port: datagram.source.port,
    })
}

pub fn retire(stack: &mut Stack, endpoint: EndpointId) -> Result<(), RetireError> {
    stack
        .retire_udp_endpoint(endpoint.0)
        .map_err(|error| match error {
            crate::udp::RetireError::UnknownEndpoint => RetireError::UnknownEndpoint,
        })
}
