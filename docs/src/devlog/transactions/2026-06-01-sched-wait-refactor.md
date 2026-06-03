# 2026-06-01 - Sched Wait Refactor

**Status:** Active
**Owners:** doruche, Codex
**Area:** scheduler / event / timer / signal / wait core
**Canonical Plan:** [RFC-20260601-sched-wait-refactor](../../rfcs/sched-wait-refactor/index.md), [Invariants](../../rfcs/sched-wait-refactor/invariants.md), [Implementation Plan](../../rfcs/sched-wait-refactor/implementation.md)
**Current Phase:** phase 5 complete; phase 6 verification pending

## Scope

本事务跟踪 `Event` wake race 对应的 scheduler wait 重构，从迁移前计划审查开始，直到 wait identity、统一 wait completion、stale-safe placement、park latch、mode-blocked listener requeue、timeout/signal/cancel 旁路收口和验证证据全部闭合。

非目标：

- 不重写调度策略、调度类或时间片策略。
- 不一次性完成 futex PI、poll/epoll 完整语义或 Linux waitqueue 全功能兼容。
- 不通过放宽 `task_enqueue()` 断言掩盖竞态。

## Invariants

- 一轮等待必须有稳定 `WaitState` 身份，旧 wake token 不能完成新 wait。
- event wake、timeout、signal 和主动 cancel 必须竞争同一个 wait core 状态。
- 逻辑 wake/cancel 与 task sched-state 更新必须有唯一线性化点。
- wake 成功后的 physical placement 必须由 wait core 触发 stale-safe `wake_enqueue()`。
- `Event` 只维护 listener 队列和 exclusive 策略，不直接修改 task 调度状态。
- mode-blocked listener 回挂必须通过短寿命 permit 再校验。
- 关键状态转换和异常分支必须保留可打开的 debug/trace 观测点。

## Phase Log

### 2026-06-01 - 迁移前计划收口

**Phase:** pre-implementation planning

**Change:** 将原 wait/wake `WaitState/WakeToken` 计划提升为公开 RFC 目录：[RFC-20260601-sched-wait-refactor](../../rfcs/sched-wait-refactor/index.md)。RFC 内部拆分为 canonical 不变量需求文档和迁移实施计划。实施计划明确阶段边界：第 1 阶段只建立 wait core 骨架；`wake_wait()` / `wake_active_wait()` 在阶段 2 补齐 stale-safe placement 和 park latch 前不得接入生产等待路径。

**Audit:** 复审重点是实施顺序是否会制造半套协议。结论是：必须避免“逻辑 wake 已完成，但 placement 仍由 Event/timer/signal 适配层补做”的中间态。计划已补充阶段前置条件、旁路审计和 `update_status_with()` 收口要求。

**Observability:** 实施计划新增可观测性要求：wait core、`wake_enqueue()`、Event publish、mode-blocked requeue、timeout/signal/cancel 关键分支都要保留 debug/trace 记录点。日志字段至少能关联 task id、wait identity、reason、mode、状态转换结果和 placement 结果。

**Validation:** 本阶段是文档与协议审查，未运行构建或 QEMU 验证。

**Next:** 开始阶段 1 前，先确认旧等待路径仍完整保留，新 wait core 骨架不会被生产路径误用；阶段 1 完成后把旁路分类结果记录到本事务日志或对应 progress 文件。

### 2026-06-01 - 阶段 1 wait core 骨架

**Phase:** phase 1 - scheduler wait core skeleton

**Change:** 新增 `sched::wait` 模块，建立 `WaitState`、`WakeToken`、`WaitGuard`、`BeginWait`、`WaitReason`、`WakeMode`、`WaitResult`、`WaitOutcome` 和 `WakeResult` 等 wait-core 类型。`WaitState` 保存本轮等待的稳定指针身份、状态、创建 task 和创建时间，不持有强 `Arc<Task>` 或 `Event` 回指。`WaitGuard` 不实现 clone，`WakeToken` 只暴露 wait identity 诊断和指针身份比较。

**Change:** 将 `Task` 的内部状态字段从旧 `TaskStatus` 换成 `TaskSchedState`。新状态区分 `Runnable`、带 `Arc<WaitState>` 和 `ParkState` 的 wait-core `Waiting`、迁移期 `LegacyWaiting` 以及 `Zombie`。`task.status()` 保持只读兼容投影，观察者仍只看到旧 `TaskStatus`。

**Change:** 增加 `begin_wait()`、`cancel_wait()`、`finish_wait()` 的阶段 1 事务骨架：它们通过 `Task::update_sched_state_with()` 在同一个 NoIrq 调度状态事务中写入或清理 wait-core `TaskSchedState::Waiting`，并在 debug 日志中记录 task id、wait identity、reason 和 outcome。

**Compatibility:** `Task::update_status_with()` 保留为迁移期兼容写入口，输入输出仍是旧 `TaskStatus`，但内部会投影到 `TaskSchedState::LegacyWaiting` / `Runnable` / `Zombie`。现有 Event、timeout、signal、exit 等生产路径继续完整走旧协议，没有被接到新 wait core。

