# signal temporary mask restore 不变量需求

**状态：** Draft
**最后更新：** 2026-06-06
**父 RFC：** [RFC-20260606-signal-temp-mask-restore](./index.md)

## 闭合条件

本 RFC 闭合的是临时 signal mask delayed restore 协议，不是完整 Linux signal 子系统。

实现完成后必须同时满足：

- `rt_sigsuspend` 安装临时 mask 后，pending/unmasked signal 的选择使用临时 mask。
- signal interrupt 返回 syscall body 后，旧 mask 不会在 trap-return signal delivery 前过早恢复。
- 用户 handler frame 保存进入临时 mask 前的旧 mask。
- handler frame 成功 commit 后，restore slot 的恢复责任转移给 `rt_sigreturn()`。
- 没有建立 handler frame 时，返回用户态前由 signal 模块统一 cleanup 恢复旧 mask。
- `ppoll` / `pselect6` 使用同一 delayed restore helper，不保留独立早恢复模型。
- delayed-restore callsite 只有在 signal-owned `classify_temporary_mask_wait()` 已为当前 task 稳定保留 delivery target 并返回 `DeferToTrapReturnDelivery` 后，才允许 defer restore slot。
- `rt_sigtimedwait` 不迁入 delayed restore helper，仍按 waited set 主动取走 signal，并在 syscall body 内恢复 mask。
- signal arrival 与 wait-core begin/precheck/schedule 之间不丢 wake。
- `TaskSigMaskState` 是 current mask 和 restore slot 的单一真相源。

## 非目标

- 不证明完整 Linux restart errno、`restart_syscall` 或 `ERESTARTNOHAND` 语义。
- 不证明 `signalfd`、job-control stop/continue、pidfd signal 或完整 realtime signal queue 行为。
- 不改变 `rt_sigprocmask` 的永久 mask 修改 contract。
- 不把 `rt_sigtimedwait` 改造成 trap-return delivery 接口。
- 不定义多层 temporary mask restore stack。
- 不重写 signal disposition、altstack、arch trampoline 或 pending signal queue 的整体模型。

## 状态所有权

临时 mask 状态由当前 task 的 signal state 拥有。current mask 和 restore slot 必须合并为一个状态：

```text
TaskSigMaskState {
    current: SigSet,
    restore: Option<SigSet>,
}
```

字段语义：

- `current` 是当前用于 signal pending/unmasked 判断的 mask。
- `restore` 为 `None` 时，没有未决 delayed restore。
- `restore = Some(old_mask)` 表示当前 task 正处于临时 mask window；如果建立 handler frame，sigframe 应保存 `old_mask`；如果没有 handler delivery，返回用户态前应恢复 `old_mask`。

所有读写 current mask、安装临时 mask、查询 sigframe 应保存的 mask、消费 restore slot 和 cleanup restore slot 的 helper 都必须通过 `TaskSigMaskState`。不得在 task 中新增第二个独立 restore truth source。

`restore != None` 时，普通永久 mask mutation 默认非法或不可达。唯一允许在 pending restore window 内改变 `current` 的路径是 signal delivery 安装 handler mask：`perform_signal_action()` 已经选定 sigframe 要保存的旧 mask，并正在把 handler `sa_mask` / self-mask 合入当前 mask。其它规则如下：

- `rt_sigprocmask` 这类永久 mask 修改接口不得在 pending restore window 内静默覆盖 `current` 或 `restore`；实现必须让该路径不可达、fail-closed，或通过 signal-owned helper 显式断言其前置条件。
- clone / fork 继承和 procfs snapshot 只能观察或继承 `current`，不得复制 `restore`，也不得把 pending restore slot 暴露为子线程或 procfs 语义。
- `rt_sigreturn` 只通过用户 frame 中保存的 mask 恢复 `current`；它不得读取、消费或覆盖 pending restore slot。
- signal 模块内部的 frame commit / no-frame cleanup 是消费 pending restore slot 的唯一归口。

