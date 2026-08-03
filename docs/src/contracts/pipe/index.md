# Pipe 当前契约

**Owner：** Pipe session / endpoint participation / byte data plane / readiness projection
**覆盖范围：** 本轮按触达提取的 filesystem-backed named FIFO open handoff、resident inode rendezvous、live session lifecycle 与共享 Pipe data plane
**不覆盖：** pipe 全领域不变量、per-user resource accounting、packet mode、zero-copy splice/vmsplice/tee、procfs pipe controls或 Unix Socket stream
**最后核验：** 2026-08-03

本目录只登记已经迁移到 contract 层的 Pipe 共享规则，不声称枚举 anonymous pipe 或 Pipe 全部不变量。

## Contract Surfaces

- [Named FIFO open 与 lifecycle](./named-fifo.md)：VFS final admission、pending participation、inode weak anchor、session teardown与 Pipe data-plane复用。

## 邻接契约

- [VFS Creation 与 Make Node](../vfs/make-node.md)：filesystem-backed FIFO identity 与 node publication。
- [File kind 与 Linux mode projection](../vfs/file-kind.md)：immutable `InodeType::Fifo` 与 `S_IFIFO` projection。
- [Opened-description lifecycle](../task/opened-description-lifecycle.md)：dup/fork sharing、fd publication与 terminal final release。
- [I/O Multiplexing poll wait](../iomux/poll-wait.md)：source-owned predicate、route registration与 final recheck。
