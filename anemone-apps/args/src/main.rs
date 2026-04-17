#![no_std]
#![no_main]

use anemone_rs::{
    env::{args, envs, exec_fn},
    prelude::*,
};

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    let args = args();
    let env = envs();

    println!("args:");
    for (idx, arg) in args.enumerate() {
        println!("\targ[{idx}] = {arg}");
    }

    println!("envs:");
    for (idx, (key, value)) in env.enumerate() {
        println!("\tenv[{idx}] = {key}={value}");
    }

    {
        // aux
        println!("execfn: {:?}", exec_fn());
    }

    Ok(())
}
