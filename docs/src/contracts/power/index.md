# System Power 当前契约

**Owner：** `power` terminal episode、跨 subsystem shutdown sequencing 与 machine-handler registry
**覆盖范围：** 当前 orderly power-off/reboot、panic/emergency handoff 与最终 machine-action attempt
**不覆盖：** strong durability、userspace lifecycle、driver-local shutdown 完整性或 machine firmware ABI
**最后核验：** 2026-07-26

本目录只登记已经迁移到 contract 层的 system-power 共享规则，不声称已经枚举 filesystem、storage、
device lifecycle 或 architecture machine action 的全部不变量。

## Contract Surfaces

- [Shutdown lifecycle](./shutdown-lifecycle.md)：`SYSTEM-POWER-EPISODE-001`、orderly 静态 plan、
  panic/emergency 隔离与共享 machine-handler fallback。

## 邻接契约

- 当前 filesystem shutdown、device tree traversal 与 concrete driver callback 尚无独立 stable contract
  ID；本 surface 只记录 `power` 对这些 owner facade 的当前调用顺序，不接管其内部状态。
