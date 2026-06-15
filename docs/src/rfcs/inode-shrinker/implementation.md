# Inode Shrinker 迁移实施计划

**状态：** Completed
**最后更新：** 2026-06-15
**父 RFC：** [RFC-20260614-inode-shrinker](./index.md)
**不变量：** [Inode Shrinker 不变量需求](./invariants.md)

本文追补记录 `2b0e3900279895c3d8eb604e463249a02c3bddc9` 中已经落地的 inode shrinker 实施形状。当前实现是 VFS inode cache 的后台 opportunistic shrink，不是通用内存压力 shrinker。

## 迁移原则

- eviction 必须是显式路径，不能藏在 `Drop` 中。
- snapshot 只是候选列表，所有正确性由 eviction recheck 和 cache lock 下 removal 负责。
- filesystem 必须通过 `FileSystemFlags` 声明自己的 shrink 能力和 lifetime 边界。
- task exit 只提交 hint，不能在 exit path 做同步 scan。
- backing file cache page counter 是观察数据，不是回收策略。

## 阶段 1：VFS eviction surface

前置条件：

- `SuperBlock` 已维护 resident inode cache。
- filesystem 已提供 `load_inode` 和 `sync_inode`。

交付：

- `SuperBlockOps` 增加 `evict_inode`。
- `SuperBlockInner` 拆分 `indexed` 和 `ghosts`。
- `unindex_inode*()` 把 resident inode 从 indexed 移到 ghosts。
- 新增 `cached_inode_snapshot(include_indexed)`。
- 新增 `try_evict_inode()`，实现 busy check、sync、cache removal、evict 和 rollback。
- `try_evict_all()` 复用 `try_evict_inode()`。

审计：

- `try_evict_inode()` 不得在持 superblock cache lock 时调用 filesystem sync / evict。
- cache removal 必须用 `Arc::ptr_eq` 确认 snapshot object。
- failure rollback 必须恢复 indexed bit 或 ghost list 身份。

write set：

- `anemone-kernel/src/fs/superblock.rs`
- `anemone-kernel/src/fs/filesystem.rs`

验证：

- 本轮文档追补未重新运行构建。建议后续运行 `just build` 和 ext4 create/unlink/reopen 定向路径。

退出条件：

- VFS 提供一个不依赖 `Drop` 的 explicit inode eviction path。

## 阶段 2：ext4 shrinkable cache 与 backing page counter

前置条件：

- ext4 inode 可以从 backing store load。
- ext4 regular file state 已持有 per-inode in-memory page cache。

交付：

- ext4 `FileSystemOps` 声明 `SHRINKABLE_ICACHE`。
- ext4 superblock ops 提供 `evict_inode`。
- `ext4_sync_inode_inner()` 同时服务 sync 和 eviction。
- ext4 regular page insert、truncate invalidation 和 `Ext4RegState::drop()` 维护 backing file cache page counter。
- `fs::resident_file_inode_cache_pages()` 暴露当前 backing file cache page count。

审计：

- `nlink == 0` 是 ext4 deletion terminal 判断，不能使用 `indexed`。
- dirty regular file pages 必须在 inode metadata flush 前同步。
- counter underflow 必须 `assert!`。

write set：

- `anemone-kernel/src/fs/ext4/superblock.rs`
- `anemone-kernel/src/fs/ext4/mod.rs`
- `anemone-kernel/src/fs/ext4/file.rs`
- `anemone-kernel/src/fs/cache_stats.rs`
- `anemone-kernel/src/fs/mod.rs`

验证：

- 建议补充覆盖 dirty page writeback、truncate invalidation counter decrement、drop counter decrement 的定向验证。

退出条件：

- ext4 indexed inode 可以被 evict 后通过 `load_inode` 重建，且 backing file page counter 不 underflow。

## 阶段 3：后台 worker 与 task exit hint

前置条件：

- [kthread service](../kthread/index.md) 可用。
- superblock eviction API 已可用。

交付：

- 新增 `anemone-kernel/src/fs/inode_shrinker.rs`。
- 新增全局 `INODE_SHRINKER` service。
- `InodeShrinkRequest` 使用 merge slot，重复 hint 合并。
- `init_inode_shrinker()` 创建单 worker `inode-shrink-0`。
- `submit_inode_shrink_request()` 在 shrinker 未初始化或 stopping 时 fail closed。
- `task/api/exit` 在 task exit 末尾提交 shrink hint。

