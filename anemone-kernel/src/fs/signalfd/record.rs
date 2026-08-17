use anemone_abi::fs::linux::signalfd::SignalFdSigInfo;

use crate::task::sig::Signal;

pub(super) fn from_signal(signal: &Signal) -> SignalFdSigInfo {
    signal.to_signalfd_siginfo()
}

#[cfg(feature = "kunit")]
mod kunits {
    use core::mem::{align_of, size_of};

    use anemone_abi::{
        fs::linux::signalfd::SignalFdSigInfo,
        process::linux::signal::{SI_QUEUE, SI_TIMER, SIGRTMIN},
    };
    use zerocopy::IntoBytes as _;

    use super::*;
    use crate::{
        prelude::*,
        task::{
            Tid, Uid,
            sig::{
                SigNo,
                info::{SiCode, SigChld, SigFault, SigInfoFields, SigKill, SigRt, SigTimer},
            },
        },
    };

    fn assert_zero_tail(info: &SignalFdSigInfo) {
        assert_eq!(info.fd, 0);
        assert_eq!(info.band, 0);
        assert_eq!(info.trapno, 0);
        assert_eq!(info.addr_lsb, 0);
        assert_eq!(info.syscall, 0);
        assert_eq!(info.call_addr, 0);
        assert_eq!(info.arch, 0);
        assert_eq!(info.__pad, [0; 28]);
    }

    #[kunit]
    fn signalfd_siginfo_layout_and_zero_initialization() {
        assert_eq!(size_of::<SignalFdSigInfo>(), 128);
        assert_eq!(align_of::<SignalFdSigInfo>(), 8);
        assert!(
            SignalFdSigInfo::default()
                .as_bytes()
                .iter()
                .all(|byte| *byte == 0)
        );
    }

    #[kunit]
    fn signalfd_projects_sender_and_realtime_fields() {
        let kill = from_signal(&Signal::new(
            SigNo::SIGUSR1,
            SiCode::User,
            SigInfoFields::Kill(SigKill {
                pid: Tid::new(41),
                uid: Uid::new(7),
            }),
        ));
        assert_eq!(
            (kill.signo, kill.pid, kill.uid),
            (SigNo::SIGUSR1.as_usize() as u32, 41, 7)
        );
        assert_zero_tail(&kill);

        let rt = from_signal(&Signal::new(
            SigNo::new(SIGRTMIN as usize),
            SiCode::Queue,
            SigInfoFields::Rt(SigRt {
                pid: Tid::new(42),
                uid: Uid::new(8),
                sigval: 0xfeed_beef_dead_cafe,
            }),
        ));
        assert_eq!(rt.code, SI_QUEUE);
        assert_eq!(rt.ptr, 0xfeed_beef_dead_cafe);
        assert_eq!(rt.int, 0xdead_cafe_u32 as i32);
    }

    #[kunit]
    fn signalfd_projects_timer_child_and_fault_fields() {
        let timer = from_signal(&Signal::new(
            SigNo::SIGALRM,
            SiCode::Timer,
            SigInfoFields::Timer(SigTimer {
                tid: 9,
                overrun: 3,
                sigval: 0x1234_5678_9abc_def0,
                sys_private: 0,
            }),
        ));
        assert_eq!(timer.code, SI_TIMER);
        assert_eq!((timer.tid, timer.overrun), (9, 3));
        assert_eq!(timer.ptr, 0x1234_5678_9abc_def0);

        let child = from_signal(&Signal::new(
            SigNo::SIGCHLD,
            SiCode::ChldExited,
            SigInfoFields::Chld(SigChld {
                pid: Tid::new(51),
                uid: Uid::new(11),
                status: 23,
                utime: 101,
                stime: 202,
            }),
        ));
        assert_eq!((child.pid, child.uid, child.status), (51, 11, 23));
        assert_eq!((child.utime, child.stime), (101, 202));

        let fault = from_signal(&Signal::new(
            SigNo::SIGSEGV,
            SiCode::Kernel,
            SigInfoFields::Fault(SigFault {
                addr: VirtAddr::new(0x1234),
            }),
        ));
        assert_eq!(fault.addr, 0x1234);
    }
}
