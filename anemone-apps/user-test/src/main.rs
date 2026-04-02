#![no_std]
#![no_main]
#![warn(unused)]

use anemone_rs::prelude::*;

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    let args: Vec<&str> = args().collect();
    if args.len() < 2 {
        println!("usage: user-test [running times...]");
        return Err(-1);
    }
    let program = args[0];
    let first_arg = args[1];
    let running_times: u32 = first_arg.parse().unwrap_or_else(|e| {
        println!(
            "failed to parse first argument as number: {}, error: {:?}",
            first_arg, e
        );
        exit(-1);
    });
    println!("user-test: running times = {}", running_times);
    if running_times < 30 {
        execve(program, &[program, &format!("{}", running_times + 1)]).unwrap();
    } else {
        println!(
            "user-test: finished running {} times, exiting...",
            running_times
        );
    }

    Ok(())
}
