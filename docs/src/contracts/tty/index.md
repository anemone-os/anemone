# TTY 当前契约

**Owner：** `device::tty` data plane、controlling relation与terminal-access protocol；参与的task/Signal/job-control owner保持独立
**覆盖范围：** serial port capability、Unix98 PTY/devpts、共享Terminal input/output、endpoint lifecycle、controlling relation、foreground/background policy、terminal-generated signal与首版TTY/PTY ABI
**不覆盖：** orphaned-process-group effect、非PTY relation-disassociation signal、`TOSTOP`、physical hardware hangup/runtime line configuration或procfs TTY字段
**最后核验：** 2026-08-11

本目录只登记已经cut over的共享规则。`TTY-DATA-CUTOVER`、`TTY-JOBCTL-CUTOVER`与`PTY-DEVPTS-CUTOVER`现均为Effective，
serial RX conditioning小迭代已在其上原子refine input condition与break signal；第一版TTY R1已经关闭，
但未列入本目录的corner仍以各contract接受边界和register为准。

## Contract Surfaces

- [Serial TTY data plane](./data-plane.md)：UART ordered condition handoff、共享 Terminal、input/output conditioning 与稳定 endpoint publication。
- [TTY controlling relation 与 job control](./job-control.md)：controlling relation、foreground/background policy、terminal signal、cleanup与首版ash ABI。
- [Unix98 PTY 与 devpts](./pty-devpts.md)：single persistent instance、pair/admission/lifecycle、public ABI、safe reuse与master hangup。

## 邻接契约

- [Signal 当前契约](../signal/index.md)：继续唯一拥有pending occurrence与ordinary action selection；TTY只提交经重验request。
- [Task 当前契约](../task/index.md)：继续唯一拥有process-group membership、lifecycle、job control与user-entry truth。
