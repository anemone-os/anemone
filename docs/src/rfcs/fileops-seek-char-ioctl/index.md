# RFC-20260605-fileops-seek-char-ioctl

**状态：** Closed
**负责人：** doruche, Codex
**最后更新：** 2026-06-05
**领域：** VFS / syscall ABI / devfs / character device
**事务日志：** [2026-06-05-fileops-seek-char-ioctl](../../devlog/transactions/2026-06-05-fileops-seek-char-ioctl.md)
**开放问题：** None；已处理的 review finding 见 [Tracking Issues](./tracking-issues.md)。
**下一步：** 本事务已按 [迁移实施计划](./implementation.md) 完成 Agent 6 收口；后续 `SEEK_DATA` / `SEEK_HOLE`、tty/random/serial 完整 ioctl、非 regular loop backing 等能力应作为 follow-up 处理。

## 摘要

本 RFC 草案定义一次小型 VFS/file-ops 整理：去除 `FileOps::validate_seek` 作为一等 file operation 的角色，引入真正的 `FileOps::seek` / `llseek` 语义入口，并把 `read_at` / `write_at` 作为后端可见的 positioned I/O file operation。同时，字符设备 ioctl 接到与块设备类似的子系统分发路径。

当前 `validate_seek(file, pos)` 同时服务 `lseek(2)`、`pread(2)` / `pwrite(2)` 和块设备 offset 校验。这个形状无法表达 Linux 风格的 per-file seek 语义：`lseek` 需要看到 `offset + whence`，并决定返回的新位置；`validate_seek` 只能检查已经算好的最终 `pos`。同时，当前 `File::read_at()` / `File::write_at()` 先调用 `validate_seek(pos)`，再用 dummy cursor 调用普通 `read` / `write`，使后端无法区分顺序 I/O 与 positioned I/O。结果是 stream file、字符设备和 loop backing file 的能力边界都被压扁在同一个 helper 里。

## 背景

当前实现中：

- `FileOps` 包含 `read`、`write`、`validate_seek`、`read_dir`、`poll`、`ioctl`。
- `sys_lseek()` 在 syscall 层解释 `SEEK_SET` / `SEEK_CUR` / `SEEK_END`，其中 `SEEK_END` 直接使用 `inode().size()` 作为基准。
- `File::seek(pos)` 调用 `validate_seek(pos)` 后统一写入 `File.pos`。
- `File::read_at()` / `File::write_at()` 也调用同一个 `validate_seek(pos)`，然后复用普通 `read` / `write`。
- 块设备通过 `block_validate_seek()` 限制 offset 的 block alignment 和设备边界。
- 字符设备 devfs 目前统一 `validate_seek` 为 `NotSupported`，`ioctl` 为 `UnsupportedIoctl`。

Linux 没有 `validate_seek` 这样的 file op。Linux 的分层是：

- `file_operations::llseek(file, offset, whence)` 表达 per-file seek 语义。
- `vfs_setpos()` 是 `llseek` 实现内部用于检查并更新最终位置的 helper。
- `FMODE_LSEEK`、`FMODE_PREAD`、`FMODE_PWRITE` 分别表达 `lseek` 和 positioned I/O 能力。
- 不可 seek 的 stream file 返回 `ESPIPE`；特殊设备可以提供 `noop_llseek` 或自定义 `llseek`。

本草案不要求完全复制 Linux 的 mode bit 体系，但应采用同样的语义拆分：`lseek` 与 positioned I/O 不再共享一个名为 `validate_seek` 的 file op。Anemone 在本轮直接把 positioned I/O 表达为 `FileOps::read_at` / `FileOps::write_at`，让后端显式知道当前请求不会消费或更新普通 file position。

## 目标

- 为 Anemone 打开文件对象提供真正的 seek file op，使具体文件类型能解释 `offset + whence`。
- 为 positioned I/O 提供真正的 `FileOps::read_at` / `FileOps::write_at`，使后端能显式接受、复用或拒绝 `pread` / `pwrite`。
- 从 `FileOps` 中删除或降级 `validate_seek`，避免它继续作为 `lseek` 抽象。
- 修正 `/dev/null`、`/dev/zero` 和可选 `/dev/full` 的 `lseek` 行为：seek 成功后位置为 `0`，返回 `0`。
- 让普通 stream char device 默认表现为不可 seek，`lseek` 返回 `ESPIPE` 风格错误。
- 保留块设备当前 alignment 和边界约束，但迁移到 `seek` 实现或块设备私有 helper 中。
- 将字符设备 ioctl 接入字符设备子系统，由 `CHAR_DEV_FILE_OPS.ioctl` 分发到 `CharDev`，默认返回 `ENOTTY` / `UnsupportedIoctl`。
- 保持已有块设备 ioctl 路径和 loop 设备私有 ioctl 边界不回退。

