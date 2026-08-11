# 2026-08-11 - PTY / devpts

**Status:** Completed / R4 / Stage 4 Closed
**Owners:** doruche, Codex
**Canonical Target:** [RFC-20260810-pty-devpts R4](../../rfcs/pty-devpts/index.md)
**Implementation Route:** [Stage 1--4](../../rfcs/pty-devpts/implementation.md)
**Contract Delta:** `PTY-DEVPTS-CUTOVER` Effective；`PTY-PAIR-001` / `PTY-ADMISSION-001` / `PTY-ABI-001` /
`DEVPTS-001` Introduce，`TTY-TERM-001` / `TTY-INPUT-001` / `TTY-OUTPUT-001` / `TTY-REL-001` / `TTY-LIFE-001` /
`TTY-JOBCTL-001` / `TTY-ABI-001` Refine

## Scope

本transaction为长期、多Stage RFC保存checkpoint execution、review、validation与下一授权handoff。target、owner、
ABI、Contract Impact、acceptance与Stage路线仍只由canonical RFC及implementation拥有；本页不建立第二份计划或
current contract。开发者先后独立授权Stage 1 Checkpoint 1、Checkpoint 2、Stage 2、Stage 3的两个checkpoint与Stage 4
的两个checkpoint；全部授权均已消费并关闭。只有Stage 4 Checkpoint 2执行一次`PTY-DEVPTS-CUTOVER`，没有后续gate授权。

## Checkpoint Log

### 2026-08-11 - Stage 1 Checkpoint 1 activation and closure

**Change:** 从`dev/drc/alpha@32b6392f`的clean worktree激活Checkpoint 1。`TtyEndpoint`删除physical `TtyPort`，只保留
shared `Terminal`与不拥有状态的weak progress edge；physical attachment与worker持有`TtyPort`并在port-owned raw
RX/TX/idle predicate和Terminal之间搬运。boot-applied line snapshot由NS16550A driver作为owner-neutral值显式提交，
不再通过semantic endpoint或`TtyPort` trait泄漏。FileOps/relation继续只消费exact semantic endpoint与窄strong wake
capability；boot identity到endpoint的并行snapshot用常开assertion保持一一对应。旧helper、双路径、temporary adapter、
test-only facade、PTY/devpts surface与runtime relation lifecycle均未保留或引入。

inline owner-local KUnit新增/加强worker-spawn failure reservation rollback、pre-publication abort的
registry-before-stop/join与reference-cycle absence、notification只触发predicate recheck，以及selected physical identity
仍取得同一个semantic Terminal；既有duplicate attach、RX/TX partial progress与serial data-plane tests继续走production
transition。

**Review / Feedback:** 独立subagent首轮review为0 Apollyon / 0 Keter，指出一个Euclid：最初selected-endpoint test只按
硬编码index取数组元素，未经过physical identity到semantic endpoint的真实映射。实现删除该弱oracle，改为production
`select_endpoint()`同时消费parallel identity/endpoint snapshot、常开断言长度一致并按selected `TtyPortId`选择；KUnit
以`/soc/serial@2000`验证对应exact Terminal。复核确认该Euclid关闭，最终为0 Apollyon / 0 Keter / 0 Euclid。

source/bypass audit确认：semantic endpoint没有port/devnum/liveness/readiness truth；port只由
attachment/worker/driver持有；FileOps/relation没有physical representation bypass；wake count不参与行为；spawn failure
与abort cleanup exact且guards-out；没有public API/shared contract扩张、无第二状态真相、无临时双路径，也没有进入
Checkpoint 2或PTY-specific分支。

**Contract Cutover:** None。current TTY、opened-description、VFS、task/Signal/job-control contracts和register均未修改；
Stage 1整体与全部PTY能力尚未完成。

