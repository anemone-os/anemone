# `/proc/<tgid>/fd` 迁移实施计划

**状态：** Draft
**最后更新：** 2026-06-04
**父 RFC：** [RFC-20260604-proc-tgid-fd](./index.md)
**不变量：** None；本改动按小型 procfs/fd 兼容入口处理，关键边界写在本文和父草案中。

本文按小阶段拆分。第一阶段应是主要实现范围；第二阶段只在第一阶段验证显示 `open` 语义已经成为新的直接阻塞项时推进。

## 迁移原则

- 复用现有 `/proc/<tgid>` binding、sub-inode、file ops 和 symlink 模式。
- `fd` 本身作为静态 `/proc/<tgid>` 子项注册；`TgidEntry` 需要支持少数 entry 使用 custom inode constructor 或 private-data factory，默认 entry 继续创建 `TgidSubInodePrivate { binding }`，`fd` entry 创建自己的 `ProcFdDirPrivate { binding, child_ino }`。
- 动态 `fd/<n>` 子项身份由 `fd` 目录自己的 private data 管理，不进入 `<tgid>` 静态 `sub_ino` 表。
- procfs fd 目录只观察 fd table，不拥有 fd table。
- 不缓存 `Arc<FileDesc>` 作为 fd entry 的长期真相；每次操作按 `(tgid, fd)` 重新查询。
- fd table 枚举通过 `FilesState` 的 fd number 只读快照 API 完成，不直接暴露内部 `fds` / `bitmap`。
- 目录枚举是 best-effort 快照，不承诺 close/dup 并发下的强一致。
- 第一阶段先支撑 `readdir`、`readlink`、`getattr`；完整 magic-link open 后移。
- 当前 task 对 `/proc/self/fd` 的访问是第一阶段主目标；跨进程权限先固定为 `same-tgid only`，其他目标返回 `SysError::AccessDenied`。
- 普通路径对象只能按目标 leader root 视角显示；root 外路径返回权限错误，不返回全局路径。
- procfs fd 边界把 fd syscall 的 `BadFileDescriptor` 映射成路径语义的 `NotFound`。
- 每个阶段都保持 `CLONE_FILES`、fork fd table clone、dup/close、`close_range` 语义不变。

## 阶段 0：fd table 只读观察接口

目标：给 procfs 一个稳定、窄的 fd table 观察入口。

前置条件：

- 确认 `FilesState` 仍是 fd table 的唯一存储。
- 确认 `Task::files_state()` 返回共享 fd table handle，`CLONE_FILES` 和 `close_range(UNSHARE)` 已按当前实现保留。

交付：

- 在 `FilesState` 增加只读快照方法，例如：
  - `opened_fd_numbers_snapshot(&self) -> Vec<Fd>`
  - 若后续保留包含 `Arc<FileDesc>` 的内部 helper，它不得作为 procfs fd 目录枚举或 entry 缓存的真相源。
- 在 `Task` 上增加可选 wrapper，例如 `opened_fd_numbers_snapshot()`，避免 procfs 直接展开 `files_state().read()` 调用模式。
- 快照按 fd number 递增排序，方便 `readdir()` 游标使用。
- 快照的打开集合语义必须和 `get_fd(fd)` 一致；若当前 `FilesState` 仍同时维护 `bitmap` 与 `fds`，snapshot 以 `fds[i].is_some()` 为准，并对 bitmap 做轻量一致性校验，或先收敛内部双重真相源。
- 保持 `get_fd(fd)` 作为 fd entry 操作的最终验证入口。

审计：

- 快照不返回内部可变引用。
- procfs `read_dir()` 只消费 fd number 快照；不得保存快照里的 `Arc<FileDesc>`。
- 快照过程中不调用 VFS、procfs 或可能阻塞的路径。
- snapshot 不得把 `bitmap` 和 `fds` 当成两个可独立解释的来源；发现两者不一致时应触发普通运行可见的轻量 invariant 检查。
- 不改变 `FilesState::fork()`、`close_fd()`、`dup*()`、`close_range()`。

