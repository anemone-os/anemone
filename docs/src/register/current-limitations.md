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

## ANE-20260523-TRUNCATE-MMAP-COHERENCY

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** VFS / truncate / mmap

**Summary:** 当前 truncate 会更新 inode 大小并裁剪驻留文件页缓存，但不会主动失效已经安装到用户地址空间的文件映射，因此 live mmap 下不承诺 Linux 级的强一致性或完整 SIGBUS 语义。

**Exit Condition:** 为文件映射补齐 shrink 场景下的映射失效或回收路径，并明确验证 truncate 与 mmap 在 grow、shrink 和并发访问下的可见性语义。

**Owner:** doruche
**Last Verified:** 2026-05-23
**Related:** [开发日志：2026-05-11 至 2026-05-24](../devlog/2026-05-11_to_2026-05-24.md)

## ANE-20260523-EXT4-TRUNCATE-CACHE-INVALIDATION

**Type:** Limitation
**Status:** Active
**Severity:** Low
**Area:** ext4 / truncate / page cache

**Summary:** 当前 ext4 truncate 在更新磁盘镜像后，会按页粒度失效“可见字节范围发生变化”的 resident page cache，而不是对边界页做原位修补并继续信任其内存内容。

**Exit Condition:** 把之前 shrink-then-extend 暴露旧字节的问题继续收敛到明确根因，并以可靠的边界页原位修补或更强的一致性不变量替换当前的页粒度失效策略，同时重新验证 resident page cache 与 truncate grow/shrink 的可见性语义。

**Owner:** doruche
**Last Verified:** 2026-05-23
**Related:** [开发日志：2026-05-11 至 2026-05-24](../devlog/2026-05-11_to_2026-05-24.md), [当前限制](./current-limitations.md)
