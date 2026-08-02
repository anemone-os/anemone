# Socket Abstraction 与 Unix Socket 目标和不变量

**状态：** R1 Closed / Effective via `SOCKET-UNIX-CUTOVER`
**最后更新：** 2026-08-02
**父 RFC：** [RFC-20260801-socket-abstraction-and-unix-socket](./index.md)
**适用修订：** R1

本文保留本RFC已经接受并cut over的target、correctness invariants与RFC-local proof obligations；不承担current
contract权威。`SOCKET-UNIX-CUTOVER`已经完成，当前effective规则唯一见[Socket contract](../../contracts/socket/index.md)、
[IOMUX-POLL](../../contracts/iomux/poll-wait.md)与[Epoll Protocol](../../contracts/epoll/protocol.md)。完整UAPI包络、
Implementation Boundary、Contract Impact、acceptance与closure见父RFC及[实施路线](./implementation.md)；本页不复制
current contract正文或执行证据。

## 规则分类

- **Correctness Invariant：** state/owner 单一真相、ABI containment、lifecycle/stale isolation、Unix-owned
  publication fail-closed、wait final recheck 与 memory/resource safety；不能通过工程妥协降低。
- **Target Guarantee / Capability：** 首版 Unix pathname stream UAPI、general Socket 被 UDP/Unix 共同消费、
  byte-stream/half-close/readiness、两架构与双 libc closure；只能通过 Target Renegotiation 改变。
- **Engineering Boundary：** VFS node publication 与 Unix live-binding publication 不提供强跨 owner 原子性；若更强
  保证会扭曲代码或要求新 VFS protocol，可以采用本文明确的 inert-inode 退路并记录 limitation。普通 heap OOM
  沿用 kernel-fatal 边界，不提升为 Socket operation 的可恢复失败。
- **Implementation Preference：** Rust type、字段、descriptor layout、精确 slot 名称/签名与 capability absence 表达、
  object graph、lock、hash、queue/buffer、route storage、module/file 与精确命令；只要保持父 RFC 已固定的能力族和上述
  三类边界，可由实现自然决定。

Engineering Boundary 只能降低已明确列为 target/工程质量的 guarantee，不能把重复 truth、半发布 Unix state、UAF、
lost wake、stale generation 命中、资源泄漏或 ABI success bluff 改写成可接受限制。

## 状态所有权与生命周期

| 状态 / 关系 | 唯一 Owner | 生命周期 / commit | 非 owner 允许持有 |
| --- | --- | --- | --- |
| ops/type association | general `Socket` | Socket creation一次确定，semantic final release后不再使用 | immutable projection |
| type-private state | 对应 concrete `SocketOps` implementation / family owner | backend creation到family cleanup | opaque storage envelope |
| opened-description publication | `task::files` | `Unpublished -> Live(n) -> Retired` | transient FileDesc/lease、static final-release ctx |
| Unix endpoint role | 对应 Unix endpoint owner | unnamed/unbound到bound/listening/connected/terminal transition | typed operation capability/snapshot |
| pathname live registration | Unix pathname-binding registry | bind commit到semantic final release撤销 | operation-local binding capability |
| VFS pathname与inode topology | VFS/filesystem | create/link/rename/unlink/eviction | stable `InodeRef` identity capability |
| Socket local/peer address | 对应 endpoint immutable name fact | unnamed或一次bind/accepted initialization后稳定 | immutable name capability/snapshot |
| listener backlog/admission | 对应 listener state | listen commit到listener terminal close | connect/accept attempt outcome |
| peer relation | 对应 connection state | connect/socketpair commit到两端terminal | endpoint持connection capability |
| directional bytes/capacity/shutdown/EOF | 对应 connection direction | connection commit到direction terminal cleanup | operation-local snapshot/outcome |
| source route entries | 对应 concrete source registry | subscribe到consumer retirement/stale cleanup | non-owning `PollRoute` |
| blocking wait round | Socket/syscall + scheduler wait owner | attempt/register/recheck到return/cancel | family提供predicate/attempt，不持waiter |
| epoll watch/delivery | `Epoll` / `EpollWatch` | ADD到DEL/final-close，按current contract | target snapshot与non-owning observer |

不存在“Socket、Unix backend、VFS、wait共同拥有”的 mutable fact。跨 owner 的 capability、snapshot、typed outcome 与
notification 只服务明确 handoff，不能反向推进原 owner state。

## Target Invariants

### SOCKET-FRONT-001 — General Socket 只拥有 immutable dispatch envelope

**分类：** Correctness Invariant / Target Guarantee。

**规则：** 每个 kernel-visible Socket file 通过同一 general `Socket` front 接入共同 FileOps 与 Socket syscall
dispatch。`Socket` 在 creation 时一次关联一份静态 `SocketOps` 和对应 type-private storage；该 association 在
Socket lifetime 内不可替换。每份静态 ops 唯一见证一个 resolved semantic `SocketType`，因此 `Socket` 不另存
family、Linux base type、protocol 或第二份 type tag。

general `Socket` 不拥有 endpoint identity、namespace、listener/backlog、connection、buffer、operation phase、
readiness 或 pending error。`Opaque` 只擦除 storage representation；只有关联的 concrete ops implementation 可以解释
自己的 private storage。共同 syscall/FileOps/opened-description/iomux/epoll 不得 downcast、按 concrete family 分支或
取得 backend-private object/lock。

