pub trait TimeArchTrait {
    /// Get the frequency of the hardware timer in hertz.
    ///
    /// Before the hardware timer frequency is determined, this function should
    /// return None.
    fn hw_freq_hz() -> Option<u64>;

    /// Get the current timer ticks.
    fn current_ticks() -> u64;

    /// Set the next timer trigger to be after the given number of ticks, when
    /// the timer interrupt will be triggered.
    fn set_next_trigger(ticks: u64);
}
