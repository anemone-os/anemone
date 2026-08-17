use crate::{prelude::*, time::MonotonicInstant};

use super::{
    phy::PhyState,
    protocol::{DmaStatus, EnhancedDescriptor, device_cause},
    regs::{Dwmac1000Regs, ProbeStartSnapshot, QuiesceSnapshot, RuntimeRegisterSnapshot},
    ring::{
        DescriptorSnapshot, Dwmac1000Rings, RingError, RxDiagnosticSnapshot, RxReservation,
        TxCompletion,
    },
};

// Linux records ETI statistically and only W1C-clears TU; neither produces a
// hard-error return. Either may legitimately be the only constituent with AIS.
const NONFATAL_ABNORMAL_CONSTITUENTS: u32 =
    DmaStatus::TX_EARLY.bits() | DmaStatus::TX_UNAVAILABLE.bits();
// Match Linux's legacy DWMAC1000 RX/TX and abnormal-cause admission,
// but do not write this mask to CSR7 during Gate 2. It only classifies the raw
// CSR5 samples consumed by the polling characterization.
const PROBE_CAUSE_ADMISSION: u32 = DmaStatus::NORMAL.bits()
    | DmaStatus::ABNORMAL.bits()
    | DmaStatus::FATAL_BUS_ERROR.bits()
    | DmaStatus::TX_UNDERFLOW.bits()
    | DmaStatus::RX.bits()
    | DmaStatus::TX.bits();
const EXPECTED_CAUSES: u32 = DmaStatus::RX.bits() | DmaStatus::TX.bits();
const ABNORMAL_CONSTITUENTS: u32 = DmaStatus::FATAL_BUS_ERROR.bits()
    | DmaStatus::RX_WATCHDOG.bits()
    | DmaStatus::RX_STOPPED.bits()
    | DmaStatus::RX_UNAVAILABLE.bits()
    | DmaStatus::TX_UNDERFLOW.bits()
    | DmaStatus::RX_OVERFLOW.bits()
    | DmaStatus::TX_JABBER.bits()
    | DmaStatus::TX_STOPPED.bits();
