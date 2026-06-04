# PROC TGID FD Agent 编排建议

本文记录 `proc-tgid-fd` 进入实现阶段时的 agent 编排方式。Canonical 协议仍以
[RFC 入口](../index.md)、[迁移实施计划](../implementation.md) 和
[Tracking Issues](../tracking-issues.md) 为准；本文只说明如何按这些 gate 组织
worker、reviewer 和验收顺序。

本轮主要关注阶段 1 和阶段 2。阶段 1 是主交付范围：`/proc/<tgid>/fd` 目录、
数字 fd lookup、`readlink()` 和 `getattr()`。阶段 2 只在阶段 1 后确认
`open("/proc/<tgid>/fd/<n>")` 已成为新的直接阻塞项时推进。

## 编排原则

1. 不按“一个文档阶段一个 agent”机械拆分。拆分边界应对应 fd table 观察入口、
   procfs tgid entry 扩展点、fd 子树生产路径和 magic-link open 的验收边界。
2. 阶段 0 的 fd number snapshot 是阶段 1 的前置补丁，可以与前置审计合并推进，
   但必须先过 review，再让 procfs 消费它。
3. 阶段 1 建议拆成两个写入型 worker：一个负责 tgid entry / fd 目录框架，
   一个负责 fd symlink `readlink()` / `getattr()` 显示策略。两者通过共享 helper
   边界衔接，不互相复制权限和 fd 查询逻辑。
4. 阶段 2 不与阶段 1 混写。只要 `readdir` / `lookup` / `readlink` / `getattr`
   能支撑 `getcwd02` 和 `pipe07`，总控就应先收口阶段 1。
5. review agent 只放在有意义的 gate 上，不在每个小补丁后立即审查。
6. 写入型 worker 只改自己的 write set；遇到必须越界的依赖，停止并回报总控。
7. 每个实现阶段退出都要更新事务日志：
   [2026-06-04 - PROC TGID FD](../../../devlog/transactions/2026-06-04-proc-tgid-fd.md)。
8. `just build` 是最低构建 gate，不能替代 procfs/fd 语义审计。
9. LTP/QEMU 默认由用户验证，除非用户后续明确授权 agent 运行。
10. 第一阶段权限固定为 `same-tgid only`；任何 worker 都不得顺手实现或放宽
    ptrace / dumpable / namespace 风格跨进程权限。

## 总控 Agent 使用方式

建议启动一个总控 agent 负责 orchestration，但不要让它自由决定新的协议拆分。
总控 agent 的权限边界是：

- 可以执行前置检查、代码搜索和构建级 gate。
- 可以启动只读 explorer / reviewer。
- 可以启动写入型 worker，但必须使用本文列出的 write set 和 worker 合同。
- 可以串行集成 worker diff。
- 可以更新事务 devlog。
- 不运行 QEMU / LTP，除非用户后续明确要求；rv64 / LTP 日志默认由用户提供。
- 不 push、不 force-push、不 reset hard、不清理未归属改动。
- 遇到停止条件时回报用户，不自行拍板。

总控第一轮不要一次性派发所有 worker。建议流程是：

1. 重新确认当前分支、工作区状态、RFC 文档和事务日志。
2. 派发 Agent 0 做当前 procfs / fd table / path helper 前置审计。
3. 派发 Agent 1，实现 `FilesState` fd number 只读快照入口。
4. 进行 Gate 1 review，确认 snapshot 与 `get_fd(fd)` 使用同一打开集合语义。
5. 派发 Agent 2，实现 `TgidEntry` custom constructor 和 `/proc/<tgid>/fd` 目录框架。
6. 进行 Gate 2 review，确认动态 `fd/<n>` 没有污染静态 tgid entry / sub-ino 空间。
7. 派发 Agent 3，实现 fd symlink `readlink()` / `getattr()`、路径显示和 pipe 显示名。
8. 进行 Gate 3 review，确认阶段 1 安全边界、缓存边界和错误码映射闭合。
9. 派发 Agent 4 做阶段 1 smoke、旁路审计、current limitations 和事务日志收口。
10. 只有阶段 1 后出现新的 fd entry open 阻塞项时，才派发 Agent 5 推进阶段 2。

可直接给总控 agent 的启动 prompt：

