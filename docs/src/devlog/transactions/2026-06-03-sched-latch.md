# 2026-06-03 - Sched Latch

**Status:** Complete
**Owners:** doruche, Codex
**Area:** scheduler / wait core / iomux / poll / select
**RFC:** [RFC-20260603-sched-latch](../../rfcs/sched-latch/index.md)
**Current Phase:** Stage 6 audit complete

## Scope

本事务跟踪 `sched-latch` 迁移：在 scheduler wait core 上建立 single-consumer / multi-producer 的一轮 OR wait 原语，并把 `poll` / `select` 从 busy polling 迁移到真实 wait 协议。

本事务覆盖：

- `sched::latch` 原语；
- `fs::iomux` 的 typed source register protocol；
- pipe 作为首个 source 的迁移；
- `ppoll` 与 `pselect6` 的共享 latch loop、final scan 和 outcome mapping；
- `PollWaiter` / `poll_waiters` / `yield_now()` 等旧 busy-poll 旁路审计；
- rv64 / LTP 中依赖 poll/select 睡眠可观测性的验证证据。

非目标：

- 不替换 `Event` 或把所有同步原语统一成 `Latch`。
- 不实现完整 Linux waitqueue、epoll、futex PI 或异步通知框架。
- 不引入跨等待轮次保存的 notification permit。
- 不引入 count-down latch、barrier 或 AND wait 语义。

## Invariants

- 一个 `Latch` 只对应一轮 wait core wait identity。
- 同一轮 `Latch` 只有一个 consumer 持有 waiter lifecycle。
- 同一轮 `Latch` 可以派生多个 producer trigger。
- producer trigger、timeout、signal、force 和 consumer cancel 竞争同一个 `WaitState`。
- 任何完成本轮 wait 的路径都由 wait core 负责逻辑完成和 stale-safe physical placement。
- source 注册必须在 source lock 下同时检查 readiness 并保存 trigger。
- source wake 必须在同一 source lock 临界区内完成 predicate update 与 trigger detach，释放 source lock 后再 trigger。
- wake 只是 readiness hint；`ppoll` / `pselect6` 返回前必须重新 final readiness scan。
- 未 armed source 不得让 syscall 进入 latch schedule。

## Handoff

**Last Updated:** 2026-06-04

**Current Branch:** `dev/drc/latch`

**Canonical RFC:** [RFC-20260603-sched-latch](../../rfcs/sched-latch/index.md), [Invariants](../../rfcs/sched-latch/invariants.md), [Implementation Plan](../../rfcs/sched-latch/implementation.md), [Tracking Issues](../../rfcs/sched-latch/tracking-issues.md)

**Completed:** `etc/plans/sched-latch` 草稿已提升为公开 RFC；文档协议审查未发现新的 Apollyon / Keter 级硬障碍；软件工程审查结果已作为实现工程指导写入 implementation gate；事务日志、事务索引、双周 devlog 和 mdBook Summary 已建立链接。Agent 0.5 已完成 wait-core surface hardening：`crate::sched::*` 不再 re-export `ActiveWait`、`WakeToken`、`WaitReason`、`WaitOutcome`、`WakeResult` 或 `WaitStateStatus`；clock / signal syscall adapter 改走受限 current-wait wrapper；`Event` 仍作为 scheduler 内部合法 wait adapter 直接使用 wait core。Agent 1 已完成 `sched::latch` 原语，并通过 Gate 1 review。Agent 2 已完成 typed `PollRequest` / `PollRegisterResult` 协议和 pipe source trigger queue 迁移，旧 `PollWaiter` / `poll_waiters` 草稿入口已清除。Agent 3 已将 `sys_ppoll` 迁移到 latch wait loop，并建立 `fs/api/iomux/wait.rs` 作为 `pselect6` 可复用的 snapshot/register/final-scan outcome mapping 边界。Agent 4 已将 `sys_pselect6` 迁移到同一 helper，移除 pselect wait path 中的 `yield_now()`，并保持 Linux fd_set / SigSetArgPack ABI copy-in/copy-out 在 syscall adapter 层。Agent 5 已完成阶段 6 旁路审计，未发现 Apollyon / Keter 阻塞项；旧 `ANE-20260531-IOMUX-INFINITE-WAIT-STAGE1` 限制已按 latch 迁移结果标记为关闭。

