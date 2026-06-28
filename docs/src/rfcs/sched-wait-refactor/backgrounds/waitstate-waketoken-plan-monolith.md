# WaitState/WakeToken 修订版实现计划（补 requeue 再校验）

日期：2026-06-01

状态：单文件归档。canonical 迁移文档已拆分为 [Sched Wait Refactor 不变量需求](../invariants.md) 和 [Sched Wait Refactor 迁移实施计划](../implementation.md)。

相关材料：

1. [Wait/Wake Race 问题简述](./wake-race-problem-brief.md)
2. [WaitState/WakeToken 问题清单](./waitstate-waketoken-issues.md)
3. [Sched Wait Refactor 不变量需求](../invariants.md)
4. [Sched Wait Refactor 迁移实施计划](../implementation.md)

## 0. Review 结论

本版继承 2026-05-31 修订版的主体设计，并补上一个新的协议缺口：`publish()` 对 mode-blocked listener 的回挂不能只依赖 `wake_wait()` 返回时的旧观察。回挂前必须再次确认该 listener 仍对应 task 当前 active wait、`WaitState` 仍为 `Armed`、并且仍被本次 `WakeMode` 阻止。

原方案中的三个核心结论仍保留：

1. wake 后 physical placement 的 current no-op 需要补 park latch，否则 `wake_wait()` 到 `schedule()` 之间的闭环只靠一次状态快照，证明不完整。
2. `publish()` 对 mode-blocked listener 必须明确回挂规则，不能把“未成功唤醒”混同为 stale 后直接丢弃。
3. wait identity 直接使用 `Arc<WaitState>` 的指针身份；所有身份比较必须用 `Arc::ptr_eq`，不能依赖字段相等，也不引入额外的 id 计数器。

本版追加的第四条要求是：

4. mode-blocked listener 回挂必须通过 `requeue_blocked_listener_if_current_armed()` 或等价窄入口完成；该入口先从 wait core 获取短寿命 `RequeuePermit`，再用 permit 回挂 listener。失败则丢弃 listener，不引入 generation、reservation 或额外公开状态。

## 1. 目标

这份计划把 `Event` wake race 的理论模型正式化为后续实现指导。

本轮目标不是重写整个 scheduler，而是把等待、唤醒、取消、超时、信号和物理入队收进一套可证明协议，消除旧一轮 wake 尾巴撞上新一轮 wait 的窗口。

完成后应满足：

1. 一轮等待有稳定身份，旧轮次 wake 不能完成新轮次 wait。
2. event wake、timeout、signal、主动 cancel 竞争同一个等待状态。
3. 逻辑 wake 和 `TaskSchedState` 更新有唯一线性化点。
4. wake 成功后的物理入队由 wait core 统一触发 stale-safe placement，不再承载等待语义。
5. `Event` 只维护 listener 队列和 exclusive 策略，不直接修改 task 调度状态。

## 2. 非目标

本计划不包括：

1. 重写调度算法、调度类或时间片策略。
2. 引入跨 CPU 迁移、负载均衡或远程 runqueue 观察。
3. 一次性完成 futex PI、poll/epoll 完整语义或 Linux waitqueue 全功能兼容。
4. 把所有同步原语重构成同一种高层抽象。
5. 通过放宽 `task_enqueue()` 断言来掩盖竞态。

这里要解决的是 wait/wake 协议边界，不是 scheduler policy。

## 3. 当前问题边界

当前竞态可以概括为：

1. waiter 在 `Event::listen*()` 中把当前 task 标记为 `Waiting` 并准备 park。
2. waker 在 `Event::publish()` 中把 task 切回 `Runnable`。
3. `try_to_wake_up()` 在状态更新后异步执行 `task_enqueue()`。
4. waiter 可能已经运行过并进入下一轮 `Waiting`。
5. 旧 wake 的入队尾巴看到新状态，于是触发 `task_enqueue()` 或 `local_enqueue()` 的 `Runnable` 断言。

根因不是单个条件检查缺失，而是缺少“等待轮次身份”和“wake 尾巴 stale-safe placement”两个协议层。

## 4. 核心设计

### 4.1 Task 调度等待状态

`Task` 的调度状态需要从单独的 `TaskStatus` 扩展为同一事务保护的等待视图。

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

要求：

