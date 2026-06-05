# FileOps seek 与字符设备 ioctl 迁移实施计划

**状态：** Draft
**最后更新：** 2026-06-05
**父 RFC：** [RFC-20260605-fileops-seek-char-ioctl](./index.md)
**不变量：** [不变量需求](./invariants.md)

本文按小 gate 推进。每个阶段都应保持可构建，并避免同时改动无关 VFS 或设备语义。

## 迁移原则

- `lseek` 语义只通过 opened file 的 seek operation 表达；syscall 层不按 inode 类型决定 `SEEK_END`、特殊设备或块设备边界。
- `SEEK_DATA` / `SEEK_HOLE` 第一阶段停在 syscall whence 转换层或等价前置 gate，返回明确 unsupported / NYI；本轮不把它们交给 backend seek。
- `pread` / `pwrite` 语义通过 fd/VFS/backend 三层 contract 表达：fd 层处理 descriptor-local policy，`File` 层处理 VFS-wide gate，`FileOps` 层处理 backend positioned I/O capability。
- `validate_seek(pos)` 不再作为 `FileOps` 字段存在；如果实现仍需要 offset helper，它只能是具体文件类型内部检查，不能重新成为跨 `lseek` 和 positioned I/O 的共享抽象。
- loop backing file 必须由 VFS/fd 边界产出 narrowed backing handle；loop 子系统不保存任意 `FileDesc` / `IoctlArgFile`，也不直接推断任意 backend 的 positioned I/O 能力。
- 字符设备默认不可 seek；只有明确有 Linux 兼容需求的设备通过 `CharDev` hook 显式选择 seek policy。
- 字符设备 ioctl 走字符设备子系统，和块设备 ioctl 一样由统一 devfs file ops 分发到设备对象。
- 本计划只建立基础接口和 memory char device 最小行为，不扩大到 tty/random/serial 完整协议。

## 阶段 0：实施前审计

前置条件：

- 当前分支已经包含 `FileOps::ioctl`、`IoctlCtx`、块设备 ioctl 和 loop private ioctl 基础。

交付：

- 用搜索确认现有写集和分层位置：

```sh
rg -n "validate_seek|FileOps \\{|sys_lseek|read_at\\(|write_at\\(|\\.seek\\(|CHAR_DEV_FILE_OPS|BLOCK_DEV_FILE_OPS|LOOP_SET_FD|IoctlArgFile|IoctlFileAccess" anemone-kernel/src anemone-abi/src
```

- 记录当前 `FileDesc::{read_at,write_at}`、`File::{read_at,write_at}`、loop `set_fd` 和 char devfs file ops 的实际形状，作为 1A/1B/1C review 对照。

验证：

- docs-only 阶段不运行 QEMU / LTP。
- 开始代码实现前至少运行 `just build`，确认迁移不是从已坏基线开始。

退出条件：

- 写集清楚，且没有发现需要先回到 RFC 的额外 shared-contract 问题。

## 阶段 1A：机械 API sweep 与 fail-closed 默认实现

前置条件：

- 阶段 0 审计完成。

交付：

- 在 `fs::file` 中新增内部 `SeekFrom` 或等价枚举，表达 `SEEK_SET`、`SEEK_CUR`、`SEEK_END`，并为未来 `SEEK_DATA` / `SEEK_HOLE` 保留明确 unsupported 路径。
- 将 `FileOps` 的 `validate_seek` 字段替换为 `seek`、`read_at`、`write_at` 字段。
- 为所有 `FileOps` initializer 补齐字段。不能留隐式 default，也不能用跨所有文件类型的通用 dummy-cursor positioned I/O wrapper。
- `File::read_at` / `File::write_at` 的外形改为经过 VFS gate 后委托 `ops.read_at` / `ops.write_at`。如果为了 1A 构建闭合保留临时 compatibility helper，它必须是命名明确、可搜索、只在允许的 backend initializer 内使用的局部 helper，并在 1C 删除或分类。
- 保留或新增命名明确的内部 absolute-position helper，例如 `seek_set_checked` / `set_pos_checked`，只用于把旧 `File::seek(pos)` 调用迁移到新 API 之前的机械过渡。不得保留模糊的旧 `File::seek(pos)` 作为长期 wrapper。
- pipe、path-only、symlink、目录写入等不支持 positioned I/O 的对象在新字段中 fail closed，返回现有稳定错误或 `IllegalSeek` / `ESPIPE` 风格错误。

审计：

- 搜索所有 `validate_seek:` initializer，确保没有残留 vtable 字段。
- 搜索所有 `.validate_seek(` 调用，确认若存在只是在具体 backend 内部 helper 或 1A 明确标记的迁移点。
- 搜索所有 `read_at:` / `write_at:` initializer，确认 pipe 和 stream char device 没有指向普通 `read` / `write` wrapper。

