# ANE-CHG-20260615-backend-aware-fcntl

**Type:** Cleanup / VFS Boundary
**Status:** Completed
**Date:** 2026-06-15
**Authors:** doruche, Codex
**Area:** VFS / FileOps / fcntl / pipe / memfd preparation

## Problem

`fcntl(F_GETPIPE_SZ)` 和 `fcntl(F_SETPIPE_SZ)` 之前由通用 syscall adapter
处理，但它会直接调用 pipe 私有 helper。这让 generic `fcntl` 层知道了 pipe
后端的私有状态；后续如果接入 memfd seals 等 file-object command，也会重复同类
抽象越界。

另一方面，`fcntl` 不能整体下沉到后端。`F_DUPFD`、`F_GETFD`、`F_SETFD`、
`F_GETFL` 和 `F_SETFL` 分别属于 fd table、fd-local flags 或 opened file
description status state。如果让后端看到完整命令集，就会重新制造 status-ctx
清理想要避免的 owner-boundary 混乱。

## Scope

本轮只增加 backend-aware dispatch 形状，并用 pipe size 命令验证：

- 完整 `FcntlCmd` 保持为 syscall adapter 私有类型。
- 增加 VFS 可见的 `FileFcntlCmd` file-object 命令子集。
- 增加由 `FileDesc` 构造的窄 `FcntlCtx`，并在下发后端前拒绝 `O_PATH`。
- 增加使用 `Handled` / `Unhandled` 的可选 `FileOps::fcntl` hook。
- 将 pipe size 处理移回 pipe 后端。

本轮不实现 memfd sealing、POSIX/OFD locks、lease、owner signal 命令、
arg-fd lookup 或其它 `fcntl` 语义。

## Solution

本轮接受的边界是：

> syscall adapter 拥有完整 Linux `fcntl` command parsing 和 fd-owned dispatch；
> VFS 与 `FileOps` 只看见显式收窄后的 file-object command 子集。

`FcntlCtx` 是短生命周期值语义 snapshot。它只携带收窄后的命令、raw argument、
read/write capability bit，以及 normalized opened-description status flags。它不携带
`FileDesc`、fd number、fd-table lookup、current task、user pointer helper 或
`O_PATH` policy bit。`FileDesc::fcntl_ctx()` 在构造 ctx 前先把 `O_PATH`
file-object fcntl command 拒绝为 `EBADF`。

`FileOps::fcntl` 是可选 hook。后端返回 `Handled` 时结果即为最终结果；返回 `Err`
表示后端已经处理该命令并选择该失败；只有 `Unhandled` 才要求 VFS wrapper 执行命令族
默认 errno。这样后端可以实现自己的 file-object capability command，但普通后端不需要
填一个伪默认函数指针。

第一版 `FileFcntlCmd` 只包含 `GetPipeSize` 和 `SetPipeSize`。非 pipe 文件通过 VFS
默认路径返回 `EBADF`，符合 pipe-size command family 边界。memfd seals 继续作为后续
特性保留，本轮不半接线 syscall parse surface。

## Change

- `fs::file` 新增 `FileFcntlCmd`、`FcntlAccess`、`FcntlCtx`、
  `FileFcntlOutcome` 和可选 `FileOps::fcntl`。
- `File::fcntl()` 先分发到 backend hook；没有处理时，对已知 file-object fcntl command
  执行 VFS 默认 errno。
- `FileDesc::fcntl_ctx()` 构造短生命周期 snapshot，并在进入 VFS 前拒绝 `O_PATH`。
- `fs/api/fcntl.rs` 不再直接调用 pipe 私有 helper；pipe-size 命令会先转换为
  `FileFcntlCmd`，再通过 `File::fcntl()` 下发。
- pipe rx/tx `FileOps` 安装 `pipe_fcntl`，由 pipe 私有状态处理 pipe size，并保持既有
  stage-1 `F_SETPIPE_SZ` 行为。
- 其它 `FileOps` initializer 显式设置 `fcntl: None`。
- 新增 hook 注释记录 fd-owned 和 opened-description-owned command 必须留在后端
  dispatch 之外。

## Validation

- 主控代码审查未发现 owner boundary、`O_PATH` 拒绝、默认 errno policy 或 pipe hook
  handoff 相关的 Apollyon / Keter 问题。
- `rg` 确认 `anemone-kernel/src/fs/api/fcntl.rs` 不再引用 `crate::fs::pipe`、
  `fs::pipe`、`pipe::capacity` 或 `pipe::set_capacity`。
- `rg` 确认所有 static `FileOps` initializer 都有显式 `fcntl` 字段。
- `just fmt kernel` 通过。
- `git diff --check` 通过。
- 新增 public 记录通过 `git diff --no-index --check`。
- `mdbook build docs` 通过。
- `just build` 通过；build 只报告既有 `sync/mono.rs` unused-import warning。
- 本轮未运行 QEMU / LTP runtime。

## Tracking Issues

### CHG-001 - Memfd seals 仍是后续工作

**Status:** Deferred
**Severity:** Safe

**Issue:** 新的 backend fcntl 形状是为了后续支持 memfd sealing，但本轮没有定义
memfd file state、seal mutation 规则、write/mmap/truncate 交互，或 `F_ADD_SEALS` /
`F_GET_SEALS` syscall surface。

**Resolution:** 本轮不把 seals 接入 public parse surface。seals 需要单独小迭代；如果
mmap 与 write exclusion 规则变得不平凡，应按需要升级到更大的文档流程。

## Risk / Follow-up

- Pipe capacity 仍是既有固定单页 stage-1 模型；本轮清理不实现真实 pipe growth 或 pipe
  resource accounting。
- POSIX/OFD file locks 仍需要单独确认 owner 和 blocking semantics，之后才能进入
  `FileFcntlCmd`。
- 新增 `FcntlAccess` 可以服务后续 seals 等 file-object command，但不能默认扩展成
  fd-table lookup 或 current-task capability。

## Links

- Biweekly devlog: [2026-06-08 至 2026-06-21](../2026-06-08_to_2026-06-21.md)
- Related change: [FileOps status ctx 边界清理](./2026-06-10-fileops-status-ctx.md)
- Register / limitations: [当前限制：pipe procfs knobs stage-1](../../register/current-limitations.md#ane-20260528-pipe-procfs-knobs-stage1), [当前限制：open status flags stage-1](../../register/current-limitations.md#ane-20260528-open-status-flags-stage1)
