# FileOps seek 与字符设备 ioctl Agent 编排建议

本文记录 `fileops-seek-char-ioctl` 进入实现阶段时的 agent 编排方式。Canonical
协议仍以 [RFC 入口](../index.md)、[不变量需求](../invariants.md)、
[迁移实施计划](../implementation.md) 和 [Tracking Issues](../tracking-issues.md)
为准；本文只说明如何按这些 gate 组织 worker、reviewer 和验收顺序。

本 RFC 尚未进入实现阶段。真正开始实现前，总控 agent 必须先建立事务 devlog，并在
RFC 入口和事务日志之间建立双向链接。

## 编排原则

1. 不按“一个文档阶段一个 agent”机械拆分。拆分边界应对应 shared `FileOps` vtable
   sweep、`lseek` intent 语义、positioned I/O 能力分类、loop backing handle 和
   `CharDev` hook 所有权边界。
2. 阶段 1A 是机械 API sweep 和 fail-closed 构建 gate，不是外部语义验收点。不要把
   1A 后的临时行为解释成兼容性完成。
3. 阶段 1B 的 `lseek` 迁移与阶段 1C 的 positioned I/O 分类必须分开 review。二者
   可以串行集成，但不能在同一个 worker diff 里互相掩盖错误。
4. loop backing handle 属于阶段 1C 的能力边界，不应由 loop worker 在后续阶段临时
   自行推断 `Arc<File>` 是否 positioned-capable。
5. 字符设备 seek 和 ioctl 都归 `CharDev` trait 所有。统一 char devfs file ops 只做
   `rdev` lookup、窄 ctx 构造和 trait hook 分发。
6. review agent 只放在有意义的 gate 上，不在每个机械 initializer 补字段后立即审查。
7. 写入型 worker 默认只改自己的 write set；若更合适的架构必须扩大范围，停止并向总控提交 write set 扩展申请，批准后再继续。
8. 每个实现阶段都要保持 `just build` 通过；阶段 1B、1C、2、3 还需要对应最小语义
   用例或手工证据。
9. LTP/QEMU 默认由用户验证，除非用户后续明确授权 agent 运行。
10. `SEEK_DATA` / `SEEK_HOLE` 第一阶段停在 syscall whence 转换层或等价前置 gate；
    任何 worker 都不得把它们交给 backend 自行猜测。

## 总控 Agent 使用方式

建议启动一个总控 agent 负责 orchestration，但不要让它自由决定新的协议拆分。
总控 agent 的权限边界是：

- 可以执行前置检查、代码搜索和构建级 gate。
- 可以启动只读 explorer / reviewer。
- 可以启动写入型 worker，但必须使用本文列出的 write set 和 worker 合同；需要扩大 write set 时，先记录原因、范围、contract/gate 影响和批准结果。
- 可以串行集成 worker diff。
- 可以在实现开始后建立并更新事务 devlog。
- 不运行 QEMU / LTP，除非用户后续明确要求；rv64 / LTP 日志默认由用户提供。
- 不 push、不 force-push、不 reset hard、不清理未归属改动。
- 遇到停止条件时回报用户，不自行拍板。

总控第一轮不要一次性派发所有 worker。建议流程是：

1. 重新确认当前分支、工作区状态、RFC 文档和是否已建立事务日志。
2. 派发 Agent 0 做当前 VFS / syscall / devfs / loop 前置审计。
3. 前置审计通过后派发 Agent 1，完成阶段 1A mechanical API sweep 和 fail-closed 默认实现。
4. 进行 Gate 1 review，确认 shared vtable 构建闭合且没有伪 positioned I/O wrapper。
5. 派发 Agent 2，完成阶段 1B `lseek` / seek intent 迁移。
6. 进行 Gate 2 review，确认 `SEEK_CUR`、`SEEK_END`、目录 rewind 和 `SEEK_DATA` /
   `SEEK_HOLE` unsupported 路径闭合。
