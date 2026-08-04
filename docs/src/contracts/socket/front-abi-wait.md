# Socket Front、ABI 与 Wait 当前契约

**Contract IDs：** `SOCKET-FRONT-001`、`SOCKET-ABI-001`、`SOCKET-WAIT-001`
**状态：** Active
**Owner：** general Socket front拥有immutable descriptor/private envelope与共同FileOps/ABI/wait orchestration；concrete family ops拥有family state与operation predicate
**参与领域：** socket syscall / VFS opened description / UDP / ICMP raw / Unix IPC / iomux / epoll
**覆盖范围：** UDP、ICMP raw与Unix共同Socket file association、typed operation boundary、Linux ABI containment、blocking与poll wait/recheck
**不覆盖：** family-specific packet/stream transaction、future family registry、通用error queue或通用mutable option bag
**实现位置：** `anemone-kernel/src/fs/socket/{front,api,udp,icmp_raw,unix}/`、`anemone-abi/src/net.rs`、`anemone-rs/src/{os,sys}/linux/net.rs`
**依赖：** `OPENED-DESC-001..003`、`NET-PROTOCOL-BOUNDARY-001`、`NET-SOCKET-ENDPOINT-001`、`NET-UDP-TRANSACTION-001`、`NET-ICMP-RAW-ENDPOINT-001`、`NET-ICMP-RAW-TRANSACTION-001`、`NET-SOCKET-WAIT-001`、`IOMUX-POLL-001..003`、`EPOLL-WATCH-001`、`EPOLL-READY-001`
**Pending Successor：** None
**最后核验：** 2026-08-04

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| semantic type与ops association | immutable static `SocketOps` descriptor | Socket保存descriptor引用与匹配的opaque private envelope | family-neutral dispatch与type query |
| Linux tuple、sockaddr、flags、errno与user copy | Socket ABI adapter | concrete ops只接收normalized value/request并返回typed outcome | containment与Linux-visible mapping |
| family role、buffer、namespace、protocol queue与operation predicate | 对应UDP、ICMP raw或Unix owner | front只持opaque private envelope并调用descriptor capability | operation commit与readiness求值 |
| fd publication、description status、fd-local flags与final-release trigger | `task::files` opened-description owner | Socket提供unpublished preparation与static final-release hook | creation rollback、dup/fork与exactly-once close |
| blocking round与iomux/epoll policy | Socket syscall、iomux或epoll各自consumer | family source提供snapshot、route publication与typed not-ready | wait、cancel与最终结果 |

## SOCKET-FRONT-001 — General front只拥有共同外壳

**规则：** 每个live Socket由一份immutable static `SocketOps` descriptor唯一见证resolved semantic `SocketType`，并与只能由该descriptor解释的opaque private storage关联。general front统一Socket inode/FileOps、opened-description hook及family-neutral operation dispatch；它不另存family/type tag、readiness/error truth，也不按UDP/ICMP raw/Unix concrete type downcast。

descriptor只表达UDP、ICMP raw或Unix至少一个真实consumer当前需要的capability。family-neutral datagram/file-I/O、address/query与option dispatch只负责normalized handoff，不保存family policy。永久不适用的operation由capability absence或typed unsupported表示；role-dependent rejection/not-ready必须由family owner的typed outcome表达，不能伪装为永久absence。UDP与ICMP raw Endpoint/packet transaction继续服从network contract，Unix runtime state继续服从本目录Unix contract。

**Failure / cleanup：** `socket`、`socketpair`与`accept`在fd publication前由unpublished preparation/reservation拥有rollback；publication后由opened-description semantic final release唯一触发family cleanup。`Drop`、临时`Arc` count或raw fd number不得成为semantic close truth，关闭非最后dup/fork alias不得推进family lifecycle。

**违反表现：** Socket缓存第二份family/type/readiness；common FileOps按private concrete type分支；为future TCP预建无consumer registry/slot；family state接收task、fd或raw Linux pointer；final release依赖Rust object destruction。

