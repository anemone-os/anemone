# KThread 迁移实施计划

**状态：** Completed / historical baseline
**最后更新：** 2026-06-16
**父 RFC：** [RFC-20260614-kthread](./index.md)
**不变量：** [KThread 不变量需求](./invariants.md)

本文追补记录 `2b0e3900279895c3d8eb604e463249a02c3bddc9` 中已经落地的 kthread 实施形状。后续 kthread core 纠偏以 [RFC-20260616-kthread-core](../kthread-core/index.md) 为准；本文不再追加新的 core 扩展阶段。

## 迁移原则

- kthread 是 task 子系统内部能力，不引入用户 ABI。
- `kthreadd` 只作为创建代理和 topology parent，不拥有 ordinary kthread lifecycle。
- ordinary kthread lifecycle 由 task-owned `KThreadControl` 管理，不侵入 scheduler wait/runqueue 状态。
- typed start object 的 lifetime 必须在创建失败路径和 entry shim 之间 exactly once 闭合。
- 后台服务的 request truth 留在 pending backend；wake event 只作为重新检查提示。

## 阶段 1：创建核心

前置条件：

- `Task::new_kernel()` 和 topology publish guard 已存在。
- `Event` 可用于 create queue 和 completion wait。

交付：

- 新增 `anemone-kernel/src/task/kthread/create.rs`。
- 新增 boot-time `init_kthreadd()`。
- 新增 `KThreadBuilder`、`KThreadEntry<A>`、`KThreadShimEntry`。
- 新增 erased `KThreadStartPointer` 与 typed `KThreadStart<A>` 回收路径。
- ordinary kthread 创建由 `kthreadd` 处理，成功路径在 task enqueue 前安装 `Arc<KThread>`。

审计：

- `kthreadd_create_kthread()` 必须断言当前 task 是 `kthreadd`。
- 创建失败必须 complete `Failed(start, err)`，由 submitter 用原始 `A` 回收 start object。
- 成功 completion 不能早于 task publish、`Task.kthread` 安装和 task enqueue。

write set：

- `anemone-kernel/src/task/kthread/create.rs`
- `anemone-kernel/src/task/kthread/mod.rs`
- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/main.rs`

验证：

- 本轮文档追补未重新运行构建。建议后续至少运行 `just build`。

退出条件：

- ordinary kthread 可以通过 `KThreadBuilder::spawn()` 创建，并返回 weak `KThreadRef`。

## 阶段 2：生命周期状态机

前置条件：

- ordinary task 在 entry shim 启动前已经安装 `KThread`。

交付：

- 新增 `KThreadControl`，管理 `Runnable`、`Parking`、`Parked`、`Stopping`、`Exited`。
- 新增 `KThreadContext`，提供 `should_stop()`、`should_park()`、`parkme()`、`wait_until()`、`wait_until_woken()`。
- 新增 `KThread::stop()`、`park()`、`unpark()`、`wake()` 和 snapshot helpers。
- `kernel_exit()` 调用 `task.assert_kthread_exit_ready()`，拒绝 ordinary kthread 跳过 finish path。

审计：

- `KThreadControl` 状态变化必须 publish `state_changed` 或 wake event，让 stop/park waiter 能继续。
- `finish_returned_entry()` 必须先写 exit code 和 `Exited`，再 detach task-owned kthread。
- `KThread::drop()` 看到未退出状态必须 panic。

write set：

- `anemone-kernel/src/task/kthread/mod.rs`
- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/task/api/exit/mod.rs`

验证：

- 建议补充定向测试或 QEMU 路径，覆盖 entry 正常返回、stop 等待 exit code、start_parked 后 unpark。

退出条件：

- ordinary kthread entry 正常返回后不触发 `kernel_exit()` 的 kthread assertion。

## 阶段 3：服务 worker 抽象

前置条件：

- `KThreadBuilder` 和 cooperative lifecycle 已可用。

交付：

- 新增 `anemone-kernel/src/task/kthread/service.rs`。
- 定义 `KThreadPending` trait，把 pending state 抽象为 backend，不固定为队列。
- 提供 `KThreadPendingSlot` 和 `KThreadPendingQueue`。
- 定义 `KThreadRequestHandler<W>`。
- 实现 `KThreadService` 的 spawn、submit、drain、stop 和 worker loop。

