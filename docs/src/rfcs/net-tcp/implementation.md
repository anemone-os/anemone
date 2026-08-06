# IPv4 TCP Socket 实施计划

**状态：** R0 / Stage 1 Closed / Stage 2 Closed / Stage 3 Closed / Stage 4 Closed / Syscall-Reachable Candidate / Not Cut Over / TCP Not Effective
**最后更新：** 2026-08-06
**父 RFC：** [RFC-20260805-net-tcp](./index.md)
**适用修订：** R0
**执行授权：** 用户于 2026-08-05 明确接受 R0、授权并关闭 P0，随后授权Stage 1解析并单独授权其implementation；
Stage 1已经关闭。用户同日解析Stage 2并明确Stage 2不发布syscall，随后分别授权并关闭CKPT 2A与CKPT 2B；
用户于2026-08-06先授权Stage 3 Implementation Resolution，随后分别授权并关闭CKPT 3A与CKPT 3B；同日接受
Stage 4提前接通真实userspace syscall candidate的Route Correction，并只授权Stage 4 Implementation Resolution；
随后分别授权并关闭CKPT 4A与CKPT 4B；Stage 5未授权
**当前 Gate：** Stage 4 Closed / Syscall-Reachable Candidate / Not Cut Over；
Stage 5 Outline / Not Resolved / Not Authorized

本文保存已经Positive / Closed的TCP engine feasibility Probe Gate和Stage 1 closure。Stage 1已经建立production
owner-driven progression handoff、原子迁移UDP/ICMP raw，并把P0结论收敛为Stack-private TCP owner foundation；
它没有交付TCP Socket capability。Stage 2已经解析为两个独立授权的execution checkpoint：CKPT 2A建立可由kernel
窄消费的TCP owner capability，CKPT 2B接入syscall-unreachable的general Socket front；整个Stage 2不注册
`AF_INET + SOCK_STREAM` creation tuple、不发布handler或部分UAPI，也不执行contract cutover。
Stage 3已经解析为两个分别授权、分别review的execution checkpoint：CKPT 3A完成Stack TCP owner fact与lifecycle，
CKPT 3B完成仍不可由syscall到达的Socket/ABI projection；整个Stage 3不注册TCP creation tuple、不发布handler、
fd或部分UAPI，也不执行contract cutover。CKPT 3A与CKPT 3B均已关闭，Stage 3 Closed / Not Cut Over。Stage 4现已
解析为两个分别授权、分别review的execution checkpoint：CKPT 4A在resolver继续拒绝TCP tuple时闭合owner predicate、
invalidation、Socket source与wait capability；CKPT 4B先闭合剩余Linux syscall projection，再通过唯一normal resolver
发布完整syscall-reachable candidate并开始repository-owned RV64 userspace纵向验证。Stage 5不再拥有首次route
activation，只拥有剩余双架构/external/capstone证据与最终`NET-TCP-CUTOVER`。这项Route Correction与Stage 4
Implementation Resolution不改变R0 target、owner、ABI、Contract Impact、acceptance或validation claim；Stage 4
现已由CKPT 4A/4B关闭并形成Syscall-Reachable Candidate / Not Cut Over；Stage 5仍为Outline / Not Resolved /
Not Authorized，未执行`NET-TCP-CUTOVER`。

P0只有在父RFC target与Contract Impact完成R0接受、live baseline重新核验且用户明确授权本gate后才能从
Not Active转为Active；三项前置已于2026-08-05满足。R0接受不自动授权执行，P0 closure也不授权任何后续gate。

## 1. P0 entry baseline

P0进入时的source提供了足以启动crate-only probe、但不足以宣称target可实现的事实边界：

- `anemone-smoltcp-stack`尚未启用vendored smoltcp TCP feature；vendored TCP public `State::Closed`
  不能独自区分handshake RST、established RST、transport timeout与local abort；
- 一个vendored listening socket没有backlog；并发acceptance需要多个engine socket或等价bounded
  composition，具体mapping尚未选择；
- kernel `DomainStack::protocol_transition()`已把Stack mutation与invalidation收集串行化，UDP与ICMP raw TX
  仍由caller/control-plane手工请求pump；这些production handoff事实只作为Stage 1 baseline，不进入P0代码或
  validation surface。

这些事实只说明engine probe入口和后续production seam存在。它们不批准vendored fork形状、listener pool、
shared effect type、wake carrier、worker routing或未来TCP module layout。

## 2. Probe Implementation Boundary

### 2.1 Target

P0只回答一个问题：能否在不泄漏smoltcp private object、不猜Linux errno且资源有界的前提下，在kernel外
crate中形成足够窄的protocol-cause seam，并用多个engine resource或等价composition承载logical listener、
completed pending child与代表性cleanup/reuse。P0只证明存在一条production路线，不预先实现kernel owner、
progression handoff、Socket integration或完整TCP resource lifecycle。

### 2.2 Non-goals

- 不发布`AF_INET + SOCK_STREAM`、Endpoint、fd、syscall、readiness、`SO_ERROR`或任何部分TCP UAPI；
- 不实现完整connect/listen/accept/stream I/O、FIN/RST/TIME_WAIT、Socket integration或CAgent workload；
- 不以probe决定最终Endpoint/engine mapping、listener算法、buffer、timer、lock、type、trait、module、
  production queue/capacity、wake carrier、per-interface或domain-wide routing；
- 不修改`anemone-kernel` source、kernel Cargo feature forwarding、Kconfig、KUnit、QEMU/rootfs或guest profile；
- 不迁移UDP/ICMP raw，不修改current contracts、register、`NET-PROTOCOL-PROGRESSION-CUTOVER`或
  `NET-TCP-CUTOVER`状态，也不把host/source/build证据写成guest TCP PASS；
- 不建立长期host-only TCP facade、dormant kernel TCP path、第二个worker/pump或通用protocol manager。

### 2.3 Owner 与 handoff

- vendored smoltcp TCP engine只拥有protocol state transition与engine-local cause；cause seam只能暴露
  protocol-domain fact，不能暴露Linux errno、poll mask、fd、Task、waiter或Socket policy；
- 本地close/abort由Stack-side TCP owner已经发出的command与connection phase权威区分；cause seam只需保留
  engine异步产生且否则会在`Closed`中合流的事实，不得为统一表示制造第二份本地outcome truth；
- probe中的listener/child composition必须有一个crate-local Stack-side TCP owner，唯一拥有logical listener、
  engine slot、pending-child admission、capacity与cleanup；test harness只能驱动owner API，不能成为第二queue owner。

### 2.4 Protected boundary

P0只允许在vendored smoltcp与`anemone-smoltcp-stack` crate内进行最小engine/Stack experiment及owner-local
host validation。它必须保持：

- current Network、Socket、opened-description、iomux/epoll与System Power contracts全部不变；
- kernel production dependency和feature graph不取得host-only facade、probe flag、diagnostic开关或test-only API；
- engine slot、ring borrow、pending child与private handle不越过Stack crate object fence；probe capacity来自
  owner admission，不来自diagnostic mirror、allocator偶然失败或test harness queue；
- host-only probe code在P0 exit删除；只有同时适用于no-default production crate、由Stage 1真实consumer直接需要
  且经review确认为最小substrate的cause seam或owner-local primitive才可保留。

若P0需要进入`anemone-kernel`、修改kernel feature/Kconfig、发布cross-crate public API、运行guest才能支持结论，
或需要改变target/non-goal、owner/handoff、failure/cleanup、ABI、Contract Impact、acceptance或validation claim，
立即停止并进入RFC review；不能把它当作probe内部route correction。

## 3. P0 — TCP engine feasibility

### 3.1 Hypothesis

在kernel外Stack object fence内可以同时证明：

1. engine在public state收敛为`Closed`前或等价owner-local边界上保留active-open RST、established RST与
   transport timeout等异步protocol cause；Stack TCP owner结合权威connection phase能够消费该cause，且无需从
   merged state、elapsed timer或Linux caller path事后猜测；
2. local close/abort继续由owner command与phase区分，不要求engine cause seam复制本地operation truth；
3. 一个logical listener可以由有界engine composition承载，最小确定性场景至少覆盖两个并发completed pending
   child、full、取走或取消一个child后的capacity恢复，以及一次slot/child generation复用的stale isolation；
4. engine slot、listener、half-open/completed child及probe所需buffer/timer均有唯一owner与显式上界，owner
   capacity exhaustion产生typed rejection/drop/backpressure，不panic、busy-spin或同步等待peer/worker。物理protocol
   cleanup可以由未来production worker异步推进，但调用probe operation不能等待其完成。

本hypothesis只要求证明存在一条符合父RFC的实现路线，不要求达到`listen(10)` acceptance capacity、完整
fd/publication rollback、TIME_WAIT/deferred reclaim或最终TCP resource matrix，也不把probe mapping或cause表示
固定为最终设计。

### 3.2 Validation

- 用确定性engine/host场景分别制造active-open RST、established RST、timeout与local abort/close，证明cause在
  owner consume前不丢失、不重复，本地command不被误报为async failure，并且seam中没有Linux errno/Socket policy；
- 用真实vendored TCP socket行为完成最小bounded listener composition：两个并发completed child、full、一个
  child取走/取消后的恢复与一次stale generation隔离；source audit确认该路线没有single-socket假backlog或
  固定为二的结构性假设，`listen(10)`与完整matrix留给Stage 1；
- source/owner audit确认smoltcp handle、ring borrow、engine slot与pending queue都没有越过Stack fence，capacity
  不是diagnostic mirror、test harness truth或allocator偶然失败；
- TCP feature/no-default与相关host targets能够构建和运行，并确认kernel Cargo graph没有转发probe-only feature。
  该证据只证明engine feasibility，不宣称kernel TCP、guest TCP、ABI、progression handoff或双架构runtime已经实现。

### 3.3 Failure signals

以下任一项使P0 hypothesis失败：

- async target outcome所需cause只能在进入Stack owner前丢失，或只能通过`Closed`、elapsed timer/Linux caller
  path事后猜测；
- cause seam必须携带Linux errno、Socket/wait state或公开smoltcp private object；
- bounded listener只能依赖单socket假backlog、无界socket/child分配、两个pending-child truth或无法隔离的复用；
- owner capacity full或代表性close/cancel需要panic、busy-spin、同步等待peer/worker，或可能释放仍可访问的slot/backing。

如果只能通过修改`anemone-kernel`、kernel feature forwarding、Kconfig/KUnit、guest runtime、永久host-only facade或
dormant production TCP path维持proof，P0不是positive；它进入Inconclusive / Review Hold并回到RFC route review，
不能把越界实现写成engine infeasible或自动触发reduced target。

## 4. Gate result、Write-back 与 Exit

### 4.1 Positive

P0 hypothesis与全部protected boundary成立时：

- 在本页记录结论、对应Git/PR evidence、实际host运行范围与Not Run边界；把长期engine/owner结论折回父RFC
  正文或不变量，但不把probe type、文件布局或命令固化成target；
- 删除host-only probe facade、临时adapter、diagnostic开关和无真实consumer的experimental API。只有同时适用于
  no-default production crate、由Stage 1真实consumer需要且经review确认为最小substrate的cause seam或owner-local
  primitive才可保留，不能仅因host test能跑而沉淀；
- current contracts、kernel source、`NET-PROTOCOL-PROGRESSION-CUTOVER`与`NET-TCP-CUTOVER`全部保持不变；
- P0标记Positive / Closed后立即停止；在P0授权范围内Stage 1保持Outline。是否解析并实施Stage 1需要独立review与
  用户授权。

### 4.2 Negative or inconclusive

P0 hypothesis失败、证据不足或必须越过crate-only protected boundary时：

- P0整体记录Negative / Closed或Inconclusive / Review Hold，保存最小失败证据与已完成slice；
- 删除probe-only code；review先区分probe route/实现缺陷、环境证据不足与真实target infeasibility。保持target的
  route correction可以重新解析P0；只有证据要求改变target、owner、contract、acceptance或validation claim时才
  进入Target Renegotiation，agent不能自行批准reduced target；
- P0 negative/inconclusive exit时Stage 1保持Outline / Not Resolved / Not Authorized，kernel source、current contracts、
  `NET-PROTOCOL-PROGRESSION-CUTOVER`与`NET-TCP-CUTOVER`全部保持不变。

### 4.3 Evidence placement

P0默认由本页保存长期结论、由Git/PR保存diff与validation evidence，不预建transaction。只有执行跨度真实需要
独立长期时间线时才另行review是否建立transaction；它不是P0 activation或closure前置。

### 4.4 P0 execution result — Positive / Closed

2026-08-05在`dev/drc/alpha@00c897c3`进入P0；worktree起始clean。用户明确接受当前target与Contract
Impact为R0并独立授权P0。重新读取AGENTS/LOCAL、R0正文/invariants/implementation、register、current
Network contracts与live vendored/Stack/kernel feature owner后，baseline与第1节一致；net-tcp transaction不存在，
且本次短probe无需建立独立长期时间线，因此transaction保持None。

**Cause experiment：** 临时vendored seam在TCP engine进入`Closed`前分别保存protocol-domain `Reset`与
`Timeout`，只提供一次consuming handoff，不携带Linux errno、Socket/wait/readiness或caller phase。active-open
RST、established RST、connect timeout与established timeout分别命中该seam；local `abort`不产生async cause。
Stack-side consumer可以把同一`Reset`与自己唯一拥有的connecting/established phase组合为不同operation outcome，
无需从merged `Closed`、elapsed timer或Linux caller path猜测。

**Listener experiment：** 临时`tcp_engine_probe`使用真实smoltcp `Interface + Loopback + SocketSet + tcp::Socket`，
由一个test-local logical listener owner持有可配置数量的engine slot与generation；pending child直接来自owner持有
socket的`Established` state，没有第二queue或test-harness admission truth。capacity=3时三个并发child完成；full后
第四个connect收到真实RST；take一个child并rearm后capacity恢复，旧generation不能cancel replacement child；
cancel另一child并rearm后再次完成新连接。slot/buffer数量显式有界，operation不等待peer/worker，也没有固定为二
的结构形状。

**Exit disposition：** 按4.1删除临时listener owner/fixture、test target、vendored cause field/type/method/test
assertion与Stack manifest中的临时`socket-tcp`启用；final vendored/Stack/kernel production source均无diff。
Stage 1只有在独立授权并建立真实Stack TCP owner时才启用正式feature。没有新增kernel feature forwarding、runtime
TCP path、host facade、diagnostic switch、cross-crate Anemone API、smoltcp handle export或current contract。

**Validation evidence：**

- 带临时fixture的`just test net-host` PASS：`tcp_engine_probe` 1/1，同时net-api/Stack unit、全部既有Stack
  integration、doctest、vendored focused IPv4与no-default build/check均通过；
- 临时cause seam上的精确vendored命令
  `cargo test -p smoltcp --lib --no-default-features --features std,medium-ip,proto-ipv4,socket-tcp socket::tcp::test::`
  PASS，177/177；一次错误filter运行0项，未计入证据；
- 删除全部probe-only code后的`just test net-host` PASS：net-api unit 4、Stack unit 2、Stack integration 45、
  compile-fail doctest 2与vendored focused IPv4 33全部通过；no-default Stack test build与check均通过；
- 临时feature存在时的`cargo tree -p anemone-kernel -e features -i smoltcp`确认它只经Stack进入production graph，
  没有`host-test`或probe-only feature；最终source删除该feature后再次确认production graph不含`socket-tcp`；
- 最终source上的`just fmt kernel --check`、`mdbook build docs`与`git diff --check`均PASS；mdBook仅报告
  search index较大的既有warning。

