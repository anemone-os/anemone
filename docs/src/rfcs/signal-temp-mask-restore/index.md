# RFC-20260606-signal-temp-mask-restore

**状态：** Accepted for Implementation
**负责人：** doruche, Codex
**最后更新：** 2026-06-07
**领域：** signal / wait-core / syscall ABI
**事务日志：** [2026-06-06-signal-temp-mask-restore](../../devlog/transactions/2026-06-06-signal-temp-mask-restore.md)
**开放问题：** None；document-layer review 与软件工程审查发现的问题均已折回 accepted contract，并列入 [Tracking Issues](./tracking-issues.md) 的 Neutralized。
**下一步：** 阶段 1A / 1B / 2、signal-owned classifier / stable handoff、`rt_sigsuspend` 与 `ppoll` / `pselect6` typed outcome 迁移已完成，并通过 Gate 1 / Gate 2 / Gate 3 / Gate 4 / Gate 5 / Gate 6 review；下一步按 [Agent 编排建议](./backgrounds/agent-orchestration.md) 启动 Agent 7 的 `rt_sigtimedwait` 本地 waited-set dequeue 边界修复，当前尚未启动。

## 摘要

本 RFC 定义 Anemone 的临时 signal mask delayed restore 语义。它的直接触发点是补齐 `rt_sigsuspend`，但范围已经扩展到一组共享协议：`rt_sigsuspend`、`ppoll` 和 `pselect6` 在 syscall body 中安装临时 mask，被 signal 打断后不能立即恢复旧 mask，而必须把旧 mask 的恢复责任推迟到 trap-return signal delivery 边界。

核心方案是在 task signal state 中引入单一的临时 restore slot。signal handler frame 成功建立时，sigframe 保存进入临时 mask 前的旧 mask，恢复责任转移给 `rt_sigreturn()`；没有建立 handler frame 时，`handle_signals()` 的统一 cleanup 在返回用户态前恢复旧 mask。这样既保留临时 mask 对 pending/unmasked signal 选择的影响，又保证 handler 返回后恢复到 syscall 进入前的 mask。

## 背景

Anemone 当前还没有注册 `rt_sigsuspend`。表面上它只是读取用户 mask、临时替换当前线程 mask、阻塞等待任意未屏蔽 signal、然后返回 `-EINTR`。真正的难点不是 syscall number，而是 signal arrival、wait-core 入睡、trap-return signal delivery 和旧 mask 恢复之间的并发与生命周期。

当前基础设施已有两个关键前提：

- wait-core 已经能表达当前 task 的 interruptible wait round。signal、timeout、predicate-ready 和 force wake 能竞争同一个 wait state，并由 stale-safe wake placement 收口。
- signal mask 只有一个 `Task::sig_mask` 当前值；没有 Linux 风格的 `saved_sigmask` / `restore_sigmask` 临时恢复状态。

因此不能实现成简单的：

```text
old = current.sig_mask
current.sig_mask = new
wait until signal
current.sig_mask = old
return EINTR
```

如果 syscall 返回前恢复旧 mask，trap-return 的 `handle_signals()` 可能看不到触发 `sigsuspend` 的 signal。典型情况是旧 mask 本来屏蔽了该 signal：syscall 被唤醒后立刻恢复旧 mask，随后 signal delivery 再按旧 mask 检查 pending signal，handler 不会运行。

反过来，如果不恢复旧 mask，当前 handler frame 若把临时 mask 写入 `ucontext.uc_sigmask`，用户 handler 返回并调用 `rt_sigreturn()` 后会恢复到临时 mask，而不是进入 `sigsuspend` 前的旧 mask。

Linux `rt_sigsuspend` 的关键形态是保存旧 blocked mask、安装新 blocked mask、等待 signal pending、标记返回用户态前需要 restore。架构 signal delivery 侧通过 `sigmask_to_save()` 选择写入用户 sigframe 的 mask：如果存在 restore flag，则保存 saved mask，而不是当前临时 blocked mask。Anemone 不需要复制 Linux 内部命名，但需要等价语义。

## 目标

- 注册并实现 `rt_sigsuspend`，使其通过 wait-core 等待 pending/unmasked signal。
- 建立 task-owned 临时 signal mask restore slot，表达旧 mask 的延迟恢复责任。
- 让 handler frame 保存进入临时 mask 前的旧 mask，使 `rt_sigreturn()` 后恢复到正确 mask。
- 让无 handler frame 的 trap-return 路径在返回用户态前恢复旧 mask。
- 固定 `TemporarySigMaskToken` 的显式状态转换，禁止 drop-time 自动恢复。
- 让 `ppoll` / `pselect6` 迁移到同一临时 mask helper，不再各自手写早恢复逻辑。
- 保持 signal arrival 与 wait-core begin/precheck/schedule 之间不丢 wake。
- 明确 `rt_sigtimedwait` 不迁入 delayed restore helper，避免边界误归类。

## 非目标

