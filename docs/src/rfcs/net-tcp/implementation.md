# IPv4 TCP Socket 实施计划

**状态：** R0 / Stage 1 Closed / Stage 2 In Progress / CKPT 2A Closed / CKPT 2B Not Authorized
**最后更新：** 2026-08-05
**父 RFC：** [RFC-20260805-net-tcp](./index.md)
**适用修订：** R0
**执行授权：** 用户于 2026-08-05 明确接受 R0、授权并关闭 P0，随后授权Stage 1解析并单独授权其implementation；
Stage 1已经关闭。用户同日解析Stage 2并明确Stage 2不发布syscall，随后单独授权并关闭CKPT 2A；CKPT 2B与
Stage 3--5均未授权
**当前 Gate：** None；Stage 2 In Progress，CKPT 2A Closed，CKPT 2B Not Authorized

本文保存已经Positive / Closed的TCP engine feasibility Probe Gate和Stage 1 closure。Stage 1已经建立production
owner-driven progression handoff、原子迁移UDP/ICMP raw，并把P0结论收敛为Stack-private TCP owner foundation；
它没有交付TCP Socket capability。Stage 2已经解析为两个独立授权的execution checkpoint：CKPT 2A建立可由kernel
窄消费的TCP owner capability，CKPT 2B接入syscall-unreachable的general Socket front；整个Stage 2不注册
`AF_INET + SOCK_STREAM` creation tuple、不发布handler或部分UAPI，也不执行contract cutover。Stage 3--5仍为
Outline；CKPT 2A closure不授权下一checkpoint或下一Stage。

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
| Stage 2 — TCP owner与Socket-front integration | In Progress / CKPT 2A Closed / CKPT 2B Not Authorized | 以CKPT 2A/2B先建立kernel窄capability，再接入syscall-unreachable Socket descriptor与nonblocking scalar integration | None；TCP target contracts继续Pending | CKPT 2A已关闭；等待CKPT 2B独立授权 |
| Stage 3 — Stream、ABI与lifecycle completion | Outline / Not Resolved / Not Authorized | 在仍未发布syscall route的candidate上闭合partial stream、FIN/RST/shutdown、async error/options、message/vector projection与final-release/reclaim | None；TCP target contracts继续Pending | Stage 2 Closed并取得internal integration的failure/cleanup证据 |
| Stage 4 — Blocking、readiness与concurrency hardening | Outline / Not Resolved / Not Authorized | 以各operation owner predicate接入blocking/poll/select/epoll，并关闭race、fault、signal与capacity recovery | None；TCP target contracts继续Pending | Stage 3 Closed且完整operation/lifecycle surface可供wait proof |
| Stage 5 — Dual-architecture与architecture-capstone closure | Outline / Not Resolved / Not Authorized | 完成mandatory双架构、remote-external、shared regression与架构封顶，原子执行最终cutover | `NET-TCP-CUTOVER` Pending | Stage 4 Closed、acceptance assets与独立final review可用 |

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

Implementation Resolution之后，用户明确决定Stage 2不做syscall，并单独授权CKPT 2A；没有授权CKPT 2B、contract
cutover或Stage 3解析。进入CKPT 2A前重新确认了以下live baseline：

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

### 6.5 Stage 3 Outline — Stream、ABI与lifecycle completion

**目的：** 在Stage 2同一owner topology和syscall-unreachable Socket integration上完成父RFC的stream与lifecycle语义：
partial scalar/vector/message I/O、
peek、buffer-before-EOF/error、FIN/RST/shutdown、`SO_ERROR` consume、`SO_REUSEADDR`、`TCP_NODELAY`、SIGPIPE/
`MSG_NOSIGNAL`、final release、orphan/TIME_WAIT与bounded reclaim。

**前置依赖：** Stage 2 Closed，internal active/passive integration、Socket rollback、cause mapping与基础byte
transaction已经形成可重复证据；未遗留第二connection/error truth或不可回收child/engine slot。

