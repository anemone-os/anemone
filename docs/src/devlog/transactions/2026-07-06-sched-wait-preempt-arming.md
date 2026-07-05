# 2026-07-06 - Sched Wait Preempt Arming

**状态：** Active
**负责人：** doruche, Codex
**领域：** scheduler / wait core / kernel preempt / latch / iomux / timer / signal
**权威计划：** [RFC-20260618-sched-wait-preempt-arming](../../rfcs/sched-wait-preempt-arming/index.md), [不变量需求](../../rfcs/sched-wait-preempt-arming/invariants.md), [迁移实施计划](../../rfcs/sched-wait-preempt-arming/implementation.md), [Tracking Issues](../../rfcs/sched-wait-preempt-arming/tracking-issues.md)
**当前阶段：** 阶段 0 - 已关闭；下一步阶段 1

## 范围

本事务跟踪 `sched-wait-preempt-arming` RFC 的 staged implementation：

- 阶段 0 先补 single-active-wait 诊断和 caller-location origin，并生成 schedule / latch / event / direct wait 调用面审计；
- 后续阶段再引入 scheduler-private mode、schedule entry wrapper、trap preempt defer 和 wait-sleep proof；
- 每个阶段按 RFC `implementation.md` 的阶段顺序、write set、review gate 和停止边界推进。

非目标：

- 不在阶段 0 改 scheduler entry split、trap preempt 路径或 wait-sleep 语义；
- 不在本 RFC 内默认修复 fanotify 等 source owner 的 post-begin sleepability 问题；
- 不引入 `WaitPrimitive`、operation 字符串或公开 `ScheduleCaller` taxonomy；
- 不通过关闭 `kernel_preempt`、busy polling、延长 timeout、source-local park-ready flag 或弱化 assert 关闭问题。

## 不变量

- `TaskSchedState` 仍是当前 task scheduler/wait 状态的唯一真相源。
- `WaitState` origin 只保存 `core::panic::Location::caller()`，是 diagnostic-only 字段，不参与 wake identity、park permission、source registration truth 或 scheduler mode。
- wait identity、completion、cancel、finish 和 physical placement 仍由 wait core / task sched-state 统一管理。
- post-begin nested scheduler wait 是 caller/source owner 边界错误，wait core 只诊断暴露，不支持同一 task 的 nested active waits。
- worker 未经总控/用户批准不得越过阶段 write set；需要扩大时先提交 expansion request，并把批准结果写入本事务日志。
- 代码实现和 review gate 必须由不同 subagent 完成；总控只负责分工、集成、事务日志和提交。

## Handoff

**Last Updated:** 2026-07-06

**Current Branch:** `dev/drc/sched-split`

**Canonical RFC:** [RFC-20260618-sched-wait-preempt-arming](../../rfcs/sched-wait-preempt-arming/index.md), [Invariants](../../rfcs/sched-wait-preempt-arming/invariants.md), [Implementation Plan](../../rfcs/sched-wait-preempt-arming/implementation.md), [Tracking Issues](../../rfcs/sched-wait-preempt-arming/tracking-issues.md)

**Completed:** 公共 RFC、invariants、implementation 和 tracking issues 已存在。阶段 0 已建立本事务日志，并连接 RFC、事务索引、当前双周 devlog 和 mdBook Summary。阶段 0 code worker 已补 `WaitOrigin` caller-location origin、begin-side nested active wait assert、crate 内 no-nested-wait helper 和 `Mutex::lock()` nested-wait 诊断；review gate 初审发现的 direct wait adapter `#[track_caller]` 缺口已修复，`WaitOrigin` follow-up closure review 无 Apollyon / Keter / Euclid finding。

**In Progress:** 无。等待阶段 0 checkpoint commit 后进入阶段 1。

**Open Blockers:** `KETER-001`、`KETER-004`、`KETER-005`、`KETER-006` 仍是 implementation feedback gates。阶段 0 若发现某个 shared wait caller 无法通过 entry split 表达，或 post-begin setup 依赖任意长 source scan，必须停止并回到 RFC review。