**Not Run：** kernel TCP、kernel build、rootfs、QEMU、RV64/LA64 guest、syscall/ABI、progression handoff、
UDP/ICMP raw迁移、CAgent、deployment probe、physical hardware、LTP与final harness均Not Run。P0 host/source
evidence不外推这些轨道。

**Boundary and stop：** hypothesis四项与protected boundary均成立；没有target、owner、handoff、failure/cleanup、
ABI、Contract Impact、acceptance或validation claim变化，没有register/current-contract write-back。P0为Positive /
Closed并在此耗尽授权；Stage 1在P0 closure时仍为Outline / Not Resolved / Not Authorized，不得自动进入。

## 5. RFC-wide engineering principles

### 5.1 Socket framework feedback

TCP不是只能适配current Socket framework的孤立family。本RFC的每个gate都必须判断实际摩擦是TCP-local fact，
还是current framework把owner-neutral capability放错位置：

- TCP-local listener、connection、stream、error、option与resource fact继续由Stack TCP owner拥有；
- creation/descriptor、ABI dispatch、wait/recheck、opened-description lifecycle或其它owner-neutral obligation若
  需要shared修订，当前gate停止并把具体证据、最小surface、existing-consumer迁移、Contract Impact与validation
  写回父RFC review；不得为了维持current contract表而增加TCP-local state、旁路或adapter；
- shared change必须消除具体重复truth、owner penetration或协议税；没有真实共同义务时，不建立generic framework、
  dynamic registry、option bag或ready bus。

这类framework反馈是本RFC的预期产出，不因需要review就被当作probe/stage失败；但在父RFC接受delta并重新授权前，
当前gate不能继续实现或cut over。

### 5.2 Allocation 与 OOM boundary

- syscall length/count/backlog/iovec等不可信输入先完成overflow、ABI与owner上界校验；过大输入返回target规定的
  typed error，不能进入可由用户稳定放大的allocation；
- owner-configured Endpoint/listener/child/buffer/timer/reclaim capacity full是正常可恢复状态，按typed
  rejection/backpressure/drop与recheck处理；不能用allocator偶然失败形成capacity policy；
- 通过校验且位于显式上界内的kernel allocation遭遇global heap OOM时，当前工程阶段允许kernel-fatal panic，
  不要求新增复杂rollback或向syscall传播伪造errno。该容许不削弱commit-before-publication、ordinary failure
  cleanup、single owner与stale isolation。

## 6. Stage 路线与 Future Outlines

### 6.1 成熟度规则

- **P0 Positive / Closed：** crate-only cause与bounded listener hypothesis、protected boundary、validation、
  probe代码处置与write-back均已满足；
- **Stage 1 Closed：** production handoff、UDP/ICMP raw迁移、Stack-private TCP foundation、mandatory evidence与
  `NET-PROTOCOL-PROGRESSION-CUTOVER`已经整体关闭；
- **Ready / Not Authorized：** Implementation Boundary、execution checkpoint、ordering、validation、cutover、exit与
  stop condition已经解析完整，但没有代码、运行或contract生效主张；
- **Outline：** 只固定目的、前置依赖、受保护边界与解析触发点，不冻结checkpoint、具体步骤、类型、算法、
  文件、命令或transaction；
- Stage 2及后续future Stage只有在前一gate独立Closed、live source/current contracts/register重新核验，并完成单独的
  Implementation Resolution与用户授权后才能进入Active；
- 前一gate关闭不自动解析、授权或启动后一Stage。任何future Outline若需改变target、owner、handoff、
  failure/cleanup、ABI、Contract Impact、acceptance或validation claim，先回到父RFC review。

shared Socket framework反馈按第5.1节同样先回到父RFC review；不得把需要review误当成必须留在TCP owner内实现。

Stage名称、数量与相邻职责可以在保持父RFC target、Stage 1 Network contract cutover和最终合取closure不变时
通过本文内的Route Correction调整；一旦某Stage完成独立解析，其交付与受保护边界不能在执行中静默重排。

### 6.2 路线图

| Gate / Stage | 当前成熟度 | 概括目的 | Contract 状态 | 解析触发点 |
| --- | --- | --- | --- | --- |
| P0 — TCP engine feasibility probe | Positive / Closed | 在kernel外crate验证async cause与bounded listener composition路线 | None；current contracts不变 | 已满足；本gate停止 |
| Stage 1 — Stack TCP owner与protocol progression foundation | Closed | 建立production owner-driven handoff、原子迁移UDP/ICMP raw，并把P0证据收敛为Stack TCP owner foundation | `NET-PROTOCOL-PROGRESSION-CUTOVER` Effective；TCP target contracts继续Pending | 已满足；本gate停止 |
| Stage 2 — TCP owner与Socket-front integration | Closed | 以CKPT 2A/2B先建立kernel窄capability，再接入syscall-unreachable Socket descriptor与nonblocking scalar integration | None；TCP target contracts继续Pending | 已满足；本Stage停止 |
| Stage 3 — Stream、ABI与lifecycle completion | Closed / Not Cut Over | 以CKPT 3A/3B先闭合Stack owner fact与lifecycle，再完成仍不可达的partial stream、option/error和message/vector Socket/ABI projection | None；TCP target contracts继续Pending | 已满足；本Stage停止 |
| Stage 4 — Userspace vertical slice、blocking/readiness与concurrency hardening | Closed / Syscall-Reachable Candidate / Not Cut Over | 以CKPT 4A闭合仍不可达的owner predicate/source/wait capability，再由CKPT 4B完成syscall projection、唯一normal resolver activation与RV64真实userspace纵向验证 | None；candidate syscall-reachable但TCP target contracts继续Pending | 已满足；本Stage停止，不进入Stage 5 |
| Stage 5 — Dual-architecture与architecture-capstone closure | Outline / Not Resolved / Not Authorized | 在Stage 4已可达candidate上完成mandatory双架构、remote-external、shared regression与架构封顶，并原子执行最终cutover | `NET-TCP-CUTOVER` Pending | Stage 4 Closed、candidate保持可达、acceptance assets与独立final review可用 |

### 6.3 Stage 1 Resolved Gate — Stack TCP owner与protocol progression foundation

#### 6.3.1 成熟度、授权与前置基线

Stage 1是一个formal execution gate，只在exit执行一次`NET-PROTOCOL-PROGRESSION-CUTOVER`。实现可以形成若干普通
Git commit，但这些commit不是额外checkpoint、semantic gate或partial cutover；本Stage不需要transaction，默认
evidence placement保持Git/PR加本页closure write-back。Stage 1后来取得独立implementation授权并已按本节边界关闭；
以下baseline保留为进入implementation时实际核验的入口事实。

进入implementation前必须重新确认以下live baseline没有语义漂移：

- P0为Positive / Closed，probe-only cause/listener fixture与临时`socket-tcp` feature均已删除，production source无
  TCP diff；
- `DomainStack::protocol_transition()`在Stack guard内提交mutation并取出owner invalidation，在guard外路由
  recheck-only hint；这条commit-before-notification顺序继续复用；
- `Ipv4Selection`仍携带`PumpWake`，UDP `send()`与ICMP raw `send_prepared()`仍在Stack mutation成功后由caller
  `request_pump()`；Stage 1必须同时移除这三处control-plane/caller wake职责；
- external `ActivePath`已经唯一保存boot-lifetime `InterfaceId -> PumpControl` association，local association由
  `InitialDomain.local_path`拥有；Stage 1复用这两个attach-owned association，不建立第二registry；
- `PumpControl::request_work()`已经以active check、explicit-work release store、active recheck和worker wake表达
  stateless/coalesced request，terminal stop先关闭admission再清除request；Stage 1先证明这套既有协议足够，不能在
  没有具体race反例时预建新的permit、锁或lifecycle protocol；
- current `NET-CONTROL-PLANE-001`与`NET-STACK-PUMP-001`仍为旧Active规则，register中的typed user-copy
  Apollyon仍Open。Stage 1不发布UAPI、不新增typed copy，因此该issue不阻塞本gate；任何syscall/user-copy接线都
  越过本Stage并触发停止。

若上述source/current contract/register事实变化已经影响target、owner、handoff、failure/cleanup或validation，先
回到Implementation Resolution或RFC review，不能按过期manifest执行。

#### 6.3.2 Implementation Boundary

**Target：** 在initial-domain既有Stack与worker topology内形成最终production progression handoff：UDP、ICMP raw
与TCP各自的Stack-side protocol owner判断自己的typed committed mutation是否产生progression obligation；共同
composition只接收affected progression interface/domain并向对应既有worker提交stateless request。Stage 1同时原子
迁移UDP/ICMP raw真实consumer，并建立Stack-private TCP Endpoint/listener/connection/resource owner foundation。

**Non-goals：** 本Stage不发布`AF_INET + SOCK_STREAM`、kernel TCP family/source、Socket operation、fd、syscall、
Linux errno、user copy、blocking、poll/select/epoll、readiness、`SO_ERROR`或其它TCP UAPI；不运行TCP guest、CAgent、
deployment probe或最终`NET-TCP-CUTOVER`；不建立新worker、第二route/deadline/association registry、shared effect
policy、通用ready/error bus、TCP专用wait loop、kernel probe或长期validation facade，也不扩张`anemone-net-api`
来承载TCP-private object或operation。kernel只可通过窄construction-time policy把owner-local capacity注入Stack；
该seam不得提供Endpoint/engine handle或runtime TCP operation。

**Protected boundary：** control plane只决定route/source/interface，不再携带或选择protocol wake policy；
`anemone-smoltcp-stack::Stack`继续唯一拥有smoltcp object、protocol state与deadline，attach/worker owner继续唯一
拥有association、admission、coalescing与stop。TCP engine handle、ring borrow、buffer、cause、generation和
listener/child representation不得越过Stack fence。UDP/ICMP raw selection、transaction、failure、readiness、retire、
local/external与双架构visible semantics保持不变。exact Rust carrier、method、module、lock与effect表示仍是
implementation preference；边界只要求它们不能形成新的truth或public contract。

#### 6.3.3 Owner 与 handoff model

每个protocol mutation由对应Stack-side owner在Stack guard内同时决定operation result与是否形成obligation；共同层
不得通过operation名称、payload、queue delta或deadline猜测effect，也不得建立跨protocol shared effect policy。
没有committed protocol effect或operation失败时不请求progression；一旦effect提交，后续pump必须先能观察该effect，
然后共同层才处理其affected interface/domain。operation只有在共同层完成这次non-blocking request提交后才能向上返回
成功；它不等待worker实际运行或等待protocol progress完成。

共同composition在attach authority guard内只解析既有local/external association并取得窄request capability，随后在
guard外提交request；它不持attach guard进入worker、Stack或wake路径，也不把`PumpControl`、worker identity、route、
deadline或stop truth存回Endpoint/Socket。active path找不到一个已选择且已提交的interface属于owner invariant
violation，不允许silent drop、fallback到其它path或动态创建association；terminal stop已关闭admission时，既有
stop语义获胜且late request不能重开worker。

UDP与ICMP raw切换必须是同一source-level migration：`Ipv4Selection`和control-plane input/storage删除`PumpWake`，
两个caller `request_pump()`路径同时删除，成功mutation改由owner-driven handoff提交request。不得先长期保留旧/新
双路径、compat fallback或caller adapter；中间commit若不能保持完整production语义，只能与切换commit合并或留在
未发布分支，不能形成独立checkpoint。

#### 6.3.4 TCP private foundation

Stage 1永久启用vendored smoltcp `socket-tcp`，并把P0证明的最小cause seam收敛为production substrate：engine只
保存并一次性交付异步protocol-domain `Reset`与`Timeout`；local close/abort继续由Stack TCP owner command与
connection phase拥有，不能伪装成async cause。cause在consume或对应generation安全reclaim前不丢失、不重复，也不
携带Linux errno、Socket state、ready mask或caller phase。

Stack-private TCP owner唯一拥有Endpoint identity、binding/role、active/passive connection phase、bounded engine
composition、logical listener admission、half-open/completed pending child、connect/terminal outcome、generation、
timer与deferred reclaim。listener/engine/pending queue不能出现第二份admission truth；cancel、take、timeout或reclaim
必须在旧generation完全detach后才允许slot复用。Stage 1不向kernel发布可消费的TCP operation surface；private
production owner method由owner-local tests直接覆盖，Stage 2才解析kernel的窄consumer API。

Endpoint、engine/listener slot、completed child、RX/TX storage、timer与deferred-reclaim等重要capacity进入
`conf/.defconfig`中的owner-local Kconfig/build policy。xtask只忠实materialize配置，不clamp、补默认或重新解释
semantic value；kernel以`static_assert!`拒绝零值、overflow或不自洽组合。具体常量名和值仍由实现选择，但Stage 1
acceptance配置必须能让
一个logical listener同时保留至少十个completed pending child，从foundation层证明未来`listen(10)`不是被固定更小
结构上限阻断；该证明不提前发布Linux backlog ABI。

#### 6.3.5 Progression race、failure 与 cleanup

handoff必须覆盖worker已经park、正处于pump、刚清除旧request、多个producer合并以及mutation制造比已armed timer
更早工作的交错。stateless request只表达“重读Stack truth”，不复制deadline；worker醒来后仍从Stack pump outcome
刷新`next_deadline`。当path保持active时，任何已提交obligation都必须导致当前或后续bounded round观察它，不能依赖
无关IRQ、旧timer、later traffic或busy-poll。多个request可以合并，但合并不能把最后一个commit吞掉。

terminal shutdown继续沿用既有线性化：stop关闭`active`并清除explicit work后优先于并发late request，late request
不得恢复admission；本Stage不承诺terminal stop之后继续推进protocol，也不为此扩大System Power contract。若真实
race证明现有atomic/wait协议不足，且修复需要新permit、lock order、worker lifecycle或System Power语义，立即停止并
提交具体counterexample与最小contract delta，不能静默加入第二admission truth。

UDP/ICMP raw operation在mutation失败时保持原typed failure且不request；mutation成功后handoff本身不得allocation、
同步等待worker或引入新的user-visible failure。TCP capacity full是owner-local可恢复typed rejection/drop/backpressure，
不得依赖allocator偶然失败；在合法显式上界内的global allocation OOM继续按第5.2节允许kernel-fatal。listener child、
cause、timer与deferred-reclaim cleanup由同一TCP owner完成，stale generation不能cancel、consume或唤醒replacement；
Stage 1没有fd publication/final release，因此不得为尚不存在的Socket lifecycle建立rollback或桥。

#### 6.3.6 Implementation ordering

本Stage按以下语义顺序形成一个closure unit：

1. 永久启用`socket-tcp`，落地窄cause seam、owner-local capacity policy与Stack-private TCP owner；先用host tests证明
   bounded listener/resource/cause/generation路线，不发布kernel operation；
2. 在既有`DomainStack::protocol_transition()` commit/invalidations结构上加入protocol-owner progression outcome，
   并让attach composition可按affected interface解析既有local/external request capability；
3. 用确定性KUnit/source proof关闭park、in-flight、coalescing、earlier-work与late-stop交错；如果现有协议不足，按
   6.3.5停止，不以caller fallback继续；
4. 在一次迁移中删除control-plane `PumpWake`与UDP/ICMP raw两个caller wake，把两者切换到owner-driven handoff；
5. 完成host/config/双架构build与focused guest regression、Architecture Friction Scan和独立engineering review；全部
   满足后才写回current contracts并原子执行唯一cutover。

