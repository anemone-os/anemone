pub trait TimeArchTrait {
    type LocalClockSource: LocalClockSourceArch;
    type LocalClockEvent: LocalClockEventArch;
}

pub trait LocalClockSourceArch {
    /// Get current monotonic time in its raw form. Typically, this is just the
    /// value read from a hardware timer register, without any scaling applied.
    fn curr_monotonic_time() -> u64;

    /// Get the frequency of the monotonic clock in hertz.
    ///
    /// Upper layers will use this to convert the raw timer ticks into actual
    /// time durations.
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
