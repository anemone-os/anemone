use core::{marker::PhantomData, ptr::NonNull};

use safe_mmio::{
    UniqueMmioPointer, field,
    fields::{ReadOnly, ReadPure, ReadPureWrite, ReadWrite, WriteOnly},
};

use crate::{
    const_assert,
    device::discovery::{fwnode::FwNode, open_firmware::OpenFirmwareNode},
    mm::{
        kptable::{self, KERNEL_PTABLE},
        remap::{IoRemap, ioremap},
    },
    prelude::*,
    static_assert,
};

macro_rules! define_combined {
    (read, $type: ident, $name: ident) => {
        paste::paste! {
            #[repr(C)]
            struct [<$name Fields>]{
                lower: $type<u32>,
                upper: $type<u32>,
            }
            pub struct $name<'a>{
                fields: UniqueMmioPointer<'a, [<$name Fields>]>,
            }

            impl<'a> $name<'a>{
                pub unsafe fn new(address: usize) -> Option<Self> {
                    unsafe {
                        Some(Self {
                            fields: UniqueMmioPointer::new(NonNull::new(address as _)?),
                        })
                    }
                }
                pub fn read_lower(&mut self) -> u32 {
                    field!(self.fields, lower).read()
                }
                pub fn read_higher(&mut self) -> u32 {
                    field!(self.fields, upper).read()
                }
                pub fn read(&mut self) -> u64 {
                    let lower = self.read_lower() as u64;
                    let higher = self.read_higher() as u64;
                    (higher << 32) | lower
                }
                pub fn read_bit(&mut self, bit: usize) -> bool {
                    assert!(bit < 64);
                    if bit < 32 {
                        self.read_lower() & (1 << bit) != 0
                    } else {
                        self.read_higher() & (1 << (bit - 32)) != 0
                    }
                }
            }
        }
    };
    (rw, $type: ident, $name: ident) => {
        paste::paste! {
            #[repr(C)]
            struct [<$name Fields>]{
                lower: $type<u32>,
                upper: $type<u32>,
            }
            pub struct $name<'a>{
                fields: UniqueMmioPointer<'a, [<$name Fields>]>,
            }

            impl<'a> $name<'a>{
                pub unsafe fn new(address: usize) -> Option<Self> {
                    unsafe {
                        Some(Self {
                            fields: UniqueMmioPointer::new(NonNull::new(address as _)?),
                        })
                    }
                }
                pub fn read_lower(&mut self) -> u32 {
                    field!(self.fields, lower).read()
                }
                pub fn read_higher(&mut self) -> u32 {
                    field!(self.fields, upper).read()
                }

                pub fn read(&mut self) -> u64 {
                    let lower = self.read_lower() as u64;
                    let higher = self.read_higher() as u64;
                    (higher << 32) | lower
                }
                pub fn read_bit(&mut self, bit: usize) -> bool {
                    assert!(bit < 64);
                    if bit < 32 {
                        self.read_lower() & (1 << bit) != 0
                    } else {
                        self.read_higher() & (1 << (bit - 32)) != 0
                    }
                }

                pub fn write_lower(&mut self, value: u32) {
                    field!(self.fields, lower).write(value)
                }
                pub fn write_higher(&mut self, value: u32) {
                    field!(self.fields, upper).write(value)
                }

                pub unsafe fn write(&mut self, value: u64) {
                    let lower = value as u32;
                    let higher = (value >> 32) as u32;
                    self.write_lower(lower);
                    self.write_higher(higher);
                }

                pub fn write_bit(&mut self, bit: usize, set: bool) {
                    assert!(bit < 64);
                    if bit < 32 {
                        let mut val = self.read_lower();
                        if set {
                            val |= 1 << bit;
                        } else {
                            val &= !(1 << bit);
                        }
                        self.write_lower(val);
                    } else {
                        let mut val = self.read_higher();
                        if set {
                            val |= 1 << (bit - 32);
                        } else {
                            val &= !(1 << (bit - 32));
                        }
                        self.write_higher(val);
                    }
                }
            }
        }
    };

    (write, $type: ident, $name: ident) => {
        paste::paste! {
            #[repr(C)]
            struct [<$name Fields>]{
                lower: $type<u32>,
                upper: $type<u32>,
            }
            pub struct $name<'a>{
                fields: UniqueMmioPointer<'a, [<$name Fields>]>,
            }

            impl<'a> $name<'a>{
                pub unsafe fn new(address: usize) -> Option<Self> {
                    unsafe {
                        Some(Self {
                            fields: UniqueMmioPointer::new(NonNull::new(address as _)?),
                        })
                    }
                }
                pub fn write_lower(&mut self, value: u32) {
                    field!(self.fields, lower).write(value)
                }
                pub fn write_higher(&mut self, value: u32) {
                    field!(self.fields, upper).write(value)
                }

                pub unsafe fn write(&mut self, value: u64) {
                    let lower = value as u32;
                    let higher = (value >> 32) as u32;
                    self.write_lower(lower);
                    self.write_higher(higher);
                }
            }
        }
    };
}