步骤顺序不冻结具体文件或commit。1可以作为不改变visible semantics的普通foundation commit，2--4必须保持最终
source没有旧/新双路径；任何普通commit都不授权下一Stage或contract生效。

#### 6.3.7 Validation、review 与 observability

Stage 1 closure必须同时给出以下证据：

- `just test net-host`：在既有suite中永久覆盖TCP `Reset`/`Timeout` one-shot、local abort exclusion、至少十个
  completed pending child、full/recovery、cancel/take、generation reuse、resource cleanup与no-default production
  build；不得新建第二个net-host wrapper；
- owner-local KUnit/source proof：三类protocol effect decision各自owner-local，Stack guard内commit/guard外handoff，
  association只来自`ActivePath`/`InitialDomain.local_path`，request在attach guard外提交，control plane与caller不再
  持有wake，以及park/in-flight/coalesced/earlier-work/late-stop matrix；
- `just test xtask`与source audit：Kconfig materialization faithful，重要capacity由kernel `static_assert!`执行语义
  检查，没有xtask clamp、第二配置truth或`anemone-net-api` TCP扩张；
- `just fmt kernel --check`，以及
  `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`和
  `just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G`；
- `./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/net-tcp-stage1-rv64.log`与
  `./scripts/run-user-test-la64.sh etc/preliminary/images/sdcard-la.img build/net-tcp-stage1-la64.log`：两架构都必须
  证明UDP与ICMP raw的local及external路径。现有`socket-test` local UDP/RAWICMP与
  BusyBox loopback/gateway ping只覆盖其中一部分；缺失的external UDP proof使用bounded、stage-scoped validation
  input/host orchestration，明确peer/guest双marker，并在closure前删除临时asset。不得恢复长期generic-wrapper peer、
  留下validation facade，或把单架构/loopback/source audit替代external execution；
- `git diff --check`、`mdbook build docs`、Stage 1 Architecture Friction Scan与独立engineering review。review必须
  明确检查第二状态truth、owner penetration、private representation leakage、public API expansion、caller/arch/test
  special case、无退出条件bridge、failure/cleanup顺序和降低validation诚实性的做法。

production不增加per-request log、diagnostic mirror或驱动admission的counter。host/KUnit marker和guest现有suite marker
提供执行证据；若为定位race临时增加trace/counter，必须标注纯诊断且在closure前删除，除非review证明其有长期
observability owner与不参与行为的边界。

TCP Socket/UAPI、TCP guest、CAgent、deployment probe、full network LTP、final harness、physical hardware、`smp>1`
与其它NIC/platform在Stage 1均Not Run；不得从TCP foundation host proof或UDP/ICMP guest回归外推这些轨道。

#### 6.3.8 Cutover、exit 与 stop conditions

只有6.3.7全部PASS、临时validation asset已删除、最终source没有caller fallback/第二truth、Architecture Friction
Scan没有本Stage引入或恶化的未处理Keter/Apollyon，且独立review接受时，Stage 1才可在同一closure write-back中：

1. 原子Refine current `NET-CONTROL-PLANE-001`与`NET-STACK-PUMP-001`并标记
   `NET-PROTOCOL-PROGRESSION-CUTOVER` Effective；
2. 把本页、父RFC与RFC导航更新为Stage 1 Closed，记录Git/PR、实际命令、结果、proof scope与Not Run；
3. 保持三项TCP Introduce与`SOCKET-ABI-001` Refine Pending，transaction仍为None，并立即停止，不解析或进入Stage 2。

任何一项迁移、验证、asset cleanup、friction scan、review或文档write-back失败时，Stage 1保持In Progress / Review
Hold，两个Network current contract继续旧规则，不允许partial cutover。需要长期时间线的真实执行跨度只能经review
再建立transaction；transaction不是activation或closure前置。

以下事实要求在implementation或cutover前立即停止并回到RFC review / Target Renegotiation：target/non-goal、owner/
handoff、failure/cleanup、超出上述construction policy seam的public API/visibility/shared contract、ABI、acceptance或
validation claim需要变化；需要新worker、
第二association/route/deadline/admission truth、shared effect policy或无退出条件bridge；需要把TCP object/operation加入
`anemone-net-api`、让kernel取得smoltcp private object、或让Stack取得Task/File/fd/waiter；需要新增syscall/typed user
copy、Socket/fd/wait/readiness或改变System Power shutdown语义；只能保留caller wake fallback、降低UDP/ICMP raw
oracle或永久validation facade才能通过。具体private type、module、method、lock、capacity数值、test fixture与普通
commit拆分只要满足本边界，属于implementation preference，不形成新的resolution gate。

#### 6.3.9 Stage 1 execution result — Closed

Stage 1在不改变R0 target、non-goals、owner、failure/cleanup、ABI、Contract Impact、acceptance或validation claim的
前提下关闭：

- `ProtocolProgression`是move-only、`must_use`的committed obligation，只携带affected `InterfaceId`；UDP与ICMP raw
  owner在Stack guard内成功commit后返回它，kernel attach composition在guard外解析既有local/external
  `PumpControl`并提交request。`PumpWake`、selection中的worker capability与两个caller `request_pump()`路径均已
  删除；source audit未发现caller fallback、第二association/deadline truth或`anemone-net-api` TCP扩张。
- production Stack通过construction-time `StackPolicy`拥有private TCP Endpoint、engine/listener/child、generation、
  timer与deferred-reclaim state；smoltcp只增加owner-local one-shot `Reset`/`Timeout` cause seam。listener覆盖至少十个
  completed child、full/recovery、FIN-before-take的`CloseWait` handoff、stale generation、timeout/late RST与checked
  engine accounting；没有发布kernel TCP operation、Socket、fd或UAPI surface。
- 六项TCP capacity由`conf/.defconfig`拥有，xtask只忠实materialize，kernel `static_assert!`拒绝零值、overflow和不
  自洽组合。`just test xtask`为`75/75` PASS；`just test net-host` PASS，其中focused smoltcp TCP为`178 passed`，
  Stack TCP owner、UDP、ICMP raw与no-default production build均通过。`just fmt kernel --check`、
  `just fmt socket-test --check`、双架构release build、`git diff --check`与`mdbook build docs`通过；RV64/LA64最终
  symbol entry分别为`6392`与`6016`。
- focused RV64/LA64日志分别为`build/net-tcp-stage1-rv64.log`与
  `build/net-tcp-stage1-la64.log`；两架构UDP `16/16`、ICMP raw `10/10`、whitelist LTP `6/6`通过，external UDP
  guest/peer双marker通过。`blocking-multi-waiter-signal`继续验证一个waiter被signal取消、另一个独立存活；fixture只
  在至少一次signal delivery成功后把child提前退出导致的`ESRCH`转为有界读取既有pipe result，预期payload、timeout、
  HUP与survivor oracle没有放宽。RV64 orderly shutdown且wrapper exit 0；LA64完成
  `filesystem -> network -> device -> PowerOff`顺序后因当前缺少实际power-off handler停在halt，人工终止后wrapper
  exit 130，因此不记为exit-0 PASS。
- 独立engineering review复核owner/race/lifecycle、`CloseWait` handoff、checked accounting与signal fixture后没有
  Apollyon、Keter或Euclid finding，并独立复跑`just test net-host` PASS。临时external validation asset已经删除；
  transaction保持None，执行证据由本页所在focused Git commit与上述日志拥有。

据此`NET-PROTOCOL-PROGRESSION-CUTOVER`原子Refine current `NET-CONTROL-PLANE-001`与
`NET-STACK-PUMP-001`并标记Effective。`NET-TCP-ENDPOINT-001`、`NET-TCP-STREAM-001`、
`NET-TCP-LIFECYCLE-001` Introduce和`SOCKET-ABI-001` Refine仍Pending。TCP Socket/UAPI、TCP guest、CAgent、
deployment probe、full network LTP、final harness、physical hardware、`smp>1`与其它NIC/platform均Not Run。
Stage 1 closure当时Stage 2--5保持Outline / Not Resolved / Not Authorized；Stage 1在此耗尽授权，不进入下一gate。

### 6.4 Stage 2 Resolved Gate — TCP owner与Socket-front integration

#### 6.4.1 成熟度、授权与前置基线

Stage 2是一个不执行semantic/contract cutover的formal execution gate，包含两个分别授权、分别review的execution
checkpoint。CKPT 2A必须形成独立安全且对current visible semantics中性的TCP owner capability；CKPT 2B在同一
Implementation Boundary内接入真实general Socket front，但保持TCP creation tuple与所有syscall handler不可达。
Stage 2默认不创建transaction，执行证据由各checkpoint的Git/PR与本页closure write-back拥有。

Implementation Resolution之后，用户明确决定Stage 2不做syscall，并先后分别授权CKPT 2A与CKPT 2B；没有授权
contract cutover或Stage 3解析。进入CKPT 2A前重新确认了以下live baseline，并在CKPT 2B开始前再次核验未漂移：

- Stage 1保持Closed，`NET-PROTOCOL-PROGRESSION-CUTOVER`保持Effective，三项TCP Introduce与
  `SOCKET-ABI-001` Refine仍Pending；register没有新增会改变本Stage owner/cleanup/validation的TCP问题；
- production `TcpEndpoints`仍是Stack-private唯一TCP owner，已经拥有bounded Endpoint/listener/engine/generation/
  cause/deferred-reclaim foundation，但kernel没有TCP Endpoint operation capability，`anemone-net-api`也没有TCP
  vocabulary；
- `DomainStack::protocol_transition()`继续在Stack guard内commit并取出invalidations/progression，在guard外路由
  recheck/request；control plane继续唯一选择route/source/interface；
- general Socket front已经有immutable `SocketOps`、`ByteStream` I/O bundle、`SocketCreation` rollback、
  `AcceptedSocket` cleanup与opened-description static final-release hook；现有四个Socket consumer及其ABI profile
  是current effective surface；
- `SocketAbiProfile`当前同时承载per-type ABI metadata与creation tuple admission，`resolve_socket_profile()`会遍历
  整张published profile表。Stage 2可以在general Socket owner内部行为保持地分离这两个职责，但任何
  `AF_INET + SOCK_STREAM + 0/IPPROTO_TCP`解析成功、syscall handler可达或现有tuple结果变化都越过本Stage；
- private TCP retire/cancel目前可能以`DeferredReclaimFull`失败。任何交给kernel或Socket rollback guard的capability
  在Stage 2都必须先取得不可失败、非阻塞的退役路线；不能把该failure丢给`Drop`/final-release hook、panic、leak或
  user-visible errno。

若baseline漂移要求改变R0 target、owner/handoff、failure/cleanup、ABI、Contract Impact、acceptance或validation
claim，先回到RFC review；不得把过期outline直接解释为implementation授权。

#### 6.4.2 Stage 2 Implementation Boundary

**Target：** 在Stage 1唯一Stack TCP owner上形成第一条真实kernel consumer路线，并以两个checkpoint完成
syscall-unreachable的Socket integration。CKPT 2A交付cross-crate TCP protocol vocabulary、kernel-private
Endpoint capability、active/passive connection、typed completion/cause observation、最小bounded scalar
send/receive transaction与不可失败retirement；CKPT 2B让general Socket front的create/address/connect/listen/
accept/scalar I/O/rollback/final-release hooks共同消费该capability。两者只证明owner、handoff、failure和cleanup
路线可以闭合，不把internal integration写成userspace vertical slice或TCP capability发布。

**Non-goals：** Stage 2不注册TCP creation tuple、不使`socket/bind/connect/listen/accept/read/write/send/recv`等
syscall到达TCP descriptor，不接收raw user pointer、不发布fd、不运行blocking或poll/select/epoll，不实现
`SO_ERROR`、`SO_REUSEADDR`、`TCP_NODELAY`、shutdown、SIGPIPE/`MSG_NOSIGNAL`、vector/message projection、完整
EOF/RST precedence、graceful final close、orphan/TIME_WAIT或用户侧fault oracle。它不运行TCP guest、CAgent、
deployment probe、network LTP或`NET-TCP-CUTOVER`，也不建立test-only syscall profile、Kconfig activation switch、
临时handler table、TCP-private wait loop、第二Socket front或长期validation facade。

**Protected boundary：** TCP Endpoint identity、binding namespace、role、connect/terminal outcome、listener/pending
child、stream capacity/reservation与reclaim继续只有Stack TCP owner一份truth；kernel只持opaque capability和一次
operation需要的typed value。route/source/interface继续只由control plane选择，Stack不接收Task/File/fd/user
pointer/errno/waiter，Socket不缓存binding、role、peer、pending child、buffer、cause、error或readiness。Stage 1
owner-driven progression handoff不能回退为caller wake。existing Socket tuple、UDP/ICMP raw/Unix visible semantics、
current contracts与R0最终acceptance保持不变，四项TCP target contract继续Pending。

#### 6.4.3 Owner、handoff、transaction与cleanup model

`anemone-net-api`只保存kernel与concrete Stack共同需要的opaque identity、normalized address/binding/peer、typed
request/outcome、operation-local reservation identity与point-in-time observation；它不拥有registry、queue、Arc、
callback、waiter、Linux errno或runtime state。kernel `net::tcp` capability持有initial-domain `DomainStack`的窄引用与
opaque Endpoint identity，不取得smoltcp handle、SocketSet、ring borrow、engine generation或owner lock。

bind/connect按“kernel向control plane验证local或取得selection -> DomainStack串行进入Stack TCP owner -> owner完成
namespace/engine commit -> 返回typed outcome/progression obligation”推进。TCP owner唯一分配nonzero ephemeral port、
判断default no-reuse下的wildcard/specific conflict、保存local binding与active connect phase；kernel不得为
`getsockname`、重试或errno mapping复制这些事实。影响外部行为的重要ephemeral range、Endpoint/engine/listener/
buffer/reclaim上界进入owner-local Kconfig；xtask只materialize，kernel consumer用`static_assert!`拒绝非法组合。

listen/accept只有一份pending-child truth。owner先把completed child作为generation-scoped capability交给kernel；
take成功后它成为新的Endpoint owner identity并原子rearm/reaccount listener slot，take前取消或take后Socket/file
preparation失败都由当前capability owner进入不可失败retirement。旧child、late cause或deferred cleanup不能命中
replacement generation；listener full、Endpoint/engine full与backlog admission full是typed recoverable结果。

scalar send先在kernel侧取得有界operation-local bytes，再由Stack owner一次提交真实可接纳prefix；未提交suffix
保持caller状态。receive由TCP owner发出exactly-once resolve的operation-local reservation：成功copy的prefix由同一
owner commit，未copy或copy失败部分rollback；reservation不得持有Stack guard、smoltcp ring borrow或允许Socket直接
消费engine。Stage 2虽然没有raw user copy，仍必须用general Socket source/sink fixture证明partial prefix与
copy-error rollback形状，不能把正确性推迟到syscall publication后再重写owner协议。

create、connect start、successful send admission、receive consume/window reopening、child cancel/release与任何使
protocol立即有work或提前deadline的retirement，都由Stack TCP owner在commit后返回Stage 1既有
`ProtocolProgression`；kernel composition必须在Stack guard外把它交给对应worker。普通observation、not-ready、
capacity rejection或receive rollback不制造虚假progression。

所有交给kernel的Endpoint、pending child和receive reservation必须有exactly-once cleanup owner。create/accept
rollback与Socket final-release hook是不可失败、不可阻塞的；owner必须在handoff前预留cleanup authority，或采用
即使外部reclaim queue饱和也能保留retiring truth并由worker后续推进的表示。具体credit、slot、scan或detach形状是
implementation preference，但`DeferredReclaimFull`不能越过handoff，cleanup不能等待worker/peer/timer，也不能释放
仍可被device/protocol/operation访问的generation。

