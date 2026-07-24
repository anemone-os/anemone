# Signal 当前契约

**Owner：** Signal pending / disposition protocol
**覆盖范围：** signal occurrence 的 private / shared pending 路由、temporary-mask delivery handoff、ordinary action selection，以及 control-signal generation handoff
**不覆盖：** ThreadGroup-owned job-control phase / report、terminal lifecycle、wait ABI
**最后核验：** 2026-07-21

本目录只登记已经从 live code 提取、且会被后续 RFC 跨模块引用的 Signal 规则，不声称枚举 Signal 全部语义。

## Contract Surfaces

- [Pending routing 与 ordinary action selection](./pending-routing.md)：private / shared pending 真相源、group-directed routing、ignored admission、ordinary action selection和control-signal generation handoff。
- [Temporary-mask delivery handoff](./temporary-mask-delivery.md)：task-owned restore slot、reserved delivery target 与 handler / no-frame cleanup。

## 邻接契约

- [Task 当前契约](../task/index.md)：process-group target selection、ThreadGroup lifecycle、child wait 和 user-entry 边界。
- [Unix job control](../task/job-control.md)：ThreadGroup-owned stop / continue phase、exposure、report和Signal/jobctl handoff。
