//! Interrupt subsystem.
//!
//! Currently, this subsystem only handles external interrupts from devices, and
//! does not handle CPU-internal interrupts such as timer interrupts and
//! inter-processor interrupts. They are handled manually in arch-specific code.

mod flow;
pub use flow::IrqFlowType;

use core::fmt::Debug;

use crate::{
    device::discovery::fwnode::{
        FwNode, InterruptResourceError, InterruptSelector, select_interrupt_resource,
    },
    prelude::*,
    utils::{any_opaque::AnyOpaque, identity::GeneralIdentity},
};

int_like!(HwIrq, usize);
int_like!(VirtIrq, usize);

/// An interrupt domain, which represents a collection of interrupt lines
/// managed by the same interrupt controller.
///
/// Each interrupt domain has a bijective mapping between virtual IRQs and
/// hardware IRQs, and the operations provided by the interrupt controller
/// associated with this domain.
///
/// **LOCK ORDERING**:
/// **`map` -> `ops`**
#[derive(Debug)]
pub struct IrqDomain {
    /// Currently only for debugging purposes, but maybe we can use it for
    /// something else in the future like sysfs.
    name: GeneralIdentity,

    /// Bijective Mapping between virtual IRQs and hardware IRQs.
    map: RwLock<BiMap<VirtIrq, HwIrq>>,

    /// Operations provided by the interrupt controller associated with this
    /// domain.
    ops: RwLock<Box<dyn IrqChip>>,

    /// Some interrupt controllers may not have an associated device, such as
    /// cpu-internal interrupt  controllers initialized before device
    /// discovery. However, every physical device must have an  associated
    /// firmware node, otherwise how do we find the interrupt controller in the
    /// first place?
    fwnode: Arc<dyn FwNode>,
}

