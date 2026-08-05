# IPv4 TCP Socket 实施计划

**状态：** R0 / Stage 1 Ready / Not Authorized
**最后更新：** 2026-08-05
**父 RFC：** [RFC-20260805-net-tcp](./index.md)
**适用修订：** R0
**执行授权：** 用户于 2026-08-05 明确接受 R0、授权并关闭 P0，随后只授权 Stage 1 解析与文档更新；
Stage 1 implementation与contract cutover均未授权
**当前 Gate：** Stage 1；Resolved / Ready / Not Authorized

本文保存已经Positive / Closed的TCP engine feasibility Probe Gate，并把Stage 1解析为一个完整execution gate。
Stage 1建立production owner-driven progression handoff、原子迁移UDP/ICMP raw，并把P0结论收敛为
Stack-private TCP owner foundation；它不交付TCP Socket capability。Stage 2--5仍为Outline，只固定目的、依赖、
受保护边界与解析触发点。Stage 1解析不等于implementation授权，也不更新current contract；执行仍需用户单独授权。

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
- **Ready / Not Authorized：** Implementation Boundary、唯一execution gate、ordering、validation、cutover、exit与
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
| Stage 1 — Stack TCP owner与protocol progression foundation | Ready / Not Authorized | 建立production owner-driven handoff、原子迁移UDP/ICMP raw，并把P0证据收敛为Stack TCP owner foundation | closure时原子执行`NET-PROTOCOL-PROGRESSION-CUTOVER`；TCP target contracts继续Pending | 已解析；等待独立implementation授权 |
| Stage 2 — Nonblocking TCP vertical slice | Outline / Not Resolved / Not Authorized | 经既有Socket front接通create/bind/connect/listen/accept与最小nonblocking scalar stream纵切 | None；TCP target contracts继续Pending | Stage 1 Closed并证明Stack owner surface可由kernel窄消费 |
| Stage 3 — Stream、ABI与lifecycle completion | Outline / Not Resolved / Not Authorized | 闭合partial stream、FIN/RST/shutdown、async error/options、message/vector projection与final-release/reclaim | None；TCP target contracts继续Pending | Stage 2 Closed并取得纵切failure/cleanup证据 |
| Stage 4 — Blocking、readiness与concurrency hardening | Outline / Not Resolved / Not Authorized | 以各operation owner predicate接入blocking/poll/select/epoll，并关闭race、fault、signal与capacity recovery | None；TCP target contracts继续Pending | Stage 3 Closed且完整operation/lifecycle surface可供wait proof |
| Stage 5 — Dual-architecture与architecture-capstone closure | Outline / Not Resolved / Not Authorized | 完成mandatory双架构、remote-external、shared regression与架构封顶，原子执行最终cutover | `NET-TCP-CUTOVER` Pending | Stage 4 Closed、acceptance assets与独立final review可用 |

### 6.3 Stage 1 Resolved Gate — Stack TCP owner与protocol progression foundation

#### 6.3.1 成熟度、授权与前置基线

Stage 1是一个formal execution gate，只在exit执行一次`NET-PROTOCOL-PROGRESSION-CUTOVER`。实现可以形成若干普通
Git commit，但这些commit不是额外checkpoint、semantic gate或partial cutover；本Stage不需要transaction，默认
evidence placement保持Git/PR加本页closure write-back。2026-08-05的授权只覆盖本次解析和文档更新，Stage 1仍为
Ready / Not Authorized。

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

### 6.4 Stage 2 Outline — Nonblocking TCP vertical slice

**目的：** 通过既有general Socket front、control plane与opened-description路径，建立第一条端到端nonblocking
IPv4 TCP纵切：创建与地址、active connect、listen/accept、基础completion/error observation及最小scalar
send/receive共同命中Stage 1的唯一TCP owner。它不以blocking或iomux旁路补齐缺口。

**前置依赖：** Stage 1 Closed，`NET-PROTOCOL-PROGRESSION-CUTOVER`已经Effective，Stack TCP owner的identity、
listener/child handoff、connect outcome、stream admission与release capability已经稳定到可被kernel窄消费；
Stage 1 review没有遗留会改变Socket owner surface的Apollyon/Keter。

**受保护边界：** Linux tuple/sockaddr/copy/errno与fd publication留在Socket ABI/opened-description owner；route/
source/interface selection仍只归control plane；Stack不接收Task/File/fd/user pointer，Socket不缓存binding、role、
connection、pending child、buffer或error truth。fd/copy publication失败必须由当前handoff owner清理，nonblocking
路径不得形成第二connect或listener状态机；TCP target contracts仍Pending，不执行partial cutover。

**解析触发点：** Stage 1 closure后根据live Socket front与真实Stack capability，选择能够同时证明active/passive
connection及最小byte path的最小vertical slice，并解析其ABI floor、rollback、validation与停止条件。若只能发布
不诚实的partial UAPI或自然方案需要调整general Socket owner/public surface，按第5.1节把它作为framework feedback
回到RFC review；不得用TCP-local wrapper绕过。

