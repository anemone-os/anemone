# 2026-06-04 - PROC TGID FD

**Status:** Active; stage 1 closed
**Owners:** doruche, Codex
**Area:** procfs / fd / path visibility / LTP
**RFC:** [RFC-20260604-proc-tgid-fd](../../rfcs/proc-tgid-fd/index.md)
**Current Phase:** Stage 1 closed; Agent 5 deferred unless fd entry open becomes a direct blocker

## Scope

本事务跟踪 `/proc/<tgid>/fd` 的第一阶段实现：先补齐 `/proc/self/fd` 目录枚举、数字 fd lookup、`readlink()` 目标字符串和基本 `getattr()`，支撑 musl `getcwd02` 与 LTP `pipe07` 这类当前阻塞点。

本事务覆盖：

- `FilesState` 的 fd number 只读快照入口；
- `/proc/<tgid>/fd` 静态 tgid entry 与 fd 目录专用 private data；
- `fd/<n>` symlink inode 的 `(ThreadGroupBinding, Fd)` 长期身份；
- `same-tgid only` 第一阶段访问策略；
- 普通路径、pipe 和其他匿名对象的 stage-1 `readlink()` 显示策略；
- `/proc/<tgid>/fd` 缺失限制的收口与 residual limitations 记录。

非目标：

- 不在第一阶段实现完整 Linux magic-link open。
- 不实现 `/proc/<tgid>/fdinfo/<n>`。
- 不实现 ptrace / dumpable / namespace 风格的跨进程权限策略。
- 不承诺所有匿名对象显示名完全等价 Linux。
- 不改变 fd table、`CLONE_FILES`、fork、dup、close 或 `close_range` 语义。

## Invariants

- `fd` 本身通过 `TGID_ENTRIES` 注册和枚举，动态 `fd/<n>` 子项只进入 `fd` 目录自己的 child ino cache。
- procfs fd 目录只观察 fd table，不拥有 fd table。
- `fd/<n>` inode 不长期保存 `Arc<FileDesc>`；`readlink()`、`getattr()` 和未来 `open()` 必须按操作重新验证当前 fd table。
- fd table snapshot 和 fd entry 当前存在性必须使用同一打开集合语义。
- 非 `same-tgid` 访问不得先读取目标 fd table，也不得通过错误码或路径字符串泄露目标 fd 是否存在。
- 普通路径只按目标 leader root 视角显示；root 外路径不得 fallback 成全局路径。
- child ino cache 只表示 procfs synthetic identity，不能证明 fd 当前仍打开。

## Handoff

**Last Updated:** 2026-06-04

**Canonical RFC:** [RFC-20260604-proc-tgid-fd](../../rfcs/proc-tgid-fd/index.md), [Implementation Plan](../../rfcs/proc-tgid-fd/implementation.md), [Tracking Issues](../../rfcs/proc-tgid-fd/tracking-issues.md)

**Completed:** `proc-tgid-fd` 已提升为公开目录级 RFC；design review 发现的 Keter / Euclid 项已吸收到 RFC 主文档与迁移实施计划，并保留在 Tracking Issues 的 Neutralized 区段。本事务日志、事务索引、双周 devlog 和 mdBook Summary 已建立链接。阶段 0 已在 `FilesState` / `Task` 增加 fd number 只读 snapshot API。Gate 1 review 已确认 snapshot 语义、bitmap/fds 普通 `assert!` 校验和窄 Task wrapper。Agent 2 已实现 `TgidEntry` private-data factory 扩展、`fd` 静态 entry、`/proc/<tgid>/fd` 目录、数字 fd lookup、fd child synthetic ino cache，以及 fd entry symlink inode 的最小存在性验证。Gate 2 review 已确认目录框架、dynamic child identity、锁序和错误映射边界。Agent 3 已实现 fd symlink `readlink()` 显示策略：普通路径按目标 leader root 视角格式化，root 外返回权限错误；pipe 通过 `fs::pipe` owner-side helper 显示为 `pipe:[ino]`；其他 anonymous namespace 对象返回稳定 `anon_inode:[anemone-<ino>]` fallback。`getattr()` 继续按操作重新验证 binding、`same-tgid` 和当前 fd 存在。Gate 3 review 已确认阶段 1 安全边界、缓存边界、pipe owner-side helper 和阶段 2 open 未泄漏。Agent 4 已完成旁路审计、`git diff --check`、`just build` 和 current limitations 收口。

