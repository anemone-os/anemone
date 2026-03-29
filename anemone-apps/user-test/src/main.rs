#![no_std]
#![no_main]

use core::panic::PanicInfo;

use anemone_abi::{
    mm::brk,
    process::{exit, sched_yield},
    syscall::syscall,
};

#[unsafe(no_mangle)]
pub fn _start() {
    unsafe {
        let str = c"hello_world".as_ptr();
        syscall(100, str as u64, 0, 0, 0, 0, 0);
    }
    // memory test
    let top = 0x40000000;
    let mb = 0x200000;
    for i in 0..10000 {
        sched_yield().unwrap();
        if let Err(_) = brk(top + i * mb) {
            exit(-1);
        }
    }
    exit(0);
}

#[panic_handler]
pub fn panic_handler(_info: &PanicInfo) -> ! {
    loop {}
}
