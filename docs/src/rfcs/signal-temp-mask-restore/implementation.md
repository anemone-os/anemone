# signal temporary mask restore 迁移实施计划

**状态：** Draft
**最后更新：** 2026-06-06
**父 RFC：** [RFC-20260606-signal-temp-mask-restore](./index.md)
**不变量：** [不变量需求](./invariants.md)

本文按可提交、可验证的阶段拆分。每个阶段完成后应能独立构建，并给出最小验证证据。

## 迁移原则

- 先建立 `TaskSigMaskState` 和 helper contract，再接入 syscall。
- 所有状态转换必须能对应 [不变量需求](./invariants.md) 的单一 restore slot 模型。
- `TemporarySigMaskToken` 不得有 drop-time restore 语义。
- `TemporarySigMaskToken` 只有 `restore_now()` 和 `defer_to_signal_delivery()` 两个终止态；不提供 `cancel_without_delivery()`。
- handler frame commit 前不得消费 restore slot。
- 无 handler frame cleanup 必须放在 signal 模块内部统一处理。
- `rt_sigsuspend` 不在 syscall body 中主动 `fetch_signal()`。
- `ppoll` / `pselect6` 迁移到 shared helper 后，不能保留第二套早恢复模型。
- `rt_sigsuspend`、`ppoll` 和 `pselect6` 不能仅凭 wait-core `Signal` / `Force` outcome defer restore；必须把 typed wait candidate 交给 signal-owned classifier，由 signal 子系统判定是否会进入 trap-return delivery。
- `rt_sigtimedwait` 做本地语义修复和边界审计，不迁入 delayed restore helper。
- 每个阶段保持 `just build` 可通过；功能阶段增加 focused smoke 或 LTP case。

## 阶段 0：UAPI 与边界预检

目标：确认实现入口和现有临时 mask callsites，避免后续遗漏。

前置条件：

- 本 RFC 已提升为公开目录级草案，并通过 implementation-readiness review。
- 没有并行实现修改同一 signal mask 生命周期模型。

交付：

- 确认 `SYS_RT_SIGSUSPEND = 133` 在 riscv64 / loongarch64 syscall ABI 层需要补齐。
- 确认目标 kernel API 文件：
  - `anemone-kernel/src/task/sig/api/rt_sigsuspend.rs`
  - `anemone-kernel/src/task/sig/api/mod.rs`
- 确认目标迁移 callsites：
  - `ppoll`
  - `pselect6`
  - `perform_signal_action`
  - `handle_signals`
- 确认 `rt_sigtimedwait` 只作为 syscall-body 本地语义修复和边界审计项。

审计：

- 搜索 `set_sig_mask(`、`sig_mask.lock()`、`ppoll`、`pselect6`、`rt_sigtimedwait`、`perform_signal_action`、`handle_signals`。
- 分类所有临时 mask 修改点：永久 mask 修改、delayed restore、syscall-body-only restore。

write set：

- 默认只读审计；如需记录实现交易，先创建 transaction devlog。

验证：

- 本阶段不要求运行构建；记录搜索结果即可。

退出条件：

- 所有临时 mask callsites 已分类。
- `rt_sigtimedwait` 的 async signal / force wake 后 waited-set dequeue 行为已分类；若发现它需要 delayed restore，停止并回到 RFC 修改 Scope。

## 阶段 1A：TaskSigMaskState storage 与 current-mask API

目标：建立 current mask 和 restore slot 的单一状态所有权。

前置条件：

- 阶段 0 callsite 分类完成。

交付：

- 将 task signal mask state 从单独 `SigSet` 升级为等价的 `TaskSigMaskState`：

  ```text
  TaskSigMaskState {
      current: SigSet,
      restore: Option<SigSet>,
  }
  ```

- 提供普通 current-mask API：
  - snapshot current mask
  - set permanent mask
  - current mask mutation helper
  - restore current mask from committed sigframe context for `rt_sigreturn`
- 迁移已知 direct access：
  - `rt_sigprocmask` 永久 mask 修改。
  - `rt_sigreturn` 从 sigframe 恢复 current mask。
  - `rt_sigtimedwait` 的 syscall-body-only temporary unmask / restore。
  - clone / fork 继承 current mask。
  - procfs status snapshot 等只读 current mask 观察点。
