//! NS16550A serial port driver code.
//!
//! References:
//! - https://datasheet4u.com/datasheets/National-Semiconductor/NS16550A/605590
//! - https://www.kernel.org/doc/Documentation/devicetree/bindings/serial/8250.yaml

use core::ops::{Deref, DerefMut};

use crate::{
    device::{
        bus::platform::{self, PlatformDriver},
        char::{CharDev, CharDriver, register_char_device, register_char_driver},
        console::{Console, ConsoleFlags, register_console},
        devnum::GeneralMinorAllocator,
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
        resource::Resource,
    },
    mm::remap::{IoRemap, ioremap},
    prelude::*,
    utils::prv_data::PrvData,
};

#[derive(Debug, PrvData, Clone)]
struct Ns16550AState {
    rc: Arc<Ns16550AStateInner>,
}

impl Deref for Ns16550AState {
    type Target = Ns16550AStateInner;

    fn deref(&self) -> &Self::Target {
        &self.rc
    }
}

#[derive(Debug)]
struct Ns16550AStateInner {
    base: PhysAddr,
    reg_shift: usize,
    reg_io_width: usize,
    remap: IoRemap,
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
    fn read(&self, buf: &mut [u8]) -> Result<usize, DevError> {
        unimplemented!()
    }

    fn write(&self, buf: &[u8]) -> Result<usize, DevError> {
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

mod driver_core {
    use core::fmt::Write;

    pub struct Ns16550ARegisters {
        base: *mut u8,
        reg_shift: usize,
        reg_io_width: usize,
    }

    const REG_RBR_THR_DLL: usize = 0;
    const REG_IER_DLM: usize = 1;
    const REG_IIR_FCR: usize = 2;
    const REG_LCR: usize = 3;
    const REG_MCR: usize = 4;
    const REG_LSR: usize = 5;
    const REG_MSR: usize = 6;

    const LSR_DR: u8 = 1 << 0;
    const LSR_THRE: u8 = 1 << 5;

    const LCR_WORD_SIZE_8: u8 = 0b11;
    const LCR_DLAB: u8 = 1 << 7;

    const FCR_ENABLE_FIFO: u8 = 1 << 0;
    const FCR_CLEAR_RX: u8 = 1 << 1;
    const FCR_CLEAR_TX: u8 = 1 << 2;

    const MCR_DTR: u8 = 1 << 0;
    const MCR_RTS: u8 = 1 << 1;
    const MCR_OUT2: u8 = 1 << 3;

    const IER_RX_AVAILABLE: u8 = 1 << 0;

    const IIR_NO_PENDING: u8 = 1 << 0;
    const IIR_ID_MASK: u8 = 0b1110;
    const IIR_ID_MODEM_STATUS: u8 = 0b0000;
    const IIR_ID_THRE: u8 = 0b0010;
    const IIR_ID_RX_AVAILABLE: u8 = 0b0100;
    const IIR_ID_RX_LINE_STATUS: u8 = 0b0110;
    const IIR_ID_RX_TIMEOUT: u8 = 0b1100;

    impl Ns16550ARegisters {
        pub unsafe fn from_raw(base: *mut u8, reg_shift: usize, reg_io_width: usize) -> Self {
            Self {
                base,
                reg_shift,
                reg_io_width,
            }
        }

        fn reg_ptr(&self, reg: usize) -> *mut u8 {
            let offset = reg << self.reg_shift;
            unsafe { self.base.add(offset) }
        }

        fn read_reg(&self, reg: usize) -> u8 {
            let ptr = self.reg_ptr(reg);
            unsafe {
                match self.reg_io_width {
                    1 => core::ptr::read_volatile(ptr),
                    2 => core::ptr::read_volatile(ptr.cast::<u16>()) as u8,
                    4 => core::ptr::read_volatile(ptr.cast::<u32>()) as u8,
                    _ => unreachable!("validated reg-io-width in probe"),
                }
            }
        }

        fn write_reg(&self, reg: usize, val: u8) {
            let ptr = self.reg_ptr(reg);
            unsafe {
                match self.reg_io_width {
                    1 => core::ptr::write_volatile(ptr, val),
                    2 => core::ptr::write_volatile(ptr.cast::<u16>(), val as u16),
                    4 => core::ptr::write_volatile(ptr.cast::<u32>(), val as u32),
                    _ => unreachable!("validated reg-io-width in probe"),
                }
            }
        }

        fn set_dlab(&self, enabled: bool) {
            let mut lcr = self.read_reg(REG_LCR);
            if enabled {
                lcr |= LCR_DLAB;
            } else {
                lcr &= !LCR_DLAB;
            }
            self.write_reg(REG_LCR, lcr);
        }

        fn set_divisor(&self, divisor: u16) {
            self.set_dlab(true);
            self.write_reg(REG_RBR_THR_DLL, (divisor & 0x00ff) as u8);
            self.write_reg(REG_IER_DLM, (divisor >> 8) as u8);
            self.set_dlab(false);
        }

        pub fn init_8n1(&self, divisor: u16) {
            self.write_reg(REG_IER_DLM, 0);
            self.write_reg(REG_IIR_FCR, FCR_ENABLE_FIFO | FCR_CLEAR_RX | FCR_CLEAR_TX);
            self.write_reg(REG_LCR, LCR_WORD_SIZE_8);
            self.set_divisor(divisor);
            self.write_reg(REG_MCR, MCR_DTR | MCR_RTS | MCR_OUT2);
            self.write_reg(REG_IER_DLM, IER_RX_AVAILABLE);
        }

        pub fn write_byte(&self, byte: u8) -> Option<u8> {
            if self.read_reg(REG_LSR) & LSR_THRE == 0 {
                return None;
            }
            self.write_reg(REG_RBR_THR_DLL, byte);
            Some(byte)
        }

        pub fn try_drain_irq(&self) -> bool {
            let mut handled = false;

            loop {
                let iir = self.read_reg(REG_IIR_FCR);
                if iir & IIR_NO_PENDING != 0 {
                    break;
                }

                handled = true;
                match iir & IIR_ID_MASK {
                    IIR_ID_RX_AVAILABLE | IIR_ID_RX_TIMEOUT => {
                        while self.read_reg(REG_LSR) & LSR_DR != 0 {
                            let _ = self.read_reg(REG_RBR_THR_DLL);
                        }
                    },
                    IIR_ID_RX_LINE_STATUS => {
                        let lsr = self.read_reg(REG_LSR);
                        if lsr & LSR_DR != 0 {
                            let _ = self.read_reg(REG_RBR_THR_DLL);
                        }
                    },
                    IIR_ID_MODEM_STATUS => {
                        let _ = self.read_reg(REG_MSR);
                    },
                    IIR_ID_THRE => {
                        // THRE interrupt is normally disabled; nothing to drain
                        // here.
                    },
                    _ => {
                        break;
                    },
                }
            }

            handled
        }
    }

    impl Write for Ns16550ARegisters {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            for byte in s.bytes() {
                while self.write_byte(byte).is_none() {}
            }
            Ok(())
        }
    }
}
pub use driver_core::Ns16550ARegisters;

#[derive(Debug, KObject, Driver)]
struct Ns16550ADriver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

impl KObjectOps for Ns16550ADriver {}

impl DriverOps for Ns16550ADriver {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), DevError> {
        let pdev = device
            .as_platform_device()
            .expect("platform driver should only be probed with platform device");

