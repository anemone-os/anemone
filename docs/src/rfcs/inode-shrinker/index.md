# RFC-20260614-inode-shrinker

**状态：** 已接受，代码已落地，文档追补完成
**负责人：** EDGW, Codex
**最后更新：** 2026-06-15
**领域：** fs / VFS / inode cache / ext4 / kthread
**事务日志：** [2026-06-14 - Inode Shrinker](../../devlog/transactions/2026-06-14-inode-shrinker.md)
**开放问题：** None；本轮文档追补未重新运行构建、QEMU 或 LTP。
**下一步：** 如果后续要从 task-exit hint 扩展到内存压力、LRU、限额或通用 shrinker registry，应新增 follow-up RFC。

## 摘要

本 RFC 记录 `2b0e3900279895c3d8eb604e463249a02c3bddc9` 中落地的 inode shrinker。它是当前 VFS 的第一版后台 inode cache 回收系统：task exit 提交合并请求，`inode-shrink-0` kthread 遍历可见 namespace 中的 mounted superblock，按 superblock cache snapshot 尝试回收不 busy 的 resident inode。

本文中的 shrinker 不是完整 generic shrinker 框架。它只覆盖 superblock resident inode cache、ext4 file-backed page cache 计数和显式 eviction path；不做内存压力评分、不管理 slab、不做通用 page cache shrink，也不把 task exit 次数当作必须逐一执行的 work item。

## 背景

VFS superblock 已经维护 resident inode cache，其中 `indexed` 保存仍可通过 inode number 发现的 inode，`ghosts` 保存已从 index 移除但仍因外部引用存活的 inode。过去 unmount 可以通过 `try_evict_all()` 做受控清理，但普通运行期间缺少后台回收入口，deleted/unhashed inode 和 backing file page cache 容易长期停留到更晚的生命周期点。

本次提交同时引入 [kthread](../kthread/index.md) 服务基础设施，因此 inode shrinker 可以作为第一个后台 consumer 落地：它把 task exit 视为回收 hint，把重复请求合并成一个 pending slot，并在 worker 中执行可中断的 superblock/inode 扫描。

## 目标

- 建立 inode cache 的显式 snapshot -> recheck -> sync -> remove -> evict 流程，禁止在 `Drop` 中做 blocking I/O。
- 让 shrinker 跳过 `KERNEL_FS` 和 `PERSISTENT_SB` superblock。
- 对 ghost inode 默认允许尝试回收；对 indexed inode 只有 filesystem 显式声明 `SHRINKABLE_ICACHE` 后才允许回收。
- 在 eviction 前后重新检查 inode 是否 busy，避免 snapshot 后被重新打开或被 mapping 引用。
- 在 cache removal 前同步 inode metadata 和 dirty regular file pages，保证后续 reload 不看到 stale state。
- eviction 失败时把 inode 按原 indexed/ghost 身份插回 resident cache。
- 为 backing filesystem page cache 提供只覆盖 backing file 的 resident page counter，作为观察数据而非 shrinker 决策真相。

## 非目标

- 不实现 generic shrinker registry、`count_objects` / `scan_objects` API 或内存压力回调。
- 不做 LRU、age-based、quota-based 或 priority-based inode 回收。
- 不从 allocator OOM 路径直接 reclaim。
- 不回收 `KERNEL_FS` 或 `PERSISTENT_SB` 的 resident inode。
- 不把 ramfs、devfs、anonymous page、SysV shm 或 slab 对象纳入本计数。
- 不承诺 task exit 提交的每个请求都对应一次完整扫描；请求是可合并 hint。
- 不在本 RFC 中定义 kthread lifecycle；该部分见 [RFC-20260614-kthread](../kthread/index.md)。

## 文档地图

Canonical：

- [不变量需求](./invariants.md)
- [迁移实施计划](./implementation.md)

依赖：

- [RFC-20260614-kthread](../kthread/index.md)

## 方案

