# IPv4 UDP Socket 能力扩展实施路线

**状态：** R0 Accepted / Stage 1 Checkpoint 1A Closed / Checkpoint 1B Not Active / Stage 2 Outline
**最后更新：** 2026-08-04
**父 RFC：** [RFC-20260804-udp-socket-extension](./index.md)
**当前修订：** R0
**当前实施阶段：** Stage 1 Checkpoint 1A已关闭并停止；Checkpoint 1B未激活；Stage 2未解析；transaction None；cutover None

本文只保存父 RFC 需要长期引用的两阶段实施路线。target、non-goals、owner、ABI、
Contract Impact、acceptance 与最终 validation boundary 仍由父 RFC [index](./index.md)和
[目标与不变量](./invariants.md)定义；本页不建立并列 target、current contract 或执行证据总表。

R0已经接受；本轮只授权并关闭Stage 1 Checkpoint 1A。Checkpoint 1B、Stage 2 resolution /
implementation、transaction与contract cutover均未授权；1A closure后已经停止。

## Live Source Baseline 与路线选择

当前 effective baseline 已经提供 IPv4 unconnected UDP、共同 Socket front、opened-description
final release、blocking retry 与 iomux/epoll recheck：

- Checkpoint 1A后，Stack UDP Endpoint唯一拥有binding、persistent peer、queue/capacity、private engine、TX phase与
  stale identity；`anemone-net-api`只暴露opaque Endpoint identity、normalized peer/peek、typed outcome与point-in-time
  facts，ingress drain在aggregate queue admission前读取owner peer；
- kernel `UdpEndpointPort`保持窄capability，control plane只产生operation-local route/source/interface selection；
  connect由Stack在一个transition中完成必要的implicit bind projection与peer commit，普通explicit send仍保持既有独立
  implicit-bind行为；
- UDP Socket family 仍以 `connect: None`、`peer_address: None`、`file_io: Unsupported` 发布；send 要求
  显式 destination，receive 不支持 peek；UDP 只接受 `MSG_DONTWAIT`，尚未接受 `MSG_NOSIGNAL`、
  `MSG_PEEK` 或 `MSG_TRUNC`；
- common Socket front 已有 typed connect/query、datagram `read/write`、direct-user scalar/vector cursor、
  datagram packet-length outcome、blocking retry 和 source-driven poll。Stage 1 应让 UDP 消费这些既有
  capability，不重建 UDP-only FileOps、wait loop 或第二个 ABI adapter；
- ordinary `readv/writev` 的 iovec import 已有共享实现，但数量上限仍是 owner-local hard-coded
  `MAX_IOVEC_CNT = 1024` 并带 Kconfig TODO。父 RFC 要求它与后续 single-message vector import 共用
  `max_iovec_count`；Stage 1 先把 ordinary vector I/O 迁到该唯一配置 owner，Stage 2 再直接复用。

这些缺口都能落在现有 Endpoint、control-plane、UDP family、common Socket ABI 与 kernel I/O owner
内，不需要 probe、owner migration、transitional public ABI 或新通用 framework。因此路线分为两个
Stage：Stage 1 交付 connected scalar/file-I/O vertical slice；Stage 2 才交付 single-message ABI、
综合 acceptance 与最终 cutover。当前不创建 transaction；普通 checkpoint、review 与验证事实由
Git/PR 保存，只有执行实际演化为长期 probe、renegotiation 或多个独立 cutover 时再重新判断。

## 全局 Implementation Boundary

### Target / non-goals

本路线只实现父 RFC 的 IPv4 UDP R0：connected association、reconnect/disconnect、scalar/file/vector/
single-message I/O、既有 wait/readiness、lifecycle 与明确拒绝面。Stage 划分只决定 accepted target
内部的施工和 review 顺序，不把任何中间 slice 降格为较弱 target、accepted limitation 或 current
contract。

IPv6、TCP、batch message、broadcast/multicast、ancillary producer、mutable option bag、`IP_RECVERR`、
`SO_ERROR`、error queue、runtime reconfiguration、generic BSD Socket framework、kernel DNS 与
caller-specific resolver path 继续是非目标。Stage 1 特别不建立 `sendmsg/recvmsg`、`msghdr`、control
message 或 output `msg_flags` surface；这些只属于尚未解析的 Stage 2。

