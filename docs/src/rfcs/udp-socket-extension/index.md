# RFC-20260804-udp-socket-extension

**状态：** Accepted for Implementation / Stage 1 Checkpoint 1A Closed / Checkpoint 1B Not Active
**修订：** R0
**负责人：** doruche
**最后更新：** 2026-08-04
**领域：** network / socket / UDP / userspace ABI
**影响契约：** R0 target 将 Refine `NET-SOCKET-ENDPOINT-001`、
`NET-UDP-TRANSACTION-001`、`SOCKET-ABI-001`；current contract 未改变
**执行记录：** Git / PR；Stage 1 Checkpoint 1A已关闭，未创建transaction；Checkpoint 1B、Stage 2与contract cutover均未授权

## 文档状态

本文是 IPv4 UDP Socket 能力扩展的R0 accepted target，也是该提案唯一的canonical target
source。它固定target、owner、ABI、failure/cleanup、acceptance与validation boundary，但在
named cutover前不覆盖current contract。

本文替代此前的私有定位讨论，公共 RFC 是唯一 canonical target source。current
effective 行为仍以 `docs/src/contracts/`、live source 与 Git/PR evidence 为准；在
达到 named cutover 前，本文描述的 connected UDP、message ABI 与新 flag 都不是当前事实。

本 RFC 已创建[实施路线](./implementation.md)：路线分为两个 Stage，Stage 1 的Endpoint-owned
connected scalar/file-I/O vertical slice解析为两个有序checkpoint；本轮只关闭Checkpoint 1A，
Checkpoint 1B保持Not Active。Stage 2的single-message ABI、综合acceptance与最终cutover只保留
outline，尚未解析或激活。mandatory
resolver 的 source-level audit 已闭合：R0 选择落在既定能力包络内的 musl IPv4 resolver path；
当前 acceptance baseline 的 glibc resolver 因强依赖 `IP_RECVERR` 保持 Not Supported / Not
Cut Over。本轮没有transaction或contract cutover授权，1A closure后必须停止。

## 摘要

当前 Anemone 已有 IPv4 unconnected UDP 的 `bind`、`sendto`、`recvfrom`、file
descriptor lifecycle 与 iomux/readiness vertical slice，但普通用户程序仍无法依赖
connected UDP 和 file-style I/O 完成常见 request/response。一个直接 consumer 是
userspace DNS resolver：kernel 不应理解 DNS，而应提供普通 userspace 可以消费的
Socket ABI。

本 RFC 将能力扩展限定为一个可复用的 IPv4 UDP envelope：connected association、
reconnect/disconnect、单消息与向量数据面、file-style I/O、blocking/nonblocking 与
poll/select/epoll，以及与现有 bind、implicit bind、route/source selection、dup/fork
和 final release 一致的 owner/lifecycle 语义。批量 message syscall、通用 ancillary
data、mutable option policy 与异步 error queue 不进入 R0。

## 背景与 Current Baseline

现有 [Network UDP Socket contract](../../contracts/net/udp-socket.md) 已固定：

- IPv4 unconnected `socket(AF_INET, SOCK_DGRAM, 0)` / `IPPROTO_UDP`；
- explicit bind、implicit bind、local address query、`sendto` / `recvfrom`；
- Socket/Endpoint owner fence、单向 association、datagram transaction、short/zero/fault
  receive 语义、opened-description final release 与 readiness recheck；
- RV64/LA64 single-NIC loopback、self-external 与 remote-external 的既有 evidence
  boundary；physical hardware、`smp > 1`、full network LTP 与 final harness 仍不是当前
  closure claim。

现有 [Network Protocol Socket contract](../../contracts/net/protocol-socket.md) 已固定
cross-owner capability 必须窄且非阻塞；route/source/interface selection 由 initial-domain
control plane 唯一拥有；Endpoint lifecycle、association、queue/capacity 与 protocol
facts 由 domain Stack 拥有；Socket source 只持 opaque capability 与 snapshot。

现有 [Socket Front、ABI 与 Wait contract](../../contracts/socket/front-abi-wait.md)
已固定 immutable `SocketOps` descriptor、family-private envelope、Linux ABI containment、
共同 FileOps/wait orchestration，以及各 operation 读取 owner-defined predicate 的
`snapshot -> register -> recheck/final scan` 协议。它不提供 connected UDP 的当前成功面，
也不承诺通用 mutable option bag、error queue 或未来 family registry。