1. `TaskSchedState` 必须由同一个 `NoIrqRwLock<TaskSchedState>` 或等价 NoIrq 事务保护。
2. 外部路径不能分别维护 `TaskStatus`、active wait 和 park latch 三套真相。
3. `Waiting { state, .. }` 表示当前 task 只属于这一轮等待；`Runnable` / `Zombie` 状态下不存在 active wait。
4. `ParkState::PrePark` / `ParkState::Parked` 只用于闭合 wait / schedule / wake-tail 的时序，不表示第二套等待状态。
5. park 状态必须和 wait identity 一起在同一个调度事务内翻转。
6. `wake_enqueue()` 对 current 的 no-op 只允许发生在 `ParkState::PrePark` 窗口里；一旦 task 已经真正 park，wake tail 不能只靠 current 快照做决定。
7. `ParkState` 不是公开的 placement 状态，它是 `schedule()` 自己消费的提交闸门；如果 wake 已经把这一轮完成为 `Runnable`，`schedule()` 必须在真正切换出去之前把这一轮判回 runnable/yield 路径，而不是留下一个“current 但已 parked”的悬挂态。
8. `task.status()` 只能作为兼容观察接口，从 `TaskSchedState` 投影出旧 `TaskStatus` 视图；迁移后不能再作为写入口。

第一阶段可以把现有 `status: NoIrqRwLock<TaskStatus>` 替换为 `sched_state: NoIrqRwLock<TaskSchedState>`，再通过兼容读接口保留 `task.status()`。

### 4.2 WaitState

`WaitState` 是一轮等待的唯一生命周期实体，不能复用给下一轮。

`Arc<WaitState>` 的指针身份就是等待轮次身份。只要本轮 `WaitGuard`、`WakeToken`、`Listener` 等引用尚未 drop，底层对象不会释放，地址也不会被下一轮等待复用。因此这里不引入额外的全局计数器。

所有身份比较必须走 `Arc::ptr_eq` 语义，不能依赖 `WaitState` 字段相等，也不要给 `WaitState` 实现会被误用为身份判断的结构化 `PartialEq`。

最小字段语义：

1. 稳定身份，即 `Arc<WaitState>` 指针身份。
2. 当前状态：`Armed`、`Completed(reason)`、`Cancelled(reason)`、`Retired`。
3. 完成原因：event、timeout、signal、force、predicate ready、guard drop 等。
4. 调试信息：创建 task、创建点、等待模式等，可以按需要在 debug build 中保留。

`WaitState` 的 `Armed -> Completed/Cancelled` 转换必须在 task 调度等待事务中提交。`WaitState` 可以有内部状态字段，但它不能成为绕过 `TaskSchedState::Waiting { state, .. }` 校验的第二套完成真相。
`WaitState` 只应保存 by-value 的调试元数据，例如创建 tid、创建点、等待模式和时间戳。它不应持有强 `Arc<Task>` / `Event` 回指，否则 `Task -> WaitState -> Task` 的强环会把退役顺序变成生命周期问题。

### 4.3 WakeToken

`WakeToken` 是事件源持有的受限唤醒能力。

它只表达：

1. 目标等待轮次身份。
2. 对该轮次尝试完成的能力。

它不表达：

1. listener 在 event 队列中的位置。
2. task 当前是否真的还在等待。
3. runqueue placement。
4. 第二套 wake state machine。

事件源拿到 token 后只能调用调度等待层提供的完成接口，不能直接写 `TaskStatus`，也不能直接调用裸 `task_enqueue()`。

`WakeToken` 和 `WaitGuard` 都必须持有同一个 `Arc<WaitState>`；外部比较只能使用 `Arc::ptr_eq`，不能让结构体字段相等误充当身份相等。

### 4.4 WaitGuard

waiter 自己持有 `WaitGuard`，用于：

1. 在 predicate 已满足、信号预检查决定不睡、timeout 为零、错误返回等路径中取消本轮等待。
2. 在 schedule 返回后读取最终完成原因。
3. 在返回调用者前 retire 本轮等待。

`WaitGuard` drop 可以作为兜底 cleanup，但正常路径应显式调用 `finish_wait()` 或等价接口，避免把主要语义隐藏在析构里。

### 4.5 模块边界

1. `sched::wait` 只拥有等待事务、完成权、退役语义和 requeue permit 的签发权。
2. `Event`、timeout、signal、exit 只作为适配器调用 wait core，不各自维护一套可完成状态机。
3. `TaskStatus` 的兼容读接口在迁移期只允许观察，不允许再成为新的写入口。
4. 任何新 helper 都应优先挂在 wait core 边界上，而不是在 `Event`、timer 或 signal 模块里重复实现状态切换。
5. wait core 不应执行 Event 提供的任意闭包；跨 event / task 的特殊顺序应通过不可外部构造的短寿命 capability 表达。