## TemporarySigMaskToken 模型

`begin_temporary_sig_mask(new_mask)` 创建一个显式终止的 token：

```text
begin_temporary_sig_mask(new_mask) -> TemporarySigMaskToken
TemporarySigMaskToken::restore_now()
TemporarySigMaskToken::defer_to_signal_delivery()
```

状态转换要求：

- token 类型必须标记 `#[must_use]`，且不得实现 `Clone` 或 `Copy`。
- token 记录创建它的 task identity 与 restore slot identity；终止时必须校验仍匹配当前 task 的 pending restore slot。
- `restore_now(self)` 和 `defer_to_signal_delivery(self)` 消耗 `self`；调用后不能再次终止同一 token。
- begin 保存旧 mask 到 `restore`，安装 `new_mask` 到 `current`。
- 如果 begin 时已经存在 `restore`，必须在安装 `new_mask` 前 fail-closed 或触发内部 invariant；不得静默覆盖，也不得产生 token。
- token drop 没有自动恢复语义。离开 syscall body 不能隐式恢复旧 mask；`Drop` 最多 assert 或记录 active-token leak，不得恢复 mask、清空 restore slot 或选择 defer。
- token 只有两个合法终止态：`restore_now()` 和 `defer_to_signal_delivery()`。
- 非 signal-delivery 返回路径必须显式 `restore_now()`，恢复 `current = old_mask` 并清空 `restore`。
- 确认会进入 trap-return signal delivery 的 carrier 路径必须显式 `defer_to_signal_delivery()`。
- defer 后 syscall body 不再拥有旧 mask 恢复责任。
- defer 后旧 mask 只能由 handler frame commit 点或 `handle_signals()` 无 frame cleanup 恢复。

禁止用普通 RAII guard 表达该协议，因为 drop-time restore 会破坏 trap-return delivery 前的 mask 选择。禁止增加 `cancel_without_delivery()` 这类第三终止态；如果临时 mask 已安装，取消就等价于 `restore_now()`。

## Handler Frame Commit 规则

用户 handler 路径必须按以下顺序转移恢复责任：

```text
mask_to_save = task.sigmask_to_save_for_signal_frame()
current sig_mask = current sig_mask + handler masks
encode_ucontext(..., mask_to_save, ...)
write_sigframe_and_prepare_trapframe(...)
task.signal_frame_committed_restore_mask()
```

`signal_frame_committed_restore_mask()` 的唯一合法语义是：用户 sigframe 已经写入成功，trapframe 已经准备跳转到 handler，旧 mask 恢复责任已经转移给 `rt_sigreturn()`。

禁止行为：

- 读取 `mask_to_save` 后立刻清除 restore slot。
- sigframe 写入前清除 restore slot。
- trapframe 准备完成前清除 restore slot。
- frame 写失败后继续返回用户态但 restore slot 已被清除。
- 不返回用户态的终止路径伪装成“已交给 `rt_sigreturn()`”。

若 frame 写入或 trapframe 准备失败并且路径可能返回用户态，restore slot 必须保留给统一 cleanup。若路径终止进程或线程组，不需要恢复旧 mask，但必须不能留下会返回用户态的半提交状态。

## 无 Handler Frame Cleanup 规则

`handle_signals()` 是无 handler frame delayed restore cleanup 的归口。默认 terminate 不返回，无需恢复。default ignore / explicit ignore 如果消费了 pending signal 但不建立用户 handler frame，则返回用户态前必须恢复旧 mask。

要求：

- cleanup 放在 signal 模块内部，避免 riscv64 / loongarch64 trap-return 层各自复制语义。
- `handle_signals()` 结束时若没有留下用户 handler frame，应调用 `restore_temporary_sig_mask_if_pending()` 或等价 helper。
- cleanup 必须幂等：没有 pending restore slot 时不改变 current mask。
- cleanup 不能消费已经成功 commit 给 handler frame 的 restore slot。

