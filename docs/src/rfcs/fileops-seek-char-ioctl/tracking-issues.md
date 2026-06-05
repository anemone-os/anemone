# FileOps seek 与字符设备 ioctl Tracking Issues

**状态：** Closed
**最后更新：** 2026-06-05
**父 RFC：** [RFC-20260605-fileops-seek-char-ioctl](./index.md)
**事务日志：** [2026-06-05-fileops-seek-char-ioctl](../../devlog/transactions/2026-06-05-fileops-seek-char-ioctl.md)

本文只跟踪当前仍影响实现顺序、review gate、停止边界或验收判断的问题。修复后必须把结论折回 `index.md`、`invariants.md` 或 `implementation.md`，本页只保留问题状态和 neutralize 依据。

## Apollyon

- None.

## Keter

- None.

## Euclid

- None.

## Safe

- None.

## Neutralized

### KETER-003：`FileOps::write_at` contract 缺少 fd/VFS/backend 三层职责边界

**原问题：** 草案把 positioned I/O 固定为 `FileOps::read_at` / `FileOps::write_at`，并要求 `File::read_at` / `File::write_at` 直接委托 ops。但当前代码里 positioned write 不只是后端 offset write：`FileDesc::write_at()` 负责 fd access、path-only 与 `O_APPEND` 语义，`File::write_at()` 负责 zero-length fast path、只读挂载检查、`after_modified()` 元数据更新，然后才调用后端写入。如果草案只强调直接委托到 `ops.write_at`，实现者容易把 Linux-visible fd 状态、VFS 挂载写保护和 inode 修改时间散落到每个 backend。

**Neutralized：** [不变量需求](./invariants.md) 的“状态所有权”把 fd / `File` / backend 三层职责固定为 canonical contract，并在“禁止退化项”中禁止通用 dummy-cursor wrapper。[RFC 主文档](./index.md) 的方案摘要同步列出三层职责。[迁移实施计划](./implementation.md) 阶段 1C 把该 contract 变成实现 gate、审计项和最小验证项。

### KETER-004：loop backing file 的 positioned I/O 能力缺少可验证 API

**原问题：** 草案要求 loop backing file 只能绑定明确支持 positioned `read_at` / `write_at` 的文件对象；但在所有 `FileOps` 都新增 `read_at` / `write_at` 字段后，“存在方法”不等于“支持能力”。如果 loop setup 只能在实际 I/O 时观察 `ESPIPE` / unsupported，错误对象可能已经保存进 loop backing state。当前 loop 代码至少通过 regular inode 过滤 backing file，这个过滤应被固定成一个窄的 backing-handle API，而不是让 loop 直接推断任意 file op 能力。

**Neutralized：** [不变量需求](./invariants.md) 新增 `BackingFileHandle` / narrowed handle contract：conversion 阶段在 VFS/fd 边界验证 access、path-only、readonly、file type 或 positioned-capable backend，loop state 只保存 narrowed handle。[RFC 主文档](./index.md) 明确“存在 `read_at` 字段”不是 capability。[迁移实施计划](./implementation.md) 阶段 1C 要求 loop `set_fd` 改用 narrowed handle，并允许第一阶段保守限制为 regular file。

### EUCLID-003：`CharDev::seek` 建议签名把 VFS `File` 暴露给字符设备 trait

**原问题：** 草案建议 `CharDev::seek(&self, file: &File, pos: &mut usize, from: SeekFrom)` 或等价 capability hook。null-style seek 实际只需要 positioned cursor 和 seek intent；把 `&File` 传给 `CharDev` 会让字符设备 trait 看到 VFS 对象，扩大字符设备子系统对 VFS 内部形状的依赖。ioctl 路径已经选择 `CharIoctlCtx` 作为窄包装，seek 路径也应保持同样风格。

**Neutralized：** [不变量需求](./invariants.md) 将 `CharSeekCtx` 固定为字符设备 seek 的默认窗口，并禁止默认携带完整 `File`、`FileDesc`、task 或 fd table。[RFC 主文档](./index.md) 的字符设备 seek 方案改为 `CharDev::seek(&self, ctx: CharSeekCtx<'_>)`。[迁移实施计划](./implementation.md) 阶段 2 把 `CharSeekCtx` 作为交付、审计和退出条件。

### EUCLID-004：阶段 1 同时承担过多 shared-contract 迁移

**原问题：** 当前阶段 1 同时引入 shared vtable 字段、迁移 `sys_lseek`、分类 positioned I/O、处理目录 rewind、审计内部 `.seek(` 调用，并补齐所有 initializer。这个阶段跨度太大，任何一个子项出错都会污染同一个 gate，也不利于 agent 分工、review 或回滚。