**In Progress:** 无。阶段 2 不自动推进。

**Open Blockers:** 暂无已确认 blocker。

**Next Action:** 等待用户或后续验证提供 musl `getcwd02`、LTP `pipe07`、smoke 或新的 fd entry open 直接阻塞证据；只有确认 `open("/proc/<tgid>/fd/<n>")` 成为新阻塞项后才进入 Agent 5。

**Do Not Redo:** 不要用 `/proc/self/fd/<n>` 字符串特判绕过 procfs/VFS inode；不要把动态 fd number 塞进 `TGID_ENTRIES` 或 `<tgid>` 目录 `sub_ino` 静态子项表；不要缓存 `Arc<FileDesc>` 作为 fd entry 真相；不要在权限基础设施缺位时扩大跨进程可见性；不要返回目标 root 外的全局路径字符串。

## Phase Log

### 2026-06-04 - RFC 提升与事务日志启动

**Phase:** docs / RFC promotion

**Change:** 新增公开 [RFC-20260604-proc-tgid-fd](../../rfcs/proc-tgid-fd/index.md)，目录包含 [迁移实施计划](../../rfcs/proc-tgid-fd/implementation.md) 和 [Tracking Issues](../../rfcs/proc-tgid-fd/tracking-issues.md)。

**Review:** 本轮只做文档提升。草案中的 Keter / Euclid review 项已经折回 RFC 主文档和实施计划；Tracking Issues 当前没有开放的 Apollyon / Keter / Euclid / Safe 项，只保留 Neutralized 记录。

**Validation:** 文档结构更新；未修改生产代码，未运行构建、QEMU 或 LTP。

**Next:** 从阶段 0 开始实现 fd table 只读观察接口；阶段 1 再落地 `/proc/<tgid>/fd` 目录、lookup、`readlink()` 和 `getattr()`。

### 2026-06-04 - 阶段 0 fd number snapshot

**Phase:** Agent 1 / fd table observation

**Change:** 在 `FilesState` 增加 `opened_fd_numbers_snapshot() -> Vec<Fd>`，并在 `Task` 上增加同名 wrapper，供后续 procfs 只观察 fd number。snapshot 遍历 `fds`，按 fd number 递增返回当前打开项；不返回 `Arc<FileDesc>`，不暴露内部可变引用，不调用 VFS、procfs、路径格式化或其他阻塞路径。

**Invariant:** snapshot 的打开集合语义与 `FilesState::get_fd(fd)` 对齐，均以 `fds[i].is_some()` 为准。遍历每个 fd slot 时使用普通 `assert!` 校验 `bitmap.test(i)` 与 `fds[i].is_some()` 一致，用于暴露当前 bitmap/fds 双重真相源的分叉；没有改变 fd alloc、dup、close、fork、`CLONE_FILES` 或 `close_range` 语义。

**Review:** Gate 1 待办：重点检查 KETER-004、EUCLID-001，以及一致性校验是否保持低成本且没有降级为 `debug_assert!`。

**Validation:** `just build` passed。构建过程中仅出现既有 `anemone-kernel/src/sync/mono.rs` 的 `AtomicBool` / `Ordering` unused import warning；未运行 QEMU 或 LTP。

**Next:** Gate 1 review 通过后，进入 Agent 2 的 `TgidEntry` custom constructor 与 `/proc/<tgid>/fd` 目录框架实现。

### 2026-06-04 - Gate 1 snapshot review

**Phase:** Gate 1 review

**Review:** Gate 1 reviewer 未发现 blocking findings。KETER-004 清除：snapshot 和 `get_fd(fd)` 均以 `fds[i].is_some()` 作为打开集合语义。EUCLID-001 清除：snapshot 只返回 `Vec<Fd>`，不暴露或缓存 `Arc<FileDesc>`。bitmap/fds 校验使用普通 `assert!`；Task wrapper 让 procfs 可通过窄 API 消费 fd number snapshot。

