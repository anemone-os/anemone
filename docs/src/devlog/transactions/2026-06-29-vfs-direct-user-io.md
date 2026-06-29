# 2026-06-29 - VFS Direct User I/O

**状态：** Active
**负责人：** doruche, Codex
**领域：** VFS / FileOps / FileDesc / syscall read-write / user access
**权威计划：** [RFC-20260629-vfs-direct-user-io](../../rfcs/vfs-direct-user-io/index.md), [不变量需求](../../rfcs/vfs-direct-user-io/invariants.md), [迁移实施计划](../../rfcs/vfs-direct-user-io/implementation.md), [Tracking Issues](../../rfcs/vfs-direct-user-io/tracking-issues.md)
**当前阶段：** 阶段 1B - 已关闭；等待阶段 1C read direct-user gate 启动

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

**Completed:** 公共 RFC、invariants、implementation 和 tracking issues 已存在。阶段 0 已建立本事务日志并连接 RFC、事务索引、当前双周 devlog 和 mdBook Summary；实施前审计、文档验证和 baseline build 已通过。阶段 1A 已提交 user-buffer cursor skeleton 与 fanotify transaction adapter。阶段 1B 已完成 `FileOps` optional hook skeleton、`None`-only fallback 和 repo 范围 static vtable closure。

**In Progress:** 无。阶段 1C 尚未启动。

**Open Blockers:** 无 active Apollyon / Keter tracking issue。若审计或 worker 反馈暴露 backend 需要保存 `UserSpaceHandle`、需要 errno fallback、需要 whole-vector prevalidation、需要改变 `File.pos` 语义、或 `RWF_*` / 完整 `O_DIRECT` 不可避免进入 direct-user ctx，必须停止当前 gate 并回到 RFC review。

**Next Action:** 进入阶段 1C 前，按 RFC 要求由总控启动 read direct-user vertical slice gate，并确认用户接受 read path 作为第一批行为 gate。阶段 1C 才允许为 ramfs/ext4 regular file 安装 `read_user_at`；write direct-user path 仍不得提前落地。

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

### 2026-06-29 - 阶段 1A user-buffer skeleton 与 fanotify adapter

**阶段：** 阶段 1A - `fs/uio.rs` user-buffer skeleton 与 fanotify adapter。

**变更：**

- 新增 `anemone-kernel/src/fs/uio.rs`，提供 VFS-owned `UserBufferSegment`、`UserBufferSink`、`UserBufferSource`、mark / delta、ordinary sink/source copy helper、`keep_prefix_from()`，以及 fanotify metadata record 使用的 exact `UserRecordSink` helper。
- `anemone-kernel/src/task/files.rs` 将 opened-description hook 从 `FileDescOps::read_user` 重命名为 `read_user_transaction`，并把 transaction ctx 从 `UserSpaceHandle + OpenedFileReadUserSegment[]` 改为 `&mut UserBufferSink`。
- `anemone-kernel/src/fs/api/read_write/mod.rs` 在 sequential non-positioned `read` / `readv` transaction dispatch 前构造 `UserBufferSink`；hook 不存在时仍走原 kernel-buffer fallback。positioned read、普通 write/writev/pwrite/pwritev 路径未引入 direct-user dispatch。
- `anemone-kernel/src/fs/fanotify/file.rs` 改用 `UserBufferSink::exact_record().write_exact()` 写 fanotify metadata，保留 event pop / wait、path-event fd reservation、copyout 成功后 commit、copyout 失败 rollback，以及 `notify_read_user_access: false`。
- `anemone-kernel/src/fs/mod.rs` 只 re-export 阶段 1A 必需的 `UserBufferSegment` / `UserBufferSink`。

**边界：**

- 未添加 `FileOps::{read_user_at,write_user_at}` skeleton。
- 未触碰 ramfs/ext4 direct hook。
- 未接入 write direct-user path。
- 未修改 RFC canonical contract 或 register/current-limitations。

**Source audit：**

执行：

```sh
rg -n "UserSpaceHandle|OpenedFileReadUserSegment|read_user_transaction|UserBufferSink|UserBufferSource|UserRecordSink|UserWriteSlice" anemone-kernel/src/fs anemone-kernel/src/task anemone-kernel/src/syscall
```

分类：

