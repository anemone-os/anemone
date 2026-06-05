# 2026-06-05 - FileOps Seek and Char Device ioctl

**Status:** Active
**Owners:** doruche, Codex
**Area:** VFS / syscall ABI / devfs / character device / loop
**RFC:** [RFC-20260605-fileops-seek-char-ioctl](../../rfcs/fileops-seek-char-ioctl/index.md)
**Current Phase:** Gate 3 passed; stopped before Agent 4

## Scope

本事务跟踪 `FileOps::validate_seek` 到 `FileOps::seek` / positioned I/O / `CharDev` ioctl
的 staged 迁移。实现按 RFC gate 推进：

- 阶段 1A：shared `FileOps` mechanical API sweep 与 fail-closed 默认实现；
- 阶段 1B：迁移 `lseek(2)` 到 opened file seek intent；
- 阶段 1C：收紧 fd / VFS / backend positioned I/O contract，并引入 loop backing narrowed handle；
- 阶段 2：接通 `CharDev` seek policy 和 memory char device null-style seek；
- 阶段 3：接通字符设备 ioctl 默认分发；
- 最终旁路审计、构建 gate、最小证据整理和收口。

非目标：

- 不引入完整 Linux `FMODE_*` 能力模型。
- 不实现完整 `SEEK_DATA` / `SEEK_HOLE`。
- 不扩大 tty、random、serial 等字符设备的完整 ioctl 协议。
- 不改变 loop 设备作为 block private ioctl hook 的边界。
- 不运行 QEMU / LTP，除非用户后续明确授权；运行态证据默认由用户提供。

## Invariants

- `lseek(2)`、positioned I/O 和 ioctl 分发必须有各自清晰 owner，不能继续共用
  `validate_seek(pos)` 这一模糊入口。
- `FileOps::seek` 接收 seek intent，而不是 syscall 层预先算好的最终 offset。
- `File::{read_at,write_at}` 不得通过通用 dummy cursor wrapper 偷换成普通
  `read` / `write`。
- fd-local policy、VFS-wide gate 和 backend positioned I/O capability 必须分层表达。
- loop state 只保存 VFS/fd 边界产出的 narrowed backing handle，不保存任意
  `FileDesc` / `IoctlArgFile` 后再推断能力。
- `CharDev` seek / ioctl 只接收窄 ctx，不默认暴露完整 `File`、`FileDesc`、task 或 fd table。
- 每个 worker 只能写入编排文档指定 write set；需要扩大 write set 时必须停止上报。

## Handoff

**Last Updated:** 2026-06-05

**Current Branch:** `dev/drc/seek`

**Canonical RFC:** [RFC-20260605-fileops-seek-char-ioctl](../../rfcs/fileops-seek-char-ioctl/index.md), [Invariants](../../rfcs/fileops-seek-char-ioctl/invariants.md), [Implementation Plan](../../rfcs/fileops-seek-char-ioctl/implementation.md), [Tracking Issues](../../rfcs/fileops-seek-char-ioctl/tracking-issues.md)

**Completed:** 公开 RFC、invariants、implementation、tracking issues 和 agent orchestration 文档已存在。Agent 0 只读前置审计完成；Agent 1 阶段 1A mechanical API sweep 和 Gate 1 review 已完成。Agent 2 阶段 1B 已迁移 `sys_lseek()` 到 opened file seek intent，`File::seek` 现在接收 `SeekFrom` 并在 `File.pos` lock 内调用 backend seek，目录类 file ops 最小支持 `SEEK_SET(0)` rewind，内部绝对定位调用已改为 `seek_set_checked()`。Agent 3 阶段 1C 已删除阶段 1A compat helper / `validate_seek()` 残留，分类 positioned I/O，并让 loop 保存 VFS/fd 边界产出的 `BackingFileHandle`；Gate 3 独立 reviewer 已通过。

**In Progress:** 暂停在 Agent 4 前。不得直接启动 Agent 5+。

**Open Blockers:** 暂无已确认 blocker。

**Next Action:** 按编排启动 Agent 4，完成阶段 2 `CharDev` seek policy 和 memory char device null-style seek。不得直接启动 Agent 5+。

**Do Not Redo:** 不要把 `FileDesc` / `ProcFile` / task / fd table 传进 file ops 或 `CharDev` hook；不要让 worker 越界改后续阶段；不要把 `SEEK_DATA` / `SEEK_HOLE` 交给 backend；不要用通用 wrapper 给 stream 后端继承 positioned I/O 能力；不要改 loop / block ioctl 所有权边界。

