# Network Protocol Socket 当前契约

**Contract IDs：** `NET-PROTOCOL-BOUNDARY-001`、`NET-SOCKET-WAIT-001`
**状态：** Active
**Owner：** shared protocol vocabulary、kernel family Socket/source与domain Stack protocol owner分别拥有各自state；本页只拥有跨owner capability与fact/wait协议
**参与领域：** network control plane / protocol Stack / VFS opened description / Socket / iomux / epoll
**覆盖范围：** IPv4 UDP与ICMP raw的窄、非阻塞owner handoff，以及owner predicate到Linux readiness的投影
**不覆盖：** family-specific association/packet transaction、generic protocol registry、TCP、IPv6或通用error queue
**实现位置：** `anemone-kernel/crates/anemone-net-api/src/{udp,icmp_raw}.rs`、`anemone-kernel/crates/anemone-smoltcp-stack/src/`、`anemone-kernel/src/net/`、`anemone-kernel/src/fs/socket/{udp,icmp_raw}/`
**依赖：** `NET-CONTROL-PLANE-001`、`OPENED-DESC-001..003`、`IOMUX-POLL-001..003`、`EPOLL-WATCH-001`、`EPOLL-READY-001`、`EPOLL-FILE-001`
**Pending Successor：** None
**最后核验：** 2026-08-03

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 非 owner 持有什么 | 行为用途 |
| --- | --- | --- | --- |
| cross-crate protocol value、opaque identity、typed outcome与snapshot | `anemone-net-api` shared semantic surface | kernel与concrete Stack只取得当前operation所需值 | 窄、非阻塞handoff；不拥有runtime state |
| Linux UAPI、blocking choice与readiness投影 | kernel family Socket/source | opaque Endpoint capability与point-in-time facts | syscall normalization、wait registration与结果映射 |
| route、source与interface selection | initial-domain IPv4 control plane | operation-local immutable selection | 在protocol admission前唯一决定路径 |
| Endpoint lifecycle、association、queue/capacity与private engine | 对应domain Stack protocol owner | kernel只持opaque identity/capability | family operation commit、retire与stale isolation |
| fact invalidation | 对应Stack owner transition | Socket source持weak route；consumer持non-owning poll route | 只提示重算，不携带ready truth |
| wait round、watch与harvest | Socket syscall、iomux或epoll各自consumer | family source提供snapshot/register/recheck | cancellation、timeout、signal与结果交付 |

这些owner之间没有共同mutable object或并列truth。opaque identity、selection、snapshot与invalidation只服务当前operation或
重查，不能反向取得另一个owner的私有state。

## NET-PROTOCOL-BOUNDARY-001 — Cross-owner protocol capability保持窄且非阻塞

**规则：** `anemone-net-api`只定义kernel与concrete Stack共同需要的protocol-domain value、opaque Endpoint identity、
role-scoped operation capability、request/outcome、point-in-time facts与invalidation vocabulary，不拥有registry、queue、
waiter或Linux ABI policy。concrete Stack独占smoltcp handle、buffer、mapping与private packet representation；kernel独占
task、fd、opened-description、user pointer、Linux wait和errno映射。

当前真实consumer只有UDP与ICMP raw。共同composition可以静态分发两者的attach、ingress、egress与invalidation，但不得
因此建立dynamic protocol manager、generic Endpoint hierarchy或把family association/transaction上收为shared truth。
Stack operation只在owner-local同步mutation/observation中立即commit或返回typed rejection/not-ready；invalidation只表示
owner fact可能改变，snapshot/outcome只供当前operation解释。

**失败与cleanup：** request在owner commit前失败时不发布partial Endpoint、association或packet state。unknown/stale
identity fail closed，不fallback到private handle、第二registry或future protocol framework。observer注销和晚到edge不
改变Endpoint owner state；host-validation facade不得进入kernel production dependency。