验证：

- `just build`
- 若有 KUnit 习惯，可补一个小型 fd snapshot 单测；否则源码审计即可。

退出条件：

- procfs 可以只通过公开只读 API 枚举 fd。

## 阶段 1：`/proc/<tgid>/fd` 目录与 fd symlink

目标：落地已知 LTP/libc 需要的最小功能：目录枚举、lookup、`readlink` 和 `getattr`。

前置条件：

- 阶段 0 完成。
- 现有 `/proc/<tgid>` 子项注册方式确认可加入 `fd`。
- 固定第一阶段访问策略：目标 tgid 必须等于当前 task 所属 tgid；跨进程访问不做 same uid、dumpable、ptrace 或 namespace 判断。
- 固定第一阶段路径显示策略：普通路径只允许 `leader.rel_abs_path()` 成功后的目标 root 内路径；失败返回 `SysError::PermissionDenied`。

交付：

- 扩展 `TgidEntry` 的 inode 构造边界：
  - 保留 `TGID_ENTRIES` 作为 `/proc/<tgid>` 静态子项的唯一注册表；`fd` 不创建平行 lookup 或平行 readdir 入口。
  - 给 `TgidEntry` 增加可选 custom inode constructor / private-data factory，签名应只接收 `Arc<ThreadGroupBinding>`、`Arc<SuperBlock>`、可选 `Ino` 和 entry 元数据所需的最小上下文。
  - 默认 constructor 继续生成 `TgidSubInodePrivate { binding }`，并复用当前 `TgidEntry::new_inode()` 的 mode、perm、uid/gid、nlink 和时间戳初始化语义。
  - `tgid_lookup()` 和 `<tgid>` `read_dir()` 仍只通过 `TGID_ENTRIES` 发现静态子项；`read_dir()` 可以继续只预分配静态 entry 的 ino，真正 inode materialization 发生在 lookup 或 iget 路径。
  - `fd` entry 在 `TGID_ENTRIES` 中注册为目录 entry，但其 constructor 创建 `ProcFdDirPrivate { binding, child_ino }`，并绑定 `fd` 目录专用 inode/file ops。
  - 普通 `cmdline`、`status`、`cwd`、`exe`、`mounts` 等 entry 不需要知道 `ProcFdDirPrivate`，也不得被迫改成 fd 专用 private data。
- 新增 `anemone-kernel/src/fs/proc/tgid/fd.rs` 或等价模块。
- 在 `TGID_ENTRIES` 中注册 `fd`：
  - name: `"fd"`
  - type: directory
  - perm: `r-x` 或沿用当前 procfs stage-1 的 root-readable 策略；需与现有 `cwd/exe/status` 风格一致。
- `fd` 目录 inode private 持有现有 `ThreadGroupBinding` 和本目录自己的 child ino cache。
- `fd` 目录和 `fd/<n>` entry 使用独立 private data，例如：
  - `ProcFdDirPrivate { binding, child_ino }`
  - `ProcFdEntryPrivate { binding, fd }`
  - `child_ino` 使用 `SpinLock<HashMap<Fd, SubInoRecord>>` 或等价本地受保护 map，按 fd number 缓存 `SubInoRecord` 或等价记录，只表示 procfs synthetic child identity。
  - `fd/<n>` 不进入 `TGID_ENTRIES` / `<tgid>` 目录 `sub_ino` 静态子项表，也不复用目标文件 inode 作为 fd entry identity。
- 在 `fd.rs` 内部提供窄 helper，并让所有入口复用：
  - `validate_fd_access(binding)`：验证 binding alive、leader 存在和 `same-tgid`，且权限失败时不读取目标 fd table。
  - `parse_proc_fd_name(name)`：集中处理非数字、负号、空字符串、解析溢出和 `Fd::new()` 范围。
  - `lookup_proc_fd(leader, fd)`：集中执行当前 fd table 查询，并把 fd syscall 的 `BadFileDescriptor` 映射为 procfs 路径语义的 `NotFound`。
  - `proc_fd_child_ino(fd)`：在 `fd` 目录 private cache 中为 fd number 分配或复用 synthetic ino。
