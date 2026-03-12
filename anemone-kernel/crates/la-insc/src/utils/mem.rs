
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum MemAccessType {
    StrongNonCache = 0,
    Cache = 1,
    WeakNonCache = 2,
    Reserved = 3,
}

impl MemAccessType{
    pub const fn from_value_or_default(value: u8) -> Self{
        match value{
            0 => Self::StrongNonCache,
            1 => Self::Cache,
            2 => Self::WeakNonCache,
            _ => Self::Reserved,
        }
    }
    pub const fn from_value(value: u8) -> Option<Self>{
        match value{
            0 => Some(Self::StrongNonCache),
            1 => Some(Self::Cache),
            2 => Some(Self::WeakNonCache),
            3 => Some(Self::Reserved),
            _ => None,
        }
    }
    pub const fn value(&self) -> u8{
        *self as u8
    }
}