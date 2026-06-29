# RFC-20260629-vfs-direct-user-io

**状态：** Accepted for Implementation
**负责人：** doruche, Codex
**最后更新：** 2026-06-29
**领域：** VFS / FileOps / FileDesc / syscall read-write / user access
**事务日志：** None
**开放问题：** 当前无 active tracking issue，见 [Tracking Issues](./tracking-issues.md)。
**下一步：** 进入实现前创建 transaction devlog 并建立双向链接；随后执行 [迁移实施计划](./implementation.md) 的阶段 0 审计。

## 摘要

本 RFC 定义 Anemone 普通文件 I/O 的 direct userspace copy 路径。目标是在不引入 Linux `O_DIRECT` 语义、不改变 opened file description 状态所有权、不把用户指针处理分散到各个 backend 的前提下，让普通 `read` / `readv` / `pread` / `preadv` 和后续 `write` / `writev` / `pwrite` / `pwritev` 能绕过 syscall helper 中的大块 kernel buffer 中转。

核心方向是在 `fs/uio.rs` 引入 VFS 拥有的 `UserBufferSink` / `UserBufferSource` 线性 cursor。`FileOps` backend 不再直接接收 `UserSpaceHandle` 或 raw user segments，而是通过短生命周期 user-buffer capability 推进用户 copy。backend 仍拥有文件内容、page cache / frame 选择和 dirty / size 提交；用户地址验证、EFAULT 映射、跨 iovec progress、partial-success 规则和 fanotify notification 仍集中在 VFS / `fs/api/read_write` 调度边界。

## 背景

当前普通 `read` / `pread` / `readv` / `preadv` 路径通过 kernel buffer 中转：

1. syscall helper 为本次 I/O 分配 `Vec<u8>`；
2. `FileDesc` / `File` / backend 把文件内容读入 kernel buffer；
3. syscall helper 再把 kernel buffer copy 到用户缓冲区。

`write` / `pwrite` / `writev` / `pwritev` 也有对称问题：先把用户缓冲区完整 copy 到 kernel buffer，再调用 backend 写入。这个形状简单，但普通文件热路径会反复分配和复制。

当前 `FileDescOps::read_user` 不是普通 filesystem fast path。它是 opened-description 特殊 read transaction hook，当前主要服务 fanotify：一次 read 可能消费 event、安装 event fd、执行 metadata copyout 和 commit / rollback，并且需要 opened-description 级 notification suppression。它没有 positioned I/O 语义，`pread*` 也不进入该 hook。

当前 `FileIoCtx` 已经把 opened-description status flags 收口为短生命周期 snapshot。direct-user I/O 必须延续这个所有权：opened file description 继续是 persistent status flags 的单一真相源，`FileOps` 只能观察 `FileIoCtx`，不能保存 fd state。

已有 register 限制也必须保持清楚：

- `pwritev2(flags != 0)` 仍是 stage-1 limitation，本 RFC 不引入 `RWF_*` per-call flag 语义；
- `O_DIRECT` 当前只是 Linux-visible flag / status snapshot 的一部分，本 RFC 的 direct userspace copy 不是完整 Linux direct I/O；
- file-backed mmap fault / truncate coherency 仍有独立限制，direct-user copy 只能把这类风险纳入验证矩阵，不能把 mmap 一致性当作本 RFC 顺手收口的范围。

## 目标

