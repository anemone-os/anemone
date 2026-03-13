//! Interrupt subsystem.

use core::fmt::Debug;

use intrusive_collections::LinkedListAtomicLink;
use spin::Lazy;

use crate::{
    device::discovery::fwnode::FwNode,
    prelude::*,
    utils::{identity::GeneralIdentity, prv_data::PrvData},
};

int_like!(HwIrq, usize);
int_like!(VirtIrq, usize);

/// An interrupt domain, which represents a collection of interrupt lines
/// managed by the same interrupt controller.
///
/// Each interrupt domain has a bijective mapping between virtual IRQs and
/// hardware IRQs, and the operations provided by the interrupt controller
/// associated with this domain.
#[derive(Debug)]
pub struct IrqDomain {
    /// Currently only for debugging purposes, but maybe we can use it for
    /// something else in the future like sysfs.
    name: GeneralIdentity,

    /// Bijective Mapping between virtual IRQs and hardware IRQs.
    map: RwLock<BiMap<VirtIrq, HwIrq>>,

    /// Operations provided by the interrupt controller associated with this
    /// domain.
    ///
    /// TODO: explain why this field along with `intc_data` is necessary, and
    /// why we don't use a trait object instead.
    ops: &'static dyn IrqChip,

    /// For those interrupt controllers initialized before device discovery,
    /// they don't have an associated device, but they still need to store some
    /// private data like register base address. This field is for that purpose.
    /// However for interrupt controllers which do have associated devices,
    /// `drv_state` field in the corresponding device struct is preferred, which
    /// has more clear ownership semantics.
    intc_data: Option<RwLock<Box<dyn PrvData>>>,

    /// Some interrupt controllers may not have an associated device, such as
    /// cpu-internal interrupt  controllers initialized before device
    /// discovery. However, every physical device must have an  associated
    /// firmware node, otherwise how do we find the interrupt controller in the
    /// first place?
    fwnode: Arc<dyn FwNode>,
}

