# Socket Abstraction 与 Unix Socket 实施路线

**状态：** R1 Accepted / Stage 1 Closed / Stage 2 Closed / Stage 3 Closed / Checkpoint 3A/3B Closed / Stage 4 Ready / Not Active
**最后更新：** 2026-08-02
**父 RFC：** [RFC-20260801-socket-abstraction-and-unix-socket](./index.md)
**当前修订：** R1
**当前实施阶段：** Stage 4 Ready / Not Active；resolution completed；尚未取得实施或cutover授权
（transaction None；contract cutover None）

本文只保存父 RFC 需要长期引用的多阶段实施路线。target、non-goals、owner、ABI、Contract Impact、acceptance 与
最终 validation boundary 仍由父 RFC [index](./index.md)和[目标与不变量](./invariants.md)定义；本页不建立并列
target、执行状态总表或验证证据副本。

R0 acceptance 与 Stage 1 resolution 已于 2026-08-02 分别完成；Checkpoint 1A、1B 随后各自取得实施授权并依次
关闭，Stage 1 因两个真实 consumer 均已落地而关闭。独立的 Stage 2 resolution 根据 Stage 1 actual diff、current
contracts、register、review finding 与 Linux 6.6.32 oracle 把本 Stage 解析为两个有序 checkpoint；Checkpoint 2A、2B
随后各自取得独立授权并依次关闭，Stage 2 已关闭并停止在 Stage 3 前。最终 `SOCKET-UNIX-CUTOVER` 仍是独立动作；
独立的 Stage 3 resolution 随后读取 Stage 1--2 final source、review、validation、current contracts、register 与 Linux
6.6.32 oracle，把本 Stage 解析为两个有序 checkpoint，但没有在 resolution 时激活任何代码实施。Checkpoint 3A、3B
随后各自取得独立授权并依次关闭，Stage 3 已关闭并停止在 Stage 4 前。resolution、checkpoint closure 与 Stage 1--3
closure均不使 pending contract 生效，也不自动进入下一 checkpoint 或 Stage。
独立的 Stage 4 resolution 随后读取最终candidate、完整actual diff、review finding、validation provenance、current
contracts与register，把最终综合验收和原子cutover解析为一个不拆checkpoint的Stage；本次resolution没有激活Stage 4、
运行验证、修改current contract或执行cutover。

## 全局 Implementation Boundary

### Target / non-goals

实施路线以 IPv4 UDP 与 filesystem pathname `AF_UNIX + SOCK_STREAM` 两个真实 consumer 共同证明最小 general
`Socket` front，并交付父 RFC 已列出的 Unix pathname stream 能力。TCP、其它 Unix Socket type、abstract namespace、
ancillary data、完整 socket option、`SO_ERROR`、Unix pending-error/error readiness 与通用 BSD Socket framework
继续是非目标。

阶段拆分只决定 accepted target 内的实施顺序，不得把尚未完成的局部 slice 写成较弱 target、accepted limitation 或
current contract。某个 Stage 可以形成安全、诚实的中间实现，但只有 Stage 4 满足父 RFC 全部 acceptance 后才能声明
Socket Abstraction closure。

### Owner / handoff / failure / cleanup

- general `Socket` 只拥有 immutable ops/type association、private storage envelope、共同 FileOps/opened-description
  projection 与 family-neutral ABI/wait orchestration；不得拥有 UDP 或 Unix runtime truth。
- UDP Stack Endpoint、Unix endpoint/listener/connection/directional stream、VFS namespace、opened-description、iomux
  与 epoll 各自保持父 RFC 已指定的唯一 owner。跨 owner 只传递 typed request/outcome、opaque capability、immutable
  snapshot 或 recheck hint。
- creation、socketpair、bind、connect、accept、stream progress 与 final release 分别由对应 transaction/state owner
  提交或清理。中间 Stage 不得用 `Drop`、fd number、pathname、ready cache、weak upgrade 或 temporary family tag
  代替 semantic lifecycle。
- 普通 allocator OOM 继续服从 kernel-fatal 边界；fd、backlog、buffer/capacity、VFS、user copy 与其它 normal
  resource failure 必须保持可返回 outcome 和相应 rollback/fail-forward。

### Protected ABI / contract / acceptance

- `NET-PROTOCOL-BOUNDARY-001`、`NET-SOCKET-ENDPOINT-001`、`NET-UDP-TRANSACTION-001` 与
  `NET-SOCKET-WAIT-001` 的 UDP owner、transaction、readiness 与 lifecycle 可见语义必须保持。
- `OPENED-DESC-001..003`、`VFS-CREATION-001`、`VFS-MAKE-NODE-001`、`IOMUX-POLL-001..003`、
  `EPOLL-WATCH-001`、`EPOLL-READY-001`与`EPOLL-FILE-001`在最终cutover前继续是current truth；Stage内部实现
  不得提前把pending Refine写成effective contract。
- 父 RFC 的首版 Linux UAPI、scoped conformance 面、显式排除、两个真实 consumer proof 与双架构/双 libc
  acceptance 不因阶段拆分而降低。