```text
工作目录是仓库根目录。请作为 proc-tgid-fd 的总控 agent，阅读
docs/src/rfcs/proc-tgid-fd/index.md、
docs/src/rfcs/proc-tgid-fd/implementation.md、
docs/src/rfcs/proc-tgid-fd/tracking-issues.md、
docs/src/rfcs/proc-tgid-fd/backgrounds/agent-orchestration.md 和
docs/src/devlog/transactions/2026-06-04-proc-tgid-fd.md。

目标是按 RFC gate 先实现阶段 1：fd number 只读 snapshot、/proc/<tgid>/fd
目录、数字 fd lookup、fd symlink readlink 和 getattr。阶段 2 的 fd entry open
只在阶段 1 后确认成为新的直接阻塞项时推进。

你可以启动子 agent，但必须按 agent-orchestration.md 的顺序、write set 和 review gate
分工，不允许 worker 越界修改。你不是独自在代码库里工作；不得 revert 用户或其他
agent 的改动。每集成一个阶段都要更新
docs/src/devlog/transactions/2026-06-04-proc-tgid-fd.md。

第一步只做前置检查、刷新当前代码落点和准备启动的 agent 列表。不要直接一次性启动
所有 worker。遇到停止条件时停止并向用户报告，不要自行拍板。
```

## Agent 0：前置审计

职责：只读审计当前代码落点是否仍符合 RFC 假设，不改代码。

读取范围：

- `anemone-kernel/src/fs/proc/tgid/mod.rs`
- `anemone-kernel/src/fs/proc/tgid/file.rs`
- `anemone-kernel/src/fs/proc/tgid/inode.rs`
- `anemone-kernel/src/fs/proc/tgid/{cwd,root,exe}.rs`
- `anemone-kernel/src/fs/proc/tgid/binding.rs`
- `anemone-kernel/src/task/files.rs`
- `anemone-kernel/src/task/fs.rs`
- `anemone-kernel/src/fs/file.rs`
- `anemone-kernel/src/fs/pipe.rs`

检查项：

- `TGID_ENTRIES` 是否仍是 `/proc/<tgid>` 静态子项的唯一注册表。
- `TgidEntry::new_inode()` 是否仍默认创建 `TgidSubInodePrivate { binding }`。
- `<tgid>` 目录 `sub_ino` 是否仍只适合静态 entry。
- `ThreadGroupBinding` 是否能重新验证 alive 并取到目标 leader。
- `FilesState` 是否仍同时维护 `bitmap` 和 `fds`，以及 `get_fd(fd)` 以哪一侧为准。
- `Task::files_state()`、`Task::get_fd()` 和 `CLONE_FILES` / `close_range(UNSHARE)`
  是否仍维持共享 fd table handle 语义。
- `Task::rel_abs_path()` 是否仍是目标 root 视角路径格式化的合适入口。
- procfs `cwd` / `root` / `exe` symlink 模式是否可复用。
- pipe 是否有 owner-side 只读显示名 helper，若没有，阶段 1 需要补最小 helper。

交付：

- 是否允许进入 Agent 1 的结论。
- 如果不允许，列出必须先修的 RFC blocker。
- 当前代码路径与阶段 0 / 阶段 1 的对应表。

停止条件：

- 当前 procfs tgid 框架已经不再通过 `TGID_ENTRIES` 统一 lookup / readdir。
- fd table 当前打开集合无法定义一个与 `get_fd(fd)` 一致的 snapshot 语义。
- path helper 只能返回全局路径，无法在阶段 1 保持目标 root 内路径边界。

## Agent 1：Fd Number Snapshot

职责：实现阶段 0，为 procfs 提供窄的 fd table 观察入口。

write set：

- `anemone-kernel/src/task/files.rs`
- 必要的 `Task` wrapper 所在文件
- `docs/src/devlog/transactions/2026-06-04-proc-tgid-fd.md`

语义要求：

- 新增 `opened_fd_numbers_snapshot() -> Vec<Fd>` 或等价只读 API。
- snapshot 按 fd number 递增排序。
- snapshot 的打开集合语义必须和 `get_fd(fd)` 一致。
- 若 `bitmap` 与 `fds` 仍并存，snapshot 以 `fds[i].is_some()` 为准，并加入轻量
  `assert!` 或等价普通运行可见校验，暴露 bitmap / fds 分叉。
- snapshot 不返回 `Arc<FileDesc>`，不暴露内部可变引用。
- snapshot 过程中不调用 VFS、procfs、路径格式化或可能阻塞的逻辑。
- 不改变 fd alloc、dup、close、fork、`CLONE_FILES`、`close_range` 语义。

验证：

```bash
just build
```

Gate 1 reviewer 检查：

- KETER-004：snapshot 与 `get_fd(fd)` 没有分叉的打开集合语义。
- EUCLID-001：snapshot 不返回 `Arc<FileDesc>` 作为目录枚举真相。
- `assert!` / 校验纪律：低成本一致性检查不能只放在 `debug_assert!`。

