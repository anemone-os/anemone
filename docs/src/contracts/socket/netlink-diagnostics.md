# Read-only Netlink Diagnostics 当前契约

**Contract IDs：** `NETLINK-TRANSPORT-001`、`NETLINK-ROUTE-DIAG-001`、
`NETLINK-SOCK-DIAG-001`
**状态：** Active
**Owner：** netlink Socket transport唯一拥有protocol-scoped port、budget、pending reply work与
receive progression；logical interface、netdev publication、IPv4 control plane与Stack TCP owner继续唯一拥有
被投影的network facts
**参与领域：** Socket ABI / opened description / logical interface / device net / IPv4 control plane /
Stack TCP / iomux
**覆盖范围：** initial-domain、IPv4-only的`NETLINK_ROUTE`与`NETLINK_SOCK_DIAG`只读
request-time diagnostics，以及bounded multipart transport
**不覆盖：** mutation、multicast/notification、userspace delivery、network namespace、runtime reconfiguration、
IPv6 capability、其它netlink protocol、UDP/Unix/raw diag、process ownership或完整iproute2 surface
**实现位置：** `anemone-kernel/src/fs/socket/netlink/`、
`anemone-kernel/src/net/diagnostics.rs`、
`anemone-kernel/crates/anemone-smoltcp-stack/src/tcp/diagnostics.rs`
**依赖：** `SOCKET-FRONT-001`、`SOCKET-ABI-001`、`SOCKET-WAIT-001`、
`OPENED-DESC-001..003`、`NET-IFACE-DOMAIN-001`、`NETDEV-LIFE-001`、
`NET-CONTROL-PLANE-001`、`NET-TCP-ENDPOINT-001`、`NET-TCP-STREAM-001`、
`NET-TCP-LIFECYCLE-001`
**Pending Successor：** None
**最后核验：** 2026-08-14；`TCP-LISTENER-INGRESS-CUTOVER` Effective

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 非 owner持有什么 | 行为用途 |
| --- | --- | --- | --- |
| protocol port、send/receive budget、pending reply work与receive cursor | netlink Socket transport | opened description持opaque private envelope | bind、publication、wait与consume |
| logical membership、ifindex、name与kind | initial-domain `LogicalInterfaces` | request-local owned snapshot | link/address/route projection |
| MAC、frame capacity与允许stale的link publication fact | `device/net` publication owner | boot-stable association和normalized snapshot | read-only link fields |
| IPv4 address、connected/default route与association | `Ipv4ControlPlane` | request-local owned snapshot | address/route dump |
| TCP role/state/tuple/queue、orphan与TIME_WAIT | Stack TCP owner | normalized owned record set | sock-diag dump |
| Linux layout、flags、errno、sequence与user copy | Socket/netlink ABI adapter | owners只接收或返回normalized values | UAPI containment |
| fd publication、dup/fork sharing与final-release trigger | opened-description owner | static final-release hook | exactly-once cleanup |

诊断snapshot、serialized datagram和wait hint都不是network truth。它们不得反向驱动route selection、
protocol progression、readiness、connect、attach或cleanup。

## NETLINK-TRANSPORT-001 — bounded request/reply lifecycle由netlink transport拥有

**规则：** 当前发布`AF_NETLINK + SOCK_RAW + NETLINK_ROUTE`与
`AF_NETLINK + SOCK_RAW + NETLINK_SOCK_DIAG`两个static tuple，并接受creation-time
`SOCK_CLOEXEC | SOCK_NONBLOCK`。每个protocol拥有独立的bounded port namespace；显式
`bind(sockaddr_nl { pid=0, groups=0 })`或首次send auto-bind得到稳定非零port，dup/fork alias共享
同一port、budget与reply progression。kernel destination固定为port 0/groups 0；userspace peer、multicast、
connect与其它protocol稳定拒绝。

