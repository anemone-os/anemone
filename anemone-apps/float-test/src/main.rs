#![no_std]
#![no_main]

use anemone_rs::{env::args, prelude::*};

pub mod common;
pub mod sig;

// ============================================================
// Main
// ============================================================

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    println!("===== float test version 5 =====");
    let mut iter = args();
    iter.next(); // skip program name
    while let Some(next) = iter.next() {
        if next == "--type" || next == "-t" {
            if let Some(val) = iter.next() {
                match val {
                    "common" => common::run_seed_range(1, 1),
                    "sig" => sig::run(),
                    _ => panic!("unexpected test type: {}", val),
                }
            } else {
                panic!("--seed argument provided but no seed value found");
            }
        } else {
            panic!("unexpected argument: {}", next);
        }
    }

    Ok(())
}
