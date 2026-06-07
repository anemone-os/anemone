# 2026-06-06 - Signal Temporary Mask Restore

**Status:** Active
**Owners:** doruche, Codex
**Area:** signal / wait-core / syscall ABI / iomux
**RFC:** [RFC-20260606-signal-temp-mask-restore](../../rfcs/signal-temp-mask-restore/index.md)
**Current Phase:** Gate 7 passed; Agent 8 pending

## Scope

本事务跟踪 `rt_sigsuspend`、`ppoll` 和 `pselect6` 的 temporary signal mask delayed
restore 协议实现，并审计 `rt_sigtimedwait` 继续保持 syscall-body-only waited-set
语义。

实现按 RFC gate 推进：

- 阶段 0：UAPI、current code landing point 和 legacy temporary-mask callsite 前置审计。
- 阶段 1A：`TaskSigMaskState` storage 与 ordinary current-mask API。
- 阶段 1B：`TemporarySigMaskToken` 与 helper contract。
- 阶段 2：trap-return signal delivery commit / cleanup 接入。
- 阶段 3：signal-owned classifier / stable delivery handoff 和 `rt_sigsuspend` syscall。
- 阶段 4：`ppoll` / `pselect6` typed outcome 与 shared helper 迁移。
- 阶段 5：`rt_sigtimedwait` 本地 waited-set dequeue 边界修复与 helper 外审计。
- 阶段 6：旁路审计、构建 gate、smoke / LTP 证据整理和收口。

非目标：

- 不引入多层 temporary mask restore stack。
- 不改变 `rt_sigprocmask` 的永久 mask 语义。
- 不把 `rt_sigtimedwait` 迁入 delayed restore helper。
- 不引入完整 Linux restart errno / `restart_syscall` 体系。
- 不把 cleanup 语义下沉到 arch-specific trap-return 层。
- 不运行 QEMU / LTP，除非用户后续明确授权；运行态证据默认由用户提供。

## Invariants

- `TaskSigMaskState { current, restore }` 是 current mask 和 restore slot 的单一真相源。
- `TemporarySigMaskToken` 必须是 must-use、线性、不可复制 token；terminal method 只有
  `restore_now(self)` 和 `defer_to_signal_delivery(self)`。
- token drop 没有恢复语义，不能清空 restore slot 或选择 defer。
- handler frame commit 前不得消费 restore slot；sigframe 保存的 mask 必须是进入
  temporary window 前的旧 mask。
- 无 handler frame 的 default ignore / explicit ignore 返回用户态路径必须由 signal 模块统一
  cleanup 恢复旧 mask。
- `rt_sigsuspend`、`ppoll` 和 `pselect6` 不得仅凭 wait-core `Signal` / `Force` outcome
  defer restore；必须通过 signal-owned classifier，并在 `DeferToTrapReturnDelivery`
  返回前完成 stable delivery target reservation / handoff。
- `ppoll` / `pselect6` 不保留独立早恢复模型。
- `rt_sigtimedwait` 不使用 delayed helper，wait 被 signal / force 唤醒后先按 waited set
  重新尝试 dequeue matching signal。
- 每个 worker 只能写入编排文档指定 write set；需要扩大 write set 时必须停止上报，并在
  本事务记录批准结果。

## Handoff

**Last Updated:** 2026-06-07

**Current Branch:** `dev/drc/signal-temp-mask`

**Canonical RFC:** [RFC-20260606-signal-temp-mask-restore](../../rfcs/signal-temp-mask-restore/index.md), [Invariants](../../rfcs/signal-temp-mask-restore/invariants.md), [Implementation Plan](../../rfcs/signal-temp-mask-restore/implementation.md), [Tracking Issues](../../rfcs/signal-temp-mask-restore/tracking-issues.md), [Agent Orchestration](../../rfcs/signal-temp-mask-restore/backgrounds/agent-orchestration.md)

**Completed:** 公开 RFC、invariants、implementation、tracking issues 和 agent orchestration
文档已存在。总控完成实现前第一轮只读刷新：当前分支为 `dev/drc/signal-temp-mask`，工作区在事务启动前干净；代码仍是旧 `Task.sig_mask: NoIrqSpinLock<SigSet>` 模型；`rt_sigsuspend` 尚未注册；`ppoll` / `pselect6` 仍保留 legacy save / set / wait / restore 路径；`rt_sigtimedwait` 仍有 RFC 点名的 signal / force wake 后 waited-set dequeue 边界。Agent 0 只读前置审计已完成，未发现 RFC blocker 或停止条件，允许进入 Agent 1 阶段 1A。Agent 1 阶段 1A 已完成 `TaskSigMaskState` storage 与 ordinary current-mask API 迁移；Gate 1 review 已通过。
Agent 2 阶段 1B 已完成 `TemporarySigMaskToken` 与 helper contract；Gate 2 review 已通过。
Agent 3 阶段 2 已完成 signal delivery commit / cleanup 接入；Gate 3 review 已通过。
Agent 4 已完成 signal-owned classifier / stable handoff；Gate 4 review 已通过。
Agent 5 已完成 `rt_sigsuspend` syscall；Gate 5 review 已通过。
Agent 6 已完成 `ppoll` / `pselect6` typed outcome 与 shared helper 迁移；Gate 6 review 已通过。
Agent 7 已完成 `rt_sigtimedwait` 本地 waited-set dequeue 边界修复；Gate 7 review 已通过。

