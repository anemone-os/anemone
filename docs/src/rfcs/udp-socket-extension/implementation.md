# IPv4 UDP Socket 能力扩展实施路线

**状态：** R1 Closed / Stage 1 Closed / Stage 2 Closed / `UDP-EXT-R1-CUTOVER` Effective
**最后更新：** 2026-08-04
**父 RFC：** [RFC-20260804-udp-socket-extension](./index.md)
**当前修订：** R1
**当前实施阶段：** Stage 1/2与全部checkpoint已关闭；transaction None；`UDP-EXT-R1-CUTOVER` Effective

本文只保存父 RFC 需要长期引用的两阶段实施路线。target、non-goals、owner、ABI、
Contract Impact、acceptance 与最终 validation boundary 仍由父 RFC [index](./index.md)和
[目标与不变量](./invariants.md)定义；本页不建立并列 target、current contract 或执行证据总表。

R1已经关闭；Stage 1 Checkpoint 1A/1B与Stage 2 Checkpoint 2A/2B均已关闭。transaction None；
`UDP-EXT-R1-CUTOVER`已经生效，没有后续自动gate。

## Live Source Baseline 与路线选择

当前 effective baseline 已经提供 IPv4 unconnected UDP、共同 Socket front、opened-description
final release、blocking retry 与 iomux/epoll recheck：

- Checkpoint 1A后，Stack UDP Endpoint唯一拥有binding、persistent peer、queue/capacity、private engine、TX phase与
  stale identity；`anemone-net-api`只暴露opaque Endpoint identity、normalized peer/peek、typed outcome与point-in-time
  facts，ingress drain在aggregate queue admission前读取owner peer；
- kernel `UdpEndpointPort`保持窄capability，control plane只产生operation-local route/source/interface selection；
  connect由Stack在一个transition中完成必要的implicit bind projection与peer commit，普通explicit send仍保持既有独立
  implicit-bind行为；
- UDP Socket family 已发布 `connect/getpeername`、connected/default destination 与 datagram
  File-I/O；`MSG_NOSIGNAL`、`MSG_PEEK` 与 `MSG_TRUNC` 均在 Stage 1 范围内实现，unsupported flags
  仍稳定拒绝；
- common Socket front 已有 typed connect/query、datagram `read/write`、direct-user scalar/vector cursor、
  datagram packet-length outcome、blocking retry 和 source-driven poll。Checkpoint 2A message adapter继续
  让UDP消费这些既有capability，没有重建UDP-only FileOps、wait loop或第二个family ABI adapter；
- ordinary `readv/writev` 与 Socket file-I/O 已迁到共享 `max_iovec_count` Kconfig owner；xtask
  只负责 deserialize/materialize/generate，合法区间由 kernel `static_assert!` 对公开 `IOV_MAX`
  约束。Stage 2 的 single-message vector import 继续复用该唯一配置。
- asm-generic RV64/LA64的`sendmsg(211)`与`recvmsg(212)`、64-bit `MsgHdr` representation和common
  `message/{mod,sendmsg,recvmsg}.rs` adapter已经由Checkpoint 2A交付；它复用live `SocketSendRequest`、
  `SocketReceiveRequest`、datagram operation snapshot与receive outcome，没有新增family op或UDP-private
  message path；
- app `Command` driver已经能用两架构当前Linux-musl C/C++ toolchain与其标准库/sysroot生成静态
  guest ELF。Stage 2选择repository-owned C consumer作为mandatory ABI oracle；C++不重复承担同一
  Socket ABI proof。未修改musl resolver以当前工具链版本条件性尝试，不再冻结1.2.0/1.2.5。

这些缺口都能落在现有 Endpoint、control-plane、UDP family、common Socket ABI 与 kernel I/O owner
内，不需要 probe、owner migration、transitional public ABI 或新通用 framework。因此路线分为两个
Stage：Stage 1 交付 connected scalar/file-I/O vertical slice；Stage 2 才交付 single-message ABI、
综合 acceptance 与最终 cutover。当前不创建 transaction；普通 checkpoint、review 与验证事实由
Git/PR 保存，只有执行实际演化为长期 probe、renegotiation 或多个独立 cutover 时再重新判断。

## 全局 Implementation Boundary

### Target / non-goals

本路线只实现父 RFC 的 IPv4 UDP R1：connected association、reconnect/disconnect、scalar/file/vector/
single-message I/O、既有 wait/readiness、lifecycle 与明确拒绝面。Stage 划分只决定 accepted target
内部的施工和 review 顺序，不把任何中间 slice 降格为较弱 target、accepted limitation 或 current
contract。

