# `/proc/<tgid>/fd` Tracking Issues

**状态：** Active
**最后更新：** 2026-06-04
**父 RFC：** [RFC-20260604-proc-tgid-fd](./index.md)
**事务日志：** [2026-06-04-proc-tgid-fd](../../devlog/transactions/2026-06-04-proc-tgid-fd.md)

本文只跟踪 design review 后确认的 RFC 草案缺陷、证明缺口、边界冲突或需要回到草案修改的设计问题。

实现前已知缺口、当前基础设施状态、暂缓范围和阶段性交付项不写入本文；它们属于 [RFC index](./index.md) 的背景、非目标、风险，或 [迁移实施计划](./implementation.md) 的阶段内容。

## Apollyon

- 暂无。

## Keter

- 暂无。

## Euclid

- 暂无。

## Safe

- 暂无。

## Neutralized

### KETER-005：`fd` 目录需要 tgid entry 自定义 inode constructor / private data 扩展点

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的方案固定：`fd` 仍通过 `TGID_ENTRIES` 注册和枚举；`TgidEntry` 增加可选 custom inode constructor / private-data factory，默认 entry 继续创建 `TgidSubInodePrivate { binding }`，`fd` entry 创建 `ProcFdDirPrivate { binding, child_ino }`。
- [迁移实施计划](./implementation.md) 的阶段 1 交付与审计固定：先扩展 `TgidEntry` 构造边界，再通过同一静态 entry 表注册 `fd`；实现不得手写平行 `<tgid>` lookup，也不得把动态 fd number 引入 `TGID_ENTRIES` 或 `<tgid>` `sub_ino` key 空间。

**原问题：** 当前修复策略已经固定 `fd` 本身作为静态 `/proc/<tgid>` 子项注册在 `TGID_ENTRIES` 中，同时要求 `fd` 目录 inode 使用 `ProcFdDirPrivate { binding, child_ino }`。但当前 `TgidEntry::new_inode()` 会无条件为所有静态子项写入 `TgidSubInodePrivate { binding }`，没有给单个 entry 提供自定义 private data 或自定义 inode constructor 的扩展点。若实现阶段直接复用默认 `TgidEntry::new_inode()`，`fd` 目录拿不到自己的 child ino cache；若绕开 `TGID_ENTRIES` 手写特殊 lookup，又会破坏现有 `<tgid>` 静态子项注册和目录枚举形状。

**原违反的不变量：** `fd` 本身可以是静态 `/proc/<tgid>` entry，但它的目录 inode 必须拥有动态子项所需的本地状态；静态 tgid entry 框架不能强迫所有子项共享同一种 private data。

### EUCLID-003：`child_ino` cache 需要明确同步和锁序

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 的阶段 1 交付固定：`child_ino` 使用 `SpinLock<HashMap<Fd, SubInoRecord>>` 或等价本地受保护 map，只在 fd number 到 synthetic ino 的短临界区内使用。
- [迁移实施计划](./implementation.md) 的阶段 1 `read_dir()` / `lookup()` 交付和审计固定锁序：先在 fd table lock 下生成 `Vec<Fd>` 或验证当前 fd 存在，释放 fd table lock 后再短暂进入 child ino cache；持有 child ino cache lock 时不得重新进入 fd table、路径格式化或 VFS open/readlink。

**原问题：** 当前方案要求 `fd` 目录 private data 持有 `child_ino`，并在 `read_dir()` / `lookup()` 中为 fd number 分配或复用 synthetic ino。但文档还没有固定 `child_ino` 的并发保护方式，也没有说明它与 fd table snapshot 锁的顺序关系。实现者如果一边持 fd table read lock 一边分配 child ino，或者在持 child ino cache lock 时重新进入 fd table / VFS 路径，后续容易形成锁序反转。

**原违反的不变量：** fd table 当前状态、procfs synthetic identity cache 和 VFS 输出应分层访问；`child_ino` cache 不能把 fd table lock、procfs cache lock、VFS sink 输出绑成一个不可审计的临界区。

