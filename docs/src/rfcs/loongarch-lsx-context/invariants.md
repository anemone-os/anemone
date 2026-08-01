# LoongArch LSX Context 目标与不变量

**状态：** Accepted Target
**最后更新：** 2026-08-01
**父 RFC：** [RFC-20260801-loongarch-lsx-context](./index.md)
**适用修订：** R0

本文定义 LoongArch LSX sticky-lazy target 的 correctness invariants、capability boundary 和
RFC-local proof obligations。当前没有提取独立 current contract；未来跨 RFC 复用时再建立最小 contract
闭包。

## 规则分类

- **Correctness Invariant：** register 单一 owner、EUEN 状态、trap ordering、clone/exec 一致性和 signal
  ABI。违反即可能泄漏或破坏用户态状态，不能作为性能折衷接受。
- **Target Guarantee / Capability：** 128-bit LSX userspace context、sticky-lazy policy 和
  Linux-compatible LSX signal payload。改变它们需要 target renegotiation。
- **Implementation Preference：** helper 名称、汇编展开方式和 Atomic ordering 的具体写法。只要保持
  可证明的生命周期和 publication 语义，可以在 R0 内调整。

## Contract Impact

| Contract ID | 变化 | 当前规则 | Target 摘要 | 生效 Gate |
| --- | --- | --- | --- | --- |
| None | None | 当前没有跨 RFC 的 LoongArch user-extension contract | R0 规则保持 RFC-local | N/A |

`TaskPropertiesArch` 是本实现使用的通用 lifecycle seam，但当前只有 LoongArch LSX policy 是非空 consumer；
不因单一 consumer 提前建立 shared current contract。

## Target Invariants

### LSX-OWNER-001 - Trapframe 是用户 extension register 的唯一 owner

**规则：** 当前 task 的 FPR、VR、FCC 和 FCSR 内容只保存在 `LA64TrapFrame::fpu_regs`。Task properties
只保存 policy bit；scheduler `TaskContext` 不保存 user extension data。

**Owner：** LoongArch user trapframe。

**违反表现：** scalar/LSX low lanes出现两份可能 stale 的 backing，clone、signal 或 switch 选择错误版本。

### LSX-LAYOUT-001 - Scalar 与 LSX 共用 interleaved 128-bit backing

**规则：** `FpuTaskContext` 为 16-byte aligned、528 bytes；32 个 register slot 各 16 bytes，lane 0 是
scalar `$fN` / LSX low lane，lane 1 是 LSX high lane；FCC offset 512，FCSR offset 520。

**Owner：** LoongArch FPU/LSX context layout。

**违反表现：** scalar assembly offset 错位、`vst/vld` 覆盖控制字段或 signal payload layout 不兼容。

### LSX-POLICY-001 - `lsx_used` 是 per-task、per-image sticky policy

**规则：** `lsx_used` 初始 false；第一次有效 SXD 成功转换为 true；clone child 继承；成功 exec 重置；
失败 exec 保留。true 状态只在成功 exec 后回到 false。

**Owner：** `LA64TaskProperties`。

**违反表现：** child 拥有完整 LSX snapshot 却走 scalar restore，或新 exec image 继承旧 image capability。

### LSX-FPU-001 - LSX 必然包含 scalar FPU ownership

**规则：** `lsx_used => fpu_used` 始终成立。LSX save/load 覆盖共享 low lanes、FCC 和 FCSR，不再叠加一次
scalar save/load。

**Owner：** LoongArch FPU/LSX trap policy。

**违反表现：** LSX restore 没有合法 scalar control state，或双重 save/load 互相覆盖。

### LSX-EUEN-001 - Kernel 与 user extension enable state 精确分离

**规则：**

```text
kernel:       FPE=0, SXE=0, ASXE=0
user none:    FPE=0, SXE=0, ASXE=0
user scalar:  FPE=1, SXE=0, ASXE=0
user LSX:     FPE=1, SXE=1, ASXE=0
```

`SXE` 不能在没有 `FPE` 时打开；`ASXE` 在 R0 永远不打开。

**Owner：** LoongArch extension status helper与 user entry/exit adapter。

**违反表现：** kernel code意外执行用户 extension、LSX 共享 register file未启用或 LASX 静默运行但无 context。

### LSX-TRAP-001 - Trap entry 先保存硬件状态再允许内核进展

**规则：** user trap 根据入口 EUEN snapshot 选择 LSX/scalar/no-save；完整保存完成后关闭
FPE/SXE/ASXE，之后才能打开中断、调度或进入可能覆盖硬件 register 的普通内核逻辑。

**Owner：** LoongArch `rust_utrap_entry` 前置段。

**违反表现：** timer preemption、syscall 或 migration 使尚未保存的用户 register 被覆盖。

### LSX-FIRST-001 - First-use 必须 capability-check 并初始化全 register file

**规则：** SXD handler先检查 `CPUCFG2.LSX`。不支持时投递 `SIGILL`，不得设置 policy。支持时，如果 task
没有 scalar FPU state则全 context 清零；已有 scalar state则保留 low lanes且 high lanes保持已初始化零值。
设置 policy后不推进 ERA，让原 LSX instruction 在恢复 context 后重试。

**Owner：** LoongArch SXD handler。

**违反表现：** 新 task 观察前一 task 的 high lanes，或 non-LSX CPU进入永久重复 exception。

### LSX-RETURN-001 - User return 按 task policy恢复唯一 snapshot

**规则：** `lsx_used` 为 true 时完整 load LSX并开启 `FPE|SXE`；否则仅在 `fpu_used` 为 true 时 load
scalar并只开启 FPE；两者均 false 时 extension bits保持关闭。

**Owner：** LoongArch user-return adapter。

**违反表现：** LSX high lanes在无调度 syscall后仍丢失，或 scalar task承担错误的 vector state。

