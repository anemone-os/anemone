use crate::{prelude::*, time::MonotonicInstant};

use super::{
    phy::PhyState,
    protocol::{CSR5_W1C_MASK, EnhancedDescriptor, device_cause},
    regs::{Dwmac1000Regs, ProbeStartSnapshot, QuiesceSnapshot},
    ring::{DescriptorSnapshot, Dwmac1000Rings},
};

const NORMAL_INTERRUPT: u32 = 1 << 16;
const ABNORMAL_INTERRUPT: u32 = 1 << 15;
const RX_INTERRUPT: u32 = 1 << 6;
const TX_INTERRUPT: u32 = 1;
const FATAL_BUS_ERROR: u32 = 1 << 13;
const TX_UNDERFLOW: u32 = 1 << 5;
const TX_EARLY: u32 = 1 << 10;
const RX_WATCHDOG: u32 = 1 << 9;
const RX_STOPPED: u32 = 1 << 8;
const RX_UNAVAILABLE: u32 = 1 << 7;
const RX_OVERFLOW: u32 = 1 << 4;
const TX_JABBER: u32 = 1 << 3;
const TX_UNAVAILABLE: u32 = 1 << 2;
const TX_STOPPED: u32 = 1 << 1;
// Linux records ETI statistically and only W1C-clears TU; neither produces a
// hard-error return. Either may legitimately be the only constituent with AIS.
const NONFATAL_ABNORMAL_CONSTITUENTS: u32 = TX_EARLY | TX_UNAVAILABLE;
// Match Linux's legacy DWMAC1000 RX/TX and abnormal-cause admission,
// but do not write this mask to CSR7 during Gate 2. It only classifies the raw
// CSR5 samples consumed by the polling characterization.
const PROBE_CAUSE_ADMISSION: u32 = NORMAL_INTERRUPT
    | ABNORMAL_INTERRUPT
    | FATAL_BUS_ERROR
    | TX_UNDERFLOW
    | RX_INTERRUPT
    | TX_INTERRUPT;
const EXPECTED_CAUSES: u32 = RX_INTERRUPT | TX_INTERRUPT;
const ABNORMAL_CONSTITUENTS: u32 = FATAL_BUS_ERROR
    | RX_WATCHDOG
    | RX_STOPPED
    | RX_UNAVAILABLE
    | TX_UNDERFLOW
    | RX_OVERFLOW
    | TX_JABBER
    | TX_STOPPED;
const QUIESCE_RECOVERABLE_CAUSES: u32 = RX_STOPPED | TX_STOPPED;

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
        self.legal & TX_EARLY != 0
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CauseClassification {
    legal: u32,
    expected: u32,
    abnormal: u32,
}