### 4.6 Event Listener

`Listener` 是 `Event` 私有队列节点。

它应保存：

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

要求：

1. `WaitTarget` 是 Event 与 wait core 之间传递等待目标的最小单位。
2. `Listener` 保存 event-local 元数据，例如队列类别；它不展开 `WakeToken` 内部状态。
3. `Listener` 相等性必须基于 `WakeToken` 内部 `Arc<WaitState>` 的指针身份，不能基于 task tid。
4. `publish()` 调用 `wake_wait()` 前如果需要访问 task，必须先把非拥有句柄升级成临时强引用；升级失败应按 stale listener 处理，而不是把它当作一个新的完成结果。
5. `Event.inner` 必须使用 `NoIrqSpinLock<EventInner>` 或等价 NoIrq 锁。Event 队列可能从中断上下文访问，文档和示例都不应再使用普通 `lock()` / `SpinLock` 语义。

## 5. 核心 API 草案

命名可以在实现时微调，但语义边界应保持不变。

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

建议接口：

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

`wake_wait()` 用于 event、timer 等持有 token 的源。它不是单纯的状态提交 helper；`WakeResult::Woke` 表示 wait core 已经完成逻辑 wake，并在释放 task sched-state lock 后执行过一次 stale-safe `wake_enqueue()`。调用方可以观察 placement 诊断结果，但不能自行补调裸 `task_enqueue()`。

`wake_active_wait()` 用于 signal、exit 等调度层授权路径。它仍然必须使用同一个 task 调度等待事务，只是目标 wait 来自当前 `TaskSchedState::Waiting { state, .. }`，不是外部 listener。这个接口不应作为 Event 侧公共完成入口对外暴露。它和 `wake_wait()` 一样拥有 wake 成功后的 stale-safe placement；signal、exit 等适配层不直接进入 runqueue。

`requeue_permit_if_mode_blocked()` 是给 Event 回挂路径使用的窄入口。它不是普通查询 API，也不暴露 `TaskSchedState` 或 `WaitState` 内部数据。它必须在持有 task sched-state lock 的情况下完成：

1. `TaskSchedState::Waiting { state, .. }` 与 `token.state` 通过 `Arc::ptr_eq` 匹配。
2. `WaitState` 仍为 `Armed`。
3. 当前 `TaskSchedState::Waiting { interruptible, .. }` 仍被传入的 `WakeMode` 阻止。
4. 以上条件成立时，返回一个不可由 Event 构造的短寿命 `RequeuePermit<'_>`。

`RequeuePermit<'_>` 是 wait core 私有构造的 capability：字段私有，不提供外部构造入口，不实现 `Clone` / `Copy`。它内部持有 task sched-state guard 或等价借用，因此不能逃逸出当前回挂调用，也不能被缓存到 listener 中。Event 只能把 permit 传给本 event 的私有回挂函数；不能用 permit 调用 `wake_wait()`、runqueue placement、timeout/signal 完成逻辑，或任意会反向获取 task sched-state lock 的代码。

## 6. 线性化点

本协议的核心线性化点是 task 调度等待事务。

`wake_wait()` 必须在同一个事务内完成：

1. 验证 `TaskSchedState::Waiting { state, .. }` 和 `token.state` 通过 `Arc::ptr_eq` 指向同一个 `WaitState`。
2. 从 `TaskSchedState::Waiting { interruptible, .. }` 判断该 wait 是否被传入的 `WakeMode` 允许唤醒。
3. 验证 `WaitState` 仍为 `Armed`。
4. 将 `WaitState` 置为 `Completed(reason)`。
5. 将 task 状态置为 `TaskSchedState::Runnable`。

事务提交后，逻辑 wake 已经完成。`wake_wait()` / `wake_active_wait()` 随后在不持有 task sched-state lock 的情况下调用 wake 专用 `wake_enqueue()`，执行 stale-safe physical placement。placement 的结果可以放入 `WakeResult` 或 trace 中用于诊断，但不能把入队责任转交给 Event、timer、signal 或 exit 适配层。

`cancel_wait()` 使用同一个事务竞争：

