pub mod ppoll;

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
        }
    }

    impl LinuxPollEvent {
        pub fn from_kernel_poll_event(events: PollEvent) -> Self {
            let mut linux_events = LinuxPollEvent::empty();

            if events.contains(PollEvent::READABLE) {
                linux_events |= LinuxPollEvent::IN;
            }
            if events.contains(PollEvent::WRITABLE) {
                linux_events |= LinuxPollEvent::OUT;
            }
            if events.contains(PollEvent::ERROR) {
                linux_events |= LinuxPollEvent::ERR;
            }
            if events.contains(PollEvent::HANG_UP) {
                linux_events |= LinuxPollEvent::HUP;
            }

            linux_events
        }
    }
}
use args::*;