**Next Action:** 阶段 1 `Scheduler-Private Mode 与 Wrapper 分流`。进入前继续保持阶段 write set 和 review gate 分工；代码实现与 review gate 仍由 subagent 执行。

**Do Not Redo:** 不要把私有草稿路径写入公共 canonical 链接；不要把 caller origin 改成手写 operation 字符串；不要在阶段 0 顺手改 scheduler entry split；不要把 fanotify/source-owner nested wait panic 包装成本 RFC 内的 source-specific workaround。

## Phase Log

### 2026-07-06 - 阶段 0 事务日志启动与实施前审计

**阶段：** 阶段 0 - Preflight 与 Single-Active-Wait 诊断。

**变更：** 在代码实现前建立本事务日志，并把 [RFC-20260618-sched-wait-preempt-arming](../../rfcs/sched-wait-preempt-arming/index.md)、[Tracking Issues](../../rfcs/sched-wait-preempt-arming/tracking-issues.md)、事务索引、mdBook Summary 和当前双周 devlog 连接到同一条实现记录。

**前置状态：**

- 分支为 `dev/drc/sched-split`，阶段启动时 `git status --short --branch` 只显示当前分支。
- RFC 状态为 `Accepted`，事务日志字段此前为 `None`。
- register/current-limitations 已在阶段前读取；相关开放项仍包括 scheduler/event wait 交错、signal wait-core 语义、LTP post-summary hang 和 IRQ/off-tail allocation audit。本阶段不关闭这些条目。

**阶段 0 搜索：**

```sh
rg -n "schedule\(\)" anemone-kernel/src
rg -n "Latch::begin_current" anemone-kernel/src
rg -n "wait_current_with_timeout|ActiveWait::begin|schedule_wait_with_timeout" anemone-kernel/src
rg -n "fn lock\(|listen_uninterruptible|listen_with_timeout" anemone-kernel/src/sync anemone-kernel/src/sched
rg -n "\.listen\(|\.listen_with_timeout\(" anemone-kernel/src
```

**裸 `schedule()` caller 分类起点：**

- `sched/event.rs` 直接 wait sleep：`listen()` 和 `listen_uninterruptible()` 在 listener 注册和 predicate/signal check 后调用裸 `schedule()`。
- `sched/mod.rs` direct timeout helper：`schedule_wait_with_timeout()` 在 timeout callback 安装后调用裸 `schedule()`；`yield_now()` 调用裸 `schedule()`。
- `sched/class/idle.rs` idle loop 调用裸 `schedule()`。
- `task/api/exit/mod.rs` exit no-return path 调用裸 `schedule()`。
- `arch/riscv64/exception/trap/{utrap,ktrap}.rs` 与 `arch/loongarch64/exception/trap/{utrap,ktrap}.rs` trap return preempt path 调用裸 `schedule()`。

**`Latch::begin_current()` direct users：**

- `fs/api/iomux/wait.rs`：source-backed iomux wait 和 no-source timeout wait。
- `fs/eventfd.rs`：blocking read 和 blocking write。
- `fs/fanotify/group.rs`：blocking read wait。
- `fs/timerfd.rs`：blocking read wait。

**direct wait helper / finite timeout users：**

- `sched/event.rs`：`Event::prepare_listener()` 通过 `ActiveWait::begin()` 发布 listener wait；`Event::listen_with_timeout()` 通过 listener token 调 `schedule_wait_with_timeout()`。
- `sched/latch.rs`：`Latch::begin_current()` 通过 `ActiveWait::begin()` 发布 latch wait；`Latch::schedule_with_timeout()` 调 `schedule_wait_with_timeout()`。
- `sched/mod.rs`：`wait_current_with_timeout()` 是 clock/signal direct wait adapter，先 `ActiveWait::begin()`，precheck 未完成时再调 `schedule_wait_with_timeout()`。
- `time/clock/api/clock_nanosleep.rs`、`task/sig/api/rt_sigsuspend.rs`、`task/sig/api/rt_sigtimedwait.rs` 当前通过 `wait_current_with_timeout()` 进入 direct wait。