1. 如果 `WaitState` 仍为 `Armed`，将其置为 `Cancelled(reason)`，把 task 恢复为 `TaskSchedState::Runnable`。
2. 如果 `WaitState` 已经 `Completed(reason)`，cancel 不覆盖完成原因。
3. 如果本轮已经 `Cancelled` 或 `Retired`，重复 cleanup 为 no-op。

## 7. stale-safe wake enqueue

必须新增 wake 专用入队入口，不能继续让 wake 尾巴调用裸 `task_enqueue()`。

建议把返回值至少分成 `Stale`、`AlreadyCurrent`、`ParkPending`、`AlreadyQueued`、`Enqueued` 五类，而不是只返回一个布尔值。这样 `schedule()` 的 handoff 窗口和 runqueue 物理 placement 的责任边更清楚。

建议命名：

```rust
pub fn wake_enqueue(task: Arc<Task>) -> WakeEnqueueResult;
```

语义：

1. 如果 task 当前不是 `Runnable`，返回 stale，不入队，不断言。
2. 如果 task 是当前 CPU 正在运行的 task，且处于 `ParkState::PrePark`，返回 already current，不入队。
3. 如果 task 是当前 CPU 正在运行的 task，且处于 `ParkState::Parked`，返回 park pending，不入队，也不把这一轮解释成已经完成的普通 placement。
4. 如果 task 已经在 runqueue 上，返回 already queued，不重复入队。
5. 如果 task 已经真正 park，不能只靠 current/no-op 结论，必须按 `Runnable` / `on_runq()` 重新做 stale-safe placement 判断。
6. 否则将 task 投递到目标 CPU 的 runqueue。

`wake_enqueue()` 不负责消费 `ParkState::Parked` 的 current handoff 窗口。那个窗口属于 `schedule()` 的内部 abort-park 逻辑；queueing 侧只做事务完成后的物理 placement。

远端 IPI handler 也必须走同一条 stale-safe 入口。远端 payload 只负责投递物理 placement 请求，不能重新完成等待语义。

保留裸 `task_enqueue()` 的严格断言，但把它限制在“新创建且已知 runnable 的任务”或其他非 wake 尾巴路径。不要通过弱化 `task_enqueue()` 断言来修复本问题。

## 8. Event 协议

### 8.1 listen 路径

`Event::listen*()` 每一轮等待应按下面顺序组织：

1. 创建 `WaitState`，并通过 `begin_wait()` 设置 task 为 `TaskSchedState::Waiting { state, .. }`。
2. 将带有 `WakeToken` 的 listener 注册到 event 队列。
3. 检查 predicate。
4. 检查 interruptible signal 条件。
5. 如果决定不 park，调用 `cancel_wait()` 并按 `WaitState` 身份 cleanup listener。
6. 如果需要 park，释放 preempt guard 后进入 schedule。
7. 返回后按 `WaitState` 身份 cleanup listener。
8. `finish_wait()` 读取完成原因，并由上层继续 recheck predicate 或返回 signal/timeout。

注意：

1. `clean_listener()` 不再修改 `TaskStatus`。
2. cleanup 是 best-effort，listener 可能已经被 publish 摘走。
3. 同一个 task 的下一轮 listener 不能被上一轮 cleanup 删除。
4. `begin_wait()` 后到 `schedule()` 前，当前 task 仍在 CPU 上运行，现有 preempt discipline 仍然需要保留。

### 8.2 publish 路径

`Event::publish()` 只做队列扫描和 candidate 选择，不直接唤醒 task。

候选 listener 应交给 `wake_wait()`：

```rust
match wake_wait(&task, listener.target.token(), WaitReason::Event, mode) {
    WakeResult::Woke => exclusive_success += 1,
    WakeResult::ModeBlocked => {
        self.requeue_blocked_listener_if_current_armed(listener, mode);
    }
    WakeResult::Stale
    | WakeResult::AlreadyCompleted
    | WakeResult::AlreadyCancelled => {}
}
```

这里的 `WakeResult::Woke` 已经包含 wait core 对 stale-safe placement 的一次执行；`publish()` 只据此更新 exclusive quota，不补调 `task_enqueue()` / `wake_enqueue()`。

要求：

