use crate::{
    phy::{ChecksumCapabilities, DeviceCapabilities},
    wire::*,
};

#[allow(clippy::large_enum_variant)]
#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg(feature = "medium-ethernet")]
pub(crate) enum EthernetPacket<'a> {
    #[cfg(feature = "proto-ipv4")]
    Arp(ArpRepr),
    Ip(Packet<'a>),
}

#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(crate) enum Packet<'p> {
    #[cfg(feature = "proto-ipv4")]
    Ipv4(PacketV4<'p>),
    #[cfg(feature = "proto-ipv6")]
    Ipv6(PacketV6<'p>),
}

impl<'p> Packet<'p> {
    pub(crate) fn new(ip_repr: IpRepr, payload: IpPayload<'p>) -> Self {
        match ip_repr {
            #[cfg(feature = "proto-ipv4")]
            IpRepr::Ipv4(header) => Self::new_ipv4(header, payload),
            #[cfg(feature = "proto-ipv6")]
            IpRepr::Ipv6(header) => Self::new_ipv6(header, payload),
        }
    }

    #[cfg(feature = "proto-ipv4")]
    pub(crate) fn new_ipv4(ip_repr: Ipv4Repr, payload: IpPayload<'p>) -> Self {
        Self::Ipv4(PacketV4 {
            header: ip_repr,
            raw_header: None,
            payload,
        })
    }

    #[cfg(all(feature = "proto-ipv4", feature = "socket-raw"))]
    pub(crate) fn new_raw_ipv4(ip_repr: Ipv4Repr, packet: &'p [u8]) -> Self {
        let packet = Ipv4Packet::new_unchecked(packet);
        Self::Ipv4(PacketV4 {
            header: ip_repr,
            raw_header: Some(RawIpv4Header {
                dscp: packet.dscp(),
                ecn: packet.ecn(),
                ident: packet.ident(),
                dont_frag: packet.dont_frag(),
                more_frags: packet.more_frags(),
                frag_offset: packet.frag_offset(),
            }),
            payload: IpPayload::Raw(packet.payload()),
        })
    }

    #[cfg(feature = "proto-ipv6")]
    pub(crate) fn new_ipv6(ip_repr: Ipv6Repr, payload: IpPayload<'p>) -> Self {
        Self::Ipv6(PacketV6 {
            header: ip_repr,
            #[cfg(feature = "proto-ipv6-hbh")]
            hop_by_hop: None,
            #[cfg(feature = "proto-ipv6-fragmentation")]
            fragment: None,
            #[cfg(feature = "proto-ipv6-routing")]
            routing: None,
            payload,
        })
    }

    pub(crate) fn ip_repr(&self) -> IpRepr {
        match self {
            #[cfg(feature = "proto-ipv4")]
            Packet::Ipv4(p) => IpRepr::Ipv4(p.header),
            #[cfg(feature = "proto-ipv6")]
            Packet::Ipv6(p) => IpRepr::Ipv6(p.header),
        }
    }

    pub(crate) fn payload(&self) -> &IpPayload<'p> {
        match self {
            #[cfg(feature = "proto-ipv4")]
            Packet::Ipv4(p) => &p.payload,
            #[cfg(feature = "proto-ipv6")]
            Packet::Ipv6(p) => &p.payload,
        }
    }

    pub(crate) fn emit_ip_header(
        &self,
        ip_repr: &IpRepr,
        buffer: &mut [u8],
        checksum_caps: &ChecksumCapabilities,
    ) {
        ip_repr.emit(&mut *buffer, checksum_caps);

        #[cfg(feature = "proto-ipv4")]
        if let (Self::Ipv4(packet), IpRepr::Ipv4(_)) = (self, ip_repr)
            && let Some(raw) = packet.raw_header
        {
            let mut header = Ipv4Packet::new_unchecked(buffer);
            header.set_dscp(raw.dscp);
            header.set_ecn(raw.ecn);
            header.set_ident(raw.ident);
            header.clear_flags();
            header.set_dont_frag(raw.dont_frag);
            header.set_more_frags(raw.more_frags);
            header.set_frag_offset(raw.frag_offset);
            if checksum_caps.ipv4.tx() {
                header.fill_checksum();
            } else {
                header.set_checksum(0);
            }
        }
    }

    pub(crate) fn emit_payload(
        &self,
        _ip_repr: &IpRepr,
        payload: &mut [u8],
        caps: &DeviceCapabilities,
    ) {
        match self.payload() {
            #[cfg(feature = "proto-ipv4")]
            IpPayload::Icmpv4(icmpv4_repr) => {
                icmpv4_repr.emit(&mut Icmpv4Packet::new_unchecked(payload), &caps.checksum)
            },
            #[cfg(all(feature = "proto-ipv4", feature = "multicast"))]
            IpPayload::Igmp(igmp_repr) => igmp_repr.emit(&mut IgmpPacket::new_unchecked(payload)),
            #[cfg(feature = "proto-ipv6")]
            IpPayload::Icmpv6(icmpv6_repr) => {
                let ipv6_repr = match _ip_repr {
                    #[cfg(feature = "proto-ipv4")]
                    IpRepr::Ipv4(_) => unreachable!(),
                    IpRepr::Ipv6(repr) => repr,
                };

                icmpv6_repr.emit(
                    &ipv6_repr.src_addr,
                    &ipv6_repr.dst_addr,
                    &mut Icmpv6Packet::new_unchecked(payload),
                    &caps.checksum,
                )
            },
            #[cfg(feature = "proto-ipv6")]
            IpPayload::HopByHopIcmpv6(hbh_repr, icmpv6_repr) => {
                let ipv6_repr = match _ip_repr {
                    #[cfg(feature = "proto-ipv4")]
                    IpRepr::Ipv4(_) => unreachable!(),
                    IpRepr::Ipv6(repr) => repr,
                };

                let ipv6_ext_hdr = Ipv6ExtHeaderRepr {
                    next_header: IpProtocol::Icmpv6,
                    length: 0,
                    data: &[],
                };
                ipv6_ext_hdr.emit(&mut Ipv6ExtHeader::new_unchecked(
                    &mut payload[..ipv6_ext_hdr.header_len()],
                ));

                let hbh_start = ipv6_ext_hdr.header_len();
                let hbh_end = hbh_start + hbh_repr.buffer_len();
                hbh_repr.emit(&mut Ipv6HopByHopHeader::new_unchecked(
                    &mut payload[hbh_start..hbh_end],
                ));

                icmpv6_repr.emit(
                    &ipv6_repr.src_addr,
                    &ipv6_repr.dst_addr,
                    &mut Icmpv6Packet::new_unchecked(&mut payload[hbh_end..]),
                    &caps.checksum,
                );
            },

            #[cfg(feature = "socket-raw")]
            IpPayload::Raw(raw_packet) => {
                let len = raw_packet.len();
                payload[..len].copy_from_slice(raw_packet)
            },
            #[cfg(any(feature = "socket-udp", feature = "socket-dns"))]
            IpPayload::Udp(udp_repr, inner_payload) => udp_repr.emit(
                &mut UdpPacket::new_unchecked(payload),
                &_ip_repr.src_addr(),
                &_ip_repr.dst_addr(),
                inner_payload.len(),
                |buf| buf.copy_from_slice(inner_payload),
                &caps.checksum,
            ),
            #[cfg(feature = "socket-tcp")]
            &IpPayload::Tcp(mut tcp_repr) => {
                // This is a terrible hack to make TCP performance more acceptable on systems
                // where the TCP buffers are significantly larger than network buffers,
                // e.g. a 64 kB TCP receive buffer (and so, when empty, a 64k window)
                // together with four 1500 B Ethernet receive buffers. If left untreated,
                // this would result in our peer pushing our window and sever packet loss.
                //
                // I'm really not happy about this "solution" but I don't know what else to do.
                if let Some(max_burst_size) = caps.max_burst_size {
                    let mut max_segment_size = caps.max_transmission_unit;
                    max_segment_size -= _ip_repr.header_len();
                    max_segment_size -= tcp_repr.header_len();

                    let max_window_size = max_burst_size * max_segment_size;
                    if tcp_repr.window_len as usize > max_window_size {
                        tcp_repr.window_len = max_window_size as u16;
                    }
                }

                tcp_repr.emit(
                    &mut TcpPacket::new_unchecked(payload),
                    &_ip_repr.src_addr(),
                    &_ip_repr.dst_addr(),
                    &caps.checksum,
                );
            },
            #[cfg(feature = "socket-dhcpv4")]
            IpPayload::Dhcpv4(udp_repr, dhcp_repr) => udp_repr.emit(
                &mut UdpPacket::new_unchecked(payload),
                &_ip_repr.src_addr(),
                &_ip_repr.dst_addr(),
                dhcp_repr.buffer_len(),
                |buf| dhcp_repr.emit(&mut DhcpPacket::new_unchecked(buf)).unwrap(),
                &caps.checksum,
            ),
        }
    }
}

