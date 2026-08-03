# ICMP Raw Socket RFC 前定位共识

**状态：** Archived RFC Background / Superseded by Draft body
**最后更新：** 2026-08-03
**范围：** IPv4 ICMP raw Socket、Socket front 反馈、Network Stack Endpoint、运行期验收边界

## 文档边界

本文归档公共 RFC 形成前的历史定位材料，已经由
[`RFC-20260803-icmp-raw-socket`](../index.md)取代，不再定义 proposal、target、Implementation Boundary、Contract
Impact 或 acceptance。公共 Draft 是唯一 canonical target source；当前 effective 行为仍以 live source 与
[`docs/src/contracts/`](../../../contracts.md)为准。

公共 Draft publication 没有授权实现、contract cutover、register 变化或下一 gate。本文已冻结，只保留早期调查与讨论
脉络；后续 review 与 implementation feedback 必须写回 canonical RFC、current contract、register 或执行证据，不能继续
维护本文形成并列权威。文中对当时本地竞赛测试快照与工具行为的描述只记录历史调查上下文，不是可复用的公共 citation
authority；若后续决策重新依赖这些事实，必须针对固定公共来源或当前 tracked input 重新核验。

本文选择的首版边界是“自然闭合的 IPv4 ICMP raw datagram Socket”，不是从某个 BusyBox `ping` 最短调用轨迹倒推的
紧包络。已经确认的能力包络仍只是公共 RFC R0 的候选；公共 review 可以在不破坏 correctness invariant 的前提下基于
真实工程证据适当扩展或收缩。RFC 接受前由 review 明确修订，接受后的变化进入 Route Correction、Target
Renegotiation 或 follow-up RFC；实现不能以“工程方便”为由静默改变 target、owner、ABI、acceptance 或验证强度。

## 已达成共识

### 工作顺序与价值

- 在 TCP 之前先完成一条 IPv4 ICMP raw Socket vertical slice。这条能力能够让当前 BusyBox `ping` 通过真实
  `AF_INET + SOCK_RAW + IPPROTO_ICMP` consumer 使用网络栈，并给用户态提供常见、直接可观察的网络诊断工具。
- 这项工作按 RFC 分类，但目标应保持紧凑；“小 RFC”表示语义范围窄，不表示可以省略 owner、handoff、failure、
  cleanup、ABI、acceptance 或 contract delta。
- 用户态验收采用三层互补证据：ICMP raw focused guest suite 负责 target-complete ABI/lifecycle/readiness matrix，
  curated Socket LTP group 负责真实glibc/musl与既有Socket family的上游兼容回归，双架构QEMU-router BusyBox
  `ping`负责可重复的真实frame/Stack/topology round trip。三层不能互相替代；公网、DNS与入站peer属于其它
  environment-qualified claim，不自动进入这三层cutover floor。
- raw Socket 是 general Socket front 的第三个异构 consumer，也是 UDP 之后第二个真实 Network Stack Endpoint
  family。它应反馈已经由真实 raw consumer 暴露的框架问题，但不得借机预建 TCP operation surface、统一 connection
  state、万能 Endpoint trait、backend registry 或其它没有当前 consumer 的抽象。
- raw Socket 能降低 TCP 进入 Socket/Network framework 时的未知数，但不证明 TCP 的 connect/listen/accept、重传、
  拥塞、pending error、half-close、orphan close 或协议时序已经准备完成。
- 首版应让普通程序获得一只可创建、bind/connect、send/receive、read/write、阻塞与iomux复用、查询及有限调节
  protocol policy的完整ICMP raw datagram Socket；它不因这些自然能力而扩大为任意IP protocol、IPv6或通用raw-IP
  framework。

### 首版用户可见能力方向

- 首版聚焦 IPv4 `AF_INET + SOCK_RAW + IPPROTO_ICMP`，不以“任意 IP protocol 的通用 raw Socket”为目标。
- raw Socket 创建必须由 Socket syscall/credential boundary 检查调用者的 effective `CAP_NET_RAW`；Network Stack、
  smoltcp Endpoint 与 driver 不接收 task、credential 或 Linux capability。tuple/flag先完成ABI归一化，权限拒绝发生在
  fd reservation、Endpoint创建与其它可见资源publication之前；创建成功后调用者降低权限不撤销既有opened
  description的raw能力。
- R0纳入IPv4 local-address `bind/getsockname`与connected raw Socket：`connect/getpeername`提交默认peer与对应RX
  source filter，`connect(AF_UNSPEC)`解除association；显式destination的`sendto`可以覆盖默认peer。local binding、
  connected peer、自动选择source与ingress filter必须由一个raw Endpoint owner原子提交和查询，kernel不能保存第二份
  近似truth。重复bind/connect、disconnect后source保留及`sockaddr_in.sin_port`投影的精确Linux矩阵仍须在公共RFC前
  由固定oracle冻结，但port不得演化为任意protocol truth。
