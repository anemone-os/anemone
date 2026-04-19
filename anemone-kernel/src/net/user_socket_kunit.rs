//! KUnit coverage for user socket helpers (ABI parsing). Full coexistence with
//! `icmp_raw` / `net-probe` is validated at runtime when those features are enabled.

use smoltcp::wire::Ipv4Address;

use crate::net::user_socket::parse_sockaddr_in;
use crate::prelude::*;

#[kunit]
fn sockaddr_in_parse_udp_tuple() {
    let mut b = [0u8; 16];
    b[0..2].copy_from_slice(&2u16.to_ne_bytes());
    b[2..4].copy_from_slice(&4660u16.to_be_bytes());
    b[4..8].copy_from_slice(&[192, 168, 100, 2]);
    let (a, p) = parse_sockaddr_in(&b).expect("parse");
    assert_eq!(p, 4660);
    assert_eq!(a, Ipv4Address::new(192, 168, 100, 2));
}
