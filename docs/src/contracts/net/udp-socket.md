# Network UDP Socket 当前契约

**Contract IDs：** `NET-SOCKET-ENDPOINT-001`、`NET-UDP-TRANSACTION-001`
**状态：** Active
**Owner：** shared protocol vocabulary、kernel UDP Socket/source、initial-domain control plane、domain Stack/Endpoint
分别拥有各自state；本页只拥有它们之间的协议
**参与领域：** network control plane / protocol Stack / VFS opened description / socket syscall / iomux / epoll
**覆盖范围：** UDP Socket/Endpoint association与retire，以及bind/connect/send/receive transaction
**不覆盖：** IPv6、`SO_REUSE*`、bound-device、async ICMP error/`SO_ERROR`、IPv4 fragment
reassembly、runtime network reconfiguration/detach、TCP或raw socket
**实现位置：** `anemone-kernel/crates/anemone-net-api/src/udp.rs`、
`anemone-kernel/crates/anemone-smoltcp-stack/src/{stack/udp.rs,udp/}`、
`anemone-kernel/src/net/{udp.rs,domain/stack/udp.rs}`、`anemone-kernel/src/fs/socket/udp/`、
`anemone-kernel/src/fs/socket/api/`
**依赖：** `NET-PROTOCOL-BOUNDARY-001`、`NET-SOCKET-WAIT-001`、`NET-BOUNDARY-001`、`NET-FRAME-OWN-001`、`NET-FRAME-PROGRESS-001`、
`NET-STACK-PUMP-001`、`NET-CONTROL-PLANE-001`、`OPENED-DESC-001..003`、`IOMUX-POLL-001..003`、
`EPOLL-WATCH-001`、`EPOLL-READY-001`、`EPOLL-FILE-001`
**Pending Successor：** None
**最后核验：** 2026-08-04

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| cross-crate UDP value、opaque identity、typed outcome与snapshot | `anemone-net-api` shared semantic surface | kernel与concrete Stack只取得操作所需值 | 表达窄、非阻塞owner handoff；不拥有runtime state |
| Linux UAPI、opened-description association、blocking choice与readiness/error投影 | kernel `UdpSocketFile` / `UdpSocketSource` | `File::prv`中的Socket private state与opaque `UdpEndpointPort` | syscall normalization、wait registration与Linux结果映射 |
| local address、route与source/interface selection | initial-domain `Ipv4ControlPlane` | operation-local immutable selection | 在Endpoint connect/send admission前唯一决定路径，不拥有binding或peer |
| Endpoint identity/lifecycle、binding/peer association、queue/capacity/private engine与datagram storage | domain Stack的UDP Endpoint owner | kernel只持opaque port并取得point-in-time facts/outcome | bind/connect、ingress filter、send/receive commit、retire与stale isolation |
| Endpoint fact invalidation | domain Stack产生的owner transition | Socket source持weak observer route；iomux/epoll持non-owning poll route | 只提示重算当前predicate，不携带readiness/error truth |
| opened-description terminal publication | `ProcFile` lifecycle owner | UDP final-release hook取得窄ctx | 最后published fd slot移除时发起一次Socket retire |
| 一轮wait、watch与harvest | iomux / epoll各自owner | Socket source提供snapshot/register/final-recheck | cancellation、timeout、signal与ready交付 |

这些owner之间没有共同mutable object或并列truth。opaque identity、selection、snapshot、invalidation和wake edge都只
服务一次operation或重查，不能反向取得另一个owner的私有state。

## NET-SOCKET-ENDPOINT-001 — Socket与Endpoint保持owner fence和单向association

**规则：** kernel Socket唯一拥有Linux-visible UAPI、blocking choice、wait/readiness/error interpretation与
opened-description integration；Stack Endpoint唯一拥有protocol-local lifecycle、committed binding、port
reservation、queue/capacity/error facts、datagram storage与private engine。`File::prv`保存Socket private state及
一条opaque Endpoint association；`FileOps`只投影窄immutable capability。association不允许kernel反向解引用
Stack-private object或复制mapping。persistent UDP peer及其ingress filter是Endpoint owner的一部分；Socket/front不保存
connected bit、peer或并列filter truth。

