//! UART16550 and UART16550Dw serial port drivers.
//!
//! References:
//! - https://datasheet4u.com/datasheets/National-Semiconductor/NS16550A/605590
//! - https://www.kernel.org/doc/Documentation/devicetree/bindings/serial/8250.yaml
//! - https://github.com/torvalds/linux/blob/master/Documentation/devicetree/bindings/serial/snps-dw-apb-uart.yaml

use crate::{
    device::{
        bus::platform::{self, PlatformDriver},
        console::{ConsoleFlags, ConsoleTerminalIdentity, register_console_with_terminal_identity},
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
        resource::Resource,
        tty::TtyPortId,
    },
    mm::remap::ioremap,
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

mod port;
mod regs;

use port::{AppliedLine, Uart16550Device};
pub use regs::Ns16550ARegisters;
use regs::UartVariant;

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
struct Uart16550Driver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

#[derive(Debug, KObject, Driver)]
struct Uart16550DwDriver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

static UART16550_DRIVER: Lazy<Arc<Uart16550Driver>> = Lazy::new(|| {
    Arc::new(Uart16550Driver {
        kobj_base: KObjectBase::new(KObjIdent::try_from("uart16550").unwrap()),
        drv_base: DriverBase::new(),
    })
});

static UART16550_DW_DRIVER: Lazy<Arc<Uart16550DwDriver>> = Lazy::new(|| {
    Arc::new(Uart16550DwDriver {
        kobj_base: KObjectBase::new(KObjIdent::try_from("uart16550-dw").unwrap()),
        drv_base: DriverBase::new(),
    })
});

impl KObjectOps for Uart16550Driver {}
impl KObjectOps for Uart16550DwDriver {}

fn probe_uart16550(device: Arc<dyn Device>, variant: UartVariant) -> Result<(), SysError> {
    let pdev = device
        .as_platform_device()
        .expect("platform driver should only be probed with platform device");

    let fwnode = pdev.fwnode().ok_or(SysError::MissingFwNode)?;
    let of_path = fwnode
        .as_of_node()
        .ok_or(SysError::MissingFwNode)?
        .node()
        .path();
    let port_id = TtyPortId::try_from(of_path.as_str())?;
    let console_terminal_identity = ConsoleTerminalIdentity::try_from_str(port_id.as_str())?;
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
    let uartclk = fwnode.prop_read_u32("clock-frequency").unwrap_or_else(|| {
        // Some firmware describes the UART input only through a clock
        // provider, which Anemone cannot resolve yet. Keep the console
        // usable with an observable fallback until baudclk resolution is
        // available, then remove this compatibility path.
        kwarningln!(
            "{}: clock-frequency is missing; using fallback UART clock {} Hz",
            pdev.name(),
            NS16550A_FALLBACK_CLOCK_HZ
        );
        NS16550A_FALLBACK_CLOCK_HZ
    });

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
        Ns16550ARegisters::from_raw_variant(
            remap.as_ptr().as_ptr().cast(),
            reg_shift,
            reg_io_width,
            variant,
        )
    };

    regs.init_line_quiescent(divisor, line)?;

    let (state, console) = Uart16550Device::new(
        port_id,
        base,
        reg_shift,
        reg_io_width,
        variant,
        remap,
        AppliedLine::new(line, divisor),
    )?;
    pdev.set_drv_state(AnyOpaque::new(state));

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
    register_console_with_terminal_identity(console, flags, Some(console_terminal_identity));

    kinfoln!("{}: probed with RX quiescent", pdev.name());

    Ok(())
}

impl DriverOps for Uart16550Driver {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        probe_uart16550(device, UartVariant::Uart16550)
    }

    fn shutdown(&self, _device: &dyn Device) {}

    fn as_platform_driver(&self) -> Option<&dyn PlatformDriver> {
        Some(self)
    }
}

impl PlatformDriver for Uart16550Driver {
    fn match_table(&self) -> &[&str] {
        &["ns16550a"]
    }
}

impl DriverOps for Uart16550DwDriver {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        let fwnode = device.fwnode().ok_or(SysError::MissingFwNode)?;
        // The binding defines this property only for DW instances that do not
        // implement the busy functionality. Such ports retain standard 16550
        // IIR and line-programming behavior while remaining owned by this
        // binding-specific driver.
        let busy_detect = fwnode.prop_read_raw("snps,uart-16550-compatible").is_none();
        probe_uart16550(device, UartVariant::Uart16550Dw { busy_detect })
    }

    fn shutdown(&self, _device: &dyn Device) {}

    fn as_platform_driver(&self) -> Option<&dyn PlatformDriver> {
        Some(self)
    }
}

impl PlatformDriver for Uart16550DwDriver {
    fn match_table(&self) -> &[&str] {
        &["snps,dw-apb-uart"]
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
    platform::register_driver(UART16550_DRIVER.clone());
    platform::register_driver(UART16550_DW_DRIVER.clone());
}

#[initcall(late)]
fn activate_tty_ports() {
    activate_driver_tty_ports(UART16550_DRIVER.as_ref(), "Uart16550");
    activate_driver_tty_ports(UART16550_DW_DRIVER.as_ref(), "Uart16550Dw");
}

fn activate_driver_tty_ports(driver: &dyn Driver, driver_name: &str) {
    let mut device_count = 0_usize;
    driver.for_each_device(|_| {
        device_count = device_count
            .checked_add(1)
            .expect("UART16550 device count overflow");
    });

    let mut devices = Vec::new();
    if devices.try_reserve_exact(device_count).is_err() {
        kerrln!(
            "{}: failed to reserve activation snapshot for {} device(s)",
            driver_name,
            device_count
        );
        return;
    }

    driver.for_each_device(|device| {
        assert!(
            devices.len() < devices.capacity(),
            "UART16550 devices changed while taking the boot-time activation snapshot"
        );
        devices.push(device.clone());
    });
    assert_eq!(
        devices.len(),
        device_count,
        "UART16550 devices changed while taking the boot-time activation snapshot"
    );

    for device in devices {
        let state = device
            .drv_state()
            .cast::<Uart16550Device>()
            .expect("UART16550 device has invalid driver state");
        match state.activate(device.as_ref()) {
            Ok(()) => {
                kinfoln!(
                    "{}: activated Stage 1 TTY transport at {}",
                    device.name(),
                    state.port().id()
                );
            },
            Err(error) => {
                kerrln!(
                    "{}: failed to activate Stage 1 TTY transport at {}: {:?}",
                    device.name(),
                    state.port().id(),
                    error
                );
            },
        }
    }
}