**In Progress:** 无。Agent 8 尚未启动。

**Open Blockers:** 暂无。

**Next Action:** 可以启动 Agent 8 旁路审计、验证证据整理和事务收口。

**Do Not Redo:** 不要一次性启动所有 worker；不要在 Stage 1A 迁移 `ppoll` / `pselect6` delayed restore；不要把 `rt_sigtimedwait` 放进 `TemporarySigMaskToken` helper；不要用 wait-core `Signal` / `Force` outcome 直接证明 defer；不要把 cleanup 语义复制到 riscv64 / loongarch64 trap-return 层；不要 revert 用户或其他 agent 的改动。

## Phase Log

### 2026-06-06 - 事务日志启动与总控前置检查

**Phase:** orchestration / pre-audit

**Change:** 建立本事务日志，并把 [RFC-20260606-signal-temp-mask-restore](../../rfcs/signal-temp-mask-restore/index.md)、Tracking Issues、事务索引、mdBook Summary 和当前双周 devlog 连接到同一条实现记录。

**Review:** 总控只读刷新当前落点：当前分支为 `dev/drc/signal-temp-mask`；事务启动前工作区干净；未发现多层 temporary mask restore stack、`rt_sigtimedwait` delayed-helper 迁移、arch-specific cleanup 下沉，或 `ppoll` / `pselect6` 在 classifier 缺失时 defer restore。当前实现仍符合 RFC 假设：`Task` 直接持有 `sig_mask`，`perform_signal_action()` 仍用当前 mask 写 sigframe 并安装 handler mask，`handle_signals()` 尚无 no-handler-frame temporary restore cleanup，iomux helper 仍提前把 `Signal` / `Force` 映射为 `SysError::Interrupted`。

**Validation:** `git diff --check` 通过；`mdbook build docs` 通过。未运行 kernel 构建、QEMU 或 LTP。

**Next:** 只启动 Agent 0 做只读前置审计；不得启动 Agent 1 或后续写入型 worker。

### 2026-06-06 - Agent 0 只读前置审计启动

**Phase:** Agent 0 / pre-audit

**Change:** Agent 0 已启动，职责限定为只读审计当前 signal / iomux / syscall ABI 落点，不改文件，不运行构建、QEMU 或 LTP。

**Review:** Agent 0 需要按 [Agent 编排建议](../../rfcs/signal-temp-mask-restore/backgrounds/agent-orchestration.md) 输出是否允许进入 Agent 1 阶段 1A、当前代码路径到阶段 1A / 1B / 2 / 3 / 4 / 5 的对应表、停止条件检查结论，以及事务 devlog 字段状态。

**Validation:** 未运行。

**Next:** 等待 Agent 0 结论。Gate 0 未通过前，不启动 Agent 1。

### 2026-06-06 - Agent 0 只读前置审计通过

**Phase:** Gate 0 / pre-audit review

**Review:** Agent 0 未发现需要回到 RFC review 的 blocker，允许进入 Agent 1 阶段 1A。当前代码仍符合 RFC 前置假设：旧 `sig_mask` 模型仍在，delayed restore helper 尚未实现，`rt_sigsuspend` 尚未接通，`ppoll` / `pselect6` 仍是阶段 4 债务。

**Review:** Agent 0 确认阶段 1A 落点包括 `Task` 的 `sig_mask: NoIrqSpinLock<SigSet>` storage、`Task::sig_mask()` / `Task::set_sig_mask()`、`rt_sigprocmask`、`rt_sigreturn`、clone 继承和 procfs status snapshot；阶段 1B helper/token 尚不存在；阶段 2 delivery 仍由 `perform_signal_action()` 直接读取当前 mask 并安装 handler mask，`handle_signals()` 尚无 no-handler-frame cleanup；阶段 3 `SYS_RT_SIGSUSPEND = 133` 尚未注册；阶段 4 iomux 仍有 legacy temporary mask path，`wait_for_iomux_ready()` 仍把 `Signal` / `Force` 映射为 `SysError::Interrupted`；阶段 5 `rt_sigtimedwait` 的 `Signal` / `Force` 分支仍未先重新尝试 waited-set dequeue。

**Stop Conditions:** 未触发。未发现多层 temporary mask restore stack，未发现 `rt_sigtimedwait` 被迁入 delayed helper，未发现 cleanup 语义下沉到 riscv64 / loongarch64 trap-return 层，也未发现 `ppoll` / `pselect6` 在无 classifier 情况下 defer restore。

