# RFC-20260810-pty-devpts

**状态：** Accepted
**修订：** R1
**负责人：** doruche
**最后更新：** 2026-08-11
**领域：** TTY / PTY / devpts / VFS / task opened-description lifecycle / job control
**影响契约：** Accepted Target；见 [Contract Impact](#contract-impact)，尚未 cut over
**执行记录：** [PTY / devpts transaction](../../devlog/transactions/2026-08-11-pty-devpts.md)

本 RFC 是 PTY / devpts R1 Accepted Target 的 public canonical source。它不覆盖 current contract，也不授权实现或
cutover。R1 acceptance轮只接受target、owner、ABI、resource guarantee、contract delta与acceptance boundary；后续
[实施路线](./implementation.md)只组织Stage 1--4的依赖与停止点。Stage 1的两个execution checkpoint已在各自授权下
关闭，未产生contract cutover；Stage 2--4仍未获execution authorization。

## 摘要

本 RFC 在已经关闭的 Serial TTY R1 之上，新增一个独立的 Unix98 PTY / devpts target。首版以一个
system-wide devpts instance 为边界：`/dev/ptmx` 分配独立 PTY pair，`/dev/pts/N` 暴露动态 slave semantic TTY
endpoint；slave 复用现有 `Terminal` data plane、controlling relation 与 job-control protocol，master 只作为 pair
另一侧的数据与 lifecycle endpoint，不成为 controlling terminal，也不伪装 physical serial port。

核心能力由仓库内普通PTY Rust test app验证kernel ABI、lifecycle、readiness、relation、hangup与identity safety。
该app使用`anemone-rs`，形状与`socket-test`相同，不调用或保证任何libc PTY wrapper，也不以userspace wrapper、
libc版本或调用链定义Anemone target。tmux create/attach/detach/exit 是必须尝试但不阻塞 core closure 的建议性集成验证；
sshd 只作为条件性诊断 consumer，不进入验收标准，因为其 network、transport、authentication 与
crypto prerequisite 不能归因于 PTY。Linux 内部`tty_driver`层次、锁和引用模型不构成兼容目标；
用户可见 ABI、唯一状态 owner、无丢失唤醒、
opened-description final release、identity safety 与 cleanup 必须形成可证明的 target。

## 背景与当前基线

[Serial TTY 当前契约](../../contracts/tty/index.md)已经交付 shared `Terminal`、input/output conditioning、termios、
winsize、readiness、controlling relation 与 foreground job control，但明确不覆盖 PTY/devpts/ptmx、hangup 和
relation-disassociation signal。PTY 因而是 follow-up target，不重新打开 Serial TTY R1，也不把 serial endpoint
的 publish-until-reboot lifecycle 强套到 runtime PTY endpoint。

当前相邻边界还包括：

- [`OPENED-DESC-001/002/003`](../../contracts/task/opened-description-lifecycle.md)以 published slot refcount 作为
  final release 唯一真相，当前只允许 creation-time 固定的单 `final_release` hook；flock handoff 在该 hook 前执行。
- [VFS dynamic positive dentry revocation](../../register/open-issues.md#ane-20260809-vfs-dynamic-positive-dentry-revocation)
  仍是 Open / Deferred，由 VFS core 独立拥有。该通用缺口不形成 PTY 的额外 implementation/cutover Stage；devpts
  只保持 backend mapping、pair identity 与 VFS projection 的 owner 边界，不得为 stale positive dentry 或迟到
  materialization 建立第二套 VFS freshness truth，也不得把当前 source-level 缺口写成已被 PTY
  证明关闭。
- devfs 已有 append-only hierarchy，可发布静态 `/dev/ptmx` 和 `/dev/pts` mountpoint，但不拥有动态 PTY namespace、
  provider teardown 或 pair lifecycle；这些边界见
  [devfs hierarchy 小迭代](../../devlog/changes/2026-08-08-devfs-hierarchical-publication.md)。
- 当前 `openat` 把 `O_NOCTTY` 作为可观测效果为空的兼容 flag。本 R1 target 要求 pathname 与 `TIOCGPTPEER`
  两条 slave-open route 都按下文的 Linux-compatible implicit acquisition 语义消费该 operation-local flag；在 cutover
  前，current contract 与 live behavior 仍保持 no-op baseline。
- [`ANE-20260604-IOCTL-LTP-STAGE1-GAPS`](../../register/current-limitations.md#ane-20260604-ioctl-ltp-stage1-gaps)
  仍将 `ioctl01` 的 PTY/devpts/ptmx 依赖列为未闭合子域。

进入 public RFC 前的定位过程见[已归档 positioning](./backgrounds/positionings.md)。该背景页不再定义 target 或
review 状态。

## 目标

- 建立动态 semantic TTY endpoint 与 PTY pair 的唯一 identity、admission、peer presence、hangup 和 retirement
  lifecycle。
- 提供单实例 Unix98 用户 ABI：静态 `/dev/ptmx`、system devpts view、动态 `/dev/pts/N`、初始 slave lock、
  `TIOCGPTN`、`TIOCSPTLCK` 与 `TIOCGPTPEER`。
- 让 master write 进入现有 `Terminal` input/line-discipline pipeline，让 slave write 与 echo 经现有 output
  processing 供 master read；不建立第二个 `Terminal` 或伪造 physical `TtyPort`。
- 让 PTY slave 以稳定 terminal identity 参与现有 controlling relation、foreground/background access、terminal
  signal 与 caller-relative `/dev/tty` protocol。
- 让 pathname slave open 与 `TIOCGPTPEER` 共享同一个 pair-owned lock/liveness/retirement 仲裁与 enrollment；前者
  先经过 pathname search 与 VFS inode-mode DAC，后者以 live master fd 作为 capability、绕过 pathname traversal 与
  ordinary pathname DAC。两条路线最终产生语义等价的 slave opened description，但不伪造相同的前置权限条件。
- 让 dup/fork/close 与 fd-table teardown 服从 opened-description final release；短命 `Arc`、裸 fd 数量、inode/dentry
  引用或 `File` storage lifetime 均不得替代它。
- 建立可验证的 peer absence、buffered data、EOF/errno、partial progress、blocking/nonblocking 和
  poll/select/epoll hangup behavior。
- 以仓库内普通PTY Rust test app与选定LTP cases证明core capability；tmux按建议性集成规则
  必须尝试和归因，sshd 不进入 core acceptance，任何 workload 都不能用 allocation smoke test 冒充语义证明。

## 非目标

- legacy BSD PTY name/device。
- multiple devpts instances、`newinstance`、mount namespace isolation、devpts `uid=`/`gid=`/`mode=` mount options，
  以及 distribution-style fixed `tty` group profile。
- Linux 内部 `tty_driver`、flip buffer、ldisc worker、锁序、引用模型或 allocator 层次的同形复制。
- packet mode、remote mode、额外 line disciplines、virtual console 或 PTY/physical UART hangup 的统一抽象。
- 未被首版 workload 证明必要的 `TIOCPKT`、`FIONREAD/TIOCINQ`、`TIOCOUTQ`、`TIOCGPTLCK` 等扩展 ioctl。
- 完整 Linux termios/ioctl errno corner、完整 `VMIN/VTIME` 组合和与目标 workload 无关的历史兼容面。
- 在本 RFC 中顺带改变 existing serial endpoint 的 implicit controlling-terminal acquisition；本 target 只闭合 PTY
  slave 的两条 open route，generic serial TTY expansion 需要独立边界。
- ssh transport、authentication、crypto、network stack 或 tmux 自身功能；它们只作为 PTY capability consumer。
- 除本修订接受的 master-hangup effect 外，顺带关闭 session-leader exit、`TIOCNOTTY`、orphaned process group、
  `TOSTOP` 或其它既有 job-control residual。
- 对 retired cached dentry 的立即物理消失或 generic pathname lookup-result freshness/linearizability
  保证；旧 episode 的 inode/open capability 必须 fail closed，无旧 cached projection 的当前 backend lookup
  可以按 live binding 解析新 pair，physical reclaim 可以更晚。

## Owner 与协议边界

本文冻结领域 owner，不冻结 Rust type、trait、模块布局、buffer placement 或 lock representation。

| 状态 / 能力 | 唯一 owner | 其它参与方只持有什么 |
| --- | --- | --- |
| committed termios、winsize、line discipline、slave input/output stream 与 data-plane readiness | `Terminal` | serial/PTY attachment 与 opened file 持窄 capability |
| immutable pair/terminal identity、master liveness、slave lock、slave opened-description participation、peer absence、hangup 与 retirement | PTY pair lifecycle owner | master/slave FileOps 持 operation-local capability；wake 只要求 predicate recheck |
| PTY index、live `N -> pair` backend binding、initial metadata policy/source 与 logical binding retirement | devpts backend | allocation transaction 持 prepared pair capability 与 operation-local allocator credential snapshot；runtime pair 只提交 retire request |
| resident inode metadata/index、dentry materialization/cache 与 mounted pathname visibility | VFS superblock/inode/dentry owner | devpts 提供 backend mapping 与 metadata input，不读取 VFS private cache/lock |
| controlling session binding、foreground selector 与 relation generation | existing TTY relation owner | PTY slave 以 stable terminal identity 参与；pair 只提交 retire request |
| opened description 的 `Unpublished -> Live -> Retired` 与 final release | `task::files` | pair 接收窄 enrollment/final-release effect，不读取 fd-table private state |
| Session/ProcessGroup membership、Signal occurrence/action、ThreadGroup stop/continue/report | existing task、Signal 与 job-control owner | TTY/PTY 只提交经重验的 target/effect request |

`semantic TTY endpoint` 是 stable terminal identity、ordinary slave-side TTY capability 与 relation participation 的
组合边界，不是 `Terminal`、PTY pair 和 relation 之外的新 mutable owner。PTY master 不加入 controlling relation；
devpts 不拥有 termios、byte stream、pair liveness 或 job-control truth；VFS 不拥有 pair lifecycle 或 backend binding。

## Lifecycle 与跨 owner handoff

### Allocation episode

system devpts instance 编排一次 `/dev/ptmx` open 的 allocation transaction：从 allocator task 取得一次 operation-local
`fsuid`/`fsgid` snapshot，prepare index/binding、pair、master opened description 与初始 locked slave metadata；slave
initial metadata 固定为 allocator `fsuid:fsgid`、mode `0600`、character kind 与 `st_rdev=136:N`，不应用 allocator
umask。在成功返回前形成完整、自洽、可发现的 episode。任一 fallible step 失败都必须回滚未发布 capability，不得留下
live pair、可打开 slave、relation、participant 或 waiter。

pair、master 与 pathname publish 后，runtime lifecycle authority 转交给 pair owner。devpts 继续拥有 index/binding；
VFS 继续拥有 inode/dentry/pathname projection。allocation transaction 不因编排这些 prepare/commit 而取得三者的
runtime state truth。

### Slave admission

`TIOCSPTLCK` 只修改 pair-owned slave admission state。pathname open 在进入 pair owner 前由 VFS 使用 current opener
credential 执行 pathname search 与 inode-mode DAC；`TIOCGPTPEER` 则必须验证 live master capability 与 ioctl flags，
不重新执行 pathname traversal 或 ordinary pathname DAC。两条路线随后进入同一 pair-owned live/lock/retirement 仲裁
与 enrollment。slave open 与 master retirement 并发时，一次 open 要么在 retirement 前完整 enroll 一个 opened
description，要么失败；不得发布 half-enrolled description。

implicit acquisition 属于 successful slave-open episode 的 relation-owner effect，而不是 pair admission condition。
全部 fallible route/admission/opened-description prepare 必须先完成；随后 pair enrollment、relation owner 的 conditional
commit 与 fd publication 进入不再失败的 success tail。pair/VFS 只传递 operation-local flag、access 与 caller
capability，不取得 relation truth；任何 failed open 都不得留下 participant 或 relation。

同一 slave 可以被 pathname 多次 open，形成多个 opened descriptions；dup/fork aliases 只增加同一 description 的
published slot participation。master live 时，最后一个 slave description final release 只形成 peer absence；只要
slave 仍 unlocked，后续 open 可以重新 enrollment，不因一次 peer absence 永久退休 pair。

### Master final close 与 cleanup

master opened description final release 是 pair owner 唯一、不可逆的 retirement trigger。pair owner 先原子禁止后续
slave admission并发布 retired/hangup predicate，再释放 owner guard，通过窄、幂等 handoff 请求 devpts retire logical
binding、TTY relation owner revoke relation并提交下文接受的 signal/job-control effect，以及各 wait owner recheck。

这些 effect 不组成持共同锁的全局原子 transaction。每个 owner-local cleanup 都必须单调、可重复请求并且不能恢复
live/discoverable state；pathname、relation、wait participation、inode/dentry 和 pair storage 的物理回收可以更晚。

### Readiness 与 peer state

PTY operation 的最终 predicate 由 `Terminal` 的 data availability/capacity 与 pair owner 的 peer
presence/hangup/retirement 无缓存组合。FileOps 可以组合 owner snapshot/capability，但不得保存第三份
readable/writable/HUP truth。notification 只触发完整 predicate recheck；register、recheck、cancel 和 final snapshot
必须服从 current iomux/epoll contract。

具体 proof obligations 见[目标与不变量](./invariants.md)。

## ABI 与可见语义

### Unix98 allocation 与 discovery

首版 surface 包括：

- character device `/dev/ptmx` 与 mounted system directory `/dev/pts`；
- 每次成功打开 `/dev/ptmx` 创建独立 pair 和动态 `/dev/pts/N` slave node；
- slave 初始 locked，unlock 前 pathname open fail closed；
- `TIOCGPTN`、`TIOCSPTLCK`、`TIOCGPTPEER`；
- `O_CLOEXEC` 与 `O_NONBLOCK` 的 generic fd/opened-description semantics，以及下文定义的 operation-local
  `O_NOCTTY` effect；
- 多个独立 slave opens、dup/fork aliases、final close 与 peer reopen；
- initial `stat` metadata 固定为 allocator `fsuid:fsgid`、mode `0600`、character kind 与 `st_rdev=136:N`；pathname
  open 执行 ordinary VFS inode-mode DAC，且多次独立 open 均使用各自 current opener credential；
- `TIOCGPTPEER` 以 live master fd 为 capability，不依赖或重新执行 slave pathname traversal/DAC；它仍与 pathname
  route 共享 pair-owned lock/liveness/retirement 仲裁并产生相同 slave opened-description semantics。

首版不在 kernel 中建立独立 grant mutation 或历史 `pt_chown` helper。slave metadata 在
`/dev/ptmx` allocation 时已经按上述 profile 完成，解锁的唯一 kernel ABI 是修改 pair-owned lock state 的
`TIOCSPTLCK`。PTY test app直接核验valid/invalid master、`TIOCGPTN`、`TIOCSPTLCK`、`TIOCGPTPEER`、
pathname discovery 与对应 errno；用户态 PTY helper 的版本、内部调用链和可见返回值不是本 R1 target 或
acceptance claim。

### Index 与 resource guarantee

首版的 PTY capacity 必须由 kernel Kconfig 的重要配置项拥有；具体 allocator、index container、incarnation 表示与
默认数值留给实现。capacity 约束当前 reserved/live allocation episode，而不是 boot 以来的累计分配次数。失败的
allocation 与 master retirement 都必须释放对应 reservation；在 current capacity 以内反复 allocate-close 不得因历史
churn 永久耗尽编号空间。

index 可以复用，但新 pair 必须具有与旧 episode 不同的 immutable identity 或 incarnation。devpts live binding、
slave inode/open capability 与 pair admission 必须绑定该 episode identity；旧 inode、handle 或迟到 operation 不能
仅凭 numeric `N` 接入复用后的新 pair。generic VFS pathname freshness、cached-positive revocation 与迟到
materialization 继续由 VFS owner 和对应 register issue 负责，不形成本 RFC 的额外 implementation/cutover Stage；devpts 不为
规避该通用缺口建立 owner-local dentry generation、读取 VFS private cache 或退成 monotonic boot-lifetime allocator。
capacity/quota 无可用 slot 返回 `ENOSPC`，ordinary backing allocation failure返回 `ENOMEM`。

### Terminal data plane 与 job control

- master write 经 `Terminal` input conditioning、canonical/raw discipline、echo 与 control-character processing 后供
  slave read；slave write 与 echo 经 output processing 后供 master read。
- master/slave 观察同一 termios/winsize truth，并支持首版目标中的 `TCGETS`、`TCSETS`、`TCSETSW`、`TCSETSF`、
  `TIOCGWINSZ`、`TIOCSWINSZ` 与 `SIGWINCH`。
- slave 复用现有 `TIOCSCTTY`、`TIOCNOTTY`、`TIOCGSID`、`TIOCGPGRP`、`TIOCSPGRP`、caller-relative `/dev/tty`、
  foreground/background access 和 terminal-generated signal。
- master 不成为 controlling terminal；explicit `TIOCSCTTY` 继续使用现有 errno 与 relation protocol。

pathname 与 `TIOCGPTPEER` 成功打开 slave 时都采用 Linux 6.6.32 的 implicit controlling-terminal acquisition
语义。未设置 `O_NOCTTY`、opened description 具有 read access、current caller 是 session leader、caller session
尚无 controlling terminal 且 slave endpoint 尚未绑定其它 session 时，existing TTY relation owner 必须原子建立
session-terminal binding，并把 caller current process group 设为 foreground。任一条件不成立时不改变 relation，
但不能仅因未取得 controlling terminal 让 slave open 失败。`O_NOCTTY` 只抑制本次 open 的 implicit effect，不成为
opened-description status truth，也不改变后续显式 `TIOCSCTTY`。

该 observable rule 由 fixed
[`xref:linux-6.6.32:drivers/tty/pty.c#ptm_open_peer`](https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git/tree/drivers/tty/pty.c?id=91de249b6804473d49984030836381c3b9b3cfb0#n602)、
[`xref:linux-6.6.32:drivers/tty/tty_io.c#tty_open`](https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git/tree/drivers/tty/tty_io.c?id=91de249b6804473d49984030836381c3b9b3cfb0#n2111)与
[`xref:linux-6.6.32:drivers/tty/tty_jobctrl.c#tty_open_proc_set_tty`](https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git/tree/drivers/tty/tty_jobctrl.c?id=91de249b6804473d49984030836381c3b9b3cfb0#n132)
证明 Linux source fact；它在本文中的 target status 来自本 RFC review decision，而不是由外部实现自动取得权威。

### Close、peer absence 与 hangup

首版必须保证：关闭非最后一个 dup/fork alias 不触发 peer transition；master live 时 last-slave-close 只形成可重新
open 的 peer absence；master final close 不可逆地 retire pair、禁止新 admission、撤销 relevant relation并唤醒全部
blocked reader/writer/poller。任何路径都不得永久阻塞、伪造 success、丢失 HUP/ERR/EOF terminal outcome 或让 stale
identity 接入新 pair。

首版已支持的 read/write/ioctl/poll/select/epoll surface 默认采用 Linux 6.6.32 的用户可观察语义；
buffered-data precedence、EOF/`EIO`、partial progress、post-hangup query ioctl 与 readiness bit 的逐格 source/test
matrix 是 implementation proof artifact，不是新的 target decision。任何有意偏离都必须在实现或 cutover 前回到
RFC review；非目标 ioctl/corner 继续诚实 fail closed，不因 Linux 内部实现存在而自动扩张首版 surface。

master final close 对 controlling relation 的 accepted effect 是：pair retirement 先禁止新 admission；relation owner
以旧 relation generation 与 stable session-leader identity 形成 snapshot，先撤销 `/dev/tty` 与 foreground policy 对
旧 relation 的可发现性，再在 owner guard 外依次向旧 controlling session leader thread group 提交 `SIGHUP`、
`SIGCONT`。没有 live relation 时不生成替代 target；master-close 不向整个旧 foreground process group额外广播
`SIGHUP`，也不启用 session-leader exit、`TIOCNOTTY`、orphaned-pgrp 或 `TOSTOP` residual。Signal 与 ThreadGroup
job-control owner继续分别决定 occurrence 与 continue phase，pair/relation 不直接推进这些状态。

## Contract Impact

下表是 R1 Accepted Target 的 delta，不是 effective contract。只有完成对应 cutover 后才更新 current contract。

| Contract ID | 变化 | 当前规则 | R1 target 摘要 | Cutover |
| --- | --- | --- | --- | --- |
| `PTY-PAIR-001` / `PTY-ADMISSION-001` / `PTY-ABI-001` | Introduce | None | pair lifecycle、single lifecycle-admission owner with route-scoped preconditions、Unix98 ABI 与 Linux-compatible hangup surface | future atomic `PTY-DEVPTS-CUTOVER` |
| `DEVPTS-001` | Introduce | None | system instance、safe-reuse index/binding、Kconfig capacity、allocator `fsuid:fsgid`/`0600`/`136:N` initial metadata profile、route-scoped permission admission 与 logical retirement | future atomic `PTY-DEVPTS-CUTOVER` |
| `TTY-TERM-001` / `TTY-INPUT-001` / `TTY-OUTPUT-001` | Refine | serial-only Terminal/port attachment | shared Terminal semantics extend to PTY slave/master attachment without moving truth ownership | future atomic `PTY-DEVPTS-CUTOVER` |
| `TTY-REL-001` / `TTY-LIFE-001` / `TTY-JOBCTL-001` / `TTY-ABI-001` | Refine | PTY/hangup excluded | runtime slave endpoint participation、Linux-compatible implicit acquisition / `O_NOCTTY`、master-hangup relation revoke与session-leader `SIGHUP`/`SIGCONT`、accepted PTY ABI profile | future atomic `PTY-DEVPTS-CUTOVER` |

### Dependencies

- [`OPENED-DESC-001/002`](../../contracts/task/opened-description-lifecycle.md)：published slots 与 dup/fork sharing
  继续定义 final release truth。
- [`OPENED-DESC-RETIRE-001`](../../contracts/task/opened-description-lifecycle.md#opened-desc-retire-001--terminal-retirement-固定进入窄-vfs-flock-handoff)：
  flock retirement 继续先于 creation-time final-release effects。
- [`OPENED-DESC-003`](../../contracts/task/opened-description-lifecycle.md#opened-desc-003--当前-final-release-callback-是创建时固定的单-hook)：
  PTY 必须在当前 creation-time 单 static hook 边界内 owner-locally composition pair participation 与既有 fanotify
  close effect；R1 不引入 final-release plan、多个 observer 或新的 task::files shared contract。
- [`TTY-PORT-001` / `TTY-ENDPOINT-001`](../../contracts/tty/data-plane.md)：physical serial owner 与 stable serial
  publication 不因 PTY 改变。
- [IOMUX poll-wait](../../contracts/iomux/poll-wait.md)与[Epoll](../../contracts/epoll/protocol.md)：readiness
  subscription、recheck 与 wake-hint protocol。
- [`DEVICE-NUMBER-001`](../../contracts/device/device-number.md)与
  [`VFS-FILE-KIND-001`](../../contracts/vfs/file-kind.md)：typed device number 与 resident inode kind projection。
- [VFS dynamic positive dentry revocation](../../register/open-issues.md#ane-20260809-vfs-dynamic-positive-dentry-revocation)：
  generic pathname freshness 缺口继续由 VFS core/register 拥有；PTY 不增加 owner-local workaround 或独立 acceptance criterion，
  也不把 PTY closure 外推为完整 namespace linearizability 证明。

## Implementation Boundary

本节只定义未来实现授权必须保护的语义边界，不构成本轮实现授权。

- **允许改变：** TTY owner 内为 PTY attachment/runtime endpoint 所需的 internal capability；new PTY pair owner；
  new devpts backend；PTY UAPI codec；VFS/devfs 的窄 handoff；current `OPENED-DESC-003` 内的 owner-local static
  final-release composition；targeted tests 和同 owner behavior-preserving splits。
- **必须保持：** physical UART/console owner、existing serial ABI、`Terminal` semantic truth、relation/task/Signal/
  job-control owner、opened-description published-ref truth与single-static-hook contract、VFS inode/dentry/cache owner、
  current contract 在 cutover 前的 effective status，以及本 RFC 的 non-goals/acceptance。
- **禁止形状：** second liveness/readiness/refcount truth、pair 读取 fd-table/VFS private state、devpts 拥有 pair data
  plane、dynamic lifecycle observer registry、PTY-local dentry freshness workaround、wake count 驱动行为、
  与 accepted initial metadata/profile 冲突的伪造 grant transition、success-no-op ABI 或 workload/test special case。
- **停止条件：** target/non-goals、owner/handoff/failure/cleanup、public ABI、Contract Impact、acceptance 或 validation
  claim 需要改变；pair/fanotify effect 无法在 current
  `OPENED-DESC-003` 内自然组合；master-hangup 要顺带解决非目标 job-control residual；mandatory PTY test app只能
  通过削弱语义或伪造 environment 才能运行。

## Acceptance 与 Validation

R1 acceptance 代表 target、owner、ABI、contract delta、proof boundary 与 closure claim 已经确定，不代表实现完成。
未来 cutover 至少需要以下 claim-scoped evidence：

- **Source/proof：** owner/bypass audit；allocator credential snapshot、initial metadata publication、route-scoped slave
  admission、allocation rollback、open-vs-retire、implicit-acquire successful-open tail、final-release composition、
  relation teardown、predicate/recheck、retire-then-reuse、old-episode capability fail-close、stale identity 与
  lock/guards-out proof；该 source/proof 不声称关闭 generic VFS namespace linearizability 缺口。
- **Linux reference：** 使用 tracked
  [`xref:linux-6.6.32:drivers/tty/pty.c#ptmx_open`](https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git/tree/drivers/tty/pty.c?id=91de249b6804473d49984030836381c3b9b3cfb0#n790)、
  `xref:linux-6.6.32:fs/devpts/inode.c#devpts_pty_new`、
  `xref:linux-6.6.32:drivers/tty/pty.c#ptm_open_peer`、
  `xref:linux-6.6.32:drivers/tty/tty_io.c#tty_open`、
  `xref:linux-6.6.32:drivers/tty/tty_jobctrl.c#tty_open_proc_set_tty`、
  `xref:linux-6.6.32:drivers/tty/tty_io.c#do_tty_hangup`、
  `xref:linux-6.6.32:drivers/tty/tty_jobctrl.c#tty_signal_session_leader` 建立 observable ABI matrix；上游
  implementation 不替代 Anemone target。源码不能唯一决定、且确实影响R1 target的observable corner只在实际需要时
  做定向reference comparison并记录环境；此类comparison只是执行证据，不形成独立harness或gate。
- **Owner-local / test app：** allocator credential snapshot、exact initial `stat` metadata、pathname DAC、master-capability
  `TIOCGPTPEER`、lock、bidirectional I/O、termios/winsize、blocking/nonblocking、poll/select/epoll、multiple open、
  dup/fork/final close、peer reopen、两条 slave-open route 的 shared lifecycle/route-specific admission matrix、
  implicit acquire / `O_NOCTTY` / read-vs-write-only / eligible-vs-ineligible caller matrix、failed-open-no-relation、
  open-vs-retire、retire-reuse 后旧 inode/capability fail-closed 且不能接入新 pair、current binding 解析新 pair、capacity
  内 repeated churn、hangup 与 job control。generic cached-positive/late-materialization forced interleaving 继续按
  VFS register 记录为 Not Proven，不升级为 PTY acceptance requirement。
- **PTY test app：** 仓库内普通`no_std` Rust app使用`anemone-rs`启动和syscall/ioctl wrapper，形状与
  `socket-test`相同；完整覆盖上述owner-local / test app matrix，并能区分allocation-only和完整语义。只证明
  allocation或只验证happy path不满足该claim。该app是长期guest-local test app，不链接libc，也不是独立probe或
  kernel semantic authority；`anemone-rs`只按真实测试需要提供窄ABI常量和wrapper。
- **Compatibility / LTP：** `pty01`、`ioctl01`，以及由 accepted Linux-compatible surface选取的 `ptem01`、
  `hangup01`；额外 line-discipline/virtual-console cases 不自动成为 acceptance requirement。PTY target 内失败必须修复或触发 target
  renegotiation；非 PTY prerequisite 必须独立归因，不能改写core test app结论。LTP 的 userspace 链接/运行
  环境只是 case prerequisite，不形成 libc wrapper compatibility claim。
- **Advisory tmux：** future cutover 必须尝试 selected tmux create/attach/detach/exit，但结果本身不阻塞 core
  closure。QEMU serial presentation 或其它非 PTY prerequisite 使其不适用时记录 Not Run / Inconclusive；若失败暴露
  本 R1 target 内且能自然闭合的 PTY 缺口，则在实现边界内修复；若需要 tmux feature、terminal presentation 或其它
  non-goal，则保留归因证据并接受不做。target 内但无法自然闭合的缺口仍必须回到 target renegotiation，不能借
  “advisory”隐藏。
- **Diagnostic sshd：** sshd interactive session 不是 acceptance criterion，也不要求为 core closure 运行。若条件具备而
  运行，只能作为诊断证据，必须把 network、transport、authentication、crypto 与 PTY failure 分开归因；不得为通过
  sshd 扩张本 RFC。
- **Architecture：** RV64 与 LA64 build/runtime evidence 分开记录，单架构结果不得外推。core cutover 要求两种架构
  build，并在至少一种架构完成mandatory PTY test app runtime；只有实际运行的架构可以取得
  runtime-proven claim，另一架构明确记录 Not Run。若实现出现 architecture-specific UAPI、user-copy 或
  ioctl 路径差异，对应架构 runtime evidence 升为 mandatory。

本 RFC 不冻结 build/QEMU command、artifact hash 或 stage 顺序，也不因文档接受运行 kernel、QEMU、LTP 或 product
workload。

## 风险与反馈

- 多Stage实施依赖、全局实现输入与停止边界见[实施路线](./implementation.md)；路线存在不表示Stage已获execution
  authorization，也不规定测试与production实现的编写先后。

- current VFS dynamic positive-dentry revocation gap 可能让 retired pathname 继续 `stat` 到 inert inode，或让迟到
  materialization/cached positive 暂时遮蔽复用编号的新 binding。该通用 pathname availability/freshness 缺口继续由
  VFS register 拥有，不阻塞 PTY implementation/cutover；PTY 必须让旧 inode/open capability按其捕获的旧 pair
  identity fail closed，绝不能仅凭 numeric `N` 接入新 pair。
- final release 同时服务 flock、fanotify 与 PTY participation。本 RFC 不规定 PTY effect 与 `FAN_CLOSE_*` 之间没有用户
  可见要求的先后；若 owner-local composition 会丢失任一 effect、需要 runtime registry、改变 published-ref truth 或
  扩大 current `OPENED-DESC-003`，必须停止并重新 review owner/contract，而不是增加 ad-hoc hook。
- master hangup 同时影响 pair、devpts、relation、Signal/job-control 与 waiters。cleanup 不是 global lock transaction；
 任何需要跨 owner rollback 或恢复 live state 的路线都违反 target shape。
- implementation proof matrix 可能暴露现有 Terminal buffering/readiness 只适合 physical port 的假设。保持 owner
 的路线修正可以自然闭合；若必须移动 stream/readiness truth，则属于 target/owner renegotiation。
- future VFS freshness/revocation protocol cutover 后，devpts 可以按通用 owner handoff 做窄适配；不得因此移动 pair、
  binding 或 inode identity truth，也不得在此前建立 devpts-local workaround。PTY closure 只证明自己的 reuse、
  retirement 与 stale-capability safety，不外推为 generic pathname linearizability 证明。
- fixed `fsuid:fsgid`/`0600` profile 不承诺 distribution-style `tty:0620`。若 product workload
  证明需要 system `tty` group、其它 mode 或 devpts mount configuration，必须回到 RFC review 改变 target/profile 与
  configuration source，不能在实现中硬编码私人 rootfs 的 numeric GID。
- tmux 的 terminal presentation prerequisite 可能使建议性验证 Not Run / Inconclusive；sshd 还可能先暴露 network、
 transport、authentication 或 crypto 缺口。两者都必须区分 PTY failure 与 environment prerequisite，不能为通过
 workload 扩张本 RFC。

## 文档与证据

- [目标与不变量](./invariants.md)
- [实施路线](./implementation.md)
- [背景材料](./backgrounds/index.md)
- [已归档的 pre-RFC positioning](./backgrounds/positionings.md)
- 外部源码：`xref:linux-6.6.32:drivers/tty/pty.c#ptmx_open`、
  `xref:linux-6.6.32:fs/devpts/inode.c#devpts_pty_new`、
  `xref:linux-6.6.32:drivers/tty/pty.c#ptm_open_peer`、
  `xref:linux-6.6.32:drivers/tty/tty_io.c#tty_open`、
  `xref:linux-6.6.32:drivers/tty/tty_jobctrl.c#tty_open_proc_set_tty`、
  `xref:linux-6.6.32:drivers/tty/tty_io.c#do_tty_hangup`、
  `xref:linux-6.6.32:drivers/tty/tty_jobctrl.c#tty_signal_session_leader`

当前没有tracking page；[transaction](../../devlog/transactions/2026-08-11-pty-devpts.md)只记录已执行checkpoint、
review、validation与handoff。R1 review的已接受结论已经折回canonical target/contract/acceptance；
`implementation.md`只组织Stage依赖与执行停止点，不保留第二份decision状态表。

## 修订记录

| 修订 | 日期 | 语义变化 | Review / Evidence |
| --- | --- | --- | --- |
| R0 | 2026-08-10 | 接受单实例 Unix98 PTY/devpts target、Linux-default ABI、safe-reuse resource guarantee、master-hangup effect 与 claim-scoped acceptance；tmux 为建议性必试，sshd 非 acceptance criterion。 | maintainer review；docs-only，runtime Not Run |
| R1 | 2026-08-11 | 保留 reusable index 与跨 episode identity safety；generic dentry freshness/revocation 继续由 VFS register 独立拥有，不形成 PTY 的额外 implementation/cutover Stage，且禁止 PTY-local workaround 或 monotonic fallback。mandatory core validation收敛为仓库内普通Rust + `anemone-rs` PTY test app，不选择、依赖或验证libc PTY wrapper profile。 | maintainer review；docs-only，runtime Not Run |

## 当前状态

- Accepted Target / R1。
- Implementation route：Draft；Stage解析状态由[实施路线](./implementation.md)统一拥有。
- Implementation authorization：Stage 1已消费并关闭；Stage 2--4为None。
- Contract cutover：None。
- Stage 1：Closed。最终candidate的RV64 build、622项KUnit、TTY 50/50、auto/vi/ash与正常关机已通过；详见
  transaction。
- LA64 build/runtime、PTY test app、LTP、sshd、tmux：Not Run。