## 非目标

- 不在本草案中实现完整 `SEEK_DATA` / `SEEK_HOLE`；本轮只保留明确 unsupported / NYI 路径。
- 不引入完整 Linux `FMODE_*` bit 体系，除非实现时发现这是最小改动。
- 不重写顺序 `read` / `write` 的位置锁模型；本轮只让 `read_at` / `write_at` 绕过普通 file position 并由后端显式处理 positioned cursor。
- 不实现 tty、random、serial 等字符设备的完整 ioctl 协议。
- 不改变 loop 设备作为 block private ioctl hook 的边界。
- 不把 devfs 改造成每个设备节点各自保存完整 file-op vtable。

## 文档地图

Canonical：

- [不变量需求](./invariants.md)
- [迁移实施计划](./implementation.md)
- [Tracking Issues](./tracking-issues.md)

背景材料：

- [背景材料索引](./backgrounds/index.md)
- [Agent 编排建议](./backgrounds/agent-orchestration.md)

## 方案

`sys_lseek()` 不再在 syscall 层用 `inode().size()` 统一解释 `SEEK_END`。它只负责 fd 查找、`O_PATH` / bad fd 检查、Linux `whence` 参数转换，然后调用打开文件对象的 seek operation。

`FileOps` 新增 seek 与 positioned I/O 入口，建议形状为：

```rust
pub enum SeekFrom {
    Set(i64),
    Cur(i64),
    End(i64),
    Data(i64),
    Hole(i64),
}

pub struct FileOps {
    pub read: fn(&File, pos: &mut usize, buf: &mut [u8]) -> Result<usize, SysError>,
    pub write: fn(&File, pos: &mut usize, buf: &[u8]) -> Result<usize, SysError>,
    pub read_at: fn(&File, pos: usize, buf: &mut [u8]) -> Result<usize, SysError>,
    pub write_at: fn(&File, pos: usize, buf: &[u8]) -> Result<usize, SysError>,
    pub seek: fn(&File, pos: &mut usize, from: SeekFrom) -> Result<usize, SysError>,
    ...
}
```

具体命名和 signed/unsigned 表达可在实现时按现有 syscall handler 风格调整，但 contract 应固定。完整不变量见 [不变量需求](./invariants.md)，这里列出主线结论：

- `seek` 接收原始 seek intent，而不是已经算好的最终 `pos`。
- `seek` 在持有 file position guard 时完成 `SEEK_CUR` 读改写，避免当前 `pos()` + `seek()` 的明显非原子窗口。
- `seek` 返回对用户可见的新位置。
- `seek` 内部可以复用普通 helper 来做最大文件大小、负 offset、块对齐和边界检查。
- `SEEK_DATA` / `SEEK_HOLE` 第一阶段不进入 backend seek contract；syscall whence 转换层或等价前置 gate 必须返回明确 unsupported / NYI。只有后续 RFC 决定实现完整语义时，才把 `Data` / `Hole` intent 交给具体 backend。
- `read_at` / `write_at` 接收 positioned cursor，不读取、不更新 `File.pos`。
- fd/VFS/backend 三层职责不能被压到 backend：
  - `FileDesc::{read_at,write_at}` 保留 fd access、path-only、status flags 和 `O_APPEND` 等 fd-local 语义；
  - `File::{read_at,write_at}` 保留 zero-length fast path、regular content 只读 mount 检查和 successful write 后的 inode metadata update；
  - `FileOps::{read_at,write_at}` 只表达具体 backend 是否支持 offset-addressed I/O，以及该 backend 的 bounds、alignment、overflow 和局部 cursor 规则。
