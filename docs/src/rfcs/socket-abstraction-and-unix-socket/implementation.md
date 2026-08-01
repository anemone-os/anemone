# Socket Abstraction 与 Unix Socket 实施路线

**状态：** R0 Accepted / Stage 1 Ready
**最后更新：** 2026-08-02
**父 RFC：** [RFC-20260801-socket-abstraction-and-unix-socket](./index.md)
**当前修订：** R0
**当前实施阶段：** Stage 1 Ready / Not Active（未授权代码或 contract cutover）

本文只保存父 RFC 需要长期引用的多阶段实施路线。target、non-goals、owner、ABI、Contract Impact、acceptance 与
最终 validation boundary 仍由父 RFC [index](./index.md)和[目标与不变量](./invariants.md)定义；本页不建立并列
target、执行状态总表或验证证据副本。

R0 acceptance 与 Stage 1 resolution 已于 2026-08-02 分别完成；Stage 1 的实施授权与最终
`SOCKET-UNIX-CUTOVER` 仍是独立动作。resolution 只把 Stage 1 解析为 Ready / Not Active，不授权代码。进入后续
Stage 前仍必须根据 live source、前一 Stage 实际 diff、review finding 与验证证据解析当前 Stage；前一 Stage关闭后
停止，不自动进入下一 Stage。

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
| Entry | Completed | 完成 Draft review、R0 acceptance 与 Stage 1 实施解析 | 当前 RFC、current contracts、register、live owner | 已完成；未激活 Stage 1 |
| Stage 1 | Ready / Not Active | 建立由 UDP 与 Unix `socketpair` 同时消费的 Socket front vertical slice | Entry resolution 已完成；Stage 1 尚待独立实施授权 | 只能由明确 Stage 1 或 Checkpoint 1A 实施授权激活 |
| Stage 2 | Outline | 闭合 pathname namespace、listener、connection admission 与 address lifecycle | Stage 1 Closed | 读取 Stage 1 实际 front、Unix endpoint/stream owner 与 validation evidence |
| Stage 3 | Outline | 闭合完整stream operation、shutdown与poll/select/epoll readiness | Stage 2 Closed | 读取 Stage 2 实际 listener/connection/direction predicate 与 race evidence |
| Stage 4 | Outline | 完成综合 conformance、回归、文档和原子 contract cutover | Stage 1-3 Closed | 读取完整实际 diff、全部 finding、validation 与 register 状态 |

Stage 1 已按下文解析到可执行粒度；Stage 2--4 仍只固定 Purpose、Prerequisites 与 Protected Boundary 所需的高层路线。
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
按下文解析。Entry 当前为 Completed，Stage 1 为 Ready / Not Active。实施授权不是 Entry 退出条件；只有后续明确
授权才能使 Stage 1 或 Checkpoint 1A Active。

## Stage 1 Ready / Not Active — 双 consumer Socket front vertical slice

**目的：** 在同一 Stage 中建立最小 general Socket front，并让现有 UDP 与 Unix `socketpair` connected-stream
vertical slice 都通过该 front 的 creation、ABI/FileOps、opened-description 与 family dispatch 路径运行。

**前置：** Entry resolution 已完成；开始代码前仍需另行取得 Stage 1 或 Checkpoint 1A 实施授权。若授权只点名一个
checkpoint，其 closure 不激活下一个 checkpoint；若用户明确授权整个 Stage 1，仍按 1A -> review -> 1B 的顺序执行。

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

### Checkpoint 1A — Common front 与 UDP migration

**Deliverable：** 引入最小general Socket front、static ops/type witness、common Socket inode/FileOps与description hooks；
把`socket/bind/getsockname/sendto/recvfrom`和poll/final release改为只识别common front并经UDP ops分发。现有
`UdpSocketFile`职责作为UDP-private state保留或按同owner语义命名/拆分；ABI adapter、common FileOps与final-release
hook不得再按UDP concrete FileOps/private type分类。旧UDP-only dispatch在1A内删除，不保留双路径。

**Focused proof：** owner-local proof覆盖resolver witness、unsupported capability、single creation abort、common
final-release到UDP retire的exactly-once handoff；现有UDP host topology与real `udp-test`回归继续证明bind/send/receive、
blocking/readiness、dup/fork/CLOEXEC与retire/reuse。测试不冻结case数量，只要求每个高风险handoff有能够证伪回归的
oracle；不得为了满足数量复制相同路径。

