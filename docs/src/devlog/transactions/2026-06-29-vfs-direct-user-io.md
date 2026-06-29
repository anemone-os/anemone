# 2026-06-29 - VFS Direct User I/O

**状态：** Active
**负责人：** doruche, Codex
**领域：** VFS / FileOps / FileDesc / syscall read-write / user access
**权威计划：** [RFC-20260629-vfs-direct-user-io](../../rfcs/vfs-direct-user-io/index.md), [不变量需求](../../rfcs/vfs-direct-user-io/invariants.md), [迁移实施计划](../../rfcs/vfs-direct-user-io/implementation.md), [Tracking Issues](../../rfcs/vfs-direct-user-io/tracking-issues.md)
**当前阶段：** 阶段 1A - 待启动

## 范围

本事务跟踪 `vfs-direct-user-io` RFC 的 staged implementation：

- 在 `fs/uio.rs` 引入 VFS-owned user-buffer cursor；
- 先把 fanotify opened-description read transaction 的 raw user copyout 收口到同一 adapter；
- 为 ordinary `FileOps` 增加 optional positioned direct-user hooks，并固定 `None`-only fallback；
- 先验证 ramfs/ext4 read direct-user vertical slice，再进入 write direct-user path；
- 实现收口时重新对齐 register/current-limitations，避免把本 RFC 混同为 Linux `O_DIRECT`、`RWF_*`、mmap coherency 或 splice/vmsplice 的完成信号。

非目标：

- 不实现完整 Linux `O_DIRECT`、page pin、zero-copy 或 bypass page cache。
- 不处理 `RWF_*` per-call flags。
- 不把 pipe、eventfd、timerfd、char dev、block dev、procfs snapshot 或 fanotify group fd 自动纳入 ordinary direct-user fast path。
- 不改变 shared opened-description sequential offset interleaving；第一阶段只接受 RFC 中明确的窄 `File.pos -> user-buffer/UserSpace` 锁序。
- 不由 agent 侧运行 QEMU / LTP，除非用户后续明确授权；阶段 gate 可记录用户侧目标测例或性能路径证据。

## 不变量

- 用户地址、iovec progress、copy 和 `EFAULT` 映射必须由 `fs/uio.rs` 的 VFS-owned cursor 统一表达。
- `FileOps` direct-user hook 的存在性是 fallback 的唯一信号；hook 返回的 `SysError` 是用户可见真实错误。
- ordinary vectored I/O 保留 partial-progress 语义，不能使用 whole-vector prevalidation 覆盖已有 segment progress。
- fanotify read transaction 继续归 opened file description hook 所有；metadata copyout 通过 exact transaction helper 收口。
- opened file description 仍是 persistent status flags 的单一真相源，backend 只观察 `FileIoCtx` snapshot。
- 顺序 direct-user I/O 第一阶段保持 `File.pos` 线性化点；任何 reserve-offset / commit-offset 或缩短持锁范围的变化必须回到 RFC review。
- worker 未经批准不得越过阶段 write set；需要扩大时先向总控/用户提交 expansion request，并把批准结果写入本事务日志。

## Handoff

**Last Updated:** 2026-06-29

**Current Branch:** `dev/drc/udio`

**Canonical RFC:** [RFC-20260629-vfs-direct-user-io](../../rfcs/vfs-direct-user-io/index.md), [Invariants](../../rfcs/vfs-direct-user-io/invariants.md), [Implementation Plan](../../rfcs/vfs-direct-user-io/implementation.md), [Tracking Issues](../../rfcs/vfs-direct-user-io/tracking-issues.md)

**Completed:** 公共 RFC、invariants、implementation 和 tracking issues 已存在。阶段 0 已建立本事务日志并连接 RFC、事务索引、当前双周 devlog 和 mdBook Summary；实施前审计、文档验证和 baseline build 已通过。

**In Progress:** 无；尚未进入阶段 1A 代码实现。

**Open Blockers:** 无 active Apollyon / Keter tracking issue。若审计或 worker 反馈暴露 backend 需要保存 `UserSpaceHandle`、需要 errno fallback、需要 whole-vector prevalidation、需要改变 `File.pos` 语义、或 `RWF_*` / 完整 `O_DIRECT` 不可避免进入 direct-user ctx，必须停止当前 gate 并回到 RFC review。

**Next Action:** 按阶段 1A write set 启动 `fs/uio.rs` user-buffer skeleton 与 fanotify adapter。不得提前实现 ordinary `FileOps` direct-user hooks、ramfs/ext4 direct hooks 或 write direct-user path。

