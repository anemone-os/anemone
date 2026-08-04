//! Static Linux ABI profiles for resolved Socket semantic types.

use core::sync::atomic::{AtomicBool, Ordering};

use anemone_abi::net::linux::{
    AF_INET, AF_UNIX, IPPROTO_ICMP, IPPROTO_UDP, MSG_DONTWAIT, MSG_NOSIGNAL, MSG_PEEK, MSG_TRUNC,
    SOCK_DGRAM, SOCK_RAW, SOCK_STREAM,
};

use crate::{
    fs::socket::{
        ICMP_RAW_SOCKET_OPS, SocketOps, SocketType, UDP_SOCKET_OPS, UNIX_STREAM_SOCKET_OPS,
    },
    prelude::*,
    task::credentials::cap::Capability,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketAddressAbi {
    Ipv4,
    UnixPathname,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketMessageIo {
    Unsupported,
    Datagram,
}

#[derive(Clone, Copy)]
enum ProtocolAdmission {
    Canonical,
    CanonicalOrZero,
}

struct NoSignalCompatibility {
    // Diagnostic-only rate limiting. This bit never participates in flag
    // admission, signal suppression, or send behavior.
    emitted: AtomicBool,
    diagnostic: &'static str,
}

impl NoSignalCompatibility {
    const fn new(diagnostic: &'static str) -> Self {
        Self {
            emitted: AtomicBool::new(false),
            diagnostic,
        }
    }

    fn observe(&self) {
        if !self.emitted.swap(true, Ordering::Relaxed) {
            knoticeln!("{}", self.diagnostic);
        }
    }
}

// UDP and raw ICMP have no peer-close or SIGPIPE producer, so MSG_NOSIGNAL is
// a visible no-op accepted for Linux compatibility. Remove these profiles only
// when the family gains a real SIGPIPE path and routes the flag through the
// ordinary signal-suppression protocol.
static UDP_NOSIGNAL_COMPATIBILITY: NoSignalCompatibility =
    NoSignalCompatibility::new("UDP send: MSG_NOSIGNAL accepted without a SIGPIPE producer");
static ICMP_RAW_NOSIGNAL_COMPATIBILITY: NoSignalCompatibility =
    NoSignalCompatibility::new("ICMP raw send: MSG_NOSIGNAL accepted without a SIGPIPE producer");

/// One immutable ABI publication profile associated with a static SocketOps
/// descriptor. Linux representation and compatibility policy remain in this
/// adapter-owned table; the descriptor remains the sole semantic type witness.
pub(super) struct SocketAbiProfile {
    ops: &'static SocketOps,
    domain: i32,
    socket_kind: i32,
    protocol: i32,
    protocol_admission: ProtocolAdmission,
    address: SocketAddressAbi,
    send_flags: i32,
    receive_flags: i32,
    message_io: SocketMessageIo,
    required_capability: Option<Capability>,
    no_signal_compatibility: Option<&'static NoSignalCompatibility>,
}

impl SocketAbiProfile {
    pub(super) const fn ops(&self) -> &'static SocketOps {
        self.ops
    }

    pub(super) const fn socket_type(&self) -> SocketType {
        self.ops.socket_type
    }

    pub(super) const fn domain(&self) -> i32 {
        self.domain
    }

    pub(super) const fn socket_kind(&self) -> i32 {
        self.socket_kind
    }

    pub(super) const fn protocol(&self) -> i32 {
        self.protocol
    }

    pub(super) const fn address(&self) -> SocketAddressAbi {
        self.address
    }

    pub(super) const fn send_flags(&self) -> i32 {
        self.send_flags
    }

    pub(super) const fn receive_flags(&self) -> i32 {
        self.receive_flags
    }

    pub(super) const fn message_io(&self) -> SocketMessageIo {
        self.message_io
    }

    pub(super) const fn required_capability(&self) -> Option<Capability> {
        self.required_capability
    }

    pub(super) fn observe_no_signal_compatibility(&self, flags: i32) {
        if flags & MSG_NOSIGNAL != 0 {
            if let Some(compatibility) = self.no_signal_compatibility {
                compatibility.observe();
            }
        }
    }

    const fn accepts_protocol(&self, protocol: i32) -> bool {
        match self.protocol_admission {
            ProtocolAdmission::Canonical => protocol == self.protocol,
            ProtocolAdmission::CanonicalOrZero => protocol == 0 || protocol == self.protocol,
        }
    }
}

static SOCKET_ABI_PROFILES: [SocketAbiProfile; 3] = [
    SocketAbiProfile {
        ops: &UDP_SOCKET_OPS,
        domain: AF_INET,
        socket_kind: SOCK_DGRAM,
        protocol: IPPROTO_UDP,
        protocol_admission: ProtocolAdmission::CanonicalOrZero,
        address: SocketAddressAbi::Ipv4,
        send_flags: MSG_DONTWAIT | MSG_NOSIGNAL,
        receive_flags: MSG_DONTWAIT | MSG_PEEK | MSG_TRUNC,
        message_io: SocketMessageIo::Datagram,
        required_capability: None,
        no_signal_compatibility: Some(&UDP_NOSIGNAL_COMPATIBILITY),
    },
    SocketAbiProfile {
        ops: &ICMP_RAW_SOCKET_OPS,
        domain: AF_INET,
        socket_kind: SOCK_RAW,
        protocol: IPPROTO_ICMP,
        protocol_admission: ProtocolAdmission::Canonical,
        address: SocketAddressAbi::Ipv4,
        send_flags: MSG_DONTWAIT | MSG_NOSIGNAL,
        receive_flags: MSG_DONTWAIT | MSG_PEEK | MSG_TRUNC,
        message_io: SocketMessageIo::Unsupported,
        required_capability: Some(Capability::NET_RAW),
        no_signal_compatibility: Some(&ICMP_RAW_NOSIGNAL_COMPATIBILITY),
    },
    SocketAbiProfile {
        ops: &UNIX_STREAM_SOCKET_OPS,
        domain: AF_UNIX,
        socket_kind: SOCK_STREAM,
        protocol: 0,
        protocol_admission: ProtocolAdmission::Canonical,
        address: SocketAddressAbi::UnixPathname,
        send_flags: MSG_DONTWAIT | MSG_NOSIGNAL,
        receive_flags: MSG_DONTWAIT | MSG_PEEK,
        message_io: SocketMessageIo::Unsupported,
        required_capability: None,
        no_signal_compatibility: None,
    },
];

pub(super) fn resolve_socket_profile(
    family: i32,
    socket_kind: i32,
    protocol: i32,
) -> Result<&'static SocketAbiProfile, SysError> {
    let profile = SOCKET_ABI_PROFILES
        .iter()
        .find(|profile| profile.domain == family && profile.socket_kind == socket_kind)
        .ok_or_else(|| {
            if SOCKET_ABI_PROFILES
                .iter()
                .any(|profile| profile.domain == family)
            {
                SysError::SocketTypeNotSupported
            } else {
                SysError::AddressFamilyNotSupported
            }
        })?;
    if !profile.accepts_protocol(protocol) {
        return Err(SysError::ProtocolNotSupported);
    }
    Ok(profile)
}

