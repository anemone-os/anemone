# Unix Job Control 当前契约

**Contract ID：** `JOBCTL-STATE` / `JOBCTL-STOP` / `JOBCTL-SIGNAL` / `JOBCTL-CONT` / `JOBCTL-LIFE` / `JOBCTL-REPORT`
**状态：** Active
**Owner：** user `ThreadGroup` job-control protocol
**参与领域：** Signal / task topology / ThreadGroup lifecycle / user entry / child wait / procfs
**覆盖范围：** cooperative stop / continue phase、user exposure closure、control-signal ordering、lifecycle cleanup、parent report与观察面
**不覆盖：** controlling TTY、foreground/background process group、orphaned-process-group policy、ptrace stop、严格SIGCHLD transition order、完整credential identity
**实现位置：** `anemone-kernel/src/task/jobctl/`、`anemone-kernel/src/task/sig/{generation,delivery,pending}.rs`、`anemone-kernel/src/task/api/{wait,exit}/`、`anemone-kernel/src/fs/proc/tgid/`
**依赖：** `SIGNAL-PENDING-001/002`、`SIGNAL-ACTION-001/002`、`SIGNAL-TEMP-MASK-001..003`、`PGRP-SIGNAL-001/002`、`TASK-LIFE-001..003`、`CHILD-WAIT-001..005`、`USER-ENTRY-001/002`、`PROCFS-TASK-STATE-001`
**Pending Successor：** None
**最后核验：** 2026-07-21

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| phase、first stop reason、continue epoch | child `ThreadGroup` job-control state | Signal只持窄epoch candidate / control request | stop、stale candidate invalidation与resume |
| live member exposure | 同一个child `ThreadGroup` membership transaction | task只提交当前member entry/exit transition | 证明Stopped user-execution barrier |
| stopped / continued report | child `ThreadGroup` | parent waiter只持scan candidate | wait4 / waitid peek或consume |
| `child_status_changed` / `jobctl_unblocked` notification | 对应predicate的publisher | listener只持重扫能力 | guards-out wake，不携带durable truth |
| procfs state snapshot | child owner与live leader派生 | procfs只读snapshot | ABI projection，不反向驱动jobctl |

`ThreadGroupLifeCycle::{Exiting,Exited}`仍由`TASK-LIFE-*`唯一拥有。job-control phase只在
`Alive`时有行为意义；Event、Signal pending、SIGCHLD、procfs、scheduler state、task-local flag
和诊断counter都不得成为第二份job-control truth。

## JOBCTL-STATE-001 — ThreadGroup持有唯一job-control truth

**规则：** 每个user `ThreadGroup`唯一拥有`Running / Stopping(reason) / Stopped(reason)` phase、first stop reason、checked monotonic continue epoch、live member exposure与单slot child report。phase与这些owner-local字段在同一个ThreadGroup transaction中推进；重复stop request不替换first reason，continue epoch只作为stale candidate equality token，不表示report identity或生命周期。

**违反表现：** Signal、scheduler、procfs、Event或task-local状态并行推进phase；derived count反向授权行为；epoch wrap/alias让旧candidate重新取得stop authority；terminal lifecycle后jobctl状态覆盖first terminal code或授权user entry。

**验证 / Enforcement：** `task/jobctl/group.rs`的owner-lock transaction、常开shape assertion、phase/progress诊断与lifecycle source audit；Stage 4全部focused / KUnit floor。

**最初来源：** [RFC-20260720-unix-jobctl R1](../../rfcs/unix-jobctl/index.md)。

