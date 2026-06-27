#import "../components/figure.typ": code-block

= 架构抽象层

Anemone 的硬件抽象层设计与往届内核很不同的是：我们遵循*依赖反转*的原则，让使用方子系统定义自己需要的架构能力，再由具体架构实现这些能力，而不是让 `arch` 模块主动向全内核暴露一组预设接口。这样，HAL 的所有权留在内存、异常、调度、信号等使用方模块中，架构层只负责实现和重导出。

在`arch` 模块，目标架构内部分别实现 `PagingArch`、`IntrArch`、`SchedArch`、`TrapArch`、`SignalArch` 等类型，`arch_select!` 根据编译目标选择 RISC-V 或 LoongArch 的实现；上层模块再通过这些别名访问自己定义的 trait 能力。

#code-block(
  ```rust
  #[cfg(target_arch = "riscv64")]
  pub use riscv64::{
      IntrArch, PagingArch, SchedArch, SignalArch, TimeArch, TrapArch, machine_init,
  };

  #[cfg(target_arch = "loongarch64")]
  pub use loongarch64::{
      IntrArch, PagingArch, SchedArch, SignalArch, TimeArch, TrapArch, machine_init,
  };

  pub type PgDir = <PagingArch as PagingArchTrait>::PgDir;
  pub type Pte = <<PagingArch as PagingArchTrait>::PgDir as PgDirArch>::Pte;
  pub type TrapFrame = <TrapArch as TrapArchTrait>::TrapFrame;
  pub type SyscallCtx = <TrapArch as TrapArchTrait>::SyscallCtx;
  pub type TaskContext = <SchedArch as SchedArchTrait>::TaskContext;
  ```.text,
  caption: [`arch` 模块按目标架构选择实现，并把关联类型收束成统一别名],
  lang: "rust",
)

通过依赖倒置的设计，Anemone的边界非常直接。内存管理只知道页表能力，调度器只知道如何保存和恢复任务上下文，异常路径只知道 trap frame 和 syscall register context，信号子系统只知道如何编码用户态 `ucontext`。不同 ISA 的寄存器布局、页表项格式、中断控制寄存器和返回用户态指令都被限制在架构实现内。

== 启动流程

Anemone 的启动流程分成两段。第一段是架构相关的早期入口，负责把 CPU 带到可以运行内核任务的状态；第二段是共享的 `bsp_kinit` / `ap_kinit`，负责进入普通内核初始化。这里我们只写到启动 `kinit` 为止，后续驱动发现、rootfs 挂载和第一个用户态进程属于其它章节的范围。

在 RISC-V 上，早期入口 `__nun` 清理临时寄存器，设置 bootstrap stack，写入 `satp` 启用临时页表，然后跳到 Rust 入口 `rusty_nun`。BSP 进入 `bsp_setup` 后清零 BSS、安装内核 trap 入口、注册 SBI early console 和基础电源处理器，从 FDT 早期扫描 CPU 数量、时钟频率和内存区域，初始化 per-CPU 区、物理页分配器、内核页表，并把 boot stack 重新映射到带 guard page 的区域。完成这些条件后，BSP 通过 SBI `hart_start` 唤醒 AP，创建 `bsp_kinit` 任务并切到调度器。

在 LoongArch 上，`__nun` 先配置 DMW、页表 walker 参数、TLB refill 入口和 bootstrap stack，再进入 `rusty_nun`。BSP 的 `bsp_setup` 同样完成 BSS、trap、早期控制台、FDT 早期扫描、per-CPU、物理内存和内核地址空间初始化，但 AP 唤醒通过 `csr_mail_send` 写入启动地址，再用 `IntrArch::send_ipi` 触发。LoongArch 还会在进入 `kinit` 前调用本架构的早期计时初始化，使本 CPU 的硬件计数源可用。

两个架构最终都创建内核任务来开始架构无关的第二阶段：BSP 创建 `bsp_kinit`，AP 创建 `ap_kinit`。从这一刻起，CPU 不再沿着裸启动栈继续推进初始化，而是作为普通内核任务进入共享路径。

== 抽象层

下面几个 HAL 是 Anemone 当前跨 RISC-V 和 LoongArch 的关键接口。它们不是集中定义在 `arch` 目录，而是放在各自的使用方子系统中；本节介绍的每个接口都对应一组由该子系统真正使用的能力。

=== PagingArch

