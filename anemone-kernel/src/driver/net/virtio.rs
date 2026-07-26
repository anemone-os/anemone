//! VirtIO-Net frame provider.

use core::{
    mem::size_of,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use anemone_net_api::{
    EthernetAddress, FrameCapabilities, FrameProvider, FrameSizeError, Instant, LinkState,
    ReceiveOutcome, RxToken, TransmitOutcome, TxToken,
};
use virtio_drivers::{
    Error as VirtIOError,
    device::net::{VirtIONetRaw, VirtioNetHdr},
    transport::{DeviceType, SomeTransport},
};

use crate::{
    device::{
        bus::virtio::VirtIODriver,
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
        net::{PublishError, PublishedNetdev, ReadyNetdev, publish},
    },
    driver::virtio::VirtIOHalImpl,
    prelude::*,
    utils::{any_opaque::AnyOpaque, identity::AnyIdentity},
};

const QUEUE_SIZE: usize = VIRTIO_NET_QUEUE_SIZE;
const BACKING_CAPACITY: usize = VIRTIO_NET_FRAME_CAPACITY_BYTES;
const HEADER_RESERVE: usize = size_of::<VirtioNetHdr>();
const FRAME_CAPACITY: usize = BACKING_CAPACITY - HEADER_RESERVE;
const RX_SLOT_COUNT: usize = QUEUE_SIZE / 2;
const TX_SLOT_COUNT: usize = QUEUE_SIZE / 2;

static_assert!(
    QUEUE_SIZE >= 4 && QUEUE_SIZE <= 1024,
    "VIRTIO_NET_QUEUE_SIZE must be in 4..=1024"
);
static_assert!(
    QUEUE_SIZE.is_power_of_two(),
    "VIRTIO_NET_QUEUE_SIZE must be a power of two"
);
static_assert!(
    BACKING_CAPACITY >= 1526,
    "VIRTIO_NET_FRAME_CAPACITY_BYTES must satisfy VirtIONetRaw's 1526-byte minimum"
);
static_assert!(
    BACKING_CAPACITY > HEADER_RESERVE,
    "VirtIO-Net backing must leave room for an Ethernet frame"
);

type RawNet = VirtIONetRaw<VirtIOHalImpl, SomeTransport<'static>, QUEUE_SIZE>;

/// Pure worker-wake capability installed by the kernel attach authority.
///
/// The IRQ path commits the provider-owned recheck predicate before invoking
/// this edge. Implementations must not read driver state or run protocol work.
pub(crate) trait VirtIONetRecheckWake: Send + Sync {
    fn wake(&self);
}

/// Diagnostic-only provider counters. They never drive queue or worker state.
#[derive(Clone, Copy, Debug)]
pub(crate) struct VirtIONetStats {
    rx_completions: usize,
    tx_submissions: usize,
    tx_completions: usize,
    irq_rechecks: usize,
    queue_full: usize,
    live_mappings: usize,
    mapping_high_water: usize,
}

impl VirtIONetStats {
    pub(crate) const fn rx_completions(self) -> usize {
        self.rx_completions
    }

    pub(crate) const fn tx_submissions(self) -> usize {
        self.tx_submissions
    }

    pub(crate) const fn tx_completions(self) -> usize {
        self.tx_completions
    }

    pub(crate) const fn irq_rechecks(self) -> usize {
        self.irq_rechecks
    }

    pub(crate) const fn queue_full(self) -> usize {
        self.queue_full
    }

    pub(crate) const fn live_mappings(self) -> usize {
        self.live_mappings
    }

    pub(crate) const fn mapping_high_water(self) -> usize {
        self.mapping_high_water
    }
}

struct VirtIONetDiagnostics {
    rx_completions: AtomicUsize,
    tx_submissions: AtomicUsize,
    tx_completions: AtomicUsize,
    irq_rechecks: AtomicUsize,
    queue_full: AtomicUsize,
    live_mappings: AtomicUsize,
    mapping_high_water: AtomicUsize,
}

impl VirtIONetDiagnostics {
    const fn new() -> Self {
        Self {
            rx_completions: AtomicUsize::new(0),
            tx_submissions: AtomicUsize::new(0),
            tx_completions: AtomicUsize::new(0),
            irq_rechecks: AtomicUsize::new(0),
            queue_full: AtomicUsize::new(0),
            live_mappings: AtomicUsize::new(0),
            mapping_high_water: AtomicUsize::new(0),
        }
    }

    fn mapping_opened(&self) {
        let live = self.live_mappings.fetch_add(1, Ordering::Relaxed) + 1;
        self.mapping_high_water.fetch_max(live, Ordering::Relaxed);
    }

    fn mapping_closed(&self) {
        let previous = self.live_mappings.fetch_sub(1, Ordering::Relaxed);
        assert!(previous > 0, "VirtIO-Net mapping counter underflow");
    }

    fn snapshot(&self) -> VirtIONetStats {
        VirtIONetStats {
            rx_completions: self.rx_completions.load(Ordering::Relaxed),
            tx_submissions: self.tx_submissions.load(Ordering::Relaxed),
            tx_completions: self.tx_completions.load(Ordering::Relaxed),
            irq_rechecks: self.irq_rechecks.load(Ordering::Relaxed),
            queue_full: self.queue_full.load(Ordering::Relaxed),
            live_mappings: self.live_mappings.load(Ordering::Relaxed),
            mapping_high_water: self.mapping_high_water.load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RxOwnership {
    Unqueued,
    Device { queue_token: u16 },
    Ready { frame_offset: usize, len: usize },
    Reserved { frame_offset: usize, len: usize },
    RequeuePending,
}

struct RxSlot {
    backing: Box<[u8]>,
    ownership: RxOwnership,
}

impl RxSlot {
    fn new() -> Self {
        Self {
            backing: vec![0; BACKING_CAPACITY].into_boxed_slice(),
            ownership: RxOwnership::Unqueued,
        }
    }

    fn reserve(&mut self) -> bool {
        let RxOwnership::Ready { frame_offset, len } = self.ownership else {
            return false;
        };
        self.ownership = RxOwnership::Reserved { frame_offset, len };
        true
    }

    fn cancel_reservation(&mut self) {
        let RxOwnership::Reserved { frame_offset, len } = self.ownership else {
            panic!("RX token cancellation requires a reserved slot")
        };
        self.ownership = RxOwnership::Ready { frame_offset, len };
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TxOwnership {
    Available,
    Reserved,
    Device { queue_token: u16, total_len: usize },
}

struct TxSlot {
    backing: Box<[u8]>,
    ownership: TxOwnership,
}

impl TxSlot {
    fn new() -> Self {
        Self {
            backing: vec![0; BACKING_CAPACITY].into_boxed_slice(),
            ownership: TxOwnership::Available,
        }
    }

    fn reserve(&mut self) -> bool {
        if self.ownership != TxOwnership::Available {
            return false;
        }
        self.ownership = TxOwnership::Reserved;
        true
    }

    fn cancel_reservation(&mut self) {
        assert_eq!(self.ownership, TxOwnership::Reserved);
        self.ownership = TxOwnership::Available;
    }
}

/// IRQ-shared device facts. Frame slots are deliberately absent: the provider
/// owns them exclusively, so protocol callbacks need no lock and cannot race
/// the IRQ path.
struct VirtIONetDevice {
    raw: SpinLock<RawNet>,
    /// Edge-only wake hint. Queue completion remains the durable truth.
    recheck_requested: AtomicBool,
    recheck_wake: spin::Once<Weak<dyn VirtIONetRecheckWake>>,
    /// Diagnostic-only mirrors of owner transitions; never behavior inputs.
    diagnostics: VirtIONetDiagnostics,
}

impl VirtIONetDevice {
    fn new(transport: SomeTransport<'static>) -> Result<(Arc<Self>, EthernetAddress), VirtIOError> {
        let raw = RawNet::new(transport)?;
        let mac = EthernetAddress::new(raw.mac_address());
        Ok((
            Arc::new(Self {
                raw: SpinLock::new(raw),
                recheck_requested: AtomicBool::new(false),
                recheck_wake: spin::Once::new(),
                diagnostics: VirtIONetDiagnostics::new(),
            }),
            mac,
        ))
    }

    fn submit_rx(&self, slot: &mut RxSlot) -> Result<(), VirtIOError> {
        assert!(matches!(
            slot.ownership,
            RxOwnership::Unqueued | RxOwnership::RequeuePending
        ));
        let mut raw = self.raw.lock_irqsave();

        // SAFETY: this exact stable boxed backing remains owned by `slot` and
        // inaccessible to CPU frame consumers until the matching queue token
        // is observed and `receive_complete` unshares it. On error no request
        // was committed, so the provider retains CPU ownership.
        let queue_token = unsafe { raw.receive_begin(&mut slot.backing)? };
        slot.ownership = RxOwnership::Device { queue_token };
        self.diagnostics.mapping_opened();
        Ok(())
    }

    fn submit_tx(&self, slot: &mut TxSlot, len: usize) -> Result<(), VirtIOError> {
        assert_eq!(slot.ownership, TxOwnership::Reserved);
        let mut raw = self.raw.lock_irqsave();
        let header_len = raw.fill_buffer_header(&mut slot.backing)?;
        if header_len != HEADER_RESERVE {
            slot.backing
                .copy_within(HEADER_RESERVE..HEADER_RESERVE + len, header_len);
        }
        let total_len = header_len + len;

        // SAFETY: the stable boxed prefix contains the initialized header and
        // frame. After the queue commit the slot becomes Device-owned and no
        // CPU path accesses it until the matching completion is harvested. An
        // error leaves the slot Reserved and therefore CPU-owned.
        let queue_token = unsafe { raw.transmit_begin(&slot.backing[..total_len])? };
        slot.ownership = TxOwnership::Device {
            queue_token,
            total_len,
        };
        self.diagnostics
            .tx_submissions
            .fetch_add(1, Ordering::Relaxed);
        self.diagnostics.mapping_opened();
        Ok(())
    }

    fn handle_irq(&self) {
        if !self.raw.lock_irqsave().ack_interrupt().is_empty() {
            self.recheck_requested.store(true, Ordering::Release);
            self.diagnostics
                .irq_rechecks
                .fetch_add(1, Ordering::Relaxed);
            if let Some(wake) = self.recheck_wake.get().and_then(Weak::upgrade) {
                wake.wake();
            }
        }
    }

    fn install_recheck_wake(&self, wake: Weak<dyn VirtIONetRecheckWake>) {
        assert!(
            self.recheck_wake.get().is_none(),
            "VirtIO-Net recheck wake installed twice"
        );
        self.recheck_wake.call_once(|| wake);
    }

    fn recheck_requested(&self) -> bool {
        self.recheck_requested.load(Ordering::Acquire)
    }

    fn take_recheck_requested(&self) -> bool {
        self.recheck_requested.swap(false, Ordering::AcqRel)
    }

    fn enable_interrupts(&self) {
        self.raw.lock_irqsave().enable_interrupts();
    }

    fn disable_interrupts(&self) {
        self.raw.lock_irqsave().disable_interrupts();
    }
}

/// Sole owner of frame slots and their queue-token lifecycle.
///
/// The IRQ handler only touches `device.raw` and an edge bit. Therefore the
/// exclusive `&mut FrameProvider` borrow is sufficient to expose one slot to a
/// protocol callback without a provider-global or IRQ-off guard.
pub(crate) struct VirtIONetProvider {
    // Declared first so pre-IRQ initialization failures drop RawNet (and unset
    // both queues) before releasing the following backing slots. Once an IRQ
    // is registered, R0 never drops this provider: publication failure retains
    // it, and success keeps it alive until power-off. Runtime removal must first
    // prevent Weak upgrades and quiesce/reset the queues before allowing drop.
    device: Arc<VirtIONetDevice>,
    rx_slots: Box<[RxSlot]>,
    tx_slots: Box<[TxSlot]>,
}

impl VirtIONetProvider {
    fn new(transport: SomeTransport<'static>) -> Result<(Self, EthernetAddress), VirtIOError> {
        let (device, mac) = VirtIONetDevice::new(transport)?;
        let mut provider = Self {
            device,
            rx_slots: (0..RX_SLOT_COUNT)
                .map(|_| RxSlot::new())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            tx_slots: (0..TX_SLOT_COUNT)
                .map(|_| TxSlot::new())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        };
        for slot in &mut provider.rx_slots {
            provider.device.submit_rx(slot)?;
        }
        Ok((provider, mac))
    }

    fn harvest_rx(&mut self) {
        let mut raw = self.device.raw.lock_irqsave();
        let Some(queue_token) = raw.poll_receive() else {
            return;
        };
        let slot = self
            .rx_slots
            .iter_mut()
            .find(|slot| slot.ownership == (RxOwnership::Device { queue_token }))
            .expect("VirtIO-Net RX completion must have one owner slot");

        // SAFETY: the queue token and exact backing were stored together by
        // `submit_rx`; the Device state denied CPU access until this point.
        // Completion performs HAL unshare/sync before the slot becomes Ready.
        let (frame_offset, len) = unsafe {
            raw.receive_complete(queue_token, &mut slot.backing)
                .unwrap_or_else(|error| panic!("VirtIO-Net RX completion failed: {error}"))
        };
        self.device.diagnostics.mapping_closed();
        self.device
            .diagnostics
            .rx_completions
            .fetch_add(1, Ordering::Relaxed);
        assert!(frame_offset + len <= slot.backing.len());
        slot.ownership = RxOwnership::Ready { frame_offset, len };
    }

    fn harvest_tx(&mut self) {
        loop {
            let mut raw = self.device.raw.lock_irqsave();
            let Some(queue_token) = raw.poll_transmit() else {
                return;
            };
            let slot = self
                .tx_slots
                .iter_mut()
                .find(|slot| {
                    matches!(
                        slot.ownership,
                        TxOwnership::Device {
                            queue_token: owned,
                            ..
                        } if owned == queue_token
                    )
                })
                .expect("VirtIO-Net TX completion must have one owner slot");
            let TxOwnership::Device { total_len, .. } = slot.ownership else {
                unreachable!()
            };

            // SAFETY: this is the same stable backing prefix and matching token
            // committed by `submit_tx`. Device ownership excluded CPU access;
            // HAL unshare completes before the slot returns to Available.
            unsafe {
                raw.transmit_complete(queue_token, &slot.backing[..total_len])
                    .unwrap_or_else(|error| panic!("VirtIO-Net TX completion failed: {error}"));
            }
            self.device.diagnostics.mapping_closed();
            self.device
                .diagnostics
                .tx_completions
                .fetch_add(1, Ordering::Relaxed);
            slot.ownership = TxOwnership::Available;
        }
    }

    pub(crate) fn install_recheck_wake(&self, wake: Weak<dyn VirtIONetRecheckWake>) {
        self.device.install_recheck_wake(wake);
    }

    pub(crate) fn recheck_requested(&self) -> bool {
        self.device.recheck_requested()
    }

    pub(crate) fn take_recheck_requested(&self) -> bool {
        self.device.take_recheck_requested()
    }

    pub(crate) fn stats(&self) -> VirtIONetStats {
        self.device.diagnostics.snapshot()
    }
}

pub(crate) struct VirtIORxToken<'a> {
    device: &'a VirtIONetDevice,
    slot: &'a mut RxSlot,
    consumed: bool,
}

impl RxToken for VirtIORxToken<'_> {
    fn consume<R, F>(mut self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        let RxOwnership::Reserved { frame_offset, len } = self.slot.ownership else {
            panic!("RX consume requires a reserved slot")
        };
        let result = f(&self.slot.backing[frame_offset..frame_offset + len]);
        self.slot.ownership = RxOwnership::RequeuePending;
        // The frame has been consumed and must not be restored as Ready if an
        // invariant-breaking refill error panics. Commit the token state first
        // so unwinding preserves the original failure instead of attempting a
        // second Reserved-only cancellation from Drop.
        self.consumed = true;
        self.device
            .submit_rx(self.slot)
            .unwrap_or_else(|error| panic!("VirtIO-Net RX refill failed: {error}"));
        result
    }
}

impl Drop for VirtIORxToken<'_> {
    fn drop(&mut self) {
        if !self.consumed {
            self.slot.cancel_reservation();
        }
    }
}

pub(crate) struct VirtIOTxToken<'a> {
    device: &'a VirtIONetDevice,
    slot: &'a mut TxSlot,
    consumed: bool,
}

impl TxToken for VirtIOTxToken<'_> {
    fn capacity(&self) -> usize {
        FRAME_CAPACITY
    }

    fn consume<R, F>(mut self, len: usize, f: F) -> Result<R, FrameSizeError>
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        if len > FRAME_CAPACITY {
            self.slot.cancel_reservation();
            self.consumed = true;
            return Err(FrameSizeError::new(len, FRAME_CAPACITY));
        }
        let result = f(&mut self.slot.backing[HEADER_RESERVE..HEADER_RESERVE + len]);
        self.device
            .submit_tx(self.slot, len)
            .unwrap_or_else(|error| {
                panic!("reserved VirtIO-Net TX descriptors disappeared before submit: {error}")
            });
        self.consumed = true;
        Ok(result)
    }
}