IPv6、TCP、batch message、broadcast/multicast、ancillary producer、mutable option bag、`IP_RECVERR`、
`SO_ERROR`、error queue、runtime reconfiguration、generic BSD Socket framework、kernel DNS 与
caller-specific resolver path 继续是非目标。Checkpoint 2A建立UDP `sendmsg/recvmsg`、`MsgHdr`与output
`msg_flags` surface；Checkpoint 2B已经完成C/resolver/external acceptance与contract cutover。ancillary producer
继续是R1非目标。

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
  `UDP-EXT-R1-CUTOVER` 满足父 RFC closure 后 Refine；Stage 1 与Checkpoint 2A都是Cutover None；
- `SOCKET-FRONT-001`、`SOCKET-WAIT-001`、`NET-SOCKET-WAIT-001`、`NET-CONTROL-PLANE-001`、
  `OPENED-DESC-001..003`、`IOMUX-POLL-001..003` 与 `EPOLL-*` 继续是 unchanged Dependencies；
- Stage 1 已交付父 RFC 固定的connected scalar/file-I/O、flag、copy/fault、owner/lifecycle与
  blocking/iomux语义，但不宣称`sendmsg/recvmsg`或完整R1 closure；
- shared `max_iovec_count` 是 kernel I/O capacity owner，不是 UDP option。acceptance configuration 为
  1024 且不得高于公开 `IOV_MAX`；较低配置只能证明 reduced-capacity behavior，不能作为 R1 cutover
  evidence；
- glibc resolver 因 `IP_RECVERR` 依赖继续 Not Supported / Not Cut Over。Stage 1 不得用 option
  success-no-op、恒零 `SO_ERROR` 或 caller branch 绕过该边界。

### Validation claim

每个 checkpoint 只声明其 deliverable 的 source/owner proof、focused test、canonical build 或明确运行的
guest behavior。architecture build、KUnit runtime、guest syscall、external path、resolver 与 final harness
证据保持分层；未运行范围写 Not Run，不由 host test、另一架构或 DNS success 替代。

Stage 1 closure只证明connected scalar/file-I/O candidate已形成安全中间状态，且current contract仍未
cut over。Stage 2组合single-message ABI、当前musl-linked mandatory C consumer、双架构、external path、
条件性resolver attempt与父RFC完整acceptance；glibc不进入mandatory matrix。

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
- mandatory C consumer只能靠caller/test/architecture-specific kernel branch通过；current musl resolver
  越出R1 envelope时应条件性分类，只有试图为其扩大target或伪造success才触发停止。

保持父 RFC target、owner、ABI、Contract Impact 与 acceptance 的模块拆分、内部 API 调整、checkpoint 合并/
重排属于 Route Correction，可以先更新本页再继续。任何上述语义边界变化必须回到 RFC review / Target
Renegotiation；agent 可以提交证据和方案，不能自行批准较弱 target。

## Activation、checkpoint 与 evidence 规则

- Stage 是默认人工授权边界。父RFC R1 acceptance、Stage 2 resolution与Checkpoint 2A/2B都已按独立授权闭合；
  历史上2A closure不构成2B implementation授权；
- Stage 内 checkpoint 是独立安全的 commit/review/recovery boundary，不默认增加一次人工 activation。若维护者只
  授权某个 checkpoint，则该更窄授权优先，关闭后必须停止；
- Checkpoint 1A、1B已按顺序关闭；Checkpoint 2A、2B也必须按顺序独立授权和执行。每个checkpoint
  关闭前完成对应review、Architecture Friction Scan、验证与evidence disposition；前一checkpoint的
  未决Keter/Apollyon阻止后一checkpoint；
- 不维护逐文件 write set。预计模块只作 live baseline 提示；同 owner 新文件、模块注册、import/re-export、
  定向测试和行为保持型拆分在当前 checkpoint 内自然闭合。若文件新增职责会混合 ABI、state、ops、lifecycle
  或 compat，先做同 owner module-boundary 判断，不为旧路径制造 adapter；
- 普通执行证据由 Git/PR 保存。current contract、register 与 transaction 只在真实 contract cutover、当前缺口或
  长期执行历史出现时更新，不能预写预期结果。

## 阶段路线图

