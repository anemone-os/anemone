# Socket Front、ABI 与 Wait 当前契约

**Contract IDs：** `SOCKET-FRONT-001`、`SOCKET-ABI-001`、`SOCKET-WAIT-001`
**状态：** Active
**Owner：** general Socket front拥有immutable descriptor/private envelope与共同FileOps/ABI/wait orchestration；concrete family ops拥有family state与operation predicate
**参与领域：** socket syscall / VFS opened description / UDP / ICMP raw / TCP / Unix IPC / read-only netlink diagnostics / iomux / epoll
**覆盖范围：** UDP、ICMP raw、IPv4 TCP、Unix stream/seqpacket与read-only netlink diagnostics的共同Socket file association、typed operation boundary、Linux ABI containment、blocking与poll wait/recheck
**不覆盖：** family-specific packet/stream transaction、future family registry、通用error queue或通用mutable option bag
**实现位置：** `anemone-kernel/src/fs/socket/{front,api,udp,icmp_raw,tcp,unix,netlink}/`、`anemone-abi/src/net.rs`、`anemone-rs/src/{os,sys}/linux/net.rs`
**依赖：** `OPENED-DESC-001..003`、`NET-PROTOCOL-BOUNDARY-001`、`NET-SOCKET-ENDPOINT-001`、`NET-UDP-TRANSACTION-001`、`NET-ICMP-RAW-ENDPOINT-001`、`NET-ICMP-RAW-TRANSACTION-001`、`NET-TCP-ENDPOINT-001`、`NET-TCP-STREAM-001`、`NET-TCP-LIFECYCLE-001`、`NET-SOCKET-WAIT-001`、`IOMUX-POLL-001..003`、`EPOLL-WATCH-001`、`EPOLL-READY-001`
**Pending Successor：** None
**最后核验：** 2026-08-17

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| semantic type与ops association | immutable static `SocketOps` descriptor | Socket保存descriptor引用与匹配的opaque private envelope | family-neutral dispatch与type query |
| Linux tuple、sockaddr、flags、errno与user copy | Socket ABI adapter | concrete ops只接收normalized value/request并返回typed outcome | containment与Linux-visible mapping |
| family role、buffer、namespace、protocol queue与operation predicate | 对应UDP、ICMP raw、TCP、Unix或netlink transport owner | front只持opaque private envelope并调用descriptor capability | operation commit与readiness求值 |
| fd publication、description status、fd-local flags与final-release trigger | `task::files` opened-description owner | Socket提供unpublished preparation与static final-release hook | creation rollback、dup/fork与exactly-once close |
| blocking round与iomux/epoll policy | Socket syscall、iomux或epoll各自consumer | family source提供snapshot、route publication与typed not-ready | wait、cancel与最终结果 |

## SOCKET-FRONT-001 — General front只拥有共同外壳

**规则：** 每个live Socket由一份immutable static `SocketOps` descriptor唯一见证resolved semantic `SocketType`，并与只能由该descriptor解释的opaque private storage关联。general front统一Socket inode/FileOps、opened-description hook及family-neutral operation dispatch；它不另存family/type tag、readiness/error truth，也不按UDP/ICMP raw/TCP/Unix concrete type downcast。

descriptor只表达UDP、ICMP raw、TCP或Unix至少一个真实consumer当前需要的capability。family-neutral datagram/stream/file-I/O、address/query与option dispatch只负责normalized handoff，不保存family policy。永久不适用的operation由capability absence或typed unsupported表示；role-dependent rejection/not-ready必须由family owner的typed outcome表达，不能伪装为永久absence。UDP、ICMP raw与TCP Endpoint/packet/stream transaction继续服从network contract，Unix runtime state继续服从本目录Unix contract。

**Failure / cleanup：** `socket`、`socketpair`与`accept`在fd publication前由unpublished preparation/reservation拥有rollback；publication后由opened-description semantic final release唯一触发family cleanup。`Drop`、临时`Arc` count或raw fd number不得成为semantic close truth，关闭非最后dup/fork alias不得推进family lifecycle。

**违反表现：** Socket缓存第二份family/type/readiness；common FileOps按private concrete type分支；为future family预建无consumer registry/slot；family state接收task、fd或raw Linux pointer；final release依赖Rust object destruction。

**验证 / Enforcement：** resolver/type witness、unsupported capability、single/pair/accept rollback与final-release KUnit；完整source audit；RV64/LA64 UDP、ICMP raw、TCP与Unix真实consumer回归。

**最初来源：** [Socket Abstraction 与 Unix Socket RFC R1](../../rfcs/socket-abstraction-and-unix-socket/index.md)。

**当前来源：** 同RFC Stage 4 `SOCKET-UNIX-CUTOVER`，并由[IPv4 ICMP Raw Socket RFC R0](../../rfcs/icmp-raw-socket/index.md)的`ICMP-RAW-CUTOVER` Refine。