impl IrqDomain {
    pub fn new(name: GeneralIdentity, ops: Box<dyn IrqChip>, fwnode: Arc<dyn FwNode>) -> Self {
        Self {
            name,
            map: RwLock::new(BiMap::new()),
            ops: RwLock::new(ops),
            fwnode,
        }
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    fn map(&self, virq: VirtIrq, hwirq: HwIrq) {
        self.map.write_irqsave().insert(virq, hwirq);
    }

    fn hw2virt(&self, hwirq: HwIrq) -> Option<VirtIrq> {
        self.map.read_irqsave().get_by_right(&hwirq).cloned()
    }

    fn virt2hw(&self, virq: VirtIrq) -> Option<HwIrq> {
        self.map.read_irqsave().get_by_left(&virq).cloned()
    }

    pub fn xlate(&self, spec: InterruptSpecifier<'_>) -> Option<InterruptInfo> {
        self.ops.read_irqsave().xlate(spec)
    }
}

#[derive(Debug)]
pub struct IrqDesc {
    virq: VirtIrq,
    hwirq: HwIrq,
    flow: IrqFlowType,
    domain: Arc<IrqDomain>,
    handler: &'static IrqHandler,
    prv_data: MonoOnce<AnyOpaque>,
}

/// An interrupt handler.
#[derive(Debug)]
pub struct IrqHandler {
    func: fn(&AnyOpaque),
}

impl IrqHandler {
    pub const fn new(func: fn(&AnyOpaque)) -> Self {
        Self { func }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Electrical sense reported by the irqchip that owns the source. Request
/// callers may compare an expectation against this fact, but cannot configure
/// or override it.
pub enum IrqSense {
    LevelHigh,
    LevelLow,
    EdgeRising,
    EdgeFalling,
    EdgeBoth,
}

impl IrqSense {
    pub(crate) fn from_linux_convention(value: u32) -> Option<Self> {
        match value {
            1 => Some(Self::EdgeRising),
            2 => Some(Self::EdgeFalling),
            3 => Some(Self::EdgeBoth),
            4 => Some(Self::LevelHigh),
            8 => Some(Self::LevelLow),
            _ => None,
        }
    }

    pub(crate) const fn is_edge(self) -> bool {
        matches!(self, Self::EdgeRising | Self::EdgeFalling | Self::EdgeBoth)
    }

    pub(crate) const fn is_low(self) -> bool {
        matches!(self, Self::LevelLow | Self::EdgeFalling)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct InterruptInfo {
    pub hwirq: HwIrq,
    pub sense: IrqSense,
    pub flow: IrqFlowType,
}

impl InterruptInfo {
    pub fn parse_2_cell_specifier(
        specifier: InterruptSpecifier<'_>,
        flow: IrqFlowType,
    ) -> Option<Self> {
        if specifier.raw.len() != 8 {
            return None;
        }
        let hwirq = HwIrq::new(u32::from_be_bytes(specifier.raw[0..4].try_into().ok()?) as usize);
        let sense = IrqSense::from_linux_convention(u32::from_be_bytes(
            specifier.raw[4..8].try_into().ok()?,
        ))?;
        Some(Self { hwirq, sense, flow })
    }
}

#[derive(Debug)]
pub struct InterruptSpecifier<'a> {
    pub fwnode: &'a dyn FwNode,
    pub raw: &'a [u8],
}

/// Interrupt controller trait.
pub trait IrqChip: Send + Sync {
    /// Mask the given interrupt line, preventing it from being delivered to the
    /// CPU.
    fn mask(&self, irq: HwIrq);
    /// Unmask the given interrupt line, allowing it to be delivered to the CPU.
    fn unmask(&self, irq: HwIrq);

    /// Acknowledge the given interrupt line, clearing the pending state.
    fn ack(&self, irq: HwIrq);

    /// Complete the controller-side interrupt transaction.
    ///
    /// The selected IRQ flow decides whether this operation is required; its
    /// use is independent of the source's electrical trigger type.
    fn eoi(&self, irq: HwIrq);

    /// Translate the raw interrupt specifier from firmware into the
    /// corresponding hardware IRQ number, electrical sense, and controller
    /// flow.
    fn xlate(&self, spec: InterruptSpecifier<'_>) -> Option<InterruptInfo>;

    fn as_core_irq_chip(&self) -> Option<&dyn CoreIrqChip> {
        None
    }
}

impl Debug for dyn IrqChip {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "dyn IrqChip")
    }
}

/// Some interrupt controllers must be initialized before device discovery, such
/// as GIC on ARM and PLIC on RiscV. For these interrupt controllers, there is
/// no associated device, so we cannot rely on device discovery to initialize
/// them. Instead, we need to initialize them manually in the early boot
/// process, and register them as the root interrupt domain.
pub trait CoreIrqChip: IrqChip {
    /// Resolve information from the given firmware node, and initialize needed
    /// data structures.
    fn init(fwnode: &dyn FwNode) -> Box<dyn CoreIrqChip>
    where
        Self: Sized;

    /// Root interrupt controllers should be able to self-discover irq number
    /// and claim interrupts without help from other interrupt controllers,
    /// since there is no one else to help them.
    ///
    /// After this bootstrap process, we can fire up normal chained interrupt
    /// controllers that rely on parent domains for interrupt information.
    fn claim(&self) -> Option<HwIrq>;
}

/// Allocate a new virtual IRQ number.
///
/// Since we do not support hotplugging of interrupt controllers, we can simply
/// use an atomic variable for allocating virtual IRQs. So no deallocation, no
/// RAII, just a plain old counter.
///
/// # Safety
///
/// After allocating a new virtual IRQ, a corresponding [IrqDesc] must be
/// created and inserted into the global IRQ descriptor table, and the mapping
/// between the virtual IRQ and the hardware IRQ must be established in the
/// corresponding interrupt domain.
///
/// TODO: How to make this an safe RAII operation in an elegant way? 🤔
unsafe fn alloc_virq() -> VirtIrq {
    static VIRQ_COUNTER: AtomicUsize = AtomicUsize::new(1);

    let mut id = VIRQ_COUNTER.load(Ordering::Relaxed);
    loop {
        assert!(id != usize::MAX, "too many virtual IRQs allocated");
        match VIRQ_COUNTER.compare_exchange_weak(id, id + 1, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return VirtIrq(id),
            Err(new_id) => id = new_id,
        }
    }
}

static IRQ_DESCS: Lazy<RwLock<HashMap<VirtIrq, IrqDesc>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

static IRQ_DOMAINS: Lazy<RwLock<VecDeque<Arc<IrqDomain>>>> =
    Lazy::new(|| RwLock::new(VecDeque::new()));

static ROOT_IRQ_DOMAIN: MonoOnce<Arc<IrqDomain>> = unsafe { MonoOnce::new() };

/// Register the given interrupt domain as the root interrupt domain.
///
/// The root interrupt domain is the first interrupt domain that will be
/// searched when looking for an interrupt domain for a device. It is usually
/// the interrupt domain associated with the primary interrupt controller of the
/// system, such as GIC on ARM and PLIC on RiscV.
pub unsafe fn register_root_irq_domain(
    name: GeneralIdentity,
    ops: Box<dyn CoreIrqChip>,
    fwnode: Arc<dyn FwNode>,
) {
    let domain = Arc::new(IrqDomain::new(name, ops, fwnode));

    IRQ_DOMAINS.write_irqsave().push_back(domain.clone());

    ROOT_IRQ_DOMAIN.init(|root| {
        root.write(domain.clone());
    });
    kinfoln!(
        "registered root irq domain: {}",
        ROOT_IRQ_DOMAIN.get().name()
    );
}

/// Register a new interrupt domain to the system.
pub fn register_irq_domain(domain: IrqDomain) {
    kinfoln!("registering new irq domain: {}", domain.name());
    IRQ_DOMAINS.write_irqsave().push_back(Arc::new(domain));
}

/// Find the interrupt domain associated with the given firmware node.
///
/// Internally, this is a simple linear search. Since the number of interrupt
/// domains is usually very small, the performance offerer by spatial locality
/// is better than a hash map? (maybe. idk. anyway. whatever.)
pub fn find_irq_domain_by_fwnode(fwnode: &dyn FwNode) -> Option<Arc<IrqDomain>> {
    IRQ_DOMAINS
        .read_irqsave()
        .iter()
        .find(|domain| domain.fwnode.as_ref().equals(fwnode))
        .cloned()
}

/// Request an IRQ for the given device, and register the given handler to it.
/// `expected` is a one-shot admission assertion checked before mapping,
/// descriptor publication, or unmask; it is never retained as IRQ state.
pub fn request_irq(
    dev: &dyn Device,
    expected: Option<IrqSense>,
    handler: &'static IrqHandler,
    prv_data: Option<AnyOpaque>,
) -> Result<(), SysError> {
    request_irq_inner(dev, None, expected, handler, prv_data)
}

/// Request one explicitly selected firmware interrupt. Callers for
/// multi-interrupt DT nodes must use this crate-local path so a whole raw
/// `interrupts` property cannot reach an irqchip translator. `expected` has the
/// same assertion-only semantics as [`request_irq`].
pub(crate) fn request_irq_selected(
    dev: &dyn Device,
    selector: InterruptSelector<'_>,
    expected: Option<IrqSense>,
    handler: &'static IrqHandler,
    prv_data: Option<AnyOpaque>,
) -> Result<(), SysError> {
    request_irq_inner(dev, Some(selector), expected, handler, prv_data)
}

fn request_irq_inner(
    dev: &dyn Device,
    selector: Option<InterruptSelector<'_>>,
    expected: Option<IrqSense>,
    handler: &'static IrqHandler,
    prv_data: Option<AnyOpaque>,
) -> Result<(), SysError> {
    let fwnode = dev.fwnode().ok_or(SysError::MissingFwNode)?;
    let ic = fwnode.interrupt_parent().ok_or(SysError::NoIrqDomain)?;
    let domain = find_irq_domain_by_fwnode(ic.as_ref()).expect("ic exists but no domain found");
    let prepared = match prepare_irq_request(fwnode.as_ref(), &domain, selector, expected) {
        Ok(prepared) => prepared,
        Err(error) => {
            kerrln!(
                "request_irq: dev_id={} domain={} admission failed: {:?}",
                dev.name(),
                domain.name(),
                error,
            );
            return Err(error);
        },
    };

    let virq = commit_irq_request(&domain, prepared, handler, prv_data)?;

    kdebugln!(
        "request_irq: dev_id={}, domain={}, virq={}, hwirq={:#x}",
        dev.name(),
        domain.name(),
        virq.get(),
        prepared.hwirq.get()
    );

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PreparedIrq {
    hwirq: HwIrq,
    flow: IrqFlowType,
}

fn prepare_irq_request(
    fwnode: &dyn FwNode,
    domain: &IrqDomain,
    selector: Option<InterruptSelector<'_>>,
    expected: Option<IrqSense>,
) -> Result<PreparedIrq, SysError> {
    let ops = domain.ops.read_irqsave();
    let selected = match selector {
        Some(selector) => {
            select_interrupt_resource(fwnode, selector).map_err(map_interrupt_resource_error)?
        },
        None => fwnode
            .interrupt_info()
            .map(
                |specifier| crate::device::discovery::fwnode::InterruptResource {
                    index: 0,
                    specifier,
                },
            )
            .ok_or(SysError::NoInterruptInfo)?,
    };
    let info = ops
        .xlate(InterruptSpecifier {
            fwnode,
            raw: selected.specifier(),
        })
        .ok_or(SysError::InvalidInterruptInfo)?;
    drop(ops);
    validate_expected_sense(expected, info.hwirq, info.sense)?;
    Ok(PreparedIrq {
        hwirq: info.hwirq,
        flow: info.flow,
    })
}

fn commit_irq_request(
    domain: &Arc<IrqDomain>,
    prepared: PreparedIrq,
    handler: &'static IrqHandler,
    prv_data: Option<AnyOpaque>,
) -> Result<VirtIrq, SysError> {
    let hwirq = prepared.hwirq;
    let virq = if let Some(_) = domain.hw2virt(hwirq) {
        return Err(SysError::IrqAlreadyRequested);
    } else {
        let virq = unsafe { alloc_virq() };
        domain.map(virq, hwirq);

        let desc = IrqDesc {
            virq,
            hwirq,
            flow: prepared.flow,
            domain: domain.clone(),
            handler,
            prv_data: unsafe { MonoOnce::new() },
        };
        if let Some(prv) = prv_data {
            desc.prv_data.init(|p| {
                p.write(prv);
            });
        }

        assert!(IRQ_DESCS.write_irqsave().insert(virq, desc).is_none());

        virq
    };

    domain.ops.read_irqsave().unmask(hwirq);
    Ok(virq)
}

fn validate_expected_sense(
    expected: Option<IrqSense>,
    hwirq: HwIrq,
    actual: IrqSense,
) -> Result<(), SysError> {
    match expected {
        None => Ok(()),
        Some(expected) if expected == actual => Ok(()),
        Some(expected) => {
            kerrln!(
                "request_irq: sense mismatch hwirq={:#x} expected={:?} actual={:?}",
                hwirq.get(),
                expected,
                actual,
            );
            Err(SysError::InvalidInterruptInfo)
        },
    }
}

fn map_interrupt_resource_error(error: InterruptResourceError) -> SysError {
    match error {
        InterruptResourceError::MissingInterrupts => SysError::NoInterruptInfo,
        InterruptResourceError::MissingParentCells
        | InterruptResourceError::InvalidCellCount
        | InterruptResourceError::SpecifierLengthMismatch
        | InterruptResourceError::IndexOutOfRange
        | InterruptResourceError::MissingNames
        | InterruptResourceError::InvalidNames
        | InterruptResourceError::NameCountMismatch
        | InterruptResourceError::DuplicateName
        | InterruptResourceError::NameNotFound => SysError::InvalidInterruptInfo,
    }
}

/// Handle the given hardware IRQ from the root interrupt domain.
pub fn handle_irq() {
    let root_domain = ROOT_IRQ_DOMAIN.get();
    let ops = root_domain.ops.write_irqsave();
    let core = ops
        .as_core_irq_chip()
        .expect("root irq domain's ops must be a core irq chip");
    if let Some(hwirq) = core.claim() {
        drop(ops);
        handle_domain_irq(root_domain, hwirq).expect("handling root irq must succeed");
    } else {
        kwarningln!("claimed no hwirq from root irq domain but got an interrupt");
    }
}

/// Handle the given hardware IRQ from the given domain.
///
/// This function can be used by those chained interrupt controllers that need
/// to handle interrupts from their own domain.
pub fn handle_domain_irq(domain: &IrqDomain, hwirq: HwIrq) -> Result<(), SysError> {
    let virq = domain.hw2virt(hwirq).ok_or(SysError::UnknownInterrupt)?;
    // TODO: a read_irqsave should be enouth. we should find a way to avoid taking a
    // write lock here.
    let mut descs = IRQ_DESCS.write_irqsave();

    let desc = descs
        .get_mut(&virq)
        .expect("desc must exist for allocated virq");

    flow::execute(desc.flow, &desc.domain.ops, desc.hwirq, || {
        (desc.handler.func)(desc.prv_data.get());
    });

    Ok(())
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::{
        device::discovery::fwnode::{FwNode, StdoutConfig},
        utils::identity::GeneralIdentity,
    };
    use core::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct RequestNode {
        parent: Option<Arc<dyn FwNode>>,
        interrupts: Option<Vec<u8>>,
        names: Option<Vec<u8>>,
        cells: Option<u32>,
    }

    impl FwNode for RequestNode {
        fn equals(&self, other: &dyn FwNode) -> bool {
            (other as &dyn core::any::Any)
                .downcast_ref::<RequestNode>()
                .is_some_and(|other| core::ptr::eq(self, other))
        }

        fn prop_read_u32(&self, name: &str) -> Option<u32> {
            (name == "#interrupt-cells").then_some(self.cells).flatten()
        }

        fn prop_read_u64(&self, _name: &str) -> Option<u64> {
            None
        }

        fn prop_read_str(&self, _name: &str) -> Option<String> {
            None
        }

        fn prop_read_present(&self, name: &str) -> bool {
            self.prop_read_raw(name).is_some()
        }

        fn prop_read_raw(&self, name: &str) -> Option<&[u8]> {
            match name {
                "interrupts" => self.interrupts.as_deref(),
                "interrupt-names" => self.names.as_deref(),
                _ => None,
            }
        }

        fn interrupt_parent(&self) -> Option<Arc<dyn FwNode>> {
            self.parent.clone()
        }

        fn interrupt_info(&self) -> Option<&[u8]> {
            self.interrupts.as_deref()
        }

        fn stdout_config(&self) -> Option<StdoutConfig<'_>> {
            None
        }
    }

    struct RequestChip {
        actual: IrqSense,
        unmask_count: Arc<AtomicUsize>,
        translated: Arc<SpinLock<Vec<HwIrq>>>,
    }

    impl IrqChip for RequestChip {
        fn mask(&self, _irq: HwIrq) {}
        fn unmask(&self, _irq: HwIrq) {
            self.unmask_count.fetch_add(1, Ordering::SeqCst);
        }
        fn ack(&self, _irq: HwIrq) {}
        fn eoi(&self, _irq: HwIrq) {}
        fn xlate(&self, spec: InterruptSpecifier<'_>) -> Option<InterruptInfo> {
            let hwirq = HwIrq::new(u32::from_be_bytes(spec.raw[..4].try_into().ok()?) as usize);
            self.translated.lock_irqsave().push(hwirq);
            Some(InterruptInfo {
                hwirq,
                sense: self.actual,
                flow: IrqFlowType::LevelMaskEoi,
            })
        }
    }

    fn test_handler(_: &AnyOpaque) {}
    static TEST_HANDLER: IrqHandler = IrqHandler::new(test_handler);

    #[kunit]
    fn request_plan_selects_named_macirq_and_rejects_before_commit() {
        let parent: Arc<dyn FwNode> = Arc::new(RequestNode {
            parent: None,
            interrupts: None,
            names: None,
            cells: Some(2),
        });
        let child: Arc<dyn FwNode> = Arc::new(RequestNode {
            parent: Some(parent.clone()),
            interrupts: Some(
                [0x100_u32, 4, 0x200_u32, 8]
                    .into_iter()
                    .flat_map(u32::to_be_bytes)
                    .collect(),
            ),
            names: Some(b"eth_wake_irq\0macirq\0".to_vec()),
            cells: None,
        });
        let unmask_count = Arc::new(AtomicUsize::new(0));
        let translated = Arc::new(SpinLock::new(Vec::new()));
        let domain = Arc::new(IrqDomain::new(
            GeneralIdentity::try_from("request-test").unwrap(),
            Box::new(RequestChip {
                actual: IrqSense::LevelLow,
                unmask_count: unmask_count.clone(),
                translated: translated.clone(),
            }),
            parent,
        ));

        let selected = prepare_irq_request(
            child.as_ref(),
            &domain,
            Some(InterruptSelector::Name("macirq")),
            None,
        )
        .unwrap();
        assert_eq!(selected.hwirq, HwIrq::new(0x200));
        assert_eq!(translated.lock_irqsave().as_slice(), [HwIrq::new(0x200)]);
        assert_eq!(unmask_count.load(Ordering::SeqCst), 0);

        assert!(
            prepare_irq_request(
                child.as_ref(),
                &domain,
                Some(InterruptSelector::Name("macirq")),
                Some(IrqSense::LevelLow),
            )
            .is_ok()
        );
        assert_eq!(
            prepare_irq_request(
                child.as_ref(),
                &domain,
                Some(InterruptSelector::Name("macirq")),
                Some(IrqSense::EdgeRising),
            ),
            Err(SysError::InvalidInterruptInfo)
        );
        assert_eq!(domain.hw2virt(HwIrq::new(0x200)), None);
        assert_eq!(unmask_count.load(Ordering::SeqCst), 0);

        let prepared = prepare_irq_request(
            child.as_ref(),
            &domain,
            Some(InterruptSelector::Name("macirq")),
            Some(IrqSense::LevelLow),
        )
        .unwrap();
        commit_irq_request(&domain, prepared, &TEST_HANDLER, None).unwrap();
        assert_eq!(domain.hw2virt(HwIrq::new(0x200)).is_some(), true);
        assert_eq!(unmask_count.load(Ordering::SeqCst), 1);
    }
}