- R0成功I/O包括`sendto/recvfrom`、libc经同一syscall路径提供的`send/recv`，以及raw Socket的`read/write`；`write`
  要求connected peer，`read`不要求connect。common Socket FileOps必须通过family-neutral normalized request表达
  stream与packet file-I/O差异，不得为raw增加downcast、旁路FileOps或复制第二套blocking loop。
- send flags限定为`MSG_DONTWAIT`及明确的`MSG_NOSIGNAL`兼容；raw send没有peer-close/SIGPIPE producer，后者的可见
  语义因此为空，但实现必须以关键注释和低噪声诊断保留该ABI选择，不能据此默许其它flag。receive flags纳入
  `MSG_DONTWAIT`、`MSG_PEEK`与`MSG_TRUNC`：短buffer默认复制prefix并消费整包，`MSG_TRUNC`返回原包完整长度，
  `MSG_PEEK`在成功、短copy与copy fault后都不消费。非peek copy fault可以消费已经detach的packet。
- zero-length receive仍等待并观察一份packet；非peek成功返回0并消费，`MSG_TRUNC`返回该packet完整长度。zero-length
  send是合法的零字节ICMP payload raw datagram并形成IPv4 protocol-1 packet，不能被common adapter提前变成
  success-no-op。
- descriptor/query surface纳入`SO_DOMAIN`、`SO_TYPE`、`SO_PROTOCOL`与`SO_ACCEPTCONN=0`。首版真实mutable option是
  `IP_TTL`、`IP_TOS`与`ICMP_FILTER`的`setsockopt/getsockopt`；default TTL是重要部署常量，必须进入Kconfig，TOS默认
  为0，ICMP filter默认接收全部type。TTL/TOS由kernel raw family在每次send取得immutable snapshot后归一化交给Stack；
  ICMP filter由Stack raw owner在fanout计费和queue admission前提交、查询与执行。
- `SO_RCVBUF`、`SO_BROADCAST`、`SO_BINDTODEVICE`、`IP_MULTICAST_IF`、`IP_HDRINCL`、`SO_ERROR`及其它未选择option继续
  返回`ENOPROTOOPT`，不能以BusyBox忽略错误为由success-no-op。`sendmsg/recvmsg`、cmsg/ancillary data与
  `sendmmsg/recvmmsg`属于独立的common Socket message ABI工作，不建立raw-only半套入口。
- `socketpair/listen/accept/shutdown`没有R0成功语义，按Linux-compatible unsupported/not-connected矩阵明确拒绝；
  connected raw不因此取得stream half-close、peer-close、pending error或HUP/RDHUP状态机。
- R0成功发送只接受Linux IPv4 raw Socket的非`IP_HDRINCL`形状：用户提供ICMP message，Socket ABI拥有user copy与
  destination normalization，initial-domain control plane唯一选择route/source/interface，domain Stack raw owner根据
  operation-local selection与TTL/TOS snapshot形成无IP option、非分片IPv4 header并提交packet。ICMP checksum仍由
  用户态形成，内核不校验或把ICMP message解释成自身控制状态；发送超过selected interface MTU减IPv4 header的message
  返回`EMSGSIZE`，R0不以隐式fragmentation掩盖。三方具体prepare、调用与锁顺序不在positioning中冻结，但不得改变
  Linux-visible copy/error/commit边界。
- R0明确不引入`IP_HDRINCL` mutable option；对应`setsockopt`/`getsockopt`继续按unsupported option返回
  `ENOPROTOOPT`。这是target范围选择，不表示full-packet数据形状更难；它把user-supplied IPv4 header fidelity、字段
  修补、outbound IP option、fragmentation/PMTU policy与`IP_HDRINCL`并发snapshot留给后续显式target review，不因
  backend恰好接收完整IP packet而自动进入R0。
- 接收在interface/IP owner完成local-destination admission后，从仍处于该mutation window的原始、未分片IPv4
  datagram建立每Endpoint独立计费的detached delivery，并向用户态保留该datagram在IPv4 `total_len`内的原始字节，
  包括header、IP option、TOS、ID、flags与ICMP message；不得从会丢字段的`Ipv4Repr`近似重建。`recvfrom`的peer
  address来自packet source，port为零。invalid IPv4 header/checksum不交付；通过IPv4 admission后的ICMP checksum、
  type与body保持raw-visible，普通ICMP processing可独立拒绝且不得被raw observation抑制。R0不接收或重组IPv4
  fragment。
- blocking/nonblocking、`poll`/`select`/`epoll`、opened-description final release、fd publication rollback 和
  Linux-visible errno/copy ordering必须复用当前 Socket front 与 iomux 协议，而不是建立 raw-only wait loop、ready
  cache 或 close trigger。readable只从当前RX queue事实派生；writable只表示raw owner当前可接受一次最小TX commit，
  不缓存route、不要求Socket已connect，也不保证任意request-specific destination/length成功。
