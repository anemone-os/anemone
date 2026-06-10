# ANE-CHG-20260610-fileops-status-ctx

**Type:** Design / VFS Boundary Cleanup
**Status:** Completed
**Date:** 2026-06-10
**Authors:** doruche, Codex
**Area:** VFS / FileOps / fcntl / openat / pipe / file status flags

## Problem

`F_SETFL` 当前暴露了 file status flags 的层次边界问题。`FileStatusFlags` 的
Linux-visible 真相源在 opened file description 层，`F_GETFL` 也从这里还原可见状态；
但 pipe read/write 行为又依赖 `PipeRx` / `PipeTx` 私有的 `nonblock` 字段。为了让
`fcntl(F_SETFL)` 修改 `O_NONBLOCK` 后 pipe 行为同步，generic fcntl 层直接调用
`fs::pipe::update_nonblock()`。

这让 fd 层和 pipe 后端各持有一份行为状态。当前入口点还能手动同步，但后续新建 fd、
dup/fork 共享语义、状态修改入口或后端重构都可能只更新其中一边；同时 generic syscall
层也被迫知道 pipe 私有实现细节，破坏 `FileOps` 后端边界。

同类问题还会影响 `O_DIRECT`、`O_SYNC`、`O_DSYNC` 和 `O_NOATIME`。这些 flag 由
opened file description 保存并通过 `F_GETFL` 可见，但真实 I/O 语义需要由 VFS 或具体
后端参与。如果没有统一的 per-operation status ctx，后续容易继续在 syscall 层或个别
helper 中堆特殊分支。

## Scope

本轮实现框架和 pipe 的直接边界修正：

- 让 opened file description 继续作为 file status flags 的唯一真相源。
- 为 data I/O 增加短生命周期 ctx，让 `FileOps::{read,write,read_at,write_at}` 能看到
  normalized status snapshot。
- 删除 pipe 私有 `nonblock` 行为状态和 `update_nonblock()` 同步路径。
- 让 pipe read/write 根据 ctx 中的 `NONBLOCK` 决定 `EAGAIN` 或等待。
- 将 block special file 对 `DIRECT` 的拒绝从 `openat` / `fcntl` 硬编码迁到后端
  status check。
- 审计非 `openat` 的 fd 创建入口，确认 status 初始化是否复用后端 check 或保留专属
  协议边界。

本轮不实现完整 Linux `O_DIRECT` I/O 对齐、cache 绕过或 block I/O direct path；不实现
pipe packet mode；不实现真实 `O_SYNC` / `O_DSYNC` 写回语义；不补齐完整 atime 更新或
`O_NOATIME` 抑制语义；不迁移 `F_GETPIPE_SZ` / `F_SETPIPE_SZ`；不引入 generic
`FileOps::fcntl`；不让后端持有 `FileDesc`、fd number、fd table 或 opened-description
lock。

## Solution

本小迭代接受的边界原则是：

> opened file description 仍然是 file status flags 的唯一真相源；后端只接收每次
> operation 的 normalized status snapshot，不缓存、不拥有、不反向驱动 fd 状态。

第一条 seam 是 `FileOps::check_status_flags(...)`。`openat` 和 `F_SETFL` 在写入 status
前构造候选状态，候选状态先交给后端做能力检查；检查成功后再一次性写回 opened file
description。check 必须无状态副作用，不能保存 status snapshot，也不能改变后续 I/O
行为。`F_SETFL` check 失败时 fd status 必须保持原值。

第二条 seam 是 data I/O ctx。`FileOps::{read,write,read_at,write_at}` 保留原名字，但
每个 data I/O vtable slot 增加一个 ctx 参数。`FileDesc` 从当前 opened-description
status snapshot 构造真实用户 fd ctx；direct `File` API 面向内核内部调用者保留现有
public wrapper，并使用默认 blocking ctx。

`O_APPEND` 的位置选择仍在 `FileDesc` / `File` 层完成，后端不重新解释 ordinary append
写入位置。但用户 fd 的 append 写路径不能回落到默认 ctx 的 direct append wrapper；最终
调用后端 `write` 时必须传入本次 `FileDesc` snapshot 生成的 ctx，避免绕过 `NONBLOCK`、
`DIRECT`、`SYNC` / `DSYNC` 等后端可观察状态。

## Change

本轮完成的代码变化：

- `IoctlFileStatusFlags` 收敛为共享的 `FileOpStatusFlags`，`FileIoCtx` 成为 data I/O
  vtable 的短生命周期 ctx，避免 ioctl 与 I/O status snapshot 分叉。
- `FileOps::{read,write,read_at,write_at}` 增加 ctx 参数；`FileDesc` 从当前
  opened-description status snapshot 构造用户 fd ctx，`O_APPEND` 的 `write` / `pwrite`
  分支也通过 ctx-aware append helper，不再回落到默认 blocking ctx。
- direct `File::{read,write,read_at,write_at,append,append_at_current_end}` 保留原签名，
  内部使用默认 blocking ctx，继续服务内核内部调用者。
- 新增 side-effect-free `FileOps::check_status_flags(...)`；`openat`、`F_SETFL`、
  `pipe2`、`fanotify_init` group fd、fanotify event fd 和 boot stdio 初始化都在提交
  status 前显式经过后端 check 或固定空状态协议。
- block devfs 后端负责拒绝 `DIRECT`，`openat` / `fcntl` 不再硬编码 block special file
  特判。
