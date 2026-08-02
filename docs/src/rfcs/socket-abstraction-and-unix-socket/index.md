# RFC-20260801-socket-abstraction-and-unix-socket

**状态：** Accepted
**修订：** R1
**负责人：** doruche
**最后更新：** 2026-08-02
**领域：** fs / socket / Unix IPC / VFS / task files / iomux / epoll
**影响契约：** Introduce `SOCKET-FRONT-001`、`SOCKET-ABI-001`、`SOCKET-WAIT-001`、
`UNIX-SOCKET-STATE-001`、`UNIX-SOCKET-STREAM-001`、`UNIX-SOCKET-NAMESPACE-001`、
`UNIX-SOCKET-ADDRESS-001`、`UNIX-SOCKET-LIFECYCLE-001`；Refine `IOMUX-POLL-002/003`、
`EPOLL-READY-001`
**执行记录：** Git/PR（Stage 1/2 Closed；Checkpoint 1A/1B/2A/2B/3A Closed；Stage 3 Active；3B Ready / Not Active）；transaction None；contract cutover None

## 文档状态

本文是 Socket Abstraction 与 Unix Socket 的公共 Accepted R1 RFC。它把前置定位中已经形成的方向固定为经过 review
的 target、owner、ABI、failure/cleanup、Contract Impact 与 acceptance 边界；私有 positioning 不成为公共依赖或
并列 canonical source。

本文不是 current contract，也不表示R1 acceptance自动激活任何实现gate。当前 UDP、opened-description、VFS、iomux
与epoll语义继续以 `docs/src/contracts/` 下的 Active contract 为准；各Stage/checkpoint的resolution、activation与closure
状态由[实施路线](./implementation.md)唯一记录。达到`SOCKET-UNIX-CUTOVER`的全部验收前，不得修改current contract。

本 RFC 按当前规模保留[目标与不变量](./invariants.md)，并因已经出现真实多阶段实施需要而增加一份
[实施路线](./implementation.md)。当前不创建tracking page或transaction；任何resolution或checkpoint/Stage closure都
不自动授权下一个gate，也不使pending contract提前生效。

## 摘要

Anemone 已经通过 IPv4 UDP 交付第一条可用 Socket vertical slice，但 kernel-facing file object、Linux Socket ABI、
opened-description lifecycle 与 wait/readiness 仍主要由 `UdpSocketFile` 一条 consumer 路径塑形。直接进入 TCP 会让
TCP 复制 UDP 的局部结构，或让 TCP 的复杂状态机过早决定所谓通用 Socket framework。

本 RFC 以 filesystem pathname `AF_UNIX + SOCK_STREAM` 作为第二个异构 consumer，在同一 target 中完成两项互相
约束的工作：交付一条有实际用途的 Unix pathname stream 能力；根据 UDP 与 Unix Socket 的真实共同需求，建立最小
general `Socket` front、静态 `SocketOps` association、resolved `SocketType`、ABI containment 与 wait/recheck
边界。共同抽象只统一已经被两个 consumer 证明的外层协议，不统一 UDP datagram 与 Unix byte stream 的 endpoint、
namespace、buffer、transaction 或状态机。

## 背景与 Current Baseline

当前 [Network UDP Socket contract](../../contracts/net/udp-socket.md) 已经固定 kernel Socket 与 protocol Endpoint 的
owner fence、opened-description final release、UDP bind/send/receive transaction 和 readiness recheck。Checkpoint
1A 已把 live UDP file/opened-description/syscall dispatch 迁移到共同 Socket front，但 UDP private/source 与 Network
Stack Endpoint 仍各守原 owner；当前只有 UDP 一个真实 consumer，因此这仍是一条已生效的具体 UDP 能力，不是 general
Socket contract cutover。

