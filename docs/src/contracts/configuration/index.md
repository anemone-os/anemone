# Build Configuration 当前契约

**Owner：** canonical build configuration objects、`scripts/xtask` system resolver 与 kernel consumers
**覆盖范围：** Platform / SystemTarget / KernelConfig / BuildPreset 的配置事实分层，以及本次
system action 的 resolved selection snapshot、kernel 参数语义合法性
**不覆盖：** Platform DT/QEMU delivery、kernel output、app/rootfs build、Boot Protocol runtime、
具体 build action workflow 或 runtime 输入校验
**最后核验：** 2026-08-15

本目录登记已经生效、会被后续配置与 kernel consumer 工作共同依赖的最小共享规则；不把 build、DT、
workflow 或 repository surface 的全部局部约束批量提升为 contract。

## Contract Surfaces

- [System target 与 resolved selection](./system-target.md)：`STM-OWNER-001`、
  `STM-TARGET-001` 与 `STM-RESOLVE-001`，包括required Nemophila embedded module selection。
- [Kernel 参数合法性](./kernel-parameter-validation.md)：`KCONFIG-VALIDATION-001`。

## 邻接契约

- [Anemone Boot Protocol](../task/boot-protocol.md)：`BOOT-PROTOCOL-001`拥有 typed initial-program
  source 从 SystemTarget selection 到 kernel ordinary exec 的 handoff。
