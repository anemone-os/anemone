use crate::exception::PreemptCounter;
use crate::arch::CurCpuOpsArch;
/// 代表一个任务的 CPU 上下文。
/// 具体的字段内容由架构实现决定。
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct TaskContext {
    pub _prepared_space: [usize; 32], 
}

impl TaskContext {
    pub fn new_zero() -> Self {
        Self::default()
    }
    ///for kernerl thread
    /// Safety
    /// 必须确保 `entry_point` 和 `stack_top` 是有效的。
    pub unsafe fn init(&mut self, entry_point: usize, stack_top: usize) {
    }

    /// Safety
    /// 调用者必须确保 `kstack_top` 是有效的内核栈顶地址。
    pub unsafe fn goto_trap_return(kstack_top: usize) -> Self {
        unimplemented!("goto_trap_return not implemented for generic TaskContext")
    }
}


/// 定义架构特定的上下文切换操作。
pub trait ContextSwitchArch {
    /// 执行上下文切换。
    /// # Safety
    /// 这个函数是高度不安全的，因为它直接操作 CPU 状态。
    /// 调用者必须确保 `current_context` 和 `next_context` 是有效的。
    /// 修改此处：在 trait 定义中也加上 extern "C"
    unsafe extern "C" fn switch(current_context: &mut TaskContext, next_context: &TaskContext);
}

/// 定义架构特定的 CPU 操作。关于cpu的操作加上ops，意为operations
pub trait CpuOpsArch {
    /// 禁用所有中断并返回之前的中断状态。
    /// 调用者必须确保在适当的时候重新启用中断。
    unsafe fn disable_interrupts() -> bool;

    /// 根据 `was_enabled` 重新启用或保持禁用中断。
    /// `was_enabled` 必须是 `disable_interrupts` 返回的有效值，也就是说必须搭配使用
    unsafe fn restore_interrupts(was_enabled: bool);

    /// 获取当前 CPU 的唯一 ID。
    fn current_cpu_id() -> usize;

    unsafe fn current_preempt_counter_mut()-> &'static mut PreemptCounter;

    unsafe fn preempt_disable();

    unsafe fn preempt_enable();
}


pub fn get_cpu_count() -> usize {
    CurCpuOpsArch::ncpus()
}

pub fn get_current_cpu_id() -> usize {
    CurCpuOpsArch::cur_cpu_id()
}