Socket创建允许在publication前失败；live Socket最多关联一个active Endpoint。创建guard在fd publication前失败时
先撤销Socket source publication并retire新Endpoint。semantic final release以`OPENED-DESC-001`的
`Live(1) -> Retired`为唯一kernel trigger：UDP hook先撤销source publication/association和observer route，再以
non-blocking request移交Endpoint retire；它不等待Stack progression、worker或完整reclamation。dup/fork aliases共享
同一opened description，关闭非最后alias不得触发retire。

**线性化与stale isolation：** Socket source撤销association后，任何新operation都不能取得旧Endpoint；Stack retire
先withdraw active identity与binding reservation，再清理owner-local engine、queue及local-link资源。延迟cleanup、
晚到invalidation或旧opaque identity不得恢复association、释放复用port的新owner或把旧datagram/error交给新Socket。

UDP connect/reconnect在control-plane selection后由Endpoint owner一次完成live identity检查、必要的implicit bind与
peer replace；任一失败保持原binding/peer。`AF_UNSPEC` disconnect只清除peer并保留binding。ingress在authoritative
queue admission前按current peer过滤新datagram；wrong-peer流量不占queue/capacity，已经queued的datagram不因后续
connect/reconnect/disconnect回溯清理或重分类。

**失败、取消与shutdown：** final-release handoff或creation rollback不以`Drop`、raw fd number、临时`Arc` borrow或
memory lifetime代替semantic close。wait cancellation只retire当前wait registration，不延长Socket/Endpoint lifetime。
orderly network shutdown先撤销新protocol admission；boot-persistent Stack/provider retention仍由Network/System Power
contracts拥有，本规则不宣称runtime detach或完整reclamation。

**违反表现：** 按fd number创建/retire Endpoint；close一个dup提前retire；final-release等待worker；kernel保存
private queue truth；Endpoint持task/waiter；old cleanup命中新generation/port owner；`Drop`成为semantic close。

**验证 / Enforcement：** creation rollback、connect/reconnect/disconnect、peer-filter与queued non-retroactivity、
dup/fork、one-alias/final close、CLOEXEC、retire publication withdrawal、late duplicate hint、port reuse isolation与
blocking wait cancellation KUnit/host/real-consumer matrix。

