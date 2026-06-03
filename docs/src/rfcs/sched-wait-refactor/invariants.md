# Sched Wait Refactor 不变量需求

日期：2026-06-01

状态：RFC canonical 需求文档。本文只定义必须保持的协议边界和不变量；具体落地顺序见 [Sched Wait Refactor 迁移实施计划](./implementation.md)。

来源：

1. [Event Wake Race 问题简述](./background/event-wake-race-problem-brief.md)
2. [Event WaitState/WakeToken 问题清单](./background/event-waitstate-waketoken-issues.md)
3. [Event wake-tail 条件入队窄化修复草案（历史备选）](./background/event-try-task-enqueue-narrow-fix.md)
4. [Event WaitState/WakeToken 2026-06-01 单文件归档](./background/event-waitstate-waketoken-plan-monolith.md)

## 0. 闭合条件

本次迁移的目标是把 `Event` wake race 的理论模型落成调度等待协议，而不是重写调度策略。

迁移完成后必须同时满足：

1. 一轮等待有稳定身份，旧轮次 wake 不能完成新轮次 wait。
2. event wake、timeout、signal 和主动 cancel 竞争同一个等待状态。
3. 逻辑 wake / cancel 与 task 调度等待状态更新有唯一线性化点。
4. wake 成功后的物理入队由 wait core 统一触发 stale-safe placement。
5. `Event` 只维护 listener 队列和 exclusive 策略，不直接修改 task 调度状态。
6. mode-blocked listener 回挂必须在回挂前再次确认仍对应当前 armed wait。

如果任一条件不成立，当前实现只能视为迁移中间态，不能声明协议闭合。

## 1. 非目标

本需求不包含：

1. 重写调度算法、调度类或时间片策略。
2. 引入跨 CPU 负载均衡或远程 runqueue 观察新策略。
3. 一次性完成 futex PI、poll/epoll 完整语义或 Linux waitqueue 全功能兼容。
4. 把所有同步原语重构成同一种高层抽象。
5. 通过放宽 `task_enqueue()` 断言掩盖竞态。

这里要固定的是 wait/wake 协议边界，不是 scheduler policy。

## 2. 状态所有权

`Task` 的等待状态必须由同一事务保护的 `TaskSchedState` 表达。

建议形状：

```rust
enum TaskSchedState {
    Runnable,
    Waiting {
        state: Arc<WaitState>,
        interruptible: bool,
        park: ParkState,
    },
    Zombie,
}

enum ParkState {
    PrePark,
    Parked,
}
```

硬性要求：

1. `TaskSchedState` 必须由同一个 `NoIrqRwLock<TaskSchedState>` 或等价 NoIrq 事务保护。
2. 外部路径不得分别维护 `TaskStatus`、active wait 和 park latch 三套真相。
3. `Waiting { state, .. }` 表示当前 task 只属于这一轮等待；`Runnable` / `Zombie` 状态下不存在 active wait。
4. `ParkState` 只闭合 wait / schedule / wake-tail 的时序，不表示第二套等待状态。
5. park 状态必须和 wait identity 一起在同一个调度事务内翻转。
6. `task.status()` 迁移后只能作为兼容观察接口，从 `TaskSchedState` 投影出旧 `TaskStatus` 视图，不得作为 wait core 写入口，也不得作为调度内部协议判断的普通入口。

## 3. 等待轮次身份

`WaitState` 是一轮等待的唯一生命周期实体，不能复用给下一轮。

`Arc<WaitState>` 的指针身份就是等待轮次身份。所有身份比较必须使用 `Arc::ptr_eq` 语义；不得依赖字段相等，不得用 task tid 代替 wait identity，也不要给 `WaitState` 实现会被误用为身份判断的结构化 `PartialEq`。

`WaitState` 的最低语义：

1. 稳定身份，即 `Arc<WaitState>` 指针身份。
2. 当前状态：`Armed`、`Completed(reason)`、`Cancelled(reason)`、`Retired`。
3. 完成原因：event、timeout、signal、force、predicate ready、guard drop 等。
4. 调试元数据可以 by-value 保存，例如创建 tid、创建点、等待模式和时间戳。

`Armed -> Completed/Cancelled` 转换必须在 task 调度等待事务中提交。`WaitState` 可以保存完成原因，但不能成为绕过 `TaskSchedState::Waiting { state, .. }` 校验的第二套完成真相。`WaitState` 不应持有强 `Arc<Task>` / `Event` 回指，避免形成强引用环。

## 4. 受限能力

`WakeToken` 是事件源持有的受限唤醒能力。

它只表达：