- raw Endpoint 的数量、每 Endpoint RX/TX packet 与 byte storage、一次 pump 工作和相关 drop accounting 都必须有界；
  影响资源与行为的重要容量进入 Kconfig，不以内嵌 magic number 固化。

### Ingress 安全与共存边界

- 只有已经通过对应 interface 的本地 IPv4 destination admission 的 packet，才能交付给用户 raw Socket。raw filter
  不得先复制随后会被 IP 层以“不是本机、本地广播或已加入 multicast group”为由拒绝的 packet。
- R0 raw Socket只生产和接收unicast destination。interface/IP owner即使为其它protocol consumer承认本地broadcast或
  joined multicast，raw owner也必须在fanout前按本target排除；不得让`SO_BROADCAST`或multicast option缺失与数据面实际
  可达形成分裂。该排除不改变普通ICMP或其它protocol的既有处理。
- 每个匹配的live raw Endpoint取得独立计费且有界的delivery ownership；具体copy/shared immutable backing策略不在
  positioning中决定。一个Endpoint满载只丢弃它自己的delivery，不得让其它raw Endpoint、普通ICMP processing、
  UDP/TCP demux或frame owner丢失原packet。
- raw Socket 是协议 observer/producer，不拥有 interface address、route、source selection、ARP/neighbor、provider
  queue 或 frame lifecycle。接收 raw 副本不能窃取原 packet，普通 ICMP处理仍按 protocol Stack规则继续。
- RX full、malformed IP、unsupported destination/source、TX admission full 与 retired Endpoint 必须形成可诊断且
  ABI诚实的不同结果；不得把普通资源不足变成 panic，也不得用无界 queue 或 silent success 掩盖失败。

## Current Baseline

### Effective dependency

当前 [Socket current contract](../../../contracts/socket/front-abi-wait.md) 已固定immutable static descriptor、
Linux ABI containment、operation-owned predicate、shared wait/recheck与opened-description final release。首版可以增加
ICMP-only semantic type；任意protocol number会引入新的per-instance truth，必须重新review，不能藏进backend private
state。

当前 [IPv4 control-plane contract](../../../contracts/net/control-plane.md) 让initial-domain control plane唯一拥有
route、source/interface selection。当前 [UDP Socket contract](../../../contracts/net/udp-socket.md) 证明了kernel
Socket与Stack Endpoint之间可以保持窄、非阻塞、opaque的owner handoff，但UDP的aggregate/per-interface engine拓扑不
自动成为raw Socket target。只有两份真实实现具有相同义务时，才提取共同capability。

### Raw-path evidence

vendored smoltcp已经启用`socket-raw`并提供protocol filter、bounded packet buffer、fanout与dispatch，但production尚未
创建raw Endpoint。它也不能原样作为Linux backend：TX输入是完整IP packet，short receive返回`Truncated`，而且当前
`raw_socket_filter()`早于本地destination admission，receive又从会丢失TOS/ID/flags/IP option的high-level
`Ipv4Repr`重建header。正式RFC必须解决这些语义差异；对未分片IPv4，首选在local admission后、原始packet仍可用时
建立独立计费delivery，而不是在outer Stack复制destination predicate或向用户暴露近似header。具体seam、Endpoint
topology及production是否复用smoltcp raw socket仍由实现调查决定。

提升前本地验收快照中的BusyBox `ping`发送ICMP message并从receive buffer解析IPv4 header；当时观察到它会尝试但不检查`SO_BROADCAST`与
`SO_RCVBUF`，因此这些option诚实返回`ENOPROTOOPT`不妨碍普通unicast ping，也不能由ping成功反推option支持。`-t`
依赖本target纳入的`IP_TTL`；`-I <local-address>`会在`bind`前强制设置并检查`IP_MULTICAST_IF`，`-I <interface>`另依赖
interface-name lookup与`SO_BINDTODEVICE`，两者都不在R0。RV64与LA64 QEMU router `10.0.2.2`是稳定验收floor；数值
公网IPv4与hostname分别另受host网络、远端ICMP和guest DNS影响，只能形成独立的environment-qualified证据，不是
R0 cutover前提。

### Current user-acceptance baseline

当前`socket-test`只有UDP与Unix suite，没有ICMP raw suite。`user-test`的curated LTP registry只有窄的`socketpair`
group，其中stock LTP仅有`socketpair02`，另一个`socket_r1_oracle`是Anemone自有oracle；它们不能代表general Socket
ABI或raw Socket验收。

提升前用于定位的LTP快照也没有可原样承担R0 target oracle的raw case：`sendmsg03`依赖本R0明确defer的`IP_HDRINCL`、`sendmsg`
与network namespace；`ping01`/`ping02`依赖`tst_net`的lhost/rhost拓扑；`socket01`/`socket02`又把Unix datagram、
TCP与其它未接受family混入同一不可拆case matrix。因此正式RFC需要新增focused suite与curated `socket` group，但
不得修改上游case、按结果跳过subcase或扩大R0来制造“完整LTP通过”。