1. 调用 `wake_wait()` 时不持有 event lock。
2. 被 wake mode 阻止且仍然 armed 的 listener 必须通过 `requeue_blocked_listener_if_current_armed()` 回挂到原队列尾部，保持相对顺序，不得直接丢弃。
3. stale、already completed、already cancelled 的 listener 可以丢弃。
4. 被 wake mode 阻止的 listener 不应消耗 exclusive quota。
5. exclusive quota 按成功完成的 wait 计数，不按弹出的 listener 计数。
6. 为避免 publish 在队列中无限旋转，每次扫描最多处理进入本次 publish 时队列中的原始节点数；本轮回挂的 listener 只在下一轮 publish 再次参与。
7. `wake_wait()` 返回 `ModeBlocked` 只表示返回时的观察结果，不能直接作为之后回挂的充分条件。
8. 回挂前必须重新验证同一轮 wait 仍是 task 当前 active wait 且仍为 `Armed`；若此时 cancel、timeout、signal 或 finish 已经完成/退役本轮 wait，回挂失败并丢弃 listener。

建议 Event 私有 helper 形状：

```rust
fn requeue_blocked_listener_if_current_armed(
    &self,
    listener: Listener,
    mode: WakeMode,
) -> RequeueBlockedResult {
    let Some(task) = listener.target.task.upgrade() else {
        return RequeueBlockedResult::Stale;
    };
    let Some(permit) =
        requeue_permit_if_mode_blocked(&task, listener.target.token(), mode)
    else {
        return RequeueBlockedResult::Stale;
    };

    let mut inner = self.inner.lock(); // NoIrqSpinLock<EventInner>
    inner.requeue_blocked(permit, listener)
}
```

这个 helper 不维护额外 generation，也不把新的 reservation 状态塞进 listener。`RequeuePermit` 只把“仍是当前 armed wait”这个判断的短寿命权利带到同一次回挂调用里；Event 不能自己构造 permit，也不能把它保存下来。

### 8.3 listener cleanup

listener cleanup 必须按 `WaitState` 身份删除。

允许的并发结果：

1. listener 已经被 publish 摘走，cleanup no-op。
2. listener 仍在队列中，cleanup 删除它。
3. 同一 task 已经注册新一轮 listener，旧 cleanup 不能影响新 listener。
4. 同一个 `WakeToken` 挂在多个 event 队列时，每个 event 只清理自己的 listener 节点。
5. `publish()` 可能先把 listener 从队列摘下，再在 mode-blocked 时回挂到尾部；这个 detached 窗口是事件局部的中间态，不是新的完成语义。
6. detached 窗口中的 listener 只有在 `requeue_blocked_listener_if_current_armed()` 再校验成功后才能重新变成 event 队列节点。
7. 如果 cleanup 在 detached 窗口里先执行，它可以 no-op；随后回挂 helper 会因 `TaskSchedState` 或 `WaitState` 状态不匹配而失败。
8. 如果回挂 helper 先成功，后续 cleanup 仍按 `WaitState` 身份删除该 listener。

## 9. Timeout 和 Signal

### 9.1 timeout

`schedule_with_timeout()` 不能继续使用独立 `AtomicBool validness + notify()` 表达 timeout 完成权。

新的 timeout 语义应当是：

1. timer callback 持有本轮 `WakeToken`。
2. timer 到期时调用 `wake_wait(task, token, WaitReason::Timeout, WakeMode::AnyWait)`。
3. 如果 event 或 signal 已经完成本轮，timer wake 会因为 `WaitState` 非 `Armed` 或 `TaskSchedState` 不匹配而 stale。
4. 如果 waiter 已经进入下一轮 wait，旧 timer token 必然无法完成新一轮。

timeout 剩余时间可以继续由调用者按 `Instant` 计算；正确性不能依赖 timer callback 是否被物理取消。

### 9.2 signal

signal notify 不能继续直接按 `TaskStatus` 修改状态并调用裸 `task_enqueue()`。

新的 signal 语义应当是：

1. signal 路径调用 `wake_active_wait(task, WaitReason::Signal, WakeMode::InterruptibleOnly)`。
2. 只有当前 `TaskSchedState::Waiting { interruptible: true, .. }` 时才完成。
3. uninterruptible wait 保持 armed。
4. force wake 或退出路径可以使用 `WakeMode::Force`，但仍必须通过同一个 task 调度等待事务。

### 9.3 主动 cancel

主动 cancel 包括：

1. predicate 已满足，不需要 park。
2. signal 预检查决定返回。
3. timeout 为零。
4. wait guard 提前退出或错误返回。

这些路径必须使用 `cancel_wait()`，不能直接写 `TaskStatus::Runnable`。

## 10. 需要维持的不变量

### 10.1 单 active wait

同一个 task 任意时刻最多只有一个 active wait。

证明依赖：