**当前来源：** [Unix job control事务Stage 5 cutover](../../devlog/transactions/2026-07-20-unix-jobctl.md#stage-5-uj-cutover与事务收口---2026-07-21)。

## JOBCTL-STOP-001 — Stopped由user exposure closure证明

**规则：** user-to-kernel entry先清除当前member exposure；最终user-entry gate只有在lifecycle仍为`Alive`且phase为`Running`时才登记exposure并允许architecture transition。fresh、clone、exec与ordinary return共享同一个逻辑gate。`Stopping`仅在所有live member都unexposed时提交`Stopped`和Stopped report；已经位于syscall、exception或ordinary wait的member可以继续原内核工作，但再次执行用户指令前必须经过gate。

**违反表现：** parent观察到Stopped后仍有member不经过gate执行用户指令；ordinary wait被取消、转移或当作participant ack；Event或force wake伪造stop completion；fresh/clone/exec绕过arbitration。

**验证 / Enforcement：** RV64 / LA64 ordinary、raw、fresh、clone与exec entry source closure；multi-member runnable + ordinary pipe wait、source完成后Stopped gate与exec new-image runtime；owner-local exposure assertions。

**最初来源：** [RFC-20260720-unix-jobctl R1](../../rfcs/unix-jobctl/index.md)。

**当前来源：** [Unix job control事务Stage 5 cutover](../../devlog/transactions/2026-07-20-unix-jobctl.md#stage-5-uj-cutover与事务收口---2026-07-21)。

## JOBCTL-SIGNAL-001 — Control-signal generation与jobctl提交同序

**规则：** concrete nonzero occurrence通过sender-specific target / permission validation后，Signal generation与ThreadGroup jobctl owner形成窄control transaction。stop-class generation清理全部ordinary private/shared pending `SIGCONT`；`SIGCONT` generation推进continue epoch、清理全部ordinary stop-class pending并无条件执行一次group resume side effect。global init执行opposite cleanup但不取得`SIGSTOP`或conditional DefaultStop authority。普通user ThreadGroup的`SIGSTOP`在generation transaction直接请求stop且不进入pending；`SIGTSTP / SIGTTIN / SIGTTOU`只有在Signal最终选择live `DefaultStop`后，才以captured epoch请求stop，epoch mismatch只取消jobctl effect。signal-0、permission failure、target失效或terminal lifecycle不产生cleanup、epoch或stop/resume side effect。

pre-existing reserved occurrence已经退出ordinary queue competition，control cleanup不撤销它；reservation不冻结live disposition，也不提交handler/default action。`SIGCONT` resume只在generation线性化一次，reserved delivery、fetch和action consume不得重放。R1保持reserved target相对later pending signal的既有优先级。

**违反表现：** `SIGSTOP`发布ordinary pending或force-complete active wait；ignored/masked conditional stop提前提交；stale DefaultStop candidate在`SIGCONT`后重新stop；reserved occurrence被cleanup删除；delivery重放resume；global init进入Stopping/Stopped；process-group selector变成jobctl owner。

**验证 / Enforcement：** `task/sig/{generation,pending,delivery}.rs`与kill/tkill/tgkill/rt_sigqueueinfo producer source audit；task/group-directed四stop matrix、opposite cleanup、global-init SIGSTOP、conditional disposition/flags、temporary-mask与stale-epoch KUnit/runtime。

**最初来源：** [RFC-20260720-unix-jobctl R1](../../rfcs/unix-jobctl/index.md)。

**当前来源：** [Unix job control事务Stage 5 cutover](../../devlog/transactions/2026-07-20-unix-jobctl.md#stage-5-uj-cutover与事务收口---2026-07-21)。

## JOBCTL-CONT-001 — Continued只来自committed Stopped

**规则：** `Stopping x SIGCONT`取消当前episode、恢复`Running`并唤醒jobctl parker，但不生成Stopped、Continued或对应SIGCHLD；只有`Stopped x SIGCONT`转换才发布一个coalesced Continued report。重复`SIGCONT`在已经Running时不产生新Continued report，ordinary occurrence仍按Signal disposition/mask规则独立处理。

**违反表现：** incomplete stop对parent可见；重复continue产生新report；resume side effect唤醒或完成原ordinary wait；Continued report驱动phase而不是由owner transaction产生。

**验证 / Enforcement：** owner-local`Stopping x SIGCONT`无report KUnit；focused committed stop、首次Continued consume、第二次SIGCONT无report且SIGCHLD count不变的runtime oracle。

**最初来源：** [RFC-20260720-unix-jobctl R1](../../rfcs/unix-jobctl/index.md)。

**当前来源：** [Unix job control事务Stage 5 cutover](../../devlog/transactions/2026-07-20-unix-jobctl.md#stage-5-uj-cutover与事务收口---2026-07-21)。

## JOBCTL-LIFE-001 — Membership与terminal不遗留exposure或parker

**规则：** join在membership发布前建立unexposed entry；任何已exposed member都由user trap-entry protocol在detach前清除exposure并完成可能的stop closure，detach本身只接受并断言unexposed membership；exec/dethread victim不能把exposure、park responsibility或reserved action转移给survivor。terminal owner在发布`Exiting(first_code)`的同一ThreadGroup transaction中清全组exposure、取消jobctl report、使phase不再具备行为意义并安排guards-out parker release；jobctl cleanup不覆盖、推迟或重新解释`TASK-LIFE-*` terminal truth。

**违反表现：** departed member永久阻塞Stopping；exec新image绕过gate；terminal group残留waitable jobctl report或parked member；jobctl phase覆盖first terminal code；cleanup在持有owner guard时执行wake/user copy/complex drop。

**验证 / Enforcement：** topology construct/join/detach/dethread与`task/api/exit` source audit；terminal exposure KUnit、multithread exec/dethread、multi-member SIGKILL、frame failure与normal teardown runtime。

**最初来源：** [RFC-20260720-unix-jobctl R1](../../rfcs/unix-jobctl/index.md)。

**当前来源：** [Unix job control事务Stage 5 cutover](../../devlog/transactions/2026-07-20-unix-jobctl.md#stage-5-uj-cutover与事务收口---2026-07-21)。

## JOBCTL-REPORT-001 — Report是child-attached coalesced truth

**规则：** child `ThreadGroup`拥有至多一个`Stopped / Continued` report slot。Stopped report的reason从同一个owner snapshot中的committed phase取得，不复制第二份reason。terminal result优先于jobctl report；wait scan与peek/consume claim都在current parent relation和child owner transaction下重验selector与selected state。`WNOWAIT`只peek，普通wait exact-once consume；predicate Event与可选SIGCHLD在guards-out后发布，只要求parent重扫。`SA_NOCLDSTOP`或ignored SIGCHLD只抑制notification，不删除report。child携带未消费report被reparent时保留该slot；new-parent relation发布后必须唤醒new parent的`child_status_changed` predicate，不能重放历史SIGCHLD，old-parent与其它stale candidate必须在relation重验时失败。waitid stopped/continued与对应SIGCHLD的`si_uid`固定为`0`。

procfs每次成功read都从live leader与同一ThreadGroup owner派生一个read-local enum：observable Zombie优先为`Z`；否则只有committed Stopped投影`T`，`Stopping / Running`使用底层task映射。`/status`的character与name pair由该次read的同一个enum序列化，`/stat`独立使用相同映射；不承诺跨文件或跨read原子snapshot。

**违反表现：** Event/SIGCHLD/procfs成为report truth；多个waiter消费同一report；reparent后stale candidate仍claim；suppressed SIGCHLD删除report；procfs显示Stopping为T或覆盖Zombie；任意member UID/credential cache填充`si_uid`。

**验证 / Enforcement：** `task/jobctl/report.rs`、`task/api/wait`、parent relation、SIGCHLD与procfs source audit；WNOWAIT双peek+consume、`si_uid=0`、SIGCHLD suppression、waitid07/08、waitpid08/13与procfs pair runtime。

**最初来源：** [RFC-20260720-unix-jobctl R1](../../rfcs/unix-jobctl/index.md)。

**当前来源：** [Unix job control事务Stage 5 cutover](../../devlog/transactions/2026-07-20-unix-jobctl.md#stage-5-uj-cutover与事务收口---2026-07-21)。

## 跨领域局部义务

| Parent Contract / Obligation | 参与方 | 必须完成的动作 | Handoff / 线性化点 | 失败 / Cleanup责任 |
| --- | --- | --- | --- | --- |
| `JOBCTL-SIGNAL-001` / producer | Signal producer | 先完成exact target/permission validation，再进入control generation | ThreadGroup generation owner transaction | target失效或terminal时不产生control side effect |
| `JOBCTL-SIGNAL-001` / delivery | Signal delivery | conditional DefaultStop只携带captured epoch；reserved target保留Signal cleanup责任 | live action selection -> epoch-validated stop request | stale只丢jobctl effect；Signal仍收口occurrence/mask |
| `JOBCTL-REPORT-001` / wait claim | child/parent wait | child提交durable report，parent只持重扫/claim candidate | parent relation -> child owner claim transaction | stale relation/selector/slot时重扫，不沿用旧status |
| `JOBCTL-REPORT-001` / reparent | topology reparent | 保留child report，发布new-parent relation后唤醒new parent predicate | reparent relation publication -> guards-out `child_status_changed` | 不重放历史SIGCHLD；old-parent candidate必须relation重验失败 |
| `JOBCTL-LIFE-001` / terminal | lifecycle/jobctl | terminal publication前清jobctl behavior truth，guards-out release parker | `Alive -> Exiting(first_code)` owner transaction | terminal owner保留first code；jobctl不得制造新terminal outcome |

## 当前接受边界

- controlling TTY、foreground/background process group、terminal-generated job-control signal、orphaned-process-group policy与ptrace stop由后续RFC负责。
- guards-out job-control SIGCHLD不承诺跨相邻Stopped/Continued/terminal transition的严格publication顺序；durable report、wait claim和procfs不受该notification窗口影响，见[`ANE-20260721-JOBCTL-SIGCHLD-PUBLICATION-ORDER`](../../register/current-limitations.md#ane-20260721-jobctl-sigchld-publication-order)。
- stopped/continued `si_uid = 0`在credential owner提供跨leader exit与report生命周期稳定的child identity snapshot前保持有效限制；不得为此在ThreadGroup缓存credential truth。
- numeric TID/PGID reuse与final gate到hardware return的窄窗口保持现有identity/architecture边界；第一版不引入稳定identity table或scheduler stop state。