## 四层角色与 Object Fence 方向

raw Socket继续扩展现有四层network model，不建立第五个raw专用架构层，也不把四层理解为依次完成的implementation
stage。`anemone-net-api`是kernel与concrete Stack共同依赖的semantic vocabulary，不是runtime中介；netdev、provider与
driver继续位于正交的frame plane，不因raw consumer存在而取得protocol Endpoint或Socket语义。

| Layer | 本slice中的角色 | 不得取得或拥有 |
| --- | --- | --- |
| `anemone-kernel::net`与kernel Socket owner | Linux ABI/credential boundary、static descriptor与family orchestration、TTL/TOS policy snapshot、blocking/readiness解释、control-plane selection、opened-description lifecycle integration | smoltcp handle/`SocketSet`、private packet queue、raw binding/peer/filter truth、protocol engine state或driver frame identity |
| `anemone-net-api` | kernel与concrete Stack确实共同需要的protocol-domain value、opaque identity、typed request/outcome、point-in-time facts与invalidation vocabulary | registry、lock、runtime owner state、fd/task/waiter、Linux errno/user pointer或smoltcp object identity |
| `anemone-smoltcp-stack` | concrete protocol resource root；拥有raw Endpoint identity namespace、lifecycle、bounded storage、fanout/drop、private engine mapping、operation commit与invalidation | fd/task/credential/user pointer、Linux wait/readiness policy或ABI representation |
| `anemone-smoltcp` | IPv4 parse/validation/emit、interface-local destination admission、普通ICMP processing与协议推进；若被选用，也只提供Stack-private raw engine mechanism | Anemone Endpoint/kernel Socket identity、control-plane policy、opened-description或wait lifecycle |

四层依赖与object fence不要求形式上的backend可替换，也不要求先建立generic trait hierarchy。它们只要求ordinary Socket
operation不能取得concrete Stack或smoltcp object，concrete Stack不能反向取得kernel object，API value不能沉淀成第二个
runtime owner。trait、`pub(crate)`或opaque type单独都不自动构成fence；若调用者仍持有public concrete `Stack`，它仍能
调用全部public inherent operation。

### `Raw`限定在Endpoint语义，不下推Linux Socket政策

本文区分两类`raw`：Linux `AF_INET + SOCK_RAW + IPPROTO_ICMP` Socket的ABI/权限形状，以及protocol
Stack向一个外部consumer提供的raw ICMP packet Endpoint语义。前者只属于kernel Socket owner；
`anemone-net-api`和concrete Stack不得取得`SOCK_RAW`数值、`CAP_NET_RAW`、fd、sockaddr、user pointer、
Linux errno、blocking/wait政策或`IP_HDRINCL` option状态。

后者是本target中真实的protocol-domain能力：Endpoint按IPv4 ICMP静态限定，以非独占方式观察已通过
local-destination admission的packet，为每个consumer建立独立计费的有界fanout/delivery ownership；持有local/peer
filter与ICMP filter的唯一protocol truth；TX接收ICMP message、operation-local selection与normalized header policy，
RX交付已脱离Stack lifetime的原始、未分片完整IPv4 packet。这些语义由
Stack raw owner实现，因此使用`Raw`限定net-api的Endpoint value/capability family或Stack-private类型不构成
Linux ABI泄漏。

这个能力不命名为含义过宽的通用`IcmpEndpoint`：ordinary ICMP protocol processing、可能的production
Echo responder，以及未来按identifier过滤或由kernel形成其它ICMP政策的Endpoint都不自动具有上述
raw fanout、数据形状和lifecycle。命名应保留ICMP scope内的raw marker，例如概念上的
`IcmpRawEndpoint*`或`icmp::RawEndpoint*`；这些只固定语义分类和object fence，不预先决定具体trait、
newtype、module path或method分解。

ICMP scope内的raw marker不引入任意IP protocol token、dynamic protocol registry或通用raw backend。具体Stack
可以复用smoltcp raw socket、ICMP socket或owner-local post-admission seam，但任何候选都只是private mechanism，
不能改变上述Anemone Endpoint语义，也不能让普通ICMP processing改由raw Endpoint所有。

### Identity、authority 与 lifecycle 分离

raw Endpoint identity、operation authority与semantic lifetime必须保持三类不同语义：

- opaque Endpoint identity只服务owner lookup和stale isolation；它不是handle，不证明liveness，不延长资源生命周期，
  也不能让kernel反向解引用smoltcp object；
- kernel raw family只应取得role-scoped、non-blocking operation capability；它可以请求facts、send/receive、retire与
  invalidation registration，但不能拆出concrete Stack、private handle或queue；
- creation rollback与opened-description final release分别拥有publication前后唯一的semantic lifecycle transition；
  capability clone、Endpoint token、`Drop`、fd number与临时引用都不能成为retire truth。