7. 派发 Agent 3，完成阶段 1C positioned I/O 分类与 loop backing handle。
8. 进行 Gate 3 review，确认 fd/VFS/backend 三层职责和 loop narrowed handle 闭合。
9. 派发 Agent 4，完成阶段 2 `CharDev` seek policy 和 memory char seek。
10. 派发 Agent 5，完成阶段 3 字符设备 ioctl 默认分发。
11. 派发 Agent 6 做旁路审计、构建 gate、最小用例证据整理和事务日志收口。

可直接给总控 agent 的启动 prompt：

```text
工作目录是仓库根目录。请作为 fileops-seek-char-ioctl 的总控 agent，阅读
docs/src/rfcs/fileops-seek-char-ioctl/index.md、
docs/src/rfcs/fileops-seek-char-ioctl/invariants.md、
docs/src/rfcs/fileops-seek-char-ioctl/implementation.md、
docs/src/rfcs/fileops-seek-char-ioctl/tracking-issues.md、
docs/src/rfcs/fileops-seek-char-ioctl/backgrounds/agent-orchestration.md。

目标是按 RFC gate 实现 FileOps seek 与字符设备 ioctl：先完成 shared FileOps
mechanical API sweep 与 fail-closed 默认实现，再迁移 lseek seek intent，然后收紧
positioned I/O 和 loop backing handle，最后接通 CharDev seek 和 CharDev ioctl 默认
分发。

你可以启动子 agent，但必须按 agent-orchestration.md 的顺序、write set 和 review gate
分工；未经批准不允许 worker 越界修改。你不是独自在代码库里工作；不得 revert 用户或其他
agent 的改动。实现开始前需要建立并维护对应事务 devlog。

第一步只做前置检查、刷新当前代码落点和准备启动的 agent 列表。不要直接一次性启动
所有 worker。遇到停止条件时停止并向用户报告，不要自行拍板。
```

## Agent 0：前置审计

职责：只读审计当前代码落点是否仍符合 RFC 假设，不改代码。

读取范围：

- `anemone-kernel/src/fs/file.rs`
- `anemone-kernel/src/task/files.rs`
- `anemone-kernel/src/fs/api/lseek.rs`
- `anemone-kernel/src/fs/api/read_write/`
- `anemone-kernel/src/fs/ext4/file.rs`
- `anemone-kernel/src/fs/ramfs/file.rs`
- `anemone-kernel/src/fs/proc/`
- `anemone-kernel/src/fs/pipe.rs`
- `anemone-kernel/src/device/block/devfs.rs`
- `anemone-kernel/src/device/block/loop.rs`
- `anemone-kernel/src/device/char/`

检查项：

- `FileOps` 是否仍有 `validate_seek` 字段，以及所有 initializer 的分布。
- `File::read_at()` / `File::write_at()` 是否仍通过 `validate_seek + dummy cursor` 复用普通
  `read` / `write`。
- `FileDesc::write_at()` 是否仍拥有 fd access、path-only、`O_APPEND` 决策。
- `File::write_at()` 是否仍拥有 zero-length fast path、只读挂载检查和 metadata update。
- `sys_lseek()` 是否仍在 syscall 层解释 `SEEK_CUR` / `SEEK_END`，以及 `SEEK_END`
  是否仍使用全局 `inode().size()`。
- block devfs 是否仍以统一 `BLOCK_DEV_FILE_OPS` 处理所有块设备。
- loop 当前是否仍保存 `Arc<File>` backing，而不是 narrowed handle。
- char devfs 是否仍统一返回 seek unsupported 和 ioctl unsupported。

交付：

- 是否允许进入 Agent 1 的结论。
- 如果不允许，列出必须先修的 RFC blocker。
- 当前代码路径与阶段 1A / 1B / 1C / 2 / 3 的对应表。

停止条件：

- 当前分支已经引入与 RFC 不兼容的 `FileOps` seek 或 positioned I/O 抽象。
- loop backing 已经扩大到任意 VFS object，但没有 narrowed conversion contract。
- 字符设备 seek/ioctl 已经开始传递完整 `FileDesc`、task 或 fd table。