- 不实现 `signalfd`、`pidfd_send_signal`、job-control stop/continue 或完整 signal restart block。
- 不改变 `rt_sigprocmask` 的基本 ABI；它仍是普通永久 mask 修改接口。
- 不把 `rt_sigtimedwait` 改成 `sigsuspend` 语义，也不迁入 delayed restore helper。
- 不扩大到完整 Linux `ERESTARTNOHAND` / `restart_syscall` 编码体系。当前 Anemone 可以继续把用户可见结果映射为 `EINTR`，但必须保证 handler delivery 与 mask 恢复顺序正确。
- 不在本 RFC 中重构 signal disposition、pending signal queue、altstack 或 arch trampoline。
- 不在本 RFC 提升阶段启动实现事务日志、register 或 current limitations 更新。

## 文档地图

Canonical：

- [不变量需求](./invariants.md)
- [迁移实施计划](./implementation.md)
- [Tracking Issues](./tracking-issues.md)

背景材料：

- [背景材料](./backgrounds/index.md)

## 方案

本 RFC 采用 task 级临时 signal mask restore slot，而不是在每个 syscall body 中手写 `old = mask; set(new); wait; set(old)`。restore slot 与 current mask 合并到同一个 `TaskSigMaskState`，由单个 `NoIrqSpinLock<TaskSigMaskState>` 保护，避免新增 `sig_mask -> sig_restore_mask` / `sig_restore_mask -> sig_mask` 的双向锁序。

建议的核心状态：

```text
TaskSigMaskState {
    current: SigSet,
    restore: Option<SigSet>,
}
```

建议的核心 helper：

```text
begin_temporary_sig_mask(new_mask) -> TemporarySigMaskToken
TemporarySigMaskToken::restore_now()
TemporarySigMaskToken::defer_to_signal_delivery()
sigmask_to_save_for_signal_frame() -> SigSet
signal_frame_committed_restore_mask()
restore_temporary_sig_mask_if_pending()
classify_temporary_mask_wait(candidate, context) -> TemporaryMaskWaitDecision
```

`classify_temporary_mask_wait()` 是 signal-owned 分类和 handoff 边界。`rt_sigsuspend`、`ppoll` 和 `pselect6` 只提供 typed wait outcome 与 syscall context；它们不得自己解释 pending queue、disposition、ignore/default/custom action 或 force wake。分类结果建议至少表达：

```text
TemporaryMaskWaitDecision {
    DeferToTrapReturnDelivery,
    RestoreThenReturn(result_or_errno),
    RestoreThenFailClosed,
    NoReturnForce,
}
```

`DeferToTrapReturnDelivery` 不只是观察到“可能存在可投递 signal”。它必须表示 signal 子系统已经为当前 task 稳定保留了一个 trap-return delivery target。若 target 来自 task-private pending queue，classifier 必须把该 signal 标记为当前 task 的 reserved delivery target，或用等价机制防止后续检查丢失。若 target 来自 thread-group shared pending queue，classifier 必须在 signal 子系统内部完成 claim / move / reservation，使该 signal 不再能被同一 thread group 的其它 eligible member 抢走。无法建立稳定 handoff 时，classifier 不得返回 `DeferToTrapReturnDelivery`，只能返回 restore / fail-closed / no-return force 类决策。

`TemporarySigMaskToken` 没有 drop-time restore 语义，并且只有两个终止态。非 signal-delivery 返回路径必须显式 `restore_now()`，语义是恢复 `current = old_mask` 并清空 `restore` slot。signal-delivery carrier 返回路径必须显式 `defer_to_signal_delivery()`，语义是 syscall body 放弃旧 mask 恢复责任。begin 失败必须发生在安装临时 mask 前，因此不产生 token；一旦临时 mask 已安装，所谓 cancel 就等价于 `restore_now()`，本 RFC 不保留独立 cancel API。

`perform_signal_action()` 的用户 handler 路径必须先用 `sigmask_to_save_for_signal_frame()` 选择 sigframe 保存的 mask，再安装 handler `sa_mask` / self-mask，写入 sigframe，准备 trapframe，最后在 handler frame commit 点调用 `signal_frame_committed_restore_mask()`。

`rt_sigsuspend` 安装临时 mask 后进入 wait-core；precheck 可以使用 `has_unmasked_signal()` 避免已经可见的 signal 丢失 wait round，但这个布尔结果不是 defer proof。`rt_sigsuspend`、`ppoll` 和 `pselect6` 共享 delayed-restore outcome 规则：不能仅凭 wait-core outcome 是 `Signal` / `Force` 就调用 `defer_to_signal_delivery()`，也不能在 syscall body 或 fs/iomux helper 中复刻 signal delivery policy。它们必须把 typed wait outcome、是否存在 temporary token、syscall 类型和必要的 ABI context 交给 `classify_temporary_mask_wait()`；signal 子系统根据 current mask、pending queue、disposition、ignore/default/custom action、force 语义和 reservation 可行性返回分类结果。调用方收到分类结果后，必须先以 `restore_now()` 或 `defer_to_signal_delivery()` 终止 token，再映射用户可见 errno、ready count、timeout 或不可返回路径。

