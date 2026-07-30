# Network UDP Socket 当前契约

**Contract IDs：** `NET-PROTOCOL-BOUNDARY-001`、`NET-SOCKET-ENDPOINT-001`、
`NET-UDP-TRANSACTION-001`、`NET-SOCKET-WAIT-001`
**状态：** Active
**Owner：** shared protocol vocabulary、kernel UDP Socket/source、initial-domain control plane、domain Stack/Endpoint
分别拥有各自state；本页只拥有它们之间的协议
**参与领域：** network control plane / protocol Stack / VFS opened description / socket syscall / iomux / epoll
**覆盖范围：** narrow UDP capability、Socket/Endpoint association与retire、bind/send/receive transaction、
readiness/wake/cancellation投影
**不覆盖：** connected UDP、IPv6、`SO_REUSE*`、bound-device、async ICMP error/`SO_ERROR`、IPv4 fragment
reassembly、runtime network reconfiguration/detach、TCP或raw socket
**实现位置：** `anemone-kernel/crates/anemone-net-api/src/udp.rs`、
`anemone-kernel/crates/anemone-smoltcp-stack/src/{stack/udp.rs,udp/}`、
`anemone-kernel/src/net/{udp.rs,domain/stack/udp.rs}`、`anemone-kernel/src/fs/socket/udp/`、
`anemone-kernel/src/fs/api/socket/`
**依赖：** `NET-BOUNDARY-001`、`NET-FRAME-OWN-001`、`NET-FRAME-PROGRESS-001`、
`NET-STACK-PUMP-001`、`NET-CONTROL-PLANE-001`、`OPENED-DESC-001..003`、`IOMUX-POLL-001..003`、
`EPOLL-WATCH-001`、`EPOLL-READY-001`、`EPOLL-FILE-001`
**Pending Successor：** None
**最后核验：** 2026-07-31

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| cross-crate UDP value、opaque identity、typed outcome与snapshot | `anemone-net-api` shared semantic surface | kernel与concrete Stack只取得操作所需值 | 表达窄、非阻塞owner handoff；不拥有runtime state |
| Linux UAPI、opened-description association、blocking choice与readiness/error投影 | kernel `UdpSocketFile` / `UdpSocketSource` | `File::prv`中的Socket private state与opaque `UdpEndpointPort` | syscall normalization、wait registration与Linux结果映射 |
| local address、route与source/interface selection | initial-domain `Ipv4ControlPlane` | operation-local immutable selection | 在Endpoint send admission前唯一决定路径，不拥有binding |
| Endpoint identity/lifecycle、binding namespace、queue/capacity/private engine与datagram storage | domain Stack的UDP Endpoint owner | kernel只持opaque port并取得point-in-time facts/outcome | bind、send/receive commit、retire与stale isolation |
| Endpoint fact invalidation | domain Stack产生的owner transition | Socket source持weak observer route；iomux/epoll持non-owning poll route | 只提示重算当前predicate，不携带readiness/error truth |
| opened-description terminal publication | `ProcFile` lifecycle owner | UDP final-release hook取得窄ctx | 最后published fd slot移除时发起一次Socket retire |
| 一轮wait、watch与harvest | iomux / epoll各自owner | Socket source提供snapshot/register/final-recheck | cancellation、timeout、signal与ready交付 |

这些owner之间没有共同mutable object或并列truth。opaque identity、selection、snapshot、invalidation和wake edge都只
服务一次operation或重查，不能反向取得另一个owner的私有state。

## NET-PROTOCOL-BOUNDARY-001 — Cross-owner UDP capability保持窄且非阻塞

**规则：** `anemone-net-api`只定义kernel与concrete Stack共同需要的protocol-domain value、opaque Endpoint
identity、request/outcome、point-in-time facts与invalidation vocabulary，不拥有registry、queue、waiter或Linux ABI
policy。concrete Stack独占smoltcp handle、buffer、mapping与representation conversion；kernel独占task、fd、
opened-description、user pointer、wait和errno映射。Stack operation以owner-local同步mutation/observation完成，不能
接收task、File、waiter或user pointer，也不能把private object交给kernel。

cross-owner handoff只有三类：non-blocking request在Stack owner内立即commit或返回typed rejection/not-ready；
invalidation edge只表示owner fact可能改变；snapshot/outcome只供当前operation解释。事件、snapshot或opaque id不得
缓存为第二份lifecycle/readiness/error/route truth，host-validation facade不得进入kernel production dependency。

**失败与cleanup：** request在owner commit前失败时不发布partial Endpoint/binding/datagram state；kernel只映射明确
outcome。unknown/stale identity fail closed，不允许fallback到private handle lookup、第二registry或future TCP
framework。observer注销和晚到edge不改变Endpoint owner state。

**违反表现：** Stack接收fd/task/Linux errno；kernel接收smoltcp handle/private queue；API crate成为第二net core；
event直接发布poll mask/error；host-only control进入production dependency；为无consumer的协议预建通用框架。

