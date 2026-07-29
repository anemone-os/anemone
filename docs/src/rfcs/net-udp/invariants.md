# net-udp 目标与不变量

**状态：** R0 Accepted Target / Stage 1 Domain Contract Cut Over / Stage 2 Ready / Later Candidates Pending
**最后更新：** 2026-07-29
**父 RFC：** [RFC-20260729-net-udp](./index.md)
**适用修订：** R0

本文定义`net-udp` R0的accepted contract delta、target invariants与RFC-local proof obligations。它不是current
contract；当前 effective 规则仍以 `docs/src/contracts/` 及已完成 cutover 的 source为准。Stage 1已使
`NETDEV-LIFE-001`、`NET-ATTACH-001`与`NET-IFACE-DOMAIN-001`生效；Stage 2虽已解析为Ready / Not Active，
`STM-TARGET-001`的network Refine与`NET-CONTROL-PLANE-001`仍未cut over，其余新增/Refine ID也仍是Pending
candidate。

本文不承担 implementation plan。concrete Rust type、internal API、lock primitive、worker、queue、buffer、
algorithm、module path、write set、probe 与验证命令均由[迁移实施计划](./implementation.md)按滚动阶段解析。

## 规则分类

- **Correctness Invariant：** 唯一 owner、身份隔离、handoff/linearization、并发、lifecycle、cleanup、无丢 wake、
  datagram ownership 和 ABI 诚实性；违反即实现不正确，不能登记为工程限制或以 target renegotiation 接受。
- **Target Guarantee / Capability：** 第一版 IPv4 unconnected UDP、loopback/external path、ABI envelope 和 evidence
  floor；可以通过显式 `Target Renegotiation Gate` 形成新修订，在此之前保持约束力。
- **Implementation Preference：** concrete `NetworkDomain` object、trait/enum、registry、table、锁、worker、queue、
  buffer、allocator、port selection algorithm、loopback medium 和 module layout；保持 target 时由 Ready stage 决定。

## Contract Impact

下表是 R0 target 的最小 contract closure。current links指向当前effective规则；SystemTarget相关规则已从
关闭的RFC与cutover history提取为本次触及的最小current baseline，没有批量迁移整个build/config领域。