### 6.5 Stage 3 Outline — Stream、ABI与lifecycle completion

**目的：** 在Stage 2同一owner topology上完成父RFC的stream与lifecycle语义：partial scalar/vector/message I/O、
peek、buffer-before-EOF/error、FIN/RST/shutdown、`SO_ERROR` consume、`SO_REUSEADDR`、`TCP_NODELAY`、SIGPIPE/
`MSG_NOSIGNAL`、final release、orphan/TIME_WAIT与bounded reclaim。

**前置依赖：** Stage 2 Closed，nonblocking active/passive纵切、fd rollback、cause mapping与基础byte transaction已经
形成可重复证据；未遗留第二connection/error truth或不可回收child/engine slot。

**受保护边界：** user copy与smoltcp storage保持object fence；每次send/receive只提交真实prefix，receive
reservation/resolve exactly once；buffered bytes先于EOF/error，RST不得伪装EOF，final release不等待worker、peer、
timer或receive operation。option/error不进入general mutable bag，Linux unsupported surface继续稳定拒绝；Stage 3
不创建family-private wait loop或提前执行contract cutover。用户控制的length/count/iovec在allocation前完成边界
校验；valid bounded internal allocation的global OOM不要求转成可恢复Socket errno。send admission、receive consume/
window reopening、shutdown/abort/final release与listener/child cleanup必须逐项接入Stage 1的owner-driven handoff，
不得回退caller手工wake或把缺失producer留到final closure临时补齐。

**解析触发点：** Stage 2关闭后用live scalar path与fixed Linux oracle确定operation组合和failure precedence，解析
Stage 3的最小implementation slices、资源上界、race proof与focused ABI validation。若target cause、partial-progress
或final-release安全只能通过降低ABI诚实性实现，进入Target Renegotiation而不是弱化oracle。

### 6.6 Stage 4 Outline — Blocking、readiness与concurrency hardening

**目的：** 让connect、accept、send与receive/EOF分别读取各自owner-defined predicate，复用existing
snapshot/register/recheck/final-scan接入blocking、poll、select与epoll；同时关闭multi-waiter、signal/cancel、
copy fault、receive/final-close、capacity saturation/recovery及late hint/stop交错。

**前置依赖：** Stage 3 Closed，全部operation outcome、stream/lifecycle fact与resource transition已经由唯一owner
表达，blocking wait不会被迫发明缺失的protocol fact。

**受保护边界：** 不建立shared `tcp_ready`、ready-mask cache、第二wait loop或event-carried outcome；notification
只提示重查，public writable只作最低admission hint，operation-specific retry重读对应owner fact。wait cancellation
不推进protocol或lifecycle，final release先withdraw source且不等待waiter；UDP、ICMP raw、Unix、iomux与epoll
existing consumer必须保持同一current contract。

**解析触发点：** Stage 3关闭后逐项核验predicate producer、source publication与current iomux/epoll consumer，
再解析wait matrix、并发validation、guest coverage与停止条件。若existing wait contract不能自然承载TCP而需要
shared readiness truth或caller special case，停止并回到RFC review；优先修订自然shared owner，而不是建立
TCP-private wait/readiness hack。

### 6.7 Stage 5 Outline — Dual-architecture与architecture-capstone closure

**目的：** 对完整candidate执行父RFC规定的TCP capability与architecture capstone合取验收：repository-owned
focused consumer、RV64/LA64 loopback/self-external/remote-external、CAgent transport marker、shared consumer
regression、source architecture audit与final independent review；全部满足后原子执行`NET-TCP-CUTOVER`。

**前置依赖：** Stage 1--4全部独立Closed，Stage 1的两项Network Refine保持Effective，父RFC mandatory acceptance
assets可用，所有target内Apollyon/Keter已关闭；实际Not Run范围与条件性deployment probe边界已经可诚实记录。

**受保护边界：** 两组closure claim缺一不可；HTTP或条件性`wget/curl/git`成功不能替代repository-owned TCP、
双架构与architecture proof，`ss -tan`/procfs/diag失败也不能扩大target。final cutover只使父RFC表中仍Pending的
三项TCP Introduce与`SOCKET-ABI-001` Refine生效；不得重复cut over Stage 1已经Effective的Network规则，也不得把
TCP-local machinery提升为无第二consumer的generic framework，或为维持旧framework而保留本应shared的TCP hack。

**解析触发点：** Stage 4关闭后重新核验完整diff、current contracts、register、双架构环境与external peer资产，
把父RFCmandatory matrix解析成最终validation/cutover unit。任一mandatory evidence失败或architecture capstone
出现第二truth/旁路时保持Not Cut Over并进入Review Hold / Target Renegotiation，不得以缩小validation收口。
