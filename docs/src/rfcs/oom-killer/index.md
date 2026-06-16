# RFC-20260615-oom-killer

**状态：** Active implementation
**负责人：** EDGW, Codex
**最后更新：** 2026-06-15
**领域：** mm / frame allocator / task / signal / user-test
**事务日志：** [OOM Killer 事务日志](../../devlog/transactions/2026-06-15-oom-killer.md)
**开放问题：** None；第一轮实现事务已启动，运行验证状态以事务日志为准。
**下一步：** 完成 [迁移实施计划](./implementation.md) 的 validation gate，补 user-app-test 或等价 runtime 验证。

## 摘要

本 RFC 定义第一版 OOM killer：frame allocator 在 `alloc_frame()` / `alloc_frames()` 成功分配后检查物理页使用率；如果使用率超过 kconfig 中的 `oom_kill_threshold`，唤醒后台 `oom-killer-0` kthread。worker 被唤醒后必须重新读取 `FrameAllocatorStats` 并再次判断阈值；只有仍超过阈值时，才选择当前独占物理页最多的进程，向其 thread group 投递 kernel-origin `SIGKILL`。worker 重复这个流程，直到物理内存使用率不再超过阈值。

第一版目标是避免用户态无限分配把系统拖到不可恢复状态。它不是完整 Linux OOM policy，不实现 badness score、memcg、OOM reaper、page reclaim、swap、cgroup 约束或 per-task `oom_score_adj`。

## 背景

当前 frame allocator 只提供全局页分配和统计。用户态匿名页 fault 会经 `alloc_frame_zeroed()` 进入 `alloc_frame()`，因此可以在单页分配成功后用当前物理页占用触发 OOM killer。内核已有 kthread 基础设施和 signal/exit 路径，适合让 OOM killer 作为普通 kthread worker 运行，并通过 `SIGKILL` 让目标进程走既有退出清理路径释放地址空间。

现有 `/proc/<pid>/status` 和 `/proc/<pid>/stat` 已明确 resident accounting 尚未接线；本 RFC 不新增一套长期 RSS 账本。victim selection 由 `UserSpace` 提供临时独占物理页 snapshot：扫描当前用户地址空间拥有、且释放该 task / thread group 时最可能立即归还给 frame allocator 的物理页。共享 VMO、共享零页和文件页缓存不计入；COW overlay、私有匿名页和私有加载页中当前只由该地址空间拥有的物理页计入。对 `ShadowObject`，如果 parent/backing VMO 只被当前 shadow 链持有，则递归计入 parent 链，因为释放该 task 会同时释放这些祖先页。该 snapshot 允许 stale，只服务 OOM victim 排序，不作为 `/proc` RSS 真相源。

## 目标

- 新增 `oom_kill_threshold` kconfig 参数，默认 90，表达触发 OOM killer 的物理内存使用百分比。
- 在 `FrameAllocatorStats` 上提供阈值判断函数，用于判断当前使用率是否超过给定 killer threshold。
- 在 `alloc_frame()` / `alloc_frames()` 成功后检查一次阈值；超过阈值时只唤醒 OOM killer，不在 allocation fast path 扫描 task 或发送 signal。
- 新增 `oom-killer-0` kthread worker；worker 被唤醒后先重查阈值，未超过则回到等待。
- worker 超过阈值时选择独占物理页最多的 eligible thread group，并投递 `SIGKILL`。
- victim selection 不得长时间持有 task topology 全局锁；全局锁下只做短 snapshot。
- 避免重复杀同一个尚未完成退出的 victim，避免 stale task / stale thread group 导致 panic。
- 新增用户态回归：clone 出子进程，子进程按 50 MiB chunk 循环分配并触碰内存；父进程等待并确认子进程因 `SIGKILL` 被 OOM killer 杀掉。

## 非目标

- 不在本阶段实现 generic reclaim、swap、page cache reclaim、slab shrink、memcg 或 cgroup OOM。
- 不实现 Linux `oom_score` / `oom_score_adj` / badness 完整策略。
- 不在 frame allocator 锁内执行 wake 以外的复杂逻辑。
- 不在 OOM killer 中直接调用其他 task 的 `kernel_exit_group()`。
- 不为 `/proc` RSS 提供新的稳定 ABI；独占物理页 snapshot 只服务 victim selection。
- 不处理没有 eligible user process 时的完整恢复策略；第一版只记录诊断并 cooperative yield。

## 文档地图

Canonical：

- [不变量需求](./invariants.md)
- [迁移实施计划](./implementation.md)

依赖：

