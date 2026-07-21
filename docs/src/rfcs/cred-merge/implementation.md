# credentials merge 多 agent 编排计划

本计划服务于 `dev/drc/merge-cred` 上的 merge 工作：把 `origin/main` 的
credentials 系统并入当前本地分支，同时保留本地分支在 credentials 以外的
syscall、VFS、exec、mm、sched、测试脚本等修复。

计划生成时的分支形状：

- 当前分支：`dev/drc/merge-cred`
- merge-base：`210a9e07d1c8381ac5913298c8d6f26daf878581`
- 本地 HEAD：`5905dcff3ce2c3a95998fd2bb95b4e6896c3395a`
- 远端目标：`origin/main = 892f89d3415a30aa7284f83d8ca619c540b6d14d`

这些 hash 只是计划快照。正式执行前由总控 agent 重新确认。

## 总原则

1. credentials 系统以 `origin/main` 为语义来源。
   包括 uid/gid/fsuid/fsgid/supplementary groups、capabilities、securebits、
   `no_new_privs`、VFS permission、exec credential transition、相关 syscall。

2. credentials 以外的 bugfix 和功能引入默认以本地分支为准。
   特别是 typed `openat`、fd access/status model、`execveat`/`PathRef`、
   empty argv 处理、sched wait refactor、procfs/statx、mmap/mremap/shm 修复、
   la64 测试脚本等，不应被远端同名修复覆盖。

3. 高风险文件不能按文本冲突机械处理。
   `git merge` 静默接受的语义冲突比文本冲突更危险。P0/P1 文件必须做三方对照：
   `merge-base`、本地 HEAD、`origin/main`。

4. 所有 worker 都必须使用 disjoint write set。
   worker 不是独自在代码库里工作，不能 revert 其他 agent 的改动；遇到越界依赖时
   停下来向总控提交 write set 扩展申请，而不是顺手改别人的文件。申请通过后，
   总控先记录新增范围、原因和 gate 影响，再继续集成。

5. 允许总控 agent 在 `dev/drc/merge-cred` 上使用 git 做本地版本管理。
   总控可以创建阶段性 checkpoint commit、worker 集成 commit 和最终 merge commit，
   以便回看和回滚本次专用 merge 分支上的进度。worker 可以在隔离 workspace /
   forked workspace 中改文件并提交 diff，或者在同一 merge-state 下只改自己的
   write set。最终由总控顺序集成、复查和提交。

6. git 权限只覆盖本次 merge 专用分支上的本地版本管理。
   未经用户明确要求，不允许 push / force-push，不允许改写 `origin/main`、
   `dev/drc/chaos` 或其他非 merge 专用分支，不允许 `git reset --hard`、
   `git clean` 或丢弃用户未归属改动。

## 前置检查

总控 agent 在启动任何写入型 worker 前执行：

```bash
git status --short --branch
git rev-parse HEAD
git rev-parse origin/main
git merge-base HEAD origin/main
git log --oneline --decorate --graph --boundary --left-right HEAD...origin/main
```

必须确认：

- 当前分支是 `dev/drc/merge-cred`。
- 工作区干净，或所有未提交改动都已明确归属。
- merge-base 仍为预期值，或计划需要重新刷新。
- `origin/main` 没有在本计划生成后继续前进；如果前进，先重新跑只读审计。

## 阶段 0：只读审计和冲突地图

目标：在实际 merge 前得到文件风险、credentials 架构边界、人工验证建议。

已知高风险重叠文件：

