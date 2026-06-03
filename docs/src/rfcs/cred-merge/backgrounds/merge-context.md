# Merge 背景与风险

本次工作需要把本地分支 `dev/drc/chaos` 与最新 `origin/main` 合并。`origin/main`
的倒数第二个 commit 已经位于本地分支历史中；最新一个 commit 由另一位开发者编写，
引入了四千余行 credentials 相关变更。

两个分支的最近共同祖先是 `210a9e07d1c8381ac5913298c8d6f26daf878581`。在该祖先之后，
本地分支已经完成：

- 重构 `openat` syscall，引入类型化解析。
- 修复大量 syscall 层 bug，包括 `execve`、`mmap` 等路径。
- 重构调度层，见 [Sched Wait Refactor](../../sched-wait-refactor/index.md)。
- 引入 loongarch64 端到端测试脚本、`clone3` 等其他能力。

同一期间，`origin/main` 的最新 commit 引入较完整的 credentials 系统，包括
capabilities、uid/gid/euid/egid/fsuid/fsgid 等 id 语义，以及相关 syscall 适配。
为了支持本地测试，该 commit 也修复了一批 bug，其中部分修复与本地分支已经完成的修复重叠。

本次 merge 的原则是：credentials 系统本身以 `origin/main` 为语义来源；credentials
以外的 bugfix、重构和功能引入默认以本地分支为准。实际 merge 在新分支
`dev/drc/merge-cred` 上进行，该分支创建时与本地分支一致。

主要风险包括：

- credentials 接入后是否破坏已有跨子系统不变量；
- git 静默接受的语义冲突，即文本无冲突但合并结果把两边语义拼错；
- 远端 credentials 测试辅助修复覆盖本地已验证的 syscall、VFS、exec、mm、sched 或
  user-test 边界。