UDP 与 Unix stream 是本 target 的两个真实 consumer。共同 surface 只保留本 target 的 type/create、lifecycle、
name/state transition、observation、send/receive 与 readiness 能力族，每项能力至少由其中一个真实 consumer要求；
只属于一种 family 的 operation state、algorithm、failure与transaction继续留在family owner。并非每种 type 都成功
支持每项 operation：永久 capability absence 由descriptor或typed unsupported表达，state-dependent rejection/not-ready
由对应family operation的typed outcome表达；共同层不得通过downcast补足区别。静态function复用不要求多type共享
一份descriptor，也不建立动态class registry。

共同 FileOps 的 read/write/vector I/O 归一到 send/receive，poll 归一到 source snapshot/register/recheck；它们不形成
第二份 backend vtable。immutable `SO_DOMAIN/SO_TYPE/SO_PROTOCOL` 直接从 ops 的 type witness 投影；首版只有
`SO_ACCEPTCONN` 需要 concrete ops 的 runtime role query，UDP稳定回答false，Unix从listener role truth派生。首版
没有成功 mutable option，因此不得预建通用 option bag 或 family-owned `set_option` state。

**Owner：** general Socket owner只拥有front envelope与family-neutral projection；concrete family owner拥有private
state与operation semantics。

**依赖：** `OPENED-DESC-001..003`、`NET-PROTOCOL-BOUNDARY-001`。

**违反表现：** UDP/Unix继续各有一套由syscall识别的file kind；`Socket`同时保存ops与可变family tag；通用层
downcast Opaque；ops动态替换；Socket缓存backend readiness/error；把state-dependent failure伪装成永久unsupported；
为每个FileOps入口复制backend slot；建立无真实consumer的万能trait/class/Endpoint registry或option bag；或Unix
operation进入network Stack。

**Cutover / Proof：** `SOCKET-UNIX-CUTOVER`；source audit必须证明UDP与Unix共同消费front且保持各自owner fence。

### SOCKET-ABI-001 — Linux Socket ABI 在 resolver/adapter 边界完成归一化

**分类：** Correctness Invariant / ABI Correctness。

**规则：** raw `family/type/protocol`、`SOCK_*`/`MSG_*` number、`sockaddr_*` layout、addrlen、user pointer、copy
ordering与Linux errno只存在于Socket ABI adapter。创建resolver验证unknown bits与unsupported组合，分离
opened-description status/fd-local flag，并把每个合法tuple归一为一份static ops association；归一后raw tuple与默认
protocol spelling不得保存到Socket或backend。

`SocketType`表示resolved semantic type，例如IPv4 UDP或Unix stream，不表示可自由组合的Linux三元组。`SO_DOMAIN`、
`SO_TYPE`与`SO_PROTOCOL`从ops见证的semantic type反向投影；显式default protocol与`protocol == 0`若解析到同一
semantic type，不产生两种creation identity。

首版`SO_ERROR`不属于支持的query；ABI adapter对`getsockopt(SOL_SOCKET, SO_ERROR, ...)`返回`ENOPROTOOPT`，不得
返回恒零成功或为它建立family dispatch/pending state。未来加入该能力必须先通过RFC Refine接受producer、owner、消费
与readiness语义。

内部operation使用typed request/context/outcome。family owner不得接收user pointer、raw sockaddr、fd number、完整
Task或Linux errno；ABI adapter不得通过private state判断未被typed outcome表达的family phase。

**Owner：** Socket syscall/ABI adapter拥有Linux representation与errno mapping；static ops association拥有immutable
semantic type witness；family owner只拥有normalized operation semantics。

**依赖：** `SOCKET-FRONT-001`、current `OPENED-DESC-001..003`。

**违反表现：** backend解析rawLinux tuple/flags；Socket缓存原始protocol；`SO_TYPE`从用户输入回显而非semantic type
派生；两个adapter对alias/default产生不同identity；ops request携带user pointer/errno；或syscall按Opaque downcast决定
错误。

**Cutover / Proof：** `SOCKET-UNIX-CUTOVER`；resolver/source audit与双架构、双libc ABI/errno matrix共同证明。

### SOCKET-WAIT-001 — Operation predicate 各自拥有，共享协议不共享 fact

**分类：** Correctness Invariant / Target Guarantee。

**规则：** connect、accept、send、receive分别读取对应operation/state owner定义的completion/not-ready predicate。
listener admission、backlog non-empty、direction capacity、data/EOF等事实不能合并为一份common `ready`/`would_block`
state，也不能由general Socket、wait、iomux或epoll统一拥有。

这些operation可以共享Linux-visible `EAGAIN`分类，以及
`attempt -> snapshot/register -> recheck/final scan -> retry/return`协议。opened-description `O_NONBLOCK`、creation/
accept flag与per-call `MSG_DONTWAIT`只决定当前operation是否等待，不修改family predicate。blocking path不得busy-poll；
signal、timeout、force、notification或register abort只结束/提示本轮，最终返回前仍由当前operation predicate和typed
outcome裁决。

readiness同样只投影owner facts：readable、writable、EOF、receive half-close与HUP不得缓存为共同mask。notification
是non-owning recheck hint。receive half-close与完整HUP使用独立source-neutral category；poll/epoll按interest交付
RDHUP，HUP保持mandatory，ordinary readable仍可由EOF满足。

首版Unix state没有pending-error producer，Unix poll/epoll source不得发布ERR。current iomux/epoll对其它真实source
error的mandatory ERR规则保持不变；本排除不能削弱已有consumer policy，也不能用恒零`SO_ERROR`或独立`has_error`
伪装兼容。未来Refine若加入pending error，必须重新闭合非消费readiness观察与I/O/`SO_ERROR`原子消费顺序。