| 阶段 | 当前状态 | 概括目的 | 前置依赖 | 下一步边界 |
| --- | --- | --- | --- | --- |
| Stage 1 | Closed；Cutover None | 交付 Endpoint-owned connected association、scalar/file/vector I/O 与既有 wait/lifecycle 的完整 candidate | 父 RFC R0 acceptance；1A与1B均已完成 | 已关闭；Stage 2 resolution已独立完成 |
| Stage 2 | Closed；2A/2B Closed；`UDP-EXT-R1-CUTOVER` Effective | 交付`sendmsg/recvmsg` single-message ABI、current-musl C consumer、条件性resolver attempt、最终综合acceptance与`UDP-EXT-R1-CUTOVER` | Stage 1 Closed；R1 acceptance revision与resolution完成 | 已关闭；没有后续自动gate |

## Stage 1 Resolved — Connected scalar/file-I/O vertical slice

**状态：** Checkpoint 1A Closed / Checkpoint 1B Closed / Cutover None

**Purpose：** 在不建立 message ABI 的前提下，让 Stack Endpoint 成为 peer/filter 唯一 owner，并让 UDP
通过现有 common Socket front 交付 connected association、scalar/file/vector datagram I/O、blocking/iomux 与
opened-description lifecycle。Stage 1 关闭时形成安全、可运行、可继续扩展的 R0 candidate，但不改变 current
contract，也不宣称父 RFC closure；musl resolver专项按用户决定保持 Not Run。

**Prerequisites：** 父RFC已经由owner/reviewer接受为R0；current UDP、Socket、control-plane、opened-
description、iomux/epoll contracts与register未出现改变本Stage边界的新事实；维护者本轮明确授权并关闭1A。
Checkpoint 1B已在本轮边界内完成。

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
1A关闭时按当次窄授权停止；后续1B只在维护者明确授权后执行。

### Checkpoint 1B — Connected Socket/File-I/O publication 与 Stage 1 closure

**状态：** Closed / Cutover None

**Purpose：** 让 UDP family消费1A capability和现有common Socket front，发布父RFC Stage 1范围内的connected
scalar/file/vector ABI、flag与wait/lifecycle，并以真实non-DNS consumer证明vertical slice；resolver专项保持Not Run。
该 checkpoint 关闭整个Stage 1，但仍不执行任何contract cutover。

**Prerequisites：** 1A Closed且review没有遗留Apollyon/Keter；父RFC仍是同一accepted revision；实际source没有
要求改变Stage 1 target、owner、ABI、Contract Impact或validation floor。

**Deliverable：**

1. 接通 UDP `connect/getpeername`、reconnect/disconnect、connected/unconnected explicit/default destination，
   并保持 bind/getsockname、sendto/recvfrom current regression；
2. 让 UDP descriptor消费common datagram File-I/O，闭合 `read/write/readv/writev`、zero/short/fault、
   `MSG_DONTWAIT | MSG_NOSIGNAL | MSG_PEEK | MSG_TRUNC`与stable unsupported rejection；
3. 将 ordinary iovec import迁移到唯一 `max_iovec_count` Kconfig owner，补齐default、reduced-capacity与
   ordinary vector I/O regression；合法区间由 kernel `static_assert!` 约束，xtask不做参数语义校验；不触碰
   Stage 2 `msghdr`/control surface；
4. 复用current UDP source的snapshot/register/recheck，验证peer filter queue、capacity retry、immediate request error、
   multi-waiter/signal及poll/select/epoll；不得让connected state或route cache驱动ready mask；
5. 闭合dup/fork/CLOEXEC/one-alias/final close、connect/send/receive-close race、late hint、port reuse与orderly
   network shutdown；final release仍只由opened-description owner触发；
6. 增加非DNS connected request/response；musl 1.2.0/1.2.5 resolver专项按用户决定保持 Not Run，不能用普通
   socket LTP或非DNS case替代 resolver 证据。DNS success只证明该consumer，不替代connected/file/vector/fault/lifecycle matrix；
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
- real consumer：两架构non-DNS connected request/response；RV64/LA64 musl resolver专项按用户决定 Not Run。

RV64 musl 1.2.0与LA64 musl 1.2.5 resolver专项均为Not Run / Not Cut Over；glibc resolver保持Not Supported /
Not Cut Over；single-message ABI、完整external-path claim、full network LTP、final harness、physical hardware、
`smp > 1`与父RFC最终contract cutover也保持Not Run / Not Cut Over。若Stage 1实际运行了更宽证据，可以记录，
但不能据此提前进入Stage 2或更新current contract。

#### Closure Evidence

- RV64 canonical wrapper：413 KUnit；UDP extension 10/10，UDP 16/16，Unix 23/23，raw ICMP 10/10；glibc/musl
  socket LTP 6/6；正常 orderly poweroff。证据：`build/udp-ext-stage1-rv64.log`。
