pub trait CpuArchTrait {
    /// Returns the number of CPUs in the system.
    fn ncpus() -> usize;
    /// Returns the ID of the current CPU.
    fn cur_cpu_id() -> usize;

    /// Sets the base address of the per-CPU area for the current CPU.
    ///
    /// Typically, this function will write the given address to the thread
    /// local register (e.g., tp on RISC-V).
    unsafe fn set_percpu_base(base: *mut u8);

    /// Returns the base address of the per-CPU area for the current CPU.
    ///
    /// TODO: explain why this is needed when we already have
    /// [CpuArch::cur_cpu_id] and [PERCPU_BASES].
    fn percpu_base() -> usize;
}
