mod ppoll;
mod pselect6;

mod wait;

pub(in crate::fs) use wait::finish_temporary_iomux_wait;

mod args {
    use crate::prelude::*;
    use anemone_abi::fs::linux::poll::*;

    bitflags! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct LinuxPollEvent: i16 {
            const IN = POLLIN;
            const PRI = POLLPRI;
            const OUT = POLLOUT;
            const ERR = POLLERR;
            const HUP = POLLHUP;
            const NVAL = POLLNVAL;
            const RDNORM = POLLRDNORM;
            const RDBAND = POLLRDBAND;
            const WRNORM = POLLWRNORM;
            const RDHUP = POLLRDHUP;
        }
    }

    impl LinuxPollEvent {
        pub fn from_kernel_poll_event(events: PollEvent, interests: Self) -> Self {
            let mut linux_events = LinuxPollEvent::empty();

            if events.contains(PollEvent::READABLE) {
                linux_events |= interests & (LinuxPollEvent::IN | LinuxPollEvent::RDNORM);
            }
            if events.contains(PollEvent::WRITABLE) {
                linux_events |= interests & (LinuxPollEvent::OUT | LinuxPollEvent::WRNORM);
            }
            if events.contains(PollEvent::ERROR) {
                linux_events |= LinuxPollEvent::ERR;
            }
            if events.contains(PollEvent::HANG_UP) {
                linux_events |= LinuxPollEvent::HUP;
            }
            if events.contains(PollEvent::READ_HANG_UP) {
                linux_events |= interests & LinuxPollEvent::RDHUP;
            }

            linux_events
        }
    }
}
use args::*;
use wait::*;
