# Scheduler 当前契约

**Owner：** scheduler core / owner-CPU placement protocol
**覆盖范围：** wait logical completion 后的 physical-placement handoff、owner-CPU revalidation 与 scheduler-local 诊断
**不覆盖：** scheduler class policy、task migration、Event listener queue、Latch source lifecycle、通用 IPI barrier 和 IRQ-off allocation
**最后核验：** 2026-07-25

本目录只登记已经迁移到 contract 层、会被 wait core、scheduler request 或其它 producer
共同依赖的规则，不声称枚举 scheduler 的全部不变量。

## Contract Surfaces

- [Asynchronous wake delivery](./wake-delivery.md)：logical completion、placement handoff、remote obligation 生命周期与 owner-CPU stale-safe placement。

## 邻接契约

- [Task 当前契约](../task/index.md)：task / ThreadGroup 生命周期、child wait 和 user entry。
- [Signal 当前契约](../signal/index.md)：signal occurrence 与 temporary-mask delivery；Signal 只作为 wait completion producer。