- LA64 canonical wrapper：418 KUnit；同上 guest suites 与 glibc/musl socket LTP 6/6；wrapper 正常收口，已知
  无 poweroff handler 导致 terminal halt。证据：`build/udp-ext-stage1-la64.log`。
- `just test xtask`：83 passed；`just test net-host`通过；`just fmt kernel --check`、`just fmt socket-test --check`
  与`git diff --check`通过。xtask测试只覆盖参数 materialize/generate；合法区间由 kernel static assertion 负责。
- 独立 reviewer 的两项 Apollyon 已闭合：blocking retry 使用 operation-local destination snapshot；unconnected
  missing destination 在 ensure_bound/user-copy 前返回 `EDESTADDRREQ`，不产生隐式 bind 或 `EFAULT`先行。
- RV64 musl 1.2.0、LA64 musl 1.2.5 resolver专项：**Not Run**（按用户决定）；glibc：**Not Supported / Not Cut Over**。
  external networking、physical hardware、`smp > 1`、full network LTP、final harness与contract cutover保持
  Not Run / Not Cut Over。

**Exit / Stop：** 1B已在两架构 focused guest、owner-local proof、config/build、source audit、final review和
Architecture Friction disposition闭合后关闭Stage 1。任何必须借助`sendmsg/recvmsg`、glibc option伪成功、shared
front contract变化或validation降级的事实仍触发停止。Stage 1 closure固定为Cutover None；当时已按授权停止。
本次Stage 2 resolution虽已独立完成，仍不能自动激活或实现Checkpoint 2A。

## Stage 2 Closed — Single-message ABI、current-musl acceptance与R1 cutover

**状态：** Closed；Checkpoint 2A/2B Closed；`UDP-EXT-R1-CUTOVER` Effective

**Purpose：** 在Stage 1完整candidate上交付`sendmsg/recvmsg` single-message vector ABI、`msghdr`/
name/control/output flag/fault oracle与当前musl工具链的真实C consumer，组合父RFC双架构、external path、
lifecycle/iomux和non-DNS evidence，条件性尝试未修改musl resolver，并在最终review通过后执行唯一
`UDP-EXT-R1-CUTOVER`。

**Prerequisites：** Stage 1 Closed且当时Cutover None；resolution已读取Stage 1 `8bdcbcee..8b6610cb`
actual diff与closure evidence、current contracts/register、live Socket/kernel I/O/app owner、固定Linux 6.6.32
message-ABI oracle及R1 acceptance revision。Checkpoint 2A/2B已按顺序分别取得授权并关闭；2A closure本身
当时不构成2B implementation授权。

### Resolution结论与Stage 2 Implementation Boundary

#### Target / non-goals

Stage 2只交付R1已有的asm-generic `sendmsg(211)`/`recvmsg(212)`、single-message vector behavior、
mandatory C consumer、双架构/external acceptance与三个既定contract Refine。Stage 1 connected、peer、
queue、datagram transaction、wait与lifecycle语义保持原样；Stage 2不重新打开Endpoint或control-plane设计。

不增加`sendmmsg/recvmmsg`、ancillary producer、`MSG_ERRQUEUE`、`SO_ERROR`、`IP_RECVERR`、IPv6、TCP、
broadcast/multicast、dynamic option/cmsg registry、32-bit compat或generic BSD Socket framework。C++标准库
已经是可用环境能力，但不是重复同一C Socket ABI matrix的mandatory consumer。

#### Owner / handoff / failure / cleanup

- Socket ABI adapter拥有raw `msghdr` snapshot、syscall flags、`msg_name`/`msg_namelen`、iovec import、
  total length、control rejection、Linux errno及output field ordering。RV64/LA64共享64-bit asm-generic layout，
  layout type与size/alignment/offset assertion止于`anemone-abi`/syscall boundary；family/front不持有raw header；
- message iovec import读取shared `max_iovec_count`。message ABI按Linux 6.6.32对超过上限返回`EMSGSIZE`，
  不得为复用helper而改变ordinary `readv/writev`现有超限errno；descriptor array、base/length与total overflow
  必须在任意datagram commit前完整验证；
- `sendmsg`把non-null、nonzero `msg_name`归一化为operation-local explicit destination，否则沿用Stage 1
  Endpoint current peer。全部vector payload在现有`SocketSendPayload`/`SocketDatagramSendOperation`内完成
  bounded copy和一次datagram commit；capacity retry可保留immutable operation snapshot，但不得保留user pointer、
  owner guard或可重复commit authority；