**验证 / Enforcement：** shared API与dependency audit、no-default stack build、host topology/owner tests、kernel
KUnit与双架构真实Socket runtime。

**最初来源：** [Network UDP RFC R0](../../rfcs/net-udp/invariants.md#net-protocol-boundary-001--cross-crate-protocol-capability保持窄且非阻塞)。

**当前来源：** [Network UDP transaction](../../devlog/transactions/2026-07-29-net-udp.md)的
`NET-UDP-FINAL-CUTOVER`。

## NET-SOCKET-ENDPOINT-001 — Socket与Endpoint保持owner fence和单向association

**规则：** kernel Socket唯一拥有Linux-visible UAPI、blocking choice、wait/readiness/error interpretation与
opened-description integration；Stack Endpoint唯一拥有protocol-local lifecycle、committed binding、port
reservation、queue/capacity/error facts、datagram storage与private engine。`File::prv`保存Socket private state及
一条opaque Endpoint association；`FileOps`只投影窄immutable capability。association不允许kernel反向解引用
Stack-private object或复制mapping。

Socket创建允许在publication前失败；live Socket最多关联一个active Endpoint。创建guard在fd publication前失败时
先撤销Socket source publication并retire新Endpoint。semantic final release以`OPENED-DESC-001`的
`Live(1) -> Retired`为唯一kernel trigger：UDP hook先撤销source publication/association和observer route，再以
non-blocking request移交Endpoint retire；它不等待Stack progression、worker或完整reclamation。dup/fork aliases共享
同一opened description，关闭非最后alias不得触发retire。

**线性化与stale isolation：** Socket source撤销association后，任何新operation都不能取得旧Endpoint；Stack retire
先withdraw active identity与binding reservation，再清理owner-local engine、queue及local-link资源。延迟cleanup、
晚到invalidation或旧opaque identity不得恢复association、释放复用port的新owner或把旧datagram/error交给新Socket。

**失败、取消与shutdown：** final-release handoff或creation rollback不以`Drop`、raw fd number、临时`Arc` borrow或
memory lifetime代替semantic close。wait cancellation只retire当前wait registration，不延长Socket/Endpoint lifetime。
orderly network shutdown先撤销新protocol admission；boot-persistent Stack/provider retention仍由Network/System Power
contracts拥有，本规则不宣称runtime detach或完整reclamation。

**违反表现：** 按fd number创建/retire Endpoint；close一个dup提前retire；final-release等待worker；kernel保存
private queue truth；Endpoint持task/waiter；old cleanup命中新generation/port owner；`Drop`成为semantic close。

**验证 / Enforcement：** creation rollback、dup/fork、one-alias/final close、CLOEXEC、retire publication withdrawal、
late duplicate hint、port reuse isolation与blocking wait cancellation KUnit/host/real-consumer matrix。

**最初来源：** [Network UDP RFC R0](../../rfcs/net-udp/invariants.md#net-socket-endpoint-001--socket与endpoint保持owner-fence和单向association)。

**当前来源：** [Network UDP transaction](../../devlog/transactions/2026-07-29-net-udp.md)的
`NET-UDP-FINAL-CUTOVER`。

## NET-UDP-TRANSACTION-001 — Bind、send与receive各自只有一个commit boundary

**规则：** initial-domain Stack唯一拥有active Endpoint namespace、binding/conflict、ephemeral port reservation与
release。explicit bind在同一owner transaction内验证control-plane确认的local address、检查对称conflict、为port 0
选择并保留port，再原子commit binding；implicit bind使用同一namespace与commit。wildcard与任意specific同port冲突，
相同specific冲突，不同specific address可共用port。失败不得发布binding或port；wildcard source selection只属于
当前send operation，不写回binding。

每次send先由control plane唯一决定route/source/interface，再由Endpoint owner在成功返回前完成binding、size与
bounded admission检查。admission commit是kernel buffer向Endpoint唯一owner的handoff；no route/source/interface、
oversize、binding conflict/exhaustion与normal Endpoint capacity不足都在该commit前返回typed failure。commit后
ordinary external loss不反向改写send结果，protocol engine也不得因缺source而静默dequeue/drop。local/loopback
datagram仍经bounded protocol egress、software-link和normal ingress；remote external流量经provider普通frame path。

receive consume的线性化点是Endpoint把队首完整datagram原子detach给operation-local kernel transaction。detach前
Endpoint独占payload；detach后kernel独占，Endpoint queue/readiness及并发receive可继续推进。short buffer只复制
prefix但消费整个datagram，zero-length合法；payload、peer或addrlen任一copyout fault都返回copy error并丢弃已detach
datagram，不requeue、不重排。任意IPv4 fragment在UDP header parse/Endpoint lookup前稳定丢弃，不形成readiness、
error或reassembly lifecycle。

**资源、失败与cleanup：** datagram在kernel transaction、Endpoint queue、protocol engine、local link/provider和
ingress之间每次只有一个访问owner。handoff前失败由当前owner rollback/retain，handoff后前owner不得再次访问。
retire释放binding与bounded storage时必须按Endpoint identity隔离；normal capacity/backpressure是可恢复结果，不得
panic、busy-spin或用无界queue吸收。

**违反表现：** kernel先公开未保留port；两份conflict table；wildcard/specific同port共存；send success后才发现
无source；kernel与Endpoint同时访问payload；copy fault后requeue；short receive只消费prefix；Socket-to-Socket
local copy；fragment进入UDP demux；旧retire释放新port owner。

**验证 / Enforcement：** full bind-conflict/port0/implicit/demux/reuse matrix，selection/admission/capacity与provider
recovery host tests，short/zero/fault/concurrent receive、fragment rejection KUnit/real-consumer tests，以及RV64/LA64
loopback、self-external与remote-external双向runtime。

**最初来源：** [Network UDP RFC R0](../../rfcs/net-udp/invariants.md#net-udp-bind-001--bind-conflictport-allocation与commit属于stack)中的bind/datagram target。

**当前来源：** [Network UDP transaction](../../devlog/transactions/2026-07-29-net-udp.md)的
`NET-UDP-FINAL-CUTOVER`。

## NET-SOCKET-WAIT-001 — Protocol fact、wake与Linux readiness保持分离

**规则：** Endpoint唯一拥有readable/writable/error所需protocol facts；kernel `UdpSocketSource`结合一次snapshot、
opened-description status与syscall context投影Linux readiness/error。Socket不缓存RX/TX、provider queue或private
engine capacity truth；Endpoint invalidation与`PollRoute` notification只是non-owning recheck hint，不是poll mask或
errno。

UDP source Preserve `IOMUX-POLL-001..003`：snapshot/register gate在source publication临界区发布route并读取当前
predicate；可能丢失精确覆盖时返回recheck而不park；任意hint、timeout、signal或force后执行final predicate scan。
default blocking与`O_NONBLOCK`/`SOCK_NONBLOCK`/`MSG_DONTWAIT`读取同一not-ready predicate，per-call flag不修改
opened-description status，blocking path不得busy-poll。

ordinary writable表示live、未retire Endpoint当前可立即接纳至少一个非空且在第一版范围内的datagram进入自身
bounded TX admission storage；它不承诺任意destination、length或provider立即发送。route/source/oversize与
request-specific capacity仍由send transaction裁决。provider backpressure只有在阻塞progression并耗尽Endpoint
admission时才间接清除writable，恢复由Endpoint owner更新facts再发布invalidation。

**取消与cleanup：** 每个blocking/iomux/epoll consumer各自拥有wait round/watch；signal、timeout、force、close或
losing waiter只retire其route/round，不撤销其它consumer。Socket retire先撤销source publication和全部route，再在
锁外发送hint/drop引用；晚到或重复edge对retired generation fail closed，不能命中新association。final release不等待
waiter，waiter也不拥有Socket/Endpoint lifecycle。

**违反表现：** event payload直接返回用户；register window lost wake；未注册source就sleep；Socket复制queue count；
route不存在就永久清除writable；`MSG_DONTWAIT`改变status；一个waiter取消撤销其它route；retire未通知empty-interest
route；old edge命中新association。

**验证 / Enforcement：** initial-unbound writable、route/request failure、Endpoint/provider saturation/recovery、
snapshot/register/final-scan、multi-waiter/signal、poll/select/epoll coexistence、dup/fork/close/retire及晚到hint
KUnit/host/real-consumer matrix；双架构17项UDP与11项epoll runtime。

**最初来源：** [Network UDP RFC R0](../../rfcs/net-udp/invariants.md#net-socket-wait-001--protocol-factwake与linux-readiness保持分离)。

**当前来源：** [Network UDP transaction](../../devlog/transactions/2026-07-29-net-udp.md)的
`NET-UDP-FINAL-CUTOVER`。

## 当前接受边界

- 当前能力是IPv4 unconnected UDP：`socket(AF_INET, SOCK_DGRAM, 0)`、`bind`、`getsockname`、`sendto`、
  `recvfrom`、blocking/nonblocking与ordinary poll/select/epoll；其余能力不从本页外推。
- QEMU runtime覆盖RV64 virtio-mmio与LA64 virtio-pci的single-NIC、`smp=1` loopback/self-external/remote-external；
  physical hardware、`smp>1`、任意其它NIC或deployment均Not Run。
- full network LTP与final harness Not Run；当前Linux ABI证据是RFC列出的focused real-consumer matrix和LTP whitelist
  4/4，不宣称完整Linux socket兼容。
- runtime address/route reconfiguration、hotplug/detach/restart和完整network teardown不在当前target；这些非目标不
  改变本页已经生效的owner、transaction、wait与stale-isolation义务。