1. `begin_wait()` 在 task 调度等待事务内把 `TaskSchedState` 置为 `Waiting { state, .. }`。
2. `wake_wait()` 和 `cancel_wait()` 在同一事务内把 `TaskSchedState` 从 matching `Waiting` 变为 `Runnable`。
3. 没有其他路径直接构造 `Waiting` 或绕过 wait core 把等待态改回 `Runnable`。

### 10.2 单轮单完成

同一个 `WaitState` 最多有一个终止原因。

证明依赖：

1. `Armed -> Completed/Cancelled` 只发生在 task 调度等待事务内。
2. 所有 event、timer、signal、cancel 都竞争同一个 `WaitState`。
3. `Retired` 只在 waiter 收口后设置，不参与 wake 竞争。

### 10.3 旧 token 不能完成新 wait

旧 `WakeToken` 只指向旧 `WaitState`。

`wake_wait()` 提交前必须使用 `Arc::ptr_eq` 验证 `TaskSchedState::Waiting { state, .. }` 和 token 指向同一轮 `WaitState`。如果 task 已经进入下一轮等待，`state` 指向新 state，旧 token 必然失败。

### 10.4 旧 wake 尾巴不能破坏新 wait

逻辑 wake 在 `wake_wait()` 事务中完成，事务后只剩物理 placement。

如果旧 wake 尾巴晚到，而 task 已经进入下一轮 `Waiting`，`wake_enqueue()` 看到非 `Runnable` 后直接 stale return。因此旧尾巴不能把新 wait 投递进 runqueue，也不能触发 `Runnable` 断言。

### 10.5 pre-park wake 不丢失

如果 wake 发生在 waiter 调用 `schedule()` 前，`wake_wait()` 会先把 task 改回 `Runnable`。

随后 waiter 进入 `schedule()` 时，调度器按 `Runnable` 语义 requeue 当前 task。若远端 wake placement 先到，`wake_enqueue()` 看到 task 仍是当前 task 或已经 queued，应 no-op。

### 10.6 post-park wake 不丢失

如果 wake 发生在 waiter 已经通过 `schedule()` 离开 CPU 后，task 不再是 current，且状态已在 `wake_wait()` 中改为 `Runnable`。后续 `wake_enqueue()` 会将它放入目标 runqueue，除非它已经由其他路径放入。

### 10.7 exclusive listener 不被 stale 节点吃掉

exclusive publish 只按成功完成的 wait 计数。

旧 listener、已完成 listener、已取消 listener 和 mode-blocked listener 都不能消耗 successful exclusive quota。

### 10.8 park latch 闭合 current / parked / queued

同一个 wait round 还需要一个独立的 park latch 来闭合 schedule 和 wake tail：

1. `begin_wait()` 创建 `Waiting { park: ParkState::PrePark, .. }`，表示 task 仍处于 pre-park window。
2. `schedule()` 在真正让出 CPU 前把同一轮 wait 提交为 `Waiting { park: ParkState::Parked, .. }`，然后立刻在同一事务里重读调度状态。
3. 如果这次重读发现本轮已经被 `wake_wait()` / `cancel_wait()` 完成，`schedule()` 必须 abort park，回到 runnable/yield 路径。
4. `wake_enqueue()` 对 current task 的 no-op 只允许发生在 `ParkState::PrePark` 窗口；`ParkState::Parked` 时，这个窗口的最终归属由 `schedule()` 决定，不由 queueing 侧决定。
5. 一旦 task 不再是 current，`wake_enqueue()` 必须按 stale-safe placement 重新判断，不能只靠一次 `TaskStatus` 快照。

## 11. 锁序和事务纪律

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

如果 `WaitState` 内部需要锁，正常完成路径应在持有 task sched-state lock 后访问 `WaitState` 状态，避免 `TaskSchedState` 和 `WaitState` 状态出现相互等待。

mode-blocked 回挂的竞争结果：

1. cancel/timeout/signal 先拿到 task sched-state lock 并完成本轮 wait：回挂 helper 后续再校验失败，listener 被丢弃。
2. 回挂 helper 先拿到 task sched-state lock 并验证成功：listener 在同一受保护顺序中回挂；后续 cancel/timeout/signal 正常完成 wait，并由 cleanup 按 wait identity 删除 listener。
3. 另一个 event 或 timeout 已经完成同一轮 wait：再校验看到 `WaitState` 非 `Armed` 或 `TaskSchedState` 不再匹配，listener 被丢弃。
4. task 已经进入下一轮 wait：`Arc::ptr_eq` 不匹配，listener 被丢弃。