- 后端必须显式选择 positioned I/O 语义：普通文件、procfs snapshot 和其他 offset-addressable 后端可以在自己的实现中通过局部 cursor 复用 `read` / `write`；pipe 和 stream char device 默认返回 `IllegalSeek` / `ESPIPE`；块设备在自己的 `read_at` / `write_at` 中保留 alignment、bounds 和 overflow 检查。
- loop backing file 不能通过“`FileOps` 存在 `read_at` 字段”推断能力。`LOOP_SET_FD` 必须经 VFS/fd 边界产出 `BackingFileHandle` 或等价窄 capability，统一验证 path-only、read/write access、readonly、regular-file 或明确 positioned-capable backend；loop state 只保存 narrowed handle，不保存任意 `Arc<File>` / `FileDesc` 后在 block I/O 时再发现错误。

原 `validate_seek` 不再作为 `FileOps` 字段存在。它不能被替换成“先校验 offset，再调用普通 `read` / `write`”的通用 wrapper；这样的 wrapper 会重新隐藏后端是否支持 positioned I/O。允许保留的 helper 只能是具体后端内部的局部检查函数，例如 regular file size helper、block device alignment helper 或 procfs snapshot bounds helper。

目录 seek 在本轮采用最小支持：`lseek(dirfd, 0, SEEK_SET)` 重置目录枚举 cursor 并返回 `0`，用于支持 `getdents64` rewind；更复杂的目录 offset cookie、`SEEK_CUR` / `SEEK_END` / 非零 `SEEK_SET` 语义不在本轮扩展。

字符设备 seek 与 ioctl 都归 `CharDev` 所有，而不是由统一 devfs file ops 按 devnum 硬编码：

- `CharDev` 增加默认 `seek(&self, ctx: CharSeekCtx<'_>) -> Result<usize, SysError>` 或等价窄 hook，默认返回 `IllegalSeek`。
- `CharSeekCtx` 第一阶段只携带 seek intent 和由 `File::seek` position guard 派生的短生命周期 `&mut pos` 能力。该 cursor 只能在当前 seek 调用内更新，不得被保存，也不得默认暴露完整 `File`、`FileDesc`、task 或 fd table；后续若具体设备确实需要更多状态，应逐项扩展窄 ctx。
- `/dev/null`、`/dev/zero` 和可选 `/dev/full` 显式 override null-style seek：不论 offset / whence，成功后 position 为 `0` 并返回 `0`。
- `CHAR_DEV_FILE_OPS.seek` 只负责通过 `rdev` 查找 `CharDev` 并分发。

字符设备 ioctl 采用块设备已经形成的分层：

- `CharDev` 增加默认 `ioctl(&self, ctx: CharIoctlCtx<'_>) -> Result<u64, SysError>`，默认返回 `UnsupportedIoctl`。
- `CharIoctlCtx` 是 `IoctlCtx` 的窄包装或 type alias，归字符设备子系统所有。
- `CHAR_DEV_FILE_OPS.ioctl` 根据 `rdev` 查找 `CharDev`，然后调用设备的 ioctl hook。
- `/dev/null`、`/dev/zero`、`/dev/full` 第一阶段不必支持私有 ioctl，默认 `ENOTTY` 即可。

## 接受边界

本 RFC 已提升为公开目录级 RFC 草案，是后续 `FileOps` seek、positioned I/O 和字符设备 ioctl 设计 review 的 canonical 文档入口。进入实现阶段前必须创建事务级 devlog，并把事务日志与本 RFC 双向链接。

接受本草案意味着可以对 `FileOps` 做一次 staged shared vtable 迁移：先机械引入 seek、read_at、write_at 并 fail closed，再迁移 `lseek` 语义，再按 fd/VFS/backend contract 收紧 positioned I/O，最后接通字符设备 seek / ioctl 默认 hook。迁移必须遵守 [不变量需求](./invariants.md)。阶段 1A 只是构建闭合的机械 gate，不是外部语义验收点；`lseek` 与 positioned I/O 的用户可见闭合分别发生在阶段 1B 和阶段 1C。

以下变化必须回到本 RFC 或新增 follow-up RFC：