`INODE_SHRINKER` 是一个全局 `KThreadService<KThreadPendingSlot<InodeShrinkRequest>, InodeShrinker>`。`InodeShrinkRequest` 没有 payload，重复提交会合并到同一个 slot；这表示“有 shrink work pending”，不是“必须执行 N 次 shrink”。

task exit 路径调用 `submit_inode_shrink_request()`。如果 shrinker 尚未初始化，提交静默忽略；如果 service 正在 stopping，也静默接受停止状态。worker 执行 `shrink_inodes()`，遍历 `mounted_superblocks()` 返回的 visible namespace superblock。每个 superblock 先按 flags 判断是否允许 shrink，再调用 `cached_inode_snapshot(include_indexed)` 获取候选列表。

真正 eviction 由 `SuperBlock::try_evict_inode()` 负责。它先检查 inode active refs 和 mapping strong refs，确认不 busy 后调用 filesystem `sync_inode`，随后在 superblock cache write lock 下再次检查 busy 并从 `indexed` 或 `ghosts` 移除。之后调用 filesystem `evict_inode`。如果 eviction 返回错误，VFS 按原位置把 inode 插回 cache。

ext4 声明 `SHRINKABLE_ICACHE`，因此 indexed ext4 inode 可以被回收并通过 `load_inode` 重建。ext4 regular file 的 in-memory page cache 通过 `Ext4RegState` 管理，page insert、truncate invalidation 和 state drop 会维护 backing file cache page counter；该 counter 不参与 eviction 选择。

## 接受边界

本 RFC 已作为 inode shrinker 的 canonical 文档被接受，并追补记录 commit `2b0e3900279895c3d8eb604e463249a02c3bddc9` 的实现形状。

后续修改如果改变以下任一边界，必须先更新本 RFC 或新增 follow-up RFC：

- task exit 是否只是 shrink hint，而不是计数型 work；
- superblock flags 对 shrink 范围的约束；
- indexed 与 ghost inode 的身份和回滚语义；
- busy 判定是否包含 inode refcount 和 mapping strong refs；
- sync-before-remove 与 evict-failure rollback 顺序；
- backing file cache counter 是否只作为观察数据。

## 备选方案

### 在 inode `Drop` 中回收

拒绝。filesystem eviction 可能需要 metadata sync、dirty page writeback 或 backend I/O。把它放入 `Drop` 会让任意引用释放路径承担 blocking I/O 和复杂锁序，难以审计失败回滚。

### 只在 unmount 时回收

拒绝。unmount 清理不能覆盖长时间运行中的 deleted/unhashed inode 和 file-backed page cache 驻留。后台 shrinker 可以在普通 task exit 后做 opportunistic 清理。

### 所有 filesystem 默认回收 indexed inode

拒绝。indexed inode 被移除后必须能通过 `load_inode` 正确重建。只有显式声明 `SHRINKABLE_ICACHE` 的 filesystem 才能让 shrinker 扫 indexed cache；ghost inode 已经不依赖 inode number index，可在非 kernel/persistent superblock 上尝试回收。

## 风险

- snapshot 后 inode 可能被重新打开。控制方式是在 sync 前和 cache write lock 下各检查一次 busy。
- sync 成功后、cache removal 前仍可能出现状态变化。控制方式是第二次 busy check 和 cache write lock 下的 ptr-eq removal。
- `evict_inode` 失败会造成部分回收。控制方式是按原 indexed/ghost 身份回滚，并返回错误供 shrinker 记录。
- task exit hint 不是内存压力信号，可能不足以覆盖所有 cache 增长场景。后续如果需要 pressure-driven reclaim，应作为 follow-up 设计。

## 收口

代码已在 commit `2b0e3900279895c3d8eb604e463249a02c3bddc9` 落地，事务事实记录在 [Inode Shrinker 事务日志](../../devlog/transactions/2026-06-14-inode-shrinker.md)。本轮只补文档，未重新运行构建、QEMU 或 LTP；验证状态以后续事务或开发日志记录为准。