**Exit / stop：** source audit必须证明syscall/FileOps/opened-description只有common front identity，只有UDP ops解释
UDP private state，Network Stack及`anemone-net-api`没有Unix/future-TCP改动，current UDP runtime无退化。若迁移需要
common层保存第二份family/type/readiness、让UDP ops接收rawLinux ABI，或扩大Network contract，立即停止。1A通过后
只标记checkpoint closed并review完整diff；不自动激活1B，不做contract cutover。

### Checkpoint 1B — Unix socketpair production vertical slice

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
回RFC review。1B通过后Stage 1 Closed；Cutover仍为None，并停止在未解析、未授权的Stage 2之前。

## Stage 2 — Pathname namespace 与 connection admission

**目的：** 在 Stage 1 的共同 front 与 Unix endpoint/stream owner 上，闭合 filesystem pathname identity 到 live
binding、listener backlog、connect/accept admission 与 Linux-visible address snapshot 生命周期。

**前置：** Stage 1 Closed；Stage 2 已根据 Stage 1 实际 diff、owner model 与验证证据独立解析和授权。

**受保护边界：** VFS 继续唯一拥有pathname resolution、DAC、umask/final formation、inode/dentry与link/rename/unlink；
Unix registry只索引stable inode identity，不按pathname或permission裁决；opened-description publication/final release
边界不变；本 Stage不改变iomux/epoll consumer policy或提前cut over pending contract。

**预期交付：**

- 激活Unix single-socket creation并建立unbound/bound/listening/connected role owner；single creation失败与final release
  继续复用Stage 1的common creation/lifecycle边界；
- pathname bind 复用 `VFS-CREATION-001` production handoff，并以 stable inode identity 提交 live binding 与
  immutable local-name snapshot；
- listen、connect、accept 建立唯一 backlog/admission/connection commit，并闭合accepted child的copyout/fd
  publication rollback或fail-forward；
- getsockname/getpeername、accepted/peer address、hard-link/rename/unlink/rebind与stale generation保持namespace、
  binding和address truth分离；
- blocking connect/accept只通过对应operation predicate与共同wait/recheck协议重试，不缓存DAC授权、binding
  capability或private phase。

**验证类别：** namespace/listener/connection/source audit；bind publication、permission handoff、identity/generation、
listener admission、accept publication 与 late capability owner-local proof；RV64/LA64 focused pathname server/client、
DAC/umask、rename/link/unlink/rebind、blocking/nonblocking connect/accept runtime；相关 Linux sockaddr、listener race、
accept copyout oracle。精确 matrix 在 Stage 2 resolution 中确定。

**Cutover：** None。即使pathname vertical slice可运行，也不得提前写入current Socket/Unix contract。

**停止 / 退出：** 若自然实现必须扩张VFS common-create payload/callback/rollback、让inode保存runtime Socket payload、
长期跨VFS持Unix global lock或用path compensation删除node，立即停止并回父RFC边界。若 live evidence 触发父RFC允许的
failed-bind inert-inode退路，Stage 2必须记录具体failure point、errno、residue与cleanup evidence，并按真实当前行为
回写register。退出要求namespace/listener/connection owner与cleanup闭合，focused pathname runtime通过；关闭后停止，
不自动进入Stage 3。

## Stage 3 — Stream operation 与完整 readiness closure

**目的：** 在Stage 1 basic socketpair stream/wait与Stage 2 namespace/connection admission基线上，以实际endpoint/
listener/connection/directional truth为唯一来源，闭合父RFC的完整stream operation、shutdown/terminal语义和
poll/select/epoll readiness。

**前置：** Stage 2 Closed；Stage 3 已根据实际predicate、wait race、copy progress与lifecycle evidence独立解析和授权。

**受保护边界：** connect、accept、send、receive各自保留owner-defined predicate与commit；共同层只拥有blocking
choice和wait/recheck；notification不是truth；iomux/epoll继续拥有consumer policy、final/exact scan与delivery commit；
Unix首版不得产生pending error、ERR readiness或成功`SO_ERROR`。

**预期交付：**