**Validation:** `git diff --check`与`just fmt kernel --check`通过。canonical
`./scripts/run-tty-test-rv64.sh --rootfs-sudo --busybox
etc/mounts/competition/preliminary/rootfs-rv/musl/busybox --sdcard etc/preliminary/images/sdcard-rv.img --mode auto
--log build/pty-devpts-stage1-ckpt1-rv64.log`在review修正后的最终candidate通过：repository RV64 release build完成，
618/618 KUnit通过；新增spawn-failure、abort/reference-cycle、selected identity/Terminal与既有TTY tests均为`ok`；guest
`TTYTEST:SUMMARY:PASS:50`，host报告`TTY-HARNESS:PASS:auto-byte-checks`与最终PASS，覆盖serial data plane、boot shared
Terminal、`/dev/tty` relation、BusyBox vi/ash并正常关机。首次不带`--rootfs-sudo`的同命令在进入kernel/QEMU前因
libguestfs/supermin不能读取host `/boot`停止；按LOCAL授权原样切换wrapper privilege mode后通过，不作为kernel失败。

LA64 build/runtime、PTY test app、LTP、tmux与sshd均Not Run，且不得从本次RV64证据外推。

**Next / Stop:** Checkpoint 1 Closed。执行严格停止在本checkpoint；Stage 1 Checkpoint 2仍Awaiting Authorization，
Stage 2--4保持Future，`PTY-DEVPTS-CUTOVER`仍Not Effective。下一步只能在维护者新的明确授权下进入Checkpoint 2。

### 2026-08-11 - Stage 1 Checkpoint 2 activation and closure

**Change:** 从`dev/drc/alpha@4fc8237d`激活Checkpoint 2。relation owner把boot-fixed slots替换为runtime registry：
`RelationEnrollment`是endpoint visibility前的一次性commit authority，`RelationParticipant`是绑定exact semantic endpoint与
participant generation的幂等retirement capability；registry仍唯一拥有participant membership、session binding、foreground
selector与relation generation。snapshot mutation同时核验endpoint exact identity、participant generation与relation
generation。retirement先从registry移除slot，再在guard外释放endpoint/session/foreground capability。

serial attach为每个unpublished semantic endpoint准备一次enrollment。boot transaction先完成devfs、file、vector capacity等
其它fallible prepare，再从unpublished owner取得authority并commit；partial commit失败由已提交participant的`Drop`回滚，
尚未发布的attachment cleanup也在registry removal后drop。成功publication把participant capability保留到reboot，不改变
`/dev/ttyS<N>`、`/dev/tty`、boot fd、devnum、console owner或existing relation/job-control行为。

inline relation-owner KUnit覆盖duplicate enrollment且不消费generation、partial prepare rollback、exact/idempotent
retire-no-entry，以及旧participant cleanup不能命中distinct replacement endpoint。全部测试直接走production registry
transition，没有validation-only facade。

**Review / Feedback:** 独立subagent review为0 Apollyon / 0 Keter，指出一个Euclid：最初stale-cleanup KUnit用同一个
endpoint的新generation，只证明generation轴，没有显式证明distinct replacement endpoint身份轴。测试改为旧endpoint
retire后enroll不同`Arc<TtyEndpoint>`，再drop旧participant并断言新membership仍在，关闭该证据缺口。review同时确认
`tty-test`只在`wait4`或pacing sleep返回`EINTR`时重验authoritative child status/predicate，PID、状态、bounded timeout与失败
cleanup均未放宽，oracle没有弱化。

首次使用`build/apps/busybox/busybox`的wrapper尝试在guest capability check停止，原因为该输入缺少验收所需BusyBox applet，
不是kernel failure。挂载盘可用后改用其RV64 static BusyBox；首轮guest暴露旧harness把快速child exit产生的预期`SIGCHLD`
`EINTR`误判为22项失败。修正仅重试`EINTR`，最终同一50项矩阵全通过，没有改变TTY状态、errno、超时或字节oracle。

source/lock/bypass audit确认：membership与session relation只有registry一份truth；endpoint、Session、Terminal、physical port、
unpublished/published vectors只持semantic capability或pre-visibility/retirement authority，不镜像relation状态；relation guard内
没有task/Signal/Event/worker调用或payload drop；physical `TtyPort`仍只由driver/attachment/worker持有；没有public API、
shared contract、serial ABI或PTY-specific branch扩张。

**Contract Cutover:** None。current TTY、opened-description、VFS、task/Signal/job-control contracts和register均未修改；
`PTY-DEVPTS-CUTOVER`仍Not Effective。