- `fd` 目录 `open()`：
  - 重新验证 binding alive。
  - 检查 `binding.tg.tgid() == current_task.get_thread_group().tgid()`。
  - 非 `same-tgid` 返回 `SysError::AccessDenied`，不读取目标 fd table。
  - 通过后返回目录 file ops。
- `fd` 目录 `read_dir()`：
  - 重新验证 binding alive。
  - 先执行 `same-tgid` 检查；失败返回 `SysError::AccessDenied`。
  - 取目标 leader；leader 不存在返回 `ESRCH` 对应内部错误。
  - 在 fd table lock 下只生成 leader 当前 fd number 快照 `Vec<Fd>`，随后释放 fd table lock。
  - 对快照中的每个 fd number，短暂进入 `fd` 目录 private cache 取得或分配 synthetic ino；不得在持有 child ino cache lock 时重新进入 fd table、路径格式化或 VFS open/readlink。
  - cache 中保留已关闭 fd 的旧记录不影响枚举，因为枚举来源只能是当前 snapshot。
  - 输出数字 fd name，`d_type = DT_LNK`。
  - 建议显式输出 `.` / `..`，若当前 procfs 子目录模式没有统一输出，也可保持与现有目录行为一致，但需要在实现注释中说明。
- `fd` 目录 `lookup(name)`：
  - 重新验证 binding alive。
  - 先执行 `same-tgid` 检查；失败返回 `SysError::AccessDenied`，不解析 fd number，也不查询目标 fd table。
  - 只接受十进制非负 fd number。
  - 非数字、负号、空字符串、解析溢出、超出 `Fd::new()` 范围都返回 `SysError::NotFound`。
  - `Task::get_fd(fd)` 返回 `BadFileDescriptor` 时映射为 `SysError::NotFound`。
  - 权限检查、leader 获取、fd 解析和当前 fd 存在性验证期间不持有 child ino cache lock。
  - 仅在确认 fd 当前存在后，短暂进入 `fd` 目录 private cache 取得或分配 fd number synthetic ino，创建或返回绑定 `(ThreadGroupBinding, Fd)` 的 symlink inode。
- `fd/<n>` symlink inode：
  - inode private 只保存 `ThreadGroupBinding` 和 `Fd`，不得保存 `Arc<FileDesc>`。
  - `readlink()` 每次重新验证 binding alive、`same-tgid` 和当前 fd 存在。
  - `getattr()` 每次重新验证 binding alive、`same-tgid` 和当前 fd 存在，mode 暴露为 symlink。
  - fd 当前不存在时，把 `Task::get_fd(fd)` 的 `BadFileDescriptor` 映射为 `SysError::NotFound`。
  - `open()` 第一阶段可以返回 `NotSupported` / `NotSymlink` 风格错误；如果 namei 对 symlink follow 会进入 readlink 字符串解析，则先依赖普通 symlink follow，不宣称 magic-link open 完整。
- 普通路径对象 `readlink()`：
  - 从 `FileDesc::vfs_file().path()` 取 `PathRef`。
  - 用目标 leader 的 root 视角格式化为绝对路径。
  - 若无法转成目标 root 下路径，返回 `SysError::PermissionDenied` 或同等权限错误；不得返回全局路径字符串。
- 路径显示和对象显示名必须通过 owner subsystem 的窄 API 取得：
  - 普通路径 root 视角格式化复用现有 task/fs path helper 或抽出共享 helper，不在 `fd.rs` 复制另一套 root 外路径判断。
  - pipe 显示名由 `fs::pipe` 暴露只读 helper 或等价 owner API 生成；`fd.rs` 不直接依赖 pipe private data。
  - 其他匿名对象显示名后续按对象 owner 提供的只读接口扩展；`fd.rs` 只负责编排，不拥有各对象类型的命名策略。