验证：

- `just build`
- `git diff --check`

退出条件：

- repo 范围 static vtable 构建闭合。
- 任何临时 helper 都有可搜索名称和 1C 删除点。
- 本阶段不要求完成 `sys_lseek` 语义迁移，也不要求完成 loop backing handle 改造。
- 本阶段不作为外部语义验收点：`lseek` 用户可见语义仍由阶段 1B 闭合，positioned I/O 用户可见语义仍由阶段 1C 闭合。1A 后的行为必须 fail closed，不能把临时构建闭合解释成兼容性完成。

## 阶段 1B：迁移 `lseek` 与 seek intent

前置条件：

- 阶段 1A 通过 `just build`。

交付：

- `sys_lseek()` 只做 fd lookup、path-only 检查、Linux `whence` 参数转换和错误映射，然后调用 opened file seek operation。
- `SEEK_DATA` / `SEEK_HOLE` 继续在 whence 转换层或等价前置 gate 返回明确 unsupported / NYI，不进入 backend `seek`。
- `sys_lseek()` 不再直接用 `inode().size()` 解释所有 `SEEK_END`。
- `File::seek` 接收 seek intent，持有 `File.pos` lock 后调用 `ops.seek`，并返回用户可见的新位置。
- 为现有 file ops 补齐 seek 实现：
  - regular file：generic seek，`SEEK_END` 使用 inode size，负结果返回 `EINVAL`；
  - directory：最小支持 `SEEK_SET(0)` rewind 并返回 `0`，复杂目录 offset cookie、`SEEK_CUR` / `SEEK_END` / 非零 `SEEK_SET` 继续作为 follow-up；
  - pipe：返回 `IllegalSeek` / `ESPIPE`；
  - procfs fixed snapshot 类文件：使用现有 bounds 逻辑迁移成 `generic_seek_with_size` 或局部 helper；
  - block device：使用 fixed-size seek，`SEEK_END` 以设备总字节数为基准，并保留 block alignment / end boundary 检查；
  - char device：本阶段可以先在 unified char devfs file ops fail closed；阶段 2 再分发到 `CharDev`。
- 内部绝对定位调用必须分类：调用方改为 `SeekFrom::Set(pos)`，或使用命名明确的 `seek_set_checked` / `set_pos_checked` helper；不能依赖旧 `File::seek(pos)` wrapper。

审计：

- 搜索 `sys_lseek`，确认没有 inode-size based global `SEEK_END`。
- 搜索所有 `.seek(` 调用，确认 syscall lseek、内部 absolute set 和测试 helper 已按新 API 分类。
- 搜索目录 file ops，确认 `lseek(dirfd, 0, SEEK_SET)` rewind 的最小语义存在，复杂目录 seek 没有被伪装支持。

验证：

- `just build`
- 最小内核单测或用户态用例覆盖：
  - regular file `SEEK_SET` / `SEEK_CUR` / `SEEK_END`；
  - pipe `lseek` 返回 `ESPIPE`；
  - block device `lseek(end)` 保持成功；
  - directory `lseek(fd, 0, SEEK_SET)` 能 rewind 后再次 `getdents64`。

退出条件：

- `lseek` 的 seek intent contract 已闭合。
- `SEEK_CUR` 的读改写在 `File.pos` lock 内完成。
- 字符设备仍可 fail closed，但不能用 devnum 特判 memory device seek。

## 阶段 1C：收紧 positioned I/O 与 loop backing handle

前置条件：

- 阶段 1A 已有 `FileOps::{read_at,write_at}`。
- 阶段 1B 已完成 seek intent 迁移。

交付：

- 固定 fd/VFS/backend 三层 contract：
  - `FileDesc::{read_at,write_at}` 保留 fd access、path-only、status flags 和 `O_APPEND` 决策；
  - `File::{read_at,write_at}` 保留 zero-length fast path、regular content 只读 mount 检查和 successful write 后的 inode metadata update；
  - `FileOps::{read_at,write_at}` 只表达 backend offset-addressed I/O 能力。
- 删除阶段 1A 可能留下的临时 compatibility wrapper，或把它们移动到具体 backend 内部并改名为局部 helper。
- 对各类文件做显式分类：
  - regular file：允许 positioned read/write，沿用文件大小和扩展写入规则；
  - block device：read/write 内部继续检查 block alignment、bounds、overflow；
  - pipe / stream char device：positioned I/O 返回 `ESPIPE` 或当前等价错误，不能伪装支持；
  - procfs snapshot 文件：按现有读取模型决定是否允许 positioned read；不在本阶段扩大语义；
  - directory、symlink/path-only：按当前 read/write 禁止边界返回对应错误。