**Validation:** `git diff --check`、`just fmt kernel --check`与`just fmt tty-test --check`通过。canonical
`./scripts/run-tty-test-rv64.sh --busybox <mounted-rv64-busybox> --sdcard <preliminary-rv64-sdcard-master> --mode auto
--log build/pty-devpts-stage1-ckpt2-rv64.log`在最终candidate以direct rootfs mode通过：repository RV64 release build完成，
622/622 KUnit通过，其中4项relation lifecycle tests均为`ok`；guest
`TTYTEST:SUMMARY:PASS:50`，BusyBox vi/ash、relation/data-plane与字节oracle通过，正常关机；host报告
`TTY-HARNESS:PASS:auto-byte-checks`与最终PASS。

LA64 build/runtime、PTY test app、LTP、tmux与sshd均Not Run，且不得从本次RV64证据外推。

**Next / Stop:** Stage 1 Closed。执行严格停止；Stage 2--4保持Future且未获execution authorization，
`PTY-DEVPTS-CUTOVER`仍Not Effective。

### 2026-08-11 - Stage 2 activation and closure

**Change:** 从`dev/drc/alpha@c39bb65c`激活Stage 2。新增owner-private PTY pair capability：`Terminal`继续唯一拥有
termios/winsize/line discipline与stream buffers；pair只拥有phase、master liveness与slave-description participation。
master write进入shared Terminal RX conditioning，slave write/echo进入同一output并由master读取；zero slave descriptions
只形成可reopen peer absence，master final release形成不可复活retirement。prepared master/slave description把全部fallible
prepare放在participation前，并由infallible success tail消费exact `Arc<FileDesc>`，避免ghost participant。

description lifecycle复用creation-time single `FileDescOps::final_release`且保持flock-before-hook；`operation: Mutex<()>`只
串行化bounded Terminal commit与final release，不拥有stream或pair truth，也不跨wait、relation effect或notification持有。
poll register路径先安装Terminal progress route并保持non-sleeping；snapshot/final scan在iomux round退役后取得`operation`，
从同一pair/Terminal predicates投影READABLE/WRITABLE/HUP/ERR。master不是semantic endpoint，不取得relation operation，
Stage 2没有提交relation enrollment，也没有建立devpts、ptmx、index、pathname或PTY UAPI。

**Review / Feedback:** 首轮独立review依次关闭notification-under-operation、prepared-description ghost commit、empty-interest
HUP subscription、sample-byte writability与unused test fixture等blocker。runtime暴露poll在active iomux wait中取得sleepable
mutex；route correction把register保留为route-first non-sleeping读取，并让最终snapshot在round retire后串行化pair/Terminal
truth。随后source review发现snapshot也不能无锁跨retirement/flush组合，最终形成上述register/snapshot双路径。

维护者进一步明确KUnit不得触碰scheduler、timer、sleep、wait或人为interleaving，也不承担并发证明。最终删除TTY
worker/KThread/Event-timeout、PTY/Terminal iomux wait-round及跨owner file-table fixture/alias-fork-cloexec composition测试，
共移除12项；TTY FileOps与PTY remaining KUnit全部使用NONBLOCK，Terminal drain test直接检查owner state。当前TTY/PTY
KUnit只证明pure deterministic transform/buffer/data-plane/snapshot。concurrent participate-vs-retire、final close、
close-vs-poll、lost-wake与锁序仅由source/lock/linearization review证明；没有runtime interleaving proof。最终独立review
为0 Apollyon / 0 Keter / 0 Euclid，Architecture Friction Scan未发现需保留的具体摩擦。

**Contract Cutover:** None。current TTY、opened-description、iomux/epoll、VFS、task/Signal/job-control contracts与register均
未修改；`PTY-DEVPTS-CUTOVER`仍Not Effective。

**Validation:** `just fmt kernel --check`与`git diff --check`通过。最终canonical
`./scripts/run-tty-test-rv64.sh --rootfs-sudo --busybox <mounted-rv64-busybox> --sdcard
<preliminary-rv64-sdcard-master> --mode auto --log build/pty-devpts-stage2-rv64.log`通过：repository RV64 release build完成，
616/616 KUnit通过；remaining TTY/PTY pure tests均为`ok`；guest `TTYTEST:SUMMARY:PASS:50`，BusyBox vi/ash与existing serial
data-plane/relation oracle通过，host报告`TTY-HARNESS:PASS:auto-byte-checks`并正常关机。repository-wide KUnit总数只作
build/regression事实，不承担并发证明；本次serial userspace结果不外推未公开PTY runtime。