#### 6.4.4 Module 与 visibility boundary

当前`stack/tcp.rs`已经同时包含policy、Endpoint/role、listener/child、connection、engine与reclaim职责；Stage 2再
加入namespace、active connect与stream transaction前先做同owner目录化。预期形状是让private TCP owner进入
`anemone-smoltcp-stack/src/tcp/`，按namespace、listener、stream与reclaim等稳定角色拆分，并让
`stack/tcp.rs`只保留`impl Stack`的aggregate operation/object-fence composition。具体文件名不是strict write set；
实现可以合并没有独立证明价值的薄文件，但不得继续把kernel-facing facade、owner state、smoltcp engine操作和全部
lifecycle塞回一个增长中的单文件。

目录化不拆semantic owner：`TcpEndpoints`或等价聚合owner仍唯一决定Endpoint identity、binding、role、listener、
connection、capacity与reclaim；子模块只实现该owner的稳定职责或保存participant-local value，不建立可独立推进的
`ListenerManager`、`StreamManager`、binding index truth或第二generation registry。跨子模块composition test放在最低
共同owner的`mod.rs` inline `#[cfg(test)] mod tests`，局部test留在对应语义文件末尾；不新建无独立编译/consumer/
lifecycle的`tests.rs`或validation facade。re-export保持最窄，smoltcp handle与private representation不能因拆分扩大
visibility。

#### 6.4.5 CKPT 2A — TCP owner capability

**Purpose / Deliverable：** 完成上述同owner目录化与cross-crate protocol vocabulary；把Stage 1 private foundation
扩展为create/bind/connect/listen/pending-child/scalar stream/release的完整Stack operation，并建立kernel-private
`net::tcp`/`DomainStack`窄capability。CKPT 2A必须用该kernel capability作为真实production consumer，不保留仅供
test调用的probe facade；它不进入`fs::socket`或任何ABI/profile/syscall路径。

**Acceptance：**

- active connect保留idle/bound/connecting/connected/terminal的owner distinction，首次start、重复in-progress、
  success、RST与timeout可由typed observation区分；没有kernel-sideshadow phase或elapsed-time guess；
- explicit/implicit bind、ephemeral allocation、default no-reuse conflict、logical listener至少十个completed child、
  take/cancel/rearm、active/passive child后续scalar bytes、capacity full/recovery与generation reuse共享唯一owner；
- send只commit owner实际接纳prefix；receive reservation在success、short copy、copy failure与Drop/cancel下exactly-once
  resolve，未提交bytes不丢失、不重复；
- create、child handoff、connection/listener retire与reservation cleanup在reclaim saturation下仍非阻塞、不可失败，
  engine/Endpoint/port最终恢复；每个产生effect的commit可靠提交`ProtocolProgression`；
- kernel capability不缓存Stack fact，不暴露smoltcp/private lock，不让`anemone-net-api`成为第二net core。

**Validation / Review：** `just test net-host`永久覆盖active/passive、cause、至少十个child、scalar partial/rollback、
capacity/reclaim saturation与stale generation；owner-local inline KUnit/source proof覆盖control-plane selection、
DomainStack guard内commit/guard外progression与kernel capability fence。若增加Kconfig policy，运行`just test xtask`
并审计xtask只materialize、kernel `static_assert!`拥有语义检查。完成`just fmt kernel --check`、RV64/LA64 release build、
`git diff --check`、Architecture Friction Scan与独立engineering review。TCP Socket、fd、syscall、guest、CAgent、
deployment probe、iomux与最终ABI evidence全部Not Run。

**Exit / Stop：** 全部acceptance/validation通过且review没有未处理Apollyon/Keter时，CKPT 2A才可标记Closed；Stage 2
保持In Progress / CKPT 2B Not Authorized，四项TCP target contract继续Pending，并立即停止。若owner surface只能通过
暴露private handle、复制binding/connect/child/error truth、保留fallible cleanup或改变shared Socket/public contract
成立，CKPT 2A保持Review Hold并回RFC review，不能进入CKPT 2B。

#### 6.4.6 CKPT 2A execution result — Closed

CKPT 2A在不改变R0 target、non-goals、owner/handoff、failure/cleanup、ABI、Contract Impact、acceptance或validation
claim的前提下关闭：

- `anemone-net-api`增加opaque Endpoint/pending-child/reservation identity与typed bind/connect/listen/stream/release
  vocabulary，但不拥有runtime state或发布kernel/smoltcp representation。Stack TCP owner按namespace、listener、
  stream与reclaim目录化；唯一聚合owner继续拥有Endpoint、binding、role、engine generation、pending child、stream
  capacity与reclaim truth。
- `DomainStack`在既有串行guard内进入TCP owner并在guard外提交`ProtocolProgression`，kernel-private
  `net::tcp` capability以opaque identity形成真实production consumer；kernel没有缓存Stack fact，也没有取得smoltcp
  handle、ring borrow、owner lock或Linux errno。`fs::socket`、profile、ABI、fd、wait、handler和syscall均未接入。
- explicit/implicit bind、nonzero ephemeral allocation、default no-reuse conflict、active connect typed observation、
  至少十个completed child的listen/take/cancel/rearm、scalar partial send与exactly-once receive reservation由同一owner
  覆盖。listener retirement会接管既有child-cancel deferred debt，generation-scoped cancel在child状态变化后仍保持
  authority；reclaim storage覆盖全部engine，因此Endpoint、child、listener与reservation cleanup不向kernel返回
  `DeferredReclaimFull`，也不等待worker、peer或timer。
- `conf/.defconfig`拥有ephemeral-port policy，xtask只materialize，kernel `static_assert!`拥有非法范围与capacity组合
  拒绝。`just test net-host` PASS：TCP owner `8/8`、focused smoltcp TCP `178/178`，no-default production
  build/check通过；`just test xtask`为`75/75` PASS。`just fmt kernel --check`、`git diff --check`与RV64/LA64
  release build通过，最终symbol entry分别为`6425`与`6018`。
- 独立engineering review检查owner、identity、listener/child retirement、receive reservation、reclaim saturation、
  progression与kernel fence，没有Apollyon、Keter或Euclid finding；未修改文件、未提交且未用额外运行替代主线证据。
  transaction保持None，执行证据由本页所在focused Git commit拥有。

据此CKPT 2A标记Closed，但不执行contract cutover。`NET-TCP-ENDPOINT-001`、`NET-TCP-STREAM-001`、
`NET-TCP-LIFECYCLE-001` Introduce和`SOCKET-ABI-001` Refine继续Pending；current Network、Socket、
Opened-description、IOMUX与Epoll contracts不变。TCP guest、CAgent、deployment probe、syscall、raw user-copy、fd、
blocking、poll/select/epoll、ABI/final harness、physical hardware、`smp>1`与其它NIC/platform均Not Run，source/host/build
proof不外推这些轨道。Stage 2保持In Progress，CKPT 2B Not Authorized；CKPT 2A在此耗尽授权，不进入下一checkpoint。

#### 6.4.7 CKPT 2B — Syscall-unreachable Socket integration

**Purpose / Deliverable：** 在CKPT 2A已经review接受的capability上增加kernel TCP family-private Socket state与static
`TCP_SOCKET_OPS`或等价immutable descriptor，让general Socket front直接覆盖create/address/connect/listen/accept、
nonblocking scalar send/receive、creation/accepted-child rollback和static final-release hook。TCP private Socket只持
Endpoint capability与operation-local guard，不保存owner fact；CKPT 2B不重开CKPT 2A的owner/handoff设计。

Stage 2可以为TCP semantic type提供syscall-unreachable的internal ABI metadata，并在general Socket owner内把
per-type metadata与published creation admission行为保持地分离；现有四个profile的tuple、flags、capability与message
policy必须逐项不变。TCP不得进入`resolve_socket_profile()`可达集合、syscall handler registration或任何test-only
activation switch，focused regression必须显式证明`AF_INET + SOCK_STREAM + 0/IPPROTO_TCP`仍按Stage 1 baseline
拒绝。descriptor为满足现有static capability bundle而保留的poll入口在Stage 2只能明确返回`Unsupported`，旁注它是
syscall-unreachable且由Stage 4真实source替换的temporary bridge；它不得被写成readiness proof或驱动operation结果。

connect adapter保留started、still-in-progress、connected与typed terminal cause的internal distinction；不得把它们
压成一份`WouldBlock/EAGAIN`或在Socket缓存pending error。address与scalar I/O只接收general front已经normalize的
value/source/sink，不能读取raw user pointer。`SocketCreation` authority、`AcceptedSocket` guard和opened-description
static final-release hook必须通过CKPT 2A不可失败retirement关闭所有未发布或最终释放的Endpoint；Stage 2不据此宣称
fd publication、dup/fork或真实semantic final close已经获得runtime proof。

**Validation / Review：** owner/host proof继续通过；kernel inline KUnit直接经general front准备TCP Socket，覆盖create
commit/drop、explicit/implicit bind、active connect repeated observation、listen/accept、accepted child在address/file
preparation失败时rollback、scalar partial/receive rollback、final-release exactly once与retired Endpoint拒绝。profile/
resolver KUnit必须证明现有四个consumer round-trip不变且TCP tuple不可达。完成`just test net-host`、受影响KUnit、
`just test xtask`（仅在config输入变化时）、`just fmt kernel --check`、RV64/LA64 release build、existing Socket/UDP/
ICMP raw/Unix source regression、`git diff --check`、Architecture Friction Scan与独立engineering review。

TCP syscall、raw user-copy、fd publication runtime、blocking、poll/select/epoll、TCP guest/CAgent/deployment probe、full
network LTP、final harness、physical hardware、`smp>1`与其它NIC/platform在CKPT 2B均Not Run；host/KUnit/build
不能外推这些轨道。

#### 6.4.8 Stage 2 exit、cutover与stop conditions

CKPT 2A与2B都Closed、temporary validation asset已删除、最终source没有test-only activation、第二truth、fallible
cleanup或private representation leakage，Architecture Friction Scan没有未处理Keter/Apollyon且final review接受时，
Stage 2才可在同一closure write-back中：

1. 标记Stage 2 Closed并记录两个checkpoint的Git/PR、实际命令、结果、proof scope与Not Run；
2. 明确`resolve_socket(AF_INET, SOCK_STREAM, 0/IPPROTO_TCP)`与全部TCP syscall route仍不可达，current Socket/
   Network/Opened-description/IOMUX/Epoll contracts不变；
3. 保持`NET-TCP-ENDPOINT-001`、`NET-TCP-STREAM-001`、`NET-TCP-LIFECYCLE-001`与`SOCKET-ABI-001` Refine Pending，
   transaction默认None，并立即停止，不解析或进入Stage 3。

Stage 2不执行partial cutover。任何checkpoint验证或review失败时，保留已经独立安全的前一checkpoint evidence，Stage 2
保持In Progress / Review Hold；不得把internal test PASS写成TCP capability、ABI或guest事实。

以下事实要求立即停止并回到RFC review / Target Renegotiation：需要发布TCP tuple/handler、raw user pointer、fd或
任何userspace-visible partial UAPI；需要改变现有Socket tuple/consumer行为、shared Socket public contract、
opened-description final-release规则或current contract；需要Stack取得Task/File/fd/waiter、kernel取得smoltcp handle/
ring borrow/owner lock，或Socket保存第二份binding/connect/child/error/readiness truth；receive只能lossy copy、重复
consume或无owner reservation，final release/rollback只能失败、阻塞、panic或leak；RST/timeout只能从merged closed
state猜测；progression只能依赖caller wake、unrelated IRQ/timer/traffic；或必须使用test-only profile、长期bridge、
validation facade、lowered oracle才能关闭。具体type、method、module文件名、锁、reservation/reclaim表示、普通commit
数量与行为保持型general Socket内部拆分，只要满足本边界，不形成新的resolution gate。

#### 6.4.9 CKPT 2B与Stage 2 execution result — Closed

CKPT 2B在同一Implementation Boundary内关闭，Stage 2据此整体关闭但不执行semantic或contract cutover：

- kernel TCP family-private `TcpSocketFile`只保存move-only Endpoint association与operation-local guard；binding、role、
  peer、connect outcome、stream与readiness仍由Stack TCP owner逐次提供。immutable `TCP_SOCKET_OPS`经general Socket
  front覆盖create/address/connect/listen/accept、nonblocking scalar send/receive、creation/accepted-child rollback与
  opened-description final-release；没有smoltcp handle、owner lock、raw user pointer、fd或第二份Socket truth越过fence。
- connect adapter保留started、in-progress、connected、reset/refused与timeout distinction；receive继续用owner
  reservation完成short-prefix commit与copy-fault rollback。creation authority、`AcceptedSocket` guard和final-release
  先撤销唯一association，再走CKPT 2A不可失败retirement。独立review指出listen/accept capacity一度被压成
  `InvalidState`的Euclid；收口前已改为internal `ResourceExhausted`并在不可达syscall adapter稳定映射`ENOBUFS`，
  KUnit固定该typed boundary，owner host test继续证明实际capacity fail/recover。
- general Socket owner把semantic metadata与published creation admission分成两张静态表；现有UDP、ICMP raw、Unix
  stream与Unix seqpacket四项tuple、flags、capability与message policy逐项不变。TCP metadata只服务internal
  descriptor，`resolve_socket_profile(AF_INET, SOCK_STREAM, 0/IPPROTO_TCP)`仍返回`SocketTypeNotSupported`；没有
  handler registration、test-only activation或Kconfig switch。temporary poll与accept wait bridge只返回
  `NotSupported`，并明确由Stage 4 owner-predicate source替换，未形成readiness claim。
- `just test net-host` PASS，其中TCP owner `8/8`、focused smoltcp TCP `178/178`，UDP、ICMP raw、frame path与
  no-default production checks继续通过。RV64 `smp=1` KUnit为`448/448` PASS，包括general-front active/passive、
  rollback/final-release、profile/resolver与capacity classification；KUnit结束后的旧Stage 1 UDP rootfs workload退出
  110，不属于本checkpoint TCP guest evidence。
- `just fmt kernel --check`、`git diff --check`与RV64/LA64 release build通过，最终symbol entry分别为`6456`与
  `6151`。existing Socket/UDP/ICMP raw/Unix source regression确认family implementation未改，四项published profile
  round-trip KUnit通过。config输入未变化，故`just test xtask`按本gate规则Not Run；没有创建temporary validation
  asset。
- 独立engineering review复核owner/capability、rollback/final-release、reservation、unpublished resolver fence、
  temporary bridge与Architecture Friction，未发现Apollyon或Keter；上述唯一Euclid已在收口前修复。最终scan没有
  第二truth、owner穿透、private representation泄漏、无退出条件bridge或降低oracle。review只读，未编辑或提交。

据此Stage 2标记Closed。`NET-TCP-ENDPOINT-001`、`NET-TCP-STREAM-001`、`NET-TCP-LIFECYCLE-001` Introduce与
`SOCKET-ABI-001` Refine仍Pending；current Network、Socket、Opened-description、IOMUX与Epoll contracts不变，
transaction保持None。TCP syscall、raw user-copy、fd publication runtime、blocking、poll/select/epoll、TCP guest、
CAgent、deployment probe、full network LTP、final harness、physical hardware、`smp>1`与其它NIC/platform均Not Run；
host/KUnit/build不外推这些轨道。Stage 2授权在此耗尽，立即停止，不解析或进入Stage 3。

### 6.5 Stage 3 Resolved Gate — Stream、ABI与lifecycle completion

#### 6.5.1 成熟度、授权与live baseline

