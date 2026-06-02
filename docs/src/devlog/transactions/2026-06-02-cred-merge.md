# 2026-06-02 - Cred Merge

**Status:** Active
**Owners:** doruche, Codex
**Area:** credentials / task / VFS / exec / syscall ABI / user-test
**Current Phase:** rv64 build gate passed; la64 pending

## Handoff

**Last Updated:** 2026-06-02

**Current Branch:** `dev/drc/merge-cred`

**Current HEAD:** `030280a65108b1e3944b5e8f8fdb1588ac25c846`

**Origin Main:** `892f89d3415a30aa7284f83d8ca619c540b6d14d`

**Merge Base:** `210a9e07d1c8381ac5913298c8d6f26daf878581`

**Merge State:** merge checkpoint 已提交。

**Merge Checkpoint Commit:** `6e094ec52a69a0c01eb553fb573ed291564db11e` (`[ckpt] merge origin main credentials`)

**Latest Build-Fix Checkpoint:** `030280a65108b1e3944b5e8f8fdb1588ac25c846` (`[ckpt] cred merge rv64 build fix`)

**Conflicts Remaining:** 0。

**Completed:** 迁移编排计划已写入 `etc/cred-merge/agent-plan.md`；事务日志已建立；merge-state 已建立；实际冲突地图已刷新；Worker A 已审查 credentials core / syscall ABI 自动合入结果，并在 `anemone-kernel/src/task/credentials/id.rs` 恢复 `Uid::get()` / `Gid::get()` inherent accessor；Worker B 已审查并修复 Task lifecycle / accessor 基座，手工改动限制在 `anemone-kernel/src/task/mod.rs` 与 `anemone-kernel/src/task/api/clone/mod.rs`；Worker C 已解析并暂存 VFS/open/fd 冲突；Worker D 已解析并暂存 exec credential / PathRef 冲突；Worker E 已解析并暂存 user-test fixtures/groups/profile/registry 冲突。

**Open Blockers:** 无文本冲突 blocker。`anemone-kernel/src/syscall/user_access.rs` 被 git 自动合入但不在当前 worker write set 中；目前只读审查看到它提供 `c_readonly_path()` 与 `ListTooLong -> NameTooLong` 路径校验支撑，未再手工修改。rv64 `just build` 已通过；la64 `just build` 和只读 reviewer 尚未运行。

**Next Action:** 切到 la64 运行 `just build`，随后启动/执行只读 reviewer 审查 P0/P1 语义闭合。LTP 仍由用户手动执行。

**Do Not Redo:** 不要重新审查已闭合的计划原则，除非 `origin/main` 或 merge-base 改变；不要恢复远端旧 `process-exec` group；不要重拆、合并或改名本地已有 LTP group；不要运行 LTP；不要 push、force-push、`git reset --hard`、`git clean` 或丢弃未归属改动。

**User-Owned Validation:** rv64 / la64 LTP 由用户手动执行。agent 只记录建议命令、构建 gate、失败归类和用户提供的日志结论。

## Scope

本事务跟踪 `dev/drc/merge-cred` 上的 credentials merge：把 `origin/main` 中由另一位开发者引入的 credentials 系统并入当前本地分支，同时保留本地分支在 credentials 以外已经验证过的 syscall、VFS、exec、mm、sched 和测试脚本修复。

本事务不是普通冲突解决日志。主要风险是语义冲突而非文本冲突：`git merge` 可能静默接受某些文件，但实际把本地 typed syscall / fd model / exec PathRef 语义与远端 credentials 语义拼错。

## Principles

- credentials 语义以 `origin/main` 为准：uid/gid/fsuid/fsgid、supplementary groups、capabilities、securebits、`no_new_privs`、VFS permission、exec credential transition 和相关 syscall 都应保留远端完整语义。
- credentials 以外的 bugfix 和功能引入默认以本地分支为准：typed `openat`、fd access/status model、`execveat` / `PathRef`、empty argv、sched wait refactor、procfs/statx、mmap/mremap/shm 修复、la64 测试脚本等不应被远端同名修复覆盖。
- LTP group/profile 形状以本地新拆分为准；已有本地 group 不因 merge 被重拆、合并或改名。当前本地缺失的 `credentials`、`chmod`、`chown` 可以作为新增 group 单独引入；远端旧 `process-exec` 不恢复。
- LTP 由用户手动执行。agent 只负责代码合并、构建 gate、失败归类说明和事务日志记录。
- 总控 agent 可以在 `dev/drc/merge-cred` 上使用 git 做本地版本管理，包括阶段性 checkpoint commit、worker 集成 commit 和最终 merge commit；但不 push、不 force-push、不改写其他分支、不使用 `git reset --hard` 或 `git clean` 丢弃未归属改动。
- 如果 merge 过程中引入临时兼容层，必须在本事务日志中显式记录后续删除点，避免后续阶段把它当作永久接口。