## 12. 实施阶段

### 阶段 1：建立调度等待核心

交付：

1. 新增 `sched::wait` 或等价子模块。
2. 定义 `WaitState`、`WakeToken`、`WaitGuard`、`WaitReason`、`WakeMode`、结果类型。
3. 将 `Task` 的 `status` 扩展为枚举式 `TaskSchedState`。
4. 提供只读兼容接口 `task.status()`，避免一次性改动所有观察路径。
5. 新增 `begin_wait()`、`wake_wait()`、`wake_active_wait()`、`cancel_wait()`、`finish_wait()`，其中 `wake_active_wait()` 只作为 sched 内部受控 helper。

验收：

1. 旧代码仍能构建。
2. `TaskStatus` 观察者不需要理解 `WaitState`。
3. 新接口文档明确线性化点和调用条件。

### 阶段 2：新增 stale-safe wake placement

交付：

1. 新增 wake 专用 `wake_enqueue()`。
2. local wake path 支持 stale、pre-park current、already queued no-op，并在 parked 后走真实 placement。
3. remote wake IPI handler 改走 stale-safe 入口。
4. 保留 `task_enqueue()` 的严格断言，限制其语义为非 wake-tail placement。

验收：

1. wake 尾巴不再因为 task 已进入新一轮 `Waiting` 而 panic。
2. 新建任务、bootstrap 首次入队等严格路径仍保留断言。
3. 远端 IPI wake 到达时重新验证 task 当前 placement 条件。

### 阶段 3：改造 Event

交付：

1. `Listener` 增加 wait identity。
2. `Listener` 相等性改为基于 `Arc::ptr_eq` 语义的 wait identity。
3. `prepare_listener()` 拆成 begin wait 与 event register 两个职责。
4. `clean_listener()` 只清 listener，不再修改 task status。
5. `Event::publish()` 改为扫描 listener 并调用 `wake_wait()`。
6. exclusive quota 按成功完成 wait 计数。
7. 增加 `requeue_blocked_listener_if_current_armed()`，用于 mode-blocked listener 的回挂前再校验。

验收：

1. `Event::publish()` 不再调用 `try_to_wake_up()`。
2. `Event` 不直接写 `TaskStatus`。
3. 旧轮次 cleanup 不能删除新轮次 listener。
4. mode-blocked listener 在 detached 窗口中遇到 cancel、timeout、signal 或 finish 时不会重新进入 event 队列。
5. `requeue_blocked_listener_if_current_armed()` 不引入额外 generation、reservation 或公开状态面。
6. futex、mutex、wait4、vfork_done 等现有 Event 用户语义不回退。

### 阶段 4：改造 timeout 和 signal

交付：

1. `schedule_with_timeout()` 的 timer callback 改为持有 token 并调用 `wake_wait()`。
2. 移除 timeout 正确性对独立 `AtomicBool validness` 的依赖。
3. signal notify 改为调用 `wake_active_wait()`。
4. `clock_nanosleep()`、`rt_sigtimedwait()` 等直接等待路径改用统一 wait core。

验收：

1. timeout、event、signal 竞争同一 `WaitState`。
2. 旧 timer callback 晚到时 stale return。
3. signal 只完成 interruptible wait，force wake 有显式 mode。
4. 代码中不再有普通等待路径直接写 `TaskStatus::Waiting` 后调用旧 timeout/notify 组合。

### 阶段 5：旁路审计和收口

交付：

1. 审计所有 `update_status_with()` 调用。
2. 审计所有 `TaskStatus::Waiting` 写入点。
3. 审计所有 `notify()`、`try_to_wake_up()` 和 `task_enqueue()` 调用点。
4. 将旧 wake API 收缩为兼容 wrapper，或明确标记为只允许非 wait 协议路径使用。
5. 给关键入口补充 debug assert，检查 `TaskSchedState` 状态转换、`ParkState::Parked` 只能出现在 `Waiting` 上、兼容 `task.status()` 投影与内部状态一致。

验收：

1. 没有 Event、timeout、signal、cancel 路径绕过 wait core。
2. wake tail 全部走 stale-safe placement。
3. 裸 `task_enqueue()` 不再出现在 wait/wake 完成尾巴中。

### 阶段 6：验证和文档跟进

交付：