`just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G`通过，final symbol table为6255 entries。LA64
runtime、PTY test app、LTP、tmux与sshd均Not Run。direct rootfs mode曾在进入kernel/QEMU前因libguestfs/supermin读取
host `/boot`失败；最终按wrapper支持路径使用`--rootfs-sudo`通过，不作为kernel failure。

**Next / Stop:** Stage 2 Closed。执行严格停止；Stage 3--4保持Future且未获execution authorization，current contracts、
register与`PTY-DEVPTS-CUTOVER`保持不变。

### 2026-08-11 - Stage 3 Checkpoint 1 activation and closure

**Change:** 从`dev/drc/alpha@1e7676c6`激活Checkpoint 1。`device::tty::pty`按composition root、pair
lifecycle/participation与opened-description/FileOps拆为目录模块；`device::tty::file`按generic TTY operation、relation ioctl
与termios/winsize ABI职责拆为目录模块。existing module entry、consumer与有效visibility保持，`PtyPairState`原
crate-visible路径和`PtySlaveDescription`原TTY-owner-visible路径经窄re-export继续成立；没有public API、trait语义或
shared contract扩张。

master read/write/poll的既有逻辑收回`PtyMasterDescription`窄operation API，使FileOps adapter不再读取pair guard、
pair-owned Terminal或private fields。Terminal effect仍在`operation` guard内完成；notification、foreground relation
callback与callback-false accounting仍按原顺序在guard外执行；wait不持`operation`，poll register先安装route且不取mutex，
final snapshot才取mutex，final release仍先pair release再执行base/fanotify effect。relation callback只取得semantic
endpoint。`PtySlaveDescription`原`Opaque` marker保留，termios-only KUnit移到被测`file/termios.rs`末尾，PTY composition
KUnit保留在最低共同owner的`pty/mod.rs`。

**Review / Feedback:** 本地module-dependency audit发现初版拆分后的master FileOps仍直接读取pair guard；实现没有把该
owner penetration写成新常态，而是以上述description capability关闭。独立subagent首轮review为0 Apollyon / 0 Keter并指出
2个Euclid：拆分时遗漏`PtySlaveDescription`的`Opaque` marker，以及termios-only KUnit仍从parent module回引private helper。
两项均按原type semantics与KUnit placement规则修正。最终复核为0 Apollyon / 0 Keter / 0 Euclid / 0 Safe；Architecture
Friction Scan未发现第二份truth、owner穿透、private representation泄漏、public surface增长、caller/arch/test特判、临时
bridge、隐含cleanup顺序或用弱化oracle换取拆分闭合。

source/mechanical audit确认所有既有consumer迁移到唯一目录模块路径；`Terminal`、pair、relation与opened-description仍各有
一份truth；pair/file/relation依赖方向没有引入完整Task、file table、VFS private state或relation private state；没有
devpts/ptmx/UAPI、VFS activation、unused facade、compat wrapper、old/new双路径或Checkpoint 2 dormant production surface。

**Contract Cutover:** None。current TTY、opened-description、VFS、iomux/epoll、task/Signal/job-control contracts与register均
未修改；existing serial ABI、generic serial `O_NOCTTY` baseline和`PTY-DEVPTS-CUTOVER`保持不变。

**Validation:** `git diff --check`、`just fmt kernel --check`与`mdbook build docs`通过。final candidate的canonical
`./scripts/run-tty-test-rv64.sh --busybox <mounted-rv64-busybox> --sdcard <preliminary-rv64-sdcard-master> --mode auto
--log build/pty-devpts-stage3-ckpt1-rv64.log`以direct rootfs mode通过：repository RV64 release build完成，609/609 KUnit
通过；TTY/PTY owner-local tests继续以原语义路径运行；guest `TTYTEST:SUMMARY:PASS:50`，BusyBox vi/ash、host byte oracle与
orderly shutdown通过。canonical `just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G`通过，final symbol
table为6451 entries。