## Phase Log

### 2026-06-05 - 事务日志启动与 Agent 0 前置审计记录

**Phase:** orchestration / pre-audit

**Change:** 建立本事务日志，并把 [RFC-20260605-fileops-seek-char-ioctl](../../rfcs/fileops-seek-char-ioctl/index.md)、事务索引、mdBook Summary 和当前双周 devlog 连接到同一条实现记录。

**Review:** 总控只读刷新当前落点：分支为 `dev/drc/seek`，前置审计时工作区干净；未发现已引入且与 RFC 不兼容的 `FileOps` seek / positioned I/O 抽象；loop backing 尚未扩大到任意 VFS object 且缺 narrowed conversion contract；字符设备 seek / ioctl 尚未开始传递完整 `FileDesc`、task 或 fd table。

**Validation:** 用户明确要求跳过 baseline `just build`；本阶段未运行构建、QEMU 或 LTP。

**Next:** 启动 Agent 1 与 Gate 1 review agent；不启动 Agent 2+。

### 2026-06-05 - Agent 1 与 Gate 1 review agent 启动

**Phase:** Agent 1 / Gate 1 setup

**Change:** Agent 1 write set 限定为 `anemone-kernel/src/fs/file.rs`、所有直接 `FileOps` initializer 文件、必要的 `fs::mod` re-export 和本事务 devlog。Agent 1 只能完成阶段 1A mechanical API sweep 与 fail-closed 默认实现；不得迁移 `sys_lseek()` 外部语义，不得改 loop backing handle，不得实现 `CharDev` hook。

**Review:** Gate 1 reviewer 只读准备审查清单，等待 Agent 1 diff 后检查阶段 1A gate。reviewer 不改代码。

**Validation:** 按用户要求，本轮不跑 baseline build；Agent 1 完成后可运行 `git diff --check`，是否运行 `just build` 由用户后续指示或 gate 需要决定。

### 2026-06-05 - Agent 1 阶段 1A mechanical sweep 完成

**Phase:** Agent 1 / Stage 1A

**Change:** `FileOps` vtable 移除 `validate_seek` 字段，新增 `seek`、`read_at`、`write_at` 字段。新增内部 `SeekFrom::{Set, Cur, End}` 和 `seek_with_fixed_size` / `seek_with_inode_size` / `seek_with_bounded_size` helper，使后端能接收 seek intent。`SEEK_DATA` / `SEEK_HOLE` 仍停留在现有 syscall whence 转换层，本阶段未进入 backend contract。

**Change:** `File::read_at()` / `File::write_at()` 不再通过 `validate_seek + dummy_pos` 复用普通 `read` / `write`，改为完成 zero-length / VFS writable gate 后委托 `ops.read_at` / `ops.write_at`。`File::seek_from()` 持有 `File.pos` lock 后调用 backend seek，并保留 `seek_set_checked()` 和旧 `seek(pos)` 作为阶段 1A 内部绝对定位兼容入口。

**Change:** 所有直接 `FileOps` initializer 显式补齐 `read_at`、`write_at`、`seek`。pipe、console、stream char devfs、path-only、symlink 和目录类对象 fail closed；regular file、block devfs 和 proc snapshot 类对象使用阶段 1A 临时 helper `compat_read_at_via_seek_then_read_1c_delete` / `compat_write_at_via_seek_then_write_1c_delete` 或 bounded seek helper 保持机械闭合。

**Boundary:** 未修改 `sys_lseek()` 外部语义；未迁移 `SEEK_CUR` / `SEEK_END` 的 syscall 层计算；未改 loop backing handle；未实现 `CharDev` seek 或 ioctl hook。`File::validate_seek()` 仍作为阶段 1A 兼容迁移点保留，因为 `FileDesc::write_at()` 属于后续 fd-layer positioned I/O 分类，不在 Agent 1 write set；该残留必须在阶段 1B/1C 分类并删除。

**Review:** 原 Agent 1 worker 在完成部分 initializer sweep 后被总控中断停止；总控接管同一 write set 补齐剩余 proc / tgid / proc root direct `FileOps` initializer。未触发 write set 扩展申请，未触发 RFC 停止条件。

