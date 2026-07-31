use core::marker::PhantomData;

use loongArch64::iocsr::{iocsr_read_d, iocsr_write_d, iocsr_write_w};

use crate::{
    const_assert,
    device::discovery::fwnode::FwNode,
    mm::remap::{IoRemap, ioremap},
    prelude::*,
    utils::mmio::{CombinedReadOnly, CombinedReadPure, CombinedReadPureWrite, CombinedWriteOnly},
};

const PCH_PIC_IRQ_COUNT: usize = 64;
const EIOINTC_VECTOR_COUNT: usize = 256;

const EIOINTC_MISC_FUNC: usize = 0x420;
const EIOINTC_MISC_ENABLE: u64 = 1 << 48;
const EIOINTC_NODEMAP: usize = 0x14a0;
const EIOINTC_IPMAP: usize = 0x14c0;
const EIOINTC_ENABLE: usize = 0x1600;
const EIOINTC_BOUNCE: usize = 0x1680;
const EIOINTC_ISR: usize = 0x1800;
const EIOINTC_ROUTE: usize = 0x1c00;

pub struct PlaticRegisters<'a> {
    base: *mut u8,
    lifetime: PhantomData<&'a ()>,
}

macro_rules! impl_reg {
    (item, $name: ident, $type:ident, $offset: expr) => {
        pub fn $name<'b>(&'b mut self) -> $type<'b> {
            unsafe { $type::new(self.base as usize + $offset) }.expect(concat!(
                "Failed to access register '",
                stringify!($name),
                "'"
            ))
        }
    };
    (item, index, $name: ident, $type:ident, $offset: expr, $size: expr, $max: expr) => {
        pub fn $name<'b, const I: usize>(&'b mut self) -> $type<'b> {
            const_assert!(I < $max, "Index out of bounds");
            unsafe { $type::new(self.base as usize + $offset + I * $size) }.expect(concat!(
                "Failed to access register '",
                stringify!($name),
                "'"
            ))
        }
    };
}

impl<'a> PlaticRegisters<'a> {
    impl_reg!(item, intc_id, CombinedReadOnly, 0x0);
    impl_reg!(item, int_mask, CombinedReadPureWrite, 0x20);
    impl_reg!(item, ht_msi_enable, CombinedReadPureWrite, 0x40);
    impl_reg!(item, int_mode, CombinedReadPureWrite, 0x60);
    impl_reg!(item, int_clr, CombinedWriteOnly, 0x80);
    impl_reg!(item, index, ctrl, CombinedReadPureWrite, 0xc0, 0x20, 2);
    impl_reg!(
        item,
        index,
        route_entry,
        CombinedReadPureWrite,
        0x100,
        0x8,
        8
    );
    impl_reg!(
        item,
        index,
        ht_msi_vec,
        CombinedReadPureWrite,
        0x200,
        0x8,
        8
    );
    impl_reg!(item, index, route_int_isr, CombinedReadPure, 0x300, 0x20, 2);
    impl_reg!(item, int_irr, CombinedReadPure, 0x380);
    impl_reg!(item, int_isr, CombinedReadPure, 0x3a0);
    impl_reg!(item, int_polarity, CombinedReadPureWrite, 0x3e0);
}

#[derive(Debug)]
pub struct LA7A1000Platic {
    remap: IoRemap,
    /// Stable boot-time snapshot of the immutable DT PCH-to-EIO vector mapping.
    pic_base_vec: usize,
    register_lock: SpinLock<()>,
}

impl IrqChip for LA7A1000Platic {
    fn mask(&self, irq: HwIrq) {
        let hwirq = irq.get();
        let Some(vector) = self.eio_vector(hwirq) else {
            kwarningln!("7a1000la-platic: refusing to mask invalid hwirq {}", hwirq);
            return;
        };
        let _guard = self.register_lock.lock_irqsave();

        // PCH MASK uses one to mean masked, unlike the old implementation.
        self.regs().int_mask().write_bit(hwirq, true);
        Self::set_eio_enable(vector, false);
    }

    fn unmask(&self, irq: HwIrq) {
        let hwirq = irq.get();
        let Some(vector) = self.eio_vector(hwirq) else {
            kwarningln!(
                "7a1000la-platic: refusing to unmask invalid hwirq {}",
                hwirq
            );
            return;
        };
        let _guard = self.register_lock.lock_irqsave();

        // LevelFlow has already let the device handler deassert its source and
        // eoi retire the EIO vector. Clear the latched PCH cause before opening
        // the child and parent gates so stale state cannot be replayed.
        unsafe {
            self.regs().int_clr().write(1u64 << hwirq);
        }
        self.regs().int_mask().write_bit(hwirq, false);
        Self::set_eio_enable(vector, true);
    }

