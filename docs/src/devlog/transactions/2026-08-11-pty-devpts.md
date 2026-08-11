# 2026-08-11 - PTY / devpts

**Status:** Active / R2 / Stage 3 Checkpoint 1 Closed
**Owners:** doruche, Codex
**Canonical Target:** [RFC-20260810-pty-devpts R2](../../rfcs/pty-devpts/index.md)
**Implementation Route:** [Stage 1--4](../../rfcs/pty-devpts/implementation.md)
**Contract Delta:** None；`PTY-DEVPTS-CUTOVER`与父RFC列出的全部Introduce/Refine ID仍Not Effective

## Scope

本transaction为长期、多Stage RFC保存checkpoint execution、review、validation与下一授权handoff。target、owner、
ABI、Contract Impact、acceptance与Stage路线仍只由canonical RFC及implementation拥有；本页不建立第二份计划或
current contract。开发者先后独立授权Stage 1 Checkpoint 1、Checkpoint 2、Stage 2与Stage 3 Checkpoint 1；四个授权均已
消费并关闭。Stage 3 Checkpoint 2、Stage 4与任何contract cutover仍未授权。

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

## Current Handoff

当前live source已经关闭runtime semantic endpoint substrate、owner-private PTY pair/data-plane/description effect与
TTY-owner-local结构拆分：existing serial caller继续使用semantic endpoint/physical attachment单一路径，Stage 2 capability
仍不可由userspace发现。transaction保持Active只因为Stage 3 Checkpoint 2与Stage 4尚未执行；本记录不授权自动继续，也不把
serial userspace、KUnit计数或source review外推为公开PTY ABI/runtime acceptance或contract cutover。