#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg(feature = "proto-ipv4")]
pub(crate) struct PacketV4<'p> {
    header: Ipv4Repr,
    // `Ipv4Repr` intentionally omits raw-socket header policy. This snapshot
    // preserves only fields that the normal emitter would otherwise reset;
    // routing and payload length remain owned by `header`.
    raw_header: Option<RawIpv4Header>,
    payload: IpPayload<'p>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg(feature = "proto-ipv4")]
struct RawIpv4Header {
    dscp: u8,
    ecn: u8,
    ident: u16,
    dont_frag: bool,
    more_frags: bool,
    frag_offset: u16,
}

#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg(feature = "proto-ipv6")]
pub(crate) struct PacketV6<'p> {
    pub(crate) header: Ipv6Repr,
    #[cfg(feature = "proto-ipv6-hbh")]
    pub(crate) hop_by_hop: Option<Ipv6HopByHopRepr<'p>>,
    #[cfg(feature = "proto-ipv6-fragmentation")]
    pub(crate) fragment: Option<Ipv6FragmentRepr>,
    #[cfg(feature = "proto-ipv6-routing")]
    pub(crate) routing: Option<Ipv6RoutingRepr<'p>>,
    pub(crate) payload: IpPayload<'p>,
}

#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(crate) enum IpPayload<'p> {
    #[cfg(feature = "proto-ipv4")]
    Icmpv4(Icmpv4Repr<'p>),
    #[cfg(all(feature = "proto-ipv4", feature = "multicast"))]
    Igmp(IgmpRepr),
    #[cfg(feature = "proto-ipv6")]
    Icmpv6(Icmpv6Repr<'p>),
    #[cfg(feature = "proto-ipv6")]
    HopByHopIcmpv6(Ipv6HopByHopRepr<'p>, Icmpv6Repr<'p>),
    #[cfg(feature = "socket-raw")]
    Raw(&'p [u8]),
    #[cfg(any(feature = "socket-udp", feature = "socket-dns"))]
    Udp(UdpRepr, &'p [u8]),
    #[cfg(feature = "socket-tcp")]
    Tcp(TcpRepr<'p>),
    #[cfg(feature = "socket-dhcpv4")]
    Dhcpv4(UdpRepr, DhcpRepr<'p>),
}

impl<'p> IpPayload<'p> {
    #[cfg(feature = "proto-sixlowpan")]
    pub(crate) fn as_sixlowpan_next_header(&self) -> SixlowpanNextHeader {
        match self {
            #[cfg(feature = "proto-ipv4")]
            Self::Icmpv4(_) => unreachable!(),
            #[cfg(feature = "socket-dhcpv4")]
            Self::Dhcpv4(..) => unreachable!(),
            #[cfg(feature = "proto-ipv6")]
            Self::Icmpv6(_) => SixlowpanNextHeader::Uncompressed(IpProtocol::Icmpv6),
            #[cfg(feature = "proto-ipv6")]
            Self::HopByHopIcmpv6(_, _) => unreachable!(),
            #[cfg(all(feature = "proto-ipv4", feature = "multicast"))]
            Self::Igmp(_) => unreachable!(),
            #[cfg(feature = "socket-tcp")]
            Self::Tcp(_) => SixlowpanNextHeader::Uncompressed(IpProtocol::Tcp),
            #[cfg(any(feature = "socket-udp", feature = "socket-dns"))]
            Self::Udp(..) => SixlowpanNextHeader::Compressed,
            #[cfg(feature = "socket-raw")]
            Self::Raw(_) => todo!(),
        }
    }
}

#[cfg(any(feature = "proto-ipv4", feature = "proto-ipv6"))]
pub(crate) fn icmp_reply_payload_len(len: usize, mtu: usize, header_len: usize) -> usize {
    // Send back as much of the original payload as will fit within
    // the minimum MTU required by IPv4. See RFC 1812 § 4.3.2.3 for
    // more details.
    //
    // Since the entire network layer packet must fit within the minimum
    // MTU supported, the payload must not exceed the following:
    //
    // <min mtu> - IP Header Size * 2 - ICMPv4 DstUnreachable hdr size
    len.min(mtu - header_len * 2 - 8)
}