**Owner：** 每个predicate由对应listener/connection/directional/family state owner拥有；Socket/syscall owner拥有blocking
choice与operation wait loop；iomux/epoll拥有consumer protocol/policy。

**依赖：** `IOMUX-POLL-001..003`、`EPOLL-WATCH-001`、`EPOLL-READY-001`、`NET-SOCKET-WAIT-001` current baseline。

**违反表现：** 四类operation共享一个not-ready bit；wake payload直接成为errno/readiness；MSG_DONTWAIT改写status；
Socket缓存ready mask；RDHUP借用HUP；EOF不再ordinary readable；Unix source制造无producer的ERR/pending error；
未armed source进入sleep；或取消一个waiter撤销其它consumer route。

**Cutover / Proof：** `SOCKET-UNIX-CUTOVER`；per-operation predicate/state audit、lost-wake/late-hint tests、
poll/select/epoll与blocking/nonblocking runtime共同证明。

### IOMUX-POLL-002 Target Refine — Receive half-close拥有独立source-neutral category

**分类：** Correctness Invariant / Shared Contract Refine。

**规则：** source-neutral `PollEvent` vocabulary必须把receive half-close与完整HUP表达为两个独立category。Unix
directional owner提交peer write-side shutdown/EOF等family fact；Unix poll source只从这些current facts投影
receive-half-close，不以HUP、notification、route presence或历史edge代替。该category只在consumer请求时作为普通
interest交付；HUP与ERR继续按Linux iomux规则mandatory。

支持该category的source仍受同一个source-state publication协议约束：相关predicate变化必须先由owner提交truth并
取得需要通知的route snapshot，再在guard外notify/drop。增加category不得让iomux取得Unix directional state、缓存
ready mask或让一个consumer的取消撤销其它route。

**Owner：** concrete source state拥有predicate与route publication；iomux只拥有source-neutral category和wait协议。

**依赖：** current `IOMUX-POLL-002`、`SOCKET-WAIT-001`、`UNIX-SOCKET-STATE-001`。

**违反表现：** `POLLRDHUP`继续借用HUP；source bridge保存第二份half-close bit；notification直接携带RDHUP结果；
或新增category绕过source lock的snapshot/register publication规则。

**Cutover / Proof：** `SOCKET-UNIX-CUTOVER`；PollEvent/source audit、half-close与full-close predicate transition、
register-window和multi-consumer runtime共同证明。

### IOMUX-POLL-003 Target Refine — Final scan分别投影RDHUP与mandatory HUP/ERR

**分类：** Correctness Invariant / ABI Correctness / Shared Contract Refine。

**规则：** poll/ppoll final scan只能在用户请求`POLLRDHUP`且source当前receive-half-close predicate成立时返回
`POLLRDHUP`；完整HUP不能冒充RDHUP，RDHUP也不能吞掉mandatory `POLLHUP`/`POLLERR`。EOF仍满足ordinary readable，
因此select/pselect6 readfds不依赖一个select不可表达的RDHUP类别前进。

route hint、timeout、signal、force或register abort仍只触发final scan；历史RDHUP hint不能直接成为revents。首版Unix
source没有error producer，final scan不得制造恒定或无producer的ERR；current mandatory ERR规则继续适用于实际报告
error的其它source。

**Owner：** source owner拥有current facts；iomux wait round拥有final scan；poll/select ABI adapter拥有Linux投影。

**依赖：** current `IOMUX-POLL-003`、`IOMUX-POLL-002` Target Refine、`SOCKET-WAIT-001`。

**违反表现：** 请求RDHUP即返回ready；peer receive half-close被投影成完整HUP；final scan漏掉mandatory HUP/ERR；
EOF不再使readfds前进；或hint payload直接写入revents。

**Cutover / Proof：** `SOCKET-UNIX-CUTOVER`；requested/unrequested RDHUP、EOF、full close、HUP、no-spurious-ERR与
race matrix。

### EPOLL-READY-001 Target Refine — EPOLLRDHUP成为真实interest与exact-scan结果

**分类：** Correctness Invariant / ABI Correctness / Shared Contract Refine。

**规则：** `EPOLLRDHUP`不能继续只是无source truth的兼容bit。epoll watch把它保存为独立delivery interest，source
route与bounded exact scan能够观察独立receive-half-close category，并且只在watch请求时交付`EPOLLRDHUP`。
`EPOLLHUP`继续mandatory，ordinary `EPOLLIN`仍可由buffered data或EOF满足；`EPOLLERR`对实际source error仍然
mandatory，但首版Unix source没有error producer，不得制造ERR。

LT/ET/ONESHOT、sticky dirty、generation、bounded fairness、copyout rollback与target final-liveness继续由current
epoll owner和`EPOLL-READY-001`协议裁决；新增event不能成为ready queue/bitmap，也不能让compat bit或callback hint
绕过exact predicate scan。

**Owner：** target source拥有readiness truth；`EpollWatch`拥有interest/policy；`EpollOperation`拥有exact scan与
delivery commit。

**依赖：** current `EPOLL-READY-001`、`EPOLL-WATCH-001`、`IOMUX-POLL-002/003` Target Refine。

**违反表现：** 接受`EPOLLRDHUP`但永不产生；未请求也交付RDHUP；RDHUP与HUP共用一bit；callback payload直接进入
epoll_event；或copyout fault/ONESHOT race丢失原有rollback义务。