1. 目标等待轮次身份。
2. 对该轮次尝试完成的能力。

它不表达：

1. listener 在 event 队列中的位置。
2. task 当前是否真的还在等待。
3. runqueue placement。
4. 第二套 wake state machine。

事件源拿到 token 后只能调用 wait core 提供的完成接口，不得直接写 `TaskStatus`，也不得直接调用裸 `task_enqueue()`。

`WaitGuard` 由 waiter 持有，用于主动 cancel、读取最终完成原因和退役本轮等待。正常路径应显式调用 `finish_wait()` 或等价接口；析构只能作为兜底 cleanup，不能隐藏主要语义。

## 5. 模块边界

1. `sched::wait` 或等价 wait core 拥有等待事务、完成权、退役语义、wake 成功后的 stale-safe placement 触发权，以及 requeue permit 的签发权。
2. `Event`、timeout、signal、exit 只作为适配器调用 wait core，不各自维护一套可完成状态机。
3. `TaskStatus` 兼容读接口只允许 procfs、debug 和一次性状态观察使用，不允许成为新的写入口或调度内部协议判断入口。
4. 新 helper 应优先放在 wait core 边界上，不能在 `Event`、timer 或 signal 模块里重复实现状态切换。
5. wait core 不应执行 Event 提供的任意闭包；跨 event / task 的特殊顺序必须通过不可外部构造的短寿命 capability 表达。

## 6. 核心 API 语义

命名可以随实现微调，但语义边界不得改变。

```rust
pub enum WaitReason {
    Event,
    Timeout,
    Signal,
    Force,
    PredicateReady,
    Cancelled,
}

pub enum WakeMode {
    InterruptibleOnly,
    AnyWait,
    Force,
}

pub struct WaitGuard {
    task: Arc<Task>,
    state: Arc<WaitState>,
}

pub struct WakeToken {
    state: Arc<WaitState>,
}

pub struct BeginWait {
    guard: WaitGuard,
    token: WakeToken,
}
```

需要提供的语义入口：

```rust
pub fn begin_wait(task: &Arc<Task>, interruptible: bool) -> BeginWait;

pub fn wake_wait(
    task: &Arc<Task>,
    token: &WakeToken,
    reason: WaitReason,
    mode: WakeMode,
) -> WakeResult;

pub fn wake_active_wait(
    task: &Arc<Task>,
    reason: WaitReason,
    mode: WakeMode,
) -> WakeResult;

pub fn cancel_wait(guard: &WaitGuard, reason: WaitReason) -> WaitResult;

pub fn finish_wait(guard: WaitGuard) -> WaitOutcome;

pub fn requeue_permit_if_mode_blocked(
    task: &Arc<Task>,
    token: &WakeToken,
    mode: WakeMode,
) -> Option<RequeuePermit<'_>>;
```

`wake_wait()` 用于 event、timer 等持有 token 的源。`WakeResult::Woke` 必须表示 wait core 已经完成逻辑 wake，并在释放 task sched-state lock 后执行过一次 stale-safe `wake_enqueue()`。调用方可以观察 placement 诊断结果，但不得自行补调裸 `task_enqueue()`。

`wake_active_wait()` 用于 signal、exit 等调度层授权路径。它必须使用同一个 task 调度等待事务，只是目标 wait 来自当前 `TaskSchedState::Waiting { state, .. }`，不是外部 listener。它和 `wake_wait()` 一样拥有 wake 成功后的 stale-safe placement。

`requeue_permit_if_mode_blocked()` 是给 Event 回挂路径使用的窄入口，不是普通查询 API。它必须在持有 task sched-state lock 的情况下确认：

1. 当前 `TaskSchedState::Waiting { state, .. }` 与 `token.state` 通过 `Arc::ptr_eq` 匹配。
2. `WaitState` 仍为 `Armed`。
3. 当前 wait 仍被传入的 `WakeMode` 阻止。

`RequeuePermit<'_>` 必须是 wait core 私有构造的 capability：字段私有，不提供外部构造入口，不实现 `Clone` / `Copy`，生命周期不能逃逸出当前回挂调用，不能被缓存到 listener 中，也不能用于 `wake_wait()`、runqueue placement、timeout/signal 完成逻辑或任意反向进入 wait core 的调用。

## 7. 唯一线性化点

本协议的核心线性化点是 task 调度等待事务。

`wake_wait()` 必须在同一个事务内完成：

