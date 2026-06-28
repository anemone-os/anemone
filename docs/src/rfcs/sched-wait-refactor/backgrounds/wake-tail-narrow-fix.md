# Wake-tail 条件入队窄化修复草案

日期：2026-05-31

状态：历史备选；已否决为完整方案，不能替代 canonical WaitState/WakeToken 协议。

本文仅供背景阅读。当前实现计划以 [Sched Wait Refactor 不变量需求](../invariants.md) 和 [Sched Wait Refactor 迁移实施计划](../implementation.md) 为准。

相关材料：

1. [Wait/Wake Race 问题简述](./wake-race-problem-brief.md)
2. [Sched Wait Refactor 不变量需求](../invariants.md)

## 0. 目标

这份草案只处理当前已观察到的 `Event::publish()` / `try_to_wake_up()` wake 尾巴竞态：

1. wake 路径已经把 task 从 `Waiting` 改成 `Runnable`。
2. 随后执行物理入队。
3. 物理入队晚到时，task 可能已经运行过，并进入下一轮 `Waiting`。
4. 旧 wake 的入队尾巴调用裸 `task_enqueue()`，触发 `Runnable` 断言。

本草案的目标是把 wake 后的物理 placement 改成 stale-safe：旧 wake 尾巴晚到时只能被观察为不可入队并丢弃，不能 panic，也不能把非 runnable task 投进 runqueue。

本草案不是完整 wait/wake 协议重构。它不尝试证明旧 listener 不会逻辑唤醒新一轮等待。

## 1. 非目标

本草案不解决：

1. listener 缺少等待轮次身份的问题。
2. 旧 listener cleanup 误删同 task 新 listener 的问题。
3. 旧 `publish()` 在 task 进入新一轮 `Waiting` 后误完成新等待的问题。
4. timeout、signal、event、futex、poll 等等待源的统一完成协议。
5. `schedule()` 与 wait 初始化之间所有 pre-park / abort-park 窗口的完整形式化。

这些问题仍属于 [Sched Wait Refactor 不变量需求](../invariants.md) 的范围。该草案只能作为历史备选或临时止血参考，不能替代完整 WaitState/WakeToken 协议。

## 2. 核心策略

保留现有 `task_enqueue()` 的强断言语义，新增一个 wake-tail 专用入口：

```rust
pub fn enqueue_woken_task_if_runnable(task: Arc<Task>);
```

`task_enqueue()` 继续用于“调用者已经证明 task 必须是 runnable 且需要投递”的路径，例如新建 task、明确 runnable task 的普通投递。

`enqueue_woken_task_if_runnable()` 只用于 wake 路径中 `Waiting -> Runnable` 状态事务已经成功提交后的物理入队尾巴。它不是通用容错入口，也不返回 wake 成功或失败。

这个接口的语义是：

1. 在 task 的 owner CPU 上观察 task 当前是否仍可入队。
2. 如果当前仍是 `Runnable`、不是正在运行的 current、也不在 runqueue 上，则入队。
3. 否则直接 no-op。

no-op 不表示 wake 失败。它只表示这个 wake tail 到达时已经没有需要执行的物理 placement。

## 3. 不变量

### 3.1 保留的强不变量

1. runqueue 中只能出现 `TaskStatus::Runnable` 的 task。
2. 非 wake-tail 路径继续使用 `task_enqueue()`，违反 runnable 前置条件仍应 panic。
3. `enqueue_woken_task_if_runnable()` 不修改 `TaskStatus`。
4. `enqueue_woken_task_if_runnable()` 不把 `Waiting` / `Zombie` task 入队。
5. `enqueue_woken_task_if_runnable()` 不重复入队已经在 runqueue 上的 task。
6. `enqueue_woken_task_if_runnable()` 不把当前正在运行的同一 task 再塞回 runqueue。

### 3.2 owner CPU 不变量

Anemone 当前已经有 runqueue membership 标记：`SchedEntity::on_runq`。这个字段必须继续作为 task 是否已经在 runqueue 上的事实源，不引入第二个 `on_rq`。

`on_runq` 的现有约束是：只能在拥有该 task 的 CPU 上访问。条件入队必须遵守这个约束：

1. `enqueue_woken_task_if_runnable()` 对远程 task 不能在当前 CPU 直接读取 `on_runq`。
2. 远程 wake 必须投递到 task owner CPU，由 owner CPU 执行本地条件入队。
3. owner CPU 执行本地条件入队时必须关中断。
4. 本地条件入队必须在同一个 owner-CPU 临界区内完成 status 观察、current 判断、`on_runq` 判断和入队。
5. IPI handler 不能继续调用裸 `local_enqueue()` 处理 wake-tail，否则 stale 仍可能在远端触发断言。

### 3.3 wake-tail 局部不变量

对一次 wake 来说，逻辑 wake 和物理入队分成两步：

1. `try_to_wake_up()` / `notify()` 在 task status 锁保护下执行 `Waiting -> Runnable`。
2. 状态转换成功后，调用 `enqueue_woken_task_if_runnable()` 做物理 placement。

如果第 2 步观察到 task 仍是 `Runnable`，它可以执行 placement，或因 task 已经是 current / 已在 runqueue 而 no-op。

如果第 2 步观察到 task 已不是 `Runnable`，它必须 no-op。这表示 wake tail 已经过期，不表示 wake 逻辑失败，也不表示可以放宽入队条件。

### 3.4 API 使用不变量