**Validation:** Gate 1 reviewer 未修改文件、未运行构建；沿用 Agent 1 的 `just build` 和 `git diff --check` 结果。

**Next:** Agent 2 允许进入。

### 2026-06-04 - Agent 2 `/proc/<tgid>/fd` 目录框架

**Phase:** Agent 2 / tgid entry and fd directory framework

**Change:** 在 `TgidEntry` 增加 private-data factory 扩展点，默认 entry 继续创建 `TgidSubInodePrivate { binding }` 并复用原有 mode、perm、uid/gid、nlink 和时间戳初始化路径。`fd` 作为静态 entry 加入 `TGID_ENTRIES`，仍由 `<tgid>` 目录的统一 lookup / read_dir 发现。新增 `fs::proc::tgid::fd` 模块，提供 `ProcFdDirPrivate { binding, child_ino }`、`ProcFdEntryPrivate { binding, fd }`、fd 目录 inode/file ops、数字 fd lookup、目录 readdir 和 fd child synthetic ino cache。

**Invariant:** `fd/<n>` 动态子项不进入 `TGID_ENTRIES`，不进入 `<tgid>` 目录 `sub_ino` 静态表，也不复用目标文件 inode。`ProcFdEntryPrivate` 不保存 `Arc<FileDesc>`；lookup、readlink、getattr 和 open 都按操作重新验证 binding alive、`same-tgid` 和当前 fd 存在性。非 same-tgid 访问先返回 `SysError::AccessDenied`，不解析 fd number、不读取目标 fd table。数字解析、`Task::get_fd(fd)` 查询和 `BadFileDescriptor` 到 `SysError::NotFound` 的映射集中在 `fd.rs` helper。

**Boundary:** 本阶段没有实现最终路径或 pipe display 策略；`fd/<n>` `readlink()` 在重新验证当前 fd 存在后返回稳定 `SysError::NotSupported`，由 Agent 3 接管。`fd/<n>` `open()` 同样只做阶段 1 前置验证后返回 `SysError::NotSupported`，不宣称 magic-link open 语义。

**Write-set note:** 为了让 `TgidEntry` 扩展点保持结构化字段而不是在 lookup 中按名字 special-case，现有 tgid entry 定义文件补充了 `make_prv: default_tgid_entry_prv`。这是 constructor API 变更的机械配套，未改变这些 entry 的 mode、ops 或行为。

**Review:** Gate 2 待办：重点检查 KETER-005、KETER-003、EUCLID-003、EUCLID-002；额外检查 fd entry `readlink()` / `getattr()` 是否只保存 `(ThreadGroupBinding, Fd)` 并按操作重新验证当前 fd。

**Validation:** `just build` passed。构建过程中仅出现既有 `anemone-kernel/src/sync/mono.rs` 的 `AtomicBool` / `Ordering` unused import warning；未运行 QEMU、LTP 或 smoke。

**Next:** Gate 2 review 通过后，进入 Agent 3 的 fd symlink `readlink()` / `getattr()` 显示策略实现。

### 2026-06-04 - Gate 2 directory framework review

**Phase:** Gate 2 review

**Review:** Gate 2 reviewer 未发现 blocking findings。KETER-005 清除：`fd` 通过 `TGID_ENTRIES` 注册，`make_prv` 创建 `ProcFdDirPrivate { binding, child_ino }`，没有在 lookup 中按名字 special-case。KETER-003 清除：动态 `fd/<n>` 状态只在 fd 目录 `child_ino` map 和 `(ThreadGroupBinding, Fd)` entry private data 中，不进入 `<tgid>` `sub_ino`、`TGID_ENTRIES` 或目标文件 inode。EUCLID-003 清除：`read_dir()` 先取 fd snapshot，再分配 synthetic ino 并输出 sink；`lookup()` 先验证 access、解析和当前 fd 存在，再 materialize child inode。`proc_fd_child_inode()` 在 child cache lock 内 seed inode 与现有 `<tgid>` lookup 形状一致，未形成 fd table / path / VFS open 锁链。EUCLID-002 清除：数字解析与 `BadFileDescriptor -> NotFound` 映射集中在 helper。