1. 验证 `TaskSchedState::Waiting { state, .. }` 和 `token.state` 通过 `Arc::ptr_eq` 指向同一个 `WaitState`。
2. 从 `TaskSchedState::Waiting { interruptible, .. }` 判断该 wait 是否被传入的 `WakeMode` 允许唤醒。
3. 验证 `WaitState` 仍为 `Armed`。
4. 将 `WaitState` 置为 `Completed(reason)`。
5. 将 task 状态置为 `TaskSchedState::Runnable`。

事务提交后，逻辑 wake 已经完成。`wake_wait()` / `wake_active_wait()` 随后在不持有 task sched-state lock 的情况下调用 wake 专用 `wake_enqueue()`。placement 的结果可以放入 `WakeResult` 或 trace 中用于诊断，但不能把入队责任转交给 Event、timer、signal 或 exit 适配层。

`cancel_wait()` 使用同一个事务竞争：

1. 如果 `WaitState` 仍为 `Armed`，将其置为 `Cancelled(reason)`，把 task 恢复为 `TaskSchedState::Runnable`。
2. 如果 `WaitState` 已经 `Completed(reason)`，cancel 不覆盖完成原因。
3. 如果本轮已经 `Cancelled` 或 `Retired`，重复 cleanup 为 no-op。

## 8. stale-safe physical placement

必须新增 wake 专用入队入口，不能继续让 wake 尾巴调用裸 `task_enqueue()`。

建议返回值至少区分：

```rust
pub enum WakeEnqueueResult {
    Stale,
    AlreadyCurrent,
    ParkPending,
    AlreadyQueued,
    Enqueued,
}
```

语义要求：

1. 如果 task 当前不是 `Runnable`，返回 stale，不入队，不断言。
2. 如果 task 是当前 CPU 正在运行的 task，且处于 `ParkState::PrePark`，返回 already current，不入队。
3. 如果 task 是当前 CPU 正在运行的 task，且处于 `ParkState::Parked`，返回 park pending，不入队，也不把这一轮解释成已经完成的普通 placement。
4. 如果 task 已经在 runqueue 上，返回 already queued，不重复入队。
5. 如果 task 已经真正 park，不能只靠 current/no-op 结论，必须按 `Runnable` / `on_runq()` 重新做 stale-safe placement 判断。
6. 远端 IPI handler 也必须走同一条 stale-safe 入口；远端 payload 只负责投递物理 placement 请求，不能重新完成等待语义。

裸 `task_enqueue()` 的严格断言必须保留，但要限制在新创建且已知 runnable 的任务或其他非 wake-tail placement 路径。不得通过弱化 `task_enqueue()` 断言修复本问题。

## 9. Event 协议需求

`Listener` 是 `Event` 私有队列节点。建议形状：

```rust
struct Listener {
    target: WaitTarget,
    queue: ListenerQueueKind,
}

struct WaitTarget {
    task: Weak<Task>,
    token: WakeToken,
}
```

硬性要求：

1. `WaitTarget` 是 Event 与 wait core 之间传递等待目标的最小单位。
2. `Listener` 保存 event-local 元数据，不展开 `WakeToken` 内部状态。
3. `Listener` 相等性必须基于 `WakeToken` 内部 `Arc<WaitState>` 的指针身份，不能基于 task tid。
4. `publish()` 调用 `wake_wait()` 前如果需要访问 task，必须先把非拥有句柄升级成临时强引用；升级失败按 stale listener 处理。
5. `Event.inner` 必须使用 `NoIrqSpinLock<EventInner>` 或等价 NoIrq 锁。

`Event::listen*()` 每一轮等待必须按同一 wait identity 注册和清理 listener：

1. 通过 `begin_wait()` 设置 task 为 `TaskSchedState::Waiting { state, .. }`。
2. 将带有 `WakeToken` 的 listener 注册到 event 队列。
3. predicate 已满足、信号预检查决定不睡、timeout 为零、错误返回等路径必须调用 `cancel_wait()`。
4. 返回后按 `WaitState` 身份 cleanup listener。
5. `finish_wait()` 读取完成原因并退役本轮等待。

`Event::publish()` 只做队列扫描和 candidate 选择，不直接唤醒 task，不直接进入 runqueue placement。

要求：