**Cutover / Proof：** `SOCKET-UNIX-CUTOVER`；LT/ET/ONESHOT、requested/unrequested RDHUP、HUP mandatory、
no-spurious-ERR、copyout rollback与source/close race matrix。

### UNIX-SOCKET-STATE-001 — Endpoint role、listener、connection 与 direction 各有唯一 owner

**分类：** Correctness Invariant。

**规则：** 每个Unix Socket endpoint只有一个authoritative role/association source，表达当前unnamed/unbound、bound、
listening、connected与terminal关系。role选择可以由一个enum、capability association或等价自然结构编码，但不得由
多个boolean、Socket front、listener object与connection object并列决定。

进入listening后，对应listener state唯一拥有backlog capacity、queued accepted endpoint与admission predicate。
connection commit后，对应connection state唯一拥有paired endpoint relation；两个directional stream分别拥有各自
byte sequence、capacity、write-side shutdown、EOF与terminal condition。一个direction的facts不能从另一direction或
fd/opened-description状态反推。

`SO_ACCEPTCONN`读取endpoint role/listener association，不写入immutable Socket type metadata。未连接错误由role/
connection owner决定，不由local-name是否unnamed反推。Socketpair直接形成connected role，但unnamed address不因此
等于unconnected。

R1不在endpoint role或listener保存pre-connection shutdown intent。unconnected、bound与listening role没有可提交
terminal transition的direction，对三个合法`how`均返回`ENOTCONN`且保持role/admission不变；只有connection commit
后的direction可以拥有和推进shutdown/EOF fact。

**Owner：** Unix endpoint role owner、listener state、connection state与每个directional stream按上文分别唯一拥有。

**依赖：** `SOCKET-FRONT-001`、`OPENED-DESC-001..003`。

**违反表现：** Socket front与backend各有connected/listening bit；backlog同时存在listener和wait queue；两端各自
复制peer relation并可独立修改；half-close从fd count推导；local-name决定connection state；在endpoint/listener保存
pre-connection shutdown bit或用success-no-op伪装该排除面；或diagnostic id驱动role。

**Cutover / Proof：** `SOCKET-UNIX-CUTOVER`；object/state transition audit与concurrent bind/listen/connect/accept/
shutdown/close tests。

### UNIX-SOCKET-STREAM-001 — Connection 与 directional owner提交 byte-stream 语义

**分类：** Correctness Invariant / Target Guarantee。

**规则：** socketpair或successful connect admission只提交一条paired connection；pathname connect还必须形成一个由
listener backlog拥有、供accept消费的accepted endpoint。非阻塞connect在本revision不提交异步`EINPROGRESS`
intent；owner predicate not-ready返回`EAGAIN`。blocking connect的重试不能重复创建connection或backlog entry。

send/write只向outbound direction提交有序byte prefix，允许partial progress；receive/read只从inbound direction消费
有序byte prefix，`MSG_PEEK`观察而不消费。data/capacity handoff每一时刻只有一个访问owner，copy fault、partial
progress与retry不能重复字节、重排或让两端同时拥有同一buffer region。copy-fault consume的用户可见结果服从父RFC
的scoped Linux conformance；内部copy chunk与cursor不提前冻结。

connected stream的peer write-side shutdown或terminal close在inbound direction形成durable EOF；buffered data先于EOF被读取。local
`SHUT_WR`阻止新的outbound commit并使peer最终观察receive half-close；`SHUT_RD`、`SHUT_RDWR`与close按scoped Linux
conformance推进directional facts。peer不再接收时send/write返回`EPIPE`，未带`MSG_NOSIGNAL`还产生`SIGPIPE`；
signal publication不能发生在family state guard内，也不能把失败写成zero-length success。

首版不建立一次性reset/pending-error latch。listener close清理queued-but-unaccepted connection，或endpoint close
丢弃该endpoint尚未读取的inbound bytes时，listener/connection/directional owner只提交既有terminal transition；存活
endpoint观察EOF/RDHUP/HUP与后续`EPIPE`/`SIGPIPE`，已经排队到其inbound direction的数据仍先于EOF交付。这些路径
不产生Linux的一次性`ECONNRESET`、`SO_ERROR`或ERR readiness。

Unix stream可以复用或提取现有IPC primitive，但不能仅把两根pipe拼接后让pipe owner、Socket owner与connection
owner同时决定peer、shutdown、HUP或close。

**Owner：** listener owner提交admission/backlog；connection owner提交pair relation；对应direction owner提交
bytes/capacity/shutdown/EOF；Socket ABI层只映射typed outcome与signal/errno。

**依赖：** `UNIX-SOCKET-STATE-001`、`SOCKET-WAIT-001`。

**违反表现：** blocking retry生成duplicate connection；accept取得无backlog owner的endpoint；partial send重复bytes；
peek消费；EOF越过buffered data；两根pipe各自复制peer close；shutdown state在Socket与stream各一份；EPIPE无SIGPIPE
或`MSG_NOSIGNAL`仍发signal；首版为close路径私建reset/pending-error state。

**Cutover / Proof：** `SOCKET-UNIX-CUTOVER`；socketpair/pathname connection、partial/peek/copy-fault、connected shutdown/
EOF/SIGPIPE、unconnected/bound/listening `ENOTCONN`、listener-abort、discarded-unread-data close与concurrent close runtime matrix。

### UNIX-SOCKET-NAMESPACE-001 — Stable VFS inode identity索引live binding

**分类：** Correctness Invariant / Target Guarantee。

