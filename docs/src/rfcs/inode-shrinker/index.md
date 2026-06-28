# RFC-20260614-inode-shrinker

**状态：** 已接受，代码已落地，文档追补完成
**负责人：** EDGW, Codex
**最后更新：** 2026-06-16
**领域：** fs / VFS / inode cache / ext4 / kthread
**事务日志：** [2026-06-14 - Inode Shrinker](../../devlog/transactions/2026-06-14-inode-shrinker.md)
**开放问题：** None；本轮文档追补未重新运行构建、QEMU 或 LTP。
**下一步：** 如果后续要从自循环单阈值 gate 扩展到 sleep/wakeup 水位、allocator direct reclaim、LRU、限额或通用 shrinker registry，应新增 follow-up RFC。

## 摘要

本 RFC 记录 `2b0e3900279895c3d8eb604e463249a02c3bddc9` 中落地并在 2026-06-15 调整触发策略的 inode shrinker。它是当前 VFS 的第一版后台 inode cache 回收系统：`inode-shrink-0` kthread 自循环检查物理页占用；占用低于或等于 `io_shrink_threshold` 时让出调度器，占用严格超过阈值时遍历可见 namespace 中的 mounted superblock，按 superblock cache snapshot 尝试回收不 busy 的 resident inode。

本文中的 shrinker 不是完整 generic shrinker 框架。它只覆盖 superblock resident inode cache、ext4 file-backed page cache 计数、显式 eviction path，以及自循环 worker 上的单一物理内存占用阈值 gate；不管理 slab、不做通用 page cache shrink，也不提供 sleep/wakeup 水位或 allocator direct reclaim。

## 背景

VFS superblock 已经维护 resident inode cache，其中 `indexed` 保存仍可通过 inode number 发现的 inode，`ghosts` 保存已从 index 移除但仍因外部引用存活的 inode。过去 unmount 可以通过 `try_evict_all()` 做受控清理，但普通运行期间缺少后台回收入口，deleted/unhashed inode 和 backing file page cache 容易长期停留到更晚的生命周期点。

本次提交同时引入 [kthread](../kthread/index.md) 基础设施，因此 inode shrinker 可以作为第一个后台 consumer 落地。当前 worker 由 `KThreadBuilder` 创建为长期普通 kthread，循环检查 stop 与物理页占用，并在低于阈值时通过 `yield_now()` 让出调度器。`kthread-core` 阶段 1 已移除 park/unpark，因此 shrinker 不再检查 park 状态。

## 目标

- 建立 inode cache 的显式 snapshot -> recheck -> sync -> remove -> evict 流程，禁止在 `Drop` 中做 blocking I/O。
- 让 shrinker 跳过 `KERNEL_FS` 和 `PERSISTENT_SB` superblock。
- 对 ghost inode 默认允许尝试回收；对 indexed inode 只有 filesystem 显式声明 `SHRINKABLE_ICACHE` 后才允许回收。
- 在 eviction 前后重新检查 inode 是否 busy，避免 snapshot 后被重新打开或被 mapping 引用。
- 在 cache removal 前同步 inode metadata 和 dirty regular file pages，保证后续 reload 不看到 stale state。
- eviction 失败时把 inode 按原 indexed/ghost 身份插回 resident cache。
- 为 backing filesystem page cache 提供只覆盖 backing file 的 resident page counter，作为观察数据而非 shrinker 决策真相。
- 让 `io_shrink_threshold` 进入 kconfig，默认 50；worker 自循环检查物理页占用，低于或等于阈值时让出调度器，严格超过阈值时才执行扫描。

## 非目标

- 不实现 generic shrinker registry、`count_objects` / `scan_objects` API 或 allocator OOM direct reclaim。
- 不做 LRU、age-based、quota-based 或 priority-based inode 回收。
- 不从 allocator OOM 路径直接 reclaim。
- 不回收 `KERNEL_FS` 或 `PERSISTENT_SB` 的 resident inode。
- 不把 ramfs、devfs、anonymous page、SysV shm 或 slab 对象纳入本计数。
- 不提供低压 sleep/wakeup 机制；当前低压路径只 cooperative yield。
- 不在本 RFC 中定义 kthread lifecycle；该部分见 [RFC-20260614-kthread](../kthread/index.md)。

## 文档地图

Canonical：

- [不变量需求](./invariants.md)
- [迁移实施计划](./implementation.md)

