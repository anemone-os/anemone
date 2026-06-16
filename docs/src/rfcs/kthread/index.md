# RFC-20260614-kthread

**状态：** Superseded / historical baseline
**负责人：** EDGW, Codex
**最后更新：** 2026-06-16
**领域：** task / scheduler / kthread / background service
**事务日志：** [2026-06-14 - KThread](../../devlog/transactions/2026-06-14-kthread.md)
**开放问题：** None；后续 kthread 纠偏以 [RFC-20260616-kthread-core](../kthread-core/index.md) 为准。
**下一步：** 不再基于本文扩展 kthread core；本文只保留 commit `2b0e3900279895c3d8eb604e463249a02c3bddc9` 的历史实现记录。

> Superseded：后续 `kthread` core 的权威设计是 [RFC-20260616-kthread-core](../kthread-core/index.md)。本文继续作为既有实现和事务日志的 historical baseline。

## 摘要

本 RFC 记录 `2b0e3900279895c3d8eb604e463249a02c3bddc9` 中落地的轻量 kthread 系统。该系统提供一个 boot-time `kthreadd` 创建代理、类型化入口参数恢复、普通 kthread 的 stop / park / wake / exited 协作生命周期，以及可复用的 `KThreadService` 后台 worker 抽象。

本文不再作为后续 kthread core 的 accepted contract。它的核心价值是保留当前实现事实：`Task` 仍然是调度和拓扑对象，`KThreadControl` 只拥有普通 kthread 的生命周期状态，`kthreadd` 只负责创建与拓扑父子关系，不拥有 ordinary kthread 的运行状态。

## 背景

内核已经有 `Task::new_kernel()` 可以创建 kernel task，但它只是低层 task 构造入口，没有为长期后台任务提供稳定的外部 handle、stop/park 协议、类型化启动参数回收或服务化 pending queue。`inode_shrinker` 这类后台清理任务需要一个可以协作停止、让出调度器、并在 exit 路径暴露错误退出的基础设施。

本次提交把 kthread 做成 task 子系统内部能力，而不是用户可见 ABI。它复用现有 scheduler、topology、`Event` 和 `kernel_exit()` 路径，但把 kthread 生命周期状态从 scheduler 状态中分离出来，避免制造第二套 task runnable/waiting 真相源。

## 目标

- 通过 `kthreadd` 统一 ordinary kthread 创建路径，并让 ordinary kthread 继承当前 task topology 模型所需的 parent / pgid / sid 字段。
- 提供 `KThreadBuilder`，让调用方以类型化入口 `fn(KThreadContext, A) -> i32` 创建 kthread。
- 保证启动参数对象要么在创建失败路径回收，要么由匹配的 typed entry shim 恢复，不能泄漏或重复释放。
- 让 `Task` 持有 ordinary kthread 的强引用，外部 handle 只持有弱引用，避免 task 与 kthread 形成强引用环。
- 定义 cooperative stop / park / unpark / wake / exited 协议，并在 `kernel_exit()` 中断言 ordinary kthread 必须先完成 kthread finish path。
- 提供 `KThreadService`，让后台服务用 slot 或 FIFO pending backend 表达请求真相，worker wake 只作为重检提示。

## 非目标

- 不提供完整 `kthread_*` 兼容 API surface。
- 不提供用户可见 syscall、procfs 管理接口或稳定 ABI。
- 不实现通用 workqueue、worker pool 动态扩缩容、CPU hotplug binding、freezer、cgroup 或优先级继承。
- 不允许普通 kthread 绕过 `KThreadContext` 直接操作 task 调度状态。
- 不把 stop / park 做成抢占式取消；kthread entry 仍必须在显式 safe point 协作检查。
- 不在本 RFC 中定义 inode shrinker 的 VFS 回收不变量；该部分见 [RFC-20260614-inode-shrinker](../inode-shrinker/index.md)。

## 文档地图

Canonical：

- [不变量需求](./invariants.md)
- [迁移实施计划](./implementation.md)

相关 RFC：

- [RFC-20260614-inode-shrinker](../inode-shrinker/index.md)：首个基于 ordinary kthread 的实际后台清理 worker；当前使用自循环 pressure polling，而不是 `KThreadService` pending backend。

