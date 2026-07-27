# Scheduler 当前契约

**Owner：** scheduler wait adapter / logical-completion 与 owner-CPU placement protocols
**覆盖范围：** 单轮 Latch wait lifecycle、wait logical completion 后的 physical-placement handoff、owner-CPU revalidation 与 scheduler-local 诊断
**不覆盖：** scheduler class policy、task migration、Event listener queue、pollable-source readiness lifecycle、通用 IPI barrier 和 IRQ-off allocation
**最后核验：** 2026-07-26

本目录只登记已经迁移到 contract 层、会被 wait core、scheduler request 或其它 producer
共同依赖的规则，不声称枚举 scheduler 的全部不变量。

## Contract Surfaces

- [Asynchronous wake delivery](./wake-delivery.md)：logical completion、placement handoff、remote obligation 生命周期与 owner-CPU stale-safe placement。
- [Latch wait round](./latch-wait-round.md)：单轮 OR wait identity、consumer lifecycle 与 producer trigger capability。

## 邻接契约

- [Task 当前契约](../task/index.md)：task / ThreadGroup 生命周期、child wait 和 user entry。
- [Signal 当前契约](../signal/index.md)：signal occurrence 与 temporary-mask delivery；Signal 只作为 wait completion producer。
- [I/O Multiplexing 当前契约](../iomux/index.md)：iomux scan/register/final-recheck 与 source readiness publication。