### Owner / handoff / failure / cleanup

- Stack UDP Endpoint 唯一拥有 binding、persistent peer、ingress filter、queue/capacity、datagram
  admission、private engine 与 retire generation；Socket/front 不保存 connected bit、peer 或 filter；
- initial-domain control plane 唯一产生 operation-local route/source/interface selection。selection 可以
  参与 connect/send owner transaction，但不能成为 persistent route/peer cache；
- Socket ABI adapter 唯一处理 Linux sockaddr、flag、user pointer、copy ordering 与 errno；UDP family
  只交换 normalized address/request/outcome，Stack 和 shared net API 不解释 Linux representation；
- connect 在 user copy 与 control-plane selection 后，由 Endpoint owner 一次完成 live identity 检查、
  必要的 implicit bind 与 peer replace。commit 前失败保持原 binding/peer，disconnect 只清 peer；
- send 的显式 destination 或 Endpoint current peer 只形成 operation-local destination。bounded user-copy、
  route/source/interface、implicit bind 与 capacity admission 继续服从一个完整 datagram 的 commit；
- receive non-peek 由 Endpoint detach 完成 owner handoff；peek 只观察 queue item而不转移 ownership或
  capacity。connect transition 不回溯重分类已经 admission 的 datagram；
- semantic final release 仍先撤销 Socket source publication、association 与 observer route，再请求
  Endpoint non-blocking retire。dup/fork alias、waiter、`Drop`、raw fd 与临时 capability clone 都不拥有
  protocol lifecycle。

### Protected ABI / contract / acceptance

- `NET-SOCKET-ENDPOINT-001`、`NET-UDP-TRANSACTION-001` 与 `SOCKET-ABI-001` 只在 Stage 2 最终
  `UDP-EXT-R0-CUTOVER` 满足父 RFC closure 后 Refine；Stage 1 的任何 checkpoint 都是 Cutover None；
- `SOCKET-FRONT-001`、`SOCKET-WAIT-001`、`NET-SOCKET-WAIT-001`、`NET-CONTROL-PLANE-001`、
  `OPENED-DESC-001..003`、`IOMUX-POLL-001..003` 与 `EPOLL-*` 继续是 unchanged Dependencies；
- Stage 1 必须交付父 RFC 已固定的 connected scalar/file-I/O、flag、copy/fault、owner/lifecycle 与
  blocking/iomux 语义，但不宣称 `sendmsg/recvmsg`、musl 1.2.5 resolver 或完整 R0 closure；
- shared `max_iovec_count` 是 kernel I/O capacity owner，不是 UDP option。acceptance configuration 为
  1024 且不得高于公开 `IOV_MAX`；较低配置只能证明 reduced-capacity behavior，不能作为 R0 cutover
  evidence；
- glibc resolver 因 `IP_RECVERR` 依赖继续 Not Supported / Not Cut Over。Stage 1 不得用 option
  success-no-op、恒零 `SO_ERROR` 或 caller branch 绕过该边界。

### Validation claim

每个 checkpoint 只声明其 deliverable 的 source/owner proof、focused test、canonical build 或明确运行的
guest behavior。architecture build、KUnit runtime、guest syscall、external path、resolver 与 final harness
证据保持分层；未运行范围写 Not Run，不由 host test、另一架构或 DNS success 替代。

Stage 1 closure 只证明 connected scalar/file-I/O candidate 已形成安全中间状态，且 current contract 仍未
cut over。Stage 2 才组合 single-message ABI、双 libc、双架构、external path 与父 RFC 完整 acceptance。

### Stop conditions