**Audit:** `rg -n "validate_seek|FileOps \\{|read_at:|write_at:|seek:" anemone-kernel/src` 显示 `validate_seek:` vtable 字段已消失，所有 direct `FileOps` initializer 都显式列出 `read_at` / `write_at` / `seek`。剩余 `validate_seek` 命中仅为 `File::validate_seek()` 兼容迁移点和 `task/files.rs` 中 `FileDesc::write_at()` 对该迁移点的调用。`compat_*_1c_delete` helper 命名可搜索，并在本条记录为阶段 1C 删除/分类点。

**Validation:** `git diff --check` 通过。按用户要求未运行 `just build`、QEMU 或 LTP。

**Next:** Gate 1 reviewer 只读审查本阶段 diff；通过后才能进入 Agent 2，不能直接启动后续 worker。

### 2026-06-05 - Gate 1 review 通过

**Phase:** Gate 1 / Stage 1A review

**Review:** Gate 1 reviewer 未发现 Apollyon / Keter blocker。确认本 diff 满足阶段 1A：`FileOps::validate_seek` 已不再是 vtable 字段，所有 direct initializer 显式提供 `seek` / `read_at` / `write_at`，`File::read_at()` / `File::write_at()` 委托新 ops，stream / path-only / directory / symlink 对象 fail closed，且本阶段没有迁移 `sys_lseek()`、loop backing handle 或 `CharDev` hook。

**Review:** reviewer 记录一个 Euclid：`compat_*_1c_delete` 临时 helper 经 `fs/mod.rs` 导出，作为短期 backend migration helper 略宽；因 helper 命名清楚、可搜索，且当前只用于 regular / block / proc snapshot 风格 initializer，不阻塞 Gate 1。阶段 1C 必须删除或 backend-localize。

**Review:** reviewer 记录一个 Safe：`File::validate_seek()` 兼容方法及 `FileDesc::write_at()` 调用仍存在。由于 `task/files.rs` 不在 Agent 1 write set，且该方法不再是 `FileOps` vtable 字段，并已在本事务记录为阶段 1A 兼容迁移点，Gate 1 接受。后续 1B/1C 必须分类并移除旧 API。

**Validation:** reviewer 未运行 build / QEMU / LTP。当前已运行且通过的验证仍为 `git diff --check`；按用户要求未运行 `just build`。

**Next:** 停止在 Agent 2 前，等待用户确认继续。

### 2026-06-05 - Agent 2 阶段 1B lseek / seek intent 迁移完成

**Phase:** Agent 2 / Stage 1B

**Change:** `sys_lseek()` 现在只做 fd lookup、path-only 检查、Linux `whence` 转换和 dispatch；不再在 syscall 层读取 `File.pos` 或 `inode().size()` 来解释 `SEEK_CUR` / `SEEK_END`。`SEEK_DATA` / `SEEK_HOLE` 继续在 whence 转换层返回 `NotYetImplemented`，不进入 backend seek。

**Change:** `File::seek` 改为接收 `SeekFrom` intent，并在持有 `File.pos` lock 时调用 backend `ops.seek`、写回并返回用户可见新 position；错误时恢复旧 position。旧 `File::seek(pos)` wrapper 已删除，内部绝对定位调用改为 `seek_set_checked(pos)`。常用 seek helper 按用户要求保留在 `anemone-kernel/src/fs/file.rs` 内的 `mod seek { ... }` 中，而不是拆成单独文件。

**Change:** regular file 后端继续通过 inode size 处理 `SEEK_END`；proc snapshot 类文件使用 bounded-size seek；block devfs seek 使用设备总字节数处理 `SEEK_END`，并保留 alignment / end boundary 检查；pipe、console 和 char devfs seek 继续返回 `IllegalSeek`。ext4、ramfs、devfs、proc root、`/proc/<tgid>` 和 `/proc/<tgid>/fd` 目录 file ops 改为 `SEEK_SET(0)` rewind-only helper，复杂目录 seek 仍 fail closed。

**Boundary:** 未修改 loop backing handle，未接通 `CharDev` seek/ioctl hook，未启动阶段 1C positioned I/O 分类。`File::validate_seek()` 和 `FileDesc::write_at()` 中的调用仍作为阶段 1C 残留迁移点存在；`compat_*_1c_delete` helper 也仍等待 Agent 3 删除或 backend-localize。

**Audit:** `rg -n "inode\\(\\)\\.size\\(\\)|vfs_file\\.pos\\(\\)" anemone-kernel/src/fs/api/lseek.rs` 无命中。`rg -n "\\.seek\\(" anemone-kernel/src --glob "*.rs"` 只剩 `sys_lseek` -> `FileDesc::seek(SeekFrom)` -> `File::seek(SeekFrom)` intent dispatch 和 `seek_set_checked()` 内部调用。`rg -n "seek: \\|_, _, _\\| Err\\(SysError::IsDir\\)" anemone-kernel/src/fs anemone-kernel/src/device` 无命中。