## Initial Branch Snapshot

- merge-base：`210a9e07d1c8381ac5913298c8d6f26daf878581`
- planning snapshot local HEAD：`5905dcff3ce2c3a95998fd2bb95b4e6896c3395a`
- planning snapshot origin/main：`892f89d3415a30aa7284f83d8ca619c540b6d14d`

这些 hash 是计划快照。真正执行 merge 前，总控 agent 必须重新确认当前分支、工作区状态、`origin/main` 和 merge-base。

## High-Risk Areas

- `openat` / fd / VFS metadata：本地 typed `OpenHow`、`O_PATH`、`O_NOFOLLOW`、fd access/status model 必须和远端 `FsPermChecker`、DAC/capability check、`O_NOATIME`、credential-aware truncate、setuid/setgid drop 合并。
- `Task` core / lifecycle：本地 `create_instant`、`sched_state` 和 wait-refactor 状态必须和远端 `cred`、`no_new_privs`、`nice`、credential accessor、clone/exec/exit credential 继承与替换合并。
- `execve` / `execveat`：本地 `PathRef` loader、`execveat`、empty argv、cmdline range 记录必须和远端 exec permission、setuid/setgid、file caps、secure exec、`no_new_privs` 合并。
- syscall ABI：riscv64 和 loongarch64 syscall 表必须同时保留本地新增 syscall 与远端 credentials/capability/prctl syscall。
- user-test harness：新增 `credentials` / `chmod` / `chown` 组和必要 fixture/注册表，同时保持本地已有 group 的边界，不恢复远端旧 `process-exec`。

## Phase Log

### 2026-06-02 - 迁移编排计划

**Phase:** planning / orchestration

**Change:** 建立 credentials merge 的多 agent 编排方案：总控 agent 负责前置检查、建立 merge-state、限制 worker write set、串行集成、构建 gate 和事务日志；worker 按 credentials core/ABI、Task lifecycle、VFS permission/open/fd、exec credentials/PathRef、user-test harness 拆分。只读 reviewer 负责审查 P0/P1 语义冲突是否闭合。

**Boundary:** 总控 agent 不应无限自治。第一轮只允许确认分支、hash、工作区状态和实际冲突地图；之后按阶段启动 worker。允许总控在 merge 专用分支上用 git 做本地 checkpoint 和阶段提交，以便管理进度和回滚本次 merge 分支上的改动。遇到 `openat`、exec credential commit order、Task credential lock 与 sched state 锁序、worker 越界、临时兼容层无删除点等停止条件时，必须回报用户拍板。

**Validation:** 本阶段只完成计划与日志结构，未执行 merge、构建或 LTP。

**Next:** 启动总控 agent，确认当前分支为 `dev/drc/merge-cred`，刷新 `origin/main` / merge-base 后建立 `git merge --no-commit origin/main` 的 merge-state，并把实际冲突地图追加到本事务日志。

### 2026-06-02 - 建立 merge-state 与首轮 worker

**Phase:** execution / merge-state

**Change:** 总控 agent 重新确认分支形状后，在 `dev/drc/merge-cred` 上执行 `git merge --no-commit origin/main`。当前本地 HEAD 为 `0f357d978cfec0c7a8485b09fa16132fd44fbb40`，`origin/main` 为 `892f89d3415a30aa7284f83d8ca619c540b6d14d`，merge-base 仍为 `210a9e07d1c8381ac5913298c8d6f26daf878581`。merge-state 已建立，尚未提交 merge。

**Conflict Map:** 实际文本冲突为 8 个文件：Worker E 负责 `anemone-apps/user-test/ltp/profile.txt`、`anemone-apps/user-test/src/ltp.rs`；Worker C 负责 `anemone-kernel/src/fs/api/openat.rs`、`anemone-kernel/src/task/files.rs`；Worker D 负责 `anemone-kernel/src/task/api/execve/binfmt/mod.rs`、`anemone-kernel/src/task/api/execve/binfmt/shebang.rs`、`anemone-kernel/src/task/api/execve/kernel.rs`、`anemone-kernel/src/task/api/execve/syscall.rs`。其余 ABI、credentials core、VFS permission、Task lifecycle、exec ELF、fixtures 和新增 LTP group 文件由 git 自动合入，但仍需按 worker 分工做语义审查。

