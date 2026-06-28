use core::{self, panic::PanicInfo};

use crate::{eprintln, process::process_id};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    // crate::eprintln!("panic: {}", info);
    eprintln!("process {} panicked: {}", process_id(), info.message());
    if let Some(location) = info.location() {
        eprintln!("\t at {}:{}", location.file(), location.line());
    }
    crate::os::linux::process::exit(1)
}