**`Event::listen*()` users：**

- `task/api/futex/futex.rs`：user-visible futex wait path 使用 `listen()` 和 `listen_with_timeout()`。
- `task/api/wait/mod.rs`：child-exit wait 使用 `listen()`。
- `time/timer/threaded.rs`：threaded timer completion internal wait 使用 `listen_with_timeout()`。

**Mutex / nested-wait 起点：**

- `sync/mutex.rs::Mutex::lock()` 已有 `#[track_caller]`，先断言非 hard IRQ、interrupt enabled、`allow_preempt()` 和非递归，fast path compare-exchange 失败后进入 `lock_released.listen_uninterruptible()`。
- 阶段 0 worker 必须在 `allow_preempt()` 诊断被遮蔽前检查 current 是否已处于 active wait，并在 compare-exchange 失败后、调用 `listen_uninterruptible()` 前再次检查。

**Implementation worker 合同：**

- 允许代码写集：`anemone-kernel/src/sched/wait.rs`、`anemone-kernel/src/sync/mutex.rs`；为传播 `#[track_caller]` 可触碰 `anemone-kernel/src/sched/latch.rs`、`anemone-kernel/src/sched/event.rs`；如需窄 re-export 可触碰 `anemone-kernel/src/sched/mod.rs`。
- 必须使用 `#[track_caller]` / `Location::caller()` 传播 caller location，不得新增 explicit operation 参数、`WaitPrimitive`、字符串 taxonomy 或 source-local park state。
- `assert_current_not_in_active_wait()` 必须只读 current task sched-state snapshot，panic 信息包含 current task、已有 wait id、已有 wait caller location 和当前 sleep-attempt caller location。
- `ActiveWait::begin()` / `begin_wait()` 的 nested active wait assert 必须包含 current task、已有 wait id、已有 wait caller location 和当前 begin caller location。
- `schedule_wait_with_timeout()` 是合法 explicit wait-sleep 点，阶段 0 不在其中调用 no-nested-wait helper。
- worker 若需要越过上述 write set，必须停止并上报 expansion request。

**Validation floor for 阶段 0：**

- `rg -n "Location::caller|assert_current_not_in_active_wait|begin_wait|ActiveWait::begin|Latch::begin_current|prepare_listener|Mutex cannot" anemone-kernel/src/sched anemone-kernel/src/sync`
- `rg -n "WaitPrimitive|operation:|caller_tag|ScheduleCaller" anemone-kernel/src/sched anemone-kernel/src/sync`
- `git diff --check`
- `just build`

**Review gate：** implementation worker 完成后，独立 review subagent 必须按阶段 0 RFC gate 审查：diagnostic-only origin 不驱动行为、nested wait assert 信息完整、`Mutex::lock()` 检查顺序符合要求、未越界修改 source owner、未引入手写 taxonomy、未改变 scheduler entry semantics。

**Validation:** 本 checkpoint 是 docs / audit 启动记录；代码实现和 build gate 尚未运行。

### 2026-07-06 - 阶段 0 Single-Active-Wait 诊断实现与 Review Gate

**阶段：** 阶段 0 - implementation / review。

**Subagents：**

- Implementation worker `Averroes` 执行代码修改，明确限制在阶段 0 write set 内。
- Review worker `Dalton` 执行只读 code review gate，使用 Anemone code review levels。

**代码变更：**