- loop backing file：
  - 引入 `BackingFileHandle` 或等价窄 API，由 VFS/fd 边界构造；
  - conversion 阶段验证 path-only、read access、write access / readonly、regular-file 或明确 positioned-capable backend；
  - loop state 保存 narrowed handle，而不是任意 `Arc<File>` / `FileDesc` / `IoctlArgFile`；
  - loop block I/O 通过 handle 暴露的 `read_exact_at` / `write_all_at` 等方法访问 backing storage；
  - 第一阶段可以保守地只接受 regular file；未来扩大对象类型必须扩展 handle conversion contract。

审计：

- 检查 `FileDesc::read_at` / `FileDesc::write_at`，确认它们只做 fd-local gate 和 append policy，不重复 VFS metadata 或 backend bounds。
- 检查 `File::read_at` / `File::write_at`，确认它们只做 VFS gate 后委托 `ops.read_at` / `ops.write_at`，不再调用 seek/validate helper。
- 搜索 `read_at(` / `write_at(` 的用户，确认 loop backing file 只绑定 narrowed handle。
- 搜索所有 `read_at:` / `write_at:` initializer，确认没有用通用 stream wrapper 给 pipe 或 char stream 误开 positioned I/O。

验证：

- `just build`
- 最小用例：
  - pipe `pread` / `pwrite` 返回 `ESPIPE`；
  - regular file positioned read/write 不改变 file cursor；
  - `O_APPEND` + positioned write 仍走 fd 层定义的 append 行为；
  - loop backing file setup 拒绝 path-only、不可读或非 accepted backing object；
  - loop backing positioned read/write 路径仍构建通过。

退出条件：

- `FileOps::read_at` / `FileOps::write_at` initializer 全部显式分类完成。
- `validate_seek` 不再表达任何 `lseek` 或 positioned I/O shared contract。
- positioned I/O 的支持边界在代码中可从具体 `read_at` / `write_at` 实现或 backing handle conversion 看出。

## 阶段 2：接通 `CharDev` seek policy 并修正 memory char seek

前置条件：

- 阶段 1B 已经提供真正的 seek intent API。

交付：

- 为字符设备子系统新增 `CharSeekCtx<'a>` 或等价窄参数：
  - 包含 seek intent 和由当前 `File::seek` position guard 派生的短生命周期 `&mut pos` 或等价 cursor 能力；
  - 该 cursor 只能在当前 seek 调用内更新，不能保存到 `CharDev` 状态或转交异步路径；
  - 不暴露完整 `File`、`FileDesc`、task 或 fd table；
  - 后续若具体设备需要 fd/file 状态，必须通过明确字段扩展窄 ctx。
- 为 `CharDev` 增加默认 seek hook，默认返回 `IllegalSeek`。
- `CHAR_DEV_FILE_OPS.seek` 只负责通过 `rdev` 查找 `CharDev` 并分发，不能按 devnum 硬编码 memory device 特例。
- `/dev/null`、`/dev/zero` 和可选 `/dev/full` 在对应 `CharDev` 实现中显式使用 null-style seek：无论 offset / whence，成功后 file position 为 `0`，返回 `0`。
- `/dev/urandom` 第一阶段可使用 noop 或不可 seek，但必须按 Linux 兼容目标明确选择；若不确定，先保持不可 seek 并记录限制。

审计：

- 搜索 `CHAR_DEV_FILE_OPS.seek` 和 memory char device 实现，确认 seek policy 归 `CharDev`。
- 搜索 `CharSeekCtx`，确认没有默认携带完整 VFS `File` 或 fd table 能力，也没有把 position cursor 保存到设备状态。
- 搜索 `IllegalSeek` / `NotSupported` 映射，确认默认 stream char device 对用户态呈现稳定 `ESPIPE` 风格 errno。

验证：

- `just build`
- 最小用例：
  - `lseek(open("/dev/null"), 123, SEEK_SET) == 0`；
  - `lseek(open("/dev/zero"), 123, SEEK_SET) == 0`；
  - 对默认 stream char device 的 `lseek` 返回 `ESPIPE`。

退出条件：

- memory char device seek 行为不再统一返回 unsupported。
- char devfs seek 分发不包含 devnum-specific policy。

## 阶段 3：接通字符设备 ioctl 默认分发

前置条件：

- `FileOps::ioctl` 已存在。
- `CHAR_DEV_FILE_OPS` 当前仍统一返回 `UnsupportedIoctl`。

交付：

- 为字符设备子系统新增 `CharIoctlCtx<'a>`：
  - 可以是 `IoctlCtx<'a>` 的透明窄包装；
  - 不暴露 `FileDesc`、`ProcFile`、当前 task 或文件表；
  - 复用现有 `IoctlCtx` 的 user pointer copy、access snapshot 和 fd-arg lookup 能力。
