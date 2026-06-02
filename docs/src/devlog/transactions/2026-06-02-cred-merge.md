# 2026-06-02 - Cred Merge

**Status:** Active
**Owners:** doruche, Codex
**Area:** credentials / task / VFS / exec / syscall ABI / user-test
**Current Phase:** planning complete; merge execution pending

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