general Socket front与raw family之间继续使用static descriptor加opaque family-private storage的front fence；kernel与
Stack之间只交换normalized request、operation-local immutable selection、typed outcome、point-in-time facts和recheck
hint；Stack与smoltcp之间的handle、`SocketSet`、buffer与mapping保持private。notification不携带readiness/error truth，
Stack transition必须先提交owner fact并退出其mutation window，再把invalidation交给kernel route。

packet fence不是“每层必须复制一次”。ingress原packet仍由protocol/frame path按原owner推进；local-destination owner完成
admission后，raw owner才能在原始IPv4 datagram仍可用时为每个matching Endpoint建立独立计费的delivery ownership。
copy与shared immutable backing都保持开放，但delivery必须保留supported packet class在IPv4 `total_len`内的原始字节；
交给Socket receive operation时必须已经从Endpoint detach，不得借用Stack lock、`SocketSet`、smoltcp buffer或driver
frame lifetime。raw delivery是non-exclusive observation，不能以“handled”为由抑制普通ICMP processing。

### 参考候选（非 target / 非 implementation decision）

以下路线与当前owner model相容，可作为正式RFC和实现调查的起点，但不因写入本文而获得优先级或授权：

- 参考当前UDP形状，由kernel network owner的private `DomainStack`持有concrete Stack，向raw family交付封装Stack
  access与opaque Endpoint identity的窄port/capability；精确类型、clone范围、方法族与module placement保持开放；
- 在`anemone-net-api`增加保留ICMP scope内raw marker的Endpoint value/capability family，而不是引入含义过宽的
  通用`IcmpEndpoint`、任意IP protocol token、通用`EndpointOps`或backend registry；具体使用struct、enum、trait还是
  函数族由真实调用面决定；
- raw Endpoint可以采用Stack-owned aggregate resource加per-interface private projection，也可以采用其它保持domain-wide
  identity、独立capacity与single-owner fanout的topology；UDP拓扑和vendored smoltcp raw socket都只是证据，不是precedent；
- ingress可以通过调整vendored smoltcp内部filter/admission顺序，或建立不复制destination truth的owner-local
  post-admission seam；不得在outer Stack、control plane或Socket层重新实现一份近似local-destination predicate；
- RX delivery可以复制，也可以使用Stack-owned immutable shared backing；无论选择哪条路线，原始IPv4字节保真、
  per-Endpoint charge、detach时刻、backing release与retire隔离都必须可证明，且不能把driver/smoltcp private identity
  暴露给kernel Socket。

## 必须保护的 Owner 与 Handoff 边界

下面只固定语义owner与跨owner义务，不冻结Endpoint对象图、opaque handle形状、engine数量、锁、helper、observer或
notification表示：

| Fact / capability | Unique owner | Cross-owner boundary |
| --- | --- | --- |
| Linux tuple、flags、sockaddr、copy、errno、sockopt与`CAP_NET_RAW` admission | Socket ABI adapter | normalized request/outcome/value |
| immutable ICMP raw semantic type与common file association | static Socket descriptor/front | family-private state |
| Linux readiness projection、wait、TTL/TOS policy与final-release integration | kernel raw Socket family | current facts、operation-local header policy与recheck capability |
| destination route、source address与interface selection | initial-domain IPv4 control plane | operation-local immutable selection |
| interface-local IPv4 destination admission | concrete protocol interface/IP owner | post-admission packet observation |
| raw lifecycle、local/peer/ICMP filter、bounded packet storage、fanout、capacity与retire | domain Stack raw owner | narrow non-blocking operation capability |
| non-`IP_HDRINCL` TX header formation与admitted原始RX packet交付 | domain Stack raw owner | ICMP message + selection + header policy / detached IPv4 packet |
| smoltcp handle、`SocketSet`与private packet representation（若使用） | concrete Stack owner | 不跨crate暴露 |
| NIC queue、DMA、IRQ与frame backing | netdev/provider/driver owner | callback-scoped frame capability |
| semantic close trigger | opened-description lifecycle owner | raw final-release hook |

- 创建在fd publication前失败时不得留下可操作Socket、live raw registration或不可回收的Stack资源；具体prepare/guard
  顺序由实现选择，只要rollback owner与可见errno/copy ordering明确。
- bind/connect/disconnect只通过narrow capability提交或查询Stack raw owner中的local/peer filter；connect前由control
  plane验证route/source，失败不得部分改变既有association。重复transition、disconnect后的source snapshot与sockaddr
  port projection在公共RFC前按Linux oracle冻结，不能由kernel和Stack各存一份近似状态。
- send只跨owner传递normalized ICMP message、operation-local immutable selection与TTL/TOS header policy；Stack raw owner
  形成IPv4 header，并在返回success前完成size、capacity与packet ownership commit。精确prepare/copy顺序保持开放，但
  commit后的普通provider loss不反向改写send结果。
