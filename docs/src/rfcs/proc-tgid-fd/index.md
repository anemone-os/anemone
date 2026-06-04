# RFC-20260604-proc-tgid-fd

**状态：** Draft，已提升为公开 RFC 草案
**负责人：** doruche, Codex
**最后更新：** 2026-06-04
**领域：** procfs / fd / path visibility / LTP
**事务日志：** [PROC TGID FD 事务日志](../../devlog/transactions/2026-06-04-proc-tgid-fd.md)
**开放问题：** None；本轮 design review 问题已折回方案与实施计划，详见 [Tracking Issues](./tracking-issues.md) 的 Neutralized。
**下一步：** 按第一阶段 gate 实现 `readdir`、`lookup`、`readlink` 和 `getattr`，暂不推进 magic-link open。

## 摘要

本草案计划为 Anemone 引入 `/proc/<tgid>/fd` 目录。目标不是一次性复制 Linux `proc_pid_fd` 的完整 magic-link 语义，而是先补齐 LTP 和 libc 当前阻塞的最小可观察入口：`/proc/self/fd` 目录枚举、`/proc/self/fd/<n>` 存在性、`readlink()` 目标字符串和基本 `stat`。

第一阶段应解决已经登记的 musl `getcwd02` 依赖 `readlink("/proc/self/fd/<n>")` 返回真实路径，以及 LTP `pipe07` 依赖 `/proc/self/fd` 目录枚举的问题。完整 `open("/proc/<tgid>/fd/<n>")` 重新打开目标 open file description、ptrace 风格跨进程权限、`fdinfo` 和匿名对象精确显示名后移。

## 背景

当前 procfs 已有动态 `/proc/<tgid>` 框架，并已经支持 `cmdline`、`environ`、`stat`、`status`、`cwd`、`root`、`exe`、`mounts` 等条目。`/proc/self` 已作为 symlink 指向当前 tgid，因此 `/proc/<tgid>/fd` 落地后 `/proc/self/fd` 不需要单独实现。

fd 模型也已经具备基础：

- `Task::files_state()` 暴露当前 task 持有的共享 fd table handle。
- `CLONE_FILES` 共享 fd table，`close_range(CLOSE_RANGE_UNSHARE)` 可替换当前 task 的 fd table handle。
- `FileDesc -> ProcFile -> File -> PathRef` 能拿到打开对象、access mode、fd flags、file status flags 和路径身份。
- `getdents64` 已经通过 `DirSink` 消费 VFS `read_dir()`。
- procfs 里已有 `cwd`、`root`、`exe` 这类动态 symlink 的实现模式。

当前限制登记为 `ANE-20260528-PROC-TGID-FD-FRAMEWORK-PENDING`：缺少 `/proc/<tgid>/fd` 会让 musl `getcwd02` 和 LTP `pipe07` 等用例失败。本草案就是该限制的最小系统性收口计划。

## 目标

- 在 `/proc/<tgid>` 下新增 `fd` 目录。
- 为目标 thread group 的 leader fd table 提供只读快照枚举。
- 支持 `/proc/<tgid>/fd` 的 `readdir()`，列出当前打开的 fd number。
- 支持 `/proc/<tgid>/fd/<n>` 的 lookup、`readlink()` 和基本 `getattr()`。
- 对普通路径对象返回目标 task root 视角下的可见路径。
- 对匿名 pipe 等非路径对象给出稳定 stage-1 伪目标字符串。
- 让 `/proc/self/fd` 通过现有 `/proc/self -> <tgid>` symlink 自然工作。
- 第一阶段只允许当前 thread group 访问自己的 fd 目录；其他目标返回稳定权限错误。
- 用明确限制保留完整 Linux magic-link open、跨进程 ptrace/dumpable/namespace 权限和 `fdinfo`。

## 非目标