Stage 3是不执行semantic/contract cutover的formal execution gate，包含两个分别授权、分别review的execution
checkpoint。CKPT 3A先完成Stack TCP owner的stream、option、error与lifecycle事实；CKPT 3B再把这些normalized
capability投影到仍不可由syscall到达的general Socket front。CKPT 3A必须独立安全且对current visible semantics
中性；CKPT 3B关闭整个Stage 3，但仍不注册TCP creation tuple或发布部分UAPI。

用户于2026-08-06先授权本节Implementation Resolution，随后单独授权CKPT 3A implementation；CKPT 3A现已按
第6.5.5节边界关闭。Stage 3保持In Progress，CKPT 3B仍为Resolved / Ready / Not Active / Not Authorized。
进入CKPT 3A前重新确认了以下live baseline没有语义漂移；closure后受保护边界仍保持不变：

- Stage 2保持Closed，四项TCP target contract保持Pending，current Network、Socket、Opened-description、IOMUX与
  Epoll contracts不变；`resolve_socket_profile(AF_INET, SOCK_STREAM, 0/IPPROTO_TCP)`仍拒绝，TCP syscall、fd、
  blocking与readiness route均不可达；
- Stack TCP owner继续唯一拥有Endpoint、binding、role、listener/child、connection phase、engine generation、
  cause、stream storage与deferred reclaim；现有kernel capability只持opaque Endpoint association和
  operation-local reservation，不缓存owner fact；
- current listener只受owner capacity约束而忽略Linux-normalized backlog；default bind conflict没有
  `SO_REUSEADDR` intent，stream没有`TCP_NODELAY` policy，pending Reset/Timeout cause也尚未形成一份可供
  `SO_ERROR`与ordinary operation共同消费的typed error truth；
- current scalar receive reservation能安全完成prefix commit/rollback，final release也可在outstanding reservation
  之后完成不可失败退役，但owner尚未表达buffered-bytes-before-EOF/error、orderly FIN、established RST、peek、
  direction shutdown、broken-stream send与graceful FIN/orphan/TIME_WAIT policy；
- kernel family-private `fs/socket/tcp.rs`同时承载file/source、creation rollback、final release、endpoint operation、
  scalar stream transaction、errno mapping与static descriptor；继续向该单文件加入message/options/shutdown/
  lifecycle职责会固化不自然的模块边界；
- general Socket message projection当前没有可供TCP stream复用的完整source/sink路线，option vocabulary也没有
  本Stage所需的normalized `SO_REUSEADDR`、consuming `SO_ERROR`与`TCP_NODELAY` dispatch；Stage 2的poll与accept
  wait bridge仍只返回`NotSupported`，其替换继续由Stage 4拥有；
- Linux ABI oracle继续固定为父RFC列出的Linux 6.6.32 snapshot。Stage 3只冻结外部可见case与precedence，不复制
  Linux内部socket/inet/tcp算法、锁或数据结构；register没有新增会改变本Stage target、owner、cleanup或validation
  的TCP问题。

若上述source/current contract/register事实变化已经影响target、owner/handoff、failure/cleanup、ABI、Contract
Impact、acceptance或validation claim，先回到RFC review或重新解析本Stage，不能按过期resolution执行。

#### 6.5.2 Stage 3 Implementation Boundary

**Target：** 在Stage 2同一owner topology与syscall-unreachable Socket integration上形成完整、可供Stage 4直接消费
的nonblocking TCP operation/lifecycle surface。CKPT 3A闭合Linux-normalized backlog admission、reuse conflict、
typed consuming error、bytes/EOF/RST、peek、direction shutdown、broken-stream send、Nagle policy、graceful final
release与bounded deferred reclaim；CKPT 3B闭合scalar/vector/message/file projection、flag/signal/option/shutdown/
copy-fault precedence和static final-release adapter。两者共同证明完整语义可以保持唯一owner、object fence与
partial-progress honesty，但不宣称userspace已经能调用TCP。

**Non-goals：** Stage 3不实现blocking connect/accept/send/receive，不发布owner-specific wait source或
poll/select/epoll predicate，不处理multiwaiter、signal cancellation或Stage 4并发矩阵；不注册
`AF_INET + SOCK_STREAM` creation tuple、handler或fd runtime route，不运行TCP guest、CAgent、deployment probe、
network LTP、final harness或`NET-TCP-CUTOVER`。它不实现`SO_REUSEPORT`、`MSG_WAITALL`、`MSG_MORE`、ancillary
producer、socket timeout、linger、keepalive、dynamic buffer option或其它R0 non-goal，也不建立通用mutable option
bag、shared pending-error cache、TCP-private wait loop、第二Socket front、test-only activation或长期validation
facade。

**Protected boundary：** Stack TCP owner继续唯一决定binding/listener admission、connection/stream/terminal fact、
pending error、protocol close/abort、orphan/TIME_WAIT与physical reclaim；kernel只传入normalized request/lifecycle
reason并取得typed result/capability，不取得smoltcp handle、ring borrow、owner lock或generation。Stack不得取得
Linux errno、raw user pointer、Task/File/fd/waiter或signal object。general Socket owner拥有raw ABI validation、
copy order、errno/signal projection和static descriptor composition，但不缓存可独立推进的TCP fact。

每次send只提交owner真实接纳且已经成功copy的prefix；每次receive/peek只通过exactly-once capability解析当前
owner snapshot，Socket不能直接消费engine。已缓冲bytes先于EOF或terminal error；orderly FIN在buffer耗尽后返回
zero，established RST不得伪装EOF。final release先withdraw kernel association，再进行不可失败、不可阻塞的protocol
release handoff；它不等待worker、peer、timer、FIN/TIME_WAIT完成或outstanding receive operation。Stage 1的
owner-driven progression handoff继续覆盖所有新producer，现有UDP、ICMP raw、Unix与published Socket tuple/ABI行为
逐项保持不变。

#### 6.5.3 Owner、error与lifecycle model

`anemone-net-api`只增加concrete Stack与kernel capability共同需要的normalized typed value：backlog/admission、reuse
intent、stream observation/reservation、pending-error consume、shutdown direction、Nagle policy和lifecycle reason。
它不发布Linux integer constant或errno，不拥有runtime registry、queue、Arc/callback/waiter，也不暴露smoltcp
representation。kernel `net::tcp`继续作为窄capability fence，进入`DomainStack`既有串行边界并在guard外提交
progression；不得为了转发新增operation复制Stack fact。

pending error只有Stack TCP owner一份可消费truth。connect failure、established reset与timeout先保持protocol-domain
typed cause，kernel在operation/ABI边界映射`ECONNREFUSED`、`ECONNRESET`、`ETIMEDOUT`等Linux errno；
`SO_ERROR`取得一份move-only consuming result，没有pending error时返回zero，重复query不能再次交付。ordinary
connect/send/receive与`SO_ERROR`竞争时必须由同一owner线性化谁消费cause；Socket不得先immutable query再另发clear，
也不得在Stack和Socket各存一份pending value。Linux 6.6.32中`SO_ERROR`在value/length copyout前consume的可见
precedence必须由focused oracle固定，copy fault不能通过kernel-side cache把已经消费的error复活。

本Stage不能把所有cleanup都压成一个不带原因的retire intent。creation rollback、accepted-child publication
rollback、listener withdrawal、semantic final release、explicit shutdown以及protocol terminal cleanup具有不同的
协议义务；kernel只交付窄的normalized lifecycle reason，Stack owner据Endpoint role/phase与该reason唯一决定
abort、FIN、half-close、orphan、TIME_WAIT、child cancel和deferred reclaim。具体enum、credit或queue形状是实现偏好，
但reason必须足以防止creation failure误发FIN、final close恒定abort、listener cleanup遗漏child，或shutdown绕过
direction truth。

receive reservation与final release保持可并发：final release可以先标记Endpoint withdrawal/retirement，既有
reservation随后仍必须exactly once地commit或rollback其generation-scoped capability；只有全部outstanding
capability已经resolve且protocol/timeout obligation允许时才能物理复用engine generation。peek只观察同一receive
truth，不推进consume cursor或window；其copy fault/drop不得改变后续普通receive结果。send admission、receive
consume造成的window reopening、Nagle policy mutation、shutdown、final release与listener/child cleanup中任何产生
immediate work或更早deadline的commit，都必须在返回前形成Stage 1 `ProtocolProgression`；普通observation、peek、
rollback或rejection不制造虚假request。

#### 6.5.4 Module 与 visibility boundary

CKPT 3B在向kernel TCP family继续加入message、option、shutdown与lifecycle projection前，必须把当前
`anemone-kernel/src/fs/socket/tcp.rs`转换为同owner目录。这个行为保持型目录化是Stage 3的implementation ordering，
不是独立checkpoint、public API变化或split-only commit要求；它不能被用来提前进入CKPT 3B，也不能改变现有调用路径
和visible semantics。

非strict的稳定角色如下：

- `tcp/mod.rs`保留窄family facade、static `TCP_SOCKET_OPS` composition与必要re-export；
- lifecycle职责保存creation/accepted-child rollback、association withdrawal与final-release adapter；
- endpoint职责保存bind/connect/listen/accept/address与normalized option/role projection；
- stream职责保存scalar/vector/message transaction、peek、shutdown、terminal precedence与broken-stream projection；
- normalized option dispatch只有在形成独立且实质的共同职责时才单列，否则留在最接近其owner的模块。

具体文件名可以合并，没有独立证明价值的薄wrapper不得为了对称而存在。TCP family不得新建承载raw Linux
struct/constant/errno的`abi.rs`；这些内容继续由general `fs/socket/api/`拥有。kernel `net/tcp.rs`当前仍是窄
cross-owner capability fence，不因family目录化机械拆分；只有真实implementation evidence表明它混入第二职责时才
在同一边界内调整。拆分不得扩大visibility、泄漏private state或建立新抽象层；owner-local KUnit留在对应语义文件
末尾的inline `kunits`，跨子模块composition test放在最低共同owner的`mod.rs`，不新建无独立consumer/lifecycle的
`tests.rs`、`kunit.rs`或validation facade。

#### 6.5.5 CKPT 3A — Stack owner fact与lifecycle completion

**Purpose / Deliverable：** 扩展Stage 2唯一Stack TCP owner及kernel-private `net::tcp` capability，交付Stage 3所有
protocol-domain normalized fact和lifecycle action，但不进入`fs::socket`的ABI/message/option adapter。CKPT 3A至少
完成：

- per-listener normalized backlog、completed-child admission与re-listen grow/shrink；backlog受existing owner-local
  capacity硬上界约束，capacity full继续typed recovery，不建立第二queue length truth；
- `SO_REUSEADDR` intent以及wildcard/specific、listener、connected/TIME_WAIT reservation conflict；放宽必须要求
  Linux oracle规定的相关reservation opt in，仍禁止duplicate live listener和fake `SO_REUSEPORT`；
- typed connect failure、established terminal failure与一份consuming pending-error truth，ordinary operation与
  `SO_ERROR` consumer共享同一线性化owner；
- bytes-before-EOF/error、orderly FIN、established RST、peek observation、send capacity、local read/write shutdown、
  broken-stream send、shutdown repetition与`TCP_NODELAY`对应的engine policy；
- creation/accepted-child/listener/final-release/shutdown原因分离，以及graceful FIN、orphan/TIME_WAIT、timer、child与
  engine deferred reclaim；final release和rollback仍不可失败、不可阻塞，旧generation不能命中replacement；
- send、receive-window reopening、policy mutation、shutdown、final release与cleanup的owner-driven progression
  obligation；失败、无mutation与pure observation不产生caller wake替代品。

**Acceptance：** owner host tests必须逐项证明backlog 0/1/10/capacity与re-listen，reuse conflict和duplicate listener
拒绝，connect reset/timeout与established reset cause，`SO_ERROR` zero/consume/repeat/ordinary-operation race，partial
send、peek、bytes-before-FIN/RST、all shutdown directions/repetition、broken send、final release/outstanding receive、
listener/child cleanup、TIME_WAIT/rebind、capacity recovery和generation isolation。focused smoltcp test可以证明
`close()`、`abort()`、`may_send`、`may_recv`、send capacity、receive queue与Nagle mapping，但不能替代Stack owner
policy/cleanup proof。

**Exit / Stop：** CKPT 3A全部acceptance、validation与review满足后只标记CKPT 3A Closed；Stage 3保持In Progress，
CKPT 3B仍Not Authorized，四项TCP target contract继续Pending，并立即停止。若只能泄漏engine/private state、在
kernel缓存pending error/phase、用同一retire action近似所有lifecycle原因、让final release等待或失败、或需要
general Socket contract变化才能形成owner capability，保持Review Hold并回RFC review，不能进入CKPT 3B。

#### 6.5.5.1 CKPT 3A execution result — Closed

CKPT 3A在同一Implementation Boundary内关闭，不执行semantic或contract cutover：

- Stack TCP owner实现normalized backlog与re-listen grow/shrink，Endpoint-owned `SO_REUSEADDR`与
  `TCP_NODELAY`，双方opt-in reuse、duplicate-listener拒绝和避开exact 4-tuple冲突的peer-aware ephemeral scan；
  binding、listener、peer tuple与engine reservation继续只有一份owner truth。
- typed pending connect/error result由Stack owner消费；engine区分establishment前后Reset并覆盖simultaneous open，
  production active-connect timeout由Kconfig拥有且在establishment后清除。partial send、peek、buffered bytes先于
  FIN/RST、EOF、direction shutdown与broken-send fact均由同一stream owner提供，不在kernel缓存phase/error truth。
- explicit final release与rollback abort reason分离，graceful FIN、engine-owned orphan timeout和outstanding receive
  reservation的最终释放由owner闭合。Stage 2 `retire()`只保留为CKPT 3B前的legacy rollback/abort bridge；explicit
  `FinalRelease`是唯一graceful FIN路径，non-consuming connection observation同样带CKPT 3B删除条件。
- `just test net-host` PASS：TCP owner `18/18`、focused smoltcp TCP `178/178`，UDP、ICMP raw、frame path与
  no-default production checks继续通过。`just test xtask`为`75/75` PASS；RV64/LA64 release build通过，最终
  verified symbol分别为`6516`与`6160`。
- exact-source RV64 wrapper
  `./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/net-tcp-stage3-ckpt3a-rv64.log`
  完成orderly shutdown：KUnit `448/448`，UDP `16/16`、UDP extended `10/10`、UDP message `7/7`、Unix stream
  `23/23`、Unix seqpacket `4/4`、raw ICMP `10/10`、Rust command `2/2`，glibc与musl LTP合计`6/6`。
- `just fmt kernel`与双架构build在最终source上通过；closure write-back后再以`just fmt kernel --check`、
  `mdbook build docs`和`git diff --check`确认最终提交源。独立engineering review最终结论为
  `0 Apollyon / 0 Keter / 0 Euclid`，只读review没有修改文件；Architecture Friction Scan没有未处理finding。

据此CKPT 3A标记Closed，Stage 3保持In Progress。四项TCP target contract继续Pending，current Network、Socket、
Opened-description、IOMUX与Epoll contracts不变，transaction保持None。published resolver/profile、TCP Socket front、
userspace ABI、blocking/readiness与test profile均未修改。TCP syscall/runtime因resolver/profile仍未发布而Not Run；
blocking/readiness、TCP guest、CAgent、deployment probe、full network LTP、final harness、physical hardware、`smp>1`
与其它NIC/platform同样Not Run。CKPT 3A授权在此耗尽，CKPT 3B保持Not Active / Not Authorized，不进入下一checkpoint。