**Boundary:** 首轮只启动 Worker A 与 Worker B。Worker A 限定在 credentials core / syscall ABI write set；Worker B 限定在 Task / lifecycle / signal / priority / `mm/uspace` write set。Worker C/D/E 暂不启动，等待 A/B 的 accessor 与 ABI 基座稳定后再进入。`anemone-kernel/src/syscall/user_access.rs` 被 git 自动合入但不在现有 worker write set 中，暂作为总控只读审查项；若后续必须修改，需要先回到分工边界处理。

**Validation:** 本阶段只完成 merge-state 建立、冲突地图刷新和 Worker A/B 启动；未运行构建，未运行 LTP。

**Next:** 等待 Worker A/B 输出，串行集成 credentials core/ABI 与 Task accessor/lifecycle，随后更新本事务日志并决定是否进入 Worker C/D。

### 2026-06-02 - Worker A/B 基座审查

**Phase:** execution / core-task-base

**Change:** Worker A 完成 credentials core / syscall ABI 审查；当前 merge-state 自动合入的 `anemone-abi/src/capability.rs`、`anemone-abi/src/lib.rs`、`anemone-abi/src/syscall/{riscv,loongarch}.rs`、`anemone-kernel/src/task/credentials/**`、`anemone-kernel/src/task/api/mod.rs`、`anemone-kernel/src/syscall/mod.rs`、`anemone-kernel/src/syserror.rs` 符合 Worker A write set，并在 `anemone-kernel/src/task/credentials/id.rs` 恢复 `Uid::get()` / `Gid::get()` inherent accessor，使本地 VFS/statx/fchown/auxv 等既有调用点不需要越界改 import。Worker B 完成 Task lifecycle / accessor 基座审查，并只在 `anemone-kernel/src/task/mod.rs` 与 `anemone-kernel/src/task/api/clone/mod.rs` 内修复：`replace_cred()` 改为 `&self` 下 credential write lock 替换 snapshot，clone 显式继承 `cred()`、`no_new_privs()` 与 `nice()`。

**Boundary:** A/B 未处理 VFS/open/fd、exec、user-test 或未分配的 `anemone-kernel/src/syscall/user_access.rs`。`Task` credential accessor 合同暂定为：读取用 `cred()` snapshot，整体替换用 `replace_cred(&self, CredentialSet)`，事务更新用 `update_cred_with()`，能力和 no-new-privs 访问用 `has_cap()`、`no_new_privs()`、`set_no_new_privs()`。调用方不能在持有 scheduler-state lock 时替换 credentials。

**Validation:** Worker A 报告 `git diff --cached --check` 在 A write set 上通过，riscv64/loongarch64 syscall 常量集合等价扩展，并同时保留 credentials/prctl/capability 编号与本地 `execveat`、`statx`、`clone3`、`close_range`、`faccessat2` 等编号。总控复查 Worker B diff，确认只改 B write set、无冲突标记，`git diff --check -- <Worker B write set>` 通过。未运行构建，未运行 LTP。

**Checkpoint:** 按用户要求，每个 worker 集成点应创建本地 checkpoint commit；但当前仍存在 8 个 unmerged paths，普通 `git commit` 不能在这种 index 状态下创建提交。该 checkpoint 暂记为 pending，等 Worker C/D/E 解析冲突并让 index 离开 unmerged 状态后补做。

**Next:** 启动 Worker C 与 Worker D；二者 write set disjoint，但总控必须串行应用和复查。Worker E 仍暂缓，最后处理 LTP group/profile 增量。

### 2026-06-02 - Worker C/D 冲突解析

**Phase:** execution / vfs-exec

**Change:** Worker C 解析并由总控暂存 `anemone-kernel/src/fs/api/openat.rs`、`anemone-kernel/src/task/files.rs`、`anemone-kernel/src/fs/api/fchmod/mod.rs`。合并结果保留本地 typed `OpenHow` / `OpenAccessMode` / `FileStatusFlags` / `LinuxOpenCompat` / `O_PATH` / `O_NOFOLLOW` / fd access-status model，同时移植 `FsPermChecker`、checked path lookup、namei search 权限、create/tmpfile parent `WRITE|EXECUTE`、`O_NOATIME` owner-or-`CAP_FOWNER`、credential-aware truncate 和 chmod 后 setid-drop hook。Worker D 解析并暂存 `anemone-kernel/src/task/api/execve/**`，保留 `execveat` / `PathRef` 边界、empty argv、shebang 修复和 cmdline range 形状，同时接入 exec permission、setid/file-cap/no-new-privs/secure-exec 计算。

