# RFC-20260809-read-only-network-diagnostics

**状态：** Closed
**修订：** R0
**负责人：** doruche
**最后更新：** 2026-08-09
**领域：** network / socket / netlink / userspace diagnostics
**影响契约：** Introduce `NETLINK-TRANSPORT-001`、`NETLINK-ROUTE-DIAG-001`、
`NETLINK-SOCK-DIAG-001`；Refine `SOCKET-ABI-001`；全部 Active
**执行记录：** implementation closed；`NETLINK-DIAGNOSTICS-CUTOVER` Effective

## 文档状态

本文是 read-only network diagnostics R0 target、实施边界与closure evidence的canonical source。已经生效的
transport、route/TCP diagnostics与Socket ABI规则见
[Read-only Netlink Diagnostics current contract](../../contracts/socket/netlink-diagnostics.md)及
[`SOCKET-ABI-001`](../../contracts/socket/front-abi-wait.md#socket-abi-001--linux-abi止于family-neutral-adapter)。

本 RFC 只有 `index.md`。当前没有多阶段实施、probe、不安全中间态、多个 cutover 或独立执行历史，
因此不创建 `invariants.md`、`implementation.md`、tracking page 或 transaction。下文的实现顺序只是
同一 closure 内的普通工作分解，不是 Stage、Checkpoint 或后续 gate 授权。

## 摘要

Anemone 已能完成常规 IPv4 网络工作，包括 ping、HTTPS clone、包管理器 update/install 与 TCP/UDP
应用，但用户态无法读取 interface、address、route 或 TCP connection facts。常用的 `ip link show`、
`ip addr show`、`ip route show` 与 `ss -tan` 因缺少 `AF_NETLINK`、rtnetlink 与 sock-diag 而不能工作。

本 RFC 引入一个 initial-domain、IPv4-only、request-time snapshot 的只读 Linux netlink 子集。
`NETLINK_ROUTE` 只投影 logical interface、boot/static IPv4 control plane 与 publication/active-attach
facts；`NETLINK_SOCK_DIAG` 只投影 Stack TCP owner 的 normalized state、tuple 与 queue facts。
诊断 adapter 不取得任何网络行为 owner，也不建立可反向驱动 route、readiness、connect、cleanup
或 packet progression 的第二真相源。

## 背景与 Current Baseline

当前 effective contracts 已经固定以下 owner topology：

- [`NET-IFACE-DOMAIN-001`](../../contracts/net/interface-domain.md) 由 `LogicalInterfaces`
  唯一拥有 initial-domain membership、ifindex、name 与 kind；当前接受边界明确没有用户可见
  interface query；
- [`NETDEV-LIFE-001`](../../contracts/net/netdev-lifecycle.md) 由 `device/net` 拥有 netdev
  identity 与 publication facts，其中 link state 允许 stale，current carrier/resource truth 仍属于
  concrete provider；
- [`NET-CONTROL-PLANE-001`](../../contracts/net/control-plane.md) 由 `Ipv4ControlPlane`
  唯一拥有 `127/8` local policy、configured external address、connected/default route 与
  source/interface selection；
- [`NET-TCP-ENDPOINT-001`、`NET-TCP-STREAM-001` 与 `NET-TCP-LIFECYCLE-001`](../../contracts/net/tcp-socket.md)
  由 Stack TCP owner 唯一拥有 binding、listener/pending child、connection state、RX/TX queue、
  orphan/TIME_WAIT 与 deferred reclaim；当前接受边界明确排除 sock-diag 与 `/proc/net/tcp`；
- [`SOCKET-FRONT-001`、`SOCKET-ABI-001` 与 `SOCKET-WAIT-001`](../../contracts/socket/front-abi-wait.md)
  已提供 static descriptor、opened-description integration、Linux ABI containment 与 owner-defined
  predicate 的 wait/recheck，但当前只发布 IPv4 UDP/ICMP raw/TCP 与 Unix stream/seqpacket tuple。

live source 也已经有 boot-stable `LogicalInterfaceSnapshot`、`NetdevSnapshot`、
`Ipv4ControlPlane` projection 与 TCP owner facts，但没有 AF_NETLINK Socket transport、Linux netlink
wire codec 或面向整个 TCP owner 的诊断 snapshot。不能通过扫描 fd table、解引用 smoltcp handle、
复制 control-plane table 或缓存 endpoint state 来临时拼出结果。

## 目标

- 发布 `AF_NETLINK + SOCK_RAW` 的 `NETLINK_ROUTE` 与 `NETLINK_SOCK_DIAG` 两个协议 tuple，
  支持 `SOCK_CLOEXEC | SOCK_NONBLOCK` creation bits；
- 让未修改的 `ip link show`、`ip addr show`、`ip route show` 与 `ss -tan` 在 acceptance guest
  中运行并验证关键内容，而不是只检查 exit status；
- 为 rtnetlink 提供 `RTM_GETLINK`、IPv4 `RTM_GETADDR` 与 IPv4 main-table `RTM_GETROUTE`
  的 request-time snapshot；
- 为 sock-diag 提供 `SOCK_DIAG_BY_FAMILY + AF_INET + IPPROTO_TCP` dump、state mask、
  normalized TCP state、local/peer tuple 与真实 `Recv-Q`/`Send-Q`；
- 保持每个 network fact 的既有 owner，只增加 owner-defined、read-only、owned snapshot seam；
- 提供足够的 Linux netlink transport 语义，包括 bind/auto-bind、getsockname、sequence、multipart、
  done/error、sendto/sendmsg、recvmsg 与 `MSG_PEEK | MSG_TRUNC`；
- 以少量高判别力、单线程 KUnit/host proof、完整 source audit、长期 direct raw-netlink oracle
  和双架构真实工具运行完成 closure。

## 非目标

- link/address/route/TCP mutation；所有已识别 mutation request 都必须失败且无副作用；
- rtnetlink multicast group、notification、`ip monitor`、userspace-to-userspace netlink、generic
  netlink、netfilter netlink、audit netlink 或其它 protocol number；
- network namespace、多个 network domain、runtime address/route reconfiguration、hotplug/detach、
  dynamic carrier subscription 或实时 carrier transition；
- IPv6 address、route、TCP 或 dual-stack capability；AF_INET6 sock-diag 的空 dump 只服务未修改
  `ss -tan` 的 query shape，不发布 IPv6；
- neighbor/ARP query、policy routing、多 table、route lookup、rule、qdisc、statistics、ethtool、
  `ip -s`、`ip -d`、`ip -j` 或完整 iproute2 command surface；
- UDP、Unix、raw 或其它 protocol diag，`/proc/net/tcp`、`TCP_INFO`、`ss -p`、`ss -e`、
  `ss -i`、process/fd ownership、security label、cgroup、BPF 或 extension attribute；
- 实时全局 transaction、跨多个独立 request 的一致性、长期 global `NetworkSnapshot`、event log
  或 history database；
- 为特定 pid、命令名、固定输出文本、测试 case 或某一条 syscall trace建立 bypass；
- full network LTP、SMP/并发压力、physical hardware、其它 NIC/platform 或完整 final harness。

## Owner 与 Snapshot 协议

### 状态与能力所有权

| Fact / capability | 唯一 Owner | Netlink 侧取得什么 | 行为边界 |
| --- | --- | --- | --- |
| logical membership、ifindex、name、kind | initial-domain `LogicalInterfaces` | owned interface snapshot | 不缓存 membership，不分配第二套 ifindex/name |
| external MAC、frame capacity、publication link fact | `device/net` publication owner | 通过 boot-stable opaque association取得 normalized snapshot | link fact允许 stale，不冒充实时 carrier，不驱动 pump |
| active attachment/publication availability | kernel attach composition | request-local active fact | 只决定 query 投影，不改变 attach/shutdown |
| IPv4 address、connected/default route 与 source/interface policy | `Ipv4ControlPlane` | owned address/route snapshot | 不从 smoltcp route 反推 policy，不执行 selection mutation |
| TCP role/state、tuple、RX/TX queue、pending child、orphan/TIME_WAIT | Stack TCP owner | normalized diagnostic records | 不暴露 handle、slot、generation、ring 或 private packet state |
| Linux netlink layout、attribute、errno 与 user copy | Socket ABI/netlink adapter | raw request与serialized reply | Linux representation止于 adapter |
| protocol、port id、真实 budget option、bounded pending reply work 与 receive predicate | netlink Socket transport | per-opened-description mutable transport state | 不拥有任何 network fact，不接收 network notification |
| fd publication、dup/fork sharing 与 final release trigger | opened-description owner | static netlink final-release hook | 非最后 alias close 不释放 port或pending reply work |

现有 owner 不迁移。owner 可以在同一 subsystem 内自然拆分 snapshot/codec/transport 模块，但 Linux
`nlmsghdr`、`ifinfomsg`、`rtattr`、`inet_diag_*` 等 UAPI struct 不得进入 Network/TCP owner，
smoltcp handle、Endpoint generation、provider queue 或 fd/task identity也不得进入 netlink reply state。

### Request-time snapshot

每个受支持的 query `nlmsghdr` 形成一次完整 snapshot：

1. adapter 先完成整条 input datagram 的 copy与datagram-level size/overflow admission，不保留user pointer；随后按
   `nlmsghdr`顺序，在每条message进入owner observation前完成该message的length/alignment/flag/attribute校验；
2. handler 在每个相关 owner 自己的 lock/guard 内复制本次 dump 所需的 normalized owned values，随后立即
   释放 guard；不持一个 owner lock进入另一个 owner，不持 guard分配用户 buffer或执行 user copy；
3. boot-lifetime logical/netdev/control-plane association可以在锁外组合，因为 R0 没有 detach、identity reuse
   或 runtime reconfiguration；不得为此创建长期综合 network cache；
4. TCP dump 必须在 Stack TCP owner 的一个 observation window 内形成 bounded owned diagnostic record set，使同一
   dump 内的 state、tuple 与 queue 相互一致；
5. Linux serialization、datagram batching 与 pending reply work publication 均在 owner guard 外完成。transport可以
   eager materialize serialized datagram，也可以保留 immutable snapshot-backed cursor并按 receive progression生成
   bounded datagram；本文不固定这两种内部路线。

同一 dump 的 record set来自一次 owner-defined snapshot。两个独立 request 可能观察不同时间点，本 RFC 不承诺
跨 request、跨 protocol 或 `ip` 多条命令之间的全局事务。已序列化 datagram或snapshot-backed reply work可以留在
netlink Socket transport中直到被消费；其中 snapshot只是immutable diagnostic output，cursor只拥有transport消费
进度，二者均不得用于 route/readiness/connect/cleanup 或任何后续网络行为，也不得重新观察live network owner。

### Failure、capacity 与 cleanup

- request size、message/attribute length、record count、snapshot/reply-work/serialized-datagram size与整数运算必须
  在admission/serialization边界检查overflow；重要reply/request byte bound由netlink transport owner的
  Kconfig/build policy拥有；
- 每条request在进入owner observation前必须保证其capacity failure可表达，并在publication前完成完整pending reply
  work的budget admission。该request要么原子发布完整data与最终`NLMSG_DONE`，要么排入有序
  `NLMSG_ERROR(-ENOBUFS)`；不得发布缺少terminal message的部分成功dump。multi-message datagram中已经完成publication的
  早期request不因后续capacity failure回滚，后续可可靠划分的message继续处理；transport可选择reservation、eager
  materialization、immutable cursor或其它直接形状，本文不固定whole-datagram preallocation、batch container或rollback
  machinery；
- allocation-free不是本target的correctness或acceptance要求。在保持唯一owner、memory safety、lock order、
  execution context、用户输入上界与cleanup边界的前提下，允许自然、适度且有界的request-local/owner-local heap
  allocation。如果消除分配会制造不自然的代码/object graph、第二份状态，或带来明显性能损失，优先选择可审查的自然
  形状；不得仅为heap OOM可恢复而引入intrusive collection、预分配mirror、duplicate index或额外reservation/rollback
  phase，也不得把分配移出其自然owner/lifecycle；
- 当前工程阶段允许适度、配置有界的kernel heap allocation在global OOM时沿用kernel-fatal/panic policy；代码设计的
  清晰度、直接性与可审查性优先于为这一类OOM构造局部恢复协议，不要求allocator failure injection、OOM recovery或向
  Socket operation传播伪造errno。用户可稳定触发的oversized input、integer overflow、
  owner/transport capacity full与malformed UAPI仍必须在对应publication/commit前形成typed rejection，不能靠allocator
  failure处理；该阶段性容许不等于允许无界增长，也不豁免nested owner lock、blocking/reclaim、持锁user copy、复杂
  callback/drop或普通failure cleanup；
- non-peek `recvmsg` 选择下一条 reply datagram后即消费它；后续 payload/name/header copy fault不 requeue。
  `MSG_PEEK` 的成功或 fault都不消费；
- final release先撤销transport publication/wait route并使新send/receive fail closed，再释放protocol-scoped
  port id、budget state与pending reply work。dup/fork aliases共享同一port/budget/reply progression，只有semantic
  final release执行一次cleanup；
- late wait hint、旧 fd number、重复 close 或 port id reuse不得恢复 retired transport或把旧 reply交给新 Socket。

## R0 用户可见 ABI

以下 envelope 已由`NETLINK-DIAGNOSTICS-CUTOVER`生效。未列出的request、flag、attribute、option或protocol
必须稳定拒绝，不能以空成功、恒零或caller特判冒充支持。

### Socket 创建、地址与 option

- `socket(AF_NETLINK, SOCK_RAW | optional SOCK_CLOEXEC | optional SOCK_NONBLOCK,
  NETLINK_ROUTE)`；
- `socket(AF_NETLINK, SOCK_RAW | optional SOCK_CLOEXEC | optional SOCK_NONBLOCK,
  NETLINK_SOCK_DIAG)`；
- `bind(sockaddr_nl { nl_family = AF_NETLINK, nl_pid = 0, nl_groups = 0 })`分配一个在该
  netlink protocol port namespace 内稳定、非零的 local port id；R0不承诺它等于 task pid；
- 未显式 bind 的 Socket 在首次 send前 auto-bind。`getsockname`返回分配后的 port id与 groups 0；
- send destination可以缺省，或显式为 kernel `sockaddr_nl { nl_pid = 0, nl_groups = 0 }`。
  nonzero peer port、multicast groups、connect与userspace delivery不支持；
- initial-domain read-only query不要求 `CAP_NET_ADMIN`或`CAP_NET_RAW`。R0因此不输出uid/inode等
  process ownership metadata，也没有跨namespace visibility policy；
- `SO_SNDBUF`/`SO_RCVBUF` 的正整数 set request作为有界 transport budget hint接受，并可被 owner-local
  min/default/max clamp；不承诺 Linux 的精确倍增或 accounting 数值；
- `NETLINK_EXT_ACK` 与 `NETLINK_GET_STRICT_CHK` 的合法布尔 set request作为无状态compatibility no-op接受。
  R0不保存对应bool、不发布getsockopt查询，不承诺extended-ack TLV，也没有宽松/严格两套parser；无论该no-op值
  如何，本文定义的精确parser保持不变。这两项兼容边界必须在实现中有关键注释与低噪声诊断；当真实
  extended-ack、dual-parser或getsockopt consumer出现时再提升为transport state，acceptance工具不再发送时可以删除；
- wrong family/addrlen、explicit nonzero port、nonzero group、unsupported protocol或option分别返回稳定的
  Linux-compatible typed error，不留下半绑定 port或reply。

### Request/reply transport

- `sendto`与`sendmsg`先把完整 request datagram复制到 kernel-owned buffer；`sendmsg` iovec使用现有共享
  kernel-I/O count/overflow边界，不保留 user pointer；send control data不支持；
- 一个datagram可以包含一个或多个`NLMSG_ALIGN`对齐的request，并在完整copy-in后按header顺序独立处理。
  已经accepted的早期request及其pending reply work不因后续request失败而rollback；具有可解析message boundary的
  invalid/unsupported request各自产生`NLMSG_ERROR`并继续后续message。短header、越界`nlmsg_len`、错误alignment或
  非零malformed tail等无法可靠形成下一message boundary的framing error只终止剩余tail，不撤销成功prefix；
- iproute2风格的oversized send buffer与末尾zero padding可接受。datagram成功copy并通过input admission后，send
  syscall报告输入已接收；per-message protocol/framing/capacity结果由上述reply或tail规则表达，
  不把已发布prefix伪装成datagram-wide rollback；
- dump reply使用 `NLM_F_MULTI`、一个或多个 data datagram与最终 `NLMSG_DONE`；exact query返回单条 data
  reply。每条 reply保留 request `nlmsg_seq`，header `nlmsg_pid`使用该 Socket local port id，receive name中的
  sender port为 kernel 0；
- invalid/unsupported request通过 `NLMSG_ERROR`携带 negative errno与足够的 original request header；
  recognized mutation即使带 `NLM_F_ACK`也只排入 error reply，send syscall本身不执行 mutation；
- `recvmsg`支持普通 blocking、opened-description nonblocking/`MSG_DONTWAIT`、`MSG_PEEK`与`MSG_TRUNC`。
  空 queue按 blocking choice等待或返回 `EAGAIN`；
- zero-length iovec配合 `MSG_PEEK | MSG_TRUNC`返回下一条 datagram完整长度且不消费。short destination复制
  prefix、设置 output `MSG_TRUNC`并按 input `MSG_TRUNC`决定返回 copied length或full datagram length；
- no ancillary producer；receive control length输出0。source `sockaddr_nl`与 `msg_flags` copyout遵守既有
  Socket message ordering，non-peek在后续 copy fault后不 requeue。

### `NETLINK_ROUTE`

#### `RTM_GETLINK`

- `NLM_F_REQUEST | NLM_F_DUMP`返回全部 committed initial-domain logical interfaces；
- 不带 dump、带有效 positive `ifi_index`的 query返回对应一条 `RTM_NEWLINK`；不存在返回
  `NLMSG_ERROR(-ENODEV)`；`IFLA_EXT_MASK`中的 `RTEXT_FILTER_VF`与`RTEXT_FILTER_SKIP_STATS`
  可以作为无额外输出的兼容 filter接受；
- 每条 reply至少包含 `ifindex`、`ifi_type`、truthful flags、`IFLA_IFNAME`、`IFLA_MTU`与可用时的
  `IFLA_ADDRESS`/`IFLA_OPERSTATE`；不伪造 statistics、qdisc、queue count或carrier counters；
- name/ifindex/kind来自 `LogicalInterfaces`。loopback的MTU来自local software link的effective IP MTU；
  external MTU来自 publication frame capacity按Ethernet header规则归一化的effective IP MTU，MAC来自
  `device/net` publication snapshot；
- `IFF_UP`只表示当前 logical member已经进入 active attachment/publication；`IFF_LOOPBACK`来自kind。
  loopback可以报告 running/lower-up；external的running/lower-up/operstate只读取 publication link snapshot，
  `Unknown`不得伪造成实时 carrier up。本 R0不承诺运行期 carrier transition；
- `ip link show`启动时发送的 `RTM_NEWLINK + NLM_F_ACK` probe必须得到
  `NLMSG_ERROR(-EPERM)`，且 interface、provider、route 与
  attachment owner state逐字段保持不变。

#### `RTM_GETADDR`

- `AF_UNSPEC`或`AF_INET` dump只返回 IPv4 `RTM_NEWADDR`；其它 family稳定拒绝或形成明确空 dump，
  但不发布 IPv6；
- 返回 `127.0.0.1/8`、scope host、loopback ifindex/label；若 deployment有 external IPv4，则返回其
  address/prefix、scope universe、external ifindex/label；
- address、prefix与interface association必须来自同一次 `Ipv4ControlPlane` snapshot；不遍历 smoltcp
  interface或 socket binding推导 address。

#### `RTM_GETROUTE`

- 只接受 IPv4 main-table dump，包括 configured external connected-prefix route，以及配置 gateway时的
  default route；没有 external deployment时返回完整空 dump；
- reply使用 `RTM_NEWROUTE`、`RT_TABLE_MAIN`、unicast type、匹配的 destination prefix、logical OIF、
  gateway与 preferred source attributes；scope/protocol必须与 connected/default route语义一致；
- 不返回 local table、broadcast、multicast、cache、neighbor、rule或policy route，不执行 destination
  route lookup。`RTA_TABLE = RT_TABLE_MAIN` filter可接受，非 main table不伪造结果。

### `NETLINK_SOCK_DIAG`

- 支持 `SOCK_DIAG_BY_FAMILY`、`NLM_F_REQUEST | NLM_F_DUMP`、`sdiag_family = AF_INET`、
  `sdiag_protocol = IPPROTO_TCP` 与 `idiag_states` filter；首版只接受 wildcard tuple/ifindex/cookie与
  `idiag_ext = 0`；
- one-owner snapshot枚举当前 TCP listener、pending/active connection、orphan与尚未 reclaim 的 TIME_WAIT
  engine，并映射为 Linux TCP state。idle/unbound/bound但未 listen/connect的 Endpoint不冒充 connection；
- 每条 `inet_diag_msg`提供真实 state、local/peer IPv4 tuple、可用时由 boot-stable mapping归一化的 logical
  ifindex，以及 owner-derived `idiag_rqueue`/`idiag_wqueue`；
- listener的 `Recv-Q`是当前 pending child count，`Send-Q`是已经 normalized 的 backlog admission limit；
  connection的 `Recv-Q`是当前未读 receive bytes，`Send-Q`是已经被 TCP owner接受但尚未完成发送/确认的 bytes。
  无 queue语义的 state才可报告0，不能把所有 queue恒零作为兼容；
- uid、inode、cookie、timer、expires、retrans与extension attributes在R0不可用：uid/inode/timer数值为0，
  cookie使用 `INET_DIAG_NOCOOKIE`，不扫描 fd table、不建立 Endpoint到opened-description/task/credential的
  反向关联，也不从 diagnostic generation伪造稳定 cookie；
- `sdiag_family = AF_INET6`且 protocol为TCP的相同 wildcard dump返回匹配 sequence的空
  `NLMSG_DONE`。这是为了完成未修改 `ss -tan` 的双family query，不表示 IPv6 Socket、address或route能力；
- UDP/Unix/raw diag、exact tuple lookup、nonzero extension request或其它 protocol稳定返回 unsupported error，
  不 fallback到 `/proc`、fd scan或其它 family owner。

## Contract Impact

本 R0 只登记真实 target delta；现有 Network/TCP owner规则作为 Dependencies，不登记 `Preserve`。
四项变化必须在完整 implementation closure 后由同一个 `NETLINK-DIAGNOSTICS-CUTOVER`原子生效：

| Contract ID | 变化 | 当前规则 | R0 target | Cutover |
| --- | --- | --- | --- | --- |
| `NETLINK-TRANSPORT-001` | Introduce | None | AF_NETLINK protocol-scoped port、bind/auto-bind、sequential multi-message processing、bounded pending reply work、multipart/sequence/error、recv consume/peek/trunc与final release由netlink transport唯一拥有 | [`NETLINK-DIAGNOSTICS-CUTOVER` Effective](../../contracts/socket/netlink-diagnostics.md#netlink-transport-001--bounded-requestreply-lifecycle由netlink-transport拥有) |
| `NETLINK-ROUTE-DIAG-001` | Introduce | None | `RTM_GETLINK/ADDR/ROUTE`只从logical/netdev/control-plane owner取得request-time snapshot；mutation无副作用 | [`NETLINK-DIAGNOSTICS-CUTOVER` Effective](../../contracts/socket/netlink-diagnostics.md#netlink-route-diag-001--route-diagnostics只投影既有owner-snapshot) |
| `NETLINK-SOCK-DIAG-001` | Introduce | None | TCP owner形成state/tuple/queue diagnostic dump；state mask、unavailable metadata与AF_INET6 empty-done边界明确 | [`NETLINK-DIAGNOSTICS-CUTOVER` Effective](../../contracts/socket/netlink-diagnostics.md#netlink-sock-diag-001--tcp-owner形成normalized-one-window-record-set) |
| `SOCKET-ABI-001` | Refine | [Active](../../contracts/socket/front-abi-wait.md#socket-abi-001--linux-abi止于family-neutral-adapter) | 增加AF_NETLINK tuple、sockaddr_nl、netlink/route/inet-diag wire validation、message copy与typed errno；Linux representation仍止于adapter | [`NETLINK-DIAGNOSTICS-CUTOVER` Effective](../../contracts/socket/front-abi-wait.md#socket-abi-001--linux-abi止于family-neutral-adapter) |

### Dependencies

- [`NET-IFACE-DOMAIN-001`](../../contracts/net/interface-domain.md)：logical membership、ifindex、name、kind
  与 boot-stable netdev association的唯一 owner；
- [`NETDEV-LIFE-001`](../../contracts/net/netdev-lifecycle.md)：normalized publication facts与允许 stale 的
  link snapshot边界；
- [`NET-CONTROL-PLANE-001`](../../contracts/net/control-plane.md)：IPv4 address、route、source/interface
  policy唯一 owner；
- [`NET-TCP-ENDPOINT-001`、`NET-TCP-STREAM-001`、`NET-TCP-LIFECYCLE-001`](../../contracts/net/tcp-socket.md)：
  TCP role/state/tuple/queue、pending child、orphan/TIME_WAIT与reclaim唯一 owner；
- [`SOCKET-FRONT-001` 与 `SOCKET-WAIT-001`](../../contracts/socket/front-abi-wait.md)：immutable static
  descriptor、common opened-description/FileOps与owner-predicate wait/recheck；
- [`OPENED-DESC-001..003`](../../contracts/task/opened-description-lifecycle.md)：fd publication、dup/fork sharing
  与 semantic final release。

如果实现需要改变这些 dependency 的 state owner、handoff、cleanup、wait或public semantic rule，而不只是增加
本文已经定义的窄 snapshot consumer，必须停止并回到 RFC review；不得把新的 shared delta藏在普通接线中。

## Implementation Boundary

### 允许改变

- Socket resolver/static descriptor与netlink family-private transport、ABI codec、message batching、pending reply
  work、真实budget option、compatibility no-op、wait与final-release wiring；
- `anemone-abi`/`anemone-rs` 中本 target实际需要的 Linux netlink/rtnetlink/inet-diag常量、layout与wrapper；
- `device/net`、initial domain、IPv4 control plane与 Stack TCP owner内的窄 owned diagnostic snapshot API，
  以及同 owner、行为保持的自然模块拆分；
- owner-local Kconfig/build bounds、inline KUnit/host proof、`socket-test` netlink module与小型真实工具 runner；
- closure时新增 current netlink diagnostics contract并更新既有 contract coverage/navigation，以及一次
  `NETLINK-DIAGNOSTICS-CUTOVER`。

实现预计按“transport/codec与pure proof -> owner snapshots与projection -> static ABI publication、真实工具验收与
原子contract cutover”推进。这只是普通实现顺序；任一中间 commit都不得被描述为 independently accepted Stage、
Checkpoint、effective ABI或partial cutover。

### 实现自由

- snapshot/reply container、具体Rust type/helper、lock/atomic组合、模块布局、allocation时机/次数、budget accounting、
  eager materialization或immutable cursor均由实现自然决定；
- reservation、preallocation、rollback或allocation-free路线只有在它们实际简化owner/lifecycle proof、满足execution
  context或带来可证明收益时才采用，不能仅因理论上的heap OOM recovery把它们提升为closure要求；
- review只以target、唯一owner、memory safety、ABI诚实性、bounded resource、publication/cleanup原子性与validation
  floor判断实现；在这些边界内，清晰、直接、性能合理的代码形状优先。

### 必须保持

- initial-domain、IPv4-only、request-time snapshot、read-only target与全部非目标；
- 每个 network/TCP fact的既有唯一 owner、boot association与opened-description final release；
- Linux ABI只存在于 Socket/netlink adapter，owner snapshot不泄漏私有表示；
- owner guard内只形成bounded owned snapshot values；允许自然、适度且有界的snapshot allocation，但Linux
  serialization、user copy、nested owner acquisition、blocking/reclaim与复杂callback/drop必须在guard外；
- diagnostic snapshot/reply永不反向驱动 network behavior；
- current contracts与公开 Socket ABI在最终 cutover前保持 unchanged；
- KUnit保持少量、高判别力、单线程，不为本 RFC 引入scheduler/concurrency harness；
- direct raw-netlink oracle是长期 ABI proof，真实 `ip`/`ss` 是integration consumer，二者不能互相替代。

### 停止条件

以下发现必须在声明 implementation closure或执行 cutover前停止并提交 RFC review / Target Renegotiation：

- target、non-goals、owner、handoff、failure/cleanup、ABI、Contract Impact、acceptance或validation claim需要改变；
- 未修改 mandatory `ip`/`ss`需要本文未列出的 mutation、multicast、netns、IPv6、neighbor/policy/stats、
  diag extension、process ownership或其它 protocol才能完成基础命令；
- truthful link output需要实时 provider carrier handoff，而 publication/active-attachment snapshot不能满足已接受边界；
- truthful sock-diag需要扫描 fd table、把 uid/inode/cookie提升为mandatory，或暴露 smoltcp/private Endpoint表示；
- implementation需要全局 `NetworkSnapshot`、长期 owner mirror、nested owner lock、持锁 user copy、reply驱动行为
  或 caller/test special case；
- bounded pending reply work无法在一次admission后保持有序multipart与terminal `NLMSG_DONE`/error语义，或
  snapshot-backed progression需要保留live owner/guard/private handle、重新观察owner或反向驱动network behavior；
- 需要 probe、不安全中间态、多个独立 cutover、两个正式 Stage或任何 execution Checkpoint。此时先决定是否
  新增 `implementation.md`，不能在本 index-only计划下自然扩张。

## Acceptance 与 Validation

### R0 target acceptance

接受本 RFC 代表接受本文的 capability envelope、owner/snapshot协议、四项 Contract Impact、单次 closure/cutover
与下述 proof floor；不表示 kernel已经实现 AF_NETLINK，也不表示任何 runtime evidence已经 PASS。

### Implementation closure 与 cutover

`NETLINK-DIAGNOSTICS-CUTOVER`必须同时满足以下三层证据。

#### KUnit / host proof

- netlink header/attribute/message alignment、length、integer overflow、sequential multi-message、成功prefix/
  later error、unparseable tail与zero-padding parser；
- sequence、local/kernel port、multipart reply-work admission/progression、`NLMSG_DONE`、`NLMSG_ERROR`、
  capacity-before-publication failure与不产生unterminated partial dump；
- bind/auto-bind/getsockname、protocol namespace、options、dup/fork/final release与stale port isolation；
- blocking/not-ready classification、peek/trunc/short copy、non-peek consume、copy fault与pending reply-work cleanup；
- `RTM_NEWLINK + ACK`及其它recognized mutation返回error且 owner snapshots前后完全一致；
- GETLINK/GETADDR/GETROUTE projection覆盖loopback、external、missing ifindex、no-external deployment、
  connected/default route与publication link Unknown/Up；
- TCP snapshot覆盖listener/pending child、active/closing/TIME_WAIT、state mask、tuple、listener/connection queue、
  unavailable metadata与AF_INET6 empty done；
- TCP纯 owner proof进入既有 network host suite；测试保持少量、高判别力和单线程，不引入调度测试。

#### Source audit

- 每个输出字段都能追溯到唯一 owner fact、ABI policy或明确 unavailable policy；没有恒零伪支持、第二真相源、
  fd scan、smoltcp handle/generation/provider私有表示泄漏；
- snapshot guard、transport guard、opened-description/final-release与wait route没有逆序、nested owner lock、持锁
  Linux serialization/user copy、blocking/reclaim或复杂callback/drop；自然snapshot allocation不因本RFC被扭曲为
  intrusive/preallocated/duplicate-state表示；
- malformed request、multi-message prefix/later failure、copy fault、reply capacity failure、mutation、
  reply-work publication、fd publication failure与final release的cleanup完整，mutation全路径无network side effect；
- `SO_SNDBUF/SO_RCVBUF` clamp与 `NETLINK_EXT_ACK/STRICT_CHK` compatibility边界有关键注释、低噪声诊断、
  可见行为与退出条件；两个compatibility no-op没有无consumer persistent state；
- production dependency不取得host-only probe/test facade，不为固定工具版本、命令或测试 marker建立分支。

#### Userspace

- 长期 direct raw-netlink oracle默认进入既有 `socket-test` 的 netlink module，覆盖 transport framing、route/TCP
  records、state mask、peek/trunc/error与mutation no-side-effect；若真实工具编排形成独立长期职责，再单独 review
  是否创建 diagnostics app，不预建 `net-control-plane-test`或平行 suite体系；
- 真实工具runner是一次性acceptance编排，不作为长期test oracle保留。runner只机械检查命令能成功exec且
  exit 0，并完整记录四条命令的原始输出与退出状态；不断言 `ip`/`ss`的显示文本、空格、列宽或字段
  顺序。closure由人工按下列语义字段审阅日志，稳定的UAPI布局、flag、record与transport语义仍由
  direct raw-netlink oracle与owner proof机械固定；
- RV64与LA64 acceptance guest各运行一次未修改 `ip link show`：人工审阅 `lo`与configured external interface的
  name/ifindex/type、truthful flags、effective MTU，以及external MAC；
- 两架构各运行一次未修改 `ip addr show`：人工审阅 `127.0.0.1/8`、external address/prefix、label与ifindex；
- 两架构各运行一次未修改 `ip route show`：人工审阅external connected route、default gateway、preferred source与OIF；
- 每个guest建立真实 TCP listener，保留至少一个completed child等待accept，并另建一对accepted connected endpoints；
  向accepted peer发送一段暂不读取的payload后运行未修改 `ss -tan`，人工审阅LISTEN与ESTAB state、local/peer tuple、
  listener pending/backlog queue以及connected receive queue。connection Send-Q的非零场景由owner proof覆盖；
  AF_INET6 query必须以empty done完成且命令整体成功；
- 真实工具的exec/exit成功只证明编排成功，不单独构成语义PASS。closure evidence必须记录工具/package或
  binary identity、原始输出及人工审阅结论、architecture/config/deployment identity与direct oracle结果；一次性
  runner完成证据采集后删除，不因本轮自然沉淀为production或长期test surface。

physical hardware、`smp > 1`、其它NIC/platform/deployment、full network LTP、压力/并发矩阵、双libc矩阵、
`ip -s/-d/-j`、其它 `ss` option与完整 final harness保持 Not Run，且不是本 R0 closure前置。

### 当前执行事实

Implementation与`NETLINK-DIAGNOSTICS-CUTOVER`已完成。最终kernel identity取得以下证据：

- `just test net-host`通过；`just fmt kernel`通过；RV64 609/609 KUnit、LA64 611/611 KUnit通过；
- 两架构长期`socket-test --netlink`均输出`NETLINKTEST:PASS`；
- RV64 acceptance使用`qemu-virt-rv64-release`、`smp=1`、`memory=1G`和final RV测试盘；LA64使用对应
  `qemu-virt-la64-release`与final LA测试盘。两者deployment均为`eth0=10.0.2.15/24`、gateway
  `10.0.2.2`；
- RV64的`/bin/ip`由competition environment安装的BusyBox 1.33.1 applet提供；LA64的`/bin/ip`
  为LoongArch ELF Build ID `a1c1d127fcc6f203857d601f028844d46b5835f7`；两架构`/glibc/ss -V`
  均报告iproute2 6.1.0；
- 一次性runner只检查exec/exit并完整写入
  `build/read-only-network-diagnostics-{rv64,la64}.log`，没有断言显示文本。人工审阅确认两架构均输出
  ifindex 1 `lo`与ifindex 2 `eth0`、effective MTU 2048/2022、external MAC
  `52:54:00:12:34:56`，以及`127.0.0.1/8`和`10.0.2.15/24`；route包含
  `10.0.2.0/24 dev eth0`与`default via 10.0.2.2`；
- runner建立listener port 32770、payload client 32771与pending client 32772。两架构`ss -tan`均显示
  LISTEN `Recv-Q=1/Send-Q=4`、accepted tuple `32770 <-> 32771`的`Recv-Q=12`、pending/active
  ESTAB tuples，并列出raw oracle留下的两条TIME_WAIT；所有tool END均为`Exited(0)`且最终
  `NETLINKTOOLS:PASS`；
- RV64 orderly poweroff且wrapper exit 0；LA64完成filesystem/network/device shutdown后按已知平台边界进入
  `no power off handler succeeded, halting the system`，外层timeout 124。独立最终review结论为
  `0 Apollyon / 0 Keter / 0 Euclid / 0 Safe`；最终source audit确认无第二truth、owner穿透、nested owner
  lock、持锁serialization/user copy、无退出条件临时桥或validation降级。

一次性`user-test` runner及接线已删除；长期raw oracle保留在`socket-test`。final测试盘中的socket LTP profile
仍为0 attempted/6 skipped，不能作为full network LTP证据。physical hardware、`smp > 1`、其它NIC/platform/
deployment、full network LTP、压力/并发矩阵、双libc矩阵、其它`ip`/`ss` option与完整final harness保持Not Run。

## 风险与反馈

- iproute2不同版本可能发送额外兼容 option、filter attribute或query。direct oracle固定长期 ABI；closure记录实际
  guest工具 identity与call shape。若 mandatory artifact越出 R0 envelope，先 review target，不按命令名特判；
- link MTU、flags与operstate容易把buffer capacity、active attachment和carrier混成一份 truth。实现必须逐字段标注
  owner与staleness，尤其不能把publication link snapshot写回provider或Stack；
- TCP `Send-Q`不能由send capacity反算，listener queue也不能从Socket readiness布尔值猜测。若owner当前缺少所需
  normalized fact，应在TCP owner内补窄snapshot，而不是在adapter缓存或扫描 fd；
- reply progression若直接依赖工具当前只有两张interface/少量Socket，会掩盖capacity、cursor与terminal-message错误。
  Kconfig bound、publication admission和host matrix必须覆盖所选production路线的真实边界，但不
  要求full-batch materialization，也不引入无界queue或live-owner cursor；
- snapshot seam可能暴露现有 owner文件职责混合。同一 owner内按 ABI/state/ops/snapshot做行为保持拆分属于结构维护；
  public owner surface、shared contract或跨owner协议变化必须按停止条件反馈。

## 文档与外部证据

- Current contracts：[`Network`](../../contracts/net/index.md)、[`Socket`](../../contracts/socket/index.md)；
- Linux netlink framing/options：
  `xref:linux-6.6.32:include/uapi/linux/netlink.h#struct-nlmsghdr`；
- rtnetlink request/reply与attributes：
  `xref:linux-6.6.32:include/uapi/linux/rtnetlink.h#RTM_GETLINK`、
  `xref:linux-6.6.32:include/uapi/linux/if_link.h#IFLA_IFNAME`、
  `xref:linux-6.6.32:include/uapi/linux/if_addr.h#struct-ifaddrmsg`；
- sock-diag/inet-diag UAPI：
  `xref:linux-6.6.32:include/uapi/linux/sock_diag.h#SOCK_DIAG_BY_FAMILY`、
  `xref:linux-6.6.32:include/uapi/linux/inet_diag.h#struct-inet_diag_req_v2`；
- Linux implementation只作为非规范性 behavior reference：
  `xref:linux-6.6.32:net/netlink/af_netlink.c`、
  `xref:linux-6.6.32:net/core/rtnetlink.c`、
  `xref:linux-6.6.32:net/ipv4/inet_diag.c`；
- commit / PR / transaction：None。

## 修订记录

- 2026-08-09：R0 accepted；授权按本文 Implementation Boundary 开始实现。澄清allocation-free不是目标要求，
  natural bounded allocation与当前kernel-fatal OOM policy优先；target、owner、ABI、Contract Impact与acceptance未变，
  contract cutover仍需完整closure证据。
- 2026-08-09：implementation、双架构acceptance、独立review与source audit闭合；执行
  `NETLINK-DIAGNOSTICS-CUTOVER`，三项新contract Active并Refine `SOCKET-ABI-001`。这是closure/证据更新，
  accepted target未变，修订仍为R0。

## Closure

Closed。`NETLINK-DIAGNOSTICS-CUTOVER`已原子生效：`NETLINK-TRANSPORT-001`、
`NETLINK-ROUTE-DIAG-001`、`NETLINK-SOCK-DIAG-001`为Active，`SOCKET-ABI-001`已Refine。
implementation、host proof、双架构KUnit/raw oracle/真实工具、人工语义审阅、独立review、source audit与临时runner
删除均已完成；没有新增register issue或accepted limitation。未运行范围保留在“当前执行事实”，不得从本closure外推。