### KETER-003：动态 fd 子项不能复用静态 tgid entry / sub-ino 框架

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的方案固定：`fd` 本身可以作为静态 `/proc/<tgid>` 子项注册在 `TGID_ENTRIES` 中；动态 `fd/<n>` 子项使用独立 `ProcFdDirPrivate { binding, child_ino }` 与 `ProcFdEntryPrivate { binding, fd }`。
- [迁移实施计划](./implementation.md) 的阶段 1 交付与审计固定 fd-number synthetic ino 来自 `fd` 目录 private cache；cache 只表示 procfs child identity，fd 是否存在仍由当前 fd table 操作时重新验证。

**原问题：** 草案已经把 `/proc/<tgid>/fd` 拆成 `fd` 目录 inode 和 `fd/<n>` 子 inode，但还没有把它映射到当前 procfs 架构的真实形状。现有 `TgidEntry` / `TgidSubInodePrivate` 只适合 `cmdline`、`status` 这类静态 `/proc/<tgid>` 子项；`fd/<n>` 是无限动态子项，`read_dir()` 又必须为每个数字 fd 提供稳定的 synthetic ino。如果实现阶段把动态 fd number 塞进 `TGID_ENTRIES`、复用 `<tgid>` 目录的 `sub_ino` 记录，或者让目标文件 inode 成为 fd entry 的 inode identity，就会混淆静态 procfs 子项注册、动态 fd 子树缓存和 fd table 当前状态三层边界。

**原违反的不变量：** `/proc/<tgid>/fd` 目录只拥有 procfs synthetic child identity；fd 是否存在仍由目标 fd table 在操作时重新验证；静态 tgid entry 注册不能成为动态 fd 子项索引。

### KETER-004：fd number snapshot 必须明确避开 FilesState 内部双重真相源

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 的阶段 0 交付固定 `opened_fd_numbers_snapshot()` 的打开集合语义必须和 `get_fd(fd)` 一致。
- [迁移实施计划](./implementation.md) 的阶段 0 交付与审计固定：若 `FilesState` 仍同时维护 `bitmap` 与 `fds`，snapshot 以 `fds[i].is_some()` 为准，并对 bitmap 做轻量一致性校验；snapshot 不得把 `bitmap` 和 `fds` 当成两个可独立解释的来源。

**原问题：** 阶段 0 要新增 `opened_fd_numbers_snapshot() -> Vec<Fd>`，但当前 `FilesState` 内部已经同时维护 `bitmap` 和 `fds`，并在代码注释中承认这是双重真相源。若 snapshot 以 bitmap 为准，而 `get_fd(fd)` 以 `fds[fd]` 为准，`/proc/<tgid>/fd` 的 `readdir()` 可能列出一个后续 `readlink()` / `getattr()` 立即认为不存在的 fd；反过来也可能漏列真实存在的 fd。这个分叉会把已有内部维护风险暴露成 procfs 观察层的不稳定 ABI。

**原违反的不变量：** fd table 的当前打开集合只能有一个语义真相；procfs snapshot 和 fd entry 操作必须从同一语义来源观察 fd table，不能放大内部实现的双重真相。

### KETER-001：stage-1 跨进程权限和 root 外路径策略必须固定

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的目标、非目标、方案、接受边界和风险固定第一阶段为 `same-tgid only`，其他目标返回 `SysError::AccessDenied`，且 root 外路径不得 fallback 成全局路径。
- [迁移实施计划](./implementation.md) 的迁移原则、阶段 1 前置条件、`open/read_dir/lookup/readlink/getattr` 交付、审计和 smoke 验证同步固定该策略。

