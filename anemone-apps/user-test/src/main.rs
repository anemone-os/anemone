#![no_std]
#![no_main]

use core::panic::PanicInfo;

use anemone_abi::syscall::syscall;

#[unsafe(no_mangle)]
pub fn _start() {
    unsafe {
        syscall(114514, 0, 0, 0, 0, 0, 0);
    }
    loop {}
}

#[panic_handler]
pub fn panic_handler(_info: &PanicInfo) -> ! {
    loop {}
}