本 RFC 不是 DNS implementation。`/etc/hosts`、`/etc/resolv.conf`、NSS、cache、重试和
query policy 继续属于 userspace/rootfs；kernel 只提供普通 UDP/Socket operations。

## 目标

- 为 IPv4 UDP 增加 connected peer association、peer observation、reconnect 与
  `AF_UNSPEC` disconnect；
- 在 connected 与 unconnected 模式下提供自洽的 datagram、file-style 与单消息
  message/vector data plane；
- 保持 datagram boundary、atomic send admission、short/peek/truncate/fault consume
  与 partial completion 的可解释语义；
- 让 connect、send、receive、poll/select/epoll 继续读取各自 owner-defined predicate，
  不建立 shared readiness truth；
- 将 local binding、peer association、queued datagram、route/source selection、opened
  description 与 final release 组合在现有 owner/handoff 协议内；
- 以未修改 musl userspace resolver、非 DNS connected request/response 和 focused ABI
  consumers 证明这组能力，而不是以固定 applet 特判证明成功；glibc resolver 不属于 R0
  mandatory consumer。

## 非目标

- IPv6、dual-stack、IPv4-mapped IPv6、TCP、新 address family、network namespace 与
  通用 BSD Socket framework；
- DHCP、动态网络配置、netlink/ioctl 配置面、runtime netdev hotplug、multicast、
  broadcast 与 membership/control plane；
- `sendmmsg` / `recvmmsg` 等批量消息 syscall；
- 通用 ancillary data、credentials、fd passing、timestamp、zero-copy 与 socket splice；
- mutable socket-option bag、`SO_RCVBUF` / `SO_SNDBUF`、`SO_RCVTIMEO` / `SO_SNDTIMEO`、
  `SO_REUSE*`、`SO_BROADCAST`、`IP_PKTINFO` 与 `IP_RECVERR`；
- `SO_ERROR`、pending error state、`MSG_ERRQUEUE` 与完整 ICMP asynchronous error
  semantics；
- kernel-aware DNS、hostname API、resolver policy、DNS parser/cache；
- 为某一个 resolver、libc、BusyBox applet、测试或固定 syscall trace 建立成功特判或
  success-no-op compatibility。

## R0 用户可见 ABI

以下是本 R0 的 target envelope。它在 cutover 前不是 effective API；不支持项必须
稳定拒绝，不能以恒零、永久成功或隐藏错误模拟支持。

### 创建、地址与关联

- `socket(AF_INET, SOCK_DGRAM, 0 或 IPPROTO_UDP)`；`SOCK_NONBLOCK` 与 `SOCK_CLOEXEC`
  可以作为 type bits 一起传入；
- `bind`、`getsockname`、IPv4 `connect`、reconnect、`connect(AF_UNSPEC)` disconnect
  与 `getpeername`；
- explicit bind 和 implicit bind 继续使用同一 Endpoint namespace、port reservation
  与 conflict transaction；
- UDP `connect` 是同步 operation；`O_NONBLOCK` 不把它改造成 `EINPROGRESS` wait；
- connected `sendto` / `sendmsg` 可以以显式 destination 作为本次 operation 的地址，
  不修改 persistent peer association；无 destination 且无 peer 时返回 `EDESTADDRREQ`。

### 数据面

- 标量 message：`sendto` / `recvfrom`；libc `send` / `recv` 继续消费相同 semantic
  path；
- file-style：connected UDP 的 `read` / `write` / `readv` / `writev`，每个调用对应
  一个 datagram；unconnected file-style send 没有默认 destination 时返回
  `EDESTADDRREQ`；
- message-style：`sendmsg` / `recvmsg` 的单消息向量形式，支持 `msg_name` 与 `iovec`；
  `sendmsg` 携带非空 ancillary/control message 返回 `EOPNOTSUPP`；`recvmsg` 可以提供
  control buffer，但没有 ancillary producer 时返回空 control region；
- 一次 datagram send 必须在 user-copy、size、route/source 与 bounded admission 全部
  成功后才提交，不返回“已发送半个 datagram”；
- short receive 复制 prefix 但消费整个 datagram；zero-length datagram 合法；`MSG_PEEK`
  不消费；`MSG_TRUNC` 在 buffer 不足时报告完整 datagram length/flag；copyout fault
  的 detach、consume 与 requeue 规则见 [目标与不变量](./invariants.md)。