define_combined!(read, ReadOnly, CombinedReadOnly);
define_combined!(read, ReadPure, CombinedReadPure);
define_combined!(rw, ReadPureWrite, CombinedReadPureWrite);
define_combined!(rw, ReadWrite, CombinedReadWrite);
define_combined!(write, WriteOnly, CombinedWriteOnly);
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
    impl_reg!(item, int_polarity, CombinedReadPure, 0x3e0);
}

#[derive(Debug)]
pub struct LA7A1000Platic {
    remap: IoRemap,
}

impl IrqChip for LA7A1000Platic {
    fn mask(&self, irq: HwIrq) {
        let hwirq = irq.get();
        self.regs().int_mask().write_bit(hwirq, false);
    }

    fn unmask(&self, irq: HwIrq) {
        kdebugln!("7a1000la-platic: unmasking irq {:?}", irq);
        let hwirq = irq.get();
        self.regs().int_mask().write_bit(hwirq, true);
    }

    fn ack(&self, irq: HwIrq) {
        todo!()
    }

    fn eoi(&self, irq: HwIrq) {
        todo!()
    }

    fn xlate(&self, spec: InterruptSpecifier<'_>) -> Option<InterruptInfo> {
        if spec.fwnode.as_of_node().is_some() {
            if spec.raw.len() != 8 {
                kwarningln!(
                    "7a1000la-platic: invalid interrupt specifier length: {}",
                    spec.raw.len()
                );
                return None;
            }
            Some(InterruptInfo::parse_2_cell_specifier(spec)?)
        } else {
            None
        }
    }
}

impl CoreIrqChip for LA7A1000Platic {
    fn init(fwnode: &dyn FwNode) -> Box<dyn CoreIrqChip> {
        if let Some(ofnode) = fwnode.as_of_node() {
            if let Some(cell_width) = ofnode.node().interrupt_cells()
                && cell_width != 2
            {
                panic!(
                    "7a1000la-platic: unsupported or invalid interrupt cells width: {}",
                    cell_width
                );
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
            let remap =
                unsafe { ioremap(base, len as usize) }.expect("failed to remap plic registers");
            PagingArch::tlb_shootdown_all();
            let mut platic = Self { remap };

            kdebugln!(
                "platic: base = {:#x}, len = {:#x}, intc id = {:#x}",
                base.get(),
                len,
                platic.regs().intc_id().read()
            );
            Box::new(platic)
        } else {
            unimplemented!("only open firmware node is supported for now");
        }
    }

    fn claim(&self) -> Option<HwIrq> {
        todo!()
    }
}

impl LA7A1000Platic {
    fn regs<'a>(&self) -> PlaticRegisters<'a> {
        unsafe {
            PlaticRegisters {
                base: self.remap.as_ptr().as_ptr().cast(),
                lifetime: PhantomData,
            }
        }
    }
}