**Compatibility:** `wake_wait()` 和 `wake_active_wait()` 只作为 `sched::wait` 内部受控骨架存在，返回 `WakeResult::DisabledUntilWakePlacement`，没有 re-export 给外部生产路径。阶段 2 完成 stale-safe `wake_enqueue()` 和 park latch 前，不允许把它们接入 Event、timeout、signal 或 cancel 路径，避免出现“逻辑 wake 已完成但 physical placement 未由 wait core 负责”的半套协议。

**Audit:** 搜索 `begin_wait()`、`cancel_wait()`、`finish_wait()`、`wake_wait()`、`wake_active_wait()` 的调用点，确认新 wait-core API 只在 `sched::wait` 内定义，未被 Event、timeout、signal 接入。旧旁路仍集中在 `try_to_wake_up()`、`notify()`、`schedule_with_timeout()` 和生产路径的 `update_status_with()` 调用上，后续阶段继续按计划迁移。

**Validation:** 运行 `just build` 通过。构建目标为当前配置的 LoongArch64 kernel release + `fs_ext4` + `kunit`，只剩既有 `sync/mono.rs` unused import warning。

**Next:** 进入阶段 2 前，需要新增 stale-safe `wake_enqueue()`，把 `schedule()` 的 park latch / abort-park 接入 `TaskSchedState::Waiting { park, .. }`，并把 `wake_wait()` / `wake_active_wait()` 补齐为“逻辑完成提交后由 wait core 执行一次 stale-safe placement”的完整入口。

### 2026-06-01 - 阶段 2 stale-safe wake placement

**Phase:** phase 2 - stale-safe wake placement

**Change:** 为 wait core 补齐 `WakeEnqueueResult`，结果分成 `Stale`、`AlreadyCurrent`、`ParkPending`、`AlreadyQueued` 和 `Enqueued`。`wake_enqueue()` 现在作为单独的物理 placement 入口，保留 `task_enqueue()` 的严格断言不变，只让它继续服务新建 runnable 或非 wake-tail 物理入队路径。

**Change:** `wake_wait()` 和 `wake_active_wait()` 现在执行完整的 wait-core 事务：在 task sched-state 事务里校验 `WaitState` 身份、`WakeMode` 和当前等待状态，完成 `WaitState` 的逻辑提交后，再在释放 task sched-state lock 后调用 `wake_enqueue()`，并把 placement 结果装入 `WakeResult::Woke { placement }`。

**Change:** `TaskSchedState::Waiting` 继续携带 `ParkState`，`schedule()` 现在会把 `PrePark` 提交成 `Parked`，再重读调度状态；如果这轮 wait 已经在 wake / cancel 事务里回到 `Runnable`，调度器会走 abort-park 路径而不是把已完成的 wait 留成悬挂态。

**Change:** 远端 wake 也改成走 stale-safe placement。`WakeUpTaskStaleSafe` IPI 只负责把 placement 请求送到目标 CPU，handler 里调用 `wake_enqueue()`，并把 `WakeEnqueueResult` 回传给发送端；旧 `WakeUpTask` IPI 和 `task_enqueue()` 仍保留给非 wait-tail 的兼容路径。

**Compatibility:** `Task::update_status_with()`、`try_to_wake_up()`、`notify()`、`schedule_with_timeout()`、`clock_nanosleep()` 和 `rt_sigtimedwait()` 仍处于迁移期兼容路径，没有改成新 wait-core 入口。阶段 2 没有把 Event、timeout 或 signal 的生产调用切到 `wake_wait()` / `wake_active_wait()`，这会留到后续阶段再接。

**Validation:** 运行 `just build` 通过。当前仍只有仓库里既有的 `sync/mono.rs` unused import warning。

**Next:** 阶段 3 开始迁移 Event listener identity、publish、exclusive quota 和 mode-blocked requeue；那一阶段才会把生产 Event 路径真正接到 `wake_wait()`，并开始收紧旧 listener 兼容层。

### 2026-06-01 - 阶段 2 代码审查与阶段 3 前置边界

**Phase:** phase 2 review / phase 3 readiness

**Audit:** 对阶段 1、阶段 2 两个提交做代码审查后，结论是当前 phase 2 skeleton 没有把生产等待路径接到新 wait core，因此下列问题不是当前运行路径的直接回归。它们主要来自迁移中间态：旧 `Event`、timeout、signal 和主动 cancel 仍走 `TaskStatus` / `notify()` / `try_to_wake_up()` 兼容协议，而新 wait core 已经具备完整 `WaitState` identity 和 stale-safe placement 入口。

**Blocker:** `Task::update_status_with()` 仍会把内部 `TaskSchedState::Waiting { state, .. }` 投影成旧 `TaskStatus::Waiting`，再写回 `TaskSchedState::from_legacy_status(status)`。如果阶段 3 只把 `Event` 接到 wait core，而阶段 4 的 signal/timeout 仍走旧 `notify()` / `try_to_wake_up()`，旧路径可以擦掉 wait identity、绕过 `WakeToken` 匹配、绕过 `WaitState` completion，并裸调用 `task_enqueue()`。这不会随着“只完成阶段 3”自然消失；阶段 3 前必须给 `update_status_with()` 加硬护栏，或把 phase 3/4 合并成不会暴露双 completion 协议的一次性迁移。首选边界是：legacy 写入口只能处理 `LegacyWaiting` / `Runnable` / `Zombie`，遇到 wait-core `Waiting` 必须拒绝、断言或显式转入 wait-core wake/cancel 入口。

