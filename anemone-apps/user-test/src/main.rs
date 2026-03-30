#![no_std]
#![no_main]
#![warn(unused)]

use core::panic::PanicInfo;

use anemone_abi::{
    mm::brk,
    process::{exit, sched_yield},
    syscall::syscall,
};

#[unsafe(no_mangle)]
pub fn _start() {
    unsafe {
        let str = c"This is task 1 by kako_!".as_ptr() as *mut u8;
        syscall(100, str as u64, 0, 0, 0, 0, 0);
    }
    // memory test
    let top = 0x40000000;
    let mb = 0x200000;
    sched_yield().unwrap();
    if let Err(_) = brk(top + 100 * mb) {
        exit(-1);
    }
    exit(0);
}

#[panic_handler]
pub fn panic_handler(_info: &PanicInfo) -> ! {
    loop {}
}
