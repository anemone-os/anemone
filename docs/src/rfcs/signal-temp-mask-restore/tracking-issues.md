# signal temporary mask restore Tracking Issues

**状态：** Closed
**最后更新：** 2026-06-06
**父 RFC：** [RFC-20260606-signal-temp-mask-restore](./index.md)
**事务日志：** [2026-06-06-signal-temp-mask-restore](../../devlog/transactions/2026-06-06-signal-temp-mask-restore.md)

本文只跟踪 design review 后确认的 RFC 缺陷、证明缺口、边界冲突或需要回到 RFC 修改的设计问题。

实现前已知缺口、当前基础设施状态、暂缓范围、验证要求和阶段性交付项不写入本文；它们属于 [RFC index](./index.md) 的背景、非目标、风险，或 [迁移实施计划](./implementation.md) 的阶段内容。

## Apollyon

- 暂无。

## Keter

- 暂无。

## Euclid

- 暂无。

## Safe

- 暂无。

## Neutralized

### SIGTEMP-017 - classifier must reserve delivery target before defer

**来源等级：** Keter

**原问题：** `classify_temporary_mask_wait()` 已收口到 signal 子系统，但原 contract 仍像观察型 defer proof：它只说明 callsite 不能自己解释 pending queue、disposition 或 force，却没有说明返回 `DeferToTrapReturnDelivery` 时是否已经为当前 task 稳定保留了一个 delivery target。当前 shared pending signal 可由同一 thread group 的多个 eligible member 竞争；如果 classifier 只是观察 shared pending 后 defer，另一个线程可能先消费该 signal，本线程已经 `defer_to_signal_delivery()` 却没有可 delivery signal。

**Neutralized：** [RFC 主文档](./index.md) 将 `classify_temporary_mask_wait()` 升级为 signal-owned classification + handoff 边界，要求 `DeferToTrapReturnDelivery` 表示 signal 子系统已经为当前 task 稳定保留 delivery target。private pending signal 必须被标记 / 移入当前 task 的 reserved target；shared pending signal 必须在 signal 子系统内 claim / move / reserve，不能继续被其它 eligible member 抢走。[不变量需求](./invariants.md) 新增 Signal Handoff / Reservation 规则，禁止纯观察型 proof。[迁移实施计划](./implementation.md) 阶段 3 / 4 把 stable delivery target reservation / handoff 作为 classifier gate；无法 reserve 时必须 `restore_now()` 并走 restore / fail-closed / no-return force 决策。

### SIGTEMP-018 - Stage 1A ppoll / pselect6 legacy mask bypass needs explicit classification

**来源等级：** Euclid

**原问题：** [迁移实施计划](./implementation.md) 阶段 1A 明确把 `ppoll` / `pselect6` 的 typed outcome 和 delayed restore 迁移留到阶段 4，但阶段 1A 的 `rg` 审计和退出条件要求 direct `sig_mask` access 不留下未分类旁路。当前 `ppoll.rs` / `pselect6.rs` 仍有旧 save / set / restore mask path；执行者可能被迫提前改 iomux，或让阶段 1A 名义通过但没有记录这是阶段 4 必迁移债务。

**Neutralized：** [迁移实施计划](./implementation.md) 阶段 1A 现在显式登记 `ppoll.rs` 与 `pselect6.rs` 的旧 save / set / restore mask path 为 classified legacy temporary-mask path。它们允许暂留到阶段 4，但阶段 1A 交付记录必须点名该债务，且不得把它们算作已完成 delayed-restore 迁移。阶段 4 交付和审计明确要求删除或替换这些 Stage-1A 登记的 legacy path，并迁移到 typed outcome + shared delayed restore helper。

### SIGTEMP-011 - `iomux` wait helper must preserve signal / force outcome

**来源等级：** Keter

**原问题：** [迁移实施计划](./implementation.md) 阶段 4 要求 `ppoll` / `pselect6` 在 signal-interrupted 路径确认会进入 trap-return delivery 后才 `defer_to_signal_delivery()`。但当前共享 `iomux` wait helper 返回 `Result<usize, SysError>`，并在 helper 内把 `LatchWaitOutcome::Signal | LatchWaitOutcome::Force` 压成 `SysError::Interrupted`。调用方拿不到原始 wait outcome，也无法区分 ready、timeout、syscall error、signal candidate 和 force。