- `recvmsg`复用现有`SocketReceiveRequest`与`SocketReceiveOutcome`。sink在Endpoint detach/peek transaction中
  scatter payload并捕获peer/outcome；family不写raw `msghdr`。adapter随后按Linux oracle依次投影name、
  `msg_flags`与`msg_controllen`，后续output fault不requeue已经detach的datagram；`MSG_PEEK`始终不detach；
- short receive无论调用者是否传入`MSG_TRUNC`都在output `msg_flags`设置`MSG_TRUNC`；syscall flags包含
  `MSG_TRUNC`时返回完整datagram length，否则返回copied length。zero-capacity、zero-length datagram与
  later-segment fault继续服从同一个detach/peek owner；
- `sendmsg`的nonzero `msg_controllen`稳定返回`EOPNOTSUPP`且不提交payload；`recvmsg`没有ancillary producer，
  不读取control buffer并把output `msg_controllen`写为0。`msghdr.msg_flags`不作为send input flag，recv只写
  R1支持的output bits；unsupported syscall flags仍在共同adapter稳定拒绝；
- fd lookup、socket type、header/iovec/name/control validation与output fault precedence以固定
  `xref:linux-6.6.32:net/socket.c#__copy_msghdr`、`#copy_msghdr_from_user`、`#____sys_recvmsg`及
  `xref:linux-6.6.32:net/ipv4/udp.c#udp_recvmsg`为oracle。R1明确选择的unsupported control结果仍为
  `EOPNOTSUPP`；focused oracle必须固定它与header/iovec/name fault的优先级，不能由实现偶然顺序决定；
- syscall return后所有header snapshot、vector cursor、payload、peer capture与operation snapshot销毁。
  final release、wait cancellation、Endpoint retire和network shutdown继续由Stage 1 owner处理，message adapter
  不增加persistent cleanup或observer。

内部type、helper、cursor与目录名不冻结。live `fs/socket/api/abi.rs`已同时包含sockaddr、scalar payload、
flag和error projection；加入完整message layout/copy protocol前必须做same-owner module-boundary判断。允许将ABI
职责按address/message/outcome等稳定角色目录化，但拆分必须保持private visibility、调用路径与visible behavior，
不能引入第二层dispatch或public framework。

#### Protected ABI / contract / acceptance

- Checkpoint 2A可以发布pending `sendmsg/recvmsg` success path，但Cutover None；current contract继续只承诺
  unconnected UDP baseline。只有2B满足全部mandatory evidence并执行`UDP-EXT-R1-CUTOVER`时，三个Refine才
  同时Effective；
- `SocketOps`现有typed send/receive capability足够。2A不得增加message-specific family op、让common front
  downcast UDP private state，或修改`anemone-net-api`/Stack peer、queue、transaction与readiness truth；
- R1只为IPv4 UDP发布single-message success surface；Unix stream与ICMP raw不能仅因复用现有typed
  send/receive而意外获得`sendmsg/recvmsg`。common adapter在family boundary稳定返回`EOPNOTSUPP`，不为
  未授权family建立第二consumer或外推通用message contract；
- mandatory userspace proof是repository-owned C consumer，由两架构当前可用Linux-musl compiler/sysroot
  静态链接标准header与libc wrapper。执行证据记录compiler path/version、sysroot/libc/binary identity；两个
  架构或未来运行之间不要求版本相同；
- 未修改musl IPv4 `getaddrinfo`在两架构都必须attempt。observed call shape在R1 envelope内却失败时阻塞2B；
  只有明确证据表明current libc需要R1非目标能力时，才可把该架构resolver记为Not Supported / Not Cut Over
  而不阻塞总体cutover。普通timeout、DNS fixture错误、未知errno或未调查失败不能冒充compatibility skip；
- external request/reply是R1 mandatory evidence，必须同时观察guest与host peer结果；loopback、resolver或既有
  net-udp历史证据不能替代本candidate external path。physical hardware、`smp>1`、full network LTP、final
  harness、IPv6与TCP保持Not Run / Not Cut Over。

### Checkpoint 2A Closed — Common message ABI与UDP publication

**状态：** Closed / Cutover None

**Purpose：** 在不引入C/resolver/external acceptance资产和不更新current contract的前提下，建立common
single-message ABI，令UDP通过Stage 1 typed send/receive path交付双架构可运行的`sendmsg/recvmsg` pending
candidate。该checkpoint先关闭layout、copy/fault与family-boundary风险，为2B保留安全停止点。

