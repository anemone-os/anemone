# Unix Socket Seqpacket 当前契约

**Contract ID：** `UNIX-SOCKET-SEQPACKET-001`
**状态：** Active
**Owner：** Unix seqpacket connection的两条record direction分别拥有queue、capacity、terminal与readiness；Unix endpoint/listener/namespace拥有共享connection-oriented control plane
**参与领域：** Unix IPC / Socket front / pathname namespace / opened description / iomux / epoll
**覆盖范围：** `AF_UNIX + SOCK_SEQPACKET + protocol 0`的unnamed pair与filesystem pathname connection、record I/O、shutdown/EOF、readiness及final-release lifecycle
**不覆盖：** Unix datagram、abstract namespace、autobind、credentials/fd passing、ancillary data、pidfd、pending error/`SO_ERROR`、timeout或可区分的zero-length record
**实现位置：** `anemone-kernel/src/fs/socket/unix/endpoint/{mod.rs,record.rs}`、`anemone-kernel/src/fs/socket/unix/{admission.rs,namespace.rs}`、`anemone-kernel/src/fs/socket/{front,api}/`
**依赖：** `SOCKET-FRONT-001`、`SOCKET-ABI-001`、`SOCKET-WAIT-001`、`UNIX-SOCKET-STATE-001`、`UNIX-SOCKET-ADDRESS-001`、`UNIX-SOCKET-LIFECYCLE-001`、`UNIX-SOCKET-NAMESPACE-001`、`OPENED-DESC-001..003`、`IOMUX-POLL-001..003`、`EPOLL-WATCH-001`、`EPOLL-READY-001`
**Pending Successor：** None
**最后核验：** 2026-08-04

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| endpoint role、profile与active association | `UnixEndpointCore` | namespace/admission持窄的profile-compatible capability | bind/listen/connect/accept与cross-type rejection |
| backlog、accepted child与connect/accept predicate | `UnixListener` | endpoint持listener association；waiter持non-owning route | pathname admission与capacity |
| 两端配对与两条record direction | `UnixSeqpacketConnection` | endpoint只持typed connection与side | send/receive/shutdown/readiness路由 |
| record queue、byte/count capacity与terminal | 对应`RecordDirection` | operation只持direction-local serialization gate与staged payload | whole-record commit、one-record consume与EAGAIN/EOF/EPIPE |
| Linux tuple、flags、sockaddr、copy cursor与返回长度 | Socket ABI adapter | record owner只接收normalized request并返回typed outcome | Linux ABI containment与`MSG_TRUNC`映射 |

## UNIX-SOCKET-SEQPACKET-001 — Record transaction由direction owner一次提交

**规则：** `socketpair`或type-compatible connect/accept admission建立一份seqpacket connection与两条有界record direction。一次成功的非空send/write call发布一个完整record；并发writer按direction-local operation gate取得顺序线性化，不能交错record。一次成功receive/read call至多观察或消费head record，不跨record拼接；并发reader同样按head owner取得gate的顺序线性化。

send在复制payload前检查maximum与当前完整record capacity；超过`unix_seqpacket_max_payload_bytes`返回`EMSGSIZE`，byte或record slot不足返回not-ready，由共同Socket wait映射blocking retry或nonblocking `EAGAIN`。payload allocation与exact copy在commit前完成，随后重检endpoint association、terminal及capacity，再以一次infallible owner-local更新charge并enqueue。失败、signal或revalidation失败均零发布；operation不得跨sleep保留reservation、staged record或private phase。当前默认maximum、每direction byte budget与record-count budget分别为65536、65536和128。

receive先稳定选择head及`min(destination length, record length)` prefix，在spinlock外exact-copy该prefix。普通short receive返回copied length并消费整个record；`MSG_TRUNC`返回original record length；`MSG_PEEK`不消费record或capacity，`MSG_PEEK | MSG_TRUNC`返回original length且保留head。prefix copy fault返回`EFAULT`并保留head及capacity。当前zero-length send/write成功但不发布record，zero-length receive成功返回0且不观察或消费head；与Linux的差异由register拥有。

writer shutdown或final close提交peer EOF，reader shutdown使peer后续send得到`EPIPE`并由共同层按`MSG_NOSIGNAL`决定`SIGPIPE`。已经入队的record先于EOF交付。READABLE来自queue nonempty或receive terminal；WRITABLE只表示send terminal，或当前至少还有一个record slot和一个payload byte，不承诺任意大小record可提交；RDHUP与完整HUP分别从direction terminal facts投影。notification只提示consumer recheck，不携带ready truth。

pathname stream与seqpacket profile不兼容时，namespace admission在backlog reservation、connection publication与client role commit前返回`EPROTOTYPE`。两种profile共享VFS identity/DAC、listener/backlog、address snapshot、fd preparation、final release及wait protocol，但stream byte direction与seqpacket record direction保持不同owner和不同data-plane state machine。

**支持的ABI：** `socket`、`socketpair`、pathname `bind/listen/connect/accept/accept4`、`getsockname/getpeername`、`SO_DOMAIN/SO_TYPE/SO_PROTOCOL/SO_ACCEPTCONN`、`read/write/readv/writev/sendto/recvfrom`、creation-time nonblock/cloexec、`MSG_DONTWAIT/MSG_NOSIGNAL/MSG_PEEK/MSG_TRUNC`、connected shutdown及poll/select/epoll READABLE/WRITABLE/RDHUP/HUP。connected `sendto`携带destination返回`EISCONN`。

**Failure / cleanup：** socketpair与accept继续使用unpublished preparation/rollback和infallible publication；semantic final release先撤销endpoint publication，再由record connection提交terminal与route snapshot，guard外notify/drop。关闭非最后dup/fork alias不推进lifecycle。record staging、queue charge与释放各有唯一owner，不能在copy fault、shutdown或retirement后stale commit、重复消费或泄漏capacity。

**违反表现：** stream与seqpacket共用一份可切换queue；endpoint缓存第二份record count/terminal/ready mask；not-ready attempt已推进user source；partial payload成为record；short receive保留record后缀；peek释放capacity；fault后既消费又重新交付；cross-type connect在backlog或role commit后才失败；poll callback payload替代current predicate。

**验证 / Enforcement：** owner-localrecord boundary/capacity/fault/shutdown KUnit；双架构同源unnamed/pathname、flags、short/peek/truncation、cross-type、capacity、shutdown、poll/select/epoll与multi-reader/writer guest suite；普通Rust `std::process::Command` exec success/failure consumer；RV64 `smp=4` focused runtime；tracked Linux source与host characterization。

**最初及当前来源：** [Unix seqpacket小迭代](../../devlog/changes/2026-08-04-unix-seqpacket.md)的`SOCKET-UNIX-SEQPACKET-CUTOVER`。

## 当前接受边界

- zero-length record与receive payload-copy-fault的Linux差异见[`ANE-20260804-UNIX-SEQPACKET-EDGE-ABI`](../../register/current-limitations.md#ane-20260804-unix-seqpacket-edge-abi)。
- pre-connection shutdown、retired bind inert inode与non-UTF-8 pathname继续由既有register条目拥有；本contract不扩大这些保证。
- ordinary Rust `Command`路径已验证；显式pidfd、`sendmsg/recvmsg`、`SCM_RIGHTS`和其它ancillary data不在本页范围。
- closure evidence覆盖RV64/LA64 release与guest、RV64 `smp=4` focused runtime及curated socket LTP；physical hardware、LA64 `smp>1`、其它SMP拓扑、full socket/network LTP与final harness Not Run。