**Blocker:** `sched::wait` 的完整入口已经通过 `sched::mod` crate-wide re-export。当前搜索确认 `begin_wait()`、`wake_wait()`、`wake_active_wait()` 仍无生产调用点，但阶段 2 的语义只是“后续阶段可受控接线”，不是“任意模块现在可半迁移”。阶段 3 接线前需要收紧可见性，或添加明确的阶段 guard / debug assert，避免 Event、timeout、signal 中某条旧旁路提前接入 wait core 后又被旧 completion 路径覆盖。

**Boundary:** `cancel_wait()` 和 `finish_wait()` 目前可以把匹配的 wait-core state 改回 `Runnable`，但不执行 `wake_enqueue()`。如果它们只服务当前 waiter 拥有的主动 cleanup，这个语义可以成立；如果后续被 timeout、signal 或其他线程当作异步 completion，会产生 runnable-but-not-queued 状态。阶段 3/4 前需要用注释、可见性或类型边界固定：异步或远端完成只能走 `wake_wait()` / `wake_active_wait()`；`cancel_wait()` / `finish_wait()` 只服务 waiter-owned cleanup。

**Hardening:** `schedule()` 的 abort-park 路径提交 `Parked` 后会重读 task sched-state，但当前只判断是否已经变成 `Runnable`，没有确认该变化来自同一轮 `WaitState` 的正常 completion。这个问题通常依赖 legacy 覆盖或异常状态触发，不单独阻塞 phase 2；阶段 3 接线时应把重读结果区分为 `Runnable`、同一 wait 仍 `Waiting`、不同 wait / `LegacyWaiting` / 异常状态，并为异常分支保留 debug log 或 debug assert。

**Decision:** 可以把当前代码作为 phase 2 skeleton 保留；不需要为了封存 phase 2 立即修所有问题。但阶段 3 不能在 `update_status_with()` 护栏和 wait-core API 接线边界未明确前接入生产 Event。`cancel_wait()` / `finish_wait()` 的语义边界和 `schedule()` abort-park 诊断可与阶段 3 hardening 同批完成。

### 2026-06-01 - 阶段 3 前置护栏与调度重读加固

**Phase:** phase 3 prerequisites / hardening

**Change:** 给 `Task::update_status_with()` 加了 wait-core `TaskSchedState::Waiting` 护栏。只要当前内部状态已经是 wait-core waiting，legacy 写入口就会直接 panic，不再允许旧 `TaskStatus` 兼容路径静默覆盖 `WaitState` identity。这个护栏把旧 completion 旁路和新 wait-core completion 线分开了，避免阶段 3 只接 Event 时被阶段 4 的 timeout/signal 旧路径绕回去。

**Change:** 收紧了 wait-core 生产接线边界。`begin_wait()`、`wake_wait()`、`wake_active_wait()` 不再从 `sched::mod` crate-wide re-export，调用方必须显式进入 `sched::wait` 模块。与此同时，`sched::wait` 里的文档补上了阶段语义：`begin_wait()` 是受控 wait-core 入口，`wake_wait()` / `wake_active_wait()` 是远端或 producer completion 入口，不能和 `cancel_wait()` / `finish_wait()` 的 waiter-owned cleanup 混用。

**Change:** `cancel_wait()` 和 `finish_wait()` 的文档边界被固定为 waiter-owned cleanup only。异步或远端 completion 需要继续走 `wake_wait()` / `wake_active_wait()`，这样逻辑完成和 stale-safe physical placement 才会一直绑在同一个 wait-core 事务里。

**Change:** `schedule()` 的 abort-park 重读路径现在会携带 `WaitState` 身份快照，并把重读结果拆成明确分支：`Runnable` 走 abort-park；同一轮 wait 仍 `Waiting` 时记录正常 parked；不同 wait、`LegacyWaiting` 或 `Zombie` 会打警告并保留 debug assert。这个加固只做诊断和边界收敛，没有改 Event 或 timeout 的生产迁移。

**Validation:** 运行 `just build` 通过。当前仍只有仓库里既有的 `anemone-kernel/src/sync/mono.rs` unused import warning。

**Next:** 阶段 3 可以开始迁移 Event listener identity、publish、exclusive quota 和 mode-blocked requeue；进入生产接线前，上面两条前置和这条 hardening 已经到位。

### 2026-06-01 - 阶段 3 Event wait-core 接线

**Phase:** phase 3 - Event wait-core adapter

**Change:** `Event` listener 现在保存 `WaitTarget { task: Weak<Task>, token: WakeToken }`，listener 相等性改为 `WakeToken::same_wait()` 的 wait identity 比较。`Event::listen*()` 每轮等待先通过 `sched::wait::begin_wait()` 建立 `WaitState`，再注册 listener；predicate ready、signal precheck、timeout zero 等主动退出路径改为 `cancel_wait()`，返回路径统一按 wait identity 清理 listener 并 `finish_wait()` 退役本轮等待。