## 方案

`kthreadd` 在 boot-time 由 `init_kthreadd()` 直接创建，不能通过 `KThreadBuilder` 创建，因为 builder 本身会把请求提交给 `kthreadd`。普通 kthread 创建时，调用方把 entry 与 owned argument 封装成 `KThreadStart<A>`，再通过 erased `KThreadStartPointer` 放入全局 create queue。`kthreadd` 创建并发布 `Task` 后，先安装 task-owned `Arc<KThread>`，再 enqueue 新 task，最后通过 completion 唤醒提交方。

ordinary kthread 的入口不是直接调用业务 entry，而是先进入 typed `kthread_entry_shim::<A>`。shim 的第一步恢复 `Box<KThreadStart<A>>`，取得 `KThreadContext`，处理 start-parked safe point，然后运行业务 entry。业务 entry 返回后，shim 调用 `finish_returned_entry()` 把生命周期状态切为 `Exited` 并记录 exit code，再进入 `kernel_exit()`。

`KThreadService` 是一层 service worker 适配。它不规定 pending state 必须是队列，实际 pending backend 可以是 merge slot 或 FIFO queue。`submit()` 只更新 pending truth，然后 wake worker；worker 被唤醒后必须重新从 backend `take()` work。drain 与 stop 只观察 pending 是否为空和 active worker 计数，不把 wake event 当作请求真相源。

## 接受边界

本 RFC 曾作为 `kthread` 系统的 canonical 文档被接受，并追补记录 commit `2b0e3900279895c3d8eb604e463249a02c3bddc9` 的实现形状。自 [RFC-20260616-kthread-core](../kthread-core/index.md) 提升后，本文只作为 historical baseline。

后续纠偏如果改变以下任一历史边界，应以 [RFC-20260616-kthread-core](../kthread-core/index.md) 或新的 follow-up RFC 为准，而不是继续扩展本文：

- ordinary kthread 是否必须经由 `kthreadd` 创建；
- `Task` 与 `KThread` 的强/弱引用所有权；
- `KThreadStart<A>` 的 exactly-once 回收责任；
- stop / park / exited 生命周期状态的单一真相源；
- `kernel_exit()` 对 ordinary kthread finish path 的断言；
- `KThreadService` pending state 与 wake event 的职责分离。

## 备选方案

### 直接暴露 `Task::new_kernel()`

拒绝。直接让后台服务调用 `Task::new_kernel()` 会把 topology publish、typed argument lifetime、stop/park state 和 exit assertion 分散到每个服务里，后续很难审计 kthread 是否按同一生命周期退出。

### 让 `kthreadd` 拥有 ordinary kthread 状态

拒绝。`kthreadd` 是创建代理和拓扑父节点，不应该反向驱动每个 ordinary kthread 的 stop / park / exited 状态。生命周期应随 task-owned `KThread` 存活。

### 用 scheduler state 表达 park/stop

拒绝。scheduler state 只表达 runnable/waiting/zombie 等调度事实；kthread stop/park 是 higher-level lifecycle request。把它们塞入 `TaskSchedState` 会制造状态所有权混叠。

## 风险

- typed start pointer 经 erased queue 传递，错误的 generic reclaim 会导致类型不匹配释放。控制方式是 `KThreadStartPointer` 没有 untyped `Drop`，只能由创建失败路径的原始 `A` 或匹配 shim 回收。
- stop / park 是协作协议，entry 如果长时间不检查 `KThreadContext`，`stop()` 或 `park()` 会等待。控制方式是服务 worker loop 在每轮 wait 和每个 work item 之间检查 lifecycle。
- service 被 drop 但没有 stop 时，当前只打 warning。长期后台服务应由拥有者显式 stop 或在全局生命周期中保持常驻。

## 收口

代码已在 commit `2b0e3900279895c3d8eb604e463249a02c3bddc9` 落地，事务事实记录在 [KThread 事务日志](../../devlog/transactions/2026-06-14-kthread.md)。本文已被 [RFC-20260616-kthread-core](../kthread-core/index.md) supersede；验证状态以后续纠偏事务或开发日志记录为准。