**Open Blockers:** 当前没有 Still open plan gap，也没有阶段 6 发现的 Apollyon / Keter 阻塞项。`TaskSchedState` 和 `Task::update_sched_state_with()` 仍是 crate-internal scheduler-state surface；阶段 6 审计未发现 fd source 直接使用这些入口或直接做 runqueue placement。`pselect6` exception / POLLPRI 仍是显式 stub，属于后续功能边界，不影响当前 READABLE / WRITABLE OR wait 语义闭合。

**Next Action:** `sched-latch` 核心迁移关闭。后续新增 source、POLLPRI / exception readiness、epoll 或异步通知必须作为独立 follow-up，不应重新打开本事务的 wait identity / register gate / outcome mapping。

**Do Not Redo:** 不要重新把 `etc/` 个人草稿作为 canonical source；不要把 `PollWaiter` / `poll_waiters` 草稿扩展成新的 waitable poll 协议；不要让 `ppoll` 与 `pselect6` 分裂 outcome mapping；不要把未迁移 source 当成 armed source；不要在 `Unsupported` register result 上 fallback 到 `yield_now()` 或其它会睡在未 armed source 上的路径。

## Phase Log

### 2026-06-03 - RFC 提升与事务日志启动

**Phase:** planning / RFC promotion

**Change:** 将 `etc/plans/sched-latch` 的已收口内容提升到 [docs/src/rfcs/sched-latch](../../rfcs/sched-latch/index.md)，并建立本事务日志。RFC 目录包含入口、[不变量需求](../../rfcs/sched-latch/invariants.md)、[迁移实施计划](../../rfcs/sched-latch/implementation.md)、[Tracking Issues](../../rfcs/sched-latch/tracking-issues.md) 和背景材料索引。

**Change:** RFC 页首、Tracking Issues、事务日志索引、mdBook Summary 和当前双周 devlog 均已建立公开链接。后续实现记录写入本事务日志，不再引用个人 `etc/` 草稿作为 canonical source。

**Review:** 协议层多线审查结论为：当前没有新的 Apollyon / Keter 硬障碍。单轮 wait identity、owner-bound `Latch`、producer no-return / fail-closed capability、source register gate、source wake detach、cleanup 非正确性支柱、final readiness scan 和 `ppoll` / `pselect6` 统一 outcome mapping 均已在 RFC 中闭合。

**Engineering Guidance:** 软件工程审查发现的维护性风险已落到实施计划：`PollWaiter` / `poll_waiters` 是旧草稿形状，不得继续扩展为新协议；pipe source 迁移时必须清理无效 waiter 队列；`ppoll` / `pselect6` 应共享 latch loop / outcome helper，避免后续漂移。

**Validation:** 本阶段只更新文档结构，未修改代码，未运行构建或 QEMU / LTP。

**Next:** 阶段 1 建立 `sched::latch` 原语，并记录 wait-core placement 前置审计、old trigger late arrival debug hook 或最小单测结果。

### 2026-06-03 - Agent 0.5 wait-core surface hardening

**Phase:** pre-Agent-1 hardening / KETER-005 blocker closure

**Change:** 收窄 `sched::mod` 的 wait-core re-export：`ActiveWait`、`WakeToken`、`WaitReason`、`WaitOutcome`、`WakeResult` 和 `WaitStateStatus` 不再作为 `crate::sched::*` 的普通外部能力暴露。raw begin / clone token / cancel / finish / producer wake result 现在只在 `sched` 子模块内部可见。

**Change:** 新增受限的 current-task wait adapter：`CurrentWaitPrecheck`、`CurrentWaitOutcome` 和 `wait_current_with_timeout()`。`clock_nanosleep` 与 `rt_sigtimedwait` 继续通过 wait core 创建、取消、timeout wake、finish 本轮等待，但调用点不再持有 `ActiveWait`、`WakeToken` 或任意 `WaitReason`。`rt_sigtimedwait` 保留 pending signal 的消费语义：precheck 命中的 signal 保存在 syscall 本地结果中，wrapper 只负责取消并 retire 本轮 wait。

