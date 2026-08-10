pub mod clock;
pub mod itimer;
pub mod posix_timer;
pub mod timer;

mod hal;
pub use hal::*;
mod api;
pub use api::*;
mod timekeeper;
pub use timekeeper::*;
mod instant;
pub use instant::{MonotonicInstant, RealtimeInstant};

pub fn on_timer_interrupt() {
    timekeeper::on_timer_interrupt();
    timer::on_timer_interrupt();
}