RV64结果只证明existing serial与owner-local KUnit回归，不外推未公开PTY runtime。LA64 runtime、PTY Rust test app、LTP、
tmux与sshd均Not Run。

**Next / Stop:** Stage 3 Checkpoint 1 Closed。执行严格停止；Stage 3 Checkpoint 2与Stage 4仍未获execution authorization，
Stage 3整体尚未关闭，current contracts、register与`PTY-DEVPTS-CUTOVER`保持不变。

### 2026-08-11 - Stage 3 Checkpoint 2 activation and closure

**Change:** 从`dev/drc/alpha@3490c0db`激活Checkpoint 2。新增尚未注册的single persistent `fs::devpts` instance，拥有
prebuilt persistent superblock/root、empty-data mount route、Kconfig capacity、reusable index、exact episode binding、
accepted metadata与retired inode reclaim；VFS继续唯一拥有mount view、inode/dentry与pathname cache。hidden ptmx
allocation在任何visibility前完成fd/index reservation、pair/master description、relation enrollment、inode/binding与static
final-release composition，随后以pair、binding和fd commit形成不失败success tail。pathname one-shot activation与
`TIOCGPTPEER`窄fd installer复用pair admission/participation和implicit relation effect；PTY ioctl raw codec只进入既有TTY
UAPI owner。

master final release先在pair owner内不可逆retire，再在pair guard外exact撤销devpts binding、回收retired inode、retire
relation并提交`SIGHUP`/`SIGCONT`及waiter recheck。cleanup capability捕获immutable episode/binding/relation identity，重复、
迟到或index reuse都不能影响新pair。devpts mount callback不读取target、namespace、credential或mount count；single-view与
last-view unmount都不驱动instance、binding或pair lifecycle。

**Review / Feedback:** 首轮stage-wide review暴露natural heap allocation被机械转成fallible API、fanotify raw-kernel open
无法安全消费activation、last-view测试未走真实mount tree、retired inode无法回收、relation guard内存在跨owner effect，以及
locked admission/冗余phase/test-only helper等问题。维护者接受R3工程约束：少量自然heap allocation在极端OOM时可按global
allocator policy panic，显式capacity/fd/VFS index/enrollment与fallible backing prepare仍保持errno和exact rollback。

实现据此保持自然owner shape；raw kernel `PathRef::open()`遇到activation稳定返回`NotSupported`并撤销prepare，fanotify映射为
`FAN_NOFD`；last-view KUnit改走真实mount tree；retired inode eviction变为exact、guards-out且可重试；relation logging、drop、
topology lookup与signal delivery全部移到relation guard外；locked admission只读取committed live pair，并删除冗余phase与
test-only production helper。最终独立review为0 Apollyon / 0 Keter / 0 Euclid / 0 Safe，Architecture Friction Scan未发现
需要保留的具体摩擦。generic VFS cached-positive freshness、late materialization、完整multi-view linearizability与并发runtime
interleaving没有被该review外推为已证明。

**Contract Cutover:** None。devpts filesystem registration、devfs `ptmx`/empty `pts` publication、persistent init mount与
public PTY namespace均未激活；current TTY、opened-description、VFS、iomux/epoll、task/Signal/job-control contracts、register与
`PTY-DEVPTS-CUTOVER`保持不变。

**Validation:** final candidate通过`git diff --check`、`just fmt kernel --check`与`mdbook build docs`。canonical RV64 TTY
wrapper通过repository build、623/623 KUnit、existing serial `TTYTEST:SUMMARY:PASS:50`、BusyBox vi/ash、host byte oracle与
orderly shutdown；结果只证明owner-local deterministic KUnit与existing serial regression，不外推未公开PTY runtime。
canonical `just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G`通过，final symbol table为6588 entries。

LA64 runtime、PTY Rust test app、LTP、tmux、sshd与所有public PTY pathname/ioctl runtime均Not Run。generic VFS
cached-positive freshness、late materialization、完整multi-view pathname linearizability，以及concurrent allocation/
open-vs-retire、final close、fd publication、relation/signal ordering、lost-wake与lock order的runtime interleaving均Not Proven；
后者只由source/lock/linearization review覆盖。