审计：

- `submit()` 只更新 pending truth，再 wake worker。
- worker handler 在 service lock 外运行。
- `complete_work()` 必须断言 active worker 不下溢。
- `StopMode::Drain` 与 `StopMode::DiscardPending` 的语义必须只影响 pending state 和 stop 等待，不绕过 kthread lifecycle。

write set：

- `anemone-kernel/src/task/kthread/service.rs`
- `anemone-kernel/src/task/kthread/mod.rs`

验证：

- 首个 consumer 是 [inode shrinker](../inode-shrinker/index.md)。建议以后用它覆盖 submit、merge slot、worker wake 和 handler 执行路径。

退出条件：

- 单 worker 和多 worker service 都能基于同一 pending backend 合同表达 drain 和 stop。

## 阶段 4：boot 接入与首个 consumer

前置条件：

- BSP 已完成基础 timer、per-cpu、IRQ 初始化。
- 所有 CPU 完成本地初始化后，才发布可能跨 CPU round-robin 的 ordinary kthread。

交付：

- `bsp_kinit()` 中在 `IntrArch::init_local_irq()` 后初始化 `kthreadd`。
- 所有 CPU online 后初始化 inode shrinker ordinary worker。
- inode shrinker worker 自循环执行 pressure polling，不依赖 task exit 提交请求。

审计：

- `kthreadd` 本身不是 ordinary kthread，不能通过 `KThreadBuilder` 创建。
- ordinary kthread worker 不应早于所有 CPU online 发布。

write set：

- `anemone-kernel/src/main.rs`
- `anemone-kernel/src/fs/inode_shrinker.rs`

验证：

- 本轮文档追补未运行 QEMU 或 LTP。建议后续运行 user-test profile，观察 `inode-shrink-0` 是否稳定运行。

退出条件：

- boot 后存在 ordinary kthread consumer，且 `inode-shrink-0` 可以在不依赖 task exit 的情况下进入自己的 lifecycle loop。

## 旁路审计清单

后续扩展 kthread 时，审计目标是确认所有 ordinary kthread 都走统一 lifecycle。实现者需要证明：

- 新增后台 worker 不绕过 `KThreadBuilder` 创建 ordinary kthread。
- ordinary kthread 退出前都会经过 kthread finish path，并能被 `kernel_exit()` 的断言覆盖。
- stop / park / wake / exited 状态仍由 `KThreadControl` 统一拥有。
- scheduler state 和 runqueue placement 没有被用来保存 kthread lifecycle request。
- service worker 的 pending state、active worker 计数和 wake event 仍保持职责分离。

审计结论应把相关路径分类为 bootstrap task、ordinary kthread、user task exit、scheduler placement 或 service worker，避免 ordinary kthread 绕过 lifecycle。

## 可观测性清单

后续 review 至少应能回答：

- 某个 ordinary kthread 是由哪个 create request 创建，名称和 tid 是什么。
- create failure 是否已经回收 typed start object。
- stop / park 请求是否已经被 worker 在 safe point 观察。
- entry 返回后 exit code 是否已经写入 `KThreadControl`。
- service pending 是否为空，active worker 数是否为 0。
- service drop 时是否还存在未 stop 的 worker。

## 停止边界

需要回到 RFC 层的情况：

- 新增能力会改变 `Task` 与 `KThread` 的引用所有权。
- 新增能力会让 `kthreadd` 持有 ordinary kthread lifecycle state。
- 新增能力会把 stop / park 塞入 scheduler state。
- 新增 service backend 需要在 handler 执行期间持有 service lock。
- 新增 worker pool 需要动态创建、销毁或迁移 worker。

可以作为普通小改继续推进的情况：

- 只新增一个遵循 `KThreadService` 合同的后台 service。
- 只补充日志、snapshot 字段或构建修复。
- 只收窄某个 consumer 的 pending merge 逻辑，且不改变 kthread lifecycle。

## Write Set 扩展记录

- 2026-06-15：本文是 commit `2b0e3900279895c3d8eb604e463249a02c3bddc9` 的追补文档，没有新的代码 write set 扩展。