**Validation:** Agent 0 只读审计；未修改文件，未运行构建、QEMU 或 LTP。

**Next:** 启动 Agent 1 阶段 1A。Agent 1 write set 限定为 RFC 编排文档的阶段 1A 范围和本事务日志；不得启动 Agent 2 或后续 worker。

### 2026-06-06 - Agent 1 阶段 1A 完成

**Phase:** Agent 1 / Stage 1A

**Change:** `Task.sig_mask` 从 `NoIrqSpinLock<SigSet>` 升级为单一 `NoIrqSpinLock<TaskSigMaskState>`，状态包含 `current: SigSet` 和预留的 `restore: Option<SigSet>`。新增 ordinary current-mask API：`snapshot_current_sig_mask()`、`set_permanent_sig_mask()`、`mutate_current_sig_mask()`、`restore_sigframe_current_sig_mask()`、`mutate_syscall_body_current_sig_mask()` 和 `restore_syscall_body_current_sig_mask()`。本阶段没有实现 `TemporarySigMaskToken`、begin/defer/restore helper、sigframe delayed restore commit、classifier、`rt_sigsuspend` 或 arch trap-return cleanup。

**Change:** `rt_sigprocmask` 的 oldset snapshot 和永久 mask 修改改走 current-mask API；`rt_sigreturn` 只通过 sigframe current-mask restore API 写回 frame 中的 mask，不读取、不消费、不覆盖 restore slot；`rt_sigtimedwait` 继续保留 syscall-body-only 临时 unmask / restore，但改走命名 API；clone / fork 只继承 parent 的 `current` snapshot，不复制 pending restore slot；procfs status 只 snapshot `current`。

**Audit:** `rg -n "sig_mask|set_sig_mask|TaskSigMaskState" anemone-kernel` 的 residual 已分类：`TaskSigMaskState` owner、`Task` storage 初始化、signal owner 内部 current snapshot / mutation、clone/procfs/rt_sigprocmask/rt_sigreturn/rt_sigtimedwait 的命名 API 调用，以及 `ppoll.rs` / `pselect6.rs` 的 legacy save / set / restore path。`ppoll.rs` 与 `pselect6.rs` 仍通过兼容 wrapper `sig_mask()` / `set_sig_mask()` 暂留，明确登记为阶段 4 必须删除或替换的 debt；本阶段未把它们迁移成 delayed restore，也未把它们算作完成。

**Validation:** `just build` 通过；仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。未运行 QEMU / LTP。

**Next:** Gate 1 review。不得跳过 review 直接启动 Agent 2。

### 2026-06-06 - Gate 1 reviewer Keter 修复

**Phase:** Gate 1 / Stage 1A review fix

**Review:** Gate 1 readonly reviewer 发现一个 Keter：`set_permanent_sig_mask()` 作为 Stage 1A 新 owner API 边界，原先只用 `debug_assert!` 检查 `SIGKILL` / `SIGSTOP` 不可屏蔽，release 路径会静默接受非法 current mask。

**Change:** 将 current-mask validity check 下沉到 `TaskSigMaskState` owner 内部，并使用普通 `assert!`。`set_permanent_current()` 在写入前检查新 mask，`mutate_current()` 在闭包修改后检查 postcondition，覆盖永久 set、ordinary mutation、sigframe restore 和 syscall-body-only restore 路径。

**Validation:** `git diff --check` 通过；`just build` 通过，仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。未运行 QEMU / LTP。

**Next:** 重新执行 Gate 1 readonly review。

### 2026-06-06 - Gate 1 review 通过

**Phase:** Gate 1 / Stage 1A review

**Review:** Gate 1 第二轮 readonly reviewer 未发现 Apollyon / Keter / Euclid blocker。确认上轮 Keter 已修：`TaskSigMaskState::set_permanent_current()` 与 `TaskSigMaskState::mutate_current()` 使用普通 `assert!` 覆盖 owner 写入和 mutation postcondition，防止 `SIGKILL` / `SIGSTOP` 进入 current mask。`perform_signal_action()` 内保留的 debug-only check 不是 owner 防线，不阻塞 Gate 1。

**Review:** reviewer 确认 current mask 与 restore slot 是单锁单真相源；ordinary API 能区分 snapshot、permanent mutation、sigframe restore、`rt_sigtimedwait` syscall-body-only temporary mutation / restore；`rt_sigprocmask`、`rt_sigreturn`、clone / fork 和 procfs status 均走命名 API，未复制或暴露 restore slot。

**Review:** reviewer 确认没有误实现 Stage 1B+ 内容：未发现 `TemporarySigMaskToken`、begin / defer / restore helper、signal-frame commit / cleanup、classifier、`rt_sigsuspend`、iomux typed outcome 或 `rt_sigtimedwait` waited-set fix。`ppoll` / `pselect6` legacy save / set / restore 只作为 Stage 4 debt 残留，并已登记。

