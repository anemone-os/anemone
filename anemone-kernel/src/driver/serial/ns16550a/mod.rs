//! NS16550A serial port driver code.
//!
//! References:
//! - https://datasheet4u.com/datasheets/National-Semiconductor/NS16550A/605590
//! - https://www.kernel.org/doc/Documentation/devicetree/bindings/serial/8250.yaml

use core::ops::DerefMut;

use crate::{
    device::{
        bus::platform::{self, PlatformDriver},
        char::register_char_device,
        console::{ConsoleFlags, register_console},
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
        resource::Resource,
    },
    mm::remap::ioremap,
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

mod port;
mod regs;

use port::{BOOKKEEPER, IRQ_HANDLER, Ns16550AState, Ns16550AStateInner};
pub use regs::Ns16550ARegisters;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UartParity {
    None,
    Odd,
    Even,
}

impl UartParity {
    fn as_char(self) -> char {
        match self {
            Self::None => 'n',
            Self::Odd => 'o',
            Self::Even => 'e',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UartLineConfig {
    baud: u32,
    parity: UartParity,
    data_bits: u8,
}

impl Default for UartLineConfig {
    fn default() -> Self {
        Self {
            baud: NS16550A_DEFAULT_BAUD,
            parity: UartParity::None,
            data_bits: 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UartOptionError {
    MissingBaud,
    InvalidBaud,
    UnsupportedParity,
    UnsupportedDataBits,
    UnsupportedFlowControl,
    TrailingCharacters,
}

fn parse_stdout_options(options: Option<&str>) -> Result<UartLineConfig, UartOptionError> {
    let Some(options) = options else {
        return Ok(UartLineConfig::default());
    };
    if options.is_empty() {
        return Ok(UartLineConfig::default());
    }

    let baud_end = options
        .bytes()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if baud_end == 0 {
        return Err(UartOptionError::MissingBaud);
    }
    let baud = options[..baud_end]
        .parse::<u32>()
        .map_err(|_| UartOptionError::InvalidBaud)?;
    if baud == 0 {
        return Err(UartOptionError::InvalidBaud);
    }

    let mut suffix = options[baud_end..].chars();
    let parity = match suffix.next() {
        None => UartParity::None,
        Some('n') => UartParity::None,
        Some('o') => UartParity::Odd,
        Some('e') => UartParity::Even,
        Some(_) => return Err(UartOptionError::UnsupportedParity),
    };
    let data_bits = match suffix.next() {
        None => 8,
        Some('7') => 7,
        Some('8') => 8,
        Some(_) => return Err(UartOptionError::UnsupportedDataBits),
    };

    match suffix.next() {
        None => {},
        Some('r') => return Err(UartOptionError::UnsupportedFlowControl),
        Some(_) => return Err(UartOptionError::TrailingCharacters),
    }

    Ok(UartLineConfig {
        baud,
        parity,
        data_bits,
    })
}

fn calculate_divisor(uartclk: u32, baud: u32) -> Option<u16> {
    let denom = baud.checked_mul(16)?;
    let divisor = ((uartclk as u64 + denom as u64 / 2) / denom as u64).max(1);
    u16::try_from(divisor).ok()
}

#[derive(Debug, KObject, Driver)]
struct Ns16550ADriver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

impl KObjectOps for Ns16550ADriver {}

impl DriverOps for Ns16550ADriver {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        let pdev = device
            .as_platform_device()
            .expect("platform driver should only be probed with platform device");

        let fwnode = pdev.fwnode().ok_or(SysError::MissingFwNode)?;
        let stdout = fwnode.stdout_config();
        let line = match stdout {
            Some(config) => {
                let options = parse_stdout_options(config.options()).unwrap_or_else(|error| {
                    // The platform bus currently logs and swallows ordinary probe
                    // failures. An explicitly selected boot console must instead
                    // fail closed: silently falling back can leave the system on a
                    // different baud/parity without an observable diagnostic. Move
                    // this policy to the boot coordinator once required-device
                    // probe failures can propagate through the bus.
                    panic!(
                        "{}: invalid stdout UART options {:?}: {:?}",
                        pdev.name(),
                        config.options(),
                        error
                    )
                });
                kdebugln!(
                    "{}: read stdout UART options {:?} -> {:?}",
                    pdev.name(),
                    config.options(),
                    options
                );
                options
            },
            None => {
                kdebugln!(
                    "{}: no stdout UART options found, using default",
                    pdev.name()
                );
                UartLineConfig::default()
            },
        };
        let uartclk = fwnode
            .prop_read_u32("clock-frequency")
            .ok_or(SysError::FwNodeLookupFailed)?;

        let reg_shift = fwnode.prop_read_u32("reg-shift").unwrap_or(0) as usize;
        let reg_io_width = fwnode.prop_read_u32("reg-io-width").unwrap_or(1) as usize;
        if !matches!(reg_io_width, 1 | 2 | 4) {
            kerrln!(
                "{}: unsupported reg-io-width={}, expected one of {{1,2,4}}",
                pdev.name(),
                reg_io_width
            );
            return Err(SysError::FwNodeLookupFailed);
        }

        let divisor = match calculate_divisor(uartclk, line.baud) {
            Some(divisor) => divisor,
            None if stdout.is_some() => {
                panic!(
                    "{}: stdout baud {} cannot be derived from UART clock {}",
                    pdev.name(),
                    line.baud,
                    uartclk
                );
            },
            None => return Err(SysError::FwNodeLookupFailed),
        };

        let (base, len) = pdev
            .resources()
            .iter()
            .find_map(|resource| match resource {
                Resource::Mmio { base, len } => Some((*base, *len)),
            })
            .ok_or(SysError::MissingResource)?;

        let remap = unsafe { ioremap(base, len) }?;
        let regs = unsafe {
            Ns16550ARegisters::from_raw(remap.as_ptr().as_ptr().cast(), reg_shift, reg_io_width)
        };

        regs.init_line(divisor, line);

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
            let (minor_alloc, _) = guard.deref_mut();
            let minor = minor_alloc.alloc().ok_or(SysError::NoMinorAvailable)?;
            minor
        };

        let state = Ns16550AState {
            rc: Arc::new(Ns16550AStateInner {
                devnum: CharDevNum::new(MajorNum::new(devnum::char::major::RAW_SERIAL), minor),
                base,
                reg_shift,
                reg_io_width,
                remap,
            }),
        };

        let prev = BOOKKEEPER
            .lock_irqsave()
            .deref_mut()
            .1
            .insert(minor, state.clone());
        assert!(
            prev.is_none(),
            "minor number {} is already taken",
            minor.get()
        );

        pdev.set_drv_state(AnyOpaque::new(state.clone()));

        register_char_device(format!("{}", pdev.name()), Arc::new(state.clone()))?;

        let mut flags = ConsoleFlags::empty();
        if stdout.is_some() {
            flags |= ConsoleFlags::ENABLE_ON_BOOT;
            kinfoln!(
                "{}: registered as stdout console ({}{}{})",
                pdev.name(),
                line.baud,
                line.parity.as_char(),
                line.data_bits
            );
        }
        register_console(Arc::new(state), flags);

        // indeed we should pass state as private data here to save time in irq
        // handling.
        //
        // following code is just a diliberate demonstration of how to use minor number
        // as a key to retrieve device state.
        request_irq(pdev, &IRQ_HANDLER, Some(AnyOpaque::new(minor)))?;

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

#[kunit]
fn stdout_options_parser() {
    let default = UartLineConfig::default();
    assert_eq!(parse_stdout_options(None), Ok(default));
    assert_eq!(parse_stdout_options(Some("")), Ok(default));
    assert_eq!(
        parse_stdout_options(Some("115200")),
        Ok(UartLineConfig {
            baud: 115200,
            parity: UartParity::None,
            data_bits: 8,
        })
    );
    assert_eq!(
        parse_stdout_options(Some("9600e7")),
        Ok(UartLineConfig {
            baud: 9600,
            parity: UartParity::Even,
            data_bits: 7,
        })
    );
    assert_eq!(
        parse_stdout_options(Some("9600o8")),
        Ok(UartLineConfig {
            baud: 9600,
            parity: UartParity::Odd,
            data_bits: 8,
        })
    );
    assert_eq!(
        parse_stdout_options(Some("0")),
        Err(UartOptionError::InvalidBaud)
    );
    assert_eq!(
        parse_stdout_options(Some("115200x8")),
        Err(UartOptionError::UnsupportedParity)
    );
    assert_eq!(
        parse_stdout_options(Some("115200n9")),
        Err(UartOptionError::UnsupportedDataBits)
    );
    assert_eq!(
        parse_stdout_options(Some("115200n8r")),
        Err(UartOptionError::UnsupportedFlowControl)
    );
}

#[kunit]
fn stdout_line_control_bits() {
    assert_eq!(
        regs::line_control_bits(UartLineConfig {
            baud: 115200,
            parity: UartParity::None,
            data_bits: 8,
        }),
        0b0000_0011
    );
    assert_eq!(
        regs::line_control_bits(UartLineConfig {
            baud: 9600,
            parity: UartParity::Odd,
            data_bits: 7,
        }),
        0b0000_1010
    );
    assert_eq!(
        regs::line_control_bits(UartLineConfig {
            baud: 9600,
            parity: UartParity::Even,
            data_bits: 7,
        }),
        0b0001_1010
    );
}

#[initcall(driver)]
fn init() {
    let kobj_base = KObjectBase::new(KObjIdent::try_from("ns16550a").unwrap());
    let drv_base = DriverBase::new();
    let driver = Arc::new(Ns16550ADriver {
        kobj_base,
        drv_base,
    });

    platform::register_driver(driver);
}
