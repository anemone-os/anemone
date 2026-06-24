#import "../template/components.typ": *
#import "../template/figures.typ": *

= 体系结构、Trap 与平台边界

#epigraph(attribution: [C. A. R. Hoare])[
  There are two ways of constructing a software design: make it so simple that there are obviously no deficiencies, or make it so complicated that there are no obvious deficiencies.
]

#thesis[
  Anemone 的多架构支持不是把内核复制成 riscv64 和 loongarch64 两份，而是把用户态、内核态、硬件异常和手写汇编之间最危险的 unsafe boundary 收束到 arch / trap 层。generic task、scheduler、syscall 和 MM 看到的是 trapframe、syscall context、page fault、interrupt reason、task context 和 machine descriptor 这些窄接口；平台差异留在寄存器布局、异常编号、FPU enable 位、页表格式和早期机器初始化里。
]

体系结构代码最容易被误读成“启动 glue”。它确实包含很多 glue：切栈、保存寄存器、写 CSR、返回用户态、初始化中断控制器。但这些代码不是外围细节，而是内核可信边界的一部分。一次 trap 进入内核时，CPU 不会自动给 Rust 一个安全函数调用环境；手写汇编必须先把硬件状态整理成 Rust 可以理解的 `TrapFrame`，并且遵守目标架构的调用 ABI。反过来，Rust 代码也不能假装所有架构都长得一样：riscv64 的 `sepc`、`sstatus.FS`、PLIC / SBI timer 和 loongarch64 的 `era`、`euen.FPE`、PLATIC / IOCSR 是不同的硬件事实。

== Bootstrap path

启动路径是这条边界的第一次显形。Anemone 的两个架构都从 `__nun`#footnote[`Nun` 这个名字借自古埃及宇宙观中的原初水域：万物尚未成形之前的混沌之水。] 进入内核，但 `__nun` 做的事情不是“进入 Rust main”这么简单。它先建立最低限度的执行条件：riscv64 清理早期寄存器、选择 bootstrap stack、打开 Sv39 bootstrap page table；loongarch64 配置 DMW、页表控制寄存器、TLB refill entry 和 bootstrap stack。两边都会把 CPU 带到一个足以调用 Rust 的环境，然后跳到 `rusty_nun()`。

`rusty_nun()` 是 arch bootstrap 的分流点。BSP 进入 `bsp_setup()`，AP 进入 `ap_setup()`。BSP 侧先清理 `.bss`、安装 kernel trap handler、注册早期 console，随后从 FDT 或内嵌 DTB 读取 CPU 数、时钟频率和物理内存信息。这个阶段还没有普通内核对象世界：per-CPU 区域、frame allocator、kernel page table、guarded boot stack、IPI 唤醒和 AP 同步都在这里逐步建立。换句话说，early bootstrap 的目标不是启动用户程序，而是把硬件给出的初始状态改造成 scheduler 和 task 系统可以接管的状态。

这个转换的关键节点是创建 `kinit`。BSP 在完成 kernel mapping 和 boot stack remap 后，通过 `Task::new_kernel_with_tid_handle()` 创建带固定 `Tid::INIT` 身份的 `bsp_kinit`；AP 在完成本地 trap/percpu/page-table 初始化后创建对应的 `ap_kinit`。这些 task 被发布到 task topology，放入本 CPU 的 run queue，然后 CPU 切换到 scheduler。到这里，启动路径才从 arch-owned bootstrap 进入 generic kernel ownership：后续初始化不再靠裸汇编继续推进，而是作为 kernel task 在调度器下运行。

`bsp_kinit()` 承接的是 normal kernel initialization。它注册 syscall handler、filesystem driver 和 built-in driver，unflatten device tree，解析 bootargs，执行 machine init、platform discovery 和 virtual device probing；随后初始化本 CPU interrupt、建立 `kthreadd`，等待所有 CPU 完成本地初始化，再运行 late initcall。最后它挂载 rootfs，打开 initial stdio，设置 root / cwd，并读取 `/.anemone/init` 后通过 `kernel_execve()` 进入第一个用户态 init。这个分界很重要：`__nun` 到 `bsp_setup()` 解决的是“如何让 Rust kernel 可以安全开始”；`bsp_kinit()` 解决的是“如何把 generic kernel object graph 接到用户态启动协议”。

#invariant[
  bootstrap path 的 owner 会随阶段改变：`__nun` / `bsp_setup()` 属于 arch-owned early hardware setup，`kinit` handoff 之后由 task、scheduler、initcall、device discovery、VFS 和 execve 各自拥有状态。我们并不将早期启动便利接口自然沉淀成普通内核路径。
]