- 引入 Linux `FMODE_*` 的完整内部模型。
- 改变 `File::read` / `File::write` 的 file position 锁和并发语义。
- 重新引入通用 positioned I/O wrapper，使 stream 后端在不知情时继承 `pread` / `pwrite` 能力。
- 改变块设备 ioctl 或 loop 设备 private ioctl 的所有权边界。
- 让 loop 保存任意 `File` 并自行推断 positioned I/O capability，而不是通过 narrowed backing handle。
- 把完整 `File` / `FileDesc` 作为 `CharDev` 默认 seek/ioctl hook 参数。
- 把完整 tty/random/serial ioctl 协议纳入本阶段验收。
- 实现完整 `SEEK_DATA` / `SEEK_HOLE`。

## 备选方案

### 继续保留 `validate_seek`，只给字符设备改返回值

拒绝。这样只能让某些 `lseek(fd, x, SEEK_SET)` 表面成功，仍无法表达 `/dev/null` / `/dev/zero` 应返回 `0`、`SEEK_END` 基准由文件类型决定、`SEEK_CUR` 应在 position 锁内完成等语义。

### 把 `validate_seek` 改名为 `seek`

拒绝。只改名但参数仍是最终 `pos`，不能解决 `whence`、返回值和特殊设备 seek 行为的问题。

### 让 `File::read_at()` / `File::write_at()` 继续通用复用 `read` / `write`

拒绝。这个方案会让后端无法区分顺序 I/O 和 positioned I/O，pipe 与 stream char device 可能在 `pread` / `pwrite` 路径上阻塞或消费状态。允许具体后端在自己的 `read_at` / `write_at` 实现中复用普通 `read` / `write`，但这个选择必须发生在后端内，而不是 VFS 通用 wrapper。

### 在 `sys_lseek()` 中继续按 inode 类型特判

拒绝。`lseek` 是打开文件行为，不是 syscall 层根据 inode 类型拼分支。块设备、字符设备、procfs、regular file 都应通过 file ops 表达差异。

### 只实现字符设备 ioctl，不处理 seek

延期价值不足。字符设备 ioctl 与 seek 问题都落在统一 char devfs file ops 和 `FileOps` shared contract 上，合并成一次小型 vtable sweep 更清晰。

## 风险

- `FileOps` 是 shared static vtable，字段改动会触发 repo 范围 initializer 扫描。控制方式是把阶段 1 拆成 1A mechanical API sweep、1B `lseek` 语义迁移、1C positioned I/O 分类，每个 gate 单独 `just build`。
- `read_at` / `write_at` 作为新 vtable 字段会扩大 mechanical initializer sweep。控制方式是阶段 1 统一补齐默认实现，阶段 2 逐类审计 positioned I/O 能力：普通文件、块设备、pipe、目录、procfs、字符设备。
- 字符设备默认 seek 如果过宽，会让 stream 设备出现假 seek 能力。控制方式是默认不可 seek，只有 memory char device 显式使用 null-style seek。
- ioctl 默认 errno 必须与现有 `UnsupportedIoctl -> ENOTTY` 映射保持一致，避免回退成 `ENOSYS` 或 `EOPNOTSUPP`。
- loop backing 的 narrowed handle 如果设计得过宽，会重建一套隐式 capability bit。控制方式是第一阶段保持 regular-file 或显式 positioned-capable 后端的窄验收，并把未来放宽留在 handle conversion contract。

## 收口

完成后需要记录：

- `FileOps::validate_seek` 不再作为 vtable 字段存在，或已降级为非 seek helper。
- `FileOps::read_at` / `FileOps::write_at` 成为 positioned I/O 的唯一通用入口，pipe 和 stream char device 不会通过普通 `read` / `write` 偶然支持 `pread` / `pwrite`。
- `sys_lseek()` 不再自行使用 `inode().size()` 解释所有 `SEEK_END`。
- `/dev/null`、`/dev/zero` 的 lseek 最小行为已在字符设备 hook 中接通；运行态覆盖情况见事务日志。
- 目录 fd 支持 `lseek(fd, 0, SEEK_SET)` rewind，复杂目录 offset 语义仍作为 follow-up 限制。
- 块设备 `lseek(end)` 和 alignment 行为保持。
- 未知字符设备 ioctl 经字符设备子系统默认 hook 返回 `ENOTTY`。
- `just build` 通过。

最终执行事实、旁路审计、agent-run validation、未运行的 QEMU / LTP 和接受限制见
[事务日志 Agent 6 收口记录](../../devlog/transactions/2026-06-05-fileops-seek-char-ioctl.md)。
