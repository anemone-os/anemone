# 2026-06-06 - Signal Temporary Mask Restore

**Status:** Active
**Owners:** doruche, Codex
**Area:** signal / wait-core / syscall ABI / iomux
**RFC:** [RFC-20260606-signal-temp-mask-restore](../../rfcs/signal-temp-mask-restore/index.md)
**Current Phase:** Agent 1 Stage 1A pending

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

**Last Updated:** 2026-06-06

**Current Branch:** `dev/drc/signal-temp-mask`

**Canonical RFC:** [RFC-20260606-signal-temp-mask-restore](../../rfcs/signal-temp-mask-restore/index.md), [Invariants](../../rfcs/signal-temp-mask-restore/invariants.md), [Implementation Plan](../../rfcs/signal-temp-mask-restore/implementation.md), [Tracking Issues](../../rfcs/signal-temp-mask-restore/tracking-issues.md), [Agent Orchestration](../../rfcs/signal-temp-mask-restore/backgrounds/agent-orchestration.md)

**Completed:** 公开 RFC、invariants、implementation、tracking issues 和 agent orchestration
文档已存在。总控完成实现前第一轮只读刷新：当前分支为 `dev/drc/signal-temp-mask`，工作区在事务启动前干净；代码仍是旧 `Task.sig_mask: NoIrqSpinLock<SigSet>` 模型；`rt_sigsuspend` 尚未注册；`ppoll` / `pselect6` 仍保留 legacy save / set / wait / restore 路径；`rt_sigtimedwait` 仍有 RFC 点名的 signal / force wake 后 waited-set dequeue 边界。Agent 0 只读前置审计已完成，未发现 RFC blocker 或停止条件，允许进入 Agent 1 阶段 1A。

**In Progress:** 无。

**Open Blockers:** 暂无。

**Next Action:** 启动 Agent 1 阶段 1A，只允许完成 `TaskSigMaskState` storage 与 ordinary current-mask API；不得迁移 `ppoll` / `pselect6` delayed restore，不得建立 temporary token contract。

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
