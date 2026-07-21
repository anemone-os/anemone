# Task 当前契约

**Owner：** task topology / ThreadGroup lifecycle / user-task transition protocols
**覆盖范围：** 本轮按触达提取的process-group signal selection、ThreadGroup terminal lifecycle、Unix job control、child wait和user-entry规则
**不覆盖：** task全领域不变量、scheduler physical state、TTY、orphaned-process-group policy或ptrace
**最后核验：** 2026-07-21

本目录只登记已经迁移到 contract 层的共享规则，不声称枚举 task 子系统全部不变量。

## Contract Surfaces

- [Process-group signal targeting](./process-group-signaling.md)：ProcessGroup 只选择 ThreadGroup，实际 signal publication 独立发生。
- [ThreadGroup lifecycle](./thread-group-lifecycle.md)：`Alive / Exiting / Exited`、member detach、exit-code 与 waitability。
- [Unix job control](./job-control.md)：ThreadGroup-owned stop / continue phase、user exposure、control-signal handoff、lifecycle cleanup和parent report。
- [Child wait](./child-wait.md)：terminal与job-control child status、selection、Event重扫和peek / consume / reap claim。
- [User entry](./user-entry.md)：RV64 / LA64 ordinary、fresh、clone和exec entry的统一Signal/lifecycle/jobctl arbitration。

## 邻接契约

- [Signal 当前契约](../signal/index.md)：pending occurrence 与 ordinary action selection。
- [Procfs 当前契约](../procfs/index.md)：TGID task-state ABI projection。
