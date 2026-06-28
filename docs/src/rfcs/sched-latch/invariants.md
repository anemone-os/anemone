# Sched Latch 不变量需求

**状态：** Protocol Closed
**最后更新：** 2026-06-03
**父 RFC：** [RFC-20260603-sched-latch](./index.md)

本文只定义 `Latch` 原语必须保持的协议边界和证明义务；具体落地顺序见 [Sched Latch 迁移实施计划](./implementation.md)。

本文当前在文档协议层已闭合：单轮 wait identity、owner-bound `Latch`、producer capability、register gate、source wake 线性化、stale-safe placement、cleanup 非正确性支柱和 `ppoll` / `pselect6` outcome mapping 均有硬性边界。实现阶段只能证明这些边界被满足；不能在实现中重新定义它们。

## 闭合条件

`Latch` 的目标是在 wait core 上提供一个单轮次 OR 组合器，供 `poll` / `select` 等场景使用。

迁移完成后必须同时满足：

1. 一个 `Latch` 只对应一轮 wait core wait identity。
2. 同一轮 `Latch` 只有一个 consumer 持有 waiter lifecycle。
3. 同一轮 `Latch` 可以派生多个 producer trigger。
4. 任意 producer 的第一发有效 trigger 完成本轮等待。
5. 旧 trigger 不能完成新一轮等待。
6. timeout、signal、force、consumer cancel 和 producer trigger 竞争同一个 `WaitState`。
7. 任何完成本轮 wait 的路径都由 wait core 负责逻辑完成和 stale-safe physical placement。
8. 被唤醒只表示“可能有源 ready”，consumer 必须重新检查实际 predicate。
9. `poll` / `select` 只有在所有参与阻塞判定的 source 都 ready、armed 或明确 unsupported/fallback 后，才允许进入 schedule。

如果任一条件不成立，当前实现只能视为草稿或迁移中间态，不能声明 `poll` / `select` 等 OR 等待语义闭合。

## 非目标

本需求不包含：

1. 替换 `Event` 或把所有同步原语统一成 `Latch`。
2. 完整实现 Linux waitqueue、epoll、futex PI 或异步通知框架。
3. 引入可跨等待轮次保存的 notification permit。
4. 引入 count-down latch、barrier 或 AND wait 语义。
5. 让 fd/device source 直接操作 task 调度状态或 runqueue。
6. 通过 busy polling 修复 `poll` / `select` 的阻塞语义。

这里要固定的是“一轮 wait round 的 OR completion capability”，不是完整 I/O 多路复用框架。

## 状态所有权

`Latch` 不拥有新的调度状态。它只是 wait core active wait 的上层封装。

硬性要求：

1. 当前 task 的等待状态仍由 `TaskSchedState::Waiting { state, interruptible, park }` 表达。
2. `Latch` 不保存第二套 `armed: AtomicBool` 作为正确性真相。
3. `LatchTrigger` 不保存第二套完成状态机。
4. producer trigger 不得直接写 `TaskStatus`、`TaskSchedState` 或 runqueue。
5. `Latch` finish 后，本轮 wait 必须进入 wait core 的 retired 或等价终态。
6. 每个 `Latch::begin*()` 后必须 exactly-once `finish()` 或等价 retire；所有 early return 都必须通过统一 guard 或显式 `cancel + finish`。
7. `cancel()` 只能作为同一 wait core transaction 上的一次 completion attempt；如果输给已有 completion，不得覆盖 winning outcome。

允许的辅助状态只用于资源管理或诊断，例如注册清理列表、debug id、source 计数和 lazy pruning 统计；这些状态不能参与决定本轮 wait 是否已完成。

## 身份与能力模型

### 名称语义

`Latch` 表示一轮等待被触发后闭合。它的核心语义是 one-shot completion。

建议将角色拆开命名：

```rust
pub struct Latch {
    // waiter-owned, not Clone
}

#[derive(Clone)]
pub struct LatchTrigger {
    // producer-held restricted wake capability
}
```

命名要求：

1. `Latch` 本体由 consumer 持有，不实现 `Clone`。
2. `LatchTrigger` 或等价 producer handle 可以 clone 给多个 producer。
3. 文档中不得把 `Latch` 描述为可重复事件源。
4. 若代码中出现 `park latch`、`WaitLatch`、`NotifyLatch` 等相近术语，必须在模块注释中区分含义。

### 等待轮次身份

`Latch` 的身份必须来自 wait core 的 `WaitState` 指针身份，而不是 `Latch` 对象地址、task tid、fd 编号或 source-local generation。

硬性要求：

1. `Latch::begin*()` 每次创建新的 wait core wait round。
2. 从同一 `Latch` 派生出的所有 trigger 必须指向同一个 `WakeToken` 或等价 wait identity。
3. source 持有旧 trigger 时，后续 trigger 只能看到 stale / already completed / retired，不能完成新 wait round。
4. `Latch` 不得复用已 finish 的 `WakeToken` 创建下一轮等待。