**Validation:** reviewer 只读运行 `git diff --check` 通过；主控在修复后运行 `git diff --check` 和 `just build` 均通过，build 仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。未运行 QEMU / LTP。

**Next:** 可以启动 Agent 2 阶段 1B；不得启动 Agent 3 或后续 worker。

### 2026-06-06 - Agent 2 阶段 1B 完成

**Phase:** Agent 2 / Stage 1B

**Change:** 在 `TaskSigMaskState` 内将 Stage 1A 预留的 `restore` slot 纳入 helper contract，并新增私有 `TemporarySigMaskSlotId` identity。`begin_temporary_sig_mask(new_mask)` 在安装 `new_mask` 前断言没有既有 pending restore、校验 mask 合法性、保存旧 mask 到单一 restore slot，并返回 `#[must_use]` 的线性 `TemporarySigMaskToken`。token 持有 owning `Task` 引用与 slot identity，不实现 `Clone` / `Copy`；`restore_now(self)` 恢复旧 mask 并清空 restore slot，`defer_to_signal_delivery(self)` 只校验当前 task 与 pending slot 仍匹配并把恢复责任留给后续 signal delivery。`Drop` 只记录 / assert active token leak，不恢复 mask、不清空 slot、也不选择 defer。

**Change:** 新增 helper contract surface：`sigmask_to_save_for_signal_frame()`、`signal_frame_committed_restore_mask()` 和 `restore_temporary_sig_mask_if_pending()`。本阶段只提供 helper，不在 `perform_signal_action()` 中调用 `sigmask_to_save_for_signal_frame()`，不新增 handler frame commit 点，不修改 `handle_signals()` cleanup，不接入 classifier、`rt_sigsuspend`、`ppoll` / `pselect6` typed outcome，且不修改 `rt_sigtimedwait`。

**Audit:** 普通 current-mask mutation 现在由 `TaskSigMaskState` owner 在 `restore != None` 时 fail-closed assert，避免 `rt_sigprocmask`、`rt_sigreturn`、syscall-body-only restore 或 Stage 4 legacy wrapper 静默覆盖 pending restore。唯一允许 pending restore window 内修改 current 的 API 是 `mutate_current_sig_mask_for_signal_delivery()`，并且 `perform_signal_action()` 的 handler-mask installation 只改为走这个命名 helper；本阶段没有改变 sigframe mask 来源、frame commit 或 delayed restore cleanup 行为。helper 的所有状态转换只在 mask-state lock 临界区内读写 `TaskSigMaskState`，没有在持锁状态下执行 user copy 或 schedule。

**Residual:** `rg` residual 中 `ppoll.rs` / `pselect6.rs` 仍是 Stage 4 legacy save / set / restore debt；`rt_sigtimedwait.rs` 仍保持 helper 外 syscall-body-only temporary mask path，Stage 5 再修 waited-set dequeue 边界；`perform_signal_action()` / `handle_signals()` 的 delayed restore delivery 接入仍是 Stage 2；未发现 `classify_temporary_mask_wait()` 或 `rt_sigsuspend` 新实现。

**Validation:** `git diff --check` 通过；`just build` 通过，仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。未运行 QEMU / LTP。

**Next:** Gate 2 review。不得跳过 review 直接启动 Agent 3。

### 2026-06-07 - Gate 2 review 通过

**Phase:** Gate 2 / Stage 1B review

**Review:** Gate 2 reviewer 未发现 Apollyon / Keter / Euclid blocker。确认 `TemporarySigMaskToken` 是 `#[must_use]`、非 `Clone` / 非 `Copy` 的线性 token，`restore_now(self)` 与 `defer_to_signal_delivery(self)` 消耗 `self`，`Drop` 只记录并普通 `assert!` active-token leak，不恢复 mask、不清空 slot、不选择 defer。最初关于 drop assert 可能引入 unwind double-panic 的 finding 已撤回：当前内核没有 unwind，该 assert 只作为 fail-closed invariant 暴露。

**Review:** reviewer 确认 token 不再保存独立 `task_id` 字段，owner 校验使用 `Arc::ptr_eq(&current, &self.task)`，日志和 assertion 需要 task id 时直接读取 `self.task.tid()`。`TemporarySigMaskSlotId` 只作为 restore slot identity 元数据，不是第二个 mask 来源；begin 时已有 restore 会在安装新 mask 前普通 `assert!` fail-closed。

**Review:** ordinary current-mask mutation 在 `restore != None` 时普通 `assert!`，唯一命名允许在 pending restore window 内修改 current 的路径是 `mutate_current_sig_mask_for_signal_delivery()`。本阶段没有接入 Stage 2+：`perform_signal_action()` 尚未调用 `sigmask_to_save_for_signal_frame()`，`handle_signals()` 尚未接入 cleanup，未实现 classifier、`rt_sigsuspend`、iomux typed outcome 或 `rt_sigtimedwait` waited-set fix。

**Validation:** `git diff --check` 通过；`just build` 通过，仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。未运行 QEMU / LTP。

