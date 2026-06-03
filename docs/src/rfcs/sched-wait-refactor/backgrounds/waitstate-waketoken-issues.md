# WaitState/WakeToken 问题清单

日期：2026-05-31

更新：2026-06-01，补充审查分级、停止边界和当前迁移前仍需处理的设计/API 问题。

来源：

1. [Sched Wait Refactor 不变量需求](../invariants.md)
2. [Sched Wait Refactor 迁移实施计划](../implementation.md)
3. [WaitState/WakeToken 2026-06-01 单文件归档](./waitstate-waketoken-plan-monolith.md)
4. 对拆分前计划的 code review 结论
5. 2026-06-01 对 `publish()` / `wake_wait()` / listener 回挂路径的追加 review

## 0. 审查分级

后续 issue 只按下面标准升级，不再把所有设计疑问都当成阻塞项。

### P0：必须修

会导致错误结果、数据损坏、安全问题、崩溃、严重不可恢复状态。

### P1：通常要修

当前不会立刻炸，但会明显阻碍后续开发，例如模块边界错位、状态所有权混乱、核心抽象方向错误。

### P2：记录，除非顺手

设计不够优雅、局部耦合、命名一般、测试不够舒服、可以重构但不影响主线。

### P3：默认不修

纯风格偏好、理论洁癖、为了抽象而抽象、未来也许用得上。

## 1. 开放问题

当前无 P0/P1 开放项。下面三个 P1 已确认是真实的 plan 文本缺口，但已通过 canonical 需求文档固化；保留在已收口章节作为迁移前检查项。

## 2. 非阻塞设计记录

### 2.1 `ListenerQueueKind` 与双队列可能形成重复事实

级别：P2

状态：记录；不阻塞迁移，除非实现时顺手收敛。

**问题**

计划里的 `Listener` 形状包含：

```rust
struct Listener {
    target: WaitTarget,
    queue: ListenerQueueKind,
}
```

如果 Event 继续保留 `non_exclusive` / `exclusive` 两个队列，那么“listener 属于哪个队列”同时存在于队列位置和 `queue` 字段里。

**为什么是 P2**

这会增加局部耦合和 drift 风险，但只要实现保证字段和实际队列一致，不必然破坏等待轮次身份、线性化点、exclusive quota 或 stale-safe placement。不应把它当作协议未闭合。

**建议**

1. 若保留双队列，`Listener` 不带 `queue` 字段；detached 回挂时用 `DetachedListener { queue_kind, listener }` 携带原队列类别。
2. 若改成单队列，`queue` / mode 字段才作为节点分类事实。

## 3. 已收口问题

### 3.1 `wake_wait()` 的 placement 归属仍需钉死

级别：P1

状态：已收口，见 canonical 需求文档的核心 API 和线性化点描述。

**问题**

拆分前计划已经要求 wake 后的物理入队走 stale-safe placement，但 API 草案仍只写：

```rust
pub fn wake_wait(...) -> WakeResult;
pub fn wake_active_wait(...) -> WakeResult;
```

`publish()` 示例中 `WakeResult::Woke` 只增加 exclusive success 计数，没有明确 `wake_wait()` 是否已经完成 post-commit `wake_enqueue()`。

**为什么是 P1**

这不是单纯优雅问题。若实现者把 `wake_wait()` 理解为只提交逻辑 wake，再让 Event、timer、signal 各自决定是否调用 `wake_enqueue()`，就会把物理 placement 策略泄漏到适配层；若某个路径漏掉 placement，会变成丢 wake。抽象不变量是“逻辑 wake/cancel 在 wait core 线性化，事务提交后只剩 stale-safe physical placement”，API 必须承载这个边界。

**需要补的点**

1. 明确 `wake_wait()` / `wake_active_wait()` 是唯一的“逻辑完成 + 释放 task sched-state lock 后执行 stale-safe placement”入口。
2. Event、timer、signal 不直接调用裸 `task_enqueue()`，也不自行决定 wake 成功后的 placement。
3. `WakeResult` 可以携带或记录 `WakeEnqueueResult` 供诊断，但不能把入队责任交给适配层。