**验证 / Enforcement：** resolver/type witness、unsupported capability、single/pair/accept rollback与final-release KUnit；完整source audit；RV64/LA64 UDP、ICMP raw与Unix真实consumer回归。

**最初来源：** [Socket Abstraction 与 Unix Socket RFC R1](../../rfcs/socket-abstraction-and-unix-socket/index.md)。

**当前来源：** 同RFC Stage 4 `SOCKET-UNIX-CUTOVER`，并由[IPv4 ICMP Raw Socket RFC R0](../../rfcs/icmp-raw-socket/index.md)的`ICMP-RAW-CUTOVER` Refine。

## SOCKET-ABI-001 — Linux ABI止于family-neutral adapter

**规则：** raw family/type/protocol tuple、creation/per-call flags、sockaddr/addrlen、file/vector/message header、user pointer与Linux errno止于Socket ABI adapter。resolver选择已关联semantic type的static descriptor；family ops只接收normalized address/request/cursor并返回typed success、not-ready或rejection。共同copy cursor和family commit只报告、消费或提交已成功复制的prefix。

UDP adapter接受IPv4 connect/reconnect、`AF_UNSPEC` disconnect与peer query，并让connected scalar/file/vector I/O和
`sendmsg/recvmsg`复用同一typed datagram operation。explicit message name优先于Endpoint current peer；两者都不存在
返回`EDESTADDRREQ`。message iovec与ordinary `readv/writev`共同读取kernel I/O `max_iovec_count`，R1 acceptance
configuration为1024且不高于公开`IOV_MAX`；UDP family不保存第二份limit。超限、range/total overflow与payload fault在
datagram commit前完成验证。

`SOCK_NONBLOCK`进入shared opened-description status，`SOCK_CLOEXEC`进入fd-local flags。descriptor直接回答`SO_DOMAIN`、`SO_TYPE`、`SO_PROTOCOL`，family role回答`SO_ACCEPTCONN`。ICMP raw额外支持`IP_TTL`、`IP_TOS`与`ICMP_FILTER`的Linux optlen/value/copy policy；common adapter只分发normalized mutation/query，不建立mutable option bag。`SO_ERROR`和其它未支持option返回`ENOPROTOOPT`，不得以恒零值或pending-error bag冒充支持。

UDP send flags支持`MSG_DONTWAIT | MSG_NOSIGNAL`，receive支持`MSG_DONTWAIT | MSG_PEEK | MSG_TRUNC`；
`MSG_NOSIGNAL`在没有SIGPIPE producer时是带诊断和退出条件的compatibility no-op，其它flag稳定返回
`EOPNOTSUPP`。nonzero send control length返回`EOPNOTSUPP`且不提交payload；receive没有ancillary producer时不读取
control buffer并输出`msg_controllen = 0`。`msghdr.msg_flags`不作为send input flag；short receive输出
`MSG_TRUNC`，syscall input `MSG_TRUNC`只决定返回full packet length还是copied length。

`recvmsg`先完成payload transaction，再按peer name、`msg_flags`、`msg_controllen`顺序写回header字段；non-peek已经
detach的datagram在后续name/header fault后不requeue，peek路径不改变queue。raw header和user pointer不得进入family
state或跨blocking retry保留。

ICMP raw tuple只接受`AF_INET + SOCK_RAW + IPPROTO_ICMP`，并在任何fd reservation、Endpoint或source preparation前检查current task effective `CAP_NET_RAW`。IPv4 raw bind/connect/disconnect/query、`MSG_PEEK`/`MSG_TRUNC`/`MSG_DONTWAIT`/`MSG_NOSIGNAL`、zero/short/fault与datagram consume均止于adapter；`MSG_NOSIGNAL`只在没有`SIGPIPE` producer的raw family作为有诊断的兼容no-op。