- 为 `CharDev` trait 增加默认方法：

```rust
fn ioctl(&self, _ctx: CharIoctlCtx<'_>) -> Result<u64, SysError> {
    Err(SysError::UnsupportedIoctl)
}
```

- `CHAR_DEV_FILE_OPS.ioctl`：
  - 通过 `rdev` 查找 `CharDev`；
  - 将 `IoctlCtx` 包装为 `CharIoctlCtx`；
  - 调用设备的 ioctl hook。
- `/dev/null`、`/dev/zero`、`/dev/full`、`/dev/urandom` 不需要立即实现私有 ioctl；默认 `UnsupportedIoctl` 即可。
- 若 serial driver 后续需要 tty ioctl，作为 follow-up 在具体设备实现，不在本阶段扩大。

审计：

- 确认 `sys_ioctl()` 不为字符设备增加新的全局特判。
- 确认 `CHAR_DEV_FILE_OPS.ioctl` 与 `BLOCK_DEV_FILE_OPS.ioctl` 的边界相似：devfs 只存发布记录，子系统 file ops 负责分发。
- 确认 unsupported errno 仍映射为 Linux ioctl 期望的 `ENOTTY`。

验证：

- `just build`
- 最小用例：
  - 对 `/dev/null` 执行未知 ioctl 返回 `ENOTTY`；
  - 对已有块设备 `BLKGETSIZE64` 仍成功；
  - pipe `FIONREAD` 不退化。

退出条件：

- 字符设备 ioctl 不再在 `CHAR_DEV_FILE_OPS` 直接固定返回 unsupported，而是经过 `CharDev` 默认 hook。
- 当前没有具体 char device 因 trait 新增方法需要重复样板实现。

## 旁路审计清单

实现完成前至少执行：

```sh
rg -n "validate_seek|FileOps \\{|sys_lseek|read_at\\(|write_at\\(|\\.seek\\(|CHAR_DEV_FILE_OPS|BLOCK_DEV_FILE_OPS|LOOP_SET_FD|BackingFileHandle|CharSeekCtx|CharIoctlCtx" anemone-kernel/src
```

分类要求：

- `validate_seek` 若仍出现，只能是历史注释或具体 backend 内部 helper；不能是 `FileOps` 字段，不能承担 shared seek contract。
- `sys_lseek` 不能直接使用 `inode().size()` 计算所有 `SEEK_END`。
- 所有 `.seek(` 调用必须已经分类为 syscall intent、内部 absolute-set helper 或测试用例；不能依赖旧 `File::seek(pos)` wrapper。
- `read_at` / `write_at` 的通用入口必须委托到 `FileOps` 字段；pipe 和 stream char device 不得通过普通 `read` / `write` 获得 positioned I/O。
- loop backing state 必须保存 narrowed handle，不保存任意 `Arc<File>` 后在 I/O 路径动态发现能力缺失。
- char devfs 的 `seek` 必须分发到 `CharDev`；memory char seek policy 不得硬编码在统一 devfs file ops。
- char devfs 的 `ioctl` 必须分发到 `CharDev`。
- block devfs 的 ioctl 和 loop hook 不应被本计划改坏。

## 可观测性清单

- 为 seek helper 的错误路径保留清晰 errno，不增加热路径噪声日志。
- 对 `SEEK_DATA` / `SEEK_HOLE` 继续返回明确 unsupported 或 NYI，不吞成普通 `EINVAL`，除非项目统一决定这么映射。
- 字符设备 ioctl 默认 unsupported 通过现有 `SysError::UnsupportedIoctl` 映射到 `ENOTTY`。
- loop backing conversion 失败应能从返回 errno 和转换位置判断是 fd access、path-only、file type 还是 positioned capability 不满足。

## 停止边界

应该停止并回到草案 review 的情况：

- 实现需要引入完整 Linux `FMODE_*` 能力模型。
- 实现发现 `read_at` / `write_at` 必须跨 VFS 建立新的全局能力状态，而不是局部 helper 或 narrowed handle 能解决。
- 实现发现必须恢复通用 dummy-cursor `read` / `write` wrapper 才能维持现有后端；这表示 positioned I/O contract 没有闭合，应回到 RFC。
- loop backing handle 无法在 fd/VFS 边界验证能力，只能在 block I/O 时发现错误。
- 字符设备 seek 或 ioctl 需要传入 task/fd table 才能工作。
- 为了修 seek 需要改变 read/write position lock 的长期并发语义。

不应阻塞本计划的情况：

- 某个具体字符设备还没有私有 ioctl。
- `SEEK_DATA` / `SEEK_HOLE` 仍未实现。
- tty 或 random 的 Linux 完整行为尚未覆盖。