**Deliverable：**

1. 增加RV64/LA64 asm-generic syscall常量、64-bit `msghdr` UAPI layout/assertion与两个syscall handler；raw ABI只
   存在于common Socket adapter，`anemone-rs`只增加focused consumer真实需要的窄wrapper/constant；
2. 实现header snapshot、shared-bound iovec import、explicit/default destination、vector payload/sink、control
   rejection、name/flags/control-length output与Linux errno/fault ordering；复用现有retry、operation snapshot、
   receive outcome与Endpoint owner，不增加UDP-private message path；
3. owner-local/KUnit与focused guest覆盖invalid fd/non-socket/header fault、negative/oversized name length、
   iovlen 0/1/1024/1025、descriptor/base/total overflow fault、zero/short/exact/oversize datagram、connected
   default与explicit override、unconnected `EDESTADDRREQ`、control rejection、unsupported flags及Unix/raw
   family的stable `EOPNOTSUPP`；
4. receive focused oracle覆盖empty/nonempty name capacity、short name与actual length、zero control output、
   `MSG_PEEK`、input/output `MSG_TRUNC`组合，以及payload -> name -> flags -> controllen每个fault point的
   visible prefix、return和consume/non-consume结果；
5. 在final 2A diff上回归Stage 1 UDP extension、current UDP/Unix/ICMP raw Socket consumers、ordinary
   `readv/writev` iovec boundary与iomux/lifecycle；执行change review与Architecture Friction Scan，确认没有
   raw header/fd/task泄漏、第二份vector limit、family-specific ABI parser或无consumer framework。

**Validation：**

```sh
just test xtask
just test net-host
just fmt kernel --check
just fmt socket-test --check
just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G
just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G
./scripts/run-user-test-rv64.sh <explicit-rv64-test-image> build/udp-ext-stage2a-rv64.log
./scripts/run-user-test-la64.sh <explicit-la64-test-image> build/udp-ext-stage2a-la64.log
git diff --check
```

build与guest wrapper串行执行。两架构focused guest必须直接调用`sendmsg/recvmsg`并输出独立message-ABI
summary；普通Socket LTP中偶然覆盖这些syscall不替代focused oracle。host/source proof、architecture build、guest
runtime和external claim分层记录；2A不运行C consumer、resolver或host external peer时，三者保持Not Run。

#### Closure Evidence

- RV64/LA64共享asm-generic syscall号211/212与64-bit `MsgHdr` layout。common `message/` adapter按用户要求保持
  `mod.rs`共享header/iovec/name逻辑、`sendmsg.rs`与`recvmsg.rs`各自承载一个syscall；UDP只消费Stage 1 typed
  send/receive、retry与Endpoint outcome，没有增加family message op、Stack/net-api或shared wait surface。
- message importer复用`MAX_IOVEC_COUNT`唯一配置owner，并保持ordinary `readv/writev`既有strict total policy；
  single-message路径按Linux 6.6.32先检查原始multi-iovec range、再把累计长度裁剪到`MAX_RW_COUNT`，single iovec
  则先裁剪再检查range。importer与结果只在`crate::fs`可见，没有扩大kernel-wide public surface。
- focused `UDPMSGTST` 7/7覆盖layout、fd/family/header admission、iovlen 0/1/1024/1025、range/clipping、
  explicit/default destination、control/flag rejection、oversize无提交、scatter/truncate/peek/zero，以及payload ->
  name -> flags -> controllen fault顺序和peek/non-peek consume边界。独立review发现的累计裁剪、缺失matrix与
  visibility三项finding均在final candidate中关闭。
- RV64 canonical wrapper：415 KUnit；`UDPMSGTST` 7/7、UDP extension 10/10、UDP 16/16、Unix 23/23、
  raw ICMP 10/10、glibc/musl socket LTP 6/6；orderly PowerOff。证据：`build/udp-ext-stage2a-rv64.log`。
- LA64 canonical wrapper：420 KUnit；同一focused/regression/LTP summaries全部通过；orderly shutdown完成后因
  已知无poweroff handler进入terminal halt。证据：`build/udp-ext-stage2a-la64.log`。
- `just test xtask`为83 passed，`just test net-host`通过；两架构`socket-test` app build、两架构release kernel
  build、两个format check与`git diff --check`通过。RV64首次sandbox build在`lwext4` C compile触发SIGSYS，
  identical canonical command在sandbox外通过，故只归类为环境限制。