## SOCKET-ABI-001 — Linux ABI止于family-neutral adapter

**规则：** raw family/type/protocol tuple、creation/per-call flags、sockaddr/addrlen、file/vector/message header、user pointer与Linux errno止于Socket ABI adapter。resolver选择已关联semantic type的static descriptor；family ops只接收normalized address/request/cursor并返回typed success、not-ready或rejection。共同copy cursor和family commit只报告、消费或提交已成功复制的prefix。

UDP adapter接受IPv4 connect/reconnect、`AF_UNSPEC` disconnect与peer query，并让connected scalar/file/vector I/O和
`sendmsg/recvmsg`复用同一typed datagram operation。explicit message name优先于Endpoint current peer；两者都不存在
返回`EDESTADDRREQ`。message iovec与ordinary `readv/writev`共同读取kernel I/O `max_iovec_count`，R1 acceptance
configuration为1024且不高于公开`IOV_MAX`；UDP family不保存第二份limit。超限、range/total overflow与payload fault在
datagram commit前完成验证。

`sendmmsg`在一个稳定opened description上顺序复用既有family `sendmsg` transaction，并逐项投影`msg_len`；首条失败
返回errno，已有成功后遇到send或copyout failure返回已完成数，partial stream message停止后续项，`vlen`按Linux上限
clamp到1024。adapter不建立batch queue、shared commit、family state或跨message原子性。

`SOCK_NONBLOCK`进入shared opened-description status，`SOCK_CLOEXEC`进入fd-local flags。descriptor直接回答`SO_DOMAIN`、`SO_TYPE`、`SO_PROTOCOL`，family role回答`SO_ACCEPTCONN`。ICMP raw额外支持`IP_TTL`、`IP_TOS`与`ICMP_FILTER`的Linux optlen/value/copy policy；TCP发布`SO_REUSEADDR`、`TCP_NODELAY`与真实consuming `SO_ERROR`，option fact与async cause仍由TCP owner唯一保存，common adapter只分发normalized mutation/query，不建立mutable option/error bag。没有对应producer的family与其它未支持option返回`ENOPROTOOPT`，不得以恒零值或pending-error bag冒充支持。

Unix stream/seqpacket的`SO_PEERCRED`由family owner返回normalized `{tgid,euid,egid}` snapshot；adapter独占Linux
`struct ucred`布局、tgid到signed pid的可表示性检查、native-endian encoding及optlen/copyout policy。成功复制
`min(requested, sizeof(struct ucred))` bytes并把optlen写为实际复制长度；value fault发生时不先改写optlen。
非连接Unix role的typed rejection映射`ENOTCONN`，没有peer-credential producer的family映射`ENOPROTOOPT`。

Socket输入队列查询由adapter把数值相同的`FIONREAD`、`TIOCINQ`与`SIOCINQ`解码为
`SocketIoctlRequest::ReadableBytes`，再经static `SocketOps::ioctl`分发；raw command、argument、Linux signed
`int`表示、checked conversion、errno与用户copyout不得越过front。family callback只返回现有owner queue/stream的
瞬时typed fact：UDP为下一datagram payload长度，ICMP raw为下一完整IPv4 packet长度，TCP与Unix stream为累计未读
stream bytes，Unix seqpacket为全部排队record payload总和。UDP的队首长度与readiness共享同一`Option` truth，
`Some(0)`表示可读零长datagram而`None`表示空队列。TCP/Unix listener的typed role rejection映射`EINVAL`；Netlink
不发布该capability，unknown ioctl与Netlink `FIONREAD`保持`ENOTTY`。查询不peek、detach或consume数据，也不缓存
byte count；`FIONBIO`继续由opened-description status owner处理，不进入family ioctl callback。

UDP发布真实`IP_RECVERR` scalar option、consuming `SO_ERROR`与`MSG_ERRQUEUE` ancillary projection。raw option/header、
Linux errno、`sock_extended_err`、sockaddr/cmsg alignment与copy ordering只存在于adapter；UDP owner只接收normalized
enable request并返回typed pending cause或move-only record。`MSG_ERRQUEUE`输出quoted UDP payload、original destination、
offender、`SOL_IP/IP_RECVERR` cmsg和`MSG_ERRQUEUE` flag；data/control short分别加`MSG_TRUNC`/`MSG_CTRUNC`，empty
queue返回`EAGAIN`，detach后的copy fault消费record。ordinary I/O与`SO_ERROR`竞争同一Endpoint pending cause，adapter
不得缓存并列error truth或用恒零query冒充支持。