**Change:** `Event::publish()` 不再调用 `try_to_wake_up()`，也不直接写 `TaskStatus` 或补做 runqueue placement。publish 只从 Event 队列 detach listener，在 event lock 外升级 weak task 并调用 `wake_wait()`；`WakeResult::Woke` 只更新 successful exclusive quota 和诊断，placement 已由 wait core 完成。stale、already completed、already cancelled 和 retired listener 会被丢弃。

**Change:** exclusive quota 改为只按成功完成的 wait 计数。非独占队列和独占队列都按本轮 publish 开始时的原始节点数设置扫描上限，本轮 mode-blocked 回挂到队尾的 listener 不会在同一轮 publish 中再次参与，避免回挂导致队列内无限旋转。

**Change:** 增加 wait-core `RequeuePermit` 和 `requeue_permit_if_mode_blocked()`，并在 Event 内部实现 `requeue_blocked_listener_if_current_armed()`。mode-blocked listener 只有在回挂前重新验证 task 当前 active wait 仍与 token 同一轮、`WaitState` 仍为 `Armed`、并且当前 `WakeMode` 仍被 interruptible 属性阻止时，才会按原队列类型回挂到尾部；验证失败时直接丢弃 detached listener。

**Temporary:** 为了把阶段 3 和阶段 4 分开提交，`notify()` 增加了一个临时桥接分支：先尝试 `wake_active_wait(task, WaitReason::Signal, mode)`，只有返回 `Stale` 时才继续处理 `LegacyWaiting`。这不是最终 signal 迁移形态；阶段 4 仍需要把 signal 路径本身收敛到 wait-core 语义、清理旧 `notify()` / `TaskStatus` completion 边界。

**Temporary:** `Event::listen_with_timeout()` 现在使用 Event-local token timer callback：timer callback 持有本轮 `WakeToken` 并调用 `wake_wait(..., WaitReason::Timeout, WakeMode::AnyWait)`，避免已迁移的 Event wait 被旧 `schedule_with_timeout()` / `notify()` 旁路写穿。这个 helper 是阶段 3/4 分离时的过渡设施，不应视为最终 timeout 架构；阶段 4 仍要迁移通用 `schedule_with_timeout()`、`clock_nanosleep()`、`rt_sigtimedwait()` 和主动 cancel 入口。

**Review Boundary:** 阶段 3 review 中观察到 `Event::listen*()` 在 wait-core `Completed(Signal | Force)` 或 `Completed(Timeout)` 返回后会重新检查 predicate，并可能把 signal/timeout completion 映射成普通成功返回。这个问题不破坏 wait identity、listener cleanup、exclusive quota、mode-blocked requeue 或 stale-safe placement，也不会重新打开原 Event wake race，因此不阻塞阶段 3。它也不会仅因阶段 4 开始而自然消失：阶段 4 迁移通用 timeout/signal 时必须显式决定并收口 `Event::listen*()` 的返回原因语义，是保留旧实现的“predicate wins”行为并更新契约，还是让 `Completed(Signal)` / `Completed(Timeout)` 严格返回 interrupted / timeout。

**Audit:** 搜索 `anemone-kernel/src/sched/event.rs` 中的 `try_to_wake_up`、`update_status_with`、`TaskStatus::Waiting` 和 `schedule_with_timeout`，确认 Event 生产路径已经不再使用旧 wake/status/timeout completion。剩余旧旁路集中在 `sched::notify()` 的 `LegacyWaiting` 分支、`try_to_wake_up()`、通用 `schedule_with_timeout()`、`clock_nanosleep()` 和 `rt_sigtimedwait()`，属于阶段 4/5 收口范围。

**Validation:** 运行 `just build` 通过。当前仍只有仓库里既有的 `anemone-kernel/src/sync/mono.rs` unused import warning。

**Next:** 阶段 4 需要迁移通用 timeout、signal 和主动 cancel，移除本阶段为了拆分提交而保留的 `notify()` wait-core bridge / Event-local token timer 过渡边界，并重新分类所有 `update_status_with()`、`try_to_wake_up()`、`notify()` 和 `schedule_with_timeout()` 命中。

### 2026-06-02 - 阶段 4 timeout / signal / cancel 收口

**Phase:** phase 4 - timeout, signal, and active cancel migration

**Change:** 新增通用 `schedule_wait_with_timeout(task, token, timeout)`。调用方必须先通过 `wait::begin_wait()` 发布本轮 `WaitState`，再把 `WakeToken` 交给 timeout helper；timer callback 只调用 `wake_wait(..., WaitReason::Timeout, WakeMode::AnyWait)`，晚到 callback 通过 wait identity 返回 stale / retired / already-completed 结果，不再依赖独立 `AtomicBool validness` 决定 timeout 是否有效。

**Change:** `clock_nanosleep()` 和 `rt_sigtimedwait()` 已从旧 `TaskStatus::Waiting` + `schedule_with_timeout()` 组合迁移到 wait core。signal precheck、timeout zero、predicate / pending-signal ready 等主动退出路径都通过 `cancel_wait()` 竞争本轮 wait，再由 `finish_wait()` 退役；真正异步 signal / force / timeout completion 统一由 `wake_active_wait()` 或 token-based timeout callback 完成。