**Next:** 可以启动 Agent 3 阶段 2 signal delivery 接入。当前先停止，不启动 Agent 3 或后续 worker。

### 2026-06-07 - Agent 3 阶段 2 完成

**Phase:** Agent 3 / Stage 2

**Change:** `perform_signal_action()` 的用户 handler 路径现在先用 `sigmask_to_save_for_signal_frame()` 选择写入 sigframe 的 mask，再通过 `mutate_current_sig_mask_for_signal_delivery()` 安装 handler `sa_mask` / self-mask。sigframe 写入和 arch trapframe 准备完成、可选 `sa_restorer` 返回地址写入完成后，才在 frame commit 点调用 `signal_frame_committed_restore_mask()`，把 pending restore 责任转移给后续 `rt_sigreturn()`。

**Change:** `handle_signals()` 记录本轮 trap-return 是否已提交用户 handler frame；如果没有留下 handler frame，则在 signal 模块内部统一调用 `restore_temporary_sig_mask_if_pending()`。default terminate 路径保持不返回；default ignore / explicit ignore 消费 signal 后由这个统一 cleanup 恢复旧 mask。

**Audit:** `rt_sigreturn` 现有代码已经只从用户 sigframe 的 `uc_sigmask` 恢复 `TaskSigMaskState.current`，不读取、不消费、也不清理 pending restore slot，因此本阶段未修改 `anemone-kernel/src/task/sig/api/rt_sigreturn.rs`。frame 写失败路径仍直接 `kernel_exit_group(SIGSEGV)`，不会伪造返回用户态 cleanup；当前 arch `prepare_trapframe_for_signal_handler()` 不返回错误，cleanup 语义未下沉到 riscv64 / loongarch64 trap-return 层。

**Scope:** 未启动 signal-owned classifier / stable handoff，未注册或实现 `rt_sigsuspend`，未修改 `ppoll` / `pselect6` / iomux typed outcome，未修改 `rt_sigtimedwait`。

**Validation:** `git diff --check` 通过；`just build` 通过，仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。未运行 QEMU / LTP。

**Next:** Gate 3 review。不得跳过 review 直接启动 Agent 4 classifier、Agent 5 `rt_sigsuspend`、Agent 6 iomux 或 Agent 7 `rt_sigtimedwait`。

### 2026-06-07 - Gate 3 review 通过

**Phase:** Gate 3 / Stage 2 review

**Review:** Gate 3 readonly reviewer 未发现 Apollyon / Keter / Euclid / Safe finding，确认阶段 2 可以通过。reviewer 核对 `perform_signal_action()` 的用户 handler 路径：先在安装 handler mask 前选择 `mask_to_save`，再写入 sigframe、准备 trapframe、应用可选 restorer，并且只在 commit 点消费 restore slot。

**Review:** reviewer 确认 `handle_signals()` 只在没有提交用户 handler frame 时调用 `restore_temporary_sig_mask_if_pending()`，覆盖 default ignore / explicit ignore 返回用户态路径；default terminate 保持不返回。`rt_sigreturn` 仍只通过 sigframe `uc_sigmask` 恢复 `TaskSigMaskState.current`，不读取、不消费、不清理 pending restore slot。

**Review:** cleanup 语义仍收口在 signal 模块内部，riscv64 / loongarch64 trap-return 层没有复制 restore cleanup。reviewer 未发现 classifier、`rt_sigsuspend`、iomux typed outcome 或 `rt_sigtimedwait` helper migration 越界实现。

**Validation:** reviewer 运行只读搜索与 `git diff --check` 通过；主控在 Stage 2 完成后运行 `git diff --check` 与 `just build` 通过，build 仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。未运行 QEMU / LTP。

**Next:** 可以启动 Agent 4 signal-owned classifier / stable handoff。不得跳过 Gate 4 review 直接启动 Agent 5 `rt_sigsuspend`、Agent 6 iomux 或 Agent 7 `rt_sigtimedwait`。

### 2026-06-07 - Agent 4 classifier / stable handoff 完成

**Phase:** Agent 4 / signal-owned classifier and handoff

**Change:** 在 `anemone-kernel/src/task/sig/mod.rs` 新增 signal-owned delayed-restore 分类 API：`TemporaryMaskWaitCandidate`、`TemporaryMaskWaitContext`、`TemporaryMaskWaitDecision` 和 `Task::classify_temporary_mask_wait()`。后续 `rt_sigsuspend`、`ppoll` 和 `pselect6` callsite 只需要传入 typed wait outcome 与 syscall context；pending queue、disposition、ignore/default/custom action 和 force wake policy 都仍由 signal 模块内部解释。

**Change:** `PendingSignals` 增加 task-private `reserved_delivery` handoff slot，并让 `Task::fetch_signal()` 通过既有 private-pending 优先路径先消费 reserved target。private pending target 会从普通 pending 队列移动到该 slot；shared thread-group pending target 会先从 shared queue claim 出来，再移动到当前 task 的 private reservation，因此不会留下“先观察、后竞争”的 shared pending 窗口。当前实际模型已经存在 shared thread-group pending queue，本阶段没有虚构新的 shared-pending 机制。