**Neutralized：** [RFC 主文档](./index.md) 将 `classify_temporary_mask_wait(candidate, context)` 固定为 signal-owned 分类边界，要求 `rt_sigsuspend`、`ppoll` 和 `pselect6` 只提供 typed wait outcome 与 syscall context，并在 token 终止后再映射 errno / result。[不变量需求](./invariants.md) 明确 wait-core `Signal` / `Force` 只是 typed candidate，fs/iomux 不得在 temporary token active 路径上提前 ABI-map 成 `EINTR`。[迁移实施计划](./implementation.md) 阶段 4 把 `anemone-kernel/src/fs/api/iomux/wait.rs` 列为必改 write set，并要求 helper 至少区分 ready、timeout、syscall/register error、signal candidate 和 force。

### SIGTEMP-012 - defer eligibility must be signal-owned

**来源等级：** Keter

**原问题：** RFC 当前把“确认存在真实 pending/unmasked signal，且会交给 trap-return `handle_signals()` 消费”的责任写给每个 delayed-restore callsite。当前可见 API 主要是 `Task::has_unmasked_signal()` 这类布尔查询；真正 delivery 行为由 signal 子系统内部的 `fetch_signal()`、`perform_signal_action()` 和 `handle_signals()` 决定。

**Neutralized：** [RFC 主文档](./index.md) 将 defer eligibility 收口为 signal-owned classifier：syscall body 和 fs/iomux 不能复制 pending queue、disposition、ignore/default/custom action 或 force wake policy。[不变量需求](./invariants.md) 规定 `has_unmasked_signal()` 只能作为 wait precheck，不是 defer proof；只有 `classify_temporary_mask_wait()` 已完成 stable delivery target reservation / handoff 并返回 `DeferToTrapReturnDelivery` 后才允许 defer restore slot。[迁移实施计划](./implementation.md) 阶段 3 / 4 均把 signal-owned classifier 作为 gate 和 write set 边界。

### SIGTEMP-013 - Stage 1 write set and gate are not executable

**来源等级：** Keter

**原问题：** [迁移实施计划](./implementation.md) 阶段 1 要把 `sig_mask` 升级为 `TaskSigMaskState`，并要求永久 mask API 不再绕开 `TaskSigMaskState`。但阶段 1 write set 只列 `task/mod.rs`、`task/sig/mod.rs` 和编译受影响的 clone/fork/procfs 读取点；当前直接写 mask 的路径还包括 `rt_sigprocmask`、`rt_sigreturn`、`rt_sigtimedwait`、clone 继承和已有 `ppoll` / `pselect6` 临时 mask 路径。

**Neutralized：** [迁移实施计划](./implementation.md) 将原阶段 1 拆成阶段 1A / 1B。阶段 1A 负责 `TaskSigMaskState` storage、普通 current-mask API 和 direct access 迁移，write set 纳入 `rt_sigprocmask.rs`、`rt_sigreturn.rs`、`rt_sigtimedwait.rs`、clone / fork 与 procfs snapshot 必要触点，并用 `rg` 旁路审计作为退出条件。阶段 1B 负责 temporary token/helper contract 和 `restore != None` 时普通 mask mutation 规则。`ppoll` / `pselect6` 的旧临时 mask 生命周期明确留到阶段 4 typed outcome 迁移，不再让阶段 1 名义闭合所有 callsite。

### SIGTEMP-014 - Ordinary mask mutation during pending restore is underspecified

**来源等级：** Euclid

**原问题：** RFC 要求所有读写 current mask、安装临时 mask、查询 sigframe mask、消费 restore slot 和 cleanup restore slot 都通过 `TaskSigMaskState`，但没有定义普通永久 mask API 在 `restore != None` 时的合法性。例如 `rt_sigprocmask`、clone inheritance、procfs snapshot 和 `rt_sigreturn` frame restore 是应 assert unreachable、拒绝、只读 `current`，还是允许改写 `current`。