#### 6.5.6 CKPT 3B — Syscall-unreachable Socket/ABI completion

**Purpose / Deliverable：** 在review接受的CKPT 3A capability上先完成第6.5.4节目录化，再通过general Socket
source/sink、option和static descriptor composition交付完整nonblocking ABI candidate。TCP creation profile继续只作
semantic metadata，不进入published resolver。CKPT 3B至少完成：

- scalar/vector/file与connected stream `sendto/recvfrom/sendmsg/recvmsg`共享同一prefix transaction；iovec count、
  length sum、overflow和target上界在allocation前验证，short copy只提交真实prefix；
- send flags只接受`MSG_DONTWAIT | MSG_NOSIGNAL`，receive只接受`MSG_DONTWAIT | MSG_PEEK`；broken-stream send
  映射`EPIPE`并请求一次`SIGPIPE`，本次`MSG_NOSIGNAL`只抑制该signal，unsupported flags稳定拒绝；
- message name/control/header、zero length、partial vector与copy-fault precedence遵循focused Linux oracle。R0没有
  ancillary producer，non-empty send control稳定拒绝，receive control输出为空；不得建立TCP-local raw user-copy
  bypass；
- `listen(int backlog)`先按Linux 6.6.32可见规则归一化：negative与超过上界的输入都clamp到配置上限，再把窄
  normalized backlog交给Stack；re-listen不得绕过owner role/admission；
- general option dispatch交付bool-like `SO_REUSEADDR`、consuming `SO_ERROR`和`TCP_NODELAY` query/mutation，并
  保留level/name/optlen/value/copy precedence；unknown level/option继续稳定拒绝，Socket不建立mutable option bag；
- `shutdown(SHUT_RD/SHUT_WR/SHUT_RDWR)`只做ABI normalization和errno projection，direction/protocol action由Stack
  owner决定；role/state、unconnected/listener、pending bytes与repeated shutdown按focused oracle固定；
- static final-release与rollback adapter传递正确lifecycle reason，并以inline KUnit固定creation rollback、rejected
  accepted-child、semantic final release和outstanding receive/peek的race；Stage 2 temporary poll/accept bridge保持
  `NotSupported`，不在本checkpoint偷渡readiness。

general Socket source/sink、message或option capability若需要owner-neutral的窄扩展，必须由general Socket owner
实现并保持现有UDP、ICMP raw与Unix consumer逐项回归；不得以TCP-local syscall parsing、family-specific message
loop或复制common copy state规避共同owner。只要该扩展位于父RFC已经接受的general Socket ABI/descriptor capability
与Pending `SOCKET-ABI-001` Refine上界内、没有改变effective public contract，它属于本Stage implementation；若
需要扩大public surface、改变existing consumer contract或引入第二shared truth，立即停止并回RFC review。

**Acceptance：** focused KUnit必须经真实general front与TCP descriptor覆盖scalar/vector/message zero/short/fault、
iovec overflow/bound、peek不消费、bytes-before-FIN/RST、shutdown matrix、EPIPE/SIGPIPE/`MSG_NOSIGNAL`、backlog
normalization、reuse matrix、`SO_ERROR` zero/consume/repeat/copy-fault/ordinary-operation race、`TCP_NODELAY`、message
name/control/header顺序、rollback/final release race与retired Endpoint拒绝。profile/resolver test必须继续证明四项
existing consumer不变，且`AF_INET + SOCK_STREAM + 0/IPPROTO_TCP`仍返回`SocketTypeNotSupported`。

#### 6.5.7 Implementation ordering

Stage 3按以下顺序形成两个独立授权的closure unit：

1. 在实现前把Linux 6.6.32 focused oracle case固定为owner/KUnit验收矩阵：backlog覆盖negative、0、1、10、超过
   capacity与re-listen grow/shrink；reuse覆盖wildcard/specific、双方/单方opt in、listener、connected/TIME_WAIT
   rebind与duplicate listener；error覆盖connect RST/refused、timeout、established reset及`SO_ERROR`全部消费顺序；
   stream覆盖zero/short/vector fault、peek、bytes-before-FIN/RST、shutdown combination/repetition、SIGPIPE/
   `MSG_NOSIGNAL`及message output precedence；
2. 单独授权并完成CKPT 3A，只扩展Stack owner与kernel-private capability；review closure后停止，不开始family ABI；
3. 经用户单独授权CKPT 3B后，先把`fs/socket/tcp.rs`行为保持地目录化，再加入general Socket message/option/
   shutdown/lifecycle adapter；不为split制造第三checkpoint或semantic gate；
4. 完成owner/KUnit/build/regression、Architecture Friction Scan与独立engineering review；只有两个checkpoint都
   Closed且第6.5.9节exit成立时，才把Stage 3标记Closed并立即停止。

步骤不冻结具体type、method、lock、enum、普通commit数量或上文非strict文件名。CKPT边界按semantic owner与
独立安全性形成，不能把尚未闭合的3A fact临时缓存在Socket，也不能把3B部分UAPI提前发布来取得runtime evidence。

#### 6.5.8 Validation、review 与 observability

每个checkpoint只记录实际执行的证据，不从前一Stage或另一checkpoint外推。Stage 3 implementation closure至少需要：

- `just test net-host`覆盖第6.5.5节全部owner、engine、cause、lifecycle、capacity与generation matrix，并保持UDP、
  ICMP raw、frame path和no-default production checks；
- repository-owned RV64 KUnit flow覆盖第6.5.6节general-front ABI/fault/race矩阵、profile/resolver fence和existing
  Socket consumer regression；validation必须走真实production descriptor/capability，不增加test-only activation、
  standalone probe facade或缺少consumer的support API；
- `just fmt kernel --check`、RV64/LA64 release build与`git diff --check`；只有config输入变化时才运行
  `just test xtask`并审计xtask只materialize、kernel `static_assert!`拥有semantic validity；
- source/behavior regression确认UDP、ICMP raw、Unix stream/seqpacket、general Socket source/profile、opened-
  description final release与Stage 1 progression handoff没有变化；目录化前后external call path、visibility与
  static descriptor保持等价；
- 每个checkpoint执行Architecture Friction Scan与独立engineering review。review必须检查第二binding/error/
  lifecycle truth、owner penetration、private representation leakage、为TCP扩大public API、family-specific ABI/
  copy/wait path、无退出条件bridge、单一retire意图、failure/cleanup/progression顺序、peek/receive capability与通过
  降低oracle/validation换取通过。

production不增加驱动行为的diagnostic error mirror、close counter或ready mask。若为race定位临时增加trace/counter，
必须明确纯诊断、不参与state machine并在checkpoint closure前删除，除非review确认长期owner和观测需求。

CKPT 3A只记录第6.5.5.1节实际执行的owner/engine/config/build与既有共享回归证据。TCP syscall/guest/CAgent/
deployment、poll/select/epoll、blocking/signal-cancel runtime、full network LTP、final harness、physical hardware、
`smp>1`与其它NIC/platform在CKPT 3A保持Not Run；host/KUnit/build不外推这些轨道，也不以激活临时profile取得本Stage
不拥有的runtime证据。

#### 6.5.9 Stage 3 exit、cutover与stop conditions

CKPT 3A与3B都Closed、temporary validation asset已删除、最终source没有test-only activation、第二truth、fallible
cleanup、private representation leakage或TCP-local common-ABI bypass，Architecture Friction Scan没有未处理
Keter/Apollyon且final review接受时，Stage 3才可在同一closure write-back中：

1. 标记Stage 3 Closed，记录两个checkpoint的Git/PR、实际命令、结果、proof scope与Not Run；
2. 明确TCP tuple、handler、fd/runtime和owner-specific wait source仍不可达，Stage 2 temporary readiness bridge仍由
   Stage 4替换，current contracts不变；
3. 保持`NET-TCP-ENDPOINT-001`、`NET-TCP-STREAM-001`、`NET-TCP-LIFECYCLE-001`与`SOCKET-ABI-001` Refine
   Pending，transaction默认None，并立即停止，不解析或进入Stage 4。

Stage 3不执行partial cutover。任一checkpoint失败时保留已经独立安全的前一checkpoint evidence，Stage 3保持
In Progress / Review Hold；不得把internal test PASS写成TCP UAPI、blocking/readiness、guest或contract事实。

以下事实要求立即停止并回RFC review / Target Renegotiation：需要发布TCP tuple/handler/fd或任何userspace-visible
partial UAPI；需要改变R0 target/non-goal、shared public API/current contract或existing consumer behavior；需要
Stack取得Linux errno/Task/File/fd/waiter、kernel取得smoltcp private object，或Socket保存第二份binding/phase/
pending-error/terminal truth；只能用一个无差别retire intent处理rollback/shutdown/final release；peek只能通过consume
后补回、receive只能重复/lossy commit、final release只能等待/失败/panic/leak；RST/EOF/`SO_ERROR`只能猜测或以恒值
伪装；任一新mutation缺少owner-driven progression；必须提前实现blocking/readiness、使用test-only route、长期
bridge、caller/test special case或降低Linux oracle/validation才能关闭。行为保持的同owner目录化与general Socket
内部窄扩展只要满足本节边界，不构成额外resolution gate。

#### 6.5.10 CKPT 3B 与 Stage 3 execution result — Closed / Not Cut Over

CKPT 3B在既定Implementation Boundary内关闭；Stage 3两个checkpoint均Closed，但不执行semantic或contract cutover：

- `fs/socket/tcp.rs`行为保持地拆为同owner的family facade、lifecycle与stream子模块。creation rollback、accepted-child
  rollback和final release使用显式reason；static final-release在outstanding consume、peek与copy-fault rollback期间
  撤销唯一Endpoint capability，reservation resolve后由Stack owner完成deferred reclaim，不建立第二lifecycle truth。
- Stack唯一拥有backlog、reuse intent、connection phase、pending error、stream terminal与shutdown fact。general Socket
  front增加owner-neutral byte-stream message、option和shutdown adapter；TCP覆盖scalar/vector/message prefix transaction、
  `MSG_PEEK`、`MSG_NOSIGNAL`/`SIGPIPE`、`SO_REUSEADDR`、consuming `SO_ERROR`与`TCP_NODELAY`，不建立TCP-local raw-copy
  path或mutable option/error bag。TCP未连接的file与syscall receive projection均保留typed `ENOTCONN`，Unix既有
  `InvalidState -> EINVAL` file语义不变。
- TCP metadata仍位于unpublished static descriptor；published resolver继续拒绝
  `AF_INET + SOCK_STREAM + 0/IPPROTO_TCP`。TCP tuple、handler、fd/runtime route与owner-specific wait source仍不可达；
  Stage 2 temporary poll bridge继续只返回`NotSupported`，由未解析的Stage 4替换。
- `just test net-host` PASS：TCP owner `18/18`、focused smoltcp TCP `178/178`，UDP、ICMP raw、frame path与no-default
  production checks继续通过；其后修复仅位于kernel Socket adapter与KUnit，没有改变这些host owner source。
- 最终source上的`just fmt kernel --check`通过；RV64 release build由repository-owned wrapper再次完成，verified symbol为
  `6596`；LA64 release build通过，verified symbol为`6027`。config输入未变化，故`just test xtask`未运行。
- exact-source RV64 wrapper
  `./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/net-tcp-stage3-ckpt3b-rv64.log`
  正常退出并完成orderly shutdown：KUnit `459/459`，新增TCP file `ENOTCONN`与static final-release
  consume/peek/fault矩阵均通过；UDP `16/16`、UDP extended `10/10`、UDP message `7/7`、Unix stream `23/23`、
  Unix seqpacket `4/4`、raw ICMP `10/10`、Rust Command `2/2`继续通过，glibc与musl socket LTP合计`6/6`。
- 独立engineering review在修正terminal-error第二truth、TCP receive name投影、production adapter coverage与
  `read/readv`未连接errno后，最终结论为`0 Apollyon / 0 Keter / 0 Euclid`并接受closure。Architecture Friction Scan
  未发现残留第二状态真相、owner穿透、private representation泄漏、TCP-local common-ABI bypass、无退出条件bridge或
  Keter/Apollyon；测试端口隔离只修正fixture collision，不进入production policy。

据此CKPT 3B与Stage 3标记Closed / Not Cut Over。`NET-TCP-ENDPOINT-001`、`NET-TCP-STREAM-001`、
`NET-TCP-LIFECYCLE-001` Introduce与`SOCKET-ABI-001` Refine继续Pending；current Network、Socket、
Opened-description、IOMUX与Epoll contracts不变，transaction保持None。TCP syscall/runtime、blocking/readiness、
TCP guest、CAgent、deployment probe、full network LTP、final harness、physical hardware、`smp>1`与其它NIC/platform
均Not Run。Stage 4保持Outline / Not Resolved / Not Authorized；Stage 3授权在此耗尽，立即停止。

### 6.6 Stage 4 Resolved Gate — Userspace vertical slice、blocking/readiness与concurrency hardening

#### 6.6.1 成熟度、授权与live baseline

Stage 4是一个不执行contract cutover、但在第二个checkpoint发布完整syscall-reachable candidate的formal execution
gate。它包含两个分别授权、分别review的execution checkpoint：CKPT 4A先在TCP tuple继续不可达时闭合owner
predicate、invalidation、Socket source与wait capability；CKPT 4B再闭合剩余Linux syscall projection，通过唯一normal
resolver activation发布完整candidate，并立即以repository-owned RV64 userspace consumer开始纵向验证。CKPT 4A
必须独立安全且对current visible semantics与current contracts中性；CKPT 4B完成整个Stage 4，只承担一次candidate
activation，不执行`NET-TCP-CUTOVER`。

用户于2026-08-06先授权本节Implementation Resolution，随后单独授权并关闭CKPT 4A；CKPT 4B仍未授权。以下是进入
CKPT 4A前实际核验的live baseline；执行结果见第6.6.10节：

- Stage 3保持Closed / Not Cut Over，四项TCP target contract继续Pending；current Network、Socket、Opened-
  description、IOMUX与Epoll contracts不变，`resolve_socket_profile(AF_INET, SOCK_STREAM, 0/IPPROTO_TCP)`继续
  拒绝，TCP syscall、fd、blocking与readiness route均不可达；
- Stack TCP owner已经唯一表达Endpoint role、connect outcome、pending child、stream capacity、RX bytes、EOF/error、
  shutdown与lifecycle fact，kernel capability只持opaque Endpoint association与operation-local reservation；但TCP
  尚未提供完整point-in-time readiness facts、Endpoint invalidation vocabulary或kernel reverse observer route；
- `DomainStack::protocol_transition()`已经在Stack guard外路由UDP与ICMP raw invalidation，production
  `SocketPollSource`已经提供fallible route publication、snapshot/register recheck、retire withdrawal与guard-out
  notification；当前两者都不覆盖TCP，Stage 4必须扩展真实TCP consumer而不是建立第二source protocol；
- TCP family的temporary `poll_unpublished_tcp()`仍返回`NotSupported`，accept would-block只携带该bridge；general
  Socket wait orchestration已经可用，send/receive blocking driver通过file source重试，connect syscall仍把TCP
  `Started`、`InProgress`与timeout映射为placeholder `NotSupported`；
- static `TCP_ABI_METADATA`已经覆盖完整R0 tuple、flag与message profile，但published resolver表仍只包含UDP、ICMP
  raw与Unix；因此CKPT 4B可以只形成一个normal production activation point，不需要Kconfig、test profile、private
  syscall或第二resolver；