这保证 `poll` / `select` 的循环可以每轮重新创建 `Latch`，而旧 fd source 队列中残留的 trigger 只会失效，不会误唤醒下一轮 syscall wait。

### 单 consumer 与多 producer

`Latch` 是 single-consumer primitive。

要求：

1. 只有创建 `Latch` 的 task 可以 schedule、cancel 和 finish 本轮等待。
2. consumer-side handle 不可 clone，不可跨 task 转移使用。
3. `Latch` 必须是 owner-bound linear guard：字段私有，不可伪造，`finish(self)` 消耗对象。
4. 实现必须让 `Latch` `!Send` / `!Sync`，或让所有 waiter-owned 方法硬校验 `current_task == owner_task` 并 fail closed。
5. producer-side trigger 可 clone，但只暴露 trigger 能力。
6. producer 侧普通 API 必须 no-return / fail-closed；诊断返回值只能用于内部日志和测试，不得成为 source 行为分支的依据。
7. producer 触发失败不得 panic，不得补做 enqueue，不得重试完成其他 wait round。
8. `LatchTrigger` 不公开 `WakeToken`、强 `Task` 引用或任何 wait-core waiter lifecycle API。

若未来需要多 consumer 语义，应另建 `Event`、wait queue 或 epoll-like 对象，不能扩展 `Latch` 本体。

## 线性化点

producer trigger 必须通过 wait core 完成本轮等待。

建议新增或等价表达：

```rust
pub enum WaitReason {
    Latch,
    // existing reasons...
}
```

trigger 路径要求：

1. trigger 使用 `wake_wait(task, token, WaitReason::Latch, WakeMode::AnyWait)` 或等价入口。
2. `WakeResult::Woke` 表示 wait core 已完成逻辑 wake 并做过 stale-safe placement。
3. `WakeResult::Stale`、`AlreadyCompleted`、`AlreadyCancelled`、`Retired` 都是合法失败结果。
4. producer 不得把 `WakeResult::Woke` 解释为 fd readiness 已经仍然成立。
5. producer 不得根据 `WakeEnqueueResult` 自行补调 `task_enqueue()`。

`WaitReason::Event` 不应复用于 `Latch`，否则后续日志、panic 诊断和 syscall outcome 分类会把两种原语混在一起。

timeout、signal、force 和 consumer cancel 的线性化要求：

1. timeout callback 持有同一轮 token，通过 wait core 完成 `WaitReason::Timeout`。
2. signal / force 仍通过 `wake_active_wait()` 完成当前 active wait。
3. producer trigger、timeout、signal 和 force 一旦把 `Waiting` 线性化为 runnable / completed，必须由 wait core 负责 stale-safe placement。
4. consumer 在 predicate ready、注册失败、timeout zero、signal precheck 或错误返回时，通过 waiter-owned cancel 结束本轮。
5. consumer cancel 使用受限的 `LatchCancelReason` / `ConsumerCancelReason` 或等价枚举，不直接暴露 wait-core `WaitReason` 命名空间。
6. finish 后必须根据 wait outcome 和实际 predicate 共同决定 syscall 返回值。
7. `Latch` 不得引入独立 `AtomicBool validness` 作为 timeout 或 trigger 有效性依据。

syscall outcome mapping 的最低规则：

1. finish 后先做 final readiness scan。
2. 如果 ready 可见，返回 ready。
3. 如果 final scan 仍无 ready，再按 winning outcome 映射 timeout、signal/force、cancel/error。
4. cancel-only 是内部 early-return 终态，不应暴露为 producer wake 或 source readiness。
5. `ppoll` 与 `pselect6` 必须共享同一套 outcome mapping。

## 锁序与生命周期规则

### Poll / Select 注册协议

`poll` / `select` source 注册必须是“检查条件并注册 trigger”的同一 source-side 临界区。

抽象要求：

1. consumer 先进行 snapshot scan；已经 ready 则不创建 `Latch`。
2. timeout zero、signal precheck 或 deadline 已过时，不创建 `Latch`。
3. 未 ready 且确实需要阻塞时，consumer 创建 `Latch`，再携带 trigger 做 register scan。
4. 每个 source 在自己的状态锁下检查 readiness；若已经 ready，返回 `Ready(events)`，不注册 trigger。
5. 若未 ready 且支持 latch wait，source 在同一状态锁下保存本轮 trigger，并返回 `Armed`。
6. 若 source 不能参与本轮 latch wait，必须返回 `Unsupported` / `NotRegistered` 或等价状态；syscall 不得在该 source 未 armed 的情况下进入 schedule。
7. consumer 被唤醒后必须清理或使本轮注册失效，并重新 snapshot scan。

这样才能闭合注册窗口：状态变化如果发生在注册前，register scan 能直接看到 ready；如果发生在注册后，source 持有的 trigger 能完成当前 wait round。