- 为普通 file content I/O 建立 direct userspace copy contract，减少普通文件 read/write hot path 的 kernel buffer 分配和二次 copy。
- 在 `fs/uio.rs` 引入 VFS 拥有的 `UserBufferSink` / `UserBufferSource`，集中用户地址验证、copy、progress 和 EFAULT 映射。
- 让 `FileOps` backend 通过 positioned hook 表达 direct-user 能力，但不直接接触 raw user segment、`UserSpaceHandle`、`FileDesc`、fd number 或 task file table。
- 保持 `FileDescOps::read_user` 的 opened-description transaction 边界，并把它重命名为 `read_user_transaction`。
- 用同一套 user-buffer adapter 收口 fanotify 当前裸 `UserSpaceHandle` / raw segment copyout，但不改变 fanotify event pop、fd reservation、commit / rollback 和 notification suppression 语义。
- 固定 `None`-only fallback 规则：optional hook 不存在时走 kernel-buffer fallback；hook 存在时返回值就是该路径结果，不允许通过 errno 表达 fallback。
- 固定 Linux 风格 partial-success 规则：已有外部可见进展 `N > 0` 时返回 `N`，只有 `N == 0` 时返回原始错误。
- 先收口 read direct-user path；write direct-user path 只能在 read path 的 fault / partial / offset / notification 证据闭合后进入。

## 非目标

- 不实现完整 Linux `O_DIRECT` direct I/O、alignment、page pin 或 bypass page cache 语义。
- 不实现 zero-copy、splice/vmsplice 或用户页长期 pin。
- 不处理 `RWF_NOWAIT`、`RWF_DSYNC`、`RWF_SYNC`、`RWF_APPEND` 等 per-call flags。
- 不把 pipe、eventfd、timerfd、char dev、block dev、procfs snapshot 或 fanotify group fd 自动纳入 ordinary direct-user fast path。
- 不重构 fanotify transaction 协议；本 RFC 只把 fanotify copyout 从 raw `UserSpaceHandle` / segment 访问改成 user-buffer adapter。
- 不缩短顺序 `read` / `write` 的 `File.pos` 线性化窗口；第一阶段接受 direct-user sequential path 的窄 `File.pos -> user-buffer/UserSpace` 锁序，以保持 shared opened-description offset interleaving。未来若要改成 reserve-offset、缩短持锁范围或改变并发 offset 可见性，必须回到 RFC review。
- 不要求 agent 侧完整跑 QEMU / LTP；用户运行的目标测例 / 性能路径作为阶段 gate 的行为证据。

## 文档地图

Canonical：

- [不变量需求](./invariants.md)
- [迁移实施计划](./implementation.md)
- [Tracking Issues](./tracking-issues.md)

背景材料：

- None

## 方案

### User-buffer ownership

新增 VFS-owned user I/O cursor，代码放在 `anemone-kernel/src/fs/uio.rs`。名称按数据流角色定义：

```rust
pub(crate) struct UserBufferSegment {
    base: VirtAddr,
    len: usize,
}

#[derive(Clone, Copy)]
pub struct UserBufferMark {
    cursor: UserBufferCursor,
    done: usize,
}

#[derive(Clone, Copy)]
struct UserBufferCursor {
    segment: usize,
    offset: usize,
}

pub struct UserBufferSink<'a> {
    uspace: &'a UserSpaceHandle,
    segments: &'a [UserBufferSegment],
    cursor: UserBufferCursor,
    written: usize,
}

pub struct UserBufferSource<'a> {
    uspace: &'a UserSpaceHandle,
    segments: &'a [UserBufferSegment],
    cursor: UserBufferCursor,
    consumed: usize,
}
```

`UserBufferSink` 用于 `read` / `readv` / `pread` / `preadv`，表示 file -> user。`UserBufferSource` 用于 `write` / `writev` / `pwrite` / `pwritev`，表示 user -> file。

这两个类型是短生命周期线性 cursor。它们由 `fs/api/read_write` 在完成 syscall 参数解析、count clamp、iovec 导入和 zero-length fast path 后构造。它们内部可以持有 `UserSpaceHandle`、segments 和当前 segment/progress，但这些字段不向 backend 暴露。backend 只能通过公开方法推进 copy，例如：