- repository已有boot-root `socket-test`、competition-root glibc/musl runtime、focused C oracle staging与RV64/LA64
  end-to-end wrapper。Stage 4复用这些production harness边界；不建立host socket proxy、test-only kernel route或长期
  TCP validation facade。

若上述source/current contract事实变化已经影响target、owner/handoff、failure/cleanup、ABI、Contract Impact、
acceptance或validation claim，先回到父RFC review或重新解析本Stage，不能按过期resolution执行。

#### 6.6.2 Stage 4 Implementation Boundary

**Target：** 在Stage 3同一Stack TCP owner、general Socket front与opened-description topology上形成完整blocking与
readiness capability，并把完整R0 candidate发布给普通用户程序。CKPT 4A让connect completion、pending child、TX
admission、RX bytes/EOF/error各自从owner fact生成point-in-time snapshot与recheck-only invalidation，通过kernel TCP
source接入existing `SocketPollSource`、operation wait与poll/select/epoll projection；TCP creation tuple继续不可达。
CKPT 4B先闭合blocking/nonblocking connect、accept、send/receive、errno/signal与generic syscall retry，再把同一
static profile加入唯一normal resolver，并以RV64 glibc/musl focused C consumer验证真实fd、copy、wait、retry、
opened-description与failure cleanup纵向路径。

**Non-goals：** Stage 4不执行`NET-TCP-CUTOVER`，不更新Pending TCP contracts或宣称TCP Effective；不完成LA64
userspace、self/remote-external、CAgent transport marker、条件性deployment probe、full shared matrix或architecture
capstone，这些仍由Stage 5拥有。它不新增partial tuple、nonblocking-only profile、private syscall、test-only resolver、
Kconfig activation switch、第二wait loop、ready-mask bus、TCP-private fd/publication path或长期validation abstraction；
也不扩张R0 non-goals、实现IPv6/diag/procfs、socket timeout/linger/keepalive或其它未选择UAPI。

**Protected boundary：** Stack TCP owner继续唯一决定binding/listener/connection/stream/terminal/error/resource与
protocol lifecycle fact；kernel TCP source只持opaque Endpoint association、reverse invalidation registration与
non-owning poll routes，并在每次snapshot重新读取owner fact。general Socket owner继续唯一拥有Linux tuple、errno、
signal、blocking choice、fd publication与wait orchestration；iomux/epoll继续只拥有round/watch/policy/final harvest。
不得让Stack取得`PollEvent`、Linux errno、Task/File/fd/waiter，不得让Socket缓存connect phase、pending error、child、
RX/TX capacity、EOF或terminal truth，也不得让notification携带mask、errno、child或operation outcome。

每个operation读取自己的owner-defined predicate；一份role-aware point-in-time fact carrier可以同时服务同一Endpoint的
多种projection，但它不能成为可独立推进的`tcp_ready` state或跨round cache。source route publication与current
snapshot必须位于同一source critical section；Stack先提交fact并取得typed invalidation，释放Stack guard后才路由
observer hint。source notification必须覆盖所有可能改变final projection的route，包括mandatory ERROR/HUP和请求的
RDHUP；允许保守多发recheck hint，不允许漏掉transition或让hint决定用户结果。

**Failure / cleanup：** TCP source preparation在fd publication前完成fallible allocation、reverse observer
registration与association publication；任一步失败都由unpublished creation owner撤销observer并以
`CreationRollback`释放同一Endpoint。accepted child在source preparation、peer copy或fd publication失败时以
`AcceptedChildRollback`释放，不能requeue或留下不可达child。semantic final release先withdraw source并detach全部
routes，再注销reverse observer，最后以`FinalRelease`向Stack提交non-blocking release；它不获取sleeping operation
guard、不等待waiter/worker/FIN/TIME_WAIT。signal、timeout、force、losing waiter或epoll watch removal只retire本轮
consumer state，不取消connect、child、stream或其它waiter；late/repeated invalidation对retired source fail closed。

#### 6.6.3 Owner fact、source与readiness model

Stack TCP owner必须提供足够的role-aware current fact来投影以下互不替代的predicate：

- active connect：未开始/已绑定、in progress、connected与typed failed/terminal outcome保持可区分；connect
  completion无论success或failure都结束当前connect wait，pending error仍由Stack一份truth拥有；
- listener：只有当前listener的completed pending child可满足accept/readable predicate；receive bytes或其它Endpoint
  activity不能复用为accept truth，claim/take/cancel后必须重新读取队列事实；
- stream send：writable只表示当前Endpoint对至少一个byte的最低admission hint，broken stream或terminal error必须
  使operation退出wait并按ordinary send/`SO_ERROR`竞争规则重新读取owner outcome；
- stream receive：buffered bytes、EOF、pending terminal error与local read shutdown按既有bytes-first规则决定
  readable；peer FIN产生receive-half-close fact，不能与complete HUP、local shutdown或RST合并；
- public poll：listener readable、connect completion writable/error、RX bytes/EOF readable、TX admission writable、
  peer FIN RDHUP、terminal HUP与pending ERROR按固定Linux 6.6.32 oracle投影。ERROR/HUP是mandatory result，RDHUP只在
  caller请求时交付；具体bit组合由focused oracle固定，不能从operation名称或历史edge猜测。

TCP Endpoint invalidation必须覆盖pump ingress/egress/timer导致的handshake、child、capacity、RX/FIN/RST与terminal
transition，也覆盖pump外connect/send/receive-window reopening/shutdown/release等committed mutation。具体是否按
Endpoint合并、怎样存储batch及fact carrier/type名称仍是implementation preference；但invalidations必须在同一次
Stack access window取出，不能靠周期轮询、无关provider IRQ、另一个syscall或worker偶然运行补偿。

kernel TCP source复用shared `SocketPollSource`的publication/retire protocol。若TCP的mandatory terminal category要求
对shared source内部做owner-neutral窄扩展，该扩展必须保持UDP/ICMP raw现有predicate、visible result与current
contract不变，并以existing consumer regression证明；不得为TCP复制source、在generic source内识别TCP role，或让
shared source计算family readiness。source poll callback只能取得non-sleeping current fact，不能持source guard进入
sleeping operation mutex；producer notification和最后reference drop都发生在Stack/source guard外。

#### 6.6.4 Blocking、Linux mapping与activation model

blocking与nonblocking connect必须消费同一Stack outcome，但syscall owner可以保存只属于当前调用的
`started-by-this-call`事实，以区分一次blocking connect自己启动后完成返回0，和调用开始前已经connected返回
`EISCONN`；该operation-local marker不复制protocol phase，也不能跨syscall或写回Socket。首次nonblocking start返回
`EINPROGRESS`，仍在推进的重复nonblocking调用返回`EALREADY`，已经connected返回`EISCONN`；RST/refused、timeout与
route/source/admission failure分别映射`ECONNREFUSED`、`ETIMEDOUT`与对应`ENETUNREACH/EADDR*/ENOBUFS`。并发
blocking/nonblocking caller、ordinary operation与`SO_ERROR`消费竞争的exact result在implementation前由focused Linux
oracle冻结；不能增加kernel error mirror使多个consumer都取得同一pending error。

accept在no-child时返回绑定到listener source的operation wait；send/receive继续由shared retry owner在每次
`WouldBlock`后释放family operation guard，再用file source等待并重新执行完整operation。`O_NONBLOCK`、creation-time
`SOCK_NONBLOCK`与per-call `MSG_DONTWAIT`只改变是否等待，不改变predicate或opened-description status；signal只终止
当前blocking round，不能撤销已经启动的connect或影响其它waiter。public readiness只是hint，operation retry仍负责
最终role、capacity、error与partial-progress判断。

resolver activation只能发生在以下条件同时满足后：4A source/wait closure已独立review；剩余connect/errno/signal
projection及全部R0 ordinary operation不再因未完成Stage 4工作落入placeholder `EOPNOTSUPP/ENOTSUP`；creation、accept
与message/file/vector failure rollback已由production path覆盖；existing published profiles回归保持不变。activation
只把`TCP_ABI_METADATA`加入normal published profile集合，不增加第二表、runtime switch或test-only branch；之后所有
userspace验证都走同一production tuple和generic handler。

#### 6.6.5 CKPT 4A — Internal owner predicate、source与wait closure

**Purpose / Deliverable：** 在TCP creation tuple继续不可达的前提下，交付完整production owner fact/invalidation与
kernel source/wait capability。CKPT 4A至少完成：

- 在shared TCP vocabulary与Stack owner内补足role-aware point-in-time fact和Endpoint invalidation，覆盖listener
  child、connect completion/error、TX capacity、RX bytes/EOF/RST、shutdown、terminal与reclaim transition；shared
  vocabulary不包含Linux poll mask、errno、waiter、callback或kernel object；
- 让`DomainStack::protocol_transition()`与pump path在Stack guard外路由TCP invalidation，通过kernel-private weak
  observer registration命中同一boot-unique Endpoint source；registration identity不参与readiness/lifecycle决定；
- 把TCP family source接入existing `SocketPollSource`，以真实owner snapshot替换`poll_unpublished_tcp()`，并为
  connect/accept would-block提供窄`SocketWait` capability；send/receive继续复用file source与shared retry owner；
- 按第6.6.2节闭合creation/accepted-child rollback、final release withdrawal、observer unregister、multi-waiter与
  late hint cleanup；不得让wait route延长Endpoint或opened-description lifecycle；
- 保持`PUBLISHED_SOCKET_ABI_PROFILES`不含TCP，profile/resolver KUnit继续证明`AF_INET + SOCK_STREAM +
  0/IPPROTO_TCP`返回`SocketTypeNotSupported`，不运行或宣称TCP userspace proof。

**Acceptance：** owner/host tests必须逐项证明not-ready -> ready/terminal transition与invalidations不会丢失或重复驱动
behavior，覆盖empty listener/child arrival/take/cancel、connect success/refused/timeout、TX full/recovery、RX bytes/
FIN/RST、shutdown、final release、old generation与late timer。kernel KUnit必须覆盖snapshot/register race、
ready-at-register仍保留route、multi-waiter、signal/cancel route retirement、mandatory terminal hint、source retire与
late invalidation、accepted-child source failure rollback，以及poll/select/epoll source-facing projection；所有case走
production descriptor/source，不增加probe或test-only activation。

**Exit / Stop：** CKPT 4A全部acceptance、validation与独立review满足后只标记CKPT 4A Closed；Stage 4保持
In Progress，CKPT 4B仍Not Active / Not Authorized，TCP tuple、fd/runtime与四项target contract继续不可达/Pending，
并立即停止。若owner fact无法在Stack fence内表达、必须缓存ready/error/phase、observer route会强持source、
source publication无法避免lost wake、final release必须等待/失败，或需要改变current Socket/IOMUX/Epoll/opened-
description contract才能闭合，保持Review Hold并回父RFC review，不能进入CKPT 4B。

#### 6.6.6 CKPT 4B — Normal syscall activation与RV64 userspace vertical slice

**Purpose / Deliverable：** 在review接受的CKPT 4A capability上闭合Linux syscall mapping，原子发布完整R0
creation tuple，并以真实userspace持续验证candidate。CKPT 4B至少完成：

- 按第6.6.4节完成blocking/nonblocking connect、concurrent retry、typed terminal errno与`SO_ERROR`竞争；accept、
  scalar/vector/message/file send/receive、shutdown、option、address query与final-release继续走Stage 3已经形成的
  generic production adapter，不建立TCP-local syscall loop；
- 在activation前通过source/KUnit/profile audit证明R0 ordinary operation只有target外flag/option/case才稳定返回
  `EOPNOTSUPP/ENOPROTOOPT`，不能把缺失blocking、readiness、errno或cleanup伪装成unsupported；
- 只修改normal resolver admission，使`AF_INET + SOCK_STREAM + 0/IPPROTO_TCP`同时解析到同一static descriptor；
  invalid family/type/protocol/flags保持原有stable rejection，UDP、ICMP raw与Unix resolution不变；
- 增加一份repository-owned focused TCP C source，并通过既有fixture/staging分别提供RV64 glibc与musl binary；source
  是validation asset，不进入production dependency，closure记录compiler、libc、binary identity与实际运行case；
- 通过RV64 guest-local loopback覆盖creation flags、explicit/implicit bind、listen/accept/accept4、blocking与
  nonblocking connect、partial stream/EOF/shutdown、`SO_ERROR`、`SO_REUSEADDR`、`TCP_NODELAY`、SIGPIPE/
  `MSG_NOSIGNAL`、poll/select/epoll、fd rollback、dup/CLOEXEC/final close及至少一组wait/cancel或final-close交错；
  deterministic RST、capacity、generation和copy-fault corner继续由owner/host/KUnit补足，不能由userspace smoke替代。

Stage 4 userspace asset可以按case可读性拆分，但不得建立只在测试时发布的tuple、kernel-private command、host-to-guest
socket bypass或第二peer protocol。boot-root Rust `socket-test`可以补充direct syscall与existing-consumer regression，
但不能替代glibc/musl C consumer；CAgent、BusyBox `wget`或其它deployment workload即使成功，也不替代本checkpoint
focused matrix。

**Acceptance：** exact-source RV64 end-to-end run必须在同一candidate上完成两套libc focused consumer与既有UDP、UDP
extension、ICMP raw、Unix stream/seqpacket、Socket、iomux和epoll regression；KUnit/host必须补齐4A全部source race、
connect/`SO_ERROR` linearization、partial/fault/rollback、capacity recovery、final-close与late-hint matrix。RV64/LA64
release build都必须通过，证明共同source与ABI代码可编译；LA64 userspace、self/remote-external、CAgent与最终full
matrix仍诚实记录Not Run并留给Stage 5，不从build或RV64结果外推。

**Exit / Stop：** CKPT 4B只有在normal route、两套RV64 libc纵向证据、existing consumer regression、Architecture
Friction Scan与独立review全部满足后才能Closed，并同时把Stage 4标记`Syscall-Reachable Candidate / Not Cut Over`。
activation后发现target defect时，CKPT 4B保持In Progress / Review Hold并在同一production route修复；不得保留
partial profile、以test skip降级matrix或把失败推迟为accepted limitation。若只能改变R0 target/ABI/acceptance、
current shared contract或owner topology，停止并回父RFC review / Target Renegotiation，不得声明Stage 4关闭。

#### 6.6.7 Implementation ordering

Stage 4按以下顺序形成两个独立授权的closure unit：

1. 在实现前把Linux 6.6.32 role/readiness/connect oracle与RV64 C consumer case冻结为验收矩阵，尤其区分
   first-start/in-progress/already-connected、success/error completion、listener/stream/terminal poll categories、
   mandatory ERROR/HUP、requested RDHUP与signal/cancel；
2. 单独授权并完成CKPT 4A，只接入owner fact/invalidation/source/wait capability；resolver保持拒绝。完成owner/
   KUnit/build/regression、Architecture Friction Scan与独立review后关闭4A并停止；
3. 经用户单独授权CKPT 4B后，先在resolver仍拒绝时闭合剩余syscall mapping与activation preflight；只有全部R0
   operation可安全到达后，才把TCP metadata加入唯一normal published resolver；
4. activation后立即运行RV64 glibc/musl focused consumer与shared regression，在同一checkpoint修复纵向暴露的target
   defect；完成final review和closure write-back后关闭4B与Stage 4，并立即停止。

每个checkpoint内部可以形成多个普通Git commit；普通commit只服务实现顺序、review和证据，不是额外checkpoint、
授权停止点、independent closure或partial cutover。步骤不冻结具体type、method、lock、fact carrier、invalidation batch
或非strict文件路径；但4A/4B的resolver可达性、visible semantics、validation与停止边界不得在执行中静默重排。