**Change:** `Event` 保留为 scheduler 内部合法 wait adapter，显式使用 `super::wait` 和内部 timeout helper。`Event::publish()` 的 producer completion 仍走 `wake_wait()`，signal / force `notify()` 仍走 `wake_active_wait()`，timeout callback 仍通过 wait identity 调 `wake_wait(..., Timeout, AnyWait)`，因此 logical completion 与 stale-safe placement 合同未改变。

**Review:** 这一步满足 Agent 0 停止条件中的 wait lifecycle surface blocker：普通 fd/device source 不能再通过 `crate::sched::*` 取得 `ActiveWait` / `WakeToken` 来直接创建、clone token、cancel、finish wait round 或读取 `WakeResult` 分支。当前改动没有新增 `Latch` / `LatchTrigger`，没有新增第二套 armed/completion 状态，也没有改变 wait identity、completion 线性化点、Event 语义、timeout/signal/force 路径或 syscall outcome mapping。

**Boundary:** `TaskSchedState` 仍通过 `sched::*` 暴露，因为 `Task` 内部字段、exit 路径、processor placement 和 wait core 仍直接依赖它；`Task::update_sched_state_with()` 也仍是 crate-internal 写入口。Agent 0.5 的 write set 不包含 `task/sched.rs` / `task/mod.rs` / exit / processor owner 文件，因此本阶段只把 wait lifecycle capability 从 fd source 面前拿掉；彻底封住普通 source 写 task sched state 需要后续单独 gate。

**Validation:** `just build` 通过。构建期间仅保留既有 warning：`anemone-kernel/src/sync/mono.rs` 中 `AtomicBool` / `Ordering` 未使用；本阶段未修改该文件。

**Next:** 总控可基于本 hardening 重新判定是否派发 Agent 1。Agent 1 仍需按 RFC 阶段 1 实现 owner-bound `Latch`、no-return / fail-closed `LatchTrigger`、受限 cancel reason 和 trigger 生命周期策略。

### 2026-06-03 - Agent 1 sched::latch primitive

**Phase:** stage 1 / `sched::latch` primitive

**Change:** 新增 `sched::latch` 子模块，并只通过 `sched::mod` 暴露 `Latch`、`LatchTrigger`、`LatchCancelReason` 和 `LatchWaitOutcome`。`sched::wait` 只新增 `WaitReason::Latch`，没有重新向 `crate::sched::*` 暴露 `ActiveWait`、`WakeToken`、`WakeResult`、`WaitOutcome` 或 raw wait-core lifecycle API。

**Change:** `Latch::begin_current(interruptible)` 为当前 task 创建一轮 `ActiveWait`；`Latch` 字段私有、不实现 `Clone`，并用 `PhantomData<*mut ()>` 保持 `!Send` / `!Sync`。waiter-owned 方法记录 owner task id 和 wait id；owner misuse、finish 后 cancel / make-trigger / schedule 和 double finish 都先记录上下文，再用 release build 生效的 `assert!` 暴露代码错误；drop-without-finish 先执行 `Drop` cancel + retire，保持 fail-closed cleanup，再 `assert!` 暴露 owner bug。

**Change:** `Latch::make_trigger()` 派生 cloneable `LatchTrigger`。producer handle 保存 weak task + strong `WakeToken`，普通 `trigger()` 通过 `wake_wait(..., WaitReason::Latch, WakeMode::AnyWait)` 完成本轮 wait，返回值不暴露给 caller，只在 scheduler debug log 中记录 woke / stale / retired / already completed / already cancelled / mode-blocked 等诊断结果。

**Change:** `Latch::cancel(reason: LatchCancelReason)` 使用受限 consumer cancel reason，并委托 wait core 的 `cancel_if_armed()`；因此 cancel 是同一 wait transaction 上的一次 completion attempt，不覆盖已经完成的 winning outcome。`finish(self)` 消耗 `Latch` 并 retire 本轮 wait；`Drop` 对未 finish 的 latch 记录 warning，执行 `Drop` cancel，再 retire，然后用 release assertion 暴露未显式 finish 的代码错误。