#book-figure(
  "../assets/figures/ch08/bootstrap-to-kinit.png",
  [Bootstrap 把硬件初态收束成 generic kernel task，再由 kinit 接入用户态启动协议。],
  width: 100%,
)

== HAL ownership inversion

Anemone 在 `arch/mod.rs` 通过 `cfg(target_arch)` 选择 riscv64 或 loongarch64 实现，然后把 `TrapArch`、`SchedArch`、`PagingArch`、`IntrArch`、`TimeArch`、`SignalArch` 等类型重新导出给 generic 层。generic task 不直接理解 `sepc` 或 `era`；syscall 层也不直接关心 syscall number 存在 `a7` 还是 `$a7`。这些差异被压进 trait 形状里。

#listing([`arch/mod.rs` 只选择并 re-export 当前架构实现，不拥有各功能模块的抽象定义])[
  ```rust
  arch_select!(riscv64, "riscv64");
  arch_select!(loongarch64, "loongarch64");

  pub type TrapFrame = <TrapArch as TrapArchTrait>::TrapFrame;
  pub type TaskContext = <SchedArch as SchedArchTrait>::TaskContext;
  pub type LocalClockEvent = <TimeArch as TimeArchTrait>::LocalClockEvent;
  ```
]

这里真正值得解释的不是“有很多 trait”，而是 trait 的归属。`TrapArchTrait` 定义在 `exception/trap/hal.rs`，因为 trap layer 才知道自己需要 trapframe、syscall context snapshot 和 trap-return handoff；`SchedArchTrait` 定义在 `sched/hal.rs`，因为 scheduler 只需要 task context switch，而不需要认识 trapframe 或 page table；`PagingArchTrait` 定义在 `mm/paging/hal.rs`，因为 MM 才拥有 PTE、TLB shootdown 和 direct mapping 的语义；`TimeArchTrait` 属于 time subsystem，`SignalArchTrait` 属于 signal subsystem。`arch` 目录负责实现这些硬件事实，但不反向替各功能模块发明一个“总 HAL”。

#listing([`TrapArchTrait` 让 syscall、signal 和 restart 逻辑只依赖可恢复的上下文形状])[
  ```rust
  pub trait TrapArchTrait {
      type TrapFrame: TrapFrameArch;
      type SyscallCtx: SyscallCtxArch;

      unsafe fn load_utrapframe(trapframe: Self::TrapFrame) -> !;
      fn syscall_ctx_snapshot(trapframe: &Self::TrapFrame) -> Self::SyscallCtx;
      fn restore_syscall_ctx(trapframe: &mut Self::TrapFrame, syscall_ctx: &Self::SyscallCtx);
  }
  ```
]

这是依赖反转原则在内核里的具体形态。常见的层次直觉会让人把 `arch` 当作底层平台包，然后由它向上提供一套通用能力清单；Anemone 反过来让功能模块先说清自己需要的最小硬件接口，再由每个架构去满足这个接口。这样做的好处是边界贴着真实使用点：syscall restart 只看到 syscall context snapshot，scheduler switch 只看到 callee-saved task context，MM fault path 只看到 page fault info 和 page-table operation，signal delivery 只看到 ucontext 编解码和 handler trapframe setup。

snapshot 是一个典型例子。`handle_syscall()` 可以直接修改当前 `TrapFrame` 的返回值和 PC；signal / restart path 需要的是一份 syscall context snapshot，用来在 signal delivery 后重建“这次 syscall 还没有真正完成”的现场。snapshot 不反向成为长期 task 状态，它只跨过一次 trap-return 决策窗口。

同样的边界也出现在 scheduler 和 MM。`SchedArchTrait::switch()` 只保存和恢复 callee-saved 寄存器，并且明确不切换 address space；地址空间切换仍归调度系统决定。`PagingArchTrait` 提供 page table、PTE flag、TLB shootdown 和 direct mapping 规则；generic fault path 从 arch trap 得到 `PageFaultInfo` 后，再交给 `handle_user_page_fault()`。这种分层避免了“架构上下文顺手接管 task / MM owner”的扩散。

#invariant[
  arch-specific context 只能表达硬件状态和进入 generic path 所需的最小上下文。task 生命周期、runnable state、地址空间 owner、signal policy 和 syscall ABI 解释权仍属于各自 generic 子系统。
]

#tradeoff[
  这种 HAL ownership inversion 更自然也更直接：接口定义留在使用者身边，review 时能马上看出“这个硬件能力是为了哪条 generic path 存在”。代价是内核很难获得完全干净的抽象分离。boot、trap、interrupt、timer、FPU、page table 和 per-CPU state 会在真实硬件路径上互相牵连；当我们想做硬件相关优化时，往往需要同时审视多个功能模块的 HAL 边界，而不是只在 `arch/` 下加一个 fast path。
]

== Trapframe ABI contract