1. 增加最小并发回归测试或 debug stress hook，覆盖旧 wake 尾巴晚到新 wait 的交错。
2. 复跑已知触发 profile。
3. 更新 open issue / devlog，记录该 race 的处理状态和剩余限制。

建议验证：

1. `just build`
2. `./scripts/run-user-test-rv64.sh rootfsconfig-rv etc/sdcard-rv.img build/event-waitstate-rv64.log`
3. 复跑曾触发 `processor.rs` `Runnable` 断言的 musl memory profile。
4. 如 la64 侧也能稳定复现相关 profile，再做一次 la64 smoke run。

## 13. 代码审计清单

实现过程中至少检查下面几类命中：

1. `rg -n "TaskStatus::Waiting" anemone-kernel/src`
2. `rg -n "update_status_with" anemone-kernel/src`
3. `rg -n "try_to_wake_up|notify\\(" anemone-kernel/src`
4. `rg -n "task_enqueue|local_enqueue|remote_enqueue" anemone-kernel/src`
5. `rg -n "listen_with_timeout|schedule_with_timeout" anemone-kernel/src`

每个命中都要分类：

1. 纯观察路径。
2. 新任务发布或 bootstrap placement。
3. wait begin。
4. wait completion。
5. wait cancel。
6. stale-safe physical placement。
7. 需要迁移的旧旁路。

不能只看是否编译通过。

## 14. 风险点

### 14.1 只加 token，不改入队尾巴

如果只给 listener 增加 `WakeToken`，但 wake 成功后仍调用裸 `task_enqueue()`，旧 wake 尾巴仍可能撞上新一轮 `Waiting`。这不能算闭合。

### 14.2 只改入队尾巴，不加 wait identity

如果只把 `task_enqueue()` 改成遇到非 `Runnable` no-op，但没有 wait identity 校验，旧 wake 仍可能错误完成新 wait。它也许不 panic，但语义仍不证明。

### 14.3 timeout 或 signal 保留旁路

只要 timeout 或 signal 仍可直接把 `TaskStatus::Waiting` 改成 `Runnable` 并入队，就仍然存在第二套 wake 协议。Event 本身改对也不能证明全局闭合。

### 14.4 clean_listener 继续按 tid 删除

按 tid 清理 listener 会误删同一个 task 的新轮次 listener，重新制造 lost wake。

### 14.5 exclusive quota 按弹出节点计数

stale listener 如果能消耗 quota，会让真正仍在等待的 exclusive waiter 漏唤醒。

### 14.6 将 WaitState 做成第二套状态机

`WaitState` 可以保存完成原因，但不能绕过 task `TaskSchedState` 校验独立决定 wake 是否成功。线性化点必须仍是 task 调度等待事务。

### 14.7 current no-op 没有 park latch

如果 `wake_enqueue()` 对 current task 一律 no-op，却没有 `ParkState` 或等价 park latch，pre-park wake 和 post-park wake 的物理 placement 无法证明闭合。

### 14.8 mode-blocked listener 被直接丢弃

被 wake mode 拒绝但仍 armed 的 listener 必须回挂。直接丢弃会让 uninterruptible waiter 或其他 mode-blocked waiter 永久丢失事件队列注册。

### 14.9 mode-blocked listener 依据旧结果直接回挂

`wake_wait()` 返回 `ModeBlocked` 后，listener 已经处于 detached 窗口。若此时 cancel、timeout、signal 或 finish 已经完成/退役同一轮 wait，直接按旧返回值回挂会把 stale listener 重新放回 event 队列。

修复必须是回挂前再校验，并且再校验与回挂共享同一条可证明顺序。单独读一次 `WaitState::Armed` 后释放锁再回挂，仍然不闭合。

## 15. 修订版完成标准

这份计划对应的实现完成标准是：

1. 已知 `Event::listen*()` 与 `publish()` race 不再触发 `processor.rs` 的 `Runnable` 断言。
2. `Event`、timeout、signal、cancel 的等待完成都通过同一 wait core。
3. wake 尾巴使用 stale-safe placement。
4. listener identity 基于 `Arc::ptr_eq` 语义。
5. exclusive quota 基于成功完成的 wait。
6. mode-blocked listener 回挂通过再校验 helper 完成，detached 窗口不会留下已完成或已取消的 stale listener。
7. 旧直接 wake API 不再被普通等待路径使用。
8. 已知触发 profile 能跑过原先的随机 panic 点。

如果其中任一项不成立，只能认为实现处于过渡状态，不能声明协议闭合。
