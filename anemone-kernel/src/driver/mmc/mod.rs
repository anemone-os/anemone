//! MMC host-controller and concrete card drivers.
//!
//! DW-MSHC implements the protocol-neutral host contract. The SD Memory block
//! driver binds only after generic discovery has published typed card identity.

pub mod consts;
mod dw_mshc;
mod sd_memory;