- P0：`anemone-kernel/src/fs/api/openat.rs`
- P0：`anemone-kernel/src/task/files.rs`
- P0：`anemone-kernel/src/task/mod.rs`
- P0：`anemone-kernel/src/task/api/execve/binfmt/mod.rs`
- P0：`anemone-kernel/src/task/api/execve/kernel.rs`
- P0：`anemone-kernel/src/task/api/execve/syscall.rs`
- P1：`anemone-kernel/src/fs/inode.rs`
- P1：`anemone-kernel/src/fs/file.rs`
- P1：`anemone-kernel/src/fs/api/fallocate.rs`
- P1：`anemone-kernel/src/task/api/execve/binfmt/elf/mod.rs`
- P1：`anemone-kernel/src/task/api/execve/binfmt/elf/init_stack.rs`
- P1：`anemone-kernel/src/task/api/execve/binfmt/shebang.rs`
- P1：`anemone-kernel/src/mm/uspace/mod.rs`
- P1：`anemone-apps/user-test/src/ltp.rs`
- P2：`anemone-apps/user-test/ltp/profile.txt`
- P2：`anemone-abi/src/syscall/riscv.rs`
- P2：`anemone-abi/src/syscall/loongarch.rs`
- P2：`anemone-kernel/src/fs/api/stat/newfstatat.rs`
- P3：`anemone-kernel/src/fs/api/readlinkat.rs`
- P3：`anemone-kernel/src/task/api/exit/mod.rs`

阶段 0 输出物：

- P0/P1 文件三方语义摘要。
- 本地必须保留的语义清单。
- `origin/main` credentials 必须保留的语义清单。
- 每个 worker 的 write set 和接口合同。
- 供用户手动执行的验证命令和失败归类规则。

## 阶段 1：建立 merge-state

总控 agent 执行：

```bash
git merge --no-commit origin/main
```

如果出现文本冲突，不在总控层手工解决 P0/P1 文件，只做两类事情：

- 记录冲突文件归属到对应 worker。
- 对明显新增且无本地对应修改的 credentials 文件，保留 `origin/main` 版本。

如果 merge 直接成功，也不能进入提交。仍然按本计划跑 worker 审查，因为本次主要风险是
语义冲突而非文本冲突。

## 总控 agent 使用方式

建议启动一个总控 agent，让它阅读本文件并负责 orchestration，但不要让它“自由发挥式”
自己决定所有拆分。总控 agent 的权限边界是：

- 可以执行前置检查和 `git merge --no-commit origin/main`。
- 可以启动只读 reviewer / explorer。
- 可以启动写入型 worker，但必须使用本文列出的 write set 和 worker 合同；需要扩大 write set 时，先记录原因、范围、contract/gate 影响和批准结果。
- 可以把 worker diff 串行集成到 merge-state。
- 可以在 `dev/drc/merge-cred` 上使用 git 做本地 checkpoint commit / 阶段 commit，
  记录 worker 集成点、review 修复点和构建 gate 点。
- 可以运行构建级 gate，例如 `git diff --check`、`just build`、
  `just xtask app build user-test --arch riscv64`。
- 可以更新事务 devlog：[2026-06-02 Cred Merge](../../devlog/transactions/2026-06-02-cred-merge.md)。
- 不运行 LTP；LTP 日志由用户提供后再归类。
- 不在 P0 停止条件上自行拍板；必须回报用户。
- 不 push、不 force-push、不 reset hard、不清理未归属改动。

总控 agent 启动后，第一轮不应该马上派发所有 worker。建议流程是：

1. 重新确认分支、hash、工作区状态和 merge-base。
2. 建立 merge-state。
3. 根据实际冲突刷新 P0/P1 文件归属。
4. 先派发 Worker A 和 Worker B，形成 credentials core / `Task` accessor 基座。
5. 再派发 Worker C 和 Worker D，分别处理 VFS/open/fd 与 exec credential。
6. 最后派发 Worker E，只处理 LTP 覆盖增量：已有本地 group 不重拆、不合并；
   缺失的 `credentials`、`chmod`、`chown` 可以作为新增 group 引入。
7. 串行集成每个 worker 结果，更新事务 devlog 的阶段条目。
8. 对每个已闭合阶段创建本地 checkpoint commit，commit message 标明阶段和范围。
9. 请求只读 reviewer 审查 P0/P1 语义闭合。
10. 构建 gate 通过后，把需要用户执行的 LTP 命令和预期日志路径列出来。

可直接给总控 agent 的启动 prompt：

