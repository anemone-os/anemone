use core::ops::Deref;

use crate::{
    device::{char::CharDev, console::Console, devnum::GeneralMinorAllocator},
    mm::remap::IoRemap,
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

use super::regs::Ns16550ARegisters;

#[derive(Debug, Opaque, Clone)]
pub(super) struct Ns16550AState {
    pub(super) rc: Arc<Ns16550AStateInner>,
}

impl Deref for Ns16550AState {
    type Target = Ns16550AStateInner;

    fn deref(&self) -> &Self::Target {
        &self.rc
    }
}

#[derive(Debug)]
pub(super) struct Ns16550AStateInner {
    pub(super) devnum: CharDevNum,
    pub(super) base: PhysAddr,
    pub(super) reg_shift: usize,
    pub(super) reg_io_width: usize,
    pub(super) remap: IoRemap,
}

impl Console for Ns16550AState {
    fn output(&self, s: &str) {
        let regs = unsafe {
            Ns16550ARegisters::from_raw(
                self.remap.as_ptr().as_ptr().cast(),
                self.reg_shift,
                self.reg_io_width,
            )
        };
        let mut regs = regs;
        use core::fmt::Write;
        let _ = write!(regs, "{}", s);
    }
}

impl CharDev for Ns16550AState {
    fn devnum(&self) -> CharDevNum {
        self.devnum
    }

    fn read(&self, buf: &mut [u8]) -> Result<usize, SysError> {
        unimplemented!()
    }

    fn write(&self, buf: &[u8]) -> Result<usize, SysError> {
        let regs = unsafe {
            Ns16550ARegisters::from_raw(
                self.remap.as_ptr().as_ptr().cast(),
                self.reg_shift,
                self.reg_io_width,
            )
        };
        for byte in buf {
            while regs.write_byte(*byte).is_none() {}
        }
        Ok(buf.len())
    }
}

pub(super) static IRQ_HANDLER: IrqHandler = IrqHandler::new(handle_irq);

fn handle_irq(prv_data: &AnyOpaque) {
    let minor = prv_data.cast::<MinorNum>().unwrap();

    let state = {
        let bookkeeper = BOOKKEEPER.lock_irqsave();
        let (_, devices) = bookkeeper.deref();
        devices
            .get(minor)
            .expect("invalid minor number in irq handler")
            .clone()
    };

    let inner = &state.rc;
    let regs = unsafe {
        Ns16550ARegisters::from_raw(
            inner.remap.as_ptr().as_ptr().cast(),
            inner.reg_shift,
            inner.reg_io_width,
        )
    };

    if !regs.try_drain_irq() {
        kdebugln!("ns16550a: spurious irq at {:#x}", inner.base.get());
    }

    kdebugln!("ns16550a: handled irq at {:#x}", inner.base.get());
}

pub(super) static BOOKKEEPER: Lazy<
    SpinLock<(GeneralMinorAllocator, HashMap<MinorNum, Ns16550AState>)>,
> = Lazy::new(|| SpinLock::new((GeneralMinorAllocator::new(), HashMap::new())));
