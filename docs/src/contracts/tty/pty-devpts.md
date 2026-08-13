# Unix98 PTY 与 devpts 当前契约

**Contract ID：** `PTY-PAIR-001` / `PTY-ADMISSION-001` / `PTY-ABI-001` / `DEVPTS-001`
**状态：** Active
**Owner：** `device::tty::pty` pair lifecycle；`fs::devpts` system instance、index、binding与metadata；TTY relation、VFS、opened-description、Signal与task job control保持各自owner
**参与领域：** PTY / devpts / devfs / VFS / task opened-description / TTY relation / Signal / job control / iomux / epoll
**覆盖范围：** single persistent devpts instance、Unix98 allocation/discovery、两条slave-open route、PTY data plane/readiness、opened-description lifecycle、safe index reuse、implicit controlling-terminal acquisition与master hangup
**不覆盖：** multiple/private devpts instance、mount-local `ptmx`、mount options、distribution-style `tty:0620`、legacy `termio`/break ioctl、generic VFS cached-positive freshness或完整multi-view concurrency
**实现位置：** `anemone-kernel/src/device/tty/pty/`、`anemone-kernel/src/fs/devpts/`、`anemone-kernel/src/fs/devfs/`、`anemone-kernel/src/fs/mod.rs`、`anemone-kernel/src/main.rs`
**依赖：** [TTY data plane](./data-plane.md)、[TTY relation 与 job control](./job-control.md)、[opened-description lifecycle](../task/opened-description-lifecycle.md)、[poll wait](../iomux/poll-wait.md)、[epoll](../epoll/protocol.md)、[mount admission](../vfs/mount-admission.md)
**当前来源：** [`PTY-DEVPTS-CUTOVER` transaction](../../devlog/transactions/2026-08-11-pty-devpts.md#2026-08-11---stage-4-checkpoint-2-public-activation与pty-devpts-cutover)；[PTY retirement与job-control ordering小迭代](../../devlog/changes/2026-08-12-pty-retirement-job-control-ordering.md)；[PTY logical cflag profile小迭代](../../devlog/changes/2026-08-13-pty-logical-cflag.md)
**最后核验：** 2026-08-13

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 |
| --- | --- | --- |
| pair identity、master liveness、slave lock、description participation、peer presence、bounded-effect admission、hangup与retirement | PTY pair | master/slave FileOps持不可克隆的operation-local permit；wake只要求predicate recheck |
| persistent system instance、capacity/index reservation、live `N -> pair` binding、initial metadata与logical retirement | devpts | allocation transaction持prepared pair与credential snapshot；mount只取得同一instance projection |
| inode/dentry、mount view、pathname traversal/DAC与cache freshness | VFS | devpts提供binding、inode projection与metadata；不读取VFS private cache |
| termios、winsize、discipline、input/output stream与data-plane readiness | shared `Terminal` | pair只组合attachment、peer predicate与progress route |
| published opened-description participation与final release | `task::files` contract | pair effect在creation-time single hook内静态组合 |
| controlling relation、foreground selector、signal occurrence与stop/continue | TTY relation / Signal / task job-control各自owner | pair retirement只提交stable-identity snapshot与窄cleanup request |

## PTY-PAIR-001 — Pair lifecycle与stream只有一组真相

每次成功打开global `/dev/ptmx`创建独立pair与一个shared `Terminal`。pair的immutable episode identity与phase唯一决定
master liveness、slave lock、slave-description participation、peer absence、hangup和retirement；numeric index、inode、
dentry、fd数或短命`Arc`都不能替代该truth。master write进入Terminal input/line-discipline，slave write和echo进入
Terminal output并由master读取；readiness由Terminal capacity/data和pair peer predicate即时组合，不缓存第三份
readable/writable/HUP/ERR状态。

master opened description final release是唯一retirement trigger。它先在pair owner内提交retirement并禁止新admission，
随后在guard外完成Terminal cleanup并撤销exact devpts binding与relation。pair phase与bounded guards-out effect admission
在同一个owner临界区仲裁：retirement先提交时不再产生permit；operation先提交时取得不可克隆permit，retirement必须在
relation撤销后等待该operation结束，才允许发布hangup `SIGHUP/SIGCONT`和最终wait notification。permit只证明一个有界
effect operation先于retirement提交，不携带liveness truth，也不得跨blocking wait。master live时last-slave-description
close只形成可reopen的peer absence；dup/fork alias不提前触发final release。notification只要求完整predicate recheck，
wait、signal、VFS cleanup与复杂drop均不得持pair operation guard。

**验证 / Enforcement：** owner-local pair/description KUnit；public `pty-test`的双向stream、partial progress、poll/select/
epoll、multiple open、dup/fork/final close、peer reopen、hangup与retire/reuse matrix；permit-first/retirement-first KUnit；
SMP8 master-close/background-read race；source/lock/linearization review。

## PTY-ADMISSION-001 — 两条open route共享lifecycle仲裁但保留各自前置条件

pathname `/dev/pts/N` open与master `TIOCGPTPEER`都必须先完成fd/opened-description等fallible prepare，再由同一pair owner
原子检查live、unlocked与未retired状态并提交slave participation；成功tail只包含不失败的pair/relation/fd publication。
failed open不得留下participant、relation或published fd。

pathname route先经过VFS traversal、current inode-mode DAC与current opener credential；`TIOCGPTPEER`以live master fd作为
capability，绕过pathname与ordinary DAC，但不绕过pair lock/liveness/retirement。两条route都只把operation-local
`O_NOCTTY`交给relation owner：eligible session leader在没有controlling terminal且endpoint未被其它session占用时隐式
取得slave，否则open仍可成功而不改变relation。`O_NOCTTY`不进入opened-description status truth。

**验证 / Enforcement：** public two-route lock/liveness/DAC/credential与implicit-acquisition正负matrix；failed-open cleanup、
open-vs-retire、fd reservation与relation commit source audit。

## PTY-ABI-001 — 首版Unix98 ABI与hangup结果必须真实可观察

首版公开global character node `/dev/ptmx`、canonical `/dev/pts/N`与additional mounted view、initial slave lock、
`TIOCGPTN`、`TIOCSPTLCK`、`TIOCGPTPEER`、`O_CLOEXEC`、`O_NONBLOCK`和operation-local `O_NOCTTY`。slave初始metadata固定为
allocator `fsuid:fsgid`、mode `0600`、character kind与`st_rdev=136:N`。现代termios/winsize、relation与job-control ioctl
由companion TTY contracts定义。PTY的Terminal-owned logical `c_cflag`初始为`B38400 | CS8 | CREAD`，master与slave
观察同一committed snapshot。`TCSETS*`保留asm-generic speed、`CSTOPB`、`PARODD`、`HUPCL`、`CLOCAL`、input-speed、
`CMSPAR`与`CRTSCTS`等logical compatibility bits，但每次清除`CSIZE | PARENB`并强制`CS8 | CREAD`；这些bit不驱动
physical line、pair lifecycle或data-plane decision。legacy `TCSETS*`无法携带arbitrary speed，因而output/input
`BOTHER`、`ADDRB`与其它首版mask之外的changed bit返回`EINVAL`并保持旧snapshot。

master final close flushes committed slave input；slave read在buffer清空后返回EOF，slave write和termios/winsize query或
mutation返回`EIO`，`TIOCSPGRP`保持现有`ENOTTY`边界，poll/select/epoll必须暴露terminal HUP/ERR outcome。首版不支持
legacy `TCGETA`/`TCSETA*`、`TCSBRK`/`TCSBRKP`，也不以silent success伪造这些命令。

**验证 / Enforcement：** RV64 public `pty-test` 14/14，包含独立cflag初值、master/slave共享、三种`TCSETS*`、
normalization、logical input speed/mark parity round-trip与`BOTHER`/unsupported rollback；focused glibc/musl
`hangup01`通过，`ioctl01` overall 7/9，
两个legacy `TCGETA` pointer-error子项非PASS；legacy probes与distribution metadata差异按register逐项归因，只有实际
PASS计入证据。

## DEVPTS-001 — 所有mount只投影同一个persistent system instance

`devpts`是generic `CAP_SYS_ADMIN` admission下的no-device filesystem。empty mount data可挂到任意existing directory；
non-empty data与unsupported options返回`EINVAL`。所有mount返回同一个persistent superblock/root、index/binding namespace
与inode projection；unmount一个或最后一个view都不驱动instance、pair或binding lifecycle。devfs静态发布global
`/dev/ptmx`与empty `/dev/pts`，长期userspace consumer在persistent `/dev` ready后显式mount canonical view。

capacity由kernel Kconfig拥有并限制current reserved/live episode；capacity exhaustion返回`ENOSPC`。index可以复用，但
binding、inode与open capability都绑定immutable episode identity；旧episode capability必须fail closed，不能仅凭numeric
`N`接入新pair。initial metadata在allocation时从credential snapshot一次形成；首版没有`pt_chown`、system `tty` group、
`0620`或mount-local metadata options。

VFS继续唯一拥有cached-positive revocation、late materialization、readdir cursor和forced multi-view concurrency。
PTY cutover只证明fresh/basic多view与stale-capability safety，不声称关闭该generic VFS缺口。

**验证 / Enforcement：** single-instance/mount lifetime/credential/index KUnit；RV64 public canonical+ordinary fresh view、
unmount/remount、metadata/DAC、capacity/churn与retire/reuse matrix；filesystem registration/publication/source audit。

## 当前接受边界

- RV64完成public runtime；LA64完成app、rootfs与kernel build，runtime Not Run。没有architecture-specific UAPI、user-copy或
  ioctl差异被发现，不能从RV64外推LA64 runtime。
- tmux create/attach/detach/exit因提供的RV64 rootfs没有target tmux executable而Not Run / Inconclusive；只有tmux terminfo
  与编辑器support文件不构成consumer。sshd按RFC为Not Run。
- distribution-style `tty:0620`与[legacy termio/break ioctl](../../register/current-limitations.md#ane-20260811-pty-legacy-termio-break)
  是当前accepted limitations；metadata profile详见
  [distribution tty limitation](../../register/current-limitations.md#ane-20260811-pty-distribution-tty-profile)。
- generic VFS cached-positive freshness、late materialization、readdir与完整concurrent multi-view linearizability保持
  [Open / Not Proven](../../register/open-issues.md#ane-20260809-vfs-dynamic-positive-dentry-revocation)。