1. 调用 `wake_wait()` 时不得持有 event lock。
2. `WakeResult::Woke` 只用于更新 successful exclusive quota，不要求调用方再做 placement。
3. 被 wake mode 阻止且仍然 armed 的 listener 必须通过 `requeue_blocked_listener_if_current_armed()` 回挂到原队列尾部，保持相对顺序，不得直接丢弃。
4. stale、already completed、already cancelled 的 listener 可以丢弃。
5. 被 wake mode 阻止的 listener 不消耗 exclusive quota。
6. exclusive quota 只按成功完成的 wait 计数，不按弹出的 listener 计数。
7. 每次 publish 扫描最多处理进入本次 publish 时队列中的原始节点数；本轮回挂的 listener 只在下一轮 publish 再次参与。
8. `wake_wait()` 返回 `ModeBlocked` 只表示返回时的观察结果，不能直接作为之后回挂的充分条件。
9. 回挂前必须重新验证同一轮 wait 仍是 task 当前 active wait 且仍为 `Armed`；若 cancel、timeout、signal 或 finish 已经完成/退役本轮 wait，回挂失败并丢弃 listener。

listener cleanup 必须按 `WaitState` 身份删除。旧轮次 cleanup 不得删除同一个 task 的新轮次 listener。detached 窗口中的 listener 只有在回挂再校验成功后才能重新变成 event 队列节点。

## 10. Timeout、Signal 和主动 cancel

timeout：

1. timer callback 必须持有本轮 `WakeToken`。
2. timer 到期时调用 `wake_wait(task, token, WaitReason::Timeout, WakeMode::AnyWait)`。
3. event 或 signal 已经完成本轮时，timer wake 必须 stale。
4. waiter 已进入下一轮 wait 时，旧 timer token 必须无法完成新一轮。
5. 正确性不得依赖 timer callback 是否被物理取消。

signal：

1. signal 路径调用 `wake_active_wait(task, WaitReason::Signal, WakeMode::InterruptibleOnly)`。
2. 只有当前 `TaskSchedState::Waiting { interruptible: true, .. }` 时才完成。
3. uninterruptible wait 保持 armed。
4. force wake 或退出路径可以使用 `WakeMode::Force`，但仍必须通过同一个 task 调度等待事务。

主动 cancel 包括 predicate 已满足、不需要 park、signal 预检查决定返回、timeout 为零、wait guard 提前退出或错误返回。这些路径必须使用 `cancel_wait()`，不能直接写 `TaskStatus::Runnable`。

## 11. 必须维持的不变量

### 11.1 单 active wait

同一个 task 任意时刻最多只有一个 active wait。

证明依赖：

1. `begin_wait()` 在 task 调度等待事务内把 `TaskSchedState` 置为 `Waiting { state, .. }`。
2. `wake_wait()` 和 `cancel_wait()` 在同一事务内把 `TaskSchedState` 从 matching `Waiting` 变为 `Runnable`。
3. 没有其他路径直接构造 `Waiting` 或绕过 wait core 把等待态改回 `Runnable`。

### 11.2 单轮单完成

同一个 `WaitState` 最多有一个终止原因。

证明依赖：

1. `Armed -> Completed/Cancelled` 只发生在 task 调度等待事务内。
2. 所有 event、timer、signal、cancel 都竞争同一个 `WaitState`。
3. `Retired` 只在 waiter 收口后设置，不参与 wake 竞争。

### 11.3 旧 token 不能完成新 wait

旧 `WakeToken` 只指向旧 `WaitState`。

`wake_wait()` 提交前必须使用 `Arc::ptr_eq` 验证 `TaskSchedState::Waiting { state, .. }` 和 token 指向同一轮 `WaitState`。如果 task 已经进入下一轮等待，`state` 指向新 state，旧 token 必然失败。

### 11.4 旧 wake 尾巴不能破坏新 wait

逻辑 wake 在 `wake_wait()` 事务中完成，事务后只剩物理 placement。

如果旧 wake 尾巴晚到，而 task 已经进入下一轮 `Waiting`，`wake_enqueue()` 看到非 `Runnable` 后直接 stale return。因此旧尾巴不能把新 wait 投递进 runqueue，也不能触发 `Runnable` 断言。

### 11.5 pre-park wake 不丢失

如果 wake 发生在 waiter 调用 `schedule()` 前，`wake_wait()` 会先把 task 改回 `Runnable`。

随后 waiter 进入 `schedule()` 时，调度器按 `Runnable` 语义 requeue 当前 task。若远端 wake placement 先到，`wake_enqueue()` 看到 task 仍是 current task 或已经 queued，应 no-op。

### 11.6 post-park wake 不丢失

如果 wake 发生在 waiter 已经通过 `schedule()` 离开 CPU 后，task 不再是 current，且状态已在 `wake_wait()` 中改为 `Runnable`。后续 `wake_enqueue()` 会将它放入目标 runqueue，除非它已经由其他路径放入。

### 11.7 exclusive listener 不被 stale 节点吃掉

exclusive publish 只按成功完成的 wait 计数。

旧 listener、已完成 listener、已取消 listener 和 mode-blocked listener 都不能消耗 successful exclusive quota。