```rust
impl<'a> UserBufferSink<'a> {
    pub(crate) fn new(
        uspace: &'a UserSpaceHandle,
        segments: &'a [UserBufferSegment],
    ) -> Self;

    pub fn remaining(&self) -> usize;
    pub fn write_from_slice(&mut self, src: &[u8]) -> Result<usize, SysError>;
    pub fn write_zeros(&mut self, len: usize) -> Result<usize, SysError>;
    pub fn exact_record<'b>(&'b mut self) -> UserRecordSink<'a, 'b>;

    pub(crate) fn mark(&self) -> UserBufferMark;
    pub(crate) fn bytes_since(&self, mark: UserBufferMark) -> usize;
}

impl<'a> UserBufferSource<'a> {
    pub(crate) fn new(
        uspace: &'a UserSpaceHandle,
        segments: &'a [UserBufferSegment],
    ) -> Self;

    pub fn remaining(&self) -> usize;
    pub fn copy_into_slice(&mut self, dst: &mut [u8]) -> Result<usize, SysError>;
    pub fn mark(&self) -> UserBufferMark;
    pub fn keep_prefix_from(&mut self, mark: UserBufferMark, committed: usize);

    pub(crate) fn bytes_since(&self, mark: UserBufferMark) -> usize;
}

pub struct UserRecordSink<'a, 'b> {
    inner: &'b mut UserBufferSink<'a>,
}

impl<'a, 'b> UserRecordSink<'a, 'b> {
    pub fn write_exact(&mut self, record: &[u8]) -> Result<(), SysError>;
}
```

具体方法名可在实现时按代码风格调整，但 contract 固定：

- `remaining()` 是当前请求尚未尝试 copy 的字节数；
- `UserBufferSink::write_from_slice()` / `write_zeros()` 允许短 copy，返回本次实际写入用户缓冲区的字节；
- read path 的用户可见成功字节数只从 `UserBufferSink` 的 mark delta 派生，`read_user_at` hook 不再返回 `usize`；
- `UserBufferSource::copy_into_slice()` 允许短 copy，表示已从用户缓冲区消费到 kernel/backend 临时目标的字节；
- write path 的 `write_user_at` 返回值表示 file-visible committed bytes，不等同于 `UserBufferSource` consumed bytes；
- `keep_prefix_from(mark, committed)` 用于把 source cursor 收敛到已提交前缀，避免 user-copy progress 反向伪装成 file-visible progress；
- `UserRecordSink::write_exact()` 只给 fanotify metadata record 这类事务 copyout 使用，必须先验证完整目标范围，失败时不产生半条 record；
- backend 不得保存 cursor、cursor 内部 segment、`UserSpaceHandle` 或用户地址作为长期状态。

### FileOps direct-user hooks

`FileOps` 增加 optional positioned direct-user hooks。hook 接收 user-buffer cursor 和 `FileIoCtx`，不接收 `UserSpaceHandle` 或 raw segments：

```rust
pub type ReadUserAt = for<'a> fn(
    &File,
    pos: usize,
    dst: &mut UserBufferSink<'a>,
    ctx: FileIoCtx,
) -> Result<(), SysError>;

pub type WriteUserAt = for<'a> fn(
    &File,
    pos: usize,
    src: &mut UserBufferSource<'a>,
    ctx: FileIoCtx,
) -> Result<usize, SysError>;

pub read_user_at: Option<ReadUserAt>,
pub write_user_at: Option<WriteUserAt>,
```

这些 hook 只表达 backend offset-addressed direct userspace copy 能力。它们不得读取 fd table、`FileDesc`、fd number、opened-description lock 或 task file table。需要 status 时只能通过 `FileIoCtx` 读取本次 snapshot。

read wrapper 的成功字节数由 VFS 采样 sink progress：

```rust
let mark = dst.mark();
let result = read_user_at(file, pos, dst, ctx);
let copied = dst.bytes_since(mark);

match result {
    Ok(()) => Ok(copied),
    Err(err) if copied > 0 => Ok(copied),
    Err(err) => Err(err),
}
```