**Review:** KETER-003 已处理：consumer capability 是 owner-bound linear guard，类型上 `!Send` / `!Sync`，字段私有，不可 clone。KETER-004 已处理：cancel / finish 仍由 wait core 线性化，first-completion-wins，begin 后通过 `finish(self)` 或 Drop fail-closed retire。KETER-005 继承 Agent 0.5 结论：producer trigger 走 `wake_wait()`，timeout / signal / force 入口未改变，`Woke` 语义仍包含 stale-safe placement。KETER-006 本阶段选择 weak task + strong `WakeToken`；阶段 2 source 注册协议必须定义 source queue pruning / cleanup 上界，正确性仍不得依赖 cleanup。KETER-007 已处理：普通 producer API no-return / fail-closed，不公开 `WakeToken`、强 `Task`、`WakeResult`、`WakeEnqueueResult` 或 waiter lifecycle API。

**Boundary:** 本阶段没有接入生产 iomux 路径，没有修改 pipe、ppoll、pselect6、fs、task 或 time 文件。old trigger late-arrival 行为当前通过 wait-core identity 和 trigger debug result code audit 覆盖；限定 write set 内未新增 KUnit/runtime hook。后续 Agent 2/3 在 source queue 和 pipe 注册落地时补充 consumer finish 后 late trigger 的 runtime evidence。

**Validation:** `just build` 通过。构建期间仅保留既有 warning：`anemone-kernel/src/sync/mono.rs` 中 `AtomicBool` / `Ordering` 未使用；本阶段未修改该文件。`git diff --check` 通过。

**Review:** Gate 1 reviewer 初审发现 `Drop` 路径在 cleanup 前执行 `debug_assert!`，debug/asserting kernel 下无法保证 drop-without-finish fail-closed retire。已修复为 warning 后执行 `Drop` cancel + `finish_inner("drop")` retire；窄复核确认 KETER-004 blocker 关闭，Gate 1 当前无 Apollyon / Keter 阻塞项。随后按用户要求固化断言纪律：非重型 invariant 不能只放在 `debug_assert!`，必须用 release build 生效的 `assert!` 让 user-test / LTP 暴露代码错误；重型扫描或昂贵诊断才使用 debug-only 检查。该规则已写入 agent 编排文档。

**Next:** 按用户要求暂停在阶段 1 / Gate 1 之后。恢复时进入 Agent 2；阶段 2/3 必须把 weak task + strong `WakeToken` 策略落到 source queue pruning / cleanup 上界，并在 pipe source queue 落地后补 consumer finish/drop 后 late trigger 的 runtime evidence。

### 2026-06-03 - Agent 2 typed poll register + pipe source

**Phase:** stage 2/3 / typed source register protocol + pipe source

**Change:** `fs::iomux` 的 `PollRequest` 已拆成 snapshot 与 register 两种构造：`PollRequest::snapshot(interests)` 保持纯快照，`PollRequest::register(interests, &LatchTrigger)` 携带本轮 producer capability。`FileOps::poll` / `File::poll` / `FileDesc::poll` 返回 typed `PollRegisterResult::{Ready(PollEvent), Armed, Unsupported}`；snapshot-only source 通过 `ready_or_unsupported()` 保持旧 readiness 语义，并在 register + not-ready 时 fail closed 为 `Unsupported`。

**Change:** 旧 `PollWaiter` / `poll_waiters` 草稿入口已移除：`fs::mod` 不再 re-export `PollWaiter`，pipe endpoint 上的 `poll_waiters` 字段已删除，代码搜索无残留。`ppoll` / `pselect6` 本阶段只做 snapshot typed-result 适配，仍保留原 busy loop；生产 latch wait loop 留给 Agent 3。

**Change:** pipe source 已迁移为共享 `Pipe` 状态内的 `rx_poll_triggers` / `tx_poll_triggers` 队列。`pipe_rx_poll()` / `pipe_tx_poll()` 在同一 `Pipe` lock 下先检查 readiness；ready 直接返回 `Ready(events)`，not-ready register 请求才保存 `LatchTrigger` 并返回 `Armed`。pipe read、write、rx drop、tx drop 在同一 `Pipe` lock 临界区内完成 predicate update 与对应 trigger detach，释放 pipe lock 后再逐个 `LatchTrigger::trigger()`。