source wake 侧要求：

1. 任何使 predicate 变 ready / hangup / error 的状态变更，必须在同一 source lock 下完成 predicate update 和对应 trigger 队列 detach。
2. 释放 source lock 后只能执行 detached trigger，不能再改变本次 wake 所依赖的 ready 状态。
3. source detach 后再 trigger，避免 source lock 与 task sched-state lock 形成不可控锁序。

### Source 队列与生命周期

持久 source 队列不得因为 consumer 提前返回或异常路径而形成正确性依赖。

要求：

1. source 队列可以保存 weak trigger 或保存可被 stale-safe wake 拒绝的 strong trigger，但必须有 lazy pruning 或显式 cleanup 策略。
2. 正确性不能依赖显式 cleanup 一定执行；cleanup 只能改善队列卫生和资源占用。
3. source 不得强持有 `Task` 形成 `Task -> File -> Source -> Task` 环。
4. 如果 source 队列保存 strong `WakeToken`，必须审计 per-syscall 残留对 `WaitState` 生命周期的影响，并提供上界或 lazy pruning。
5. weak/strong 策略必须在 source 注册协议落地前确定，不能留到 `ppoll` 迁移后再补。

首版实现可以选择“weak trigger + lazy pruning”降低异常路径风险；若实现复杂度过高，也可以用 strong trigger 起步，但必须在实施文档中把资源上界和清理点列为验收项。

## 模块边界

建议模块边界：

1. `sched::wait` 继续拥有 wait identity、completion、cancel、finish 和 stale-safe placement。
2. `sched::latch` 只封装 single-consumer / multi-producer one-shot round。
3. `fs::iomux` 只定义 `PollRequest`、poll interest、source 注册协议和 syscall glue。
4. fd/device source 只维护自己的 readiness predicate 和 trigger 队列。
5. `Event` 不作为 `Latch` 的实现依赖。

`sched::latch` 可以调用 wait core，但 fd/device source 不能直接进入 wait core 的 waiter lifecycle。

## 禁止退化项

以下退化会破坏证明，不能作为实现 shortcut：

1. 让 `Latch` 变成可复用事件源或跨轮次 permit。
2. 让 source 直接写 `TaskStatus`、`TaskSchedState` 或 runqueue。
3. 让 source 在持有 source lock 时进入 wait core wake。
4. 让 `LatchTrigger` 暴露 `WakeToken` 本体或 `WakeResult` 分支给普通 source 行为。
5. 让 cleanup 成为 old trigger fail-closed 的正确性支柱。
6. 让 timeout、signal 或 force 走绕过 wait core stale-safe placement 的完成路径。
7. 让 `ppoll` / `pselect6` 分别解释 ready / timeout / signal / cancel race。
8. 让 busy polling 被描述为 `poll` / `select` 阻塞语义闭合。

## 可观测性

日志或 trace 至少应能回答：

1. 某个 `Latch` 属于哪个 task、哪一轮 wait id。
2. 某个 trigger 来自哪个 source 注册点。
3. trigger 结果是 woke、stale、already completed、already cancelled 还是 retired。
4. wake 成功后的 placement 结果是什么。
5. consumer finish 时 outcome 是 latch、timeout、signal、force、predicate ready 还是 cancel。
6. poll/select 最终返回前的实际 readiness scan 结果。

日志只能辅助诊断，不能成为协议正确性依赖。

## 完成标准

可以声明文档协议闭合的情况：

1. 闭合条件全部满足。
2. 状态所有权、身份与能力模型、线性化点、锁序与生命周期规则、模块边界、禁止退化项和 outcome mapping 都在本文中有不可替代的约束。
3. source 注册 gate、source wake detach、weak/strong trigger 生命周期策略、producer capability 和可观测性都有对应实施 gate。
4. tracking issues 中没有 Still open plan gap。

可以声明实现语义闭合的情况：

1. source 注册和 wake 侧线性化点都有实现和审计证据。
2. 旧 trigger late arrival 能稳定 fail closed，且不依赖显式 cleanup 成功。
3. `ppoll` / `pselect6` 使用同一套 final scan 与 outcome mapping。
4. 审计确认没有 fd source 直接写 task sched state、直接调用 wait core lifecycle，或在持有 source lock 时触发 wait core wake。
5. tracking issues 中所有 implementation gate 都有阶段退出证据。

只能声明为迁移中间态的情况：

1. 某个 source 尚未迁移，且 syscall 不会睡在该 source 的未 armed 状态上。
2. 首版 weak/strong trigger 选择已有资源上界和 pruning gate，但尚未迁移所有 fd source。
3. 问题属于 epoll、异步通知或 AND wait 扩展，不影响当前 OR primitive。
4. 仍有 Still open plan gap，或生产路径上仍有未满足的 implementation gate。
