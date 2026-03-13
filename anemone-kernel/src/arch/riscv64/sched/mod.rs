#![allow(unused)]

use crate::sched::hal::{ContextSwitchArch, CpuOpsArch, TaskContext};
use core::arch::asm;
use crate::exception::PreemptCounter;
use crate::arch::riscv64::exception::trap::RiscV64TrapFrame;
use core::mem::size_of;

// RISC-V 64   的 TaskContext 结构体
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct Riscv64TaskContext {
    // 鳄梨提出相关寄存器信息，有助于我的代码编写，后面就不在描述
    // x1 (ra)          - 返回地址
    // x8 (s0/fp)       - 帧指针/保存寄存器
    // x9 (s1)          - 保存寄存器
    // x18-x27 (s2-s11) - 保存寄存器
    pub ra: usize,  // x1
    pub sp: usize,  // x2
    pub s0: usize,  // x8
    pub s1: usize,  // x9
    pub s2: usize,  // x18
    pub s3: usize,  // x19
    pub s4: usize,  // x20
    pub s5: usize,  // x21
    pub s6: usize,  // x22
    pub s7: usize,  // x23
    pub s8: usize,  // x24
    pub s9: usize,  // x25
    pub s10: usize, // x26
    pub s11: usize, // x27

    // 特权级状态寄存器
    pub sstatus: usize,
    // 异常返回地址
    pub sepc: usize,
}

// into
impl From<&TaskContext> for &Riscv64TaskContext {
    fn from(ctx: &TaskContext) -> &Riscv64TaskContext {
        unsafe { &*(ctx as *const TaskContext as *const Riscv64TaskContext) }
    }
}

impl From<&mut TaskContext> for &mut Riscv64TaskContext {
    fn from(ctx: &mut TaskContext) -> &mut Riscv64TaskContext {
        unsafe { &mut *(ctx as *mut TaskContext as *mut Riscv64TaskContext) }
    }
}

impl TaskContext {
    /// for kernerl thread 可能目前没什么用？
    /// Safety
    /// 必须确保 `entry_point` 和 `stack_top` 是有效的。
    pub unsafe fn init(&mut self, entry_point: usize, stack_top: usize) {
        let ctx: &mut Riscv64TaskContext = self.into();

        ctx.ra = entry_point;
        ctx.sp = stack_top;

        ctx.sstatus = (1 << 8) | (1 << 5); // SPP = 1, SPIE = 1  //这里做好在检查一下，不确定是否正确

        // 任务入口点entry_point
        ctx.sepc = entry_point;
    }

    /// Safety
    /// 调用者必须确保 `kstack_top` 是有效的内核栈顶地址。
    pub unsafe fn goto_trap_return(
        kstack_top: usize, 
        user_entry: usize, 
        user_sp: usize
    ) -> Self {
        let tf_size = core::mem::size_of::<RiscV64TrapFrame>();
        let tf_ptr = (kstack_top - tf_size) as *mut RiscV64TrapFrame;

        let mut tf = RiscV64TrapFrame::default();
        
        tf.regs[10] = 0;        // a0: 往往作为主函数的第一个参数或返回值
        tf.regs[2] = user_sp;   // sp: 用户态栈指针
        
        tf.sepc = user_entry;   // sret 后从这里开始执行
        
        // sstatus 处理:
        // - SPP (Supervisor Previous Privilege) = 0 (返回后是 User 模式)
        // - SPIE (Supervisor Previous Interrupt Enable) = 1 (返回后开启中断)
        let mut sstatus = riscv::register::sstatus::read();
        sstatus.set_spp(riscv::register::sstatus::SPP::User);
        sstatus.set_spie(true);
        tf.sstatus = sstatus.bits();   
        unsafe { tf_ptr.write(tf) };

        let mut ctx = Self::new_zero();

        //__restore_from_ktrap暂时未能实现
        unsafe extern "C" {
            fn __restore_from_ktrap(); 
        }  

        ctx._prepared_space[0] = __restore_from_ktrap as usize; 
        ctx._prepared_space[1] = tf_ptr as usize; 

        ctx
    }
}