**Change:** 阶段 1 选择的 weak task + strong `WakeToken` 策略已落到 source queue hygiene：`LatchTrigger::is_prunable()` 只暴露资源清理 hint，不返回 wake result，不允许 source 据此判断 readiness 或补做 completion；pipe 在注册与 detach 前 lazy prune retired / task-gone trigger。old trigger 正确性仍依赖 wait-core identity 和 retired-state fail-closed，不依赖 cleanup 成功。

**Review:** KETER-001 已处理：typed register result 已落地，未迁移 snapshot-only source 在 register + not-ready 时返回 `Unsupported`，不能被后续 syscall 当作 armed source。KETER-002 已处理：pipe predicate update + trigger detach 使用共享 `Pipe` lock 作为线性化点，触发发生在释放 source lock 之后。KETER-006 已处理到 Agent 2 边界：source queue cleanup 是 lazy pruning / resource hygiene，不参与 correctness；强 `WakeToken` 残留只到 source queue 下次 register / detach / drop。EUCLID-002 / EUCLID-003 已处理到诊断边界：pipe register / detach / trigger 日志包含 side、wait id、interests、reason 和 detach/prune count。

**Boundary:** 本阶段没有迁移 `ppoll` / `pselect6` 到 latch schedule；它们仍通过 snapshot scan + `yield_now()` busy loop 工作。Agent 3 必须复用本阶段的 typed register result，并保证 `Unsupported` 不进入 schedule。

**Validation:** `just build` 通过。构建期间仅保留既有 warning：`anemone-kernel/src/sync/mono.rs` 中 `AtomicBool` / `Ordering` 未使用；本阶段未修改该文件。`git diff --check` 通过。用户侧已运行 `LTP iomux`，结果通过。

**Next:** 进入 Agent 3：迁移 `ppoll` 并建立可供 `pselect6` 复用的 latch wait helper、final scan 和 outcome mapping。

### 2026-06-04 - Agent 3 ppoll latch loop + shared iomux wait helper

**Phase:** stage 4 / `ppoll` latch loop

**Change:** 新增 `fs/api/iomux/wait.rs`，作为 `ppoll` 与后续 `pselect6` 共享的 wait loop / outcome mapping 边界。helper 只暴露 `IomuxScanMode::{Snapshot, Register(&LatchTrigger)}`、`IomuxScanOutcome::{Ready, NotReady, Unsupported}` 和 `wait_for_iomux_ready()`；Linux `pollfd` / `fd_set` ABI copy-in/copy-out 仍留在 syscall adapter 层。

**Change:** `sys_ppoll` 现在先安装临时 signal mask，再通过 snapshot scan 填充 Linux `revents`。snapshot 有 `Ready`、`POLLNVAL` 或 source poll error 时直接返回，不创建 `Latch`。snapshot 后若已有 unmasked signal、zero timeout 或 deadline expired，直接返回 `EINTR` / `0`，不创建 `Latch`。

**Change:** 只有确实需要阻塞时才 `Latch::begin_current(true)`。`ppoll` 用同一个 `LatchTrigger` 对所有有效 fd 做 register scan；register scan 发现 ready / `POLLNVAL` / source poll error 时立即停止扫描，`cancel(PredicateReady)` + `finish()` 后返回当前 ready 结果，不 schedule。所有未 ready source 都返回 `Armed` 后，才调用 `schedule_with_timeout(remaining)`。

**Change:** wake / timeout / signal / force 返回后，helper 先 `finish()` 当前 latch round，再做 final snapshot scan。register scan 的 `Unsupported` 或 `Err` 路径也在 `cancel + finish` 后执行同一 final snapshot scan，避免前序已 armed source 竞态触发后被误报成 register failure。final scan 的 ready 结果优先；若 final scan 仍无 ready，则统一映射 `Triggered -> retry`、`Timeout -> 0`、`Signal | Force -> EINTR`、`Cancelled | Unexpected -> EIO`。这条 mapping 是阶段 5 `pselect6` 必须复用的边界。