- repository-owned C consumer、未修改musl resolver、host external peer、physical hardware、`smp > 1`、full
  network LTP、final harness、IPv6、TCP与`UDP-EXT-R1-CUTOVER`：**Not Run / Not Cut Over**，全部由2B或
  RFC非目标边界继续拥有。transaction保持None，current contract与register不变。

**Exit / Stop：** 只有layout、ABI/fault matrix、两架构guest、shared-bound regression、review与Architecture
Friction disposition全部闭合时2A才可Closed。若实现需要新family op、Stack/net-api change、第二份iovec truth、
control success-no-op、copy fault requeue或改变shared front/wait contract，立即停止并回到RFC review。2A关闭后
current contract仍不变，并按checkpoint边界停止；不得自动激活2B。

### Checkpoint 2B Closed — Current-musl consumer、external acceptance与`UDP-EXT-R1-CUTOVER`

**状态：** Closed / `UDP-EXT-R1-CUTOVER` Effective

**Purpose：** 在2A final candidate上用当前实际工具链证明普通C/libc consumer、两架构external路径与完整R1
回归，条件性尝试未修改musl resolver，完成最终review并原子执行三个contract Refine和RFC closure。

**Prerequisites：** 2A Closed且没有未解决Apollyon/Keter；R1仍是accepted revision；current contracts/register
没有改变本checkpoint target、owner、ABI或acceptance的新事实；维护者单独授权2B。

**Deliverable：**

1. 增加repository-owned C Command app，使用标准`sys/socket.h`/`sys/uio.h`/`netdb.h`与libc
   `sendmsg/recvmsg/getaddrinfo` wrapper；它由当前RV64/LA64 Linux-musl toolchain静态链接并作为普通
   guest-local test app保留，不进入kernel production dependency；
2. mandatory C matrix覆盖connected/unconnected single-message request/reply、explicit/default destination、
   scatter/gather、zero/short/peek/truncate、control/flag rejection、fault/output ordering与non-DNS lifecycle；
   与2A Rust/raw oracle重叠只用于证明真实C layout/wrapper，不复制一套kernel behavior分支；
3. 在Stage 2B临时验证期间让同一真实consumer执行双架构remote-external token/reply。窄host peer及其启动、
   READY、timeout、child cleanup和日志只由临时focused acceptance orchestration拥有；通用`run-user-test`
   wrapper不恢复无条件host-peer依赖。该orchestration与focused rootfs/marker必须在closure前删除，不成为长期
   test/build/QEMU owner；C app本身的guest-local matrix继续保留；
4. 在两架构各自当前toolchain上构建并attempt未修改musl IPv4 `getaddrinfo`。fixture只提供IPv4 nameserver与
   普通non-TC A answer；resolver结果独立输出PASS、COMPAT-NOT-SUPPORTED或FAIL分类。compat分类必须附实际
   binary/libc identity和越界syscall/feature证据；kernel不识别hostname、answer、libc或caller；
5. 在2B final source回归2A message ABI、Stage 1 connected/file/vector、unconnected UDP、Unix、ICMP raw、
   poll/select/epoll、dup/fork/CLOEXEC/final release/stale identity与orderly network shutdown；两架构guest、
   external peer与mandatory C summary必须全部通过，resolver按R1条件性规则处置；
6. 对`8bdcbcee..2B final`完整actual diff做source/dependency audit、change review、exact-diff review与
   Architecture Friction Scan。Apollyon/Keter先修复；Euclid按真实影响消除或报告，不能通过降低oracle、
   external evidence或ABI诚实性换取closure；
7. 所有mandatory evidence通过后，在同一closure中Refine current `NET-SOCKET-ENDPOINT-001`、
   `NET-UDP-TRANSACTION-001`与`SOCKET-ABI-001`，更新RFC/index导航状态并执行`UDP-EXT-R1-CUTOVER`。
   register只在出现真实current issue/accepted limitation时更新；resolver compatibility位于R1 target之外，
   不自动建立limitation。

**Validation：**

```sh
just test xtask
just test net-host
just fmt kernel --check
just fmt socket-test --check
just app build --arch riscv64 <udp-extension-c-consumer>
just app build --arch loongarch64 <udp-extension-c-consumer>
just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G
just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G
<focused-stage2-rv64-orchestration> <explicit-rv64-test-image> build/udp-ext-stage2b-rv64.log
<focused-stage2-la64-orchestration> <explicit-la64-test-image> build/udp-ext-stage2b-la64.log
git diff --check
mdbook build docs
```