const fn classify_causes(status: u32, admitted: u32, recoverable: u32) -> CauseClassification {
    let legal = status & CSR5_W1C_MASK;
    let admitted_causes = device_cause(status, admitted);
    let raw_abnormal_constituents = legal & ABNORMAL_CONSTITUENTS;
    let raw_nonfatal_constituents = legal & NONFATAL_ABNORMAL_CONSTITUENTS;
    let abnormal_constituents = raw_abnormal_constituents & !recoverable;
    let summary_is_recoverable =
        (raw_abnormal_constituents | raw_nonfatal_constituents) != 0 && abnormal_constituents == 0;
    let abnormal_summary = if legal & ABNORMAL_INTERRUPT != 0 && !summary_is_recoverable {
        ABNORMAL_INTERRUPT
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
    rings: Dwmac1000Rings,
    /// Accepted PHY snapshot retained for Gate 3 in-place adoption.
    phy: PhyState,
    /// Diagnostic-only characterization snapshot for shutdown logging. Probe
    /// disposition is decided from the returned snapshot, never from this copy.
    result: SpinLock<Option<CharacterizationResult>>,
}

impl Dwmac1000Owner {
    pub(super) fn new(regs: Arc<Dwmac1000Regs>, rings: Dwmac1000Rings, phy: PhyState) -> Arc<Self> {
        Arc::new(Self {
            regs,
            rings,
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

    pub(super) fn characterize(&self) -> CharacterizationSnapshot {
        let probe_start = self.regs.start_probe();
        let tx_published = if probe_start.linux_sequence_valid() {
            // Linux publishes TX ownership only after MAC and both DMA paths
            // are running, then writes CSR1 to demand descriptor polling.
            let descriptor = self.rings.publish_probe_tx();
            self.regs.demand_tx();
            descriptor
        } else {
            self.rings.probe_snapshot().tx
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
            let descriptor = self.rings.probe_snapshot();
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
                && status & CSR5_W1C_MASK == 0
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
        let descriptor = self.rings.probe_snapshot();
        // Frame bytes return to the CPU only after RX completion and DMA
        // process-state quiescence. A failed quiesce retains backing untouched.
        let payload_match = quiesce.stopped
            && descriptor.rx_complete()
            && !descriptor.rx_error()
            && self.rings.probe_payload_matches();
        if failure.is_none() && !payload_match {
            failure = Some(CharacterizationFailure::Descriptor);
        }
        if failure.is_none() {
            self.rings.prepare_production();
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
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn polling_admission_has_expected_linux_legacy_shape() {
        assert_eq!(EXPECTED_CAUSES & ABNORMAL_CONSTITUENTS, 0);
        assert_eq!(PROBE_CAUSE_ADMISSION & EXPECTED_CAUSES, EXPECTED_CAUSES);
        assert_ne!(PROBE_CAUSE_ADMISSION & ABNORMAL_INTERRUPT, 0);
        assert_eq!(PROBE_CAUSE_ADMISSION & !CSR5_W1C_MASK, 0);
        assert_eq!(PROBE_CAUSE_ADMISSION, 0x1_a061);
    }

    #[kunit]
    fn raw_legal_status_is_acked_while_polling_admission_filters_evidence() {
        let status = NORMAL_INTERRUPT | RX_INTERRUPT | TX_INTERRUPT | TX_UNAVAILABLE;
        let classified = classify_causes(status, NORMAL_INTERRUPT | RX_INTERRUPT, 0);
        assert_eq!(classified.legal, status);
        assert_eq!(classified.expected, RX_INTERRUPT);
        assert_eq!(classified.abnormal, 0);
    }

    #[kunit]
    fn early_tx_is_linux_statistical_evidence_not_a_hard_error() {
        // Linux's legacy handler increments tx_early_irq for ETI but does not
        // return tx_hard_error. Keep it in legal W1C evidence without failing
        // an otherwise complete bounded transfer.
        let status = ABNORMAL_INTERRUPT | TX_EARLY | NORMAL_INTERRUPT | RX_INTERRUPT | TX_INTERRUPT;
        let classified = classify_causes(status, PROBE_CAUSE_ADMISSION, 0);
        assert_eq!(classified.legal, status);
        assert_eq!(classified.expected, EXPECTED_CAUSES);
        assert_eq!(classified.abnormal, 0);
    }

    #[kunit]
    fn abnormal_summary_classifies_raw_linux_constituents() {
        let status = ABNORMAL_INTERRUPT | RX_STOPPED | TX_UNAVAILABLE | TX_STOPPED;
        let classified = classify_causes(status, ABNORMAL_INTERRUPT, 0);
        assert_eq!(classified.legal, status);
        assert_eq!(classified.expected, 0);
        assert_eq!(classified.abnormal, status & !TX_UNAVAILABLE);
    }

    #[kunit]
    fn abnormal_constituent_does_not_require_irq_summary_while_csr7_is_zero() {
        let status = RX_STOPPED | TX_UNAVAILABLE;
        let classified = classify_causes(status, PROBE_CAUSE_ADMISSION, 0);
        assert_eq!(classified.legal, status);
        assert_eq!(classified.expected, 0);
        assert_eq!(classified.abnormal, RX_STOPPED);
    }

    #[kunit]
    fn tx_and_rx_samples_may_reuse_summary_without_becoming_uncleared() {
        let tx = classify_causes(NORMAL_INTERRUPT | TX_INTERRUPT, PROBE_CAUSE_ADMISSION, 0);
        let rx = classify_causes(NORMAL_INTERRUPT | RX_INTERRUPT, PROBE_CAUSE_ADMISSION, 0);
        assert_eq!(tx.expected | rx.expected, EXPECTED_CAUSES);
        assert_eq!(tx.abnormal | rx.abnormal, 0);
        assert_ne!(tx.legal & rx.legal, 0);
    }

    #[kunit]
    fn tx_unavailable_is_linux_w1c_evidence_not_a_hard_error() {
        let classified = classify_causes(TX_UNAVAILABLE, PROBE_CAUSE_ADMISSION, 0);
        assert_eq!(classified.legal, TX_UNAVAILABLE);
        assert_eq!(classified.expected, 0);
        assert_eq!(classified.abnormal, 0);
    }

    #[kunit]
    fn abnormal_summary_with_only_tx_unavailable_is_not_a_hard_error() {
        let status = ABNORMAL_INTERRUPT | TX_UNAVAILABLE;
        let classified = classify_causes(status, PROBE_CAUSE_ADMISSION, 0);
        assert_eq!(classified.legal, status);
        assert_eq!(classified.abnormal, 0);
    }

    #[kunit]
    fn abnormal_summary_without_a_known_constituent_fails_closed() {
        let classified = classify_causes(ABNORMAL_INTERRUPT, PROBE_CAUSE_ADMISSION, 0);
        assert_eq!(classified.abnormal, ABNORMAL_INTERRUPT);
    }

    #[kunit]
    fn deliberate_stop_causes_are_not_quiesce_failures() {
        let status = ABNORMAL_INTERRUPT | RX_STOPPED | TX_UNAVAILABLE | TX_STOPPED;
        let classified = classify_causes(status, PROBE_CAUSE_ADMISSION, QUIESCE_RECOVERABLE_CAUSES);
        assert_eq!(classified.legal, status);
        assert_eq!(classified.abnormal, 0);
    }

    #[kunit]
    fn cleanup_abnormal_summary_without_a_recoverable_constituent_fails() {
        let classified = classify_causes(
            ABNORMAL_INTERRUPT,
            PROBE_CAUSE_ADMISSION,
            QUIESCE_RECOVERABLE_CAUSES,
        );
        assert_eq!(classified.abnormal, ABNORMAL_INTERRUPT);
    }

    #[kunit]
    fn cleanup_recoverable_constituent_does_not_hide_fatal_cause() {
        let status = ABNORMAL_INTERRUPT | RX_STOPPED | FATAL_BUS_ERROR;
        let classified = classify_causes(status, PROBE_CAUSE_ADMISSION, QUIESCE_RECOVERABLE_CAUSES);
        assert_eq!(classified.legal, status);
        assert_eq!(classified.abnormal, ABNORMAL_INTERRUPT | FATAL_BUS_ERROR);
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