**Change:** `Event::listen_with_timeout()` 移除了阶段 3 的 Event-local token timer callback 和独立有效位，改用同一个 `schedule_wait_with_timeout()`。阶段 3 review 中标出的返回语义也在本阶段收口：`Completed(Signal | Force)` 严格映射为 signal return，`Completed(Timeout)` 严格映射为 timeout return，不再在 completion 后重新检查 predicate 并把 signal / timeout 改写成普通成功。

**Change:** `notify()` 现在是 signal / force 的 wait-core producer 入口：先以 `wake_active_wait(task, WaitReason::Signal, mode)` 完成当前 active wait，并由 wait core 负责 stale-safe placement。`LegacyWaiting` 分支只保留为阶段 5 旁路审计前的迁移兼容尾巴，不再是 Event、timeout、`clock_nanosleep()` 或 `rt_sigtimedwait()` 的普通 completion 路径。

**Compatibility:** `schedule_with_timeout()` 仍保留为兼容 wrapper，但内部已经自行 `begin_wait()` / `schedule_wait_with_timeout()` / `finish_wait()`，不再要求调用方先写 `TaskStatus::Waiting`。阶段 4 结束时仓库内没有生产调用点继续使用它；阶段 5 可以决定删除、收窄可见性或继续作为明确分类的兼容 API。

**Audit:** 搜索 `schedule_with_timeout(`、`TaskStatus::Waiting`、`update_status_with`、`try_to_wake_up`、`notify(` 和 `task_enqueue(`。Event、timeout、signal、主动 cancel 的迁移路径已经不再写 `TaskStatus::Waiting`，也不再通过旧 `notify()` timeout callback 或裸 `task_enqueue()` 完成 wake tail。剩余旧命中分类为：`try_to_wake_up()` / `schedule_with_timeout()` 兼容 API 定义本身、`notify()` 的 `LegacyWaiting` 兼容分支、`update_status_with()` 的 exit zombie 写入、clone / bootstrap 的新任务 placement、procfs/status 观察投影，以及非 wait 协议的 itimer / futex / iomux 自有状态。

**Review Finding:** 阶段 4 审计发现 `clock_nanosleep()` 和 `rt_sigtimedwait()` 在 `schedule_wait_with_timeout()` 返回后，对 `finish_wait()` 的结果分类还不够 fail-closed。当前正常设计下，阻塞后的直接等待路径应只接受 `Completed(Timeout)`、`Completed(Signal | Force)`，以及调用方自己明确处理的 pending-signal ready；如果出现 `Armed`、`Cancelled`、`Retired` 或其他完成原因，说明 wait-core 协议或调用方状态机已经偏离预期。当前实现会把这些异常 outcome 折叠成普通 timeout / success 返回。这不是已观察到的用户态语义回归，也不重新打开原 Event wake race，但它会掩盖 wait-core 不变量破坏，应在阶段 5 旁路收缩前或同批改成显式 match、debug assert / warning、或受控 retry。

**Validation:** 运行 `just build` 通过。当前仍只有仓库里既有的 `anemone-kernel/src/sync/mono.rs` unused import warning。

**Next:** 阶段 5 进入旁路审计和旧 API 收缩：重点处理 `try_to_wake_up()`、`schedule_with_timeout()`、`notify()` 的 `LegacyWaiting` fallback、`Task::update_status_with()` 保留边界，以及 `task_enqueue()` 命中的非 wait-tail 分类说明。

### 2026-06-02 - 阶段 5 审计 1：旧兼容接口收缩

**Phase:** phase 5 audit 1 - bypass audit and legacy API removal

**Change:** 删除 `TaskSchedState::LegacyWaiting`，调度器不再接受无 wait identity 的 legacy waiting 状态。`schedule()` 的 park / abort-park 重读分支只处理 `Runnable`、同一轮 `Waiting`、不同轮 `Waiting` 和 `Zombie`；旧状态覆盖 wait-core park 的诊断分支随状态一起移除。

**Change:** 删除无生产调用点的 `try_to_wake_up()` / `WakeUpError` 和 `schedule_with_timeout()` 兼容 wrapper。timeout 用户必须显式 `begin_wait()` 后把 `WakeToken` 交给 `schedule_wait_with_timeout()`，signal / force 用户必须走 `notify()` -> `wake_active_wait()`，event / timer producer 必须走 `wake_wait()`。这次没有保留“以后再迁移”的旧 wrapper。

**Change:** 删除 `Task::update_status_with()` 和 `TaskSchedState::from_legacy_status()`。剩余的 exit zombie 写入改为直接 `update_sched_state_with()`，并在退出任务仍持有 active wait-core state 时记录 warning 和普通 `assert!`。`TaskStatus` 保留为 `task.status()` 和 procfs 等观察投影，不再是普通等待 begin/completion/cancel 写入口。

**Change:** 关闭阶段 4 review finding：`clock_nanosleep()` 和 `rt_sigtimedwait()` 对 `finish_wait()` outcome 改为显式 match。阻塞后只接受 `Completed(Timeout)`、`Completed(Signal | Force)`，以及 syscall 自己处理的 pending-signal ready；`Armed`、`Cancelled`、`Retired` 或其他 completion reason 会 warning + 普通 `assert!`，不再折叠成普通 timeout / success。

