# Task 当前契约

**Owner：** task topology / ThreadGroup lifecycle / user-task transition protocols
**覆盖范围：** 本轮按触达提取的 process-group signal selection、ThreadGroup terminal lifecycle、child wait 和 ordinary user-entry 规则
**不覆盖：** task 全领域不变量、scheduler physical state、future Unix job-control phase、TTY 或 ptrace
**最后核验：** 2026-07-20

本目录只登记已经迁移到 contract 层的共享规则，不声称枚举 task 子系统全部不变量。

## Contract Surfaces

- [Process-group signal targeting](./process-group-signaling.md)：ProcessGroup 只选择 ThreadGroup，实际 signal publication 独立发生。
- [ThreadGroup lifecycle](./thread-group-lifecycle.md)：`Alive / Exiting / Exited`、member detach、exit-code 与 waitability。
- [Child wait](./child-wait.md)：exited-child truth、selection、Event 重扫和 peek / reap claim。
- [Ordinary user entry](./user-entry.md)：RV64 / LA64 ordinary trap-return 的 Signal arbitration 与 architecture transition 顺序。

## 邻接契约

- [Signal 当前契约](../signal/index.md)：pending occurrence 与 ordinary action selection。
- [Procfs 当前契约](../procfs/index.md)：TGID task-state ABI projection。
