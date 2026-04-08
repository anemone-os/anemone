mod hal;
pub use hal::*;

pub mod timer;

mod api;

mod timekeeper;
pub use timekeeper::{duration_to_ticks, program_first_timer, set_boot_mono, ticks, uptime};

pub fn on_timer_interrupt() {
    use crate::prelude::*;
    //kdebugln!("current time {} ticks {}", uptime().as_secs(), ticks());

    timekeeper::on_timer_interrupt();
    timer::on_timer_interrupt();
}