**Neutralized：** [不变量需求](./invariants.md) 的“状态所有权”明确 `restore != None` 时普通永久 mask mutation 默认非法或不可达；唯一允许在 pending restore window 内修改 `current` 的路径是 signal delivery 安装 handler mask。clone / fork 继承和 procfs snapshot 只观察或继承 `current`，不得复制或暴露 `restore`；`rt_sigreturn` 只通过 frame restore API 恢复 `current`，不得读取、消费或覆盖 pending restore slot。[迁移实施计划](./implementation.md) 阶段 1A / 1B 把这些规则拆成 storage 迁移 gate 和 helper contract gate。

### SIGTEMP-015 - `TemporarySigMaskToken` must be a linear token

**来源等级：** Euclid

**原问题：** RFC 已规定 `TemporarySigMaskToken` 没有 drop-time restore，且只有 `restore_now()` 与 `defer_to_signal_delivery()` 两个终止态；但没有要求终止方法消耗 token、禁止 `Clone` / `Copy`、标记 `#[must_use]`，或对未终止 drop 做 invariant check。

**Neutralized：** [不变量需求](./invariants.md) 的 `TemporarySigMaskToken` 模型要求 token 标记 `#[must_use]`、不得实现 `Clone` / `Copy`，`restore_now(self)` 与 `defer_to_signal_delivery(self)` 消耗 `self`，并记录 task identity 与 restore slot identity 以便终止时校验匹配。`Drop` 最多 assert 或记录 active-token leak，不能恢复 mask、清空 restore slot 或选择 defer。[迁移实施计划](./implementation.md) 阶段 1B 将 linear token、slot identity、drop no-restore 和 exactly-one terminal path 列为交付、审计和旁路禁项。

### SIGTEMP-016 - Promotion gate and transaction closure are mixed

**来源等级：** Euclid

**原问题：** RFC index 曾把 review 通过后提升到公开 RFC、实现开始时建立 transaction devlog 和实现 transaction 关闭混在同一条收口路径里；[迁移实施计划](./implementation.md) 阶段 6 的退出条件也曾把“RFC 可提升为 Accepted for Implementation”和“实现 transaction 可关闭”放在同一个末尾 gate。

**Neutralized：** [RFC 主文档](./index.md) 的下一步明确公开 RFC review 与实现事务启动是两个阶段：若进入实现，先建立事务级 devlog，再按 [迁移实施计划](./implementation.md) 推进。[迁移实施计划](./implementation.md) 阶段 6 前置条件改为阶段 1A 到 5 已按公开 RFC 和 implementation transaction 完成，退出条件只保留 transaction 可关闭、RFC status / 收口说明已更新，以及剩余失败归类为非目标、环境限制、accepted limitation 或 follow-up RFC。

### SIGTEMP-001 - Default ignore cleanup

**来源等级：** Euclid

**原问题：** default ignore / explicit ignore 路径可能消费 pending signal 但不建立用户 handler frame。此时 pending restore mask 不会被 `rt_sigreturn()` 消费，如果没有统一 cleanup，task 会继续携带临时 mask 返回用户态。

**Neutralized：** [不变量需求](./invariants.md) 的“无 Handler Frame Cleanup 规则”把 cleanup 归口到 `handle_signals()`，要求没有留下用户 handler frame 时调用 `restore_temporary_sig_mask_if_pending()` 或等价 helper。[迁移实施计划](./implementation.md) 阶段 2 把该恢复点列为交付和审计项。

### SIGTEMP-002 - Nested temporary mask policy

**来源等级：** Euclid

**原问题：** 一个 task 同时只能处在一个 syscall 临时 mask 窗口内。如果已有 pending restore mask 时再次 begin，静默覆盖会丢失旧 mask。正常用户 handler 内再次调用 `ppoll` / `pselect6` / `rt_sigsuspend` 应是新的 syscall 上下文；此时上一轮 restore 状态已经被 sigframe 消费并清除。

**Neutralized：** [不变量需求](./invariants.md) 的 `TemporarySigMaskToken` 模型要求 begin 时已有 `restore` 必须 fail-closed 或触发内部 invariant，不得静默覆盖。[迁移实施计划](./implementation.md) 阶段 1 把 nested begin 审计和 assertion / fail-closed 行为列为验收项。