pub(super) fn socket_abi_profile(socket_type: SocketType) -> &'static SocketAbiProfile {
    let profile = match socket_type {
        SocketType::Ipv4Udp => &SOCKET_ABI_PROFILES[0],
        SocketType::Ipv4IcmpRaw => &SOCKET_ABI_PROFILES[1],
        SocketType::UnixStream => &SOCKET_ABI_PROFILES[2],
    };
    assert_eq!(
        profile.socket_type(),
        socket_type,
        "Socket ABI profile table disagrees with its SocketOps type witness"
    );
    profile
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn profile_table_round_trips_every_semantic_type_and_canonical_tuple() {
        for profile in &SOCKET_ABI_PROFILES {
            assert!(core::ptr::eq(
                socket_abi_profile(profile.socket_type()),
                profile
            ));
            assert!(core::ptr::eq(
                resolve_socket_profile(profile.domain(), profile.socket_kind(), profile.protocol())
                    .unwrap(),
                profile
            ));
        }
    }

    #[kunit]
    fn message_publication_is_independent_of_datagram_family_capability() {
        assert_eq!(
            socket_abi_profile(SocketType::Ipv4Udp).message_io(),
            SocketMessageIo::Datagram
        );
        assert_eq!(
            socket_abi_profile(SocketType::Ipv4IcmpRaw).message_io(),
            SocketMessageIo::Unsupported
        );
    }
}