### Flag 与 option

- 创建 flag：`SOCK_NONBLOCK`、`SOCK_CLOEXEC`；
- send flag：`MSG_DONTWAIT`、`MSG_NOSIGNAL`；UDP 没有 SIGPIPE producer 时，
  `MSG_NOSIGNAL` 只作为带一次性诊断的可见 no-op，未来真实 producer 出现时必须移除；
- receive flag：`MSG_DONTWAIT`、`MSG_PEEK`、`MSG_TRUNC`；其它 flag 返回 `EOPNOTSUPP`；
- descriptor queries：`SO_TYPE`、`SO_DOMAIN`、`SO_PROTOCOL`、`SO_ACCEPTCONN`；
- 不支持的 option 返回 `ENOPROTOOPT`；UDP `shutdown` 返回 `EOPNOTSUPP`；不为
  `SO_ERROR` 建立 pending-error 或恒零成功路径。

single-message vector ABI 使用共享 kernel I/O `max_iovec_count` Kconfig 参数，不建立 UDP
私有上限。R0 默认与 acceptance 配置固定为 1024，与公开 `IOV_MAX` 和 Linux 6.6.32
`UIO_MAXIOV` 一致；`readv/writev/sendmsg/recvmsg` 必须读取同一参数，避免并列真相源。
低于 1024 的非 acceptance 配置是显式 reduced-capacity profile，不能作为完整 R0 ABI
cutover evidence；高于公开 `IOV_MAX` 的配置必须在 build/config resolution 时拒绝。

`sendmsg/recvmsg` 超出上限的 errno、`msghdr` field copyout 顺序、`MSG_TRUNC` 返回值与 fault
oracle 服从固定 Linux 6.6.32 reference。它们属于共同 Socket ABI adapter 的 focused oracle
与实现输入，不是新的 UDP target 决策；只有无法保持该 oracle 时才回到 RFC review。

## Owner 与协议边界

### State owner

- Socket syscall/ABI boundary 拥有 Linux tuple、sockaddr、flag、user copy、errno 与
  blocking choice；
- general Socket front 拥有 immutable descriptor、family-private opaque envelope、
  common FileOps/opened-description integration 与 family-neutral dispatch；
- UDP family owner 拥有 Linux-visible UDP operation 和 readiness/error projection；
- initial-domain control plane 唯一拥有 route、source 与 interface selection；
- Stack Endpoint 唯一拥有 local binding、peer association、queue/capacity、datagram
  storage、admission 与 protocol facts；
- Socket source、iomux、epoll 只持 capability、snapshot、non-owning route 或 consumer
  watch，不复制 peer/filter/queue/readiness truth；
- opened-description lifecycle owner 继续以最后 published reference 的 final release
  触发 Socket/Endpoint retire。

### Handoff、failure 与 cleanup

`connect` 的 target transaction 是：ABI normalize peer -> control plane 取得 operation-local
route/source selection -> Endpoint 在 owner guard 内验证 identity、implicit bind 与 peer
commit -> guard 外发布 fact invalidation。初次 connect 或 reconnect 的任何失败都保持
原 binding/peer；`AF_UNSPEC` 只清除 peer，保留 binding。

ingress admission 以 commit 时的 peer fact 过滤新 datagram。已经入队的 datagram 不因
connect/reconnect 回溯清理或重分类，仍由 Endpoint queue 交付。Endpoint detach 后，kernel
transaction 独占 payload；payload、peer、addrlen 或 message header copyout fault 不能
requeue 已 detach datagram。

final release 先撤销 Socket source publication、association 与 observer route，再以窄
non-blocking handoff 请求 Endpoint retire；dup/fork alias 的非最后 close 不推进 family
lifecycle。late invalidation、旧 identity 或旧 queue 不得命中新 association。

### Readiness

- receive predicate 是 Endpoint 已 admission 的 queued datagram predicate；
- send/write predicate 是当前 bounded capacity predicate；缺少 destination、无 route 或
  invalid address 属于立即错误，不应伪装成 wait；
- blocking、`O_NONBLOCK`、`SOCK_NONBLOCK` 与 `MSG_DONTWAIT` 共用 `EAGAIN` 分类和
  `snapshot -> register -> recheck/final scan`；
- operation-specific predicate 由对应 owner 定义，notification 只是 recheck hint，
  不能成为 callback payload、ready mask 或 errno truth。

