# RFC-20260729-net-udp

**状态：** Accepted for Implementation / Stage 0-4 Closed / Stage 5 Outline
**修订：** R0
**负责人：** doruche
**最后更新：** 2026-07-30
**领域：** network / socket / VFS / iomux / build configuration
**事务日志：** [2026-07-29 net-udp](../../devlog/transactions/2026-07-29-net-udp.md)
**影响契约：** [Network](../../contracts/net/index.md)中的`NETDEV-LIFE-001`、`NET-ATTACH-001`、
`NET-BOUNDARY-001`、`NET-FRAME-OWN-001`、`NET-FRAME-PROGRESS-001`、`NET-STACK-PUMP-001`；
[Opened-description](../../contracts/task/opened-description-lifecycle.md)中的`OPENED-DESC-001..003`；
[IOMUX](../../contracts/iomux/poll-wait.md)中的`IOMUX-POLL-001..003`；
[Epoll](../../contracts/epoll/protocol.md)中的`EPOLL-WATCH-001`、`EPOLL-READY-001`、`EPOLL-FILE-001`；
[System Target](../../contracts/configuration/system-target.md)中的`STM-OWNER-001`、`STM-TARGET-001`、
`STM-RESOLVE-001`；Stage 1已新增`NET-IFACE-DOMAIN-001`，Stage 2已新增`NET-CONTROL-PLANE-001`，后续候选新增
`NET-PROTOCOL-BOUNDARY-001`、`NET-SOCKET-ENDPOINT-001`、`NET-UDP-TRANSACTION-001`、
`NET-SOCKET-WAIT-001`
**开放问题：** R0 target无新增开放问题；Checkpoint 3B/3C的历史findings及neutralization见transaction
**下一步：** 只能由新的明确授权运行`4 -> 5 Implementation Resolution Gate`；不得自动激活Stage 5或执行
current-contract cutover

本目录是`net-udp` R0 accepted target的canonical source。Stage 1 `NET-UDP-DOMAIN-CUTOVER`已原子Refine
`NETDEV-LIFE-001`/`NET-ATTACH-001`并Introduce `NET-IFACE-DOMAIN-001`；其它R0 candidate仍须在后续明确
implementation cutover达到各自evidence floor后才能生效。[迁移实施计划](./implementation.md)的Stage 0-2
均已独立关闭；Checkpoint 2A完成behavior-preserving same-owner split，Checkpoint 2B随后原子切换static IPv4
control plane与production local path。`NET-UDP-CONTROL-CUTOVER`已Refine `STM-TARGET-001`并Introduce
`NET-CONTROL-PLANE-001`；2026-07-30 post-close correction恢复`NET-BOUNDARY-001`既有artifact-neutral
validation seam并收拢KUnit共置规则，不改变R0 target或Stage 2 runtime closure。独立`2 -> 3`resolution已把
Stage 3解析为3A same-owner split、3B Endpoint/File/address lifecycle与3C nonblocking datagram三个checkpoint；
3A已经按behavior-preserving same-owner split独立关闭；3B已建立真实Endpoint/File/address lifecycle实现，完成
获批VFS/shared-surface correction、capacity Route Correction、RV64 correction-source证据复用与独立复审后关闭。
3C随后形成`sendto/recvfrom`、implicit bind、control-plane/Stack send admission与owned receive transaction；初审的
RX credit refill、fresh oversize bind retention和specific `127/8` source三个correctness finding均在原owner内修复，
修复后host、双架构build与RV64 fresh-disk证据通过。Stage 3已Closed；独立`3 -> 4`resolution已把Stage 4解析为
4A complete plural-route source、4B blocking syscall与4C race/fragment/evidence closure；三个checkpoint均已分别独立
Closed，Stage 4现已Closed。4B复用唯一shared iomux wait loop，blocking/nonblocking读取同一Endpoint predicate；
4C完成capacity/provider、lifecycle、concurrent copy-fault与fragment真实注入收口，并修复final review发现的
empty-interest retire lost wake。multi-waiter始终是4A/4B固有能力；全部Socket/Endpoint/UDP/wait candidate contract
继续Pending。