- ingress只有在protocol interface owner完成本地destination admission后才能向matching raw Socket fanout；其它层不得
  复制该predicate。每个consumer的capacity与lifecycle互相隔离，receive以detached完整IPv4 packet的单一owner handoff
  实现prefix-copy、peek与consume；ICMP filter在计费和queue admission前执行，不改变普通ICMP processing。
- readable/writable从raw owner的当前fact派生；route、source与request-specific length仍由本次send裁决。shared
  Socket层只复用现有blocking choice与wait/recheck，不缓存ready truth。
- semantic final release必须阻止新operation并移交raw lifecycle cleanup；`Drop`、fd number、临时引用或关闭单个
  dup/fork alias不得成为retire truth。late notification或重复cleanup不得恢复已退休对象。

## Framework Feedback 边界

raw consumer可以暴露framework缺口，但只允许处理本轮真实义务：

- 为ICMP-only descriptor增加准确的`SO_DOMAIN/SO_TYPE/SO_PROTOCOL`投影与tuple resolver；
- 现有datagram ABI helper若已经表达UDP/raw共同语义则直接复用；否则先保持family-local，只有重复义务被两份真实实现
  证明后才提取最窄helper或capability；
- 为raw `read/write`把common FileOps当前隐含的stream-only request投影收敛成family-neutral file-I/O request；UDP继续
  保持既有unconnected能力，Unix stream行为不变，不为raw增加第二份FileOps、downcast或私有wait loop；
- 只为R0真实选定的`IP_TTL`、`IP_TOS`与`ICMP_FILTER`增加最窄option dispatch和值/结果类型；其它option继续诚实返回
  `ENOPROTOOPT`，不建立通用mutable option bag、dynamic registry或无producer的state；
- production ingress必须满足local-admission-before-raw-delivery并保持普通ICMP/UDP语义，但具体Stack/smoltcp修改点由
  实现调查决定；supported RX packet必须保留原始IPv4字节，不能用会丢字段的近似重建降低oracle。

下列变化不因“为TCP准备”而自动授权：

- 任意protocol的dynamic raw descriptor或protocol registry；
- generic `ProtocolStack`、`EndpointOps`、Socket manager、shared queue或connection state machine；
- 为TCP预加listen/accept/shutdown/error slot或errno；raw connect只服务本轮真实的default-peer与RX-filter义务；
- 把route、interface、smoltcp/private handle、readiness或packet queue truth复制进common Socket；
- 为raw单独实现`sendmsg/recvmsg`薄层，或在没有common Socket message ABI target时预建cmsg/ancillary框架；
- 通过test-only injection、Socket-to-Socket fast path或降低Linux oracle来绕过真实frame/Stack路径。

若真实实现要求改变`SOCKET-FRONT-001`的唯一type witness、移动protocol/state owner、扩大shared public API、改变
ABI/non-goals/acceptance或降低验证claim，必须在公共RFC review或Target Renegotiation中停止并明确决定，不能作为
“按需改进”静默进入实现。

## 验收能力与证据分层

### Owner-local / host proof

正式RFC至少应要求：

- tuple、flag、`CAP_NET_RAW`成功/拒绝与fd publication rollback matrix；
- bind/connect/reconnect/disconnect、local/peer query、send destination override、RX local/peer filter与失败原子性；
- raw lifecycle create/retire、multiple matching consumer fanout、ICMP filter、final release后无交付与late notification
  fail closed；
- local-destination admission先于raw delivery，且raw receive不抑制普通ICMP processing；
- RX/TX capacity exhaustion、drop isolation、recovery、有界进展与shutdown；
- send copy/header/admission commit、TTL/TOS snapshot、zero-length packet、MTU rejection；receive原始IPv4字节保真、
  zero/short copy、`MSG_TRUNC`、peek/non-peek copy fault与consume；
- snapshot/register/recheck/final-scan、blocking/nonblocking、poll/select/epoll、multi-waiter取消与final release；
- ordinary UDP、Unix Socket、iomux/epoll与network shutdown regression不退化。

### 双架构 focused guest ABI

`socket-test`必须新增明确限定为ICMP raw的suite；它是R0 target-complete guest oracle，不命名或演化为任意IP protocol
的通用raw suite。RV64与LA64 guest都应通过该suite或等价focused oracle验证：

- `socket(AF_INET, SOCK_RAW | SOCK_{NONBLOCK,CLOEXEC}, IPPROTO_ICMP)`与缺失`CAP_NET_RAW -> EPERM`；
- `bind/getsockname`、`connect/getpeername`、`connect(AF_UNSPEC)`、destination override与精确sockaddr/errno矩阵；
- `SO_DOMAIN/SO_TYPE/SO_PROTOCOL/SO_ACCEPTCONN`、`IP_TTL/IP_TOS/ICMP_FILTER`以及未支持option的诚实errno；
- `sendto/recvfrom/send/recv/read/write`的zero/short/invalid pointer、copy-fault、message size和flag matrix；
- blocking/nonblocking与poll/select/epoll readiness；
- dup/fork/CLOEXEC/non-final alias close/final close以及创建失败cleanup。