- `OpenedFileReadUserSegment` 无命中，raw segment transaction ctx 已删除。
- `read_user_transaction` 命中只在 `task/files.rs` hook 定义 / dispatch、`fs/api/read_write/mod.rs` sequential read/readv transaction dispatch、`fs/fanotify/file.rs` group fd transaction，以及 fanotify fail-closed legacy comment 中；该 hook 仍是 opened-description transaction，不是 ordinary `FileOps` fast path。
- `UserBufferSink` 命中在 `fs/uio.rs` owner、`fs/mod.rs` crate-visible re-export、`task/files.rs` transaction ctx、`fs/api/read_write/mod.rs` construction、`fs/fanotify/file.rs` exact metadata copyout；没有 backend 保存 cursor。
- `UserBufferSource` / `UserRecordSink` 只在 `fs/uio.rs` skeleton 内命中；source 未接入 write path，record helper只供 exact fanotify copyout。
- `UserWriteSlice` 在 fanotify 中已无命中；剩余命中是 `syscall/user_access.rs` 定义、`fs/uio.rs` 集中 helper、`fs/api/read_write/mod.rs` 旧 kernel-buffer fallback、以及 getrandom/getcwd/getdents/readlink/pipe2 等非本阶段 syscall copyout。
- `UserSpaceHandle` 命中仍包括 task/mm/futex 既有身份或 uspace handle 管理、`fs/api/read_write` syscall helper 持有当前 uspace、`fs/file.rs` ioctl ctx、以及 `fs/uio.rs` cursor owner；fanotify transaction 不再直接持有或裸 lock `UserSpaceHandle`。

**Validation:**

- `just fmt kernel`：通过。
- `just build`：通过；构建过程仍有 build wrapper 的 cargo cache warning，无 Rust warning。
- 阶段 1A 未运行 QEMU / LTP。

**结论：** 阶段 1A implementation worker 的代码实现、格式化、构建和 source audit 已通过。未触发阶段 1A stop condition；后续仍需由总控决定是否进入阶段 1B。

**Validation note:** `just fmt kernel` 运行通过，但全局 formatter 还尝试重排 `anemone-kernel/src/task/topology/parent_child.rs` 中一处阶段 1A write set 外的注释换行；该无关格式化变更已撤回，最终 write set 保持在阶段 1A 允许范围内。reviewer 补充 `fs/uio.rs` 注释后，总控修正了本阶段文件中的 formatter 换行；随后 `just fmt kernel --check` 仍因 generated `kconfig_defs.rs` / `platform_defs.rs` 和上述 write set 外旧注释返回非零，但不再报告阶段 1A 文件。最终 `just build`、`git diff --check`、`git diff --no-index --check -- /dev/null anemone-kernel/src/fs/uio.rs` 和上述 source audit 均通过。

**Review gate：**

- 阶段 1A reviewer 未发现 Apollyon / Keter / Euclid / Safe finding；确认 write set 只覆盖阶段 1A 允许文件和本事务日志，且未引入 `FileOps` direct-user hook、ramfs/ext4 hook、write direct-user path 或 RFC canonical contract 变更。
- reviewer 额外在 `fs/uio.rs` 补充 comment-only 注释，记录 cursor 是短生命周期线性 userspace capability、ordinary copy 使用 partial-progress 语义、exact record helper 只服务 fanotify 这类事务 metadata、mark/delta 是 read progress 的唯一派生依据，以及 `UserBufferSource::keep_prefix_from()` 用 file-visible commit 丢弃 speculative user-copy suffix。

### 2026-06-29 - 阶段 1B FileOps optional hook skeleton 与 None-only fallback

**阶段：** 阶段 1B - `FileOps` optional hook skeleton 与 `None`-only fallback。

**Write Set expansion record：**

- 初始阶段 1B write set 覆盖 `anemone-kernel/src/fs/file.rs`、`anemone-kernel/src/fs/uio.rs`、`anemone-kernel/src/fs/**` 下现有 `FileOps` initializer、`anemone-kernel/src/task/files.rs` 和 `anemone-kernel/src/fs/api/read_write/*`。
- `just build` 首次编译发现 `FileOps` 还有 device 层静态 initializer：`anemone-kernel/src/device/block/devfs.rs`、`anemone-kernel/src/device/char/devfs.rs`、`anemone-kernel/src/device/console.rs`。这些 initializer 必须随新增字段显式补齐，否则 repo 范围 static vtable 无法构建。
- worker 按 write set 规则停止并上报；用户/总控随后批准窄扩展，只允许在上述三个 device 文件的既有 `FileOps` initializer 中加入 `read_user_at: None` / `write_user_at: None`。
- 本扩展不改变 accepted contract、阶段顺序或验证 floor；它只让所有既有 file-object vtable 显式 opt out direct-user hook。批准结果记录在本事务日志；canonical RFC 文本不需要修改。

**变更：**

- `anemone-kernel/src/fs/file.rs` 为 `FileOps` 增加 optional `read_user_at` / `write_user_at` 字段，并定义 crate-visible hook type：
  - `read_user_at(&File, pos, &mut UserBufferSink, FileIoCtx) -> Result<(), SysError>`；
  - `write_user_at(&File, pos, &mut UserBufferSource, FileIoCtx) -> Result<usize, SysError>`。
