pub mod clock;
pub mod timer;

mod hal;
pub use hal::*;
mod api;
pub use api::*;
mod timekeeper;
pub use timekeeper::*;
mod instant;
pub use instant::Instant;

pub fn on_timer_interrupt() {
    timekeeper::on_timer_interrupt();
    timer::on_timer_interrupt();
}
