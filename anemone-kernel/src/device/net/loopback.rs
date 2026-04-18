//! Software loopback network device (`lo`).
//!
//! Presents an Ethernet-shaped L2 medium: TX frames are queued and surfaced on RX.

use alloc::{collections::VecDeque, vec::Vec};

use super::{LinkState, NetDev, NetDevClass, NetPhyIo, PhyCapabilities, PhyMedium};

use crate::prelude::*;

/// Dummy Ethernet MAC when `LoopbackNetDev::mac()` is `None`; not a real NIC address.
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

    fn capabilities(&self) -> PhyCapabilities {
        PhyCapabilities {
            max_transmission_unit: 1514,
            medium: PhyMedium::Ethernet,
        }
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