**规则：** task filesystem context唯一拥有process umask；user-thread kernel creation operation取得operation-local
credential/mask snapshot并拥有pathname resolution、directory search/create DAC与final permission/owner formation；
context-free VFS primitive与filesystem backend拥有mount invariant、inode/dentry publication、link/rename/unlink与
immutable `InodeType::Socket`。Unix pathname-binding registry唯一拥有stable inode identity到current live binding的
mapping；它是admission capability index，不是第二份pathname namespace。

bind使用调用task的filesystem context解析parent，路径前缀服从directory search，final parent必须通过
`WRITE | EXECUTE` DAC且mount可写；任何已存在的final entry映射为`EADDRINUSE`，创建socket-kind不额外要求
`CAP_MKNOD`。Unix只向user-thread kernel creation operation提交`InodeType::Socket + 0777` requested permission；
fsuid/fsgid、SGID parent inheritance、process umask与最终permission沿用`VFS-CREATION-001`的现行owner/handoff。
Socket/Unix owner不得读取、缓存或应用umask，也不得以anonymous Socket file inode permission或
`fchmod(socket_fd)`结果作为pathname创建输入。

registry使用完整inode object identity判等；hash可以使用ino分桶，但裸ino不能跨superblock、eviction或reuse作为key。
live registration持`InodeRef`或等价窄identity/lifetime capability，保证binding存活时identity不被reload替换。registry
不保存pathname、不执行lookup/DAC、不修改dentry，也不把runtime state挂入generic inode ops或backend `prv`。

每次connect attempt都先由VFS完成普通lookup与DAC：路径前缀要求directory search，resolved target只要求`WRITE`，
不要求target `READ`/`EXECUTE`或final parent `WRITE`。DAC success只授权本次attempt对该resolved inode identity继续
执行socket-kind检查、registry查询与listener admission；not-ready attempt进入blocking wait后必须丢弃该授权与binding
capability，下一次attempt重新lookup和检查DAC。同一次attempt内并发`chmod`/`chown`按DAC检查的先后形成合法结果，
不得为此跨VFS与Unix owner持锁。

pathname inode当前permission/owner是新连接DAC的唯一truth。`chmod`/`chown`以及解析到同一inode的hard-link alias
影响后续attempt，但不撤销已经commit的connection，也不触发listen/accept或connected stream I/O重复检查。普通
mknodat/reboot恢复/final-close残留的socket inode没有registration，不能恢复旧Socket；同名rebind形成新identity/
generation，旧entry或late cleanup不能命中新binding。

unlink只撤销一个VFS name；hard-link alias若仍解析到同一inode，应继续命中同一live binding。pathname unlink不
终结listener/connection，Socket final close也不自动unlink。

**Owner：** task filesystem context拥有umask；user-thread kernel creation operation拥有current-context lookup、DAC与
final formation；context-free VFS/filesystem拥有namespace primitive、publication与inode identity；Unix binding
registry拥有live mapping；Unix listener/connection owner拥有runtime state。

**依赖：** `VFS-FILE-KIND-001`、`VFS-CREATION-001`、`VFS-MAKE-NODE-001`、`UNIX-SOCKET-STATE-001`。

**违反表现：** pathname-keyed runtime map；linear Vec lookup作为长期namespace；裸ino跨sb判等；inode `prv`
保存Socket pointer；VFS lookup回调Unix；Socket读取/缓存umask、重复应用mask、绕过current kernel creation
operation或用anonymous inode mode决定pathname mode；registry执行DAC；connect retry跨wait复用旧authorization；
close自动unlink；rename使binding失效；旧generation cleanup撤销新binding；或mknodat socket inode自动成为listener。

**Cutover / Proof：** `SOCKET-UNIX-CUTOVER`；`0777` requested-permission进入`VFS-CREATION-001` production path、
`umask 0027`形成`0750` pathname mode、bind parent search/write DAC、connect target write-only DAC、`chmod`后新旧
connection分离、blocking retry recheck，以及ext4/ramfs identity、hard-link/rename/unlink/rebind、reload/inert inode与
stale-generation tests。通用umask owner/lifecycle由current VFS contract证明，本RFC只证明新consumer接线。

### UNIX-SOCKET-ADDRESS-001 — Address snapshot 与 namespace/binding truth 分离

**分类：** Correctness Invariant / ABI Correctness。

**规则：** 每个Unix Socket endpoint的Linux-visible local-name从unnamed开始，并最多通过一次successful bind transition
形成immutable pathname snapshot；accepted endpoint可以从listener已提交的snapshot初始化。snapshot保存ABI adapter
归一化后足以重建地址的bind-time pathname spelling，不保存raw user pointer、完整sockaddr padding，也不从resolved
`PathRef`生成absolute/canonical path。

snapshot只服务`getsockname`、connected peer的`getpeername`、accept peer address、connected-stream recvfrom source
address与必要诊断。rename、hard-link alias与unlink不改写snapshot；同名rebind可以得到相同bytes但不同inode identity/
binding generation。snapshot不得参与lookup、admission、hash key、unlink或binding cleanup。

peer address通过connection capability观察peer endpoint唯一拥有的name fact，不保存connect调用者使用的pathname
alias。accepted endpoint共享listener name capability但不继承live binding registration。socketpair与未绑定peer保持
unnamed。peer close后的可观察期与connected unnamed Socket后续bind服从父RFC的scoped Linux conformance；内部
capability lifetime可以自然选择，但不能复制address truth或延长binding admission。