**Boundary:** Worker C/D 未修改 user-test 或 devlog。`anemone-kernel/src/syscall/user_access.rs` 仍是 git 自动合入结果，作为 path validator 支撑保留在总控只读审查项。Exec credential 替换仅在 loader 成功返回后的提交路径调用 `Task::replace_cred()`；VFS/open/fd 侧只通过 `Task::cred()` snapshot 消费 credential。

**Validation:** 总控复查 C 范围内 `openat.rs`、`task/files.rs`、`fchmod/mod.rs` 无冲突标记，`git diff --check -- <Worker C touched files>` 通过；复查 D 范围内 `task/api/execve/**` 无冲突标记，`git diff --check -- anemone-kernel/src/task/api/execve` 通过。未运行构建，未运行 LTP。

**Checkpoint:** 仍 pending。当前只剩 Worker E 的 user-test `UU` 文件；待 E 解析后创建本地 checkpoint commit 并刷新 handoff。

**Next:** 启动 Worker E，只处理 LTP fixtures/groups/profile/registry 增量，不重拆或合并本地已有 group，不恢复远端旧 `process-exec`。

### 2026-06-02 - Worker E user-test 增量

**Phase:** execution / user-test

**Change:** Worker E 解析并暂存 `anemone-apps/user-test/fixtures/passwd`、`anemone-apps/user-test/fixtures/group`、`anemone-apps/user-test/ltp/groups/credentials.txt`、`anemone-apps/user-test/ltp/groups/chmod.txt`、`anemone-apps/user-test/ltp/groups/chown.txt`、`anemone-apps/user-test/ltp/profile.txt`、`anemone-apps/user-test/src/ltp.rs`。`LTP_GROUPS` 保留本地已有 group 边界，只新增注册 `credentials`、`chmod`、`chown`；`profile.txt` 保留本地 `all`；远端旧 `process-exec` 未恢复。

**Boundary:** Worker E 未修改内核文件或 devlog。`credentials.txt` 中的 `case => executable args...` alias 语法已在 parser 中支持：左侧作为 LTP case id，右侧作为实际 executable 和 argv；无 alias 的旧 `case args...` 格式仍兼容。

**Validation:** Worker E 报告 write set 冲突标记扫描通过，`git diff --check -- <Worker E write set>` 与 `git diff --cached --check -- <Worker E write set>` 通过。总控复查无 unmerged paths，全局 `git diff --check` 通过，关键冲突标记扫描无输出。未运行构建，未运行 LTP。

**Checkpoint:** 所有 unmerged paths 已解析，下一步创建本地 merge checkpoint commit。

**Next:** 创建 checkpoint commit，随后刷新 handoff 记录 commit hash，再进入构建 gate 和只读 reviewer 阶段。

### 2026-06-02 - Merge checkpoint

**Phase:** execution / checkpoint

**Change:** 总控在所有 worker write set 解析完成、无 unmerged paths、全局 `git diff --check` 通过后，创建本地 merge checkpoint commit `6e094ec52a69a0c01eb553fb573ed291564db11e`，提交信息为 `[ckpt] merge origin main credentials`。该 commit 有两个 parent：本地 pre-merge checkpoint `0f357d978cfec0c7a8485b09fa16132fd44fbb40` 与 `origin/main` 的 `892f89d3415a30aa7284f83d8ca619c540b6d14d`。

**Boundary:** 本 checkpoint 只固化当前 merge 分支状态；未 push、未 force-push、未改写其他分支、未运行 LTP。

**Validation:** merge 提交前确认无 unmerged paths，`git diff --check` 通过，关键冲突标记扫描无输出。构建 gate 尚未运行。

**Next:** 运行构建 gate，并在 gate 结果后刷新本 handoff。

### 2026-06-02 - Build gate rv64 first failure

**Phase:** validation / build-gate

**Change:** rv64 `just build` 首次执行失败，编译错误为 `anemone-kernel/src/fs/pipe.rs` 中 `SigKill.uid` 仍写 raw `0`，而 credentials merge 后 `SigKill.uid` 是 typed `Uid`。总控做窄修：`send_sigpipe()` 的 `si_uid` 改为 `get_current_task().cred().uid.real`，与 `kill` / `tkill` / `tgkill` 等信号路径的 sender credential 来源一致。

**Boundary:** 这是构建 gate 暴露的 post-merge typed credential 修复，不改变 pipe I/O 语义，不运行 LTP。

**Validation:** `just xtask app build user-test --arch riscv64` 已通过。rv64 `just build` 首次失败后已修复代码，rerun 通过；仅余既有 unused-import warnings。

**Next:** 提交本 build-gate 修复 checkpoint，然后切到 la64 运行 `just build`。