用户态 trap 的第一段代码在汇编里。riscv64 的 `__utrap_entry` 用 `sscratch` 切到 kernel stack，保存通用寄存器和 CSR，然后把 trapframe 指针作为 `a0` 调用 `rust_utrap_entry()`；loongarch64 的入口用 `SAVE0` / `SAVE1` 完成类似工作，保存 `PRMD`、`ERA`、`BADV` 和 `ESTAT` 后进入 Rust。两边的共同点比指令差异更重要：在调用 Rust 前，汇编必须已经构造出一个满足 Rust/C ABI 假设的栈和内存布局。

#listing([trapframe layout guard 把汇编偏移、Rust 结构体布局和调用 ABI 绑在一起])[
  ```rust
  #[repr(C, align(16))]
  pub struct RiscV64TrapFrame {
      gpr: Gpr,
      sstatus: u64,
      sepc: u64,
      stval: u64,
      scause: u64,
      sscratch: u64,
      ktp: u64,
      fpu_regs: FpuTaskContext,
  }

  static_assert!(align_of::<RiscV64TrapFrame>() == 16);
  static_assert!(size_of::<RiscV64TrapFrame>() % 16 == 0);
  static_assert!(offset_of!(RiscV64TrapFrame, gpr) == 0);
  ```
]

这段 guard 的背景不是“格式洁癖”。一次 rv64 用户态浮点路径上的 `SIGILL` 暴露出一个更底层的问题：user-trap assembly 在 trapframe 入栈后调用 Rust 时，必须满足 RISC-V C ABI 对栈对齐的要求；插桩、优化级别和寄存器保存顺序都可能改变现象。trapframe 对齐、尺寸和 CSR 偏移护栏说明：`Task::fpu_used()` 这类高层状态只能说明 task 拥有 FPU context，不能证明 trap entry 已经满足编译器后端的 ABI 前提。

#boundary[
  unsafe arch boundary 的可审查性来自局部、机械的布局护栏，而不是来自高层 task 状态。trap entry、trap return、FPU save/restore 和 signal frame handoff 只要跨过 assembly/Rust ABI 边界，就必须把 layout、alignment 和 offset contract 固定下来。
]

#book-figure(
  "../assets/figures/ch08/trap-entry-handoff.png",
  [Trap entry 把硬件异常转换为 generic kernel path，但 trapframe layout 仍是 unsafe ABI 合同。],
  width: 100%,
)

== Trap path dispatch

在 `rust_utrap_entry()` 中，riscv64 和 loongarch64 都先保存必要的 FPU 状态、记录当前 task 从 user privilege 进入 kernel privilege，然后取得 syscall context snapshot。之后才按 trap 类型分流。

syscall 是最直接的路径。riscv64 把 `UserEnvCall` 分给 `handle_syscall()`，loongarch64 把 `Syscall` exception 分给同一个 generic handler。`handle_syscall()` 从 trapframe 读取 syscall number 和六个 raw argument，提前推进 syscall PC，调用 link-section registry 中注册的 handler，并把返回值或负 errno 写回 trapframe。syscall adapter 因此不需要知道具体架构的 `sepc` / `era` 字段名，只依赖 `SyscallCtxArch`。

exception 不走 syscall adapter。页故障被翻译成 `PageFaultInfo`，携带 faulting PC、fault address 和 read / write / execute 类型交给 MM；非法指令、breakpoint 或未处理 exception 会转成 signal。FPU lazy enable 也属于 exception 分流的一部分：rv64 当前用 illegal-instruction path 初始化 FPU，la64 用 floating-point-disabled exception 初始化 FPU。这个差异留在 arch trap 层；generic task 只观察 `fpu_used` 和 trapframe 中保存的 `FpuTaskContext`。

interrupt 又是另一条路径。riscv64 将 supervisor software / timer / external interrupt 分别映射到 IPI、timer 和 root IRQ；loongarch64 从 `ESTAT` / IOCSR 中区分 timer、IPI 和 hardware interrupt。两边都在进入硬中断环境时调用 per-CPU hwirq enter/leave，再转到 generic `handle_ipi()`、`handle_timer_interrupt()` 或 `handle_irq()`。这条路径的边界尤其窄：它只适合完成短小的中断分发和最小状态交接；复杂对象析构、普通分配或睡眠式工作应回到 process context。

#boundary[
  hard IRQ / IRQ-off tail 的设计边界是短小、不可睡眠、尽量 allocation-free。复杂对象析构、用户态可见结果分类或可能阻塞的工作属于 process context 或对应 owner 的 completion path；interrupt return tail 只保留硬件返回和最小调度交接所需的工作。
]

== FPU trap-return context