UDP send flags支持`MSG_DONTWAIT | MSG_NOSIGNAL`，ordinary receive支持`MSG_DONTWAIT | MSG_PEEK | MSG_TRUNC`；
`MSG_NOSIGNAL`在没有SIGPIPE producer时是带诊断和退出条件的compatibility no-op，其它flag稳定返回
`EOPNOTSUPP`。nonzero send control length返回`EOPNOTSUPP`且不提交payload；receive没有ancillary producer时不读取
control buffer并输出`msg_controllen = 0`；UDP `MSG_ERRQUEUE`是该规则的具名ancillary producer。`msghdr.msg_flags`不作为send input flag；short receive输出
`MSG_TRUNC`，syscall input `MSG_TRUNC`只决定返回full packet length还是copied length。

`recvmsg`先完成payload transaction，再按peer name、`msg_flags`、`msg_controllen`顺序写回header字段；non-peek已经
detach的datagram在后续name/header fault后不requeue，peek路径不改变queue。raw header和user pointer不得进入family
state或跨blocking retry保留。

ICMP raw tuple只接受`AF_INET + SOCK_RAW + IPPROTO_ICMP`，并在任何fd reservation、Endpoint或source preparation前检查current task effective `CAP_NET_RAW`。IPv4 raw bind/connect/disconnect/query、`MSG_PEEK`/`MSG_TRUNC`/`MSG_DONTWAIT`/`MSG_NOSIGNAL`、zero/short/fault与datagram consume均止于adapter；`MSG_NOSIGNAL`只在没有`SIGPIPE` producer的raw family作为有诊断的兼容no-op。

Unix pathname输入按首版UTF-8/NUL/length边界归一化；output由immutable address snapshot生成并服从Linux addrlen/copyout prefix语义。未命名Socket与peer address absence是typed结果，不让family生成raw sockaddr。

IPv4 TCP tuple接受`AF_INET + SOCK_STREAM + 0/IPPROTO_TCP`。adapter投影explicit/implicit bind、listen/backlog、
blocking/nonblocking connect、accept/accept4、local/peer query和typed async errno；TCP send支持
`MSG_DONTWAIT | MSG_NOSIGNAL`，receive支持`MSG_DONTWAIT | MSG_PEEK`。scalar/vector/single-message stream I/O只提交
已成功copy且被TCP owner接受或消费的prefix；buffered bytes先于EOF/error，RST不得映射为EOF。broken-stream send的
`SIGPIPE`由adapter投递，本次`MSG_NOSIGNAL`只抑制该信号。unsupported flag稳定返回`EOPNOTSUPP`，不得因consumer忽略
错误而success-no-op。

Netlink tuple只接受`AF_NETLINK + SOCK_RAW + NETLINK_ROUTE/NETLINK_SOCK_DIAG`。`sockaddr_nl`、
`nlmsghdr`、rtnetlink/inet-diag layout、message alignment、sequence、Linux state/flag/errno与user copy均止于
Socket/netlink adapter；network/TCP owner只提供normalized owned snapshot。`SO_SNDBUF/SO_RCVBUF`分发为
netlink transport的bounded budget mutation，`NETLINK_EXT_ACK/NETLINK_GET_STRICT_CHK`只作为有注释、低噪声
诊断与退出条件的stateless compatibility no-op，不建立mutable option bag或第二套parser。transport/framing、
route/TCP projection与allocation/cleanup边界见[Read-only Netlink Diagnostics](./netlink-diagnostics.md)。

**违反表现：** family ops解析Linux bit或返回Linux errno；raw user pointer越过adapter；descriptor与另存type不一致；
copy fault提交未复制bytes；`SO_ERROR`恒零成功；Socket复制TCP/UDP pending cause；Linux netlink struct或state进入
Network/TCP owner；`sendmmsg`建立batch-owned state或绕过single-message transaction；没有producer却建立error state。

**验证 / Enforcement：** tuple/permission/flag、IPv4 connected/unconnected与Unix sockaddr input/output、file/vector/
message iovec boundary、control/name/header ordering、zero/short/peek/truncate/fault、`sendmmsg` partial/copyout/clamp与fd
rollback KUnit/focused oracle；repository-owned C/libc consumer、musl/glibc resolver、glibc/musl curated Socket LTP；
两架构既有Socket/TCP suite、RV64 UDP extended-error deterministic chain，以及双架构netlink raw oracle和未修改
`ip`/`ss` consumer。

**最初来源：** [Socket Abstraction 与 Unix Socket RFC R1](../../rfcs/socket-abstraction-and-unix-socket/index.md)。