- pipe 删除私有 `nonblock` 行为状态和 `update_nonblock()`，read/write 只根据本次
  `FileIoCtx` 中的 `NONBLOCK` 决定 `EAGAIN` 或等待。
- fanotify group fd 的 compat 状态不再从 `event_f_flags` 派生；`event_f_flags` 仅作为
  fanotify event object fd 的模板状态。

## Validation

草案提升阶段完成的验证：

- 私有草稿仍由 `.gitignore:7:/etc` 覆盖，不作为公共稳定链接发布。
- `git diff --check -- etc/qdev/fileops-status-ctx-20260610.md` 通过。
- public docs 提升后，tracked docs 的 `git diff --check` 通过；新记录用
  `git diff --no-index --check -- /dev/null docs/src/devlog/changes/2026-06-10-fileops-status-ctx.md`
  单独检查，无 whitespace warning。
- public docs 提升后，`mdbook build docs` 通过。

实现后的验证结果：

- 构建与机械检查：`just fmt kernel`、`git diff --check`、`just build` 通过；`just build`
  仅剩既有 `anemone-kernel/src/sync/mono.rs` unused-import warning。
- `F_SETFL` 原子性：对 block special file 设置 `O_DIRECT` 仍返回 `EINVAL`；失败后
  `F_GETFL` 不显示新的 `O_DIRECT`；普通 accepted flag 成功后 `F_GETFL` 可见。本轮通过
  block devfs `check_block_status_flags` kunit 覆盖后端拒绝点；未运行用户态
  `F_GETFL/F_SETFL` runtime。
- pipe `O_NONBLOCK`：`pipe2(O_NONBLOCK)` 后空读返回 `EAGAIN`，满管道写返回 `EAGAIN`；
  `fcntl(F_SETFL)` 打开或清除 `O_NONBLOCK` 后，后续 pipe I/O 采用对应语义；dup 后共享
  opened-description status，任一 fd 上 `F_SETFL` 影响同一 pipe endpoint。本轮完成代码
  审查与构建验证，未运行 pipe runtime / LTP。
- regular file 兼容：ext4 / ramfs 上设置 `O_NONBLOCK` 后普通 read/write 行为保持当前
  可用；positioned I/O 和 `O_APPEND` 的 `write` / `pwrite` 分支仍走同一份 ctx。本轮通过
  代码审查确认 ctx 传递路径，未运行 ext4 / ramfs runtime。
- regression smoke：pipe 原有 `SIGPIPE` / `FIONREAD` / poll readiness 行为不因删除
  `nonblock` 字段回退；fanotify `read_user` 现有 status snapshot 语义不被新的 FileOps
  ctx 混淆；非 `openat` fd 创建入口的 status 初始化路径已经审计。QEMU / LTP 未在本轮
  agent 侧运行。

## Tracking Issues

### CHG-001 - append ctx 旁路

**Status:** Neutralized
**Severity:** Euclid

**Issue:** 现有 `FileDesc::write()` 的 `O_APPEND` 分支走 `File::append()`，如果 direct
append helper 保持默认 blocking ctx，而用户 fd 路径继续复用它，就会绕过本次 status
snapshot。

**Resolution:** 代码已让 `FileDesc::write()` / `FileDesc::write_at()` 先 snapshot
`FileStatusFlags`，再把同一 `FileIoCtx` 传给 ctx-aware append helper；direct append
wrapper 只保留给内核内部默认 blocking 调用。

### CHG-002 - I/O ctx 与 ioctl status snapshot 漂移

**Status:** Neutralized
**Severity:** Euclid

**Issue:** 新增 `FileIoStatusFlags` 而不复用现有 `IoctlFileStatusFlags` 会制造两套同构
映射，后续新增或收敛 flag 时容易漂移。

**Resolution:** 代码已删除独立 `IoctlFileStatusFlags`，`IoctlFileAccess` 和 `FileIoCtx`
共同使用 `FileOpStatusFlags`，并只从 `FileStatusFlags::to_file_op_status_flags()` 派生。

### CHG-003 - 非 openat fd 创建入口

**Status:** Neutralized
**Severity:** Euclid

**Issue:** 只把 `check_status_flags` 接入 `openat` / `F_SETFL`，可能让 `pipe2`、
`fanotify_init`、stdio 初始化等直接 fd 创建路径绕过后端能力检查。

**Resolution:** `pipe2`、fanotify group fd、fanotify event fd 和 boot stdio 初始化均已
显式 check 或固定空状态协议；fanotify event fd 复用目标文件后端 check。

## Risk / Follow-up

- 这不是完整 status flag 语义闭环。`DIRECT`、`SYNC`、`DSYNC`、`NOATIME` 的真实语义仍按
  current limitations 继续跟踪，不能因为框架迁移误写成已完成。
- 如果后续要统一 pipe-specific fcntl command、socket/fifo/eventfd readiness 模型、
  direct I/O、sync write、atime 或 packet-mode pipe，应新开小迭代或升级 RFC。
- `FileOps` data I/O vtable 是 shared-static-vtable 改动，实现时应预期一次机械
  initializer sweep，并保留构建级验证 gate。

## Links

- Biweekly devlog: [2026-06-08 至 2026-06-21](../2026-06-08_to_2026-06-21.md)
- Register / limitations: [当前限制：open status flags stage-1](../../register/current-limitations.md#ane-20260528-open-status-flags-stage1)