#### 6.6.8 Validation、review与observability

每个checkpoint只记录实际执行的证据，不从另一checkpoint、architecture或harness外推。Stage 4 implementation
closure至少需要：

- CKPT 4A运行`just test net-host`覆盖TCP owner facts、invalidation、capacity、generation、progression与UDP/ICMP raw/
  frame-path regression；production/no-default target继续通过；
- repository-owned RV64 KUnit flow覆盖shared `SocketPollSource`、TCP source/observer、connect/accept/send/receive
  wait、poll/select/epoll projection、rollback/final-release与existing Socket consumer；4A必须保持resolver rejection，
  4B必须改为normal tuple success并保留invalid tuple stable rejection；
- CKPT 4B使用
  `./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/net-tcp-stage4-ckpt4b-rv64.log`
  运行最终source的glibc/musl focused TCP consumer与selected shared regression。master image只作只读输入，wrapper
  使用worktree-local副本；实际profile、compiler与binary identity写入closure evidence；
- `just fmt kernel --check`、RV64/LA64 release build与`git diff --check`；只有Kconfig/config materialization输入变化时才
  运行`just test xtask`并证明xtask没有解释semantic value；C validation source按其实际toolchain执行format/build
  check，不把预生成binary当作canonical source；
- source/behavior audit确认UDP、ICMP raw、Unix stream/seqpacket、general Socket profile/source/wait、opened-
  description final release、iomux/epoll和Stage 1 progression handoff保持原contract；4B activation只有一个normal
  resolver truth，没有test branch或第二profile；
- CKPT 4A与4B分别执行Architecture Friction Scan与独立engineering review。review至少检查第二readiness/error/
  lifecycle truth、owner penetration、private representation leakage、source/Stack lock order、guard内callback/drop、
  family-specific wait/syscall path、partial public profile、无退出条件bridge、rollback/final-release顺序、ordinary
  operation与`SO_ERROR`竞争，以及通过降低oracle/validation换取userspace通过。

production不增加驱动行为的ready/error cache、wake sequence或diagnostic state。若为race定位临时增加trace/counter，
必须明确纯诊断、不参与predicate、notification、retry或cleanup，并在checkpoint closure前删除，除非review确认长期
owner、用途和不影响behavior。

Implementation Resolution当时只运行文档检查。CKPT 4A的实际代码、review与validation证据见第6.6.10节；CKPT 4B
代码与runtime、TCP guest、CAgent、deployment probe、full network LTP、final harness、physical hardware、`smp>1`
与其它NIC/platform仍Not Run，不从Stage 3或CKPT 4A的source/build证据外推。

#### 6.6.9 Stage 4 exit、cutover与stop conditions

CKPT 4A与4B都Closed、normal resolver只有一份TCP publication truth、focused validation asset没有production
dependency、最终source没有test-only activation、ready/error cache、second wait loop、fallible final release、private
representation leakage或TCP-local common-ABI bypass，Architecture Friction Scan没有未处理的本Stage Keter/Apollyon且
final review接受时，Stage 4才可在同一closure write-back中：

1. 标记Stage 4 Closed / Syscall-Reachable Candidate / Not Cut Over，记录两个checkpoint的Git/PR、实际命令、
   compiler/binary identity、结果、proof scope与Not Run；
2. 明确`AF_INET + SOCK_STREAM + 0/IPPROTO_TCP`已经通过normal resolver可达，TCP fd、blocking与poll/select/epoll
   candidate由真实userspace持续回归，但不把该candidate写成current effective contract或final architecture closure；
3. 保持`NET-TCP-ENDPOINT-001`、`NET-TCP-STREAM-001`、`NET-TCP-LIFECYCLE-001` Introduce与
   `SOCKET-ABI-001` Refine Pending，transaction默认None，并立即停止，不解析或进入Stage 5。

任一checkpoint失败时保留已经独立安全的前一checkpoint evidence，Stage 4保持In Progress / Review Hold；4A失败时
TCP resolver必须继续拒绝，4B activation后的失败不能被写成Stage closure、contract cutover或accepted reduced target。
不得用build、KUnit、Rust-only smoke、单一libc、HTTP workload或旧日志替代本Stage要求的exact-source RV64双libc纵向
证据。

以下事实要求立即停止并回父RFC review / Target Renegotiation：需要改变R0 target/non-goal、owner/handoff、failure/
cleanup、public ABI、Contract Impact、acceptance或validation claim；需要Stack取得Linux errno/Task/File/fd/waiter，
或Socket/notification保存第二份phase/error/ready/child/capacity truth；existing Socket/IOMUX/Epoll/opened-description
contract不能自然承载TCP而必须发生shared semantic Refine；只能通过partial profile、caller/test special case、
busy-poll、周期轮询、test-only route、长期bridge、丢失mandatory terminal hint或降低Linux oracle/validation才能
发布candidate。owner-neutral、保持current consumer behavior的shared source内部窄扩展与同owner行为保持型拆分只要
满足本节边界，不构成额外resolution gate。

#### 6.6.10 CKPT 4A execution result — Closed

CKPT 4A在既定Implementation Boundary内关闭；Stage 4保持In Progress，不执行resolver、semantic或contract cutover：

- Stack TCP owner以role-aware point-in-time facts唯一表达listener child、connect phase、TX/RX、FIN/RST、shutdown、
  pending error与terminal；recheck-only invalidation不携带readiness或operation outcome。external/local pump每个有界
  reclaim round都保守失效所属接口的TCP Endpoint，因此timer已提交timeout而TX provider exhausted时也不会漏通知；
  valid bounded invalidation batch沿用R0 allocation/OOM boundary，不为消除适度重新分配引入第二buffer protocol或
  fallible final-release结果。
- kernel以boot-unique Endpoint identity注册weak reverse observer，production TCP source复用shared
  `SocketPollSource`，connect与accept分别读取自己的owner-defined predicate。operation从source guard内复制窄
  `TcpEndpointAccessPort`后释放guard再进入Stack；move-only `TcpEndpointPort`仍唯一拥有lifecycle authority。
  source retirement按withdraw/detach routes、unregister weak observer、release Endpoint、notify detached routes排序，
  creation、accepted-child rollback、multi-waiter、late invalidation与outstanding stream reservation均未建立第二truth。
- 固定Linux 6.6.32 loopback oracle覆盖idle/bound、connected、local `SHUT_RD/SHUT_WR/SHUT_RDWR`、peer FIN与RST/refused
  poll组合。RST pending error消费后仍保持`peer_receive_closed=false`；terminal结束requested READABLE/WRITABLE，
  mandatory ERROR只在pending error尚未消费时出现，HUP与requested RDHUP保持Linux分类。
- `TCP_ABI_METADATA`仍只作static descriptor metadata，`PUBLISHED_SOCKET_ABI_PROFILES`继续排除TCP；KUnit证明
  `AF_INET + SOCK_STREAM + 0/IPPROTO_TCP`仍返回`SocketTypeNotSupported`。TCP tuple、handler、fd/runtime与普通
  userspace route继续不可达，未进入CKPT 4B。
- `just test net-host`通过：TCP owner `20/20`、focused smoltcp TCP `178/178`，新增timeout +
  `TransmitOutcome::Exhausted`回归证明同轮产生invalidation及`Failed/pending/terminal` facts；UDP、ICMP raw、frame path
  与no-default production checks继续通过。最终source上的`just fmt kernel --check`与`git diff --check`通过。
- exact-source RV64 wrapper
  `./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/net-tcp-stage4-ckpt4a-rv64.log`
  正常退出并完成orderly shutdown：KUnit `465/465`，TCP source/wait/lifecycle、resolver rejection及既有Socket/UDP/
  ICMP raw/Unix regression通过，glibc与musl socket LTP合计`6/6`；verified symbol为`6653`。这不是TCP userspace
  proof。LA64 release build通过，verified symbol为`6255`；config/Kconfig输入未变化，故`just test xtask`未运行。
- 独立engineering review先报告RST/FIN分类、error-consumed terminal poll组合与TX-exhausted timer invalidation三项
  Keter；修复及回归后复审为`0 Apollyon / 0 Keter / 0 Euclid`并接受closure。Architecture Friction Scan未发现第二
  readiness/error/lifecycle truth、owner/private representation泄漏、锁序反转、guard内通知/drop、强reverse route、
  family-specific wait/syscall旁路、无退出条件bridge或rollback/final-release顺序问题。

据此CKPT 4A标记Closed，Stage 4保持In Progress。`NET-TCP-ENDPOINT-001`、`NET-TCP-STREAM-001`、
`NET-TCP-LIFECYCLE-001` Introduce与`SOCKET-ABI-001` Refine继续Pending；current Network、Socket、
Opened-description、IOMUX与Epoll contracts不变，transaction保持None。TCP guest与glibc/musl focused TCP consumer、
TCP fd/runtime、normal resolver activation、LA64 userspace、self/remote-external、CAgent、deployment probe、full network
LTP、final harness、physical hardware、`smp>1`与其它NIC/platform均Not Run。CKPT 4A授权在此耗尽；CKPT 4B保持
Not Active / Not Authorized，不进入下一checkpoint。

#### 6.6.11 CKPT 4B execution result — Closed / Syscall-Reachable Candidate / Not Cut Over

CKPT 4B在既定Implementation Boundary内关闭并完成Stage 4，不执行`NET-TCP-CUTOVER`：

- 唯一normal `PUBLISHED_SOCKET_ABI_PROFILES`现在同时发布`AF_INET + SOCK_STREAM + 0/IPPROTO_TCP`，invalid
  family/type/protocol/flag继续稳定拒绝；UDP、ICMP raw与Unix resolver保持原路径。TCP继续使用family-neutral
  syscall adapter、shared `SocketPollSource`与opened-description final release，没有test-only profile、partial
  activation、TCP-local syscall/wait loop或第二resolver truth。
- blocking/nonblocking connect在syscall-local wait round区分首次`EINPROGRESS`、重复`EALREADY`、fresh
  `EISCONN`与`ETIMEDOUT`。Stack TCP owner在唯一failed-result消费窗口同时移除closed engine并rearm retained
  binding；`SO_ERROR`或stream operation先消费pending error后，下一次connect以`ECONNABORTED`暴露terminal history，
  随后可重新发起连接，没有Socket-side phase/error mirror。
- namespace binding与selected local tuple保持分离：implicit connect只长期保留wildcard autobind reservation，live
  address query投影connection-local selected source；失败rearm不会把一次route/source selection固化为长期binding。
  owner regression证明rearm后可换用另一selected source，双libc oracle覆盖implicit/explicit wildcard bind、connected
  `getsockname()`与failure rearm。
- repository-owned source `anemone-apps/user-test/ltp/oracles/tcp-r0.c`由
  `riscv64-linux-gnu-gcc 13.3.0`和`riscv64-linux-musl-gcc 16.1.0`以
  `-static -O2 -Wall -Wextra -Werror`构建。source SHA-256为
  `47054e0cf2fe5f2500d6f19ac55c9ba62ed4cb9f9cd8bee16721dda1876d39b6`；glibc/musl binary SHA-256分别为
  `8e772d1a87b60268498bc821e2a9b933c4a655904ab7a756a916c15016ba8145`与
  `60a57b76cb4ba2c9c82e7d2efef3483241642bfa4b0b3049b8f1856f1385c4cf`。source是canonical validation
  asset，staged binary不进入production dependency。
- exact-source RV64 wrapper
  `./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/net-tcp-stage4-ckpt4b-rv64.log`
  通过并完成orderly shutdown：KUnit `466/466`，glibc与musl TCP oracle均TPASS，socket LTP合计`8/8`；UDP
  `16`、UDP extension `10`、UDP message `7`、Unix stream `23`、Unix seqpacket `4`、ICMP raw `10`与Rust
  command `2`项shared regression全部通过，verified RV64 symbol为`6592`。
- `just test net-host`通过TCP owner `20/20`、focused smoltcp TCP `178/178`及shared frame/UDP/ICMP raw
  regression；`just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G`通过并verified `6268`
  symbols；`just test xtask`为`75/75`。kernel、socket-test与user-test format check、双C `-Werror` build、
  `git diff --check`与`mdbook build docs`作为最终静态/文档检查。
- 独立review依次发现blocking caller进入既有`InProgress`、pending error先被消费后的connect/rearm，以及namespace
  binding与selected local混淆；均在同一checkpoint以production driver/owner/C oracle修正并复审。最终结果为
  `0 Apollyon / 0 Keter / 0 Euclid / 0 Safe`并接受closure。Architecture Friction Scan未发现第二readiness/error/
  lifecycle/binding truth、owner/private representation泄漏、family/test/architecture special path、无退出条件bridge、
  隐藏cleanup owner或降低oracle/validation换取通过。

据此CKPT 4B与Stage 4标记Closed / Syscall-Reachable Candidate / Not Cut Over。`NET-TCP-ENDPOINT-001`、
`NET-TCP-STREAM-001`、`NET-TCP-LIFECYCLE-001` Introduce与`SOCKET-ABI-001` Refine继续Pending；current Network、
Socket、Opened-description、IOMUX与Epoll contracts不变，transaction保持None。LA64 userspace、self/remote-external
TCP、CAgent、deployment probe、full network/final harness、physical hardware、`smp>1`与其它NIC/platform均Not Run。
Stage 5保持Outline / Not Resolved / Not Authorized；Stage 4授权在此耗尽，不进入下一Stage。

### 6.7 Stage 5 Outline — Dual-architecture与architecture-capstone closure

**目的：** 在Stage 4已经由正常resolver发布、可被真实userspace持续回归的完整candidate上，补齐父RFC仍要求的
TCP capability与architecture capstone合取验收：repository-owned focused C/libc consumer、RV64/LA64
loopback/self-external/remote-external、CAgent transport marker、shared consumer regression、source architecture
audit与final independent review；全部满足后原子执行`NET-TCP-CUTOVER`。Stage 5不再拥有首次route activation，也
不通过第二开关把同一candidate重新发布一次。

**前置依赖：** Stage 1--4全部独立Closed，Stage 1的两项Network Refine保持Effective，父RFC mandatory acceptance
assets可用，所有target内Apollyon/Keter已关闭；实际Not Run范围与条件性deployment probe边界已经可诚实记录。

**受保护边界：** Stage 4 candidate reachability不是contract cutover、accepted limitation或较弱TCP target；Stage 5
必须沿同一normal production route完成剩余证据，不能以test-only profile、第二activation switch、parallel handler或
缩小validation掩盖Stage 4暴露的问题。两组closure claim缺一不可；HTTP或条件性`wget/curl/git`成功不能替代
repository-owned TCP、双架构与architecture proof，`ss -tan`/procfs/diag失败也不能扩大target。final cutover只使
父RFC表中仍Pending的三项TCP Introduce与`SOCKET-ABI-001` Refine生效；不得重复cut over Stage 1已经Effective的
Network规则，也不得把TCP-local machinery提升为无第二consumer的generic framework，或为维持旧framework而保留
本应shared的TCP hack。

**解析触发点：** Stage 4以`Syscall-Reachable Candidate / Not Cut Over`关闭后，重新核验完整diff、current contracts、
register、Stage 4真实userspace证据、双架构环境与external peer资产，把父RFC尚未满足的mandatory matrix解析成最终
validation/cutover unit，不机械重跑已经由exact-source Stage 4证据充分覆盖且未受后续修改影响的同一轨道。任一
mandatory evidence失败、candidate存在未关闭target defect或architecture capstone出现第二truth/旁路时保持
Not Cut Over并进入Review Hold / Target Renegotiation，不得以缩小validation收口。