完整request datagram先copy到kernel-owned buffer，再按`nlmsghdr`边界顺序处理。已accepted的prefix不因
later request失败回滚；可可靠划分的invalid/unsupported message各自产生`NLMSG_ERROR`，无法可靠继续的
framing tail只终止tail。dump reply保留sequence/local port，使用`NLM_F_MULTI`并以`NLMSG_DONE`结束；
recognized mutation只产生typed error，不改变network owner。每个request在publication前完成reply-work
budget admission：要么发布完整data与terminal message，要么发布有序`-ENOBUFS`，不能留下unterminated
partial dump。

`SO_SNDBUF`与`SO_RCVBUF`只修改owner-local、min/default/max clamp后的真实budget；
`NETLINK_EXT_ACK`与`NETLINK_GET_STRICT_CHK`是带一次低噪声诊断和退出条件的stateless compatibility
no-op，不建立behavior-driving bool或第二套parser。recv支持blocking/nonblocking、`MSG_DONTWAIT`、
`MSG_PEEK`与`MSG_TRUNC`；non-peek选择下一datagram时即消费，后续copy fault不requeue，peek success/fault
均不消费。receive没有ancillary producer，source address固定为kernel port 0/groups 0。

**Allocation / failure：** request、reply、snapshot与queue growth允许自然、适度、配置有界的kernel heap
allocation；allocation-free不是correctness要求。若消除allocation会制造不自然object graph、duplicate truth、
额外reservation/rollback phase或明显性能损失，优先选择直接且可审查的自然形状。当前工程阶段允许这些
合法bounded allocation在global OOM时沿用kernel-fatal/panic policy，不要求局部OOM recovery或伪造errno；
但用户可触发的oversized input、integer overflow、protocol/owner capacity full与malformed UAPI必须在
publication/commit前typed reject。该容许不覆盖无界增长、nested owner lock、blocking/reclaim、持锁user copy、
普通failure cleanup或复杂callback/drop。

**Cleanup：** semantic final release先retire source/transport并使新send/receive fail closed，再释放protocol
port、budget state与pending reply work。非最后alias close、raw fd number、temporary `Arc`或Rust `Drop`不能
推进lifecycle；late hint、重复close与旧port identity不能恢复retired transport。

**违反表现：** protocol共用一份port namespace；reply缺少terminal message；capacity full靠OOM或partial
publication表达；family内部保留user pointer；reply驱动network行为；compatibility option建立无consumer state；
final release前释放port或让旧reply进入新Socket。

**验证 / Enforcement：** framing/alignment/overflow、multi-message prefix/error、sequence/multipart/error/done、
capacity admission、bind/auto-bind/protocol namespace、budget mutation、blocking/peek/trunc/fault、creation rollback、
dup/fork/final release与stale identity KUnit；长期raw-netlink oracle；source audit；双架构真实consumer运行。

## NETLINK-ROUTE-DIAG-001 — route diagnostics只投影既有owner snapshot

**规则：** `RTM_GETLINK`只从committed initial-domain logical membership、boot-stable netdev association、
active attachment与publication facts形成request-local snapshot，投影ifindex/name/kind、truthful flags、effective
IP MTU、可用MAC与允许stale的operstate。`IFF_UP`只表示active publication；external `Unknown`不伪造成实时
carrier up。`RTM_NEWLINK + ACK`及其它recognized mutation返回error且逐字段无副作用。

`RTM_GETADDR`只从同一次IPv4 control-plane snapshot返回`127.0.0.1/8`与configured external address；
`RTM_GETROUTE`只返回main table的configured connected route和可选default gateway。adapter不遍历smoltcp
interface/socket binding反推policy，不输出local/broadcast/cache/neighbor/rule/policy route。BusyBox legacy
`rtgenmsg`只接受精确四字节C结构槽并忽略其三字节未初始化padding；完整request继续严格校验，扩展legacy
payload拒绝。

**违反表现：** diagnostic adapter复制route table或ifindex namespace；从smoltcp/private provider state推导policy；
把active、carrier与operstate合成一个bool；mutation执行或改变attach/control-plane facts；为固定命令建立bypass。