## Agent 1：阶段 1A Mechanical API Sweep

职责：完成阶段 1A，只做 shared vtable API sweep 和 fail-closed 默认实现。

write set：

- `anemone-kernel/src/fs/file.rs`
- 所有直接 `FileOps` initializer 文件
- 必要的 `fs::mod` re-export
- 实现开始后对应的事务 devlog

语义要求：

- 新增内部 `SeekFrom` 或等价 seek intent 类型，第一阶段只支持 `Set`、`Cur`、`End`
  进入 backend contract。
- 将 `FileOps::validate_seek` 替换为 `seek`、`read_at`、`write_at` 字段。
- 所有 initializer 显式补齐字段；不能依赖隐式 default。
- `File::read_at()` / `File::write_at()` 改为 VFS gate 后委托 `ops.read_at` /
  `ops.write_at`。
- 不能新增跨所有文件类型的通用 dummy-cursor positioned I/O wrapper。
- 如果为了构建闭合保留过渡 helper，必须命名清楚、可搜索，并标明 1C 删除点。
- 不迁移 `sys_lseek()` 外部语义，不改 loop backing handle，不实现 char device hook。

验证：

```bash
just build
git diff --check
```

Gate 1 reviewer 检查：

- `validate_seek:` vtable 字段和 `.validate_seek(` shared contract 已消失或仅剩明确迁移点。
- pipe、stream char device、path-only、symlink、目录写入等对象 fail closed。
- 所有 `read_at:` / `write_at:` initializer 都是显式分类或明确临时点。
- 阶段 1A 没有宣称 `lseek` / positioned I/O 用户可见语义完成。

## Agent 2：阶段 1B `lseek` 与 Seek Intent

职责：完成阶段 1B，迁移 `sys_lseek()` 和 opened file seek intent。

write set：

- `anemone-kernel/src/fs/api/lseek.rs`
- `anemone-kernel/src/fs/file.rs`
- seek helper 所在的具体 filesystem / device file ops 文件
- 必要的内部 `.seek(` 调用点
- 实现开始后对应的事务 devlog

语义要求：

- `sys_lseek()` 只做 fd lookup、path-only 检查、Linux whence 转换和错误映射。
- `SEEK_DATA` / `SEEK_HOLE` 在 whence 转换层或等价前置 gate 返回明确 unsupported / NYI。
- `File::seek` 接收 seek intent，持有 `File.pos` lock 后调用 `ops.seek`，返回用户可见新位置。
- `SEEK_CUR` 的读改写必须在 `File.pos` lock 内完成。
- regular file 使用 inode size 处理 `SEEK_END`。
- directory 最小支持 `SEEK_SET(0)` rewind，复杂目录 seek fail closed。
- pipe 返回 `IllegalSeek` / `ESPIPE`。
- block device 以设备总字节数处理 `SEEK_END`，保留 alignment 和 end boundary 检查。
- char device 本阶段仍可在 unified char devfs file ops fail closed，不得 devnum 特判 memory device。
- 内部 absolute-position 调用必须改为 `SeekFrom::Set(pos)` 或命名明确的
  `seek_set_checked` / `set_pos_checked` helper。

验证：

```bash
just build
```

最小证据：

- regular file `SEEK_SET` / `SEEK_CUR` / `SEEK_END`。
- pipe `lseek` 返回 `ESPIPE`。
- block device `lseek(end)` 保持成功。
- directory `lseek(fd, 0, SEEK_SET)` 能 rewind 后再次 `getdents64`。

Gate 2 reviewer 检查：

- `sys_lseek()` 不再直接用 `inode().size()` 解释所有 `SEEK_END`。
- `SEEK_CUR` 没有 syscall 层 `pos()` + `seek()` 非原子窗口。
- `SEEK_DATA` / `SEEK_HOLE` 没有进入 backend seek。
- 所有 `.seek(` 调用已分类为 syscall intent、内部 absolute set helper 或测试用例。

