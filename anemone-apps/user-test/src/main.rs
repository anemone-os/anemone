#![no_std]
#![no_main]
#![warn(unused)]

use user_lib::{self as _, args, println, proc::execve};

#[unsafe(no_mangle)]
pub fn main() -> i32 {
    for str in args() {
        println!("received arg: {}", str);
    }
    execve(c"/user-test", &[c"abcde"]).unwrap();
    return 0;
}
