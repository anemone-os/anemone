# Signal Temporary-mask Delivery Handoff 当前契约

**Contract ID：** `SIGNAL-TEMP-MASK`
**状态：** Active
**Owner：** current task Signal temporary-mask / delivery-handoff protocol
**参与领域：** signal mask / private and shared pending / wait-core outcome classification / ordinary trap return
**覆盖范围：** delayed temporary-mask restore、stable delivery reservation，以及 handler-frame / no-frame cleanup
**不覆盖：** `rt_sigtimedwait` 的syscall-body-only mask、job-control phase / report、fresh / clone / exec user entry
**实现位置：** `anemone-kernel/src/task/sig/`、`anemone-kernel/src/task/sig/api/{rt_sigsuspend,rt_sigreturn}.rs`、`anemone-kernel/src/fs/api/iomux/`
**依赖：** `SIGNAL-PENDING-001`、`SIGNAL-ACTION-002`
**Pending Successor：** None
**最后核验：** 2026-07-21

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| current mask 与 pending restore slot | current `Task::sig_mask` state | syscall 持有 linear token | temporary mask install / restore |
| reserved delivery target | current `Task::sig_pending` | classifier 只返回 typed decision | 下一次 ordinary trap-return delivery |
| restore responsibility | token、Signal no-frame cleanup 或 committed user frame 三者之一 | wait-core / iomux 只持 candidate | 保证旧 mask 不丢失 |

reserved target 是尚未完成 action selection 的 task-private Signal handoff，不是 wait-core outcome、notification 或 caller-owned signal。

## SIGNAL-TEMP-MASK-001 — Current mask 与 restore slot 只有一个 task owner

**规则：** current task 的 current signal mask、pending old-mask restore 和 active linear-token identity 由同一个 Signal mask state 持有。一次 temporary-mask window 只能由对应 token 显式 `restore_now()` 或 `defer_to_signal_delivery()` 终止；clone / fork 和 procfs 只观察 current mask，不继承或暴露 pending restore。

**违反表现：** syscall、wait-core 或 architecture 另存 restore truth，nested begin 静默覆盖旧 slot，token drop 隐式恢复，或 child 继承 parent 的 pending restore responsibility。

**验证 / Enforcement：** `TaskSigMaskState`、`TemporarySigMaskToken`、mask mutation helper 与 clone / procfs 路径源码审计；temporary-mask signal 回归。

**最初来源：** [RFC-20260606-signal-temp-mask-restore](../../rfcs/signal-temp-mask-restore/index.md)；[实现事务](../../devlog/transactions/2026-06-06-signal-temp-mask-restore.md)。

**当前来源：** live Signal mask owner，2026-07-20 源码核验。

## SIGNAL-TEMP-MASK-002 — Defer 必须先建立 task-private delivery handoff

**规则：** signal-owned classifier只有在已经从private或shared ordinary pending中claim一个具体occurrence、并把它放入current task的reserved delivery target后，才能允许temporary-mask restore defer。该occurrence不再参加private / shared pending competition，但在action selection前仍是task-private pending snapshot的一部分；ordinary `Task::fetch_signal()`优先取得它。后续control-signal generation只清理ordinary opposite-class pending，不撤销这个已经完成claim的target；reservation不冻结live disposition，也不表示handler / default action已经提交。

**违反表现：** 仅凭 wait-core `Signal / Force` 或 pending snapshot 推断后续一定 delivery、shared occurrence 被多个 member 竞争、reserved target 被普通 pending scan 越过，或把 reservation 当成已经提交 handler / default action。

**验证 / Enforcement：** `classify_temporary_mask_wait()`、`reserve_temporary_mask_delivery_target()`、`PendingSignals::reserved_delivery`、`to_sigset()` 与 `fetch_any()` 源码审计；pending-before-wait 和 shared-pending handoff 回归。

**最初来源：** [RFC-20260606-signal-temp-mask-restore](../../rfcs/signal-temp-mask-restore/invariants.md#signal-handoff--reservation-规则)；[实现事务](../../devlog/transactions/2026-06-06-signal-temp-mask-restore.md)。

**当前来源：** [RFC-20260720-unix-jobctl R1](../../rfcs/unix-jobctl/index.md)保留reservation-first handoff；[Stage 5 cutover事务](../../devlog/transactions/2026-07-20-unix-jobctl.md#stage-5-uj-cutover与事务收口---2026-07-21)。

## SIGNAL-TEMP-MASK-003 — Handler commit 或 Signal no-frame cleanup 终结 restore responsibility

**规则：** reserved target建立custom handler frame时，frame保存temporary window之前的mask，并且只在frame与trapframe已提交后把恢复责任转移给`rt_sigreturn()`。如果ordinary user-entry arbitration没有提交handler frame，Signal owner在离开`handle_signals()`前恢复pending old mask；如果action进入SIGKILL、frame-failure或其它既有no-return terminal path，Signal owner必须在把task交给terminal teardown前清除该责任。jobctl与terminal teardown不拥有、删除或复制restore responsibility；wait-core、syscall caller和architecture return也不自行终结它。

**违反表现：** frame commit前清除restore slot、ignored/no-frame path携带temporary mask返回用户态、no-return terminal teardown遗留restore responsibility、jobctl擅自终结reservation，或architecture重复cleanup。

**验证 / Enforcement：** `sigmask_to_save_for_signal_frame()`、`signal_frame_committed_restore_mask()`、`restore_temporary_sig_mask_if_pending()`、`perform_signal_action()` 与 `handle_signals()` 源码审计；handler / ignore / no-frame mask-restore 回归。

**最初来源：** [RFC-20260606-signal-temp-mask-restore](../../rfcs/signal-temp-mask-restore/invariants.md#handler-frame-commit-规则)；[实现事务](../../devlog/transactions/2026-06-06-signal-temp-mask-restore.md)。

**当前来源：** live Signal delivery owner；[RFC-20260720-unix-jobctl R1](../../rfcs/unix-jobctl/index.md)保持本规则；[Stage 5 cutover事务](../../devlog/transactions/2026-07-20-unix-jobctl.md#stage-5-uj-cutover与事务收口---2026-07-21)。