- 明确登记暂留到阶段 4 的 classified legacy temporary-mask path：
  - `anemone-kernel/src/fs/api/iomux/ppoll.rs` 的旧 save / set / restore mask path。
  - `anemone-kernel/src/fs/api/iomux/pselect6.rs` 的旧 save / set / restore mask path。
  - 这些路径只能作为阶段 4 必迁移债务保留；阶段 1A 不得把它们改写成新的 delayed-restore 语义，也不得把它们算作已完成迁移。

审计：

- 所有旧 `sig_mask` access 必须被分类为 current mask snapshot/mutation 或 temporary restore 操作。
- `rg -n "sig_mask|set_sig_mask|TaskSigMaskState" anemone-kernel` 结果中，除 `TaskSigMaskState` owner 内部、初始化、已分类只读 snapshot、`rt_sigtimedwait` syscall-body-only temporary path，以及已登记的 Stage-4 `ppoll` / `pselect6` legacy temporary path 外，不得留下未分类旁路。
- 普通 current-mask API 必须命名化，调用点能区分 snapshot、永久 mutation、sigframe restore 和 syscall-body-only temporary mutation。
- `rt_sigreturn` 只通过 current-mask API 恢复 sigframe mask，不读取、消费或覆盖 pending restore slot。
- clone / fork / procfs snapshot 只继承或观察 `current`，不得复制或暴露 `restore`。

write set：

- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/task/sig/mod.rs`
- `anemone-kernel/src/task/sig/api/rt_sigprocmask.rs`
- `anemone-kernel/src/task/sig/api/rt_sigreturn.rs`
- `anemone-kernel/src/task/sig/api/rt_sigtimedwait.rs`
- `anemone-kernel/src/task/api/clone/mod.rs`
- 必要的 fork / procfs snapshot 触点。

可观测性：

- direct-access 迁移过程中新增的 impossible state 使用普通 `assert!` 或清晰错误路径。
- rare fail-closed path 带 task id / syscall context 日志。

验证：

- `just build`

退出条件：

- current mask 和 restore slot 只有一个锁和一个真相源。
- 永久 mask API 不再绕开 `TaskSigMaskState`。
- `rg` 审计显示 direct `sig_mask` access 除 owner 内部、初始化、已分类只读 snapshot、明确 syscall-body-only temporary 路径和已登记的 Stage-4 `ppoll` / `pselect6` legacy temporary path 外无未分类旁路。
- 阶段 1A 交付记录必须点名 `ppoll` / `pselect6` legacy temporary path 仍存在，并把它们列为阶段 4 必须删除或替换的债务。

## 阶段 1B：TemporarySigMaskToken 与 helper contract

目标：固定 temporary mask 的线性 token 生命周期，以及普通 mask mutation 与 pending restore 的关系。

前置条件：

- 阶段 1A storage 和普通 current-mask API 已完成。

交付：

- 提供 temporary mask API：
  - `begin_temporary_sig_mask(new_mask) -> TemporarySigMaskToken`
  - `TemporarySigMaskToken::restore_now(self)`
  - `TemporarySigMaskToken::defer_to_signal_delivery(self)`
  - `sigmask_to_save_for_signal_frame() -> SigSet`
  - `signal_frame_committed_restore_mask()`
  - `restore_temporary_sig_mask_if_pending()`
- 明确 `restore != None` 时普通永久 mask mutation 规则：
  - 默认非法或 unreachable，不能静默覆盖 pending restore。
  - 只允许 signal delivery 安装 handler mask 的路径修改 `current`。
  - `rt_sigprocmask` 等普通永久 mutation 必须先 fail-closed 或触发内部 invariant。
- `TemporarySigMaskToken` 线性化：
  - `#[must_use]`
  - 非 `Clone` / 非 `Copy`
  - terminal method 消耗 `self`
  - token 记录 task / slot identity，并在终止时校验匹配。
  - `Drop` 只能 assert/log active-token leak，不得恢复 mask。

审计：

- begin 时已有 `restore` 的路径必须在安装新 mask 前 fail-closed 或触发内部 invariant，不能覆盖，也不能产生 token。
- token 所有 return path 必须明确落入 `restore_now(self)` 或 `defer_to_signal_delivery(self)`；不得新增第三终止态。
- helper 不能在持锁状态下执行用户 copy 或 schedule。
- 普通永久 mask mutation 不能在 `restore != None` 时绕开 token contract。

write set：

- `anemone-kernel/src/task/sig/mod.rs`
- 必要时只调整调用方 API 名称，不改变 Stage 2 到 5 的语义迁移范围。

可观测性：

