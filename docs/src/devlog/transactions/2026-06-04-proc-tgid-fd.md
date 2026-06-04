# 2026-06-04 - PROC TGID FD

**Status:** Active
**Owners:** doruche, Codex
**Area:** procfs / fd / path visibility / LTP
**RFC:** [RFC-20260604-proc-tgid-fd](../../rfcs/proc-tgid-fd/index.md)
**Current Phase:** RFC promoted; implementation pending

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

**Completed:** `proc-tgid-fd` 已提升为公开目录级 RFC；design review 发现的 Keter / Euclid 项已吸收到 RFC 主文档与迁移实施计划，并保留在 Tracking Issues 的 Neutralized 区段。本事务日志、事务索引、双周 devlog 和 mdBook Summary 已建立链接。

**In Progress:** 实现尚未开始。

**Open Blockers:** 暂无已确认 blocker。

**Next Action:** 按实施计划进入阶段 0：新增 fd number 只读快照 API，并确认 snapshot 与 `get_fd(fd)` 使用同一打开集合语义；通过后进入阶段 1 实现 `/proc/<tgid>/fd` 目录与 fd symlink。

**Do Not Redo:** 不要用 `/proc/self/fd/<n>` 字符串特判绕过 procfs/VFS inode；不要把动态 fd number 塞进 `TGID_ENTRIES` 或 `<tgid>` 目录 `sub_ino` 静态子项表；不要缓存 `Arc<FileDesc>` 作为 fd entry 真相；不要在权限基础设施缺位时扩大跨进程可见性；不要返回目标 root 外的全局路径字符串。

## Phase Log

### 2026-06-04 - RFC 提升与事务日志启动

**Phase:** docs / RFC promotion

**Change:** 新增公开 [RFC-20260604-proc-tgid-fd](../../rfcs/proc-tgid-fd/index.md)，目录包含 [迁移实施计划](../../rfcs/proc-tgid-fd/implementation.md) 和 [Tracking Issues](../../rfcs/proc-tgid-fd/tracking-issues.md)。

**Review:** 本轮只做文档提升。草案中的 Keter / Euclid review 项已经折回 RFC 主文档和实施计划；Tracking Issues 当前没有开放的 Apollyon / Keter / Euclid / Safe 项，只保留 Neutralized 记录。

**Validation:** 文档结构更新；未修改生产代码，未运行构建、QEMU 或 LTP。

**Next:** 从阶段 0 开始实现 fd table 只读观察接口；阶段 1 再落地 `/proc/<tgid>/fd` 目录、lookup、`readlink()` 和 `getattr()`。