        let fwnode = pdev.fwnode().ok_or(DevError::MissingFwNode)?;
        let uartclk = fwnode
            .prop_read_u32("clock-frequency")
            .ok_or(DevError::FwNodeLookupFailed)?;

        let reg_shift = fwnode.prop_read_u32("reg-shift").unwrap_or(0) as usize;
        let reg_io_width = fwnode.prop_read_u32("reg-io-width").unwrap_or(1) as usize;
        if !matches!(reg_io_width, 1 | 2 | 4) {
            kerrln!(
                "{}: unsupported reg-io-width={}, expected one of {{1,2,4}}",
                pdev.name(),
                reg_io_width
            );
            return Err(DevError::FwNodeLookupFailed);
        }

        let baud: u32 = 115200;
        let denom = baud.saturating_mul(16);
        if denom == 0 {
            return Err(DevError::FwNodeLookupFailed);
        }
        let mut divisor = ((uartclk as u64 + (denom as u64 / 2)) / denom as u64) as u32;
        if divisor == 0 {
            divisor = 1;
        }
        if divisor > u16::MAX as u32 {
            return Err(DevError::FwNodeLookupFailed);
        }

        let (base, len) = pdev
            .resources()
            .iter()
            .find_map(|resource| match resource {
                Resource::Mmio { base, len } => Some((*base, *len)),
            })
            .ok_or(DevError::MissingResource)?;

