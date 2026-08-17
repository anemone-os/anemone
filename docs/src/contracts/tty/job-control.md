# TTY Controlling Relation 与 Job Control 当前契约

**Contract ID：** `TTY-REL-001` / `TTY-JOBCTL-001` / `TTY-LIFE-001` / `TTY-ABI-001`
**状态：** Active
**Owner：** `device::tty` controlling-relation 与 terminal-access protocol；task topology、Signal 与 ThreadGroup job control 继续分别拥有 membership、occurrence/action 与 stop/continue/report truth
**参与领域：** serial TTY / PTY / VFS / task topology / process group / Signal / ThreadGroup job control / task lifecycle
**覆盖范围：** controlling-terminal relation、caller-relative `/dev/tty`、PTY slave implicit acquisition、foreground selector、terminal signal、ordinary background read、relation cleanup、PTY master-hangup effect与首版 BusyBox ash job-control ABI
**不覆盖：** orphaned-process-group errno/effect、`TOSTOP` write、其它 terminal-modifying `SIGTTOU` matrix、非PTY relation-disassociation signal、physical hardware hangup、runtime line reconfiguration或procfs TTY字段
**实现位置：** `anemone-kernel/src/device/tty/`、`anemone-kernel/src/task/{jobctl,sig}/`
**依赖：** [TTY data plane](./data-plane.md)、[process-group signaling](../task/process-group-signaling.md)、[Signal pending/action](../signal/pending-routing.md)、[Unix job control](../task/job-control.md)、[task lifecycle](../task/thread-group-lifecycle.md)、[user entry](../task/user-entry.md)
**当前来源：** [`TTY-JOBCTL-CUTOVER` transaction](../../devlog/transactions/2026-07-23-tty-subsystem.md#stage-4-user-evidence-tty-jobctl-cutover-and-closure---2026-07-24)；[Serial TTY RX conditioning 小迭代](../../devlog/changes/2026-08-03-tty-serial-rx-conditioning.md)；[TTY TAB3/XTABS output processing 小迭代](../../devlog/changes/2026-08-08-tty-tab3-output.md)；[`PTY-DEVPTS-CUTOVER`](./pty-devpts.md)；[PTY retirement与job-control ordering小迭代](../../devlog/changes/2026-08-12-pty-retirement-job-control-ordering.md)；[TTY IUTF8与明确compatibility set小迭代](../../devlog/changes/2026-08-13-tty-iutf8-compat.md)；[`TIOCSCTTY` context-sensitive argument小迭代](../../devlog/changes/2026-08-17-tty-tiocsctty-argument.md)；[TTY FIONREAD/TIOCINQ小迭代](../../devlog/changes/2026-08-17-tty-fionread.md)；[TTY TCFLSH队列清空小迭代](../../devlog/changes/2026-08-17-tty-tcflush.md)
**最后核验：** 2026-08-17

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

PTY runtime slave endpoint按stable identity exact enrollment/retirement进入同一registry。pathname open与`TIOCGPTPEER`
在未设置operation-local `O_NOCTTY`且caller满足session-leader/no-current-CTTY/read-access条件时请求implicit acquire；
未取得relation不使open失败。pair和devpts不保存relation truth。

## TTY-JOBCTL-001 — Terminal policy只产生经重验的guards-out effect

**规则：** TTY拥有foreground/background access policy与terminal-effect decision generation；task topology拥有
caller/group/session membership，Signal拥有occurrence/action，ThreadGroup job control拥有stop/continue、ordering、
user-entry gate与parent report。TTY不得直接设置stopped/continued状态、完成ordinary wait、修改report，或从
`jobs`/wait结果反推foreground selector。

每次read/ioctl使用同步current caller。TTY在relation owner内取得stable-identity decision snapshot，释放TTY/relation
guard后进入topology/Signal owner重验caller与target membership。`TIOCSPGRP`的non-orphan核心必须区分：foreground
允许；background且`SIGTTOU` blocked/ignored允许；background且actionable时先向caller process group生成`SIGTTOU`
并返回restart，不提交foreground mutation。mutation还必须返回relation owner重验generation后才commit。

`VINTR/VQUIT/VSUSP`只向live foreground process group生成`SIGINT/SIGQUIT/SIGTSTP`。`BRKINT`不受`ISIG`控制：
Terminal先按ordered stream位置清除已经condition的input与尚未提交port的pending output，再形成foreground
`SIGINT` effect；break之后仍在worker/raw handoff中的units继续处理。local flush不因relation缺失、target stale或
Signal publication失败而回滚，也不得回退到current task、最近reader或global PGID。changed winsize只生成一次
`SIGWINCH`。普通background read在每次可能消费input前及blocking wait后重验：actionable `SIGTTIN`向caller process
group生成signal并返回idempotent restart，本次不消费input；blocked/ignored或没有live foreground selector时返回
`EIO`。relation失效或target revalidation失败必须retry/fail-close，不能回退到current task、opener或global PGID。

`TCFLSH`是terminal-modifying operation：serial与PTY slave必须在解释selector之前执行同一non-orphan
foreground/background检查；actionable background caller先生成`SIGTTOU`并返回restart，本轮不清队列，blocked/ignored
与foreground caller继续。PTY master不是controlling-terminal view，不参与该检查。PTY slave的signal effect仍必须
服从pair effect permit与retirement排序。

Terminal、relation、port与topology guard外才允许Signal publication、Event wake、echo TX与复杂drop。effect request
不进入持久队列；`SIGCONT` wake只触发重新仲裁，不能携带restart permit或反向驱动relation/job-control truth。

PTY slave read、relation ioctl、master-input control signal与changed winsize signal还必须先在pair owner取得bounded
effect permit。permit与pair retirement共享仲裁点，但不保存relation、target或signal truth；它可以跨guards-out
relation/topology/Signal调用，只覆盖一个有界effect operation，并必须在任何blocking wait前释放。retirement先提交时
slave read按PTY ABI返回EOF，不能使用尚未撤销的relation发布`SIGTTIN`或返回background `EIO`；operation先提交时才允许
完成本轮job-control effect，master retirement必须等permit排空后再发布hangup `SIGHUP/SIGCONT`。serial TTY不参与该
pair lifecycle协议。

**违反表现：** session外group收到terminal signal、TTY保存或推进jobctl phase、background policy使用opener/global
PGID、持TTY guard进入Signal/topology、background read提前消费input，或signal result反向改写relation。

**验证 / Enforcement：** foreground/background `TIOCSPGRP`三分支、`VINTR/VQUIT/VSUSP`、QEMU serial break在
`ISIG=0`下的flush与foreground `SIGINT`、changed-only winsize、actionable/blocked/ignored background read、
detach-no-effect与BusyBox ash RV64 matrix；PTY permit-first/retirement-first KUnit与SMP8 master-close/background-read；
19项Unix job-control focused回归、`TCFLSH` invalid-selector-before-`SIGTTOU` user oracle；guard/identity/restart
capability source audit。

## TTY-LIFE-001 — Relation cleanup先撤销可发现性再执行外部效果

**规则：** session leader/controlling process exit与首版session-leader `TIOCNOTTY`终结relation。cleanup由relation
owner唯一、幂等提交：先让旧relation不能再被`/dev/tty`、foreground check或mutation取得，再释放guard并执行必要的
owner-local wake/drop。并发access只能观察合法旧前态或已撤销后态，不能观察无owner的half-detached relation。

首版cleanup只撤销relation与foreground selector，不依据旧foreground snapshot生成`SIGHUP`/`SIGCONT`。foreground
process group消失只失效selector，不拆除session-terminal relation；ordinary last close不拆relation。已发布serial
endpoint、devfs node与Terminal不因relation cleanup销毁。

ThreadGroup terminal lifecycle、first terminal code、job-control terminal precedence与newly orphaned stopped-group
transition仍由task/jobctl owner持有。hardware hangup/backend fatal不得用node消失、编号复用或Terminal销毁伪装。

PTY master final close是窄例外：pair先不可逆retire并禁止新bounded-effect permit，relation owner再用旧generation与
stable session-leader identity撤销discoverability。master release不持pair/relation guard等待此前已提交的permit全部
结束，然后才在guards-out按序向旧session leader thread group提交`SIGHUP`、`SIGCONT`。没有live relation时不生成替代
target，也不向整个旧foreground process group额外广播；该规则不改变session-leader exit与`TIOCNOTTY`的首版边界。

**违反表现：** detach后`/dev/tty`仍取得旧relation、两个owner重复cleanup effect、TTY覆盖first exit code、foreground
group消失误删endpoint，或last close销毁仍受session控制的Terminal。

**验证 / Enforcement：** detach后`/dev/tty`与old-effect失效、reacquire、session-leader exit reuse、foreground group
clear与endpoint persistence RV64 matrix；PTY effect-drain KUnit与SMP8 late-stop recovery oracle；eager/lazy cleanup、
generation与guards-out source/lifecycle audit。

## TTY-ABI-001 — 首版兼容包络必须真实可观察

**规则：** 首版同时交付稳定`/dev/ttyS0`、caller-relative`/dev/tty`、real Terminal boot fd 0/1/2、canonical与
noncanonical `VMIN=1,VTIME=0` input、blocking/nonblocking read、byte-stream write、poll/select、目标termios/control
chars/winsize/ioctl、显式`setsid + TIOCSCTTY` acquisition、`TIOCGPGRP/TIOCSPGRP/TIOCGSID`、foreground control
signals、serial `BRKINT` foreground `SIGINT`、changed winsize `SIGWINCH`、普通background read `SIGTTIN`以及
session-leader detach/exit cleanup。termios input envelope还真实round-trip并执行`IGNBRK`、`BRKINT`、`IGNPAR`、
`PARMRK`、`INPCK`、`ISTRIP`、`INLCR`、`IGNCR`、`ICRNL`与`IUTF8`；`IUTF8`按`TTY-INPUT-001`与
`TTY-OUTPUT-001`执行Linux N_TTY continuation-byte erase/column语义。termios output envelope真实round-trip
`TAB0`与`TAB3/XTABS`并执行TAB3 expansion。精确compatibility set `XCASE`、`FLUSHO`、`PENDIN`、`NLDLY`、
`CRDLY`、`TAB1/TAB2`、`BSDLY`、`VTDLY`、`FFDLY`、`OFILL`与`OFDEL`按tracked Linux 6.6.32稳定保存，
但不获得数据面行为；`IMAXBEL`、unknown bits及其它具有Linux-visible行为而未实现的flag继续原子`EINVAL`，
不得因实现成本降级为success-no-op。

`TIOCSCTTY`参数只在relation owner确认endpoint已由另一live session控制时取得特殊含义：此时`arg=1`
请求privileged steal；当前没有对应authority与旧session cleanup协议，必须记录notice并返回`EPERM`。endpoint
未绑定时任意参数都沿普通acquisition，精确relation幂等也先于file access与参数解释成功；caller已有另一
controlling TTY、非session leader、write-only first acquisition与其它live conflict继续按既有规则返回`EPERM`。
不得在relation inspection前把任意非零参数直接解释为steal。

`FIONREAD`与`TIOCINQ`是同一个asm-generic命令值，参数是Linux `int *`。serial与PTY slave在owner lock内取得
discipline committed-input snapshot，释放owner lock后执行copyout；snapshot超过`int`范围返回`EFBIG`，坏用户指针
返回`EFAULT`且不得消费或改变队列。其byte-count语义由`TTY-INPUT-001`定义，PTY master的output projection由
`TTY-OUTPUT-001`与`PTY-ABI-001`共同定义；该命令不改变`FIONBIO`、readiness、`VMIN/VTIME`或unknown ioctl边界。

`TCFLSH`接受asm-generic scalar selector `TCIFLUSH=0`、`TCOFLUSH=1`与`TCIOFLUSH=2`，其它值返回`EINVAL`。
serial与PTY slave分别清input、output或两者；PTY master按其read/write view反转input/output投影，both保持不变。
命令不改变termios、winsize、relation、logical output column或opened-description state；具体queue handoff由
`TTY-INPUT-001`、`TTY-OUTPUT-001`与`PTY-ABI-001`定义。

BusyBox ash必须取得真实controlling TTY；`jobs`、Ctrl-Z、`fg`、`bg`、foreground Ctrl-C、background read与shell
reclaim都必须经过本页的relation/Signal/job-control handoff。BusyBox vi依赖真实raw/canonical切换、readiness与byte
I/O完成启动、编辑、保存和退出。shell prompt、`job control turned off`、unconditional `TIOCSPGRP`、anonymous-console
特判或ioctl success stub都不满足该能力。GNU `less 668`必须能以真实`TCSETSW`进入可用全屏界面，并以`q`
退出；仅接受`XTABS`却继续输出literal tab不满足该能力。

包络外能力可以稳定拒绝或保留明确限制，但不得把已经交付的non-orphan `TIOCSPGRP`三分支、ordinary background
read或foreground signal重新归入延期范围，也不得成功后丢弃状态。

**违反表现：** ash降级运行、foreground job结束后shell不能reclaim、`TIOCSPGRP`无条件放行或错误拒绝
blocked/ignored路径、vi依赖fake ioctl、unsupported设置成功无效果，或background read绕过foreground policy。
GNU `less`因其`TAB3` candidate被拒绝，或`TAB3`被当作success-no-op，也属于违反。
`IUTF8`只被保存却不改变erase/column，或将compatibility set之外的行为flag静默接纳，同样属于违反。
未绑定endpoint上的`TIOCSCTTY(arg=1)`被当作steal拒绝，或unsupported steal替换、破坏既有relation，也属于违反。
`FIONREAD`消费input、包含canonical pending edit、把空EOF readiness伪装成非零byte或在owner lock内执行user copyout，
同样属于违反。`TCFLSH`接受非法selector、清错PTY方向、在actionable background检查前返回`EINVAL`或让flush前
worker-local input重新出现，也属于违反。

PTY refine同时交付`/dev/ptmx`与`/dev/pts/N`、`TIOCGPTN`/`TIOCSPTLCK`/`TIOCGPTPEER`、两条slave-open route的
implicit acquisition、PTY foreground/background policy和master-hangup relation effect。支持的termios/winsize surface是
`TCGETS`/`TCSETS`/`TCSETSW`/`TCSETSF`、`TIOCGWINSZ`/`TIOCSWINSZ`；legacy `TCGETA`/`TCSETA*`与
`TCSBRK`/`TCSBRKP`不在首版ABI，必须诚实拒绝而非success-stub。

**验证 / Enforcement：** RV64自动TTY matrix、BusyBox vi与ash host oracle、native Python 3.13 basic REPL与
PyREPL、用户人工ash checklist、GNU `less 668`进入可用全屏界面并以`q`退出的用户运行证据、404项KUnit、
IUTF8/TAB3 inline KUnit runtime与public PTY/TTY byte oracle、19项Unix job-control focused回归、ABI/source/bypass
audit与final review；serial与PTY定向matrix覆盖unbound `arg=1` acquisition、nonzero exact-relation idempotence、
live occupied `arg=1 -> EPERM`与旧relation保持，以及`TCFLSH`三种selector、非法参数、PTY双视图与
background `SIGTTOU`顺序。

## 跨领域handoff义务

| Protocol / obligation | 参与方 | Handoff / 线性化点 | 失败 / cleanup责任 |
| --- | --- | --- | --- |
| Relation lookup/mutation | TTY relation / topology | stable identity snapshot；topology验证后relation generation commit | stale/invalid target retry或fail-close；relation owner撤销 |
| Terminal signal | Terminal / relation / topology / Signal | Terminal形成request；guard外target revalidation后Signal occurrence commit | 无live target只诊断；不得fallback或持guard publication |
| Background read | FileOps / relation / Signal / job control | consume前decision；Signal default-stop；user-entry resume后restart | blocked/ignored/no-foreground为`EIO`；actionable路径不提前consume |
| Lifecycle cleanup | task lifecycle / PTY pair / relation | relation owner先撤销discoverability，再guards-out cleanup | cleanup幂等；只有PTY master hangup生成本契约定义的session-leader signals |

## 验证范围与当前接受边界

- 2026-08-17 `TCFLSH` refinement的agent-run RV64证据：app/kernel build通过；public PTY SMP8与serial wrapper
  分别通过784/784 KUnit、PTY `19/19`、TTY `59/59`，三种selector、非法参数、master/slave方向、flush后新输入与
  actionable background `SIGTTOU`顺序均有user oracle；BusyBox vi/ash与host byte oracle通过，两次均有序关机。
  focused LTP `ioctl02`在执行目标断言前被既有`/proc/meminfo`解析前置条件阻断；LA64 build/runtime与hardware为Not Run。
- 2026-08-17 `FIONREAD` / `TIOCINQ` refinement的agent-run RV证据：RV64 app/kernel build通过；public PTY
  SMP8与serial wrapper分别通过701/701 KUnit、PTY `18/18`、TTY `57/57`，新增queue query matrix、serial
  BusyBox vi/ash与host byte oracle通过，两次均有序关机。LA64 build/runtime、hardware与LTP为Not Run。
- 2026-08-17 `TIOCSCTTY` argument refinement的agent-run RV证据：RV64 app/kernel build通过；public PTY
  SMP8与serial SMP1分别通过692/692 KUnit、PTY `17/17`、TTY `56/56`，serial BusyBox vi/ash与host byte
  oracle通过，两次均有序关机。LA64 build/runtime、hardware、LTP与真实privileged steal为Not Run。
- 2026-08-13 IUTF8 refinement的agent-run RV64证据：两套wrapper均通过633/633 KUnit；PTY SMP8
  `16/16`，TTY `55/55`、host UTF-8/TAB3 byte oracle与BusyBox vi/ash均PASS并有序关机；双架构app与
  KUnit-enabled kernel build通过，独立review最终0 Apollyon / 0 Keter / 0 Euclid。LA64 runtime、LTP、tmux与
  实体UART/hardware为Not Run，build不得外推为runtime proof。
- 2026-07-24原cutover的agent-run RV64证据：404项KUnit、TTY `50/50`、QEMU serial break、BusyBox vi/ash、host byte oracle、
  native Python 3.13 `-c`/basic REPL/PyREPL、source/lock/bypass audit，独立review最终0 Apollyon / 0 Keter / 0 Euclid。
- 既有user-run RV64 job-control证据仍为同一contract的历史验证，本轮未重新运行：同一base/candidate、platform、
  BusyBox与kernel hash上的ash checklist完成Ctrl-C、
  `Ctrl-Z -> jobs -> fg -> Ctrl-Z -> bg -> jobs -> fg -> Ctrl-C`、background `cat`的`SIGTTIN` stop、foreground
  input与clean exit，launcher与wrapper均PASS。
- 2026-08-08维护者在决赛RV64 guest中确认GNU `less 668`进入可用全屏界面，并以`q`退出。
- 2026-07-24原cutover的build/runtime acceptance只覆盖RV64；当时LA64 compile/runtime、实体UART parity/framing
  injection、hardware与LTP为Not Run。focused pretest中的
  signal/wait profile为`attempted=0`，不是LTP通过证据。
- relation-disassociation `SIGHUP`/`SIGCONT`、newly orphaned stopped-group policy、orphaned-pgrp errno/effect、
  `TOSTOP`与其它terminal-modifying background access、physical hardware hangup/runtime line change和
  procfs TTY字段仍不在本契约。