Unix pathname输入按首版UTF-8/NUL/length边界归一化；output由immutable address snapshot生成并服从Linux addrlen/copyout prefix语义。未命名Socket与peer address absence是typed结果，不让family生成raw sockaddr。

**违反表现：** family ops解析Linux bit或返回Linux errno；raw user pointer越过adapter；descriptor与另存type不一致；copy fault提交未复制bytes；`SO_ERROR`恒零成功或建立无producer的error state。

**验证 / Enforcement：** tuple/permission/flag、IPv4 connected/unconnected与Unix sockaddr input/output、file/vector/
message iovec boundary、control/name/header ordering、zero/short/peek/truncate/fault与fd rollback KUnit/focused oracle；
repository-owned C/libc consumer、musl resolver、glibc/musl curated Socket LTP；两架构guest Socket suite与BusyBox ping。

**最初来源：** [Socket Abstraction 与 Unix Socket RFC R1](../../rfcs/socket-abstraction-and-unix-socket/index.md)。

**当前来源：** 同RFC Stage 4 `SOCKET-UNIX-CUTOVER`，并由[IPv4 ICMP Raw Socket RFC R0](../../rfcs/icmp-raw-socket/index.md)的
`ICMP-RAW-CUTOVER`和[UDP Socket Extension RFC R1](../../rfcs/udp-socket-extension/index.md)的
`UDP-EXT-R1-CUTOVER` Refine。

## SOCKET-WAIT-001 — Operation predicate由各自owner定义

**规则：** connect、accept、send与receive分别读取其state owner定义的current predicate；共同Socket层只统一Linux-visible `EAGAIN`分类、blocking choice、signal/SIGPIPE处理和`snapshot -> register -> recheck/final scan`协议。family attempt不得跨sleep保留private phase，也不得在family内部运行第二套wait loop或缓存ready mask。

source在更新owner truth并取得route snapshot后，必须在guard外notify/drop；notification只提示consumer重算。`O_NONBLOCK`、`SOCK_NONBLOCK`与`MSG_DONTWAIT`读取同一predicate，per-call flag不改变opened-description status。Unix首版没有pending error或ERROR readiness producer；真实source的ERROR与HANG_UP mandatory consumer policy仍由iomux/epoll contract决定。

**取消与cleanup：** signal、timeout、force、final close或losing waiter只retire自身wait round/route。late或重复hint对retired source/generation fail closed；waiter不延长family lifecycle，final release也不等待waiter。

**违反表现：** 一份shared Socket-ready truth替代各operation predicate；callback payload直接决定return；register window lost wake；busy-poll；family跨sleep持锁或commit token；cancel一个waiter撤销其它consumer。

**验证 / Enforcement：** source/operation表audit；connect/accept capacity、stream direction、UDP/raw capacity、snapshot-register-recheck、signal与late-hint KUnit/host proof；双架构blocking/nonblocking和poll/select/epoll runtime。

**最初来源：** [Socket Abstraction 与 Unix Socket RFC R1](../../rfcs/socket-abstraction-and-unix-socket/index.md)。

**当前来源：** 同RFC Stage 4 `SOCKET-UNIX-CUTOVER`与Git/PR closure evidence。

## 当前接受边界

- 当前三个真实consumer是IPv4 connected/unconnected UDP、`AF_INET + SOCK_RAW + IPPROTO_ICMP`与
  `AF_UNIX + SOCK_STREAM + protocol 0`；message-style success surface当前只发布给IPv4 UDP，本页不外推TCP或通用
  BSD Socket framework。
- closure evidence覆盖RV64/LA64 release与guest runtime、UDP C/libc和musl resolver、raw focused ABI、glibc/musl
  curated Socket LTP、owner-local proof以及UDP/Unix regression。glibc resolver保持Not Supported / Not Cut Over；
  physical hardware、`smp>1`、full socket/network LTP与final harness Not Run。