`ppoll` / `pselect6` 的临时 mask 逻辑迁移到同一 helper。ready count、timeout 和 syscall error 等非 signal 返回路径通过 `restore_now()` 恢复旧 mask 后返回；确认会进入 trap-return delivery 的 signal interrupted 路径不在 syscall body 中恢复旧 mask，而是交给 signal delivery / cleanup。

`rt_sigtimedwait` 保持在 helper 外。它按 waited set 主动取走 matching signal，并在 syscall body 内返回 signal number；它不是“等待任意未屏蔽 signal 然后交给 trap-return delivery”的接口。若 wait 被 signal / force 唤醒，`rt_sigtimedwait` 必须先按 waited set 重新尝试 dequeue matching signal；若拿到 waited signal，恢复 mask 并返回 signal number；若没有 matching waited signal，恢复 mask 后返回 `EINTR` 或处理 force。

## 接受边界

接受本 RFC 意味着 signal temporary mask restore 可以按 [迁移实施计划](./implementation.md) 推进。第一阶段成功标准是 delayed restore protocol 正确闭合，不是完整 Linux signal restart 体系等价。

以下变化必须回到本 RFC 或新增 follow-up RFC：

- 允许同一 task 存在多层 pending temporary mask restore stack。
- 将 `rt_sigtimedwait` 迁入 delayed restore helper。
- 改变 `rt_sigprocmask` 的永久 mask 修改语义。
- 任一 delayed-restore callsite 仅凭 wait-core `Signal` / `Force` outcome 调用 `defer_to_signal_delivery()`。
- 把 `Force` outcome 直接暴露为普通可恢复 `EINTR`。
- 在 arch trap-return 层分别复制 cleanup 逻辑，而不是通过 signal 模块内部统一恢复点收口。
- 引入 Linux 风格 restart errno / `restart_syscall`，并改变本 RFC 的 `EINTR` carrier 边界。
- 将 `TaskSigMaskState` 拆回多把锁或新增未记录的锁序。

## 备选方案

### 只注册 `rt_sigsuspend` 并在 syscall body 恢复旧 mask

拒绝。旧 mask 若屏蔽触发 signal，syscall 被唤醒后立刻恢复旧 mask 会让 trap-return delivery 看不到 pending signal，handler 不会运行。

### 只推迟恢复，不改变 sigframe 保存来源

拒绝。这样 handler frame 会保存临时 mask，`rt_sigreturn()` 后恢复到临时 mask，而不是 syscall 进入前的旧 mask。

### 只修 `rt_sigsuspend`，不迁移 `ppoll` / `pselect6`

拒绝。`ppoll` / `pselect6` 是同类临时 mask syscall；保留各自手写早恢复逻辑会让公开 signal wait ABI 有两套生命周期模型。

### 在 `rt_sigsuspend` body 中主动 `fetch_signal()`

拒绝。这样会绕开默认 action、handler frame、SA_RESTART、altstack 和 `rt_sigreturn` 的现有路径。

### 用普通 RAII guard drop 恢复旧 mask

拒绝。guard 在 syscall body 返回时 drop 会重新制造 signal interrupt 返回后、trap-return delivery 前过早恢复 mask 的问题。

### 两把锁加固定顺序

拒绝作为本 RFC 方向。它能局部修补死锁风险，但会把 `current` 和 `restore` 拆成两个真相源。本 RFC 直接采用单一 `TaskSigMaskState`。

## 风险

- delayed-restore callsite 如果只按 wait-core `Signal` / `Force` outcome 分类，会把 fatal/stop 或非 delivery wake 降级成普通 `EINTR` carrier。控制方式是所有 callsite 在 defer 前都必须调用 signal-owned classifier，且不得在 syscall body 或 fs/iomux 中自行解释 pending/disposition/force。
- handler frame 写入失败若过早消费 restore slot，会丢失旧 mask 恢复责任。控制方式是只允许 frame commit 点消费 restore slot。
- default ignore / explicit ignore 路径不建立 handler frame，容易遗漏 cleanup。控制方式是把无 handler frame cleanup 放在 signal 模块内部统一恢复点。
- 单一 restore slot 禁止嵌套 pending restore。控制方式是在 begin helper 中 fail-closed 或触发内部 invariant；合法 handler 内再次调用属于新 syscall 上下文，上一轮 slot 应已被 sigframe commit 消费。
- `rt_sigtimedwait` 与 delayed restore helper 混用会改变 ABI。控制方式是把它明确排除在 helper 外，只做 syscall-body 本地语义修复和边界审查。

## 收口

进入实现后，事务日志需要记录：

- `TaskSigMaskState` 和 helper API 的实现位置。
- `perform_signal_action()` 中 sigframe mask 来源和 commit 消费点。
- `handle_signals()` 无 handler frame cleanup 的统一恢复点。
- `rt_sigsuspend` syscall number、handler 注册和 wait-core outcome mapping。
- `ppoll` / `pselect6` 迁移到 shared helper 的证据。
- `rt_sigtimedwait` 未迁入 delayed restore helper、并在本地修复 signal / force wake 后 waited-set dequeue 的边界审查。
- `just build`、自建 signal smoke、`rt_sigsuspend01`、`sigsuspend01` 和相关 signal group 验证结果。