1. `enqueue_woken_task_if_runnable()` 只能在“调用者刚刚赢得一次 wake 状态转换”之后使用。
2. 不能用 `enqueue_woken_task_if_runnable()` 替换所有 `task_enqueue()`。
3. 该函数的 no-op 只能被 wake-tail 调用点解释为旧尾巴不需要 placement。
4. 任何新调用点如果不能说明自己是 wake-tail，都必须继续使用 `task_enqueue()` 或先补证明。
5. 该函数不返回结果，调用者不能根据它推导 wake 成功、wake 失败、是否超时或是否被信号打断。

## 4. 可证明性

本草案可以证明的是 wake-tail placement stale-safe。

证明轮廓：

1. `try_to_wake_up()` 只有在观察到允许的 `Waiting` 状态时才提交 `Runnable`。
2. 提交后，task 已经具备运行资格。后续入队只是物理 placement。
3. 在 owner CPU 的本地条件入队临界区内重新读取 task status。
4. 如果 status 仍是 `Runnable`，则检查 current / on-runq 后入队或 no-op。runqueue runnable 不变量保持成立。
5. 如果 status 已变成 `Waiting` 或 `Zombie`，则不入队。runqueue runnable 不变量仍保持成立。
6. 因为 `enqueue_woken_task_if_runnable()` 不修改 status，所以它不能把一个新等待错误地改回 runnable。
7. 因为 current / on-runq 判断和入队都发生在 owner CPU 关中断临界区内，当前 CPU 上不会有另一个合法执行者同时修改该 task 的 runqueue placement。
8. 远程 wake 通过 IPI 把条件入队移动到 owner CPU 执行，因此不会违反 `on_runq` 只能由 owner CPU 访问的约束。

这个证明只依赖当前 task status 和 runqueue placement 检查，不依赖 listener 身份。

## 5. no-op 容忍的含义

这里的“容忍”不是允许错误继续执行，而是把 wake-tail 的过期情况从 panic 改为 no-op：

1. task 已经不是 `Runnable`：旧 tail 不再有权入队。
2. task 已经是当前正在运行的 task：不能重复入队。
3. task 已经在 runqueue 上：不能重复入队。
4. task 仍是 `Runnable` 且没有 placement：本次调用完成入队。

因此 `enqueue_woken_task_if_runnable()` 的容错是 fail-closed：无法证明可以入队时，它拒绝入队。这个函数没有返回值，是为了防止调用者把 placement 结果误当作 wake 语义结果。

## 6. 无法保证什么

本草案不能保证旧 listener 不会完成新一轮等待。

典型无法排除的交错：

1. task 在第 N 轮等待中注册 listener。
2. event 取走这个 listener，但尚未调用 wake。
3. task 因其他原因醒来，清理或重入等待。
4. task 进入第 N+1 轮等待。
5. 旧 event listener 调用 `try_to_wake_up()`，看到 task 当前是 `Waiting`，于是把第 N+1 轮等待改成 `Runnable`。

只要 listener 仍只以 task 身份表示等待者，这类逻辑误唤醒就无法靠条件入队区分。

本草案也不能完整证明 pre-park 窗口：

1. waiter 已经把状态设为 `Waiting`，但还没有真正 `schedule()`。
2. waker 把它改回 `Runnable`。
3. wake tail 看到它是 current，于是 no-op。
4. waiter 随后调用 `schedule()`。

当前 `schedule()` 会重新读取 task status。若 status 是 `Runnable`，它会 requeue current；若 status 是 `Waiting`，它会 park。因此在现有模型下，current no-op 不应直接造成丢 placement，但这只是基于当前 `schedule()` 行为的局部论证，不是完整 wait/wake 协议证明。

## 7. 实现建议

第一阶段只做最小改动：

1. 在 `sched::processor` 中增加 `enqueue_woken_task_if_runnable()`。
2. 本地实现只在 owner CPU、关中断、访问本地 `PROCESSOR.runq` 时读取 `on_runq`。
3. 远程实现不能直接读取 `on_runq`，必须发送 wake-tail IPI，让目标 CPU 调用本地条件入队 helper。
4. IPI handler 的 wake-tail 分支不能调用裸 `local_enqueue()`。
5. `try_to_wake_up()` 中 `task_enqueue(task.clone())` 改为 `enqueue_woken_task_if_runnable(task.clone())`。
6. `notify()` 中 wake 成功后的入队同样改为 `enqueue_woken_task_if_runnable(task.clone())`。
7. 可以为 no-op 分支加 debug 日志或计数器，但不要把这些原因暴露为 API 返回值。
8. 保留 `task_enqueue()` 和 `local_enqueue()` 的断言，不扩大容错范围。

第二阶段如果仍观察到逻辑误唤醒，再引入最小等待身份，例如 per-listen token 或 listener generation。不要把第二阶段混入本次止血补丁。

## 8. 接受标准

1. 原先由旧 wake tail 撞上新 `Waiting` 触发的 `task_enqueue()` panic 消失。
2. runqueue 中不会出现非 `Runnable` task。
3. 新建 task 和普通 runnable 投递路径仍保留强断言。
4. 远程 wake-tail 的 IPI handler 不再通过裸 `local_enqueue()` 处理 placement。
5. 文档和日志都明确 no-op 是旧 wake tail 不需要 placement，不是 wake 语义成功或失败的通用结果。
6. 如果后续出现错误唤醒或丢唤醒，不能把本草案当成已经证明 listener 语义正确。