**Audit:** 搜索 `LegacyWaiting`、`update_status_with`、`from_legacy_status`、`try_to_wake_up`、`WakeUpError`、`schedule_with_timeout(` 和 `is_sleeping(`，确认内核源码已无命中。剩余 `TaskStatus::Waiting` 只在 wait-core 到旧观察状态的投影和 procfs/status 观察层。剩余 `task_enqueue()` 命中为新任务发布和调度器基础 placement；wait completion 尾巴只通过 `wake_enqueue()`。剩余 `notify()` 命中为 signal sender 调用和 `notify()` 自身的 wait-core active-wake 入口。

**Validation:** 运行 `just build` 通过。当前仍只有仓库里既有的 `anemone-kernel/src/sync/mono.rs` unused import warning。

**Boundary:** 这只是阶段 5 的第一次审计，不代表阶段 5 完成。当前只收缩了低成本、无生产调用点的旧兼容接口，并关闭阶段 4 direct-wait outcome 的 fail-closed 问题。后续阶段 5 审计仍需要继续检查 wait-core API 可见性、`notify()` signal 入口语义、`task_enqueue()` / `wake_enqueue()` placement 分类、`TaskStatus` 观察投影边界、异常状态 `assert!` 覆盖面，以及是否还有应该进一步收窄或删除的普通入口。

**Next:** 继续阶段 5 的后续审计轮次，直到所有保留旁路都有明确分类和理由；完成多轮审计后再进入阶段 6 验证和文档跟进。

### 2026-06-02 - 阶段 5 审计 2：Event outcome 与保留入口边界

**Phase:** phase 5 audit 2 - Event finish outcome and retained API boundary audit

**Audit:** 复核两轮审计结果后，确认以下问题与上一轮人工审计中的 Event fail-closed、wait-core API 可见性和 placement 入口分类问题重复，应合并记录，不再拆成独立 issue。它们不重新打开原始 Event wake race，也没有证明当前存在裸 `task_enqueue()` wait-tail 调用；风险在于阶段 5 还不能声明异常状态覆盖和保留入口收缩已经闭合。

**Finding (Medium):** `Event::listen()`、`Event::listen_uninterruptible()` 和 `Event::listen_with_timeout()` 对 `finish_wait()` outcome 仍未 fail-closed。阶段 5 审计 1 已把 `clock_nanosleep()` / `rt_sigtimedwait()` 改为显式分类，但 Event 侧仍会把除 `Completed(Signal | Force)` 或 `Completed(Timeout)` 以外的结果归入普通循环重试。如果出现 `Armed`、`Cancelled`、`Retired`，或当前 listener 不应收到的 completion reason，Event 会静默吞掉 wait-core 协议偏离。修复方向是让 Event 阻塞返回后显式 match outcome：只允许 `Completed(Event)` 作为普通 spurious/event wake 继续循环，按接口语义处理 signal/timeout，其余状态 warning + 普通 `assert!` 或受控 fail-closed。

**Finding (Medium):** `sched::wait` 仍是公开模块，`begin_wait()`、`wake_wait()`、`wake_active_wait()`、`cancel_wait()` 和 `finish_wait()` 等生命周期原语仍可被任意内核模块直接调用。阶段 5 审计 1 删除了旧 wrapper，但迁移证明仍依赖调用约定：后续代码可以绕开 listener cleanup 纪律创建 wait round，或把 `wake_active_wait()` 当成泛用 wake API。修复方向是把生命周期原语收窄到已知 adapter，或只公开带明确调用契约的窄 helper，避免 wait-core capability 边界退化成普通状态操作面。

**Finding (Low/Medium):** `local_enqueue()` / `remote_enqueue()` / `task_enqueue()` 当前调用面仍分类为新任务发布和调度器基础 placement，未发现 wait completion tail 调用；但 `local_enqueue()` 文档仍写着“for wakeup and newly-created runnable tasks”，与阶段 5 不变量中“wait completion tail 必须走 `wake_enqueue()`”的表述冲突。修复方向是把这些裸 enqueue API 的注释和分类改成明确的非 wait-tail placement / new-task placement，保留严格断言；把 wait completion 相关命名和文档只留给 stale-safe `wake_enqueue()`。

**Validation:** 本轮只记录审计结果，未改代码，未运行构建或 QEMU 验证。

**Next:** 阶段 5 后续修复应优先关闭 Event outcome fail-closed；随后收窄 wait-core 生命周期 API 与裸 enqueue placement 文档/可见性边界。修复后需要重新搜索 `wait::begin_wait` / `wake_wait` / `wake_active_wait` / `task_enqueue` / `local_enqueue` / `remote_enqueue` 命中并更新旁路分类。

### 2026-06-02 - 阶段 5 审计 2 修复：Event fail-closed 与入口收缩

**Phase:** phase 5 audit 2 fixes

**Change:** `Event::listen()`、`Event::listen_uninterruptible()` 和 `Event::listen_with_timeout()` 的阻塞返回路径改为显式分类 `finish_wait()` outcome。正常 Event wake 只接受 `Completed(Event)` 并继续按 spurious wake 语义重新检查 predicate；signal / force 和 timeout 按各接口返回语义处理；`Armed`、`Cancelled`、`Retired` 或非当前接口应收到的 completion reason 会记录 warning 并触发普通 `assert!`，不再静默归入普通循环重试。

