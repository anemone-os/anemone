use core::fmt::Write;

use super::{UartLineConfig, UartParity};

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

    pub(super) fn init_line(&self, divisor: u16, line: UartLineConfig) {
        self.write_reg(REG_IER_DLM, 0);
        self.write_reg(REG_IIR_FCR, FCR_ENABLE_FIFO | FCR_CLEAR_RX | FCR_CLEAR_TX);
        self.write_reg(REG_LCR, line_control_bits(line));
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