**验证 / Enforcement：** owner-local snapshot/projection与mutation no-side-effect proof；raw oracle覆盖dump/exact/error；
RV64未修改BusyBox 1.33.1与LA64 final-image `/bin/ip`的`link/addr/route show`原始输出人工审阅。

## NETLINK-SOCK-DIAG-001 — TCP owner形成normalized one-window record set

**规则：** `SOCK_DIAG_BY_FAMILY + AF_INET + IPPROTO_TCP` wildcard dump由Stack TCP owner在一个
observation window内枚举listener、pending/active connection、closing/orphan与尚未reclaim的TIME_WAIT，形成
不含handle、slot、generation、ring、fd、task或credential的owned records。adapter只做Linux state、tuple、
ifindex、state-mask与queue projection。一份logical listener只形成一条aggregate LISTEN record；`Recv-Q`是owner
已经admit的`Pending + Claimed` child count，`Send-Q`是normalized backlog。没有唯一interface scope的listener投影为
`idiag_if = 0`；SYN-RECEIVED、accepted/closing connection与deferred record继续报告真实interface。connection queue
分别来自owner RX/TX facts。uid/inode/timer为明确不可用的0，cookie为
`INET_DIAG_NOCOOKIE`；不得扫描fd table或伪造稳定identity。

相同wildcard TCP request的`AF_INET6`查询只返回匹配sequence的empty `NLMSG_DONE`，服务当前未修改
`ss -tan`双family query shape，不发布IPv6 capability。UDP/Unix/raw diag、exact tuple、extension与其它protocol
稳定拒绝。

**违反表现：** Socket缓存TCP state/queue；每个private projection各输出一条LISTEN或分别计算queue；diagnostics持有
live engine handle或guard跨receive；从readiness bool猜listener/queue；恒零queue冒充支持；为uid/inode扫描opened
description或task。

**验证 / Enforcement：** owner host proof覆盖single logical listener、aggregate pending/claimed queue、真实child interface、
active/closing/TIME_WAIT、tuple与state mask；raw oracle覆盖`idiag_if = 0`、Linux record/empty-done/error；source audit；
RV64/LA64 iproute2 6.1.0 `ss -tan`原始输出审阅。

## 当前接受边界

- 当前只承诺initial-domain、single-CPU acceptance configuration与boot/static topology；两个独立request可以观察
  不同时间点，不承诺cross-request/global transaction。
- 2026-08-09 closure在RV64运行609/609 KUnit、LA64运行611/611 KUnit；两架构raw oracle均
  `NETLINKTEST:PASS`，RV64 BusyBox 1.33.1、LA64 final-image `/bin/ip`与双架构iproute2 6.1.0
  `ss -tan`均exit 0，并人工确认logical link、IPv4 address/route、TCP listener/pending/accepted/TIME_WAIT
  tuple及queue字段。
- 2026-08-14 listener-ingress closure在owner host proof及RV64 `639/639`、LA64 `642/642` KUnit后，由双架构raw
  sock-diag oracle确认exactly one LISTEN、aggregate queue与`idiag_if = 0`，并由两架构`ss -tan`确认真实consumer仍exit 0。
- physical hardware、`smp > 1`、其它NIC/platform/deployment、full network LTP、压力/并发矩阵、双libc矩阵、
  其它`ip`/`ss` option与完整final harness均Not Run。

**最初来源 / 当前来源：** [Read-only Network Diagnostics RFC R0](../../rfcs/read-only-network-diagnostics/index.md)
的`NETLINK-DIAGNOSTICS-CUTOVER`、同RFC Closure与同一focused Git/PR evidence；
[TCP listener ingress publication RFC R0](../../rfcs/tcp-listener-ingress-publication/index.md)的
`TCP-LISTENER-INGRESS-CUTOVER`随后Refine logical listener scope与aggregate queue projection。