write wrapper 的成功字节数由 backend 返回的 file-visible commit 决定，并用 source progress 做一致性护栏：

```rust
let mark = src.mark();
let written = write_user_at(file, pos, src, ctx)?;
assert!(written <= src.bytes_since(mark));
src.keep_prefix_from(mark, written);
Ok(written)
```

fallback 只由 `Option` 表达：

- `None`：该 file type / backend 没有 ordinary direct-user 能力，VFS 走现有 kernel-buffer fallback；
- `Some(hook)`：该 hook 对该 direct path 负责到底，read 成功由 sink delta 派生，write 成功由 hook 返回的 committed bytes 表达，失败返回真实错误；
- hook 不得通过 `SysError::NotSupported`、`SysError::UnsupportedIoctl`、`SysError::Again` 或其它 errno 表达“请 fallback”。

### File / FileDesc / syscall ownership

`fs/api/read_write` 负责：

- syscall 参数解析、`MAX_RW_COUNT` clamp 和 iovec 导入；
- 构造 `UserBufferSink` / `UserBufferSource`；
- ordinary direct-user path 与 kernel-buffer fallback 的调度；
- vectored I/O partial-progress 聚合；
- fanotify `FAN_ACCESS` / `FAN_MODIFY` notification；
- `pwritev2(flags != 0)` 继续按当前 limitation 返回 unsupported。

`FileDesc` 层负责：

- read/write access；
- path-only 检查；
- `FileStatusFlags` snapshot -> `FileIoCtx`；
- `O_APPEND` 决策。`pwrite` 在 `O_APPEND` 下是否走 append 路径继续由 `FileDesc` 层决定，backend 只看到最终 offset / append helper 选择。

`File` 层负责：

- zero-length fast path；
- regular content 的 readonly mount gate；
- 顺序 I/O 的 `File.pos` lock、offset 推进和 scoped `File.pos -> user-buffer/UserSpace` 锁序；
- successful write 后 inode metadata update；
- hook 不存在时回退到现有 kernel-buffer path。

顺序 direct-user wrapper 在 `File.pos` guard 覆盖范围内调用 direct-user hook 并推进 user-buffer cursor。这个选择只用于保持 shared opened-description cursor 的现有线性化点：同一个 opened file description 上的并发 sequential I/O 仍按 `File.pos` guard 排队，offset 只按实际 copied / committed 字节推进。它不是 backend I/O 的全局串行化证明；backend 仍必须先在短锁段中取得稳定 page/frame 或数据片段，释放 backend 锁后再访问 user-buffer cursor。

backend 负责：

- 文件内容可见范围、EOF、page/frame 获取和文件系统错误；
- 不在不合适的锁内访问 user-buffer cursor；
- write path 在 user copy 完成后用明确短锁段提交 dirty / size / metadata；
- read hook 不返回字节数，write hook 返回字节数必须只包含已经对外可见、可计入 `File.pos` / syscall result 的 file-visible 进展。

### Fanotify transaction adapter

`FileDescOps::read_user` 重命名为 `read_user_transaction`，语义保持 opened-description transaction，不变成 ordinary `FileOps` fast path。

transaction ctx 改为接收 `UserBufferSink` 或其 restricted transaction adapter：

```rust
pub struct OpenedFileReadUserTransactionCtx<'a> {
    pub file: &'a File,
    pub status_flags: FileStatusFlags,
    pub dst: &'a mut UserBufferSink<'a>,
    pub notification_suppressed: bool,
}
```

fanotify 仍拥有：

- group event pop / wait；
- path-event fd reservation；
- metadata 中 fd number 填写；
- 完整 metadata copyout；
- copyout 成功后 fd commit；
- copyout 失败时 fd reservation rollback；
- control fd `notify_read_user_access: false` 和 notification suppression。