impl Drop for VirtIOTxToken<'_> {
    fn drop(&mut self) {
        if !self.consumed {
            self.slot.cancel_reservation();
        }
    }
}

impl FrameProvider for VirtIONetProvider {
    type RxToken<'a> = VirtIORxToken<'a>;
    type TxToken<'a> = VirtIOTxToken<'a>;

    fn receive(&mut self, _now: Instant) -> ReceiveOutcome<Self::RxToken<'_>, Self::TxToken<'_>> {
        self.harvest_tx();
        self.harvest_rx();
        let Some(rx_index) = self.rx_slots.iter_mut().position(RxSlot::reserve) else {
            return ReceiveOutcome::Empty;
        };
        if !self.device.raw.lock_irqsave().can_send() {
            self.device
                .diagnostics
                .queue_full
                .fetch_add(1, Ordering::Relaxed);
            self.rx_slots[rx_index].cancel_reservation();
            return ReceiveOutcome::TransmitExhausted;
        }
        let Some(tx_index) = self.tx_slots.iter_mut().position(TxSlot::reserve) else {
            self.device
                .diagnostics
                .queue_full
                .fetch_add(1, Ordering::Relaxed);
            self.rx_slots[rx_index].cancel_reservation();
            return ReceiveOutcome::TransmitExhausted;
        };

        ReceiveOutcome::Ready {
            rx: VirtIORxToken {
                device: &self.device,
                slot: &mut self.rx_slots[rx_index],
                consumed: false,
            },
            tx: VirtIOTxToken {
                device: &self.device,
                slot: &mut self.tx_slots[tx_index],
                consumed: false,
            },
        }
    }