`PagingArch` 属于内存管理子系统。 它定义架构必须提供的页表形状和硬件动作：页大小、页表层级、物理页号位宽，等等。页表遍历和映射算法可以共用一套通用实现（第四章提到的PageTable和Mapper），但页表目录和页表项的二进制格式由 `PgDirArch` / `PteArch` 定义。

#code-block(
  ```rust
  pub trait PagingArchTrait: Sized {
      type PgDir: PgDirArch;

      const MAX_HUGE_PAGE_LEVEL: usize;
      const PAGE_LEVELS: usize;
      const MAX_PPN_BITS: usize;
      const PAGE_SIZE_BYTES: usize;
      const PTE_PER_PGDIR: usize;

      fn setup_direct_mapping_region(pgtbl: &mut PageTable);
      unsafe fn activate_addr_space(root_ppn: PhysPageNum);
      fn tlb_shootdown(vpn: VirtPageNum);
      fn tlb_shootdown_all();
  }

  pub trait PteArch: Sized + From<u64> + Into<u64> + Copy {
      const ZEROED: Self;

      fn ppn(&self) -> PhysPageNum;
      fn is_valid(&self) -> bool;
      fn is_leaf(&self) -> bool;
      fn is_branch(&self) -> bool;
  }

  pub trait PgDirArch:
      Sized + Copy + Index<usize, Output = Self::Pte> + IndexMut<usize, Output = Self::Pte>
  {
      type Pte: PteArch;

      const ZEROED: Self;
      fn is_empty(&self) -> bool;
  }
  ```.text,
  caption: [`PagingArchTrait`、`PteArch` 和 `PgDirArch` 把通用页表算法与架构页表格式分开],
  lang: "rust",
)

通用页表代码可以根据 `PAGE_LEVELS`、`PTE_PER_PGDIR` 和 `PgDir` 索引规则完成查找、创建和释放；RISC-V 与 LoongArch 只需要在 `PteArch::new`、`flags`、`ppn` 等方法中解释自己的 PTE layout。

=== IntrArch

`IntrArch` 属于中断子系统。它的接口分成三类：读写本 CPU 的 IRQ flags，开关本地中断；发送和确认 IPI；初始化本 CPU 的本地 IRQ 入口。

#code-block(
  ```rust
  pub trait IntrArchTrait: Sized {
      const ENABLED_IRQ_FLAGS: IrqFlags;
      const DISABLED_IRQ_FLAGS: IrqFlags;

      fn current_irq_flags() -> IrqFlags;
      unsafe fn restore_local_intr(flags: IrqFlags);
      unsafe fn local_intr_enable();
      unsafe fn local_intr_disable();

      fn send_ipi(cpu_id: usize);
      unsafe fn claim_ipi();
      unsafe fn init_local_irq();

      fn local_intr_enabled() -> bool;
      fn local_intr_disabled() -> bool;
  }

  pub struct IrqFlags(u64);
  ```.text,
  caption: [`IntrArchTrait` 用不透明的 `IrqFlags` 表示本地中断状态],
  lang: "rust",
)

我们把 `IrqFlags` 设计成不透明值，是为了让上层只能保存和恢复中断状态，而不能解释某个 ISA 的状态寄存器位。调度、锁和中断保护路径需要的是“保存当前状态、关中断、最后恢复”的能力，而不是 `sstatus` 或 LoongArch CSR 的具体布局。IPI 同样被抽象成 `send_ipi` / `claim_ipi`，使调度唤醒、TLB shootdown 或启动 AP 的路径不必知道底层是 SBI call 还是 LoongArch mail + IPI。

=== SchedArch

`SchedArch` 属于调度子系统，但它只负责最低层的寄存器上下文切换。调度队列、抢占策略、任务状态转换和地址空间切换都不在这个 trait 中。地址空间切换由内存管理通过 `PagingArch::activate_addr_space` 完成，`SchedArch::switch` 只保存当前上下文的 callee-saved registers，并加载下一个上下文。

#code-block(
  ```rust
  pub trait SchedArchTrait {
      type TaskContext: TaskContextArch;

      unsafe fn switch(cur: *mut TaskContext, next: *const TaskContext);
  }

  pub trait TaskContextArch {
      const ZEROED: Self;

      fn from_kernel_fn(entry: VirtAddr, stack_top: VirtAddr, args: ParameterList) -> Self;
      fn from_user_fn(entry: VirtAddr, ustack_top: VirtAddr, kstack_top: VirtAddr) -> Self;
      fn pc(&self) -> u64;
      fn sp(&self) -> u64;
  }
  ```.text,
  caption: [`SchedArchTrait` 只把任务上下文的保存与恢复交给架构层],
  lang: "rust",
)