- nested temporary mask begin 的 fail-closed path 有清晰 assertion 或错误日志。
- token identity mismatch 和 leaked active token 使用 debug/assert 日志暴露 task id / slot context。

验证：

- `just build`

退出条件：

- `TemporarySigMaskToken` 是 must-use、线性、不可复制的 token。
- 所有 terminal method 都消费 `self`，`Drop` 没有恢复语义。
- `restore != None` 时普通永久 mask mutation 的合法路径和非法路径都能被代码审查定位。

## 阶段 2：signal delivery 接入

目标：让 delayed restore 在 trap-return signal delivery 边界闭合。

前置条件：

- 阶段 1A storage 和阶段 1B helper contract 已可用并通过构建。

交付：

- `perform_signal_action()` 用户 handler 路径改为：
  - 使用 `sigmask_to_save_for_signal_frame()` 选择 sigframe 保存的 mask。
  - 安装 handler `sa_mask` 和 self-mask。
  - 写入 sigframe。
  - 准备 trapframe。
  - 在 frame commit 点调用 `signal_frame_committed_restore_mask()`。
- `handle_signals()` 在没有留下用户 handler frame 的返回用户态路径调用 `restore_temporary_sig_mask_if_pending()`。
- `rt_sigreturn` 恢复 sigframe 中保存的 mask 时必须走新的 current-mask API；它是 handler frame commit 后的恢复责任落点，只恢复 `current`，不得读取、消费或覆盖 pending restore slot。
- default terminate 路径保持不返回。
- default ignore / explicit ignore 消费 signal 后必须通过统一 cleanup 恢复旧 mask。

审计：

- 确认 restore slot 没有在读取 `mask_to_save` 后提前清除。
- 确认 sigframe 写失败或 trapframe 准备失败不会返回用户态并丢失 restore slot。
- 确认 `rt_sigreturn` 的 sigframe mask 恢复路径不绕开 `TaskSigMaskState`，且不会把 pending restore slot 当作自己的恢复来源或清理目标。
- 确认 cleanup 位于 signal 模块内部，不在 riscv64 / loongarch64 trap-return 层复制。

write set：

- `anemone-kernel/src/task/sig/mod.rs`
- `anemone-kernel/src/task/sig/api/rt_sigreturn.rs`
- 必要时调整 arch `SignalArchTrait` 调用点，但不把 cleanup 语义下沉到 arch 层。

可观测性：

- frame write failure 日志应保留 task id、signal number 和用户栈地址。
- cleanup helper 可以在 debug 日志中标记 restored pending temporary mask。

验证：

- `just build`
- focused smoke：已 pending 的 handler signal 能进入 handler，handler `ucontext.uc_sigmask` 是旧 mask。

退出条件：

- handler frame commit 点和无 frame cleanup 点都能被代码审查明确定位。
- `rt_sigreturn` 的 sigframe mask restore 落点能被代码审查明确定位，且只作用于 `TaskSigMaskState.current`。

## 阶段 3：rt_sigsuspend syscall

目标：补齐 `rt_sigsuspend`，并通过 wait-core 等待 pending/unmasked signal。

前置条件：

- 阶段 2 delayed restore delivery 边界已经闭合。

交付：

- ABI 层注册 `SYS_RT_SIGSUSPEND = 133`。
- 新增 `anemone-kernel/src/task/sig/api/rt_sigsuspend.rs`。
- 在 signal API module 导出并接入 syscall table。
- syscall 语义：

  ```text
  rt_sigsuspend(umask, sigsetsize):
      validate sigsetsize
      copy in umask
      clear SIGKILL/SIGSTOP
      token = begin_temporary_sig_mask(mask)
      wait_current_with_timeout(interruptible=true, timeout=None, precheck has_unmasked_signal)
      candidate = typed wait outcome
      decision = classify_temporary_mask_wait(candidate, syscall context)
      DeferToTrapReturnDelivery: token.defer_to_signal_delivery(); return EINTR carrier
      RestoreThenReturn/RestoreThenFailClosed: token.restore_now(); map errno/result
      NoReturnForce: token.restore_now(); enter no-return force path
  ```

- `Signal` / `Force` outcome 只是 delivery candidate；`rt_sigsuspend` 必须调用 signal-owned classifier，不能在 syscall body 中复制 pending queue、disposition、ignore/default/custom 或 force wake policy。
- classifier 返回 `DeferToTrapReturnDelivery` 前，必须已经为当前 task 建立 stable delivery target reservation / handoff。若 signal 子系统不能 claim private / shared pending signal target，不能 defer，必须返回 restore / fail-closed / no-return force 类 decision。