**Do Not Redo:** 不要把 `FileDescOps::read_user` 扩成 ordinary filesystem fast path；不要让 backend 直接接收 raw user memory capability；不要用 `SysError::NotSupported` 等 errno 表达 fallback；不要把本 RFC 当成 Linux `O_DIRECT` 或 `RWF_*` 实现；不要把 write path 抢在 read gate 闭合前落地。

## Phase Log

### 2026-06-29 - 阶段 0 事务日志启动与实施前审计

**阶段：** 阶段 0 - 实施前审计。

**变更：** 在代码实现前建立本事务日志，并把 [RFC-20260629-vfs-direct-user-io](../../rfcs/vfs-direct-user-io/index.md)、[Tracking Issues](../../rfcs/vfs-direct-user-io/tracking-issues.md)、事务索引、mdBook Summary 和当前双周 devlog 连接到同一条实现记录。`docs/src/rfcs.md` 已有本 RFC 入口，本阶段确认其仍指向公共 RFC。

**前置状态：**

- 分支为 `dev/drc/udio`，阶段启动时 `git status --short` 为空。
- RFC 状态为 `Accepted for Implementation`，tracking issues 为 `No active issues`。
- register/current-limitations 已在阶段前读取；与本 RFC 最相关的仍是 `O_DIRECT`/status flags、file-backed mmap fault、truncate coherency、ROFS mmap/writeback、splice copy-backed stage-1 等独立限制，阶段 0 不关闭这些条目。

**写集审计：**

- 阶段 0 执行 RFC 要求的搜索：

```sh
rg -n "read_user|OpenedFileReadUser|UserSpaceHandle|UserReadSlice|UserWriteSlice|FileIoCtx|FileOps \\{|read_at|write_at|FAN_ACCESS|FAN_MODIFY" anemone-kernel/src/fs anemone-kernel/src/task anemone-kernel/src/syscall
```

- 当前 `FileOps` 只有 kernel-buffer `read` / `write` / `read_at` / `write_at` entry，尚无 direct-user hook 字段；阶段 1B 必须显式完成 vtable skeleton。
- `FileDescOps::read_user` 仍是 opened-description rare hook，ctx 当前携带 `UserSpaceHandle` 和 `OpenedFileReadUserSegment`；普通 positioned `pread*` 不进入该 hook。
- `fs/api/read_write` 在 sequential single-buffer 和 vectored read 上优先尝试 `file.read_user()`；hook 不存在时先验证 user write buffer，再分配 kernel buffer、执行 `FileDesc::{read,read_at}`，最后 copyout 到用户缓冲区。
- write path 当前仍先从用户缓冲区 copy 到 kernel buffer，再调用 `FileDesc::{write,write_at}`；阶段 0 未改变该路径。
- `pwritev2(flags != 0)` 仍打印 `[NYI]` 并返回 `SysError::NotSupported`；`RWF_*` 不纳入本 RFC 代码范围。

**fanotify read transaction 顺序：**

- `fanotify::file::description_ops()` 安装 `read_user: Some(fanotify_read_user)`，并设置 `notify_read_user_access: false`，说明 control fd read transaction 不产生 ordinary `FAN_ACCESS`。
- `fanotify_read_user()` 先计算总 user buffer 长度，短于 metadata 时返回 `InvalidArgument`；随后 `group.pop_read_state()`，空队列下按 `NONBLOCK` 返回 `Again` 或 `wait_for_event()` 阻塞等待。
- `pop_read_state()` / `wait_for_event()` 返回后 event 已从 queue 移除且 group lock 已释放；后续 user copy、path open 和 fd-table work 都在 group lock 外执行。
- `submit_event_record()` 先 `prepare_event_fd()` 取得 path-event fd reservation，再构造 metadata，调用 `write_metadata_to_segments()` 做完整 metadata copyout；copyout 成功后才 `pending_fd.commit()`。
- `PendingEventFd` drop-before-commit 负责 rollback 未发布 fd table slot；当前 copyout failure 不会留下用户未收到的 fd。
- 当前 metadata copyout 仍裸用 `UserSpaceHandle::lock()`、`UserWriteSlice` 和 raw `OpenedFileReadUserSegment`，但已经先验证完整 record 再 copy；这是阶段 1A 必须替换为 user-buffer exact helper 的点。

**ramfs/ext4 copy 与锁序审计：**