fanotify 不再裸调 `UserSpaceHandle::lock()` 或遍历 raw segments。metadata copyout 通过 `UserBufferSink::exact_record().write_exact()` 或等价 helper 完成，确保坏第二个 iovec 不留下半条 metadata record。

### Dispatch rules

顺序 `read` / `readv`：

1. clamp count / parse iovec；
2. 构造 `UserBufferSink`；
3. 如果存在 `FileDescOps::read_user_transaction`，由 transaction hook 完整接管；
4. 否则尝试 `File` 顺序 direct-user read wrapper；
5. wrapper 发现 backend hook 为 `None` 时走 kernel-buffer fallback；
6. 最终成功字节数大于 0 时提交一次 `FAN_ACCESS`。

`pread` / `preadv`：

1. parse offset；
2. 构造 `UserBufferSink`；
3. 调用 positioned direct-user read wrapper；
4. wrapper 发现 backend hook 为 `None` 时走 kernel-buffer fallback；
5. 不修改 `File.pos`；
6. 最终成功字节数大于 0 时提交一次 `FAN_ACCESS`。

`write` / `writev` / `pwrite` / `pwritev`：

1. 构造 `UserBufferSource`；
2. `FileDesc` 层完成 access、path-only、status snapshot 和 `O_APPEND` 决策；
3. `File` 层完成 readonly mount gate 和顺序 offset / positioned offset wrapper；
4. wrapper 发现 backend hook 为 `None` 时走 kernel-buffer fallback；
5. 成功字节数大于 0 时提交一次 `FAN_MODIFY`。

## 接受边界

接受本 RFC 意味着可以对 `fs/uio.rs`、user access、`fs/api/read_write`、`FileDescOps`、`FileOps`、fanotify、ramfs 和 ext4 做 staged shared-contract 迁移。实现必须遵守 [不变量需求](./invariants.md)，并按 [迁移实施计划](./implementation.md) 的 gate 推进。

以下变化必须回到本 RFC 或新增 follow-up RFC：

- 让 backend 直接接收 `UserSpaceHandle`、raw user segment 或 fd / task object。
- 用 errno 表达 direct-user fallback。
- 为 ordinary vectored I/O 使用 whole-vector prevalidation，导致已有 segment progress 被后续 fault 覆盖。
- 把 fanotify transaction 改造成普通 `FileOps` content I/O。
- 缩短 sequential `File.pos` 持锁范围、引入 reserve-offset / commit-offset 协议，或改变 shared opened-description offset interleaving 语义。
- 引入完整 Linux `O_DIRECT`、page pin、zero-copy 或 bypass page cache。
- 把 pipe、eventfd、timerfd、char dev、block dev、procfs snapshot 或 fanotify 自动纳入 ordinary direct-user fast path。
- 把 `RWF_*` per-call flags 混入 `FileIoCtx` 或 direct-user ctx，而不是单独扩展 per-IO ctx。

## 备选方案

### 继续使用 kernel buffer trampoline

拒绝作为长期方向。它简单且语义稳定，但普通文件热路径会持续多一次分配和 copy，后续 write 侧 partial / fault 行为也只能维持粗粒度 trampoline 语义。

### 让 `FileOps` 直接接收 `UserSpaceHandle + segments`

拒绝。这样会把用户指针 ABI、EFAULT 映射、iovec progress 和锁序散落到 ramfs、ext4 和后续 backend 中，违反 user access 的单一 owner boundary。

### 继续使用 `UserReadIter` / `UserWriteIter`

拒绝。按内核动作命名会让 read path 的 `UserWriteIter` 和 write path 的 `UserReadIter` 在 review 中反复反向解释，也容易让 hook 返回值和 cursor progress 被误读成同一个 truth source。`UserBufferSink` / `UserBufferSource` 直接表达数据流角色：read 写入用户 sink，write 从用户 source 读取。

### 用 `Result<usize, SysError>` 中的某个错误表示 fallback