**违反表现：** Stack接收fd/task/Linux errno；kernel取得smoltcp handle/private queue；API crate成为第二net core；event
直接发布poll mask；UDP与raw复制同一association/route truth；或为无consumer协议预建通用框架。

**验证 / Enforcement：** shared API/dependency/source audit、no-default Stack build、host topology/owner tests、kernel KUnit、
RV64/LA64 UDP/ICMP raw真实consumer回归与orderly network shutdown。

**最初来源：** [Network UDP RFC R0](../../rfcs/net-udp/invariants.md#net-protocol-boundary-001--cross-crate-protocol-capability保持窄且非阻塞)与`NET-UDP-FINAL-CUTOVER`。

**当前来源：** [IPv4 ICMP Raw Socket RFC R0](../../rfcs/icmp-raw-socket/index.md)的`ICMP-RAW-CUTOVER` Refine。

## NET-SOCKET-WAIT-001 — Protocol fact、wake与Linux readiness保持分离

**规则：** 每个Endpoint owner唯一拥有本family readable/writable/error所需protocol facts；kernel family source结合一次
snapshot、opened-description status与syscall context投影Linux readiness。Socket不缓存RX/TX、provider queue、route或
private engine capacity truth；Endpoint invalidation与`PollRoute` notification只是non-owning recheck hint，不是poll mask
或errno。UDP与ICMP raw各自读取owner-defined predicate，不共享一份ready truth。

source服从`snapshot -> register -> recheck/final scan`：在source publication临界区发布route并读取current predicate，
可能丢失精确覆盖时返回recheck而不park；任意hint、timeout、signal或force后再次读取owner predicate。default blocking与
`O_NONBLOCK`/`SOCK_NONBLOCK`/`MSG_DONTWAIT`使用同一not-ready分类，per-call flag不修改opened-description status。

UDP writable与ICMP raw writable都只承诺live Endpoint当前可以接纳本family范围内的最小TX admission；不承诺任意
destination、length、route/source selection或provider立即发送。raw readable只由RX queue非空产生，且raw不因connect
取得peer-close、HUP/RDHUP或pending-error producer。family-specific predicate与request failure分别由其family contract
拥有。

**取消与cleanup：** 每个blocking/iomux/epoll consumer独自拥有wait round/watch。signal、timeout、force、close或losing
waiter只retire自身route/round；Socket retire先撤销source publication和routes，再在guard外发送hint/drop引用。晚到或
重复edge对retired generation fail closed；final release不等待waiter，waiter也不拥有Socket/Endpoint lifecycle。

**违反表现：** event payload直接返回用户；register window lost wake；Socket复制queue count/ready mask；route不存在就
永久清除writable；`MSG_DONTWAIT`改变status；一个waiter取消其它route；或old edge命中新association。

**验证 / Enforcement：** UDP与raw的initial writable、request failure、capacity saturation/recovery、snapshot/register/
final-scan、multi-waiter/signal、poll/select/epoll、dup/fork/final close/late hint KUnit、host与双架构real-consumer matrix。

**最初来源：** [Network UDP RFC R0](../../rfcs/net-udp/invariants.md#net-socket-wait-001--protocol-factwake与linux-readiness保持分离)与`NET-UDP-FINAL-CUTOVER`。

**当前来源：** [IPv4 ICMP Raw Socket RFC R0](../../rfcs/icmp-raw-socket/index.md)的`ICMP-RAW-CUTOVER` Refine。

## 当前接受边界

- 当前protocol Socket consumer只有IPv4 unconnected UDP与`AF_INET + SOCK_RAW + IPPROTO_ICMP`；本页不外推TCP、任意raw protocol或generic BSD Socket framework。
- closure evidence覆盖owner-local/host proof、RV64/LA64 release与guest runtime、UDP/Unix regression、raw focused ABI、glibc/musl curated Socket LTP和BusyBox gateway ping。
- physical hardware、`smp > 1`、其它NIC、full network LTP与final harness均Not Run。