**Semantics:** `DeferToTrapReturnDelivery` 只在成功建立当前 task 的 stable reserved delivery target 后返回。无法 reserve / handoff 的 `Signal` candidate 返回 `RestoreThenFailClosed(SysError::IO)`；`Cancelled` / `Unexpected` 也 fail-closed。`Force` candidate 只按 `SIGKILL` / `SIGSTOP` 这类 force-wake target claim，并返回 `NoReturnForce`，不会被降级成 ordinary `EINTR` defer proof。普通 `Signal` candidate 若实际 claim 到 `SIGKILL` / `SIGSTOP`，同样返回 `NoReturnForce`。

**Audit:** reserved target 被 `handle_signals()` 的真实 `fetch_signal()` 路径优先消费；custom handler 继续走 Agent 3 的 frame commit，default / explicit ignore no-handler-frame 路径继续由 signal 模块统一 cleanup 恢复 temporary mask。classifier 不注册或实现 `rt_sigsuspend`，不修改 `ppoll` / `pselect6` / iomux typed outcome，不修改 `rt_sigtimedwait`，也不改 arch trap-return cleanup 或 scheduler/wait-core。

**Validation:** `git diff --check` 通过；`just build` 通过，仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。未运行 QEMU / LTP。

**Next:** Gate 4 review。不得跳过 Gate 4 review 直接启动 Agent 5 `rt_sigsuspend`、Agent 6 iomux 或 Agent 7 `rt_sigtimedwait`。

### 2026-06-07 - Gate 4 review 通过

**Phase:** Gate 4 / signal-owned classifier review

**Review:** Gate 4 readonly reviewer 未发现 Apollyon / Keter / Euclid / Safe finding，确认阶段 3 classifier / stable handoff 可以通过。reviewer 确认 `classify_temporary_mask_wait()` 是 signal-owned API，只接收 typed wait outcome 与 syscall context，`DeferToTrapReturnDelivery` 只在建立 stable reserved target 后返回。

**Review:** reviewer 确认 private pending signal 会移动到 `reserved_delivery`，shared thread-group pending signal 会先从 shared queue claim，再移动到当前 task reservation，避免 shared pending “先观察、后竞争”窗口。`fetch_signal()` 通过真实 trap-return delivery 路径优先消费 reservation。

**Review:** reviewer 确认 `Force` 没有被降级为 ordinary `EINTR` proof；force target 返回 `NoReturnForce`，reserve / handoff 失败路径 fail-closed。Stage 2 no-handler cleanup 仍通过 `handle_signals()` 收口，未发现 `rt_sigsuspend`、iomux typed outcome、`rt_sigtimedwait` 或 arch cleanup 越界实现。

**Validation:** reviewer 运行 `git diff --check` 与 `just build` 通过，build 仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。未运行 QEMU / LTP。

**Next:** 可以启动 Agent 5 `rt_sigsuspend` syscall。不得跳过 Gate 5 review 直接启动 Agent 6 iomux 或 Agent 7 `rt_sigtimedwait`。

### 2026-06-07 - Agent 5 rt_sigsuspend syscall 完成

**Phase:** Agent 5 / Stage 3 `rt_sigsuspend`

**Change:** 在 riscv64 / loongarch64 ABI 常量表注册 `SYS_RT_SIGSUSPEND = 133`，新增
`anemone-kernel/src/task/sig/api/rt_sigsuspend.rs` 并通过 signal API module 导出。syscall
handler 使用现有 `#[syscall(SYS_RT_SIGSUSPEND)]` 机制进入自动 syscall handler table，没有
新增手写 dispatch 表。未修改 `anemone-rs` wrapper，因为本阶段不需要 smoke ergonomics
包装。

**Semantics:** `sigsetsize` 校验和用户 mask copy-in 都在安装 temporary mask 前完成，bad
size / bad pointer 不产生 token。copy-in 后清除 `SIGKILL` / `SIGSTOP`，随后调用
`begin_temporary_sig_mask()` 安装临时 mask。wait-core precheck 只使用
`has_unmasked_signal()` 作为 wait precheck；wait outcome 统一交给 signal-owned
`classify_temporary_mask_wait(..., RtSigsuspend)`，syscall body 不主动 `fetch_signal()`，也不复制
pending queue、disposition、ignore/default/custom action 或 force wake policy。

**Semantics:** 只有 classifier 返回 `DeferToTrapReturnDelivery` 时才调用
`token.defer_to_signal_delivery()` 并返回 `EINTR` carrier。restore / fail-closed / ordinary
error path 都先 `token.restore_now()`，再映射 errno。`NoReturnForce` 不被当成 ordinary
`EINTR`；当前没有 syscall-side no-return force helper，因此该路径先终止 token，再记录
fail-closed 日志并返回 `EIO`，保留与 ordinary `EINTR` carrier 的边界，trap-return 仍应消费
classifier 已保留的 force target。