尖括号是能力级占位而非文件名冻结：实现时由同owner app manifest与临时focused orchestration自然命名，并在2B
activation preflight写出实际命令。临时orchestration必须复用canonical rootfs/kernel/QEMU路径、显式test image和
通用wrapper语义，只额外拥有focused host-peer lifecycle；不能建立第二build/QEMU owner，且closure前必须删除。
C consumer artifact、
compiler/sysroot/libc identity、guest/peer summary与resolver classification分别写入两架构日志或Git/PR evidence。

#### Closure Evidence

- repository-owned `udp-extension-c`作为普通guest-local Command app保留，使用标准C Socket/netdb wrapper；local
  matrix在RV64/LA64均为7/7，覆盖connected/unconnected request/reply、explicit/default destination、control与
  send/receive flag、empty control output、peek/truncate/zero、payload与header/name fault ordering、lifecycle及
  未修改musl `getaddrinfo` fixture。resolver在两架构均PASS。
- 2B runtime candidate在RV64为415 KUnit，LA64为420 KUnit；两架构均通过UDP 16/16、UDP extension 10/10、
  message 7/7、Unix 23/23、raw 10/10与curated Socket LTP 6/6。临时remote-external guest/host token/reply在两架构
  均PASS；RV64 orderly poweroff，LA64 orderly shutdown后进入已知无poweroff handler的terminal halt。证据：
  `build/udp-ext-stage2b-{rv64,la64}.log`及对应peer/build日志。
- runtime candidate source identity为`fa408f03bb17dc26f38bf28fdb77a363cac8d4d1e80c98142445b8974b7a10fe`；
  RV64/LA64 ELF分别为`b3cd3bee0f36baf0bf3d33102a1a4c4c13ebf4a76d00825455f7591ab2015eb9`和
  `e6aea20a5660de274921a1367b71dca5a969e6837956afa5b1bd04094457f6a3`。binary audit确认两架构musl ELF调用
  `getaddrinfo/sendmsg/recvmsg/sendto/recvfrom/poll/connect/getsockname`，resolver路径包含预期
  `socket/bind/sendto/poll/recvmsg/connect`形状。
- 按Route Correction，focused host peer、runner、rootfs、marker及C app external-only mode在closure前删除；通用
  `run-user-test` wrapper保持不变。删除不改变kernel ABI或guest-local matrix；删除后的两架构app build通过。
- `just test xtask`为83 passed，`just test net-host`、kernel/socket-test format与`git diff --check`通过；两架构
  `user-test` build及其format check也通过。final exact-diff review发现resolver completion pipe的SIGPIPE/child-reap
  Keter与partial socket cleanup Euclid，均以app-local失败路径修复；按维护者停止运行的要求，该最终失败路径修复未再
  build或runtime验证。临时第二build/QEMU owner已随focused orchestration删除。transaction None，register不变。
- `UDP-EXT-R1-CUTOVER`已原子Refine `NET-SOCKET-ENDPOINT-001`、`NET-UDP-TRANSACTION-001`与
  `SOCKET-ABI-001`并关闭R1。glibc resolver保持Not Supported / Not Cut Over；physical hardware、`smp > 1`、
  full network LTP与final harness保持Not Run。

**Exit / Stop：** mandatory C、2A regression、RV64/LA64 guest与两架构external peer均通过，resolver在R1 call
shape内通过。2B已关闭，三个current contract已原子Refine，RFC R1与Stage 2均Closed；没有自动follow-up gate。

## 实现反馈与 write-back

| 反馈 | 权威落点 | 当前行为 |
| --- | --- | --- |
| 保持target的checkpoint合并/重排、module split、内部API与validation route | 本页 | Route Correction后继续；必要时重新取得当前Stage授权 |
| target/non-goals、owner、handoff、failure/cleanup、ABI、Contract Impact、acceptance或validation strength变化 | `index.md` / `invariants.md` | 停止并进入RFC review / Target Renegotiation |
| 当前实现缺陷或review finding | 代码/tests；确为当前开放问题时才进register | 不预建limitation，不用reduced target隐藏bug |
| checkpoint/review/validation事实 | Git/PR；长期复杂执行时按需transaction | 不复制target或预期矩阵 |
| effective shared rule | current contracts | 只在Stage 2最终`UDP-EXT-R1-CUTOVER`原子更新 |

每个checkpoint与Stage收口前执行Architecture Friction Scan。没有具体摩擦或只剩Safe时不写占位结论；未在当前
boundary内消除的Euclid简短记录证据、模型偏差、影响和最小修正；Keter/Apollyon立即停止，不得声明checkpoint/
Stage完成或执行cutover。