**Next / Stop:** Stage 3 Checkpoint 2与Stage 3 Closed。执行严格停止；Stage 4保持Future且未获execution authorization，
`PTY-DEVPTS-CUTOVER`仍Not Effective。下一步只能在维护者新的明确授权下进入Stage 4 public activation与acceptance。

### 2026-08-11 - Stage 4 Checkpoint 1 acceptance consumer and entry closure

**Change:** 从`dev/drc/alpha@59946c43`激活Checkpoint 1。新增唯一普通`pty-test`，由同一binary内的mount、allocation、
stream、lifecycle与relation模块直接判断R3显式输入和observable结果；覆盖single-instance multi-view mount、metadata/
lock/two-route admission、双向stream/termios/winsize/blocking/nonblocking/poll/select/epoll/partial progress、multiple
open/dup/fork/final release/reopen、retire/reuse fresh identity、implicit/explicit relation negative matrix、foreground/
background terminal signal、master hangup与capacity/churn。setup失败、case结果、summary drain与shutdown共享一条收口路径。

`anemone-rs`只新增app真实使用的`mount_with_data`、`getuid`/`getgid`/`setuid`与`TIOCGPTN`/`TIOCSPTLCK`/
`TIOCGPTPEER`窄wrapper；`anemone-abi`只补实际用于`F_GETFD`验证的`FD_CLOEXEC` raw constant。RV64/LA64
`pty-acceptance-*.toml`均通过existing rootfs composer选择`pty-test`并安装为`/sbin/pty-test`；两份init input都只指向该
binary，没有libc PTY wrapper、C实现、`*-oracle`、private artifact或旁路runner。

**Review / Feedback:** 独立review初轮发现三组Keter。第一组是foreground交给child后leader reclaim会被默认`SIGTTOU`
停止，foreground signal与background stop负路径又可能无限等待；修正为明确ignore `SIGTTOU`、predicate加failure bound，
并让background read在未触发job-control时以nonblocking结果fail closed。第二组是identity case在同一cached canonical
pathname上retire/reuse并假设first-free index，会越入generic VFS Not Proven边界并冻结allocator policy；修正为旧episode
只在canonical view materialize、持有其它live index直到目标index复用，再从此前未lookup该N的additional fresh view解析
current binding，capacity/churn只验证释放集合可复用。第三组补齐双向queue partial/readiness、master HUP出现/撤销、
post-hangup mutating ioctl、caller已有CTTY/endpoint已被其它session占用，以及setup/summary drain/shutdown；partial oracle
改为fill-to-`EAGAIN`、drain one byte、two-byte partial与suffix retry，不假设TTY Kconfig capacity。

最终独立review为0 Apollyon / 0 Keter / 0 Euclid；Architecture Friction Scan未发现第二份pair/readiness/relation truth、
test-only production API、重复UAPI、kernel/private owner穿透、app/architecture特判、无consumer wrapper、无退出bridge或
用弱化oracle换取build通过。一个app、一条`anemone-rs` ABI route与一份case result authority保持成立。

**Contract Cutover:** None。kernel、devpts registration、devfs `/dev/ptmx`/empty `/dev/pts` publication、persistent init、
current TTY/opened-description/VFS/iomux/epoll/task/Signal/job-control contracts与register均未修改；existing serial ABI、
generic serial `O_NOCTTY` baseline和`PTY-DEVPTS-CUTOVER`保持不变。

**Validation:** final candidate通过`git diff --check`与`just fmt all --check`。repository-native
`just app build --arch riscv64 pty-test`及`--arch loongarch64`均通过；
`just rootfs mkfs -c conf/rootfs/pty-acceptance-rv64.toml`与LA64 counterpart均通过并生成fresh staging/image。
RV64 `/sbin/pty-test`为12,554,824-byte static ELF，SHA-256
`aca5cda4a97f1d6c8fb36c2b6eeafa15886e7077d17fa3dd01738c188bb93299`，image SHA-256
`51a43f194b9674bb44b62c6947278658402242f1b3d373db181a0444b233bc1b`；LA64 binary为6,513,496-byte static ELF，SHA-256
`4168b7b52d03c66dbb66c4025949e4357b710ea8450c60452a71a0224cd64ec4`，image SHA-256
`48ec398ca6353767b912bcca39135614351bbf14a9e7c98cf0c675b222a0b270`。两者均无dynamic section，rootfs init均精确指向
`/sbin/pty-test`。

