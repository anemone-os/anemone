# System Power 当前契约

**Owner：** `power` terminal invocation 与跨 subsystem shutdown sequencing
**覆盖范围：** 当前 orderly power-off/reboot、panic handoff 与最终 machine-handler capability
**不覆盖：** proposed terminal episode、通用 writeback、driver-local shutdown 语义或 machine firmware ABI
**最后核验：** 2026-07-26

本目录只登记已经迁移到 contract 层的 system-power 共享规则，不声称已经枚举 filesystem、storage、
device lifecycle 或 architecture machine action 的全部不变量。

## Contract Surfaces

- [Shutdown lifecycle](./shutdown-lifecycle.md)：当前 orderly、panic/emergency 与 machine-handler
  执行链；pending target 由 System Power RFC 保存。

## 邻接契约

- 当前 filesystem shutdown、device tree traversal 与 concrete driver callback 尚无独立 stable contract
  ID；本 surface 只记录 `power` 对这些 owner facade 的当前调用顺序，不接管其内部状态。