2026-07-30开发者以`approved`批准3B correction最小扩展：只为anonymous UDP socket增加正确`S_IFSOCK`投影并
处置`InodeType`的五个exhaustive VFS consumer；允许Stack/shared value surface把namespace policy改为构造时一次
拥有，并把参数semantic与direct storage arithmetic唯一验收收归kernel compile-time assertion。后续capacity
model复核否决了穷尽private container layout和为`SocketSet`eager reserve的路线：它不能证明OOM success，且会把
namespace policy传播到interface/local-link owner。cross-crate layout predicate已删除，不扩张这两个owner文件。
Endpoint collection与per-interface `SocketSet`都保持按实际publication lazy growth；namespace capacity只是
normal exhaustion的逻辑上限，不是启动期内存承诺。该Route Correction保持R0 target、owner、ABI visible
semantics与acceptance不变，Contract Impact为None；精确
manifest、validation与停止边界见[implementation](./implementation.md#15-write-set扩展记录)和transaction。

同日后续backend admission复核确认：ramfs/proc创建入口只传固定类型，不需要为新枚举机械扩张；devfs公开
`DevfsNodeAttr.ty`却以“leaf只要不是Dir”作负向准入，若保持不变会意外允许发布Socket inode。开发者允许按实际
需求判断后，3B correction再最小加入`fs/devfs/mod.rs`，只在publish owner入口明确拒绝Socket并增加owner-local
KUnit；这不授权ramfs/proc改动，也不改变上述target、owner、Contract Impact或3C状态。

## 摘要

本草案为 Anemone 建立第一版普通用户态可用的 IPv4 unconnected UDP vertical slice。production 中只有一个
initial network domain；该 domain 只有一个 global protocol `Stack` instance，loopback 与外部 interface 均进入
这一 domain 和同一个 Stack。Stack 唯一拥有 UDP Endpoint、binding namespace、private protocol engine state
与 protocol progression；kernel Socket 唯一拥有 Linux/POSIX UAPI、opened-description 交互、阻塞选择、wait、
readiness 与 error publication。

第一版同时建立后续 `net-tcp` 可以复用的最小 network socket foundation：socket syscall containment、
opened-description lifecycle、opaque Socket/Endpoint association、logical-interface/control-plane 边界和 iomux
source protocol。它不提前建设完整 BSD socket framework，也不把 concrete Rust trait、锁、worker、queue、
registry 或算法写成 target。

## 背景

已关闭的 `net-frame-path` 提供 boot-time external netdev publication、frame ownership、bounded provider
progress、concrete Stack pump 与 attach/shutdown handoff。当前 production wiring 在每个 external netdev attach
时创建独立的 `Stack + Provider + PumpCore + worker`，每个 Stack 内又为 interface 保存独立 SocketSet。该 wiring
是 frame-path 阶段的有效实现，不是 endpoint namespace 或 multi-interface UDP topology 的长期语义决定。

现有 frame contracts 明确不覆盖 Endpoint、Socket/fd、Linux readiness、IP address/route control plane 或
loopback。若直接在当前 per-netdev Stack 上添加 UDP，会把 endpoint namespace、bind/port conflict、route/source
selection 与 interface identity 按设备分裂，并使 loopback 只能通过旁路或另一 Stack 加入。该形状不能支持本草案
需要的 wildcard bind、同域 endpoint namespace 和普通 route/source/interface selection。

因此，本草案把 frame-path capability 纳入一个 initial network domain，以一个 global protocol Stack 承接
所有 protocol resources；同时把 logical network interface 从 device-backed NIC publication 中分离。该方向只
改变 UDP 及其必要 shared foundation 的 target，不把 frame ownership、provider resource truth 或 System Power
global shutdown episode 移交给新 owner。

## 目标

- 建立一个 initial network domain 以及该 domain 内唯一的 global protocol `Stack`；所有 production loopback
  和外部 interface 均挂入该 Stack。
- 建立 domain-local logical-interface namespace、membership/lifecycle、identity/ifindex/name/kind 与统一
  interface view，同时保留 device-backed NIC publication、provider backing 和 hardware lifecycle 的既有 owner。
- 提供 production loopback。`lo` 是 initial domain 必备的 first-class software interface，流量通过普通 route、
  protocol egress、bounded software-link handoff 和正常 protocol ingress，不建立 socket-to-socket 私有路径。
- 建立 boot-time static IPv4 control plane，覆盖 interface address/prefix、connected route、目标环境需要的
  default route，以及 route/source/interface selection 的唯一行为权威。
- 本地产生并发往任一已配置本地单播 IPv4 address（包括 external-interface address）的 datagram 命中
  domain-local route，经有界 software handoff 和同一 Stack 的正常 protocol ingress 交付，不进入 external
  provider 或 socket fast path。
- 建立 kernel Socket 与 protocol Endpoint 的 owner/object fence、opaque association、final-release retire
  trigger、non-blocking operation 与 recheck protocol。
- 提供普通用户态可用的 IPv4 unconnected UDP：`socket`、`bind`、`sendto`、`recvfrom` 和 `getsockname`，并与
  已有 `close`、`fcntl(O_NONBLOCK)`、`poll` / `select` / `epoll`、`dup` / `fork` 生命周期互操作。
- 第一版同时覆盖 loopback 与一个 production external interface；外部路径必须经过真实 frame provider 的
  ingress 与 egress，不能由 host fixture、kernel packet injection 或 loopback 结果代替。
- 维持有界 datagram storage、capacity/backpressure、无 busy-poll wait、唯一 packet/datagram owner、失败回滚、
  stale identity isolation 与 non-blocking final release。
- 建立分层 proof boundary：deterministic host proof、双架构同源用户测试、RV64 QEMU agent-run runtime 和
  LA64 QEMU user-run runtime，不从未执行平台外推结论。

## 非目标

- network namespace、多个 network domain、runtime domain creation/destruction 或 domain 配置数量。
- IPv6、DHCP、用户态 ioctl/netlink 配置、runtime address/route change、runtime netdev hotplug/detach/retry。
- connected UDP：`connect` / `getpeername`；`sendmsg` / `recvmsg`；`getsockopt` / `setsockopt`；`shutdown`。
- `MSG_PEEK`、完整 `MSG_TRUNC` 长度语义、ancillary data、socket timeout、`SO_REUSE*`、buffer-size option、
  broadcast/multicast option、timestamp 与其它 socket option。
- IPv4 fragmentation/reassembly 能力。第一版发送只承诺 MTU 内无需 fragmentation 的有界 datagram；ingress
  只接受 `MF = 0 && fragment offset = 0` 的完整 IPv4 packet，fragment 在 UDP demux 前稳定丢弃。
- ICMP asynchronous error、pending `SO_ERROR`、完整 `POLLERR` 与 connected-UDP error semantics。
- TCP 的 `listen` / `accept*`、AF_INET `socketpair` 或完整 BSD socket subsystem。
- runtime complete teardown、provider/device quiesce proof、worker join、全部网络资源回收或重启。
- physical hardware、virtio-pci、新 NIC backend、final harness、完整 network LTP 或 `smp>1` runtime closure。
- allocator OOM recovery、network-specific `AllocError` / `OutOfMemory` ABI 或 allocation-failure injection proof。
- 为部署配置与实际 Platform/runtime 环境不匹配建立自动探测、fallback、retry、降级或防误用框架。
- 预先冻结 concrete `NetworkDomain` 类型、registry representation、trait hierarchy、方法签名、lock、worker、
  mailbox、queue、buffer、port allocator、算法、模块路径、stage、probe、write set 或验证命令。

## 文档地图

RFC target：

- [目标与不变量](./invariants.md)
- [迁移实施计划](./implementation.md)：Stage 0-4 Closed；Stage 5 Outline

背景材料：RFC前的私有定位已经折入本页和目标不变量，不作为公共引用目标。

当前 effective contracts：

- [Network current contracts](../../contracts/net/index.md)
- [Opened-description lifecycle](../../contracts/task/opened-description-lifecycle.md)
- [Poll wait / source registration](../../contracts/iomux/poll-wait.md)
- [Epoll protocol](../../contracts/epoll/protocol.md)
- [System target与resolved selection](../../contracts/configuration/system-target.md)

相关 accepted/effective source：

- [System Target Model](../system-target-model/index.md)
- [Network Frame Path](../net-frame-path/index.md)

公共外部源码证据：None。implementation中的vendored smoltcp source只作为live implementation audit输入；未来
若使用外部Linux/smoltcp对照事实，必须按公共external-source citation规则固定来源。

## 修订记录

| 修订 | 日期 | 结论 | 证据 |
| --- | --- | --- | --- |
| R0 | 2026-07-29 | 接受initial domain/global Stack、logical interface/control plane、Socket/Endpoint owner fence、IPv4 unconnected UDP能力包络与五项用户可见决定；Stage 1 domain contract delta已cut over，其余candidate保持Pending | [R0 review、Stage 0-1执行与cutover](../../devlog/transactions/2026-07-29-net-udp.md) |

## 方案

### Initial network domain 与 global protocol Stack

production 中只有一个 initial network domain。它表示共享同一 logical-interface namespace、interface
membership/lifecycle、address/route control plane、endpoint namespace 与 protocol Stack 的 network
participants；它不要求存在同名 Rust object，也不引入 network namespace UAPI。

该 domain 内只有一个 global protocol Stack instance。global 只表示 domain-wide 的单一 protocol owner；它
不表示 global static、类型层 singleton、单一大锁、固定 worker 数或固定 CPU affinity。Stack 独占 private
smoltcp object、protocol `InterfaceId` mapping、UDP Endpoint/binding namespace、protocol datagram storage、
deadline 与 progression。任一时刻最多一个 pump 推进该 Stack，但 kernel 可以在 Stack 外决定具体同步、调度和
wake wiring。

Endpoint 属于 network domain/Stack，不属于某一 netdev 或 interface。普通 route/source/interface selection
不得演变成 per-interface endpoint namespace；future bound-interface constraint 也只能约束 selection，不能复制
endpoint identity 或 binding truth。

### Logical interface、device-backed NIC 与 loopback

domain 内存在唯一 logical-interface registry 语义 owner。它拥有 domain-local membership、identity、ifindex、
name、kind 与共同 interface facts；`lo` 和 external interface 都通过这一 view 被 control plane、route/source
selection 和 future interface query 引用，并分别映射为 Stack-private protocol `InterfaceId`。

`device/net` 与 concrete driver/provider 继续拥有 external NIC 的 discovery、publication identity、frame
capability、queue、DMA、IRQ、completion 与 current link/resource truth。external published netdev 通过一次
logical-interface admission 和 provider-capability handoff 进入 initial domain；domain registry 不因此保存或
解释 provider backing。device identity、external netdev publication identity、domain-local interface identity/
ifindex、protocol `InterfaceId` 与 private engine object identity 互不等价。

`lo` 是 initial domain 随生随灭的 first-class software interface；第一版 initial domain 因此恰有一个
boot-persistent `lo`。它不伪造 generic `Device` / `Driver`、Ethernet MAC、ARP、IRQ、DMA、completion 或
external attach lifecycle。software-link data path 只拥有 packet 在 protocol egress handoff 与后续 ingress
handoff 之间的访问权和 resource truth，不拥有 membership、ifindex、address、route、Endpoint 或 Linux
readiness。

loopback flow 必须由 control plane 选择 local route，经同一 Stack 的 protocol egress、有界 software-link
handoff 和普通 protocol ingress完成。`sendto` 不得直接复制到另一个 kernel Socket；host-only
`smoltcp::phy::Loopback` 或 packet-injection helper 不构成 production loopback。

### Cross-crate capability 与 object fence

`anemone-net-api` 继续是 kernel net 与 concrete protocol stack 共同依赖的 semantic contract，不是 runtime
中介、第二个 net core 或 mutable registry。`anemone-smoltcp-stack` 是 concrete protocol resource owner，不是
只做 representation conversion 的薄 adaptor。

跨 crate 交互只交换 protocol-domain value、opaque identity、窄 capability、一次性 outcome/snapshot 和
recheck edge：

- Command 表示请求 protocol owner 执行一次非阻塞操作；不要求总 enum 或 command queue；
- Event 表示 owner fact 可能变化、调用方应重查；它不是 readiness、error 或 durable truth；
- Snapshot 表示一个观察点的一次性 protocol facts；它不能沉淀为第二份行为状态。

Endpoint lifecycle、UDP operation 与 bounded progression 是三类 capability family，但本草案不要求建立
`EndpointOps`、`UdpOps`、`ProtocolPump` 三个 literal Rust trait。只有 shared contract、dependency inversion、
backend replacement 或独立 fake 的真实需求证明后，Ready stage 才能决定是否提取 trait。

Stack 不接收 fd、opened description、task、waiter、用户地址或 Linux errno；kernel 不接收 private smoltcp
object、protocol buffer 或内部 state enum。Stack surface 自身不拥有 lock、worker、waker、admission 或跨线程
串行化语义；kernel owner 只能通过受控访问窗口取得 Stack mutation capability。

### Kernel Socket、File 与 protocol Endpoint

kernel Socket 是 Linux-visible socket semantics owner，负责 syscall/UAPI containment、sockaddr 与 flag 解析、
opened-description status 的使用、阻塞选择、wait protocol，以及 readiness/error 的解释与发布。protocol
Endpoint 是 Stack 内的 resource，负责 protocol-local lifecycle、committed bind、port reservation、queue/
capacity/error facts、private engine state 与 datagram storage。

socket private state 与 stack-independent opaque Endpoint association 安装在 `File::prv`。`FileOps` 只能通过
窄的 immutable private-state projection 访问它；generic `ProcFile` 继续唯一拥有 opened-description
publication lifecycle、shared status flags 与 semantic final release，`FileDesc` 继续只拥有 fd-slot publication
和 fd-local flags。`dup` / `fork` 共享同一个 opened description、File private state 与 Endpoint
association；不得按 fd number 创建、复制或 retire Endpoint。

Socket 与 Endpoint 不建立 storage lifetime 上的永久一一对应。正常 live 路径可以是一 Socket 对一 active
Endpoint；创建失败时 association 可以尚未建立，final release 可以先撤销 association 后发起 retire，Endpoint
也可以在 association 失效后完成 owner-local cleanup。旧 identity、datagram、error 或延迟 cleanup 不能命中新
Socket、复用 identity 或新 port owner。

semantic final release 是 kernel 侧正常 close 发起 endpoint retire 的唯一 trigger。该 trigger 只能发起
non-blocking retire/invalidation，不能等待 Stack progression、worker join 或完整回收；`Drop`、syscall borrow、
底层 `Arc` count 与 raw fd close 都不能替代 final-release truth。

### 第一版用户 ABI

硬最小新增 syscall：

- `socket(AF_INET, SOCK_DGRAM, 0)` 与 `socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP)`；
- `bind`，覆盖具体 local IPv4、`0.0.0.0` wildcard、指定端口与 port 0；
- `sendto`，覆盖 unconnected destination 和首次发送 implicit bind；
- `recvfrom`，返回 datagram payload 与 peer address；
- `getsockname`，观察 explicit/implicit bind 的 committed local constraint 与 ephemeral port；wildcard/implicit
  bind 返回 `0.0.0.0`，不把一次 `sendto` 选择的 source address 写回 binding truth。

最低双向闭环是：server `socket -> bind(port 0) -> getsockname`，client `socket -> sendto`，server
`recvfrom -> sendto`，client `recvfrom`。`close`、`fcntl`、iomux 与 dup/fork 是已有通用 syscall surface，
但必须通过真实 socket 路径验收其互操作。

第一版还承诺：

- socket type flags 支持 `SOCK_NONBLOCK` 与 `SOCK_CLOEXEC`；未知 type bits 稳定拒绝；
- `sendto` / `recvfrom` 支持 flags `0` 与 `MSG_DONTWAIT`；其它 flags 稳定拒绝；
- default blocking、opened-description `O_NONBLOCK` 和 per-call `MSG_DONTWAIT` 使用同一 not-ready predicate；
  `MSG_DONTWAIT` 不修改 status flags；
- datagram 保持消息边界并支持 zero-length payload；短 receive buffer 复制可容纳前缀、返回实际复制长度并
  消费整个 datagram；
- Endpoint 把队首 datagram 原子移交给 operation-local receive transaction 时完成消费；后续 payload 或 peer-address
  copyout fault 返回 copy error 并丢弃该 datagram，不回滚、重排或重新入队；
- functional payload 只承诺配置 MTU 内无需 IPv4 fragmentation/reassembly 的有界 datagram；超出支持范围的
  send 必须同步失败，不能截断后发送；fragmented ingress 不得进入 UDP Endpoint 或形成 readable；
- unsupported family/type/protocol、invalid/non-local address、bind conflict、no route/source/interface、
  oversize、would-block 和 normal bounded-resource exhaustion 形成稳定、可诊断的 syscall outcome。

完整 sockaddr 长度/alignment、精确 errno、partial user-memory effect 与 copy 顺序在 R0 接受前或相应 ABI review
gate 中解析；它们不得改变已经固定的 consume-on-dequeue、扩大前述 capability，或把 unsupported input 静默报告
为成功。

### Address、route、bind 与 send admission

network control-plane owner 读取 domain owner 发布的 logical-interface facts，唯一拥有 per-interface local
address、local-address validity、route 和 route/source/interface selection policy。Stack 可以持有供 protocol
engine 使用的 projection，但不能建立第二份 address/route table 或独立 selection policy。

每个已配置本地单播 IPv4 address 都形成优先于 connected/default route 的 domain-local route。本地产生并发往
本地 external-interface address 的 datagram 与 `127.0.0.0/8` loopback 一样，必须在 route/source selection 后
进入有界 software handoff，再从同一 Stack 的正常 IP/UDP ingress 交付；它不得发送到 external provider 依赖
backend hairpin，也不得由 kernel Socket 直接复制给目标 Socket。wildcard sender 的 source address 由当前 local
route/source policy 为本次 operation 选择；该选择不更新 committed binding。

Stack 唯一拥有 domain 内 Endpoint binding namespace、committed local binding、active port reservation、
conflict decision、ephemeral allocation 与 protocol send admission。kernel Socket 只把 Linux sockaddr/options
normalize 为 protocol-domain constraint，并把 outcome 映射回 Linux-visible result。

explicit `bind` 的目标 transaction 是：kernel normalize 请求；control plane 判断 explicit local address 是否
属于本 domain；Stack 在唯一 binding namespace 内原子完成 conflict check、port 0 allocation/reservation 与
binding commit。wildcard 表示没有具体 local-address constraint，不是 kernel/control-plane 额外保存的一条绑定
记录。第一版没有 `SO_REUSE*` 或 bound-device constraint：wildcard 与任一同端口 binding 冲突，相同具体地址的
同端口 binding 冲突，两个不同具体本地地址可以使用同一端口。port 0 必须按请求的 address constraint 选择满足
同一矩阵的端口。

implicit bind 服从相同的 reservation/commit boundary，并提交 wildcard address constraint 与 ephemeral port。
explicit wildcard/implicit bind 的 `getsockname` 返回 `0.0.0.0` 和 committed port；每次发送选择的实际 source
address 只是 operation-local result。允许的不同具体地址同端口 binding 只按 ingress destination 命中相应
Endpoint；第一版不存在 wildcard/specific demux precedence，因为二者不能并存。

`sendto` 只能读取 committed local constraint 与当前 control-plane facts完成 selection。只有 selection 已成功，
或者 datagram 已交给保证不会再因缺少 route/source/interface 而静默丢弃的 protocol owner 后，syscall 才能报告
成功。后续普通链路丢包不反向改写 syscall result；protocol engine dequeue 后因无 source address 丢包不能伪装成
成功 admission。

### Receive consumption 与 fragmented ingress

Endpoint 将队首 datagram 原子 detach 到一次 operation-local receive transaction 的时刻，是 receive consume
linearization point。该 handoff 发生在任何用户 payload 或 peer address copyout 之前；commit 后 Endpoint queue、
readable predicate 和其它并发 receive 可以继续推进，kernel transaction 成为该 datagram 的唯一 owner。payload、
peer address 或 address-length copyout 随后失败时，当前 transaction 返回相应 copy error 并丢弃 datagram，不得
重新入队、交给其它 receiver 或把它作为下一次调用可重试的数据。用户内存可能已经部分改变，本 target 不承诺
跨多个 copyout 的原子 rollback。short buffer、zero-length datagram 与普通成功路径使用同一 consume boundary；
具体 owned payload/token 表示和 copy 顺序留给 Ready/ABI stage。

第一版 ingress 只把 `MF = 0 && fragment offset = 0` 的完整 IPv4 packet 交给 UDP demux。任一 fragmented packet
必须在 UDP header parsing/Endpoint lookup 前稳定丢弃，不建立 reassembly buffer、timeout、overlap policy 或
fragment lifecycle，不形成 Socket readable/error，也不把单个 fragment 当作完整 UDP datagram。仅仅没有启用
protocol engine 的 fragmentation feature 不是符合性证据；相应 Ready stage 必须证明 explicit ingress gate 和
first/later-fragment rejection。

### Boot-time static configuration

第一版使用 SystemTarget-owned boot-time static IPv4 deployment。支持本 RFC external-path acceptance 的
SystemTarget 至少声明选中的 domain-local external interface name、IPv4 address/prefix，以及目标环境需要的
default route；connected route 从 address/prefix 唯一派生。Platform 继续只拥有 guest machine/device topology
和 QEMU frontend/backend wiring，KernelConfig 只拥有 feature/policy/capacity，BuildPreset 只选择 target/config/
profile，rootfs 不拥有第一版 network configuration truth。

配置边界刻意保持直接：loader/resolver 只做基本结构、类型、IPv4/prefix 表示、必填字段、未知字段和明显范围
错误的合法性检查。它不证明声明的 interface、address、gateway 或 route 与 selected Platform、QEMU backend、
外部网络或实际 boot runtime 相匹配，也不做 reachability、拓扑、冲突或部署正确性证明。

例如，具体 SystemTarget 可以按约定引用首张 external interface `eth0`。该 target/Platform 组合的维护者负责保证
设备拓扑、发现/publication 顺序和部署事实使该名称成立；“配置合法但运行时不存在 `eth0`”属于部署前提未满足，
默认不在本 RFC 的产品语义、验收矩阵或恢复设计范围。实现不需要为它增加替代接口搜索、MAC/bus selector、
fallback、retry、降级、兼容映射或专用 lifecycle state，也不得静默改用其它 interface。该类错配仍须保持内存/
资源安全，但本草案不规定 boot survival、专用 errno、诊断形状或自动修复。

loopback 不获得 enable/count 配置。每个 domain 必有一个 `lo`，`127.0.0.0/8` 和 local route 是 protocol/
control-plane fact，不由 SystemTarget 重复声明。重要 capacity 只有在 concrete resource model 证明其独立存在后才
进入 KernelConfig；本草案不提前发明 knob、默认值或统一 pool。

### Blocking、iomux 与 error publication

socket 是现有 iomux protocol 的普通 pollable source：

- blocking syscall 在 owner predicate 不满足时完成 snapshot/register gate 后进入现有 wait protocol；signal/
  force outcome 按现有 syscall wait rule 中断；
- nonblocking mode 对同一 predicate 同步返回 would-block，不得 sleep 或 busy-poll；
- source owner 在同一 source-state critical section 中发布 route 并读取 readiness，wake/event 只提示重查；
- wake 后由 syscall/iomux/epoll owner读取当前 source predicate，旧、重复或晚到 edge 不直接形成 readiness；
- close/cancellation 撤销 publication并清理 subscription；Endpoint event 不持有 task/waiter，也不进入 consumer
  wait lifecycle。

readable/writable facts 源自 Endpoint owner snapshot，但 Linux-visible poll mask 由 kernel Socket owner解释。
kernel 不缓存第二份 queue/capacity/error truth。第一版只承诺 ordinary level-triggered read/write readiness；
socket-specific edge-trigger/one-shot corner matrix不作为本 RFC 新增 target，但 socket source 必须遵守既有 epoll
watch/ready protocol。

ordinary UDP writable 是 destination-independent 的 general admission predicate：Endpoint 必须 live、未 retire，
并能够立刻接纳至少一个非空、第一版支持范围内的 datagram 进入自己的 bounded TX admission storage。未显式
bind 本身不使 live socket 不可写；implicit bind 的 conflict/exhaustion 在具体 `sendto` 中裁决。writable 不承诺
任意 future destination、任意 datagram length 或当前 provider 都可立即发送；no route/source/interface、oversize
与 request-size-specific capacity failure仍由每次 `sendto` 返回。provider backpressure 只有在持续阻塞 progression
并耗尽 Endpoint-owned admission capacity 时才间接清除 writable；kernel Socket 不读取或复制 provider queue/link
truth，private protocol engine 的近似 `can_send` 也不能未经符合性证明直接成为 Linux predicate。

ICMP asynchronous error与pending `SO_ERROR` 后延。该后延不能让 protocol event或private smoltcp state绕过
kernel Socket owner直接发布 Linux error。

### Evidence responsibility

- Deterministic host proof：Endpoint lifecycle/完整 bind conflict 矩阵、至少两个 interface 的 route/source/egress
  selection、本地 external-address delivery、datagram capacity/writable recheck/copy-fault consumption、fragment
  rejection、retire/stale identity 与 loopback bounded handoff。
- RV64 QEMU agent-run mandatory runtime：`smp=1`，普通用户态测试覆盖真实 syscall/fd/copy/wait、loopback 和
  production external ingress/egress。
- LA64 QEMU user-run mandatory closure：与 RV64 构建同一测试源码和同一 case；允许 launcher、镜像和外部
  endpoint 参数不同。失败阻塞 closure，未提供时只记录 `Not Run`。
- 双架构 build：共享代码不得通过 arch-specific cfg、私有 syscall number、inline assembly 或不同 expected
  output 隐藏功能差异。
- `smp>1` runtime不是 mandatory gate，但 owner/lock/wake/lifecycle/cleanup 必须按 SMP-safe correctness 设计；
  未运行时明确 `Not Run`。

host result 不替代 syscall/scheduler/production provider runtime；RV64 不外推 LA64；QEMU 不外推 physical
hardware、virtio-pci、final harness 或完整 LTP。

## R0 前已关闭的 Target 决策

下列用户可见 target / contract delta 已在R0 normative sections中关闭，implementation route 不得静默
改变：

1. 本地产生并发往本机 external-interface address 的 datagram 进入 domain-local software delivery 与正常 ingress，
   不走 external backend hairpin 或 Socket fast path。
2. 无 `SO_REUSE*` 时，wildcard 与任一同端口 binding 冲突；相同具体地址冲突；不同具体本地地址同端口允许。
3. UDP writable 只表示 live Endpoint 的 destination-independent general TX admission capacity；具体 destination、
   datagram length、selection failure 与 provider backpressure不成为 kernel Socket 的并列 truth。
4. `recvfrom` 在 Endpoint detach 到 operation-local transaction 时消费 datagram；后续任一 copyout fault 不回滚或
   重新入队。
5. 第一版 fragmented ingress 在 UDP demux 前稳定丢弃，不形成 reassembly、Endpoint delivery 或 readiness。

具体 local-delivery medium、binding table/port allocator、writable capacity accounting/storage representation、owned
receive token/copy strategy、fragment gate placement、errno table、internal API 和 test command 仍留给后续
implementation resolution。若工程证据要求改变上述语义，必须在 cutover 前返回 RFC review / Target
Renegotiation Gate。

## 接受边界

本 RFC 的 R0 acceptance 只接受 target、contract delta、correctness boundary、最终 proof obligations 与
implementation feedback boundary；不让任何新语义立即 effective，也不授权实现或 contract cutover。current
contracts 在实际 cutover 前继续有效。

R0 public review确认target / contract proposal已经自洽收口：

- 五项已关闭的用户可见决定与[目标与不变量](./invariants.md)一致且没有重新悬空；
- `NETDEV-LIFE-001`与`NET-ATTACH-001`均固定为Refine，新logical-interface/control-plane/socket-endpoint
  contract及各自cutover gate已经声明；
- System Target Model所需的最小current baseline已经提取，network deployment schema构成
  `STM-TARGET-001`的真实Refine delta。
- 每份mutable state、跨owner handoff、linearization、failure与cleanup均已在目标与不变量中声明唯一owner。

Implementation readiness已经完成：

- [迁移实施计划](./implementation.md)已按positive route关闭Stage 0 multi-interface UDP topology probe；独立
  `0 -> 1`resolution选择per-provider worker + domain-owned single Stack access window路线，并把Stage 1完整解析为
  Ready；后续独立授权已完成Stage 1实现、review、validation和`NET-UDP-DOMAIN-CUTOVER`。Stage 1 closure不构成
  Stage 2 resolution或implementation授权。

R0把 concrete types、内部 API、module placement、lock/worker/queue、capacity、port algorithm、loopback
medium 和 stage-later probe 后延，只要对应 Outline 明确保护本 target、contract IDs、owner、ABI 与 acceptance
boundary。实现证据若只改变内部路线，更新 implementation/transaction；若要求改变上述 target、owner、ABI、
contract 或 claim boundary，必须在 cutover 前停止并返回 RFC review / Target Renegotiation Gate。

## 备选方案

### 保持 per-netdev Stack

拒绝作为 target。它会按设备分裂 Endpoint/binding namespace，并迫使 wildcard bind、loopback 和普通
route/source selection跨多个 protocol owner协调。frame-path 当前 wiring不构成该语义的依据。

### loopback 使用独立 Stack 或 socket-to-socket fast path

拒绝。独立 Stack制造第二 endpoint namespace；socket fast path绕过protocol parsing、ingress、datagram
resource、pump budget和readiness owner，无法与external path共享同一证明。

### 让 `device/net` 直接拥有所有 logical interface

拒绝。`lo`没有generic device/driver/hardware lifecycle；强行纳入会制造假 MAC、IRQ、DMA或external attach
state，并继续混淆device publication identity与domain-local ifindex/name。

### 让 Stack 拥有 ifindex/name/address/route registry

拒绝。Stack可以消费control-plane projection并拥有private protocol InterfaceId，但不能建立第二套Linux-visible/
domain-local interface identity或address/route policy truth。

### 现在建立完整通用 socket/backend trait hierarchy

后延。第一版只有一个 concrete protocol backend和UDP consumer；没有第二个真实consumer或dependency inversion
证据前，优先保持单一 owner和窄直接surface。future TCP复用的是owner/ABI foundation，不是零修改trait承诺。

### 对静态配置做跨 Platform/runtime 完全校验

拒绝。schema/resolver没有runtime topology、backend reachability或部署环境真相；建立第二套selector、probe、
fallback或provenance系统不能证明实际网络正确，反而扩大owner和failure state。当前阶段只保留基本合法性检查与
受维护的target/Platform部署前提。

## 风险

- global Stack使当前per-netdev wiring需要结构变化。以`NET-STACK-PUMP-001`唯一推进和窄capability保护owner，
  不提前用大锁/single worker固化实现。
- logical-interface split会改变`NETDEV-LIFE-001`的identity owner。R0接受前必须确认最小contract closure，
  不以兼容字段或并行registry维持旧ifindex/name truth。
- smoltcp multi-interface/source-selection行为可能与target不一致。send success boundary要求selection前置或可靠
  admission，必要时通过implementation probe验证，但probe不能自行降级target。
- local software handoff、readiness和copyout transaction存在并发/cleanup风险。相应Ready stage必须解析唯一
  packet/datagram owner、capacity、recheck、cancellation和已固定consume点，不得在copy fault后requeue。
- protocol engine未启用IPv4 reassembly不等于fragment安全拒绝；Ready stage必须用first/later fragment输入证明
  UDP demux前的explicit gate，不能让fragment payload被偶然解释为完整UDP packet。
- shared socket foundation容易诱发推测性TCP抽象。只冻结future TCP不得复制的owner truth，不预建完整framework。
- 静态部署依赖受维护的`eth0`事实。该取舍接受环境错配不在target内，不用runtime防呆掩盖部署错误。

## 收口

R0已经接受。Stage 0的positive topology decision、Stage 1 initial-domain/global-Stack walking skeleton与Stage 2
static control-plane/production-loopback均已独立关闭；两项domain/control contract cutover已经生效。Stage 3的3A
same-owner split与3B Endpoint/File/address lifecycle也已分别独立关闭。3B形成unbound Endpoint、bind/port0
namespace、anonymous `UdpSocketFile`、creation rollback/semantic final release以及`socket/bind/getsockname` RV64
用户纵切，并删除Stage 2 temporary probe；获批correction把namespace policy收归Stack构造、正确投影`S_IFSOCK`并
由ext4/devfs明确拒绝named Socket admission。capacity Route Correction删除private-layout predicate和eager reserve，
只保留kernel-owned semantic/direct-byte assertions。最终独立review为Apollyon/Keter/Euclid/Safe全0。Contract
Impact为None，candidate contracts继续Pending。

Checkpoint 3C已经关闭Stage 3：`sendto/recvfrom`在同一opened-description operation guard下完成implicit bind、user
copy与Stack admission，receive在Stack锁内detach后才执行payload/peer/addrlen copyout；RX aggregate credit在detach
后立即由Stack owner refill，fresh oversize失败保留implicit binding，任意specific `127/8` local source与bind admission
共用同一control-plane predicate。host topology 9/9、双架构app/kernel build与RV64 fresh-disk exact-source evidence
通过；RV64实际执行271/271 KUnit、`UDPTEST:SUMMARY:PASS:13`、`EPOLLTEST:SUMMARY:PASS:11`与LTP whitelist 4/4。
invalid-KernelConfig matrix因本checkpoint未修改配置owner/assertion而明确Not Re-run，只沿用3B已记录证据。

Checkpoint 4A已形成Stack-owned facts/coalesced invalidation、DomainStack锁外weak reverse routing与Socket-owned
association/复数interest-bearing `PollRoute` source；`FileOps::poll`以同一fresh facts实现snapshot/register，creation
rollback与semantic final release按撤publication、reverse route和routes后锁外通知。ready-at-register、复数route、
capacity recovery与ppoll/pselect/epoll ordinary LT均有host/KUnit/runtime证据。独立final review为Apollyon 0、Keter 0、
acceptance-affecting Euclid 0；可选的combined-case诊断改进不阻塞4A。final exact-source RV64执行273/273 KUnit、
`UDPTEST:SUMMARY:PASS:14`、`EPOLLTEST:SUMMARY:PASS:11`与LTP whitelist 4/4，并正常PowerOff。

Checkpoint 4B将唯一source-neutral wait loop下移到iomux owner，`ppoll/pselect6`与UDP blocking syscall共用
snapshot/register/schedule/final-scan协议。`sendto/recvfrom`在`WouldBlock`后释放operation guard再等待；send payload
只copyin一次并在每轮重做selection/admission，receive只在成功detach后保持transaction。NONBLOCK/DONTWAIT继续
映射`EAGAIN`且不修改status，signal final miss映射`EINTR`，Stage 3 temporary bridge已删除。RV64 deterministic
multi-waiter/signal case以局部procfs mount确认两个receiver同时park，并以bounded result poll证明无lost wake；最终
运行273/273 KUnit、UDP 15/15、epoll 11/11、LTP whitelist 4/4与正常PowerOff。

Checkpoint 4C完成general writable/provider backpressure、blocking send/receive与iomux并存、dup/close/port reuse、
并发copy-fault消费和first/later/fragment-pair真实注入证据。final review发现retire会漏掉empty-interest route的
final-close wake；修复改为撤publication/reverse route后锁外通知全部detached routes，ordinary invalidation仍保持
interest过滤，复审Apollyon/Keter/Euclid/Safe全0。final exact-source RV64执行274/274 KUnit、UDP 16/16、epoll
11/11、LTP whitelist 4/4与正常PowerOff。

Contract Impact为None；effective OPENED-DESC/IOMUX/EPOLL与Network/control-plane contracts保持Preserve，全部Socket/
Endpoint/UDP/wait candidate继续Pending。Stage 4 Closed；LA64 runtime、remote external、SMP>1、hardware、full
network LTP与final harness仍Not Run。Stage 4关闭后立即停止，只能由新的明确授权运行`4 -> 5`resolution，不得
自动激活Stage 5或执行current-contract cutover。