## Agent 3：阶段 1C Positioned I/O 与 Loop Backing Handle

职责：完成阶段 1C，收紧 `read_at` / `write_at` 能力边界，并引入 loop narrowed handle。

write set：

- `anemone-kernel/src/task/files.rs`
- `anemone-kernel/src/fs/file.rs`
- 各具体 filesystem / procfs / pipe / device file ops 的 `read_at` / `write_at` 实现
- `anemone-kernel/src/device/block/loop.rs`
- 必要的 loop backing handle helper 所在文件
- 实现开始后对应的事务 devlog

语义要求：

- `FileDesc::{read_at,write_at}` 只做 fd-local gate、path-only、status flags 和
  `O_APPEND` 决策。
- `File::{read_at,write_at}` 只做 zero-length fast path、regular content 只读挂载检查和
  successful write 后 metadata update。
- `FileOps::{read_at,write_at}` 只表达 backend offset-addressed I/O 能力。
- 删除阶段 1A 临时 compatibility wrapper，或把它们移动到具体 backend 内部并改名为局部 helper。
- regular file 允许 positioned read/write。
- block device 在自己的 positioned I/O 中保留 alignment、bounds 和 overflow 检查。
- pipe / stream char device positioned I/O 返回 `ESPIPE` 或当前等价错误。
- procfs snapshot 文件按现有读取模型分类，不扩大语义。
- 引入 `BackingFileHandle` 或等价窄 API，由 VFS/fd 边界构造。
- loop state 保存 narrowed handle，而不是任意 `Arc<File>` / `FileDesc` / `IoctlArgFile`。
- 第一阶段可以保守地只接受 regular file；未来扩大对象类型必须扩展 handle conversion contract。

验证：

```bash
just build
```

最小证据：

- pipe `pread` / `pwrite` 返回 `ESPIPE`。
- regular file positioned read/write 不改变 file cursor。
- `O_APPEND` + positioned write 仍走 fd 层定义的 append 行为。
- loop backing file setup 拒绝 path-only、不可读或非 accepted backing object。
- loop backing positioned read/write 路径仍构建通过。

Gate 3 reviewer 检查：

- fd/VFS/backend 三层职责没有被压到 backend。
- `File::read_at()` / `File::write_at()` 不再调用 seek/validate helper。
- loop backing state 不保存任意 `Arc<File>` 后在 I/O 路径才发现能力缺失。
- `BackingFileHandle` 对 loop 只暴露必要 positioned I/O 和显示/属性方法。

## Agent 4：阶段 2 `CharDev` Seek

职责：接通字符设备 seek policy，并修正 memory char seek。

write set：

- `anemone-kernel/src/device/char/mod.rs`
- `anemone-kernel/src/device/char/devfs.rs`
- `anemone-kernel/src/device/char/null.rs`
- `anemone-kernel/src/device/char/zero.rs`
- 可选 `/dev/full` 所在文件
- 必要的 `fs::file` seek ctx 类型使用点
- 实现开始后对应的事务 devlog

语义要求：

- 新增 `CharSeekCtx<'a>` 或等价窄参数。
- ctx 只包含 seek intent 和由当前 `File::seek` guard 派生的短生命周期 `&mut pos`
  或等价 cursor 能力。
- cursor 只能在当前 seek 调用内更新，不能保存到 `CharDev` 状态或转交异步路径。
- `CharDev::seek` 默认返回 `IllegalSeek`。
- `CHAR_DEV_FILE_OPS.seek` 只通过 `rdev` 查找 `CharDev` 并分发。
- `/dev/null`、`/dev/zero` 和可选 `/dev/full` 显式 override null-style seek：
  无论 offset / whence，成功后 position 为 `0`，返回 `0`。
- 不实现 tty/random/serial 完整 seek 语义；不确定时保持不可 seek 并记录限制。

验证：

