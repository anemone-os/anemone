# TTY Controlling Relation 与 Job Control 当前契约

**Contract ID：** `TTY-REL-001` / `TTY-JOBCTL-001` / `TTY-LIFE-001` / `TTY-ABI-001`
**状态：** Active
**Owner：** `device::tty` controlling-relation 与 terminal-access protocol；task topology、Signal 与 ThreadGroup job control 继续分别拥有 membership、occurrence/action 与 stop/continue/report truth
**参与领域：** TTY / VFS / task topology / process group / Signal / ThreadGroup job control / task lifecycle
**覆盖范围：** controlling-terminal relation、caller-relative `/dev/tty`、foreground selector、terminal-generated signal、ordinary background read、relation cleanup 与首版 BusyBox ash job-control ABI
**不覆盖：** PTY/devpts/ptmx、orphaned-process-group errno/effect、`TOSTOP` write、其它 terminal-modifying `SIGTTOU` matrix、relation-disassociation `SIGHUP`/`SIGCONT`、hardware hangup、runtime line reconfiguration或procfs TTY字段
**实现位置：** `anemone-kernel/src/device/tty/`、`anemone-kernel/src/task/{jobctl,sig}/`
**依赖：** [TTY data plane](./data-plane.md)、[process-group signaling](../task/process-group-signaling.md)、[Signal pending/action](../signal/pending-routing.md)、[Unix job control](../task/job-control.md)、[task lifecycle](../task/thread-group-lifecycle.md)、[user entry](../task/user-entry.md)
**当前来源：** [`TTY-JOBCTL-CUTOVER` transaction](../../devlog/transactions/2026-07-23-tty-subsystem.md#stage-4-user-evidence-tty-jobctl-cutover-and-closure---2026-07-24)
**最后核验：** 2026-07-24

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| session-terminal binding、foreground selector与relation generation | TTY relation registry | Session/terminal lookup持同一relation handle；effect路径持immutable stable-identity snapshot | `/dev/tty`解析、foreground判断与mutation revalidation |
| Session/ProcessGroup membership与stable identity | task topology | TTY只持窄caller/group capability或snapshot | caller/candidate/target membership重验 |
| signal occurrence、mask/disposition与ordinary action selection | Signal | TTY只提交经重验的kernel-origin request | `SIGINT/SIGQUIT/SIGTSTP/SIGTTIN/SIGTTOU/SIGWINCH` |
| stop/continue phase、ordering、user exposure与parent report | ThreadGroup job control | TTY不保存或推进该状态 | default-stop、`SIGCONT`、wait与user-entry gate |
| committed termios、winsize、input与readiness | shared `Terminal` | relation只选择target，不缓存data-plane truth | terminal I/O与effect decision |
| session-leader detach/exit relation revocation | TTY relation owner | task lifecycle只触发窄cleanup capability | 先撤销可发现性，再完成guards-out cleanup |

Event/wake、signal request、relation snapshot、diagnostic counter与test marker都不是持久行为真相；每个参与方只在自己的
owner边界提交local state，随后由consumer重验durable predicate或stable identity。

## TTY-REL-001 — Controlling-terminal relation是单一双向binding truth

**规则：** 一个relation唯一持有terminal identity、stable session identity、foreground process-group identity与
用于stale detection的generation。Session/SID lookup和terminal lookup可以取得指向同一relation的handle，但不得
各缓存一份可变binding或foreground PGID。每个session至多一个controlling terminal，每个terminal至多一个
controlling session。

task topology继续唯一拥有Session/ProcessGroup membership。TTY通过窄capability验证caller、session leader和候选
foreground group；SID/PGID只用于ABI lookup，跨lifecycle保存的relation或target必须绑定stable identity并在owner
边界重验，numeric ID reuse不得复活旧控制权。

`/dev/tty`按同步caller的stable session identity解析live relation，成功后返回普通shared Terminal file。后续I/O
不得用opener、最近reader或global PGID猜测terminal。foreground mutation必须先由topology验证candidate，再回到
relation owner重验relation generation与caller authority后提交。

**违反表现：** Session与Terminal各保存一份foreground PGID、ID reuse取得旧relation、`TIOCSPGRP`只检查正整数、
non-controlling caller取得任意Terminal，或opener identity驱动后续access policy。

**验证 / Enforcement：** acquire/query/idempotence、wrong-session、candidate errno、`/dev/tty` caller-relative open、
detach/reacquire与exit/reuse RV64 matrix；stable identity/generation source audit；relation owner KUnit与assertion。

## TTY-JOBCTL-001 — Terminal policy只产生经重验的guards-out effect

**规则：** TTY拥有foreground/background access policy与terminal-effect decision generation；task topology拥有
caller/group/session membership，Signal拥有occurrence/action，ThreadGroup job control拥有stop/continue、ordering、
user-entry gate与parent report。TTY不得直接设置stopped/continued状态、完成ordinary wait、修改report，或从
`jobs`/wait结果反推foreground selector。

每次read/ioctl使用同步current caller。TTY在relation owner内取得stable-identity decision snapshot，释放TTY/relation
guard后进入topology/Signal owner重验caller与target membership。`TIOCSPGRP`的non-orphan核心必须区分：foreground
允许；background且`SIGTTOU` blocked/ignored允许；background且actionable时先向caller process group生成`SIGTTOU`
并返回restart，不提交foreground mutation。mutation还必须返回relation owner重验generation后才commit。

`VINTR/VQUIT/VSUSP`只向live foreground process group生成`SIGINT/SIGQUIT/SIGTSTP`；changed winsize只生成一次
`SIGWINCH`。普通background read在每次可能消费input前及blocking wait后重验：actionable `SIGTTIN`向caller process
group生成signal并返回idempotent restart，本次不消费input；blocked/ignored或没有live foreground selector时返回
`EIO`。relation失效或target revalidation失败必须retry/fail-close，不能回退到current task、opener或global PGID。

Terminal、relation、port与topology guard外才允许Signal publication、Event wake、echo TX与复杂drop。effect request
不进入持久队列；`SIGCONT` wake只触发重新仲裁，不能携带restart permit或反向驱动relation/job-control truth。

**违反表现：** session外group收到terminal signal、TTY保存或推进jobctl phase、background policy使用opener/global
PGID、持TTY guard进入Signal/topology、background read提前消费input，或signal result反向改写relation。

**验证 / Enforcement：** foreground/background `TIOCSPGRP`三分支、`VINTR/VQUIT/VSUSP`、changed-only winsize、
actionable/blocked/ignored background read、detach-no-effect与BusyBox ash RV64 matrix；19项Unix job-control focused
回归；guard/identity/restart capability source audit。

## TTY-LIFE-001 — Relation cleanup先撤销可发现性再执行外部效果

**规则：** session leader/controlling process exit与首版session-leader `TIOCNOTTY`终结relation。cleanup由relation
owner唯一、幂等提交：先让旧relation不能再被`/dev/tty`、foreground check或mutation取得，再释放guard并执行必要的
owner-local wake/drop。并发access只能观察合法旧前态或已撤销后态，不能观察无owner的half-detached relation。

首版cleanup只撤销relation与foreground selector，不依据旧foreground snapshot生成`SIGHUP`/`SIGCONT`。foreground
process group消失只失效selector，不拆除session-terminal relation；ordinary last close不拆relation。已发布serial
endpoint、devfs node与Terminal不因relation cleanup销毁。

ThreadGroup terminal lifecycle、first terminal code、job-control terminal precedence与newly orphaned stopped-group
transition仍由task/jobctl owner持有。hardware hangup/backend fatal不得用node消失、编号复用或Terminal销毁伪装。

**违反表现：** detach后`/dev/tty`仍取得旧relation、两个owner重复cleanup effect、TTY覆盖first exit code、foreground
group消失误删endpoint，或last close销毁仍受session控制的Terminal。

**验证 / Enforcement：** detach后`/dev/tty`与old-effect失效、reacquire、session-leader exit reuse、foreground group
clear与endpoint persistence RV64 matrix；eager/lazy cleanup、generation与guards-out source/lifecycle audit。

## TTY-ABI-001 — 首版兼容包络必须真实可观察

**规则：** 首版同时交付稳定`/dev/ttyS0`、caller-relative`/dev/tty`、real Terminal boot fd 0/1/2、canonical与
noncanonical `VMIN=1,VTIME=0` input、blocking/nonblocking read、byte-stream write、poll/select、目标termios/control
chars/winsize/ioctl、显式`setsid + TIOCSCTTY(arg=0)`、`TIOCGPGRP/TIOCSPGRP/TIOCGSID`、foreground control
signals、changed winsize `SIGWINCH`、普通background read `SIGTTIN`以及session-leader detach/exit cleanup。

BusyBox ash必须取得真实controlling TTY；`jobs`、Ctrl-Z、`fg`、`bg`、foreground Ctrl-C、background read与shell
reclaim都必须经过本页的relation/Signal/job-control handoff。BusyBox vi依赖真实raw/canonical切换、readiness与byte
I/O完成启动、编辑、保存和退出。shell prompt、`job control turned off`、unconditional `TIOCSPGRP`、anonymous-console
特判或ioctl success stub都不满足该能力。

包络外能力可以稳定拒绝或保留明确限制，但不得把已经交付的non-orphan `TIOCSPGRP`三分支、ordinary background
read或foreground signal重新归入延期范围，也不得成功后丢弃状态。

**违反表现：** ash降级运行、foreground job结束后shell不能reclaim、`TIOCSPGRP`无条件放行或错误拒绝
blocked/ignored路径、vi依赖fake ioctl、unsupported设置成功无效果，或background read绕过foreground policy。

**验证 / Enforcement：** RV64自动TTY matrix `45/45`、BusyBox vi与ash host oracle、用户人工ash checklist、239项
KUnit、19项Unix job-control focused回归、ABI/source/bypass audit与final review。

## 跨领域handoff义务

| Protocol / obligation | 参与方 | Handoff / 线性化点 | 失败 / cleanup责任 |
| --- | --- | --- | --- |
| Relation lookup/mutation | TTY relation / topology | stable identity snapshot；topology验证后relation generation commit | stale/invalid target retry或fail-close；relation owner撤销 |
| Terminal signal | Terminal / relation / topology / Signal | Terminal形成request；guard外target revalidation后Signal occurrence commit | 无live target只诊断；不得fallback或持guard publication |
| Background read | FileOps / relation / Signal / job control | consume前decision；Signal default-stop；user-entry resume后restart | blocked/ignored/no-foreground为`EIO`；actionable路径不提前consume |
| Lifecycle cleanup | task lifecycle / relation | relation owner先撤销discoverability，再guards-out cleanup | cleanup幂等；不生成首版范围外disassociation signals |

## 验证范围与当前接受边界

- agent-run RV64证据：239项KUnit、TTY `45/45`、BusyBox vi/ash与host byte oracle、19项focused
  `jobctl-test`、source/lock/bypass audit，最终0 Apollyon / 0 Keter / 0 Euclid。
- user-run RV64证据：同一base/candidate、platform、BusyBox与kernel hash上的ash checklist完成Ctrl-C、
  `Ctrl-Z -> jobs -> fg -> Ctrl-Z -> bg -> jobs -> fg -> Ctrl-C`、background `cat`的`SIGTTIN` stop、foreground
  input与clean exit，launcher与wrapper均PASS。
- build/runtime acceptance只覆盖RV64。LA64 compile/runtime、hardware与LTP为Not Run；focused pretest中的
  signal/wait profile为`attempted=0`，不是LTP通过证据。
- relation-disassociation `SIGHUP`/`SIGCONT`、newly orphaned stopped-group policy、orphaned-pgrp errno/effect、
  `TOSTOP`与其它terminal-modifying background access、PTY/devpts/ptmx、hardware hangup/runtime line change和
  procfs TTY字段仍不在本契约。