    fn transmit(&mut self, _now: Instant) -> TransmitOutcome<Self::TxToken<'_>> {
        self.harvest_tx();
        if !self.device.raw.lock_irqsave().can_send() {
            self.device
                .diagnostics
                .queue_full
                .fetch_add(1, Ordering::Relaxed);
            return TransmitOutcome::Exhausted;
        }
        let Some(index) = self.tx_slots.iter_mut().position(TxSlot::reserve) else {
            self.device
                .diagnostics
                .queue_full
                .fetch_add(1, Ordering::Relaxed);
            return TransmitOutcome::Exhausted;
        };
        TransmitOutcome::Ready(VirtIOTxToken {
            device: &self.device,
            slot: &mut self.tx_slots[index],
            consumed: false,
        })
    }

    fn capabilities(&self) -> FrameCapabilities {
        FrameCapabilities {
            max_frame_len: FRAME_CAPACITY,
        }
    }

    fn link_state(&self) -> LinkState {
        // VirtIONetRaw does not expose a durable negotiated link-status fact.
        // Unknown is honest and does not prevent publication or attach.
        LinkState::Unknown
    }
}

#[derive(Opaque)]
struct VirtIONetState {
    /// Non-owning shutdown capability. The provider is the only durable strong
    /// owner so driver state cannot keep RawNet alive beyond its slot backing.
    device: Weak<VirtIONetDevice>,
    /// One-shot typed capability, not a second lifecycle truth. Checkpoint 4
    /// moves it to the attach authority before any protocol callback runs.
    published: SpinLock<Option<PublishedNetdev<VirtIONetProvider>>>,
}