- `sched/wait.rs` 为 `WaitState` 增加 private diagnostic-only `WaitOrigin`，集中保存 creator tid、creation timestamp 和 `begin_caller: &'static Location<'static>`。`begin_caller` 由 `#[track_caller]` + `Location::caller()` 捕获；结构与字段旁已说明它不参与 wait identity、wake、park permission、source registration 或 scheduler mode。
- `ActiveWait::begin()` 和内部 `begin_wait()` 加 `#[track_caller]`。如果当前 task 已经处于 `TaskSchedState::Waiting`，begin-side assert 会报告 task id、existing wait id、existing begin caller location 和 new begin caller location。
- 新增 crate-internal `assert_current_not_in_active_wait()`，只读取 current task sched-state snapshot；若当前已有 active wait，会报告 task id、existing wait id、existing begin caller location 和 sleep-attempt caller location。
- `Mutex::lock()` 在 `allow_preempt()` assert 前先调用 no-nested-wait helper，并在 fast path compare-exchange 失败后、进入 `Event::listen_uninterruptible()` 前再次调用 helper。
- `Latch::begin_current()`、`Event::{listen, listen_uninterruptible, listen_with_timeout}`、`Event::prepare_listener()` 和 `wait_current_with_timeout()` 加 `#[track_caller]`，让 latch、event、clock sleep 和 signal direct wait 的 origin 指向更有意义的 caller site。
- `sched/mod.rs` 只增加窄 `pub(crate)` helper re-export，没有暴露 `WaitState` 内部状态结构。

**边界：**

- 未修改 arch trap、scheduler entry split、fs source owner、timer/signal/futex caller 或 QEMU / LTP profile。
- 未在 `schedule_wait_with_timeout()` 内调用 no-nested-wait helper；它仍是当前 active wait round 的合法 explicit wait-sleep 点。
- 未新增 explicit caller-location 参数、operation 字符串、`WaitPrimitive`、`ScheduleCaller`、`caller_tag` 或 source-local parkability truth。`rg` 只发现 `event.rs` 中既有 `operation: &str`，它属于旧的 unexpected-outcome diagnostic，不是本阶段新增 taxonomy。

**Review gate：**

- 初审 finding：`Keter` - `wait_current_with_timeout()` 作为 direct wait adapter 未标 `#[track_caller]`，会让 clock/signal wait origin 退化为 helper body location。
- 修复：implementation worker 只在 `wait_current_with_timeout()` 增加 `#[track_caller]`，未增加 explicit 参数或 taxonomy。
- Closure review：无 Apollyon / Keter / Euclid finding。reviewer 确认 previous Keter closed，`WaitState.begin_caller` 仍是 diagnostic-only，nested begin/helper panic payload 满足阶段 0 要求，`Mutex::lock()` 检查顺序符合 gate。
- User feedback follow-up：implementation worker 将 `created_by`、`created_at` 和 `begin_caller` 整理为 `WaitOrigin`；独立 reviewer 确认 `WaitOrigin` 只作为 private diagnostic metadata，唯一非 debug 使用是读取 `begin_caller()` 生成 panic 诊断，无新增行为状态或 caller taxonomy。

**Validation:**

```sh
rg -n "WaitOrigin|Location::caller|assert_current_not_in_active_wait|begin_wait|ActiveWait::begin|Latch::begin_current|prepare_listener|wait_current_with_timeout|Mutex cannot" anemone-kernel/src/sched anemone-kernel/src/sync
rg -n "WaitPrimitive|operation:|caller_tag|ScheduleCaller" anemone-kernel/src/sched anemone-kernel/src/sync
rg -n "schedule\(\)" anemone-kernel/src
rg -n "Latch::begin_current" anemone-kernel/src
rg -n "\.listen\(|\.listen_with_timeout\(" anemone-kernel/src
git diff --check
mdbook build docs
just build
```

结果：`git diff --check` clean；`mdbook build docs` 通过；`just build` 通过，只保留 build wrapper 的 cargo cache warning。`schedule()` / `Latch::begin_current()` / `Event::listen*()` 搜索结果与阶段 0 caller 分类一致。agent 未运行 QEMU / LTP。

**结论：** 阶段 0 已关闭。未触发 write set expansion，也未发现需要回到 RFC review 的 accepted-contract 问题。后续可在阶段 0 checkpoint commit 后进入阶段 1。
