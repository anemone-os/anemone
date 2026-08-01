# RFC-20260801-loongarch-lsx-context

**状态：** Implemented / Closed
**修订：** R0
**负责人：** EDGW, Codex
**最后更新：** 2026-08-01
**领域：** LoongArch64 / Task / user trap / FPU / LSX / signal ABI
**事务日志：** [2026-08-01-loongarch-lsx-context](../../devlog/transactions/2026-08-01-loongarch-lsx-context.md)
**影响契约：** None；本轮规则保持 LoongArch architecture-local，并未修改已有 current contract。
**开放问题：** None；已接受的 LASX、HWCAP 和 full-lazy 边界见
[当前限制](../../register/current-limitations.md#ane-20260801-la64-lsx-sticky-lazy-scope)。
**下一步：** None；只有出现 LASX、动态 CPU feature 公布或 LSX 热路径性能需求时才启动 follow-up。

## 摘要

旧 LoongArch 路径在启动时全局打开 `EUEN.SXE/ASXE`，但 user trap 只保存 32 个 64-bit 标量
FPR。用户程序执行 LSX 后，`$vr0..$vr31` 的高 64 位会在 syscall、中断、抢占或 task 切换后丢失
或串入其它执行上下文。GCC `cc1` 大量使用 LSX，因此该缺口会以编译器内部状态损坏和随机崩溃暴露。

R0 采用 sticky-lazy 策略：task 第一次执行 LSX 时由 SXD exception 建立完整 128-bit 上下文；从此到
成功 exec 为止，每次 user trap 都保存、每次 user return 都恢复全部 32 个 LSX 寄存器及共享
FCC/FCSR。该实现不引入 Linux 的 per-CPU last-owner/full-lazy 优化，优先保证状态唯一 owner 和可审计
生命周期。2K1000 实机验收已经由用户确认通过。

## 背景

改造前同时存在以下事实：

- bootstrap 无条件打开 `SXE | ASXE`，用户指令不会进入 first-use trap；
- `FpuTaskContext` 只有 32 个 64-bit FPR，汇编只执行 `fst.d/fld.d`；
- SXD/ASXD 虽已能被 trap decoder 识别，但没有对应 user handler；
- 标量 FPR 是 LSX `$vrN` 的低 64 位，两套独立 backing 会制造双重真相源；
- scheduler `switch()` 也用于没有对应 `Task` 的 scheduler loop context，不能直接承担 task-owned
  LSX policy 或 user register state；
- LoongArch signal frame 原先只能携带标量 FPU context，无法在 signal handler 和
  `rt_sigreturn()` 之间保存 LSX 高半部分。

Linux 6.6 的 UAPI 使用 16-byte `sctx_info`、`LSX_CTX_MAGIC=0x53580001` 和 528-byte
`lsx_context` 表示 signal extension context；其内核也分别提供 LSX save/restore 路径。本 RFC 只采用
这些可见 ABI 和寄存器布局事实，不复制 Linux 的完整 FPU owner 优化：

- `xref:linux-6.6.32:arch/loongarch/include/uapi/asm/sigcontext.h#LSX_CTX_MAGIC`
- `xref:linux-6.6.32:arch/loongarch/kernel/signal.c#protected_save_lsx_context`
- `xref:linux-6.6.32:arch/loongarch/kernel/fpu.S#_save_lsx_context`
- `xref:linux-6.6.32:arch/loongarch/include/asm/loongarch.h#CPUCFG2_LSX`

## 目标

- 为 `Task` 增加不可替换、只通过共享引用暴露的 architecture-specific properties 对象。
- 由 `SchedArchTrait::TaskProperties` 导出具体类型；RISC-V 使用空的零大小实现。
- LoongArch properties 唯一保存 `lsx_used` policy bit，第一次有效 SXD 后保持到成功 exec。
- 让 `LA64TrapFrame::fpu_regs` 成为 FPR/VR/FCC/FCSR 内容的唯一 owner。
- 以同一份 16-byte 对齐、528-byte interleaved context 表示标量 FPU 与 LSX。
- 未使用 FP/LSX 的 task 不复制 extension context；scalar-only task 仍只保存低 64-bit lanes。
- LSX task 每次 user trap 保存、每次 user return 恢复完整 32 x 128-bit register file。
- 内核态保持 `FPE=0, SXE=0, ASXE=0`；user LSX 只打开 `FPE | SXE`。
- first-use 前读取 `CPUCFG2.LSX`，不支持 LSX 的 CPU 稳定投递 `SIGILL`。
- clone/fork 继承完整 trapframe snapshot 和 `lsx_used`；成功 exec 清除 FP/LSX policy 与上下文。
- signal frame 使用 Linux-compatible FPU/LSX extcontext magic、size、alignment 和 payload layout。
- 第一次 `false -> true` 转换打印一次 `kinfoln!`，clone 继承不重复打印，exec 后新映像可重新打印。
- R0 支持的系统要求在线 LoongArch CPU 对 LSX capability 同构；2K1000 满足该边界。

## 非目标

- 不实现 LASX 256-bit register context；ASXD 继续投递 `SIGILL`，`ASXE` 始终关闭。
- 不实现 Linux per-CPU FPU owner、last-owner cache、delayed save 或完整 full-lazy 优化。
- 不允许普通内核 Rust/C codegen 使用 LSX；LoongArch kernel target 继续声明 `-lsx`。
- 不把真实 register data 放入 `TaskArchProperties` 或 scheduler `TaskContext`。
- 不实现 LBT/BTE context switching。
- 不实现 `AT_HWCAP`、`AT_HWCAP2`、IFUNC 选择或通用 CPU feature publication framework。
- 不支持在线 CPU 对 LSX capability 异构的拓扑，也不实现 LSX-aware task affinity。
- 不把 software unaligned access 的独立内存破坏问题归因于或并入 LSX context target。

## 文档地图

RFC target：

- [目标与不变量](./invariants.md)
- [迁移实施计划](./implementation.md)

执行与运行事实：

- [事务日志](../../devlog/transactions/2026-08-01-loongarch-lsx-context.md)
- [双周开发日志](../../devlog/2026-07-20_to_2026-08-02.md)
- [当前限制](../../register/current-limitations.md#ane-20260801-la64-lsx-sticky-lazy-scope)

Current contracts：None。本 RFC 的规则目前只由 LoongArch implementation 和本 RFC 共同消费；未来首次
跨 RFC 复用时再提取最小 current contract 闭包。

## 修订记录

| 修订 | 日期 | 状态 | 语义变化 | Review / 事务 |
| --- | --- | --- | --- | --- |
| R0 | 2026-08-01 | Closed | 建立 LoongArch LSX sticky-lazy task policy、完整 trap context、clone/exec 生命周期与 Linux-compatible signal extcontext。 | [事务](../../devlog/transactions/2026-08-01-loongarch-lsx-context.md) |

## 方案

### Architecture-specific Task properties

`SchedArchTrait` 导出 `TaskProperties`，其生命周期接口只有 `NEW`、
`inherit_for_clone()` 和 `reset_for_exec()`。`Task` 私有持有该对象，只提供共享引用，通用 task code
不能替换对象或读取 LoongArch private state。RISC-V properties 是 ZST；LoongArch properties 使用
`AtomicBool` 保存 `lsx_used`。

`lsx_used` 是行为状态，不是诊断字段：它决定 user return 选择 scalar 或 LSX restore。其
`compare_exchange(false, true)` 是 first-use 和一次性日志的线性化点。clone 在 child publish 前继承；
成功 exec commit 后 reset；exit 和 CPU migration 不需要额外动作。

### 共享 FPU/LSX backing

`FpuTaskContext` 使用 `[[u64; 2]; 32]`，lane 0 同时表示 `$fN` 和 `$vrN` low 64 bits，lane 1 表示
LSX high 64 bits。FCC、FCSR 和 reserved tail 使整体布局固定为 528 bytes、16-byte aligned。

scalar save/load 只访问每个 16-byte slot 的 lane 0；LSX `vst/vld` 访问完整 slot。两条路径共用
FCC/FCSR helper，因此不会出现标量和向量状态各自拥有一份 low lane 的情况。

### Sticky-lazy trap policy

bootstrap 不再打开 SXE/ASXE。第一次 LSX 指令触发 SXD：handler 先确认 `CPUCFG2.LSX`，再保留已有
scalar low lanes或创建全零 context，设置 `fpu_used` 和 `lsx_used`，但不推进 ERA；user return 完整恢复
LSX 后原指令重试。

已使用 LSX 的 task 从用户态进入内核时，trap entry 根据当时的 EUEN snapshot 完整保存 LSX，然后在
任何开中断、调度或普通内核逻辑前关闭 FPE/SXE/ASXE。返回用户态时根据 task policy 恢复 scalar 或
LSX context 并设置精确的 EUEN bits。裸 scheduler `switch()` 只切换 kernel callee-saved context。

### Lifecycle 与 signal ABI

clone 发生时父 task 已完成 user-trap LSX save，因此复制 trapframe 后同步继承 `lsx_used`。成功 exec
在新映像切换点重置 properties 和 `fpu_used`；失败 exec 不触碰旧映像状态。

signal encode 对 LSX task 写入 Linux-compatible `LSX_CTX_MAGIC` 和 `LsxContext`；scalar task 写入
`FPU_CTX_MAGIC` 并清零 union tail。`rt_sigreturn` 按 magic 和最小 size 选择 payload。若 signal handler
成为该 task 的第一个 LSX user，而被中断 context 只有 scalar FPU，恢复 scalar payload 前清零全部 high
lanes，防止旧向量数据泄漏。

## 接受边界

R0 acceptance 表示 LoongArch userspace 的 128-bit LSX register state 在 ordinary user trap、调度、
clone、exec 和 signal return 路径上受到保护；用户已确认 2K1000 实机验收通过。它不表示：

- LASX 已支持；
- 内核会通过 `AT_HWCAP*` 自动公布 LSX；
- post-first-use trap save/load 具有 Linux full-lazy 的性能；
- task 可以在 LSX capability 异构的 CPU 之间任意迁移；
- 普通 kernel code 可以执行 LSX；
- software unaligned access 已可靠。

改变 register owner、EUEN 规则、clone/exec 语义、signal extcontext ABI 或上述接受边界必须进入 RFC
review。只优化 helper 形状、汇编展开或内部 offset 表达，在保持不变量和验收矩阵时不构成 R1。

## 备选方案

### 继续全局打开 SXE

拒绝。没有完整保存恢复时会破坏用户 register state；即使增加无条件全系统 save/load，也会让从未使用
LSX 的 task 承担成本并失去 first-use capability check。

### 把 LSX 放入裸 scheduler switch

拒绝。scheduler loop context 没有对应 Task，把 user state 放进 `TaskContext` 会制造第二份 register
truth，并迫使底层 switch 接收不属于它的 task policy。

### 立即实现 Linux full-lazy owner tracking

延期。per-CPU owner、migration handoff 和 last-owner cache 会显著扩大状态机；当前 workload 只要求正确
保存恢复，sticky-lazy 已通过验收。性能证据出现前不增加该复杂度。

### 为 LSX 单独保存 high-lane 数组

拒绝。硬件 `vst/vld` 使用 interleaved 128-bit layout，分离数组会增加汇编 lane 拆装，并使 scalar low
lanes 更容易形成双重副本。

## 风险

- LSX task 在每次 user trap round trip 复制约 1 KiB save+restore 数据；这是 R0 接受的性能边界。
- signal extcontext 只覆盖 FPU/LSX，不表示 LASX/LBT extension chain 已实现。
- `AT_HWCAP*` 尚未实现，依赖 runtime feature publication 的 libc/IFUNC 不能从本能力自动获知 LSX。
- first-use capability check只发生在触发SXD的CPU；R0依赖在线CPU的LSX capability同构，不允许把task迁移
  到不支持LSX的核心。
- future trap/scheduler refactor 若绕过 user entry save/restore，可能重新暴露 register corruption；必须保持
  [目标与不变量](./invariants.md) 的 owner 和 ordering assertions。

## 收口

R0 implementation、布局/KUnit proof、2K1000 release build与用户实机验收已经完成，RFC 与事务状态均为
Closed/Completed。LASX、HWCAP 和 full-lazy 优化作为 accepted limitations 保留，不阻塞本 RFC。software
unaligned access 的内存破坏风险由独立 register issue 跟踪，不改变 LSX closure。
