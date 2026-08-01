use core::fmt::Write;

use crate::prelude::*;

use super::{UartLineConfig, UartParity};

pub struct Ns16550ARegisters {
    base: *mut u8,
    reg_shift: usize,
    reg_io_width: usize,
    variant: UartVariant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UartVariant {
    Uart16550,
    Uart16550Dw { busy_detect: bool },
}

impl UartVariant {
    fn has_dw_busy_functionality(self) -> bool {
        matches!(self, Self::Uart16550Dw { busy_detect: true })
    }
}

const REG_RBR_THR_DLL: usize = 0;
const REG_IER_DLM: usize = 1;
const REG_IIR_FCR: usize = 2;
const REG_LCR: usize = 3;
const REG_MCR: usize = 4;
const REG_LSR: usize = 5;
const REG_MSR: usize = 6;
const DW_REG_USR: usize = 0x1f;

const LSR_DR: u8 = 1 << 0;
const LSR_OVERRUN_ERROR: u8 = 1 << 1;
const LSR_PARITY_ERROR: u8 = 1 << 2;
const LSR_FRAMING_ERROR: u8 = 1 << 3;
const LSR_BREAK_INTERRUPT: u8 = 1 << 4;
const LSR_THRE: u8 = 1 << 5;
const LSR_TRANSMITTER_EMPTY: u8 = 1 << 6;
const LSR_RX_ERROR_MASK: u8 =
    LSR_OVERRUN_ERROR | LSR_PARITY_ERROR | LSR_FRAMING_ERROR | LSR_BREAK_INTERRUPT;

const LCR_WORD_SIZE_7: u8 = 0b10;
const LCR_WORD_SIZE_8: u8 = 0b11;
const LCR_PARITY_ENABLE: u8 = 1 << 3;
const LCR_EVEN_PARITY: u8 = 1 << 4;
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
const DW_IIR_ID_MASK: u8 = 0b1111;
const DW_IIR_ID_BUSY: u8 = 0b0111;

const DW_USR_BUSY: u8 = 1 << 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InterruptReason {
    None,
    ModemStatus,
    TxHoldingEmpty,
    RxAvailable,
    RxLineStatus,
    RxTimeout,
    BusyDetect,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RxStatus {
    pub(super) data_ready: bool,
    pub(super) line_error: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RxSample {
    pub(super) byte: Option<u8>,
    pub(super) line_error: bool,
}

impl Ns16550ARegisters {
    pub unsafe fn from_raw(base: *mut u8, reg_shift: usize, reg_io_width: usize) -> Self {
        unsafe { Self::from_raw_variant(base, reg_shift, reg_io_width, UartVariant::Uart16550) }
    }

    pub(super) unsafe fn from_raw_variant(
        base: *mut u8,
        reg_shift: usize,
        reg_io_width: usize,
        variant: UartVariant,
    ) -> Self {
        Self {
            base,
            reg_shift,
            reg_io_width,
            variant,
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

    /// Apply the boot line configuration while leaving RX interrupts disabled.
    ///
    /// The normal console may transmit after this point, but the driver-local
    /// Late activation transaction must bind the TTY consumer and request the
    /// IRQ before calling [`Self::enable_rx_irq`].
    pub(super) fn init_line_quiescent(
        &self,
        divisor: u16,
        line: UartLineConfig,
    ) -> Result<(), SysError> {
        if self.variant.has_dw_busy_functionality() {
            return self.init_dw_line_quiescent(divisor, line);
        }

        self.write_reg(REG_IER_DLM, 0);
        self.write_reg(REG_IIR_FCR, FCR_ENABLE_FIFO | FCR_CLEAR_RX | FCR_CLEAR_TX);
        self.write_reg(REG_LCR, line_control_bits(line));
        self.set_divisor(divisor);
        self.write_reg(REG_MCR, MCR_DTR | MCR_RTS | MCR_OUT2);
        self.write_reg(REG_IER_DLM, 0);
        Ok(())
    }

    fn init_dw_line_quiescent(&self, divisor: u16, line: UartLineConfig) -> Result<(), SysError> {
        let lcr = line_control_bits(line);

        // DW APB UART can ignore LCR/DLL/DLH writes while USR.BUSY is set.
        // Every affected write is therefore made only while idle and verified
        // before the probe publishes this port as configured.
        if !self.write_dw_lcr_checked(lcr) {
            return Err(SysError::Timeout);
        }
        self.write_reg(REG_IER_DLM, 0);
        self.write_reg(REG_IIR_FCR, FCR_ENABLE_FIFO | FCR_CLEAR_RX | FCR_CLEAR_TX);
        if !self.write_dw_lcr_checked(lcr | LCR_DLAB) {
            return Err(SysError::Timeout);
        }
        if !self.write_dw_divisor_byte_checked(REG_RBR_THR_DLL, divisor as u8)
            || !self.write_dw_divisor_byte_checked(REG_IER_DLM, (divisor >> 8) as u8)
            || !self.write_dw_lcr_checked(lcr)
        {
            return Err(SysError::Timeout);
        }
        self.write_reg(REG_MCR, MCR_DTR | MCR_RTS | MCR_OUT2);
        self.write_reg(REG_IER_DLM, 0);
        Ok(())
    }

    fn write_dw_lcr_checked(&self, value: u8) -> bool {
        for _ in 0..NS16550A_TX_POLL_ITERATIONS {
            if !self.dw_idle() {
                continue;
            }
            self.write_reg(REG_LCR, value);
            if self.read_reg(REG_LCR) == value {
                return true;
            }
            let _ = self.read_reg(DW_REG_USR);
        }
        false
    }

    fn write_dw_divisor_byte_checked(&self, reg: usize, value: u8) -> bool {
        assert!(matches!(reg, REG_RBR_THR_DLL | REG_IER_DLM));
        for _ in 0..NS16550A_TX_POLL_ITERATIONS {
            if !self.dw_idle() {
                continue;
            }
            self.write_reg(reg, value);
            if self.read_reg(reg) == value {
                return true;
            }
            let _ = self.read_reg(DW_REG_USR);
        }
        false
    }

    /// Irreversibly enable the Stage 1 RX path after IRQ registration and
    /// attachment publication have completed.
    pub(super) fn enable_rx_irq(&self) {
        self.write_reg(REG_IER_DLM, IER_RX_AVAILABLE);
    }

    pub fn write_byte(&self, byte: u8) -> Option<u8> {
        if self.read_reg(REG_LSR) & LSR_THRE == 0 {
            return None;
        }
        self.write_reg(REG_RBR_THR_DLL, byte);
        Some(byte)
    }

    /// True only after both the holding register and shift register drain.
    pub(super) fn tx_idle(&self) -> bool {
        self.read_reg(REG_LSR) & LSR_TRANSMITTER_EMPTY != 0
    }

    pub(super) fn interrupt_reason(&self) -> InterruptReason {
        decode_interrupt_reason(self.read_reg(REG_IIR_FCR), self.variant)
    }

    pub(super) fn clear_busy_detect(&self) {
        assert!(
            self.variant.has_dw_busy_functionality(),
            "busy-detect acknowledgement requires a DW APB UART"
        );
        let _ = self.read_reg(DW_REG_USR);
    }

    /// Clear a DesignWare RX-timeout interrupt that reports neither received
    /// data nor a break. Linux's 8250_dw driver uses the same dummy RBR read to
    /// prevent the level interrupt from remaining asserted.
    pub(super) fn clear_spurious_rx_timeout(&self) -> bool {
        if !self.variant.has_dw_busy_functionality() {
            return false;
        }
        let lsr = self.read_reg(REG_LSR);
        if !should_clear_spurious_rx_timeout(self.variant, lsr) {
            return false;
        }
        let _ = self.read_reg(REG_RBR_THR_DLL);
        true
    }

    fn dw_idle(&self) -> bool {
        assert!(
            self.variant.has_dw_busy_functionality(),
            "DW busy status requires a DW APB UART"
        );
        self.read_reg(DW_REG_USR) & DW_USR_BUSY == 0
    }

    pub(super) fn interrupt_pending(&self) -> bool {
        !matches!(self.interrupt_reason(), InterruptReason::None)
    }

    pub(super) fn rx_status(&self) -> RxStatus {
        let lsr = self.read_reg(REG_LSR);
        RxStatus {
            data_ready: lsr & LSR_DR != 0,
            line_error: lsr & LSR_RX_ERROR_MASK != 0,
        }
    }

    pub(super) fn read_rx_sample(&self) -> RxSample {
        let status = self.rx_status();
        let byte = status.data_ready.then(|| self.read_reg(REG_RBR_THR_DLL));
        RxSample {
            byte,
            line_error: status.line_error,
        }
    }

    pub(super) fn clear_modem_status(&self) {
        let _ = self.read_reg(REG_MSR);
    }
}

fn decode_interrupt_reason(iir: u8, variant: UartVariant) -> InterruptReason {
    if variant.has_dw_busy_functionality() {
        match iir & DW_IIR_ID_MASK {
            IIR_NO_PENDING => return InterruptReason::None,
            DW_IIR_ID_BUSY => return InterruptReason::BusyDetect,
            _ => {},
        }
    } else if iir & IIR_NO_PENDING != 0 {
        return InterruptReason::None;
    }

    match iir & IIR_ID_MASK {
        IIR_ID_MODEM_STATUS => InterruptReason::ModemStatus,
        IIR_ID_THRE => InterruptReason::TxHoldingEmpty,
        IIR_ID_RX_AVAILABLE => InterruptReason::RxAvailable,
        IIR_ID_RX_LINE_STATUS => InterruptReason::RxLineStatus,
        IIR_ID_RX_TIMEOUT => InterruptReason::RxTimeout,
        _ => InterruptReason::Unknown,
    }
}

fn should_clear_spurious_rx_timeout(variant: UartVariant, lsr: u8) -> bool {
    variant.has_dw_busy_functionality() && lsr & (LSR_DR | LSR_BREAK_INTERRUPT) == 0
}

#[kunit]
fn interrupt_decoder_separates_ns16550a_and_dw_busy() {
    assert_eq!(
        decode_interrupt_reason(DW_IIR_ID_BUSY, UartVariant::Uart16550),
        InterruptReason::None
    );
    assert_eq!(
        decode_interrupt_reason(
            DW_IIR_ID_BUSY,
            UartVariant::Uart16550Dw { busy_detect: false }
        ),
        InterruptReason::None
    );
    assert_eq!(
        decode_interrupt_reason(
            DW_IIR_ID_BUSY,
            UartVariant::Uart16550Dw { busy_detect: true }
        ),
        InterruptReason::BusyDetect
    );
}

#[kunit]
fn interrupt_decoder_preserves_standard_rx_causes() {
    for variant in [
        UartVariant::Uart16550,
        UartVariant::Uart16550Dw { busy_detect: false },
        UartVariant::Uart16550Dw { busy_detect: true },
    ] {
        assert_eq!(
            decode_interrupt_reason(IIR_ID_RX_AVAILABLE, variant),
            InterruptReason::RxAvailable
        );
        assert_eq!(
            decode_interrupt_reason(IIR_ID_RX_LINE_STATUS, variant),
            InterruptReason::RxLineStatus
        );
        assert_eq!(
            decode_interrupt_reason(IIR_ID_RX_TIMEOUT, variant),
            InterruptReason::RxTimeout
        );
        assert_eq!(
            decode_interrupt_reason(IIR_NO_PENDING, variant),
            InterruptReason::None
        );
    }
}

#[kunit]
fn spurious_rx_timeout_clear_is_dw_only_and_preserves_breaks() {
    let dw = UartVariant::Uart16550Dw { busy_detect: true };
    assert!(should_clear_spurious_rx_timeout(dw, 0));
    assert!(!should_clear_spurious_rx_timeout(dw, LSR_DR));
    assert!(!should_clear_spurious_rx_timeout(dw, LSR_BREAK_INTERRUPT));
    assert!(!should_clear_spurious_rx_timeout(UartVariant::Uart16550, 0));
    assert!(!should_clear_spurious_rx_timeout(
        UartVariant::Uart16550Dw { busy_detect: false },
        0
    ));
}

pub(super) fn line_control_bits(line: UartLineConfig) -> u8 {
    let mut lcr = match line.data_bits {
        7 => LCR_WORD_SIZE_7,
        8 => LCR_WORD_SIZE_8,
        _ => unreachable!("UART data bits were validated while parsing console options"),
    };
    match line.parity {
        UartParity::None => {},
        UartParity::Odd => lcr |= LCR_PARITY_ENABLE,
        UartParity::Even => lcr |= LCR_PARITY_ENABLE | LCR_EVEN_PARITY,
    }
    lcr
}

impl Write for Ns16550ARegisters {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for byte in s.bytes() {
            while self.write_byte(byte).is_none() {}
        }
        Ok(())
    }
}