依赖：

- [RFC-20260614-kthread](../kthread/index.md)

## 方案

`INODE_SHRINKER` 是一个全局 `KThreadRef` slot，用于记录由 `KThreadBuilder` 创建的 `inode-shrink-0` worker，防止重复初始化。

worker entry 循环执行三个步骤：先检查 `KThreadContext` 的 stop 请求；再读取 `frame_allocator_stats()`；只有 `used_pages * 100 > total_pages * io_shrink_threshold` 时才执行 `shrink_inodes()`。低于或等于阈值时调用 `yield_now()` 让出调度器后进入下一轮。默认阈值是 50。真正扫描时，worker 遍历 `mounted_superblocks()` 返回的 visible namespace superblock。每个 superblock 先按 flags 判断是否允许 shrink，再调用 `cached_inode_snapshot(include_indexed)` 获取候选列表。

真正 eviction 由 `SuperBlock::try_evict_inode()` 负责。它先检查 inode active refs 和 mapping strong refs，确认不 busy 后调用 filesystem `sync_inode`，随后在 superblock cache write lock 下再次检查 busy 并从 `indexed` 或 `ghosts` 移除。之后调用 filesystem `evict_inode`。如果 eviction 返回错误，VFS 按原位置把 inode 插回 cache。

ext4 声明 `SHRINKABLE_ICACHE`，因此 indexed ext4 inode 可以被回收并通过 `load_inode` 重建。ext4 regular file 的 in-memory page cache 通过 `Ext4RegState` 管理，page insert、truncate invalidation 和 state drop 会维护 backing file cache page counter；该 counter 不参与 eviction 选择。

## 接受边界

本 RFC 已作为 inode shrinker 的 canonical 文档被接受，并追补记录 commit `2b0e3900279895c3d8eb604e463249a02c3bddc9` 的实现形状。

后续修改如果改变以下任一边界，必须先更新本 RFC 或新增 follow-up RFC：

- shrinker 是否仍由普通 kthread 自循环检查压力，而不是 task exit 或 allocator OOM 直接触发；
- `io_shrink_threshold` 是否仍是 worker 内部的严格大于百分比 gate，低压路径是否只 cooperative yield；
- superblock flags 对 shrink 范围的约束；
- indexed 与 ghost inode 的身份和回滚语义；
- busy 判定是否包含 inode refcount 和 mapping strong refs；
- sync-before-remove 与 evict-failure rollback 顺序；
- backing file cache counter 是否只作为观察数据。

## 备选方案

### 在 inode `Drop` 中回收

拒绝。filesystem eviction 可能需要 metadata sync、dirty page writeback 或 backend I/O。把它放入 `Drop` 会让任意引用释放路径承担 blocking I/O 和复杂锁序，难以审计失败回滚。

### 只在 unmount 时回收

拒绝。unmount 清理不能覆盖长时间运行中的 deleted/unhashed inode 和 file-backed page cache 驻留。后台 shrinker 可以在普通运行期间做 opportunistic 清理。

### 所有 filesystem 默认回收 indexed inode

拒绝。indexed inode 被移除后必须能通过 `load_inode` 正确重建。只有显式声明 `SHRINKABLE_ICACHE` 的 filesystem 才能让 shrinker 扫 indexed cache；ghost inode 已经不依赖 inode number index，可在非 kernel/persistent superblock 上尝试回收。

## 风险

- snapshot 后 inode 可能被重新打开。控制方式是在 sync 前和 cache write lock 下各检查一次 busy。
- sync 成功后、cache removal 前仍可能出现状态变化。控制方式是第二次 busy check 和 cache write lock 下的 ptr-eq removal。
- `evict_inode` 失败会造成部分回收。控制方式是按原 indexed/ghost 身份回滚，并返回错误供 shrinker 记录。
- 高压但无可回收 inode 时，worker 会按当前 loop 继续尝试扫描；后续如果需要 sleep/wakeup 水位、timer backoff、allocator OOM direct reclaim 或 LRU/priority reclaim，应作为 follow-up 设计。

## 收口

代码已在 commit `2b0e3900279895c3d8eb604e463249a02c3bddc9` 落地，2026-06-15 的自循环阈值 gate follow-up 记录在 [Inode Shrinker 事务日志](../../devlog/transactions/2026-06-14-inode-shrinker.md)。验证状态以后续事务或开发日志记录为准。
