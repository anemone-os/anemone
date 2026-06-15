# Inode Shrinker 不变量需求

**状态：** Canonical
**最后更新：** 2026-06-15
**父 RFC：** [RFC-20260614-inode-shrinker](./index.md)

本文定义 inode shrinker 与 VFS resident inode cache 的协议边界。具体落地顺序与 commit 事实见 [Inode Shrinker 迁移实施计划](./implementation.md) 和 [事务日志](../../devlog/transactions/2026-06-14-inode-shrinker.md)。

## 闭合条件

inode shrinker 被视为闭合时，必须同时满足：

1. shrink request 是可合并 hint，不是需要逐一处理的计数型 work。
2. shrinker 不扫描 `KERNEL_FS` 或 `PERSISTENT_SB` superblock。
3. ghost inode 可以作为候选；indexed inode 只有在 filesystem 声明 `SHRINKABLE_ICACHE` 时才作为候选。
4. `cached_inode_snapshot()` 只提供候选快照，不是 eviction 决策真相。
5. eviction 前必须检查 inode 是否 busy。
6. sync 后、cache removal 前必须在 superblock cache write lock 下重新检查 busy。
7. busy 判定至少包含 inode active refcount 和 mapping strong refcount。
8. inode 从 resident cache 移除前必须完成 filesystem `sync_inode`。
9. `evict_inode` 不在 superblock cache lock 下运行。
10. `evict_inode` 失败时必须按原 indexed/ghost 身份回滚。
11. backing file cache page counter 只统计 backing filesystem regular file cache pages，不驱动 eviction 决策。
12. task-exit hint 被 worker 消费后，只有物理页占用严格超过 `io_shrink_threshold` 时才执行 inode scan。

任一条件不成立时，当前实现只能视为临时 cache cleanup helper，不能声明为可审查的 inode shrinker。

## 非目标

本需求不包含：

1. 完整 generic shrinker API。
2. allocator OOM direct reclaim。
3. LRU、age、priority、memcg、quota、timer-driven reclaim 或 allocator direct reclaim。
4. slab、anonymous page、ramfs、devfs、SysV shm 或 kernel-internal filesystem 回收。
5. 对每次 task exit 都执行完整 shrink scan 的保证。

## 状态所有权

### SuperBlock resident cache

`SuperBlockInner` 是 inode cache 的真相源：

- `indexed` 保存仍能通过 inode number 命中的 resident inode；
- `ghosts` 保存已 unindexed 但仍 resident 的 inode；
- `Inode::indexed()` 是 inode 对当前 cache 身份的投影，必须跟 `indexed` / `ghosts` 更新保持一致。

硬性要求：

1. `iget()` 是按 inode number 加载 indexed inode 的 canonical 入口。
2. `seed_inode()` 只能插入没有 live entry 的 inode。
3. `unindex_inode*()` 从 `indexed` 移到 `ghosts` 时必须清 `indexed` bit。
4. `try_evict_inode()` 是 ordinary shrinker 和 unmount eviction 的受控入口。
5. filesystem `evict_inode` 只能由显式 eviction path 调用，不能由 `Drop` 隐式调用。

### Filesystem capability

`FileSystemFlags` 决定 shrinker 是否能扫描某类 superblock 或 inode：

1. `KERNEL_FS` 表示 kernel-internal filesystem，不被 shrinker 扫描。
2. `PERSISTENT_SB` 表示 superblock lifetime 由 filesystem 拥有，不被 shrinker 扫描。
3. `SHRINKABLE_ICACHE` 表示 indexed resident inode 可被 evict 后用 `load_inode` 重建。

这些 flag 是 filesystem 对 VFS 的能力声明，不是 shrinker 动态判断结果。

### Pending request

`InodeShrinkRequest` 没有 payload。pending slot 只说明“至少有一次 shrink hint 尚未处理”。

要求：

1. 重复 task exit 提交可以 merge，不累计扫描次数。
2. service stopping 时提交失败不向 task exit 暴露错误。
3. shrinker 尚未初始化时 task exit 提交可以静默忽略。
4. 低于或等于 `io_shrink_threshold` 时，worker 可以消费该 hint 并跳过扫描。

### Memory pressure gate

`io_shrink_threshold` 是 kconfig 百分比，默认 50。它只控制 task-exit hint 到达后 worker 是否执行一次 inode scan。

要求：

1. 阈值判断属于 inode shrinker worker，不属于 task exit path 或 frame allocator。
2. frame allocator 只提供 `FrameAllocatorStats`；shrink policy 不能下沉到 frame 层。
3. 判断必须是严格大于：`used_pages * 100 > total_pages * threshold`。
4. `total_pages == 0` 时不执行 scan。
5. 该 gate 只影响是否进入 scan，不参与 candidate 选择、busy 判定、eviction 成败或回滚语义。

## 身份与能力模型

### Inode 身份

eviction 必须基于 `Arc<Inode>` 指针身份和 inode number 双重判断：