```bash
just build
```

最小证据：

- `lseek(open("/dev/null"), 123, SEEK_SET) == 0`。
- `lseek(open("/dev/zero"), 123, SEEK_SET) == 0`。
- 默认 stream char device 的 `lseek` 返回 `ESPIPE`。

## Agent 5：阶段 3 `CharDev` ioctl

职责：接通字符设备 ioctl 默认分发，不实现具体 tty/random/serial ioctl 协议。

write set：

- `anemone-kernel/src/device/char/mod.rs`
- `anemone-kernel/src/device/char/devfs.rs`
- 必要的具体 char device 默认实现调整
- 实现开始后对应的事务 devlog

语义要求：

- 新增 `CharIoctlCtx<'a>`，作为 `IoctlCtx<'a>` 的透明窄包装或 type alias。
- ctx 不暴露 `FileDesc`、`ProcFile`、当前 task 或文件表。
- 复用现有 `IoctlCtx` 的 user pointer copy、access snapshot 和 fd-arg lookup 能力。
- `CharDev::ioctl` 默认返回 `UnsupportedIoctl`。
- `CHAR_DEV_FILE_OPS.ioctl` 通过 `rdev` 查找 `CharDev`，包装 ctx，再调用设备 hook。
- `/dev/null`、`/dev/zero`、`/dev/full`、`/dev/urandom` 不需要立即实现私有 ioctl。

验证：

```bash
just build
```

最小证据：

- 对 `/dev/null` 执行未知 ioctl 返回 `ENOTTY`。
- 对已有块设备 `BLKGETSIZE64` 仍成功。
- pipe `FIONREAD` 不退化。

## Agent 6：旁路审计与收口

职责：做最终审计、构建 gate、最小用例证据整理和事务日志收口。

读取范围：

- `anemone-kernel/src/fs/file.rs`
- `anemone-kernel/src/task/files.rs`
- `anemone-kernel/src/fs/api/lseek.rs`
- `anemone-kernel/src/fs/api/read_write/`
- `anemone-kernel/src/fs/pipe.rs`
- `anemone-kernel/src/device/block/devfs.rs`
- `anemone-kernel/src/device/block/loop.rs`
- `anemone-kernel/src/device/char/`

必须执行的搜索：

```bash
rg -n "validate_seek|FileOps \\{|sys_lseek|read_at\\(|write_at\\(|\\.seek\\(|CHAR_DEV_FILE_OPS|BLOCK_DEV_FILE_OPS|LOOP_SET_FD|BackingFileHandle|CharSeekCtx|CharIoctlCtx" anemone-kernel/src
```

验证：

```bash
just build
git diff --check
```

收口检查：

- `validate_seek` 不再作为 `FileOps` 字段或 shared seek / positioned I/O contract。
- `sys_lseek` 不直接使用 `inode().size()` 解释所有 `SEEK_END`。
- pipe 和 stream char device 没有通过普通 `read` / `write` 获得 positioned I/O。
- loop state 保存 narrowed handle。
- char devfs seek / ioctl 都分发到 `CharDev`。
- block devfs ioctl 和 loop hook 没有回退。
- 事务 devlog 区分 agent-run 验证、用户-run 验证、未运行验证和接受限制。

## 全局停止条件

出现以下情况时，总控应停止并回到 RFC review，不要让 worker 自行扩大范围；若确实需要扩大 write set，必须先按 RFC 工作流提交扩展申请并记录批准结果：

- 实现需要引入完整 Linux `FMODE_*` 能力模型。
- `read_at` / `write_at` 必须跨 VFS 建立新的全局能力状态。
- 必须恢复通用 dummy-cursor `read` / `write` wrapper 才能维持现有后端。
- loop backing handle 无法在 fd/VFS 边界验证能力，只能在 block I/O 时发现错误。
- 字符设备 seek 或 ioctl 需要传入 task/fd table 才能工作。
- 为了修 seek 需要改变 read/write position lock 的长期并发语义。