- pipe fd `readlink()`：
  - 返回 `pipe:[ino]`。
  - inode number 来自 `file.path().inode().ino()`。
- 其他匿名对象：
  - 若当前阶段没有对象类型，可不特殊处理。
  - 若遇到不可分类对象，返回稳定 `anon_inode:[anemone-<ino>]` 或等价格式，并记录限制。

审计：

- fd entry inode 不长期保存 `Arc<FileDesc>`。
- `fd` 目录必须仍由 `TGID_ENTRIES` 注册和枚举；实现不得为了拿到 `ProcFdDirPrivate` 而在 `<tgid>` lookup 中手写绕过静态 entry 表的特殊路径。
- `TgidEntry` 的扩展点必须是默认 constructor 的窄扩展，不得把 `fd/<n>` 的动态 fd number 引入 `TGID_ENTRIES` 或 `<tgid>` `sub_ino` key 空间。
- fd 目录和 fd entry 必须使用独立 procfs private data；`fd` 本身可以进入静态 `TGID_ENTRIES`，但动态 fd number 只能进入 `fd` 目录自己的 child ino cache，不得塞进 `<tgid>` `sub_ino` 缓存，也不得让目标文件 inode 成为 fd entry identity。
- fd-number child ino cache 只提供稳定 synthetic identity；fd 是否存在仍以当前 fd table 查询为准。
- fd table snapshot lock、child ino cache lock 和 VFS/path 操作不得嵌套成不可审计链条：`read_dir()` 先取 `Vec<Fd>` 再分配 ino 再输出；`lookup()` 先验证当前 fd 存在，再短暂分配 ino；`readlink()` / `getattr()` 不需要进入 child ino cache。
- `same-tgid`、binding alive、leader 获取、fd name 解析和 `BadFileDescriptor` 到 `NotFound` 的映射必须集中在 `fd.rs` 内部 helper 中，避免 `open/read_dir/lookup/readlink/getattr` 各自复制安全检查顺序。
- `read_dir()` 不在持有 fd table read lock 时调用 `sink.push()` 之外的复杂 VFS 路径；先生成 `Vec<Fd>` 再输出。
- 已经缓存的 `fd/<n>` dentry/inode 可以继续存在，但不能证明 fd 当前仍打开。
- close 后 `readlink("/proc/self/fd/<closed>")` / `getattr("/proc/self/fd/<closed>")` 不应继续成功，应返回 `SysError::NotFound`。
- dup 后两个 fd 可以指向同一 `ProcFile`，但目录项仍按 fd number 分开。
- `/proc/self/fd` 通过现有 `/proc/self` symlink 正常解析，不需要单独入口。
- `O_PATH` fd 的 `readlink()` 仍能取到 path-only `File` 的 `PathRef`。
- root 外普通路径不得 fallback 成全局路径。
- `fd.rs` 不直接拥有各文件对象的显示名策略；普通 path、pipe 和后续匿名对象应通过各 owner subsystem 的只读 helper 暴露显示字符串。
- 非 `same-tgid` 访问不得通过错误码或路径字符串泄露目标 fd 是否存在。

验证：

- `just build`
- 手工或用户态 smoke：
  - `getdents64(open("/proc/self/fd"))` 能看到 `0`、`1`、`2` 和测试打开的 fd。
  - `readlink("/proc/self/fd/<regular-fd>")` 返回预期路径。
  - `readlink("/proc/self/fd/<dir-fd>")` 返回预期目录路径。
  - `pipe2()` 后两个 fd 的 `readlink()` 返回稳定 `pipe:[ino]`。
  - close fd 后对应 entry 的 `readlink()` / `getattr()` 失败为 `ENOENT`；如果 VFS 缓存导致 lookup 仍能拿到旧 inode，不作为失败。
  - 非数字、负数、超范围和未打开 fd entry 返回 `ENOENT`。
  - 尝试访问非当前 tgid 的 `/proc/<tgid>/fd` 返回 `EACCES`，不泄露 fd 存在性。
  - 对目标 root 外路径执行 `readlink()` 返回权限错误，不返回全局路径。
