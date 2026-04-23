use la_insc::utils::privl::PrivilegeLevel;

use crate::sched::Privilege;

impl From<Privilege> for PrivilegeLevel {
    fn from(value: Privilege) -> Self {
        match value {
            Privilege::User => PrivilegeLevel::PLV3,
            Privilege::Kernel => PrivilegeLevel::PLV0,
        }
    }
}

impl From<PrivilegeLevel> for Privilege {
    fn from(value: PrivilegeLevel) -> Self {
        match value {
            PrivilegeLevel::PLV3 => Privilege::User,
            _ => Privilege::Kernel,
        }
    }
}
