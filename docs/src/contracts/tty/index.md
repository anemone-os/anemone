# TTY 当前契约

**Owner：** `device::tty` data plane、controlling relation与terminal-access protocol；参与的task/Signal/job-control owner保持独立
**覆盖范围：** serial port capability、共享Terminal、input/output、endpoint publication、controlling relation、foreground/background policy、terminal-generated signal与首版BusyBox ash/vi ABI
**不覆盖：** PTY/devpts/ptmx、orphaned-process-group effect、relation-disassociation signal、`TOSTOP`、hardware hangup/runtime line configuration或procfs TTY字段
**最后核验：** 2026-07-24

本目录只登记已经cut over的共享规则。`TTY-DATA-CUTOVER`与`TTY-JOBCTL-CUTOVER`现均为Effective；
第一版TTY R1已经关闭，但未列入本目录的corner仍以各contract接受边界和register为准。

## Contract Surfaces

- [Serial TTY data plane](./data-plane.md)：UART capability handoff、共享 Terminal、input/output 与稳定 endpoint publication。
- [TTY controlling relation 与 job control](./job-control.md)：controlling relation、foreground/background policy、terminal signal、cleanup与首版ash ABI。

## 邻接契约

- [Signal 当前契约](../signal/index.md)：继续唯一拥有pending occurrence与ordinary action selection；TTY只提交经重验request。
- [Task 当前契约](../task/index.md)：继续唯一拥有process-group membership、lifecycle、job control与user-entry truth。