### LSX-CLONE-001 - Clone flag 与 register snapshot 必须原子一致

**规则：** parent 从用户态进入 clone syscall前已经完成 extension save。child 复制 trapframe后、publish前
继承 properties；`child.lsx_used == true` 当且仅当 child trapframe包含有效完整 LSX snapshot。

**Owner：** clone task construction。

**违反表现：** child 使用未初始化 high lanes，或已有完整 snapshot却再次触发 first-use并清零。

### LSX-EXEC-001 - 只有成功 exec commit 丢弃旧 image extension state

**规则：** successful exec在新 user context接管时重置 `fpu_used`、`lsx_used` 和新 trapframe context；failed
exec不改变旧状态。

**Owner：** exec commit path。

**违反表现：** failed exec返回旧映像后丢失 register，或新映像继承旧 LSX data。

### LSX-SIGNAL-001 - Signal frame完整表达被中断 extension state

**规则：** LSX task使用 Linux `LSX_CTX_MAGIC` 和至少
`sizeof(SctxInfo)+sizeof(LsxContext)` 的 record；scalar task使用 `FPU_CTX_MAGIC` 并清零 union tail。
restore按 magic/size选择 active payload。handler首次使用 LSX 后恢复旧 scalar context时必须清零 high lanes。

**Owner：** LoongArch signal encode/restore adapter与 ABI layout。

**违反表现：** signal handler覆盖被中断 LSX high lanes、union tail泄漏内核数据或 `rt_sigreturn` 解释错误类型。

### LSX-LASX-001 - LASX fail closed

**规则：** R0不保存 256-bit LASX context；ASXD投递 `SIGILL`，所有 helper和return path都保持ASXE关闭。

**Owner：** LoongArch exception adapter与 EUEN helper。

**违反表现：** 用户程序可以执行LASX但trap/signal/clone只保存低128 bits，形成静默状态损坏。

### LSX-CPU-001 - R0依赖在线CPU的LSX capability同构

**规则：** `CPUCFG2.LSX`在task first-use所在CPU检查一次。R0支持的LoongArch系统必须保证所有可调度在线CPU
对LSX capability同构；已经设置`lsx_used`的task不得迁移到不支持LSX的CPU。

**Owner：** platform CPU capability边界；当前2K1000为同构系统。

**违反表现：** user return会在不支持LSX的CPU上执行kernel `vld`，在用户指令获得可控`SIGILL`之前先触发
kernel exception。

## 状态所有权

| 状态 | 唯一 owner | 生命周期 |
| --- | --- | --- |
| FPR/VR/FCC/FCSR 内容 | `LA64TrapFrame::fpu_regs` | task user context；clone复制，exec替换 |
| `lsx_used` policy | `LA64TaskProperties` | task image；clone继承，successful exec reset |
| `fpu_used` policy | `Task` | task image；clone继承，successful exec reset |
| live hardware extension enable | current CPU EUEN | user entry/exit临时设置，kernel mode关闭 |
| signal extension snapshot | userspace `UContext::uc_extcontext` | signal delivery到`rt_sigreturn` |
| LSX capability同构事实 | LoongArch platform | boot/runtime固定；R0不支持异构变化 |

## 线性化点

- first-use：`mark_lsx_used()` 的 successful compare-exchange。
- clone inheritance：child trapframe已复制且尚未publish时的 `inherit_for_clone()`。
- exec reset：新 image user context进入commit路径后的 `reset_for_exec()`。
- trap capture：完整 save helper返回，随后 extension bits统一关闭。
- signal publication：完整 extcontext写入 signal frame后，handler trapframe才成为可返回状态。

## 锁序与生命周期规则

- LSX save/load只在 local interrupts disabled 区间改变 EUEN。
- trap entry save发生在任何可能 schedule/preempt 的路径之前。
- `TaskArchProperties` 只能通过共享引用和 interior mutability更新，通用层不能替换对象。
- clone inheritance发生在 child publish前，不需要对外同步第二个状态。
- register data不进入 Arc、callback、event、per-CPU owner cache或 scheduler loop context。

## 禁止退化项

- 重新在 bootstrap 或 kernel mode 全局打开 SXE/ASXE。
- 只保存 `$fN` low lane却允许用户执行 LSX。
- 把 high lanes复制到 `TaskArchProperties`、`TaskContext` 或 per-CPU cache而不建立新 owner RFC。
- 先清 EUEN再用 task flag猜测入口硬件状态。
- clone只继承 `lsx_used` 而不复制完整 trapframe，或反过来只复制 context不继承 policy。
- signal frame仍写 FPU payload却允许 handler覆盖被中断 LSX high lanes。
- 把 2K1000 LSX验收扩大成 software unaligned access 已安全。

## 非目标

- LASX、LBT、HWCAP/IFUNC、kernel LSX codegen和per-CPU full-lazy owner。
- heterogeneous LSX CPU topology和LSX-aware task affinity。
- 对 post-first-use save/load性能作 Linux 等价保证。
- 建立跨架构 SIMD 抽象或通用 vector register API。

## 完成标准

- interleaved context的alignment、offset和size有编译期断言。
- scalar与LSX汇编均使用同一 backing，完整 LSX round-trip KUnit通过。
- task properties clone/exec lifecycle KUnit通过。
- SXD/ASXD、EUEN entry/return和CPUCFG capability路径完成source audit。
- 目标硬件满足在线CPU LSX capability同构前提。
- Linux-compatible FPU/LSX signal record layout有编译期尺寸约束。
- LoongArch release build通过，用户确认2K1000实机验收通过。
- LASX/HWCAP/full-lazy边界进入register，software unaligned问题保持独立。

上述条件已满足，R0为Closed；没有 current contract cutover。