**受保护边界：** user copy与smoltcp storage保持object fence；每次send/receive只提交真实prefix，receive
reservation/resolve exactly once；buffered bytes先于EOF/error，RST不得伪装EOF，final release不等待worker、peer、
timer或receive operation。option/error不进入general mutable bag，Linux unsupported surface继续稳定拒绝；Stage 3
不创建family-private wait loop、不注册TCP creation tuple或提前执行contract cutover。用户控制的
length/count/iovec在allocation前完成边界校验；valid bounded internal allocation的global OOM不要求转成可恢复
Socket errno。send admission、receive consume/
window reopening、shutdown/abort/final release与listener/child cleanup必须逐项接入Stage 1的owner-driven handoff，
不得回退caller手工wake或把缺失producer留到final closure临时补齐。

**解析触发点：** Stage 2关闭后用live scalar path与fixed Linux oracle确定operation组合和failure precedence，解析
Stage 3的最小implementation slices、资源上界、race proof与focused ABI validation。若target cause、partial-progress
或final-release安全只能通过降低ABI诚实性实现，进入Target Renegotiation而不是弱化oracle。

### 6.6 Stage 4 Outline — Blocking、readiness与concurrency hardening

**目的：** 让connect、accept、send与receive/EOF分别读取各自owner-defined predicate，复用existing
snapshot/register/recheck/final-scan接入blocking、poll、select与epoll；同时关闭multi-waiter、signal/cancel、
copy fault、receive/final-close、capacity saturation/recovery及late hint/stop交错。Stage 4先在未发布candidate上完成
source/wait proof，production syscall route仍留给Stage 5最终activation。

**前置依赖：** Stage 3 Closed，全部operation outcome、stream/lifecycle fact与resource transition已经由唯一owner
表达，blocking wait不会被迫发明缺失的protocol fact。

**受保护边界：** 不建立shared `tcp_ready`、ready-mask cache、第二wait loop或event-carried outcome；notification
只提示重查，public writable只作最低admission hint，operation-specific retry重读对应owner fact。wait cancellation
不推进protocol或lifecycle，final release先withdraw source且不等待waiter；UDP、ICMP raw、Unix、iomux与epoll
existing consumer必须保持同一current contract。Stage 4关闭时TCP creation tuple与handler仍不可达，不执行partial
UAPI或contract cutover。

**解析触发点：** Stage 3关闭后逐项核验predicate producer、source publication与current iomux/epoll consumer，
再解析wait matrix、并发validation、guest coverage与停止条件。若existing wait contract不能自然承载TCP而需要
shared readiness truth或caller special case，停止并回到RFC review；优先修订自然shared owner，而不是建立
TCP-private wait/readiness hack。

### 6.7 Stage 5 Outline — Dual-architecture与architecture-capstone closure

**目的：** 原子激活完整candidate的TCP creation tuple与syscall route，并执行父RFC规定的TCP capability与
architecture capstone合取验收：repository-owned focused consumer、RV64/LA64 loopback/self-external/
remote-external、CAgent transport marker、shared consumer regression、source architecture audit与final
independent review；全部满足后原子执行`NET-TCP-CUTOVER`。

**前置依赖：** Stage 1--4全部独立Closed，Stage 1的两项Network Refine保持Effective，父RFC mandatory acceptance
assets可用，所有target内Apollyon/Keter已关闭；实际Not Run范围与条件性deployment probe边界已经可诚实记录。

**受保护边界：** route activation与最终cutover属于同一Stage 5 closure unit，不能在更早Stage或独立commit形成
长期可见partial UAPI。两组closure claim缺一不可；HTTP或条件性`wget/curl/git`成功不能替代repository-owned TCP、
双架构与architecture proof，`ss -tan`/procfs/diag失败也不能扩大target。final cutover只使父RFC表中仍Pending的
三项TCP Introduce与`SOCKET-ABI-001` Refine生效；不得重复cut over Stage 1已经Effective的Network规则，也不得把
TCP-local machinery提升为无第二consumer的generic framework，或为维持旧framework而保留本应shared的TCP hack。

**解析触发点：** Stage 4关闭后重新核验完整diff、current contracts、register、双架构环境与external peer资产，
把父RFCmandatory matrix解析成最终validation/cutover unit。任一mandatory evidence失败或architecture capstone
出现第二truth/旁路时保持Not Cut Over并进入Review Hold / Target Renegotiation，不得以缩小validation收口。