审计：

- `rt_sigsuspend` 不主动 `fetch_signal()`。
- precheck 在安装临时 mask 后、schedule 前执行。
- `has_unmasked_signal()` 只能作为 wait precheck，不作为 defer proof。
- signal interrupted path 只有在 signal-owned classifier 已完成 stable delivery target reservation / handoff 并返回 `DeferToTrapReturnDelivery` 后才 defer，不在 syscall body 中恢复旧 mask。
- 无法确认 trap-return delivery 的 `Signal` / `Force` candidate 先 `restore_now()`，再 fail-closed、映射返回或走不可返回 force 路径。
- 若 shared pending signal 在 classifier 内无法从 thread-group pending 队列 claim / move / reserve 给当前 task，必须先 `restore_now()`，不得用观察结果调用 `defer_to_signal_delivery()`。
- bad user pointer / bad sigsetsize 在安装临时 mask 前返回。

write set：

- `anemone-abi/src/syscall/{riscv,loongarch}.rs`
- `anemone-kernel/src/task/sig/api/rt_sigsuspend.rs`
- `anemone-kernel/src/task/sig/api/mod.rs`
- signal-owned classifier 所在 signal 文件，优先 `anemone-kernel/src/task/sig/mod.rs` 或 signal-internal module。
- syscall dispatch 表。
- 可选：`anemone-rs/src/sys/linux.rs` 和 `anemone-rs/src/os/linux.rs` wrapper，仅用于 smoke ergonomics。

可观测性：

- unexpected outcome 日志包含 task id、outcome、是否仍有 unmasked signal。

验证：

- `just build`
- smoke：旧 mask 屏蔽 `SIGUSR1`，`rt_sigsuspend` 临时解除屏蔽，handler 必须运行，handler 返回后旧 mask 恢复。
- smoke：signal 在调用前已经 pending，syscall 不永久睡眠。

退出条件：

- `rt_sigsuspend` 正常不会 success return。
- handler delivery 和 old-mask restore 语义通过 focused smoke。

## 阶段 4：ppoll / pselect6 迁移

目标：消除第二套临时 mask 生命周期，并让 iomux wait helper 保留可分类的 typed outcome。

前置条件：

- 阶段 3 `rt_sigsuspend` 已证明 helper 可用。

交付：

- `ppoll` 使用 `begin_temporary_sig_mask` / explicit restore / defer helper。
- `pselect6` 使用同一 helper。
- 删除或替换阶段 1A 明确登记的 `ppoll` / `pselect6` legacy temporary-mask save / set / restore path。
- `wait_for_iomux_ready()` 或替代共享 helper 返回 typed outcome，至少区分：
  - ready
  - timeout
  - syscall/register error
  - signal candidate
  - force
- 非 signal 返回路径恢复旧 mask 后返回 ready count、timeout 或 syscall error。
- signal interrupted 路径只有在 signal-owned classifier 已完成 stable delivery target reservation / handoff 并返回 `DeferToTrapReturnDelivery` 后才 defer，不在 syscall body 中恢复旧 mask，也不在 fs/iomux 中复制 signal delivery policy。

审计：

- 删除或替换旧的：

  ```text
  prev = task.sig_mask()
  task.set_sig_mask(mask)
  wait
  task.set_sig_mask(prev)
  ```

- 确认 copy-out error、ready path、timeout path、interrupted path 都有明确 token 终止。
- 确认 `Signal` / `Force` outcome 不直接等同于 defer；它们只能作为 signal-owned classifier 的 typed candidate。
- 确认 `DeferToTrapReturnDelivery` 不是观察型 proof：private pending 或 shared pending signal 已由 signal 子系统 claim / move / reserve 给当前 task。
- 确认 errno/result mapping 发生在 token `restore_now(self)` 或 `defer_to_signal_delivery(self)` 之后。
- 确认 copy-out error、ready、timeout、syscall/register error、signal candidate、force 每条路径都有且只有一个 token terminal。
- 确认 `ppoll` / `pselect6` 的 no-sigmask path 不创建 token。
- 确认阶段 1A 登记的 `ppoll` / `pselect6` legacy temporary path 已删除或替换，不再作为允许残留旁路存在。
- 确认 `wait_for_iomux_ready()` 或替代 helper 不再把 `Signal` / `Force` 过早压成 `SysError::Interrupted`。

write set：