**Unsupported Strategy:** register scan 遇到 `PollRegisterResult::Unsupported` 时 fail closed：`cancel(RegisterError)` + `finish()` 后先做 final snapshot scan；若 final scan 仍无 ready，返回 `SysError::NotSupported`，映射为 `EOPNOTSUPP`。本阶段不使用 busy-loop fallback，也不让 syscall 睡在未 armed source 上；尚未迁移的 snapshot-only source 只有在 snapshot 已 ready 时可返回。

**Review:** KETER-001 已处理到 `ppoll` 边界：未 armed source 不进入 schedule。KETER-004 已处理：helper 中每个 begin 后路径都显式 `finish()`，包括 register ready、register unsupported、register error 和 schedule return。KETER-008 已固定：final scan 先于 timeout/signal outcome mapping，register-fail 竞态也先 final scan 再返回错误，且 mapping 位于 `wait.rs`，供 `pselect6` 复用。EUCLID-001 已处理：signal precheck、zero timeout 和 deadline expired 都发生在 latch begin 前。

**Boundary:** 本阶段没有修改 `pselect6.rs`；该 syscall 仍是 snapshot + `yield_now()` 旧路径，阶段 5 必须迁移到 `wait.rs`。本阶段没有改变 source 协议语义，也没有调整 pipe source。

**Validation:** `just build` 通过。构建期间仅保留既有 warning：`anemone-kernel/src/sync/mono.rs` 中 `AtomicBool` / `Ordering` 未使用；本阶段未修改该文件。`git diff --check` 通过。未运行 QEMU / LTP。

**Next:** 进入 Agent 4：迁移 `pselect6`，复用 `fs/api/iomux/wait.rs` 的 latch loop、final scan 和 outcome mapping。

### 2026-06-04 - Agent 4 pselect6 latch loop

**Phase:** stage 5 / `pselect6` latch loop

**Change:** `sys_pselect6` 已迁移到 `wait_for_iomux_ready("sys_pselect6", &task, timeout, |mode| ...)`。等待 loop、timeout / signal / force / cancel outcome mapping、register abort 后 final snapshot scan 均复用 Agent 3 的 `fs/api/iomux/wait.rs` helper；`pselect6` 不再复制 `ppoll` 的 latch loop，也不再在 iomux wait path 调用 `yield_now()`。

**Change:** Linux `fd_set` 与 `SigSetArgPack` copy-in/copy-out 仍保留在 `pselect6.rs`。输入 fdsets 现在作为 immutable interest bitmaps 保存；每次 snapshot / register / final scan 都先清空 scratch ready-output bitmaps，再根据本次扫描结果重建输出。用户态 fdsets 只在 helper 返回后写回 scratch ready-output fdsets，不再写回或破坏原始 interest sets。

**Change:** READABLE 与 WRITABLE fdset 扫描分别通过 `mode.poll_request(PollEvent::READABLE)` 和 `mode.poll_request(PollEvent::WRITABLE)` 构造 snapshot/register request。register scan 全量扫描 read/write fdsets 并验证 exception fdset 后才返回结果，不在第一个 ready fd 处早停；scratch ready-output fdsets 因此来自本次完整扫描。遇到 `Unsupported` 且没有真实 ready 时流入共享 helper 的 cancel + finish + final scan 逻辑，不做 pselect-local busy-loop fallback。

**Exception Boundary:** exception fdset 仍是 stub。当前内部 `PollEvent` 没有 POLLPRI / exception bit，因此 `pselect6` 不用 `PollEvent::empty()` 表示 exception register 请求。snapshot/final snapshot 只验证 exception fdset 中的 fd 仍有效，并保持 exception output fdset 为空；register scan 如果存在 exception interest 且没有 read/write ready，则 fail closed 为 `Unsupported`，避免睡在没有 armed source 的 exception wait 上。

**Review:** KETER-008 已处理到 `pselect6` 边界：`ppoll` / `pselect6` 使用同一个 helper 执行 final scan 优先的 outcome mapping。EUCLID-001 由 helper 统一处理：snapshot 后的 signal precheck、zero timeout 和 deadline expired 都发生在 latch begin 前。fdset 输出来自实际确定 readiness 的 snapshot/register/final scan scratch bitmaps，输入 interest bitmaps 跨轮次保持不变。