| Contract ID | 变化 | 当前规则 | R0 Target 摘要 | 生效 Gate |
| --- | --- | --- | --- | --- |
| `NET-BOUNDARY-001` | Preserve | [Active](../../contracts/net/frame-path.md#net-boundary-001--frame-slice依赖方向与object-fence) | 保持frame shared API / kernel / stack / driver依赖方向和private object fence；不把Endpoint/UDP规则挤入frame-only ID | 全程保持 |
| `NET-FRAME-OWN-001` | Preserve | [Active](../../contracts/net/frame-path.md#net-frame-own-001--frame-backing只有一个访问owner) | external frame ownership与DMA/CPU handoff不变；loopback另受同等唯一packet-owner义务约束 | 全程保持 |
| `NET-FRAME-PROGRESS-001` | Preserve | [Active](../../contracts/net/frame-path.md#net-frame-progress-001--有界资源normal-backpressure与durable-recheck) | provider capacity、normal backpressure、durable recheck与fair progression不变 | 全程保持 |
| `NET-STACK-PUMP-001` | Preserve | [Active](../../contracts/net/frame-path.md#net-stack-pump-001--stack-instance唯一推进protocol-state) | current contract已允许一个Stack拥有多interface；target改为initial domain唯一Stack，不改变单instance唯一推进规则 | 全程保持 |
| `NETDEV-LIFE-001` | Refine | [Active](../../contracts/net/netdev-lifecycle.md#netdev-life-001--boot-time-identity与publication是单向transaction) | `device/net`保留external NIC publication identity/facts/capability；domain-local ifindex/name/kind与logical-interface lifecycle移入新的domain contract | Stage 1 `NET-UDP-DOMAIN-CUTOVER`（已完成） |
| `NET-ATTACH-001` | Refine | [Active](../../contracts/net/attach-lifecycle.md#net-attach-001--attach-publicationrollback与best-effort-shutdown) | attach authority从“每netdev新建Stack path”改为external NIC admission到initial domain/global Stack；保留publication-last、rollback isolation与shutdown admission | Stage 1 `NET-UDP-DOMAIN-CUTOVER`（已完成） |
| `NET-IFACE-DOMAIN-001` | Introduce | [Active](../../contracts/net/interface-domain.md#net-iface-domain-001--initial-domain拥有logical-interface-namespace) | initial domain唯一拥有logical-interface membership/lifecycle/ifindex/name/kind；`lo`与external interface共享这一namespace | Stage 1 `NET-UDP-DOMAIN-CUTOVER`（已完成） |
| `NET-CONTROL-PLANE-001` | Introduce | None（尚未生效） | 唯一拥有local address、route、source/interface selection policy与Stack projection边界 | Stage 2 `NET-UDP-CONTROL-CUTOVER` |
| `NET-PROTOCOL-BOUNDARY-001` | Introduce | None（尚未生效） | 固定kernel Socket/control plane与concrete Stack之间的Endpoint/UDP capability、依赖方向和object fence | Protocol capability cutover |
| `NET-SOCKET-ENDPOINT-001` | Introduce | None（尚未生效） | kernel Socket / File与Stack Endpoint的owner fence、opaque association、final-release retire与stale isolation | Socket/Endpoint lifecycle cutover |
| `NET-UDP-TRANSACTION-001` | Introduce | None（尚未生效） | bind namespace/commit、local delivery、send admission、datagram ownership/consumption、fragment rejection与同步失败边界 | Functional UDP cutover |
| `NET-SOCKET-WAIT-001` | Introduce | None（尚未生效） | protocol facts到Linux readiness/error的唯一投影、socket source publication与recheck义务 | Socket iomux cutover |
| `OPENED-DESC-001..003` | Preserve | [Active](../../contracts/task/opened-description-lifecycle.md) | published refs继续唯一决定final release；dup/fork共享description；socket只使用创建时固定的单final-release hook | 全程保持 |
| `IOMUX-POLL-001..003` | Preserve | [Active](../../contracts/iomux/poll-wait.md) | socket source遵守snapshot/register/final-scan、source-lock publication与wake-is-hint协议 | 全程保持 |
| `EPOLL-WATCH-001` / `EPOLL-READY-001` / `EPOLL-FILE-001` | Preserve | [Active](../../contracts/epoll/protocol.md) | socket只作为普通poll source加入，不改变watch owner、ready harvest与non-sleeping publication | 全程保持 |
| `STM-OWNER-001` | Preserve | [Active](../../contracts/configuration/system-target.md#stm-owner-001--每个配置事实只有一个规范-owner) | Platform/KernelConfig/SystemTarget/Preset owner分层不变 | 全程保持 |
| `STM-TARGET-001` | Refine | [Active](../../contracts/configuration/system-target.md#stm-target-001--systemtarget-是-bootdeploy-contract) | SystemTarget增加first-version static IPv4 deployment；不把network value移入Platform/rootfs/Preset | Stage 2 `NET-UDP-CONTROL-CUTOVER` |
| `STM-RESOLVE-001` | Preserve | [Active](../../contracts/configuration/system-target.md#stm-resolve-001--resolved-build-是不可手写的派生-snapshot) | resolver只物化本次typed input，不建立runtime deployment truth或兼容fallback | 全程保持 |

`NETDEV-LIFE-001` 当前把external netdev publication identity与ifindex/name共同交给`device/net`。R0 target采用
最小拆分：原ID继续描述external NIC publication transaction，但不再声称拥有domain-local ifindex/name；新的
`NET-IFACE-DOMAIN-001`承接logical-interface namespace。因为external publication identity/facts/capability与
publication-last、failure isolation仍由原owner和原协议延续，R0固定将该变化分类为Refine而不是Replace；
不能通过并行registry或兼容字段同时保留两份ifindex/name truth。

`NET-ATTACH-001`继续由kernel attach authority拥有完整external NIC admission协议。R0将它分类为Refine：
原有publication-last、mapping rollback、failure isolation、shutdown admission与provider retention继续有效，新增
logical-interface admission并把mapping destination改为initial domain/global Stack。把这段handoff另建为并列
协议会分裂同一次attach transaction，因此不采用Preserve加第二attach owner的表达。

Stage 1已按上述分类执行`NET-UDP-DOMAIN-CUTOVER`；三项current truth分别见
[Netdev lifecycle](../../contracts/net/netdev-lifecycle.md)、
[Attach lifecycle](../../contracts/net/attach-lifecycle.md)和
[Interface domain](../../contracts/net/interface-domain.md)。本页仍保存R0 target与cutover理由，不成为并列current
authority；functional loopback/control plane/socket相关candidate不因这一局部cutover提前生效。

SystemTarget三项已经提取到`docs/src/contracts/configuration/system-target.md`。`STM-OWNER-001`与
`STM-RESOLVE-001`保持current baseline；`STM-TARGET-001`当前仍不包含network deployment schema，R0只在
未来SystemTarget network-schema cutover对其Refine。Platform/KernelConfig/Preset的必要直接边界由同页owner
table覆盖，不额外批量迁移System Target Model的其它RFC-local invariant。

下文`NET-IFACE-DOMAIN-001`、`NET-CONTROL-PLANE-001`、`NET-PROTOCOL-BOUNDARY-001`、
`NET-SOCKET-ENDPOINT-001`和`NET-SOCKET-WAIT-001`直接对应候选current-contract条目。其它`NET-UDP-*`标题是R0 target/proof ID；其中
bind、datagram和capability规则在cutover时按共同owner/proof surface聚合进`NET-UDP-TRANSACTION-001`，不为每条
局部规则新建一份contract文档。

## Target Invariants

### NET-UDP-DOMAIN-001 — Initial domain只有一个global protocol Stack

**分类：** Correctness Invariant + Target Guarantee

**规则：** 第一版production只有一个initial network domain，该domain内只有一个global protocol Stack instance。
所有production loopback与external interface均映射到该Stack。Stack是该domain内protocol `InterfaceId` mapping、
UDP Endpoint/binding namespace、private protocol state、datagram storage、deadline和progression的唯一owner。

global表示domain-wide单一语义owner，不要求global static、singleton type、单一大锁、固定worker或CPU affinity。
`NetworkDomain`首先是语义边界；若第一版没有独立domain-owned mutable state，可以不引入concrete同名object，
但logical-interface membership/lifecycle仍必须有唯一owner且不得落回device/Stack/socket形成并列truth。

**Owner：** concrete protocol Stack拥有protocol state；network-domain interface owner拥有domain membership；
两者不能合并各自私有状态，也不能由第三个registry复制。

**依赖：** `NET-STACK-PUMP-001`、`NET-IFACE-DOMAIN-001`。

**违反表现：** per-netdev/per-interface/per-socket Stack成为并列endpoint owner；loopback使用另一endpoint namespace；
kernel缓存private SocketSet/Endpoint mapping；global被误写成固定大锁或唯一worker contract。

**Cutover：** Stage 1 `NET-UDP-DOMAIN-CUTOVER`必须先撤销current per-netdev Stack语义路径，再发布唯一domain
Stack；不能在两个wiring同时可用于protocol mutation时宣称生效。

### NET-IFACE-DOMAIN-001 — Logical-interface namespace与device publication分离

**分类：** Correctness Invariant

**规则：** initial domain内唯一logical-interface owner拥有membership/lifecycle、domain-local identity、ifindex、
name、kind与共同interface facts。loopback和external interface共享这一namespace与control-plane view；
external NIC publication仍由`device/net`拥有，queue/DMA/IRQ/current link/resource truth仍由provider拥有，
protocol `InterfaceId`与private engine identity仍由Stack拥有。

external published netdev进入domain是一次logical-interface admission与provider-capability handoff。该protocol的
唯一owner是`anemone-kernel::net`的domain/attach authority；它协调各owner的transaction-local result，但不复制
device registry、provider resource、control-plane table或Stack mapping truth。

**参与方局部义务：**

- `device/net`只移交opaque published capability与必要immutable facts，不让domain解引用driver backing；
- domain owner分配并发布logical identity/ifindex/name/kind，不从protocol `InterfaceId`反推；
- control plane只读取已发布interface fact并维护自己的address/route truth；
- Stack只分配private `InterfaceId`/mapping并接受窄provider port，不取得ifindex/name owner；
- provider保持frame/resource truth，notification只请求重读；
- admission authority在所有必要参与方准备完成后一次发布usable external path，失败只撤销本次transaction-local
  result并保持其它interface/domain state不变。

**身份域：** generic Device identity、external netdev publication identity、domain-local interface identity/ifindex、
protocol `InterfaceId`和private engine object identity互不等价、不可互换、不可跨owner解引用。

**失败/Cleanup：** publication前失败不留下半发布logical interface。external admission失败可以让netdev继续
保持published/unattached；一个external NIC失败不回滚`lo`或其它已发布interface。runtime detach/retry/reuse不在
第一版target中。

**依赖：** Refined `NETDEV-LIFE-001`、Refined `NET-ATTACH-001`、`NET-BOUNDARY-001`。

**违反表现：** domain和device各有ifindex/name registry；Stack映射成为Linux-visible interface identity；
domain保存provider queue/link truth；失败留下可被control plane/Stack部分观察的interface；通过兼容字段让旧新
registry同时驱动行为。

**Cutover：** logical-interface owner、external admission与global-Stack attach必须在Stage 1
`NET-UDP-DOMAIN-CUTOVER`中切换；旧`device/net` ifindex/name行为在新owner生效后不得继续作为决策输入。

### NET-UDP-LOOPBACK-001 — Loopback是domain-local first-class software interface

**分类：** Correctness Invariant + Target Guarantee

**规则：** 每个network domain必有一个loopback logical interface；第一版initial domain因此恰有一个
boot-persistent `lo`。domain只有在`lo`的logical identity、control-plane fact、Stack mapping与bounded
software-link capability准备完成后才达到第一版socket可用前提。

`lo`不需要generic Device/Driver identity，不伪造Ethernet MAC、ARP、IRQ、DMA、completion、hardware shutdown
或external published/unattached lifecycle。software-link只拥有protocol egress handoff到后续normal ingress
handoff之间的packet和bounded resource truth。

发往loopback route的packet必须在bounded network progression中重新进入同一Stack的normal protocol ingress，
继续服从protocol parsing、datagram boundary、pump budget、capacity/recheck和readiness protocol。socket syscall
不得直接访问peer Socket/Endpoint storage；host-only loopback/packet injection不得进入production dependency。

**Owner：** domain owner拥有`lo` membership/identity/lifecycle；control plane拥有`127.0.0.0/8`/local route和
source selection；Stack拥有protocol interface/progression；software-link owner拥有in-flight packet/capacity。

**失败/Cleanup：** `lo`不是optional external attach，不能以published/unattached表示其正常状态。concrete
construction failure、shutdown retention与cleanup由引入相应state的Ready stage闭合，但不得形成第二domain/
Stack或无界queue作为fallback。

**依赖：** `NET-UDP-DOMAIN-001`、`NET-IFACE-DOMAIN-001`、`NET-CONTROL-PLANE-001`、
`NET-FRAME-PROGRESS-001`的bounded/recheck原则。

**违反表现：** socket-to-socket copy；独立loopback Stack；让`device/net`为`lo`伪造hardware facts；无界local
queue；loopback backend私自识别local address/route；直接重入Endpoint state machine且绕过pump budget。

**Cutover：** production loopback与global Stack一起通过functional syscall/runtime evidence生效；host fixture
alone不能cut over本规则。

### NET-PROTOCOL-BOUNDARY-001 — Cross-crate protocol capability保持窄且非阻塞

**分类：** Correctness Invariant

**规则：** `anemone-net-api`只定义kernel与concrete stack共同需要的protocol-domain value、opaque identity、
operation outcome、snapshot、notification/recheck和必要narrow capability；它不拥有runtime state、registry、
waiter或Linux ABI policy。`anemone-smoltcp-stack`独占private smoltcp object和representation conversion；kernel
独占task/fd/wait/UAPI。

跨owner语义只允许：

- non-blocking Command/request，由protocol owner立即执行或返回normal not-ready/rejection；
- Event/invalidation edge，只说明owner fact可能变化并请求重查；
- point-in-time Snapshot/outcome，只服务当前operation/readiness interpretation。

三类语义不要求总enum、queue、callback family、trait hierarchy或统一snapshot struct。Stack只以`&mut self`/
`&self`表达owner-local mutation/observation，不内建lock、task、worker、waker、admission或cross-thread lifecycle。

**Owner：** shared semantic surface由`anemone-net-api`拥有；具体mutable state仍分别由kernel Socket、control
plane、Stack、interface owner与provider拥有。

**依赖：** `NET-BOUNDARY-001`。

**违反表现：** Stack接收fd/task/waiter/user pointer/Linux errno；kernel接收smoltcp handle/private buffer；
event直接发布readiness/error；snapshot长期缓存后驱动行为；API crate成为第二net core/registry；为了future TCP
预建无真实consumer的framework。

**Cutover：** dependency/public-surface audit与host/runtime integration共同证明；host-test-only injection/control
不得成为kernel dependency。

### NET-SOCKET-ENDPOINT-001 — Socket与Endpoint保持owner fence和单向association

**分类：** Correctness Invariant

**规则：** kernel Socket唯一拥有Linux-visible UAPI、blocking choice、wait/readiness/error interpretation和
opened-description交互；protocol Endpoint唯一拥有protocol-local lifecycle、committed bind、port reservation、
queue/capacity/error facts、datagram storage与private engine state。两者不能合并为跨kernel/stack共享对象，也
不能各自保存一份会驱动行为的endpoint/readiness/error/lifecycle truth。

socket private state和stack-independent opaque Endpoint association安装在`File::prv`。`FileOps`只有窄immutable
projection；`ProcFile`继续拥有description lifecycle/status，`FileDesc`继续拥有fd-slot publication/
fd-local flags。association只授权narrow operation/lookup，不允许反向解引用Stack-private object或复制mapping。

Socket/Endpoint不建立永久一一对应：创建失败允许无association；normal live path允许一Socket关联一active
Endpoint；final release允许先撤销association再发起retire；Endpoint可在association失效后完成owner-local cleanup。
任何延迟cleanup都不能恢复旧association、命中新identity或把旧datagram/error交给复用port的新Socket。

semantic final release是kernel侧正常close发起retire的唯一trigger。final-release hook只做non-blocking
retire/invalidation，不等待Stack progression、worker、lock acquisition with sleep或完整reclamation。`Drop`、raw
fd close、syscall-local borrow、`Arc` last drop和memory lifetime不替代`OPENED-DESC-001`的terminal event。

**Owner：** `ProcFile`拥有opened-description lifecycle；File private state拥有kernel Socket；Stack拥有
Endpoint mapping/resource；final-release hook只完成kernel-to-Stack retire request handoff。

**依赖：** `OPENED-DESC-001..003`、`NET-PROTOCOL-BOUNDARY-001`。

**违反表现：** 按fd number创建/retire Endpoint；close一个dup提前retire；kernel保存smoltcp handle/queue truth；
Endpoint持有ProcFile/task/waiter；memory Drop成为semantic close；old cleanup命中新generation/port owner；final
release阻塞等待protocol cleanup。

**Cutover：** socket/endpoint lifecycle tests必须覆盖creation rollback、dup/fork、one-alias close、semantic final
release、delayed retire、identity/port reuse isolation和shutdown/cancellation；具体generation编码留给Ready stage。

### NET-CONTROL-PLANE-001 — Address/route/selection只有一个行为权威

**分类：** Correctness Invariant + Target Guarantee

**规则：** network control-plane owner唯一拥有per-interface local-address configuration、local-address validity、
route和route/source/interface selection policy。它读取domain owner发布的logical-interface fact，但不取得
membership/lifecycle、provider resource、Endpoint、port或Linux errno owner。

每个已配置本地单播IPv4 address都形成优先于connected/default route的domain-local route。本地产生并发往本地
external-interface address的datagram必须完成route/source selection，再经bounded software handoff进入同一
Stack的normal IP/UDP ingress；不得进入external provider依赖backend hairpin，也不得由kernel Socket直接复制给
目标Socket。wildcard sender的source address是本次selection的operation-local result，不写回committed binding。

Stack可以使用control-plane提供的immutable/owned projection执行protocol operation，但该projection不能成为
第二份address/route table或独立selection policy；更新机制即使第一版只有boot-time一次性materialization，也
必须能够说明哪份state是行为权威。

kernel Socket负责Linux sockaddr/flag/options parsing与request normalization，不保存route或selection truth。
Stack负责bind/port与send admission，不通过private interface iteration自行发明不同selection policy。

**Owner：** network control-plane owner。domain owner、Stack和kernel Socket只拥有各自局部fact/capability。

**依赖：** `NET-IFACE-DOMAIN-001`、`NET-UDP-CONFIG-001`。

**违反表现：** kernel和Stack各有route table；SystemTarget、Platform和kernel runtime分别保存可独立变更的address；
loopback backend私自识别`127/8`；发往本地external address的流量偶然进入external provider或Socket fast path；
Stack根据private interface order形成另一selection policy；一次source selection反向更新binding；snapshot被更新并
作为长期并列truth。

**Cutover / evidence partition：** Stage 2 `NET-UDP-CONTROL-CUTOVER`要求host至少两个interface的deterministic
route/source/egress selection与self-external-address local delivery，并在RV64 production control-plane、bounded
local worker和normal protocol ingress上证明loopback与self-external local handoff。Stage 2尚无用户态UDP consumer，
因此真实remote external UDP ingress/egress不作为本ID在Stage 2生效的前置条件；它仍是R0 Evidence Matrix与Stage 5
final acceptance的强制证据，必须由后续`NET-UDP-TRANSACTION-001`/external-path closure在final exact code上证明。
local delivery不能替代external provider ingress/egress，单NIC结果不能证明selection；这一proof-stage分工不降低
R0 target或最终验收边界。

### NET-UDP-BIND-001 — Bind conflict、port allocation与commit属于Stack

**分类：** Correctness Invariant + Target Guarantee

**规则：** initial domain/global Stack唯一拥有active Endpoint binding namespace、committed local binding、
port reservation/conflict decision、ephemeral allocation与release。kernel Socket把ABI request normalize成
protocol-domain constraint；control plane只裁决explicit address是否属于本domain；二者都不维护并行reservation
或conflict set。

explicit bind的线性化点是Stack在唯一namespace内原子完成address constraint/port conflict check、port 0
selection/reservation与binding commit。port 0在reservation成功前不得向kernel/userspace发布。wildcard只是
committed binding中的“无具体local-address constraint”，不是kernel/control-plane单独记录。implicit bind使用同一
namespace和commit boundary。

第一版没有`SO_REUSE*`或bound-device constraint。同一port的conflict relation必须对新旧binding对称，并使用下表；
`A`与`B`表示两个不同、已经由control plane确认属于本domain的具体本地IPv4 address：

| Existing / New | Wildcard | Specific `A` | Specific `B != A` |
| --- | --- | --- | --- |
| Wildcard | Conflict | Conflict | Conflict |
| Specific `A` | Conflict | Conflict | Allow |
| Specific `B != A` | Conflict | Allow | Conflict |

port 0按请求的address constraint在同一transaction中选择满足该矩阵的端口。implicit bind提交wildcard address
constraint与ephemeral port；explicit wildcard/implicit bind的`getsockname`返回`0.0.0.0`和committed port。每次
`sendto`选择的source address只是operation-local result，不得写回binding；允许共存的different-specific binding
按ingress destination命中各自Endpoint。第一版不存在wildcard/specific demux precedence，因为二者不能共存。

具体ephemeral range/default/randomization/scan algorithm、storage和exhaustion errno留给Ready/ABI stage，不能改变
唯一owner、上述conflict relation与原子commit。

**Owner：** concrete Stack的Endpoint/binding authority。

**依赖：** `NET-CONTROL-PLANE-001`、`NET-SOCKET-ENDPOINT-001`。

**失败/Cleanup：** invalid/non-local address、conflict、range exhaustion或partial Endpoint construction不能发布
binding/port；retire后reservation释放必须与stale identity隔离。allocator OOM可以kernel-fatal，但普通capacity/
conflict/cancellation必须有normal rollback。

**违反表现：** kernel先选择并公开未保留port；control plane保存第二conflict set；per-interface namespace让同域
conflict分裂；wildcard与specific同端口共存；无理由拒绝different-specific同端口；一次source selection改写
wildcard binding；bind失败留下active reservation；old Endpoint delayed cleanup释放new owner的port。

**Cutover：** deterministic完整conflict matrix、port0/implicit wildcard、destination demux、exhaustion/rollback/
reuse-isolation proof与真实getsockname lifecycle runtime。

### NET-UDP-DATAGRAM-001 — Send success与datagram消费具有明确handoff

**分类：** Correctness Invariant + Target Guarantee

**规则：** datagram在kernel syscall transaction、cross-crate handoff、Endpoint storage、protocol egress、
frame/software-link和ingress delivery的任一时刻只有一个访问或消费owner。每次ownership transfer有明确commit；
failure发生在commit前则由当前owner rollback/retain，commit后前owner不得再访问或重复消费。

`sendto`只有在route/source/interface selection成功，或datagram已移交给保证不会再因缺少该selection而静默丢弃
的protocol owner后才能报告成功。no route/source/interface、oversize和normal Endpoint capacity exhaustion必须在
success boundary前形成typed protocol-domain outcome；kernel根据当前syscall/nonblocking context映射Linux result。

成功admission后的ordinary external link loss不反向改写syscall result。protocol engine因缺少source address而
dequeue/drop不构成合法成功。loopback仍通过protocol egress/normal ingress，不以Socket间copy绕过handoff。

receive保持datagram boundary；zero-length合法；short buffer复制prefix、返回copied length并消费完整datagram。
`MSG_PEEK`和显式`MSG_TRUNC`完整长度返回不在第一版target。Endpoint将队首datagram原子detach到operation-local
receive transaction的时刻是consume linearization point，并发生在任何用户payload、peer address或address-length
copyout之前。commit后Endpoint queue/readable与其它concurrent receive可以继续推进，kernel transaction成为该
datagram唯一owner。任一后续copyout fault返回相应copy error并丢弃datagram，不回滚、重新入队、重排或作为下次
调用可重试的数据；用户内存可能已经部分改变，本target不承诺多次copyout的原子rollback。具体owned payload/
token表示与copy顺序留给Ready/ABI stage，但不得持有Stack/source private lock跨user copy。

第一版只允许`MF = 0 && fragment offset = 0`的完整IPv4 packet进入UDP demux。任一fragmented packet必须在UDP
header parsing/Endpoint lookup前稳定丢弃，不建立reassembly buffer、timeout、overlap policy或fragment lifecycle，
不形成Socket readable/error，也不得把fragment payload解释为完整UDP datagram。未启用protocol engine
fragmentation feature不是符合性证据；实现必须提供并验证explicit ingress gate。

**Owner：** 每个阶段的当前datagram/packet holder；Stack拥有Endpoint queues，provider/software-link拥有自己的
in-flight resource，kernel只在copy transaction期间拥有kernel buffer/user-copy commit。

**依赖：** `NET-FRAME-OWN-001`、`NET-FRAME-PROGRESS-001`、`NET-UDP-BIND-001`、
`NET-CONTROL-PLANE-001`。

**违反表现：** success后才发现无source并静默drop；kernel与Endpoint同时访问同一mutable payload；copyout失败
后requeue或交给其它receiver；short receive只消费prefix；本地external address走backend hairpin；loopback绕过
normal ingress；fragment进入UDP demux；normal queue exhaustion panic/busy-spin；无界queue吸收backpressure。

**Cutover：** host deterministic ownership/capacity/failure/recheck/short/zero-length、payload/peer/address-length fault、
concurrent receive与first/later fragment rejection proof，RV64/LA64真实syscall copy、self-external local delivery与
loopback/remote-external双向runtime。未执行fragmented ingress不得被外推为拒绝正确或支持。

### NET-SOCKET-WAIT-001 — Protocol fact、wake与Linux readiness保持分离

**分类：** Correctness Invariant + Target Guarantee

**规则：** Endpoint唯一拥有支撑readable/writable/error判断的protocol facts；kernel Socket owner结合一次性
snapshot/outcome、opened-description status与syscall context解释Linux-visible readiness/error。kernel不得缓存
第二份queue/capacity/error truth，Endpoint event不得直接成为poll mask或errno。

socket source必须Preserve `IOMUX-POLL-001..003`：snapshot/register gate、source-state critical section中的route
publication/current predicate、notification后final recheck和cancellation cleanup。default blocking与
`O_NONBLOCK`/`SOCK_NONBLOCK`/`MSG_DONTWAIT`读取同一not-ready predicate；per-call flag不修改opened-description
status。blocking path不得busy-poll。

source notification只持non-owning recheck capability，不持task/waiter或consumer lifecycle truth。close/retire
先撤销source publication/association，再完成owner-local cleanup；晚到/重复edge对retired round fail closed，不能
发布旧Endpoint readiness给新Socket。

第一版覆盖ordinary level-triggered readable/writable。ordinary UDP writable是destination-independent general
admission predicate：Endpoint必须live、未retire，并能够立刻接纳至少一个非空、第一版支持范围内的datagram进入
自己的bounded TX admission storage。未显式bind本身不使live socket不可写；implicit bind conflict/exhaustion在
具体`sendto`中裁决。writable不承诺任意future destination、任意datagram length或当前provider可立即发送；no
route/source/interface、oversize与request-size-specific capacity failure仍由每次`sendto`返回。

provider backpressure只有在阻塞progression并耗尽Endpoint-owned admission capacity时才间接清除writable；kernel
Socket不得读取或复制provider queue/link truth，private protocol engine的近似`can_send`也不能未经admission
等价性证明直接成为Linux predicate。capacity恢复由Endpoint owner更新truth并发布recheck edge；event本身仍不是
writable。ICMP async error、`SO_ERROR`和完整`POLLERR`后延，但kernel Socket仍保留future Linux error publication
的唯一owner。

**Owner：** Endpoint owner拥有protocol predicate facts；kernel Socket/source owner拥有Linux projection和route
registry；iomux/epoll拥有wait round/watch/ready harvest。

**依赖：** `IOMUX-POLL-001..003`、`EPOLL-WATCH-001`、`EPOLL-READY-001`、`EPOLL-FILE-001`、
`NET-SOCKET-ENDPOINT-001`。

**违反表现：** wake/event payload直接返回用户；lost subscription window；source未注册却sleep；Socket复制RX/TX
或provider queue count驱动readiness；route不存在就永久清除writable；private engine近似predicate未经证明直接
投影；MSG_DONTWAIT改变status；final release等待waiter；old edge命中新association。

**Cutover：** focused initial-unbound writable、route failure while writable、request-size-specific failure、Endpoint
TX saturation/recovery、provider-backpressure propagation、blocking/nonblocking/signal/cancel/poll/select/epoll/dup/
fork/close races与source protocol audit；socket-specific ET/oneshot扩展不作为本RFC新增proof claim。

### NET-UDP-CONFIG-001 — Static deployment只做基本合法性检查

**分类：** Correctness Invariant + Target Boundary

**规则：** 支持本RFC external-path acceptance的SystemTarget拥有first-version static IPv4 deployment：选定的
domain-local external interface name、address/prefix和目标环境需要的default route。connected route从address/
prefix唯一派生。Platform只拥有guest machine/device/QEMU topology，KernelConfig只拥有feature/policy/capacity，
Preset只做selection，rootfs不保存或注入同一network truth。

build loader/resolver只要求基本机械合法性：schema结构、类型、必填字段、unknown field、IPv4/prefix可解析性和
明显范围错误。它不证明interface实际存在、name与runtime enumeration一致、gateway可达、address不冲突、route
符合外部拓扑、QEMU backend配置正确或部署环境满足测试条件。

第一版允许具体SystemTarget按受维护的target/Platform事实引用`eth0`。维护者负责保证对应device topology和
deterministic discovery/publication order；合法配置在runtime找不到`eth0`属于deployment precondition violation，
不属于本RFC的产品输入、恢复语义或acceptance matrix。本RFC不要求alternate-interface search、MAC/bus selector、
fallback、retry、degraded mode、compatibility alias、preflight probe或专用state machine，也禁止静默选取其它
interface。该范围外输入仍不得破坏memory/resource safety，但不规定boot survival、专用errno、日志或自动修复。

loopback无enable/count配置，实例数从domain数推导；`127.0.0.0/8`、header length、port width等protocol fact不是
deployment knob。重要capacity只有在Ready stage证明concrete bounded resource与独立policy后才进入KernelConfig，
不能为每个literal创建knob或让SystemTarget/KernelConfig同时拥有同一值。

**Owner：** SystemTarget拥有deployment declaration；resolver/materializer拥有本次typed projection；kernel
control plane消费projection但不解析TOML；target/Platform maintainer拥有声明与部署环境一致的人工责任。

**依赖：** effective `STM-OWNER-001`、`STM-TARGET-001`、`STM-RESOLVE-001`，以及
`NET-CONTROL-PLANE-001`。

**违反表现：** Platform/rootfs/Preset复制guest IP/route；resolver宣称证明runtime reachability；kernel解析TOML；
找不到`eth0`后静默改用其它interface；为了防误用引入第二selector/probe/fallback truth；connected route由配置和
派生逻辑各保存一份。

**Cutover：** SystemTarget schema/materialization与control-plane cutover原子更新；当前effective baseline在
cutover前保持不变。验证证明合法/基本非法输入边界与targeted QEMU deployment，不宣称穷举或完全校验。

### NET-UDP-CAPABILITY-001 — 第一版IPv4 unconnected UDP能力包络

**分类：** Target Guarantee

**规则：** 第一版必须由普通用户态经真实syscall/fd/copy/wait/lifecycle路径完成：

- `socket(AF_INET, SOCK_DGRAM, 0|IPPROTO_UDP)`；`SOCK_NONBLOCK|SOCK_CLOEXEC`；
- `bind`具体地址/wildcard、port0；`getsockname`观察committed result；
- `sendto` explicit destination与implicit bind；
- 本地产生并发往本地external-interface address的datagram经local route/software handoff/normal ingress交付；
- `recvfrom` payload/peer address、zero length、short buffer consume-whole、copy fault consume-on-dequeue；
- `close` final release、`fcntl(O_NONBLOCK)`、poll/select/epoll、dup/fork互操作；
- loopback和一个production external interface；external ingress/egress必须走真实frame path；
- MTU内无需fragmentation/reassembly的bounded datagram；fragmented ingress在UDP demux前拒绝。

unsupported family/type/protocol/flag/feature稳定拒绝，不能通过host-only入口、kernel injection、静默no-op或成功后
drop冒充capability。完整BSD socket、network LTP或本文件非目标不因基础syscall存在而进入claim。

**Owner：** kernel Socket/UAPI owner、Stack/Endpoint owner、control plane、logical-interface owner与provider各自
承担前述local obligation；本target不创建综合“UDP manager”owner。

**依赖：** 本文全部Correctness Invariants。

**违反表现：** 只有host packet smoke；self-external local delivery冒充remote external production proof；固定port
测试回避port0/implicit bind；copy fault后重新交付datagram；fragment payload进入UDP；blocking syscall busy-poll；
RV64结果外推LA64；unsupported flag静默改变用户可见行为。

**Cutover：** 只有最终functional UDP cutover满足本文Evidence Matrix后生效；partial stage只能记录execution fact，
不能更新current contract或宣称first-version UDP closed。

## RFC-local Invariants

- RFC前私有定位只作为历史讨论输入；公共R0形成后，target修正折回`index.md`/本文件，私有材料不成为
  公共链接或并列target authority。
- accepted-before-cutover不得把global Stack、logical interface、UDP syscall或SystemTarget network schema
  写成current fact；current per-netdev wiring和existing contracts继续有效。
- implementation probe只能验证route、Stack integration、loopback handoff、readiness/copy等假设；probe code不得
  在target/contract未接受时自然沉淀为长期public API、第二owner或compatibility bridge。
- 临时双wiring、legacy ifindex projection或host control若在Ready stage确有必要，必须说明唯一behavior authority、
  可见边界、观测方式和删除/cutover gate；不得让两条路径同时处理production protocol mutation。
- 工程证据可以触发Target Renegotiation Gate，但不得自行降低functional capability、LA64 evidence、external
  path、send-success honesty、owner/lifecycle correctness或配置owner边界。

## 状态所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 明确不拥有 |
| --- | --- | --- | --- |
| initial-domain membership/lifecycle | domain logical-interface owner | opaque identity / immutable fact | provider backing、address/route、protocol mapping |
| domain-local ifindex/name/kind | domain logical-interface registry | stable identity projection | device identity、`InterfaceId` |
| external NIC publication identity/facts | `device/net` | published capability/snapshot | domain ifindex/name、route、Endpoint |
| queue/DMA/IRQ/completion/current link/resource | concrete provider/driver | frame token、recheck edge | interface membership、readiness、route |
| local address/route/source/interface policy | network control plane | point-in-time selection/projection | Endpoint/port、provider queue、Linux errno |
| protocol InterfaceId/Endpoint/bind/port/queue | global Stack | opaque identity、outcome/snapshot | fd/task/waiter/user pointer、Linux readiness |
| kernel Socket/private association | `File::prv`中的kernel Socket owner | File operation projection | private smoltcp object、binding mapping copy |
| opened-description lifecycle/status | `ProcFile` / `OPENED-DESC-*` | published FileDesc、transient borrow | Endpoint lifetime truth、fd-local flags |
| fd slot/publication/fd-local flags | fd table / `FileDesc` | fd key | Socket/Endpoint identity |
| Linux readiness/error projection | kernel Socket source owner | iomux route/recheck hint | protocol queue/error truth copy |
| wait round/watch/ready harvest | existing iomux/epoll owners | non-owning source route | source predicate、Endpoint state |
| local-delivery in-flight packet/capacity | software-link owner | transfer token/recheck edge | route/address/Endpoint/readiness |
| static deployment declaration | SystemTarget | resolved typed input | runtime topology/reachability proof |

不得通过综合`NetworkState`、`NetdevLifecycle`、`SocketState`或generic manager复制上述多owner facts。跨owner需要
组合观察时使用operation-local transaction/snapshot，不保存可独立更新的综合truth。

## 身份与能力模型

- external Device/netdev identity只证明device publication；不能用作domain ifindex或Stack lookup key。
- domain-local interface identity/ifindex在本次boot内稳定；第一版无runtime reuse/generation contract。
- protocol `InterfaceId`只在Stack owner内定位private mapping，跨边界最多作为opaque capability，不等同ifindex。
- Endpoint association需要opaque identity/capability并隔离stale cleanup；具体是否命名`EndpointId`、是否包含
  generation和storage representation由Ready stage决定。
- fd number是可复用slot，不是Socket/Endpoint identity。dup/fork共享opened description；semantic final release
  由published description refs决定。
- wildcard/implicit binding只保存`0.0.0.0` constraint与committed port；一次`sendto`选择的source address是
  operation-local result，不是新binding identity或可写回state。
- notification/wake capability只请求重查，不证明owner fact、readiness、liveness或operation success。
- diagnostic ID/label若后续引入，不得驱动behavior或替代上述identity；一旦参与决策必须升级为显式protocol state。

## Cross-owner handoff

| Handoff | Protocol Owner | Participant-local obligation | Commit / publication | Failure / cleanup |
| --- | --- | --- | --- | --- |
| external NIC进入domain | kernel domain/attach authority | device移交published capability；domain分配logical identity；control plane接受interface fact；Stack建立private mapping；worker/progression准备 | 全部准备后发布usable external path | 撤销transaction-local mapping/record，netdev保持published/unattached；不回滚`lo` |
| loopback建立 | kernel domain/attach authority | domain建立`lo` identity；control plane建立loopback fact/local route；Stack建立interface；software-link准备capacity/recheck | initial domain对socket可用前一次发布 | 不用external published/unattached fallback；不得留下部分可见`lo` |
| explicit/implicit bind | Stack binding authority | kernel normalize；control plane校验explicit local address；Stack conflict/allocate/reserve | binding/port在Stack内原子commit | commit前不发布port；失败撤销partial Endpoint/reservation |
| `sendto` | Stack/UDP transaction owner | kernel copy/normalize；control plane selection；Endpoint capacity/admission；后续provider owns packet | selection成功且datagram进入不会因缺selection丢弃的owner | commit前返回typed failure；commit后前owner不再访问 |
| local delivery | Stack/UDP transaction owner | control plane选择local route/source；Endpoint/egress移交packet；software-link bounded handoff；Stack normal ingress | packet进入不会再走external provider且保留normal ingress义务的software owner | software capacity不足时当前owner retain并durable recheck；仍在send commit前可返回not-ready，send success后不得backend hairpin、Socket fast path或silent drop |
| receive/copyout | kernel Socket transaction与Endpoint owner | Endpoint原子detach队首datagram；kernel独占operation-local payload/peer并copyout | Endpoint detach是consume point，先于任一user copy | copy/cancel fault丢弃当前datagram；不requeue、重排、double consume或交给其它receiver |
| readiness wait | kernel Socket source owner | Endpoint提供snapshot/event；source原子publish route/current predicate；iomux/epoll final recheck | source route publication与current predicate同一protocol point | retire/cancel撤销publication；late edge fail closed |
| final close | opened-description lifecycle owner | `Live(1)->Retired`后调用固定hook；Socket撤销association；Stack接受nonblocking retire | semantic final release | 不等待cleanup；delayed cleanup隔离new identity/port |
| static config materialization | build resolver/materializer | SystemTarget声明；resolver基本解析/typed materialize；kernel control plane消费 | resolved build/input handoff | runtime environment mismatch在target外；无fallback/alternate selector |

## 线性化点

- Logical interface：domain/attach authority在所有必要owner-local resource准备后发布membership/usable fact。
- Stack mutation：取得同一global Stack的唯一`&mut`/pump capability；具体lock/actor不是target。
- Bind：Stack内conflict check、port reservation和committed binding的原子commit。
- Send success：selection完成且datagram ownership commit到不会因缺route/source/interface静默drop的owner。
- Local delivery：selection命中domain-local route，packet commit到保留normal ingress义务且不进入external provider的
  bounded software owner。
- Receive consume：Endpoint把队首datagram原子detach到operation-local receive transaction；发生在payload/peer/
  address-length copyout前，fault不回滚或重新入队。
- Readiness subscription：source-state owner同一critical section内route publication与current predicate observation。
- Wait return：iomux/epoll读取source current predicate的final scan，不是event/wake arrival。
- Socket retire trigger：opened description首次`Live(1) -> Retired`，不是fd number close或memory last drop。
- Contract cutover：对应implementation gate同时满足source、tests、docs/current-contract update；partial success不生效。

## 锁序与生命周期规则

R0不冻结lock primitive或完整lock order，但冻结以下不可违反的边界：

- Stack owner外不得持有或暴露Stack-private lock；所有protocol mutation经过唯一access window。
- provider/driver callback不得在持有protocol-owner或device-wide lock时执行可能重入的cross-owner operation。
- source owner更新predicate并snapshot/detach routes后，在source lock外notify/drop route。
- receive transaction取得独占datagram ownership后必须释放Stack/source private lock，再执行可能sleep/fault的user
  copy；copy failure只drop transaction-local datagram，不重新取得queue owner做rollback。
- final-release先撤销Socket association/source publication，再发起non-blocking Endpoint retire；cleanup/Drop先撤销
  对外可见状态，再以常开assert暴露遗漏。
- wait/cancellation/shutdown不能持fd-table、source、Stack或provider private lock跨sleep/join；第一版normal final
  release也不能等待worker/progression。
- old notification、identity、Endpoint、port或interface capability在retire后fail closed；不得恢复publication。
- provider/device无法证明quiesce时Preserve `NET-ATTACH-001` retention到reset/power-off，UDP不得为完整reclaim
  反向发明新的driver lifecycle。

具体lock order、operation batching、mailbox/worker形状、reentrancy防护与shutdown sequence必须在引入相应state的
Ready stage闭合，并受以上owner/handoff约束。

## Heap OOM与normal capacity

`anemone-net-api`和`anemone-smoltcp-stack`不为kernel heap allocation failure暴露`AllocError`、OOM outcome、
event/snapshot或fallback contract。普通infallible allocation失败可以kernel-fatal；host proof不要求allocator
failure injection或OOM recovery。

这不覆盖可预期的bounded resource exhaustion。Endpoint RX/TX storage、binding/port capacity、local-delivery
handoff、frame credit或其它明确capacity用尽必须返回normal、可重查的not-ready/exhausted outcome，不得panic、
busy-spin或伪装成OOM。来自UAPI/config的length/count必须在allocation前做supported bound和integer overflow检查。

## 禁止退化项

- 以current per-netdev Stack wiring固化per-interface Endpoint/bind namespace。
- 用第二Stack、host fixture或Socket间copy实现production loopback。
- 让发往本地external-interface address的datagram进入external backend hairpin，或以local delivery冒充真实remote
  external ingress/egress proof。
- 让`device/net`为`lo`伪造Device/MAC/IRQ/DMA/hardware lifecycle。
- 让Stack拥有ifindex/name/address/route policy，或让control plane保存Endpoint/port truth。
- 让kernel保存private smoltcp handle、Endpoint queue/capacity/error mirror，或让Stack接收task/fd/waiter/Linux errno。
- 用event/wake payload直接形成readiness/error，或跳过final predicate recheck。
- 用destination route、temporary provider queue或未经证明的private engine近似predicate直接形成Socket writable。
- `sendto`先返回成功再因缺route/source/interface静默drop。
- 在payload/peer/address-length copyout fault后把已detach datagram重新入队、重排或交给其它receiver。
- 因未启用reassembly就假设fragment安全拒绝，或让first/later fragment进入UDP header parsing/Endpoint lookup。
- 以fd number、memory last drop、raw`Arc` count或one-alias close触发Endpoint retire。
- 让old cleanup释放new generation/port owner，或让old datagram/error进入复用Socket。
- 用unbounded queue、busy-poll、fatal normal exhaustion或隐式fallback掩盖未闭合resource protocol。
- 为部署错配增加alternate interface search、MAC/bus selector、retry/degraded state或第二配置truth。
- 用RV64/host/loopback结果外推LA64/external path/hardware/SMP，或把`Not Run`写成通过。
- 为future TCP零修改接入预建无真实consumer的trait/manager/dispatch framework。

## R0前已关闭的Target决策

- **Local delivery：** 本地产生并发往本地external-interface address的datagram命中优先domain-local route，经
  bounded software handoff与同一Stack normal ingress交付；不走external backend或Socket fast path。
- **Bind conflict：** 无`SO_REUSE*`时wildcard与任一同端口binding冲突，相同specific冲突，不同specific允许；
  port0、implicit wildcard与getsockname服从同一committed binding语义。
- **Writable：** 只表示live Endpoint能够接纳至少一个非空target datagram的一般TX admission capacity；destination、
  request length与provider truth不成为Socket并列predicate。
- **Receive consume：** Endpoint detach是copyout前的消费线性化点；payload、peer address或address-length fault不
  rollback/requeue。
- **Fragmented ingress：** first/later fragment都在UDP demux前稳定丢弃，不建立reassembly或readiness。

这些结论已经折回各自normative invariant。future implementation resolution可以决定local medium、binding table、
port allocator、capacity representation、owned receive token、copy顺序与fragment gate placement，但不能静默改变
上述用户可见行为；需要改变时必须返回RFC review / Target Renegotiation Gate。

## Evidence Matrix

| Claim | Host deterministic | RV64 QEMU agent-run | LA64 QEMU user-run | 不能外推 |
| --- | --- | --- | --- | --- |
| global Stack/multi-interface selection | 至少2 interface，route/source/egress、self-external local route与single-owner | loopback + self-external + 1 remote external | 同case/同源码 | host不证明kernel wiring；self-local不证明provider path；single NIC不证明selection |
| bind/port/Endpoint lifecycle | 完整wildcard/specific matrix、port0、implicit wildcard、destination demux、rollback、retire/reuse | real syscall/getsockname/close/dup/fork | 同case/同源码 | fixed port或单地址smoke不证明namespace |
| datagram/capacity | zero/short/oversize/exhaustion/recheck、3类copy fault consume、concurrent receive、first/later fragment rejection | real copyin/out + loopback/self-external/remote-external | 同case/同源码 | allocator OOM不在proof；未测fragment不证明安全拒绝；local不证明frame path |
| wait/readiness | unbound/general predicate、route/size failure、Endpoint saturation、provider propagation、register/recheck/cancel races | blocking/nonblocking/signal/poll/select/epoll | 同case/同源码 | wake marker不证明predicate；writable不承诺任意send；ordinary LT不外推完整ET matrix |
| loopback | bounded handoff/normal ingress/resource recovery | production `lo` syscall flow | 同case/同源码 | host smoltcp loopback不证明production path |
| external path | deterministic provider ingress/egress/failure isolation | production VirtIO ingress + egress | production LA64 selected NIC | QEMU不外推hardware/virtio-pci |
| build/static config | basic valid/invalid schema/materialization | selected target deployment | selected target deployment | 不证明arbitrary runtime topology、reachability或missing `eth0` behavior |
| SMP safety | source/concurrency audit与host races | `smp=1` mandatory | `smp=1` mandatory | 不外推`smp>1` runtime；未运行写`Not Run` |

具体test case、command、marker、capacity和stage分布由future implementation resolution决定。Evidence Matrix只固定
claim owner和最低证据类别，不成为执行计划。

## 完成标准

R0 acceptance review已经确认：

- 本文五项R0 target decision已关闭并折回normative sections；
- Contract Impact已完成最小current baseline提取，并固定`NETDEV-LIFE-001`与`NET-ATTACH-001`均为Refine；
- 每个state、identity、handoff、commit、failure、cancellation与cleanup均有唯一owner；
- first-version ABI/capability、non-goal、configuration boundary和evidence claim自洽；
- 独立[迁移实施计划](./implementation.md)只把第一个可执行Stage 0完整解析为Ready，且不自动授权执行。

最终RFC closure还必须：

- 每个Preserve contract保持source/tests一致；每个Refine/Introduce ID在明确gate原子cut over并写入current contract；
- host、双架构build、RV64 agent-run与LA64 user-run达到Evidence Matrix，未执行项如实记`Not Run`；
- current per-netdev Stack/old ifindex owner/临时bridge不再驱动production behavior；
- accepted limitation位于target外，target内失败进入open issue而不是改名为限制；
- transaction记录每个contract ID的Effective/Pending/Not Cut Over结果与最终validation claim。
