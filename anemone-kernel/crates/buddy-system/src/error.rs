#[derive(Debug, PartialEq, Eq)]
pub enum BuddyError {
    InvalidOrder,
    UnalignedAddr,
    InvalidAddr,
    OutOfMemory,
}
