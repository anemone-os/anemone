use super::ipi::IpiSend;

macro_rules! define_iocsr {
    (64, $name: ident, $num:expr) => {
        paste::paste! {
            pub const [<CR_ $name:upper>]: u16 = $num;
        }
        pub mod $name {
            use core::arch::asm;
            #[inline(always)]
            pub unsafe fn io_csr_read() -> u64 {
                let val: u64;
                // 直接用环境中已定义的CSR_NUM，无参数、无冗余
                unsafe {
                    asm!(
                        "iocsrrd.d {0}, {1}",
                        out(reg) val,
                        in(reg) $num, // 直接用环境中已定义的常量
                        options(nomem, nostack),
                    );
                }
                val
            }

            #[inline(always)]
            pub unsafe fn io_csr_write(value: u64) {
                unsafe{
                    asm!(
                        "iocsrwr.d {0}, {1}", // LoongArch写入CSR指令：csrwr 通用寄存器, CSR编号
                        in(reg) value, // 要写入的值（环境中预定义）
                        in(reg) $num,     // 环境中预定义的CSR编号
                        options(nomem, nostack)
                    );
                }
            }
        }

    };
    (64, $name: ident, $num:expr, $type: ident) => {
        paste::paste! {
            pub const [<CR_ $name:upper>]: u16 = $num;
        }
        pub mod $name {
            use core::arch::asm;
            #[inline(always)]
            pub unsafe fn io_csr_read() -> super::$type {
                let val: u64;
                // 直接用环境中已定义的CSR_NUM，无参数、无冗余
                unsafe {
                    asm!(
                        "iocsrrd.d {0}, {1}",
                        lateout(reg) val,
                        in(reg) $num, // 直接用环境中已定义的常量
                        options(nomem, nostack),
                    );
                }
                super::$type::from_u64(val)
            }

            #[inline(always)]
            pub unsafe fn io_csr_write(value: super::$type) {
                unsafe{
                    asm!(
                        "iocsrwr.d {0}, {1}", // LoongArch写入CSR指令：csrwr 通用寄存器, CSR编号
                        in(reg) value.to_u64(), // 要写入的值（环境中预定义）
                        in(reg) $num,     // 环境中预定义的CSR编号
                        options(nomem, nostack)
                    );
                }
            }
        }

    };
    (32, $name: ident, $num:expr) => {
        paste::paste! {
            pub const [<CR_ $name:upper>]: u16 = $num;
        }
        pub mod $name {
            use core::arch::asm;
            #[inline(always)]
            pub unsafe fn io_csr_read() -> u32 {
                let val: u32;
                // 直接用环境中已定义的CSR_NUM，无参数、无冗余
                unsafe {
                    asm!(
                        "iocsrrd.w {0}, {1}",
                        out(reg) val,
                        in(reg) $num, // 直接用环境中已定义的常量
                        options(nomem, nostack),
                    );
                }
                val
            }

            #[inline(always)]
            pub unsafe fn io_csr_write(value: u32) {
                unsafe{
                    asm!(
                        "iocsrwr.w {0}, {1}", // LoongArch写入CSR指令：csrwr 通用寄存器, CSR编号
                        in(reg) value, // 要写入的值（环境中预定义）
                        in(reg) $num,     // 环境中预定义的CSR编号
                        options(nomem, nostack)
                    );
                }
            }
        }

    };
    (32, $name: ident, $num:expr, $type: ident) => {
        paste::paste! {
            pub const [<CR_ $name:upper>]: u16 = $num;
        }
        pub mod $name {
            use core::arch::asm;
            #[inline(always)]
            pub unsafe fn io_csr_read() -> super::$type {
                let val: u32;
                // 直接用环境中已定义的CSR_NUM，无参数、无冗余
                unsafe {
                    asm!(
                        "iocsrrd.w {0}, {1}",
                        lateout(reg) val,
                        in(reg) $num, // 直接用环境中已定义的常量
                        options(nomem, nostack),
                    );
                }
                super::$type::from_u32(val)
            }

            #[inline(always)]
            pub unsafe fn io_csr_write(value: super::$type) {
                unsafe{
                    asm!(
                        "iocsrwr.w {0}, {1}", // LoongArch写入CSR指令：csrwr 通用寄存器, CSR编号
                        in(reg) value.to_u32(), // 要写入的值（环境中预定义）
                        in(reg) $num,     // 环境中预定义的CSR编号
                        options(nomem, nostack)
                    );
                }
            }
        }

    };
}

define_iocsr!(32, ipi_status, 0x1000);
define_iocsr!(32, ipi_enable, 0x1004);
define_iocsr!(32, ipi_set, 0x1008);
define_iocsr!(32, ipi_clear, 0x100c);

define_iocsr!(32, ipi_send, 0x1040, IpiSend);