**当前来源：** 同RFC Stage 4 `SOCKET-UNIX-CUTOVER`，并由[IPv4 ICMP Raw Socket RFC R0](../../rfcs/icmp-raw-socket/index.md)的
`ICMP-RAW-CUTOVER`和[UDP Socket Extension RFC R1](../../rfcs/udp-socket-extension/index.md)的
`UDP-EXT-R1-CUTOVER` Refine；随后由[IPv4 TCP Socket RFC R0](../../rfcs/net-tcp/index.md)的
`NET-TCP-CUTOVER`及[IPv4 UDP ICMP extended error小迭代](../../devlog/changes/2026-08-06-ipv4-udp-icmp-extended-error.md)
Refine；[Read-only Network Diagnostics RFC R0](../../rfcs/read-only-network-diagnostics/index.md)的
`NETLINK-DIAGNOSTICS-CUTOVER`随后增加AF_NETLINK tuple与wire containment；[Unix peer credentials小迭代](../../devlog/changes/2026-08-13-unix-peer-credentials.md)
的`SOCKET-UNIX-PEERCRED-CUTOVER`增加`SO_PEERCRED` layout/copyout containment。
随后由[Socket FIONREAD小迭代](../../devlog/changes/2026-08-17-socket-fionread.md) Refine typed ioctl family dispatch与
输入队列查询语义。

## SOCKET-WAIT-001 — Operation predicate由各自owner定义

**规则：** connect、accept、send与receive分别读取其state owner定义的current predicate；共同Socket层只统一Linux-visible `EAGAIN`分类、blocking choice、signal/SIGPIPE处理和`snapshot -> register -> recheck/final scan`协议。public file readiness可以是低水位admission hint；若一次operation需要更强条件，blocking wait必须注册该operation-specific owner predicate，不能因public hint已ready而busy-retry。family attempt不得跨sleep保留private phase，也不得在family内部运行第二套wait loop或缓存ready mask。

source在更新owner truth并取得route snapshot后，必须在guard外notify/drop；notification只提示consumer重算。`O_NONBLOCK`、`SOCK_NONBLOCK`与`MSG_DONTWAIT`读取同一predicate，per-call flag不改变opened-description status。Unix首版没有pending error或ERROR readiness producer；TCP真实async cause可以产生ERROR，ERROR与HANG_UP mandatory consumer policy仍由iomux/epoll contract决定。

**取消与cleanup：** signal、timeout、force、final close或losing waiter只retire自身wait round/route。late或重复hint对retired source/generation fail closed；waiter不延长family lifecycle，final release也不等待waiter。

**违反表现：** 一份shared Socket-ready truth替代各operation predicate；callback payload直接决定return；register window lost wake；busy-poll；family跨sleep持锁或commit token；cancel一个waiter撤销其它consumer。

**验证 / Enforcement：** source/operation表audit；connect/accept capacity、stream direction、seqpacket payload-specific capacity、UDP/raw capacity、snapshot-register-recheck、signal与late-hint KUnit/host proof；双架构blocking/nonblocking和poll/select/epoll runtime。

**最初来源：** [Socket Abstraction 与 Unix Socket RFC R1](../../rfcs/socket-abstraction-and-unix-socket/index.md)。

**当前来源：** 同RFC Stage 4 `SOCKET-UNIX-CUTOVER`与Git/PR closure evidence；TCP作为新consumer由[IPv4 TCP Socket RFC R0](../../rfcs/net-tcp/index.md)的`NET-TCP-CUTOVER`验证，不改变本ID语义。

## 当前接受边界

- 当前七个published static tuple是IPv4 connected/unconnected UDP、`AF_INET + SOCK_RAW + IPPROTO_ICMP`、
  `AF_INET + SOCK_STREAM + 0/IPPROTO_TCP`、
  `AF_UNIX + SOCK_STREAM + protocol 0`、`AF_UNIX + SOCK_SEQPACKET + protocol 0`，以及
  `AF_NETLINK + SOCK_RAW + NETLINK_ROUTE/NETLINK_SOCK_DIAG`；message-style
  success surface发布给IPv4 UDP与TCP，`sendmmsg`只逐条复用这些已发布family transaction；本页不外推通用BSD
  Socket framework或`recvmmsg`。
- 既有closure evidence覆盖RV64/LA64 release与guest runtime、UDP/TCP C/libc和musl resolver、raw/seqpacket focused ABI、
  glibc/musl curated Socket LTP、owner-local proof、UDP/Unix regression、TCP external/CAgent及RV64 `smp=4` focused runtime。
  UDP extended-error增量覆盖RV64 dual-libc oracle、deterministic packet chain与glibc final-product resolver；该增量的
  LA64、physical hardware、`smp>1`、long pressure、full socket/network LTP均Not Run。final-image Socket LTP因缺少
  executable为0 attempted/6 skipped，初赛盘对应curated suite为6/6 PASS。
- Netlink增量覆盖RV64 609/609、LA64 611/611 KUnit、双架构raw oracle、RV64 BusyBox 1.33.1与
  LA64 final-image `/bin/ip`的`link/addr/route show`，以及双架构iproute2 6.1.0 `ss -tan`；hardware、
  `smp>1`、压力/并发、full network LTP、双libc与完整final harness保持Not Run。