- 当前 [VFS non-UTF-8 pathname limitation](../../register/current-limitations.md#ane-20260801-vfs-non-utf8-pathname)
  和 [VFS common-create publication issue](../../register/open-issues.md#ane-20260801-vfs-create-publication-atomicity)
  保持原 owner；Unix Socket 不建立局部绕过。

### Validation claim

每个 Stage 只声明其 deliverable 对应的 source/owner、owner-local proof、canonical build 与 focused runtime 已被验证；
未覆盖的后续能力继续是 Not Run / Not Cut Over。Stage 4 才组合父 RFC 的完整 source audit、双架构、双 libc、Unix
pathname runtime、iomux/epoll、UDP regression 与真实 consumer 证据。architecture build、guest runtime 与 hardware
证据保持分层，不互相替代。

### Stop conditions

除父 RFC [停止条件](./index.md#停止条件)外，任一 Stage 遇到以下情况必须在完成声明或继续扩张前停止：

- 当前 slice 只有通过第二份 family/type/role/readiness/lifecycle truth、共同层 downcast 或 caller-specific branch
  才能继续；
- 为保持阶段顺序必须加入对外可见的临时 ABI、无退出条件的双路径，或把 target operation 的缺失伪装成成功；
- 某个 Stage 无法形成独立安全的中间状态，需要与下一 Stage 合并或重排；此时先更新本页路线，再取得新的当前
  Stage 授权；
- pathname bind 需要扩张 VFS common-create payload/callback/rollback protocol，或 wait/readiness 需要改变 iomux/
  epoll consumer policy；
- focused Linux oracle 证明现有 target 只能通过接受用户可见偏差、降低 validation 或扩大 scope 实现。

前三项若保持父 RFC target、owner、ABI、Contract Impact 与 acceptance，可以作为本页的 Route Correction；后两项
或任何语义边界变化必须回到 RFC review / Target Renegotiation。

## 路线与反馈原则

- Stage 按语义闭合单元组织，不按 syscall 数量、目录、crate 或 commit 数量组织。同 owner 的模块注册、新文件、
  import/re-export、定向测试和行为保持型拆分在当前 Stage boundary 内自然闭合。
- Stage 1 在关闭前必须同时拥有 UDP 与 Unix 两个真实 consumer。允许在 Stage 内先后迁移，但不得把只有 UDP 的
  common front 当成独立 Stage closure。
- Stage 2 先闭合 namespace、listener、connection admission 与 address lifecycle；Stage 3 再在Stage 1 basic
  stream/wait基线上把完整operation、shutdown与readiness一起闭合。若 live evidence 表明二者无法安全分离，在
  激活前合并或重排，不建立 temporary adapter 维持旧计划。
- 对应 Linux conformance oracle 必须在相关行为落地前形成 focused validation input；oracle 负责证伪 ABI bluff，
  不冻结 Linux 内部对象图、锁序或算法。
- 普通执行证据由 Git/PR 保存。当前不创建 transaction；只有实际执行变成长周期、多 checkpoint、probe/
  renegotiation 或多个需要独立追踪的 cutover 时，再按真实需要建立，且不复制本页计划。

实现反馈按以下权威面回写：

| 反馈 | 回写位置 | 当前 Stage 行为 |
| --- | --- | --- |
| target 内部的阶段合并、拆分、顺序、validation 或 owner-local 实施路线 | 本页 | 先更新路线；必要时重新取得当前 Stage 授权 |
| target、non-goals、owner、handoff、failure/cleanup、ABI、Contract Impact、acceptance 或 validation strength | `index.md` / `invariants.md` | 停止并进入 RFC review / Target Renegotiation |
| 实际采用 bind weak-atomicity 退路或发现其它当前缺口 | register + RFC closure | 有 live evidence 后记录，不预建 limitation |
| effective shared rule | current contract | 仅 Stage 4 `SOCKET-UNIX-CUTOVER` 更新 |
| checkpoint、review、validation 与 cutover 执行事实 | Git/PR；按需 transaction | 不复制 target 或阶段路线 |

## 阶段路线图

| 阶段 | 当前状态 | 概括目的 | 前置依赖 | 下一步解析触发点 |
| --- | --- | --- | --- | --- |
| Entry | Completed | 完成 Draft review、R0 acceptance 与 Stage 1 实施解析 | 当前 RFC、current contracts、register、live owner | 已完成；后续 1A 由独立授权激活并关闭 |
| Stage 1 | Closed / 1A Closed / 1B Closed | 建立由 UDP 与 Unix `socketpair` 同时消费的 Socket front vertical slice | Entry resolution 与两个独立 checkpoint 授权 | 已完成；未激活后续 Stage |
| Stage 2 | Closed；2A/2B Closed；Cutover None | 闭合 pathname namespace、listener、connection admission 与 address lifecycle | Stage 1 Closed；Stage 2 resolution completed | 已完成并停止；Stage 3随后独立解析为Ready |
| Stage 3 | Closed；3A/3B Closed；Cutover None | 先闭合directional stream operation、shutdown与message/query ABI，再接通listener/stream poll/select/epoll readiness | Stage 2 Closed；Stage 3 resolution completed | 已完成并停止；Stage 4随后独立解析为Ready |
| Stage 4 | Ready / Not Active；Cutover None | 复核最终candidate的综合acceptance并原子执行`SOCKET-UNIX-CUTOVER` | Stage 1-3 Closed；Stage 4 resolution completed | 只能由新的明确授权激活整个Stage 4 |

Stage 1--4 已按下文解析到可执行粒度；Stage 4不拆checkpoint，最终review、validation disposition与contract cutover共同
构成一个原子closure unit。
不创建逐文件 write set 或 Resolved Write Set Manifest；预计模块只作非穷举提示。同 owner 新文件、模块注册、
import/re-export、定向测试和行为保持型拆分可在当前 checkpoint 内自然闭合。若用户只授权当前 Stage 或 checkpoint，
关闭后必须停止。

## Entry — R0 与首阶段解析

Entry 不是实现 Stage，也不产生代码或 contract cutover。

**目的：** 完成父 RFC 的 Draft review 与 R0 acceptance，并把 Stage 1 从 Outline 解析为一个可独立授权、可安全停止的
实施单元。

**前置：** 父 RFC [Draft -> R0](./index.md#draft---r0-文档接受)条件全部满足；current UDP、opened-description、VFS、
iomux、epoll contracts 与 register 已重新核验。

**受保护边界：** R0 只接受 target，不授权代码；Stage 1 resolution 不得修改 target、Contract Impact、最终
acceptance 或 current contracts。

**解析义务：** 基于 live source 确认 UDP-specific front/ABI/wait 到共同 front 的迁移闭包，选择能够让 Unix
`socketpair` 成为真实第二 consumer 的最小 production vertical slice，并判断 Stage 1 是否需要有序 checkpoint。
checkpoint 只能作为同一 Stage 的 review/恢复边界；任何 checkpoint 都不能单独宣称 Socket Abstraction 已被两个
完整 consumer 证明。

**退出：** R0 已被 owner/reviewer 接受，Stage 1 的语义 deliverable、两个有序 checkpoint、验证、停止与退出条件也已
按下文解析。Entry 以 Stage 1 Ready / Not Active 状态完成；后续两个独立授权已依次激活并关闭 Checkpoint 1A、1B，
但未激活 Stage 2。

## Stage 1 Closed — 双 consumer Socket front vertical slice

**目的：** 在同一 Stage 中建立最小 general Socket front，并让现有 UDP 与 Unix `socketpair` connected-stream
vertical slice 都通过该 front 的 creation、ABI/FileOps、opened-description 与 family dispatch 路径运行。

**前置：** Entry resolution 已完成；Checkpoint 1A、1B 分别取得明确实施授权并按 1A -> review -> 1B 的顺序关闭。
任何 checkpoint closure 均未自动激活下一 checkpoint 或 Stage。

### Live baseline 与解析结论

resolution 读取的 live baseline 包括：`fs::socket::udp` 以 UDP-specific `FileOps` 与 `UdpSocketFile`承载private
association；`socket/bind/getsockname/sendto/recvfrom` adapter通过该FileOps identity取得concrete UDP state；
`task::files`已提供单fd reservation、opened-description status、静态direct-read/final-release hook与exactly-once
terminal callback；UDP source已经接入current iomux snapshot/register/final-scan协议。Network Stack Endpoint与
`anemone-net-api`不需要为Unix Socket改变。

基于这些事实，Stage 1 不需要 probe，也不需要把 Unix state 放入 Network Stack、VFS inode或pipe owner。它解析为两个
有序 checkpoint：Checkpoint 1A 先完成共同 front 与 UDP 行为保持迁移，Checkpoint 1B 再以 Unix `socketpair`闭合
第二consumer和Stage 1。1A结束时只有UDP消费共同front，因而只是安全的owner-migration checkpoint，不能单独宣称
`SOCKET-FRONT-001`已被两个consumer证明。

`socketpair`一旦对用户成功，默认blocking opened description就不能在empty/full predicate上临时返回`EAGAIN`、
`EOPNOTSUPP`或busy-poll。因此Stage 1必须随paired creation一并交付basic read/write blocking loop、`O_NONBLOCK`/
`SOCK_NONBLOCK`、ordered-prefix progress、EOF、`EPIPE`/`SIGPIPE`与peer final close的HUP。支撑这些operation的
ordinary READABLE/WRITABLE/HANG_UP source predicate和route publication同属Stage 1；shutdown、`MSG_*`、RDHUP/ERR、
half-close与完整poll/select/epoll矩阵仍由Stage 3闭合。这个划分增加的是target内的实施顺序，不改变R0 target、
Contract Impact或最终acceptance。

### Stage 1 Implementation Boundary

**Target：** 建立唯一common Socket file/front、静态ops/type witness和family-private envelope；把当前UDP全部迁移到
该front且保持current行为；交付`AF_UNIX + SOCK_STREAM + protocol 0`的`socketpair`，并以真实basic stream/wait/
final-release路径证明Unix是第二consumer。

**Non-goals：** Unix single-socket creation、pathname bind、listen/connect/accept、address snapshot/query、shutdown、
`send*`/`recv*`与`MSG_*`、socket options、RDHUP/ERR、half-close与完整iomux/epoll conformance、Unix datagram与TCP
均不进入本Stage。尚未交付的target operation必须从descriptor absence或typed unsupported稳定拒绝，不建立success
no-op或caller特判。

**Owner / handoff：**

- general Socket owner只保存一份静态ops/type association及对应opaque private storage，并拥有共同anonymous Socket
  inode/FileOps、family-neutral resolver、opened-description hook和blocking/wait orchestration；只有关联的concrete ops
  可以解释private storage；
- UDP private state继续拥有operation serialization、source association与Endpoint capability，Network Stack继续唯一
  拥有Endpoint/binding/datagram/readiness facts；1A只改变kernel dispatch shape；
- Unix endpoint owner拥有endpoint role和connection association；paired connection唯一拥有peer relation，两条
  directional stream分别拥有bytes、capacity、writer/reader terminal与basic source predicate。endpoint只持side与
  connection capability，不复制directional facts；
- `task::files`继续唯一拥有fd reservation/publication、status/fd-local flags和semantic final release。Socket ABI只传
  normalized type/request、typed outcome与user-copy cursor，不把Task、fd number、raw pointer或Linux errno传给family。

**Failure / cleanup：**

- UDP single creation在fd publication前由creation guard拥有abort；1A必须把UDP现有source/Endpoint retirement顺序原样
  收入共同creation/final-release orchestration，不能让`Drop`或`Arc` count变成semantic close；
- socketpair transaction先用现有fd reservation能力取得两个slot，并按tracked Linux oracle在publication前完成fd pair
  copyout、paired state与两份opened description准备。任一reservation、copyout或preparation失败都撤销两个reservation
  及全部unpublished Unix state；用户内存可以保留oracle允许的fd数字前缀，但对应slot不得live。两次commit之后不再有
  可返回失败；不为形式上的同时publication扩张task/files为generic multi-fd transaction；
- directional buffer capacity是normal resource boundary，必须进入KernelConfig并形成partial progress或not-ready，不能
  用无界queue、panic或allocator-OOM分类吸收；copy fault不得消费/提交超过已成功copy的prefix。若当前generic read/write
  trampoline无法保持该边界，可窄化复用或扩展创建时固定的direct-user transaction hook，但不得改变opened-description
  lifecycle owner或暴露family private state；
- final release先撤销本endpoint的新operation/source publication，再由connection/direction owner提交peer可观察的EOF/
  write failure并取得需要通知的route snapshot，guard外notify/drop。关闭非最后dup/fork alias不得推进这些事实。

**Protected ABI / contract：**

- 1A完整保持`NET-PROTOCOL-BOUNDARY-001`、`NET-SOCKET-ENDPOINT-001`、`NET-UDP-TRANSACTION-001`与
  `NET-SOCKET-WAIT-001`；现有UDP family/type/protocol/flag errno、bind/getsockname、send/receive、blocking/readiness、
  copy与final-release结果不得漂移；
- resolver只接受本Stage两个resolved type：IPv4 UDP与Unix stream。`SOCK_NONBLOCK`进入shared description status，
  `SOCK_CLOEXEC`进入各fd-local flag；raw tuple不进入Socket或private state。`socketpair`只支持Unix stream pair，UDP
  pair继续按Linux oracle稳定拒绝；
- Stage 1 Unix成功面至少包括paired create、basic read/write及其自然vector projection、blocking/nonblocking、
  dup/fork/final close、EOF与`EPIPE`/`SIGPIPE`。对read/readv与write/writev，ABI adapter、user-copy cursor和
  family commit必须共同保证只报告/消费/提交成功prefix；完整`MSG_PEEK`、shutdown与fault/race矩阵留待Stage 3复核；
- basic Unix source只从direction owner的current bytes/capacity/EOF/terminal-write outcome投影普通READABLE/
  WRITABLE，并在peer final close使endpoint完整terminal时投影current source-neutral HANG_UP；它服从current
  `IOMUX-POLL-001..003` publication/final-scan协议，不发布RDHUP/ERR，不缓存ready mask，不改变epoll policy。本Stage
  不执行任何pending Socket/Unix/IOMUX/Epoll contract cutover。

### Checkpoint 1A Closed — Common front 与 UDP migration

**状态：** Closed。共同 front、UDP 等价迁移、完整 diff review 与 focused proof 已闭合；执行证据由 Git/PR 保存。
1A closure 当时仍只有 UDP 一个真实 consumer，因此本 checkpoint 本身不宣称 Stage 1、`SOCKET-FRONT-001` 或任何
current-contract cutover 已完成；Checkpoint 1B 后续由独立授权激活并关闭。

**Deliverable：** 引入最小general Socket front、static ops/type witness、common Socket inode/FileOps与description hooks；
把`socket/bind/getsockname/sendto/recvfrom`和poll/final release改为只识别common front并经UDP ops分发。现有
`UdpSocketFile`职责作为UDP-private state保留或按同owner语义命名/拆分；ABI adapter、common FileOps与final-release
hook不得再按UDP concrete FileOps/private type分类。旧UDP-only dispatch在1A内删除，不保留双路径。

**Focused proof：** owner-local proof覆盖resolver witness、unsupported capability、single creation abort、common
final-release到UDP retire的exactly-once handoff；现有UDP host topology与`socket-test` UDP suite继续证明bind/send/receive、
blocking/readiness、dup/fork/CLOEXEC与retire/reuse。测试不冻结case数量，只要求每个高风险handoff有能够证伪回归的
oracle；不得为了满足数量复制相同路径。

**Exit / stop：** source audit必须证明syscall/FileOps/opened-description只有common front identity，只有UDP ops解释
UDP private state，Network Stack及`anemone-net-api`没有Unix/future-TCP改动，current UDP runtime无退化。若迁移需要
common层保存第二份family/type/readiness、让UDP ops接收rawLinux ABI，或扩大Network contract，立即停止。1A通过后
只标记checkpoint closed并review完整diff；不自动激活1B，不做contract cutover。

### Checkpoint 1B Closed — Unix socketpair production vertical slice

**状态：** Closed。Unix `socketpair` 已作为第二个真实 consumer 通过同一 Socket front 交付，Stage 1 随之关闭。
transaction 仍为 None，contract cutover 仍为 None；本checkpoint关闭时没有解析或激活Stage 2，后续独立resolution
不改变这一历史边界。

**Deliverable：** 增加Unix stream concrete ops、paired endpoint与connection/directional owner；接通
`socketpair(AF_UNIX, SOCK_STREAM, 0, ...)`及两种creation flag；通过common FileOps/description hook交付basic
read/write/vector、blocking/nonblocking、ordinary READABLE/WRITABLE recheck、dup/fork/final-close、EOF、
`EPIPE`/`SIGPIPE`与full-close HANG_UP。Unix single-socket create capability保持明确unsupported，留给Stage 2与unbound/
pathname role一起闭合。Unix不导入`net`owner；不把两根pipe拼成并列lifecycle/readiness truth。

directional capacity作为单一KernelConfig项进入现有kconfig生成/验证owner，并以编译期non-zero assertion保护。Stage 1
不预建backlog、pathname registry、SocketClass、dynamic ops registry、option bag、pending error或future TCP slot。

**Focused proof：**

- owner-local proof组合验证paired connection只提交一次、两条direction不串线、capacity/partial progress、
  snapshot-register-recheck窗口、copy-fault prefix、pair abort、close非最后alias与final-close terminal handoff；这些语义
  由composition test按共同state transition证明，acceptance看oracle覆盖而不看test数量；
- 同一`socket-test` guest consumer在RV64与LA64验证resolver/flags、bad pair pointer不留下live fd、双向byte order、
  empty/full blocking wake、nonblocking `EAGAIN`、dup/fork alias与final EOF/`EPIPE`/HUP。glibc/musl的focused
  `socketpair02`只作为creation flag的独立ABI oracle；要求Unix datagram成功的`socketpair01`不属于本R0 target；
- 1A的既有UDP real-consumer回归必须在1B最终diff上重跑。新增Unix case不得复制已有UDP、opened-description或iomux
  owner-local proof；只补跨owner handoff与用户可见结果。

**Canonical validation：** 使用repository入口执行`just test xtask`、`just test net-host`、`just fmt all --check`、
双架构`socket-test` app build与两个显式release preset build；RV64/LA64 build串行，避免共享generated DTB竞争。focused
guest evidence通过两架构`run-user-test` wrapper及显式preliminary image运行，并在同源guest路径执行`socket-test`、
UDP regression与选择的`socketpair02` oracle。最后运行source/residual-reference audit、`git diff --check`与
`mdbook build docs`。命令成功只证明其对应层级，不把build、KUnit或一架构runtime冒充另一层证据。

**Not Run / non-claim：** pathname/listener/connect/accept、shutdown与`MSG_*`、RDHUP/ERR、half-close及其HUP矩阵、
完整socket LTP、final harness、physical hardware与`smp>1`均不属于Stage 1 closure proof；保持Not Run / Not Cut Over。
Stage 1不要求以case数量证明质量，也不因未运行广泛但非当前oracle的suite而阻塞。

**Exit / stop：** 两个consumer必须实际消费同一front，旧UDP-only dispatch与Stage 1 migration bridge为零；Unix
pair transaction、stream predicate、copy progress、wait与final release均有唯一owner和focused runtime。若basic
blocking只能通过第二套socket wait queue、family隐藏wait loop、ready cache或跨sleep private phase实现，若pair rollback
需要改变task/files lifecycle/shared contract，或若正确EOF/`EPIPE`必须提前引入Stage 3的第二truth，停止并先修订路线或
回RFC review。1B通过后Stage 1 Closed；Cutover仍为None，并按当时授权停止在尚未解析的Stage 2之前。

**Closure evidence：** 最终 source review 无剩余 Apollyon/Keter；一项 suite dispatcher Euclid 已在本 checkpoint
内以“两个 suite 均运行后再汇总返回”关闭。`just test xtask` 为 66/66，`just test net-host` 全部通过，RV64/LA64
`socket-test` app build与显式 release kernel build通过。最终两架构 canonical wrapper分别运行同源`socket-test`：
RV64 KUnit 345/345、LA64 KUnit 350/350，两个架构均为 UDP 16/16、Unix 6/6，glibc/musl `socketpair02`各4 TPASS；
RV64完成machine power-off，LA64完成orderly shutdown后因当前平台无成功power-off handler进入末尾halt，并由QEMU
monitor退出。最终dispatcher-only failure aggregation修正后，双架构app build与formatter重新通过；成功suite路径未变。

**Not Run / non-claim：** pathname/listener/connect/accept、shutdown与`MSG_*`、RDHUP/ERR、half-close及其HUP矩阵、
完整socket LTP、final harness、physical hardware与`smp>1`均未运行，保持Not Run / Not Cut Over。任何pending
Socket/Unix/IOMUX/Epoll contract均未生效。

## Stage 2 Closed — Pathname namespace 与 connection admission

**Resolution状态：** Completed 2026-08-02。Stage 2已解析为Checkpoint 2A“endpoint/name/namespace”和Checkpoint 2B
“listener/connection admission”；两者随后各自取得独立授权并依次关闭，Stage 2 Closed / Cutover None。本resolution只保存
accepted target内的实施顺序、验证与停止边界，不创建transaction、不修改current contract，也不增加R0修订号。

### Live baseline 与解析结论

Stage 1 closure后的live `SocketOps`已经拥有single/paired creation、bind/local-address、send/receive、poll与final-release
能力族，但Unix descriptor只提供paired creation、basic stream/poll与final release；live `UnixEndpoint`也仍由一份
`connection + side`直接表示已连接端。它没有single-socket state、local-name、binding、listener或accept transaction，
因此Stage 2不能在现有connection-only object上继续追加optional pathname/backlog字段，也不能让general Socket以
concrete Unix downcast补齐role。

current VFS已经由`KernelCreationPolicy`与`kernel_make_node_at`形成current-task lookup/DAC/umask/final metadata到
context-free make-node的production handoff；`InodeRef`以完整resident object identity判等并由clone保持identity lifetime；
`task::files`已有unpublished `FdReservation`及infallible commit。这些能力足以支撑pathname creation、stable identity
registry与accept publication，不需要probe、VFS payload/callback、generic inode attachment、multi-fd transaction或
opened-description lifecycle扩张。

Linux 6.6.32 evidence显示：pathname bind先创建filesystem node再发布Unix address；listen要求已有local name并由backlog
拥有admission；stream connect在full backlog返回`EAGAIN`并在blocking retry重新查找listener；accept先取得unused fd，
consume连接后才copy peer address，copyout失败关闭已consume child且不发布fd；unnamed address输出只包含family长度。
这些是scoped user-visible oracle，不导入Linux object graph或锁序：
`xref:linux-6.6.32:net/unix/af_unix.c#unix_bind_bsd`、
`xref:linux-6.6.32:net/unix/af_unix.c#unix_listen`、
`xref:linux-6.6.32:net/unix/af_unix.c#unix_stream_connect`、
`xref:linux-6.6.32:net/unix/af_unix.c#unix_accept`、
`xref:linux-6.6.32:net/unix/af_unix.c#unix_getname`、
`xref:linux-6.6.32:net/socket.c#__sys_accept4_file`与
`xref:linux-6.6.32:net/socket.c#move_addr_to_user`。

resolution选择两个checkpoint而不是按syscall拆分：2A先让endpoint role/local-name与VFS identity形成可独立验证、可安全
停止的namespace slice，并用现有socketpair证明name与connected association正交；2B再一次闭合listener backlog、
connect commit与accept consume。`listen/connect/accept`不拆成独立正式gate，因为三者共同定义一个admission protocol，
任一单独暴露都会产生没有完整producer/consumer/cleanup的半协议。

### Stage 2 Implementation Boundary

**Target：** 在Stage 1 common Socket front、opened-description与basic Unix stream之上，交付Unix single-socket creation、
filesystem pathname bind、local/peer address observation、listen/connect/accept/accept4、blocking/nonblocking admission、
namespace alias/unlink/rebind与对应final-release cleanup，使pathname连接可以通过现有read/write/vector路径实际交换数据。

**Non-goals：** `shutdown`、connected-stream `sendto/recvfrom`的完整flag/addr行为、`MSG_PEEK`、RDHUP、完整HUP矩阵、
listener/stream的poll/select/epoll conformance、socket option surface、`SO_ERROR`/pending-error/ERR readiness、Unix datagram、
abstract namespace、TCP、non-UTF-8 pathname修复与VFS common-create事务均留在既定后续边界。Stage 2不得为了等待
connect/accept提前cut over `IOMUX-POLL-002/003`或`EPOLL-READY-001`。

**Owner / state model：**

- Unix endpoint owner必须提供唯一role/association transition surface，表达unconnected、listening、connected与retired；
  local-name fact与connected association正交，但仍由同一endpoint owner协调一次bind、binding registration与cleanup。
  不能用多个boolean、pathname是否存在、Socket front字段或diagnostic id共同推导role。
- listener state唯一拥有backlog limit、pending accepted endpoint、connect-capacity predicate、accept-item predicate与各自
  recheck publication；两个operation可以共享wake infrastructure，但不得共享一份万能not-ready/ready fact。
- paired connection与两个directional stream继续拥有Stage 1的peer、bytes、capacity、EOF与terminal truth。pathname
  connect只能建立同一种connection/direction capability，不能复制一套pathname stream或把accepted queue变成第三份
  connection truth。
- Unix binding registry只拥有完整`InodeRef` identity到live binding capability的index。`ino`可以只作bucket/hash input，
  最终命中、cleanup与generation isolation必须比较完整identity或exact publication capability；registry不保存pathname、
  permission、listener ready mask或VFS private representation。
- local-name是endpoint-owned、bind-time immutable observation fact；binding registration是独立admission capability。
  accepted endpoint可以共享listener已提交的name capability，但不能继承listener registration。connection观察peer的
  唯一name capability，不能复制connect使用的alias；这必须覆盖connected unnamed endpoint后续bind与peer close后的
  address可观察期。

精确enum、capability carrier、map/bucket、queue、lock、route storage与module path是implementation preference。live
`unix/mod.rs`已经混合connection/direction、stream operation、poll、lifecycle与composition tests；在2A加入新职责前，
允许并预期先做同owner、行为保持的目录化拆分，使endpoint/name、namespace、listener/admission、stream/lifecycle等角色
拥有可审查的最低自然边界。拆分不是独立checkpoint，不得扩大public API、visibility policy或shared contract；精确
文件名和re-export布局不冻结。

**Handoff / failure / cleanup：**

- bind transaction先完成typed pathname/name snapshot、同Socket state admission，以及任何会正常返回失败的registration
  capacity/resource preparation；随后调用current-task kernel creation operation提交`InodeType::Socket + 0777` requested
  permission。VFS返回stable identity后，Unix commit不得再分配会正常失败的资源、interruptible wait或重新做state
  admission；registration与local-name在成功返回前共同可见。
- VFS node可lookup而Unix binding尚未publish的短窗口保持父RFC允许边界，并发connect可以失败。final release或其它
  late change若在VFS成功后使Unix commit无法安全完成，必须fail closed，不得让retired endpoint重新publication；若自然
  结果是failed bind留下inert inode，调用者显式unlink后才能复用，final close不自动unlink。
- connect每次attempt独立完成pathname lookup、prefix search、target `WRITE` DAC、socket-kind、exact identity registry
  lookup与listener admission；一次DAC或binding capability不跨wait。commit必须一次性形成client connected association、
  paired connection和由listener backlog拥有的accepted endpoint；任何retry、signal或late hint都不得duplicate commit。
- accept transaction使用现有fd reservation与unpublished Socket边界。listener consume后由accept transaction唯一拥有
  child；peer-address copyout成功后才commit fd。copyout或后续pre-publication失败按scoped Linux oracle关闭已consume child
  并rollback fd，不requeue、泄漏child或发布无返回fd。accepted description不继承listener `O_NONBLOCK`；仅accept4 flags
  决定accepted status/fd-local flags。
- final release先撤销endpoint取得新operation、binding与listener admission的publication，再让late capability在commit前
  revalidate/fail closed；随后drain queued child、提交对应direction terminal facts并取得需要通知的route snapshot，guard外
  notify/drop。cleanup不等待waiter运行，不通过path lookup删除node，也不让old binding/queue token命中新generation。

**Protected ABI / contract：** Stage 2补齐父RFC已接受的`AF_UNIX + SOCK_STREAM` syscall constants、`sockaddr_un`、
addrlen/copyout、errno与creation/accept flags，但不增加R0之外的ABI。Stage 1 UDP与socketpair的resolver、FileOps、wait、
stream、poll和final-release结果必须保持；新unconnected/bound/listening role进入既有FileOps时必须由Unix owner返回typed
state outcome，不能panic、让general Socket downcast或用success no-op隐藏未支持operation。新增family-neutral syscall
entry在UDP等不支持的operation上只通过static capability absence或typed outcome给出父RFC errno，不增加UDP peer/listener
state或caller特判。`OPENED-DESC-001..003`、`VFS-CREATION-001`、`VFS-MAKE-NODE-001`与current iomux/epoll contract
保持effective且不修改；Stage 2 Cutover为None。

### Checkpoint 2A Closed — Endpoint、name 与 pathname namespace

**状态：** Closed。single Socket、endpoint/local-name owner、filesystem pathname bind、exact identity registry和
`getsockname/getpeername`已形成可独立停止的namespace slice；transaction仍为None，contract cutover仍为None。

**Purpose：** 把connection-only Unix endpoint提升为能承载single Socket、orthogonal local-name与exact binding cleanup的
唯一role owner；接通single creation、filesystem pathname bind、getsockname/getpeername和namespace/address lifecycle，
同时保持Stage 1 socketpair与basic stream行为。

**Deliverable：**

1. `AF_UNIX + SOCK_STREAM + protocol 0` single creation使用Stage 1 common preparation/publication/final-release路径；
   unconnected、bound、connected与retired状态由一份Unix owner模型解释。现有read/write/vector/poll/final-release不再
   假设每个Unix endpoint必然connected；新role的失败必须是typed、稳定且不改变Stage 3最终oracle。
2. ABI boundary按raw bytes、family、addrlen与NUL boundary解析/输出filesystem `sockaddr_un`，继承non-UTF-8 limitation；
   unnamed、truncation、actual-length store与copy fault服从父RFCscoped oracle。raw sockaddr/padding/user pointer不进入
   endpoint、registry或VFS。
3. pathname bind复用current-task `VFS-CREATION-001` production handoff，existing final entry映射`EADDRINUSE`，不要求
   `CAP_MKNOD`；`umask 0027`下requested `0777`必须形成`0750` pathname inode。Socket/Unix不读取或缓存umask、uid/gid、
   parent permission，也不扩张`MakeNodeDescription`。
4. registry以stable identity索引exact live binding，支持同inode hard-link alias，rename/unlink不改变registration；
   同名rebind的新identity/generation与old final-release cleanup隔离。mknodat/reload/inert socket inode没有registration，
   不能自动恢复旧endpoint。
5. local-name snapshot只服务getsockname/getpeername与后续accepted/recvfrom观察，不参与lookup、hash、DAC或cleanup。
   socketpair endpoint与其它connected unnamed endpoint后续bind必须保持connection truth不变，并使peer通过唯一name
   capability观察新name；peer close、rename、link与unlink不改写snapshot。

**Focused validation：**

- source/owner audit证明general Socket没有新增family/role/name/registry truth，VFS没有Socket runtime payload，registry
  没有pathname/permission，Stage 1 connection/direction owner仍唯一；same-owner split若发生，旧path/re-export与双路径为零；
- owner-local proof覆盖single preparation abort、role/name一次提交、bind prepare-before-publish、exact identity/alias、
  generation-safe removal、final-release race、connected unnamed bind、peer-name capability与inert/reloaded inode miss；
- focused Linux oracle覆盖`sockaddr_un` input/output length、NUL/truncation、copy ordering/fault、repeat bind、unnamed
  getsockname/getpeername与connected-later-bind；oracle在相关代码落地前形成，不以header layout代替runtime side effect；
- RV64/LA64 canonical release build与同源guest runtime验证single Socket flags、ext4 pathname bind/stat/mode、
  `umask 0027 -> 0750`、parent DAC、rename/hard-link/unlink/rebind、final close不unlink、socketpair bind/name observation；
  Stage 1双架构UDP与socketpair suite在最终2A diff上回归。ramfs identity与failure paths由owner-local proof覆盖；
- `listen/connect/accept`、pathname data exchange、shutdown、RDHUP/完整HUP、完整socket LTP、final harness、hardware与
  `smp>1`保持Not Run / Not Cut Over，不以2A namespace成功替代。

**Exit / stop：** 2A退出要求single Socket与namespace/name slice形成独立安全实现，registry/local-name publication和
final cleanup全闭合，Stage 1 regression与focused双架构runtime通过，完整diff review没有未解决Apollyon/Keter。若必须
把runtime binding挂入generic inode/backend `prv`、按pathname做live lookup、持Unix global lock跨VFS、扩张VFS
common-create payload/callback/rollback、通过path compensation unlink，或为了支持新role提前引入Stage 3 readiness/
shutdown truth，立即停止。若采用inert-inode退路，2A closure必须记录具体failure point、errno、residue、cleanup与
observability，并按实际当前行为回写register。2A关闭后只标记checkpoint closed并停止；不得自动激活2B。

**Closure evidence：** Unix owner现以单一association表达unconnected/connected/retired，local-name与binding publication
正交；registry以`ino`只作bucket selector，命中和cleanup均比较完整`InodeRef`并使用exact generation capability。pathname
bind复用current `KernelCreationPolicy + kernel_make_node_at`，VFS只把该production handoff收窄暴露到`fs` owner，没有新增
Socket payload/callback、backend `prv`或pathname-keyed runtime map。same-owner拆分把endpoint/stream/lifecycle与namespace
职责分开，旧双路径为零。

owner-local proof覆盖single preparation abort、未连接typed outcome、一次bind/name commit、retire-during-prepare、exact
identity与generation-safe removal、connected-later-bind和peer close后name观察；raw sockaddr proof覆盖family/addrlen、NUL、
108-byte pathname及Linux的111-byte actual length。Linux 6.6.32 runtime oracle另确认unnamed length 2、unconnected
`read=EINVAL`、`write/getpeername=ENOTCONN`、无输入NUL的输出终止、`umask 0027 -> 0750`、repeat bind、connected-later-bind
与peer-close name lifetime。

最终RV64/LA64显式release preset build通过；同源pretest wrapper分别通过KUnit 351/351与356/356、UDP 16/16、Unix
10/10，以及glibc/musl `socketpair02`共2个case/8个TPASS。两架构均完成filesystem/network/device orderly shutdown；RV64
machine power-off成功，LA64因当前平台无成功power-off handler在末尾halt后由QEMU monitor退出。formatter、双架构
`socket-test` app build、`git diff --check`与文档build通过。

实现采用了父RFC允许的fail-closed退路：若endpoint在VFS node创建成功后、Unix registry/name commit前已经retire，bind
返回`EBADF`，留下没有registration/name的inert socket inode；final close不按pathname删除，调用者必须显式unlink，内核
notice记录pathname、ino与errno。该边界已登记为
[`ANE-20260802-UNIX-BIND-RETIRED-INERT-INODE`](../../register/current-limitations.md#ane-20260802-unix-bind-retired-inert-inode)。

**2A closure时的 Not Run / non-claim：** `listen/connect/accept`、pathname data exchange、shutdown、RDHUP/完整HUP、
完整socket LTP、final harness、physical hardware与`smp>1`当时均为Not Run / Not Cut Over；Checkpoint 2B与后续Stage
也尚未激活。后续2B证据只由下节closure拥有，不回写冒充2A证据。

### Checkpoint 2B Closed — Listener 与 connection admission

**状态：** Closed 2026-08-02；Stage 2 Closed / Cutover None；transaction None。已停止在未解析、未授权的Stage 3前。

**Purpose：** 在2A endpoint/name/registry基础上，一次闭合listen backlog、connect commit、accept consume/fd publication
与blocking/nonblocking admission，使filesystem pathname server/client通过Stage 1 basic stream形成production vertical
slice。listen、connect与accept是同一个protocol的producer/commit/consumer，不再拆正式checkpoint。

**Deliverable：**

1. listen只由已绑定且role允许的endpoint建立或更新唯一listener state；backlog按Linux-visible规则归一化，并受一个
   KernelConfig maximum约束。重复listen、capacity增长/收缩、listener close与queued count都由listener owner解释，
   不能让wait queue或Socket front复制capacity/count。
2. connect attempt在不持registry guard跨VFS或listener/client lock的前提下，取得operation-local exact binding
   capability；listener与client按明确、可审查的lock order revalidate binding/listening/client role/capacity，再一次提交
   connection、client association与pending accepted endpoint。资源在commit前准备；commit后不再有可返回失败。
3. full backlog的nonblocking connect返回R0 target的`EAGAIN`且不建立`EINPROGRESS` intent。blocking connect只持窄
   recheck/wake capability进入共同wait protocol；被唤醒后丢弃旧DAC/binding capability并从pathname lookup开始新attempt。
   listener close、capacity增长、client final release与signal/cancel必须唤醒或终止相应wait，notification只提示重查。
4. accept/accept4使用accept predicate而不是connect-capacity predicate；empty backlog的nonblocking结果为`EAGAIN`，
   blocking path复用共同wait/recheck外壳。fd reservation、child consume、peer-address copyout、FileDesc preparation与fd
   commit只有一个accept transaction owner；accept4只接收`SOCK_NONBLOCK | SOCK_CLOEXEC`，accept等价flags 0。
5. connected client、queued child与accepted Socket复用Stage 1 connection/direction；accepted local-name共享listener
   snapshot但没有registration，peer-name观察client唯一name capability。listener unlink/final close不终结已accepted
   connection；listener close对queued-but-unaccepted child和存活peer只推进既有terminal/EOF/HUP路径，不创建pending error、
   `ECONNRESET` latch或ERR readiness。

**Focused validation：**

- owner/predicate/commit/cancel表覆盖listen、connect、accept各自truth、linearization、late revalidation、signal与final
  cleanup；source audit证明registry guard不跨VFS/listener、general Socket不解释backlog/private role，listener routes不携带
  ready mask/errno/connection commit，public poll/epoll policy未改变；
- owner-local deterministic proof覆盖backlog 0/bound/max与repeat listen、full/not-full transition、parallel connect唯一
  commit、blocked connect重新DAC、listener/client close与late capability、accept empty/consume、fd exhaustion、copyout fault、
  accepted flag/noninheritance、queued child drain及lost-wake/final-recheck；
- focused Linux source/runtime oracle覆盖backlog归一化、listen/connect state errno、blocking connect与close/signal/capacity
  竞争、accept reserve/consume/copyout/fd publication、unnamed/bound peer address与allowed race outcomes；
- RV64/LA64同源guest pathname server/client验证blocking/nonblocking listen/connect/accept、basic read/write/vector、
  backlog pressure、parent search/create DAC、target write DAC、`chmod`后新连接拒绝而既有stream继续、rename/link/unlink
  listener、rebind、dup/fork/final close与accept4 flags；glibc/musl focused case覆盖ABI/errno/addrlen/copy fault；
- Stage 1 UDP/socketpair与2A namespace/name matrix在最终2B diff上回归。shutdown、`MSG_*`完整面、RDHUP/完整HUP、
  public listener/readiness conformance、完整socket LTP、final harness、hardware与`smp>1`继续Not Run / Not Cut Over。

**Exit / stop：** 2B退出要求pathname namespace、listener、connection admission、address lifecycle与cleanup形成完整
Stage 2 vertical slice，双架构focused runtime和两个libc oracle通过，所有临时unsupported/bridge从Stage 2 target surface
退出，完整diff review没有未解决Apollyon/Keter。若attempt/wait必须让family ops隐藏task wait loop、跨sleep保留DAC/
binding/private phase、重复connection或accept consume，若正确accept需要扩张task/files lifecycle/shared contract，若listener
blocking只能通过第二套Socket wait queue、ready cache或提前修改iomux/epoll consumer policy实现，立即停止并回RFC review
或先修订Stage 2路线。2B关闭后Stage 2 Closed / Cutover None，并停止在未解析、未授权的Stage 3之前。

**Closure evidence：** syscall ABI按“一项syscall一个`.rs`”落在`listen.rs`、`connect.rs`、`accept/accept.rs`与
`accept/accept4.rs`；`accept/mod.rs`只拥有两项accept syscall共同的fd reservation、child consume、peer-address copyout、
FileDesc preparation与publication transaction。唯一`UnixListener`拥有normalized backlog、pending child、connect-capacity
与accept-item predicate；endpoint association仍是role唯一truth，connection/direction继续复用Stage 1 owner。

connect每次attempt重新执行pathname lookup、target `WRITE` DAC、exact binding/listener/client revalidation；跨sleep只保存
non-owning recheck capability。route publication与predicate在同一owner lock/commit gate下复核，terminal register在没有保存
route时返回`Ready`而不伪称`Subscribed`。client connection commit、listener close、accept consume与admission分别在guard外
通知对应snapshot；owner-local KUnit显式先取得`Subscribed`，再证明client role变化、capacity恢复、listener close、accept
admission/close均产生hint且final snapshot裁决为ready。`ADMISSION_COMMIT`内没有allocation、VFS、user copy、wait或notify。

accept先reserve fd，再consume child；peer copyout或后续pre-publication失败关闭child并释放reservation，不requeue或发布fd。
accepted description不继承listener `O_NONBLOCK`，只消费accept4的`SOCK_NONBLOCK | SOCK_CLOEXEC`。listener final close先撤销
admission、唤醒connect/accept waiter并drain queued child；已accepted connection继续使用Stage 1 terminal/EOF/EPIPE路径。
完整对抗review发现并关闭了client role变化lost-wake、terminal伪`Subscribed`、late listen错误`EINVAL`和非确定性signal test；
最终review无剩余Apollyon、Keter或Euclid finding。

formatter、`just test xtask` 66/66、RV64/LA64 `socket-test` app build与显式release preset build通过。最终canonical wrapper
分别通过RV64 KUnit 361/361、LA64 KUnit 366/366，两个架构均为UDP 16/16、Unix 19/19，glibc/musl `socketpair02`各4 TPASS。
RV64完成machine power-off；LA64完成filesystem/network/device orderly shutdown后因当前平台无成功power-off handler进入
末尾halt，并由QEMU monitor退出。使用只读mounted初赛盘构造的glibc/musl focused admission oracle又各通过1 TPASS，覆盖
backlog、errno、addrlen、accept flags/copy fault与listener lifecycle；临时替换只发生在worktree runtime副本，完成后两份
binary的SHA256与mode均恢复为master内容，runtime uid/gid恢复为原始`0/0`。

**Not Run / non-claim：** `shutdown`、`MSG_*`完整面、RDHUP/完整HUP、public listener/readiness conformance、完整socket LTP、
final harness、physical hardware与`smp>1`继续Not Run / Not Cut Over。Stage 3/4与全部pending Socket/Unix/IOMUX/Epoll
contract均未激活或生效。

### Stage 2 Cutover、evidence 与 write-back

Stage 2没有独立current-contract cutover。Checkpoint 2A/2B与Stage 2 closure由本页和Git/PR保存实际review、validation与
Not Run evidence；transaction保持None。aggregate source/owner/lifecycle audit确认本Stage没有修改`task::files`、iomux或
epoll owner/policy，general Socket不解释family backlog/private role，VFS binding registry不持pathname/permission truth，
route只携带notification/recheck capability，旧Unix双路径为零。

2A已采用的failed-bind inert inode路线及其failure point、errno、residue、cleanup与observability继续由
[`ANE-20260802-UNIX-BIND-RETIRED-INERT-INODE`](../../register/current-limitations.md#ane-20260802-unix-bind-retired-inert-inode)
拥有；2B未产生新的current issue或accepted limitation，因此register不新增条目。所有pending Socket/Unix/IOMUX/Epoll
contract继续Not Effective。Stage 2关闭后已停止，不自动解析或激活Stage 3。

## Stage 1--2 implementation feedback interlude — 软件工程审查

**状态：** Closed 2026-08-02；Stage 1/2保持Closed，Stage 3保持Not Active，Cutover与transaction均为None。本节只
记录已关闭实现的review反馈与同owner修正，不修改R0 target、owner、ABI、shared contract、acceptance或validation
强度。

本轮source/owner/lifecycle审查发现一项correctness finding：accepted endpoint共享listener的不可变local-name
capability，但没有继承listener的live namespace registration；旧`BindingState::Unnamed`把“没有registration”错误地
等同于“没有name”。因此对accepted socket再次`bind`会先创建VFS inode、发布registry，再在重复name publication的
assertion处panic，留下跨owner的半发布状态。修正后`BindingPublication::{Absent, Preparing, Live}`只表达endpoint-local
registration phase，name仍由`EndpointName`唯一拥有；bind admission同时检查name与publication，在任何VFS创建前将
accepted rebinding稳定映射为既有`EINVAL`。owner-local KUnit证明该路径没有进入namespace preparation，guest regression
同时证明目标pathname保持`ENOENT`。

审查还完成了三项行为保持型结构维护。共同front由单文件拆为`front/{mod.rs,file.rs}`：前者拥有typed request/outcome、
static `SocketOps` dispatch与creation/accept guard，后者拥有anonymous inode、common `FileOps`、opened-description hooks、
user-copy adapter与`SIGPIPE`映射。Unix endpoint由单文件拆为`endpoint/{mod.rs,stream.rs}`：前者拥有role、local name、
binding publication与family composition，后者拥有paired connection、directional bytes/terminal facts、stream operation与
readiness route；跨sibling能力只使用`pub(in crate::fs::socket...)`，没有扩大socket owner之外的可见性。`socket(2)`与
`socketpair(2)`的tuple/flag normalization收敛到`api/resolve.rs`，`socketpair`只保留其Linux-specific
`EOPNOTSUPP` errno projection；resolver matrix与双架构runtime证明既有ABI不变。另将shared sockaddr reader改为中性的
`read_socket_address`，移除已失效的dead-code annotation，并把receive errno mapping移回production helper区域。

**Validation evidence：** `just fmt all --check`、`git diff --check`、`just test xtask` 66/66与`just test net-host`
通过；RV64/LA64 `socket-test` app build与两个显式release preset build通过。RV64 canonical build第一次在sandbox内命中
既有`lwext4` `Bad system call/SIGSYS`，完全相同命令在sandbox外通过，故只归类为环境限制。最终两架构canonical
pretest wrapper运行同源guest路径：RV64 KUnit 362/362、LA64 KUnit 367/367，均为UDP 16/16、Unix 19/19，glibc/musl
`socketpair02`各4 TPASS；RV64正常power-off，LA64完成filesystem/network/device orderly shutdown后因无成功
power-off handler进入既有末尾halt，并由QEMU monitor退出。独立change review复核publication/retire锁序、module
visibility、resolver ABI/errno、lifecycle与证据边界后没有Apollyon、Keter或Euclid finding。唯一residual coverage gap是
`socketpair(AF_INET, unsupported-type)`的`EOPNOTSUPP` syscall adapter没有直接assert；其分支保持基线等价并经source
audit，不构成本interlude finding或cutover blocker。

**Not Run / non-claim：** 完整socket LTP、final harness、physical hardware与`smp>1`未运行；Stage 3拥有的shutdown、
`MSG_*`、RDHUP/ERR与完整readiness closure未实现或验证。全部pending Socket/Unix/IOMUX/Epoll contract继续Not
Effective，本interlude不产生register条目，也不自动解析或激活Stage 3。

## Stage 3 Closed — Stream operation 与完整 readiness closure

**当前状态：** Closed 2026-08-02；Checkpoint 3A/3B Closed；Stage 4 Not Active；Cutover None；transaction None。
Stage 3 resolution 已把本 Stage 解析为Checkpoint 3A“directional stream operation与message/query ABI”和Checkpoint 3B
“listener/stream readiness与iomux/epoll projection”。resolution本身只更新accepted target内的实施路线、验证与停止边界；
未修改R0 target、Contract Impact或current contract，未创建transaction，也未在当时授权任何代码实施。

Checkpoint 3A实施期review发现Linux会在没有peer时持久化`sk_shutdown`，而R0禁止endpoint-local shutdown truth。
2026-08-02 Target Renegotiation已由维护者批准为R1 reduced target：unconnected、bound与listening role返回
`ENOTCONN`且不保存pending intent；connected direction语义不变。该修订新增register limitation与role/errno验证，
不改变Contract Impact/cutover，不激活3B，也不创建transaction。

### Live baseline、Linux oracle 与解析结论

Stage 1--2 final source已经形成三组可直接复用的事实和协议。第一，live paired `UnixConnection`中的每条direction唯一
拥有ordered bytes、capacity、writer/reader terminal fact与对应operation serialization；endpoint association在每次commit前
revalidate，final release先撤销endpoint publication，再把terminal transition提交给connection owner。current
`endpoint_terminal`只为Stage 1 full-close HUP保存peer-close fact；Stage 3引入half-close后若继续让它与direction共同决定HUP，
就会形成并列terminal truth，因此3A必须把完整投影收敛到direction端点事实并删除被替代字段。第二，listener唯一
拥有normalized backlog、pending child、connect-capacity与accept-item两个独立predicate；connect与accept已经分别通过窄
`SocketWait` source进入共同`wait_for_iomux_ready` snapshot/register/final-scan协议。第三，common Socket front已经支持
family-neutral send/receive capability、direct-user read/write transaction、opened-description status与poll dispatch，但
`sendto/recvfrom` adapter仍按UDP datagram形状解析address/payload，且尚无shutdown、socket option query或Unix完整message
flags入口。

live readiness baseline同时暴露出Stage 3不能只修改Unix stream source：

- `PollEvent`当前只有READABLE、WRITABLE、ERROR与HANG_UP，`ppoll`把requested `POLLRDHUP`折叠为HANG_UP；
- epoll接受`EPOLLRDHUP`但只记录“没有source truth”的compat notice，watch interest、exact scan与Linux copyout均不能交付
  独立RDHUP；
- connected Unix stream已经能发布READABLE/WRITABLE/HANG_UP route，但listener public `poll`仍是Unsupported，虽然同一个
  listener owner已经为blocking accept维护accept-item predicate与route；
- current iomux snapshot/register/final-scan与epoll watch/dirty/ONESHOT/copyout protocol已经满足所需consumer控制流，
  不需要新wait framework、ready queue、callback payload或consumer policy改写。

tracked Linux 6.6.32 source为本resolution提供以下scoped oracle：

- `unix_stream_sendmsg`只在没有destination address时执行connected stream send；connected stream携带address返回
  `EISCONN`，zero progress的terminal send返回`EPIPE`，且只有没有`MSG_NOSIGNAL`时产生`SIGPIPE`；已有prefix progress
  优先于随后terminal error；
- `unix_stream_read_generic`在queue有data时先交付ordered prefix，queue为空且receive side shutdown时返回EOF；
  `MSG_PEEK`不consume，copy fault只有在零进展时覆盖结果，已有prefix progress保持可见；`recvfrom`在family receive已经
  commit/consume后才执行用户地址copyout，后者失败是明确fail-forward而不是requeue；
- `unix_shutdown`把`SHUT_RD/WR/RDWR`提交为本端receive/send terminal，并把对偶terminal传播给peer。direction的writer/
  reader端点因此足以表达local shutdown、peer shutdown与final close；receive-half-close和full HUP必须从这些owner facts
  投影，不能另存ready mask或与direction并列的shutdown/HUP cache；
- `unix_poll`让receive shutdown同时形成ordinary readable与RDHUP，让双向terminal形成HUP，让下一次send立即返回terminal
  error的Socket保持writable，并让listener只从pending accept queue形成readable。unconnected、bound、listening、connected、
  half-closed与retired role的精确组合继续由tracked source和focused runtime共同证伪；
- common `getsockopt`从immutable type/family/protocol及listener role投影`SO_TYPE`、`SO_DOMAIN`、`SO_PROTOCOL`与
  `SO_ACCEPTCONN`。父RFC已经明确排除`SO_ERROR`与成功mutable option，因而实现只复用Linux optlen/copyout形状，不导入
  Linux pending-error或其它option state。

这些事实形成单向依赖：direction/listener owner先提交operation truth，source再从同一truth计算current readiness，iomux/
epoll最后按各自consumer policy投影Linux结果。resolution因此采用两个checkpoint而不是按syscall拆分：3A先闭合directional
terminal、stream message与query ABI，使3B有稳定、唯一的source facts；3B再一次接通listener、stream、poll/select与epoll。
3A结束时新增operation可以形成安全中间实现，但独立RDHUP与完整public readiness尚未交付，不能宣称Stage 3或任何pending
contract完成。3B完成Stage 3后仍没有contract cutover，并必须停止在Stage 4前。

### Stage 3 Implementation Boundary

**Target：** 在Stage 1 basic stream和Stage 2 pathname admission基线上，交付父RFC首版Unix stream的shutdown、connected
`sendto/recvfrom`、`MSG_DONTWAIT | MSG_NOSIGNAL | MSG_PEEK`、socket option query/rejection、完整role/stream readiness与
poll/select/epoll RDHUP/HUP矩阵；复核read/write/vector ordered-prefix与wait/lifecycle race，使UDP与Unix继续共同消费同一
Socket ABI/wait boundary。

**Non-goals：** `SO_ERROR`、Unix pending-error/ERR readiness、成功mutable socket option、`sendmsg/recvmsg`、ancillary data、
credentials、`MSG_WAITALL`/OOB、Unix datagram/seqpacket/abstract namespace、TCP、socket timeout、async I/O、splice与新
iomux/epoll delivery policy均不进入本Stage。unsupported flag/option必须按R1稳定拒绝并保留必要notice，不能为了运行现有
程序静默成功。

**Owner / state model：**

- 每条direction继续唯一拥有该方向的bytes、capacity、producer端与consumer端是否仍开放；shutdown与final release只在
  对应direction owner提交terminal transition。对某endpoint，receive terminal从incoming direction两端事实派生，send
  terminal从outgoing direction两端事实派生；receive-half-close、immediate terminal send与full HUP均是snapshot projection，
  不新增endpoint-local shutdown bit、`endpoint_terminal`副本、ready mask或writable cache。若live field shape需要调整，
  必须删除被替代truth，不能保留双写兼容桥；
- endpoint association仍只拥有unconnected/listening/connected/retired role及connection capability，listener仍唯一拥有
  backlog/pending queue。public listener READABLE与blocking accept读取同一个accept-item predicate、可以共享同一route
  registry；它们不得复制queue count，也不得误用connect-capacity predicate；
- general Socket只拥有immutable ops/type witness、family-neutral normalized request/outcome、blocking choice、wait orchestration
  与Linux结果mapping。family op拥有一次attempt的state admission、prefix selection、copy对应commit/consume、peek与typed
  terminal outcome；iomux/epoll只读取source snapshot，不解释Unix role、direction或shutdown；
- existing per-direction read/write operation serialization或等价窄owner mechanism必须防止user copy期间shutdown/final close
  之后仍提交非法prefix；不得持spinlock跨user copy/等待，也不得仅为shutdown新建generation cache。精确lock、cursor、buffer
  与request/result Rust类型是implementation preference。

Stage 3解析后的operation表如下；它定义owner与证明义务，不冻结函数签名或万能result：

| Operation / consumer | 唯一 completion / not-ready truth | Commit / consume owner | 等待与final recheck |
| --- | --- | --- | --- |
| connect | client role、resolved listener liveness与connect capacity | existing admission commit | operation-local connect source；retry重新lookup/DAC |
| accept / listener public read | listener pending child或terminal role | listener consume与accept transaction | 同一accept-item predicate和listener route；public poll不consume |
| send / write | outgoing direction send-terminal、reader admission与capacity | direction writer operation提交successful prefix | Socket wait只订阅该direction WRITABLE；terminal outcome也使attempt可立即完成 |
| receive / read | incoming bytes、local/peer receive terminal | direction reader operationconsume prefix；peek不consume | Socket wait只订阅该direction READABLE；data优先于EOF |
| shutdown | endpoint role及相关direction terminal facts | direction owner一次提交所选read/write terminal | 不等待readiness；transition后只发recheck hint |
| poll/select/epoll | listener或direction owner的current predicate | source snapshot；delivery由对应consumer owner提交 | current iomux/epoll register/final/exact scan，不共享operation commit |

**Handoff / failure / cleanup：**

- ABI adapter在进入family前解析raw flags、optional address、socklen/optlen与user cursor，family只接收typed semantic request；
  Unix和UDP可以消费同一normalized message envelope，但各自保留stream prefix与datagram whole-message transaction，不建立
  generic packet/stream queue或共同consume result；
- send/receive在零进展时返回typed would-block/terminal/copy outcome，已有successful prefix不得被随后signal、shutdown、
  peer close或copy fault抹去。`MSG_PEEK`必须在并发reader与shutdown下保持不consume；zero-length在R1 scoped oracle允许的
  state检查前后次序内稳定：receive不等待或虚假consume，send仍先裁决connection与terminal state，因此terminal
  zero-length send可以返回`EPIPE`并按`MSG_NOSIGNAL`决定`SIGPIPE`，但不得进入capacity wait或虚假commit；
- shutdown在connected direction上提交幂等terminal transition；invalid `how`返回`EINVAL`，unconnected/bound/listening
  返回`ENOTCONN`且不产生状态，retired/fd失败保持对应fd错误。transition与正在进行的copy/commit必须有明确linearization。final release继续先撤销endpoint operation publication，
  再提交双向terminal并在guard外notify/drop，不改为等待operation/waiter完成；
- Unix `recvfrom` peer-address snapshot继续来自peer endpoint唯一name capability。payload/stream consume与用户地址copyout的
  fail-forward、addrlen store和partial user-memory效果服从scoped Linux oracle；UDP现有datagram copy/consume contract保持
  不变，不能为了统一adapter静默改写；
- source transition先在listener/direction lock内更新truth并取得route snapshot，guard外notify/drop。late/duplicate hint只
  触发final/exact scan，不能携带RDHUP/HUP/errno/bytes或推进ET/ONESHOT policy。

**Protected ABI / contract：** Stage 3补齐父RFC已经接受的`shutdown`、`sendto/recvfrom`stream semantics、message flags、
`SOL_SOCKET` query/rejection与RDHUP ABI，不增加R1之外的新成功面。`O_NONBLOCK`与`MSG_DONTWAIT`只决定当前operation是否等待；
`MSG_NOSIGNAL`只抑制本次terminal send的SIGPIPE；accepted Socket status inheritance继续由Stage 2规则拥有。UDP tuple、address、
datagram atomicity、blocking/readiness、SIGPIPE absence与final release必须保持`NET-*`current contracts。Stage 3可以实现
`IOMUX-POLL-002/003`与`EPOLL-READY-001`的pending target shape，但current contract正文和状态在Stage 4前不修改。

**结构边界：** live Unix stream文件已经共同承载direction state、operation、poll与retirement。若3A/3B实际增加职责后使
这些角色无法独立审查，允许在当前checkpoint内先做同owner、行为保持的目录化拆分；拆分不构成独立gate，不扩大socket
owner之外visibility，不增加facade/trait层，也不冻结文件名。ABI syscall继续遵守一项syscall一个语义文件；只有真实共享的
normalization、copy或wait义务留在最低共同owner。

### Checkpoint 3A Closed — Directional stream operation 与 message/query ABI

**3A关闭时的状态：** Closed 2026-08-02；Stage 3尚未关闭；Checkpoint 3B尚未激活；Cutover None；transaction None。

**Purpose：** 在不改变iomux/epoll consumer policy的前提下，先让direction owner完整表达read/write terminal与shutdown，
并闭合Unix connected message、flags、query/rejection和prefix/copy语义，为3B提供稳定source facts。

**Deliverable：**

1. `SHUT_RD/SHUT_WR/SHUT_RDWR`通过family capability进入Unix direction owner；local read/write terminal、peer对偶terminal、
   repeated shutdown、concurrent shutdown/I/O与final close各有一次可解释commit。buffered data仍按scoped oracle先于EOF交付，
   terminal send返回`EPIPE`，zero progress时按`MSG_NOSIGNAL`决定SIGPIPE；没有connected direction的三种role按R1返回
   `ENOTCONN`且不改变后续role/admission；
2. read/write/readv/writev与connected `sendto/recvfrom`共享同一direction truth和prefix transaction。sendto允许null destination，
   connected Unix携带destination稳定返回`EISCONN`；send接受`MSG_DONTWAIT | MSG_NOSIGNAL`，receive接受
   `MSG_DONTWAIT | MSG_PEEK`，其它flag不以no-op成功。per-call nonblocking不修改opened-description status；
3. stream user-copy只提交成功prefix，peek不consume或释放capacity，receive consume与recvfrom address copy fault按Linux
   fail-forward处理；peer name仍由connection capability读取，不复制address truth。signal只在零进展、最终predicate仍
   not-ready时形成`EINTR`，已有progress优先；
4. 加入family-neutral `getsockopt/setsockopt` ABI入口：`SO_TYPE`、`SO_DOMAIN`、`SO_PROTOCOL`来自immutable descriptor，
   `SO_ACCEPTCONN`从concrete listener role派生，非listener Unix与UDP返回0；支持query的optlen/truncation/copyout服从
   Linux oracle。`SO_ERROR`、其它unsupported query和全部mutable option稳定`ENOPROTOOPT`，不分配option/pending-error
   state；
5. connect/accept现有operation-local wait与send/receive file-source wait继续进入同一shared protocol；只抽取真实重复的
   blocking choice/errno/SIGPIPE或cursor义务，不引入万能attempt result、family隐藏wait loop或第二套Socket wait queue。

**Focused validation：**

- owner/operation表source audit证明endpoint没有shutdown/ready副本，direction terminal是send/receive/RDHUP/HUP的唯一事实，
  general Socket不解释Unix private state，UDP datagram transaction与current wait source未改变；
- owner-local proof覆盖四种direction端点组合、buffered-before-EOF、read/write operation与shutdown/final-close竞争、repeated
  shutdown、capacity恢复、peek/consume、zero-length、short copy、copy fault、signal与SIGPIPE/NOSIGNAL；
- focused Linux 6.6.32 source/runtime oracle覆盖sendto null/non-null address、message flag拒绝、connected shutdown与buffered
  receive、partial progress、recvfrom peer/addrlen/copy fault，以及supported/unsupported socket option的optlen/copyout；
- R1 differential matrix独立覆盖unconnected、bound、listening三个role的`ENOTCONN`、无状态副作用及fd/invalid-`how`
  错误优先级，不把Linux pre-connection成功行为误报为通过；
- RV64/LA64同源guest runtime覆盖socketpair与pathname accepted stream的read/write/vector、sendto/recvfrom、blocking/
  nonblocking、peek、shutdown、peer close、dup/fork/final release和query matrix；glibc/musl focused case覆盖raw syscall ABI/
  errno/copy side effect；Stage 1/2 Unix与UDP send/receive/blocking/lifecycle suite在3A final diff上回归；
- public listener poll、independent RDHUP、epoll LT/ET/ONESHOT与Stage 4 integrated acceptance保持Not Run / Not Cut Over，
  不以3A operation success替代。

**Exit / stop：** 3A退出要求所有target operation与query的temporary unsupported/compat bridge为零，direction/shutdown/
prefix truth唯一，focused双架构/双libc与UDP regression通过，完整diff review没有未解决Apollyon/Keter。若shutdown只能通过
endpoint与direction双写truth、in-flight user copy无法在不持spinlock或新增stale generation cache的前提下正确linearize，
若family-neutral message shape迫使UDP改变datagram commit/copy contract，或Linux oracle要求加入R1排除的`SO_ERROR`/
pending-error/其它成功option，立即停止并回RFC review/Target Renegotiation。3A关闭后只标记checkpoint closed并停止；
不得自动激活3B或修改current contract。

**Closure evidence：** common Socket front只新增family-neutral shutdown、message与query capability；endpoint仍唯一拥有role/
association，connection direction仍唯一拥有bytes、capacity与writer/reader terminal facts，原`endpoint_terminal`副本已经删除。
shutdown、staged read/write与final release遵守endpoint-state到connection-state锁序，operation gate和commit前recheck阻止terminal/
retired transition后的非法prefix提交；route只携带recheck capability，truth更新后在guard外notify。R1的unconnected、bound与
listening `ENOTCONN`拒绝不保存intent，raw oracle确认后续connect/accept与data exchange不受影响。

最终RV64/LA64显式release kernel build与`socket-test` app build通过；canonical preliminary wrapper分别通过KUnit 366/366与
371/371，两个架构均为UDP 16/16、Unix 22/22，glibc/musl `socketpair02`各4 TPASS。RV64正常power-off；LA64完成
filesystem/network/device orderly shutdown后因当前平台无成功power-off handler进入既有末尾halt，并由QEMU monitor退出。
临时双libc raw-syscall probe在RV64/LA64四种组合均输出`SOCKET3A_R1_LIBC_PASS`，覆盖三种pre-connection role、无后续状态
副作用、shutdown fd/`how`优先级、signed negative `setsockopt` optlen、`ENOPROTOOPT`与`getsockopt`长度/copyout；probe与
临时profile wiring均已移除，初赛master image未修改。formatter与`git diff --check`通过。

最终独立diff review为Apollyon 0、Keter 0、Euclid 1。残余Euclid是验证覆盖而非状态模型缺陷：non-socket fd配合valid/
invalid `how`的lookup-priority cross-check没有直接runtime case，shutdown与staged read/write/final-close竞争也只有operation
gate、recheck与锁序source proof，没有同步并发runtime test。最小补强是在后续真实触达该面时加入non-socket errno matrix和
可控copy barrier race test；本缺口不扩大3A ABI/owner、不进入register，也不授权3B。

**3A closure时的 Not Run / non-claim：** public listener poll、independent RDHUP、poll/select/epoll完整readiness矩阵、完整
socket/network LTP、final harness、physical hardware与`smp>1`均未运行或未cut over。当时Stage 3尚未关闭，3B尚未激活；
transaction与contract cutover均为None，全部pending Socket/Unix/IOMUX/Epoll contract继续Not Effective。

### Checkpoint 3B Closed — Listener/stream readiness 与 iomux/epoll projection

**状态：** Closed 2026-08-02；Stage 3 Closed / Cutover None；transaction None；Stage 4 Not Active。

**Purpose：** 只从3A最终direction truth和Stage 2 listener truth计算current readiness，把listener、connected stream、
half-close与full HUP接入现有poll/select/epoll consumer protocol，并关闭Stage 3。

**Deliverable：**

1. Unix public poll按endpoint role分发：listener READABLE复用accept-item predicate与route，connected stream读取direction
   snapshot；unconnected/bound/listening/connected/half-closed/retired的ordinary readable、immediate writable与mandatory HUP
   组合服从scoped Linux oracle。public poll不consume backlog/data，不读取pathname或缓存role/queue count；
2. `PollEvent`或等价source-neutral vocabulary增加独立receive-half-close category。incoming data或receive terminal继续产生
   ordinary READABLE；send有capacity或下一次attempt会立即返回terminal error时产生WRITABLE；receive-half-close只在caller
   请求时投影RDHUP，full HUP与真实ERROR继续mandatory。Unix首版没有ERROR producer；
3. ppoll把`POLLRDHUP`解析/输出为独立category，不再折叠HANG_UP；pselect不直接暴露RDHUP，而由receive terminal同时形成的
   ordinary READABLE进入readfds，并按Linux分组把HUP只纳入read side、把真实ERROR纳入read/write side；writefds仍依赖
   immediate send completion，exception fdset不被伪装成Unix ERR/PRI。current snapshot/register/final-scan控制流不改变；
4. epoll把`EPOLLRDHUP`从compat-only bit提升为真实watch interest、route coverage、exact snapshot与Linux event output。
   `EPOLLERR|EPOLLHUP`保持mandatory，RDHUP不因source hint或compat acceptance直接deliver；LT/ET dirty、ONESHOT disable、
   generation、bounded scan、copyout rollback与epoll-file coverage owner保持current contract；
5. listener admission/consume/backlog update、direction data/capacity/shutdown/final close在owner lock内更新truth并选择需要
   recheck的route，guard外notify。register point同时发布route并读取同一predicate；late hint、retired watch与fd reuse只由
   final/exact scan裁决，不新建socket ready queue或poll/epoll special case。

**Focused validation：**

- source/owner audit证明listener public poll与blocking accept使用同一predicate/route owner，direction snapshot是I/O和
  readiness唯一truth，source-neutral category不携带Unix type，iomux/epoll没有解释family private state或降低final scan；
- owner-local deterministic proof覆盖listener empty/admit/consume/close、data/capacity、local `SHUT_RD`、peer `SHUT_WR`、
  `SHUT_RDWR`、buffered EOF、peer final close、send-terminal writable、register window、late/duplicate hint与route retirement；
- focused iomux/epoll proof覆盖ppoll requested/unrequested RDHUP、select read/write projection、epoll LT/ET/ONESHOT、ADD/MOD
  initial dirty、copyout rollback、watch/fd reuse、multiple waiters与mandatory HUP/ERR filtering；
- RV64/LA64同源guest runtime对socketpair和pathname listener/accepted stream执行poll/ppoll/select/pselect6/epoll矩阵，
  glibc/musl focused oracle覆盖role、EOF/RDHUP/HUP、shutdown与close组合；3A operation、Stage 1/2 Unix、current iomux/epoll
  product paths及UDP blocking/readiness/lifecycle在3B final diff上回归；
- full socket/network LTP、final harness、physical hardware与`smp>1`不属于Stage 3 mandatory proof，未运行时明确Not Run；
  Stage 4仍须在最终actual diff上复核父RFC完整acceptance，Stage 3 evidence不能自动替代cutover。

**Exit / stop：** 3B退出要求listener与stream完整target readiness可由owner facts推导，ppoll/select/epoll全部通过各自
final/exact scan交付，compat-only RDHUP notice和Stage 3 temporary bridge为零，双架构/双libc、iomux/epoll与UDP regression
通过，完整diff review没有未解决Apollyon/Keter。若listener public poll需要第二份queue/readiness truth，receive-half-close
必须与HUP共用一bit或独立缓存，source必须携带ready payload，或者正确结果要求改变current iomux/epoll wait、ET/ONESHOT、
copyout或delivery policy，立即停止并回RFC review；不得把consumer policy改动伪装成source接线。3B通过后Stage 3 Closed /
Cutover None，按授权立即停止，不自动解析或激活Stage 4。

**Closure evidence：** `PollEvent`现以source-neutral独立category表达receive-half-close，ppoll只在caller请求
`POLLRDHUP`时输出RDHUP；pselect继续按Linux HUP/ERROR fdset分组投影。Unix public readiness只读取endpoint role、listener
accept-item predicate与connection direction facts：listener public poll与blocking accept共享同一predicate/route owner，
connected stream的READABLE、WRITABLE、RDHUP与mandatory HUP均不依赖endpoint ready cache或第二份terminal truth。
epoll已把`EPOLLRDHUP`接入真实interest、route coverage、exact scan与copyout，LT/ET/ONESHOT及mandatory HUP策略保持由既有
consumer owner裁决。

pre-listen/pre-connect watch使用不携带readiness truth的非owning route。`listen`/`connect`在admission与endpoint owner锁内
把同一个预分配route原子移交给listener或connection-side route registry，发布role后在guard外发出recheck hint；exact
predicate scan仍是唯一交付依据。确定性owner-local KUnit分别证明pre-listen与pre-connection route在role transition后收到
第一次hint，并能从新owner收到后续admission/direction hint，不依赖sleep或调度时序。最终同一独立reviewer复核确认
Apollyon 0、Keter 0、Euclid 0；早先关于blocked-wait marker不能证明已经parked的Euclid由上述确定性proof闭合。

最终canonical release kernel与`socket-test` app在RV64/LA64均构建通过，`just test xtask`为66/66，`just test net-host`
全部通过。最终guest wrapper在RV64通过KUnit 373/373、UDP 16/16、Unix 23/23以及glibc/musl `socketpair02`，LA64通过
KUnit 378/378及相同runtime矩阵；两架构focused glibc/musl oracle均输出
`TPASS: socket_stage3b_oracle listener ppoll pselect rdhup hup lt et oneshot`，既有epoll product regression在两架构均为
`EPOLLTEST:SUMMARY:PASS:11`。LA64完成filesystem/network/device orderly shutdown后因平台没有成功power-off handler进入
既有末尾halt并由QEMU monitor退出；运行副本使用worktree-local磁盘，未修改master image。

**Not Run / non-claim：** full socket/network LTP、final harness、physical hardware与`smp>1`未运行，Stage 4 integrated
acceptance也未运行。Stage 3 closure的transaction与current-contract cutover均为None；全部pending
Socket/Unix/IOMUX/Epoll contract继续Not Effective。本checkpoint没有产生新的current defect或accepted limitation，register
不变；Stage 4没有被解析、激活或授权。

### Stage 3 Cutover、evidence 与 write-back

Stage 3没有独立current-contract cutover。3A/3B实施、review、validation与Not Run事实默认由Git/PR保存；只有实际执行演变
成长周期probe、renegotiation或多个需要独立追踪的cutover时才创建transaction。3A/3B分别关闭且3B final source满足本节
Exit后，implementation页才标记Stage 3 Closed；`SOCKET-FRONT-001`、`SOCKET-ABI-001`、`SOCKET-WAIT-001`、全部
`UNIX-SOCKET-*`以及`IOMUX-POLL-002/003`、`EPOLL-READY-001`仍保持Pending/Not Effective，统一留待Stage 4
`SOCKET-UNIX-CUTOVER`。

如果实施只采用R1内的owner-local路线修正、module split或focused oracle，不增加修订号，也不更新register。只有实际出现
新的current defect、采用新的accepted engineering limitation，或现有inert-inode limitation的可见边界发生变化时才维护
register；target、owner、ABI、Contract Impact、acceptance或validation strength变化必须先回RFC review。Stage 3 closure
本身不解析或授权Stage 4；后续独立resolution才基于完整actual diff、finding、validation与register把Stage 4解析为下文
Ready状态。

## Stage 4 Ready / Not Active — Integrated acceptance 与 `SOCKET-UNIX-CUTOVER`

**Resolution状态：** Completed 2026-08-02；Stage 4 Ready / Not Active；不拆checkpoint。本resolution只更新accepted
target内的最终审查、证据复用、contract write-back与停止路线；未修改R1 target、Contract Impact、current contract或
register，未创建transaction，未运行build/KUnit/QEMU/LTP，也未授权Stage 4实施或cutover。

### Live candidate 与解析结论

resolution读取的final source candidate为`d027b6c9`。Stage 1--3从`f0378d0f`父RFC进入，依次形成common Socket front、
Unix socketpair、pathname namespace、listener/admission、directional stream operation和完整readiness；最终candidate没有
尚未实现的R1 production capability或仍存活的compat-only RDHUP/temporary unsupported bridge。当前register只保留R1已经
接受的pre-connection shutdown、retired-bind inert inode与继承的VFS pathname/common-create边界，没有新的current defect
要求Stage 4先扩张target。

`d027b6c9`本身已经取得Stage 3B final release build、owner-local proof和双架构guest evidence：RV64/LA64分别通过
373/373与378/378项KUnit，两个架构均通过UDP 16/16、Unix 23/23、glibc/musl `socketpair02`、focused readiness oracle与
`EPOLLTEST:SUMMARY:PASS:11`。这些事实使Stage 4的最小自然形状成为一个最终conformance/cutover unit，而不是新的feature
checkpoint。Stage 4不为了给相同candidate重新命名证据而机械重跑全部runtime；它先审计candidate、配置、artifact和每项
proof的相关性，只在证据缺失、provenance不足或candidate变化时重跑受影响类别。

本结论不预先宣称RFC acceptance已经完成。Stage 3A的临时双libc raw-syscall probe产生于较早candidate，Stage 3B又修改了
Unix role/readiness owner；该probe只能在final source relevance audit证明后复用，否则必须以final candidate上的focused
oracle替代。current-contract正文、最终exact-diff review、综合evidence index与原子cutover也仍全部未执行。

### Stage 4 Implementation Boundary

**Target：** 不增加R1能力；在一个最终candidate上把父RFC[最终closure证据](./index.md#最终-closure-证据)逐项映射到
live source、owner-local proof、architecture/libc/runtime evidence与accepted Not Run边界，完成final change review，随后
把全部pending Socket/Unix contract以及`IOMUX-POLL-002/003`、`EPOLL-READY-001`一次切换为current truth并关闭RFC。

**Non-goals：** TCP、Unix datagram/seqpacket/abstract namespace、`SO_ERROR`、pending-error/ERR readiness、成功mutable
socket option、ancillary data、timeout、async I/O和其它父RFC non-goal均不重开。physical hardware、`smp>1`、full
network/socket LTP与final harness不是R1 mandatory floor；未运行时保留Not Run，不为取得更宽宣传而临时扩大Stage 4。
现有VFS common-create issue、non-UTF-8 pathname、retired-bind inert inode和pre-connection shutdown limitation不在本Stage
修复。

**Owner / handoff / cleanup：** Stage 4不重塑production owner。general Socket继续只拥有immutable descriptor、private
envelope、共同FileOps/ABI lowering与wait orchestration；UDP Endpoint、Unix endpoint role、listener/backlog、connection
direction、VFS namespace、opened-description、iomux与epoll分别保持唯一truth。final audit必须沿socket/socketpair/accept的
fd publication与rollback、bind的VFS-to-Unix publication、connect/accept admission、stream copy/commit、route
publication/recheck和final release逐条确认handoff前后只有一个cleanup owner。任何修正先形成新的candidate，再重新判断受影响
证据；不得让closure文档替代代码或测试修复。

**Protected ABI / contract / acceptance：** R1 scoped Linux behavior、两种真实Socket consumer、六组conformance surface、
双架构/双libc floor与父RFC Contract Impact均保持。UDP的`NET-*`、opened-description、VFS creation/make-node、
`IOMUX-POLL-001`、`EPOLL-WATCH-001`与`EPOLL-FILE-001`继续作为Dependencies，不登记`Preserve`或顺手改写。contract正文
必须从final source提取最小current闭包，不能把RFC-local证明、阶段路线、实现类型或Not Run能力写成长期规则。

**允许的route correction：** 若final audit只发现R1内的source bug、缺失comment/assert、owner-local test gap、validation
asset缺口或同owner行为保持拆分，可以在Stage 4内作最小修正并重冻结candidate；修改后的相关proof、exact-diff review和
architecture/runtime evidence必须按影响重跑。若需要改变target、owner、handoff、failure/cleanup、public ABI、
Contract Impact、acceptance或validation strength，则停止并回RFC review / Target Renegotiation，不能在Stage 4自批。

### Final evidence matrix 与复用规则

父RFC acceptance是唯一完整矩阵；下表只定义如何把已有执行事实提升为final-candidate closure evidence，不建立并列target。

| Proof类别 | Final-candidate义务 | 当前可审计基线 | Stage 4 closure动作 |
| --- | --- | --- | --- |
| Source / owner / Architecture Friction | 覆盖raw ABI containment、两个consumer、namespace/readiness唯一truth、无socket-local VFS policy、无pending-error、fd/final-release/stale cleanup及iomux/epoll consumer边界 | Stage 1--3各checkpoint review与`d027b6c9` live source | 对完整actual diff执行一次综合owner/protocol audit和final exact-diff review；Apollyon/Keter必须先修复，Euclid按真实影响处置 |
| Owner-local proof | tuple resolver、fd rollback、VFS production handoff、identity/rebind、DAC lifetime、listener admission、stream prefix/peek/EOF/shutdown/SIGPIPE、per-operation predicate与late hint | final candidate的RV64 373/373、LA64 378/378项KUnit，`just test xtask` 66/66与`just test net-host`通过记录 | 建立contract-ID到真实case/source invariant的evidence index；candidate或test asset未变且provenance完整时复用，否则重跑受影响owner proof |
| Architecture build | RV64/LA64 canonical release kernel与`socket-test` app | `d027b6c9`两架构final build已通过 | 核验preset、KernelConfig、app export与artifact identity；若需要重跑，两个架构串行，不能共享generated DTB并发 |
| Guest / real consumer | 两架构覆盖socketpair、pathname server/client、DAC/umask/mode、listen/connect/accept、I/O、nonblocking、dup/fork/final close、rename/unlink/rebind、poll/select/epoll与RDHUP/HUP；UDP保持current语义 | `d027b6c9`两架构Unix 23/23、UDP 16/16与epoll 11项；`socket-test`的pathname stream case是真实guest testcase | 逐项对照父RFC而不是只看summary count；只有缺失mandatory row、artifact provenance不完整或相关source/config变化时才运行对应canonical wrapper |
| libc / ABI oracle | glibc与musl覆盖tuple/flags/errno、sockaddr/addrlen/copy-fault、R1 pre-connection shutdown以及readiness投影 | final candidate的双libc `socketpair02`与Stage 3B readiness oracle；Stage 3A raw probe来自较早candidate | 对Stage 3A之后相关hunk做relevance audit；无法证明不受影响的ABI row必须在final candidate上重跑focused双libc oracle，不把旧probe文字当作当前runtime |
| Contract / closure | 八个Introduce ID、两个IOMUX Refine与一个Epoll Refine共同生效，RFC与public navigation同步 | current contract仍保持旧规则，全部ID Pending/Not Effective | 先以final source生成最小current contract diff，再与RFC closure、evidence入口和导航在同一cutover提交中原子回写；任一ID不满足则全部保持Not Cut Over |
| Non-claim | 不把未运行范围或较窄证据扩大宣传 | hardware、`smp>1`、full network/socket LTP、final harness均Not Run | 在closure/current接受边界中保留Not Run；不以build、单架构或socketpair替代pathname/architecture proof |

证据复用以语义相关性而不是日期或Stage标签决定：必须能够识别candidate、配置、test/rootfs输入、架构、libc和结果；后续只改
RFC/current-contract文本不会使runtime失效。任何kernel、ABI、test oracle、rootfs composition、Kconfig、platform/preset或
wrapper变化都要先判断影响面；能证明无关时保留其它证据，不能证明时重跑对应类别。重跑共享generated architecture output
的build/runtime必须串行，并继续显式选择LOCAL中对应的preliminary master image，由wrapper创建worktree-local运行副本。

### Review finding 与 Architecture Friction disposition

Stage 3A留下的两个Euclid目前都是验证覆盖风险：non-socket fd配合valid/invalid `shutdown how`缺少直接runtime
lookup-priority cross-check，shutdown/read-write/final-close竞争缺少可控copy barrier runtime。现有source lock-order、operation
gate、commit recheck和相邻oracle没有显示第二truth、lost wake或错误commit，因此本resolution不把它们预先升级为Stage 4
blocker，也不为形式闭合引入production probe。

Stage 4 final review必须重新处置这两项：若final source、owner-local proof与父RFC mandatory matrix已经给出足够proof，可以
保留为closure中的residual Euclid并说明最小补强方向；若审查发现它们掩盖可达correctness failure或mandatory proof缺口，
则在当前owner内加入最小确定性case并重跑受影响证据。不得通过降低copy-fault/race oracle、静默忽略errno priority或把
test-only phase/barrier沉淀为production API来消除finding。

综合Architecture Friction Scan还必须检查：`Socket`是否出现第二family/type/readiness truth；Unix endpoint、listener、
direction与namespace是否发生owner穿透；`UnixPollRoute`等notification carrier是否反向驱动行为；KUnit-only validation入口
是否仍有真实consumer且未进入production dependency；bind/final-release/accept Drop的cleanup顺序是否显式；以及current
contract提取是否制造无真实复用的新抽象层。没有具体摩擦或只剩Safe时不写占位结论。

### Contract cutover 与 write-back

`SOCKET-UNIX-CUTOVER`是Stage 4唯一formal gate。cutover建立新的Socket current-contract owner入口，并按共同owner与共同
证明面组织general front/ABI/wait以及Unix endpoint/namespace/stream/lifecycle最小surface；不按每个ID机械建文件，也不把
UDP、VFS、opened-description、iomux或epoll Dependencies复制进Socket正文。`IOMUX-POLL-002/003`与`EPOLL-READY-001`
在各自现有contract页原地Refine，分别记录independent receive-half-close category、final-scan interest projection与
EPOLLRDHUP exact-scan delivery，保留其现有consumer policy和来源历史。

同一cutover提交必须共同完成：

- 新Socket contract入口和最小surface、既有iomux/epoll Refine、`docs/src/contracts.md`与`SUMMARY.md`导航；
- 父RFC `index.md`/本页的Closed状态、逐ID old/new/evidence索引、final candidate与Not Run边界；
- `docs/src/rfcs.md`公共状态，以及只在live evidence要求时维护的register条目；
- `git diff --check`、残余Pending/Not Effective/Ready状态搜索与`mdbook build docs`。

执行证据默认继续由Git/PR和RFC closure拥有；单一Stage、单一cutover不需要transaction。若实际执行演变成长期probe、多个
独立cutover或target renegotiation，再按真实需要创建，不得为Stage编号预建日志。

### Activation、执行顺序与停止 / 退出

本resolution不授权Stage 4。只有新的明确授权才能把整个Stage置为Active；激活后按以下顺序闭合，不再建立4A/4B：

1. 重新确认Git根、branch、dirty state、candidate identity、current contracts/register和Stage 1--3 evidence provenance；
2. 完成final source/owner/Architecture Friction audit与父RFC逐项evidence index，决定旧证据复用或focused rerun；
3. 只对mandatory gap实施R1内最小source/test/validation修正，若candidate变化则重算影响面并执行对应canonical proof；
4. 冻结最终diff，完成final change review，确认Apollyon/Keter为零并处置residual Euclid；
5. 全部mandatory proof满足后，在同一提交中执行current-contract、RFC closure、navigation与真实register/Not Run回写。

以下任一情况立即停止，Stage 4保持Active / Not Cut Over或在未激活时保持Ready：mandatory evidence无法绑定final candidate；
双架构、双libc、pathname real-consumer或UDP regression proof缺失；存在未关闭validation failure、target内correctness bug、
Apollyon/Keter、第二truth/owner穿透/无owner cleanup；contract正文无法形成唯一owner的最小闭包；或继续需要改变target、
Contract Impact、ABI、acceptance与validation strength。不得部分激活ID、先写RFC Closed再补runtime，或以accepted limitation
吸收R1 target内错误。

退出要求所有pending ID共同满足父RFC acceptance，final review和Architecture Friction disposition完成，current contracts与
RFC closure原子回写，实际限制和Not Run范围诚实记录，文档验证通过。届时Stage 4与父RFC同时Closed；Stage 4之后没有自动
进入的下一gate。
