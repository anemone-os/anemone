//! IOCSR register definitions and accessors.

use super::ipi::IpiSend;

macro_rules! define_iocsr {
    (64, $name: ident, $num:expr) => {
        paste::paste! {
            #[doc = concat!("IOCSR number for IOCSR `", stringify!($name), "`")]
            pub const [<CR_ $name:upper>]: u16 = $num;
        }
        #[doc = concat!("Accessor for IOCSR `", stringify!($name), "`")]
        pub mod $name {
            use core::arch::asm;

            /// Read the IOCSR value
            #[inline(always)]
            pub unsafe fn io_csr_read() -> u64 {
                let val: u64;
                unsafe {
                    asm!(
                        "iocsrrd.d {0}, {1}",
                        out(reg) val,
                        in(reg) $num,
                        options(nomem, nostack),
                    );
                }
                val
            }

            /// Write the IOCSR value
            #[inline(always)]
            pub unsafe fn io_csr_write(value: u64) {
                unsafe{
                    asm!(
                        "iocsrwr.d {0}, {1}",
                        in(reg) value,
                        in(reg) $num,
                        options(nomem, nostack)
                    );
                }
            }
        }

    };
    (64, $name: ident, $num:expr, $type: ident) => {
        paste::paste! {
            #[doc = concat!("IOCSR number for IOCSR `", stringify!($name), "`")]
            pub const [<CR_ $name:upper>]: u16 = $num;
        }
        #[doc = concat!("Accessor for IOCSR `", stringify!($name), "`")]
        pub mod $name {
            use core::arch::asm;

            /// Read the IOCSR value
            #[inline(always)]
            pub unsafe fn io_csr_read() -> super::$type {
                let val: u64;

                unsafe {
                    asm!(
                        "iocsrrd.d {0}, {1}",
                        lateout(reg) val,
                        in(reg) $num,
                        options(nomem, nostack),
                    );
                }
                super::$type::from_u64(val)
            }

            /// Write the IOCSR value
            #[inline(always)]
            pub unsafe fn io_csr_write(value: super::$type) {
                unsafe{
                    asm!(
                        "iocsrwr.d {0}, {1}",
                        in(reg) value.to_u64(),
                        in(reg) $num,
                        options(nomem, nostack)
                    );
                }
            }
        }

    };
    (32, $name: ident, $num:expr) => {
        paste::paste! {
            #[doc = concat!("IOCSR number for IOCSR `", stringify!($name), "`")]
            pub const [<CR_ $name:upper>]: u16 = $num;
        }
        #[doc = concat!("Accessor for IOCSR `", stringify!($name), "`")]
        pub mod $name {
            use core::arch::asm;

            /// Read the IOCSR value
            #[inline(always)]
            pub unsafe fn io_csr_read() -> u32 {
                let val: u32;

                unsafe {
                    asm!(
                        "iocsrrd.w {0}, {1}",
                        out(reg) val,
                        in(reg) $num,
                        options(nomem, nostack),
                    );
                }
                val
            }

            /// Write the IOCSR value
            #[inline(always)]
            pub unsafe fn io_csr_write(value: u32) {
                unsafe{
                    asm!(
                        "iocsrwr.w {0}, {1}",
                        in(reg) value,
                        in(reg) $num,    
                        options(nomem, nostack)
                    );
                }
            }
        }

    };
    (32, $name: ident, $num:expr, $type: ident) => {
        paste::paste! {
            #[doc = concat!("IOCSR number for IOCSR `", stringify!($name), "`")]
            pub const [<CR_ $name:upper>]: u16 = $num;
        }
        #[doc = concat!("Accessor for IOCSR `", stringify!($name), "`")]
        pub mod $name {
            use core::arch::asm;

            /// Read the IOCSR value
            #[inline(always)]
            pub unsafe fn io_csr_read() -> super::$type {
                let val: u32;

                unsafe {
                    asm!(
                        "iocsrrd.w {0}, {1}",
                        lateout(reg) val,
                        in(reg) $num,
                        options(nomem, nostack),
                    );
                }
                super::$type::from_u32(val)
            }

            /// Write the IOCSR value
            #[inline(always)]
            pub unsafe fn io_csr_write(value: super::$type) {
                unsafe{
                    asm!(
                        "iocsrwr.w {0}, {1}",
                        in(reg) value.to_u32(),
                        in(reg) $num,
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