```text
工作目录是仓库根目录。请作为 credentials merge 的总控 agent，
阅读 docs/src/rfcs/cred-merge/backgrounds/merge-context.md、
docs/src/rfcs/cred-merge/implementation.md 和
docs/src/devlog/transactions/2026-06-02-cred-merge.md。

目标是在 dev/drc/merge-cred 上把 origin/main 的 credentials 系统合入当前分支。
原则：credentials 语义以 origin/main 为准；credentials 以外的本地 syscall、VFS、
exec、mm、sched、LTP group 拆分和脚本修复默认以当前分支为准。

你可以启动子 agent，但必须按 docs/src/rfcs/cred-merge/implementation.md 的
worker write set 分工；未经批准不允许让 worker
越界修改。你不是独自在代码库里工作；不得 revert 用户或其他 agent 的改动。
你可以在 dev/drc/merge-cred 上使用 git 做本地版本管理，包括阶段性 checkpoint commit
和 worker 集成 commit；但不要 push、force-push、reset hard、clean，或改写其他分支。
你可以运行构建 gate，但不要运行 LTP，LTP 由用户手动执行。
每集成一个阶段都要更新 docs/src/devlog/transactions/2026-06-02-cred-merge.md。
遇到 docs/src/rfcs/cred-merge/implementation.md 的停止条件，停止并向用户报告，
不要自行拍板。

第一步只做前置检查、建立或确认 merge-state、刷新实际冲突地图，然后给出你准备启动的
worker 列表和顺序。不要直接一次性启动所有 worker。
```

## 阶段 2：写入型 worker 分工

### Worker A：Credentials Core + ABI

职责：恢复并接入 `origin/main` 的 credentials 核心和 syscall ABI。

write set：

- `anemone-abi/src/capability.rs`
- `anemone-abi/src/lib.rs`
- `anemone-abi/src/syscall/riscv.rs`
- `anemone-abi/src/syscall/loongarch.rs`
- `anemone-kernel/src/task/credentials/**`
- `anemone-kernel/src/task/api/mod.rs`
- `anemone-kernel/src/syscall/mod.rs`
- `anemone-kernel/src/syserror.rs`

语义要求：

- Linux ABI 数字、capget/capset/prctl struct 和常量只停留在 `anemone-abi`
  与 syscall parser 边界。
- kernel 内部继续使用 typed `Uid`、`Gid`、`Capability`、`SecureBits`、
  `CredentialSet`。
- 保留 `origin/main` 的 `capget`、`capset`、`prctl`、uid/gid/res/fs id、
  supplementary groups syscall 结构。
- syscall number 必须同时包含本地新增的 `statx`、`execveat`、clone3 等编号和
  远端新增的 credentials/capability/prctl 编号，不允许架构间遗漏。

禁止：

- 不要修改 `Task` 字段布局；`Task` 接入由 Worker B 负责。
- 不要在 kernel 内部散落 Linux capability magic number。

交付：

- 改动文件清单。
- syscall 编号合并说明。
- 仍需 Worker B/C/D 提供的接口清单。

### Worker B：Task Core + Lifecycle + Signal/Priority

职责：把 credentials 挂入 `Task`，同时保留本地 sched wait refactor 和 lifecycle 修复。

write set：

- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/task/api/clone/mod.rs`
- `anemone-kernel/src/task/api/exit/mod.rs`
- `anemone-kernel/src/task/api/priority.rs`
- `anemone-kernel/src/task/sig/api/mod.rs`
- `anemone-kernel/src/task/sig/api/kill.rs`
- `anemone-kernel/src/task/sig/api/tkill.rs`
- `anemone-kernel/src/task/sig/api/tgkill.rs`
- `anemone-kernel/src/task/sig/api/rt_sigqueueinfo.rs`
- `anemone-kernel/src/task/sig/info.rs`
- `anemone-kernel/src/task/topology/thread_group.rs`
- `anemone-kernel/src/mm/uspace/mod.rs`

语义要求：

- `Task` 同时保留本地的 `create_instant`、`sched_state`、wait-refactor 相关状态，
  以及远端的 `cred: RwLock<CredentialSet>`、`no_new_privs`、`nice`。
- credentials 读写只能通过 snapshot / transactional accessor：
  `cred()`、`replace_cred()`、`update_cred_with()`、`has_cap()`、
  `no_new_privs()`、`set_no_new_privs()` 等窄接口。
- 不引入 sched 状态锁与 credential lock 的新锁序耦合。
- clone/exec/exit 路径必须明确 credential 继承、替换和清理顺序。
- signal 的 `si_uid` 等用户可见字段使用正确 credential 来源。
- `detach_all_sysv_shm_for` 若采用远端无返回语义，exec/exit 调用点不能继续把它当成
  可失败事务处理。

禁止：

- 不要回退本地 `TaskSchedState` / `WaitState` / `WakeToken` 迁移。
- 不要把 permission policy 写进 `Task` 私有字段访问。

交付：

- `Task` 字段初始化点清单：普通 task、kernel task、idle task、clone。
- credential accessor 合同，供 Worker C/D 使用。
- 生命周期路径中仍需审查的问题。

### Worker C：VFS Permission + Open/Fd/Metadata

职责：合并 VFS permission、namei search check、typed openat、fd model、write/truncate
metadata 语义。

write set：

- `anemone-kernel/src/fs/permission.rs`
- `anemone-kernel/src/task/fs.rs`
- `anemone-kernel/src/fs/namei.rs`
- `anemone-kernel/src/fs/mod.rs`
- `anemone-kernel/src/fs/file.rs`
- `anemone-kernel/src/fs/inode.rs`
- `anemone-kernel/src/task/files.rs`
- `anemone-kernel/src/fs/api/access/**`
- `anemone-kernel/src/fs/api/openat.rs`
- `anemone-kernel/src/fs/api/fallocate.rs`
- `anemone-kernel/src/fs/api/fchmod/**`
- `anemone-kernel/src/fs/api/fchown/**`
- `anemone-kernel/src/fs/api/chdir/**`
- `anemone-kernel/src/fs/api/chroot.rs`
- `anemone-kernel/src/fs/api/mkdirat.rs`
- `anemone-kernel/src/fs/api/mount.rs`
- `anemone-kernel/src/fs/api/renameat2.rs`
- `anemone-kernel/src/fs/api/stat/newfstatat.rs`
- `anemone-kernel/src/fs/api/symlinkat.rs`
- `anemone-kernel/src/fs/api/truncate/**`
- `anemone-kernel/src/fs/api/umount.rs`
- `anemone-kernel/src/fs/api/unlinkat.rs`
- `anemone-kernel/src/fs/api/utimensat.rs`
- `anemone-kernel/src/fs/api/readlinkat.rs`

语义要求：

- 保留本地 `OpenHow`、`OpenAccessMode`、`FileStatusFlags`、
  `LinuxOpenCompat`、`O_PATH`、`O_NOFOLLOW`、fd access/status model。
- 保留远端 `FsPermChecker`，不能退化成“任意 owner/group/other 位存在即可”。
- ordinary VFS operation 使用 fsuid/fsgid/supplementary groups/effective caps。
- `access(2)`、`faccessat(2)`、`AT_EACCESS` 必须保留 real-id/effective-id 区分。
- namei 遍历每个目录组件时检查 search/execute 权限，包括普通组件、`.`、`..`。
- create/tmpfile parent 必须检查 `WRITE|EXECUTE`。
- `O_NOATIME` 必须检查 owner 或 `CAP_FOWNER`。
- write/truncate/chown/chmod 后按当前 credential 触发 setuid/setgid drop，
  `truncate(size, &cred)` 调用链不能被无 credential 旧路径绕过。
- `readlinkat` 保留本地“不写 NUL”修复，同时使用合并后的 checked path 入口。
- `newfstatat` 保留本地内部 stat -> Linux ABI 转换边界，同时接入远端 path validator。

禁止：

- 不要恢复旧 `FileFlags` fd 模型。
- 不要让 filesystem driver 自己承担 VFS permission policy。
- 不要用 root/non-root 二值判断代替 capability/DAC 规则。

交付：

- `openat` 三方合并说明：本地保留项、远端移植项、冲突处理。
- 所有 `truncate` / write metadata 调用点清单。
- access/namei/open/chmod/chown 的 permission checker 路径图。

### Worker D：Exec Credentials + PathRef

职责：把远端 exec credential transition 移植到本地 `PathRef`/`execveat` loader 流。

write set：

- `anemone-kernel/src/task/api/execve/**`

语义要求：

- 保留本地 `kernel_execve_from_pathref()`、`execveat`、`PathRef` 解析边界、
  shebang 修复、empty argv 处理、`/proc/<pid>/cmdline` range 记录。
- 在 resolved `PathRef` 上接入远端 exec permission check、setuid/setgid、
  file capabilities、ambient/bounding/permitted/effective/inheritable capability、
  securebits、`no_new_privs`、`secure_exec`。
- 新 credential 应在旧 credential + executable metadata 计算后进入 loaded binary
  metadata；只有 exec 成功到提交点后才能替换当前 task credential。
- auxv / secure exec 语义保留远端 credentials 逻辑，同时不破坏本地 init stack 修复。

禁止：

- 不要把 `PathRef` 回退成重新从用户字符串解析。
- 不要在装载失败路径提前替换 task credential。
- 不要把 capability 约束简化成 setuid root。

交付：

- exec 成功路径上的 credential 计算和提交顺序。
- shebang/interpreter 路径的 permission 检查位置。
- 与 Worker B `Task` accessor 的依赖说明。

### Worker E：User-test Harness + Fixtures

职责：在不改动本地已有 LTP group 拆分的前提下，新增 credentials / chmod / chown
测试覆盖。

write set：

- `anemone-apps/user-test/fixtures/passwd`
- `anemone-apps/user-test/fixtures/group`
- `anemone-apps/user-test/ltp/groups/credentials.txt`
- `anemone-apps/user-test/ltp/groups/chmod.txt`
- `anemone-apps/user-test/ltp/groups/chown.txt`
- `anemone-apps/user-test/ltp/profile.txt`
- `anemone-apps/user-test/src/ltp.rs`

语义要求：

- 本地当前已经存在的 group 是边界，不能因为 merge 重新拆分、合并或改名。
- 当前本地缺失的 `credentials`、`chmod`、`chown` 可以作为新增 group 单独引入，
  并在 `LTP_GROUPS` 中注册。
- 远端旧 `process-exec` 不恢复；exec 相关覆盖应继续进入本地已有 `exec` group。
- 如果远端 case 属于本地已有 group，按本地现有 group 形状追加或保持现状；不要把
  远端旧 group layout 当作 canonical 形状覆盖本地结构。
- 保留本地 `all`、clone、exec、futex、open、shm、tmp 等 profile 能力。
- `ACTIVE_PROFILE` 是 compile-time `include_str!`，用户每次改 profile 后会自行重建
  `user-test` 并执行 LTP。
- `credentials.txt` 中如果存在 `case => executable args...` 这类语法，必须确认当前
  parser 是否支持；不支持时先不要把它当成可运行 alias。

禁止：

- 不要修改、拆分、合并或改名本地已经存在的 group，除非用户另行明确要求。
- 不要恢复远端旧 `process-exec` group。
- 不要为了让 profile smoke 通过删除 credentials / chmod / chown 覆盖。
- 不要启动 agent 执行 LTP；LTP 由用户手动运行。

交付：

- `credentials` / `chmod` / `chown` group 注册表和 `profile.txt` 一致性说明。
- 新增 case 与本地现有 group 的关系说明。
- 需要用户手动验证的 profile / group 建议。

## 阶段 3：总控集成顺序

建议顺序：

1. 集成 Worker A：credentials core 和 ABI。
2. 集成 Worker B：`Task` shell、lifecycle、signal/priority。
3. 并行集成 Worker C 和 Worker D，但由总控串行应用 diff。
   如果 C/D 对 `Task` accessor 有不同假设，以 Worker B 的 accessor 合同为准。
4. 集成 Worker E：测试 harness。
5. 总控跑文件形状检查。
6. 只读 reviewer agent 审查 P0/P1 语义冲突是否闭合。
7. 总控把建议验证命令和预期风险写入事务 devlog，等待用户执行 LTP。

总控每集成一个 worker 后至少检查：

```bash
git status --short
git diff --check
```

对于 P0 文件，额外做三方局部审查：

```bash
git show 210a9e07d1c8381ac5913298c8d6f26daf878581:<path>
git show HEAD:<path>
git show origin/main:<path>
```

## 阶段 4：只读 reviewer

reviewer 不改文件，只检查以下问题：

- `Task` 同时保留 credentials state 和 sched wait state，且没有新锁序风险。
- VFS permission 是否统一走 `FsPermChecker`，没有回到 bit 粗判。
- `access/faccessat` real/effective-id 语义是否与普通 open 分离。
- namei 是否逐组件执行 search/execute check。
- write/truncate/chown/chmod 是否都触发 setuid/setgid drop。
- exec credential 是否只在成功提交点替换 task cred。
- `PathRef`/`execveat`/typed `openat`/fd model 是否被保留。
- syscall ABI 表在 riscv64 和 loongarch64 是否等价扩展。
- `profile.txt` 中所有 group 是否都在 `LTP_GROUPS` 注册；新增 group 是否只作为
  增量引入，且没有重拆或合并本地已有 group。
- 是否出现临时兼容层；如果有，必须记录到 devlog，避免后续误认为永久接口。

reviewer 输出按 P0/P1/P2：

- P0：必须在 merge 提交前修复。
- P1：原则上提交前修复；如果要延期，必须有明确原因和验证影响。
- P2：可记录为后续工作或 register 限制。

## 阶段 5：用户验证建议

LTP 不交给 agent 执行。agent 的职责是保证代码形状、构建 gate、失败归类说明和事务
devlog 记录清楚；rv64 / la64 LTP 由用户手动运行并把日志结果反馈给总控。

构建仍遵守仓库约束：构建、rootfs、QEMU、app build 均通过 `just` 或 `just xtask`，
不要裸跑 `cargo`。

最小编译 gate：

```bash
just xtask app build user-test --arch riscv64
just conf switch qemu-virt-rv64
just build
```

由于本次触及 syscall number / ABI 表，还要做 loongarch64 编译 gate：

```bash
just conf switch qemu-virt-la64
just build
```

建议用户执行的 rv64 user-test 主路径：

```bash
./scripts/run-user-test-rv64.sh <preliminary-rv64-image> build/ltp-cred-merge-rv64.log
```

建议用户执行的可选 la64 user-test：

```bash
./scripts/run-user-test-la64.sh <preliminary-la64-image> build/ltp-cred-merge-la64.log
```

### LTP 分层建议

profile 是编译期嵌入。若用户临时修改 `profile.txt` 或 `tmp.txt` 做分层验证，每次都
需要重建 `user-test`。agent 不应替用户修改 profile 并运行 LTP，除非用户在后续明确
要求。

1. profile/parser smoke：

```text
credentials
chmod
chown
open
exec
shm
```

```bash
just xtask app build user-test --arch riscv64
./scripts/run-user-test-rv64.sh <preliminary-rv64-image> build/ltp-profile-smoke-rv64.log
```

2. cred-core 窄集合，优先使用 `tmp`：

```text
getuid01
geteuid01
getgid01
getegid01
setuid01
setgid01
access01
faccessat01
open02
open08
open10
openat04
chmod01
fchmod01
chown01
fchown01
execve03
shmat02
shmctl02
shmctl04
shmget04
```

```bash
./scripts/run-user-test-rv64.sh <preliminary-rv64-image> build/ltp-cred-core-rv64.log
```

3. group 拆分：

```text
credentials
```

```bash
./scripts/run-user-test-rv64.sh <preliminary-rv64-image> build/ltp-credentials-rv64.log
```

```text
chmod
chown
```

```bash
./scripts/run-user-test-rv64.sh <preliminary-rv64-image> build/ltp-mode-owner-rv64.log
```

```text
open
exec
```

```bash
./scripts/run-user-test-rv64.sh <preliminary-rv64-image> build/ltp-open-exec-rv64.log
```

```text
shm
```

```bash
./scripts/run-user-test-rv64.sh <preliminary-rv64-image> build/ltp-shm-rv64.log
```

```text
memory
```

```bash
./scripts/run-user-test-rv64.sh <preliminary-rv64-image> build/ltp-memory-rv64.log
```

当前本地没有的 `credentials`、`chmod`、`chown` 可以单独成组；本地已经存在的 group
不要因为 merge 被重拆、合并或改名。`all` 只作为最终广覆盖扫尾，不能作为唯一
merge gate。

### 失败归类

优先归为 credentials merge 回归：

- `getuid/geteuid/getgid/getegid/setuid/setgid` unknown syscall、panic、返回值错误。
- `access/faccessat/open02/open08/open10/openat04` 中 uid/gid、mode bit、
  owner/group class、capability bypass 错误。
- `chmod/fchmod/chown/fchown/fchownat/lchown` 的权限拒绝、元数据更新、ctime/mode 错误。
- `execve03` 一类 execute-bit / `EACCES` 退化。
- `shmat02/shmctl02/shmctl04/shmget04` 在已接 IPC permission hook 后仍失败。

不要直接归为 credentials merge 回归：

- `capget/capset/prctl keepcaps/filecaps/userns*`，除非本 merge 明确覆盖这些完整语义。
- `execve04`：已有 VFS writer accounting 限制。
- `shmctl03/shmget02/shmget03/shmget05/shmget06/shmat01`：多半涉及 `/proc/sys*`、
  `/proc/sysvipc/shm`、kconfig、rlimit 等基础设施。
- `shmctl01`：已知与 iomux/procfs sleep observability 相关。
- `mmap04/mmap10/mmap12/mmap14/mmap18/munmap03`：memory 组的 procfs、`/dev/zero`、
  rlimit、`MAP_GROWSDOWN` 限制。
- rootfs 构建、sudo/libguestfs、镜像缺失、QEMU 启动失败：基础设施问题。

## 停止条件

出现以下任一情况，总控应停止并让人拍板：

- `openat` 无法同时保留 typed parser/fd model 和 `FsPermChecker` DAC 语义。
- exec credential 计算与 `PathRef`/`execveat` loader 的提交顺序无法闭合。
- `Task` credential lock 与 sched wait state 出现真实锁序不确定性。
- worker 需要扩大 write set 才能继续，且尚未获得批准或记录。
- 用户反馈的 cred-core 窄集合日志出现无法归入既有限制的权限/uid/gid/exec 回归。
- 需要引入临时兼容层，但没有明确的后续删除点和 devlog 记录位置。

## 最终提交前检查清单

- credentials module、capability ABI、uid/gid/group/cap/prctl syscall 均存在。
- riscv64 / loongarch64 syscall 表都包含本地和远端新增项。
- `Task` 初始化、clone、exec、exit 都有明确 credential 语义。
- typed `openat`、`O_PATH`、`O_NOFOLLOW`、fd status/access model 未回退。
- `FsPermChecker` 是 VFS permission 的中心入口。
- checked namei search 权限未被绕过。
- `truncate(size, &cred)` / write metadata 路径完整。
- exec 只在成功提交点替换 credential。
- LTP `credentials` / `chmod` / `chown` group 注册和 profile 一致，且没有重拆或合并
  本地已有 group。
- 所有临时兼容层写入 devlog。
- `git diff --check` 通过。
- 最小 build gate 通过。
- 用户执行的 cred-core LTP gate 通过，或失败已按上面的失败归类记录。

## 可直接使用的 worker prompt 模板

所有 worker prompt 前缀：

```text
工作目录是仓库根目录。你不是独自在代码库里工作；不要 revert
别人或其他 agent 的改动。未经批准只允许修改本任务列出的 write set。遇到必须扩大
write set 的依赖，停止并向总控提交扩展申请。merge 原则：credentials 语义以 origin/main 为准，
credentials 以外的本地 bugfix 和重构默认以当前 HEAD 为准。最终回复必须列出：
改动文件、保留的本地语义、移植的 origin/main 语义、未闭合问题、已跑的构建检查；
不要运行 LTP，LTP 由用户手动执行。
```

然后追加对应 Worker A/B/C/D/E 的职责、write set、语义要求和禁止项。