## Wait-Core 线性化点

`rt_sigsuspend` 的 wait 发布顺序固定为：

1. 安装临时 mask。
2. begin wait-core active wait。
3. precheck `has_unmasked_signal()`。
4. schedule。

线性化要求：

- signal 在步骤 1 后、步骤 2 前到达，precheck 必须看到 pending/unmasked signal 并取消 wait round。
- signal 在步骤 2 后、步骤 4 前到达，发送侧 `notify()` 与 precheck 竞争同一个 wait-core completion。
- signal 在真正 parked 后到达，发送侧通过 wait-core 唤醒。
- 确认会进入 trap-return delivery 的 syscall body 只提交 `defer_to_signal_delivery()`，不主动 `fetch_signal()`。
- 真正消费 signal 的位置仍是 trap-return `handle_signals()`。

`rt_sigsuspend`、`ppoll` 和 `pselect6` 的 delayed-restore outcome 分类遵守同一规则：wait-core `Signal` / `Force` 只是 typed candidate，不是 defer proof。defer eligibility 必须由 signal-owned classifier 决定；syscall body、fs/iomux helper 和 wait-core helper 不得自己解释 pending queue、disposition、ignore/default/custom action 或 force wake。

`has_unmasked_signal()` 只能用于 wait precheck，证明“当前 mask 下已有可唤醒 wait 的 signal”；它不能证明该 wake 一定会建立 handler frame、一定会由 trap-return `handle_signals()` 消费，或一定允许 delayed restore defer。分类完成后，callsite 必须先终止 token，再映射 ABI 结果。fs/iomux 不得在 temporary token active 的语义路径上提前把 `Signal` / `Force` ABI-map 成 `EINTR`。

## Signal Handoff / Reservation 规则

`classify_temporary_mask_wait()` 返回 `DeferToTrapReturnDelivery` 时，必须同时完成 signal-owned handoff。该返回值表示当前 task 已经拥有一个稳定 delivery target，而不是只观察到 pending/unmasked signal。

要求：

- private pending signal 可通过在当前 task 内标记 reserved delivery target、从 pending queue 移入 task-local handoff slot，或等价机制完成 handoff。
- shared pending signal 必须在 signal 子系统内部完成 claim / move / reservation，使它不再能被同一 thread group 的其它 eligible member 竞争消费。
- reservation 必须与 `handle_signals()` 的实际 fetch / delivery 路径对接；trap-return 优先消费当前 task 的 reserved delivery target，再按普通 pending 规则继续。
- 若 classifier 不能稳定 reserve 或 handoff signal target，不得返回 `DeferToTrapReturnDelivery`；必须返回 `RestoreThenReturn`、`RestoreThenFailClosed` 或 `NoReturnForce`。
- reserved target 被 default ignore / explicit ignore 消费但没有建立 handler frame 时，仍由 signal 模块的无 handler frame cleanup 恢复旧 mask。
- reserved target 若在 handler frame commit 前遇到可返回用户态的失败路径，restore slot 不能丢失；失败必须回到 signal 模块 cleanup 或 fail-closed 路径。

禁止把 `DeferToTrapReturnDelivery` 实现为纯观察型 proof：不能先观察 shared pending signal，然后在没有 claim / reservation 的情况下 defer restore 并依赖之后的 `fetch_signal()` 再竞争一次。

## 锁序与生命周期规则

当前 signal 锁顺序仍以现有文档化顺序为基础：

```text
sig_pending -> sig_mask -> sig_disposition
```

本 RFC 把 `sig_mask` 概念升级为 `TaskSigMaskState` 的单锁状态，因此不会新增 `sig_mask -> sig_restore_mask` 或 `sig_restore_mask -> sig_mask` 的双向顺序。

要求：