- [RFC-20260614-kthread](../kthread/index.md)
- [RFC-20260614-inode-shrinker](../inode-shrinker/index.md)：共享 frame usage threshold 的表达习惯，但 OOM killer policy 独立。

## 方案

`oom_kill_threshold` 是 kconfig 百分比，默认 90。`FrameAllocatorStats` 暴露语义明确的 `exceeds_oom_kill_threshold()`；内部和 `exceeds_io_shrink_threshold()` 共用私有百分比判断 helper，并使用严格大于语义：

```rust
used_pages * 100 > total_pages * threshold
```

`total_pages == 0` 时返回 false。threshold 必须在 0..=100 内，kconfig 常量通过 `const_assert!` 或等价检查约束。

`alloc_frame()` / `alloc_frames()` 成功分配物理页后读取 `frame_allocator_stats()`。如果 stats 超过 `oom_kill_threshold`，调用 `wake_oom_killer()`。该路径不持有 frame allocator 内部锁，不扫描 task，不分配 heap，不发送 signal。

OOM killer worker 使用 `Event` 或等价 wake capability 等待。worker 每次被唤醒后先重新读取 `FrameAllocatorStats`；如果阈值已经不满足，直接回到等待。若仍超过阈值，worker 执行 victim selection：先在 topology 锁下快速复制 thread group `Arc` 列表，释放 topology 锁后逐个读取 leader / user-space handle / 独占物理页 snapshot，选出独占物理页最多的用户地址空间。选中后向该 thread group 投递 `SIGKILL`，记录 active victim，并让出调度器，避免在同一个 victim 尚未进入退出清理前继续扩大杀伤面。

worker 循环再次检查阈值。如果 victim 退出释放内存后阈值不再满足，回到等待；如果仍超过阈值，则继续选择下一个 eligible victim。已经 exiting/exited、kernel task、idle、init、kthreadd 和 OOM killer 自身都不是 eligible victim。

## 接受边界

本 RFC 被接受意味着第一版 OOM killer 的合同是：

- allocator 侧只负责 threshold check 和 wake，不负责 victim policy；
- OOM worker 必须醒后重查 threshold；
- victim selection 使用用户地址空间独占物理页 snapshot；
- topology 全局锁不得覆盖 VMO / user-space 独占物理页扫描；
- kill 动作通过 signal/exit 既有路径完成；
- worker 重复执行直到 threshold 不满足，或没有 eligible victim 时记录并 yield。

后续如果要加入 badness score、per-process 权重、memcg、OOM reaper、direct reclaim、page cache reclaim、slab shrink、wait-for-victim-exit 事件或 `/proc` RSS ABI，需要更新本 RFC 或新增 follow-up RFC。

## 备选方案

### 在 alloc path 同步杀进程

拒绝。`alloc_frame()` 是高频路径，且可能在 page fault、kernel page table allocation 和 VMO resolve 中被调用。同步扫描 task 或发送 signal 会把 allocator fast path 变成跨 task / signal / mm 的复杂路径，也更容易在 allocator 锁或中断状态上制造死锁。

### 用 VMA virtual size 选择 victim

拒绝作为第一选择。`vsize_bytes()` 已存在且便宜，但它统计 reservation，不等于杀掉该进程能立即释放的物理页。OOM killer 的目标是解除当前物理内存压力，因此第一版应按 `UserSpace` 的独占物理页 snapshot 排序。`vsize_bytes()` 只能作为 snapshot 不可用时的诊断 fallback，不能作为 canonical policy。

### 直接修改 victim 的 thread-group lifecycle

拒绝。只有目标 task 自己的 exit path 才能可靠完成 futex、fd、address-space、thread-group 和 parent notification cleanup。OOM killer 应只投递 `SIGKILL`，由 signal/exit 现有协议完成终止。

## 风险

- victim snapshot 可能 stale。控制方式是把 snapshot 限定为排序依据；kill 前后都允许目标已经 exiting/exited，stale 只导致跳过或重复重查。
- SIGKILL 后内存不会立即释放。控制方式是 active victim + yield，避免在同一轮连续杀多个进程。
- 没有 eligible victim 时阈值仍可能超过。控制方式是记录诊断并让出调度器；完整 direct reclaim 或 panic policy 不在第一版范围内。
- VMO 独占物理页扫描成本可能较高。控制方式是只在 OOM worker 中执行，不在 allocator path 执行，并且 topology 锁只覆盖短 snapshot。

## 收口

本 RFC 已进入实现事务。后续收口以 [OOM Killer 事务日志](../../devlog/transactions/2026-06-15-oom-killer.md) 记录 `git diff --check`、`just fmt kernel --check`、`just build` 以及 user-app-test / user-test runtime 结果。