**Owner：** 对应Unix endpoint拥有immutable name fact；VFS继续拥有current namespace；binding registry只拥有identity
index。

**依赖：** `SOCKET-ABI-001`、`UNIX-SOCKET-NAMESPACE-001`、`UNIX-SOCKET-STATE-001`。

**违反表现：** getsockname重新查询VFS；rename改变reported address；hard-link alias覆盖bind spelling；connect-time
alias成为peer truth；snapshot作为map key或cleanup path；accepted Socket复制listener registration；或address bytes
决定connected state。

**Cutover / Proof：** `SOCKET-UNIX-CUTOVER`；sockaddr/addrlen、relative/rename/link/unlink/rebind、accepted/unnamed/
peer-close address runtime matrix。

### UNIX-SOCKET-LIFECYCLE-001 — Publication、final release与late capability fail closed

**分类：** Correctness Invariant / Target Guarantee。

**规则：** `task::files` published-description lifecycle仍是semantic final release的唯一trigger。dup/fork aliases共享同一
description；关闭非最后alias不得撤销Unix endpoint、binding、listener或connection。final-release hook只取得窄ctx，
不通过raw fd、Arc count、File Drop或path lookup决定liveness。

Unix final release先撤销新operation可取得的endpoint/binding/source publication，使已经取得的operation-local
capability在commit前revalidate并fail closed；再撤销route/observer并在guard外notify/drop；最后释放inode identity、
listener/connection/directional resources。cleanup不等待waiter运行，也不允许late hint/old capability恢复association或
命中新generation。

socketpair的两个fd在一次可回滚transaction中publication；任一preparation/user copy failure不留下half pair或published
slot。accept取得的Socket在fd commit前unpublished；accept failure必须由一个transaction owner回滚或fail-forward，不能
泄漏accepted endpoint、重复backlog item或发布无返回fd。accept copyout/consume/fd publication的用户可见结果服从
父RFC的scoped Linux conformance；内部transaction carrier与rollback次序由实现自然选择。

unlink与Socket lifecycle保持正交：unlink不关闭live endpoint，final release不自动unlink。accepted endpoint可以持
listener local-name capability用于观察，但不持listener registration或延长其admission。

**Owner：** `task::files`拥有description lifecycle；Unix endpoint/listener/connection/source owner各自执行本地withdraw/
cleanup；Socket creation/socketpair/accept transaction分别拥有publication rollback。

**依赖：** `OPENED-DESC-001..003`、`UNIX-SOCKET-STATE-001`、`UNIX-SOCKET-NAMESPACE-001`、`SOCKET-WAIT-001`。

**违反表现：** close一个dup提前retire；File Drop触发unlink；final release等待worker/waiter；route晚到恢复binding；
old capability连接到新inode generation；socketpair只发布一个fd；accept fault泄漏child或duplicate backlog；accepted
Socket继承listener registration。

**Cutover / Proof：** `SOCKET-UNIX-CUTOVER`；dup/fork/CLOEXEC/exit/final-close、socketpair fault、accept fault、late
admission/hint与identity reuse tests。

## Heap OOM 与 normal resource exhaustion

普通 `Box` / `Arc`、collection growth 以及 owner-local node/buffer allocation 沿用当前 kernel-fatal OOM 边界；allocator
OOM 可以 panic，不映射为 `ENOMEM`、`ENOBUFS`、`EAGAIN` 或 family-private error，也不进入 socket/socketpair/bind/
connect/accept/I/O transaction 的 normal rollback。host/KUnit/guest acceptance 不要求 allocator failure injection 或
OOM recovery。

本 RFC 不为这种 OOM recovery 预先引入 intrusive collection、preallocated pool、fallible mirror、duplicate index、
额外 reservation/rollback state 或 allocation-free operation surface，也不要求把普通分配从其自然 state owner、
object graph 或 lifecycle 中搬走。execution context 若本身禁止分配或 sleep，仍必须由对应路径满足；kernel-fatal OOM
不能豁免唯一 owner、memory safety、publication、late-capability isolation 或普通成功/失败路径的 cleanup。

该边界不覆盖协议与 ABI 已明确建模的 normal resource exhaustion。fd slot reservation、listener backlog、directional
buffer/capacity、binding/admission limit、用户提供 length/count 的 supported bound 与 integer overflow，以及 user copy、
VFS lookup/create 或其它可返回 operation failure，仍必须给出 typed outcome/errno，并在对应 commit 前拒绝或按本文
定义的 transaction 规则 rollback。不得把这些可预期失败伪装成 allocator OOM 或 panic。

## RFC-local Proof Obligations

### UNIX-PUBLICATION-001 — Bind weak atomicity必须诚实且保持Unix state fail closed

**分类：** Correctness Invariant（Unix-owned state）/ Engineering Boundary（跨VFS原子性）。

**规则：** bind的首选路线在VFS make-node前以自然代码形状完成local-name preparation、会正常返回错误的capacity/
state admission与同Socket operation serialization；VFS成功返回后使用stable identity提交Unix registration与
local-name，成功返回前两者均可见。本R0 target允许VFS node可lookup但live binding尚未发布的短窗口；并发connect
可以失败。普通heap allocation服从上文kernel-fatal OOM边界，不要求仅为OOM recovery把registration resource前置或
改变自然对象图。

