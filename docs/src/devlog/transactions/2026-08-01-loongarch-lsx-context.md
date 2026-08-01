# 2026-08-01 - LoongArch LSX Sticky-Lazy Context

**Status:** Completed
**Owners:** EDGW, Codex
**Area:** LoongArch64 / Task / user trap / FPU / LSX / signal ABI
**Canonical Plan:** [RFC-20260801-loongarch-lsx-context](../../rfcs/loongarch-lsx-context/index.md)
**RFC Revision:** R0
**Contract Impact:** None
**Current Phase:** Closed

## Scope

本事务关闭 LoongArch userspace LSX register context 缺口：旧实现允许用户执行 LSX，却只在 user trap
保存 64-bit scalar FPR。R0 增加 per-task sticky-lazy policy、唯一 128-bit trapframe backing、SXD first-use、
user entry/return save/restore、clone/exec lifecycle和Linux-compatible signal extcontext。

本事务不实现 LASX、LBT、`AT_HWCAP*`、IFUNC feature selection、per-CPU last-owner/full-lazy optimization或
kernel LSX codegen。

## Baseline

- bootstrap曾全局打开`SXE|ASXE`，用户LSX不会产生可由kernel接管的first-use transition。
- trapframe只保存32个64-bit FPR、FCC和FCSR，LSX high lanes没有task-owned backing。
- scheduler switch也服务scheduler loop伪上下文，不能直接拥有user extension state。
- signal frame只编码scalar FPU，signal handler会破坏被中断LSX context。
- GCC `cc1`真实使用大量LSX instruction，使该缺口以编译器内部状态损坏暴露。

## Phase Log

### 2026-07-31 - Task policy与统一register backing

**Change:** `SchedArchTrait`增加architecture-specific Task properties关联类型；RISC-V接入ZST，LoongArch
用`AtomicBool`保存`lsx_used`。Task只暴露shared accessor；clone在child publish前继承，successful exec
reset。`FpuTaskContext`改为32个interleaved 128-bit slots，scalar和LSX汇编共用同一low lane与FCC/FCSR。

**Invariant:** trapframe是用户FPR/VR内容的唯一owner；properties只保存policy，scheduler `TaskContext`不复制
register data。

### 2026-07-31 - SXD sticky-lazy与EUEN closure

**Change:** bootstrap关闭SXE/ASXE；SXD先检查`CPUCFG2.LSX`，再从zeroed或已有scalar context建立完整LSX
snapshot，首次转换打印一次info且不推进ERA。user trap按入口EUEN snapshot保存LSX/scalar后关闭全部extension；
user return按task policy恢复并只为LSX task打开`FPE|SXE`。ASXD始终投递`SIGILL`。

**Invariant:** `lsx_used => fpu_used`；kernel mode保持`FPE=SXE=ASXE=0`；ASXE没有enable path。

### 2026-08-01 - Signal ABI与lifecycle closure

**Change:** LoongArch signal UAPI增加Linux-compatible `SctxInfo`、`FPU_CTX_MAGIC`、`LSX_CTX_MAGIC`及对应
payload。signal encode为LSX task保存完整32 x 128-bit registers；restore按magic/size选择payload，并在从
scalar frame恢复时清零high lanes。clone/exec路径与完整trapframe snapshot同步。

**Reference:** `xref:linux-6.6.32:arch/loongarch/include/uapi/asm/sigcontext.h#LSX_CTX_MAGIC`，
`xref:linux-6.6.32:arch/loongarch/kernel/signal.c#protected_restore_lsx_context`。

### 2026-08-01 - Runtime acceptance与关闭

**Validation:**

- 528-byte context layout由alignment/offset/size compile-time assertions固定。
- `lsx_context_round_trip_preserves_full_register_file` KUnit覆盖32组不同high/low lanes。
- `task_properties_follow_clone_and_exec_lifecycle` KUnit覆盖first-use、clone inheritance与exec reset。
- 2K1000 LoongArch release build通过。
- 用户确认2K1000实机LSX验收通过。agent未保存该次原始实机日志，因此不补写未提供的计数、精确时序或
  case-by-case输出。

**Feedback:** LSX实现后GCC crash仍可能在不同pass暴露；后续调查确认software unaligned access是独立、
仍可能破坏用户内存的路径。该问题继续由open issue跟踪，不改变LSX context验收结论。

**Closure:** R0、全部checkpoint和本事务Completed/Closed；没有current contract cutover。

## Final Invariants

- `LA64TrapFrame::fpu_regs`唯一拥有FPR/VR/FCC/FCSR内容。
- task properties只拥有sticky `lsx_used` policy；clone继承，successful exec reset。
- user trap在任何开中断/调度前按live EUEN snapshot保存，kernel mode关闭extension bits。
- user return只按task policy恢复scalar或LSX，不同时执行两套save/load。
- signal frame用Linux-compatible magic、size、alignment和payload保存完整LSX state。
- LASX fail closed，普通kernel codegen保持`-lsx`。

完整规则见[目标与不变量](../../rfcs/loongarch-lsx-context/invariants.md)。

## Remaining Boundaries

- LASX 256-bit context未实现。
- `AT_HWCAP`/`AT_HWCAP2`仍不支持，kernel不向libc/IFUNC发布LSX capability。
- post-first-use每次user trap完整save/restore，不提供Linux full-lazy性能保证。
- R0依赖在线CPU对LSX capability同构，不支持LSX-aware affinity或异构核心迁移。
- software unaligned access可靠性是独立开放问题。

这些边界已登记在[当前限制](../../register/current-limitations.md#ane-20260801-la64-lsx-sticky-lazy-scope)
和[开放问题](../../register/open-issues.md#ane-20260801-la64-soft-unaligned-user-memory-corruption)中。

## Links

- [RFC](../../rfcs/loongarch-lsx-context/index.md)
- [Implementation map](../../rfcs/loongarch-lsx-context/implementation.md)
- [Biweekly devlog](../2026-07-20_to_2026-08-02.md)
- [Register limitation](../../register/current-limitations.md#ane-20260801-la64-lsx-sticky-lazy-scope)