**收口条件**

1. `wake_wait()` / `wake_active_wait()` 的语义明确包含 post-commit stale-safe `wake_enqueue()`。
2. `WakeResult::Woke` 不要求调用方再执行 placement。
3. `WakeEnqueueResult` 只作为诊断结果，不改变入队责任归属。

### 3.2 `TaskStatus` 仍出现在 wait core 写路径描述里

级别：P1

状态：已收口，见 canonical 需求文档的目标、核心 API 和线性化点描述。

**问题**

拆分前计划前面要求 `TaskStatus` 只能作为 `TaskSchedState` 的兼容观察投影，不能再作为写入口；但线性化步骤仍写着验证/设置 `task.status`：

```text
验证 task.status 是该 wake mode 允许唤醒的 Waiting
将 task.status 置为 Runnable
将 task 状态置为 TaskSchedState::Runnable
```

**为什么是 P1**

这是状态所有权混乱，不是命名或文风问题。迁移尚未开始时它不会立刻造成运行时崩溃，但如果按字面实现，会重新引入 `TaskStatus` 与 `TaskSchedState` 两套写入事实，破坏“单 active wait”和“单一线性化点”的证明。实现后若真的形成双重真相源，应升级为 P0。

**需要补的点**

1. wait core 的事务只读写 `TaskSchedState`。
2. wake mode 判断从 `TaskSchedState::Waiting { interruptible, .. }` 得出，不再通过 `task.status`。
3. `task.status()` 只作为观察接口，从 `TaskSchedState` 投影出旧 `TaskStatus` 视图。

**收口条件**

1. 目标和线性化点不再描述 `TaskStatus` 写入。
2. wake mode 判断来自 `TaskSchedState::Waiting { interruptible, .. }`。
3. `task.status()` 保留为兼容观察投影，而不是 wait core 写入口。

### 3.3 `RequeuePermit` 类型边界需要足够硬

级别：P1

状态：已收口，见 canonical 需求文档的核心 API 和 Event 回挂 helper 描述。

**问题**

拆分前计划已经提出 mode-blocked listener 回挂必须先从 wait core 获取短寿命 `RequeuePermit`，再用 permit 回挂 listener。这个方向正确，但需要把 capability 的边界写硬，避免后续实现把它退化成普通查询 API 或可缓存状态。

**为什么是 P1**

这里关系到锁序例外和回挂再校验是否可证明。`requeue_blocked_listener_if_current_armed()` 是唯一允许 `task sched-state lock -> event lock` 的窄路径；如果 `RequeuePermit` 可复制、可构造、可长期保存，Event 就可能绕过 wait core 的所有权边界，重新制造 listener 回挂和 wait 状态之间的双重事实。

**需要补的点**

1. `RequeuePermit` 字段私有，不提供外部构造入口，不实现 `Clone` / `Copy`。
2. permit 绑定 task sched-state guard 或等价借用，生命周期不能逃逸出同一次回挂调用。
3. permit 只能被 Event 私有 `requeue_blocked` 消费，不能用于 `wake_wait()`、runqueue placement、timeout/signal 完成逻辑或任意反向进入 wait core 的调用。

**收口条件**

1. `RequeuePermit` 被定义为 wait core 私有构造的 capability。
2. permit 字段私有，不实现 `Clone` / `Copy`，不能缓存到 listener。
3. permit 只能交给 Event 私有回挂函数消费，不能作为普通查询结果或跨路径授权使用。

### 3.4 wake 后的物理入队丢唤醒窗口

级别：P0 级协议风险，已在 canonical 需求文档中固化。

状态：已收口，见 canonical 需求文档的 park latch 与 stale-safe wake enqueue 设计。

**问题**

`wake_wait()` 完成逻辑 wake 后，如果 wake tail 对 current task 一律 no-op，而 waiter 正处在 `wake_wait()` 到 `schedule()` 的 handoff 窗口，可能出现“逻辑上已唤醒，但物理上没有正确 placement”的丢 wake。