## Agent 2：Tgid Entry 与 Fd 目录框架

职责：实现阶段 1 的 procfs 目录框架，不负责最终 `readlink()` 显示策略。

write set：

- `anemone-kernel/src/fs/proc/tgid/mod.rs`
- `anemone-kernel/src/fs/proc/tgid/file.rs`
- `anemone-kernel/src/fs/proc/tgid/inode.rs`
- `anemone-kernel/src/fs/proc/tgid/fd.rs` 或等价新模块
- 必要的 procfs module export
- `docs/src/devlog/transactions/2026-06-04-proc-tgid-fd.md`

语义要求：

- 给 `TgidEntry` 增加可选 custom inode constructor / private-data factory。
- 默认 constructor 继续生成 `TgidSubInodePrivate { binding }`，并保留当前 mode、
  perm、uid/gid、nlink 和时间戳初始化语义。
- `fd` 作为目录 entry 加入 `TGID_ENTRIES`，由同一 `<tgid>` lookup / readdir 发现。
- `fd` entry constructor 创建 `ProcFdDirPrivate { binding, child_ino }`。
- `fd/<n>` entry private data 只保存 `(ThreadGroupBinding, Fd)`。
- `child_ino` 是 `fd` 目录自己的受保护 map，只表示 procfs synthetic child identity。
- 动态 fd number 不进入 `TGID_ENTRIES`，不进入 `<tgid>` 目录 `sub_ino` 静态子项表，
  也不复用目标文件 inode。
- 在 `fd.rs` 内集中 helper：
  - `validate_fd_access(binding)`
  - `parse_proc_fd_name(name)`
  - `lookup_proc_fd(leader, fd)`
  - `proc_fd_child_ino(fd)`
- `open()`、`read_dir()` 和 `lookup()` 先执行 `same-tgid` 检查；失败返回
  `SysError::AccessDenied`，不得读取目标 fd table。
- `read_dir()` 先取 fd number snapshot，释放 fd table lock 后再分配 child ino 和输出。
- `lookup()` 先验证 fd 当前存在，再短暂分配 child ino。

验证：

```bash
just build
```

Gate 2 reviewer 检查：

- KETER-005：`fd` 目录有自己的 private data，没有绕开 `TGID_ENTRIES`。
- KETER-003：动态 fd 子项没有污染静态 tgid entry / sub-ino 框架。
- EUCLID-003：fd table lock、child ino cache lock 和 VFS 输出没有形成不可审计锁序。
- EUCLID-002：数字解析和 `BadFileDescriptor` 到 `NotFound` 的映射集中在 helper。

## Agent 3：Fd Symlink Readlink / Getattr

职责：完成阶段 1 的 fd symlink 操作、路径显示和匿名对象显示名。

write set：

- `anemone-kernel/src/fs/proc/tgid/fd.rs`
- `anemone-kernel/src/task/fs.rs` 或现有 path helper 所在文件的最小共享 helper
- `anemone-kernel/src/fs/pipe.rs`
- 必要的 `File` / `PathRef` 只读 helper
- `docs/src/devlog/transactions/2026-06-04-proc-tgid-fd.md`

语义要求：

- `fd/<n>` symlink inode 不保存 `Arc<FileDesc>`。
- `readlink()` 每次重新验证 binding alive、`same-tgid` 和当前 fd 存在。
- `getattr()` 每次重新验证 binding alive、`same-tgid` 和当前 fd 存在。
- fd 当前不存在时，把 fd syscall 的 `BadFileDescriptor` 映射成 `SysError::NotFound`。
- 普通路径对象从 owner path API 取得 `PathRef`，再用目标 leader root 视角格式化。
- root 外路径返回 `SysError::PermissionDenied` 或等价权限错误，不返回全局路径。
- pipe 显示名由 `fs::pipe` 暴露只读 helper 或等价 owner API 生成，返回 `pipe:[ino]`。
- 其他不可分类匿名对象返回稳定 stage-1 fallback，或明确保持 unsupported limitation。
- `open()` 第一阶段返回稳定 unsupported / symlink 行为，不宣称完整 Linux magic-link。

验证：

```bash
just build
```

建议 smoke：

