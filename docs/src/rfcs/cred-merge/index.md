# RFC-20260602-cred-merge

**状态：** 已接受，merge checkpoint 与 reviewer P0/P1 修复已完成，LTP 待用户验证
**负责人：** doruche, Codex
**最后更新：** 2026-06-03
**领域：** credentials / task / VFS / exec / syscall ABI / user-test
**事务日志：** [2026-06-02 - Cred Merge](../../devlog/transactions/2026-06-02-cred-merge.md)
**开放问题：** 当前没有未闭合的文本冲突或 reviewer P0/P1；剩余工作是用户执行 rv64 / la64 LTP 并按事务日志归类结果。
**下一步：** 等待用户提供 LTP 日志；如出现无法归入既有限制的权限、uid/gid 或 exec 回归，再按本 RFC 的 merge 原则做窄修。

## 摘要

本 RFC 记录 credentials feature merge 的共享执行计划：在
`dev/drc/merge-cred` 上把 `origin/main` 引入的 credentials 系统并入本地分支，同时保留本地分支在 credentials 以外已经验证过的 syscall、VFS、exec、mm、sched 和测试脚本修复。

这次 merge 的主要风险不是普通文本冲突，而是 git 静默接受的语义冲突：远端 credentials 语义可能和本地 typed `openat`、fd access/status model、`PathRef`/`execveat`、sched wait refactor、LTP group 拆分等同时改动的边界拼错。

## 文档地图

Canonical：

- [实施计划](./implementation.md)：merge 原则、worker write set、集成顺序、reviewer 检查清单、验证建议和失败归类规则。

背景材料：

- [背景材料索引](./backgrounds/index.md)
- [Merge 背景与风险](./backgrounds/merge-context.md)

## 接受边界

本 RFC 已作为 credentials merge 的 canonical source 被接受。事务日志记录实际执行进度、checkpoint、构建 gate、reviewer 结果和用户验证状态；本 RFC 保留用于解释这些记录背后的 merge 原则和审查合同。

如果后续需要改变 credentials 与本地 typed open/fd、exec `PathRef`、Task lifecycle、syscall ABI 或 user-test group 边界的合并原则，应先更新本 RFC 或新增 follow-up RFC，再把结论写入事务日志。