**Change:** 新增 wait-core owner-side `ActiveWait` facade，并把 Event、`clock_nanosleep()`、`rt_sigtimedwait()` 迁移到 `ActiveWait::begin()` / `cancel()` / `finish()`。底层 `WaitGuard`、`BeginWait`、`WaitResult`、`begin_wait()`、`cancel_wait()` 和 `finish_wait()` 改为 `sched::wait` 私有实现细节；`sched::wait` 模块本身也从公开模块收回，只通过 `sched` 根导出必要的受控类型和窄 capability。

**Change:** `local_wake_enqueue()` / `remote_wake_enqueue()` 不再从 `sched` 根导出。`local_enqueue()`、`remote_enqueue()` 和 `task_enqueue()` 的文档改为严格非 wait-tail placement / 新任务发布路径，明确 wait completion tail 必须走 stale-safe `wake_enqueue()`，并保留现有 runnable 断言。

**Audit:** 重新搜索 `wait::begin_wait`、`wait::cancel_wait`、`wait::finish_wait`、`wait::wake_wait`、`wait::wake_active_wait`、`BeginWait`、`WaitGuard` 和 `WaitResult`。raw lifecycle 原语只剩 `sched::wait` 内部定义和 Event / timer / notify 这些 scheduler 内部 adapter 的 producer 入口；外部直接等待路径只使用 `ActiveWait`。重新搜索 `TaskStatus::Waiting`、`update_status_with`、`try_to_wake_up`、`schedule_with_timeout(` 和 `is_sleeping(`，内核源码中仍无旧 completion API 命中，`TaskStatus::Waiting` 只剩 wait-core 投影和 procfs/stat 观察。重新搜索 `task_enqueue()` / `local_enqueue()` / `remote_enqueue()` / `wake_enqueue()`，wait completion tail 仍只通过 wait core 调用 `wake_enqueue()`；裸 enqueue 命中为 IPI strict wake payload、新任务发布和调度器内部定义。

**Validation:** 运行 `just build` 通过。当前仍只有仓库里既有的 `anemone-kernel/src/sync/mono.rs` unused import warning。

**Next:** 阶段 5 继续后续旁路审计，重点检查剩余 `notify()` active-wake 语义、`WakeUpTask` IPI strict placement 分类、`TaskStatus` 观察投影边界、异常状态 `assert!` 覆盖面和 debug/trace 字段完整性。阶段 5 仍未整体收口。

### 2026-06-02 - 阶段 5 审计 3：保留入口分类与阶段 5 收口

**Phase:** phase 5 audit 3 - retained entry classification and closeout

**Finding:** `notify(task, uninterruptible=true)` 使用 `WakeMode::Force` 允许 SIGKILL / SIGSTOP 等强制通知完成不可中断 wait，但 completion reason 仍写成 `WaitReason::Signal`。这不会绕过 wait core，也不会重开旧 wake-tail race；风险是 `finish_wait()` outcome 把 force wake 伪装成普通 signal，导致 Event、timeout 或 direct wait 的 fail-closed 分类无法从日志和结果中区分强制完成。

**Change:** `notify()` 现在按语义同时选择 reason 和 mode：普通 signal 使用 `WaitReason::Signal` + `WakeMode::InterruptibleOnly`，强制通知使用 `WaitReason::Force` + `WakeMode::Force`。日志字段补充 reason 和 mode，方便把 signal 被 interruptible 阻止、force wake 成功和 stale/no-active-wait 分支区分开。

**Audit:** 重新分类 `notify()` 调用面。当前 signal sender 仍是唯一调用者；普通未屏蔽 signal 通过 `wake_active_wait()` 只完成 interruptible wait，SIGKILL / SIGSTOP 通过 force mode 完成当前 active wait。若 task 没有 active wait，`notify()` 只记录 stale/no-active-wait，不写 `TaskSchedState`，不调用裸 enqueue。

**Audit:** 重新分类 `WakeUpTask` / `WakeUpTaskStaleSafe` IPI 和 enqueue 入口。`WakeUpTask` 只由 `remote_enqueue()` 发出，属于 strict non-wait-tail placement；handler 中的 `local_enqueue()` 保留 runnable 断言。wait completion 远端尾巴只经 `wake_enqueue()` -> `remote_wake_enqueue()` -> `WakeUpTaskStaleSafe`，handler 会重新执行 stale-safe placement 并回传 `WakeEnqueueResult`。

**Audit:** 重新分类 `TaskStatus::Waiting`、旧 completion API 和直接等待路径。`TaskStatus::Waiting` 只剩 `TaskSchedState::as_task_status()` 观察投影和 procfs/stat 状态字符映射；`update_status_with`、`try_to_wake_up`、`schedule_with_timeout(`、`LegacyWaiting` 和 `is_sleeping(` 在内核源码中无命中。Event、`clock_nanosleep()` 和 `rt_sigtimedwait()` 都通过 `ActiveWait` owner-side facade 开始、主动 cancel 和 finish，异常 `WaitOutcome` 分支均 warning + 普通 `assert!`。