- `getdents64(open("/proc/self/fd"))` 能看到 `0`、`1`、`2` 和测试打开的 fd。
- `readlink("/proc/self/fd/<regular-fd>")` 返回目标 root 视角路径。
- `readlink("/proc/self/fd/<dir-fd>")` 返回目标 root 视角目录路径。
- `pipe2()` 后两个 fd 的 `readlink()` 返回稳定 `pipe:[ino]`。
- close fd 后对应 entry 的 `readlink()` / `getattr()` 失败为 `ENOENT`。
- 非数字、负数、超范围和未打开 fd entry 返回 `ENOENT`。
- 非当前 tgid 的 `/proc/<tgid>/fd` 返回 `EACCES`，不泄露 fd 存在性。

Gate 3 reviewer 检查：

- KETER-001：`same-tgid only` 和 root 外路径策略没有被放宽。
- KETER-002：缓存 dentry/inode 不代表 fd 当前存在。
- fd entry 操作没有长期保存 `Arc<FileDesc>`。
- `readlink()` / `getattr()` 的 fd 存在性重新验证路径一致。
- `fd.rs` 只编排显示策略，不直接读取 pipe 或其他对象 private data。

## Agent 4：阶段 1 收口

职责：做旁路审计、最低验证和文档收口。

write set：

- `docs/src/devlog/transactions/2026-06-04-proc-tgid-fd.md`
- `docs/src/register/current-limitations.md`
- 必要的双周 devlog 追加项
- 只在发现局部遗漏时最小修改阶段 1 代码

审计命令：

```bash
rg -n "TODO: fd|TGID_ENTRIES|proc-tgid-fd|fdinfo" anemone-kernel/src/fs/proc
rg -n "opened_fd_numbers_snapshot|FilesState|FileDesc|CLONE_FILES|close_range" anemone-kernel/src/task anemone-kernel/src/fs
rg -n "/proc/self/fd|pipe07|getcwd02" docs/src anemone-apps/user-test
```

验证：

```bash
just build
```

收口要求：

- 更新事务日志，记录实现范围、review 结论、`just build` 结果和未跑验证。
- 更新 `current-limitations`：关闭 `/proc/<tgid>/fd` 目录缺失，保留完整 magic-link open、
  跨进程权限、`fdinfo`、匿名对象精确显示名和 `O_PATH` 后续能力等 residual limitations。
- 若用户提供 musl `getcwd02` 或 LTP `pipe07` 结果，写入事务日志；agent 不伪造未运行证据。
- 如果阶段 1 失败只剩环境或测试基础设施问题，明确分类，不把它写成 procfs 语义 blocker。

停止条件：

- 还有开放 Keter / Euclid gate 未回到 RFC 或代码修复。
- `/proc/self/fd` 的 `readdir` / `readlink` / `getattr` 任一主路径无法通过构建级或 smoke 级验证。
- 需要实现 magic-link open 才能继续时，停止并要求进入阶段 2 决策。

## Agent 5：阶段 2 Fd Entry Open（条件性）

职责：仅在阶段 1 后确认 fd entry open 成为新的直接阻塞项时推进。

write set：

- `anemone-kernel/src/fs/proc/tgid/fd.rs`
- 必要的 VFS symlink / open helper
- 必要的 path reopen helper
- `docs/src/devlog/transactions/2026-06-04-proc-tgid-fd.md`
- `docs/src/register/current-limitations.md`

语义要求：

- 明确这是阶段 2，不修改阶段 1 验收口径。
- 第一版继续沿用 `same-tgid only`。
- 对 `/proc/self/fd/<n>`，可以从当前 fd 的 `PathRef` 按 requested flags 重新打开。
- 若要复用目标 open file description，必须先建立明确的 magic-link helper，不能用普通
  symlink 字符串解析伪装完整 Linux 语义。
- 固定 `O_PATH`、`O_NOFOLLOW`、directory fd、write access、truncate、append、
  nonblock 等边界。
- 不绕过 VFS 权限检查。
- 不让 fd entry open 修改目标 fd 的 file status flags。
- 不把 target fd number 当成 current task fd number。
- 不引入 fd table lock 和 VFS open 的锁序反转。

验证：

```bash
just build
```

建议 smoke：

- `open("/proc/self/fd/<regular-fd>", O_RDONLY)`。
- `open("/proc/self/fd/<dir-fd>", O_DIRECTORY)`。
- `open("/proc/self/fd/<closed>")` 失败。
- `O_NOFOLLOW` / `O_PATH` 边界按文档策略返回稳定结果。

Gate 4 reviewer 检查：

- 阶段 2 没有回头放宽跨进程权限。
- reopen 路径和 VFS 权限边界清楚。
- target fd 的 open file description、status flags 和 current task fd number 没有混用。
- current limitations 已区分“普通 symlink follow 风格 reopen”和“完整 Linux magic-link”差异。