impl IrqDomain {
    pub fn new(
        name: GeneralIdentity,
        ops: &'static dyn IrqChip,
        intc_data: Option<RwLock<Box<dyn PrvData>>>,
        fwnode: Arc<dyn FwNode>,
    ) -> Self {
        Self {
            name,
            map: RwLock::new(BiMap::new()),
            ops,
            intc_data,
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
    is_masked: AtomicBool,
    trigger: IrqTriggerType,
    flow: &'static dyn IrqFlow,
    domain: Arc<IrqDomain>,
    handlers: LinkedList<IrqHandlerAdapter>,
    //inner: RwLock<IrqDescInner>,
}

#[derive(Debug)]
pub struct IrqDescInner {
    handlers: LinkedList<IrqHandlerAdapter>,
}

#[derive(Debug)]
pub enum IrqHandleResult {
    Handled,
    Unhandled,
    NotMyInterrupt,
}

/// An interrupt handler.
///
/// An interrupt line may be shared by multiple interrupt sources. Each source
/// can register its own handler, and the handlers will be called in order until
/// one of them claims the interrupt (by returning `Handled`).
///
/// Internally, this structure is just a wrapper around a function pointer, with
/// a linked list link.
#[derive(Debug)]
pub struct IrqHandler {
    func: fn() -> IrqHandleResult,
    link: LinkedListAtomicLink,
}

impl IrqHandler {
    pub const fn new(func: fn() -> IrqHandleResult) -> Self {
        Self {
            func,
            link: LinkedListAtomicLink::new(),
        }
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

#[derive(Debug)]
pub struct EdgeFlow;

impl IrqFlow for EdgeFlow {
    fn enter(&self, desc: &IrqDesc) {
        desc.domain.ops.ack(desc.hwirq);
    }

    fn exit(&self, desc: &IrqDesc) {}
}

#[derive(Debug)]
pub struct LevelFlow;

impl IrqFlow for LevelFlow {
    fn enter(&self, desc: &IrqDesc) {
        desc.domain.ops.mask(desc.hwirq);
    }

    fn exit(&self, desc: &IrqDesc) {
        desc.domain.ops.eoi(desc.hwirq);
        desc.domain.ops.unmask(desc.hwirq);
    }
}

intrusive_adapter!(
    IrqHandlerAdapter = &'static IrqHandler: IrqHandler { link => LinkedListAtomicLink }
);

#[derive(Debug, Clone, Copy)]
pub enum IrqTriggerType {
    Edge,
    Level,
}

#[derive(Debug, Clone, Copy)]
pub struct InterruptInfo {
    pub hwirq: HwIrq,
    pub trigger: IrqTriggerType,
}

pub trait IrqChip: Sync {
    /// Enable the interrupt controller.
    fn startup(&self);
    /// Disable the interrupt controller.
    fn shutdown(&self);

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
    fn xlate(&self, raw: &[u8]) -> Option<InterruptInfo>;
}

impl Debug for dyn IrqChip {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "dyn IrqChip")
    }
}

/// Some interrupt controllers must be initialized before device discovery, such
/// as GIC on ARM and PLIC on RiscV. For these interrupt controllers, there is
/// no associated device, but we still need to store some private data like
/// register base address, and provide some initialization function to be called
/// during early boot process. This trait is for that purpose.
pub trait CoreIrqChip: IrqChip {
    /// Resolve information from the given firmware node, and initialize needed
    /// data structures. The returned private data will be stored in the
    /// `intc_data` field of the corresponding interrupt domain, and can be
    /// accessed by the interrupt controller driver later.
    fn init(&self, fwnode: Arc<dyn FwNode>) -> Box<dyn PrvData>;
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

/// Register a new interrupt domain to the system.
pub fn register_irq_domain(domain: Arc<IrqDomain>) {
    kinfoln!("registering new irq domain: {:?}", domain);
    IRQ_DOMAINS.write_irqsave().push_back(domain);
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

#[derive(Debug)]
pub enum RequestIrqError {}

/// Request an IRQ for the given device, and register the given handler to it.
pub fn request_irq(dev: &dyn Device, handler: &'static IrqHandler) -> Result<(), DevError> {
    let fwnode = dev.fwnode().ok_or(DevError::MissingFwNode)?;
    let ic = fwnode.interrupt_parent().ok_or(DevError::NoIrqDomain)?;
    let domain = find_irq_domain_by_fwnode(ic.as_ref()).expect("ic exists but no domain found");
    let intr_info_raw = fwnode.interrupt_info().ok_or(DevError::NoInterruptInfo)?;

    let InterruptInfo { hwirq, trigger } = domain
        .ops
        .xlate(intr_info_raw)
        .ok_or(DevError::InvalidInterruptInfo)?;

    let virq = if let Some(virq) = domain.hw2virt(hwirq) {
        virq
    } else {
        let virq = unsafe { alloc_virq() };
        domain.map(virq, hwirq);

        // TODO: custom flow handler.

        let desc = IrqDesc {
            virq,
            hwirq,
            is_masked: AtomicBool::new(true),
            trigger,
            flow: match trigger {
                IrqTriggerType::Edge => &EdgeFlow,
                IrqTriggerType::Level => &LevelFlow,
            },
            domain: domain.clone(),
            handlers: LinkedList::new(IrqHandlerAdapter::new()),
        };
        assert!(IRQ_DESCS.write_irqsave().insert(virq, desc).is_none());

        virq
    };

    {
        let mut descs = IRQ_DESCS.write_irqsave();
        let desc = descs.get_mut(&virq).expect("desc must exist");
        desc.handlers.push_back(handler);
    }

    kdebugln!(
        "requested irq: dev={:?}, handler={:?}, virq={:?}, hwirq={:?}",
        dev,
        handler,
        virq,
        hwirq
    );

    Ok(())
}

pub fn handle_irq() {
    todo!()
}