**Audit:** 审计 debug/trace 覆盖面后，阶段 5 需要的关键字段已经可从当前记录点恢复：wait core begin/cancel/finish/wake 记录 task、wait identity、reason、mode 和 result；`wake_enqueue()` / IPI placement 记录 task、park 和 placement；Event publish、detach、quota、discard、mode-blocked requeue 记录 event、listener/wait identity、queue 和结果；timeout callback 与 direct wait finish 记录 task、wait identity 或 outcome、剩余时间和 result。默认路径仍使用 debug 级记录，未新增高频 notice 级刷屏。

**Validation:** 运行 `just build` 通过。当前仍只有仓库里既有的 `anemone-kernel/src/sync/mono.rs` unused import warning。

**Decision:** 阶段 5 的旁路审计到此收口。保留入口都有明确分类：`notify()` 是 signal/force active-wake producer，`wake_wait()` 是 token producer，`ActiveWait` 是 waiter-owned lifecycle facade，`task_enqueue()` / `local_enqueue()` / `remote_enqueue()` 是 strict non-wait-tail placement，`TaskStatus` 是观察投影。下一阶段不再继续阶段 5 issue-hunting，转入阶段 6 验证和证据沉淀。

### 2026-06-02 - shmat01 panic 修复：不可中断 Event 的 Force 分类

**Phase:** phase 6 validation follow-up

**Finding:** `build/shmat01-panic.log` 显示 `shmat01` 运行中，task 在 `Event::listen_uninterruptible()` 内收到 `SIGKILL` 触发的 `WaitReason::Force`，wait core 正常完成 active wait 并返回 `Completed(Force)`；随后 Event adapter 把该 outcome 归入 unexpected 分支并在 `event.rs:339` panic。该现象不破坏 wait identity、listener cleanup、mode-blocked requeue 或 stale-safe placement；问题是 Event 不可中断接口在阶段 5 fail-closed 修复后仍遗漏了 force completion 的合法分类。

**Change:** `Event::listen_uninterruptible()` 现在接受 `Completed(Force)` 作为合法非 predicate wake，并回到循环重新检查 predicate。它不会把 Force 当作 predicate success 直接返回，因为该 API 没有 interrupted return，且现有调用点（内核 mutex、vfork wait）都依赖“返回时等待条件已经成立”。普通 `Completed(Signal)` 仍保持 unexpected，因为 `WakeMode::InterruptibleOnly` 不应完成不可中断 wait。

**Validation:** 运行 `just build` 通过。当前仍只有仓库里既有的 `anemone-kernel/src/sync/mono.rs` unused import warning。

### 2026-06-03 - TaskStatus 观察投影边界收紧

**Phase:** phase 6 documentation and retained-boundary follow-up

**Decision:** `TaskStatus` / `task.status()` 继续保留，但只作为 procfs、debug 和一次性状态观察接口。它是从 `TaskSchedState` 投影出的有损 snapshot，不携带 wait identity 或 park latch，不再作为调度、wait、wake、enqueue 路径的协议判断入口。

**Change:** `TaskStatus` 和 `Task::status()` 的代码注释改为 observation-only compatibility snapshot。新增 `TaskSchedState::is_runnable()` 与 `Task::is_sched_runnable()`，让调度 placement / assertion 路径直接检查内部 scheduler state，而不是通过 `TaskStatus` 投影判断 runnable。

**Change:** `local_enqueue()`、`local_requeue_current()`、`local_pick_next()`、`remote_enqueue()`、`task_enqueue()`、`wake_enqueue()`、`local_wake_enqueue()`、`remote_wake_enqueue()`、bootstrap first enqueue 和 `yield_now()` 已从 `task.status() == TaskStatus::Runnable` 改为使用 scheduler-state helper 或直接记录 `TaskSchedState` snapshot。stale wake 日志字段也从 `status={:?}` 改为 `sched_state={:?}`，避免把兼容投影当成诊断真相。

**Docs:** RFC 不变量和实施计划同步收紧：`task.status()` 只能作为观察接口，不得作为 wait core 写入口，也不得作为调度内部协议判断入口；阶段 5 验收增加 `TaskStatus` 命中分类要求，要求调度内部 runnable / waiting / zombie 判断使用 `TaskSchedState` helper 或事务。

**Audit:** 搜索 `get_current_task().status`、`task.status()`、`.status() == TaskStatus`、`.status() != TaskStatus` 和 `TaskStatus::Runnable`，`sched` 内部已无通过 `Task::status()` 判断 runnable 的命中。剩余 `TaskStatus` 命中为 `TaskSchedState::as_task_status()` 观察投影、procfs/stat 和 procfs/status 映射。

**Validation:** 运行 `just build` 通过。当前仍只有仓库里既有的 `anemone-kernel/src/sync/mono.rs` unused import warning。

## Open Items

- 阶段 6：运行已知触发 profile，并保存带 debug/trace 的验证摘要。

## Closure

事务尚未收口。完成时需要记录最终验证命令、late wake / stale placement / mode-blocked requeue / timeout-signal 竞争的观测证据，以及剩余限制或 register 链接。