**Boundary:** 本阶段没有修改 pipe、FileOps、`sched::latch` 或 `ppoll`。exception readiness / POLLPRI 仍未实现；该 stub 只保留有效 fd 检查和空输出，不参与 wait source 注册。

**Validation:** `just build` 通过。构建期间仅保留既有 warning：`anemone-kernel/src/sync/mono.rs` 中 `AtomicBool` / `Ordering` 未使用；本阶段未修改该文件。`git diff --check` 通过。未运行 QEMU / LTP。

**Next:** 进入 Agent 5：做旁路审计、构建 gate 和事务日志收口。

### 2026-06-04 - Agent 5 side audit and closure

**Phase:** stage 6 / bypass audit and closure

**Audit:** `PollRequest` / `PollWaiter` / `poll_waiters` / `yield_now()` 搜索已分类。`PollWaiter` 和 `poll_waiters` 没有代码残留；`yield_now()` 只剩 pipe read/write 自身阻塞、`sched_yield` 和 thread-group wait 轮询路径，不在 `ppoll` / `pselect6` iomux wait loop 中。`ppoll` 与 `pselect6` 都通过 `fs/api/iomux/wait.rs` 的 `wait_for_iomux_ready()` 进入同一 latch owner lifecycle、register abort final scan 和 outcome mapping。

**Audit:** `WaitReason::Latch` / `LatchTrigger` / `sched::latch` 搜索只命中 latch 原语、`fs::iomux` register request、共享 iomux wait helper 和 pipe source trigger queue。producer 普通 API 仍是 no-return / fail-closed，不公开 raw `WakeToken`、`WakeResult` 或 waiter lifecycle；`LatchTrigger::is_prunable()` 只作为 source queue hygiene hint 使用，不参与 readiness 或补偿 wake。

**Audit:** `wake_wait()` / `wake_active_wait()` 调用点仍局限在 scheduler 内部、`Event`、`LatchTrigger` 和 timeout / signal wrapper。fd source 没有直接调用 wait-core lifecycle，也没有直接写 `TaskSchedState` 或直接调用 `task_enqueue` / `local_enqueue` / `remote_enqueue`。pipe source 的 predicate update + trigger detach 在同一 `Pipe` lock 临界区内完成，`LatchTrigger::trigger()` 发生在释放 pipe lock 之后。

**Audit:** typed register gate 保持 fail closed：snapshot-only source 通过 `ready_or_unsupported()` 在 register + not-ready 时返回 `Unsupported`；共享 wait helper 在 register `Unsupported` / error 后执行 `cancel + finish + final snapshot scan`，final scan 仍无 ready 时返回错误，不 fallback 到 busy polling，也不睡在未 armed source 上。pipe source queue 采用 lazy pruning；强 `WakeToken` 残留只影响资源卫生，old trigger fail-closed 仍由 wait identity / retired state 保证。

**Review:** 阶段 6 未发现 Apollyon / Keter 阻塞项。原 implementation gates 的收口状态：KETER-001 register scan fail closed、KETER-002 source wake detach、KETER-003 owner-bound `Latch`、KETER-004 cancel / finish first-completion-wins、KETER-005 stale-safe placement、KETER-006 source queue hygiene、KETER-007 producer capability 边界和 KETER-008 统一 outcome mapping 均有代码与审计证据。剩余 `pselect6` exception / POLLPRI stub 是功能边界，不参与当前 latch wait source 注册。

**Validation:** `git diff --check 1caf6d5..HEAD` 通过。`just build` 通过，仅保留既有 `anemone-kernel/src/sync/mono.rs` 中 `AtomicBool` / `Ordering` unused import warning。本轮 agent 未重新运行 QEMU / LTP；用户侧阶段 6 测试已通过，覆盖最终 runtime gate。

**Register:** `ANE-20260531-IOMUX-INFINITE-WAIT-STAGE1` 已从当前 active limitation 调整为 resolved：`ppoll` / `pselect6` 的有 fd 阻塞路径不再通过 busy polling 表达睡眠，改由 wait-core `Latch` 和 source typed register gate 提供可观察等待。

## Open Items

- 本事务无剩余 implementation blocker。后续 source 扩展、`pselect6` exception / POLLPRI readiness、epoll、异步通知或更完整 Linux waitqueue 兼容应单独建 follow-up。