    fn ack(&self, irq: HwIrq) {
        let hwirq = irq.get();
        let Some(vector) = self.eio_vector(hwirq) else {
            kwarningln!(
                "7a1000la-platic: refusing to acknowledge invalid hwirq {}",
                hwirq
            );
            return;
        };
        let _guard = self.register_lock.lock_irqsave();

        // The current DT translation rejects edge sources, but keep the trait
        // operation complete and hardware-local rather than leaving a panic if
        // an internal caller ever acknowledges a mapped source directly.
        unsafe {
            self.regs().int_clr().write(1u64 << hwirq);
        }
        Self::clear_eio_pending(vector);
    }

    fn eoi(&self, irq: HwIrq) {
        let hwirq = irq.get();
        let Some(vector) = self.eio_vector(hwirq) else {
            kwarningln!(
                "7a1000la-platic: refusing to complete invalid hwirq {}",
                hwirq
            );
            return;
        };
        let _guard = self.register_lock.lock_irqsave();
        Self::clear_eio_pending(vector);
    }

    fn xlate(&self, spec: InterruptSpecifier<'_>) -> Option<InterruptInfo> {
        if spec.raw.len() != 8 {
            kwarningln!(
                "7a1000la-platic: invalid interrupt specifier length: {}",
                spec.raw.len()
            );
            return None;
        }
        let hwirq = u32::from_be_bytes(spec.raw[0..4].try_into().ok()?) as usize;
        let interrupt_type = u32::from_be_bytes(spec.raw[4..8].try_into().ok()?);
        if hwirq >= PCH_PIC_IRQ_COUNT {
            kwarningln!("7a1000la-platic: invalid hwirq {}", hwirq);
            return None;
        }
        // QEMU's PCH-PIC DT uses IRQ_TYPE_LEVEL_HIGH. The generic trigger type
        // currently loses polarity, so accepting level-low or either edge here
        // would advertise behavior this driver has not configured or proved.
        if interrupt_type != 4 {
            kwarningln!(
                "7a1000la-platic: unsupported interrupt type {:#x} for hwirq {}",
                interrupt_type,
                hwirq
            );
            return None;
        }
        Some(InterruptInfo {
            hwirq: HwIrq::new(hwirq),
            trigger: IrqTriggerType::Level,
            flow: IrqFlowType::LevelMaskEoi,
        })
    }

    fn as_core_irq_chip(&self) -> Option<&dyn CoreIrqChip> {
        Some(self)
    }
}

impl CoreIrqChip for LA7A1000Platic {
    fn init(fwnode: &dyn FwNode) -> Box<dyn CoreIrqChip> {
        if let Some(ofnode) = fwnode.as_of_node() {
            if ofnode.node().interrupt_cells() != Some(2) {
                panic!("7a1000la-platic: requires #interrupt-cells = <2>");
            }
            let reg = ofnode
                .node()
                .reg()
                .expect("failed to read reg property from platic node");
            let (base, len) = {
                let mut it = reg.iter();
                let first = it
                    .next()
                    .expect("platic node must have exactly one reg region");
                if it.next().is_some() {
                    panic!("platic node must have exactly one reg region");
                }
                (PhysAddr::new(first.0), first.1)
            };
            if len < 0x400 {
                panic!("7a1000la-platic: reg region is smaller than 0x400 bytes");
            }
            let pic_base_vec = fwnode
                .prop_read_u32("loongson,pic-base-vec")
                .expect("7a1000la-platic: missing loongson,pic-base-vec")
                as usize;
            let pic_vector_end = pic_base_vec
                .checked_add(PCH_PIC_IRQ_COUNT)
                .expect("7a1000la-platic: pic vector range overflow");
            if pic_vector_end > EIOINTC_VECTOR_COUNT {
                panic!(
                    "7a1000la-platic: PCH vector range {:#x}..{:#x} exceeds EIOINTC",
                    pic_base_vec, pic_vector_end
                );
            }
            let remap =
                unsafe { ioremap(base, len as usize) }.expect("failed to remap PCH-PIC registers");
            let mut platic = Self {
                remap,
                pic_base_vec,
                register_lock: SpinLock::new(()),
            };

            Self::init_eiointc();
            platic.init_pch_pic();

            kdebugln!(
                "platic: base = {:#x}, len = {:#x}, pic base vector = {:#x}, intc id = {:#x}",
                base.get(),
                len,
                pic_base_vec,
                platic.regs().intc_id().read()
            );
            Box::new(platic)
        } else {
            unimplemented!("only open firmware node is supported for now");
        }
    }

    fn claim(&self) -> Option<HwIrq> {
        for group in 0..(EIOINTC_VECTOR_COUNT / 64) {
            let enabled = iocsr_read_d(EIOINTC_ENABLE + group * 8);
            let mut pending = iocsr_read_d(EIOINTC_ISR + group * 8) & enabled;
            while pending != 0 {
                let vector = group * 64 + pending.trailing_zeros() as usize;
                if (self.pic_base_vec..self.pic_base_vec + PCH_PIC_IRQ_COUNT).contains(&vector) {
                    return Some(HwIrq::new(vector - self.pic_base_vec));
                }
                pending &= pending - 1;
            }
        }
        None
    }
}