**收口条件**

1. `TaskSchedState::Waiting` 携带 `ParkState::PrePark` / `ParkState::Parked`。
2. `schedule()` 在真正 park 前提交 park latch 并重读状态，发现 wait 已完成则 abort park。
3. `wake_enqueue()` 对 stale、pre-park current、park-pending、already queued、enqueued 有可区分结果。

### 3.5 `publish()` 对 mode-blocked listener 处理不清楚

级别：P0 级协议风险，已在 canonical 需求文档中固化。

状态：已收口，见 canonical 需求文档的 `ModeBlocked` 和 requeue 规则。

**问题**

如果被 wake mode 阻止但仍 armed 的 listener 被直接丢弃，会让 uninterruptible waiter 或其他 mode-blocked waiter 永久丢失 event 队列注册。

**收口条件**

1. 被 wake mode 阻止且仍 armed 的 listener 必须回挂。
2. stale、already completed、already cancelled 的 listener 可以丢弃。
3. exclusive quota 只按成功完成的 wait 计数。
4. 本轮 publish 扫描最多处理进入本轮时已有的原始节点，避免本轮回挂导致无限旋转。

### 3.6 mode-blocked listener 的 detached requeue 窗口缺少再校验

级别：P0 级协议风险，已在 canonical 需求文档中固化。

状态：已收口，见 canonical 需求文档的 `requeue_blocked_listener_if_current_armed()` / `RequeuePermit` 设计。

**问题**

`wake_wait()` 返回 `ModeBlocked` 后，listener 已经处于 detached 窗口。若此时 cancel、timeout、signal 或 finish 已经完成/退役同一轮 wait，直接按旧返回值回挂会把 stale listener 重新放回 event 队列。

**收口条件**

1. 回挂前重新确认 listener 仍对应 task 当前 active wait。
2. `WaitState` 仍为 `Armed`。
3. 当前 wait 仍被本次 `WakeMode` 阻止。
4. 再校验和回挂通过短寿命 `RequeuePermit` 共享同一条可证明顺序，失败则丢弃 listener。

## 4. 审查停止边界

这份清单只继续追问 P0/P1，P2 只记录，P3 默认不进入清单。

应继续审查的情况：

1. 缺失会改变等待轮次身份、线性化点、状态转移、锁序或状态所有权。
2. 缺失会改变 wake / cancel / timeout / signal 的可见语义。
3. 缺失会改变 listener 离队、回挂、exclusive quota 或 stale-safe placement 的结果。
4. 缺失会让 Event、timer、signal、runqueue 或 Task 之间出现双重真相源。
5. 缺失会让 wait core 的 capability 边界退化成外部可伪造、可缓存或可滥用的普通状态。

应停止 issue 查找的情况：

1. 方案已经明确协议边界，但没有规定具体代码形状。
2. 迁移还没开始，某个 API 只是暂未落地。
3. 只是实现路径选择不同，但不改变上面的不变量。
4. 问题属于 P2，且不会影响当前迁移主线。
5. 问题属于 P3。

## 5. 结论

当前 canonical 需求文档已经收口原始 P0 协议缺口：wait identity、逻辑完成、listener 回挂、exclusive quota、park latch 和 stale-safe placement 都有对应边界。

2026-06-01 复审确认的 3 个 P1 是真实存在的 plan 文本缺口，但不是当前旧代码的新运行时 bug；它们已经通过 canonical 需求文档收口：

1. 明确 `wake_wait()` / `wake_active_wait()` 拥有 wake 成功后的 stale-safe placement。
2. 从 wait core 事务描述里移除 `task.status` 写入口，只保留 `TaskSchedState` 单一真相源。
3. 把 `RequeuePermit` 写成真正的短寿命 capability，而不是普通查询结果。

`ListenerQueueKind` 与双队列的重复事实按 P2 记录，不阻塞迁移。