- 不在第一阶段实现 `/proc/<tgid>/fd/<n>` 的完整 magic-link open 语义。
- 不在第一阶段实现 `/proc/<tgid>/fdinfo/<n>`。
- 不在第一阶段实现 ptrace / dumpable / namespace 风格的完整跨进程权限检查；第一阶段以 `same-tgid only` 作为保守访问策略。
- 不承诺匿名对象显示名完全等价 Linux；`pipe:[ino]` 可以作为 stage-1 目标，socket、eventfd、pidfd 等后续对象另行补齐。
- 不改变 fd table 的共享、fork、close、dup 或 `close_range` 语义。
- 不把 fd 目录实现为 ad-hoc path 字符串重写；必须挂在 procfs/VFS inode/file ops 框架内。

## 文档地图

Canonical：

- [迁移实施计划](./implementation.md)
- [Tracking Issues](./tracking-issues.md)

背景材料：

- [当前限制：PROC-TGID-FD-FRAMEWORK-PENDING](../../register/current-limitations.md#ane-20260528-proc-tgid-fd-framework-pending)
- [Agent 编排建议](./backgrounds/agent-orchestration.md)

## 方案

新增 `fs::proc::tgid::fd` 模块，包含两个层次：

1. `fd` 目录 inode：绑定现有 `ThreadGroupBinding`，负责 lookup 数字 fd、open 目录和 `readdir()`，并持有本目录私有的 fd-number synthetic ino cache。
2. `fd/<n>` 子 inode：绑定 `ThreadGroupBinding + Fd`，每次 `readlink()` / `getattr()` 时重新验证 thread group alive，并从目标 leader 的当前 fd table 取 fd。

`fd` 本身继续作为静态 `/proc/<tgid>` 子项注册在 `TGID_ENTRIES` 中，但 `TGID_ENTRIES` 只负责声明名字、mode、ops 和静态目录枚举身份，不负责承载 `fd/<n>` 动态子项。为支持这类拥有内部动态子树的静态 entry，`TgidEntry` 需要增加一个最小扩展点：可选 custom inode constructor 或 private-data factory。默认 entry 仍按现有路径创建 `TgidSubInodePrivate { binding }`；`fd` entry 使用专用 constructor 创建 `ProcFdDirPrivate { binding, child_ino }`，并保留与 `TGID_ENTRIES` 一致的名字注册、`read_dir()` 枚举和 `<tgid>` `sub_ino` 分配行为。

动态 `fd/<n>` 子项不得复用 `<tgid>` 目录的 `sub_ino` 静态子项表。`fd` 目录的 `child_ino` 按 fd number 分配并缓存 procfs synthetic ino。`fd/<n>` inode 使用 `ProcFdEntryPrivate { binding, fd }`。这些 ino 只表示 procfs 目录项身份，不表示目标 fd 当前仍打开，也不得复用目标文件 inode。

`FilesState` 增加只读 fd number 快照 API，例如 `opened_fd_numbers_snapshot() -> Vec<Fd>`。procfs 只通过该 API 枚举 fd，不直接访问 `FilesState` 内部 `fds` / `bitmap`，也不把 `Arc<FileDesc>` 当作目录枚举或 fd entry 的长期真相。

第一阶段权限策略固定为 `same-tgid only`：目标 tgid 必须等于当前 task 的 tgid，否则 `fd` 目录 `open()`、`read_dir()`、`lookup()` 以及 `fd/<n>` 的 `readlink()` / `getattr()` 都返回 `SysError::AccessDenied`，且不得先枚举目标 fd table 或格式化目标路径。

普通文件、目录和 symlink fd 的 `readlink` 目标基于 `file_desc.vfs_file().path().to_pathbuf()`。若该路径位于目标 leader root 下，则用 `leader.rel_abs_path()` 转成目标进程视角的绝对路径；若不可见，则沿用 `/proc/<tgid>/exe` 的非泄露方向返回 `SysError::PermissionDenied` 或等价权限错误，不返回全局路径视图。

`fd/<n>` inode 的长期身份只能是 `(ThreadGroupBinding, Fd)`。已经 materialize 的 dentry/inode 可以被 VFS 缓存，但缓存存在不代表 fd 当前仍打开；`readlink()`、`getattr()` 和未来 `open()` 必须在操作时重新读取目标 fd table，并把 fd 不存在映射成 `SysError::NotFound`。

匿名 pipe 的 `readlink` 第一阶段返回 `pipe:[ino]`。其他匿名对象如果暂时不存在可不扩展；若已有对象无法分类，则返回 `anon_inode:[anemone-<ino>]` 或类似稳定格式，并在当前限制中保留精确 Linux 名称差异。

## 接受边界

接受本草案意味着 `/proc/<tgid>/fd` 可以作为小型 procfs/fd 兼容改动推进。第一阶段完成标准是目录枚举和 `readlink` 可支撑已知 LTP/libc 依赖，不是完整 Linux procfs fd 子树。

以下变化必须回到本草案或新增 follow-up：

- 把 `open("/proc/<tgid>/fd/<n>")` 完整 magic-link 语义并入第一阶段验收。
- 引入跨进程 ptrace/dumpable/namespace 权限策略，或放宽第一阶段 `same-tgid only` 访问策略。
- 新增 `/proc/<tgid>/fdinfo`。
- 改变 fd table 生命周期、`CLONE_FILES`、dup/close 或 open file description 共享语义。
- 让 procfs fd 目录缓存 stale `Arc<FileDesc>` 而不是按操作重新验证当前 fd table。
- 让 root 外路径 fallback 成全局路径字符串。

## 备选方案

### 只特殊处理 `/proc/self/fd/<n>` 字符串

拒绝。这样可以临时推进 musl `getcwd02`，但会绕过 procfs inode、`getdents64` 和 `/proc/<tgid>` binding 生命周期，不能解决 `pipe07` 的目录枚举，也会为后续 `fdinfo` 和 open 语义制造第二套入口。

### 一次性实现完整 Linux magic-link

延期。完整语义涉及重新打开目标 open file description、权限、namespace、deleted 文件显示、匿名对象命名和 `fdinfo`。当前 LTP 阻塞点不需要第一批全部完成。

### fd 目录长期缓存 `FileDesc`

拒绝。`/proc/<tgid>/fd/<n>` 应观察当前 fd table，而不是 lookup 时的旧 fd。缓存 fd number 和 binding 可以，具体 `FileDesc` 应在每次操作时重新读取。

## 风险

- `readdir()` 与 close/dup 并发时可能看到 best-effort 快照。控制方式是用 fd table read lock 生成快照，目录游标只表示枚举位置，不承诺强一致。
- `readlink()` 对 chroot/root 不可见路径的处理可能与 Linux 不完全一致。控制方式是优先使用目标 leader 视角，并把不可见路径作为 stage-1 限制记录。
- 匿名对象显示名可能影响 LTP 字符串判断。控制方式是先覆盖 pipe 的 `pipe:[ino]`，其他对象按稳定格式后续扩展。
- 如果 `fd/<n>` inode 缓存过强，close 后仍可能错误存在。控制方式是只缓存 `(ThreadGroupBinding, Fd)`，每次 `readlink/getattr/open` 都重新 `get_fd()`，并在 procfs 边界把 fd 不存在映射为 `SysError::NotFound`。
- `fd` 目录的 fd-number ino cache 可能保留已经关闭过的 fd number。控制方式是把该 cache 限定为 synthetic identity cache；`read_dir()` 只枚举当前 fd snapshot，`lookup()`、`readlink()` 和 `getattr()` 都重新验证当前 fd table。

## 收口

实现完成后应更新 `docs/src/register/current-limitations.md` 中 `ANE-20260528-PROC-TGID-FD-FRAMEWORK-PENDING`：关闭 `/proc/<tgid>/fd` 缺失本身，并保留 residual limitations，包括完整 magic-link open、跨进程权限、`fdinfo`、匿名对象精确命名和 `O_PATH` 后续能力。

验证至少包括：

- `just build`
- `/proc/self/fd` `getdents64()` 能列出 `0`、`1`、`2` 和测试打开的临时 fd。
- `readlink("/proc/self/fd/<regular-fd>")` 返回目标进程视角路径。
- `readlink("/proc/self/fd/<pipe-fd>")` 返回稳定 pipe 伪目标。
- musl `getcwd02` 和 LTP `pipe07` 重新分类或通过。