- ramfs regular file 的状态是 `pages: RwLock<BTreeMap<usize, FrameHandle>>`。write path 通过 `ensure_page()` 在 map 锁内取得或插入 `FrameHandle` clone，随后对 frame slice copy；后续 direct-user write 应保持“先取得稳定 frame，再 user copy”的形状。
- ramfs read path 当前 `copy_out()` 在 `pages.read()` 命中的分支里从 frame slice copy 到 kernel buffer；阶段 1C direct-user read 不应在持 pages map lock 时触发 user copy，应先取得 stable frame handle 或数据片段。
- ext4 regular file 的状态是 `pages: RwLock<BTreeMap<usize, Ext4RegPage>>`，并通过 ext4 superblock `read_tx` / `write_tx` 访问 lwext4。
- ext4 read path `load_page()` 先查 page cache，miss 时在 `read_tx()`/`with_fs()` 下读取整页到 frame，再把 page 插入 cache；`copy_out()` 使用返回的 cloned page/frame copy 到 kernel buffer。后续 direct-user read 可在 page/frame 稳定后释放 backend 锁再 copy。
- ext4 write path `page_for_write()` 获取或创建 page，`copy_in()` 先向 frame copy，再用短 `pages.write()` 标 dirty；后续 direct-user write 必须继续避免在 ext4 transaction lock、page-cache map lock或 spin/noirq context 内推进 user-buffer source。
- `sync_page()` 当前在 pages write lock 下调用 ext4 writeback，这不是本 RFC 阶段 1C direct read gate 的目标，但后续 write gate 审计不能把该形状误当成 user copy 可用锁序。

**模块边界预检：**

- `fs/api/read_write/mod.rs` 当前同时包含 syscall 参数解析、iovec import、kernel-buffer fallback、notification 和 copy helper。阶段 1A 若继续扩大该文件，可以按 RFC 在同一 owner 内做行为保持的局部目录化拆分；任何 public API、`FileDesc` surface 或 fanotify transaction 语义变化都必须先申请 write-set expansion。

**Review / subagents：**

- 总控启动两个只读 explorer：一个审计 fanotify / `FileDescOps::read_user` / `pwritev2`，一个审计 ramfs/ext4 copy 与锁序。两个 explorer 都被明确禁止修改文件；其结论只用于补强阶段 0 审计，不授权阶段 1A 之外的 write set。
- ramfs/ext4 explorer 已返回：未发现阶段 0 停止条件。它确认普通 ramfs/ext4 read/write 仍走 kernel-buffer trampoline；ramfs read 当前可能在 `pages.read()` guard 下 copy 到 kernel buffer，后续 direct-user read 必须短锁段 clone/pin frame 后释放 map lock 再 user copy；ext4 read 当前可在 `load_page()` 返回 stable page 后 copy，后续必须保持 user copy 不进入 ext4 `tx_lock` / `fs_lock` / `pages.write()`；write gate 需要把 user-copy consumed bytes 与 file-visible committed bytes 分开。
- fanotify / `pwritev2` explorer 已返回：未发现阶段 0 停止条件。它确认 `FileDescOps::read_user` 当前仍是 opened-description / fd-facing transaction hook，唯一 `read_user: Some(...)` initializer 是 fanotify group fd；ordinary read fallback 仍是 kernel buffer + `FileDesc::{read,read_at}`；fanotify read transaction 顺序为 buffer 长度检查、event pop / wait、event fd reservation、metadata fd number 构造、完整 metadata copyout、copyout 成功后 commit，失败则由 reservation drop rollback；`notify_read_user_access: false` 保持 fanotify control fd 不提交 ordinary `FAN_ACCESS`；`pwritev2(flags != 0)` 仍打印 NYI 并返回 `NotSupported`，不进入本 RFC 后续代码范围。

**Validation:**

- `git diff --check`：通过。
- `git diff --no-index --check -- /dev/null docs/src/devlog/transactions/2026-06-29-vfs-direct-user-io.md`：无 whitespace 诊断；非零退出码是新增文件与 `/dev/null` 比较的正常 no-index difference 状态。
- `mdbook build docs`：通过，输出到 `docs/book`。
- `just build`：通过；构建过程中仅输出 cargo cache warning。
- 阶段 0 不运行 QEMU / LTP。

**结论：** 阶段 0 已关闭。未发现需要回到 RFC review 的额外 shared-contract 问题；后续可启动阶段 1A，但不得越过阶段 1A write set。