**原问题：** 草案计划暴露 `/proc/<tgid>/fd`，但把完整 ptrace / dumpable / namespace 权限后移，只写“保守 stage-1 策略”，没有定义第一阶段到底允许哪些目标。`readlink()` 对目标 leader root 不可见路径也留下“返回稳定错误或全局路径视图”的分支，这会让实现者在安全边界上自由选择，甚至可能通过 `/proc/<tgid>/fd/<n>` 泄露目标 root 之外的全局路径。

**原违反的不变量：** procfs fd 入口不能在权限基础设施缺位时默认扩大跨进程可见性；路径显示必须以目标 task root 视角为边界，root 外路径不得被 fallback 成全局路径泄露。

### KETER-002：fd entry dentry/inode 可能缓存，不能承诺 close 后 lookup 重新失败

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的方案与风险明确：fd entry 长期身份只能是 `(ThreadGroupBinding, Fd)`；VFS 可以缓存已经 materialize 的 dentry/inode，但 `readlink()`、`getattr()` 和未来 `open()` 必须操作时重新读取 fd table。
- [迁移实施计划](./implementation.md) 的阶段 1 交付、审计和验证改为 close 后 `readlink()` / `getattr()` 返回 `ENOENT`；如果 lookup 因 VFS 缓存拿到旧 inode，不作为失败。

**原问题：** 草案要求 `/proc/<tgid>/fd/<n>` 每次操作重新验证当前 fd table，这是正确方向；但实施计划同时写了 close 后对应 entry lookup 应失败。当前 VFS lookup 可能命中已经 materialize 的 child dentry，不一定重新调用目录 inode 的 `lookup()`。如果文档把 lookup 层精确性当成阶段退出条件，会推动实现者绕过现有 VFS 缓存模型，或者误以为缓存 inode 可以代表当前 fd 存在性。

**原违反的不变量：** fd entry 的长期身份只能是 `(ThreadGroupBinding, Fd)`；当前 fd 是否存在必须由 `readlink()`、`getattr()` 和未来 `open()` 在操作时重新读取 fd table 决定，不能由缓存 dentry/inode 代表。

### EUCLID-001：fd table 快照 API 不应返回 `Arc<FileDesc>` 作为目录枚举真相

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的方案改为 `opened_fd_numbers_snapshot() -> Vec<Fd>`，并明确 procfs 不把 `Arc<FileDesc>` 当作目录枚举或 fd entry 的长期真相。
- [迁移实施计划](./implementation.md) 的阶段 0 交付、审计和旁路审计清单都改为 fd number snapshot；fd entry 操作继续以 `Task::get_fd(fd)` 或等价只读入口最终验证。

**原问题：** 草案允许 `opened_fds_snapshot() -> Vec<(Fd, Arc<FileDesc>)>`。目录枚举只需要 fd number；如果快照返回 `Arc<FileDesc>`，容易让 `readdir()` 或 fd entry inode 把打开文件描述符当成缓存事实，削弱“每次操作重新验证当前 fd table”的边界。

### EUCLID-002：数字 fd 解析和 fd table 查询错误码需要在 procfs 边界映射

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的方案与风险明确 fd 不存在在 procfs 边界映射为 `SysError::NotFound`。
- [迁移实施计划](./implementation.md) 的迁移原则和阶段 1 交付固定：非数字、负号、空字符串、解析溢出、超出 `Fd::new()` 范围、未打开 fd 都返回 `SysError::NotFound`；目标 thread group 不存在仍返回 `SysError::NoSuchProcess`。
- [迁移实施计划](./implementation.md) 的验证新增对应 `ENOENT` smoke。

**原问题：** `Fd::new()` 只表达 raw fd 类型范围，`FilesState::get_fd()` 对越界和未打开 fd 通常返回 bad-fd 语义；但 `/proc/<tgid>/fd/<n>` 是路径 lookup / symlink readlink 入口，用户可见行为应更接近路径不存在，而不是泄露 fd syscall 层的 EBADF 语义。草案原先只写“`ENOENT` / `EBADF` 边界由实现阶段固定”，不足以作为实现 gate。