### SIGTEMP-003 - Restore slot lock ordering

**来源等级：** Euclid

**原问题：** 如果 restore slot 与 `sig_mask` 拆成两把锁，容易引入 `sig_mask -> sig_restore_mask` / `sig_restore_mask -> sig_mask` 双向锁序，和现有 `sig_pending -> sig_mask -> sig_disposition` 顺序交错。

**Neutralized：** [RFC 主文档](./index.md) 和 [不变量需求](./invariants.md) 将 current mask 和 restore slot 合并进同一个 `TaskSigMaskState`，由单个 `NoIrqSpinLock<TaskSigMaskState>` 保护，并拒绝“两把锁但约定固定顺序”的临时方案。

### SIGTEMP-004 - `rt_sigtimedwait` boundary check

**来源等级：** Safe

**原问题：** `rt_sigtimedwait` 主动 fetch waited signal 并在 syscall body 内返回 signal number，语义不同，不应自动并入 `sigsuspend` delayed restore。但它也存在临时改 mask 的代码路径，需要复查未等待 signal 中断路径是否仍按预期恢复。

**Neutralized：** [RFC 主文档](./index.md) 的非目标和方案明确 `rt_sigtimedwait` 不迁入 delayed restore helper。[不变量需求](./invariants.md) 的 ABI 边界规定它继续按 syscall-body-only restore 处理。[迁移实施计划](./implementation.md) 阶段 5 改为本地语义修复与边界审计，要求 signal / force wake 后先按 waited set 重新尝试 dequeue matching signal，再恢复 mask 并返回 signal number、`EINTR` 或 force 结果。

### SIGTEMP-005 - Temporary mask guard must have an explicit deferred state

**来源等级：** Keter

**原问题：** `begin_temporary_sig_mask(new_mask) -> TemporarySigMaskGuard` 如果按普通 RAII guard 设计，guard 在 syscall body 返回时 drop 就会恢复旧 mask，重新制造 `rt_sigsuspend` / `ppoll` / `pselect6` 因 signal interrupt 返回后、trap-return delivery 前过早恢复 mask 的问题。

**Neutralized：** [RFC 主文档](./index.md) 和 [不变量需求](./invariants.md) 将 helper 固定为显式终止的 `TemporarySigMaskToken` / state API。token 没有 drop-time restore 语义，且只有 `restore_now()` 与 `defer_to_signal_delivery()` 两个终止态；非 signal-delivery 返回路径必须显式 `restore_now()`，确认会进入 trap-return delivery 的 carrier 路径必须显式 `defer_to_signal_delivery()`。

### SIGTEMP-006 - Force outcome must not be treated as ordinary EINTR

**来源等级：** Euclid

**原问题：** `rt_sigsuspend` 草图曾把 `Force` 与普通 `Signal` 一并写成“不恢复旧 mask 并返回 EINTR”。wait-core 的 `Force` 主要对应 `SIGKILL` / `SIGSTOP` 这类强制唤醒，不应被泛化成可返回用户态的普通 `EINTR` 路径。

**Neutralized：** [RFC 主文档](./index.md) 的方案和 [不变量需求](./invariants.md) 的 Wait-Core 线性化点把 `Signal` / `Force` 分类提升为所有 delayed-restore callsite 的共同规则：`rt_sigsuspend`、`ppoll` 和 `pselect6` 都不能仅凭 wait-core outcome defer restore，必须把 typed candidate 交给 signal-owned `classify_temporary_mask_wait()`；否则先 `restore_now()`，再 fail-closed 或走不可返回 force 路径。[迁移实施计划](./implementation.md) 阶段 3 和阶段 4 都把该分类列为审计项。

### SIGTEMP-007 - Restore slot must be consumed only after handler frame is committed

**来源等级：** Euclid

**原问题：** `signal_frame_consumed_restore_mask()` 如果在读取 `mask_to_save` 后、用户 sigframe 写入和 trapframe 准备完成前调用，失败路径可能提前清除 restore slot。这样 frame 写失败、用户栈错误或 trapframe 准备失败时，旧 mask 的恢复责任既没有转移给 `rt_sigreturn()`，也可能不再由 trap-return cleanup 看到。

