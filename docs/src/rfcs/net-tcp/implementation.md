# IPv4 TCP Socket 实施计划

**状态：** Draft / Not Active
**最后更新：** 2026-08-05
**父 RFC：** [RFC-20260805-net-tcp](./index.md)
**适用修订：** Draft
**执行授权：** None
**当前 Gate：** Probe Gate P0；尚未执行

本文只把父RFC在进入任何TCP production stage前必须关闭的TCP engine feasibility Probe Gate解析到可执行
语义；P0全部限制在kernel外的vendored/Stack crate，不修改`anemone-kernel`、不迁移production consumer、
不改变current contract，也不交付TCP Socket capability。Stage 1--5在本文保留Outline，只固定目的、依赖、
受保护边界与解析触发点，不给出可执行步骤。P0无论positive、negative或inconclusive都形成一个授权停止点；
后续工作必须重新review与授权。

P0只有在父RFC target与Contract Impact完成R0接受、live baseline重新核验且用户明确授权本gate后才能从
Not Active转为Active。R0接受不自动授权执行，P0 closure也不授权任何后续gate。

## 1. Live baseline

当前source提供了足以启动crate-only probe、但不足以宣称target可实现的事实边界：

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
- P0标记Positive / Closed后立即停止。Stage 1保持Outline；是否解析并实施Stage 1需要独立review与用户授权。

### 4.2 Negative or inconclusive

P0 hypothesis失败、证据不足或必须越过crate-only protected boundary时：

- P0整体记录Negative / Closed或Inconclusive / Review Hold，保存最小失败证据与已完成slice；
- 删除probe-only code；review先区分probe route/实现缺陷、环境证据不足与真实target infeasibility。保持target的
  route correction可以重新解析P0；只有证据要求改变target、owner、contract、acceptance或validation claim时才
  进入Target Renegotiation，agent不能自行批准reduced target；
- Stage 1保持Outline / Not Resolved / Not Authorized，kernel source、current contracts、
  `NET-PROTOCOL-PROGRESSION-CUTOVER`与`NET-TCP-CUTOVER`全部保持不变。

### 4.3 Evidence placement

P0默认由本页保存长期结论、由Git/PR保存diff与validation evidence，不预建transaction。只有执行跨度真实需要
独立长期时间线时才另行review是否建立transaction；它不是P0 activation或closure前置。

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

- **P0 Draft / Not Active：** hypothesis、protected boundary、validation、failure signal、write-back与exit已经
  解析，但尚无执行授权；
- **Outline：** 只固定目的、前置依赖、受保护边界与解析触发点，不冻结checkpoint、具体步骤、类型、算法、
  文件、命令或transaction；
- future Stage只有在前一gate独立Closed、live source/current contracts/register重新核验，并完成单独的
  Implementation Resolution与用户授权后才能进入Active；
- 前一gate关闭不自动解析、授权或启动后一Stage。任何future Outline若需改变target、owner、handoff、
  failure/cleanup、ABI、Contract Impact、acceptance或validation claim，先回到父RFC review。

shared Socket framework反馈按第5.1节同样先回到父RFC review；不得把需要review误当成必须留在TCP owner内实现。

Stage名称、数量与相邻职责可以在保持父RFC target、Stage 1 Network contract cutover和最终合取closure不变时
通过本文内的Route Correction调整；一旦某Stage完成独立解析，其交付与受保护边界不能在执行中静默重排。

### 6.2 路线图

