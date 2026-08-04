#![no_std]
#![no_main]

mod icmp_raw;
mod udp;
mod udp_extension;
mod udp_message;
mod unix;
mod unix_seqpacket;

use anemone_rs::{env, prelude::*};

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    let mut args = env::args();
    let _program = args.next();
    match args.next() {
        Some("--udp-cloexec-child") => {
            let fd = args.next().ok_or(EINVAL)?;
            if args.next().is_some() {
                return Err(EINVAL);
            }
            udp::run_cloexec_child(fd)
        },
        Some("--icmp-raw-cloexec-child") => {
            let fd = args.next().ok_or(EINVAL)?;
            if args.next().is_some() {
                return Err(EINVAL);
            }
            icmp_raw::run_cloexec_child(fd)
        },
        None => {
            let udp_result = udp::run();
            let udp_extension_result = udp_extension::run();
            let udp_message_result = udp_message::run();
            let unix_result = unix::run();
            let seqpacket_result = unix_seqpacket::run();
            let icmp_raw_result = icmp_raw::run();
            udp_result
                .and(udp_extension_result)
                .and(udp_message_result)
                .and(unix_result)
                .and(seqpacket_result)
                .and(icmp_raw_result)
        },
        Some(_) => Err(EINVAL),
    }
}
