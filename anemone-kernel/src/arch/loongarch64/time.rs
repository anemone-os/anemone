use crate::time::TimeArchTrait;

pub struct LA64TimeArch;
impl TimeArchTrait for LA64TimeArch{
    fn hw_freq_hz() -> Option<u64> {
        todo!()
    }

    fn current_ticks() -> u64 {
        todo!()
    }

    fn set_next_trigger(ticks: u64) {
        todo!()
    }
}