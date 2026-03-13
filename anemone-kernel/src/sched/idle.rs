#![allow(unused)]
/// 空闲任务的入口点,目前还不去使用它，可能后面有用
/// 这是一个永不返回的函数，它会使 CPU 进入低功耗状态或简单地循环。cpu没有任务运行时执行，和负载均衡如何协作?
pub extern "C" fn idle_task_entry() -> ! {
    unsafe{
        loop {
        // 一直执行一些低功耗操作
        core::arch::asm!("wfi"); 
        }
    }
}