“VFS create成功后绝不返回普通错误”不是要求以任意工程代价满足的correctness invariant。若消除可返回的
post-create failure需要长期跨VFS operation持Unix global lock、扭曲object graph、proof-only state/adapter、复制
VFS truth，或新增/修改VFS common-create/rollback protocol，可以选择不实现强原子性。该路径允许bind返回error并
留下inert socket inode，但必须满足：Unix registration与local-name都未发布；所有unpublished resource释放；inode没有
live binding；connect失败；
调用者显式unlink后才能复用pathname；final close不自动unlink。

该工程退路必须由live failure/cost evidence触发，并在RFC closure/current limitation记录具体failure point、errno与
visible residue。不得用重新解析pathname的best-effort unlink冒险删除rename/rebind后的其它inode，也不得把VFS现有
backend/cache/dentry window当作Unix rollback证明。

无论是否采用退路，stale identity、duplicate live publication、registration/local-name最终分裂、late admission、
UAF或resource leak都是correctness bug，不在弱原子边界内。

**Owner：** VFS拥有node publication；Unix bind transaction拥有preparation与Unix-owned commit/abort；existing VFS
common-create issue继续由VFS register owner承担。

**依赖：** `VFS-CREATION-001`、`VFS-MAKE-NODE-001`、`UNIX-SOCKET-NAMESPACE-001`、
`UNIX-SOCKET-ADDRESS-001`。

**违反表现：** 为atomic bind扩张MakeNodeDescription payload；持global registry lock睡眠做VFS lookup；failed bind留下
half registration；path-based compensation删错inode；退路未记录却宣称Linux atomicity；或把inert inode恢复成旧Socket。

**Cutover / Proof：** RFC-local；source/failure-path audit与实际采用路线的targeted runtime/fault evidence。

### SOCKET-ATTEMPT-001 — Readiness-nonblocking attempt 是可证伪的默认路线

**分类：** RFC-local Implementation Preference / Stop Condition。

**规则：** 对完成条件依赖peer/data/capacity/backlog等可变fact的operation，默认由family op提供不拥有task wait loop的
单次attempt/reconcile；上层Socket/syscall组合blocking choice、signal、timeout、registration与final recheck。这里的
nonblocking只表示ops不等待readiness改变，不禁止普通mutex、VFS lookup或其它owner-local sleep。

family operation仍唯一拥有一次attempt内的state transition、admission、commit、partial progress、rollback与typed
outcome。accept、send/receive与connect不需要共享万能attempt result；只有真实重复义务才能提取共同类型。

若live implementation证明attempt/wait拆分必须跨sleep持有family transaction/guard，否则会重复commit、无法rollback、
丢失partial progress、泄露private phase或破坏register/final-scan，则必须在固化例外前回到RFC review。不得让ops静默
拥有隐藏wait loop，也不得让通用层解析private state来弥补。

**Proof：** 每个waitable operation的owner/predicate/commit/partial-progress/cancel table在对应实现前完成；不需要为此
预建implementation.md。

### SOCKET-CONSUMER-PROOF-001 — 两个真实 consumer共同证明抽象

**分类：** Target Acceptance Obligation。

**规则：** final closure必须同时证明：

- UDP在general Socket迁移后保持current Endpoint/bind/send/receive/readiness/final-release语义；
- Unix Socket完成socketpair和有实际用途的pathname bind/listen/connect/accept/stream/unlink/half-close路径；
- common front/ABI/wait surface确实被两者消费，family-specific state没有被提升为通用truth。

socketpair-only、Unix-only、build-only或目录整理均不能证明Socket Abstraction closure。单架构runtime不能替代另一架构，
poll/select不能替代epoll，build/KUnit不能替代guest ABI/stream behavior；未运行项按Not Run记录。

**Proof：** 父RFC Acceptance矩阵、source audit、双架构/双libc runtime与UDP regression；证据保存在Git/PR或未来
closure，不因本义务自动创建transaction。

## 首版 Linux conformance 验证面

以下六组不是R0 Review Hold，也不是授权代码按便利选择外部语义。对于父RFC明确支持且未由non-goal、current
limitation或显式例外排除的行为，Linux 6.6.32用户可见结果是scoped target；tracked Linux source与focused Linux
runtime共同补足UAPI header和man page未完整规定的copy side effect、race与lifecycle oracle。`SO_ERROR`已经选择首版
排除路线，不属于这里的默认兼容范围。

每组实现仍必须能够说明相关fact与transaction的唯一owner、linearization/commit/consume或revalidation边界，以及
failure、signal、close与copy fault后的cleanup或fail-forward。竞态可以由实现自然选择的线性化先后形成Linux允许的
一组结果，不要求脱离commit point指定固定调度winner。内部Rust type、锁、对象图、queue/buffer/cursor、helper、fd
reservation物理表达、copy chunk和精确模块路径继续是implementation preference。

实现只需在对应路径落地前建立足以证伪ABI bluff的focused oracle和测试，不需要把Linux内部实现抄成Anemone target，
也不需要把每项实现发现回写成RFC新规则。若live evidence表明兼容目标工程代价过高，或需要改变owner、handoff、
failure/cleanup、ABI、acceptance或validation strength，必须在合入较弱行为前进入Target Renegotiation。

### `sockaddr_un` input/output与copyout

**自然落点：** `SOCKET-ABI-001`；若copyout附带accept dequeue、stream consume等state-changing side effect，对应
transaction rule同时拥有rollback或fail-forward，ABI adapter不能替family/lifecycle owner决定。

**规范目标：** input addrlen与NUL boundary、output actual length与truncation、地址与addrlen的copy ordering、partial
copy fault的可见用户内存和errno，以及各state-changing operation在copy fault时已经commit/consume的可见结果，
均与scoped Linux oracle一致。

