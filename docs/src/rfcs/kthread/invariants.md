# KThread 不变量需求

**状态：** Superseded / historical baseline
**最后更新：** 2026-06-16
**父 RFC：** [RFC-20260614-kthread](./index.md)

本文保留既有 `task::kthread` 实现的协议边界和证明义务。后续 kthread core 纠偏的权威设计见 [RFC-20260616-kthread-core](../kthread-core/index.md)；具体历史落地顺序与 commit 事实见 [KThread 迁移实施计划](./implementation.md) 和 [事务日志](../../devlog/transactions/2026-06-14-kthread.md)。

## 闭合条件

`kthread` 系统被视为闭合时，必须同时满足：

1. `kthreadd` 在 ordinary kthread 创建前初始化，且只初始化一次。
2. ordinary kthread 创建请求经由 `KThreadBuilder` 提交到 `kthreadd`，由 `kthreadd` 在自身 task context 中创建和发布新 task。
3. `KThreadStart<A>` exactly once 回收：创建失败由 submitter 用原始 `A` 回收，创建成功由匹配的 `kthread_entry_shim::<A>` 恢复。
4. 新 task 在 enqueue 前已经安装 task-owned `Arc<KThread>`。
5. `Task` 是 ordinary kthread state 的强 owner；`KThread` 只弱引用 `Task`；外部 `KThreadRef` 也只持有 weak handle。
6. ordinary kthread 生命周期状态只由 `KThreadControl` 维护，不由 `TaskSchedState`、`TaskStatus` 或 `kthreadd` 决定。
7. stop / park 是 cooperative lifecycle request，必须通过 `KThreadContext` 在 entry safe point 观察。
8. entry 返回后必须先进入 `finish_returned_entry()`，再进入 `kernel_exit()`。
9. `kernel_exit()` 必须拒绝 ordinary kthread 未完成 finish path 的退出。
10. `KThreadService` 的 pending backend 是请求真相源；wake event 只提示 worker 重新检查 pending 与 lifecycle。

如果任一条件不成立，当前实现只能视为局部 kernel task helper，不能声明为统一 kthread 生命周期基础设施。

## 非目标

本需求不包含：

1. 完整 `kthread_*` 兼容 API。
2. 用户可见 kthread 管理 ABI。
3. 通用 workqueue、动态 worker pool、freezer、cgroup、CPU hotplug 或优先级继承。
4. 抢占式取消正在运行的 kthread entry。
5. 允许 kthread entry 直接驱动 task 调度状态或 runqueue placement。

## 状态所有权

`Task` 仍然拥有调度、拓扑、credentials、文件状态和 exit 状态。`KThreadControl` 只拥有 ordinary kthread lifecycle：

- `Runnable`
- `Parking`
- `Parked`
- `Stopping`
- `Exited`

这些状态不是 scheduler runnable/waiting/zombie 的替代品。它们只说明 kthread entry 应如何响应 lifecycle request。

硬性要求：

1. `KThreadInner.state` 是 stop / park / exited 的单一真相源。
2. `KThreadInner.exit_code` 只有在 entry 返回并切入 `Exited` 后才稳定。
3. `Task.kthread` 是 task-owned `Arc<KThread>` 的唯一存放点。
4. `KThread.task` 必须是 weak pointer，不能形成 `Task -> KThread -> Task` 强引用环。
5. `kthreadd` 不保存 ordinary kthread 列表，不参与 ordinary kthread 的 stop / park / wake / exit 决策。
6. `KThreadServiceState.pending` 与 `active_workers` 是 service drain / stop 的真相源；`KThreadControl.wake_event` 不是 pending truth。

允许的诊断状态包括 kthread name、tid snapshot、exit code snapshot 和日志字段。这些字段不得反向驱动生命周期协议。

## 身份与能力模型

### 创建身份

ordinary kthread 的拓扑父节点是 `kthreadd`。`creator`、`parent_tgid`、`pgid` 和 `sid` 是当前 topology 模型需要的兼容字段，不是 lifecycle owner。

要求：

1. `init_kthreadd()` 只能在 boot path 调用一次。
2. ordinary kthread 创建函数必须断言当前 task 是 `kthreadd`。
3. ordinary kthread 发布后才允许 task enqueue。
4. create completion 只能在 task publish 和 `Task.kthread` 安装完成后返回成功。

### 外部 handle

`KThreadRef` 是 weak handle。它只能尝试 `upgrade()`，不能保证目标仍存活。

要求：

1. 停止后的 kthread 会 detach task-owned strong reference，后续 weak upgrade 可能失败。
2. ordinary entry 正常返回也会 detach task-owned strong reference。
3. `KThread::drop()` 看到未 `Exited` 状态必须 panic，暴露生命周期 bug。
4. `clear_kthread()` 必须用 `Arc::ptr_eq` 确认清理的是同一个 installed kthread。

