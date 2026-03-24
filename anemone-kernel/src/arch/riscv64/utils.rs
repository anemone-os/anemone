use riscv::register::sstatus::SPP;

use crate::task::Privilege;

impl From<Privilege> for SPP {
    fn from(value: Privilege) -> Self {
        match value {
            Privilege::User => SPP::User,
            Privilege::Kernel => SPP::Supervisor,
        }
    }
}

impl From<SPP> for Privilege {
    fn from(value: SPP) -> Self {
        match value {
            SPP::User => Privilege::User,
            SPP::Supervisor => Privilege::Kernel,
        }
    }
}