**Scope:** 未启动 `ppoll` / `pselect6` / iomux typed outcome 迁移，未修改
`rt_sigtimedwait`，未修改 arch trap-return cleanup 或 scheduler / wait-core 语义。

**Validation:** `git diff --check` 通过；`just build` 通过，仅有既有
`anemone-kernel/src/sync/mono.rs` unused import warning。未运行 QEMU / LTP。

**Next:** Gate 5 review。不得跳过 review 直接启动 Agent 6 iomux 或 Agent 7
`rt_sigtimedwait`。

### 2026-06-07 - Gate 5 review 通过

**Phase:** Gate 5 / `rt_sigsuspend` review

**Review:** Gate 5 readonly reviewer 未发现 Apollyon / Keter / Euclid / Safe finding，确认 `rt_sigsuspend` 可以通过。reviewer 确认 `SYS_RT_SIGSUSPEND = 133` 已加入 riscv64 / loongarch64 ABI，`#[syscall(SYS_RT_SIGSUSPEND)]` 进入现有 `.syscall` 自动注册表。

**Review:** reviewer 确认 `sigsetsize` 校验、用户 mask copy-in、`SIGKILL` / `SIGSTOP` 清除都发生在 `begin_temporary_sig_mask()` 前；temporary token 安装后才进入 `wait_current_with_timeout()`。precheck 只使用 `has_unmasked_signal()`，defer proof 交给 `classify_temporary_mask_wait(..., RtSigsuspend)`。

**Review:** reviewer 确认 syscall body 不主动 `fetch_signal()`，不复制 pending queue、disposition 或 force policy。所有 token path 都 exactly one terminal：`DeferToTrapReturnDelivery` 后 `defer_to_signal_delivery()` 并返回 `EINTR` carrier；restore / fail-closed / `NoReturnForce` 先 `restore_now()` 再映射结果。`NoReturnForce` 没有降级为 ordinary `EINTR`；当前 fail-closed `EIO` 路径先终止 token，reserved force target 仍由 trap-return `fetch_signal()` 优先消费。

**Review:** 未发现 `ppoll` / `pselect6` / iomux、`rt_sigtimedwait`、arch cleanup 或 scheduler / wait-core 越界修改。`rt_sigsuspend` smoke、LTP `rt_sigsuspend01` / `sigsuspend01` 仍是后续运行态验证风险。

**Validation:** reviewer 和主控均运行 `git diff --check` 与 `just build` 通过，build 仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。未运行 QEMU / LTP。

**Next:** 可以启动 Agent 6 `ppoll` / `pselect6` typed outcome 与 shared helper 迁移。不得跳过 Gate 6 review 直接启动 Agent 7 `rt_sigtimedwait`。

### 2026-06-07 - Agent 6 ppoll / pselect6 迁移完成

**Phase:** Agent 6 / Stage 4 iomux temporary mask migration

**Change:** `wait_for_iomux_ready()` 现在返回 typed `IomuxWaitOutcome`，区分 ready、timeout、syscall/register error、signal candidate 和 force，不再在 shared iomux wait helper 内把 latch `Signal` / `Force` 直接压成 `SysError::Interrupted`。register abort 仍保留既有 final snapshot scan，以免 concurrent ready fd 被 unsupported/register error 路径吞掉。

**Change:** 新增 `finish_temporary_iomux_wait()` 作为 `ppoll` / `pselect6` 的 token-active completion 边界：ready、timeout、wait/register error 都先 `restore_now()` 再返回；signal candidate / force 交给 signal-owned `classify_temporary_mask_wait()`，只有 `DeferToTrapReturnDelivery` 才 `defer_to_signal_delivery()` 并返回 `EINTR` carrier。`NoReturnForce` 保持与 ordinary `EINTR` 分离，先终止 token 后 fail-closed 返回 `EIO`，预留给 trap-return 消费已保留的 force target。

**Change:** `sys_ppoll` 和 `sys_pselect6` 的 sigmask path 改为 `begin_temporary_sig_mask()` / typed wait outcome / shared completion helper，删除阶段 1A 登记的 `prev = task.sig_mask(); set_sig_mask(); wait; set_sig_mask(prev)` legacy save / set / restore path。no-sigmask path 不创建 token，继续把 typed outcome 按普通 syscall 结果映射。copy-out 更新 fd revents / fdsets 发生在 token terminal 之后，因此 copy-out error 不会携带 active temporary token 返回。

**Scope:** 未修改 `rt_sigtimedwait`、arch trap-return 层、scheduler / wait-core 语义、ABI syscall number 表或 unrelated docs/code；未扩大 write set。

**Validation:** `git diff --check` 通过；`just build` 通过，仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。按 Agent 6 要求未运行 QEMU / LTP。