RV64/LA64 `pty-test` runtime、public PTY mount/path/ioctl/data-plane/lifecycle/relation/hangup、kernel activation、LTP、
tmux与sshd均Not Run；current contract/register与cutover为None/unchanged。generic VFS cached-positive freshness、late
materialization与full multi-view linearizability继续Not Proven。

**Next / Stop:** Stage 4 Checkpoint 1 Closed。执行严格停止；Checkpoint 2与`PTY-DEVPTS-CUTOVER`仍未获authorization，
不得由本次build/package/source review自动进入public activation或runtime acceptance。

### 2026-08-11 - Stage 4 Checkpoint 2 public activation与PTY-DEVPTS-CUTOVER

**Change:** 从`dev/drc/alpha@86e38e8b`激活Checkpoint 2。`devpts`在fs init中进入generic filesystem registry，并从该
registered filesystem构造唯一persistent system instance；全部fs initcall返回后，显式public activation先在devfs发布
empty `pts` directory，再发布global `ptmx` character node，避免依赖sibling initcall link order。repository-owned
user-test consumer只在chroot后的persistent `/dev` mount完成后显式把同一devpts instance挂到`/dev/pts`；pre-chroot
temporary devfs transport保持不变。

public activation没有新增kernel ABI或第二条allocation/open route。`/dev/ptmx`、pathname slave与`TIOCGPTPEER`继续消费
Stage 3已经关闭的single instance、binding、pair admission、fd reservation、relation enrollment与static final-release
composition；master retirement继续先提交pair state，再在guards-out撤销exact binding/relation并唤醒waiters。public
`pty-test`针对Linux 6.6.32 hangup source fact修正oracle：N_TTY hangup清除committed slave input，read返回EOF，termios/
winsize query与mutation返回`EIO`；generic relation child忽略只由case cleanup产生的`SIGHUP`，dedicated hangup case仍安装
handler并验证`SIGHUP`/`SIGCONT`。这些修正没有改变kernel visible behavior或降低coverage。

**Accepted R4 feedback:** focused LTP暴露两个不属于首版target的compatibility profile。维护者明确选择保留
`fsuid:fsgid`/`0600`，不引入system `tty` group、`0620`、mount options或`pt_chown`；随后明确同意保持首版现代
`TCGETS`/`TCSETS*`、winsize、relation/PTY ioctl surface，不新增legacy `TCGETA`/`TCSETA*`、`TCSBRK`/`TCSBRKP`。
四个selected LTP cases仍必须尝试并保留原始结果；直接对应上述accepted limitations的子项不阻塞closure，也不计为PASS。
R4只收窄accepted ABI/acceptance，没有移动owner、cleanup或Contract Impact。

**Source / owner / bypass audit:** boot route只有“all fs providers initialized -> devpts static namespace activation ->
persistent userspace mount -> workload”一条依赖链；devpts registry和instance各一份，mount只取得同一superblock/binding
projection，mount count不参与pair lifecycle。temporary consumer没有devpts特判。allocation/open的fallible prepare与
infallible success tail、opened-description final release、exact binding retire/reuse、stale capability、relation/hangup
guards-out cleanup、poll route-first/final predicate recheck均保持Stage 3 owner边界；public code没有test-only bypass、
second state truth、kernel ABI expansion或VFS private-cache workaround。generic VFS cached-positive revocation、late
materialization、readdir与forced concurrent multi-view linearizability仍为Not Proven。

**Validation:** final RV64 public PTY candidate log为`build/pty-devpts-stage4-ckpt2-pty-rv64-candidate.log`：repository build
完成，623/623 KUnit通过，guest `PTYTEST:SUMMARY:PASS:12`并orderly shutdown。12项覆盖allocation/identity/capacity、
mount views、stream/termios/readiness、description/fork/final release、两条slave-open route、implicit negative matrix、
relation/job-control/hangup与reuse。一次复用发生过KUnit `/kunit-openat-readonly-trunc` `EEXIST`，归因为复用writable
acceptance image留下旧fixture；重新生成rootfs后同candidate完整通过，不归因PTY。