1. indexed cache 命中同一 `ino` 后，还必须用 `Arc::ptr_eq` 确认是同一个 inode object。
2. ghost removal 必须按 `Arc::ptr_eq` 从 ghosts 列表中定位。
3. snapshot 中的 `Arc<Inode>` 不能保证仍在 cache 中，`try_evict_inode()` 必须允许 `NotFound`。
4. inode number 只用于 indexed lookup 和日志，不足以证明 snapshot 候选仍是当前 resident object。

### Busy 判定

首版 busy 判定至少包含：

1. `inode.rc() > 0`，表示 inode 仍有 active `InodeRef` 或等价外部使用。
2. `inode.mapping().is_some_and(|mapping| Arc::strong_count(mapping) > 1)`，表示 file-backed mapping 仍被 VMA、file object 或其他 consumer 持有。

后续如果引入 dentry alias、writeback pin、page lock 或 mmap invalidation pin，必须扩展 busy 判定或新增 follow-up RFC，不能让 shrinker 在这些 pin 存在时 evict。

### Cache page counter

`resident_file_inode_cache_pages()` 是 backing file cache 的观察数据。

要求：

1. 只有 backing filesystem regular file cache page insert 调 `backing_file_cache_page_inserted()`。
2. truncate invalidation 和 `Ext4RegState::drop()` 必须调 `backing_file_cache_pages_removed(n)`。
3. counter underflow 使用 release 生效的 `assert!` 暴露 bug。
4. counter 不参与 shrink candidate 选择、busy 判定或 eviction 成败判断。

## 线性化点

### Candidate snapshot

`cached_inode_snapshot(include_indexed)` 在线性化点内只复制 `Arc<Inode>`：

1. 持 superblock read lock 收集 ghosts。
2. 如果 `include_indexed` 为 true，再收集 indexed values。
3. 释放锁后，snapshot 只作为候选列表。

snapshot 之后 inode 可能被重新打开、unindexed、evicted 或从 cache 消失。因此所有正确性必须由 `try_evict_inode()` 的 recheck 负责。

### Eviction

`try_evict_inode()` 的线性化顺序必须是：

1. 锁外检查 busy，busy 则返回 `SysError::Busy`。
2. 构造临时 `InodeRef` 调 filesystem `sync_inode`。
3. 持 superblock cache write lock。
4. 再次检查 busy，busy 则返回 `SysError::Busy`。
5. 从 `indexed` 或 `ghosts` 中按指针身份移除。
6. 释放 cache lock。
7. 调 filesystem `evict_inode`。
8. 如果 `evict_inode` 失败，重新持 cache write lock 按原身份插回。

这样保证后续 opener 如果 miss cache 并通过 `load_inode` 重建，不会读到 eviction 前未同步的 dirty metadata 或 dirty regular file page state。

### Ext4 sync

ext4 的 `sync_inode` / `evict_inode` 共享 `ext4_sync_inode_inner()`：

1. `nlink == 0` 是 terminal deletion 判断；不能用 `indexed` 作为删除判断，因为 eviction 会先清 indexed bit。
2. regular inode 必须先 `sync_all()` dirty file pages。
3. metadata 更新和 lwext4 flush 在 ext4 write transaction 内完成。

## 锁序与生命周期规则

1. shrinker 遍历 mounted superblocks 时不能持 VFS namespace lock 执行 inode eviction；`mounted_superblocks()` 返回 `Arc<SuperBlock>` 快照。
2. superblock cache lock 不得覆盖 filesystem `sync_inode` 或 `evict_inode`。
3. ext4 backend I/O 由 ext4 自身 `tx_lock` 与 `fs_lock` 管理，VFS shrinker 不绕过它们。
4. task exit 只提交 shrink hint，不在 exit path 直接读取 frame stats 或执行 inode scan。
5. kthread worker 在每个 superblock 和 inode 边界检查 stop / park。

## 禁止退化项

以下模式会破坏证明：

1. 在 `Drop` 中调用 filesystem `evict_inode` 或执行 blocking sync。
2. 对未声明 `SHRINKABLE_ICACHE` 的 filesystem 回收 indexed inode。
3. 回收 `KERNEL_FS` 或 `PERSISTENT_SB` superblock。
4. 把 snapshot 候选当成仍在 cache 中的事实，不做 recheck。
5. sync 之后不在 cache write lock 下再次检查 busy。
6. `evict_inode` 失败后不回滚 resident cache。
7. 用 backing file cache counter 驱动 eviction 正确性判断。
8. 在 task exit path 同步扫描所有 superblock。
9. 把 `io_shrink_threshold` 判断放入 frame allocator 或 task exit path。

## 完成标准

本 RFC 当前满足文档层闭合：superblock cache 身份、filesystem flag、candidate snapshot、busy recheck、sync-before-remove、failure rollback、task-exit hint、memory pressure gate 和 page counter 边界均已写入 canonical 文本。

后续只有在以下验证完成后，才能把运行验证也标记为闭合：

1. `just build` 或等价构建通过。
2. QEMU/user-test 中存在 task exit 后 shrinker worker 执行记录，且无 kthread lifecycle panic。
3. 针对 ext4 indexed inode reload、deleted ghost inode 回收、dirty regular page sync 和 eviction failure rollback 的路径有定向验证或审计记录。