详细 correctness invariants 与 proof obligations 见 [目标与不变量](./invariants.md)。

## Contract Impact

R0 acceptance与Checkpoint 1A closure都不更新 current contract。live source 与 current contract audit 已确认 R0 target
需要以下 delta；它们只有在实现、closure evidence 与 named cutover 完成后才成为 effective：

| Contract ID | Impact | Target delta | Effective gate |
| --- | --- | --- | --- |
| `NET-SOCKET-ENDPOINT-001` | Refine | Endpoint 增加唯一 peer association truth；reconnect/disconnect、ingress admission 与 queued datagram 不回溯规则进入 lifecycle protocol | R0 contract cutover |
| `NET-UDP-TRANSACTION-001` | Refine | 增加 connect/implicit-bind/peer atomic commit、connected destination selection 与 single-message vector datagram transaction | R0 contract cutover |
| `SOCKET-ABI-001` | Refine | 增加 connected address behavior、file/message/vector ABI、R0 flags、shared iovec bound 与 stable rejection | R0 contract cutover |

`SOCKET-FRONT-001`、`SOCKET-WAIT-001`、`NET-SOCKET-WAIT-001` 与其它 current Network、
Opened-description、IOMUX、Epoll、control-plane contracts 作为 Dependencies。当前 static
descriptor、typed operation/cursor 和 operation-specific wait/recheck 已能承载 R0；普通
capability 接线不构成 contract delta。若实现反而要求改变这些 shared rule，必须停止并回到
RFC review。未发生的语义不登记 `Preserve`。

## Implementation Boundary

本 R0 允许后续实现改变与本 target 直接对应的 Socket ABI adapter、general front capability
dispatch、UDP family/Stack Endpoint protocol surface、anemone ABI constants/wrappers、
focused tests 与同 owner 的自然模块拆分。

必须保持：IPv4-only scope、Endpoint/Stack 与 control-plane owner fence、opened-description
final release、operation-specific readiness、datagram atomicity、copy/fault/cleanup 语义、
R0 non-goals、双架构 acceptance floor 与 current contract 在 cutover 前的 unchanged 状态。

以下发现必须停止并回到 RFC review：

- 需要改变 target、non-goals、state owner、handoff、failure/cleanup、ABI、Contract Impact、
  acceptance 或 validation claim；
- chosen mandatory musl resolver path 需要本 RFC 明确排除的 option、ancillary、error queue、
  IPv6 或 TCP semantics，或要把 glibc resolver 提升为 R0 mandatory consumer；
- 实现需要在 Socket 与 Endpoint 之间复制 peer/filter/queue/readiness truth，或需要新的
  generic registry、option bag、通用 connection state machine；
- 为绕过 cross-owner failure/cleanup 而降低 datagram、copy、errno 或 evidence 诚实性；
- 需要把批量 message、IPv6、broadcast/multicast、TCP 或其它无真实 consumer 的能力带入本 RFC。

本轮只授权并关闭Stage 1 Checkpoint 1A；Checkpoint 1B、Stage 2 resolution/implementation、probe与
contract cutover均未授权。

## Acceptance 与 Validation

### R0 target acceptance

R0 acceptance 接受的是 target 与未来 closure boundary，不要求尚未实现的 syscall、guest
runtime 或 final run 已经 PASS。本轮已经闭合：

- 本文的 target/non-goals、owner/handoff/failure/cleanup、R0 ABI 与三个 Contract Impact；
- Linux 6.6.32 message ABI reference、共享 iovec Kconfig boundary 与后续 focused oracle 的
  claim 边界；具体 adapter、cursor、copy loop 和测试命令留给实施路线；
- mandatory resolver 固定为下文 source audit 已确认落在 R0 内的 musl IPv4
  `getaddrinfo` path；glibc 2.35/2.38 因 `IP_RECVERR` 依赖明确保持 Not Supported /
  Not Cut Over，不能由实现者以 option success-no-op、caller 特判或降低 error semantics
  静默改写该决定；
- implementation closure、contract cutover 与 Not Run 范围按下节分层记录，不把预期验证
  写成已有执行证据。

### Implementation closure 与 contract cutover

实现完成后，R0 closure/cutover 至少需要：

- owner-local state/transaction/readiness proof，以及 [目标与不变量](./invariants.md) 中
  的 correctness obligations；
- unconnected 与 connected request/response consumer，覆盖 implicit/explicit bind、
  reconnect/disconnect、peer filter、queued datagram 与 explicit destination override；