const QUIESCE_RECOVERABLE_CAUSES: u32 = DmaStatus::RX_STOPPED.bits() | DmaStatus::TX_STOPPED.bits();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CharacterizationResult {
    Passed,
    Failed(CharacterizationFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CharacterizationFailure {
    Timeout,
    Descriptor,
    InterruptEnabled,
    AbnormalCause,
    UnclearedCause,
    Quiesce,
    Register,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct CharacterizationSnapshot {
    pub(super) result: CharacterizationResult,
    pub(super) legal: u32,
    pub(super) observed: u32,
    pub(super) abnormal: u32,
    pub(super) uncleared: u32,
    pub(super) poll_count: u32,
    pub(super) w1c_samples: u32,
    pub(super) interrupt_enable: u32,
    pub(super) start: ProbeStartSnapshot,
    pub(super) tx_published: EnhancedDescriptor,
    pub(super) descriptor: DescriptorSnapshot,
    pub(super) payload_match: bool,
    pub(super) quiesce: QuiesceSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProbeDisposition {
    BindRetained,
    ReturnFailure,
    FailStop,
}

impl CharacterizationSnapshot {
    pub(super) const fn early_tx(&self) -> bool {
        self.legal & DmaStatus::TX_EARLY.bits() != 0
    }

    pub(super) const fn disposition(&self) -> ProbeDisposition {
        match (self.result, self.quiesce.stopped) {
            (CharacterizationResult::Passed, true) => ProbeDisposition::BindRetained,
            (CharacterizationResult::Failed(_), true) => ProbeDisposition::ReturnFailure,
            _ => ProbeDisposition::FailStop,
        }
    }
}

#[derive(Opaque)]
pub(super) struct Dwmac1000State {
    pub(super) owner: Arc<Dwmac1000Owner>,
    /// Gate 3's runtime context is installed exactly once after the Gate 2
    /// owner is retained; it is a lifecycle capability, not a second owner.
    pub(super) runtime: SpinLock<Option<Arc<super::irq::Dwmac1000IrqContext>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CauseClassification {
    legal: u32,
    expected: u32,
    abnormal: u32,
}

const fn classify_causes(status: u32, admitted: u32, recoverable: u32) -> CauseClassification {
    let legal = status & DmaStatus::W1C.bits();
    let admitted_causes = device_cause(status, admitted);
    let raw_abnormal_constituents = legal & ABNORMAL_CONSTITUENTS;
    let raw_nonfatal_constituents = legal & NONFATAL_ABNORMAL_CONSTITUENTS;
    let abnormal_constituents = raw_abnormal_constituents & !recoverable;
    let summary_is_recoverable =
        (raw_abnormal_constituents | raw_nonfatal_constituents) != 0 && abnormal_constituents == 0;
    let abnormal_summary = if legal & DmaStatus::ABNORMAL.bits() != 0 && !summary_is_recoverable {
        DmaStatus::ABNORMAL.bits()
    } else {
        0
    };
    CauseClassification {
        legal,
        expected: admitted_causes & EXPECTED_CAUSES,
        // CSR7 stays zero in Gate 2, so classification cannot depend on an
        // interrupt summary bit accompanying each raw constituent. During
        // quiesce, TPS/RPS are expected consequences of clearing ST/SR. AIS
        // is recoverable only when it actually summarizes constituents that
        // this phase permits and no fatal constituent is present.
        abnormal: abnormal_constituents | abnormal_summary,
    }
}

/// Long-lived Gate 2 owner retained by the bound platform device after a
/// successful polling characterization. Gate 3 must extend this same owner
/// with IRQ/runtime capability instead of rebuilding MMIO or DMA state.
pub(super) struct Dwmac1000Owner {
    regs: Arc<Dwmac1000Regs>,
    rings: SpinLock<Dwmac1000Rings>,
    /// Accepted PHY snapshot retained for Gate 3 in-place adoption.
    phy: PhyState,
    /// Diagnostic-only characterization snapshot for shutdown logging. Probe
    /// disposition is decided from the returned snapshot, never from this copy.
    result: SpinLock<Option<CharacterizationResult>>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct IrqServiceSnapshot {
    pub(super) csr5_raw: u32,
    pub(super) mac_status: u32,
    pub(super) mac_status_after: u32,
    pub(super) csr5_raw_after: u32,
    pub(super) csr5: u32,
    pub(super) csr5_after: u32,
}

impl Dwmac1000Owner {
    pub(super) fn new(regs: Arc<Dwmac1000Regs>, rings: Dwmac1000Rings, phy: PhyState) -> Arc<Self> {
        Arc::new(Self {
            regs,
            rings: SpinLock::new(rings),
            phy,
            result: SpinLock::new(None),
        })
    }

    pub(super) fn result(&self) -> Option<CharacterizationResult> {
        *self.result.lock_irqsave()
    }

    pub(super) fn phy(&self) -> PhyState {
        self.phy
    }

    pub(super) fn publication_link_state(&self) -> anemone_net_api::LinkState {
        // Gate 2 retained this resolved PHY snapshot; Gate 3 publishes that
        // same owner fact instead of inventing a second visible link state.
        if matches!(self.phy.link.speed_mbps, 10 | 100 | 1000) {
            anemone_net_api::LinkState::Up
        } else {
            anemone_net_api::LinkState::Down
        }
    }

    pub(super) fn characterize(&self) -> CharacterizationSnapshot {
        let probe_start = self.regs.start_probe();
        let tx_published = if probe_start.linux_sequence_valid() {
            // Linux publishes TX ownership only after MAC and both DMA paths
            // are running, then writes CSR1 to demand descriptor polling.
            let descriptor = self.rings.lock_irqsave().publish_probe_tx();
            self.regs.demand_tx();
            descriptor
        } else {
            self.rings.lock_irqsave().probe_snapshot().tx
        };

        let start = MonotonicInstant::now();
        let mut failure = if probe_start.linux_sequence_valid() {
            None
        } else {
            Some(CharacterizationFailure::Register)
        };
        let mut legal = 0;
        let mut observed = 0;
        let mut abnormal = 0;
        let mut uncleared = 0;
        let mut poll_count = 0u32;
        let mut w1c_samples = 0u32;
        let mut interrupt_enable = 0;
        let mut status = self.regs.status();
        loop {
            poll_count = poll_count.saturating_add(1);
            interrupt_enable |= self.regs.interrupt_enable();
            if interrupt_enable != 0 {
                failure = Some(CharacterizationFailure::InterruptEnabled);
            }
            let descriptor = self.rings.lock_irqsave().probe_snapshot();
            let classification = classify_causes(status, PROBE_CAUSE_ADMISSION, 0);
            if classification.legal != 0 {
                legal |= classification.legal;
                observed |= classification.expected;
                abnormal |= classification.abnormal;
                status = self.regs.acknowledge_causes(classification.legal);
                w1c_samples = w1c_samples.saturating_add(1);
            } else {
                status = self.regs.status();
            }
            // Treat W1C readback as the next raw sample. A newly arriving
            // event may legitimately reuse NIS/AIS and must not be called an
            // uncleared copy of the event just acknowledged.

            if failure.is_some() {
                break;
            }
            if abnormal != 0 {
                failure = Some(CharacterizationFailure::AbnormalCause);
                break;
            }
            if observed & EXPECTED_CAUSES == EXPECTED_CAUSES
                && descriptor.tx_complete()
                && descriptor.rx_complete()
                && status & DmaStatus::W1C.bits() == 0
            {
                if !descriptor.probe_layout_valid() {
                    failure = Some(CharacterizationFailure::Descriptor);
                }
                break;
            }
            if start.elapsed() >= Duration::from_millis(DWMAC1000_PROBE_TIMEOUT_MS) {
                failure = Some(CharacterizationFailure::Timeout);
                break;
            }
            core::hint::spin_loop();
        }

        let quiesce = self.regs.quiesce();
        let active_quiesce_causes = classify_causes(quiesce.active_legal, PROBE_CAUSE_ADMISSION, 0);
        let cleanup_causes = classify_causes(
            quiesce.cleanup_legal,
            PROBE_CAUSE_ADMISSION,
            QUIESCE_RECOVERABLE_CAUSES,
        );
        legal |= active_quiesce_causes.legal | cleanup_causes.legal;
        observed |= active_quiesce_causes.expected | cleanup_causes.expected;
        abnormal |= active_quiesce_causes.abnormal | cleanup_causes.abnormal;
        uncleared |= quiesce.uncleared;
        w1c_samples = w1c_samples.saturating_add(quiesce.w1c_samples);
        if !quiesce.stopped {
            failure = Some(CharacterizationFailure::Quiesce);
        } else if abnormal != 0 {
            failure = Some(CharacterizationFailure::AbnormalCause);
        } else if uncleared != 0 {
            failure = Some(CharacterizationFailure::UnclearedCause);
        } else if quiesce.mac_status_after != 0 {
            failure = Some(CharacterizationFailure::UnclearedCause);
        }
        let descriptor = self.rings.lock_irqsave().probe_snapshot();
        // Frame bytes return to the CPU only after RX completion and DMA
        // process-state quiescence. A failed quiesce retains backing untouched.
        let payload_match = quiesce.stopped
            && descriptor.rx_complete()
            && !descriptor.rx_error()
            && self.rings.lock_irqsave().probe_payload_matches();
        if failure.is_none() && !payload_match {
            failure = Some(CharacterizationFailure::Descriptor);
        }
        if failure.is_none() {
            let (rx_desc, tx_desc) = {
                let rings = self.rings.lock_irqsave();
                rings.prepare_production();
                (rings.rx_desc() as u32, rings.tx_desc() as u32)
            };
            if self.regs.rearm_descriptor_bases(rx_desc, tx_desc).is_err() {
                failure = Some(CharacterizationFailure::Register);
            } else {
                self.rings.lock_irqsave().reset_runtime_state();
            }
        }
        let result = failure.map_or(
            CharacterizationResult::Passed,
            CharacterizationResult::Failed,
        );
        *self.result.lock_irqsave() = Some(result);
        CharacterizationSnapshot {
            result,
            legal,
            observed,
            abnormal,
            uncleared,
            poll_count,
            w1c_samples,
            interrupt_enable,
            start: probe_start,
            tx_published,
            descriptor,
            payload_match,
            quiesce,
        }
    }

    pub(super) fn suppress_device(&self) -> QuiesceSnapshot {
        self.regs.quiesce()
    }

    pub(super) fn start_device(&self) {
        self.regs.start_runtime();
    }

    pub(super) fn runtime_snapshot(&self) -> RuntimeRegisterSnapshot {
        self.regs.runtime_snapshot()
    }

    pub(super) fn rx_diagnostic_snapshot(&self) -> RxDiagnosticSnapshot {
        self.rings.lock_irqsave().rx_diagnostic_snapshot()
    }

    pub(super) fn service_irq(&self) -> IrqServiceSnapshot {
        let (mac_status, mac_status_after) = self.regs.service_mac_interrupts();
        let status = self.regs.status();
        let legal = status & DmaStatus::W1C.bits();
        let raw_after = if legal != 0 {
            self.regs.acknowledge_causes(legal)
        } else {
            self.regs.status()
        };
        IrqServiceSnapshot {
            csr5_raw: status,
            mac_status,
            mac_status_after,
            csr5_raw_after: raw_after,
            csr5: legal,
            csr5_after: raw_after & DmaStatus::W1C.bits(),
        }
    }

    pub(super) fn frame_capacity(&self) -> usize {
        self.rings.lock_irqsave().frame_capacity()
    }

    pub(super) fn ring_size(&self) -> usize {
        self.rings.lock_irqsave().ring_size()
    }

    pub(super) fn reserve_tx(&self) -> Option<usize> {
        self.rings.lock_irqsave().reserve_tx()
    }

    pub(super) fn cancel_tx(&self, index: usize) -> Result<(), RingError> {
        self.rings.lock_irqsave().cancel_tx(index)
    }

    pub(super) fn commit_tx_with<R>(
        &self,
        index: usize,
        length: usize,
        fill: impl FnOnce(&mut [u8]) -> R,
    ) -> Result<R, RingError> {
        let (ptr, length) = {
            let mut rings = self.rings.lock_irqsave();
            rings.tx_frame_parts(index, length)?
        };
        let result = fill(unsafe { core::slice::from_raw_parts_mut(ptr, length) });
        self.rings.lock_irqsave().commit_tx(index, length)?;
        self.regs.demand_tx();
        Ok(result)
    }

    pub(super) fn reclaim_tx(&self) -> Result<Option<TxCompletion>, RingError> {
        self.rings.lock_irqsave().reclaim_tx()
    }

    pub(super) fn reserve_rx(&self) -> Option<RxReservation> {
        self.rings.lock_irqsave().reserve_rx()
    }

    pub(super) fn cancel_rx(&self, index: usize) -> Result<(), RingError> {
        self.rings.lock_irqsave().cancel_rx(index)
    }

    pub(super) fn discard_rx(&self, index: usize) -> Result<(), RingError> {
        let result = self.rings.lock_irqsave().discard_rx(index);
        if result.is_ok() {
            self.regs.demand_rx();
        }
        result
    }

    pub(super) fn consume_rx<R>(
        &self,
        index: usize,
        consume: impl FnOnce(&[u8]) -> R,
    ) -> Result<R, RingError> {
        let (ptr, length) = self.rings.lock_irqsave().rx_frame_parts(index)?;
        let result = consume(unsafe { core::slice::from_raw_parts(ptr, length) });
        self.rings.lock_irqsave().finish_rx_public(index)?;
        self.regs.demand_rx();
        Ok(result)
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn polling_admission_has_expected_linux_legacy_shape() {
        assert_eq!(EXPECTED_CAUSES & ABNORMAL_CONSTITUENTS, 0);
        assert_eq!(PROBE_CAUSE_ADMISSION & EXPECTED_CAUSES, EXPECTED_CAUSES);
        assert_ne!(PROBE_CAUSE_ADMISSION & DmaStatus::ABNORMAL.bits(), 0);
        assert_eq!(PROBE_CAUSE_ADMISSION & !DmaStatus::W1C.bits(), 0);
        assert_eq!(PROBE_CAUSE_ADMISSION, 0x1_a061);
    }

    #[kunit]
    fn raw_legal_status_is_acked_while_polling_admission_filters_evidence() {
        let status = DmaStatus::NORMAL.bits()
            | DmaStatus::RX.bits()
            | DmaStatus::TX.bits()
            | DmaStatus::TX_UNAVAILABLE.bits();
        let classified =
            classify_causes(status, DmaStatus::NORMAL.bits() | DmaStatus::RX.bits(), 0);
        assert_eq!(classified.legal, status);
        assert_eq!(classified.expected, DmaStatus::RX.bits());
        assert_eq!(classified.abnormal, 0);
    }

    #[kunit]
    fn early_tx_is_linux_statistical_evidence_not_a_hard_error() {
        // Linux's legacy handler increments tx_early_irq for ETI but does not
        // return tx_hard_error. Keep it in legal W1C evidence without failing
        // an otherwise complete bounded transfer.
        let status = DmaStatus::ABNORMAL.bits()
            | DmaStatus::TX_EARLY.bits()
            | DmaStatus::NORMAL.bits()
            | DmaStatus::RX.bits()
            | DmaStatus::TX.bits();
        let classified = classify_causes(status, PROBE_CAUSE_ADMISSION, 0);
        assert_eq!(classified.legal, status);
        assert_eq!(classified.expected, EXPECTED_CAUSES);
        assert_eq!(classified.abnormal, 0);
    }

    #[kunit]
    fn abnormal_summary_classifies_raw_linux_constituents() {
        let status = DmaStatus::ABNORMAL.bits()
            | DmaStatus::RX_STOPPED.bits()
            | DmaStatus::TX_UNAVAILABLE.bits()
            | DmaStatus::TX_STOPPED.bits();
        let classified = classify_causes(status, DmaStatus::ABNORMAL.bits(), 0);
        assert_eq!(classified.legal, status);
        assert_eq!(classified.expected, 0);
        assert_eq!(
            classified.abnormal,
            status & !DmaStatus::TX_UNAVAILABLE.bits()
        );
    }

    #[kunit]
    fn abnormal_constituent_does_not_require_irq_summary_while_csr7_is_zero() {
        let status = DmaStatus::RX_STOPPED.bits() | DmaStatus::TX_UNAVAILABLE.bits();
        let classified = classify_causes(status, PROBE_CAUSE_ADMISSION, 0);
        assert_eq!(classified.legal, status);
        assert_eq!(classified.expected, 0);
        assert_eq!(classified.abnormal, DmaStatus::RX_STOPPED.bits());
    }

    #[kunit]
    fn tx_and_rx_samples_may_reuse_summary_without_becoming_uncleared() {
        let tx = classify_causes(
            DmaStatus::NORMAL.bits() | DmaStatus::TX.bits(),
            PROBE_CAUSE_ADMISSION,
            0,
        );
        let rx = classify_causes(
            DmaStatus::NORMAL.bits() | DmaStatus::RX.bits(),
            PROBE_CAUSE_ADMISSION,
            0,
        );
        assert_eq!(tx.expected | rx.expected, EXPECTED_CAUSES);
        assert_eq!(tx.abnormal | rx.abnormal, 0);
        assert_ne!(tx.legal & rx.legal, 0);
    }

    #[kunit]
    fn tx_unavailable_is_linux_w1c_evidence_not_a_hard_error() {
        let classified =
            classify_causes(DmaStatus::TX_UNAVAILABLE.bits(), PROBE_CAUSE_ADMISSION, 0);
        assert_eq!(classified.legal, DmaStatus::TX_UNAVAILABLE.bits());
        assert_eq!(classified.expected, 0);
        assert_eq!(classified.abnormal, 0);
    }

    #[kunit]
    fn abnormal_summary_with_only_tx_unavailable_is_not_a_hard_error() {
        let status = DmaStatus::ABNORMAL.bits() | DmaStatus::TX_UNAVAILABLE.bits();
        let classified = classify_causes(status, PROBE_CAUSE_ADMISSION, 0);
        assert_eq!(classified.legal, status);
        assert_eq!(classified.abnormal, 0);
    }

    #[kunit]
    fn abnormal_summary_without_a_known_constituent_fails_closed() {
        let classified = classify_causes(DmaStatus::ABNORMAL.bits(), PROBE_CAUSE_ADMISSION, 0);
        assert_eq!(classified.abnormal, DmaStatus::ABNORMAL.bits());
    }

    #[kunit]
    fn deliberate_stop_causes_are_not_quiesce_failures() {
        let status = DmaStatus::ABNORMAL.bits()
            | DmaStatus::RX_STOPPED.bits()
            | DmaStatus::TX_UNAVAILABLE.bits()
            | DmaStatus::TX_STOPPED.bits();
        let classified = classify_causes(status, PROBE_CAUSE_ADMISSION, QUIESCE_RECOVERABLE_CAUSES);
        assert_eq!(classified.legal, status);
        assert_eq!(classified.abnormal, 0);
    }

    #[kunit]
    fn cleanup_abnormal_summary_without_a_recoverable_constituent_fails() {
        let classified = classify_causes(
            DmaStatus::ABNORMAL.bits(),
            PROBE_CAUSE_ADMISSION,
            QUIESCE_RECOVERABLE_CAUSES,
        );
        assert_eq!(classified.abnormal, DmaStatus::ABNORMAL.bits());
    }

    #[kunit]
    fn cleanup_recoverable_constituent_does_not_hide_fatal_cause() {
        let status = DmaStatus::ABNORMAL.bits()
            | DmaStatus::RX_STOPPED.bits()
            | DmaStatus::FATAL_BUS_ERROR.bits();
        let classified = classify_causes(status, PROBE_CAUSE_ADMISSION, QUIESCE_RECOVERABLE_CAUSES);
        assert_eq!(classified.legal, status);
        assert_eq!(
            classified.abnormal,
            DmaStatus::ABNORMAL.bits() | DmaStatus::FATAL_BUS_ERROR.bits()
        );
    }

    fn characterization(
        result: CharacterizationResult,
        quiesced: bool,
    ) -> CharacterizationSnapshot {
        CharacterizationSnapshot {
            result,
            legal: 0,
            observed: 0,
            abnormal: 0,
            uncleared: 0,
            poll_count: 0,
            w1c_samples: 0,
            interrupt_enable: 0,
            start: ProbeStartSnapshot {
                mac_enabled_control: 0,
                rx_started_control: 0,
                tx_started_control: 0,
                hash_high: 0,
                hash_low: 0,
                frame_filter: 0,
                loopback_control: 0,
                flow_control: 0,
                interrupt_enable: 0,
            },
            tx_published: EnhancedDescriptor::default(),
            descriptor: DescriptorSnapshot {
                rx: EnhancedDescriptor::default(),
                tx: EnhancedDescriptor::default(),
            },
            payload_match: false,
            quiesce: QuiesceSnapshot {
                status: 0,
                active_legal: 0,
                cleanup_legal: 0,
                uncleared: 0,
                w1c_samples: 0,
                control: 0,
                mac_control: 0,
                tx_process: 0,
                rx_process: 0,
                stopped: quiesced,
                mac_status_before: 0,
                mac_status_after: 0,
            },
        }
    }

    #[kunit]
    fn gate2_lifecycle_retains_only_a_quiesced_pass() {
        assert_eq!(
            characterization(CharacterizationResult::Passed, true).disposition(),
            ProbeDisposition::BindRetained
        );
        assert_eq!(
            characterization(
                CharacterizationResult::Failed(CharacterizationFailure::Timeout),
                true,
            )
            .disposition(),
            ProbeDisposition::ReturnFailure
        );
        assert_eq!(
            characterization(
                CharacterizationResult::Failed(CharacterizationFailure::Quiesce),
                false,
            )
            .disposition(),
            ProbeDisposition::FailStop
        );
    }
}
