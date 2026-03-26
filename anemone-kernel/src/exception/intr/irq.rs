//! Interrupt subsystem.
//!
//! Currently, this subsystem only handles external interrupts from devices, and
//! does not handle CPU-internal interrupts such as timer interrupts and
//! inter-processor interrupts. They are handled manually in arch-specific code.

use core::fmt::Debug;

use crate::{
    device::discovery::fwnode::FwNode,
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
}

#[derive(Debug)]
pub struct IrqDesc {
    virq: VirtIrq,
    hwirq: HwIrq,
    trigger: IrqTriggerType,
    flow: &'static dyn IrqFlow,
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

/// The flow of an interrupt.
///
/// We didn't implement this as an enum since there are many
pub trait IrqFlow: 'static + Sync {
    fn enter(&self, desc: &IrqDesc);
    fn exit(&self, desc: &IrqDesc);
}

impl Debug for dyn IrqFlow {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "dyn IrqFlow")
    }
}

// TODO: flow guard

#[derive(Debug)]
pub struct EdgeFlow;

impl IrqFlow for EdgeFlow {
    fn enter(&self, desc: &IrqDesc) {
        desc.domain.ops.read_irqsave().ack(desc.hwirq);
    }

    fn exit(&self, desc: &IrqDesc) {}
}

#[derive(Debug)]
pub struct LevelFlow;

impl IrqFlow for LevelFlow {
    fn enter(&self, desc: &IrqDesc) {
        desc.domain.ops.read_irqsave().mask(desc.hwirq);
    }

    fn exit(&self, desc: &IrqDesc) {
        desc.domain.ops.read_irqsave().eoi(desc.hwirq);
        desc.domain.ops.read_irqsave().unmask(desc.hwirq);
    }
}

#[derive(Debug, Clone, Copy)]
pub enum IrqTriggerType {
    Edge,
    Level,
}

impl IrqTriggerType {
    pub fn from_linux_convention(value: u32) -> Option<Self> {
        match value {
            1 | 2 | 3 => Some(Self::Edge),
            4 | 8 => Some(Self::Level),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct InterruptInfo {
    pub hwirq: HwIrq,
    pub trigger: IrqTriggerType,
}

impl InterruptInfo {
    pub fn parse_2_cell_specifier(specifier: InterruptSpecifier<'_>) -> Option<Self> {
        if specifier.raw.len() != 8 {
            return None;
        }
        let hwirq = HwIrq::new(u32::from_be_bytes(specifier.raw[0..4].try_into().ok()?) as usize);
        let trigger_type = IrqTriggerType::from_linux_convention(u32::from_be_bytes(
            specifier.raw[4..8].try_into().ok()?,
        ))?;
        Some(Self {
            hwirq,
            trigger: trigger_type,
        })
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

    /// End the interrupt, allowing it to be delivered again.
    ///
    /// This only makes sense for level-triggered interrupts, since
    /// edge-triggered interrupts are automatically deasserted by the hardware
    /// after being acknowledged.
    fn eoi(&self, irq: HwIrq);

    /// Translate the raw interrupt specifier from firmware into the
    /// corresponding hardware IRQ number and trigger type.
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
pub fn request_irq(
    dev: &dyn Device,
    handler: &'static IrqHandler,
    prv_data: Option<AnyOpaque>,
) -> Result<(), DevError> {
    let fwnode = dev.fwnode().ok_or(DevError::MissingFwNode)?;
    let ic = fwnode.interrupt_parent().ok_or(DevError::NoIrqDomain)?;
    let domain = find_irq_domain_by_fwnode(ic.as_ref()).expect("ic exists but no domain found");
    let ops = domain.ops.read_irqsave();
    let intr_info_raw = fwnode.interrupt_info().ok_or(DevError::NoInterruptInfo)?;
    kdebugln!("request intr info");
    let InterruptInfo { hwirq, trigger } = ops
        .xlate(InterruptSpecifier {
            fwnode,
            raw: intr_info_raw,
        })
        .ok_or(DevError::InvalidInterruptInfo)?;
    drop(ops);

    let virq = if let Some(_) = domain.hw2virt(hwirq) {
        return Err(DevError::IrqAlreadyRequested);
    } else {
        let virq = unsafe { alloc_virq() };
        domain.map(virq, hwirq);

        // TODO: custom flow handler.

        let desc = IrqDesc {
            virq,
            hwirq,
            trigger,
            flow: match trigger {
                IrqTriggerType::Edge => &EdgeFlow,
                IrqTriggerType::Level => &LevelFlow,
            },
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

    kdebugln!(
        "request_irq: dev_id={}, domain={}, virq={}, hwirq={:#x}",
        dev.name(),
        domain.name(),
        virq.get(),
        hwirq.get()
    );

    Ok(())
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
pub fn handle_domain_irq(domain: &IrqDomain, hwirq: HwIrq) -> Result<(), DevError> {
    let virq = domain.hw2virt(hwirq).ok_or(DevError::UnknownInterrupt)?;
    // TODO: a read_irqsave should be enouth. we should find a way to avoid taking a
    // write lock here.
    let mut descs = IRQ_DESCS.write_irqsave();

    let desc = descs
        .get_mut(&virq)
        .expect("desc must exist for allocated virq");

    desc.flow.enter(desc);
    (desc.handler.func)(desc.prv_data.get());
    desc.flow.exit(desc);

    Ok(())
}
