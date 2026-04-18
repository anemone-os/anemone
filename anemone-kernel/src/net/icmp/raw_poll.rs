//! One polling round for the ICMP raw socket (drain RX, enqueue echo replies).

use smoltcp::{socket::raw, time::Instant as SmolInstant};

use crate::prelude::*;

use super::{build_icmpv4_echo_reply, classify_icmpv4_echo_packet, IcmpEchoLimiter, IcmpEchoStats, IcmpRawClass};

/// Returns `(icmp_echo_rx_this_round, icmp_echo_tx_this_round, queued_reply_count_this_round)`.
pub(crate) fn poll_icmp_raw_socket(
    socket: &mut raw::Socket<'_>,
    stats: &mut IcmpEchoStats,
    limiter: &mut IcmpEchoLimiter,
    now: SmolInstant,
) -> (usize, usize, usize) {
    let mut icmp_echo_rx = 0usize;
    let mut icmp_echo_tx = 0usize;
    let mut queued_this_round = 0usize;

    while socket.can_recv() {
        let frame = match socket.recv() {
            Ok(frame) => frame.to_vec(),
            Err(_) => break,
        };
        match classify_icmpv4_echo_packet(&frame) {
            IcmpRawClass::BadParse => {
                stats.rx_parse_errors += 1;
            }
            IcmpRawClass::NotEchoRequest => {
                stats.rx_not_echo_request += 1;
            }
            IcmpRawClass::EchoRequest => {
                stats.rx_echo_requests += 1;
                icmp_echo_rx += 1;
                if !limiter.try_consume_token(now) {
                    stats.tx_rate_limited += 1;
                    continue;
                }
                if let Some(reply) = build_icmpv4_echo_reply(&frame) {
                    if socket.send_slice(&reply).is_ok() {
                        icmp_echo_tx += 1;
                        stats.tx_echo_replies_queued += 1;
                        queued_this_round += 1;
                    } else {
                        stats.tx_enqueue_errors += 1;
                        kerrln!("net: failed to enqueue icmp echo reply");
                    }
                } else {
                    stats.rx_parse_errors += 1;
                }
            }
        }
    }

    (icmp_echo_rx, icmp_echo_tx, queued_this_round)
}
