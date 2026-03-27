//! CSR definitions and accessors


use crate::reg::{
    asid::Asid,
    crmd::Crmd,
    dmw::Dmw,
    exception::{Ecfg, Estat},
    pwc::{Pwch, Pwcl},
    timer::Tcfg,
};

macro_rules! define_csr {
    (64, $name: ident, $num:expr) => {
        paste::paste! {
            #[doc = concat!("CSR number for CSR `", stringify!($name), "`")]
            pub const [<CR_ $name:upper>]: u16 = $num;
        }

        #[doc = concat!("Accessor for CSR `", stringify!($name), "`")]
        pub mod $name {
            use core::arch::loongarch64::{csrrd, csrwr};
            /// Read the CSR value
            #[inline(always)]
            pub unsafe fn csr_read() -> u64 {
                unsafe{
                    csrrd::<$num>() as u64
                }
            }

            /// Write the CSR value
            #[inline(always)]
            pub unsafe fn csr_write(value: u64) {
                unsafe{
                    csrwr::<$num>(value as i64);
                }
            }
        }

    };
    (64, $name: ident, $num:expr, $type: ident) => {
        paste::paste! {
            #[doc = concat!("CSR number for CSR `", stringify!($name), "`")]
            pub const [<CR_ $name:upper>]: u16 = $num;
        }
        #[doc = concat!("Accessor for CSR `", stringify!($name), "`")]
        pub mod $name {
            use core::arch::loongarch64::{csrrd, csrwr};
            #[inline(always)]
            /// Read the CSR value
            pub unsafe fn csr_read() -> super::$type {
                let val = unsafe { csrrd::<$num>() } as u64;
                super::$type::from_u64(val)
            }

            /// Write the CSR value
            #[inline(always)]
            pub unsafe fn csr_write(value: super::$type) {
                unsafe{
                    csrwr::<$num>(value.to_u64() as i64);
                }
            }
        }

    };
    (32, $name: ident, $num:expr) => {
        paste::paste! {
            #[doc = concat!("CSR number for CSR `", stringify!($name), "`")]
            pub const [<CR_ $name:upper>]: u16 = $num;
        }
        #[doc = concat!("Accessor for CSR `", stringify!($name), "`")]
        pub mod $name {
            use core::arch::loongarch64::{csrrd, csrwr};
            #[inline(always)]
            /// Read the CSR value
            pub unsafe fn csr_read() -> u32 {
                let val = unsafe { csrrd::<$num>() } as u32;
                val
            }

            /// Write the CSR value
            #[inline(always)]
            pub unsafe fn csr_write(value: u32) {
                unsafe{
                    csrwr::<$num>(value as i64);
                }
            }
        }

    };
    (32, $name: ident, $num:expr, $type: ident) => {
        paste::paste! {
            #[doc = concat!("CSR number for CSR `", stringify!($name), "`")]
            pub const [<CR_ $name:upper>]: u16 = $num;
        }
        #[doc = concat!("Accessor for CSR `", stringify!($name), "`")]
        pub mod $name {
            use core::arch::loongarch64::{csrrd, csrwr};
            /// Read the CSR value
            #[inline(always)]
            pub unsafe fn csr_read() -> super::$type {
                let val = unsafe { csrrd::<$num>() } as u32;
                super::$type::from_u32(val)
            }

            /// Write the CSR value
            #[inline(always)]
            pub unsafe fn csr_write(value: super::$type) {
                unsafe{
                    csrwr::<$num>(value.to_u32() as i64);
                }
            }
        }

    };
}

define_csr!(64, asid, 0x18, Asid);
define_csr!(64, crmd, 0x0, Crmd);
define_csr!(64, prmd, 0x1, Crmd);
define_csr!(64, ecfg, 0x4, Ecfg);
define_csr!(64, estat, 0x5, Estat);
define_csr!(64, era, 0x6);
define_csr!(64, badv, 0x7);
define_csr!(64, eentry, 0xc);
define_csr!(64, cpuid, 0x20);
define_csr!(64, dmw0, 0x180, Dmw);
define_csr!(64, dmw1, 0x181, Dmw);
define_csr!(64, dmw2, 0x182, Dmw);
define_csr!(64, tlbrsave, 0x8b);
define_csr!(64, tlbrentry, 0x88);
define_csr!(64, tlbrbadv, 0x89);
define_csr!(32, pwcl, 0x1c, Pwcl);
define_csr!(32, pwch, 0x1d, Pwch);
define_csr!(64, pgdl, 0x19);
define_csr!(64, pgdh, 0x1a);
define_csr!(64, pgd, 0x1b);

define_csr!(32, tid, 0x40);
define_csr!(32, ticlr, 0x44);
define_csr!(64, tcfg, 0x41, Tcfg);


define_csr!(64, save0, 0x30);
define_csr!(64, save1, 0x31);