/// 实现RISC-V64的上下文切换
pub struct Riscv64ContextSwitch;

impl ContextSwitchArch for Riscv64ContextSwitch {
    #[unsafe(naked)]
    unsafe extern "C" fn switch(current_context: &mut TaskContext, next_context: &TaskContext) {
        core::arch::asm!(
            // a0 = &mut current_context, a1 = &next_context
            "sd ra, 0(a0)",
            "sd sp, 8(a0)",
            "sd s0, 16(a0)",
            "sd s1, 24(a0)",
            "sd s2, 32(a0)",
            "sd s3, 40(a0)",
            "sd s4, 48(a0)",
            "sd s5, 56(a0)",
            "sd s6, 64(a0)", 
            "sd s7, 72(a0)",
            "sd s8, 80(a0)",
            "sd s9, 88(a0)",
            "sd s10, 96(a0)",
            "sd s11, 104(a0)",

            // 保存 CSR 寄存器
            "csrr t0, sstatus",
            "sd t0, 112(a0)",
            "csrr t0, sepc",
            "sd t0, 120(a0)",

            // 切换到下一个上下文
            "ld ra, 0(a1)",
            "ld sp, 8(a1)",
            "ld s0, 16(a1)",
            "ld s1, 24(a1)",
            "ld s2, 32(a1)",
            "ld s3, 40(a1)",
            "ld s4, 48(a1)",
            "ld s5, 56(a1)",
            "ld s6, 64(a1)",
            "ld s7, 72(a1)",
            "ld s8, 80(a1)",
            "ld s9, 88(a1)",
            "ld s10, 96(a1)",
            "ld s11, 104(a1)",

            // 恢复 CSR 寄存器
            "ld t0, 112(a1)",
            "csrw sstatus, t0",
            "ld t0, 120(a1)",
            "csrw sepc, t0",

            "ret",
            options(noreturn)
        );
    }
}


//关中断开中断视为一种操作，所以添加了Ops，不知道这样符不符合命名归范
pub struct Riscv64Cpu;

impl CpuOpsArch for Riscv64Cpu {
    #[inline]
    unsafe fn disable_interrupts() -> bool {
        let mut sstatus: usize;
        core::arch::asm!("csrrci {0}, sstatus, {1}", out(reg) sstatus, in(reg) 1 << 1, options(nomem, nostack));
        (sstatus & (1 << 1)) != 0
    }

    #[inline]
    unsafe fn restore_interrupts(was_enabled: bool) {
        if was_enabled {
            core::arch::asm!("csrsi sstatus, {0}", in(reg) 1 << 1, options(nomem, nostack));
        }
    }

    #[inline]
    fn current_cpu_id() -> usize {
        let hartid: usize;
        unsafe {
            core::arch::asm!("csrr {0}, mhartid", out(reg) hartid, options(nomem, nostack));
        }
        hartid
    }

    #[inline]
    unsafe fn current_preempt_counter_mut() -> &'static mut PreemptCounter {
        let tp: usize;
        asm!("mv {}, tp", out(reg) tp);
        unsafe
        {
            //这里需要考虑CPU的本地内存的区域划分，这里简单设置为0位置
            let ptr = tp as *mut PreemptCounter;
            &mut *ptr
        }
     }
    #[inline]
    unsafe fn preempt_disable() {
        //关中断
        unsafe
        {
            let was_enabled = Self::disable_interrupts();
        
            let pc = Self::current_preempt_counter_mut();
            pc.increment_preempt_count();
        
            Self::restore_interrupts(was_enabled);
        }
    }

    #[inline]
    unsafe fn preempt_enable() {
        unsafe
        {
            let was_enabled = Self::disable_interrupts();
        
            let pc = Self::current_preempt_counter_mut();
            pc.decrement_preempt_count();
    
            Self::restore_interrupts(was_enabled);
        }
    }
}