#[derive(Opaque)]
struct VirtIONetIrq {
    /// Non-owning IRQ capability. See `VirtIONetProvider` for the boot-only
    /// lifetime rule that prevents an upgrade from racing provider teardown.
    device: Weak<VirtIONetDevice>,
}

#[derive(Debug, KObject, Driver)]
struct VirtIONetDriver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

impl KObjectOps for VirtIONetDriver {}

impl DriverOps for VirtIONetDriver {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        let vdev = device
            .as_virtio_device()
            .expect("VirtIO-Net driver must only receive a VirtIO device");
        let transport = vdev.take_transport().ok_or(SysError::ProbeFailed)?;
        let (provider, mac) = VirtIONetProvider::new(transport).map_err(|error| {
            kerrln!("failed to initialize VirtIO-Net queues: {error}");
            SysError::ProbeFailed
        })?;
        let device = Arc::downgrade(&provider.device);

        vdev.request_irq(
            &IRQ_HANDLER,
            Some(AnyOpaque::new(VirtIONetIrq {
                device: device.clone(),
            })),
        )?;
        provider.device.enable_interrupts();

        let origin = AnyIdentity::try_from(vdev.name())
            .expect("VirtIO device name must fit the kernel identity limit");
        let ready = ReadyNetdev::new(
            origin,
            Some(mac),
            provider.capabilities(),
            provider.link_state(),
            provider,
        );
        let published = publish(ready).map_err(|(error, ready)| {
            // IRQ registration is not removable. Retain the ready provider so
            // its RX mappings/backing stay valid until reset instead of
            // freeing memory that the device may still access. Suppress further
            // queue notifications first so the failed device remains inert.
            if let Some(device) = device.upgrade() {
                device.disable_interrupts();
            }
            core::mem::forget(ready);
            match error {
                PublishError::DuplicateOrigin => {
                    kerrln!("VirtIO-Net device {} was published twice", vdev.name());
                },
                PublishError::IdentityExhausted => {
                    kerrln!("network-device identity space exhausted");
                },
                PublishError::NameTooLong => {
                    kerrln!("network-device name exceeded its capacity");
                },
            }
            SysError::ProbeFailed
        })?;
        let snapshot = published.snapshot();
        kinfoln!(
            "VirtIO-Net {} published as {} (ifindex {}, MAC {:?}, frame capacity {})",
            vdev.name(),
            snapshot.name(),
            snapshot.ifindex(),
            snapshot.facts().ethernet_address,
            snapshot.facts().max_frame_len,
        );