**Neutralized：** [迁移实施计划](./implementation.md) 将原阶段 1 拆成阶段 1A mechanical API sweep 与 fail-closed 默认实现、阶段 1B `lseek` / seek intent 迁移、阶段 1C positioned I/O 分类与 loop backing handle。每个子阶段都有独立前置条件、交付、审计、验证和退出条件。

### KETER-001：positioned I/O 不能通过移除 `validate_seek` 自动继承普通 `read` / `write`

**原问题：** 草案曾允许简单迁移：移除通用 `validate_seek` 调用，由各 `read` / `write` 根据 positioned `pos` 自行检查。当前代码中 `File::read_at()` / `File::write_at()` 是先做 `validate_seek(pos)`，再用 dummy position 调用同一套 `FileOps::read` / `FileOps::write`。pipe 和 stream char device 的 read/write 实现无法区分普通顺序 I/O 与 `pread` / `pwrite`，如果直接删除 positioned I/O gate，可能把本应返回 `ESPIPE` 的 positioned I/O 变成真实读写、阻塞或状态消费。

**Neutralized：** [RFC 主文档](./index.md) 已把 `FileOps::read_at` / `FileOps::write_at` 固定为后端可见的 positioned I/O file operation，并拒绝通用 dummy-cursor wrapper。[不变量需求](./invariants.md) 把该禁止项列入 shared contract。[迁移实施计划](./implementation.md) 阶段 1A 要求 fail-closed initializer，阶段 1C 要求删除临时 compatibility wrapper 并完成 backend 分类。

### KETER-002：memory char device 的 seek 特例缺少 `CharDev` 所有权入口

**原问题：** 草案要求字符设备默认不可 seek，但 `/dev/null`、`/dev/zero` 和可选 `/dev/full` 使用 null-style seek：成功后 position 为 `0` 并返回 `0`。当前 devfs 使用统一 `CHAR_DEV_FILE_OPS`，`CharDev` trait 只有 `read` / `write`，没有 seek hook 或 seek policy。若不补接口，`CHAR_DEV_FILE_OPS.seek` 只能按 devnum 硬编码 memory device 特例，绕开字符设备子系统所有权。

**Neutralized：** [RFC 主文档](./index.md) 已明确字符设备 seek 与 ioctl 都归 `CharDev` 所有，`CHAR_DEV_FILE_OPS.seek` 只做 `rdev` lookup 和分发。[不变量需求](./invariants.md) 把 `CharSeekCtx` 和 char devfs 分发边界固定为状态所有权规则。[迁移实施计划](./implementation.md) 阶段 2 要求给 `CharDev` 增加默认 seek hook / capability，memory char device 在对应实现中显式 override null-style seek，并禁止 devfs devnum 特判。

### EUCLID-001：目录 seek 语义需要明确是本轮非目标还是最小支持

**原问题：** 阶段 1 写到 directory seek 返回 `EISDIR` 或沿用当前目录 seek 错误边界。当前 `getdents64` 使用 file position 作为目录游标，Linux 目录 fd 也支持 llseek 用于 rewind 或定位。如果新的 `FileOps::seek` 继续把目录固定为 `EISDIR`，会把一个已有目录枚举能力的 cursor reset 路径排除掉。

**Neutralized：** [RFC 主文档](./index.md) 选择本轮最小支持 `lseek(dirfd, 0, SEEK_SET)` rewind 目录 cursor，并把复杂目录 offset cookie、`SEEK_CUR` / `SEEK_END` / 非零 `SEEK_SET` 留作 follow-up。[迁移实施计划](./implementation.md) 阶段 1B 的 seek 分类、验证和退出条件已经包含 directory rewind。

### EUCLID-002：迁移审计清单漏掉内部 `File::seek(pos)` 调用

**原问题：** 阶段 1 的前置搜索和退出条件主要覆盖 `validate_seek`、`FileOps`、`sys_lseek`、`read_at` 和 `write_at`。当前树中 `openat` 的 `O_APPEND` 初始化、ELF loader、`utils::data` 和若干内核测试也直接调用 `File::seek(pos)`。迁移 `File::seek` 为 seek intent API 后，这些内部绝对定位调用也必须分类。

**Neutralized：** [迁移实施计划](./implementation.md) 阶段 1A/1B 前置搜索和审计清单已加入 `.seek(`，并要求内部绝对定位调用改成 `SeekFrom::Set(pos)` 或命名明确的 `seek_set_checked` / `set_pos_checked` helper；旁路审计清单也要求所有 `.seek(` 调用必须分类，不能依赖旧 `File::seek(pos)` wrapper。
