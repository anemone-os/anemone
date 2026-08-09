# ANE-CHG-20260809-vfs-open-request

**Type:** Small refactor / kernel-internal open protocol
**Status:** Completed
**Date:** 2026-08-09
**Authors:** doruche, Codex
**Area:** VFS / openat / opened description / Pipe

## Problem / Context

VFS 已经需要在 ordinary inode open 之外处理 named FIFO activation，但相关 normalized access、nonblocking
snapshot、status admission 与 opened-description hook composition 直接散落在 `openat` syscall 实现中。syscall
因而按 inode kind 认识 Pipe owner 的输入与 description hook；继续增加 special file activation 会让 ABI adapter
逐步成为 backend dispatch 和 lifecycle composition owner。

本轮是行为保持的内部边界整理，不改变 Linux open flags、errno、FIFO admission/restart/cleanup、FAN_OPEN/fd
publication 顺序或 opened-description owner。

## Decision

新增 VFS-owned `FileOpenRequest`，仅承载一次 ordinary file activation 所需的 `FileOpenAccess` 三态与 normalized
status snapshot。它是 operation-local request，不保存 mutable status，也不是另一份 opened-description truth。
`O_PATH` 在 VFS handoff 边界先行形成 path capability，不构造 request，因此 backend 不可能把 path-only open
解释为 I/O participation。

`vfs_open_description` 负责 resolved path 的 kind dispatch、ordinary/FIFO activation 与创建时静态
`FileDescOps` composition。syscall 继续拥有 Linux ABI parse、pathname/final admission、fd reservation、FAN_OPEN
和 publication；resident inode kind 仍是 dispatch 的 authoritative fact；Pipe 只拥有 FIFO session admission、
participation、wait/restart rollback 与 endpoint lifecycle。

命名采用 `Request` 而不是 `Context`：该值没有跨调用生命周期、取消义务或资源所有权，只是一次 handoff 的
normalized immutable facts。没有引入动态 hook registry、通用 backend trait 或可扩展 open framework。

## Implementation Boundary

Target 是移除 `openat` 对 FIFO access/nonblock/status/description-hook 的 inode-kind 特判，并建立一个可审查的 VFS
activation seam。受保护边界包括现有 public API/ABI、visible errno/flags、`PIPE-FIFO-OPEN-001`、
`PIPE-FIFO-LIFECYCLE-001`、`OPENED-DESC-001..003`、FAN_OPEN/fd publication 顺序及所有 failure/cleanup 语义。

本轮不改变 pathname resolution、create/truncate、permission/mount admission、File/FileDesc owner、Pipe data plane、
readiness、anonymous pipe 或 filesystem backend open contract，也不把尚不存在的第二种 special activation 做成
推测性抽象。若实现要求迁移上述 owner、扩大 public surface、改变 ABI/contract/acceptance 或引入跨生命周期 open
state，则停止并升级为 RFC；本轮没有触发这些条件。`Contract Impact` 为 `None`。

## Change

- 新增 `fs/vfs/open.rs`，集中 resolved-path activation、inode-kind dispatch、candidate status seam 与静态
  description-hook composition。
- 新增 fs-owned `FileOpenAccess` 与 `FileOpenRequest`；`O_PATH` 在 request 构造前结束，Pipe 不再接收 task-owned
  `OpenAccessMode` 或保留 `(false, false)` 不可能态。
- `openat` 删除 FIFO access/nonblock/status 与 hook 特判，只向 VFS 交付 normalized access/status 并消费 activation
  result；Pipe 删除原有 `FifoOpenAccess` / `FifoOpenContext`。
- 增加一个 owner-local inline KUnit 场景，区分 inert `O_PATH`、无 reader 的 nonblocking writer `ENXIO` 与
  nonblocking reader activation，并核对 Pipe read transaction hook 和既有 final-release hook 同时保留。

## Validation

- 独立 reviewer 首轮发现一个 Euclid：request 仍保存 task-owned `OpenAccessMode::Path`，迫使 Pipe 以
  `unreachable!()` 排除不可能态。最终实现改为 fs-owned 三态、在 VFS 边界先处理 `O_PATH`；同一 reviewer
  只读复核确认该 finding 已关闭，且没有 Apollyon、Keter 或残余 Euclid。
- 首轮实现执行 canonical RV64 release build；sandbox 内 lwext4 以 `Bad system call` / SIGSYS 失败，相同命令在
  sandbox 外通过。另一次 KUnit-enabled RV64 build 通过；1 CPU、8 GiB QEMU 中 601/601 KUnit 通过，新增场景明确
  执行并通过。KUnit footer 后因运行盘缺少 `/.anemone/init` panic 并关机，因此不构成 userspace runtime 证据。
- reviewer finding 修正后按维护者要求不重跑 build 或 KUnit。最终 diff 只执行 `just fmt kernel`、源码/owner/
  lifecycle review、`git diff --check` 与 `mdbook build docs`；先前执行证据不冒充为最终修正的重跑证据。
- **Not Run:** 最终修正后的 kernel build/KUnit、LA64、SMP>1、LTP、userspace focused runtime、final harness 与
  hardware。

## Remaining Risk / Links

- 当前只有 ordinary inode 与 FIFO 两条 activation path；未来 special file 若需要新的 owner handoff，应在这个 seam
  上按真实 normalized facts 增量设计，不能把 raw Linux flags、task internals 或第二份 mutable status 下推给 backend。
- 本轮只调整 [Named FIFO 当前契约](../../contracts/pipe/named-fifo.md)的实现位置，不改变其 effective rules；
  [Opened-description lifecycle](../../contracts/task/opened-description-lifecycle.md)保持不变。
- Register：没有新增或关闭条目。RFC / transaction：None。
- Issue / PR / commit：this change's Git commit