**Boundary:** 现有 tgid entry 文件补 `make_prv: default_tgid_entry_prv` 已由用户允许作为 constructor API 的机械配套；review 未发现这些 entry 的 mode、ops 或行为变化。Agent 2 没有实现 Agent 3 display 策略，也没有实现阶段 2 magic-link open。

**Validation:** Gate 2 reviewer 未修改文件、未运行构建；沿用 Agent 2 的 `just build` 和 `git diff --check` 结果。

**Next:** Agent 3 允许进入。

### 2026-06-04 - Agent 3 fd symlink readlink / getattr display

**Phase:** Agent 3 / fd symlink display

**Change:** `fd/<n>` `readlink()` 现在在每次操作时重新验证 binding alive、`same-tgid` 和当前 fd 存在，然后基于当前 `FileDesc::vfs_file()` 生成 stage-1 目标字符串。普通路径对象从 `File::path().to_pathbuf()` 取得全局 `PathRef` 路径，再用目标 leader `rel_abs_path()` 转成目标 root 视角路径。若目标 path 不在 leader root 下，返回 `SysError::PermissionDenied`，不返回全局路径。`fs::pipe` 新增只读 owner-side `display_name()` helper，pipe fd 返回 `pipe:[ino]`。其他 anonymous namespace 对象返回稳定 `anon_inode:[anemone-<ino>]` fallback，不暴露 anonymous namespace 内部路径。

**Invariant:** `ProcFdEntryPrivate` 仍只保存 `(ThreadGroupBinding, Fd)`，不保存 `Arc<FileDesc>`。`readlink()` 与 `getattr()` 都通过 `validate_fd_access()` 和 `lookup_proc_fd()` 按操作重新验证当前 fd；fd 关闭后 `BadFileDescriptor` 仍映射为 `SysError::NotFound`。`fd.rs` 不读取 pipe private data，pipe 命名由 `fs::pipe` 提供。`fd/<n>` `open()` 仍返回 `SysError::NotSupported`，未实现阶段 2 magic-link open。

**Review:** Gate 3 待办：重点检查 KETER-001、KETER-002、fd entry 是否不缓存 `Arc<FileDesc>`、`readlink()` / `getattr()` 重新验证路径是否一致，以及 `fd.rs` 是否只编排 display 策略而不读取 pipe private data。

**Validation:** `just build` passed；构建过程中仅出现既有 `anemone-kernel/src/sync/mono.rs` 的 `AtomicBool` / `Ordering` unused import warning。`git diff --check` passed。未运行 QEMU、LTP 或 smoke。

**Next:** Gate 3 review 通过后，进入 Agent 4 阶段 1 smoke、旁路审计、current limitations 和事务日志收口。

### 2026-06-04 - Gate 3 fd symlink display review

**Phase:** Gate 3 review

**Review:** Gate 3 reviewer 未发现 blocking findings。KETER-001 清除：所有 fd 目录与 fd entry 路径继续复用 `validate_fd_access()`，在 fd lookup 或 display 前执行 `same-tgid` 检查；普通路径显示使用目标 leader `rel_abs_path()`，root 外路径返回 `SysError::PermissionDenied`，不返回全局路径。KETER-002 清除：`ProcFdEntryPrivate` 只保存 `(ThreadGroupBinding, Fd)`；`readlink()` 和 `getattr()` 每次重新执行 `validate_fd_access()` 与 `lookup_proc_fd()`，`BadFileDescriptor` 仍映射为 `SysError::NotFound`。pipe display 边界清除：`fd.rs` 只调用 `pipe::display_name(file)`，不读取 pipe private data；`pipe::display_name()` 是 `fs::pipe` owner-side helper，`pub(super)` 足够供 procfs 使用且保持 fs-internal。阶段 2 边界清除：`proc_fd_entry_open()` 仍重新验证 access/current fd 后返回 `SysError::NotSupported`，未实现 magic-link open。