- `anemone-kernel/src/fs/api/iomux/wait.rs`
- `anemone-kernel/src/fs/api/iomux/ppoll.rs`
- `anemone-kernel/src/fs/api/iomux/pselect6.rs`
- signal-owned classifier 所在 signal 文件，优先 `anemone-kernel/src/task/sig/mod.rs` 或 signal-internal module。

可观测性：

- interrupted path 日志应能区分 signal candidate、force、timeout、ready 和 syscall/register error。

验证：

- `just build`
- smoke：`ppoll` 带临时 mask 被 handler 打断，handler 运行，返回后旧 mask 恢复。
- smoke：`pselect6` 带临时 mask 被 handler 打断，handler 运行，返回后旧 mask 恢复。
- ready path 和 timeout path 不改变旧 mask。

退出条件：

- `ppoll` / `pselect6` 不再有独立早恢复模型。
- iomux helper 不再在 token-active 语义路径上提前把 `Signal` / `Force` ABI-map 为 `EINTR`。
- `ppoll` / `pselect6` 的用户可见 errno/result mapping 都位于 token terminal 之后。

## 阶段 5：rt_sigtimedwait 本地语义修复与边界审计

目标：确认 `rt_sigtimedwait` 没有被误迁入 delayed restore helper，并修复 wait 被 signal / force 唤醒后的 waited-set dequeue 分类。

前置条件：

- 阶段 4 完成。

交付：

- 审计 `rt_sigtimedwait` 的临时 unmask / restore 路径。
- 确认 `rt_sigtimedwait` 不迁入 delayed restore helper。
- wait 被 signal / force 唤醒后，先按 waited set 重新尝试 dequeue matching signal。
- 若拿到 waited signal，恢复 mask 并返回 signal number。
- 若没有 matching waited signal，恢复 mask 后返回 `EINTR` 或处理 force。
- 确认未等待 signal interrupted path 不把恢复责任交给 `rt_sigreturn()`。
- 若发现它需要 trap-return delivery / delayed restore helper，停止实现并回到 RFC 更新 Scope。

审计：

- 搜索 `rt_sigtimedwait`、`fetch_specific_signal`、temporary mask restore。
- 对每个 return path 标注 mask 是否已恢复。
- 对 `CurrentWaitOutcome::Signal | CurrentWaitOutcome::Force` 分支标注是否先尝试 waited-set dequeue。

write set：

- `anemone-kernel/src/task/sig/api/rt_sigtimedwait.rs`
- 若需要超出该文件或改变 trap-return delivery 协议，上报 RFC scope 变更。

验证：

- `just build`
- focused test：waited signal 在 precheck 前 pending 时返回 signal number。
- focused test：waited signal 在 wait 已发布后到达时返回 signal number。
- focused test：非 waited unmasked signal interrupt 返回 `EINTR` 且 mask 恢复。
- LTP：`rt_sigtimedwait01`、`sigtimedwait01`。

退出条件：

- `rt_sigtimedwait` 的本地语义修复和 helper 外边界记录在 transaction devlog 或实现阶段总结中。

## 阶段 6：验证与收口

目标：用 targeted tests 和 LTP case 证明用户可见语义。

前置条件：

- 阶段 1A 到 5 已按公开 RFC 和 implementation transaction 完成。

交付：

- 自建 smoke：
  - 旧 mask 屏蔽 `SIGUSR1`，`rt_sigsuspend()` 临时解除屏蔽；另一个线程发送 `SIGUSR1`；handler 必须运行；handler 返回后 `rt_sigsuspend` 返回 `EINTR`；随后查询 mask，`SIGUSR1` 仍处于旧 mask 的屏蔽状态。
  - signal 在调用 `rt_sigsuspend()` 前已经 pending；syscall 不应永久睡眠。
  - signal 在 begin wait 与 schedule 之间到达时不 lost wake，可用 stress 循环提高覆盖概率。
  - `ppoll` / `pselect6` 带临时 mask 被 handler 打断后，handler 运行且返回后旧 mask 恢复。
  - `rt_sigreturn` 从 handler sigframe 恢复的 mask 是 frame 中保存的旧 mask，且不会消费 pending restore slot。
- LTP：
  - `rt_sigsuspend01`
  - `sigsuspend01`
  - 现有 signal group 中依赖 mask 恢复的用例。

审计：