FPU context 不是普通 task 字段。它横跨三个层次：硬件 enable 位决定用户浮点指令是否会 trap，trapframe 保存当前 task 的 FPU register file，generic task 用 `fpu_used` 表示该 task 是否已经拥有 FPU context。任意一个层次错位，用户态看到的可能不是“慢一点的 lazy restore”，而是 `SIGILL`、状态泄漏或随机 UB。

riscv64 的 `FpuTaskContext` 保存 32 个 FPR 和 `fcsr`。进入用户 trap 时，如果 `sstatus.FS` 是 Dirty，当前 FPU 寄存器会被保存到 trapframe；返回用户态前，如果 task 已使用 FPU，则加载 trapframe 里的寄存器并把 `sstatus.FS` 写成 Clean，否则保持 Off。loongarch64 的 `FpuTaskContext` 还保存 `fcc0` 至 `fcc7` 和 `fcsr`，并用 layout assertion 固定 `f`、`fcc` 和 `fcsr` 的偏移，因为 save/load assembly 直接按这些偏移访问内存。

lazy FPU 的收益是让没有使用浮点的 task 不为 FPU save/restore 付费；代价是 trap-return path 必须同时维护 task 标志、trapframe、硬件 CSR 和 assembly ABI。这个路径展示了 Rust 内核里最需要显式 unsafe 边界的地方：类型可以帮忙固定布局和接口，但无法替代对 ABI、CSR 和汇编偏移的审计。

== Machine abstraction boundary

Anemone 的 machine abstraction 介于“到处硬编码 QEMU virt 地址”和“完整设备树化 / sysfs 化平台拓扑”之间。启动早期先 unflatten device tree，再用 root compatible string 匹配编译进内核的 `MachineDesc`。descriptor 只负责极早期必须先完成的事情：root interrupt controller 和 timer。

#listing([`MachineDesc` 只承载早期平台初始化，不接管设备模型或完整拓扑叙事])[
  ```rust
  pub trait MachineDesc: Sync {
      fn compatible(&self) -> &[&str];
      unsafe fn early_init_intc(&self);
      unsafe fn early_init_timer(&self);
  }
  ```
]

riscv64 的 QEMU virt descriptor 匹配 `riscv-virtio`，从 DTB 的 `/soc/plic` 找到 PLIC 节点并注册 root IRQ domain；timer 则走 SBI，所以 early timer init 基本是 no-op。loongarch64 的 QEMU 3A5000-compatible descriptor 匹配 `linux,dummy-loongson3`，从 `/platic` 找到平台中断控制器并注册 root IRQ domain；timer 初始化也保留在 arch time hook 附近。后续普通设备仍通过 firmware node、platform bus 和 driver model 发布，不由 machine descriptor 拥有设备语义。

这个折中有明确成本。machine descriptor 知道某些平台事实，例如根中断控制器路径、compatible string 和早期 timer 入口；它也暂时没有把 CPU-local interrupt-controller node、热插拔、完整设备 topology 或 sysfs 可观察性做成长期承诺。它的价值在于把“启动必须先知道”的平台 glue 收束到一个可以替换的位置，让 driver model 和 VFS/devfs 不必反向理解架构启动细节。

#historical-note[
  Linux ARM 早期 machine description 可以作为理解这种折中的历史参照：它能让一个内核在有限平台集合上先启动起来，但长期工业系统最终会把更多设备发现和拓扑语义迁出这种早期描述。对 Anemone 来说，更完整的 DTB 驱动发现、设备 topology、sysfs / procfs 可观察面和热插拔语义也应形成独立的设备模型 contract，而不是自然沉淀进 machine descriptor。
]

#book-figure(
  "../assets/figures/ch08/machine-boundary.png",
  [机器抽象层只承载早期不可推迟的平台事实，设备语义仍由 driver owner 维护。],
  width: 100%,
)

== TradeOff: Generic kernel 复用与 unsafe ABI 成本

这套边界让可审查性更直接。riscv64 和 loongarch64 可以共享 task、scheduler、syscall registry、signal policy 和大部分 MM 路径，因为 generic 层看到的是窄上下文而不是硬件寄存器全集。trap entry/return、FPU save/restore、interrupt decode、page table format 和 early machine init 留在 arch 层；当 unsafe boundary 出错时，工程记录也能把问题定位到“trapframe layout / ABI / IRQ-off tail”这类可审查区域，而不是把它混成普通 syscall 或 MM 语义失败。

代价是 arch 层必须承担真正的工程纪律。每个 `unsafe extern "C"` 入口、每个 naked assembly save/load、每个 trapframe offset 和每个 CSR restore 都可能成为全系统行为的前提。Anemone 的平台边界不是为了隐藏这些危险，而是为了让危险有名字、有 owner、有 source guard，并且在 review 时可以被单独拿出来审。