- 复核Stage 1 read/write/vector的ordered-prefix边界，并让目标send/receive、`MSG_PEEK`、zero-length、copy fault、
  signal、EOF、shutdown、peer close与SIGPIPE在同一family operation boundary下完整闭合；
- connect/accept/send/receive的blocking loop统一使用attempt、snapshot/register、recheck/final-scan协议，同时保留各自
  predicate、transaction与typed outcome；
- source-neutral receive-half-close与完整HUP成为独立事实，poll/select/epoll按current consumer protocol投影
  readable/writable/RDHUP/HUP，不建立ready mask cache或socket-only wait queue；
- 父RFC目标中的address/query/flags/socket option拒绝矩阵与共同FileOps语义补齐，Stage 1/2的明确unsupported不再
  覆盖本target要求成功的operation。

**验证类别：** per-operation owner/predicate/commit/cancel audit；stream partial/peek/EOF/shutdown/SIGPIPE、lost-wake/
late-hint、poll/select/epoll LT/ET/ONESHOT与copyout rollback proof；RV64/LA64、glibc/musl focused ABI/runtime；六组
scoped Linux conformance surface的对应路径证据；UDP blocking/readiness regression。若体量需要checkpoint，Stage 3
resolution可以按stream operation与readiness的依赖顺序拆分，但二者仍属于同一Stage且不得形成第二truth或独立cutover。

**Cutover：** None。`IOMUX-POLL-002/003`与`EPOLL-READY-001`Target Refine仍待Stage 4原子cutover。

**停止 / 退出：** attempt/wait拆分若导致重复commit、丢失partial progress、跨sleep泄漏private phase或无法由最终
predicate裁决，必须停止并回RFC review；不能让family ops隐藏wait loop或让共同层解析private state补洞。若Linux
oracle要求首版pending-error/`SO_ERROR`或其它被排除能力，同样停止并走Target Renegotiation。退出要求完整target
operation与readiness surface已实现、定向race/ABI/runtime通过、无未退出临时bridge；关闭后停止，不自动进入Stage 4。

## Stage 4 — Integrated acceptance 与 `SOCKET-UNIX-CUTOVER`

**目的：** 不再扩张能力；对Stage 1-3的完整实际实现执行最终owner/source/conformance/regression审计，完成父RFC
acceptance、文档回写与单一原子contract cutover。

**前置：** Stage 1-3 Closed；所有实际diff、review finding、validation evidence、Not Run边界与register状态可审计；
Stage 4已独立授权。

**受保护边界：** 父RFC target、scoped Linux behavior、两个consumer proof、完整validation floor与Contract Impact
不得在closure阶段静默收窄。Stage 4默认不修复无关相邻问题，也不以文档声明替代缺失runtime。

**预期交付：**

- 按父 RFC [最终 closure 证据](./index.md#最终-closure-证据)完成source/owner audit、owner-local proof、双架构/
  双libc runtime、pathname实际用途、iomux/epoll与UDP regression；
- 扫描并清除第二truth、owner穿透、private representation泄漏、caller/test特判、无退出桥、隐含cleanup顺序和无真实
  consumer抽象；只在有具体证据时按Architecture Friction规则回写；
- 按实际采用路线维护register；未执行的hardware、`smp>1`、full network/socket LTP与final harness等证据明确记为
  Not Run，且不冒充父RFC mandatory acceptance；
- 原子更新父RFC列出的Socket/Unix contract ID以及`IOMUX-POLL-002/003`、`EPOLL-READY-001`，回写RFC closure、
  public navigation与唯一执行证据入口。

**验证类别：** 以父RFC acceptance为唯一完整矩阵；Stage 1-3 evidence可以复用，但必须针对最终实际diff复核其仍然
有效。共享generated architecture output的build/runtime串行执行。

**Cutover：** `SOCKET-UNIX-CUTOVER`。所有pending ID共同达到acceptance后一次生效；任一mandatory owner、ABI、
lifecycle、architecture或consumer proof缺失时保持Not Cut Over，不做部分current-contract宣传。

**停止 / 退出：** 未关闭的validation failure、target内correctness bug、Keter/Apollyon、UDP regression、双consumer
proof缺失，或需要改变target/Contract Impact/acceptance时立即停止，不声明RFC Closed。全部证据满足、current contracts
与RFC closure原子回写、实际限制和Not Run范围诚实记录后，Stage 4与父RFC才可关闭。