### Curated Socket LTP regression

正式RFC与cutover必须建立一个永久注册、语义名为`socket`的curated LTP group，作为common Socket ABI与既有
UDP/Unix family的mandatory regression floor。它不是`icmp-raw` group，也不拥有raw-specific target proof；现有
`socketpair` group如何并入或退役属于实现路线，但同一case不得因两个group重复执行而制造额外coverage。group及case
清单进入tracked test infrastructure，具体`profile.txt`选择只是单次validation input，不自动改变长期默认策略。

case准入以整个stock LTP binary的完整matrix为单位：只有所有subcase都位于effective或本RFC accepted target内，且
fixture不要求本轮defer的family、syscall、option、namespace或remote-host能力时才能纳入。不能通过修改上游case、
降低errno oracle、把unsupported分支改成success或只运行有利subcase来取得PASS。按当前源码初筛，首批stock候选是
`socketpair02`、`bind03`与`listen01`；它们仍须在实施时经过双架构实际运行和结果分类，positioning不预写PASS。
Anemone自有`socket_r1_oracle`可以继续提供focused ABI证据，但不得计作stock LTP coverage。

cutover floor是在RV64与LA64 guest中由现有runner分别执行glibc与musl root：纳入group的case不得出现`TFAIL`、
`TBROK`、runner timeout或infra failure。`TCONF`必须逐项说明环境或target原因，且不计作对应能力已覆盖；只记录
profile summary而没有case provenance不足以证明该floor。该group通过只证明其明确case matrix与相邻Socket regression，
不能替代owner-local proof、ICMP raw focused suite或真实ping。

### 真实 ping 与网络拓扑

以下项目证明不同claim，不构成从本地到公网单调增强的同一oracle。只有QEMU router floor属于R0 mandatory
acceptance；其它项目即使成功也不能替代它，未运行也不阻塞R0 cutover：

1. **QEMU router floor / Mandatory：** RV64与LA64当前user-mode网络分别成功执行当前final BusyBox
   `ping 10.0.2.2`，证明真实raw Socket、IPv4、VirtIO、external Stack path与QEMU router round trip。数值目标不依赖
   guest DNS或`/etc/resolv.conf`；该同网段目标不单独证明default-route Internet reachability。BusyBox忽略
   `SO_BROADCAST/SO_RCVBUF`失败，因此本gate不把它们误判为已支持；另以focused oracle证明`IP_TTL`实际改变header。
2. **公网数值IPv4 / Optional smoke：** 公网ping不进入mandatory `user-test`与R0 cutover gate。只有通过独立、显式选择的
   smoke invocation记录host effective group与`ping_group_range`、宿主机外网和目标确实响应ICMP后，成功ping数值公网
   IPv4，才可声明“guest经默认路由能够ping Internet”。数值目标不依赖DNS；第三方目标失败必须先按
   host permission、环境、远端与实现分层诊断，不能直接形成kernel finding。
3. **域名 / 独立DNS integration：** `ping <hostname>`另外证明guest resolver、`/etc/resolv.conf`与UDP DNS，不属于
   ICMP raw R0 closure。若未来建立该测试，DNS配置及其它fixture由DNS测试自己声明和解释；通用fixture staging只负责
   搬运，不取得DNS或ping语义。
4. **入站peer ping Anemone：** 需要production ICMP echo responder与支持入站ICMP的受控网络。raw Socket本身不会
   自动生成Echo Reply，QEMU user-mode NAT也不是透明入站ICMP环境；production responder不进入本RFC，未来若需要
   作为独立Stack ICMP target与入站验收处理。

`ping -I <local-address>`依赖本R0不支持且BusyBox会强制检查的`IP_MULTICAST_IF`，`ping -I <interface>`另依赖
interface-name lookup与`SO_BINDTODEVICE`；两者不得由bind focused PASS或普通ping成功冒充。broadcast/multicast ping同样
不属于R0。

QEMU router ping的test-owned入口拥有目标地址、BusyBox argv、timeout/retry、成功oracle、稳定marker与所需fixture；
外层`user-test`或其它通用harness只负责环境进入、声明式fixture staging、进程/超时管理、退出状态和日志收集。外层不得
硬编码`10.0.2.2`、解析BusyBox自然语言输出、理解Echo Reply或以ping-specific分支决定fixture内容。实现可以复用现有
staging能力，但不能让通用runner反向成为ping或DNS语义owner。

公网、DNS、physical hardware、`smp > 1`、非当前VirtIO设备、full network LTP与final harness若未运行，在closure
evidence中必须逐项记为Not Run；这些可选或更宽claim的Not Run不阻塞R0，也不能由build、host smoltcp test、QEMU
router ping或另一架构替代为PASS。

## 当前明确的硬 Gate

正式RFC接受前必须把以下事项闭合为target、owner或实施stop condition；实现cutover前必须取得对应证据：