- `sendto/recvfrom`、`read/write`、`readv/writev`、`sendmsg/recvmsg` 的 datagram boundary、
  short/zero/peek/truncate、copy fault、blocking/nonblocking 与 partial completion；
- poll/select/epoll、dup/fork、CLOEXEC、final release、stale identity 与 late hint isolation；
- 未修改 musl resolver 的 IPv4 `getaddrinfo` path；当前 RV64 musl 1.2.0 与 LA64 musl
  1.2.5 acceptance baseline 都必须运行，执行证据还须记录实际部署 libc identity；名称
  解析成功只能证明 resolver path，不能替代 non-DNS UDP ABI coverage；
- RV64 与 LA64 guest runtime；external networking 需要单独 evidence，不以 loopback 替代。

physical hardware、`smp > 1`、full network LTP、final harness、IPv6 与其它未运行范围明确
标记 Not Run，并且不是本 R0 的 closure 前置。若后续 review 要求把 final run 提升为 mandatory
closure evidence，该变化属于 acceptance/validation claim 变更，必须先回到 RFC review。
owner-local proof 不能替代真实 guest userspace evidence。

## Resolver 决定与实施输入

### Mandatory consumer 与 exact call shape

R0 mandatory resolver 是未修改 musl `getaddrinfo` 的 IPv4 path。acceptance consumer 传入
`AF_INET`、`SOCK_DGRAM`、`IPPROTO_UDP` hints，不设置 `AI_ADDRCONFIG`，查询非 numeric
hostname；resolver 配置只含 IPv4 nameserver，成功响应包含普通、未设置 TC 的 IPv4 A
answer。hostname、answer address 与 nameserver 由 fixture 决定，但 kernel 不识别这些值。

当前两种 acceptance baseline 的 source shape 都落在既定 R0 envelope：

- musl 1.2.0 创建 `SOCK_DGRAM | SOCK_CLOEXEC | SOCK_NONBLOCK` IPv4 Socket，绑定 wildcard
  ephemeral endpoint，以 `sendto(MSG_NOSIGNAL)` 发送单个 A query，通过 `poll(POLLIN)` 等待并
  用 `recvfrom` 接收；解析结果排序另行创建 `SOCK_DGRAM | SOCK_CLOEXEC` Socket，对每个 IPv4
  answer 执行 `connect`、`getsockname` 和 `close`；
- musl 1.2.5 的同一路径把 UDP receive 改为单 iovec `recvmsg`，并在 TC / `MSG_TRUNC` 时转入
  TCP fallback。R0 只接受未截断 UDP answer；TCP fallback 继续 Not Supported / Not Cut Over，
  不能把 TC case 写成 resolver PASS。

RV64/LA64 implementation evidence 必须以实际部署 binary identity 和 guest trace 重新确认
上述 source shape；libc patch/version drift 若仍保持同一 R0 obligation 只更新实施证据，若新增
option、IPv6、TCP、ancillary 或 error-queue dependency 则停止并回到 RFC review。

glibc 2.35 与 2.38 不属于 R0 mandatory resolver。两版 UDP resolver 都在 `connect` 前无条件
调用 `setsockopt(SOL_IP, IP_RECVERR, 1)`，失败会关闭 Socket 并终止该 nameserver attempt；
这不是可忽略的探测。`IP_RECVERR`、pending error、`MSG_ERRQUEUE` 与完整 ICMP asynchronous
error semantics 仍是 R0 非目标，因此不得为 glibc 返回伪成功。未来要把 glibc resolver 提升为
mandatory consumer，必须由 follow-up target review 定义 error owner、queue/pending state、
ordinary I/O 与 poll error projection、`MSG_ERRQUEUE`/`SO_ERROR` ABI 及双架构 proof。

固定 source evidence：