审计：

- worker 应在 superblock 和 inode 循环边界检查 stop / park。
- worker 不应扫描 kernel/persistent superblock。
- worker 对 `Busy` / `NotFound` 静默跳过，对其他错误记录日志。

write set：

- `anemone-kernel/src/fs/inode_shrinker.rs`
- `anemone-kernel/src/fs/mod.rs`
- `anemone-kernel/src/task/api/exit/mod.rs`

验证：

- 建议运行 task exit 密集的 user-test 或 LTP profile，观察 shrinker 不影响 exit 收敛。

退出条件：

- task exit 可以异步触发 inode shrinker，且重复 exit hint 不导致 pending queue 无界增长。

## 阶段 4：boot 接入与边界审计

前置条件：

- `kthreadd` 已初始化。
- 所有 CPU 已完成本地 init 并 online。

交付：

- `bsp_kinit()` 在所有 CPU online 后调用 `fs::init_inode_shrinker()`。
- `fs::mounted_superblocks()` 返回 visible namespace 的 unique superblock snapshot。
- `FileSystemFlags::{KERNEL_FS,PERSISTENT_SB,SHRINKABLE_ICACHE}` 作为 shrinker policy boundary。

审计：

- `mounted_superblocks()` 不重复返回同一 superblock。
- shrinker 不扫描 anonymous namespace。
- persistent devfs 不被 shrinker 回收。

write set：

- `anemone-kernel/src/main.rs`
- `anemone-kernel/src/fs/mod.rs`
- `anemone-kernel/src/fs/filesystem.rs`

验证：

- 本轮文档追补未运行 QEMU 或 LTP。后续应至少覆盖 boot 后 shrinker service 创建成功。

退出条件：

- inode shrinker 在 boot 后常驻，并能处理 task exit 提交的合并 hint。

## 旁路审计清单

后续扩展 shrinker 时，审计目标是确认新增路径仍满足 VFS eviction 协议。实现者需要证明：

- 所有 resident inode 回收都走 explicit eviction path，不在 `Drop` 或任意引用释放路径中做 blocking eviction。
- indexed / ghost 身份、`indexed` bit 和 failure rollback 仍保持一致。
- filesystem capability flag 仍是 shrink 范围的边界，kernel/persistent superblock 不被 shrinker 回收。
- eviction 仍保持 snapshot 候选、busy recheck、sync-before-remove、cache-lock removal 和 evict-failure rollback 顺序。
- backing file cache page counter 只作为观察数据，不参与候选选择或回收正确性判断。
- task exit 或其它 trigger 只提交 shrink hint，不在触发路径同步执行全局 inode scan。

审计结论应把相关路径分类为 cache identity、filesystem capability、eviction path、counter path、trigger path 或观察路径。

## 可观测性清单

后续 review 至少应能回答：

- shrinker 本轮扫描了哪些 filesystem 类型。
- 某个 inode 被跳过是 busy、not found 还是 filesystem error。
- successful eviction 数量是多少。
- ext4 dirty regular pages 是否在 eviction 前同步。
- backing file cache page counter 是否和 page insert / invalidate / drop 匹配。
- task exit 是否只提交 hint，没有同步执行 shrink scan。

## 停止边界

需要回到 RFC 层的情况：

- 新增内存压力、LRU、quota 或 allocator direct reclaim。
- 新增 filesystem 想回收 indexed inode，但不能证明 `load_inode` 可重建。
- 新增 busy pin 类型，例如 dentry alias、writeback pin、mmap invalidation pin。
- 修改 eviction 顺序为 remove-before-sync 或 evict-under-cache-lock。
- 让 counter 或 task exit 次数参与正确性判断。

可以作为普通小改继续推进的情况：

- 新增日志或统计字段，且不参与 eviction 决策。
- 修复某个 filesystem 的 `sync_inode` / `evict_inode` 错误映射。
- 为已有 shrinker 路径补定向测试。

## Write Set 扩展记录

- 2026-06-15：本文是 commit `2b0e3900279895c3d8eb604e463249a02c3bddc9` 的追补文档，没有新的代码 write set 扩展。