**Validation:** `git diff --check` 通过。`just build` 通过；构建期间仅出现既有 `anemone-kernel/src/sync/mono.rs` unused import 警告。未运行 QEMU / LTP。

**Next:** 启动 Gate 2 review；不得直接进入 Agent 3。

### 2026-06-05 - Gate 2 review 通过

**Phase:** Gate 2 / Stage 1B review

**Review:** Gate 2 readonly reviewer 未发现 Apollyon / Keter / Euclid blocker。确认 `sys_lseek()` 不再直接使用 `inode().size()` 解释全局 `SEEK_END`，也不再在 syscall 层执行 `pos()` + `seek()` 的非原子 `SEEK_CUR` 读改写；`SEEK_DATA` / `SEEK_HOLE` 仍停在 whence 转换层返回 `NotYetImplemented`，没有进入 backend seek。

**Review:** reviewer 确认 `FileDesc::seek(SeekFrom)` 只做 fd/path-only gate 和 intent forwarding，`File::seek(SeekFrom)` 在 `File.pos` lock 内调用 backend seek、成功写回新位置、失败恢复旧位置。regular file、block devfs、pipe、char devfs 和目录 rewind 行为符合阶段 1B contract；没有发现越界接通 loop backing handle、`CharDev` seek/ioctl 或阶段 1C positioned I/O 分类。

**Review:** reviewer 记录一个 Safe：`FileDesc::write_at()` 仍调用 `File::validate_seek()`，`compat_*_1c_delete` 仍被 regular / block / proc snapshot 后端使用。这符合 Gate 2 边界，不阻塞阶段 1B；Agent 3 必须删除或 backend-localize 这些阶段 1A/1C 残留。

**Audit:** 总控复查 `rg -n "\\.seek\\(|seek_set_checked\\(|validate_seek\\(|seek_from\\(" anemone-kernel/src --glob '*.rs'`，确认 `.seek(` 只剩 `sys_lseek` -> `FileDesc::seek(SeekFrom)` -> `File::seek(SeekFrom)` intent dispatch；内部绝对定位调用已改为 `seek_set_checked()`；`validate_seek()` 只剩阶段 1C positioned I/O 残留。`rg -n "inode\\(\\)\\.size\\(\\)|vfs_file\\.pos\\(\\)|pos\\(\\).*\\+|SEEK_DATA|SEEK_HOLE|sys_lseek|NotYetImplemented" anemone-kernel/src/fs/api/lseek.rs` 确认 lseek syscall 层没有全局 inode-size / pos arithmetic 残留。

**Validation:** `git diff --check` 通过。`just build` 通过；构建期间仅出现既有 `anemone-kernel/src/sync/mono.rs` unused import 警告。未运行 QEMU / LTP。

**Next:** 停止在 Agent 3 前，等待用户确认继续阶段 1C。

### 2026-06-05 - Agent 3 阶段 1C positioned I/O / loop handle 完成

**Phase:** Agent 3 / Stage 1C

**Change:** `FileDesc::{read_at,write_at}` 去掉旧 `File::validate_seek()` 兼容调用，只保留 fd-local read/write access、path-only gate、status flags 和 `O_APPEND` 决策。`File::{read_at,write_at}` 保持 zero-length fast path、regular content readonly mount gate 和 successful write metadata update 后委托 `ops.read_at` / `ops.write_at`。

**Change:** 删除阶段 1A 的全局 `compat_read_at_via_seek_then_read_1c_delete` / `compat_write_at_via_seek_then_write_1c_delete` helper 和 `File::validate_seek()`。regular file 后端改为 backend-local positioned helper：`ext4_read_at` / `ext4_write_at`、`ramfs_read_at` / `ramfs_write_at`。block devfs 改为 `block_read_at` / `block_write_at`，继续复用 block backend 内 alignment、bounds 和 overflow 检查。pipe、console、stream char devfs、directory、symlink/path-only 等对象继续 fail closed。

**Change:** procfs snapshot 类文件按现有读取模型显式分类：`/proc/meminfo`、`/proc/uptime`、`/proc/<tgid>/{cmdline,environ,mounts,stat,status}` 增加局部 `read_at` 实现，并复用 procfs 内部 `read_snapshot_at` helper 或局部 cursor。未扩大 procfs 写入或复杂 snapshot 语义。