=== TrapArch

`TrapArch` 属于异常和系统调用入口。它定义了两个对象：`TrapFrame` 是一次进入内核时保存的完整用户态寄存器现场，`SyscallCtx` 是系统调用分发需要的寄存器视图。这里，我们要求架构实现`syscall_ctx_snapshot`方法，它返回一个快照，可以用来需要重启系统调用或恢复信号前现场。

#code-block(
  ```rust
  pub trait TrapArchTrait {
      type TrapFrame: TrapFrameArch;
      type SyscallCtx: SyscallCtxArch;

      unsafe fn load_utrapframe(trapframe: Self::TrapFrame) -> !;
      fn syscall_ctx_snapshot(trapframe: &Self::TrapFrame) -> Self::SyscallCtx;
      fn restore_syscall_ctx(trapframe: &mut Self::TrapFrame, syscall_ctx: &Self::SyscallCtx);
  }

  pub trait TrapFrameArch: SyscallCtxArch {
      const ZEROED: Self;

      fn sp(&self) -> u64;
      fn set_sp(&mut self, sp: u64);
      fn set_tls(&mut self, tls: u64);
      fn set_scratch(&mut self, scratch: u64);
      fn set_arg<const IDX: usize>(&mut self, arg: u64);
      fn set_return_addr(&mut self, addr: u64);
  }

  pub trait SyscallCtxArch: Clone + Copy {
      fn syscall_arg<const IDX: usize>(&self) -> u64;
      fn set_syscall_arg<const IDX: usize>(&mut self, arg: u64);
      fn syscall_no(&self) -> usize;
      fn set_syscall_no(&mut self, sysno: usize);
      fn syscall_pc(&self) -> u64;
      fn advance_syscall_pc(&mut self);
      fn syscall_retval(&self) -> u64;
      fn set_syscall_retval(&mut self, retval: u64);
  }
  ```.text,
  caption: [`TrapArchTrait` 把 trap frame、syscall context 和返回用户态动作交给架构层],
  lang: "rust",
)

这组接口让 syscall 层只处理 Linux ABI 的分发语义，而不需要知道 RISC-V 用哪些寄存器传参、LoongArch 的 syscall number 放在哪里、异常返回地址应该如何推进。`load_utrapframe` 是最终返回用户态的架构动作，`advance_syscall_pc` 和 `set_syscall_retval` 则把 syscall 成功、失败和重启路径压缩成通用代码能调用的寄存器操作。

=== SignalArch

`SignalArch` 属于信号子系统。信号投递的策略、pending set、mask 和线程组选择由通用信号代码维护，但真正暴露给用户态的 `ucontext` 和 signal handler 入参依赖架构寄存器布局。因此我们把“如何把 trap frame 编码成 POSIX `ucontext`”“如何从 `ucontext` 恢复 trap frame”“如何让用户态下一步进入 handler”放进 `SignalArchTrait`。

#code-block(
  ```rust
  pub trait SignalArchTrait {
      const MINSIGSTKSZ: usize;

      fn encode_ucontext(
          buf: &mut UContext,
          trapframe: &TrapFrame,
          mask: SigSet,
          altstack: linux_signal::SigStack,
          fpu: bool,
      );

      fn restore_ucontext(ucontext: &UContext, trapframe: &mut TrapFrame, fpu: bool);

      fn prepare_trapframe_for_signal_handler(
          trapframe: &mut TrapFrame,
          signo: SigNo,
          handler: VirtAddr,
          sigframe_base: VirtAddr,
      );
  }
  ```.text,
  caption: [`SignalArchTrait` 负责用户态信号 ABI 中与寄存器现场相关的部分],
  lang: "rust",
)

这个拆分避免了两个常见问题。第一，信号核心不需要直接读写每个架构的通用寄存器数组；第二，架构层也不需要知道完整的信号投递策略。`RtSigFrame` 入栈、`rt_sigreturn` 恢复和 handler 入口布置都是用户可见 ABI 的一部分，但它们依赖的是 trap frame 形状，而不是调度器或 VFS 的内部状态。把它放在 `SignalArch` 中，正好把“信号语义”和“寄存器现场编码”分开。