**Validation:** Gate 3 reviewer 未修改文件、未运行构建；沿用 Agent 3 的 `just build` 与 `git diff --check` 结果。

**Next:** Agent 4 允许进入。

### 2026-06-04 - Agent 4 阶段 1 收口

**Phase:** Agent 4 / stage 1 closure

**Change:** 完成阶段 1 旁路审计、最低验证和文档收口。`docs/src/register/current-limitations.md` 中 `ANE-20260528-PROC-TGID-FD-FRAMEWORK-PENDING` 已从“`/proc/<tgid>/fd` 目录缺失”重分类为阶段 1 residual limitations：保留完整 Linux magic-link open / fd entry open、跨进程 ptrace / dumpable / namespace 权限、`fdinfo`、pipe 以外匿名对象精确显示名，以及 `O_PATH` proc fd link / 权限 / 后续能力。未声称 musl `getcwd02` 或 LTP `pipe07` 已通过。

**Audit:** `rg -n "TODO: fd|TGID_ENTRIES|proc-tgid-fd|fdinfo" anemone-kernel/src/fs/proc` 只命中 `TGID_ENTRIES` 的统一 `<tgid>` readdir / lookup 路径和 `mod.rs` 注册表，未再命中 `TODO: fd` 或 procfs 代码内 `fdinfo` 实现。`rg -n "opened_fd_numbers_snapshot|FilesState|FileDesc|CLONE_FILES|close_range" anemone-kernel/src/task anemone-kernel/src/fs` 确认 snapshot wrapper、`fd.rs` 的当前 fd 查询和既有 `CLONE_FILES` / `close_range` 路径；旁路审计未发现 fd table 语义被阶段 1 修改。`rg -n "/proc/self/fd|pipe07|getcwd02" docs/src anemone-apps/user-test` 命中 RFC / devlog / current-limitations 中的阶段目标、`anemone-apps/user-test/ltp/groups/fs.txt:getcwd02` 和 `anemone-apps/user-test/ltp/groups/pipe.txt:pipe07`；没有新的代码侧临时特判。

**Review:** Agent 4 复核阶段 1 主路径未发现开放 Keter / Euclid gate：`fd` 仍通过 `TGID_ENTRIES` 注册和枚举；动态 `fd/<n>` 只在 fd 目录 `child_ino` cache 中分配 synthetic identity；fd entry private data 只保存 `(ThreadGroupBinding, Fd)`；`readlink()`、`getattr()` 和阶段 2 未实现的 `open()` 入口均按操作重新执行 `validate_fd_access()` 和 `lookup_proc_fd()`；非 same-tgid 访问在 fd name 解析或 fd table 查询前返回 `SysError::AccessDenied`；root 外普通路径返回 `SysError::PermissionDenied`，不返回全局路径。

**Validation:** `git diff --check` passed。`just build` passed；构建过程中仍只有既有 `anemone-kernel/src/sync/mono.rs` 的 `AtomicBool` / `Ordering` unused import warning。当前工作区存在与本 RFC 无关的 `.agents/skills/anemone-build-system/SKILL.md`、`Justfile`、`scripts/xtask/**` 改动；本轮未修改这些文件，且它们没有导致 `just build` 失败。

**Not Run:** 未运行 QEMU、LTP、user-test 或手工 smoke；因此没有把 musl `getcwd02`、LTP `pipe07` 或 `/proc/self/fd` runtime smoke 记录为已通过。

**Remaining Limitations:** 阶段 2 fd entry open / magic-link follow 仍 deferred；跨进程权限仍是 `same-tgid only`；`fdinfo` 未实现；匿名对象显示名只有 pipe 和稳定 fallback；`O_PATH` 的完整 proc fd 可见性与权限边界仍属后续能力。

**Handoff:** 阶段 1 可在构建级别收口。除非后续验证显示 `open("/proc/<tgid>/fd/<n>")` 已成为新的直接阻塞项，否则不要启动 Agent 5。
