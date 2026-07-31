# Build Configuration 当前契约

**Owner：** canonical build configuration objects 与 `scripts/xtask` system resolver
**覆盖范围：** Platform / SystemTarget / KernelConfig / BuildPreset 的配置事实分层，以及本次
system action 的 resolved selection snapshot
**不覆盖：** Platform DT/QEMU delivery、kernel output、app/rootfs build、Boot Protocol runtime、
具体 build action workflow 或尚未生效的 network deployment schema
**最后核验：** 2026-07-29

本目录只登记已经由 System Target Model 实现、并在后续 RFC 首次复用时按触达提取的最小共享规则；
不批量迁移该 RFC 的全部 build、DT、workflow 或 repository-surface invariant。

## Contract Surfaces

- [System target 与 resolved selection](./system-target.md)：`STM-OWNER-001`、
  `STM-TARGET-001` 与 `STM-RESOLVE-001`。

## 邻接契约

- [Anemone Boot Protocol](../task/boot-protocol.md)：`BOOT-PROTOCOL-001`拥有 typed initial-program
  source 从 SystemTarget selection 到 kernel ordinary exec 的 handoff。
