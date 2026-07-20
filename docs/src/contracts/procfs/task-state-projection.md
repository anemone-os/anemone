# Procfs TGID Task-state Projection 当前契约

**Contract ID：** `PROCFS-TASK-STATE`
**状态：** Active
**Owner：** procfs TGID state ABI projection
**参与领域：** procfs / task / scheduler
**覆盖范围：** 成功读取 `/proc/<tgid>/stat` state 字段与 `/proc/<tgid>/status` `State` 行时的 current truth source和编码
**不覆盖：** procfs binding lifetime、leader-missing error、ThreadGroup terminal publication、job-control / ptrace state、其它 stat / status 字段
**实现位置：** `anemone-kernel/src/fs/proc/tgid/{stat,status}.rs`
**依赖：** None
**Pending Successor：** [RFC-20260720-unix-jobctl R0](../../rfcs/unix-jobctl/index.md)；`UJ-CUTOVER` 前不生效
**最后核验：** 2026-07-20

## PROCFS-TASK-STATE-001 — 当前 TGID state 只投影 leader TaskStatus

**规则：** 当前成功的 TGID `stat` / `status` read从 live leader的 scheduler-owned `TaskStatus` 读取 state：`Runnable -> R / running`、interruptible `Waiting -> S / sleeping`、uninterruptible `Waiting -> D / disk sleep`、`Zombie -> Z / zombie`。procfs不保存第二份 state truth；当前也不从 ThreadGroup lifecycle、signal pending、wait status或尚不存在的 job-control phase合成其它 state字符。`status` 的 character与 name当前由两次 live read生成，本条不承诺它们来自同一个原子 snapshot。

**违反表现：** procfs缓存可变 task state、从 notification或pending signal推断状态，或把当前映射误写成已经支持 `T` job-control stop / ptrace stop或 coherent character/name snapshot。

**验证 / Enforcement：** `build_stat_line()`、`build_status_text()`、`proc_state()` 与 `proc_state_name()` 源码审计；procfs stat / status 用户态回归。

**最初来源：** 现有 procfs TGID stat / status 实现。

**当前来源：** live procfs task-state projection，2026-07-20 源码核验。