**Neutralized：** [不变量需求](./invariants.md) 的 Handler Frame Commit 规则把消费点定义为“用户 sigframe 已成功写入，且 trapframe 已准备跳转到 handler”。[迁移实施计划](./implementation.md) 阶段 2 要求 `perform_signal_action()` 只在 frame commit 点调用 `signal_frame_committed_restore_mask()`。

### SIGTEMP-008 - `cancel_without_delivery()` terminal state is ambiguous

**来源等级：** Euclid

**原问题：** RFC 曾把 `TemporarySigMaskToken::cancel_without_delivery()` 列为非 signal 返回路径的合法终止方式，但没有定义它对 `current` mask 和 `restore` slot 的精确影响。由于 `begin_temporary_sig_mask()` 已经安装临时 mask，若实现者把 cancel 理解成“只清除 restore slot，不恢复 current”，会让 task 携带临时 mask 返回用户态，且 trap-return delivery 也不再知道需要恢复旧 mask。

**Neutralized：** [RFC 主文档](./index.md) 和 [不变量需求](./invariants.md) 删除 `cancel_without_delivery()`，并把 token contract 固定为两个终止态：非 signal-delivery 返回路径只允许 `restore_now()`，恢复 `current = old_mask` 并清空 `restore`；signal-delivery carrier 返回路径只允许 `defer_to_signal_delivery()`。begin 失败必须发生在安装临时 mask 前，因此不产生 token。[迁移实施计划](./implementation.md) 阶段 1 把“不得新增第三终止态”列为 helper contract 审计项。

### SIGTEMP-009 - `Force` classification is scoped too narrowly

**来源等级：** Euclid

**原问题：** RFC 只在 `rt_sigsuspend` 口径下强调不能把 wait-core `Force` outcome 当作 ordinary `EINTR`。但 delayed restore helper 同时服务 `ppoll` / `pselect6`，如果 iomux callsite 直接把 `Signal` / `Force` outcome 归入 signal-interrupted 分支，就可能在没有证明存在可交给 trap-return delivery 的 pending signal 时 defer restore slot。

**Neutralized：** [RFC 主文档](./index.md) 和 [不变量需求](./invariants.md) 已把分类规则提升为 `rt_sigsuspend`、`ppoll` 和 `pselect6` 的共同不变量：不能仅凭 wait-core `Signal` / `Force` outcome 调用 `defer_to_signal_delivery()`；必须把 typed candidate 交给 signal-owned `classify_temporary_mask_wait()`，并只在其完成 stable delivery target reservation / handoff 且返回 `DeferToTrapReturnDelivery` 后 defer。否则先 `restore_now()`，再按 fail-closed 或不可返回 force 路径处理。[迁移实施计划](./implementation.md) 阶段 3 和阶段 4 都要求审计该规则。

### SIGTEMP-010 - `rt_sigtimedwait` boundary audit must not assume the current code is already correct

**来源等级：** Euclid

**原问题：** `rt_sigtimedwait` 不应迁入 delayed restore helper，但当前代码仍需要作为边界审计对象。现有实现会在 wait-core 返回 `CurrentWaitOutcome::Signal | CurrentWaitOutcome::Force` 时先分类为 `Interrupted`，随后恢复旧 mask 并返回 `EINTR`；它没有在该分支先尝试 `fetch_specific_signal(uthese)`。如果 waited signal 在 precheck 之后到达并唤醒等待，syscall 可能返回 `EINTR` 而不是消费该 signal 并返回 signal number。

**Neutralized：** [RFC 主文档](./index.md) 和 [不变量需求](./invariants.md) 保留 `rt_sigtimedwait` 在 delayed helper 外，但明确它需要 syscall-body 本地语义修复：wait 被 signal / force 唤醒后，先按 waited set 重新尝试 dequeue matching signal；若拿到 waited signal，恢复 mask 并返回 signal number；若没有 matching waited signal，恢复 mask 后返回 `EINTR` 或处理 force。[迁移实施计划](./implementation.md) 阶段 5 已改为“本地语义修复与边界审计”，write set 限定在 `rt_sigtimedwait.rs`，若需要 trap-return delivery / delayed helper 则回到 RFC 修改 Scope。
