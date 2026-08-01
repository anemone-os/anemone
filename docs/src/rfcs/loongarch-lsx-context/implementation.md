# LoongArch LSX Context 迁移实施计划

**状态：** Completed
**最后更新：** 2026-08-01
**父 RFC：** [RFC-20260801-loongarch-lsx-context](./index.md)
**目标与不变量：** [目标与不变量](./invariants.md)
**当前契约：** None
**当前修订：** R0

## 迁移说明

实现先于公共 RFC promotion 完成。本页把已经发生的 source change、验证和关闭边界映射为可审计
checkpoint，不重新授权或重演 implementation。执行事实由
[事务日志](../../devlog/transactions/2026-08-01-loongarch-lsx-context.md)拥有。

迁移遵循以下原则：

- 先建立 task policy owner，再改变硬件 extension enable policy。
- scalar FPU 与 LSX 必须在同一 checkpoint切换到唯一 interleaved backing。
- first-use、trap entry/return、clone/exec 和 signal ABI作为同一个 correctness closure验收。
- LASX、HWCAP和full-lazy optimization不以临时兼容路径混入R0。
- 实现反馈不得把 software unaligned access 的独立问题归因于LSX。

## 阶段路线图

| 阶段 | 状态 | 交付 |
| --- | --- | --- |
| Checkpoint A | Closed | architecture-specific Task properties与clone/exec lifecycle |
| Checkpoint B | Closed | 528-byte interleaved FPU/LSX context和scalar/vector assembly |
| Checkpoint C | Closed | SXD sticky-lazy、EUEN entry/return与LASX fail-closed |
| Checkpoint D | Closed | Linux-compatible signal extcontext与完整lifecycle closure |
| Checkpoint E | Closed | build、KUnit、source audit和2K1000用户验收 |

## Checkpoint A - Task properties scaffold

**Change：**

- `SchedArchTrait`增加`TaskProperties`关联类型，新增窄`TaskPropertiesArch` lifecycle trait。
- `Task`私有持有`TaskArchProperties`，只提供shared accessor。
- RISC-V接入空ZST implementation；LoongArch接入`AtomicBool lsx_used`。
- clone在child trapframe copy后、publish前继承properties。
- successful exec commit调用architecture reset；failed exec不进入reset。

**Proof：** `task_properties_follow_clone_and_exec_lifecycle` KUnit覆盖first transition、clone inheritance和exec
reset；source audit确认没有mutable/replacement accessor。

## Checkpoint B - Unified 128-bit context

**Change：**

- `FpuTaskContext`从32个`u64`改为32个`[u64; 2]` register slot。
- scalar `fst.d/fld.d` offset按16-byte stride更新，只触及low lane。
- 新增32个`vst/vld`组成的完整LSX save/load。
- scalar与LSX共用FCC/FCSR helper。
- layout assertion固定align 16、FCC 512、FCSR 520、size 528。
- scheduler `LA64TaskContext`不保存FPU/LSX data。

**Proof：** `lsx_context_round_trip_preserves_full_register_file` KUnit在支持LSX的CPU上用32组不同high/low
pattern完成load/save round trip；编译期layout assertions保护Rust/assembly ABI。

## Checkpoint C - Sticky-lazy user trap

**Change：**

- bootstrap从EUEN默认值删除SXE/ASXE，只保留现有BTE配置。
- 新增`CPUCFG2.LSX` runtime capability check。
- SXD在首次使用时初始化或扩展已有scalar context，设置`fpu_used`和`lsx_used`，打印一次info，保持ERA重试。
- user trap依据入口EUEN snapshot选择LSX/scalar save，然后统一关闭extension bits。
- user return依据task policy选择LSX/scalar restore并精确设置EUEN。
- ASXD打印unsupported并向当前task投递`SIGILL`；ASXE始终关闭。

**Proof：** correctness assertions覆盖`SXE => FPE`、live SXE必须对应`lsx_used`以及
`lsx_used => fpu_used`；source audit确认没有普通kernel LSX execution窗口。

## Checkpoint D - Clone、exec与signal ABI

**Change：**

- clone同时复制完整trapframe和architecture policy；成功exec同时reset policy和FPU use state。
- LoongArch UAPI增加16-byte `SctxInfo`、`FPU_CTX_MAGIC`、`LSX_CTX_MAGIC`、272-byte FPU payload和
  528-byte LSX payload。
- signal encode按当前task policy选择FPU/LSX record；scalar record先清零largest union member。
- `rt_sigreturn`按magic和minimum size恢复；从scalar frame恢复时清零LSX high lanes。

**Proof：** ABI compile-time assertions固定record size和`uc_extcontext` placement；实现与
`xref:linux-6.6.32:arch/loongarch/include/uapi/asm/sigcontext.h#LSX_CTX_MAGIC`及Linux signal parser的
magic/size规则核对。

## Checkpoint E - Acceptance与关闭

**Validation：**

- LoongArch 2K1000 release build通过。
- LSX register round-trip KUnit与task properties lifecycle KUnit进入内核测试集。
- source audit确认kernel target仍为`-lsx`、ASXE无enable路径、register data只有trapframe backing。
- 2K1000验收运行在LSX capability同构的online CPU topology；R0不包含异构迁移证明。
- 用户于2026-08-01确认2K1000实机LSX验收通过；agent未保存该次原始实机日志，因此事务只记录用户确认，
  不推断未提供的计数或时序细节。
- 后续GCC仍可由software unaligned access缺陷在不同位置崩溃；该问题已独立登记，不能作为LSX验收失败
  或LSX已修复它的证据。

**Closure：** R0 target、RFC和transaction均Closed/Completed；contract impact为None。LASX、HWCAP和
full-lazy optimization进入current limitations。

## Resolved Write Set

R0 implementation实际涉及：

```text
anemone-abi/src/process.rs
anemone-kernel/src/sched/hal.rs
anemone-kernel/src/arch/mod.rs
anemone-kernel/src/arch/riscv64/sched.rs
anemone-kernel/src/arch/loongarch64/sched.rs
anemone-kernel/src/arch/loongarch64/bootstrap.rs
anemone-kernel/src/arch/loongarch64/fpu.rs
anemone-kernel/src/arch/loongarch64/exception/trap/signal.rs
anemone-kernel/src/arch/loongarch64/exception/trap/utrap.rs
anemone-kernel/src/task/mod.rs
anemone-kernel/src/task/api/clone/mod.rs
anemone-kernel/src/task/api/execve/kernel.rs
```

公共文档收口涉及本RFC目录、事务日志、双周devlog、register和导航页；不修改private草案。

## 停止与退出条件

实施期间以下条件会阻止closure：

- trapframe之外出现第二份FPR/VR truth；
- kernel mode保留SXE/ASXE；
- clone flag与完整register snapshot不同步；
- signal只恢复64-bit low lanes；
- non-LSX CPU或LASX instruction进入无context的执行路径；
- target topology不能保证online CPU的LSX capability同构；
- 2K1000实机验收未通过。

这些停止条件均已neutralize。future performance optimization若需要per-CPU owner，必须新建follow-up RFC，
不能直接修改本页的Closed route。
