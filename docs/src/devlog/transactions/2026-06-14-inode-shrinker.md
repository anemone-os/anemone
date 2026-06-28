# 2026-06-14 - Inode Shrinker

**Status:** Complete for retrospective documentation
**Owners:** EDGW, Codex
**Area:** fs / VFS / inode cache / ext4 / kthread
**RFC:** [RFC-20260614-inode-shrinker](../../rfcs/inode-shrinker/index.md)
**Current Phase:** post-commit documentation

## Scope

本事务记录 commit `2b0e3900279895c3d8eb604e463249a02c3bddc9` 中落地的 inode shrinker：

- superblock resident inode cache 的 indexed / ghosts 身份；
- explicit `try_evict_inode()` 路径；
- filesystem `evict_inode` callback；
- `SHRINKABLE_ICACHE` filesystem capability；
- backing file cache page counter；
- `inode-shrink-0` 自循环 kthread worker；
- `io_shrink_threshold` 控制的物理页占用 gate。

非目标：

- 不实现 generic shrinker registry。
- 不实现内存压力、LRU、quota 或 allocator direct reclaim。
- 不回收 kernel/persistent filesystem。
- 不把轮询次数作为必须逐个处理的 work count。

## Invariants

- shrinker worker 自循环检查物理页占用，不依赖 task exit request。
- `KERNEL_FS` 和 `PERSISTENT_SB` superblock 不被扫描。
- indexed inode 只有在 filesystem 声明 `SHRINKABLE_ICACHE` 时才作为候选。
- snapshot 只提供候选，eviction 前后必须 recheck busy。
- sync 在 cache removal 前完成，`evict_inode` 在 cache lock 外执行。
- eviction failure 必须按原 indexed/ghost 身份回滚。
- backing file cache page counter 只作为观察数据，不参与 shrinker 决策。

## Handoff

**Last Updated:** 2026-06-15

**Current Branch:** `dev/kako/kthread`

**Implementation Commit:** `2b0e3900279895c3d8eb604e463249a02c3bddc9` (`basic shrinker and kthread`)

**Canonical RFC:** [RFC-20260614-inode-shrinker](../../rfcs/inode-shrinker/index.md), [Invariants](../../rfcs/inode-shrinker/invariants.md), [Implementation Plan](../../rfcs/inode-shrinker/implementation.md)

**Dependency:** [RFC-20260614-kthread](../../rfcs/kthread/index.md), [KThread transaction](./2026-06-14-kthread.md)

**Completed:** VFS 增加 explicit inode eviction path；ext4 声明 shrinkable icache 并提供 eviction callback；regular file page cache 维护 backing page counter；worker 按 `io_shrink_threshold` 判断物理页占用后决定是否扫描；boot path 创建自循环 `inode-shrink-0` worker。

**Open Blockers:** 当前文档层没有已确认的 Apollyon / Keter 设计 blocker。阈值 gate follow-up 需要以本轮 agent 输出中的构建/检查结果为准；QEMU 或 LTP 未作为强制 gate。

**Next Action:** 如果后续需要超出单一 kconfig 阈值 gate 的 sleep/wakeup 水位、pressure-driven reclaim、LRU、限额或跨 filesystem 的通用 shrinker registry，另建 follow-up RFC。当前 shrinker 只作为自循环 opportunistic cleanup。

**Do Not Redo:** 不要在 inode `Drop` 中做 blocking eviction；不要回收 kernel/persistent superblock；不要对未声明 `SHRINKABLE_ICACHE` 的 filesystem 回收 indexed inode；不要用 cache page counter 驱动 eviction 正确性；不要把 `io_shrink_threshold` 判断下沉到 frame allocator、allocator OOM path 或 task exit path。

## Phase Log

### 2026-06-14 - basic shrinker and kthread

**Phase:** implementation / commit import

**Change:** 新增 `fs::inode_shrinker`，以 `KThreadService<KThreadPendingSlot<InodeShrinkRequest>, InodeShrinker>` 实现单 worker shrink service。重复 task exit hint 合并到一个 pending slot。

**Change:** `task/api/exit` 在 ordinary exit 路径末尾提交 inode shrink hint。提交不在 exit path 执行 scan，也不把 shrinker 停止状态暴露给 exiting task。

**Superseded:** 上述 request service 与 task-exit hint 触发形状已被 2026-06-15 follow-up 替代；当前实现由 `inode-shrink-0` worker 自循环检查 `io_shrink_threshold`。

**Change:** `SuperBlock` resident cache 拆出 indexed 与 ghosts，并提供 `cached_inode_snapshot()` 与 `try_evict_inode()`。eviction 执行 busy check、sync-before-remove、cache-lock recheck、remove、filesystem evict 和 failure rollback。

**Change:** ext4 增加 `evict_inode` callback，声明 `SHRINKABLE_ICACHE`，并维护 backing regular file cache page counter。

**Review:** 文档追补时未发现需要单独 tracking 的 confirmed design issue。关键协议已折入 RFC index、invariants 和 implementation 文本。

**Validation:** 本事务追补文档时未运行 `just build`、QEMU 或 LTP。commit 级运行结果如需长期引用，应由后续开发日志或新的 validation transaction 补充。

### 2026-06-15 - self-loop pressure gate

**Phase:** follow-up implementation / RFC alignment

**Correction:** 原始事务范围中的“不实现内存压力”指不实现 generic pressure-driven reclaim、LRU、quota 或 allocator direct reclaim。2026-06-15 的 follow-up 把 task-exit hint 触发改为 worker 自循环检查：worker 每轮读取 frame allocator stats，只有物理页占用严格超过 `io_shrink_threshold` 才扫描 inode cache。

**Change:** `io_shrink_threshold` 进入 kconfig，默认 50；task exit 路径不再提交 shrink hint，不读取 frame allocator stats，不同步执行 scan。

**Review:** 阈值判断属于 inode shrinker worker 策略；frame allocator 只提供统计类型和 stats 读取接口。低于或等于阈值时 worker 调用 `yield_now()` 让出调度器并跳过扫描。

**Validation:** 增加 KUnit 覆盖严格大于阈值、等于阈值不触发和零总页数不触发。构建和运行验证见本轮 agent 输出。

## Open Items

- 运行验证未在本轮文档任务中重跑。
- 超出单一 kconfig 阈值 gate 的 pressure-driven reclaim、LRU、quota、allocator direct reclaim 和完整 generic shrinker API 不在本事务范围内。