- `File` 增加 read direct-user wrapper skeleton：read hook 存在时由 wrapper 采样 `UserBufferSink` mark / delta 派生成功字节数；hook 返回错误且已有 sink progress 时返回已完成字节，否则返回原始错误。sequential wrapper 仍持 `File.pos` guard，并只按派生 read 字节推进 offset。
- `File` 增加 write direct-user positioned wrapper skeleton，但本阶段没有任何 caller 接入 write direct path；wrapper 只保留 API 形状和 `written <= src.bytes_since(mark)` / `keep_prefix_from()` 一致性护栏。
- `anemone-kernel/src/task/files.rs` 增加 read-side `FileDesc` dispatch wrapper，继续由 fd layer 负责 read access、path-only positioned gate 和 `FileIoCtx` status snapshot。
- `anemone-kernel/src/fs/api/read_write/mod.rs` 在普通 read / readv fallback 前尝试 read direct-user skeleton；hook 为 `None` 时继续走旧 kernel-buffer trampoline。fanotify `read_user_transaction` 仍优先处理 non-positioned transaction read，positioned read 不进入 transaction hook。
- 所有既有 `FileOps` initializer 显式设置 `read_user_at: None` / `write_user_at: None`，包括获批扩展的三个 device vtable。未安装任何 `Some` hook。

**边界：**

- 未实现 ramfs/ext4 direct-user hook。
- 未安装任何 non-`None` direct-user hook initializer。
- 未接入 write direct-user syscall dispatch，也未改变 write path 语义。
- 未启动阶段 1C read direct-user vertical slice或阶段 2 write path。
- 未改变 `RWF_*` / `pwritev2(flags != 0)`、fanotify transaction behavior、public RFC contract、register/current-limitations 或非 regular backend capability。

**Source audit：**

执行：

```sh
rg -n "read_user_at|write_user_at|UserSpaceHandle|Fallback|NotSupported" anemone-kernel/src/fs anemone-kernel/src/task anemone-kernel/src/device
```

分类：

- `read_user_at` / `write_user_at` 在 `fs/file.rs` 的 `FileOps` 字段、hook type、read/write skeleton wrapper 和 `fs/api/read_write/mod.rs` / `task/files.rs` read-side dispatch 中命中。
- 所有 `FileOps` initializer 命中均为 `read_user_at: None` / `write_user_at: None`；搜索 `read_user_at: Some|write_user_at: Some` 无命中。
- `UserSpaceHandle` 命中仍在既有 owner 边界：`fs/uio.rs` cursor owner、`fs/api/read_write` syscall helper、`fs/file.rs` ioctl ctx、device ioctl ctx、task/futex/mm ownership。`FileOps` direct-user hook signature 不接收 `UserSpaceHandle`、raw `VirtAddr` segment、`FileDesc`、fd number 或 task。
- `Fallback` 无命中。direct-user dispatch 使用 `Option` presence only；没有 `SysError` / errno fallback 分支。
- `NotSupported` 命中均为既有 unsupported operation errno、inode/file ops stubs、`pwritev2(flags != 0)` limitation、splice/vmsplice/tee stage-1 stubs、iomux unsupported source handling或设备/arch功能缺口；没有 direct-user fallback 语义。

**Validation:**

- `just fmt kernel`：通过。formatter 再次尝试重排阶段 1B write set 外的 `anemone-kernel/src/task/topology/parent_child.rs` 注释换行；该 formatter-only out-of-scope change 已撤回，最终 diff 不包含该文件。
- `just build`：通过；构建过程仍输出 cargo cache warning。
- `git diff --check`：通过。
- Source audit 命令：通过并按上文分类；`rg -n "read_user_at: Some|write_user_at: Some|Fallback" anemone-kernel/src/fs anemone-kernel/src/task anemone-kernel/src/device` 无命中。

**结论：** 阶段 1B implementation worker 的代码实现、格式化、构建、source audit 和 whitespace validation 已通过。repo 范围 static vtable 构建闭合，hook skeleton 存在但所有 backend 均为 `None` fallback，普通 read/write 行为保持旧 kernel-buffer path。后续进入阶段 1C 前仍需由总控启动 read direct-user vertical slice gate；本阶段不授权 ramfs/ext4 hook implementation 或 write direct path。

**Review gate：**

- 阶段 1B reviewer 未发现 Apollyon / Keter；代码范围符合阶段 1B，hook signature 只接收 `UserBufferSink` / `UserBufferSource` 与 `FileIoCtx`，`None` fallback 仍落回 kernel-buffer path，fanotify transaction 优先级不变，`write_user_at_with_ctx()` 仍无 syscall caller。
- reviewer 提出一个 Euclid：事务日志顶部 handoff 仍停留在“阶段 1A 待启动”。总控已将 `当前阶段`、`Completed`、`In Progress` 和 `Next Action` 更新为阶段 1B 已关闭、阶段 1C 尚未启动，并明确阶段 1C 前需启动 read direct-user vertical slice gate。
