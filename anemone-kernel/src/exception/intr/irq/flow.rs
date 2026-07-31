use super::{HwIrq, IrqChip};
use crate::prelude::*;

/// Controller-side operation sequence for one mapped interrupt source.
///
/// The irqchip selects this independently from the source's electrical trigger
/// type when it translates the firmware interrupt specifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqFlowType {
    /// Acknowledge the recorded edge before invoking the device handler.
    EdgeAck,
    /// Mask a level source around the handler, then complete and unmask it.
    LevelMaskEoi,
    /// Claim already acknowledged the source; complete it after the handler.
    FastEoi,
}

/// Execute the production flow around one device handler invocation.
///
/// Each operation takes the domain's irqchip lock independently so the device
/// handler never runs while that lock is held. The handler must clear its
/// device-side cause before this function performs any trailing completion.
pub(super) fn execute(
    flow: IrqFlowType,
    ops: &RwLock<Box<dyn IrqChip>>,
    hwirq: HwIrq,
    handler: impl FnOnce(),
) {
    match flow {
        IrqFlowType::EdgeAck => {
            ops.read_irqsave().ack(hwirq);
            handler();
        },
        IrqFlowType::LevelMaskEoi => {
            ops.read_irqsave().mask(hwirq);
            handler();
            ops.read_irqsave().eoi(hwirq);
            ops.read_irqsave().unmask(hwirq);
        },
        IrqFlowType::FastEoi => {
            handler();
            ops.read_irqsave().eoi(hwirq);
        },
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Step {
        Mask,
        Unmask,
        Ack,
        Handler,
        Eoi,
    }

    struct Recorder(SpinLock<Vec<Step>>);

    impl Recorder {
        fn new() -> Self {
            Self(SpinLock::new(Vec::new()))
        }

        fn push(&self, step: Step) {
            self.0.lock_irqsave().push(step);
        }

        fn snapshot(&self) -> Vec<Step> {
            self.0.lock_irqsave().clone()
        }
    }

    struct RecordingChip(Arc<Recorder>);

    impl IrqChip for RecordingChip {
        fn mask(&self, _irq: HwIrq) {
            self.0.push(Step::Mask);
        }

        fn unmask(&self, _irq: HwIrq) {
            self.0.push(Step::Unmask);
        }

        fn ack(&self, _irq: HwIrq) {
            self.0.push(Step::Ack);
        }

        fn eoi(&self, _irq: HwIrq) {
            self.0.push(Step::Eoi);
        }

        fn xlate(
            &self,
            _spec: super::super::InterruptSpecifier<'_>,
        ) -> Option<super::super::InterruptInfo> {
            None
        }
    }

    fn record(flow: IrqFlowType) -> Vec<Step> {
        let recorder = Arc::new(Recorder::new());
        let ops = RwLock::new(Box::new(RecordingChip(recorder.clone())) as Box<dyn IrqChip>);

        execute(flow, &ops, HwIrq::new(7), || recorder.push(Step::Handler));
        recorder.snapshot()
    }

    #[kunit]
    fn edge_flow_acknowledges_before_handler() {
        assert_eq!(record(IrqFlowType::EdgeAck), [Step::Ack, Step::Handler]);
    }

    #[kunit]
    fn level_flow_masks_and_completes_around_handler() {
        assert_eq!(
            record(IrqFlowType::LevelMaskEoi),
            [Step::Mask, Step::Handler, Step::Eoi, Step::Unmask,]
        );
    }

    #[kunit]
    fn fast_eoi_flow_completes_after_handler_without_masking() {
        assert_eq!(record(IrqFlowType::FastEoi), [Step::Handler, Step::Eoi]);
    }
}