LA64分别通过`just app build --arch loongarch64 pty-test`、`just rootfs mkfs -c
conf/rootfs/pty-acceptance-la64.toml`与`just build --preset qemu-virt-la64-release --bind smp=8 --bind memory=8G`，final
symbol table为6632 entries。没有architecture-specific UAPI/user-copy/ioctl差异，因此LA64 runtime保持Not Run，不从RV64
外推。

existing serial regression log为`build/pty-devpts-stage4-ckpt2-tty-rv64.log`：canonical SMP1/1G wrapper通过623/623
KUnit、`TTYTEST:SUMMARY:PASS:50`、BusyBox vi/ash、host byte oracle与orderly shutdown。该结果只证明existing serial
regression，不伪装成SMP8 public PTY topology evidence。

focused LTP log为`build/pty-devpts-stage4-ckpt2-ltp-rv64.log`；temporary profile修改已完整恢复。glibc与musl结果一致：
`hangup01` PASS；`ioctl01`各7/9 subtests PASS，两个legacy `TCGETA` pointer-error subtests返回`ENOTTY`而不是`EFAULT`；
`pty01`因actual `020600`不满足distribution-style `020620`而BROK，legacy binary随后把remaining cases标为broken；
`ptem01`首先在unsupported `TCGETA`失败，后续还要求`TCSETA*`与`TCSBRK`。合计attempted 8 case instances、passed 2、
failed/non-pass 6、infra_failed 0；只有实际PASS计入证据，未执行子项不外推。

tmux create/attach/detach/exit已尝试满足environment prerequisite：preliminary/final RV64 roots及build artifacts均没有
target tmux executable；final root只有tmux terminfo与editor support文件。因此workload为Not Run / Inconclusive，归因
缺少consumer而非PTY failure。sshd按R4设计为Not Run。

**Architecture Friction Scan:** 未发现第二份pair/binding/readiness/relation truth、owner穿透、private representation
泄漏、为局部需求扩大public API、caller/architecture/test特判、无退出条件bridge、隐含failure/cleanup逆序或降低
validation/ABI诚实性。public activation是同owner窄入口，post-chroot mount是既有persistent consumer的显式依赖；
accepted LTP limitations进入register而未以stub或弱oracle绕过。没有需要保留的Euclid/Keter/Apollyon。

**Review / Feedback:** independent final-candidate review首先确认源码activation、single instance、owner/lifecycle、
guards-out cleanup、final release、predicate/recheck与oracle均无Apollyon/Keter，随后指出三组Euclid：current contract的
transaction anchor顺序会断链；Closed/R4 RFC与invariants仍残留未标历史的future/R3 authority措辞；`ioctl01` 7/9一度
被误写成accepted-surface denominator。分别改为date-first heading anchor、retrospective R4/closure wording，以及
`overall 7/9 + two legacy pointer probes non-PASS`的逐项口径。最终复核为0 Apollyon / 0 Keter / 0 Euclid / 0 Safe。

**Contract Cutover:** mandatory evidence和final review关闭后已一次性执行`PTY-DEVPTS-CUTOVER`：新增
[Unix98 PTY与devpts current contract](../../contracts/tty/pty-devpts.md)，refine TTY data plane/relation/job-control，更新
register中的distribution metadata与legacy termio/break accepted limitations，并按真实结果缩减原ioctl LTP gap。
`OPENED-DESC-*`、VFS owner/current contracts与generic dynamic-positive issue不变。

**Next / Stop:** Stage 4 Checkpoint 2、Stage 4与RFC Closed；transaction Completed。执行严格停止，不进入任何后续gate。

## Current Handoff

Unix98 PTY/devpts已成为effective current capability。后续工作必须从current contracts和live source出发：首版保持single
persistent instance、`fsuid:fsgid`/`0600`、现代termios/winsize/relation/PTY ioctl，以及pair/description/relation/VFS各自
唯一owner。distribution-style `tty:0620`与legacy termio/break ioctl是accepted limitations；generic VFS cached-positive
freshness、late materialization与完整multi-view concurrency仍Open / Not Proven。LA64 PTY runtime、tmux与sshd保持Not Run，
不得从RV64或build evidence外推。本transaction不授权下一gate。