1. **Raw ingress local-admission：** local-destination truth继续由protocol interface/IP owner唯一拥有；production raw
   delivery不得沿用当前“先raw filter、后本地destination判断”的顺序，也不得由outer layer复制predicate。必须形成只有
   本地admitted packet可见、同时普通ICMP仍继续处理的实现与测试证据；supported packet必须从post-admission seam保留
   原始IPv4字节，不能以`Ipv4Repr`重建丢失TOS/ID/flags/IP option。具体seam仍是实现调查。
2. **Linux TX/RX不对称：** Socket ABI、control plane与Stack raw owner的分工已经固定，但正式RFC仍须闭合user ICMP
   message、operation-local selection、TTL/TOS snapshot、TX header formation与detached original IPv4 packet之间的精确
   数据、copy/error和commit boundary；不能直接把smoltcp full-IP TX buffer当成Linux UAPI。
3. **Static type witness：** 首版ICMP-only descriptor不得暗中演化成backend-private arbitrary protocol truth；若需要
   任意protocol，先回到Socket contract review。
4. **State、resource与wake：** local/peer/ICMP filter、raw lifecycle、packet storage、fanout/drop与TX recovery必须有
   单一owner和有界进展；不能在kernel/Stack复制association、照搬UDP state、建立ready cache或依赖worker偶然轮询
   恢复。具体Endpoint/engine topology不是Gate。
5. **ABI honesty：** BusyBox忽略某些`setsockopt`错误不等于option已经支持；所有unsupported tuple/flag/option必须返回
   明确errno，不得success-no-op。唯一选定的`MSG_NOSIGNAL`兼容必须由“raw没有SIGPIPE producer”的可证明行为、关键
   注释与低噪声诊断支撑，不能扩张成generic flag forgiveness。
6. **用户验收三层闭合：** cutover必须同时取得双架构ICMP raw focused guest matrix、双架构/双libc curated
   `socket` LTP regression与双架构QEMU router BusyBox ping。这里的ping gate只指`10.0.2.2` floor，不要求公网或
   hostname。任一mandatory层缺失都保持对应claim Not Run / Not Cut Over；不得用custom oracle冒充stock LTP、用LTP
   syscall PASS冒充真实网络round trip，或用ping成功覆盖权限、copy、readiness与lifecycle matrix。
7. **自然边界与renegotiation：** 当前包络不是不可修改的机械清单。公共review或实现证据可以提出扩展、Route
   Correction、Accepted Reduced Target或follow-up RFC，但必须列出成本/失败证据、代码处置、受影响ABI/owner/acceptance
   与保持的correctness invariant；新边界接受前不能把较弱实现写成R0完成，也不能为保持原文字面制造不自然adapter。

## 提升时保留的实现精化清单

以下事项在提升时已归类为实现、ABI conformance或validation精化，不阻塞公共Draft。若调查结果要求改变公共RFC的
target、owner、failure/cleanup、Contract Impact或acceptance，仍须按公共RFC停止条件进入review：

1. repeated bind/connect/reconnect、`connect(AF_UNSPEC)`、bind-after-connect、disconnect后explicit/auto source保留及
   `sockaddr_in.sin_port`输入/输出的精确Linux 6.6.32 errno与projection矩阵；这些细节不得制造第二份association truth。
2. `sendto/send/write`与`recvfrom/recv/read`之间共享copy cursor、zero-length、`MSG_PEEK/MSG_TRUNC`、copy fault和
   Endpoint commit/consume的精确顺序；`sendmsg/recvmsg`明确不因`MSG_TRUNC`存在而进入R0。
3. post-admission seam能否在不复制destination predicate、不延长frame owner lifetime的条件下保留原始未分片IPv4
   datagram。若工程证据证明该fidelity只能通过owner穿透或扭曲object graph取得，必须带丢失字段、consumer影响和验证
   差异进入Target Renegotiation；不得静默退回近似header。
4. `IP_TTL/IP_TOS/ICMP_FILTER`的exact optlen、值域、concurrent snapshot与getsockopt copyout矩阵，以及
   `MSG_NOSIGNAL`低噪声diagnostic形状；这些是ABI精化，不授权通用option bag。
5. 公共RFC的最小Contract Impact：需按live source审计第三consumer及raw `read/write`是否真的Refine Socket
   front/ABI/wait，以及raw-specific owner/transaction规则是否需要新的shared contract；不能从本positioning预判
   Introduce/Refine。
6. 实施是否需要多stage/probe/独立`implementation.md`。当前positioning不预建checkpoint、transaction或逐文件
   write set；只有真实不安全中间态、probe或多个cutover才增加supporting artifact。

## 提升结果

开发者已确认当前能力边界并授权更新文档。2026-08-03创建公共
[`RFC-20260803-icmp-raw-socket`](../index.md)；该Draft及其
[目标与不变量](../invariants.md)成为proposal/target的唯一canonical source。
