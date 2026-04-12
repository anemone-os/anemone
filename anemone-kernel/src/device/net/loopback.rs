//! Software loopback network device (`lo`).
//!
//! Presents an Ethernet medium to smoltcp: TX frames are queued and surfaced on RX
//! (`send_raw` → `try_recv_frame`), matching [`super::NetPhyIo`] (see workspace
//! `anemone-kernel/docs/NETWORK_ROADMAP.md` §2.3).

use alloc::{collections::VecDeque, vec::Vec};

use smoltcp::phy::{DeviceCapabilities, Medium};

use super::{LinkState, NetDev, NetDevClass, NetPhyIo};

use crate::prelude::*;

/// Fallback L2 address when [`LoopbackNetDev::mac`] is `None` (see [`super::NetDev`]).
pub const LOOPBACK_MAC: [u8; 6] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

const QUEUE_CAP: usize = 32;
const MAX_FRAME_LEN: usize = 2048;

struct LoopbackInner {
    queue: VecDeque<Vec<u8>>,
}

impl LoopbackInner {
    fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }
}

/// Kernel loopback netdev; register as [`NetDevClass::Loopback`] for name `lo`.
pub struct LoopbackNetDev {
    inner: Arc<SpinLock<LoopbackInner>>,
}

impl LoopbackNetDev {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(SpinLock::new(LoopbackInner::new())),
        }
    }
}

struct LoopbackPhy<'a> {
    inner: &'a mut LoopbackInner,
}

impl NetPhyIo for LoopbackPhy<'_> {
    fn try_recv_frame(&mut self) -> Option<Vec<u8>> {
        self.inner.queue.pop_front()
    }

    fn can_send(&self) -> bool {
        self.inner.queue.len() < QUEUE_CAP
    }

    fn send_raw(&mut self, frame: &[u8]) -> Result<(), ()> {
        if frame.is_empty() || frame.len() > MAX_FRAME_LEN {
            return Err(());
        }
        if self.inner.queue.len() >= QUEUE_CAP {
            return Err(());
        }
        self.inner.queue.push_back(frame.to_vec());
        Ok(())
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1514;
        caps.medium = Medium::Ethernet;
        caps
    }

    fn ack_interrupt(&mut self) {}

    fn disable_interrupts(&mut self) {}
}

impl NetDev for LoopbackNetDev {
    fn class(&self) -> NetDevClass {
        NetDevClass::Loopback
    }

    fn mac(&self) -> Option<[u8; 6]> {
        None
    }

    fn mtu(&self) -> usize {
        1500
    }

    fn link_state(&self) -> LinkState {
        LinkState::Up
    }

    fn with_phy_mut(&self, f: &mut dyn FnMut(&mut dyn NetPhyIo)) {
        let mut guard = self.inner.lock_irqsave();
        let mut phy = LoopbackPhy {
            inner: &mut *guard,
        };
        f(&mut phy);
    }
}