impl LA7A1000Platic {
    fn init_eiointc() {
        iocsr_write_d(
            EIOINTC_MISC_FUNC,
            iocsr_read_d(EIOINTC_MISC_FUNC) | EIOINTC_MISC_ENABLE,
        );

        // Begin masked and clear inherited pending state before publishing any
        // PCH source. All first-version QEMU routes target node 0 / CPU 0 / HWI1.
        for group in 0..(EIOINTC_VECTOR_COUNT / 64) {
            iocsr_write_d(EIOINTC_ENABLE + group * 8, 0);
            iocsr_write_d(EIOINTC_ISR + group * 8, u64::MAX);
        }
        for word in 0..(EIOINTC_VECTOR_COUNT / 32) {
            let node_map = ((1u32 << (word * 2 + 1)) << 16) | (1u32 << (word * 2));
            iocsr_write_w(EIOINTC_NODEMAP + word * 4, node_map);
            iocsr_write_w(EIOINTC_BOUNCE + word * 4, u32::MAX);
        }
        for word in 0..(EIOINTC_VECTOR_COUNT / 128) {
            iocsr_write_w(EIOINTC_IPMAP + word * 4, 0x0202_0202);
        }
        for word in 0..(EIOINTC_VECTOR_COUNT / 4) {
            iocsr_write_w(EIOINTC_ROUTE + word * 4, 0x0101_0101);
        }
    }

    fn init_pch_pic(&self) {
        let mut regs = self.regs();
        unsafe {
            regs.int_mask().write(u64::MAX);
            regs.int_clr().write(u64::MAX);
            regs.ht_msi_enable().write(u64::MAX);
            regs.int_mode().write(0);
            regs.int_polarity().write(0);
            regs.ctrl::<0>().write(0);
            regs.ctrl::<1>().write(0);

            regs.route_entry::<0>().write(0x0101_0101_0101_0101);
            regs.route_entry::<1>().write(0x0101_0101_0101_0101);
            regs.route_entry::<2>().write(0x0101_0101_0101_0101);
            regs.route_entry::<3>().write(0x0101_0101_0101_0101);
            regs.route_entry::<4>().write(0x0101_0101_0101_0101);
            regs.route_entry::<5>().write(0x0101_0101_0101_0101);
            regs.route_entry::<6>().write(0x0101_0101_0101_0101);
            regs.route_entry::<7>().write(0x0101_0101_0101_0101);

            regs.ht_msi_vec::<0>()
                .write(Self::ht_vector_word(self.pic_base_vec, 0));
            regs.ht_msi_vec::<1>()
                .write(Self::ht_vector_word(self.pic_base_vec, 1));
            regs.ht_msi_vec::<2>()
                .write(Self::ht_vector_word(self.pic_base_vec, 2));
            regs.ht_msi_vec::<3>()
                .write(Self::ht_vector_word(self.pic_base_vec, 3));
            regs.ht_msi_vec::<4>()
                .write(Self::ht_vector_word(self.pic_base_vec, 4));
            regs.ht_msi_vec::<5>()
                .write(Self::ht_vector_word(self.pic_base_vec, 5));
            regs.ht_msi_vec::<6>()
                .write(Self::ht_vector_word(self.pic_base_vec, 6));
            regs.ht_msi_vec::<7>()
                .write(Self::ht_vector_word(self.pic_base_vec, 7));
        }
    }

    fn ht_vector_word(base: usize, group: usize) -> u64 {
        let mut word = 0;
        for byte in 0..8 {
            word |= ((base + group * 8 + byte) as u64) << (byte * 8);
        }
        word
    }

    fn eio_vector(&self, hwirq: usize) -> Option<usize> {
        (hwirq < PCH_PIC_IRQ_COUNT).then_some(self.pic_base_vec + hwirq)
    }

    fn set_eio_enable(vector: usize, enabled: bool) {
        let register = EIOINTC_ENABLE + vector / 64 * 8;
        let bit = 1u64 << (vector % 64);
        let current = iocsr_read_d(register);
        iocsr_write_d(
            register,
            if enabled {
                current | bit
            } else {
                current & !bit
            },
        );
    }

    fn clear_eio_pending(vector: usize) {
        iocsr_write_d(EIOINTC_ISR + vector / 64 * 8, 1u64 << (vector % 64));
    }

    fn regs<'a>(&'a self) -> PlaticRegisters<'a> {
        unsafe {
            PlaticRegisters {
                base: self.remap.as_ptr().as_ptr().cast(),
                lifetime: PhantomData,
            }
        }
    }
}
