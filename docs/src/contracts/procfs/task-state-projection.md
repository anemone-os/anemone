# Procfs TGID Task-state Projection 当前契约

**Contract ID：** `PROCFS-TASK-STATE`
**状态：** Active
**Owner：** procfs TGID state ABI projection
**参与领域：** procfs / task / scheduler / job control
**覆盖范围：** 成功读取 `/proc/<tgid>/stat` state 字段与 `/proc/<tgid>/status` `State` 行时的 current truth source和编码
**不覆盖：** procfs binding lifetime、leader-missing error、ThreadGroup terminal publication、ptrace state、其它stat / status字段
**实现位置：** `anemone-kernel/src/fs/proc/tgid/{stat,status}.rs`
**依赖：** `TASK-LIFE-001`、`JOBCTL-STATE-001`
**Pending Successor：** None
**最后核验：** 2026-07-21

## PROCFS-TASK-STATE-001 — TGID state投影read-local derived snapshot

**规则：** 每次成功的TGID `stat` / `status` read都从live leader和ThreadGroup owner取得一个read-local只读derived enum。observable leader `Zombie`优先投影`Z / zombie`；否则只有committed `Stopped`投影`T / stopped`，`Stopping`与`Running`继续使用leader scheduler-owned `TaskStatus`映射：`Runnable -> R / running`、interruptible `Waiting -> S / sleeping`、uninterruptible `Waiting -> D / disk sleep`。`/status`的character与name pair由该次read的同一个enum序列化，`/stat`独立使用相同映射；不承诺跨文件或跨read原子snapshot。procfs不保存第二份state truth，也不从Signal pending、wait report、notification或scheduler状态反向驱动jobctl。

**违反表现：** procfs缓存可变task state、从notification或pending signal推断状态、把`Stopping`显示为`T`、让job-control投影覆盖observable Zombie，或单次`/status` read的character/name来自不同enum。

**验证 / Enforcement：** `build_stat_line()`、`build_status_text()`、`proc_state()`与job-control diagnostic snapshot源码审计；focused `/proc/<tgid>/{stat,status}` stopped/continued pair回归。Zombie precedence在binding成功且能采样到Zombie时适用；本规则不改变binding失败边界。

**最初来源：** 现有 procfs TGID stat / status 实现。

**当前来源：** [RFC-20260720-unix-jobctl R1](../../rfcs/unix-jobctl/index.md)；[Stage 5 cutover事务](../../devlog/transactions/2026-07-20-unix-jobctl.md#stage-5-uj-cutover与事务收口---2026-07-21)。