- musl 1.2.0：[`res_msend.c`](https://github.com/ifduyue/musl/blob/040c1d16b468c50c04fc94edff521f1637708328/src/network/res_msend.c#L70-L176)、
  [`lookup_name.c` DNS path](https://github.com/ifduyue/musl/blob/040c1d16b468c50c04fc94edff521f1637708328/src/network/lookup_name.c#L133-L166)、
  [`lookup_name.c` result sort](https://github.com/ifduyue/musl/blob/040c1d16b468c50c04fc94edff521f1637708328/src/network/lookup_name.c#L358-L415)；
- musl 1.2.5：[`res_msend.c`](https://github.com/ifduyue/musl/blob/0784374d561435f7c787a555aeab8ede699ed298/src/network/res_msend.c#L122-L264)、
  [`lookup_name.c` DNS path](https://github.com/ifduyue/musl/blob/0784374d561435f7c787a555aeab8ede699ed298/src/network/lookup_name.c#L143-L178)、
  [`lookup_name.c` result sort](https://github.com/ifduyue/musl/blob/0784374d561435f7c787a555aeab8ede699ed298/src/network/lookup_name.c#L408-L432)；
- glibc 2.35：[`res_enable_icmp.c`](https://github.com/bminor/glibc/blob/f94f6d8a3572840d3ba42ab9ace3ea522c99c0c2/resolv/res_enable_icmp.c#L23-L37)、
  [`res_send.c`](https://github.com/bminor/glibc/blob/f94f6d8a3572840d3ba42ab9ace3ea522c99c0c2/resolv/res_send.c#L794-L859)；
- glibc 2.38：[`res_enable_icmp.c`](https://github.com/bminor/glibc/blob/36f2487f13e3540be9ee0fb51876b1da72176d3f/resolv/res_enable_icmp.c#L23-L37)、
  [`res_send.c`](https://github.com/bminor/glibc/blob/36f2487f13e3540be9ee0fb51876b1da72176d3f/resolv/res_send.c#L799-L864)。

当前没有target-level open item；R0已经接受，Stage 1 Checkpoint 1A按本轮单独授权关闭。
Checkpoint 1B、Stage 2、probe与cutover不会因1A closure或文档存在而自动激活。

### 已收束，不构成设计 blocker

- iovec 数量复用共享 `max_iovec_count` Kconfig；R0 acceptance 配置为 1024，不建立 UDP
  私有 limit。超限 errno、header copyout、fault、zero-capacity 与 partial oracle 在共同
  message-ABI 实施 slice 中按 Linux 6.6.32 固化。
- connected queue 使用 Endpoint admission-time peer truth；transition 不回溯重分类已入队
  datagram，Socket front 不在 receive 时二次过滤。
- Contract Impact 已由 live contract/source audit 收束为三个 `Refine`；front 与 wait contracts
  是 Dependencies，除非实现证据要求改变 shared rule。
- 是否建立多个 checkpoint、内部类型、helper、模块布局、同 owner 拆分和精确测试命令属于
  `implementation.md` 路线选择。只有 probe、不安全中间态、独立 cutover 或正式人工 gate
  才需要对应 gate；文档切分本身不是 target finding。

## 文档与证据

- [目标与不变量](./invariants.md)
- [实施路线](./implementation.md)（Stage 1 Checkpoint 1A Closed / Checkpoint 1B Not Active；Stage 2 Outline）
- [历史定位共识](./backgrounds/positionings.md)（仅背景材料，非 canonical target）
- Current baseline：[Network UDP Socket](../../contracts/net/udp-socket.md)、
  [Network Protocol Socket](../../contracts/net/protocol-socket.md)、
  [Socket Front、ABI 与 Wait](../../contracts/socket/front-abi-wait.md)
- 执行记录：Git / PR（Checkpoint 1A）；transaction None
- 外部源码证据：resolver audit 使用上文固定 upstream commit permalink；Linux message ABI
  oracle 使用 `xref:linux-6.6.32:<repo-relative-path>#<locator>`。私人 checkout 不作为 authority。

## 修订记录

| 修订 | 日期 | 语义变化 | Review / Evidence |
| --- | --- | --- | --- |
| R0 | 2026-08-04 | 初始accepted target：IPv4 connected UDP、file/message/vector ABI、状态owner、非目标与acceptance boundary，并选择musl mandatory resolver、保持glibc Not Supported / Not Cut Over。 | 本轮独立接受并只关闭Stage 1 Checkpoint 1A；Git / PR拥有执行证据 |

## Closure

R0尚未closure。Stage 1 Checkpoint 1A已经关闭Endpoint-owned peer、atomic bind+peer transaction、
ingress admission、default-destination resolution、peek与retire/stale isolation；SocketOps connected/
file-I/O success path、guest syscall/runtime、KUnit runtime、LTP、resolver与contract cutover均保持Not Run /
Not Cut Over。Checkpoint 1B保持Not Active，current contract与register均未改变。
