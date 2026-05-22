# 当前限制

本页记录当前已接受的限制。这些条目不是未知异常，而是当前阶段明确存在、后续需要系统性收敛的能力缺口。

## ANE-20260522-OTMPFILE-STAGE1

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** VFS / openat

**Summary:** 当前 O_TMPFILE 采用 create-open-unlink 的 stage-1 仿真实现，不具备真正匿名 inode、强原子性或后续 link 回目录的完整语义。

**Exit Condition:** 实现文件系统支撑的无名临时 inode，并补齐 linkat、AT_EMPTY_PATH 与 O_EXCL 相关语义。

**Owner:** doruche
**Last Verified:** 2026-05-22
**Related:** [开发日志：2026-05-11 至 2026-05-24](../devlog/2026-05-11_to_2026-05-24.md)