**Next:** Gate 6 review。不得跳过 review 直接启动 Agent 7 `rt_sigtimedwait` 或 Agent 8 收口。

### 2026-06-07 - Gate 6 review 通过

**Phase:** Gate 6 / iomux temporary mask migration review

**Review:** Gate 6 readonly reviewer 未发现 Apollyon / Keter / Euclid / Safe finding，确认阶段 4 可以通过。reviewer 确认 `wait_for_iomux_ready()` 返回 typed `IomuxWaitOutcome`，`Signal` / `Force` 保持 typed candidate 直到 temporary-mask completion 边界。

**Review:** reviewer 确认 `ppoll` / `pselect6` 的 no-sigmask path 不创建 temporary token；token-active path 中 ready、timeout、wait/register error、signal candidate 和 force 都通过 `restore_now()` 或 `defer_to_signal_delivery()` exactly one terminal 收口。copy-out 发生在 token terminal 之后，不会携带 active token 返回用户态。

**Review:** reviewer 确认 fs/iomux 只调用 signal-owned `classify_temporary_mask_wait()`，没有复制 pending queue、disposition、ignore/default/custom action 或 force policy。未发现 `rt_sigtimedwait`、arch trap-return、scheduler / wait-core 或 ABI syscall number 表越界修改。

**Validation:** reviewer 与主控均运行 `git diff --check` 与 `just build` 通过，build 仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。未运行 QEMU / LTP。

**Next:** 可以启动 Agent 7 `rt_sigtimedwait` 本地 waited-set dequeue 边界修复。不得跳过 Gate 7 review 直接启动 Agent 8 收口。

### 2026-06-07 - Agent 7 rt_sigtimedwait 边界修复完成

**Phase:** Agent 7 / Stage 5 `rt_sigtimedwait`

**Change:** `sys_rt_sigtimedwait` 的 `CurrentWaitOutcome::Signal | CurrentWaitOutcome::Force`
分支现在先按 waited set 调用 `fetch_specific_signal(uthese)` 重新尝试 dequeue matching
signal。若拿到 waited signal，沿既有 syscall-body restore 路径恢复旧 mask 后返回 signal
number；若没有 matching waited signal，恢复旧 mask 后继续返回 `EINTR`。

**Semantics:** `rt_sigtimedwait` 仍不使用 delayed restore helper，也不调用
`begin_temporary_sig_mask()`。临时 unmask / restore 保持在 syscall body 内，通过
`mutate_syscall_body_current_sig_mask()` 和 `restore_syscall_body_current_sig_mask()` 闭合。
未等待 signal interrupted path 不把恢复责任交给 `rt_sigreturn()`；所有用户可见返回仍经过
统一 restore 点之后再写 siginfo、返回 signal number、`EINTR` 或 `EAGAIN`。

**Scope:** 未修改 `ppoll`、`pselect6`、iomux helper、signal delayed helper、
`TemporarySigMaskToken`、arch trap-return 层、scheduler / wait-core、ABI syscall number 表或
测试 profile；未触发 write-set 扩展或 Scope 变更停止条件。

**Validation:** `git diff --check` 通过；`just build` 通过，仅有既有
`anemone-kernel/src/sync/mono.rs` unused import warning。按 Agent 7 要求未运行 QEMU / LTP。

**Next:** Gate 7 review。不得跳过 Gate 7 review 直接启动 Agent 8 收口。

### 2026-06-07 - Gate 7 review 通过

**Phase:** Gate 7 / `rt_sigtimedwait` boundary review

**Review:** Gate 7 readonly reviewer 未发现 Apollyon / Keter / Euclid / Safe finding，确认阶段 5 可以通过。reviewer 确认 `CurrentWaitOutcome::Signal | CurrentWaitOutcome::Force` 分支现在先重试 `fetch_specific_signal(uthese)`，再进入恢复旧 mask 后返回 signal number 或 `EINTR` 的本地 syscall-body 路径。

**Review:** reviewer 确认 `rt_sigtimedwait` 没有使用 `begin_temporary_sig_mask()`、`TemporarySigMaskToken`、delayed cleanup 或 classifier helper；`restore_syscall_body_current_sig_mask(prev_mask)` 仍在 siginfo copy-out 和所有用户可见返回前执行，未等待 signal interrupted path 没有把恢复责任交给 `rt_sigreturn()`。

**Review:** reviewer 确认 Agent 7 scope 只包含 `rt_sigtimedwait.rs` 和事务 devlog。工作区中存在 unrelated `anemone-apps/user-test/ltp/profile.txt` 与 `anemone-apps/user-test/ltp/groups/signal.txt` 修改，但它们未参与 Agent 7 语义，未纳入本阶段提交。

**Validation:** reviewer 与主控均运行 Agent 7 范围 `git diff --check` 与 `just build` 通过，build 仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。未运行 QEMU / LTP。

**Next:** 可以启动 Agent 8 旁路审计、验证证据整理和事务收口。
