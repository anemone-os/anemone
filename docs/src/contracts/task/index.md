# Task 当前契约

**Owner：** task topology / ThreadGroup lifecycle / kthread cooperative wait / kworker boot topology、system queue与drain protocol / fd-table opened-description与POSIX holder lifecycle / user-task transition protocols
**覆盖范围：** 本轮按触达提取的process-group signal selection、ThreadGroup terminal lifecycle、Unix job control、child wait、kthread timed wait、boot-fixed kworker submission/execution、opened-description lifecycle、file-table POSIX holder/cleanup、initial user-program boot和user-entry规则
**不覆盖：** task全领域不变量、VFS inode/file backend lifecycle、scheduler physical state、TTY、orphaned-process-group policy或ptrace
**最后核验：** 2026-08-05

本目录只登记已经迁移到 contract 层的共享规则，不声称枚举 task 子系统全部不变量。

## Contract Surfaces

- [Process-group signal targeting](./process-group-signaling.md)：ProcessGroup 只选择 ThreadGroup，实际 signal publication 独立发生。
- [ThreadGroup lifecycle](./thread-group-lifecycle.md)：`Alive / Exiting / Exited`、member detach、exit-code 与 waitability。
- [Unix job control](./job-control.md)：ThreadGroup-owned stop / continue phase、user exposure、control-signal handoff、lifecycle cleanup和parent report。
- [Child wait](./child-wait.md)：terminal与job-control child status、selection、Event重扫和peek / consume / reap claim。
- [Kthread timed wait](./kthread-wait.md)：cooperative stop/deadline完成、ordinary-wake重查与stale timeout isolation。
- [Kworker](./kworker.md)：boot-fixed per-CPU system worker、submission ownership/FIFO、IRQ handoff与process-context execution。
- [Opened-description lifecycle](./opened-description-lifecycle.md)：terminal published-slot lifecycle、non-owning identity/liveness capability、dup/fork sharing、flock retirement handoff 与 final release。
- [File-table POSIX lock](./file-table-posix-lock.md)：sharing-episode holder identity、fork/share/unshare/exec topology与任意相关fd removal cleanup。
- [Anemone Boot Protocol](./boot-protocol.md)：rootfs metadata选择初始用户程序、kernel boot准备与ordinary exec handoff。
- [User entry](./user-entry.md)：RV64 / LA64 ordinary、fresh、clone和exec entry的统一Signal/lifecycle/jobctl arbitration。

## 邻接契约

- [Signal 当前契约](../signal/index.md)：pending occurrence 与 ordinary action selection。
- [Procfs 当前契约](../procfs/index.md)：TGID task-state ABI projection。
- [I/O Multiplexing 当前契约](../iomux/index.md)：poll wait 对 opened-description identity 与 source readiness 的依赖。
- [VFS POSIX record lock](../vfs/posix-record-lock.md)：inode-associated range/conflict truth与blocking predicate recheck。