[VFS Creation 与 Make Node contract](../../contracts/vfs/make-node.md#vfs-creation-001--current-task-creation-policy-止于-kernel-operation)
已经固定 user-thread named-object creation 的 current-context owner：task filesystem context 唯一拥有 process
umask，`kernel_*` creation operation 取得 operation-local credential/mask snapshot并完成pathname admission与final
metadata formation，context-free VFS primitive只消费显式facts并负责backend/dentry handoff。该能力已能创建
filesystem-backed `S_IFSOCK` inode，但明确不拥有pathname Socket data plane；Unix Socket需要在不把runtime state塞入
inode backend `prv` 的前提下，把VFS identity连接到live listener/connection。

[IOMUX-POLL](../../contracts/iomux/poll-wait.md) 和 [Epoll Protocol](../../contracts/epoll/protocol.md) 已经固定
snapshot/register/final-scan、source-owned predicate、non-owning route 与 consumer-owned watch/delivery policy。
Unix Socket 应作为新的真实 source 接入这些协议，并补齐 receive half-close 与完整 HUP 的独立表达；不能建立第二套
socket-only wait queue、ready mask cache 或 wake payload truth。

Linux 6.6.32 的 pathname bind、stream connect、socketpair、accept 与 stream I/O 只作为 ABI/observable behavior 和
owner-shape 比较证据：`xref:linux-6.6.32:net/unix/af_unix.c#unix_bind_bsd`、
`xref:linux-6.6.32:net/unix/af_unix.c#unix_stream_connect`、
`xref:linux-6.6.32:net/unix/af_unix.c#unix_socketpair`、
`xref:linux-6.6.32:net/unix/af_unix.c#unix_accept`、
`xref:linux-6.6.32:net/unix/af_unix.c#unix_stream_sendmsg` 与
`xref:linux-6.6.32:net/unix/af_unix.c#unix_stream_read_generic`。这些引用不自动决定 Anemone 的内部对象图、锁或
eventual target。

首版`SO_ERROR`排除的Linux-visible差异还参考
`xref:linux-6.6.32:net/unix/af_unix.c#unix_release_sock`、
`xref:linux-6.6.32:net/unix/af_unix.c#unix_poll`、
`xref:linux-6.6.32:include/net/sock.h#sock_error`与
`xref:linux-6.6.32:net/core/sock.c#sock_getsockopt`；它们只证明Linux存在reset producer、ERR投影与一次性消费，
不把该能力变成首版target。

## 目标

- 建立一份由现有 IPv4 UDP 与新增 Unix stream 共同消费的 general `Socket` front，固定其 immutable ops/type
  association、type-private storage、共同 FileOps projection、opened-description integration 与 ABI containment。
- 交付 `AF_UNIX + SOCK_STREAM + protocol 0` 的 `socket`、`socketpair`、pathname bind/listen/connect/accept、
  connected byte-stream I/O与shutdown、地址查询、blocking/nonblocking 和 poll/select/epoll 能力。
- 让 VFS inode identity、Unix live pathname binding 与 Linux-visible Socket address snapshot 各有唯一 owner，闭合
  bind/connect/unlink/final-close/stale identity 的 handoff 与 cleanup。
- 为 Unix endpoint role、listener backlog、connection、两个 directional byte stream、EOF/half-close/HUP 建立明确的
  唯一 truth 与生命周期边界。
- 让 connect、accept、send、receive 各自读取对应 state owner 定义的 predicate，只共享 Linux-visible `EAGAIN`
  分类及 snapshot/register/recheck/final-scan 协议。
- 以 UDP 与 Unix Socket 两个真实 consumer 的 source/ABI/runtime 证据共同证明 Socket Abstraction；只通过 Unix
  用例或只完成目录整理均不足以关闭 RFC。

## 非目标

- TCP、IPv6、raw socket，以及为未来 TCP 预建 operation surface、Endpoint identity 或状态机。
- Unix datagram、seqpacket、abstract namespace、autobind（包括只提供 family、不提供 pathname 的 bind）、credentials、
  fd passing、ancillary data、`sendmsg` / `recvmsg`、zero-copy 与 socket splice。connected but unnamed Socket 后续
  pathname bind 属于本 revision 的 scoped Linux conformance surface，不属于这里排除的 autobind。
- `SO_SNDBUF`、`SO_RCVBUF`、`SO_RCVTIMEO`、`SO_SNDTIMEO`、`SO_PEERCRED`、`SO_PASSCRED` 或完整 socket option
  compatibility。
- `SO_ERROR`、Unix Socket error readiness，以及 Linux AF_UNIX 对 listener close 清理 queued-but-unaccepted connection
  或 endpoint close 丢弃未读 inbound bytes 所产生的一次性 `ECONNRESET` pending-error 兼容语义。
- Linux AF_UNIX 在unconnected、bound或listening role上成功并持久化的pre-connection shutdown。R1在没有connected
  direction时对三个合法`how`均明确返回`ENOTCONN`，不保存pending shutdown intent，也不改变后续bind/listen/connect/
  accept；该兼容缺口由[register](../../register/current-limitations.md#ane-20260802-unix-preconnection-shutdown)记录。
- 完整 BSD Socket framework、万能 Rust trait、统一 backend registry、共同 datagram/stream transaction 或通用
  connection state machine。
- 修改 smoltcp/network control plane 来承载 Unix Socket，或把 Unix IPC state 放进 `anemone-net-api` /
  `anemone-smoltcp-stack`。
- 为 AF_UNIX 单独建立 byte-path、socket-local umask、VFS lookup、inode attachment 或 common-create rollback
  协议；pathname representation以及current-task creation admission/formation继续复用现行VFS Creation边界，现有
  VFS限制和开放问题继续由register拥有。
- 以强行跨 VFS operation 持有 Unix global lock、proof-only state、双重 VFS truth 或新 generic rollback framework
  换取形式上的 bind 全有或全无。
- 父 RFC target本身不冻结内部checkpoint、probe、逐文件write set、内部锁、queue/buffer算法或精确测试命令；
  [实施路线](./implementation.md)可以在各Stage resolution中固定需要长期保存的checkpoint顺序、语义边界、验证类别与
  停止条件，但这些实施路线不构成新的target或current contract。

## Target Architecture

### General Socket front 只拥有共同外壳

VFS/opened-description 层继续拥有 fd publication、opened-description status、fd-local flag 与 semantic final-release
trigger。general `Socket` 是所有 Socket file 共同的 kernel-facing front object，通过静态 `SocketOps` struct-vtable
分发 Socket operation，并通过一条共同 FileOps projection 接入 VFS。它不是名为 core 的中心 runtime state owner。

每份静态 `SocketOps` 唯一关联一个 resolved semantic `SocketType`，并作为该 type 的运行时见证。`Socket` 不另存
family、Linux base type、protocol 或第二份 `SocketType`；type-private storage 使用 `Opaque`，且只能由对应 ops
implementation 解释。general syscall、FileOps、opened-description、iomux 或 epoll 不得 downcast 后按 UDP/Unix
分支，也不得取得 backend-private identity、namespace、buffer、operation lock、readiness 或 error truth。

`SocketOps` 表达 kernel Socket operation boundary，而不是 network protocol backend vtable。UDP 继续通过窄
Endpoint capability 与 Network Stack 交互；Unix Socket 继续留在 kernel IPC/VFS owner 内。不同 type 默认使用不同
静态 descriptor，可以复用具体 function，但不预建 `SocketClass`、动态 class registry 或第二层 type/ops association。

### SocketOps 表先固定能力族，不冻结 Rust 签名

本 R1 不再把 `SocketOps` surface 留成完全开放问题。它应覆盖本 target 的 creation、Socket syscall、共同
FileOps、wait/readiness 与 semantic final release 所需能力；每项能力必须至少有 UDP 或 Unix stream 这一真实
consumer，不能为未来 TCP 预留没有当前义务的 slot。下表中的 entry 名称只表示方向，不冻结最终字段名、参数、返回
类型、`Option<fn>` / typed unsupported 的物理表达或多个紧密 entry 是否合并：

| 能力族 | 方向性 entry / descriptor 信息 | 共同外壳负责什么 | concrete ops / family owner 负责什么 |
| --- | --- | --- | --- |
| 类型与创建 | immutable `SocketType` witness；single-Socket create；paired create | resolver 选择静态 descriptor，分离 fd/status flags，并包装 unpublished Socket | 创建 type-private state；返回 publication 前 rollback authority；Unix paired create 原子准备两个已连接端，UDP 明确不支持 pair |
| 生命周期 | unpublished abort；semantic final release | creation/socketpair/accept transaction 决定 fd publication；opened-description 只在 final release trigger 调用一次 | 撤销 source/endpoint/binding admission，再完成 family-local retire/cleanup；不得把 `Drop` 或 `Arc` count 当语义 close |
| 名称与状态转换 | bind；listen；connect attempt；accept attempt；shutdown | 解析 sockaddr/backlog/flags，组合 blocking choice 与 accepted-fd publication | 校验并推进 endpoint/listener/connection owner state，返回 typed success/not-ready/rejection；accept 产出仍未发布的 child Socket 与 peer-name snapshot |
| 状态观察 | local address；peer address；runtime role query | 完成 sockaddr/addrlen copyout；从 descriptor 直接投影 `SO_DOMAIN/SO_TYPE/SO_PROTOCOL` | 返回 immutable name snapshot 与 `SO_ACCEPTCONN` 所需 listening fact；UDP 稳定回答 false，Unix 从 listener role truth 派生；不返回 raw sockaddr 或 Linux errno |
| 数据面 | send attempt；receive attempt | `read/write/readv/writev/sendto/recvfrom` 归一为 typed request/cursor，处理 user copy、per-call flags、blocking 与 errno | UDP 保持 datagram transaction，Unix 保持 stream partial progress/peek/EOF；各自返回 owned/borrow-scoped typed outcome，不共享 buffer 或 transaction |
| wait / readiness | source snapshot；route register/recheck；retire invalidation | 共同 FileOps `poll`、blocking loop、iomux/epoll 只组合 interest、route、signal/timeout 与 final scan | 从当前 family facts 投影 source-neutral readable/writable/RDHUP/HUP；notification 只提示重查，不交付 ready mask truth或 operation commit |

并非每个 semantic type 都成功支持表中每项 operation。永久不适用的能力必须由静态 descriptor absence 或 typed
`Unsupported` 明确表达，并由共同 ABI adapter 映射既定 errno；它不是 family tag，也不能要求通用层 downcast。
state-dependent rejection/not-ready 则必须进入对应 family operation 的 typed outcome，不能伪装成永久 capability absence。

首版没有成功的 mutable socket option，因此不为假想 option bag 预建 `set_option` state/slot：immutable type query
直接来自 descriptor，`SO_ACCEPTCONN` 通过 concrete ops 读取 runtime role，其余 `getsockopt` / `setsockopt` 由
family-neutral ABI entry 按本文矩阵稳定拒绝。未来新增成功 option 时，再由真实 consumer 的 RFC Refine 决定它属于
共同 descriptor、family op
还是独立 owner。

共同 FileOps 也不复制一套 backend operation table：`read/write` 与 vector variants 进入同一 send/receive 能力，
`poll` 进入 readiness 能力；seek、directory I/O 与 unsupported ioctl 由 Socket 外壳给出共同文件语义。connect、accept、
send、receive 中会等待 family fact 变化的部分按单次 attempt 暴露，task wait loop 仍留在 Socket/syscall owner；若该
拆分被 live transaction 证据证伪，按本文 stop condition 回到 RFC review。

### Linux ABI 只存在于边界

`family/type/protocol` 是创建期 Linux ABI 编码。Socket ABI resolver 唯一负责未知 bit、family/type/protocol errno、
alias/default normalization，以及 `SOCK_NONBLOCK` / `SOCK_CLOEXEC` 向 opened-description status 与 fd-local flag 的
分离；合法 tuple 被解析为一份静态 ops association 后，raw tuple 不再进入 Socket 或 backend state。

`sockaddr_*` layout、addrlen、Linux flag number、user pointer 与 errno 同样止于 syscall/ABI adapter。内部 operation
只接收 typed request/context 并返回 typed outcome。`SO_DOMAIN`、`SO_TYPE` 与 `SO_PROTOCOL` 从 ops 关联的 immutable
`SocketType` 反向投影，不缓存原始 syscall 参数或默认 protocol spelling。

在本 RFC 明确列入、且未被 non-goal、current limitation 或显式例外排除的 `AF_UNIX + SOCK_STREAM` 行为面上，
Linux 6.6.32 的用户可见行为是首版 target。该范围包括 `sockaddr_un`/addrlen/copyout、connected unnamed bind与
peer-name可观察期、listener/connect竞争、accept consume/copyout/fd publication、stream partial progress/copy fault，
以及readable/writable/EOF/RDHUP/HUP。UAPI header或man page不能单独覆盖的copy side effect、race和lifecycle结果，
由tracked Linux source与focused Linux runtime共同提供oracle。

这条规则不冻结Linux内部对象图，也不要求Anemone复制Linux锁序、queue、buffer、helper或完整调用顺序。实现可以自然
选择内部表示、commit point与linearization；竞态不指定脱离commit point的固定winner，但每个用户可见结果必须落在
Linux允许的集合内，并由Anemone的唯一owner、commit/consume和cleanup解释。若live implementation证明某项兼容目标
工程代价过高，必须在合入较弱行为前进入Target Renegotiation；代码不能静默选择偏差。

### Family state 各自拥有 operation truth

UDP Endpoint、Unix endpoint/listener/connection 与未来另行接受的 TCP Endpoint 不共享 backend identity、buffer、
namespace 或状态机。Unix Socket 内部按语义 role 分配唯一 owner：per-Socket endpoint role 拥有 local-name 与当前
unbound/bound/listening/connected association；listener state 拥有 backlog/admission；connection state 拥有 peer
relation 与两个 directional stream；每个 direction 拥有自己的 byte sequence、capacity、write-side shutdown、EOF 和
terminal facts。精确 Rust object graph 可以变化，但不得让多个结构同时推进同一 role、backlog、connection、data、
half-close 或 terminal truth。

## Owner 与协议边界

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| raw Linux tuple、sockaddr、flags、user copy 与 errno | Socket syscall/ABI adapter | core/family 只接收 typed request/outcome | Linux compatibility containment |
| fd slot、status、fd-local flag 与 semantic final release | `task::files` opened-description owner | Socket 持 creation/final-release hook | publication、dup/fork/close |
| immutable ops/type association 与 private storage envelope | general `Socket` | syscall/FileOps 持共同 front capability | family-neutral dispatch |
| UDP Endpoint/binding/datagram/protocol facts | Network Stack Endpoint owner | UDP ops 持 opaque capability | 保持当前 UDP contract |
| pathname resolution、directory search/create DAC、umask/owner formation、inode/dentry与link/rename/unlink | task filesystem context / user-thread kernel creation operation / context-free VFS primitive / filesystem backend | Unix namespace owner只提交requested kind/permission，并取得`InodeRef` identity与operation-local authorization | namespace resolution、创建权限形成与publication |
| inode identity到live binding的index | Unix pathname-binding registry | connect取得operation-local binding capability | VFS identity到live listener admission |
| Unix endpoint role与local-name fact | 对应 Unix Socket endpoint owner | accepted/peer view持窄immutable name capability | bind/listen/connect与地址观察 |
| backlog、pending accepted endpoint与admission | 对应 listener state | connect/accept持operation-local capability/outcome | connect commit与accept dequeue |
| peer relation与两个directional stream | 对应 connection state | 两端endpoint持connection capability | stream I/O、shutdown与close |
| data/capacity/EOF/RDHUP/HUP predicate | 对应 Unix directional state | Socket/iomux/epoll只取snapshot并投影 | operation progress与readiness |
| blocking syscall wait loop | Socket/syscall owner | family op提供单次attempt与owner predicate | status/per-call flag、signal、timeout、`EAGAIN` |
| iomux wait round / epoll watch与delivery | iomux / epoll各自owner | source提供snapshot/register/recheck | consumer policy与最终交付 |

这些 owner 之间只传递当前 operation 所需的 typed request/outcome、opaque capability、immutable snapshot 或 recheck
hint。完整 `Task`、fd table、user pointer、Linux errno、backend-private object、锁 guard 或 mutable state container 不得
跨到不拥有它的层。

## 关键 Handoff、Failure 与 Cleanup

### 一般堆分配与 OOM

本 RFC 继承当前内核的工程原则：普通 `Box` / `Arc`、集合扩容以及 owner-local node/buffer 的堆分配使用
infallible allocation；真实 heap OOM 可以 panic，并视为 kernel-fatal，不进入 Socket operation 的 typed failure、
Linux errno 或 rollback target。实现和验收不要求 allocator failure injection 或 OOM recovery。

因此，不能只为在 heap OOM 后继续运行而引入 intrusive collection、预分配 pool、fallible mirror、duplicate index、
额外 reservation/rollback phase，或把分配移出其自然 owner/lifecycle。实现应优先保持满足唯一 owner、memory safety、
execution-context 与 cleanup 约束的自然代码形状。该边界只覆盖 allocator OOM；fd slot、backlog/buffer 等明确容量耗尽，
用户长度/整数溢出、user copy、VFS 与其它会正常返回的 admission/operation failure，仍必须按本 RFC 的 ABI、failure 与
rollback 规则处理。完整区分见[Heap OOM 与 normal resource exhaustion](./invariants.md#heap-oom-与-normal-resource-exhaustion)。

### Creation 与 fd publication

`socket` 在 fd publication 前完成 type/backend creation；可返回失败时撤销未发布的 backend association 与 reservation。
`socketpair` 直接创建一条已连接的 Unix stream，并把两个 opened descriptions 作为一次可回滚 publication 对用户
可见；reservation/description construction 中任何会正常返回的失败，以及用户数组写回失败，都不能留下半个 pair、
published fd 或泄漏的 connection。

`accept` 只在 listener owner 已经给出可消费 connection/accepted endpoint 后构造新 Socket。accepted Socket 在
新 fd commit 前保持 unpublished，不继承 listener 的 `O_NONBLOCK`；失败路径必须由 accept transaction 明确决定
是否保留 backlog item 或消费并关闭该 connection，不能留下无 owner 的 accepted endpoint。copyout fault、consume与
fd publication的用户可见结果服从上述scoped Linux conformance；reservation、child构造与内部rollback次序由实现
自然决定。

### Pathname bind、connect 与 unlink

`bind` 使用调用task的filesystem context解析parent；路径前缀逐级服从directory search，最终parent必须通过
`WRITE | EXECUTE` DAC且mount必须可写。final component只用于创建，任何已经存在的entry都使`bind`返回
`EADDRINUSE`。创建pathname socket不额外要求`CAP_MKNOD`；普通DAC capability bypass继续完全由VFS
`FsPermChecker`语义决定。

Unix bind只把`InodeType::Socket`与`0777` requested permission交给user-thread kernel creation operation。task
filesystem context继续唯一拥有process umask；该operation取得一次credential/mask snapshot，完成parent lookup、
create DAC与final permission/owner formation，再把显式metadata交给context-free VFS primitive。Socket/Unix owner
不得读取、缓存或应用umask，也不得把anonymous Socket file inode的permission或`fchmod(socket_fd)`结果作为pathname
创建输入。

上述owner与handoff已经作为`VFS-CREATION-001`生效，并有跨架构umask/creation runtime证据。本RFC不重新证明通用
umask生命周期，也不建立Socket-local oracle；但AF_UNIX是新的named-object consumer，最终closure必须以focused
integration case证明pathname bind确实复用该production creation path并得到umask-adjusted mode。

在上述VFS权限边界内，`bind`先完成自然可前置且会正常返回错误的Unix-owned preparation/admission，再由VFS
创建node，最后以返回的stable inode identity发布live binding。VFS node publication与Unix live-binding publication
不要求组成强原子transaction；两者之间允许短暂出现“socket inode可lookup、但尚无live binding”，并发`connect`
可以按没有live binding失败。`bind`成功返回前，live binding与Socket local-name必须都已经提交。

首选路线让 VFS create 成功后的 Unix commit 不再进入 interruptible wait 或执行会正常返回错误的 readmission；普通
堆分配继续服从上述 kernel-fatal OOM 边界，不要求仅为消除 post-create allocation 而预留资源或改变自然对象图。
若 live implementation 证明这需要长期跨 VFS operation 持有 Unix global lock、扭曲对象图、增加 proof-only adapter、
复制 VFS truth，或反向修改 VFS common-create/rollback protocol，本 target 允许采用较弱但诚实的工程边界：post-create
可返回 error 可以让 `bind` 失败并遗留没有 live binding 的 inert socket inode。Unix local-name/registration 必须
共同未发布，临时资源必须释放，调用者显式 `unlink` 后才能复用 pathname，final close 不自动 unlink。该退路若实际
发生，必须在 closure 与 current limitation 中记录触发点和用户可见结果。

每次`connect` attempt都先由VFS完成普通pathname lookup：路径前缀要求directory search，最终解析到的inode只要求
`WRITE` DAC，不要求该inode的`READ`/`EXECUTE`，也不要求final parent可写。成功的DAC检查只授权本次attempt对该
resolved inode identity继续执行socket-kind检查、Unix registry查询与listener admission；attempt以not-ready结束并
进入blocking wait后，下一次attempt必须重新lookup、重新检查DAC并重新取得binding capability，不能跨wait缓存权限
结果。同一次attempt内并发`chmod`/`chown`按VFS DAC检查的先后形成合法success或`EACCES`，不为此跨owner持锁。

pathname inode当前permission/owner始终是新连接DAC的唯一truth；`chmod`/`chown`与hard-link alias因此自然影响后续
attempt，但不撤销已经commit的connection，也不要求listen/accept或connected stream I/O重复检查pathname。VFS inode
ops/private data不回调或嵌入Unix runtime state。普通`mknodat(S_IFSOCK)`、reboot恢复或final close后留名的socket
inode没有live binding，不能恢复旧endpoint。

unlink 只撤销对应 VFS name；其它 hard-link alias 若仍解析到同一 inode，应继续命中同一 live binding。unlink 不终结
已有 listener/connection，close 也不自动 unlink。final release 由 Unix owner 撤销 live-binding publication，使已经
取得 capability 的 late admission fail closed，再释放 inode identity capability。同名 rebind 形成新 inode identity，
不能命中旧 generation。

VFS common-create 自身从 backend commit 到 cache/dentry materialization 的既有窗口继续由
[`ANE-20260801-VFS-CREATE-PUBLICATION-ATOMICITY`](../../register/open-issues.md#ane-20260801-vfs-create-publication-atomicity)
承接；本 RFC 不声称关闭它，也不得把该 VFS failure path 当成 Unix rollback proof。

### Connection、stream 与 close

非阻塞 connect 不建立本 revision 未定义的异步 `EINPROGRESS` phase；not-ready 按 `EAGAIN` 分类。blocking connect
通过 listener admission predicate 的 wait/recheck 后重试，只有 listener owner 的 admission commit 才建立 connection
与 pending accepted endpoint。accept dequeue、accepted-fd publication、signal/copyout failure 与 listener close 的
用户可见结果服从scoped Linux conformance；内部winner由唯一owner的commit/linearization自然决定，不额外冻结固定
调度结果。

byte-stream send/write 由对应 outbound direction owner裁决capacity、peer receive shutdown 与 partial progress；
receive/read 由 inbound direction owner裁决data、`MSG_PEEK`、EOF 与 partial consume。peer 不再接收时，未带
`MSG_NOSIGNAL` 的 send/write 产生 `SIGPIPE + EPIPE`，带 flag 时只返回 `EPIPE`。`SHUT_RD/WR/RDWR` 与 final close
通过 directional owner 推进，不由 Socket front、wait source 或 opened-description 复制 half-close truth。

R1只在endpoint已经关联paired connection时提供上述shutdown能力。unconnected、bound与listening role没有direction
owner，因而三个合法`how`均返回`ENOTCONN`且不产生状态；invalid `how`仍返回`EINVAL`，fd/socket lookup保持Linux
错误优先级。不得用success-no-op伪装Linux兼容，也不得为该排除面在endpoint或listener中增加shutdown bit。

首版不为 teardown 建立一次性 reset/pending-error latch。listener close 清理 queued-but-unaccepted connection，或
endpoint final close 丢弃该 endpoint 尚未读取的 inbound bytes时，由 listener/connection/directional owner推进已有
terminal、EOF、RDHUP/HUP与后续`EPIPE`/`SIGPIPE`语义；已经排队到存活endpoint inbound direction的数据仍先于EOF
交付。上述路径不额外产生`ECONNRESET`、`SO_ERROR`或Unix error readiness。若后续真实consumer要求Linux这一兼容
行为，必须另行Refine producer、唯一owner、I/O与`SO_ERROR`消费顺序以及ERR投影，不能把reset原因缓存到Socket front。

## ABI 与可见语义

### Family、type 与 protocol

- 现有 `AF_INET + SOCK_DGRAM + 0/IPPROTO_UDP` 解析为当前 IPv4 UDP semantic type；显式 UDP protocol 与默认
  protocol 不形成两种创建后 identity。
- 新增 `AF_UNIX`，以及数值相同的 `AF_LOCAL` / `PF_UNIX` alias；首版只支持 `SOCK_STREAM + protocol 0`。
- `socketpair` 只支持 `AF_UNIX + SOCK_STREAM + protocol 0`。
- unknown type bits 返回 `EINVAL`；unsupported base type 返回 `ESOCKTNOSUPPORT`；unsupported protocol 返回
  `EPROTONOSUPPORT`；unknown family 返回 `EAFNOSUPPORT`。
- `socket` / `socketpair` 支持 `SOCK_NONBLOCK | SOCK_CLOEXEC`，分别落在 opened-description status 与 fd-local
  flag；其它未知 type flag 不静默接受。

### Syscall 与数据面

首版 target 包含：

- `socket`、`socketpair`；
- `bind`、`listen`、`connect`、`accept`、`accept4`；
- `getsockname`、`getpeername`；
- connected Unix stream的`shutdown(SHUT_RD/SHUT_WR/SHUT_RDWR)`；没有connected direction时返回`ENOTCONN`；
- family-neutral `getsockopt` / `setsockopt` ABI entry；immutable type query 由 descriptor 投影，runtime
  `SO_ACCEPTCONN` 由 concrete ops 的 role query 回答，首版没有成功的 mutable option；
- Unix stream 的 `read`、`write`、`readv`、`writev` 与 connected-stream `sendto` / `recvfrom`；
- 普通 poll/ppoll/select/pselect6/epoll 对 data、writable、EOF、RDHUP 与 HUP 的投影。

`accept` 等价于 `accept4(..., 0)`；`accept4` 只接受 `SOCK_NONBLOCK | SOCK_CLOEXEC`。accepted description 不继承
listener 的 `O_NONBLOCK`。send 支持 `MSG_DONTWAIT | MSG_NOSIGNAL`，recv 支持 `MSG_DONTWAIT | MSG_PEEK`；
per-call `MSG_DONTWAIT` 不修改 opened-description status。其它未列 flag 不属于成功语义，不能以 no-op 冒充支持。

connect、accept、send、receive 各自读取对应 operation/state owner 定义的 not-ready predicate。这些 predicate 不合并
为一份共同 fact，也不由 general Socket、wait、iomux 或 epoll 拥有；它们只共享 `EAGAIN` 分类和外层
registration/recheck/final-scan 协议。blocking path 不得 busy-poll，wake/notification 不能替代最终 predicate。

### Pathname 与地址观察

首版只支持 filesystem pathname。`sockaddr_un` ABI adapter 按 raw bytes 解析 family、addrlen 与 NUL boundary，再
把 filesystem component 交给当前 VFS representation；本 RFC 继承
[`ANE-20260801-VFS-NON-UTF8-PATHNAME`](../../register/current-limitations.md#ane-20260801-vfs-non-utf8-pathname)，
不建立 Unix-local byte-path fallback。

pathname创建与DAC服从上文规则。Unix bind只把`InodeType::Socket + 0777` requested permission交给
[VFS Creation 与 Make Node](../../contracts/vfs/make-node.md#vfs-creation-001--current-task-creation-policy-止于-kernel-operation)
的user-thread kernel creation operation；最终permission与owner formation是该current contract dependency，不形成
Socket-local umask state或anonymous-inode mode template。Socket closure只验证新consumer的production handoff，不
重开通用umask owner/lifecycle proof。

Unix Socket local name 是 bind-time immutable address snapshot，不是 VFS current pathname。rename、hard-link alias 与
unlink 不改写它；`getsockname` 不重新查询 VFS，`getpeername` 不保存 connect 调用使用的 alias。accepted Socket
共享 listener 已提交的 name snapshot，但不继承 listener 的 live-binding registration。socketpair 与未绑定 peer 的
地址保持 unnamed。精确 addrlen/truncation/copyout、connected unnamed Socket 后续 bind，以及 peer close 后地址可见期
服从上述scoped Linux conformance；内部typed address、snapshot与capability lifetime仍由对应owner自然实现。

### Socket options 与首版排除

首版确定支持的 immutable query 是 `SO_TYPE`、`SO_DOMAIN` 与 `SO_PROTOCOL`；`SO_ACCEPTCONN` 经 concrete ops 的
runtime role query读取当前 listening fact，UDP稳定返回0，Unix从listener role owner派生。首版不承诺任何成功
`setsockopt` option；unsupported option 稳定返回
`ENOPROTOOPT`，不建立无行为的兼容 state。

首版明确排除`SO_ERROR`与Unix Socket error readiness；`getsockopt(SOL_SOCKET, SO_ERROR, ...)`稳定返回
`ENOPROTOOPT`。Unix family state不建立pending-error fact，poll/epoll source也不发布无producer的ERR。connect、
accept与stream I/O的同步失败继续由对应operation owner返回typed outcome，并由ABI adapter映射errno；上述teardown
路径按既定terminal/EOF/HUP/EPIPE边界收敛，不把Linux的一次性`ECONNRESET`语义伪装成已支持。

未来只有在TCP或真实Unix consumer产生需求，并通过RFC Refine闭合producer、owner、覆盖/消费顺序、copyout与error
readiness后才能加入该能力。permanent-zero `SO_ERROR`、独立 `has_error` readiness cache或预建error queue均不允许。

### Readiness 与 half-close

Unix directional state 唯一拥有 data、capacity、EOF、peer write-side shutdown、local/peer close 与 terminal facts。
receive half-close 与完整 HUP 是两项独立事实；source-neutral readiness 增加独立 receive-half-close category，poll 与
epoll 分别按 interest 投影为 `POLLRDHUP` / `EPOLLRDHUP`。EOF 仍使 ordinary readable/select readfds 可观察；HUP
保持 mandatory delivery。current iomux/epoll 对真实source error的mandatory ERR规则保持不变，但本首版Unix source
没有error producer，不得制造ERR。writable/RDHUP/HUP的用户可见组合服从上述scoped Linux conformance；内部
predicate、snapshot与notification实现不因此冻结。

## Contract Impact

以下均为 pending target；`SOCKET-UNIX-CUTOVER` 前 current contract 保持不变。

| Contract ID | 变化 | 当前规则 | Target 摘要 | Cutover |
| --- | --- | --- | --- | --- |
| `SOCKET-FRONT-001` | Introduce | None（尚未生效） | general Socket 只拥有 immutable ops/type association、private storage envelope与共同FileOps projection，不拥有family runtime truth | `SOCKET-UNIX-CUTOVER` |
| `SOCKET-ABI-001` | Introduce | None（尚未生效） | Linux tuple/sockaddr/flags/user copy/errno止于ABI adapter；resolved semantic type由静态ops唯一见证；首版`SO_ERROR`返回`ENOPROTOOPT` | `SOCKET-UNIX-CUTOVER` |
| `SOCKET-WAIT-001` | Introduce | None（尚未生效） | 各operation读取各自owner-defined predicate，只共享`EAGAIN`分类与wait/recheck协议；notification不是truth；Unix首版无pending-error/error readiness | `SOCKET-UNIX-CUTOVER` |
| `UNIX-SOCKET-STATE-001` | Introduce | None（尚未生效） | endpoint role、listener/backlog、connection与directional stream各有唯一owner | `SOCKET-UNIX-CUTOVER` |
| `UNIX-SOCKET-STREAM-001` | Introduce | None（尚未生效） | connect/accept建立paired stream；send/receive、partial progress、peek、EOF、connected shutdown与SIGPIPE由directional owner提交；pre-connection shutdown返回`ENOTCONN` | `SOCKET-UNIX-CUTOVER` |
| `UNIX-SOCKET-NAMESPACE-001` | Introduce | None（尚未生效） | bind只提交socket kind与`0777` requested permission；task filesystem context、user-thread kernel creation operation与context-free VFS primitive沿用`VFS-CREATION-001`的umask/admission/formation/handoff owner；Unix只索引stable inode identity到live binding | `SOCKET-UNIX-CUTOVER` |
| `UNIX-SOCKET-ADDRESS-001` | Introduce | None（尚未生效） | bind-time immutable address snapshot独立于current namespace与binding key | `SOCKET-UNIX-CUTOVER` |
| `UNIX-SOCKET-LIFECYCLE-001` | Introduce | None（尚未生效） | socketpair/connect/accept/stream/final-release handoff、unlink independence与stale-generation isolation | `SOCKET-UNIX-CUTOVER` |
| `IOMUX-POLL-002` | Refine | [当前规则](../../contracts/iomux/poll-wait.md#iomux-poll-002--source-锁拥有-readiness-与-route-publication) | source-neutral readiness与route publication可独立承载receive-half-close category；具体predicate仍由source owner定义 | `SOCKET-UNIX-CUTOVER` |
| `IOMUX-POLL-003` | Refine | [当前规则](../../contracts/iomux/poll-wait.md#iomux-poll-003--wake-只是-hint最终-predicate-决定返回) | final scan按interest投影RDHUP并保持真实source HUP/ERR mandatory；Unix首版不制造ERR | `SOCKET-UNIX-CUTOVER` |
| `EPOLL-READY-001` | Refine | [当前规则](../../contracts/epoll/protocol.md#epoll-ready-001--bounded-exact-scan) | exact scan接入独立EPOLLRDHUP interest/delivery，不把兼容bit或source hint变成ready truth | `SOCKET-UNIX-CUTOVER` |

### Dependencies

- [`NET-PROTOCOL-BOUNDARY-001`](../../contracts/net/udp-socket.md#net-protocol-boundary-001--cross-owner-udp-capability保持窄且非阻塞)：UDP cross-crate capability与private Stack boundary不变。
- [`NET-SOCKET-ENDPOINT-001`](../../contracts/net/udp-socket.md#net-socket-endpoint-001--socket与endpoint保持owner-fence和单向association)：UDP迁移到general Socket front时保持Endpoint owner、retire与stale isolation语义。
- [`NET-UDP-TRANSACTION-001`](../../contracts/net/udp-socket.md#net-udp-transaction-001--bindsend与receive各自只有一个commit-boundary)：UDP datagram transaction不被Unix stream统一。
- [`NET-SOCKET-WAIT-001`](../../contracts/net/udp-socket.md#net-socket-wait-001--protocol-factwake与linux-readiness保持分离)：UDP作为新共同wait边界的真实consumer，但其predicate、writability与recheck语义不变。
- [`OPENED-DESC-001..003`](../../contracts/task/opened-description-lifecycle.md)：published-ref、dup/fork与static final-release hook继续有效。
- [`VFS-FILE-KIND-001`](../../contracts/vfs/file-kind.md#vfs-file-kind-001--inode-kind-是唯一-file-type-truth)：socket-kind只由immutable inode kind决定。
- [`VFS-CREATION-001`](../../contracts/vfs/make-node.md#vfs-creation-001--current-task-creation-policy-止于-kernel-operation)：pathname bind只提交socket kind与`0777` requested permission，复用task filesystem-context umask、operation-local credential/mask snapshot、create DAC与final metadata formation，不在Socket owner重复状态或策略。
- [`VFS-MAKE-NODE-001`](../../contracts/vfs/make-node.md#vfs-make-node-001--make-node-admission-与-backend-publication-只有一个-handoff)：context-free VFS primitive继续只消费显式final description并拥有backend/dentry handoff；pathname bind不扩张payload。
- [`IOMUX-POLL-001`](../../contracts/iomux/poll-wait.md#iomux-poll-001--阻塞前必须完成-snapshotregister-gate)：Unix source接入现有snapshot/register gate。
- [`EPOLL-WATCH-001`](../../contracts/epoll/protocol.md#epoll-watch-001--watch-ownership)与
  [`EPOLL-FILE-001`](../../contracts/epoll/protocol.md#epoll-file-001--non-sleeping-wait-publication)：watch/policy与epoll-file publication owner不变。

## Implementation Boundary

### 允许改变

- kernel Socket syscall/FileOps/private association，使 UDP 与 Unix stream 共同消费 general Socket front；
- Unix pathname namespace、endpoint/listener/connection/directional stream、blocking/I/O/readiness 与 lifecycle；
- `anemone-abi` / `anemone-rs` 中本 target 必需的 socket constants、struct 与 wrapper；
- 本表列出的 pending contract ID，以及与其直接对应的 current-contract cutover；
- 同 owner 内自然的模块注册、新文件、定向测试和行为保持型拆分。

### 必须保持

- UDP Stack Endpoint、binding/datagram transaction、network control plane与private engine的当前owner和可见语义；
- task filesystem-context umask、user-thread kernel creation operation的DAC/final formation、context-free VFS
  primitive与filesystem backend的inode/dentry、link/rename/unlink、make-node payload及common-create open issue owner；
- opened-description published-ref/final-release、fd flags/status与dup/fork sharing；
- iomux/epoll consumer policy、final predicate scan与notification-only语义；
- 本 RFC 的首版 UAPI、non-goals、failure/cleanup、validation strength与两个真实consumer closure claim。

### 实现自由度

`Socket` 额外字段、`SocketType` 编码、静态 descriptor 物理布局、`SocketOps` 精确字段名/签名与 capability absence
表达、resolver使用`match`还是表、Opaque helper、Unix object graph、hash bucket/storage、registration token、route
storage、锁、queue/buffer/capacity算法、pipe primitive复用程度与模块路径均是 implementation preference。重要容量
常量按仓库规则进入 kconfig；不得改变上文能力族、把 state-dependent failure 降成 permanent unsupported，或为了
避免自然 owner boundary 建立万能 trait、linear `Vec` namespace lookup、pathname-keyed runtime map或generic inode
attachment。

### 停止条件

- 需要改变本文 target/non-goals、operation/state owner、handoff、failure/cleanup、public ABI、Contract Impact、
  acceptance 或 validation claim；
- general Socket 需要解释 backend-private state、缓存 readiness/error 或让 syscall按concrete family downcast；
- Unix Socket 需要进入 network Stack、让 VFS持runtime binding payload，或新建generic VFS callback/rollback protocol；
- attempt/wait 拆分导致重复commit、无法保留partial progress、丢失wake、跨sleep泄漏family-private phase，且不能在
  当前target内自然修正；
- [首版 Linux conformance 验证面](./invariants.md#首版-linux-conformance-验证面)中的实现证据表明目标行为无法在
  既有owner、handoff、failure/cleanup与validation边界内自然实现，需要接受用户可见偏差、降低oracle或扩大scope；
- 工程退路超出本文已允许的“failed bind留下inert inode”边界，或会造成Unix registration/local-name半发布、stale
  identity命中新generation、UAF、final close后admission或用户可见成功伪装未支持语义。

## Acceptance 与 Validation

### Draft -> R0 文档接受

2026-08-02 已完成。R0 acceptance 已确认：

- 接受[目标与不变量](./invariants.md)中的 owner、namespace、stream、lifecycle 与 wait proof obligations；
- 确认pathname bind只提交socket kind与`0777` requested permission，现行`VFS-CREATION-001`的task
  filesystem-context、kernel creation operation与context-free VFS owner边界保持不变；connect只对resolved target
  检查`WRITE`，且blocking retry不复用旧DAC结果或binding capability；
- 确认 `SocketOps` 能力族、共同 FileOps lowering、永久 capability absence 与 state-dependent outcome 的边界；
- 确认首版排除`SO_ERROR`、Unix pending-error与error readiness，且未加入恒零query或预建error state；
- 确认[首版 Linux conformance 验证面](./invariants.md#首版-linux-conformance-验证面)是scoped target与实现期
  validation checklist，而不是六组待选语义：sockaddr/addrlen/copyout、connected unnamed bind与peer-close address
  visibility、listener/connect竞争、accept failure、stream partial/copy-fault，以及readable/writable/EOF/RDHUP/HUP；
- 确认 pending contract ID、cutover边界与非目标没有把现行 UDP/VFS/iomux/epoll 规则提前写成已改变；
- 完成文档 review，不存在未 neutralize 的 Apollyon/Keter。

R0 acceptance 只接受 target，不授权实现。

### R0 -> R1 Target Renegotiation

2026-08-02，Checkpoint 3A final review确认Linux 6.6.32 `unix_shutdown()`在没有peer时仍成功写入
`sk_shutdown`，该状态可跨unconnected/bound/listening role持续存在；完整兼容因此需要新增role-owned
pre-connection shutdown truth，并定义它对listen/connect admission、accepted child以及connection commit时direction
初始化的handoff。R0同时要求shutdown truth只由connected direction拥有，禁止endpoint-local副本，两者无法在原
Implementation Boundary内同时成立。

review比较了两条路线：扩大owner/handoff以实现Linux持久语义，或诚实收窄首版能力。维护者批准R1 reduced target：
unconnected、bound与listening Unix stream的合法shutdown稳定返回`ENOTCONN`，不改变任何role、admission或未来
connection。connected direction的shutdown、EOF、half-close、`EPIPE`/`SIGPIPE`与readiness target保持不变；唯一owner、
lifecycle与ABI containment correctness invariant不降低。Contract Impact种类和cutover保持不变，当前contract没有更新；
新的Linux兼容缺口进入register，3A validation增加三种role与错误优先级matrix。

### 最终 closure 证据

最终 `SOCKET-UNIX-CUTOVER` 至少需要：

- source/owner audit：raw Linux ABI containment、两个真实 Socket consumer、无通用层downcast、无第二namespace/
  readiness truth、无socket-local umask/anonymous-inode creation mode或DAC cache、无Unix pending-error state、
  final-release与stale-generation cleanup；
- owner-local KUnit/host proof：tuple resolver、socket/socketpair/accept fd rollback、`0777` requested-permission到
  `VFS-CREATION-001` production path的handoff、pathname identity与hard-link/unlink/rebind、connect attempt DAC
  lifetime、listener admission、stream partial/peek/EOF/shutdown/SIGPIPE、per-operation predicate与late hint；
- RV64 与 LA64 canonical release build；架构共享generated output的命令必须串行；
- RV64 与 LA64 guest runtime：socketpair，pathname server/client，bind parent search/write DAC、`umask 0027`下
  `0777` bind形成`0750` pathname mode、connect target write DAC、`chmod`后新连接拒绝且已连接stream继续，
  listen/connect/accept，read/write/vector I/O，nonblocking，
  dup/fork/final close，rename/unlink/rebind，poll/select/epoll与RDHUP/HUP；
- glibc 与 musl focused ABI/errno/addrlen/copy-fault matrix，以及真实现有程序或测例证明pathname stream有实际用途；
- UDP current regression，证明general Socket迁移没有改变现行UDP bind/send/receive/readiness/lifecycle；
- 对hardware、`smp>1`、full network/socket LTP、final harness等未执行范围明确记录Not Run，不用单架构build或
  socketpair代替pathname/runtime/架构证据。

`socketpair` 可以是早期 vertical slice，但不能单独支撑 RFC closure。最终 claim 必须同时证明 pathname namespace/
listener/unlink/lifecycle 和 UDP/Unix共同 Socket boundary。

## 风险与反馈

- **抽象过度：** 如果共同 front 产生无真实consumer的slot/class/registry，收窄到UDP与Unix共同需要的surface；TCP
  假设不能作为保留理由。
- **owner重复：** endpoint role、listener backlog、connection direction或readiness若在实现中出现在多份结构，立即
  停止并明确唯一owner，不以“共享状态”掩盖。
- **bind工程原子性：** 首选prepare-before-publish；若自然实现做不到，使用本文已定义的诚实退路并记录limitation，
  不扩张VFS或引入hack。
- **pathname DAC：** bind parent create admission、pathname inode metadata与connect target authorization都由VFS拥有；
  Unix registry不得检查或缓存permission，blocking retry不得复用旧attempt的授权。
- **wait模型：** 不同operation共享协议而不共享predicate；任何万能not-ready/attempt result都需要真实复用与race proof。
- **SO_ERROR首版排除：** 实现不得加入恒零query、pending-error/`has_error` state或Unix ERR投影；任何加入请求都必须
  由真实consumer触发并回到RFC Refine。
- **Linux conformance：** addrlen/copyout、connected unnamed bind、listener/connect与accept竞争、stream
  partial/copy-fault、peer-close address visibility与event mapping以Linux 6.6.32用户可见行为为scoped target；实现
  负责选择自然的内部形状和可证明linearization，并以focused oracle验证。若只能通过降低oracle或接受可见偏差才能
  前进，进入Target Renegotiation，不让代码偶然决定新语义。

## 文档与证据

- [目标与不变量](./invariants.md)
- [实施路线](./implementation.md)
- Current dependencies：
  [UDP Socket](../../contracts/net/udp-socket.md)、
  [Opened-description](../../contracts/task/opened-description-lifecycle.md)、
  [VFS Make Node](../../contracts/vfs/make-node.md)、
  [IOMUX-POLL](../../contracts/iomux/poll-wait.md)、
  [Epoll](../../contracts/epoll/protocol.md)
- Register：
  [VFS create publication atomicity](../../register/open-issues.md#ane-20260801-vfs-create-publication-atomicity)、
  [non-UTF-8 pathname](../../register/current-limitations.md#ane-20260801-vfs-non-utf8-pathname)、
  [retired-bind inert inode](../../register/current-limitations.md#ane-20260802-unix-bind-retired-inert-inode)、
  [pre-connection shutdown](../../register/current-limitations.md#ane-20260802-unix-preconnection-shutdown)
- External source evidence：`xref:linux-6.6.32:net/unix/af_unix.c`、`net/socket.c`、`net/core/sock.c`与`fs/select.c`
- commit / PR：Git/PR 保存 Stage 1 Checkpoint 1A/1B、Stage 2 Checkpoint 2A/2B与Stage 3 Checkpoint 3A实现、review和
  验证证据，以及Stage 3 resolution；transaction：None；cutover：None

## 修订记录

- **R1（2026-08-02）：** 接受connected-direction-only shutdown reduced target；unconnected、bound与listening
  role稳定返回`ENOTCONN`且不保存pending intent。该修订保持direction唯一truth与既有Contract Impact/cutover，新增
  register limitation和role/errno validation，不授权3B或current-contract cutover。
- **R0（2026-08-02）：** 接受本文 target、non-goals、owner/handoff、ABI、Contract Impact、acceptance 与 validation
  boundary；不授权 Stage 1 resolution、代码实现或 current-contract cutover。Draft 期间的措辞、证据和 review 修正
  由 Git 保存，不建立并列历史副本。

## Closure

Not Cut Over。Checkpoint 1A、1B、2A、2B与Stage 1/2已关闭；UDP与Unix `socketpair`共同证明front vertical slice，
single Unix Socket、pathname namespace/name与listener/connection admission已形成Stage 2完整vertical slice。Stage 3已
解析为3A directional stream operation/message-query ABI与3B listener/stream readiness两个checkpoint；3A已关闭，Stage 3
保持Active，3B Ready / Not Active。Stage 4仍未解析或激活，最终acceptance尚未运行。transaction与contract cutover保持
None，任何pending Socket/Unix/IOMUX/Epoll contract都尚未生效。