        vdev.set_drv_state(AnyOpaque::new(VirtIONetState {
            device,
            published: SpinLock::new(Some(published)),
        }));
        Ok(())
    }

    fn shutdown(&self, device: &dyn Device) {
        let state = device
            .drv_state()
            .cast::<VirtIONetState>()
            .expect("VirtIO-Net device must carry VirtIONetState");
        if let Some(device) = state.device.upgrade() {
            device.disable_interrupts();
        }
    }

    fn as_virtio_driver(&self) -> Option<&dyn VirtIODriver> {
        Some(self)
    }
}

impl VirtIODriver for VirtIONetDriver {
    fn id_table(&self) -> &'static [usize] {
        &[DeviceType::Network as usize]
    }
}

static IRQ_HANDLER: IrqHandler = IrqHandler::new(irq_handler);

static VIRTIO_NET_DRIVER: Lazy<Arc<VirtIONetDriver>> = Lazy::new(|| {
    Arc::new(VirtIONetDriver {
        kobj_base: KObjectBase::new(KObjIdent::try_from("virtio-net").unwrap()),
        drv_base: DriverBase::new(),
    })
});

pub(crate) fn take_published_netdevs() -> Vec<PublishedNetdev<VirtIONetProvider>> {
    let mut published = Vec::new();
    let driver: &dyn Driver = VIRTIO_NET_DRIVER.as_ref();
    driver.for_each_device(|device| {
        let state = device
            .drv_state()
            .cast::<VirtIONetState>()
            .expect("VirtIO-Net device must carry VirtIONetState");
        if let Some(netdev) = state.published.lock().take() {
            published.push(netdev);
        }
    });
    published
}

fn irq_handler(prv_data: &AnyOpaque) {
    let irq = prv_data
        .cast::<VirtIONetIrq>()
        .expect("VirtIO-Net IRQ must carry VirtIONetIrq");
    if let Some(device) = irq.device.upgrade() {
        device.handle_irq();
    }
}

#[initcall(driver)]
fn init() {
    bus::virtio::register_driver(VIRTIO_NET_DRIVER.clone());
}

#[kunit]
fn owner_local_slot_cancellation_restores_the_prior_state() {
    let mut rx = RxSlot::new();
    rx.ownership = RxOwnership::Ready {
        frame_offset: 12,
        len: 64,
    };
    assert!(rx.reserve());
    rx.cancel_reservation();
    assert_eq!(
        rx.ownership,
        RxOwnership::Ready {
            frame_offset: 12,
            len: 64
        }
    );

    let mut tx = TxSlot::new();
    assert!(tx.reserve());
    tx.cancel_reservation();
    assert_eq!(tx.ownership, TxOwnership::Available);
}