除父 RFC [Implementation Boundary](./index.md#implementation-boundary)外，遇到以下任一事实必须在当前
checkpoint completion 或范围扩张前停止：

- connect 只有通过 Socket/Endpoint 两份 peer truth、先发布 binding 后补偿 peer、持 Endpoint guard 进入
  control-plane lookup/user copy/notification，或把 route selection 持久化才能实现；
- ingress filter 需要 Socket receive 二次过滤、回溯清空既有 queue、泄漏 private engine identity，或让
  wrong-peer datagram 先进入 authoritative queue；
- common front 需要 downcast UDP private state、增加 shared ready/connected truth、family-private wait loop，
  或改变 `SOCKET-FRONT-001` / `SOCKET-WAIT-001` 才能承载 Stage 1；
- iovec 配置迁移需要改变公开 `IOV_MAX`、普通 vector I/O acceptance、splice family 或其它非本 RFC consumer
  contract，而不是只替换既有 hard-coded owner；
- Stage 1 只有引入 `sendmsg/recvmsg`、ancillary/error queue、IPv6/TCP 或降低 copy/fault/lifecycle/双架构
  validation 才能形成可运行 candidate；
- mandatory consumer 的实际 libc identity/call shape越出父 RFC，或 architecture evidence只能靠
  caller/test/architecture-specific branch 通过。

保持父 RFC target、owner、ABI、Contract Impact 与 acceptance 的模块拆分、内部 API 调整、checkpoint 合并/
重排属于 Route Correction，可以先更新本页再继续。任何上述语义边界变化必须回到 RFC review / Target
Renegotiation；agent 可以提交证据和方案，不能自行批准较弱 target。

## Activation、checkpoint 与 evidence 规则

- Stage 是默认人工授权边界。父RFC R0 acceptance已经闭合，本轮维护者只授权Checkpoint 1A；1A关闭后必须停止，
  不能自动进入Checkpoint 1B，Stage 1 closure后也不能自动解析或进入Stage 2；
- Stage 内 checkpoint 是独立安全的 commit/review/recovery boundary，不默认增加一次人工 activation。若维护者只
  授权某个 checkpoint，则该更窄授权优先，关闭后必须停止；
- Checkpoint 1A、1B 按顺序执行。每个 checkpoint 关闭前完成对应 review、Architecture Friction Scan、验证与
  evidence disposition；1A 的未决 Keter/Apollyon 阻止 1B；
- 不维护逐文件 write set。预计模块只作 live baseline 提示；同 owner 新文件、模块注册、import/re-export、
  定向测试和行为保持型拆分在当前 checkpoint 内自然闭合。若文件新增职责会混合 ABI、state、ops、lifecycle
  或 compat，先做同 owner module-boundary 判断，不为旧路径制造 adapter；
- 普通执行证据由 Git/PR 保存。current contract、register 与 transaction 只在真实 contract cutover、当前缺口或
  长期执行历史出现时更新，不能预写预期结果。

## 阶段路线图

| 阶段 | 当前状态 | 概括目的 | 前置依赖 | 下一步边界 |
| --- | --- | --- | --- | --- |
| Stage 1 | 1A Closed；1B Resolved / Not Active | 交付 Endpoint-owned connected association、scalar/file/vector I/O 与既有 wait/lifecycle 的完整 candidate | 父 RFC R0 acceptance；每个更窄checkpoint明确授权 | 1A已关闭并停止；1B需单独授权，之后才可关闭Stage 1 |
| Stage 2 | Outline / Not Resolved / Not Active | 交付 `sendmsg/recvmsg` single-message ABI、musl 1.2.5、最终综合 acceptance 与 `UDP-EXT-R0-CUTOVER` | Stage 1 Closed；读取 actual diff/evidence/review 后单独 resolution 和授权 | 当前不得激活或推断 checkpoint |

## Stage 1 Resolved — Connected scalar/file-I/O vertical slice

**状态：** Checkpoint 1A Closed / Checkpoint 1B Resolved / Not Active / Cutover None

**Purpose：** 在不建立 message ABI 的前提下，让 Stack Endpoint 成为 peer/filter 唯一 owner，并让 UDP
通过现有 common Socket front 交付 connected association、scalar/file/vector datagram I/O、blocking/iomux、
opened-description lifecycle 与 musl 1.2.0 resolver 所需能力。Stage 1 关闭时形成安全、可运行、可继续扩展的
R0 candidate，但不改变 current contract，也不宣称父 RFC closure。

**Prerequisites：** 父RFC已经由owner/reviewer接受为R0；current UDP、Socket、control-plane、opened-
description、iomux/epoll contracts与register未出现改变本Stage边界的新事实；维护者本轮明确授权并关闭1A。
Checkpoint 1B仍需单独授权。

**Protected Boundary：** 保持父 RFC 的 IPv4-only scope、Endpoint/control-plane/Socket owner fence、
datagram atomicity、operation-specific readiness、final-release lifecycle、glibc rejection、Stage 2 message
ABI non-goal 与双架构最终 validation floor。Checkpoint 1A 不发布新 Linux-visible success path；Checkpoint 1B
可以交付 Stage 1 target 内的 pending connected ABI，但仍为 Not Cut Over。

### Resolved state、handoff 与 failure model

#### Peer、binding 与 connect transaction

- persistent peer 是 active Endpoint identity 下与 binding、queue 正交的 owner fact。query、connect、reconnect、
  disconnect 和 send-default resolution 都通过窄 Endpoint capability，不把 peer snapshot缓存到 `UdpSocketFile`；
- IPv4 connect 先完成完整 sockaddr copy/normalize，再由 control plane 产生 operation-local selection；Stack owner
  随后在一个 transition 中验证 selection、identity、existing binding，必要时 prepare/project/commit implicit
  binding，并原子替换 peer。任一失败保持进入 operation 前的 binding/peer；
- reconnect 到相同或不同 peer 都走同一 transaction。`AF_UNSPEC` 完整执行 Linux input copy/length admission后
  只清 peer，保留 binding、port、queue 与 capacity；already-unconnected disconnect 幂等；
- connect/disconnect 从不返回 capacity would-block，不进入 Socket wait，也不因 `O_NONBLOCK` 产生
  `EINPROGRESS`。transition 后只发布无 payload 的 invalidation hint。

内部锁、phase、request/outcome 类型和 capability 物理签名由实现选择；不得把准备步骤分别暴露为可由 caller
错序调用的 public protocol，也不得为了跨 crate 访问而公开 concrete Stack/private Endpoint。

#### Ingress admission、receive 与 peek

- ingress drain 在 authoritative queue admission 前读取 Endpoint current peer。connected 状态只接纳 matching
  sender，wrong-peer datagram 不占 aggregate queue/capacity、不产生 readable transition；
- peer absent 继续使用 current binding/demux admission。connect/reconnect/disconnect 不扫描或清空已经 queued
  datagram；queue item保留实际 sender metadata；
- non-peek receive 继续以完整 datagram detach 为线性化点；short/zero/fault 后的 consume 语义保持 current
  contract。peek 只取得 operation-local observation，不弹出 queue、不释放 RX credit；peek copy fault 也不改变
  queue；
- readable 仍只由 admitted queue nonempty 派生。filter、peek 或 transition 不建立第二个 queue count、ready bit
  或 Socket-side predicate。

#### Destination、send 与 retry

- explicit `sendto` destination 优先；缺省时由 Endpoint current peer形成一次 operation-local destination；两者
  都不存在立即返回 `EDESTADDRREQ`。显式 destination 不修改 persistent peer或receive filter；
- operation-local destination/selection 可以由现有 datagram send operation承载，但 owner guard、可重复 commit
  authority与mutable phase不得跨 sleep。capacity retry重复 family attempt并重查 live Endpoint；
- user-copy、maximum/MTU、route/source/interface、implicit bind 与 capacity admission全部在 datagram commit 前；
  zero-length datagram合法，任何错误不提交 payload；
- writable 继续只投影 current Endpoint bounded TX admission capacity。connected bit、route presence或某个
  destination 的合法性不成为持久 writable truth；no destination、invalid address、route/source failure与
  oversize都是立即结果而不是 wait reason。

#### Socket ABI、File-I/O 与 shared iovec bound

- common connect adapter 对 UDP 接受 IPv4 peer和 `AF_UNSPEC` disconnect；`getpeername` 读取 Endpoint current
  peer，peer absent 返回 `ENOTCONN`。raw sockaddr、tail fault、addrlen 与 errno policy止于 adapter；
- UDP descriptor消费现有 `SocketFileIo::Datagram`、typed send/receive、connect/query和poll capability。
  connected `read/write/readv/writev` 每次调用对应一个 datagram；unconnected write/writev返回
  `EDESTADDRREQ`，read/readv仍可消费 ordinary admitted queue；
- Stage 1 send flag为 `MSG_DONTWAIT | MSG_NOSIGNAL`，receive flag为
  `MSG_DONTWAIT | MSG_PEEK | MSG_TRUNC`。UDP `MSG_NOSIGNAL` 在没有 SIGPIPE producer 时是带一次性诊断、
  注释和退出条件的 compatibility no-op；其它 flag稳定返回 `EOPNOTSUPP`；
- `MSG_TRUNC` 的 scalar return使用common datagram packet-length outcome；Stage 1 不创建 output `msg_flags`。
  Stage 2 message ABI必须复用同一 family receive outcome，不能再定义一套 UDP truncate truth；
- kernel I/O owner把现有 hard-coded iovec count迁到 `max_iovec_count` Kconfig，ordinary
  `readv/writev`与Socket file-I/O共同消费。config generation/build约束确保非零且不高于公开 `IOV_MAX`；
  UDP family不接收或缓存该 limit，splice/message以外的无关 I/O policy不随本 checkpoint扩张。

### Checkpoint 1A — Endpoint peer、admission 与 owner capability

**状态：** Closed

**Purpose：** 在不接通 UDP `SocketOps` connected/file-I/O success path 的情况下，先关闭 persistent peer、
bind+peer connect transaction、ingress admission、default-destination resolution、peek 与 retire/stale isolation
的协议 owner。该 checkpoint 是 review/recovery boundary，不形成 current capability或 contract cutover。

**Deliverable：**

1. 在 shared semantic surface中只增加 Stack/kernel真实 consumer所需的 normalized peer transition/query、
   peek/detach 与 typed failure能力；不暴露 Stack-private object、Linux sockaddr/errno、task/fd/waiter或
   persistent route；
2. 在 Stack Endpoint owner内实现 connect/reconnect/disconnect、atomic implicit-bind+peer commit、ingress
   peer filter、queued-datagram non-retroactivity、peek observation与retire cleanup；
3. 在 kernel-private Endpoint capability中完成 control-plane selection与Stack transaction的窄handoff，并提供
   后续 UDP family会消费的 production operation；不增加 test-only production facade；
4. 补齐 owner-local/host/KUnit proof：unbound/bound connect、same/different peer、route/source/port failure、
   wrong/right peer、transition前后queue、peek/consume、concurrent query/send、retire/reuse与late invalidation；
5. 对 actual diff 执行 change review 与 Architecture Friction Scan，重点检查 parallel peer truth、prepare/commit
   错序surface、private engine泄漏、guard跨callback和无consumer抽象。Apollyon/Keter 未关闭时不得进入 1B。

**Validation：**

```sh
just test net-host
just fmt kernel --check
just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G
just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G
git diff --check
```

两条 architecture build 串行执行，避免共享 generated DTB。build只证明对应source的compile/link/export，不替代
KUnit runtime、guest syscall、external networking或resolver；这些在未实际运行时明确保持 Not Run。若新增KUnit
只被build编译而没有guest boot执行，不得写成KUnit PASS。

#### Closure Evidence

- shared `anemone-net-api`只增加normalized peer transition/query、peek snapshot与typed failure；不含Linux
  sockaddr/errno、task/fd/waiter、persistent route或Stack-private engine。kernel-private `ConnectError`继续区分
  control-plane route/source/interface failure与Stack transaction failure。
- Stack `Endpoint`唯一拥有persistent peer；unbound connect在同一Stack transition内完成source-specific ephemeral
  bind projection与peer commit，reconnect替换peer，disconnect只清peer。ingress在authoritative aggregate queue前丢弃
  wrong-peer datagram；既有queued datagram不回溯；peek复制queue-head observation但不detach或释放RX credit。
- production `UdpEndpointPort`提供connect/query/disconnect/peek与explicit-or-current-peer send resolution；默认
  destination形成operation-local snapshot，无peer返回typed destination-required，显式destination不修改persistent peer。
  UDP `SocketOps`的connected/file-I/O success path仍未接通。
- `just test net-host`通过：新增atomic connect/reconnect/disconnect/failure-preserves-old-state/stale identity与
  connected admission/queue non-retroactivity/peek/default destination/explicit override focused tests；全部既有
  frame、bounded progression、ICMP raw、multi-instance、multi-interface、UDP topology与vendored smoltcp IPv4 regression
  通过，no-default-features compile/check也通过。
- `just fmt kernel --check`与`git diff --check`通过；`qemu-virt-rv64-release`、
  `qemu-virt-la64-release`在最终source上均完成discovery/final pass与symbol table验证。RV64首次sandbox尝试在lwext4
  C compile触发`Bad system call`，相同canonical命令在sandbox外通过，因此该次失败只归类为环境限制。
- reviewer首轮发现control-plane failure被压入Stack taxonomy和default-destination可被拆成caller两步协议；前者由
  kernel-private `ConnectError`修复，后者由owner capability内的typed resolution修复。最终独立复核与Architecture
  Friction disposition见本checkpoint Git / PR evidence。
- KUnit runtime、guest syscall、external networking、resolver、LTP、physical hardware、`smp > 1`与final harness：
  **Not Run**。architecture build只证明compile/link/export，不能替代这些证据。

**Exit / Stop：** 1A只有在 production capability、owner-local proof、两架构build、review和evidence disposition
闭合后才能关闭。若 peer transition 无法在不发布Socket-side truth或不扭曲control-plane/Stack owner的情况下原子
实现，停止并回到 RFC review；不得把部分 binding/peer commit 写成 temporary limitation。1A closure为Cutover None；
若维护者授权的是整个Stage 1，可在review无阻断finding后继续1B；若只授权1A，关闭后停止。

### Checkpoint 1B — Connected Socket/File-I/O publication 与 Stage 1 closure

**状态：** Resolved / Not Active

**Purpose：** 让 UDP family消费1A capability和现有common Socket front，发布父RFC Stage 1范围内的connected
scalar/file/vector ABI、flag与wait/lifecycle，并以真实non-DNS consumer和musl 1.2.0 resolver证明vertical slice。
该 checkpoint 关闭整个Stage 1，但仍不执行任何contract cutover。

**Prerequisites：** 1A Closed且review没有遗留Apollyon/Keter；父RFC仍是同一accepted revision；实际source没有
要求改变Stage 1 target、owner、ABI、Contract Impact或validation floor。

**Deliverable：**

1. 接通 UDP `connect/getpeername`、reconnect/disconnect、connected/unconnected explicit/default destination，
   并保持 bind/getsockname、sendto/recvfrom current regression；
2. 让 UDP descriptor消费common datagram File-I/O，闭合 `read/write/readv/writev`、zero/short/fault、
   `MSG_DONTWAIT | MSG_NOSIGNAL | MSG_PEEK | MSG_TRUNC`与stable unsupported rejection；
3. 将 ordinary iovec import迁移到唯一 `max_iovec_count` Kconfig owner，补齐default、reduced-capacity、
   above-`IOV_MAX` config resolution和ordinary vector I/O regression；不触碰Stage 2 `msghdr`/control surface；
4. 复用current UDP source的snapshot/register/recheck，验证peer filter queue、capacity retry、immediate request error、
   multi-waiter/signal及poll/select/epoll；不得让connected state或route cache驱动ready mask；
5. 闭合dup/fork/CLOEXEC/one-alias/final close、connect/send/receive-close race、late hint、port reuse与orderly
   network shutdown；final release仍只由opened-description owner触发；
6. 增加非DNS connected request/response和未修改musl 1.2.0 IPv4 resolver focused consumer，记录guest实际libc
   identity/call shape。DNS success只证明该consumer，不替代connected/file/vector/fault/lifecycle matrix；
7. 完成Stage 1 final source/dependency audit、change review与Architecture Friction Scan；确认没有caller/libc/test/
   architecture branch、第二份peer/readiness truth、无退出条件bridge或为Stage 2预建的generic framework。

**Validation：**

```sh
just test xtask
just test net-host
just fmt kernel --check
just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G
just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G
./scripts/run-user-test-rv64.sh <explicit-rv64-test-image> build/udp-ext-stage1-rv64.log
./scripts/run-user-test-la64.sh <explicit-la64-test-image> build/udp-ext-stage1-la64.log
git diff --check
```

build与guest wrapper串行执行；test image必须由调用者显式选择，公共文档不固定开发者私人资源路径。guest profile
只运行Stage 1 focused consumer与必要regression，并保留wrapper的rootfs/kernel/QEMU完整链；执行证据记录实际image、
rootfs/libc identity、case list、architecture、log和结果。

Stage 1 mandatory matrix至少包括：

- owner/transaction：unbound/bound connect、reconnect、disconnect、failure-preserves-old-state、peer-filter与queued
  non-retroactivity；
- ABI/data plane：connect/getpeername、explicit/default destination、read/write/readv/writev、zero/short/peek/truncate、
  每个copy segment与address output fault、unsupported flag/option；
- wait/lifecycle：capacity saturation/recovery、immediate non-wait errors、blocking/nonblocking、poll/select/epoll、
  multi-waiter/signal、dup/fork/CLOEXEC/final release/stale identity；
- real consumer：两架构non-DNS connected request/response；RV64 acceptance baseline的未修改musl 1.2.0 IPv4 resolver。

LA64 musl 1.2.5 resolver依赖`recvmsg`，在Stage 1明确Not Run / Not Cut Over；single-message ABI、完整external-path
claim、full network LTP、final harness、physical hardware、`smp > 1`与父RFC最终contract cutover也保持Not Run / Not
Cut Over。若Stage 1实际运行了更宽证据，可以记录，但不能据此提前进入Stage 2或更新current contract。

**Exit / Stop：** 1B只有在两架构Stage 1 focused guest、owner-local proof、config/build、source audit、final review和
Architecture Friction disposition全部闭合后才能关闭Stage 1。任何必须借助`sendmsg/recvmsg`、glibc option伪成功、
shared front contract变化或validation降级的事实都触发停止。Stage 1 closure固定为Cutover None；关闭后必须停止，
等待维护者单独授权Stage 2 resolution，不能自动解析、激活或实现Stage 2。

## Stage 2 Outline — Single-message ABI、综合 acceptance 与 R0 cutover

**状态：** Outline / Not Resolved / Not Active

**Purpose：** 在Stage 1完整candidate上交付`sendmsg/recvmsg` single-message vector ABI、`msghdr`/name/control/
output flag/fault oracle与musl 1.2.5 resolver，组合父RFC双架构、external path、lifecycle/iomux和non-DNS evidence，
并在最终review通过后执行唯一`UDP-EXT-R0-CUTOVER`。

**Prerequisites：** Stage 1 Closed且Cutover None；读取Stage 1 actual diff、Git/PR evidence、review finding、current
contracts、register、实际guest libc identity与固定Linux 6.6.32 message-ABI oracle；维护者单独授权Stage 2
resolution。Stage 1 closure本身不满足该授权。

**Protected Boundary：** 必须复用Stage 1唯一Endpoint peer/queue/readiness/lifecycle与shared
`max_iovec_count`，Linux `msghdr/iovec/cmsg` representation止于common Socket ABI adapter；不得增加ancillary
producer、batch message、option/error queue、future-family registry或caller-specific resolver path。最终cutover只
Refine父RFC已固定的三个contract ID，不改变Dependencies或降低acceptance。

Stage 2是否需要内部checkpoint、如何安排ABI publication与final cutover、精确类型/模块、Linux oracle case与验证命令
均留待Stage 1 closure后的独立resolution；当前不得从本outline推断实现授权或形成finding。

## 实现反馈与 write-back

| 反馈 | 权威落点 | 当前行为 |
| --- | --- | --- |
| 保持target的checkpoint合并/重排、module split、内部API与validation route | 本页 | Route Correction后继续；必要时重新取得当前Stage授权 |
| target/non-goals、owner、handoff、failure/cleanup、ABI、Contract Impact、acceptance或validation strength变化 | `index.md` / `invariants.md` | 停止并进入RFC review / Target Renegotiation |
| 当前实现缺陷或review finding | 代码/tests；确为当前开放问题时才进register | 不预建limitation，不用reduced target隐藏bug |
| checkpoint/review/validation事实 | Git/PR；长期复杂执行时按需transaction | 不复制target或预期矩阵 |
| effective shared rule | current contracts | 只在Stage 2最终`UDP-EXT-R0-CUTOVER`原子更新 |

每个checkpoint与Stage收口前执行Architecture Friction Scan。没有具体摩擦或只剩Safe时不写占位结论；未在当前
boundary内消除的Euclid简短记录证据、模型偏差、影响和最小修正；Keter/Apollyon立即停止，不得声明checkpoint/
Stage完成或执行cutover。