### typed start object

`KThreadStart<A>` 的类型参数是启动参数回收的正确性边界。

要求：

1. `KThreadStartPointer` 不得拥有 untyped destructor。
2. submit failure 必须用原始 `A` 调 `reclaim::<A>()`。
3. shim 必须先恢复 `Box<KThreadStart<A>>`，再读取 entry 和 arg。
4. erased `entry: *const ()` 只能还原为与 shim 匹配的 `KThreadEntry<A>`。
5. start object 不得被 service、`Task` 或 `kthreadd` 长期保存。

## 线性化点

### 创建

创建请求的线性化点分为两段：

1. submitter 在 create queue 中插入 `KThreadCreateInfo` 并 publish create event。
2. `kthreadd` 创建并 publish task，安装 `Arc<KThread>`，enqueue task，然后 complete 创建结果。

成功 completion 表示 task 已经发布并可被调度；不表示业务 entry 已开始运行。失败 completion 表示 start object 尚未被 shim 接管，必须由 submitter 回收。

### entry 启动与退出

entry shim 的第一条生命周期动作是恢复 typed start object。随后：

1. 取得当前 task 的 `KThread`。
2. 如已 start-parked，进入 `parkme()` safe point。
3. 若已收到 stop request，返回 `-EINTR`；否则调用业务 entry。
4. 业务 entry 返回后，写入 exit code 并切到 `Exited`。
5. publish state change 和 wake event。
6. detach task-owned kthread strong reference。
7. 调用 `kernel_exit()`。

`kernel_exit()` 的 kthread assertion 是最后的正确性防线：ordinary kthread 不能跳过 kthread finish path 直接退出。

### stop / park / unpark

`stop()` 的线性化点是 `KThreadControl::request_stop()` 把非终态切到 `Stopping`。之后 `wake()` 只负责让 worker 重检 lifecycle。`stop()` 返回值来自 `wait_exited()` 观察到的稳定 exit code。

`park()` 只把 `Runnable` 改为 `Parking`，真正 `Parked` 必须由 running kthread 在 safe point 调 `parkme()` 完成。`unpark()` 可以把 `Parking` 或 `Parked` 改回 `Runnable` 并 wake worker。

### service work

`KThreadService::submit()` 在线性化点内只更新 pending backend；worker wake 在锁外发生。worker `take_work()` 成功时增加 `active_workers`，handler 返回后 `complete_work()` 减少计数并在 pending 为空时 publish drained。

handler 执行时不能持有 service inner lock。handler 内部应使用 `KThreadContext` 检查 stop / park，不应直接访问 service internal state。

## 锁序与生命周期规则

1. create queue lock 只保护 pending create request，不得在持有该锁时执行 `Task::new_kernel()` 或 publish task。
2. `KThreadControl.inner` 只保护 lifecycle state 和 exit code，不能在持有它时调用可能阻塞的业务 handler。
3. `KThreadServiceState.inner` 只保护 pending、stopping 和 active worker 计数；handler 必须在锁外运行。
4. `KThreadContext` 不暴露底层 `Task`，调用方不能通过 context 取得 task internals。
5. boot path 必须先初始化 `kthreadd`，再创建 ordinary kthread service。
6. 可以跨 CPU 调度的 ordinary kthread 应在所有 CPU 完成本地初始化并 online 后发布。

## 禁止退化项

以下模式会破坏本文不变量：

1. 后台服务直接调用 `Task::new_kernel()` 创建 ordinary worker，绕过 `KThreadBuilder`。
2. 在 `TaskSchedState` 中加入 stop / park / service pending truth。
3. `KThread` 强持有 `Task`。
4. `KThreadRef` 变成强引用并长期阻止 exited kthread 回收。
5. `KThreadStartPointer` 增加 untyped `Drop` 或被多个路径回收。
6. kthread entry 直接调用 `kernel_exit()`，不先调用 `finish_returned_entry()`。
7. service worker 把 wake event 当成 work request truth，而不是重新查询 pending backend。
8. service handler 在持有 service inner lock 时执行业务逻辑。

## 完成标准

本 RFC 当前满足文档层闭合：创建代理、typed start lifetime、Task/KThread 引用所有权、lifecycle 状态机、exit assertion 和 service pending/wake 分离均已写入 canonical 文本。

后续只有在以下验证完成后，才能把运行验证也标记为闭合：

1. `just build` 或等价构建通过。
2. 至少一个 `KThreadService` consumer 在 QEMU/user-test 中完成 submit、wake、handler 执行和无 panic exit 路径。
3. 针对 stop / park / drop-without-stop 的行为有明确测试、审计记录或接受限制。