- `TaskSigMaskState` 由单个 `NoIrqSpinLock<TaskSigMaskState>` 保护。
- pending signal 检查读取 current mask 时，遵守现有 `sig_pending -> sig_mask` 顺序。
- disposition 读取不能在持有不必要的 mask state 锁时扩大临界区。
- helper 不得在持有 mask state 锁时执行用户态 copy。
- helper 不得在持有 mask state 锁时进入 wait-core schedule。

## ABI 边界

`rt_sigsuspend`：

- `sigsetsize` 必须按 Linux-compatible 规则校验。
- 用户 mask copy-in 成功后清除 `SIGKILL` / `SIGSTOP`。
- 正常不会成功返回；被 signal 打断后用户可见结果是 `-EINTR`，但该返回是 trap-return delivery 的 carrier。

`ppoll` / `pselect6`：

- 如果用户提供临时 mask，使用同一 begin/defer/restore helper。
- ready count、timeout、copy-out error 等非 signal 返回路径必须恢复旧 mask。
- signal interrupted 路径不得仅凭 wait-core outcome defer；必须把 typed candidate 交给 signal-owned classifier，并只在其返回 `DeferToTrapReturnDelivery` 后 defer。
- 无法确认 trap-return delivery 时必须先恢复旧 mask，再按 syscall error、fail-closed 或 force 路径处理。

`rt_sigtimedwait`：

- 不使用 delayed restore helper。
- 临时 unmask / restore 局限在 syscall body。
- waited signal 由 syscall body 主动取走并返回 signal number。
- wait 被 signal / force 唤醒后，必须先按 waited set 重新尝试取走 matching signal。
- 如果拿到 waited signal，恢复旧 mask 并返回 signal number。
- 如果没有 matching waited signal，恢复旧 mask 后返回 `EINTR` 或处理 force。
- 任何路径都不把恢复责任交给 `rt_sigreturn()`。

## 禁止退化项

- 在 syscall body 中手写 `old = mask; set(new); wait; set(old)` 来实现 `rt_sigsuspend`。
- 用 drop-time RAII guard 恢复旧 mask。
- 让 handler frame 保存临时 mask。
- 在 handler frame commit 前消费 restore slot。
- 让 default ignore / explicit ignore 消费 signal 后携带临时 mask 返回用户态。
- 增加 `cancel_without_delivery()` 这类第三 token 终止态。
- 在任一 delayed-restore callsite 中把 wait-core `Signal` / `Force` outcome 直接等同于 `defer_to_signal_delivery()`。
- 在 temporary token active 的语义路径上由 fs/iomux helper 把 wait-core `Signal` / `Force` 过早映射成 `EINTR`。
- 在 delayed-restore callsite 中用 `has_unmasked_signal()` 代替 signal-owned classifier。
- 把 `Force` 当成普通 `EINTR`。
- 在 `rt_sigsuspend` body 中主动 `fetch_signal()`。
- 保留 `ppoll` / `pselect6` 的独立早恢复逻辑。
- 将 `rt_sigtimedwait` 误迁入 delayed restore helper。
- 拆出独立 restore slot 锁并在实现阶段补口头锁序。

## 完成标准

- `TaskSigMaskState` 和 temporary mask helper 的状态转换可由代码审查逐项对应本文件。
- `perform_signal_action()` 使用 restore slot 选择 sigframe mask，并只在 frame commit 点消费。
- `handle_signals()` 覆盖无 handler frame cleanup。
- `rt_sigsuspend`、`ppoll` 和 `pselect6` 的 wait-core outcome mapping 不把 `Signal` / `Force` 直接等同于 defer。
- `ppoll` / `pselect6` 共享 delayed restore helper。
- `rt_sigtimedwait` 的 signal / force wake 后 waited-set dequeue 修复完成，且未被误迁入 helper。
- smoke 和 LTP 证据能覆盖 handler delivery、pending-before-call、lost-wake stress、`ppoll` / `pselect6` interrupt 和 mask restore。