拒绝。`SysError::NotSupported` 等错误已经有真实用户可见 errno 语义，不能同时表示调度策略。fallback 只由 hook 是否存在表达。

### 先在 read path 做 whole-vector prevalidation

拒绝作为 ordinary vectored I/O 语义。当前 `readv` / `writev` 已经按 segment 推进，已有 progress 后后续 fault 返回已完成字节数。whole-vector prevalidation 只能用于 fanotify metadata record 这类确实需要 exact transaction copyout 的特殊路径。

### 直接复用 fanotify `FileDescOps::read_user`

拒绝。fanotify hook 是 opened-description transaction，不具备 positioned I/O 语义，也包含 event consumption 和 fd publication 协议。普通文件 direct-user I/O 必须留在 `FileOps` / `File` / `FileDesc` 的 data I/O 分层。

### reserve-offset 后锁外 direct-user copy

暂不作为第一阶段方案。它可以避免在 `File.pos` guard 内进入 user-buffer copy，但会立即引入新的 sequential offset reservation / commit 协议：read/write 在 fault、short I/O、EOF、overflow 或 backend error 后需要决定已预留但未实际完成的区间如何回滚或暴露。若预留过多，坏后段 iovec 和短读会把 offset 推进到未产生可见进展的位置；若不预留，又会让共享 opened-description 上的并发 sequential I/O 取得同一个起始 offset。当前 RFC 优先保持现有 cursor 线性化点，把 reserve-offset 作为未来性能优化单独 review。

## 风险

- `UserBufferSink` / `UserBufferSource` 若设计过宽，会变成新的 `UserSpaceHandle` 透传。控制方式是只暴露 copy/progress 方法，不暴露 raw segments、guard 或 `UserSpaceHandle`。
- Direct-user read 可能暴露出当前 trampoline 没有覆盖的细粒度 EFAULT / partial 行为差异。控制方式是用 targeted userspace fault tests 固定 ABI，并把 read path 单独作为 gate。
- Write direct-user 比 read 风险更高：copyin、dirty、size、append 和 metadata update 必须把 user-copy progress 与 file-visible committed bytes 分开。控制方式是 read path 验证通过后再开 write stage，并用 source mark / `keep_prefix_from()` 约束 commit 前缀。
- Sequential direct-user I/O 会把用户页 fault 和 `UserSpace` mutex 放入 `File.pos` guard 覆盖范围。控制方式是把该规则收窄为 `File.pos -> user-buffer/UserSpace`，禁止 backend 锁内 user copy 和 `UserSpace -> File.pos` 反向路径；未来若要用 reserve-offset 缩短持锁范围，必须单独 review。
- fanotify adapter 如果没有 exact transaction helper，可能把坏第二个 iovec 变成半条 metadata record。控制方式是 fanotify 使用 transactional exact copyout，不走 ordinary partial copy helper。
- `O_DIRECT` 命名容易混读。控制方式是所有新类型和注释使用 `user` / `userspace` / `buffered direct copy`，不得把它描述为 Linux direct I/O。

## 收口

完成后需要记录：

- `UserBufferSink` / `UserBufferSource` 是 user buffer copy/progress 的唯一普通 VFS cursor，代码位于 `fs/uio.rs`；
- `FileOps` direct-user hook 不接收 raw `UserSpaceHandle` 或 user segments；
- `FileDescOps::read_user_transaction` 保留 fanotify transaction 边界，并通过 user-buffer adapter copyout；
- ramfs/ext4 regular file read direct-user path 通过 offset、EOF、bad first iovec、bad later iovec、cross-page fault 和 `FAN_ACCESS` 验证；
- write direct-user path 若进入实现，必须补齐 `O_APPEND`、`pwrite`、dirty/size/metadata、partial fault 和 `FAN_MODIFY` 验证；
- 非零 `pwritev2` flags、完整 `O_DIRECT`、zero-copy、page pin 和非 regular file fast path 仍按各自 limitation / follow-up 处理。