**最初来源：** [Network UDP RFC R0](../../rfcs/net-udp/invariants.md#net-socket-endpoint-001--socket与endpoint保持owner-fence和单向association)。

**当前来源：** [Network UDP transaction](../../devlog/transactions/2026-07-29-net-udp.md)的
`NET-UDP-FINAL-CUTOVER`，并由[UDP Socket Extension RFC R1](../../rfcs/udp-socket-extension/index.md)的
`UDP-EXT-R1-CUTOVER` Refine。

## NET-UDP-TRANSACTION-001 — Bind、connect、send与receive各自只有一个commit boundary

**规则：** initial-domain Stack唯一拥有active Endpoint namespace、binding/conflict、ephemeral port reservation与
release。explicit bind在同一owner transaction内验证control-plane确认的local address、检查对称conflict、为port 0
选择并保留port，再原子commit binding；implicit bind使用同一namespace与commit。wildcard与任意specific同port冲突，
相同specific冲突，不同specific address可共用port。失败不得发布binding或port；wildcard source selection只属于
当前send operation，不写回binding。

connect在ABI完成peer normalize且control plane取得operation-local selection后，由Endpoint owner在同一transition内
验证identity、完成必要的implicit bind并commit peer。reconnect原子替换peer，disconnect只清peer；任何commit前失败
保持原binding/peer。该transition只在guard外发布fact invalidation，不向caller暴露可错序调用的prepare/commit协议。

每次send先由control plane唯一决定route/source/interface，再由Endpoint owner在成功返回前完成binding、size与
bounded admission检查。admission commit是kernel buffer向Endpoint唯一owner的handoff；no route/source/interface、
oversize、binding conflict/exhaustion与normal Endpoint capacity不足都在该commit前返回typed failure。commit后
ordinary external loss不反向改写send结果，protocol engine也不得因缺source而静默dequeue/drop。local/loopback
datagram仍经bounded protocol egress、software-link和normal ingress；remote external流量经provider普通frame path。
显式destination只属于当前operation；没有显式destination时使用Endpoint current peer，两者都不存在返回
`EDESTADDRREQ`。scalar、file/vector和single-message send都在完整user-copy、size/overflow与admission后只提交一个
datagram；capacity retry只保留immutable operation snapshot，不保留user pointer、owner guard或重复commit authority。

receive consume的线性化点是Endpoint把队首完整datagram原子detach给operation-local kernel transaction。detach前
Endpoint独占payload；detach后kernel独占，Endpoint queue/readiness及并发receive可继续推进。short buffer只复制
prefix但消费整个datagram，zero-length合法；payload、peer或addrlen任一copyout fault都返回copy error并丢弃已detach
datagram，不requeue、不重排。任意IPv4 fragment在UDP header parse/Endpoint lookup前稳定丢弃，不形成readiness、
error或reassembly lifecycle。

peek只取得operation-local queue-head observation，不detach datagram或释放RX credit。single-message receive复用同一
detach/peek outcome：adapter按payload、peer name、`msg_flags`、`msg_controllen`顺序投影输出；short receive设置
`MSG_TRUNC`，input `MSG_TRUNC`决定返回完整packet length还是copied length；没有ancillary producer时control length输出0。
detach后的后续output fault不requeue，peek路径的任一fault不改变queue。

**资源、失败与cleanup：** datagram在kernel transaction、Endpoint queue、protocol engine、local link/provider和
ingress之间每次只有一个访问owner。handoff前失败由当前owner rollback/retain，handoff后前owner不得再次访问。
retire释放binding与bounded storage时必须按Endpoint identity隔离；normal capacity/backpressure是可恢复结果，不得
panic、busy-spin或用无界queue吸收。

**违反表现：** kernel先公开未保留port；两份conflict table；wildcard/specific同port共存；send success后才发现
无source；kernel与Endpoint同时访问payload；copy fault后requeue；short receive只消费prefix；Socket-to-Socket
local copy；fragment进入UDP demux；旧retire释放新port owner。

**验证 / Enforcement：** full bind-conflict/port0/implicit/demux/reuse、connect/reconnect/disconnect、explicit/default
destination、selection/admission/capacity与provider recovery host tests，scalar/vector/message short/zero/peek/truncate/
fault/concurrent receive、fragment rejection KUnit/real-consumer tests，以及RV64/LA64 focused UDP runtime。Stage 5
transaction保留loopback、self-external与remote-external双向cutover evidence；通用
`run-user-test`不再维护专用host peer，后续external-path持续回归由进入canonical验证的真实UDP consumer承接；
validation asset维护见[2026-07-31清理记录](../../devlog/changes/2026-07-31-net-udp-external-peer-retirement.md)。

**最初来源：** [Network UDP RFC R0](../../rfcs/net-udp/invariants.md#net-udp-bind-001--bind-conflictport-allocation与commit属于stack)中的bind/datagram target。

**当前来源：** [Network UDP transaction](../../devlog/transactions/2026-07-29-net-udp.md)的
`NET-UDP-FINAL-CUTOVER`，并由[UDP Socket Extension RFC R1](../../rfcs/udp-socket-extension/index.md)的
`UDP-EXT-R1-CUTOVER` Refine。

## 当前接受边界

- 当前能力是IPv4 UDP：`socket(AF_INET, SOCK_DGRAM, 0或IPPROTO_UDP)`、bind/local query、connect/reconnect/
  disconnect/peer query、显式或默认destination、scalar/file/vector/single-message I/O、R1 flag以及ordinary
  poll/select/epoll；批量message、ancillary producer、error queue与其它能力不从本页外推。
- cutover evidence覆盖RV64 virtio-mmio与LA64 virtio-pci的single-NIC、`smp=1` loopback/self-external/
  remote-external，以及两架构guest-local C/libc与musl resolver consumer；临时focused host orchestration已删除。
  physical hardware、`smp>1`、
  任意其它NIC或deployment均Not Run。
- full network LTP与final harness Not Run；glibc resolver因`IP_RECVERR`依赖保持Not Supported / Not Cut Over。
  当前Linux ABI证据是RFC列出的focused real-consumer matrix和curated Socket LTP，不宣称完整Linux socket兼容。
- runtime address/route reconfiguration、hotplug/detach/restart和完整network teardown不在当前target；这些非目标不
  改变本页已经生效的owner、transaction、wait与stale-isolation义务。