### 11.8 park latch 闭合 current / parked / queued

同一个 wait round 必须有 park latch 闭合 schedule 和 wake tail：

1. `begin_wait()` 创建 `Waiting { park: ParkState::PrePark, .. }`，表示 task 仍处于 pre-park window。
2. `schedule()` 在真正让出 CPU 前把同一轮 wait 提交为 `Waiting { park: ParkState::Parked, .. }`，然后立刻在同一事务里重读调度状态。
3. 如果重读发现本轮已经被 `wake_wait()` / `cancel_wait()` 完成，`schedule()` 必须 abort park，回到 runnable/yield 路径。
4. `wake_enqueue()` 对 current task 的 no-op 只允许发生在 `ParkState::PrePark` 窗口；`ParkState::Parked` 时，这个窗口的最终归属由 `schedule()` 决定，不由 queueing 侧决定。
5. 一旦 task 不再是 current，`wake_enqueue()` 必须按 stale-safe placement 重新判断，不能只靠一次 `TaskStatus` 快照。

## 12. 锁序和事务纪律

建议锁序：

1. event lock 只保护 listener 队列。
2. task sched-state lock 保护整个 `TaskSchedState`。
3. runqueue / processor 本地临界区只保护物理 placement。
4. mode-blocked listener 回挂再校验的唯一允许组合锁序是 task sched-state lock -> event lock。

约束：

1. 不在持有 event lock 时调用 `wake_wait()`。
2. 不在持有 event lock 时进入 runqueue placement。
3. `wake_wait()` 不访问 event queue。
4. `wake_enqueue()` 不访问 event queue，也不修改 `WaitState`。
5. timer callback 和 signal path 不持有 event lock。
6. `requeue_blocked_listener_if_current_armed()` 是唯一允许通过 `RequeuePermit` 在 task sched-state 受保护顺序中短暂获取 event lock 的路径；它只能把已摘下的同一个 listener 回挂到同一个 event 队列。
7. 任何路径都不得在持有 event lock 时再获取 task sched-state lock。`publish()` 取候选 listener、listener register、listener cleanup 都必须保持 event-lock-only。
8. `requeue_blocked_listener_if_current_armed()` 失败时必须丢弃 listener，不能退回到无校验回挂。

mode-blocked 回挂的竞争结果必须闭合：

1. cancel/timeout/signal 先完成本轮 wait：回挂 helper 后续再校验失败，listener 被丢弃。
2. 回挂 helper 先验证成功：listener 在同一受保护顺序中回挂；后续 cancel/timeout/signal 正常完成 wait，并由 cleanup 按 wait identity 删除 listener。
3. 另一个 event 或 timeout 已经完成同一轮 wait：再校验看到 `WaitState` 非 `Armed` 或 `TaskSchedState` 不再匹配，listener 被丢弃。
4. task 已经进入下一轮 wait：`Arc::ptr_eq` 不匹配，listener 被丢弃。

## 13. 禁止退化项

下面任一退化都会破坏不变量：

1. 只加 token，不改入队尾巴。
2. 只改入队尾巴，不加 wait identity。
3. timeout 或 signal 保留直接写 `TaskStatus` 并入队的旁路。
4. `clean_listener()` 继续按 tid 删除 listener。
5. exclusive quota 按弹出节点计数。
6. 将 `WaitState` 做成可绕过 `TaskSchedState` 的第二套状态机。
7. current no-op 没有 park latch。
8. mode-blocked listener 被直接丢弃。
9. mode-blocked listener 依据 `wake_wait()` 的旧返回结果直接回挂。
10. 调度、wait、wake 或 enqueue 路径通过 `TaskStatus` 投影判断 runnable / waiting / zombie，而不是直接使用 `TaskSchedState` 或受控 helper。
11. `RequeuePermit` 可外部构造、可复制、可缓存，或被用于回挂之外的能力扩张。

## 14. 完成标准

实现完成必须满足：

1. 已知 `Event::listen*()` 与 `publish()` race 不再触发 `processor.rs` 的 `Runnable` 断言。
2. `Event`、timeout、signal、cancel 的等待完成都通过同一 wait core。
3. wake 尾巴使用 stale-safe placement。
4. listener identity 基于 `Arc::ptr_eq` 语义。
5. exclusive quota 基于成功完成的 wait。
6. mode-blocked listener 回挂通过再校验 helper 完成，detached 窗口不会留下已完成或已取消的 stale listener。
7. 旧直接 wake API 不再被普通等待路径使用。
8. 已知触发 profile 能跑过原先的随机 panic 点。
