# RFC-20260814-tcp-listener-ingress-publication

**状态：** Accepted\
**修订：** R0\
**负责人：** doruche, Codex\
**最后更新：** 2026-08-14\
**领域：** Network / IPv4 TCP / listener / sock-diag\
**影响契约：** Refine [`NET-TCP-ENDPOINT-001`](../../contracts/net/tcp-socket.md#net-tcp-endpoint-001--endpointlistener与connection-outcome由stack-tcp-owner统一拥有)、[`NET-CONTROL-PLANE-001`](../../contracts/net/control-plane.md#net-control-plane-001--initial-domain唯一决定ipv4-routesourceinterface)、[`NETLINK-SOCK-DIAG-001`](../../contracts/socket/netlink-diagnostics.md#netlink-sock-diag-001--tcp-owner形成normalized-one-window-record-set)\
**执行记录：** None；Accepted R0只固定target与Implementation Boundary，不构成实现授权，current contract尚未切换

## 摘要

当前 IPv4 TCP `listen()` 把 local binding 当作主动连接 destination 执行 route/source/interface selection，随后只在
选中的一个 interface `SocketSet` 中安装 listener engine。configured external address 的本机自连因此命中 local
path，而 external provider ingress 命中另一 engine collection；前者可成功，后者找不到 listener。wildcard listener
也被同一 single-interface 选择错误约束。

本 RFC 将主动连接 egress selection 与 listener ingress publication 分开。Stack TCP owner继续唯一拥有 binding、
logical listener、backlog、pending child和protocol lifecycle，并把一份逻辑 listener 投影到当前 initial-domain
boot-static topology 的适用 ingress path。每条 path 的 private engine只形成候选完成项；只有 Stack TCP owner在同一
Stack guard内执行的 aggregate admission，才能把它发布为可由`accept()`观察的 pending child。

## 背景

当前 [IPv4 TCP contract](../../contracts/net/tcp-socket.md) 已把 Endpoint、binding、listener、backlog、pending child与
reclaim统一交给 Stack TCP owner；[IPv4 control-plane contract](../../contracts/net/control-plane.md) 则拥有 local
address set与operation-local route/source/interface selection。live source仍由kernel TCP Socket在`listen()`前执行
destination selection，并把一个`InterfaceId`提交给 Stack；Stack listener随后以该接口定位唯一`SocketSet`，accept
facts、child handoff、rearm、diagnostics与release也都沿用这个single-interface前提。

这不是把`lo`替换成external interface即可修复的显示错误。configured external address 的local self-connect必须继续
走local path，而对应external SYN必须继续走provider path；任何单接口选择都会牺牲其中一条路径。修复同时要求多个
private projection仍共享一个logical backlog与pending-child truth，不能把accept queue或容量静态拆成per-interface
副本。

本 RFC只处理这一条listener publication/lifecycle闭包。既有owner不迁移，不建立新的跨owner异步协议，也不修改主动
连接或通用network topology模型。

## 目标

- `connect()`继续由IPv4 control plane为当前operation唯一选择route、source与single egress interface；该选择不写回
  binding，也不成为listener truth。
- `bind()`只提交或保存local binding；non-wildcard address继续由control plane的local-address set验证。
- `listen()`不再伪造destination执行egress selection。Domain Stack依据已经committed的boot-static topology，把逻辑
  listener投影到全部适用private engine collection。
- wildcard binding同时覆盖local与configured external ingress；loopback-specific只覆盖local；configured external
  specific同时覆盖local self-connect与对应external ingress。
- 所有projection共享一个Linux-normalized backlog、pending-child集合、accept predicate、readiness与listener
  diagnostics truth。
- listener publication、withdrawal、child handoff、re-listen与deferred reclaim在一个Stack TCP owner模型内闭合，
  不向kernel Socket暴露interface、engine handle、slot container或generation。

## 非目标

- IPv6、dual-stack、IPv4-mapped IPv6、`SO_BINDTODEVICE`、`SO_REUSEPORT`、transparent/free bind或runtime
  address/route reconfiguration。
- 多个configured external interface、runtime hotplug/detach/restart、network namespace、多个domain、其它NIC或
  physical hardware guarantee。
- 改变active `connect()`的route/source/interface selection、tuple、errno、timeout或stream语义。
- 新建generic multi-interface protocol framework、dynamic protocol manager、shared Endpoint hierarchy、第二pump、
  socket-to-socket copy或direct packet injection。
- Dropbear、Python server、QEMU hostfwd、测试程序或architecture专用production path。
- 顺带重构UDP、ICMP raw、provider、worker、Socket front或整个smoltcp Stack。

## Owner 与协议边界

### 状态所有权

- IPv4 control plane唯一拥有initial-domain local address set，以及active-connect的route/source/interface policy。
- Domain Stack唯一拥有boot-static interface/protocol mapping与各private`SocketSet`拓扑；它只提供listener projection
  所需的owner-local topology view，不建立第二套route或address truth。
- Stack TCP owner唯一拥有Endpoint identity、binding/port reservation、logical listener role、normalized backlog、
  aggregate admission、pending child、connection association、projection/engine lifecycle、tuple reservation与deferred
  reclaim。
- kernel TCP Socket只提交normalized listen intent/backlog，并拥有Linux ABI、blocking与opened-description integration；
  它不选择listener interface，也不轮询per-interface accept queue。
- sock-diag只消费一次request-local normalized snapshot，不能反向驱动matching、admission、readiness或cleanup。

### Listener publication

一次首次`listen()`在同一个Stack owner transaction中完成implicit binding/port reservation、适用path解析、owner
capacity admission、private engine publication与logical Listener role commit。recoverable failure必须发生在commit前：
已显式bind的Endpoint保持原binding，implicit bind失败不留下port reservation，任何路径都不能单独成功发布。

适用path是当前initial-domain boot-static topology的owner-local事实，不是route lookup。local path必然存在；存在一个
configured external deployment时，wildcard增加external projection，loopback-specific不增加，configured external
specific增加与该address对应的external projection。其它runtime或multi-external形状不属于本修订。

projection所需engine和owner storage必须有界。用户输入上界、engine/endpoint capacity与integer overflow在publication
前形成typed rejection；配置有界的普通kernel allocation继续沿用当前global OOM kernel-fatal policy，不为形式上的
allocation-free引入预分配池、镜像状态或额外owner。production Kconfig关系必须以checked arithmetic证明supported
maximum path count乘以per-path engine上界，再加必要handoff/rearm headroom后，仍落在global engine与deferred
storage上界内；其它live Endpoint/engine造成的runtime contention继续形成typed rejection。

### Aggregate backlog admission

每条适用path可以保留有界的private listener engines，使其它path没有已admit child时，任一单独path都能使用完整
logical backlog。engine数量、slot布局和projection容器属于实现选择；它们本身不是pending-child或backlog truth。

每次interface pump仍由唯一`&mut Stack`能力串行推进一个private`SocketSet`。在该次pump提交engine状态后、Stack guard
释放和invalidation投递前，Stack TCP owner执行listener reconciliation：

1. private engine进入completed transport state只形成一个尚未发布的candidate；half-open和unadmitted candidate不计入
   listener`Recv-Q`、accept readiness或logical pending count；
2. candidate只有在当前aggregate admitted加claimed child数量小于normalized backlog时，才在线性化点转为logical
   pending child；该转换是backlog credit的唯一消费点；
3. backlog已满时，本次新candidate不发布为pending child，owner对该engine执行abort/deferred reclaim并保留既有child；
   不驱逐、重排或使另一path已经admit的child失效；
4. accept claim只把一个pending child转换为claimed，take把它exactly once转换为实际命中interface上的Connection；
   cancel、fd/copy publication rollback或stale capability只处理同一个projection/slot/generation资源；
5. take在child转为Connection时、cancel在撤销logical child时分别exactly once释放occupancy；后续deferred engine/
   tuple cleanup只拥有protocol reservation，不继续占用backlog。rearm可以补充private candidate capacity，但不得另建
   credit counter或per-interface admission truth。

owner-local slot的pending/claimed phase是logical occupancy的唯一真相源。`TcpPendingChild`只是一份stale-safe
transition capability/locator；它的存在、复制或Drop不贡献occupancy，也不能在不重新校验slot phase与generation时驱动
行为。若实现为性能保留derived count，必须明确允许的staleness边界并用轻量断言校验，且该count不得绕过slot phase
决定行为。

incoming connection在backlog满时可能在private transport完成后被立即abort。这是未被logical listener接纳的过载
结果，不形成可见pending child，也不承诺“任何engine都不会先完成握手”的更强语义。若实现必须为避免该结果引入
跨projection预留、packet bypass或新的owner，应回到RFC review，而不是扩大本target。

repeated`listen(backlog)`只更新同一个logical limit。grow允许后续admission使用新增容量；shrink低于当前
pending/claimed数量时保留既有child，并拒绝新的logical admission直到occupancy回落。此时sock-diag`Recv-Q`可以暂时
高于新的`Send-Q`，但不得出现第二份backlog truth。

### Child handoff 与cleanup

opaque child capability在Stack TCP owner内部携带足以定位实际projection、slot与generation的信息。kernel Socket仍只
持`TcpPendingChild`语义能力；实际interface、engine handle和tuple只在owner的claim/take/cancel路径中解析。只有成功的
take commit才原子释放claimed occupancy，并把实际engine与tuple移交给一个独立Connection Endpoint；commit前的typed
failure保持原claimed phase与capability有效，由同一cancel/Drop路径exactly once闭合。accepted Connection此后独立使用
真实命中interface完成stream progression、shutdown与reclaim。

listener final release先撤销logical admission和observer-visible Listener role，再遍历全部private projection并remove、
abort或defer各engine。任一pending/claimed child或listener-owned deferred engine仍保留tuple时，listener binding与
Endpoint identity必须继续保留；最后一个此类engine完成reclaim后，同一TCP owner才释放listener Endpoint。已经成功take
的Connection不再保活listener Endpoint；它由自己的Endpoint与complete-tuple reservation独立参与namespace conflict，
并在自己的protocol reclaim后释放。

一次mutation可以影响零个、一个或多个interface。每个真正产生immediate或earlier-deadline work的interface分别形成一
个现有move-only`ProtocolProgression`值，kernel attach composition在Stack guard外全部消费。单个value仍只描述一个
affected interface，不携带packet、deadline、readiness、capacity或completion truth；不新增batch truth、第二worker或
caller补wake协议。

## ABI 与可见语义

Linux`bind/listen/accept/accept4/getsockname/getpeername`ABI、backlog normalization、errno、fd/copy publication
rollback、blocking/nonblocking/readiness及opened-description final release保持不变。target path matrix为：

| Binding | 必须接受的入站路径 | 约束 |
| --- | --- | --- |
| `0.0.0.0:port` | local与已配置external | 没有external deployment时退化为local-only |
| `127/8:port` | local | external ingress不得绕过local-address/path边界 |
| configured external address | local self-connect与对应external ingress | 不得二选一，也不得把self-connect发往provider补偿 |

sock-diag对一份logical listener只输出一条LISTEN record。local address/port来自committed binding，`Recv-Q`来自
aggregate admitted pending child，`Send-Q`来自normalized backlog。未使用device-binding能力的logical listener没有
唯一interface scope，其normalized diagnostic scope为unscoped，Linux adapter投影`idiag_if = 0`；SYN-RECEIVED、accepted
Connection、closing与deferred records继续报告真实interface与tuple。具体normalized scope类型不由本RFC冻结。

## Contract Impact

三项变化只在实现、验证和review整体满足后由`TCP-LISTENER-INGRESS-CUTOVER`原子生效；此前current contract保持不变。

| Contract ID | 变化 | 当前规则 | Target 摘要 | Cutover |
| --- | --- | --- | --- | --- |
| [`NET-TCP-ENDPOINT-001`](../../contracts/net/tcp-socket.md#net-tcp-endpoint-001--endpointlistener与connection-outcome由stack-tcp-owner统一拥有) | Refine | Stack TCP owner统一拥有listener resource、pending child与backlog；live listener以一个interface定位private engines | 明确一份logical listener可以拥有多个private ingress projection；completed candidate经aggregate admission后才成为pending child，backlog/readiness/accept保持单一truth | `TCP-LISTENER-INGRESS-CUTOVER` |
| [`NET-CONTROL-PLANE-001`](../../contracts/net/control-plane.md#net-control-plane-001--initial-domain唯一决定ipv4-routesourceinterface) | Refine | control plane拥有operation-local route/source/interface selection，当前TCP listen复用destination selection | egress selection只服务主动operation；listener ingress publication从Domain Stack boot-static topology形成，不执行destination route lookup | `TCP-LISTENER-INGRESS-CUTOVER` |
| [`NETLINK-SOCK-DIAG-001`](../../contracts/socket/netlink-diagnostics.md#netlink-sock-diag-001--tcp-owner形成normalized-one-window-record-set) | Refine | TCP record携带可映射的interface，listener queue来自owner snapshot | logical listener只输出一条aggregate record；无唯一interface scope时`idiag_if = 0`，child/connection/deferred record保持真实interface | `TCP-LISTENER-INGRESS-CUTOVER` |

### Dependencies

- [`NET-STACK-PUMP-001`](../../contracts/net/frame-path.md#net-stack-pump-001--stack-instance唯一推进protocol-state)：
  Stack仍是唯一推进owner；本target只要求为每个affected interface交付一个既有single-interface obligation。
- [`NET-PROTOCOL-BOUNDARY-001`](../../contracts/net/protocol-socket.md#net-protocol-boundary-001--cross-owner-protocol-capability保持窄且非阻塞)：
  child与diagnostic scope继续使用窄normalized value，private engine representation不越过object fence。
- [`NET-TCP-LIFECYCLE-001`](../../contracts/net/tcp-socket.md#net-tcp-lifecycle-001--socket-publication与protocol-reclaim分离)：
  opened-description final release仍只触发一次owner handoff，binding与tuple保持到最后protocol reclaim。
- [`NET-TCP-STREAM-001`](../../contracts/net/tcp-socket.md#net-tcp-stream-001--字节流committerminal-precedence与readiness读取owner-fact)：
  accepted Connection继续读取实际interface上的stream owner fact，不改变FIN/RST/shutdown语义。
- [`NET-SOCKET-WAIT-001`](../../contracts/net/protocol-socket.md#net-socket-wait-001--protocol-factwake与linux-readiness保持分离)：
  listener invalidation仍只提示重算，readiness只来自aggregate owner fact。

## Implementation Boundary

**允许改变：** Stack TCP listener/projection/admission/child/reclaim/facts/diagnostics内部形状；Domain Stack读取
boot-static topology的窄owner API；kernel TCP listen移除egress selection后的local-address validation wiring；opaque
child与normalized diagnostic scope value；多interface progression carrier；owner-local Kconfig capacity、定向host/KUnit/
runtime tests；同owner、行为保持的模块拆分。

**必须保持：** 本RFC的owner分配、aggregate admission线性化、path matrix、all-or-nothing publication、opaque child
handoff、withdraw-before-cleanup与单一cutover；Linux Socket ABI与active connect；现有UDP/ICMP raw、provider、worker、
opened-description、iomux/epoll和runtime topology contract；private`SocketSet`、handle、slot、generation、ifindex mapping
不得越过object fence。

具体slot、phase、projection、plan或progression collection的类型名、容器、模块布局、engine数量算法与测试case数量不由
本RFC冻结。实现应优先复用当前唯一Stack guard和existing owner lifecycle，不为本地需求引入generic protocol framework。

本R0默认使用一个closure checkpoint，并只在最终target、validation与review整体满足时执行唯一
`TCP-LISTENER-INGRESS-CUTOVER`。实现可以包含多个普通commit或review slice；同owner、行为保持的single-projection
reshape不形成独立checkpoint，也不得借此发布dormant multi-projection path、partial visible semantics或transitional
contract。当前不建立`implementation.md`或transaction。

**停止条件：** 若实现需要移动binding/listener/backlog/child owner，建立per-interface independent queue/credit或第二
truth；需要跨owner异步ack、新worker、长期持有多个owner lock、callback/复杂Drop、transitional dual path或high-risk
probe；无法在单一Stack owner window内闭合publication/admission/withdrawal；必须改变Socket ABI、active connect、runtime
topology、public visibility、path matrix、backlog guarantee、acceptance或validation claim；需要多个独立cutover、target
renegotiation或发现本轮无法关闭的Apollyon/Keter，则停止并回到RFC review，不得以静态分池、silent partial coverage、
caller补wake、diagnostic特判或降低oracle绕过。

## Acceptance 与 Validation

RFC接受只固定target与Implementation Boundary，不构成实现授权。最终closure与cutover至少需要：

- bounded source review确认TCP listen不再执行或伪装egress selection；active connect仍由control plane唯一选择；binding、
  backlog、pending/claimed phase、readiness、diagnostics与reclaim没有per-interface mirror或diagnostic反向依赖；
- deterministic owner host proof覆盖wildcard、loopback-specific与external-specific path matrix，无external deployment，
  local-only/external-only完整backlog，mixed arrival aggregate admission，0/1/普通/backlog上界，re-listen grow/shrink，
  full-backlog candidate rejection，claim/take/cancel/stale generation与全部projection cleanup；
- failure proof覆盖implicit/explicit bind与engine/endpoint capacity，证明每种publication前typed rejection都不留下private
  projection或implicit reservation，并由source review确认第一条engine publication之后不存在recoverable failure；同时
  覆盖listener close面对idle、half-open、pending、claimed与deferred engine，以及binding只在最后reservation消失后复用；
- capacity/config proof以checked arithmetic确认supported maximum path count、per-path engine上界、handoff/rearm headroom
  与deferred storage闭合且不溢出；其它live Endpoint/engine造成的global contention继续由typed rejection覆盖；
- diagnostics proof确认一条logical LISTEN record、aggregate queue、unscoped`idiag_if = 0`，并保持child/connection/
  deferred record的真实interface；
- repository wrapper完成相关host/no-default检查、RV64与LA64 release build，owner-local KUnit只覆盖host proof无法直接
  触达的真实kernel wiring，不创建validation-only production facade或live-scheduling test协议；
- RV64与LA64 focused runtime分别证明wildcard和external-specific listener同时完成guest local self-connect与hostfwd
  external ingress，loopback-specific hostfwd负向case不误命中，raw sock-diag/`ss -tan`输出正确，并回归至少一个
  remote-external active connect及stream/FIN/RST consumer；
- Architecture Friction Scan确认没有第二份listener/backlog/interface truth、owner穿透、private representation泄漏、
  caller/hostfwd/architecture特判、无退出条件bridge或遗漏progression obligation。

full network LTP、完整preliminary/final harness、physical hardware、`smp > 1`、其它NIC/provider/deployment、runtime
hotplug、多external interface、IPv6及压力/长时backlog测试默认Not Run；除非实际执行，不得由build、host proof或
single-NIC QEMU外推。LA64若仍在完整shutdown顺序后因缺少power-off handler停在halt，必须记录人工终止与不能证明
wrapper exit 0的边界。

## 风险与反馈

- 为保证任一单独path使用完整backlog，private engine上界可能随适用path数量增长。当前target只有local加至多一个
  external path；若自然的Kconfig上界仍造成不可接受资源成本，应带实测容量证据进入Target Renegotiation，不得静态
  分割logical backlog。
- full-backlog candidate在private transport完成后被abort的peer-visible结果必须通过deterministic host proof核验；若
  smoltcp实际行为无法形成有界、可诊断且不损害已有child的过载结果，应停止review，而不是增加packet bypass。
- 若实现证据表明必须引入独立probe、不安全中间态、正式多阶段或多个cutover，应在进入该路线前按停止条件回到RFC
  review，再按真实需要建立`implementation.md`；普通commit顺序不构成增加stage的理由。

## 文档与证据

- 当前baseline：[IPv4 TCP Socket](../../contracts/net/tcp-socket.md)、[IPv4 control plane](../../contracts/net/control-plane.md)、
  [Stack progression](../../contracts/net/frame-path.md)、[Read-only Netlink Diagnostics](../../contracts/socket/netlink-diagnostics.md)。
- 历史来源：[IPv4 TCP Socket RFC](../net-tcp/index.md)、[Read-only Network Diagnostics RFC](../read-only-network-diagnostics/index.md)。
- commit / PR / optional transaction：None。
- 外部源码证据：None；本RFC的target由Anemone live owner与current contract决定。

## 修订记录

| 修订 | 日期 | 语义变化 | Review / Evidence |
| --- | --- | --- | --- |
| R0 | 2026-08-14 | 接受boot-static local/external listener projection、aggregate backlog admission、slot phase唯一occupancy truth、独立Connection handoff与单一closure checkpoint；三项contract delta保持pending直到最终原子cutover。 | 维护者决定；RFC review；文档验证 |