- 目标 LTP：
  - musl `getcwd02`
  - `pipe07`

退出条件：

- `/proc/self/fd` 目录和 fd symlink 对当前 task 可用。
- `ANE-20260528-PROC-TGID-FD-FRAMEWORK-PENDING` 可以从“目录缺失”收口为 residual limitations。
- `tracking-issues.md` 没有剩余 Open Keter / Euclid gate。

## 阶段 2：fd entry open 与 magic-link follow（可选 follow-up）

目标：在第一阶段之后补齐 `open("/proc/<tgid>/fd/<n>")` 的更真实语义。

前置条件：

- 阶段 1 完成。
- 已确认有新的 LTP/libc 阻塞项依赖 fd entry open，而不仅是 `readlink` / `readdir`。

交付：

- 定义 `/proc/<tgid>/fd/<n>` open 策略：
  - 对 `/proc/self/fd/<n>`，可以从当前 fd 的 `PathRef` 重新按 requested flags 打开。
  - 若要复用目标 open file description，则需要更明确的 magic-link helper，不能用普通 symlink 字符串解析伪装。
- 固定 `O_PATH`、`O_NOFOLLOW`、directory fd、write access、truncate、append、nonblock 等边界。
- 明确 fd entry open 的权限策略；第一版沿用 `same-tgid only`，其他进程返回 `EACCES`。
- 若实现普通 symlink follow 到路径字符串，必须在文档中明确它不是完整 Linux magic-link 语义。

审计：

- 不绕过 VFS 权限检查。
- 不让 `/proc/<tgid>/fd/<n>` open 误修改目标 fd 的 file status flags。
- 不把 target fd number 当成 current task fd number。
- 不引入 fd table 锁和 VFS open 的锁序反转。

验证：

- `just build`
- `open("/proc/self/fd/<regular-fd>", O_RDONLY)` smoke。
- `open("/proc/self/fd/<dir-fd>", O_DIRECTORY)` smoke。
- `open("/proc/self/fd/<closed>")` 失败。

退出条件：

- fd entry open 的阶段性语义可以写入 current limitations 或从 residual limitations 中移除。

## 阶段 3：权限、`fdinfo` 与显示名精度（长期 follow-up）

目标：补齐完整 procfs fd 子树的后续兼容面。

候选交付：

- `/proc/<tgid>/fdinfo/<n>`：
  - `pos`
  - `flags`
  - `mnt_id` 如果 mount id 基础设施存在
  - pipe/eventfd/timerfd/fanotify 等专用字段按对象能力补齐
- 跨进程权限：
  - self 访问
  - same uid / dumpable / ptrace-style check
  - namespace/root 可见性
- deleted 文件显示后缀。
- socket、eventfd、timerfd、pidfd、fanotify fd 等匿名对象的 Linux-like target string。
- 与 `O_PATH` 后续能力、`fchdir`、metadata mutation、ioctl 边界联动收口。

验证：

- 后续按触发用例选择，不作为本次小改动验收范围。

## 旁路审计清单

- `rg -n "TODO: fd|TGID_ENTRIES|proc-tgid-fd|fdinfo" anemone-kernel/src/fs/proc`
- `rg -n "opened_fd_numbers_snapshot|FilesState|FileDesc|CLONE_FILES|close_range" anemone-kernel/src/task anemone-kernel/src/fs`
- `rg -n "/proc/self/fd|pipe07|getcwd02" docs/src anemone-apps/user-test`

## 收口动作

第一阶段实现完成后：

- 更新 `docs/src/register/current-limitations.md`：
  - 关闭 `/proc/<tgid>/fd` 目录缺失。
  - 保留完整 magic-link open、跨进程权限、`fdinfo`、匿名对象精确显示名和 `O_PATH` 后续能力。
- 在双周 devlog 或事务日志中记录：
  - `just build` 结果。
  - musl `getcwd02` 结果。
  - `pipe07` 结果。
  - 剩余失败分类。
