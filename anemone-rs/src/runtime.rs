use core::{self, panic::PanicInfo};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    crate::println!("panic: {info:?}");
    crate::os::linux::process::exit(1)
}