- 所有 `TemporarySigMaskToken` 路径都有显式终止。
- delayed-restore callsite 没有仅凭 wait-core `Signal` / `Force` outcome 直接 defer。
- 所有 `restore_temporary_sig_mask_if_pending()` 调用点都在 signal 模块边界内。
- `rt_sigreturn` 只通过 current-mask API 恢复 sigframe mask，不绕开 `TaskSigMaskState`，不清理 delayed restore slot。
- 没有新增 arch-specific cleanup 分叉。

write set：

- 测试 profile / user smoke 程序按 repo 现有测试布局选择。
- 不在本阶段扩大 signal ABI Scope。

验证：

- `just build`
- rv64 用户测试 profile 或用户指定的最小 validation floor。
- 如用户要求，再扩展到 la64。

退出条件：

- 实现 transaction 可关闭。
- RFC status / 收口说明已更新。
- 剩余失败能明确归类为非目标、环境限制、accepted limitation 或 follow-up RFC。

## 旁路审计清单

实现 review 必须执行以下搜索并分类：

- `rg -n "sig_mask|set_sig_mask|TaskSigMaskState|TemporarySigMaskToken" anemone-kernel`
- `rg -n "restore_now|defer_to_signal_delivery|begin_temporary_sig_mask|restore_temporary_sig_mask_if_pending" anemone-kernel`
- `rg -n "perform_signal_action|handle_signals|rt_sigreturn" anemone-kernel/src/task/sig`
- `rg -n "ppoll|pselect6|rt_sigtimedwait|rt_sigsuspend|wait_for_iomux_ready" anemone-kernel/src`
- `rg -n "WaitOutcome::Signal|WaitOutcome::Force|CurrentWaitOutcome::Signal|CurrentWaitOutcome::Force|has_unmasked_signal|SysError::Interrupted" anemone-kernel/src`

允许保留的旁路：

- `TaskSigMaskState` owner 内部和初始化路径。
- Stage 1A 已迁移到普通 current-mask API 的 `rt_sigprocmask` 永久 mask 修改路径。
- Stage 1A 已分类的 clone / fork current-mask 继承路径。
- Stage 1A 已分类的 procfs/status 等只读 snapshot path。
- Stage 5 明确保留的 `rt_sigtimedwait` syscall-body-only temporary unmask / restore 与 waited-set dequeue 路径。

不允许保留的旁路：

- `TaskSigMaskState` owner 外 direct `sig_mask` mutation，除非已在 Stage 1A 审计中明确分类。
- `restore != None` 时普通永久 mask mutation 静默覆盖 pending restore。
- 可复制、非 must-use 或 drop-time restore 的 `TemporarySigMaskToken`。
- `rt_sigsuspend` syscall body 早恢复旧 mask。
- `ppoll` / `pselect6` 独立保存/恢复旧 mask。
- `ppoll` / `pselect6` 或 `fs/api/iomux/wait.rs` 仅凭 `Signal` / `Force` 直接 defer 或提前映射 `EINTR`。
- syscall body 或 fs/iomux 复制 signal delivery policy，而不是调用 signal-owned classifier。
- handler frame commit 前消费 restore slot。
- arch trap-return 层各自处理 restore cleanup。

## 可观测性清单

- nested temporary mask begin 的 fail-closed path 有清晰 assertion 或错误日志。
- unexpected wait outcome 记录 task id、outcome 和 pending/unmasked signal 状态。
- signal-owned classifier 的 restore/fail-closed decision 记录 task id、typed candidate、decision 和 pending/unmasked signal 状态。
- token identity mismatch 和 leaked active token 记录 task id / slot context，且不执行 drop-time restore。
- sigframe write failure 日志包含 task id、signal number 和用户栈地址。
- transaction devlog 记录每阶段 agent-run / user-run / unrun validation。

## 停止边界

应停止并回到 RFC review 的情况：

- 需要多层 temporary mask restore stack。
- 需要把 `rt_sigtimedwait` 纳入 delayed restore helper。
- 需要改变 `rt_sigprocmask` 永久 mask 语义。
- 需要引入完整 Linux restart errno / `restart_syscall`。
- 需要把 cleanup 语义下沉到 arch-specific trap-return 层。

可以继续实现的情况：

- 命名调整但状态转换保持不变量。
- helper 放置模块调整但仍由 signal 子系统拥有。
- smoke / LTP 暴露的是已列入本 RFC 的 delayed restore 路径缺口。

## Write Set 扩展记录

- 暂无。实现开始后，任何超出本计划默认 write set 的架构性扩展都必须记录原因、拟新增范围、受影响 contract、验证 gate 和批准来源。
