# TTY 当前契约

**Owner：** `device::tty` terminal data plane 与 UART/TTY handoff protocol
**覆盖范围：** 已经 cut over 的 serial port capability、共享 Terminal、input/output data plane 与 endpoint publication
**不覆盖：** controlling-terminal relation、`/dev/tty`、foreground/background policy、terminal-generated signal、relation cleanup、完整 BusyBox ash/job-control ABI 或 PTY
**最后核验：** 2026-07-23

本目录只登记已经由 `TTY-DATA-CUTOVER` 生效的共享规则，不声称第一版 TTY RFC 已经完成。
`TTY-REL-001`、`TTY-JOBCTL-001`、`TTY-LIFE-001` 与 `TTY-ABI-001` 仍为 Not Cut Over。

## Contract Surfaces

- [Serial TTY data plane](./data-plane.md)：UART capability handoff、共享 Terminal、input/output 与稳定 endpoint publication。

## 邻接契约

- [Signal 当前契约](../signal/index.md)：pending occurrence 与 ordinary action selection；Stage 2 尚未接入 terminal signal。
- [Task 当前契约](../task/index.md)：process-group、lifecycle、job control 与 user-entry owner；Stage 2 不建立 controlling relation。
