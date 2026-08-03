pub trait TimeArchTrait {
    type LocalClockSource: LocalClockSourceArch;
    type LocalClockEvent: LocalClockEventArch;
}

pub trait LocalClockSourceArch {
    /// Get current monotonic time in its raw form.
    ///
    /// Every online CPU must observe one non-regressing counter domain. Any
    /// platform-specific offset calibration belongs in the architecture source,
    /// not in the common timekeeper.
    fn curr_monotonic_time() -> u64;

    /// Get the nonzero, boot-stable frequency of the shared counter in hertz.
    fn monotonic_freq_hz() -> u64;
}

/// Architecture-specific interface for programming timer interrupts.
pub trait LocalClockEventArch {
    /// Program the next timer interrupt to occur at the given monotonic time,
    /// which is specified in the same raw form as returned by
    /// [`LocalClockSourceArch::curr_monotonic_time()`].
    ///
    /// The `deadline` is an absolute time, not a relative duration.
    fn program_next_timer(deadline: u64);

    // no ack functions. it should be handled directly in architectural code after
    // the timer interrupt is received.
}
