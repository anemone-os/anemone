# Procfs 当前契约

**Owner：** procfs ABI projection
**覆盖范围：** 本轮按触达提取的 TGID task-state display
**不覆盖：** procfs 全字段、binding lifetime、task enumeration、signal / memory / scheduler accounting
**最后核验：** 2026-07-20

本目录只登记已经从 live code 提取、且会被后续 RFC 改变或复用的 procfs 规则，不声称枚举 procfs 全部 ABI。

## Contract Surfaces

- [TGID task-state projection](./task-state-projection.md)：`/proc/<tgid>/stat` 与 `/proc/<tgid>/status` 的 current state 来源和映射。

## 邻接契约

- [Task 当前契约](../task/index.md)：ThreadGroup lifecycle 与 user-task transition。