**Change:** 新增 VFS/fd 边界 narrowed `BackingFileHandle`，由 `IoctlArgFile` conversion 验证 path-only、read access、regular file 和 backing writability，并只向 loop 暴露 `read_exact_at`、`write_all_at`、`visible_size`、`get_attr` / display 信息。`LoopBoundState` / snapshot 保存 `BackingFileHandle`，不再保存裸 `Arc<File>`；loop block I/O 通过 handle 的 positioned methods 访问 backing storage。

**Boundary:** Agent 3 worker 先完成 positioned I/O 分类后停在 loop handle 中间态；总控在同一 Agent 3 write set 内删除 loop 本地重复 handle 并收口到唯一 `fs::BackingFileHandle`。未启动 Agent 4 / Agent 5，未接通 `CharDev` seek 或 ioctl hook，未触发 write set 扩展申请或 RFC 停止条件。

**Audit:** `rg -n "compat_.*1c_delete|validate_seek|struct BackingFileHandle|IoctlArgFile|Arc<File>|file_handle\\(|read_exact_at\\(|write_all_at\\(" anemone-kernel/src/fs anemone-kernel/src/device/block/loop.rs anemone-kernel/src/task/files.rs` 显示 `compat_*_1c_delete` 和 `validate_seek` 无残留；`BackingFileHandle` 只在 `fs::file` 定义，loop state / snapshot 使用该 narrowed handle；裸 `Arc<File>` 只存在于 fd/VFS-owned `ProcFile` / `IoctlArgFile` / handle 内部，不由 loop state 直接保存。

**Audit:** 旁路审计搜索 `rg -n "validate_seek|FileOps \\{|sys_lseek|read_at\\(|write_at\\(|\\.seek\\(|CHAR_DEV_FILE_OPS|BLOCK_DEV_FILE_OPS|LOOP_SET_FD|BackingFileHandle|CharSeekCtx|CharIoctlCtx" anemone-kernel/src` 已执行。结果确认 `validate_seek` 无残留，`BLOCK_DEV_FILE_OPS` 使用 explicit `block_read_at` / `block_write_at`，`CHAR_DEV_FILE_OPS` 仍保持阶段 2 前 fail-closed seek/ioctl 边界，未提前引入 `CharSeekCtx` / `CharIoctlCtx`。

**Validation:** `git diff --check` 通过。`just build` 通过；构建期间仅出现既有 `anemone-kernel/src/sync/mono.rs` unused import 警告。未运行 QEMU / LTP。

**Next:** 启动 Gate 3 readonly reviewer；review 必须由独立 agent 执行，不能由总控代替。Gate 3 通过后才能启动 Agent 4。

### 2026-06-05 - Gate 3 review 通过

**Phase:** Gate 3 / Stage 1C review

**Review:** Gate 3 独立 readonly reviewer 未发现 Apollyon / Keter / Euclid blocker。确认 `FileDesc::{read_at,write_at}` 只保留 fd-local access、path-only gate、status flags / `O_APPEND` 决策；`File::{read_at,write_at}` 只做 zero-length、regular readonly mount gate、successful write metadata update 后委托 ops；`File::validate_seek()` 和 `compat_*_1c_delete` 已无残留。

**Review:** reviewer 确认 regular / block / proc positioned I/O 已 backend-local，pipe、stream char、console、directory、symlink/path-only fail closed。`BackingFileHandle` 在 VFS/fd 边界验证 path-only、read access、regular file 和 writable snapshot，loop state 保存该 narrowed handle，而不是任意 `Arc<File>` / `FileDesc` / `IoctlArgFile`。未发现越界接通 `CharDev` seek/ioctl，block / loop ioctl ownership 未回退。

**Review:** reviewer 记录一个 Safe：未重跑 `just build`、QEMU 或 LTP；只重跑 `git diff --check` 并通过。构建结论沿用主控已运行通过的 `just build`。

**Validation:** 主控已运行 `git diff --check` 和 `just build`，均通过；构建期间仅出现既有 `anemone-kernel/src/sync/mono.rs` unused import 警告。reviewer 重跑 `git diff --check` 通过。未运行 QEMU / LTP。

**Next:** 启动 Agent 4；不得直接启动 Agent 5+。后续 review 仍由独立 reviewer agent 执行，不能由总控代替。
