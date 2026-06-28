# ANE-CHG-20260613-vfs-stream-file-mode

**Type:** Cleanup / VFS Boundary Cleanup
**Status:** Completed
**Date:** 2026-06-13
**Authors:** doruche, Codex
**Area:** VFS / File / pipe / fanotify

## Problem

`File` 原先对所有 opened file 的普通 `read`、`write` 和 `read_exact` 都使用同一把
VFS-managed cursor lock。这个形状适合 regular file、directory 和需要 VFS cursor 的
pseudo file，但对匿名 pipe endpoint、fanotify group fd 这类 stream-like 对象会制造
人为的 cursor owner。后端已经拥有自己的 queue、blocking 和 wakeup 状态，普通 I/O
期间继续持有 `File::pos` 只会增加无关串行化，并让后续 stream fd 继承错误边界。

## Scope

本轮只改变显式标记为 stream 的文件在 VFS cursor-lock 上的策略，并标注现有 pipe
rx/tx 与 fanotify group fd 路径。regular file、directory、proc pseudo file、block
devfs、character devfs、console、`seek`、positioned I/O slot、`read_dir`、`poll`、
`ioctl` 和 `FileOps` vtable 形状均不在本轮改变范围内。

## Solution

引入内部 `FileMode` bitflag，并在 `File` 创建时固定。第一枚 flag `STREAM` 表示普通
`read` 和 `write` 不消费 VFS-managed cursor。`FileMode` 不是 Linux UAPI，不是 Linux
`fmode_t` 位值镜像，也不是可变 fd status；它描述的是 opened file object 与
`FileOps`、private state 一起确定下来的 VFS 行为形状。

默认构造路径保持 `FileMode::empty()`，因此既有 path-based open 继续使用 VFS cursor，
除非打开路径的 owner 显式 opt in。pipe endpoint 与 fanotify group fd 在 anonymous open
边界显式设置 `STREAM`。

## Change

- 新增 `FileMode` 与 `File` 上 immutable 的 `mode` 字段，并提供窄查询
  `mode()` / `is_stream()`。
- 新增 `OpenedFile::new()` 表达默认 empty mode，新增 `OpenedFile::with_mode()` 供
  显式 stream 构造使用。
- 将 `OpenedFile.mode` 贯穿到 `PathRef::open()` 与 anonymous open helper。
- 将匿名 pipe rx/tx 文件与 fanotify group fd 标记为 `FileMode::STREAM`。
- `File::{read_with_ctx, write_with_ctx, read_exact}` 在 `STREAM` 文件上使用局部 dummy
  cursor，不再锁定或更新 `File::pos`。
- `FileDesc::{write, write_at}` 在 `STREAM` 文件上绕过 generic append-to-EOF helper，
  但仍通过 `FileIoCtx` 把 `APPEND` status snapshot 传给后端。

## Validation

- `git check-ignore -v` 确认来源 qdev 草案仍位于被忽略的 `etc/` 私有工作材料范围。
- `git diff --check` 通过。
- `mdbook build docs` 通过。
- `just build` 通过；build 仅报告既有 `sync/mono.rs` unused-import warning。
- 本轮 agent 侧未运行 QEMU、LTP、pipe 或 fanotify runtime regression profile。

## Tracking Issues

### CHG-001 - runtime regression 覆盖

**Status:** Deferred
**Severity:** Safe

**Issue:** 本轮代码变化已经完成源码级审查与构建验证，但 agent 侧未运行 QEMU、LTP 或
定向 pipe/fanotify runtime profile。

**Resolution:** runtime 验证作为本小迭代的用户侧 follow-up 保留。本轮源码边界只覆盖
pipe/fanotify stream 标注与 VFS cursor-lock 分流。

## Risk / Follow-up

后续 eventfd、socket、FIFO 或 character-device stream 工作应在对应 owner 明确 cursor 与
readiness 边界之后再 opt in。本轮不会自动把所有 non-seekable file 标为 stream，因为
proc pseudo file 和其他特殊文件仍可能依赖 VFS cursor 行为。

## Links

- Biweekly devlog: [2026-06-08 至 2026-06-21](../2026-06-08_to_2026-06-21.md)
- Register / limitations: [当前限制：open status flags stage-1](../../register/current-limitations.md#ane-20260528-open-status-flags-stage1), [当前限制：pipe procfs knobs stage-1](../../register/current-limitations.md#ane-20260528-pipe-procfs-knobs-stage1)