        let remap = unsafe { ioremap(base, len) }.map_err(DevError::IoRemapFailed)?;
        let regs = unsafe {
            Ns16550ARegisters::from_raw(remap.as_ptr().as_ptr().cast(), reg_shift, reg_io_width)
        };

        regs.init_8n1(divisor as u16);

        let state = Ns16550AState {
            rc: Arc::new(Ns16550AStateInner {
                base,
                reg_shift,
                reg_io_width,
                remap,
            }),
        };

        // TODO: if one of following operation fails, how can we elegantly unwind the
        // previous successful operations (e.g. free the allocated minor, unmap the MMIO
        // region, etc.)? we probably need some sort of "transaction" mechanism for
        // driver probing, just like what we did in memory management when unmapping
        // mapped pages.
        //
        // We should implement something that might be called `ProbeTransaction` or
        // `ProbeCtx`, which can keep track of the resources allocated during probing
        // and automatically free them when dropped.

        let minor = {
            let mut guard = BOOKKEEPER.lock_irqsave();
            let (minor_alloc, devices) = guard.deref_mut();
            let minor = minor_alloc.alloc().ok_or(DevError::NoMinorAvailable)?;

            let prev = devices.insert(minor, state.clone());
            debug_assert!(
                prev.is_none(),
                "minor number {} is already taken",
                minor.get()
            );
            minor
        };

        pdev.set_drv_state(Some(Box::new(state.clone())));

        register_char_device(
            DevNum::new(*MAJOR.get(), minor),
            ident_format!("{}", pdev.name()).unwrap(),
            Arc::new(state.clone()),
        )?;

        let mut flags = ConsoleFlags::empty();
        if fwnode.is_stdout() {
            flags |= ConsoleFlags::ENABLED;
            kinfoln!("{}: registered as stdout console", pdev.name());
        }
        register_console(Arc::new(state), flags);

        // indeed we should pass state as private data here to save time in irq
        // handling.
        //
        // following code is just a diliberate demonstration of how to use minor number
        // as a key to retrieve device state.
        request_irq(pdev, &IRQ_HANDLER, Some(Box::new(minor)))?;

        kinfoln!("{}: probed", pdev.name());

        Ok(())
    }

    fn shutdown(&self, device: &dyn Device) {}

    fn as_platform_driver(&self) -> Option<&dyn PlatformDriver> {
        Some(self)
    }
}

impl PlatformDriver for Ns16550ADriver {
    fn match_table(&self) -> &[&str] {
        &["ns16550a"]
    }
}

impl CharDriver for Ns16550ADriver {
    fn major(&self) -> MajorNum {
        *MAJOR.get()
    }
}

static IRQ_HANDLER: IrqHandler = IrqHandler::new(handle_irq);

fn handle_irq(prv_data: Option<&mut dyn PrvData>) {
    let minor = unsafe { *prv_data.unwrap().cast_unchecked::<MinorNum>() };

    let state = {
        let bookkeeper = BOOKKEEPER.lock_irqsave();
        let (_, devices) = bookkeeper.deref();
        devices
            .get(&minor)
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

static MAJOR: MonoOnce<MajorNum> = unsafe { MonoOnce::new() };
static BOOKKEEPER: Lazy<SpinLock<(GeneralMinorAllocator, HashMap<MinorNum, Ns16550AState>)>> =
    Lazy::new(|| SpinLock::new((GeneralMinorAllocator::new(), HashMap::new())));

#[initcall(driver)]
fn init() {
    let kobj_base = KObjectBase::new(KObjIdent::try_from("ns16550a").unwrap());
    let drv_base = DriverBase::new();
    let driver = Arc::new(Ns16550ADriver {
        kobj_base,
        drv_base,
    });

    platform::register_driver(driver.clone());
    match register_char_driver(driver) {
        Ok(m) => MAJOR.init(|major| {
            major.write(m);
        }),
        Err(e) => {
            kerrln!("failed to register ns16550a as a char driver: {:?}", e);
        },
    }
}