**实现自由度：** user-copy helper、临时buffer、预校验方式与内部typed address representation；这些选择不得把
non-atomic copyout伪装为全有或全无，也不得把同一copy helper扩张为跨operation transaction owner。

### Connected unnamed bind与peer-name可观察期

**自然落点：** `UNIX-SOCKET-STATE-001`、`UNIX-SOCKET-ADDRESS-001`与`UNIX-SOCKET-LIFECYCLE-001`。

**规范目标：** connected but unnamed endpoint后续pathname bind、peer final close后`getpeername`与其它name观察的
用户可见结果均与scoped Linux oracle一致；local-name fact仍必须与connected role正交，不形成第二份role truth。

**实现自由度：** name capability是共享snapshot、owner-local immutable object还是等价窄表示；不得用延长binding
admission或peer liveness来换取地址观察，也不得让address bytes反向决定connected state。

### Listener admission与blocking connect竞争

**自然落点：** `UNIX-SOCKET-STATE-001`、`UNIX-SOCKET-STREAM-001`与`SOCKET-ATTEMPT-001`。

**规范目标：** blocking connect与listener close、signal、capacity变化竞争产生的success/error/errno必须属于scoped
Linux oracle允许的结果；实现必须说明backlog/admission commit point、何时尚可取消，以及connection/backlog entry
何时已经唯一提交。

**实现自由度：** 锁序、wait token、queue结构与wake实现；无需指定脱离commit point的固定调度winner，但必须让每个
结果由明确线性化先后解释，且blocking retry不能duplicate commit。

### Accept consume、copyout与fd publication

**自然落点：** `UNIX-SOCKET-LIFECYCLE-001`与`SOCKET-ABI-001`；listener owner拥有dequeue/consume，opened-description
owner拥有fd reservation/publication，accept transaction拥有跨两者的rollback或fail-forward编排。

**规范目标：** dequeue、child construction、peer-address copyout与fd commit形成的用户可见consume结果，以及copy
fault、signal、fd exhaustion或listener close竞争的success/error/side effect，均与scoped Linux oracle一致；无论内部
次序如何，都不得泄漏child、duplicate backlog item或发布无返回fd。

**实现自由度：** reservation API、unpublished child的物理构造次序与transaction carrier；不能让fd publication先于
成功返回所需copyout，也不能让copy fault后出现无owner child或duplicate backlog item。

### Stream partial progress与copy fault

**自然落点：** `UNIX-SOCKET-STREAM-001`与`SOCKET-ABI-001`；direction owner拥有bytes/capacity commit，ABI/user-copy
边界只报告已经可见的copy progress和typed outcome。

**规范目标：** send/receive在零进展与已有prefix进展时对copy fault、signal、shutdown和peer close的返回优先级，
`MSG_PEEK`、zero-length、EOF与shutdown race的consume/commit结果，以及合法short result，均与scoped Linux oracle
一致。

**实现自由度：** copy chunk、buffer/cursor representation与一次attempt搬运量。无需为所有fault位置冻结一个固定
正数字节数，但必须保证只提交/消费成功copy对应的有序prefix，且不重复、不重排、不让错误抹去已承诺的可见进展。

### Readable/writable/EOF/RDHUP/HUP

**自然落点：** `SOCKET-WAIT-001`、`IOMUX-POLL-002/003` Target Refine与`EPOLL-READY-001` Target Refine；Unix
directional owner定义source facts，iomux/epoll只拥有consumer projection与delivery policy。

**规范目标：** data、capacity、local/peer shutdown、EOF与terminal close形成的ordinary readable、immediate
writable、receive-half-close和full HUP用户可见结果与scoped Linux oracle一致，包括下一次I/O立即返回terminal error
时的writable投影，以及local `SHUT_RD`、peer `SHUT_WR`和full close对RDHUP/HUP的区别。

**实现自由度：** route storage、notification、snapshot实现与consumer内部扫描；无需在RFC重复current
poll/select/epoll policy的完整笛卡尔积，但source predicate和source-neutral category必须足以由current contract推导
requested RDHUP、mandatory HUP/ERR、ordinary EOF readability和epoll exact-scan结果。

这些验证面只为实现和acceptance提供范围与oracle，不建立第二份状态表。确定内部路线、补充focused test或澄清不改变
scoped target的Linux结果，不增加RFC修订号；只有要接受用户可见偏差，或改变已经接受的target、owner、ABI、contract、
acceptance与validation boundary时，才进入语义修订或Target Renegotiation。

## 禁止退化项

- 不得缓存可以从ops或family owner直接推导的family/type/readiness/role字段。
- 不得让diagnostic owner/id/label反向驱动lifecycle、admission或generation选择。
- 不得以pathname string、裸ino、fd number、raw pointer或Weak upgrade替代stable identity/liveness capability。
- 不得让source notification携带用户可见ready mask、errno、bytes或connection commit。
- 不得建立无真实consumer的SocketClass、通用Endpoint、万能attempt result、generic inode attachment或第二套wait queue。
- 不得以照搬Linux内部对象图、通过一个用例或静默兼容flag替代scoped Linux behavior、Anemone owner invariant与ABI proof。
- 不得以降低oracle、跳过final scan、忽略copy fault、把unsupported option返回成功或只验证一个架构换取closure。
- 不得把工程允许的bind weak atomicity扩大为Unix-owned半发布、stale identity、UAF、leak或silent success。