| Gate / Stage | 当前成熟度 | 概括目的 | Contract 状态 | 解析触发点 |
| --- | --- | --- | --- | --- |
| P0 — TCP engine feasibility probe | Draft / Not Active | 在kernel外crate验证async cause与bounded listener composition路线 | None；current contracts不变 | R0接受、baseline复核与P0独立授权 |
| Stage 1 — Stack TCP owner与protocol progression foundation | Outline / Not Resolved / Not Authorized | 建立production owner-driven handoff、原子迁移UDP/ICMP raw，并把P0证据收敛为Stack TCP owner foundation | positive时原子执行`NET-PROTOCOL-PROGRESSION-CUTOVER`；TCP target contracts继续Pending | P0 Positive / Closed并完成probe代码处置 |
| Stage 2 — Nonblocking TCP vertical slice | Outline / Not Resolved / Not Authorized | 经既有Socket front接通create/bind/connect/listen/accept与最小nonblocking scalar stream纵切 | None；TCP target contracts继续Pending | Stage 1 Closed并证明Stack owner surface可由kernel窄消费 |
| Stage 3 — Stream、ABI与lifecycle completion | Outline / Not Resolved / Not Authorized | 闭合partial stream、FIN/RST/shutdown、async error/options、message/vector projection与final-release/reclaim | None；TCP target contracts继续Pending | Stage 2 Closed并取得纵切failure/cleanup证据 |
| Stage 4 — Blocking、readiness与concurrency hardening | Outline / Not Resolved / Not Authorized | 以各operation owner predicate接入blocking/poll/select/epoll，并关闭race、fault、signal与capacity recovery | None；TCP target contracts继续Pending | Stage 3 Closed且完整operation/lifecycle surface可供wait proof |
| Stage 5 — Dual-architecture与architecture-capstone closure | Outline / Not Resolved / Not Authorized | 完成mandatory双架构、remote-external、shared regression与架构封顶，原子执行最终cutover | `NET-TCP-CUTOVER` Pending | Stage 4 Closed、acceptance assets与独立final review可用 |

### 6.3 Stage 1 Outline — Stack TCP owner与protocol progression foundation

**目的：** 第一次进入kernel时直接形成最终production owner形状：让UDP、ICMP raw与TCP各自的Stack-side
protocol owner判断pump外committed effect并向既有worker可靠移交stateless progression obligation，原子删除
UDP/ICMP raw caller/control-plane手工wake路径；同时把P0证明过的cause与bounded listener/resource路线收敛为
production Stack TCP owner，使其唯一拥有Endpoint identity、binding/role、engine composition、pending child、
connect/terminal outcome、timer与deferred reclaim。Stage 1不发布kernel TCP Socket UAPI。

**前置依赖：** P0 Positive / Closed，probe-only代码已按第4节删除或确认成为no-default production crate的最小
substrate；父RFC R0、current Network contracts、live `DomainStack`/control-plane/worker source与register未漂移。

**受保护边界：** Stack TCP owner不得泄漏smoltcp handle、ring borrow或Linux errno；listener/pending-child/
engine/cause只能有一份truth，全部资源有界且owner capacity exhaustion可恢复；global allocator OOM按第5.2节
允许panic。三类protocol effect decision保持各自owner-local，共同composition只处理affected progression domain
与stateless request；Stack state/deadline、worker admission/coalescing/stop与control-plane selection各只有一个owner。
commit先对后续pump可见，request在operation success/release completion前完成handoff；park、in-flight pump、
earlier deadline、coalesced producer与late stop不能丢失或恢复admission。UDP/ICMP raw迁移必须保持selection、
transaction、failure、readiness、retire、local/external与双架构visible semantics，且不保留旧/新双路径或fallback。
Stage 1不得建立Socket、fd、wait/readiness、第二registry、新worker、shared effect policy或长期validation facade。

**Contract与证明边界：** Stage 1只在production handoff、UDP/ICMP raw迁移、Stack TCP foundation、owner/race
proof、host/build验证和RV64/LA64 focused UDP/ICMP raw local/external回归全部成立后，以
`NET-PROTOCOL-PROGRESSION-CUTOVER`原子Refine current `NET-CONTROL-PLANE-001`与`NET-STACK-PUMP-001`。
该cutover只记录当前UDP/ICMP raw真实consumer与shared production substrate；TCP target contracts继续Pending，
Stage 1只证明当前foundation mutation能够产生obligation，不提前声称尚未实现的send、receive-window reopening、
shutdown或final-release producer已经通过。任一迁移、验证、review或文档项失败时两项Network ID都保持旧规则。

**解析触发点：** P0独立关闭后重新读取retained crate code与live Stack/control-plane/worker owner，解析Stage 1的
单一production交付、implementation ordering、validation、cutover、退出条件和必要checkpoint。普通commit不形成
新gate；handoff不得先以kernel probe、旧/新双路径或临时caller adapter落地。若自然实现需要新的worker、shared
effect truth、第二route/deadline truth、public API扩张，或需要改变父RFC target/owner/Contract Impact，停止并进入
RFC review / Target Renegotiation。

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
