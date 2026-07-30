# net-udp 迁移实施计划

**状态：** R0 / Stage 0-3 Closed / Stage 4 Ready / Not Active / Stage 5 Outline
**最后更新：** 2026-07-30
**父 RFC：** [RFC-20260729-net-udp](./index.md)
**目标与不变量：** [net-udp 目标与不变量](./invariants.md)
**当前契约：** [Network current contracts](../../contracts/net/index.md)、
[Opened-description lifecycle](../../contracts/task/opened-description-lifecycle.md)、
[Poll wait / source registration](../../contracts/iomux/poll-wait.md)、
[Epoll protocol](../../contracts/epoll/protocol.md)、
[System Target](../../contracts/configuration/system-target.md)
**当前修订：** R0
**事务日志：** [2026-07-29 net-udp](../../devlog/transactions/2026-07-29-net-udp.md)

> 本文是R0的canonical实施顺序、stage maturity、probe、验证和resolved write set。R0已由public review接受，
> transaction已经建立；Checkpoint 0B关闭后的工程审查确认两个Keter和一个Euclid，0B Feedback Correction已
> 修复并通过独立复审。0C按positive route完成decision closure并关闭Stage 0；独立的`0 -> 1`resolution与后续
> Stage 1 Checkpoint 1A均已于2026-07-29完成。`NET-UDP-DOMAIN-CUTOVER`已生效；独立的`1 -> 2`
> resolution、Checkpoint 2A split与Checkpoint 2B control/local cutover也已完成，Stage 2 Closed。独立的
> `2 -> 3`resolution已把Stage 3完整解析为Ready；Checkpoint 3A随后完成same-owner module split并独立关闭。
> Checkpoint 3B已形成Endpoint/File/address lifecycle实现；获批VFS/shared-surface correction与capacity Route
> Correction完成后，RV64 correction-source证据经behavior-preserving lazy-Vec改动复用，独立final review关闭本checkpoint。
> Checkpoint 3C随后完成nonblocking datagram纵切、correctness repair、final validation与review，Stage 3已Closed；
> 独立`3 -> 4`resolution现已把Stage 4解析为4A socket source、4B blocking syscall与4C race/evidence
> closure三个checkpoint。Stage 4为Ready / Not Active，Stage 5保持Outline；本次没有激活4A或执行current-contract
> cutover。

## 1. 计划角色与 authority

本计划把[R0 accepted target](./index.md)和[目标与不变量](./invariants.md)转化为可滚动解析的
实施路径。它不得重新选择以下 target：initial domain 内唯一 global protocol `Stack`、domain-local logical
interface owner、Stack-owned Endpoint/binding truth、control-plane-owned route/source/interface policy、kernel
Socket-owned Linux ABI/readiness/error，以及五项 R0 用户可见决定。

Stage 0-3已经独立关闭。Stage 4已经由独立`3 -> 4`resolution完整解析为Ready / Not Active；本阶段的checkpoint、
验证和Resolved Write Set Manifest以6.4为唯一权威。Stage 5仍是future Outline，其中列出的目录、模块和contract
gate只是后续resolution输入，不是write permission，也不是concrete object graph。Stage N必须先按自己的验证和退出
条件独立Closed，之后才能运行只读的`N -> N+1 Implementation Resolution Gate`。解析完成只让下一阶段达到Ready，
不自动进入Active。

进入实现前必须：

1. R0由独立public review接受；Draft promotion本身不构成acceptance；
2. 建立独立transaction，并重新读取当时的live source、current contracts、register、branch/HEAD与dirty state；
3. 确认Stage 4的路线、命令和Resolved Write Set Manifest未漂移；如有漂移，先在本文重新解析并review；
4. transaction只记录activation preflight、批准事实和本文链接，不复制Ready定义或manifest；Stage 4仍须
   获得独立启动授权。

## 2. Live baseline 与首阶段选择

2026-07-29 的 source audit 得到以下 baseline：

- `anemone-kernel/src/net/worker.rs` 的每次 external netdev attach 都执行 `Stack::new()`；每个 worker 的
  `PumpCore` 独占一份 `Stack + provider + InterfaceId`。当前 production 因此仍是 per-netdev Stack wiring。
- `anemone-smoltcp-stack::Stack` 虽能保存多个 `InterfaceEntry`，但每个 entry 独立保存 `SocketSet`，`pump()`
  仍按一个 `InterfaceId + FrameProvider` 推进。该形状尚未形成 domain-wide Endpoint namespace。
- `FrameDevice` 固定报告 `smoltcp::phy::Medium::Ethernet`。当前 shared frame contract 没有 production
  IP-medium software-link capability，不能把 `lo` 伪装成 Ethernet provider。
- vendored smoltcp UDP egress 会先从 TX buffer dequeue，再用当前 `Interface` context 选择 source address；
  找不到 source 时记录 trace 并把 datagram 当作已处理而丢弃。把同一 `SocketSet` 依次交给多个 interface
  poll，既不能保证目标 interface 先观察 datagram，也不能满足本 RFC 的 send-success boundary。
- kernel 尚无本 RFC 五项 network syscall handler；generic syscall table 对未注册入口返回 `ENOSYS`。
- `ProcFile` publication lifecycle、静态 single `final_release` hook、`File::prv`、`PollRequest` 以及
  iomux/epoll snapshot-register-recheck protocol 已存在，应作为后续 socket consumer 的窄基线，不另建第二套
  fd、final-close 或 wait truth。
- current `SystemTarget` schema 只包含 Platform、root 与 initial-program selection，没有 network section；
  build resolver把它保存在 resolved selection 中，但尚未 materialize network typed input。
- register 中与本路径直接相关的 frame-path conformance issue 已关闭；没有可替代本 RFC target 或允许绕过
  current contracts 的 active network limitation。

首个高风险问题不是 syscall 参数布局，而是：在当前 smoltcp interface/socket egress 形状下，能否建立一个
Stack-level Endpoint owner，使同一个 endpoint namespace 安全跨多个 external interface 与 production-shaped
local software link推进，并在 admission 前固定 route/source/interface selection，而不复制 Endpoint truth或让
错误 interface dequeue datagram。

因此 Stage 0 只验证这一条架构主链。它不触碰 kernel、UAPI、build configuration、current contract 或 public
shared API；成功结果为 Stage 1 提供最窄可用内部 seam，失败结果精确说明下一阶段是否需要 vendored smoltcp
扩展、不同 engine-resource mapping，或 target review。先写 syscall/file object 只会把未决 topology 埋进 ABI
lifecycle，本计划不采用该顺序。

## 3. 实施原则

### 3.1 纵向结果，而不是按 crate 分层施工

后续每个语义阶段必须交付可验证的 cross-owner result。单独“完成 API types”“完成 smoltcp socket wrapper”或
“注册 syscall number”都不构成 stage closure。Stage 0 是例外意义上的 probe stage：它本身必须形成一个可判定
的 topology 结论，但不形成用户能力或 contract cutover。

### 3.2 Host-first，但 host 不替代 production

Stage 0 使用 deterministic host provider 和 software link 精确控制 interface order、capacity、recheck、source
selection 与 packet ownership。它只证明 protocol-owner 内部路线，不证明 kernel lock/wake、真实 fd/syscall、
VirtIO、QEMU、LA64 或 external deployment。后续 stage 必须分别提供 kernel/runtime 证据；Stage 0 PASS 不得
外推为 functional UDP。

### 3.3 Control plane 决定 selection，Stack 执行 admission

Stage 0 可以使用 test-owned immutable selection input，但不得让 Stack 根据 private interface iteration 建立
第二份 route/source policy。operation 进入 Stack 前必须已经携带选中的 logical egress identity 与 source address；
Stack只验证对应 private interface mapping仍有效、完成 Endpoint capacity/admission，并保证非目标 interface不会
consume该datagram。一次选择结果不得写回 wildcard/implicit binding。

### 3.4 Probe 不建立 shared API 或第二 owner

Stage 0新增的ordinary UDP owner type和operation method保持`anemone-smoltcp-stack`私有。独立integration test若
无法调用crate-private fixture，可以保留最窄的`#[cfg(feature = "host-test")] pub` validation facade；它只传递
protocol-domain input/outcome和test observation，不暴露smoltcp handle、buffer/queue identity或production
authority，并且不得进入kernel的`default-features = false`dependency。Stage 0不得修改`anemone-net-api`、复制
一份per-interface Linux Endpoint，或建立future TCP trait hierarchy。只有后续真实kernel consumer与stack
implementation共同证明某个value/capability必须跨crate后，对应Ready stage才可解析shared surface及其具体Rust编码。

### 3.5 Bounded resource 与 recheck 从第一条路径开始

external provider credit、Endpoint TX/RX storage和local software-link handoff都必须有有限capacity。normal full/
empty/exhausted返回可重查outcome，不得panic、busy-spin或用无界`VecDeque`吸收压力。notification只请求重查，
不得成为capacity、selection或delivery truth。

### 3.6 实现反馈不改写 target

Stage 0 若证明当前smoltcp public surface不够，只说明本次“无需vendored change”的route失败，不说明global
Stack、single Endpoint owner或send-success target失败。保持target的内部route修正写回本文与transaction；只有
证据要求移动owner、复制binding truth、允许success后source-drop、绕过normal ingress或降低acceptance floor时，
才停止并进入RFC review / `Target Renegotiation Gate`。

## 4. 阶段成熟度与滚动解析

- `Outline`：future stage，只冻结目的、依赖、受保护边界和解析触发点。
- `Ready`：交付、probe路线、审计、可观测性、验证、停止/退出条件、contract cutover与
  `Resolved Write Set Manifest`均已解析，但尚未获得执行授权。
- `Active`：公共RFC、transaction与用户/编排协议已经明确授权当前stage开始执行。
- `Closed`：当前stage按自己的review、验证与退出条件独立关闭；不因下一stage仍是Outline而保持Active。
- Stage N关闭后，单独运行只读`N -> N+1 Implementation Resolution Gate`；不得把解析下一stage作为
  Stage N closure的一部分。
- Ready/Active manifest冻结后才存在write-set expansion。future Outline的自然收窄、扩大、拆分或重排不是
  expansion，但改变target、owner、ABI、contract或acceptance boundary必须回到RFC review。

## 5. 阶段路线图

| Stage | 成熟度 | 概括目的 | 前置依赖 | Contract 状态 |
| --- | --- | --- | --- | --- |
| Stage 0 — Multi-interface UDP topology probe | Closed；positive decision | 验证单一Stack-level Endpoint owner、显式egress selection、双Ethernet interface与bounded IP-medium local link能否在现有shared/vendored边界内闭合 | 公共R0接受与transaction activation | None；全部保持现状 |
| Stage 1 — Initial domain / global Stack walking skeleton | Closed | 把current per-netdev Stack wiring迁移为initial-domain唯一Stack与logical-interface/attach authority，保留现有frame traffic | Stage 0 Closed；`0 -> 1`resolution完成 | `NET-UDP-DOMAIN-CUTOVER`已Refine `NETDEV-LIFE-001`/`NET-ATTACH-001`并Introduce `NET-IFACE-DOMAIN-001` |
| Stage 2 — Static control plane与production loopback | Closed | materialize SystemTarget network input，建立唯一IPv4 control plane、local route与bounded production `lo` | Stage 1 Closed；`1 -> 2`resolution完成 | `NET-UDP-CONTROL-CUTOVER`已Refine `STM-TARGET-001`并Introduce `NET-CONTROL-PLANE-001` |
| Stage 3 — Endpoint/socket nonblocking vertical slice | Closed | 建立opaque Endpoint association、bind/port/send/receive transaction与五项syscall的nonblocking纵切 | Stage 2 Closed；`2 -> 3`resolution完成 | Contract Impact为None；`NET-PROTOCOL-BOUNDARY-001`、`NET-SOCKET-ENDPOINT-001`与`NET-UDP-TRANSACTION-001`继续Pending |
| Stage 4 — Blocking/iomux与datagram hardening | Ready / Not Active | 以完整复数route socket source接入poll/select/epoll，复用同一predicate完成blocking/signal，并关闭race、copy-fault、capacity/writable与fragment evidence | Stage 3 Closed；`3 -> 4`resolution完成 | Contract Impact为None；保持既有OPENED-DESC/IOMUX/EPOLL IDs，全部Socket/Endpoint/UDP/wait candidate继续Pending |
| Stage 5 — External/dual-architecture closure | Outline | 完成remote external双向路径、双架构同源测试、RV64 agent-run、LA64 user-run、旁路删除与原子final cutover | Stage 4 Closed | 所有仍Pending ID在达到各自evidence floor后Effective或明确Not Cut Over |

Stage名称与数量可以在保持target的Route Correction中调整，但Ready / Active阶段必须先更新本文与transaction后
再改变冻结边界。Stage 3的具体Rust边界、逐文件write set、capacity数值与精确命令已在6.3保留；Stage 4的完整
Ready定义位于6.4，Stage 5仍不预定具体实现细节。

## 6. Current Stage and Future Outlines

### 6.1 Stage 1 Closed — Initial domain / global Stack walking skeleton

**阶段成熟度与授权：** Closed。本节保留2026-07-29 `0 -> 1`resolution冻结的完整Ready定义、路线、contract
cutover、验证、停止/退出条件与Resolved Write Set Manifest；后续独立授权只执行单一Checkpoint 1A，不授权
`1 -> 2`resolution或Stage 2。

**前置条件：**

- Stage 0已经按7.14以positive decision独立Closed；最终source位于`dev/drc/alpha@1a37c6cb`，进入resolution时
  tracked/untracked worktree clean；
- Stage 0最终topology、5项host matrix、no-default/RV64 compile evidence与最终review保持有效；
- R0 target、Contract Impact、current Network contracts与register未漂移；当前没有会改变本stage owner、顺序或
  验收的open tracking issue；
- 实现开始前仍须重新核对branch/HEAD、dirty state、本节manifest和live source；Ready不替代独立activation。

**Resolved route：**

- 保留Stage 0证明的`anemone-smoltcp-stack::Stack`与aggregate private-engine形状。Stage 1不修改vendored
  smoltcp或`anemone-net-api`，也不引入新的shared trait/value；现有
  `Stack::{add_interface, remove_interface, pump}`足以承接production external path。
- kernel `net`建立唯一`InitialDomain`。它组合但不混淆两个owner：`LogicalInterfaces`唯一拥有domain-local
  membership、`LogicalInterfaceId`、ifindex、name与kind；`DomainStack`以private `SpinLock<Stack>`唯一拥有
  protocol object和mutation access window。raw `Stack`只存在于`DomainStack`实现内。
- `InitialDomain`启动时提交一个boot-persistent logical `lo`：ifindex 1、name `lo`、kind `Loopback`。Stage 1只使
  该logical membership成为domain fact，不建立IP address、route、Stack local-interface mapping或software-link；
  production loopback仍由Stage 2一次闭合，不能把logical record写成functional `lo`证据。
- external logical identity由`LogicalInterfaces`以reservation -> commit transaction分配：external ordinal从0
  开始形成name `eth<N>`，ifindex从2开始单调分配，失败reservation不发布且已分配identity不复用。它可以保存
  opaque `NetdevId`关联，但不保存provider backing、queue、link/resource truth或protocol `InterfaceId`。
- `DomainStack::attach_external()`在唯一Stack lock下建立private mapping并返回`ExternalPumpPort`；port只携带
  `Arc<DomainStack> + InterfaceId`，只允许本interface的bounded pump与transaction-local rollback，不能暴露
  `Stack`、`SocketSet`、Endpoint、route或其它interface mutation。
- 继续保留每个external provider一个generic worker。worker core只拥有concrete provider与
  `ExternalPumpPort`；每次调用只在一个finite `Stack::pump`期间取得domain Stack lock，repoll round之间释放。
  provider recheck、IRQ wake、deadline和terminal control继续per-path；provider仍唯一拥有queue/DMA/IRQ/resource
  truth。多个heterogeneous provider因此无需trait object、downcast、全局provider registry或单一worker。
- 采用per-provider worker + single Stack window，而不采用“一个worker拥有全部provider”：后者需要新的heterogeneous
  provider erasure/registry并移动provider lifecycle，Stage 1没有第二个真实consumer或证明收益。也不把
  `Arc<SpinLock<Stack>>`直接交给worker；`ExternalPumpPort`是阻止future raw-Stack依赖的窄capability。

**单一Checkpoint 1A — domain/global-Stack attach cutover：**

1. `device/net` registry保留`NetdevId`、origin、normalized publication facts、record与pending provider capability；
   删除device-owned ifindex/name分配、`NameTooLong` publication failure和相关driver/KUnit依赖。Netdev publication
   identity与logical interface identity保持不可互换。
2. 新建`net/domain.rs`，实现上述logical-interface owner、唯一`DomainStack`和narrow pump port；
   `attach_published_netdevs()`在drain任何external capability前无条件初始化initial domain和`lo`，即使本次没有
   external netdev也不能跳过。`net/mod.rs`继续拥有attach transaction、active publication与shutdown admission，
   `worker.rs`只拥有worker/control/provider progression。
3. external attach先在authority下取得未发布logical reservation，再把provider与MAC交给`DomainStack`建立mapping，
   安装wake并创建inactive worker；最后在同一authority critical section提交logical membership、active-path record
   并activate worker。任何reader都不能在mapping/worker/wake/time未准备时观察usable external path。
4. missing Ethernet address在reservation/mapping前失败并返回同一published capability。worker spawn失败先从唯一
   Stack撤销mapping，再abort未发布logical reservation，最后把同一provider capability交回`device/net` pending
   owner；一个失败不回滚`lo`或其它active interface，也不自动retry。
5. 若terminal shutdown在prepare后、publish前关闭admission，先撤销Stack mapping与logical reservation，再请求
   inactive worker stop并retain provider到reset/power-off；这是terminal retention，不伪装成ordinary
   published/unattached retry。normal active shutdown继续只关闭admission、请求stop且不wait/join，domain Stack、
   mappings与provider backing保持retained。
6. 删除production worker内`Stack::new()`和per-worker Stack field；最终production只有`InitialDomain`构造一次
   `Stack::new()`，old/new wiring不能同时推进protocol mutation。
7. 更新Stage 0 temporary comments：aggregate Endpoint owner随唯一production Stack保留，但Endpoint operation仍
   dormant到Stage 3；IP-medium local-link仍只是Stage 2 resolution input。conditional host facade只服务长期
   deterministic matrix，不进入kernel dependency；Stage 3的real endpoint consumer出现后重新判断其最小保留面。
8. 同一checkpoint完成source、host/KUnit/runtime、change review、current-contract正文与transaction write-back；
   任一cutover evidence失败时三个contract delta全部保持Not Cut Over，不提交一半device identity或global Stack
   wiring。

**Internal object/API boundary：**

- `InitialDomain`：boot-persistent initial-domain composition；持有`LogicalInterfaces`和唯一
  `Arc<DomainStack>`，不持有provider、route/control-plane table、Endpoint association或Linux readiness。
- `LogicalInterfaceReservation`：只表示一次未发布external admission；只能commit为immutable logical snapshot或
  abort，不得被control plane、Stack pump、日志lookup或future UAPI当成membership truth。
- `DomainStack`：唯一raw `Stack` owner；提供attach/rollback和`ExternalPumpPort`所需的private方法。lock admission、
  contention与wake policy留在kernel owner，不下沉到stack crate。
- `ExternalPumpPort`：worker-local narrow capability；`pump(provider, now, budget)`一次只推进自己的opaque
  `InterfaceId`，返回原`PumpOutcome`。rollback只在active publication前由attach transaction使用；active path没有
  runtime detach入口。
- `ActivePath`：attach-authority的published record，保存logical snapshot、device publication snapshot与
  `PumpControl`；两份snapshot只用于各自identity/diagnostic scope，不缓存provider current truth或Stack mapping。
- 这些类型保持kernel-private，不新增`anemone-net-api` surface。future socket/control-plane consumer也不能由本stage
  取得`DomainStack`或`ExternalPumpPort`。

**Lock order与progress boundary：**

- attach order固定为authority logical reservation -> release authority -> domain Stack mapping/worker prepare ->
  authority publication；需要同时cleanup时先撤销未发布Stack mapping，再在authority下abort reservation。代码用
  comments和常开`assert!`保留这一顺序，不建立可并发更新的combined lifecycle cache。
- active worker不取得attach-authority/device-registry lock；它只在一个bounded pump call内持domain Stack lock并
  调用自己的provider。provider callback不得反向进入domain Stack或attach authority，也不得sleep。
- authority shutdown只在自身lock内关闭admission并snapshot `PumpControl`，锁外请求stop；不取得domain Stack lock、
  provider lock或等待worker。worker在round间检查stop，queued timer/wake不能重新activate。
- global Stack contention是Stage 1明确的serialization boundary，不是固定单worker或大锁target；finite pump、有限
  repoll与round间释放lock使其它interface有机会推进。若真实review证明provider callback会sleep/reenter或该边界
  无法保持有限推进，命中本stage停止条件，不能加第二Stack规避。

**模块边界预检：**

- 当前`net/mod.rs`同时承担attach orchestration、active publication和shutdown admission，`worker.rs`承担worker
  lifecycle、provider ownership、timer/recheck与per-netdev Stack。Stage 1若把logical registry和global Stack lock
  继续塞入任一文件，会混合domain state owner与worker progression。
- 因此本checkpoint新建同owner目录内的`net/domain.rs`，只承载logical-interface registry、initial-domain
  composition、Stack access window与pump port；`mod.rs`保留cross-owner attach/shutdown transaction，`worker.rs`
  删除Stack ownership后保留per-provider worker。`device/net/registry.rs`已经是独立publication owner，不再拆分。
- 这是same-subsystem结构维护；不建立generic network manager、runtime registry framework或public facade。若实现
  需要移动provider owner、改变shared API、增加route/control plane或触碰Stage 2 surface，必须停止并申请manifest/
  gate扩展。

**Scope envelope：**

- 本stage只建立logical membership/global Stack/attach walking skeleton；不实现IP配置、route/source selection、
  production local link、Endpoint operation、socket UAPI、wait/readiness或runtime detach。
- `lo`只在logical namespace中存在；不把Stage 0 `LocalPort`接入production。Stage 0 UDP topology tests继续证明
  aggregate candidate，但不构成Stage 1 runtime UDP或functional loopback evidence。
- frame ownership、provider capacity/recheck、single-instance pump、post-`Late` activation、terminal retention与
  network-before-device order继续受current contract保护；不调整KernelConfig capacity、worker count、CPU affinity、
  initcall、timer、power或build configuration。

**审计与review：**

- 搜索production `Stack::new()`、raw `Stack` field/reference、`SocketSet`/smoltcp handle与pump callsite：唯一
  construction位于domain owner，worker只能经`ExternalPumpPort`，old per-netdev Stack path为零；
- 搜索`NetdevSnapshot::{ifindex,name}`、device registry `eth`命名、driver publication log和logical lookup：cutover后
  ifindex/name只由`LogicalInterfaces`驱动，device/logical/protocol identity没有转换或共同numeric key；
- 逐项审计reservation、mapping、worker spawn、active publication、ordinary failure、shutdown race与terminal
  retention，确认每个partial resource有唯一rollback/retention owner；
- 审计domain Stack lock、provider callback、worker repoll/deadline、wake与shutdown paths，确认无lock inversion、
  sleep/reentry、并发`&mut Stack`、unbounded round或notification-as-truth；
- 审计Cargo feature/public surface与Stage 0 candidate，确认kernel继续`default-features = false`、host facade不进入
  production、`anemone-net-api`/vendored smoltcp无diff；
- 对最终aggregate diff执行一次change review，重点覆盖owner/identity single truth、module boundary、attach
  linearization、rollback、global-Stack concurrency、provider lifetime、shutdown retention与contract/doc原子性。
  Apollyon/Keter必须为0；会影响本cutover owner、lifecycle或验证结论的Euclid也必须修复或进入明确停止/回写路径。

**可观测性：**

- boot日志分别打印device publication ID/origin与logical name/ifindex/kind，Stack mapping只打印opaque diagnostic
  `InterfaceId`；不得把任一数值用于跨ownerlookup或反推。
- initial-domain summary打印一次logical `lo`和唯一global Stack初始化；external active/ordinary failure/terminal
  retention使用不同消息，不能把retained-unpublished path写成active或retryable。
- 不增加per-packet log、queue/resource mirror或global diagnostic registry。`ActivePath`中的immutable snapshots用于
  shutdown/diagnostic，不参与provider/Stack admission；字段旁必须标明其snapshot/diagnostic边界。
- lightweight invariants使用常开`assert!`：单一domain Stack construction、reservation单次commit/abort、active
  publication前mapping/worker/control齐备、rollback命中原mapping、shutdown后不能activate。

**Contract cutover — `NET-UDP-DOMAIN-CUTOVER`：**

- 同一Stage 1 closure原子Refine `NETDEV-LIFE-001`：`device/net`继续拥有boot publication record、NetdevId、origin、
  normalized facts与pending capability，但不再拥有domain ifindex/name。
- 同一closure原子Refine `NET-ATTACH-001`：attach destination改为initial-domain logical admission + global Stack
  mapping + per-provider narrow pump port；publication-last、failure isolation、shutdown admission与unsafe provider
  retention继续有效。
- 同一closureIntroduce `NET-IFACE-DOMAIN-001`，新建`contracts/net/interface-domain.md`，记录initial-domain
  logical membership/identity/name/kind owner、boot `lo` membership、identity domains与external admission义务；明确
  Stage 1不使production loopback/control plane/socket生效。
- `NET-BOUNDARY-001`、`NET-FRAME-OWN-001`、`NET-FRAME-PROGRESS-001`、`NET-STACK-PUMP-001`保持Preserve；
  frame-path contract只同步global Stack access的live enforcement，不改变stable rule语义或来源修订。
- cutover必须把code、tests、current contracts、Network contract index/SUMMARY、RFC/transaction状态作为一个
  checkpoint更新。任一host/build/runtime/review/docs gate失败时，current per-netdev contract继续Effective，三个
  delta均保持Pending；不得让新contract正文先于production wiring生效。
- `NET-CONTROL-PLANE-001`、`NET-PROTOCOL-BOUNDARY-001`、`NET-SOCKET-ENDPOINT-001`、
  `NET-UDP-TRANSACTION-001`、`NET-SOCKET-WAIT-001`与`STM-TARGET-001` Refine继续Pending；Stage 1不宣称
  functional loopback、UDP或SystemTarget network schema。

**Resolved Write Set Manifest：**

允许production source写入：

- `anemone-kernel/src/net/{mod.rs,worker.rs}`；
- 计划新建`anemone-kernel/src/net/domain.rs`；
- `anemone-kernel/src/device/net/{mod.rs,registry.rs}`；
- `anemone-kernel/src/driver/net/virtio/mod.rs`，只用于改写publication diagnostic与删除device-owned name/ifindex
  failure mapping；VirtIO frame/device data plane保持只读；
- `anemone-kernel/crates/anemone-smoltcp-stack/{Cargo.toml,src/lib.rs,src/stack/mod.rs}`，只用于声明新的长期
  multi-interface host target和更新Stage 0 candidate退出注释；ordinary UDP/local-link/pump semantics保持只读。

允许test/validation source写入：

- `anemone-kernel/src/net/domain.rs`中的owner-local KUnit；
- `anemone-kernel/src/device/net/registry.rs`中的existing KUnit expectation更新；
- 计划新建`anemone-kernel/crates/anemone-smoltcp-stack/tests/multi_interface.rs`；
- repository owner生成的`build/**`、Cargo target output与wrapper创建的worktree-local runtime disk copy；这些不进入
  source diff或freshness authority。

允许closure文档写回：

- `docs/src/rfcs/net-udp/{implementation.md,index.md,invariants.md}`，只同步Stage 1 closure、contract结果与
  Stage 2仍Outline边界，不改变R0 target；
- 当前net-udp transaction、transactions index、当前biweekly devlog与`docs/src/rfcs.md`；
- `docs/src/contracts/net/{index.md,frame-path.md,netdev-lifecycle.md,attach-lifecycle.md}`；
- 计划新建`docs/src/contracts/net/interface-domain.md`；
- `docs/src/{contracts.md,SUMMARY.md}`，只加入新的Active contract导航。register只有出现target内defect或target外
  accepted gap时才能写入；正常closure不修改register。

validation-only只读输入：

- `anemone-kernel/crates/anemone-smoltcp-stack/src/{pump.rs,udp.rs,local_link.rs,adapter.rs,stack/host_validation.rs}`与
  existing `tests/{support/mod.rs,frame_path.rs,bounded_progress.rs,multi_instance.rs,udp_topology.rs}`；
- `anemone-kernel/crates/anemone-net-api/**`、vendored smoltcp相关interface/UDP/phy source；
- `anemone-kernel/src/{main.rs,power.rs,device/mod.rs,driver/net/virtio/{device.rs,frame.rs}}`；
- `scripts/run-user-test-rv64.sh`、`conf/rootfs/pretest-rv64.toml`、调用者显式选择的初赛RV64 master image与当前
  user-test profile；wrapper只复制master，不得修改master或把个人`etc/`路径写成公共接口；
- R0 target、Stage 0 commits/diff/transaction、current Network/System Power contracts与register。

明确禁止写入：

- `anemone-kernel/crates/anemone-net-api/**`与vendored smoltcp；
- `anemone-kernel/crates/anemone-smoltcp-stack/src/{pump.rs,udp.rs,local_link.rs,adapter.rs,
  stack/host_validation.rs}`及existing host test source；若真实failure需要修改这些文件，先上报manifest扩展与
  target/contract/验证影响；
- kernel VFS、task、syscall、iomux、timer、power、main/initcall、architecture、generic device或其它driver source；
- root `Cargo.toml`/`Cargo.lock`、Justfile、xtask、`conf/**`、anemone-apps、SystemTarget/current configuration contract；
- Stage 2 control plane/production loopback、Stage 3 Endpoint/socket、Stage 4 wait或与net-udp无关的tracked/private
  file。

**验证：**

1. Crate host gate：`cargo test -p anemone-net-api -p anemone-smoltcp-stack`。新的`multi_interface` target必须以
   一个`Stack`、两个独立provider/InterfaceId覆盖双向ICMP/frame progression、wrong-provider isolation、一个
   provider blocked时另一interface继续、mapping rollback不污染剩余interface和monotonic private ID；existing
   `udp_topology` 5项与frame/bounded/multi-instance targets继续实际执行。Cargo命令是既有crate-owned host gate，
   仓库目前没有并列test wrapper，不新增临时Just/script入口。
2. Production feature gate：`cargo test -p anemone-smoltcp-stack --no-default-features --no-run`与
   `cargo check -p anemone-smoltcp-stack --no-default-features`；证明新test由`required-features = ["host-test"]`
   隔离，kernel dependency不获得host facade/control。
3. `just fmt kernel --check`；任何Stage 1 authored file新增formatter diff阻塞。既有vendored smoltcp baseline只能
   原样记录，不能借本stage扩大write set。
4. Repository-owned compile gate：
   `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`。它只证明当前resolved target的RV64
   compile/link，不证明runtime、frame traffic或contract cutover。
5. Fresh-disk RV64 closure：
   `./scripts/run-user-test-rv64.sh <preliminary-rv64-sdcard-image> build/net-udp-stage1-rv64.log`。不修改profile；
   同一current image必须看到全部enabled KUnit通过，其中domain KUnit覆盖`lo`唯一性、external reservation/
   commit/abort、monotonic no-reuse、identity分离与failure isolation；boot日志只出现一次global Stack初始化，
   `lo` logical membership与`eth0` external active attach成立，随后严格`filesystem -> network -> device -> PowerOff`
   且QEMU正常退出。master image保持只读，runtime disk/log为validation output。
6. Source/contract gate：执行前述identity/raw-Stack/lock/rollback/temporary-seam audit；对final diff完成change review；
   `git diff --check`、每个新文件逐项`git diff --no-index --check -- /dev/null <file>`与`mdbook build docs`通过，
   contract links/anchors与manifest文件存在。

Stage 1不恢复已删除的production ICMP validation probe，也不新增kernel packet injection。host real-stack
multi-interface ICMP证明protocol/frame migration，RV64只证明logical/domain wiring、真实VirtIO active attach、KUnit
与shutdown regression；它不形成configured IP traffic、functional loopback、UDP syscall、LA64、SMP>1、virtio-pci、
hardware、LTP或final-harness evidence，这些明确Not Run并由后续stage负责。

**停止条件：**

- 一个global Stack无法在per-provider finite round边界内串行推进，必须第二Stack、并发`&mut Stack`、单一大worker
  owning all providers、sleep/reentrant provider callback或无界mailbox才能工作；
- logical identity需要由device `NetdevId`/protocol `InterfaceId`反推，或device/domain两个registry必须同时驱动
  ifindex/name/membership；
- attach ordinary failure无法在publication前撤销mapping/reservation并交回同一provider capability，或terminal
  retention会释放device/IRQ仍可能访问的backing；
- 需要修改`anemone-net-api`、vendored smoltcp、provider public API、SystemTarget、route/control plane、production
  local link、socket/Endpoint UAPI或existing frame contract语义才能完成；
- global Stack lock必须跨sleep、authority/device lock、worker wait或shutdown join，或notification/diagnostic snapshot
  必须成为admission truth；
- host/RV64/review/docs任一mandatory gate失败，或current-contract三项无法原子切换；
- Ready manifest需要越界且尚无批准，或真实证据要求改变R0 target/owner/ABI/acceptance boundary。

前五类先区分Route Correction与target/contract change：保持target的internal encoding可在更新本节并记录transaction
后重做；改变owner、contract delta、normal-ingress/send-success或acceptance时停止进入RFC review / Target
Renegotiation Gate。不得用parallel registry、compat mirror、第二Stack或较弱验证绕过。

**退出条件：**

- production `Stack::new()`只在initial-domain owner出现一次；所有external worker只持narrow pump port，有限round
  与shutdown路径没有并发Stack mutation、lock inversion或失去provider lifetime；
- `device/net`不再拥有或导出domain ifindex/name；`LogicalInterfaces`唯一发布`lo`与external identity，ordinary
  failure/terminal race/active shutdown各自的rollback或retention闭合；
- existing frame/Stage 0 host suites、no-default、format、RV64 build、fresh-disk KUnit/attach/shutdown、source audit、
  final change review与docs/whitespace gate全部达到本节floor，Not Run没有冒充PASS；
- `NET-UDP-DOMAIN-CUTOVER`在同一checkpoint使`NETDEV-LIFE-001`/`NET-ATTACH-001`更新并使
  `NET-IFACE-DOMAIN-001`Active；其它R0 candidate保持Pending；
- transaction记录exact source route、review、validation、old/new contract、cutover point与claim boundary，RFC/
  navigation同步Stage 1 Closed；
- Stage 1先独立Closed。Stage 2仍是Outline，只有后续独立`1 -> 2 Implementation Resolution Gate`可以解析为
  Ready；本stage closure不得自动解析或启动Stage 2。

**Closure result — 2026-07-29：** Checkpoint 1A按本节路线删除device-owned ifindex/name与per-worker Stack，
建立boot logical `lo`、external reservation/commit/abort、initial-domain唯一`DomainStack`与per-provider narrow pump
port。普通spawn failure按mapping -> reservation顺序回滚并交回同一provider；publish前terminal race先撤销两份
未发布状态，再stop inactive worker并retain provider。source audit确认raw Stack、identity truth、authority/Stack
lock order、finite pump和terminal lifetime均保持唯一owner，未修改shared/vendored API或Stage 2 surface。

host stack/net-api、one-Stack/two-provider matrix、no-default test/check、authored-file format、RV64 release build与
fresh-disk RV64 closure达到本节floor；RV64运行实际执行263/263 KUnit、真实VirtIO active attach、strict
`filesystem -> network -> device -> PowerOff`并正常退出。最终日志调用按开发者要求保持`kinfoln!`；runtime run
曾临时提高这三项lifecycle记录的直接console可见性，因此该run不作为最终INFO console print-level的exact-code
proof，行为/KUnit/shutdown证据仍有效。LA64、SMP > 1、functional loopback、UDP syscall/UAPI、configured external
IP traffic、virtio-pci、hardware、network LTP和final harness均Not Run。

`NET-UDP-DOMAIN-CUTOVER`在本checkpoint原子Refine `NETDEV-LIFE-001`/`NET-ATTACH-001`并Introduce
`NET-IFACE-DOMAIN-001`；frame-path四项ID保持Preserve，其它R0 candidate继续Pending。exact执行、review、验证和
claim boundary见[transaction](../../devlog/transactions/2026-07-29-net-udp.md)。Stage 1在此Closed，未运行或解析
`1 -> 2`gate。

### 6.2 Stage 2 Closed — Static control plane与production loopback

**阶段成熟度与授权：** Checkpoint 2A与Checkpoint 2B均Closed。2026-07-29的`1 -> 2`resolution
冻结了本节两个checkpoint、module/file切分、配置和runtime路线、contract cutover、验证、停止/退出条件与Resolved
Write Set Manifest。2A已经按split-only route独立关闭；该closure不授权2B source、SystemTarget/KConfig/
current-contract修改或进入Stage 3；2B随后由独立授权完成，本closure不授权`2 -> 3`resolution或Stage 3。

**前置条件与live baseline：**

- Stage 1已在`dev/drc/alpha@5cc0669c`独立Closed；进入本resolution时tracked/untracked worktree clean，最终review
  为Apollyon 0、Keter 0、Euclid 0、Safe 0，`NET-UDP-DOMAIN-CUTOVER`三项current contract已经生效；
- production `InitialDomain`现在唯一组合`LogicalInterfaces`与`DomainStack`。boot `lo`只有logical membership；
  global Stack仍没有local protocol mapping、IP projection或production local worker；
- external worker只持`ExternalPumpPort`与concrete provider，raw Stack仍只在`DomainStack`的private lock内；
  `ActivePath`保存device/logical diagnostic snapshot和shutdown control，但没有control-plane behavior state；
- Stage 0保留的`LocalLink`/`LocalPort`/`pump_local` candidate已经在host matrix证明IP-medium、bounded handoff、
  normal ingress、queue full/recovery、Endpoint-local blocking与retire cleanup；它仍是crate-private dormant route，
  kernel dependency未编译host validation surface；
- live SystemTarget只拥有Platform/root/initial-program，resolver完整保存target snapshot；kernel build在
  `tasks/build`物化ignored `kconfig_defs.rs`、`platform_defs.rs`和`boot_defs.rs`，kernel不解析TOML；
- RV64与LA64 QEMU Platform都使用user network backend；普通pretest preset分别选择
  `qemu-virt-{rv64,la64}`。两份SystemTarget尚未声明`eth0`、IPv4/prefix或gateway；
- register、R0 target、current SystemTarget/Network contracts与Stage 1 exact diff没有发现会改变本stage owner、
  ABI、acceptance或顺序的active issue。Stage 2不需要新的probe，也不修改vendored smoltcp。

#### 6.2.1 Resolved owner与数据流

Stage 2保持Stage 1的三个owner分离，并只增加一个control-plane owner：

- `LogicalInterfaces`继续唯一拥有membership、logical identity、ifindex/name/kind；不保存address、route或protocol
  mapping；
- `DomainStack`继续唯一拥有raw Stack、protocol `InterfaceId` mapping和mutation window；Stack只保存smoltcp所需
  address/default-route projection，不执行route/source/interface policy；
- `Ipv4ControlPlane`唯一拥有boot-time local-address set、loopback/local/connected/default route precedence、
  explicit-source validity与每次operation的source/interface selection。它保存logical-to-protocol association的
  immutable boot-lifetime snapshot和窄pump-wake capability；这些是行为协议状态，不是diagnostic field。真实mapping
  仍由DomainStack拥有，第一版无runtime detach/reuse，因此snapshot不允许stale；
- external/local worker各自拥有自己的pump scheduling、deadline与stop projection。external provider继续拥有
  queue/DMA/IRQ/resource truth；bounded local software link只拥有packet从protocol egress到后续normal ingress之间
  的访问权与capacity truth；
- SystemTarget只声明deployment input；xtask resolver/materializer生成只读typed Rust projection。Platform、
  KernelConfig、BuildPreset、rootfs和generated file都不成为IP/route truth。

control-plane selection固定为一次pure owner-local decision，结果只携带protocol `InterfaceId`、selected source
IPv4和窄`PumpWake`：

1. destination命中任一configured local unicast address时选择local protocol port；destination是external local
   address时默认source就是该address，不进入external provider；
2. destination位于`127.0.0.0/8`时选择local port，未约束source时使用`127.0.0.1`；local Stack projection可以
   使用smoltcp AnyIP，但该mechanism只允许接收control plane已经选择到bounded local link的packet，不建立第二
   route policy；
3. 其后才检查configured external connected prefix，最后检查该SystemTarget显式声明的default gateway；两者都
   选择对应external protocol mapping和该interface的configured address；
4. explicit source必须属于当前domain。local destination可以使用任一local source；external connected/default
   route只接受该external interface的configured source。失败返回owner-local typed `NoRoute`、`SourceUnavailable`
   或`InterfaceUnavailable`，不得fallback到其它interface；
5. selection不修改binding或address truth，不执行Stack admission，也不把一次source写回Socket/Endpoint。

Stage 2只建立上述private kernel control-plane capability。Stage 3解析真实Socket/Endpoint consumer后才能决定
该capability是否需要进一步跨module收窄；本stage不增加通用`RouteTable` trait、runtime configurator或TCP框架。

#### 6.2.2 Checkpoint 2A — same-owner module split only

2A先做行为保持的目录化切分，避免把Stage 2职责继续写进已经混合多个proof surface的文件：

1. `net/domain.rs`迁移为`net/domain/{mod.rs,interfaces.rs,stack.rs}`：`mod.rs`只组合`InitialDomain`，
   `interfaces.rs`保存logical membership/reservation与其KUnit，`stack.rs`保存DomainStack、mapping transaction和
   narrow pump port；
2. `net/worker.rs`迁移为`net/worker/{mod.rs,control.rs,external.rs}`：shared bounded time/budget入口保持在
   `mod.rs`，wake/deadline/stop projection位于`control.rs`，provider/PumpCore/prepare path位于`external.rs`；
3. stack crate的`stack/mod.rs`把existing external-interface mapping/`InterfaceEntry`与dormant UDP operations分别
   迁入`stack/interfaces.rs`与`stack/udp_ops.rs`；root只保存Stack composition/error与保持原visibility的窄re-export，
   `host_validation.rs`保持conditional child module；
4. `pump.rs`迁移为`pump/{mod.rs,external.rs,local.rs,common.rs}`：`external.rs`保存public `Stack::pump`与
   provider-facing ingress/egress，`local.rs`保存private `pump_local`与local ingress/egress，`common.rs`保存
   `PumpBudget`、fair-order和outcome computation；existing tests随被测职责迁入`common.rs`/`external.rs`，
   `mod.rs`只做module wiring和保持既有public re-export。`local_link.rs`仍是单一packet/capacity owner，不拆；
5. `tasks/build/mod.rs`把existing kconfig/platform/boot defs rendering和相关unit test迁入
   `tasks/build/generated_defs.rs`；embedded-artifact validation及其test继续留在build orchestration，app export、DT与
   kernel-output owner不变。

2A不得新增type/method/feature/config field、扩大owner-external/public visibility或exported surface、修改生成文本、
移动owner、激活local path或更新current contract。只允许目录拆分所必需的owner-internal `pub(super)`/private
import/re-export机械调整，且原consumer callsite与可见能力集合必须不变。git rename detection不是closure条件；
行为、依赖方向和callsite surface不变才是。2A完成host/no-default/xtask/双架构compile和change review后独立
Closed；不得自动进入2B。

**Closure：** 2026-07-29，五组same-owner split与获批的stale xtask fixture修正通过6.2.8全部2A gate并以
独立checkpoint关闭。consumer/public surface、owner、runtime behavior和generated text不变，Contract Impact为None；
Checkpoint 2B仍Ready / Not Active。精确diff、review、validation与Not Run边界见
[transaction](../../devlog/transactions/2026-07-29-net-udp.md#2026-07-29---stage-2-checkpoint-2a-activation-and-same-owner-split)。

#### 6.2.3 Checkpoint 2B — static IPv4与production loopback cutover

2B是本stage唯一semantic checkpoint，配置、control plane、Stack projection、local worker、runtime proof和
current contract必须一起闭合：

1. SystemTarget增加optional `[network.ipv4]`，第一版只描述一个external interface：
   `interface`、`address`、`prefix`和optional `default-gateway`。loader拒绝unknown field、空interface、非法IPv4、
   prefix `> 32`、unspecified/multicast/broadcast/loopback external address及非法gateway；不做Platform topology、
   interface存在性、gateway reachability或backend匹配证明。section缺失表示该target只有implicit loopback，不触发
   fallback/default external configuration；
2. tracked `qemu-virt-rv64.toml`与`qemu-virt-la64.toml`显式声明`eth0 = 10.0.2.15/24`、default gateway
   `10.0.2.2`，与其tracked QEMU user backend配套；`example.toml`/schema同步字段形状。physical与final-harness
   SystemTarget本stage不自动获得external IPv4配置；
3. resolver继续把canonical SystemTarget放入`ResolvedSystemBuild`。`generated_defs.rs`从resolved target生成独立
   ignored `anemone-kernel/src/network_defs.rs`，其closed typed value只包含optional static IPv4 deployment；
   `main.rs`声明private generated module，只有kernel `net` owner读取。不得把network字段塞进
   `Platform::gen_platform_defs`、kconfig defs或boot defs，kernel不得解析manifest；`clean`删除该generated file；
4. `anemone-net-api/src/ipv4.rs`只增加cross-crate `no_std` value：IPv4 address/CIDR及必要checked construction/
   observation。它不包含route table、Socket/Endpoint、errno、smoltcp type、worker或mutable owner；
5. Stack提供由真实kernel consumer使用的窄ordinary methods：建立唯一IP-medium local port、对指定
   `InterfaceId`安装IPv4 CIDR/default-gateway projection，以及bounded local pump。local port使用
   `127.0.0.1/8`与local-only AnyIP projection；它不会自行决定route/source，也不暴露`LocalPort`、queue、
   `SocketSet`或smoltcp handle；
6. `InitialDomain`构造时在同一global Stack建立唯一local mapping并准备sleeping local worker。local worker只持
   `LocalPumpPort + PumpControl`，被explicit work/deadline唤醒，每轮复用既有PumpBudget与
   `NET_WORKER_REPOLL_ROUNDS`，round间释放Stack lock；local ingress可释放full queue时必须继续有限进展，真正无
   progress时睡眠，不能因`work_remaining`或已到期deadline busy-repoll；
7. tracked KernelConfig新增`net_local_link_packet_capacity = 64`与`net_local_link_mtu_bytes = 1500`。前者是
   shared ingress+egress packet slot上限，后者是IP-medium packet byte上限；两者在resolved kconfig defs中物化，
   kernel以`static_assert!`要求capacity非零且MTU至少容纳IPv4+UDP固定header。既有pump budget/repoll常量继续
   约束local worker，不新增同义knob；
8. boot attach drain完成后，attach authority把committed logical snapshot、stable protocol mapping和窄wake
   capability交给`InitialDomain`一次性激活control plane：先验证configured interface恰好命中已发布external
   logical name，再在Stack window安装lo/external projection，最后发布control plane。missing/duplicate/mismatched
   `eth0`记录target/interface诊断并fail closed；不搜索替代NIC、不降级为别的address，也不留下可供Stage 3读取的
   partial control plane；
9. network shutdown先在现有authority关闭global admission，再锁外请求local与external worker stop。local link和
   Stack仍由boot-persistent domain拥有，不引入join、runtime detach或resource reclamation；queued wake/deadline
   不能重新activate control plane或pump admission；
10. stack crate增加独立于`std`的no-std `kunit` feature，kernel的existing `kunit` feature显式转发
    `anemone-smoltcp-stack/kunit`；ordinary dependency继续`default-features = false`且不无条件启用该feature。该
    conditional surface只允许kernel test调用opaque Endpoint/local operation与有限observation。RV64 KUnit必须
    通过production control-plane selection、real DomainStack/local worker和normal protocol ingress，完成
    `127.0.0.1`、另一`127/8`地址与self-external
    address datagram交付，并在测试后retire；不得用direct Endpoint injection、Socket copy或kernel packet injection
    替代。Stage 3真实Socket consumer出现后，逐项删除不再必要的KUnit operation bridge；长期host topology matrix
    可以保留其最窄conditional facade。

#### 6.2.4 Concurrency、failure与旁路审计

- 唯一锁序保持attach authority/control-plane publication在外、DomainStack window在内；任何provider/local
  device callback都不能持authority lock运行或反向进入logical/control owner。pure selection在owner lock内完成并
  返回immutable result，后续Stack operation不持control-plane lock；
- `PumpWake`只是请求重查的capability，不是route、capacity、work或lifecycle truth。shutdown关闭control active
  projection后，late wake只能产生无害唤醒；
- local packet从TX token commit到RX token consume始终只有LocalLink owner；未消费RX token按原队首恢复，Endpoint
  retire先withdraw aggregate identity再清owner-tagged local packet。AnyIP不允许external ingress绕入local port；
- SystemTarget parse/build failure不生成半份network defs；runtime interface mismatch不回写generated/config值，
  不创建alternate selector。tracked manifests是canonical input，generated file是一次build projection；
- source audit必须确认production `Stack::new()`仍只有DomainStack一个调用点、local port只创建一次、route/source
  decision只在Ipv4ControlPlane、smoltcp route只作projection、kernel没有smoltcp/private handle、Platform/rootfs/
  Preset没有network copy；
- 2A re-export若变宽、2B需要vendored smoltcp变化、raw Stack泄露、第二route table、local Socket fast path、无界
  queue、busy-poll、automatic fallback或不能在conditional build删除的test bridge，当前checkpoint立即停止并按
  manifest/owner/target影响上报。

#### 6.2.5 可观测性

- build resolution摘要只增加`network=loopback-only`或configured external interface/address/prefix/gateway；不把
  generated text或QEMU backend推断写成另一份truth；
- control-plane activation一次记录target、logical name/ifindex、configured IPv4/prefix、default-route有无与local
  port ready；runtime mismatch在fail closed前记录期待/实际logical names。protocol `InterfaceId`只可作为opaque
  diagnostic，不暴露smoltcp handle；
- local worker只记录prepare/activation、unexpected pump failure与shutdown摘要；normal packet、full/recovery和
  每轮pump不打日志。轻量correctness不变量使用`assert!`；配置/ordinary环境失败使用typed error或明确boot failure；
- KUnit marker分别标明control-plane table、loopback worker、self-external local handoff；host、RV64 runtime、LA64
  compile与Not Run结果不得合并成一个“network works”结论。

#### 6.2.6 Contract cutover — `NET-UDP-CONTROL-CUTOVER`

Checkpoint 2A Contract Impact为None。Checkpoint 2B只有在final source/config/runtime evidence一起满足时，才原子：

- Refine `STM-TARGET-001`：SystemTarget可以声明上述optional first-version static IPv4 deployment；Platform、
  KernelConfig、Preset和rootfs owner边界不变，generated projection不成为canonical truth；
- Introduce `NET-CONTROL-PLANE-001`：initial-domain control plane唯一拥有local address、route precedence、
  source/interface selection、stable mapping/wake capability projection，以及bounded production local-handoff协议；
- Preserve `STM-OWNER-001`、`STM-RESOLVE-001`、`NET-BOUNDARY-001`、`NET-FRAME-OWN-001`、
  `NET-FRAME-PROGRESS-001`、`NET-STACK-PUMP-001`、`NETDEV-LIFE-001`、`NET-ATTACH-001`和
  `NET-IFACE-DOMAIN-001`。implementation location/navigation可同步，语义owner和现有attach/frame contract不改；
- `NET-PROTOCOL-BOUNDARY-001`、`NET-SOCKET-ENDPOINT-001`、`NET-UDP-TRANSACTION-001`、
  `NET-SOCKET-WAIT-001`继续Pending。conditional Endpoint KUnit不使Socket/UAPI/bind/receive transaction或Linux
  readiness contract提前生效。

本cutover的proof floor是deterministic multi-interface selection、self-external local routing，以及RV64 production
control-plane/local-worker/normal-ingress closure。Stage 2尚无用户态UDP consumer，真实remote external UDP
ingress/egress因此继续Not Run；它仍是R0 Evidence Matrix与Stage 5 final acceptance的强制项，必须在后续
`NET-UDP-TRANSACTION-001`/external-path closure由final exact code证明。该proof-stage分工不降低R0 target、
`NET-CONTROL-PLANE-001`规则或最终验收边界。

cutover前current SystemTarget contract不包含network schema，`NET-CONTROL-PLANE-001`为None；2B closure达到本节
proof floor后才由同一checkpoint切换为current。若任一Keter/Apollyon、production local-traffic failure、RV64
runtime未运行/失败或config/code/contract不能原子切换，则撤销或保留partial code作未发布证据，但不得更新current
contract、把logical `lo`称为functional或进入Stage 3。

#### 6.2.7 Resolved Write Set Manifest

**Checkpoint 2A tracked source：**

- `anemone-kernel/src/net/{domain.rs,domain/mod.rs,domain/interfaces.rs,domain/stack.rs}`；
- `anemone-kernel/src/net/{worker.rs,worker/mod.rs,worker/control.rs,worker/external.rs}`；
- `anemone-kernel/crates/anemone-smoltcp-stack/src/stack/{mod.rs,interfaces.rs,udp_ops.rs,host_validation.rs}`；
- `anemone-kernel/crates/anemone-smoltcp-stack/src/{pump.rs,pump/mod.rs,pump/common.rs,pump/external.rs,pump/local.rs}`；
- `scripts/xtask/src/tasks/build/{mod.rs,generated_defs.rs}`；
- `scripts/xtask/src/config/resolve.rs`，仅允许把`resolved_selection_owns_all_snapshot_inputs`的
  `max_logical_cpus` fixture/expected snapshot与canonical `conf/.defconfig`值`1`重新对齐。

2A只允许move、module declaration、owner-internal visibility/import/re-export和为保持原测试位置所需的机械调整；
这些调整不得扩大owner-external consumer surface。上述new/old path都列出是为了允许tracked rename。

**Checkpoint 2A docs/status write-back：**

- `docs/src/rfcs/net-udp/{implementation.md,index.md}`；
- `docs/src/devlog/transactions/{2026-07-29-net-udp.md,index.md}`；
- `docs/src/devlog/2026-07-20_to_2026-08-02.md`、`docs/src/rfcs.md`。

这些文档只记录2A activation/closure、exact diff、review、验证与2B Not Active状态，不得改变R0 target、candidate
contract或2B manifest。`local_link.rs`、`udp.rs`、kernel `net/mod.rs`、Cargo、除上述获批test-only
`resolve.rs`修正外的config source、`invariants.md`与current contracts均只读。

**Checkpoint 2B tracked source/config：**

- `anemone-kernel/.gitignore`，只允许增加`src/network_defs.rs`这一条generated-output ignore rule；
- `scripts/xtask/src/config/{system_target.rs,kconfig.rs}`；
- `scripts/xtask/src/tasks/{clean.rs,build/mod.rs,build/generated_defs.rs}`；
- `conf/.defconfig`、`conf/system-targets/{schema.jsonc,example.toml,qemu-virt-rv64.toml,qemu-virt-la64.toml}`；
- `anemone-kernel/Cargo.toml`、`anemone-kernel/src/{main.rs,net/mod.rs}`；
- `anemone-kernel/src/net/domain/{mod.rs,interfaces.rs,stack.rs,control_plane.rs}`；
- `anemone-kernel/src/net/worker/{mod.rs,control.rs,external.rs,local.rs}`；
- `anemone-kernel/src/net/kunit.rs`；
- `anemone-kernel/crates/anemone-net-api/src/{lib.rs,ipv4.rs}`；
- `anemone-kernel/crates/anemone-smoltcp-stack/Cargo.toml`；
- `anemone-kernel/crates/anemone-smoltcp-stack/src/{lib.rs,local_link.rs}`；
- `anemone-kernel/crates/anemone-smoltcp-stack/src/stack/{mod.rs,interfaces.rs,udp_ops.rs,host_validation.rs}`；
- `anemone-kernel/crates/anemone-smoltcp-stack/src/pump/{mod.rs,common.rs,local.rs}`；
- `anemone-kernel/crates/anemone-smoltcp-stack/tests/udp_topology.rs`。

repository build可覆盖ignored generated outputs
`anemone-kernel/src/{kconfig_defs.rs,platform_defs.rs,boot_defs.rs,network_defs.rs}`；这些文件不得手工编辑或作为
canonical evidence提交。`kconfig`仍是ignored developer-local input，不在tracked write set；Stage 2 build通过
selected preset解析`conf/.defconfig`。

**Checkpoint 2B contract/docs closure：**

- `docs/src/contracts/configuration/system-target.md`；
- `docs/src/contracts/net/{index.md,interface-domain.md,control-plane.md}`、`docs/src/contracts.md`、
  `docs/src/SUMMARY.md`；
- `docs/src/rfcs/net-udp/{implementation.md,index.md,invariants.md}`；
- `docs/src/devlog/transactions/{2026-07-29-net-udp.md,index.md}`、
  `docs/src/devlog/2026-07-20_to_2026-08-02.md`、`docs/src/rfcs.md`。

current-contract文件只在2B final cutover checkpoint写入；2A和2B implementation中途保持只读。若closure日期跨出
当前biweekly窗口，只能改当时current devlog，不回写旧timeline。没有confirmed design issue时不创建
`tracking-issues.md`或register条目。

**Validation-only inputs（只读）：**

- `Justfile`、`scripts/xtask/src/main.rs`、`scripts/run-user-test-{rv64,la64}.sh`；
- `conf/build-presets/qemu-virt-{rv64,la64}-release.toml`、`conf/platforms/qemu-virt-{rv64,la64}.toml`、
  `conf/rootfs/pretest-{rv64,la64}.toml`；
- caller-selected `etc/preliminary/images/sdcard-{rv,la}.img`只作为只读master；wrapper生成的worktree-local runtime
  disk/log是validation output，不是source write set；
- vendored `anemone-kernel/crates/anemos/smoltcp/**`、`anemone-abi`、device/driver/provider、VFS/syscall/iomux/epoll、
  rootfs/app test content、final-harness scripts/config与其它RFC/current contract保持只读。

对manifest外tracked file的任何修改都是write-set expansion。formatter触及的既有风格diff按AGENTS规则可以保留，
但必须在checkpoint review中单列；owner/public API/shared contract/ABI/acceptance变化不能按formatter例外处理。

#### 6.2.8 验证

**Checkpoint 2A：**

1. `cargo test -p anemone-net-api -p anemone-smoltcp-stack`；确认existing stack unit、`bounded_progress`、
   `frame_path`、`multi_instance`、`multi_interface`、`udp_topology`和net-api doctest数量/结果不退化；
2. `cargo test -p anemone-smoltcp-stack --no-default-features --no-run`与
   `cargo check -p anemone-smoltcp-stack --no-default-features`；
3. repository-owned `just xtask-test`与`just fmt kernel --check`；authored file不得增加formatter diff，既有vendored
   baseline单列；
4. `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`与对应LA64 command；只证明split后的
   双架构compile integration，不外推runtime；
5. `git diff --summary`、callsite/visibility/source audit、`git diff --check`与final change review。2A不运行QEMU/
   LTP，不更新contract，完成后独立Closed。

**Checkpoint 2B focused/config/host：**

1. `just xtask-test`必须覆盖network section absent/present、unknown/invalid field、IPv4/prefix/gateway/interface
   validation、preset/tuple同一resolved target、generated network defs exact shape、clean output清单和Platform/rootfs/
   Preset rejection；
2. `cargo test -p anemone-net-api -p anemone-smoltcp-stack`新增/保持：`127.0.0.1`与另一`127/8`destination、
   self-external address走selected local port、connected/default selection projection、bounded full/recovery、local
   ingress finite progress、retire cleanup和external isolation；
3. `cargo test -p anemone-smoltcp-stack --no-default-features --no-run`、
   `cargo check -p anemone-smoltcp-stack --no-default-features`、
   `cargo check -p anemone-smoltcp-stack --no-default-features --features kunit`与kernel dependency feature audit，
   证明no-std KUnit surface可单独编译，ordinary kernel不携带conditional validation facade；
4. kernel owner KUnit覆盖route precedence、explicit-source matrix、missing interface fail-closed前的pure validation、
   one-time control publication、late wake/shutdown predicate和local queue finite progress；
5. `just fmt kernel --check`、`git diff --check`、new-file whitespace、manifest-file-existence、generated-file
   provenance/clean、source bypass audit和最终change review。

**Checkpoint 2B build/runtime：**

1. 使用fresh generated output分别运行RV64/LA64 release build：
   `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`与
   `just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G`；检查本次`network_defs.rs`确实来自各自
   selected SystemTarget，不使用stale projection；
2. 使用caller-selected preliminary RV64 master运行
   `./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/net-udp-stage2-rv64.log`。必须实际执行
   control-plane/local-worker KUnit，证明`127.0.0.1`、另一`127/8`与self-external local handoff都经production
   selection、protocol egress、bounded local link、后续normal ingress完成，并保持真实VirtIO active attach与strict
   `filesystem -> network -> device -> PowerOff`；
3. 不为Stage 2修改`profile.txt`或增加用户态UDP测试；syscall尚未生效。现有pretest其它PASS/FAIL只作regression
   observation，不能替代focused KUnit或声明network LTP；
4. LA64 runtime、SMP > 1、remote external UDP/frame traffic、UDP syscall/UAPI、VFS/iomux、virtio-pci functional
   traffic、hardware、full network LTP和final harness均为Not Run。LA64 build不能外推LA64 runtime；Stage 5的
   mandatory LA64同源用户测试不因本stage缩减。

docs closure最后运行`mdbook build docs`和whitespace/link/heading检查。若sandbox内repository build命中已知
lwext4 `Bad system call`/`SIGSYS`，只有同一command在sandbox外PASS才能分类为environmental；不得改成bare cargo
或删减selected target输入。

#### 6.2.9 停止与退出条件

任一checkpoint发现下列情况立即停止：module split扩大production public surface或移动semantic owner；SystemTarget
需要Platform/rootfs/Preset fallback；control plane/Stack各自决定route；`127/8`或self-external只能靠Socket fast
path/direct injection；local queue无法以有限pump/wake恢复；需要vendored smoltcp、第二Stack、unbounded storage、
busy-poll或non-conditional validation facade。保持R0的private API/file/capacity修正可走Route Correction并更新本文；
owner、contract classification、visible semantics或acceptance改变则进入Target Renegotiation Gate。

2A退出要求行为保持split、全部2A验证和review通过，source以独立checkpoint关闭且2B仍未Active。2B退出要求typed
config、one-time control publication、Stack projection、production local worker、三类RV64 local KUnit、双架构build、
source audit、final review与`NET-UDP-CONTROL-CUTOVER`全部在final exact code满足；conditional bridge带Stage 3删除
条件，current contract与status同步。Stage 2随后独立Closed，但Stage 3仍是Outline；`2 -> 3`resolution不得由2B
closure自动执行。

**Closure：** 2026-07-29，2B按上述atomic route完成optional SystemTarget static IPv4、generated typed input、
唯一control-plane selection、Stack projection、bounded production local worker与conditional production-path KUnit。
首次RV64运行暴露local pump在protocol egress后才把packet transfer为normal ingress、但production 32/32预算因本轮
egress未耗尽而返回Idle，sleeping worker可能遗留已发布ingress。获批Route Correction在`LocalLink::transfer()`
实际移动packet时强制后续有限round，并以production budget regression证明第二round转为Idle而非busy-poll；重新
执行的RV64三类local KUnit全部通过。

focused host/no-default/kunit、60项xtask、双架构release build、RV64 fresh-disk 270项KUnit、指定LTP 4/4、严格
shutdown、source/write-set/final review与docs gate达到本节floor。`NET-UDP-CONTROL-CUTOVER`原子Refine
`STM-TARGET-001`并Introduce `NET-CONTROL-PLANE-001`；Socket/Endpoint/UDP transaction/wait candidates继续
Pending。LA64 runtime、SMP > 1、remote external UDP、UDP syscall/UAPI、VFS/iomux、virtio-pci、hardware、full
network LTP与final harness均Not Run。Stage 2 Closed，并在进入`2 -> 3`resolution前停止；精确命令、review与
claim boundary见[transaction](../../devlog/transactions/2026-07-29-net-udp.md)。

### 6.2A Stage 2 post-close boundary correction — Closed

Stage 2关闭后的periodic engineering review发现，item 10把kernel `kunit`直接传播为stack crate的同名feature，
并让stack的conditional type/method理解具体KUnit harness；这重复了net-frame-path已经Neutralized的`NFP-005`。
同一review还确认kernel测试应与最低共同semantic owner共置，而不是只因使用KUnit就建立独立`kunit.rs`。这些反馈不
改变R0 target、runtime owner、ABI、visible semantics、acceptance或Stage 2既有proof，因此不增加RFC revision，
也不重新打开Stage 2；本节是进入`2 -> 3`resolution前独立关闭的behavior-preserving boundary correction。

**实现与生命周期：**

1. kernel `kunit`只启用stack的artifact-neutral `udp-validation-probe`；`anemone-smoltcp-stack`和
   `anemone-net-api`不得出现KUnit vocabulary。stack把conditional DTO、错误转换和create/send/receive/retire
   入口集中到`stack::udp_probe` namespace；ordinary `udp_ops.rs`不含validation `cfg`或conditional public method；
2. kernel删除独立`net/kunit.rs`，三项production local-path KUnit进入最低共同composition owner
   `net/mod.rs::kunits`。`LogicalInterfaceSnapshot`与`DomainStack`的KUnit-only support分别集中在对应owner文件
   底部，不建立单用途测试文件，也不散入ordinary impl；
3. `host_validation.rs`继续是长期、kernel dependency不会启用的host fixture facade。其packet injection与只读
   observation只有host test consumer；Stage 3解析真实Endpoint capability时逐项删除被正式surface替代的operation
   wrapper，保留项仍必须有真实host consumer；
4. `udp-validation-probe`是Stage 2缺少真实Socket consumer时的临时跨crate bridge。Stage 3 real
   Socket-to-Endpoint capability出现后必须删除feature、stack probe module和DomainStack probe support；现有三项
   KUnit改走真实consumer或由更强functional test替代，不能把probe重命名后沉淀为production API；
5. repo `AGENTS.md`记录KUnit与被测semantic owner共置、独立validation/probe文件必须有真实编译/consumer/
   lifecycle边界的编码规范。该module-layout规则是implementation preference，不进入current contract。

**Contract Impact：** `NET-BOUNDARY-001`保持Preserve。本checkpoint只把net-frame-path已接受、但当前contract
正文未完整保留的harness-owner边界补入current rule，并让live source重新符合它；其它network/configuration/
power/task/iomux/epoll ID全部保持当前状态。没有Socket/Endpoint/UDP transaction或wait candidate提前cut over。

**Resolved Write Set：** `AGENTS.md`；`anemone-kernel/Cargo.toml`；
`anemone-smoltcp-stack/{Cargo.toml,src/lib.rs,src/udp.rs,src/stack/{mod.rs,udp_ops.rs,udp_probe.rs}}`；
kernel `src/net/{mod.rs,kunit.rs,domain/{interfaces.rs,stack.rs}}`，其中`net/kunit.rs`只允许删除；以及本RFC
`{index.md,invariants.md,implementation.md}`、current `contracts/net/frame-path.md`、本transaction与当前biweekly
devlog。`anemone-net-api`、vendored smoltcp、host tests、SystemTarget/build configuration、Socket/VFS/syscall/
user-test、register与其它RFC保持只读。

**Validation / exit：** 两种stack no-default build、artifact-neutral probe feature build、完整stack/net-api host
gate、kernel formatter、双架构release build与fresh-disk RV64 wrapper必须通过；RV64必须实际执行三项local-path
KUnit并保持strict shutdown。source/dependency audit必须证明两个外部crate没有KUnit vocabulary、ordinary stack
owner source没有validation `cfg`、host facade未进入kernel dependency、probe仍带Stage 3退出条件。whitespace与
mdBook通过后追加transaction/biweekly closure；LA64 runtime、SMP>1、remote external traffic、UDP syscall/LTP、
hardware和final harness继续Not Run。本checkpoint关闭后仍停在Stage 3 Outline，不自动运行`2 -> 3`resolution。

**Closure：** kernel `kunit`现只转发artifact-neutral `udp-validation-probe`；stack crate的feature、module、type和
operation均不再理解KUnit，`anemone-net-api`仍无validation feature。独立`net/kunit.rs`已删除，三项测试进入
`net/mod.rs::kunits`；owner-local conditional support集中在对应owner文件底部。长期`host_validation.rs`未进入
kernel dependency，临时probe明确以Stage 3 real Socket-to-Endpoint consumer为删除gate。

最终stack/net-api host gate、两种no-default gate、probe feature check、60项xtask、双架构release build与
fresh-disk RV64 wrapper均通过；RV64实际执行270/270 KUnit、三项local-path测试、LTP whitelist 4/4与strict
`filesystem -> network -> device -> PowerOff`。RV64 build首次在sandbox内遇到既有`lwext4` `SIGSYS`，相同命令
在sandbox外通过。source/dependency audit、authored formatting、whitespace与mdBook gate通过；formatter整命令只
保留三处未触及vendored smoltcp baseline。LA64 runtime、SMP>1、remote external traffic、UDP syscall/LTP、
hardware与final harness继续Not Run。`NET-BOUNDARY-001`为Preserve，R0 revision、Stage 2 runtime closure与其它
contract状态不变；本checkpoint已Closed，并继续停在Stage 3 Outline。

### 6.3 Stage 3 Closed — Endpoint/socket nonblocking vertical slice

**阶段成熟度与授权：** Checkpoint 3A-3C Closed。2026-07-30独立
`2 -> 3 Implementation Resolution Gate`已完成；
本节冻结Stage 3完整路线、三个checkpoint、ABI/lifecycle边界、验证、停止/退出条件与Resolved Write Set Manifest。
Checkpoint 3A随后由独立授权完成；3B implementation/validation、获批correction、capacity Route Correction与
final exact-diff review也已独立关闭。3C随后完成implementation、correctness repair、validation与review并关闭
Stage 3；Stage 4 resolution与current-contract cutover均未授权。

#### 6.3.1 Resolution baseline 与阶段结果

进入resolution时为`dev/drc/alpha@2e083891771c`，tracked/untracked worktree clean。Stage 0-2及2026-07-30
post-close validation-boundary correction均Closed；R0 target、current Network/Opened-description/IOMUX/Epoll/
SystemTarget contracts与register没有漂移，也没有替代本stage或允许绕过final-release/copy/owner边界的active issue。

live source审计得到以下直接输入：

- `anemone-smoltcp-stack/src/udp.rs`同时承载Endpoint、provisional bind truth、per-interface engine projection、TX
  phase、RX queue与namespace；`stack/udp_ops.rs`组合Stack/interface operation。继续把真实bind/port0与datagram
  transaction塞入这两个文件会混合single-resource、namespace和handoff职责；
- kernel `net/mod.rs`与`net/domain/stack.rs`已经拥有initial-domain composition、唯一raw `Stack` access window和
  pump capability，但没有真实Endpoint consumer；`udp-validation-probe`仍是Stage 2缺少该consumer时的临时桥，
  kernel Cargo的`kunit` feature仍向该stack feature转发；
- `ProcFile::description_refs`、创建时固定的`FileDescOps::final_release`、`File::prv`、anonymous File、fd
  reservation/commit与opened-description status flags已经提供所需lifecycle/fd owner；Stage 3不另建VFS close
  framework，也不修改semantic final-release truth；
- kernel尚未注册五项network syscall；`anemone-abi`已具有所需Linux errno常量，但没有network layout/constants或
  asm-generic syscall number，`anemone-rs`也没有network raw/typed wrapper；
- current control plane只需增加operation-local UDP selection入口；route/address/interface truth仍由
  `Ipv4ControlPlane`拥有，`DomainStack`只执行projection revalidation与protocol admission。

Stage 3按三个独立checkpoint执行：3A先做same-owner split；3B建立Endpoint/File association与
`socket/bind/getsockname`真实地址/lifecycle纵切；3C增加`sendto/recvfrom`、implicit bind与nonblocking datagram
纵切。每个checkpoint独立review、验证、关闭并停止；不得把3A/3B closure当作下一checkpoint activation。

#### 6.3.2 Capability 与 module boundary

本stage不引入`ProtocolStack`、`EndpointOps`、TCP/UDP generic Socket或catch-all backend trait。trait不能阻止已经
取得concrete `Stack`的caller调用其其它public method；真实object fence由现有private instance ownership建立：

```text
DomainStack private raw Stack
  -> kernel-private UdpEndpointPort
    -> File::prv中的 UdpSocketFile
      -> fs/api/socket Linux ABI projection
```

`DomainStack`之外没有raw `Stack`实例或private lock。`UdpEndpointPort`只携带`Arc<DomainStack>`与opaque、boot-local、
不复用的`UdpEndpointId`，提供create后Endpoint所需的bind/query/send/detach/retire窄操作；它不暴露smoltcp handle、
SocketSet、engine/queue identity、route table或Stack其它operation。capability clone只形成operation-local access，
不延长Endpoint semantic lifecycle；retire后old clone通过ID lookup fail closed。

`anemone-net-api::udp`承载真实stack/kernel consumer共同需要的最小protocol-domain value/outcome：opaque
`UdpEndpointId`、local binding、peer endpoint、egress selection、owned received datagram与typed create/bind/send/
receive/retire outcome。它可以为owned datagram引入`alloc`，但不包含Linux sockaddr/errno/fd/task、control-plane
policy、smoltcp type或trait。若实现审查证明某个candidate value只在`DomainStack`内部流动，应留在stack crate而不
扩大shared surface；不得反向把整个concrete Stack或kernel facade搬入`anemone-net-api`。

最终module shape冻结为：

```text
anemone-kernel/crates/anemone-smoltcp-stack/src/
  udp/{mod.rs,endpoint.rs,namespace.rs,datagram.rs}
  stack/{mod.rs,udp.rs,host_validation.rs}

anemone-kernel/src/net/
  udp.rs
  domain/stack/{mod.rs,udp.rs}

anemone-kernel/src/fs/
  socket/{mod.rs,udp.rs}
  api/socket/{mod.rs,abi.rs,socket.rs,bind.rs,getsockname.rs,sendto.rs,recvfrom.rs}

anemone-abi/src/net.rs  # inline native/linux modules; native remains empty
anemone-rs/src/
  sys/{linux/{mod.rs,fs.rs,time.rs,process/{mod.rs,signal.rs},net.rs},
       anemone/{mod.rs,debug.rs,power.rs}}
  os/{linux/{mod.rs,fs.rs,tty.rs,time.rs,process/{mod.rs,signal.rs},net.rs},
      anemone/{mod.rs,debug.rs,power.rs}}
anemone-apps/udp-test/{Cargo.toml,Cargo.lock,app.toml,src/main.rs}
```

职责边界如下：

- `udp/endpoint.rs`：单Endpoint resource、committed binding、per-interface private engine projection、bounded TX/RX；
- `udp/namespace.rs`：`UdpEndpoints`、ID/capacity、完整bind conflict、ephemeral allocation、publish/withdraw/retire；
- `udp/datagram.rs`：pending-send phase、send ownership commit、owned receive transaction；
- `stack/udp.rs`：唯一把Endpoint owner与interfaces/SocketSet/local link组合起来的concrete Stack operation；
- `net/domain/stack/udp.rs`：同一DomainStack owner下的raw Stack UDP access window；
- kernel `net/udp.rs`：control-plane point-in-time selection与DomainStack operation composition，并向Socket只导出
  `UdpEndpointPort`；
- `fs/socket/udp.rs`：`UdpSocketFile`、anonymous FileOps、association与final-release hook；
- `fs/api/socket/abi.rs`：byte-level sockaddr、flag、user-copy ordering与typed outcome到`SysError`映射；五个
  syscall各自使用同名文件，只组合ABI helper和Socket operation，不复制解析规则；
- `anemone-abi::net::linux`：在单一`net.rs`中唯一承载Linux network UAPI layout/constant；同文件保留空
  `native` module以显式表达两类ABI identity，不在`net`根部提供flat compatibility re-export；`anemone-rs`保留
  raw `sys::linux::net`与typed `os::linux::net`两个不同层次，现有其它公开inline domain module全部物理拆分但
  保持Rust public path不变。

`fs/socket/mod.rs`不得因只有UDP一个consumer就建立generic Socket manager、protocol registry、option framework或
trait hierarchy；它只做module wiring与“是否为本stage UDP Socket”的窄分类。

#### 6.3.3 Endpoint、binding 与 capacity

Endpoint创建时未绑定，建立必要的private engine resources但不发布port。committed binding是Endpoint内唯一
`Option<address constraint + nonzero port>`；per-interface engine bind只是private projection。namespace不建立第二份
`BindingTable`，第一版在bounded Endpoint集合上扫描committed binding完成冲突判断：wildcard与任一同port binding
冲突，相同specific address冲突，不同specific address允许共用port。以后只有性能证据出现时才能把index作为带
一致性assert的derived structure加入，不得让它成为并列reservation truth。

explicit bind在唯一Stack access window内一次完成conflict check、port0 selection/reservation、engine projection与
binding commit；commit前不向kernel/userspace发布port。新增interface根据Endpoint current binding建立projection，
未绑定Endpoint保持engine unbound。retire先withdraw Endpoint identity/binding reservation，再清理engine/local-link
resource；ID不复用，因此本stage不引入generation table。普通Endpoint capacity、port range exhaustion、conflict和
queue full返回typed outcome，不panic或依赖allocator OOM。

implicit bind使用同一wildcard/ephemeral transaction。与Linux IPv4 UDP顺序一致，本stage选择“一旦implicit binding
commit便保留”：后续同一次`sendto`若因route、MTU或capacity失败，`getsockname`仍可观察已提交的wildcard/port；
只有binding commit前的allocation/conflict failure不发布port。一次send source selection始终是operation-local
result，不写回wildcard binding。

Stage 3新增并由现有KernelConfig owner生成以下参数：

| Parameter | R0 Stage 3 default | 约束 |
| --- | ---: | --- |
| `net_udp_endpoint_capacity` | 64 | nonzero；达到上限返回normal resource outcome |
| `net_udp_tx_datagram_capacity` | 8 | per Endpoint；nonzero |
| `net_udp_rx_datagram_capacity` | 64 | per Endpoint；nonzero |
| `net_udp_max_payload_bytes` | 1472 | `1..=65507`；实际send上限仍取本值与selected interface MTU ceiling最小值 |
| `net_udp_ephemeral_port_first` | 32768 | `1..=65535` |
| `net_udp_ephemeral_port_last` | 60999 | `first <= last <= 65535` |

`conf/.defconfig`保存default；xtask config owner只负责typed TOML parsing、default materialization与
`kconfig_defs.rs`常量生成，不验收UDP参数的nonzero、protocol range、cross-field或storage arithmetic。kernel
compile在消费generated constants处以常开static assertions唯一验收这些语义；TX/RX count与payload的直接乘积
还必须进入contiguous byte-storage domain。private
`Endpoint`、`VecDeque`、smoltcp metadata与`SocketSet`的元素尺寸和allocator growth不是KernelConfig contract；它们
继续服从R0明确的ordinary infallible-allocation/OOM边界，不通过cross-crate layout predicate反向塑造配置或owner
surface。物理上不可满足的trusted build configuration不获得allocation成功保证，也不等同于用户运行时的normal
capacity exhaustion；generated file只能由repository build入口更新，不能手改。
Stage 3不引入random allocator或可配置scan policy；从last回绕到first的单调scan是owner-local implementation
preference，不能改变冲突矩阵或exhaustion outcome。

#### 6.3.4 Socket/File lifecycle 与锁边界

`File::prv`安装一个`UdpSocketFile`：

```text
association: SpinLock<Option<UdpEndpointPort>>
operation: Mutex<()>
```

association只拥有opaque capability，不缓存binding、port、route、queue、capacity或Linux errno。operation mutex
串行化同一opened description的bind/name/send/receive transaction；它不是Endpoint state owner。每次operation在
association lock内取得短期capability clone后立即释放该lock，再进入control-plane/Stack operation。Stack mutation
继续只在DomainStack private spin lock的有限non-sleeping window内发生。

`socket`按`fd reservation -> Endpoint create -> anonymous File/UdpSocketFile -> ProcFile/FileDesc -> fd commit`
准备。fd commit前由linear creation guard拥有Endpoint并在任一步失败时non-blocking retire；commit后guard disarm，
只有`ProcFile::description_refs`首次`Live(1) -> Retired`调用创建时固定的final-release hook。`SOCK_CLOEXEC`只设置
新fd的fd-local flag，`SOCK_NONBLOCK`设置opened-description共享status flag；dup/fork不复制Socket/Endpoint。

final-release先在association lock内`take()`撤销Socket association，释放lock后调用non-sleeping Endpoint retire；
它不取operation mutex、不等待worker/progression、不以`Drop`或`Arc` last drop替代semantic event。Stage 3证明
creation rollback、ordinary dup/fork sharing、one-alias close、final close和stale ID fail-closed基本路径；delayed
retire、close/operation interleaving、shutdown/cancellation与identity/port reuse race的完整hardening仍属于Stage 4。

user copy不得持Stack/control-plane/source private lock。receive在Stack lock内把队首datagram原子detach到owned
operation-local transaction并立即释放Stack lock；随后才执行可能fault的payload/peer/addrlen copy。Socket operation
mutex可以保持到当前syscall结束以固定同一description上receive顺序，但不能参与Endpoint liveness或final-release
decision。

#### 6.3.5 Linux ABI、copy order 与 temporary blocking bridge

`anemone-abi::net::linux`新增`AF_INET`、`SOCK_DGRAM`、`SOCK_NONBLOCK`、`SOCK_CLOEXEC`、`IPPROTO_UDP`、
`MSG_DONTWAIT`、`socklen_t`与16-byte `SockAddrIn`/`InAddr`，并以compile-time size/alignment/offset assertion固定布局；
port/address使用network byte order。RISC-V与LoongArch syscall number都按asm-generic固定为`socket=198`、
`bind=200`、`getsockname=204`、`sendto=206`、`recvfrom=207`。布局和入口参考
`xref:linux-6.6.32:include/uapi/linux/in.h#sockaddr_in`与
`xref:linux-6.6.32:include/uapi/asm-generic/unistd.h`；上游只证明Linux snapshot，不替代R0 target。

input sockaddr采用byte copy，不要求user pointer自然对齐：`addrlen < 16`或`addrlen > 128`返回`EINVAL`，合法长度
只读取前16 bytes；family不是`AF_INET`返回`EAFNOSUPPORT`。output遵循
`xref:linux-6.6.32:net/socket.c#move_addr_to_user`：先读取user `socklen_t`，复制`min(user_len, 16)` bytes，再把
actual length 16写回；读出的32-bit length若按kernel `int`解释为负则返回`EINVAL`。任一fault返回`EFAULT`并保留
此前已经发生的user-memory effect。unbound UDP Socket的`getsockname`返回`0.0.0.0:0`；binding已经commit后再次
`bind`返回`EINVAL`，不改变原binding。

`recvfrom`遵循`xref:linux-6.6.32:net/socket.c#__sys_recvfrom`的可见顺序并受R0更强consume boundary约束：先detach
whole datagram，再复制payload prefix，然后在non-null peer pointer时执行peer sockaddr与actual addrlen copy。
payload、peer或addrlen任一fault都消费当前datagram；payload fault后不继续peer copy，peer/addrlen fault可能发生在
payload已经可见之后。null peer pointer不读取addrlen pointer。zero-length datagram和zero receive length仍按同一
detach boundary消费；short buffer返回copied prefix length而不是original datagram length。

Linux-visible outcome在Stage 3固定为：

| 情形 | errno/result |
| --- | --- |
| unsupported family/type/protocol | `EAFNOSUPPORT` / `ESOCKTNOSUPPORT` / `EPROTONOSUPPORT` |
| unknown socket type bits | `EINVAL` |
| unsupported `sendto/recvfrom` flags | `EOPNOTSUPP`并输出可诊断notice |
| non-socket fd | `ENOTSOCK` |
| invalid sockaddr length/family/address/zero destination port | `EINVAL`或上述family error |
| explicit local address不属于domain | `EADDRNOTAVAIL` |
| bind conflict | `EADDRINUSE` |
| implicit ephemeral range exhaustion | `EAGAIN` |
| no destination / no route | `EDESTADDRREQ` / `ENETUNREACH` |
| supported payload上限外 | `EMSGSIZE` |
| ordinary Endpoint resource exhaustion | `ENOBUFS`；若是当前nonblocking TX/RX not-ready则`EAGAIN` |
| invalid user pointer | `EFAULT` |

kernel `SysError`增加对应精确variant并映射既有`anemone-abi::errno`；不得把socket outcome压成宽泛
`EINVAL`/`ENOSPC`，也不得让Stack接收Linux errno。

Stage 3只实现立即尝试：effective nonblocking是opened-description `O_NONBLOCK`或per-call `MSG_DONTWAIT`的OR，
后者不修改status flags。default-blocking operation若立即可完成仍成功；若必须sleep，temporary bridge返回
`EOPNOTSUPP`并输出一次受控notice，注释明确以Stage 4 blocking/wait接入为删除gate。nonblocking would-block返回
`EAGAIN`。不得busy-poll、睡眠或把default-blocking would-block静默伪装成`EAGAIN`。

`anemone-rs::sys::linux::net`提供raw六参数syscall wrapper；`os::linux::net`提供typed IPv4 UDP wrapper，同时保留
最窄unsafe raw pointer/length/flag入口给ABI conformance/fault测试。`udp-test`是两层API的真实consumer，测试不得
手写另一套syscall number或sockaddr layout。`socket`只接受`AF_INET + SOCK_DGRAM`，protocol为0或
`IPPROTO_UDP`；type可附加`SOCK_NONBLOCK|SOCK_CLOEXEC`，不支持的base type/protocol/type bit按上表拒绝。

#### 6.3.6 Checkpoint 3A — Closed / same-owner module split only

**目的：** 在增加真实consumer、shared API或ABI前，行为保持地拆开已经混合职责的protocol/domain files。

**交付：** `udp.rs`目录化为`udp/{mod,endpoint,namespace,datagram}.rs`，`stack/udp_ops.rs`改为
`stack/udp.rs`；kernel `net/domain/stack.rs`目录化为`stack/{mod,udp}.rs`，但3A的`stack/udp.rs`只承接既有
probe operation access window。ordinary visibility、conditional probe/host facade、public symbol、capacity值与
所有call path保持不变；不得趁拆分实现unbound/port0/syscall或删除probe。

**验证/退出：** stack完整host tests、两种no-default gate、formatter，以及精确的
`cargo check -p anemone-smoltcp-stack --no-default-features --features udp-validation-probe`必须保持baseline；kernel
双架构release build使用6.3.9列出的两个`just build`命令。source audit证明只有same-owner movement且kernel仍没有
real Endpoint consumer。Contract Impact为None。3A单独Closed后停止，3B仍Not Active。

**Closure：** 2026-07-30，protocol UDP按endpoint/namespace/datagram职责目录化，Stack UDP operation与
DomainStack KUnit-only probe window分别进入同owner child module。item body、ordinary/public visibility、capacity、
conditional probe/host facade与consumer call path保持不变；host/no-default/probe、xtask、formatter、双架构release
build、whitespace、mdBook与独立review通过。Contract Impact为None，Checkpoint 3A独立Closed；Checkpoint 3B-3C
保持Ready / Not Active。精确source、validation、review与Not Run边界见
[transaction](../../devlog/transactions/2026-07-29-net-udp.md#2026-07-30---stage-3-checkpoint-3a-final-review-and-closure)。

#### 6.3.7 Checkpoint 3B — Closed / Endpoint/File/address lifecycle vertical slice

**目的：** 一次建立真实Endpoint capability、Socket/File association与`socket/bind/getsockname`用户纵切，不把
datagram或blocking假装已经完成。

**交付：** 引入6.3.2最小shared values；Endpoint从unbound创建，完成full conflict matrix、specific/wildcard、
port0、query与retire；增加`net::udp::UdpEndpointPort`和`UdpSocketFile`；注册`socket/bind/getsockname`，接入
fd/status flags、creation rollback、dup/fork sharing与semantic final release。新增Kconfig参数和双架构
`anemone-abi`/`anemone-rs` network surface，建立`udp-test`并接入两份pretest rootfs和`user-test`。

real `UdpEndpointPort`出现后，必须删除stack Cargo的`udp-validation-probe` feature、kernel Cargo中`kunit`向该
feature的forwarding edge、`stack/udp_probe.rs`与DomainStack probe support；kernel `kunit` feature本身继续服务
owner-local KUnit。现有三项local-path KUnit改走real network capability或由更强`udp-test` case替代，不能把probe
重命名为production API。长期`host_validation.rs`只保留仍有真实host-test consumer的deterministic control/
observation；其ordinary operation必须调用与production相同的Stack path。

**验证/退出：** deterministic host matrix覆盖64-capacity boundary、完整bind conflict表、port0 scan/exhaustion、
failed bind rollback、retire/port reuse与stale ID；kernel KUnit覆盖File association和creation guard；RV64 fresh-disk
`udp-test`覆盖family/type/protocol/flags、unbound name、wildcard/specific/port0/rebind/getsockname、
unaligned/truncated sockaddr、dup/fork/one-alias/final close和CLOEXEC/NONBLOCK projection，并输出稳定
`UDPTEST:SUMMARY`。LA64 app/kernel只做同源build，
runtime仍Not Run。3B不注册send/receive、不执行contract cutover；关闭后停止，3C仍Not Active。

**Implementation / validation candidate：** 2026-07-30，3B建立Stack-owned unbound Endpoint、bounded
64-endpoint namespace、完整
specific/wildcard conflict、deterministic port0、query/retire与non-reused identity；kernel-private
`UdpEndpointPort`只投影到anonymous `UdpSocketFile`，creation guard在fd publication前rollback，semantic final
release先撤销association再non-blocking retire。`socket/bind/getsockname`按16-byte IPv4 sockaddr、unaligned input、
Linux prefix-copy/actual-length顺序与精确errno运行；CLOEXEC保持fd-local，NONBLOCK保持opened-description-local。

temporary `udp-validation-probe` feature/module/forwarding与DomainStack support已删除；host matrix、两项File KUnit、
62项xtask、双架构app/kernel build和fresh-disk RV64 `UDPTEST:SUMMARY:PASS:6`全部通过。RV64实际执行269/269 KUnit、
LTP whitelist 4/4并严格按`filesystem -> network -> device -> PowerOff`关机。LA64 runtime及6.3.9其余Not Run范围
没有外推。

**Review hold：** final exact-diff review分类为Apollyon 2、Keter 1、Euclid 0、Safe 0：Kconfig没有覆盖actual
allocation layout与count上界；anonymous UDP socket向`fstat`/`statx`投影`S_IFREG`而非`S_IFSOCK`；namespace-wide
Endpoint capacity与ephemeral range仍由每次operation传入，没有单一policy owner。`S_IFSOCK`修正至少需要扩张到
manifest外的VFS `InodeType` owner及其exhaustive consumers，命中6.3.10停止条件。现有runtime证据有效但不覆盖
这三项defect。Contract Impact仍为None，全部candidate继续Pending；Checkpoint 3B保持Review Hold，3C仍Ready /
Not Active。精确finding、证据与停止边界见
[transaction](../../devlog/transactions/2026-07-29-net-udp.md#2026-07-30---stage-3-checkpoint-3b-final-review-and-stop)。

**Correction authorization：** 2026-07-30开发者明确回复`approved`。3B correction因此只按第15节冻结的扩展继续：
kernel compilation唯一验收全部UDP参数语义与direct storage arithmetic；`UdpEndpoints`在Stack构造时一次拥有
Endpoint capacity和ephemeral range，create/bind caller不再传入namespace policy；anonymous UDP inode投影
`S_IFSOCK`，五个exhaustive VFS consumer显式映射或拒绝Socket。该授权不改变anonymous/opened-description
lifecycle owner，不修改current contract，不进入3C；全部finding清零、受影响validation和独立exact-diff复审完成前
3B继续保持Review Hold。

后续backend admission复核中，开发者指出ramfs等backend是否进入写集应由实际需求决定。live source确认ramfs/
proc创建入口只传固定类型；devfs的public `DevfsNodeAttr.ty`与“leaf只要不是Dir”负向检查则会因新增枚举而自动接纳
Socket。第15节因此只再加入`fs/devfs/mod.rs`，在publish owner入口显式返回`NotSupported`并以owner-local KUnit
锁定；ramfs/proc继续只读。该修正仍只落实anonymous-only Socket边界，不扩大owner、ABI、contract或acceptance。

后续capacity model复核确认，穷尽private container element layout和模拟`Vec` amortized growth不是R0的supported
bound，也不能证明物理allocation成功。该类failure要求开发者先构建物理上不可满足的KernelConfig，普通用户程序
不能修改这些参数；它不属于normal bounded-resource exhaustion。原cross-crate layout predicate因此删除，kernel
只保留上述semantic/direct-byte assertions；Endpoint collection与external/local `SocketSet`都保持按实际publication
lazy growth，不传播namespace capacity或增加eager allocation。该Route Correction不改变normal capacity outcome、
target、owner、ABI、Contract Impact或3C状态。

**Closure：** closure source完成namespace policy single-owner、anonymous Socket inode与backend admission correction，
并删除为穷尽private layout而引入的predicate/eager reserve。62/62 xtask、完整host/no-default、双架构app/kernel build、
invalid-config kernel compile-fail matrix、formatter、whitespace与mdBook均PASS；correction-source fresh-disk RV64执行271/271 KUnit、
`UDPTEST:SUMMARY:PASS:7`与`EPOLLTEST:SUMMARY:PASS:11`，关机保持
`filesystem -> network -> device -> PowerOff`。最后的`Vec::with_capacity -> Vec::new`只恢复Endpoint collection
lazy growth，明显不改变已验证的publication/capacity/ABI path，因此按开发者指示不重复QEMU。独立final review为
Apollyon/Keter/Euclid/Safe全0。musl LTP未形成完整aggregate，不宣称本轮4/4；它也不是3B自身的UDPTEST退出条件。
Contract Impact为None，全部candidate继续Pending。Checkpoint 3B独立Closed并停止，3C仍Ready / Not Active。

##### Post-3B module-boundary correction — Closed

2026-07-30完成3B后的周期工程审计确认两个Keter：kernel `fs/api/socket/address.rs`同时承载`bind`与
`getsockname`，且3C原计划继续把`sendto/recvfrom`合入`datagram.rs`，违反kernel一syscall一文件的既有形状；
`anemone-abi::net`根部全部是Linux UAPI，却没有以`linux` module标明compatibility boundary。审计同时确认
`anemone-rs::sys::linux::net`与`os::linux::net`分别是raw syscall和typed OS wrapper，不应合并，但父级与其它
公开inline module应目录化以清楚表达层次。

开发者明确批准在3B Closed、3C Ready / Not Active之间执行一个独立behavior-preserving纠偏checkpoint，并要求
一并物理拆分`anemone-rs::{sys,os}::{linux,anemone}`下全部现有公开inline module。该checkpoint不重开3B、不实现
3C、不改变syscall number/layout/errno、Socket/Endpoint owner、opened-description lifecycle、current contract或
acceptance；唯一public Rust source-path cutover是`anemone_abi::net::* -> anemone_abi::net::linux::*`，workspace内
direct consumers原子更新且不保留flat compatibility re-export。`anemone-abi/src/net.rs`保持单文件，在其中inline
空`native`与承载现有UAPI的`linux`，不目录化；获批public Rust API delta仅为该空module identity与strict
source-path cutover。验证至少覆盖repository formatter、workspace source/API audit、
`udp-test`双架构app build、RV64/LA64 kernel build、whitespace与mdBook；最终change review与write-set audit清零
后独立关闭并停止，3C继续Not Active。

**Closure：** 2026-07-30，kernel Socket API已按`socket`/`bind`/`getsockname`一syscall一文件重排，未创建
`sendto.rs`/`recvfrom.rs`；单文件`anemone-abi/src/net.rs`现inline空`native`与Linux UAPI `linux`，workspace
direct consumers完成strict cutover且flat路径清零；`anemone-rs`四个parent目录化，`fs`/`tty`/`time`/`process`/
`signal`/`debug`/`power`全部成为同owner child file。13项拆分前后module-body逐字对照、结构/API/write-set audit、
kernel与`udp-test` formatter check、双架构`udp-test` app build、RV64/LA64 release kernel build、whitespace与mdBook
均PASS；RV64首次sandbox build的`lwext4` `Bad system call`由完全相同命令在sandbox外PASS证明为环境SIGSYS。
final change review为Apollyon/Keter/Euclid/Safe全0。QEMU、KUnit runtime、UDPTEST/LTP、hardware与final harness
均Not Run且不属于本behavior-preserving checkpoint退出条件。Contract Impact为None，existing Effective IDs
Preserve，全部Socket/Endpoint/UDP/wait candidate继续Pending；checkpoint独立Closed并停止，3C仍Ready / Not Active。

#### 6.3.8 Checkpoint 3C — Closed / nonblocking datagram vertical slice

**目的：** 在3B真实lifecycle上增加send/receive ownership transaction，形成五项syscall的可运行nonblocking闭环。

**交付：** 注册`sendto/recvfrom`；implicit wildcard bind、control-plane selection、Stack revalidation、MTU/TX
admission与ownership commit使用6.3.3边界；receive返回owned transaction并按6.3.5顺序copyout。loopback与
self-external local address均经过protocol egress、bounded local link和normal ingress；kernel不得直接查目标Socket
或复制payload。host fixture增加selected external-interface egress/ingress、TX/RX capacity和failure-before-success
证据，但remote external QEMU peer仍属于Stage 5。

**验证/退出：** host matrix覆盖wrong-interface-first、no route/source/interface、oversize、TX/RX full/recovery、
implicit port exhaustion、zero/short datagram、owned detach后abandon不重新入队与two-receiver deterministic order；
RV64 fresh-disk `udp-test`必须完成server `bind(port0)->getsockname`、client implicit-bind send、server recv/reply、
client recv，以及loopback/self-external、empty nonblock `EAGAIN`、blocking temporary `EOPNOTSUPP`、zero/short、
payload/peer/addrlen fault consume和precise errno/flag case。Stage 3只建立这些ordinary paths；concurrent close/
receive stress、wait/readiness与fragment gate仍由Stage 4解析。

**Closure：** `sendto/recvfrom`、shared typed outcome、Stack send/receive operation与typed `anemone-rs` wrapper已在
3B真实lifecycle上形成ordinary nonblocking纵切。File operation guard串行bind/name/send/receive；send在同一guard下
先提交implicit wildcard binding再执行bounded copyin与Stack route/source/interface/MTU/TX revalidation，receive在
Stack锁内detach whole datagram后释放protocol owner，再按payload/peer/addrlen顺序copyout。local loopback与
self-external均经protocol egress、bounded local link与normal ingress，不存在Socket fast path。

初次独立source review得到Apollyon 3：aggregate RX credit恢复后缺少durable progression、fresh oversize被ABI
precheck在implicit bind前拒绝、specific noncanonical `127/8` bind与source selection predicate不一致。修复保持原
owner/write set：Stack detach同步refill engine datagram；`UdpSendOperation`保持File guard并在copyin前完成persistent
implicit bind；control plane统一使用`Ipv4Address::is_loopback()`。focused host recovery与新增fresh oversize retention/
specific loopback runtime case覆盖三项repair；最终复审清零后本checkpoint独立Closed。

#### 6.3.9 Validation、review 与 claim boundary

每个checkpoint在自己的final source上执行适用子集；Stage 3 closure必须在3C exact code上执行完整适用集合：

```text
just xtask-test
# For each workspace-relative temporary invalid KernelConfig, the repository
# build must pass xtask parsing/generation and fail in kernel static assertions.
cargo test -p anemone-net-api -p anemone-smoltcp-stack
cargo test -p anemone-smoltcp-stack --no-default-features --no-run
cargo check -p anemone-smoltcp-stack --no-default-features
just fmt kernel --check
just fmt udp-test --check
just app build --arch riscv64 udp-test
just app build --arch loongarch64 udp-test
just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G
just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G
./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/net-udp-stage3-rv64.log
mdbook build docs
git diff --check
```

3B correction的invalid-config compile-fail matrix覆盖zero、payload protocol range、ephemeral cross-field以及
direct payload-storage arithmetic；至少包含TX/RX maximum TOML count与payload-two case。maximum count配
payload-one仍是物理上不可满足的trusted configuration，private metadata allocation是否先触发layout/allocator
failure不在R0保证内；不应为逼迫该case compile-fail而重新耦合private metadata layout。
这些case不得在xtask semantic validation中失败；每次失败必须定位到kernel compilation，随后用canonical config
恢复generated input并完成正常双架构build。

3C没有修改KernelConfig schema、generated values、kernel static assertion或配置consumer；按开发者明确决定，
invalid-KernelConfig matrix在本checkpoint记为**Not Re-run / unchanged owner**，只沿用3B final source已经记录的
compile-fail evidence，不宣称3C重跑。其它final-source gate不因此降低。

3C final source执行62/62 xtask、完整host matrix（`udp_topology` 9/9）、两项no-default gate、kernel/`udp-test`
formatter、双架构`udp-test` app与release kernel build、RV64 fresh-disk wrapper、whitespace与mdBook。RV64执行
271/271 KUnit、`UDPTEST:SUMMARY:PASS:13`、`EPOLLTEST:SUMMARY:PASS:11`、LTP whitelist 4/4与strict
`filesystem -> network -> device -> PowerOff`。sandbox内unchanged `lwext4`的`Bad system call`由相同RV64 build在
sandbox外PASS归类为环境限制。

host crate tests是protocol deterministic proof，app build只证明architecture-specific compile/export，kernel build只
证明integration；它们都不能替代fresh-disk RV64 syscall/fd/copy/lifecycle runtime。RV64 wrapper会重建pretest rootfs、
覆盖worktree-local runtime disk并运行QEMU，必须由独立checkpoint授权后才执行。LA64 runtime、remote external peer、
SMP>1、poll/select/epoll、blocking/signal、fragment injection、hardware、network LTP与final harness继续明确Not Run，
不得由Stage 3结果外推。

每个checkpoint closure需要source/write-set audit和Apollyon/Keter/Euclid review。Stage 3 final review特别检查：

- raw Stack/private lock未逃逸，kernel没有smoltcp handle/queue truth，`anemone-net-api`没有Linux/trait pollution；
- binding/port只在Endpoint namespace，Socket/control plane没有mirror，implicit bind失败/保留语义与getsockname一致；
- fd-local/opened-description flags、creation guard、final-release trigger和probe删除满足lifecycle边界；
- send success前完成selection/admission，local path没有Socket fast path，receive detach先于全部copyout且fault不requeue；
- generated Kconfig definition来自repository command，rootfs/app architecture与wrapper一致；
- unsupported input和temporary blocking bridge有typed errno、受控notice与Stage 4删除注释，diagnostic字段/log不驱动状态。

#### 6.3.10 Contract Impact、停止条件与退出

Stage 3的current-contract cutover为**None**。`OPENED-DESC-001..003`、`NET-BOUNDARY-001`、
`NET-IFACE-DOMAIN-001`、`NET-CONTROL-PLANE-001`及existing frame/pump IDs保持Effective/Preserve；
`NET-PROTOCOL-BOUNDARY-001`、`NET-SOCKET-ENDPOINT-001`、`NET-UDP-TRANSACTION-001`与
`NET-SOCKET-WAIT-001`继续Pending。原因是Stage 3只证明ordinary nonblocking/lifecycle纵切，尚未达到
`NET-SOCKET-ENDPOINT-001`要求的delayed retire/shutdown/cancellation/race floor，也未达到
`NET-UDP-TRANSACTION-001`的fragment、concurrent receive、双架构和remote-external evidence floor。partial code、
syscall registration或RV64 PASS都不使这些ID生效。

以下任一情况必须停止当前checkpoint并写入transaction；不得用临时wrapper或宽errno继续：

- 需要修改`task/files.rs`、`fs/file.rs`、anonymous/VFS lifecycle、iomux/epoll source或opened-description current
  contract才能完成3B/3C；先报告owner/write-set/contract expansion；
- concrete route要求vendored smoltcp change、第二binding/route/queue truth、Socket-to-Socket delivery或让Socket取得
  raw Stack；保持partial code disposition并进入Route Correction或Target Renegotiation Gate；
- implicit bind、copy ordering、blocking bridge或errno无法满足本节已冻结可见语义；这属于ABI/target反馈，不得
  静默调整；
- ordinary capacity exhaustion panic/busy-spin、final release等待worker/锁sleep、user copy跨Stack/source lock、
  retire后old capability恢复publication，或Apollyon/Keter未清零；
- 需要编辑本节manifest外source。先上报理由、拟新增路径、owner/contract影响与验证，再批准并更新manifest。

3A、3B、3C现已全部独立Closed，最终source通过6.3.9 review/validation且stop condition未触发，Stage 3 Closed。
closure当时只授权transaction/RFC/biweekly状态写回；Stage 4当时保持Outline，`3 -> 4`resolution与任何candidate
contract cutover均须新的明确授权，Stage 3 closure本身没有执行。

#### 6.3.11 Resolved Write Set Manifest

以下是整个Stage 3允许触及的union；每个checkpoint只能使用上文属于自己的最小子集。目录pattern只覆盖列出的
planned files，不授权顺手修改相邻owner。

- configuration owner：`conf/.defconfig`、`scripts/xtask/src/config/kconfig.rs`，以及只由repository build生成的
  `anemone-kernel/src/kconfig_defs.rs`；
- shared protocol values：`anemone-kernel/crates/anemone-net-api/src/{lib.rs,udp.rs}`；
- protocol Stack：`anemone-kernel/crates/anemone-smoltcp-stack/Cargo.toml`、
  `src/{lib.rs,udp.rs,udp/{mod.rs,endpoint.rs,namespace.rs,datagram.rs},stack/{mod.rs,udp_ops.rs,udp.rs,
  udp_probe.rs,host_validation.rs}}`与`tests/udp_topology.rs`；其中`udp.rs`/`udp_ops.rs`允许删除，
  `udp_probe.rs`必须在3B删除；
- kernel feature/network composition：`anemone-kernel/Cargo.toml`、
  `anemone-kernel/src/net/{mod.rs,udp.rs,domain/{mod.rs,control_plane.rs,stack.rs,
  stack/{mod.rs,udp.rs}}}`；其中`domain/stack.rs`允许在3A目录化删除；
- Socket/File/syscall projection：`anemone-kernel/src/fs/{mod.rs,socket/{mod.rs,udp.rs},api/{mod.rs,
  socket/{mod.rs,abi.rs,socket.rs,bind.rs,getsockname.rs,sendto.rs,recvfrom.rs}}}`与
  `anemone-kernel/src/syserror.rs`；其中旧`create.rs`/`address.rs`允许删除，3C只能在独立授权后新增
  `sendto.rs`/`recvfrom.rs`；
- 3B correction获批VFS expansion：`anemone-kernel/src/fs/inode.rs`、
  `anemone-kernel/src/fs/api/getdents64.rs`、`anemone-kernel/src/fs/ext4/{mod.rs,inode.rs,superblock.rs}`与
  `anemone-kernel/src/fs/devfs/mod.rs`；只允许新增`InodeType::Socket`的Linux mode/dirent投影、ext4显式拒绝及
  devfs publish admission拒绝，不改变anonymous/VFS lifecycle；
- ABI/library：`anemone-abi/src/{lib.rs,net.rs,syscall/{riscv.rs,loongarch.rs}}`；`net.rs`保持单文件，只允许inline
  空`native`与承载现有Linux UAPI的`linux`并执行strict source-path cutover；这两项是本checkpoint仅有的
  public Rust API delta；`anemone-rs/src/{sys/{linux.rs,
  linux/{mod.rs,fs.rs,time.rs,
  process/{mod.rs,signal.rs},net.rs},anemone.rs,anemone/{mod.rs,debug.rs,power.rs}},
  os/{linux.rs,linux/{mod.rs,fs.rs,tty.rs,time.rs,process/{mod.rs,signal.rs},net.rs},anemone.rs,
  anemone/{mod.rs,debug.rs,power.rs}}}`，其中四个旧parent `.rs`允许目录化删除；
- real user consumer/rootfs：`anemone-apps/udp-test/{Cargo.toml,Cargo.lock,app.toml,src/main.rs}`、
  `anemone-apps/user-test/src/main.rs`、`conf/rootfs/{pretest-rv64.toml,pretest-la64.toml}`；
- execution write-back：本RFC`{index.md,implementation.md}`、本transaction、`docs/src/rfcs.md`、
  `docs/src/devlog/transactions/index.md`与当前biweekly devlog。

从Stage 3 activation baseline起，`invariants.md`、current contracts、register/current limitations、vendored smoltcp、
`task/files.rs`、`fs/file.rs`、上述六个文件以外的anonymous/VFS lifecycle与filesystem backend、iomux/epoll、
net worker/provider、SystemTarget/Platform/build preset、其它apps/rootfs、其它RFC与
scripts保持只读。formatter若产生允许的相邻style diff按repo规则审计，不将其解释为owner/write-set授权扩大。

### 6.4 Stage 4 Ready / Not Active — Blocking/iomux与datagram hardening

本节由Stage 3 Closed后的独立`3 -> 4 Implementation Resolution Gate`解析。**成熟度：Ready / Not Active**；
Checkpoint 4A、4B、4C均未激活。本次只冻结完整Stage 4交付、owner/handoff、review/validation、停止/退出条件和
Resolved Write Set Manifest，不修改source、current contract、R0 revision或acceptance boundary。

#### 6.4.1 前置条件与live baseline

- Stage 3已经在`0204ac74`关闭：anonymous `UdpSocketFile`、opaque `UdpEndpointPort`、semantic final release、
  nonblocking `sendto/recvfrom`与owned receive transaction均已形成；default-blocking would-block仍通过带删除条件的
  `EOPNOTSUPP` bridge停止，`FileOps::poll`仍是snapshot-only empty stub；
- Endpoint当前readable fact来自aggregate receive queue，writable fact来自live Endpoint是否能接受一个普通非空
  datagram进入Endpoint-owned bounded TX admission；provider backpressure只有通过占满该admission间接改变writable，
  kernel没有也不得新增provider/private-engine queue mirror；
- effective `OPENED-DESC-001..003`、`IOMUX-POLL-001..003`与`EPOLL-*`已经提供semantic final release、复数
  source-neutral `PollRoute`、snapshot/register/final-scan和epoll ordinary-source consumer；Stage 4 Preserve这些规则，
  不修改epoll watch/ready owner；
- shared `wait_for_iomux_ready()`当前私有位于`fs/api/iomux/wait.rs`并只服务`ppoll/pselect6`。Stage 4需要复用或
  下移其中source-neutral wait loop，不能在socket syscall复制第二套Latch/signal/final-recheck协议；
- vendored smoltcp已经在IPv4 representation parse、UDP header/Endpoint lookup之前显式拒绝`MF != 0`或
  `fragment offset != 0`；Stage 4首先用真实frame injection证明该production gate，不预先取得vendored write权限；
- activation前必须重新核对branch/HEAD、dirty state、live source、current contracts、register、4A manifest与本文
  anchor。drift若只影响实现偏好，先按6.4.3记录Route Correction；影响owner、ABI、contract或acceptance时停止。

#### 6.4.2 Resolved owner、predicate与handoff

Stage 4固定以下语义形状，不固定无真实摩擦证据的Rust拼写：

1. **Endpoint fact owner：** concrete Stack/Endpoint继续唯一拥有liveness、RX aggregate、TX admission phase、
   capacity与private engine fact。跨crate只增加一次性readiness facts snapshot和可合并的“Endpoint facts may have
   changed” invalidation；二者不携带Linux poll mask、errno、task、waiter或consumer callback truth。
2. **Kernel Socket/source owner：** Socket-owned source state一次拥有opaque association publication与复数
   `PollRoute` registry，File operation mutex只串行syscall operation。source依据当前Endpoint snapshot解释
   `READABLE/WRITABLE`，不得缓存第二份queue/capacity/error truth。reverse event route只把opaque Endpoint identity
   定位到non-owning Socket source capability；它不决定association、liveness或readiness。
3. **Event handoff：** Stack state transition只在Stack锁内更新事实并留下可合并recheck indication；DomainStack在
   释放Stack锁后取出/路由edge。kernel reverse registry取得non-owning source后，在source锁内选择route snapshot，
   释放锁后再notify/drop。Stack不保存`PollRoute`、task或waiter，source callback不进入Stack。
4. **Poll publication：** register request先fallibly准备复数route replacement，再在同一source-state临界区发布
   route并读取当前association；保持`source -> Stack`的短时只读snapshot顺序。Stack transition永不持Stack锁获取
   source锁，所以不存在反向嵌套。ready-at-register也保留route；stale route pruning、旧snapshot drop与所有notify
   都在source/Stack锁外。普通allocation failure形成typed registration failure，不能退化成单slot或睡在未armed source。
5. **Readiness projection：** readable当且仅当live Endpoint已有可detach datagram。ordinary writable当且仅当
   Endpoint live、未retire且能立即接纳至少一个非空、第一版支持范围内datagram进入自身bounded TX admission。
   unbound本身仍writable；route/source/interface、具体长度/oversize和provider queue不进入poll truth。Stage 4不新增
   ICMP/`SO_ERROR`/完整`POLLERR`语义，也不把socket-specific ET/ONESHOT扩展写成新target。
6. **Retire/cleanup：** semantic final release在source owner内先撤销association publication、反向event route和
   route registry，释放source锁后唤醒/丢弃detached routes，再发起non-blocking Endpoint retire。它不取得operation
   mutex、不等待waiter/worker/Stack progression；晚到event只能找不到retired source或命中已撤销publication后
   fail closed，不能把旧Endpoint fact发布给复用identity/port的新Socket。

这里的复数route不是4C压力测试才增加的扩展：4A交付的source和4B交付的blocking path从第一版就不得假设只有一个
waiter。一次edge可以提示全部当前interested routes；真正返回什么仍分别由每个consumer的final predicate scan决定。

#### 6.4.3 实现弹性与Route Correction

Ready冻结的是上述能力、owner、handoff、锁/cleanup和evidence floor，不把当前候选类型/文件内形状升级为target。
真实实现出现摩擦时，以下内容允许在保持R0的Route Correction中调整：一次性facts/event的具体类型名、pending edge
是bit/set/batch还是等价可合并表示、reverse registry与route snapshot的容器、helper的具体函数签名/落点、
`udp/{mod.rs,source.rs}`内部拆分，以及同一checkpoint内不改变交付/停止点的施工顺序。若调整仍在6.4.10 manifest内，
先更新本文并在transaction记录理由与验证变化；若需要新增文件/owner，先走write-set expansion。

以下不是可接受的“实现弹性”：

- 4A/4B只支持单waiter，等4C再扩成复数route或多blocking waiter；
- 为socket复制`wait_for_iomux_ready()`、直接使用裸Latch形成第二套wait loop，或用busy-poll/yield bridge；
- Socket缓存RX/TX/provider count、event payload直接决定poll mask/errno，或Stack保存task/waiter/`PollRoute`；
- 为绕开锁/通知摩擦改变source/Stack owner、建立第二association/readiness truth，或让final release等待operation mutex；
- 降低blocking/signal/copy/fragment可见语义、Stage 4 evidence floor或Stage 5 dual-architecture/external closure。

前述允许项的Route Correction不递增RFC修订；上述五项退化均不能以实现摩擦批准。任何owner、ABI/visible
semantics、contract或acceptance变化必须停止并进入RFC review / Target Renegotiation Gate。

#### 6.4.4 Checkpoint 4A — Complete plural-route Socket poll source

**目的：** 先形成可被poll/select/epoll与后续blocking syscall共同消费的完整ordinary socket source，而不是先写
一个single-waiter blocking adapter。

**交付：**

- 为shared UDP surface增加最小facts snapshot与opaque invalidation value；Stack在create/bind/send/receive/pump/
  capacity recovery/retire等所有可能改变predicate的transition上形成可合并edge，并由DomainStack在Stack锁外路由；
- 将当前聚合`fs/socket/udp.rs`按同一Socket owner目录化为`udp/{mod.rs,source.rs}`；source state拥有association
  publication、复数interest-bearing route registry和reverse-event registration，operation mutex仍只串行File operation；
- `FileOps::poll`完整实现snapshot/register。route publication/current facts遵守6.4.2；`READABLE/WRITABLE`只由
  current snapshot解释，unsupported interests不伪造ready；
- final release、creation rollback与event-route rollback覆盖publication-before/after failure；不留下强引用环、
  stale reverse entry或能够命中新Endpoint的old route；
- source/KUnit/host与普通用户态基础matrix至少覆盖empty/readable/writable snapshot、ready-at-register、
  register-then-transition、两个并存route均收到hint、一个route retire不影响另一个、late/duplicate edge、stale pruning，
  以及`ppoll`/`pselect6`/epoll对同一UDP source的ordinary LT重查。basic matrix不是socket-specific ET扩展承诺。

**停止/退出：** 若需要修改iomux/epoll contract或implementation才能让普通source工作、Stack必须持consumer callback、
只能保存一条route、route publication无法在predicate transition窗口内闭合，或final release需要等待operation/worker，
停止4A。完整source、复数route和基础poll/select/epoll evidence通过review/validation后，4A独立Closed并停止；4B仍须
新的明确授权。

#### 6.4.5 Checkpoint 4B — Blocking `sendto/recvfrom` on the same source

**目的：** 删除Stage 3 temporary blocking bridge，让blocking与nonblocking只在“是否等待”上分叉，不复制
predicate或wait protocol。

**交付：**

- 把现有source-neutral iomux wait loop移动到最低共同owner或窄化暴露给socket API；`ppoll/pselect6`继续走同一
  implementation。socket adapter只提供单source snapshot/register/final scan与operation retry，不复制Latch、signal、
  timeout或register-abort cleanup；
- 每轮`sendto/recvfrom`在File operation guard内尝试一次。`WouldBlock`时释放operation guard，再通过4A source
  等待；schedule期间不持operation/source/Stack锁。wake/late edge只触发final snapshot和重新取得operation guard后的
  operation retry；
- effective `O_NONBLOCK`（包括creation-time `SOCK_NONBLOCK`）与per-call `MSG_DONTWAIT`对同一operation/predicate
  立即返回`EAGAIN`，且per-call flag不修改opened-description status。default blocking才进入共享wait loop；
- blocking send保持implicit bind先提交、payload只copyin一次并由kernel transaction跨wait保存、每次重试重新执行
  current selection/admission。no route/source/interface、oversize和其它非`WouldBlock`失败立即返回，不因poll
  writable而等待或伪装成功；
- blocking receive在empty时释放operation guard等待；只有成功detach后才保持现有operation-local transaction跨
  payload/peer/addrlen copyout。fault仍消费datagram，等待重试不能detach两次或requeue；
- final predicate仍不满足时，signal/force按现有wait规则返回`EINTR`；第一版不使用仅适合无副作用operation的
  `RestartSyscall::Idempotent`。删除Stage 3 `EOPNOTSUPP` notice/temporary bridge及其注释；
- 最小deterministic multi-waiter coverage在本checkpoint完成：同一opened description上至少两个blocking receive
  waiter先同时park，一个datagram只允许一个detach，另一个继续等待并由后续datagram完成；同时覆盖一个waiter
  signal/cancel不撤销其它waiter的route。该能力不得推迟到4C。

**停止/退出：** operation/source/Stack任一锁必须跨schedule、wait helper无法由socket与现有iomux共享、同一source
只能保留一个blocking route、nonblocking与blocking需要不同fact，或signal结果要求改变既有wait/signal contract时，
停止4B。temporary bridge为0、focused blocking/nonblocking/signal与multi-waiter matrix通过后，4B独立Closed并停止；
4C仍须新的明确授权。

#### 6.4.6 Checkpoint 4C — Race、fragment与evidence closure

**目的：** 在4A/4B已经具备完整多waiter能力的实现上做竞态、资源与proof closure；本checkpoint不再承担
single-waiter到multi-waiter的能力扩展。

**交付：**

- 扩展initial-unbound writable、route failure while writable、oversize while writable、request-size-specific
  rejection、TX saturation/recovery与provider backpressure传播；证明capacity恢复由Endpoint fact transition触发
  recheck，而不是Socket/provider mirror；
- 覆盖多个blocking receive/send waiter、poll/select/epoll与blocking syscall并存、ready/timeout/signal/cancel、
  late/duplicate edge、dup/fork/one-alias close/final close、Endpoint retire/identity/port reuse和concurrent receive；
  一次edge可唤醒多人，但每次datagram只有一个detach owner，未胜出者按final predicate继续等待；
- 扩展zero/short/oversize、payload/peer/addrlen三类copy fault在并发下的consume/partial-effect matrix，确认任何
  Stack/source lock不跨user copy、fault后不回滚或重排；
- 通过真实frame/provider injection分别输入first fragment、later fragment和完整fragment pair，证明它们在UDP header/
  Endpoint lookup前被拒绝，不形成delivery/readiness；随后完整datagram仍可收取。当前vendored IPv4 explicit gate若
  已满足，只记录source/evidence，不修改vendor；若失败需要`adapter.rs`或vendored smoltcp change，先停止并报告
  owner/write-set/contract影响；
- 对4A/4B真实压力发现的correctness defect在既有owner/manifest内修复并重跑受影响matrix；不得把修复写成4C新增
  “更多waiter能力”或降低4A/4B closure事实。

**停止/退出：** 任一lost wake、single-waiter restriction、duplicate readiness truth、old Endpoint edge命中新source、
copy fault requeue、fragment进入UDP demux、normal capacity panic/busy-spin或Apollyon/Keter未清零都阻塞Stage 4。
4C final exact source通过6.4.8全部适用gate、review与write-set audit后Stage 4 Closed；只允许随后独立运行
`4 -> 5 Implementation Resolution Gate`，不自动激活Stage 5或cut over candidate contracts。

#### 6.4.7 审计与可观测性

每个checkpoint按实际diff审计，Stage 4 final至少确认：

- 全部Endpoint predicate-changing transition都形成durable/coalescible recheck；edge batching/diagnostic count不反向
  驱动readiness，event loss不能依赖未来无关traffic修复；
- source route registry确实支持复数live routes；fallible replacement先构造后publication，stale pruning、notify与
  final drop均在锁外，没有强引用环或unbounded stale retention；
- 允许的锁偏序最多为`operation -> source -> Stack`，其中poll publication只有短时`source -> Stack snapshot`；
  Stack transition/event drain不反向持锁，operation/source/Stack锁不跨schedule，Stack/source锁不跨user copy，
  final release不取得operation mutex；
- `FileOps::poll`、blocking retry与Endpoint operations读取同一facts；raw Stack/private engine/capacity/provider truth
  不逃逸到Socket或syscall，reverse registry不成为association/liveness owner；
- Stage 3 `EOPNOTSUPP` bridge、busy-poll、single-route slot、socket-local wait loop和直接Latch use均为0；
- fragment source gate、real injection path和完整datagram control case可追溯；禁用reassembly或“测试未收到”不能单独
  作为proof；
- log只区分registration failure、would-block、signal/cancel、retire与unexpected outcome；opaque Endpoint/wait ID
  只服务诊断，不参与lookup之外的状态决策。ordinary packet/edge不产生默认高频日志。

#### 6.4.8 Validation、review与claim boundary

每个checkpoint在自己的final source执行适用子集；4C/Stage 4 closure在exact code上执行完整集合：

```text
just xtask-test
cargo test -p anemone-net-api -p anemone-smoltcp-stack
cargo test -p anemone-smoltcp-stack --no-default-features --no-run
cargo check -p anemone-smoltcp-stack --no-default-features
just fmt kernel --check
just fmt udp-test --check
just app build --arch riscv64 udp-test
just app build --arch loongarch64 udp-test
just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G
just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G
./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/net-udp-stage4-rv64.log
mdbook build docs
git diff --check
```

host matrix必须覆盖facts/event/source publication、复数route、capacity/progression、concurrent receive与真实fragment
injection；KUnit/source-local tests覆盖rollback、late route、retire和锁外handoff；RV64 fresh-disk `udp-test`覆盖真实
fd/status/per-call flags、blocking/nonblocking、poll/select/epoll、signal、dup/fork/close、multi-waiter、copy fault与
loopback/self-external。每个checkpoint closure都需要source/write-set audit和Apollyon/Keter/Euclid review；4C final
review绑定完整exact diff，Apollyon/Keter必须为0。

`cargo test`是protocol/source deterministic proof，app build只是architecture-specific compile/export，kernel build只是
integration；它们都不能替代RV64 syscall/fd/wait runtime。Stage 4不运行或宣称LA64 runtime、remote external peer、
hardware、`smp>1`、full network LTP或final harness；这些继续Not Run并由Stage 5 closure拥有。RV64 local/self-external
也不替代remote external ingress/egress。

#### 6.4.9 Contract Impact、停止条件与退出

Stage 4 **Contract Impact为None**。`OPENED-DESC-001..003`、`IOMUX-POLL-001..003`、`EPOLL-WATCH-001`、
`EPOLL-READY-001`、`EPOLL-FILE-001`与现有Network/control-plane IDs保持Effective/Preserve；
`NET-PROTOCOL-BOUNDARY-001`、`NET-SOCKET-ENDPOINT-001`、`NET-UDP-TRANSACTION-001`与
`NET-SOCKET-WAIT-001`继续Pending。Stage 4提供blocking/iomux/race/fragment evidence，但LA64同源runtime与remote
external双向proof仍由Stage 5拥有；因此不提前执行Protocol/Socket/UDP/wait candidate cutover。

除各checkpoint局部停止条件外，以下任一情况停止整个Stage 4并写入transaction：

- 需要改变R0 readable/writable、blocking/nonblocking、copy consume、fragment或errno/flag可见语义；
- 需要改变Stack/Endpoint、Socket/source、iomux/epoll或opened-description owner/current contract；
- 需要修改`task/files.rs`、`fs/file.rs`、epoll implementation、anemone ABI/library、rootfs/config、vendored smoltcp或
  6.4.10之外source；先报告真实consumer、拟新增路径、owner/contract影响与验证计划；
- exact route只能依赖single waiter、second wait core、busy-poll、unbounded queue、strong consumer reference、
  source/Stack lock inversion或无法退出的compat bridge；
- 需要把Stage 5 mandatory LA64/remote-external evidence降级为optional，或把Stage 4 local/host proof外推为final PASS。

4A、4B、4C必须分别获得activation并独立停止。三者全部Closed、final review/validation通过且没有未处置stop condition
时，Stage 4才Closed；closure只授权状态/evidence write-back，不授权`4 -> 5`resolution、Stage 5或contract cutover。

#### 6.4.10 Resolved Write Set Manifest

以下是Stage 4三个checkpoint允许触及的union；每个checkpoint只使用上文属于自己的最小子集。新文件只允许
`fs/socket/udp/{mod.rs,source.rs}`，并在目录化后删除旧`fs/socket/udp.rs`；其它目录pattern不授权新增相邻文件。

- shared UDP values：`anemone-kernel/crates/anemone-net-api/src/udp.rs`；
- protocol fact/event owner与host proof：`anemone-kernel/crates/anemone-smoltcp-stack/src/udp/{mod.rs,endpoint.rs,
  namespace.rs,datagram.rs}`、`src/stack/{mod.rs,udp.rs,host_validation.rs}`与`tests/udp_topology.rs`；
- kernel Endpoint event routing/access window：`anemone-kernel/src/net/{udp.rs,domain/stack/{mod.rs,udp.rs}}`；
- Socket source/lifecycle：`anemone-kernel/src/fs/socket/{mod.rs,udp.rs,udp/{mod.rs,source.rs}}`；
- blocking syscall与ABI-local mapping：`anemone-kernel/src/fs/api/socket/{mod.rs,abi.rs,sendto.rs,recvfrom.rs}`；
- shared wait owner与既有consumer correction：`anemone-kernel/src/fs/iomux/{mod.rs,wait.rs}`、
  `anemone-kernel/src/fs/api/iomux/{mod.rs,wait.rs,ppoll.rs,pselect6.rs}`；这里只允许移动/窄化source-neutral wait loop并
  保持两个既有consumer，不改变PollRoute/IomuxWaitRound contract；
- real user consumer：`anemone-apps/udp-test/src/main.rs`；
- execution write-back：本RFC`{index.md,implementation.md}`、本transaction、`docs/src/rfcs.md`、
  `docs/src/devlog/transactions/index.md`与当前biweekly devlog。

从4A activation baseline起，`anemone-abi`、`anemone-rs`、`task/files.rs`、`fs/file.rs`、epoll implementation、
其它iomux source、net worker/provider/control-plane、KernelConfig/SystemTarget/build/rootfs、vendored smoltcp、其它apps、
current contracts、`invariants.md`、register/current limitations、其它RFC与scripts保持只读。现有ABI/library wrapper、
opened-description hook、epoll consumer与fragment parse gate已经足够；若live implementation证明不够，按6.4.9停止，
不在当前manifest内做兼容绕行。

### 6.5 Stage 5 Outline — External/dual-architecture closure

概括目的：

- 在最终exact code上证明loopback、self-external local delivery和remote external ingress/egress是三条不同证据；
- 完成host deterministic matrix、RV64 QEMU `smp=1` agent-run与LA64 QEMU `smp=1` user-run；
- 删除probe-only control、temporary dual wiring、legacy ifindex projection与validation-only production seam；
- 原子更新所有达到gate的current contracts、RFC状态、transaction、register/limitations与最终claim。

前置依赖：

- Stage 4 Closed；所有in-target Keter/Apollyon已经neutralized或转成明确cutover stop condition；
- 开发者准备LA64 runtime输入并负责user-run evidence。

受保护边界：

- 同一份architecture-neutral test源码与case用于RV64/LA64；launcher、镜像和remote endpoint参数可以不同；
- LA64失败阻塞closure，未运行只记`Not Run`；RV64/host结果不得外推；
- local path不冒充external provider proof，QEMU不外推physical hardware/virtio-pci/`smp>1`；
- partial success不更新functional contract或把target内失败登记为accepted limitation。

解析触发点：

- Stage 4 Closed后的只读preflight。该gate冻结final case/marker/log/command、external harness、LA64 handoff、
  exact contract write-back、probe deletion与final resolved manifest。

预计范围：

- host/network tests、architecture-neutral user-test资产、两个SystemTarget/Platform wrapper输入、current contracts、
  RFC/transaction/register与validation outputs。具体文件不是当前授权。

## 7. Stage 0 Ready — Multi-interface UDP topology probe

### 7.1 阶段状态与 activation preflight

**成熟度：** Closed；Checkpoints 0A-0C Closed。Stage 1及`0 -> 1`resolution均未授权。

进入Active前必须同时满足：

1. 本RFC target已经R0接受，本文仍是canonical implementation authority；
2. 已创建指向R0的transaction，并记录branch、HEAD、dirty worktree、Stage 0 manifest和独立启动授权；
3. 重新读取live `anemone-smoltcp-stack::{Stack,InterfaceEntry,pump,FrameDevice}`、Cargo features、host provider、
   vendored smoltcp UDP `dispatch`/`Interface::socket_egress`与current network contracts；
4. 确认baseline仍是per-entry `SocketSet`、Ethernet-only adapter和per-netdev production Stack；若任一事实漂移，
   先更新公共implementation并重新review，不以旧manifest开始；
5. 检查manifest内文件与用户dirty changes是否重叠；任何重叠先确认归属并做语义合并，不覆盖；
6. 确认现有crate-owned host gate和`just fmt kernel --check`仍可用。仓库当前没有通用`just test`命令，本stage
   沿用net-frame-path已经接受的native Cargo host-test入口，不为一次probe增加Just/xtask/script wrapper。

### 7.2 Gate P0 probe contract

**Hypothesis：** 在不修改vendored smoltcp、`anemone-net-api`和kernel的前提下，`anemone-smoltcp-stack`内部可以用
一个Stack-level Endpoint owner和一个domain-wide binding/identity语义，驱动至少两个Ethernet interface和一个
bounded IP-medium local software link；control-plane提供的operation-local `selected interface + source address`
能够在queue/admission前固定，使非目标interface不会dequeue datagram，并且local flow仍经protocol egress与normal
ingress。

**Protected Goal / Invariant：** `NET-UDP-DOMAIN-001`、`NET-UDP-LOOPBACK-001`、
`NET-PROTOCOL-BOUNDARY-001`、`NET-CONTROL-PLANE-001`、`NET-UDP-DATAGRAM-001`和
`NET-STACK-PUMP-001`。本probe不得以per-interface Linux Endpoint/binding truth、Socket fast path、unbounded queue、
success后source-drop、fake UDP encoder/decoder或第二route table换取PASS。

**Contract Impact：** None。所有current contracts保持Effective；全部R0 candidate保持Pending / Not Effective。

**Non-goals：** kernel functional integration、production cross-crate API、syscall/File/iomux、port0 allocator、
完整bind conflict matrix、exact Linux errno、SystemTarget/KConfig、production VirtIO、fragment gate、runtime
shutdown、QEMU或contract cutover。独立integration test所需的feature-gated validation facade只属于test seam，
不构成production API。

### 7.3 Checkpoint 0A — Candidate seam risk characterization

交付：

- 在新增`udp_topology` host target中构造一个真实smoltcp UDP socket、两个具有不同IPv4 prefix的deterministic
  Ethernet interface，以及一份明确指向第二interface的operation-local selection；这描述candidate topology，
  不是声称current production Stack已经具有共享UDP Endpoint并发生故障；
- 证明naive candidate把同一engine UDP resource暴露给错误interface先poll时，会进入vendored UDP
  source-selection/drop或wrong-interface egress风险；测试记录queue ownership与两个provider的TX observation，
  不依赖trace文本判定；
- 把风险归类为candidate engine egress-admission seam ordering，而不是current frame provider、route table或syscall
  baseline defect；
- reproduction只服务本checkpoint。Checkpoint 0B形成正确路径后，不能保留“期望错误行为”为长期PASS测试。

停止条件：

- 若vendored/current source变化使candidate risk不再存在，停止当前Ready route并重新审计actual egress
  semantics、更新Stage 0 definition后再review；不能继续按旧假设修改Stack；
- 若只能通过packet injection、raw socket或手工UDP packet构造重现，说明fixture没有覆盖真实Endpoint路径，
  0A不能关闭。

**关闭记录：** 2026-07-29，0A以真实smoltcp UDP socket enqueue/egress路径稳定复现wrong-interface-first会
消费共享engine queue并从错误provider发包；ARP输入只用于准备普通Ethernet neighbor，没有注入或手工构造UDP
packet。风险归类为candidate egress-admission seam，不是current per-netdev production缺陷。精确证据见
[transaction](../../devlog/transactions/2026-07-29-net-udp.md#2026-07-29---checkpoint-0a-implementation-source-audit-and-closure)。
预期错误行为测试按0A退出条件保留；0B形成正确路径时必须删除或改写，且本记录不授权进入0B。

### 7.4 Checkpoint 0B — Stack-private production-shaped candidate

交付：

- 仅在`anemone-smoltcp-stack`内部建立provisional Endpoint/selection/local-link shape。ordinary owner logic保持
  `no_std + alloc`可编译；注入frame、设置selection、读取完整queue/packet仅存在于`host-test` fixture；
- 同一个Stack拥有一个UDP Endpoint identity/binding owner。不得为两个Ethernet interface和local interface复制
  三份会独立bind/retire的logical Endpoint；private engine resource可以存在多个，但必须由一个Endpoint owner
  聚合并证明binding、capacity、receive order与retire没有并列truth；
- test-owned control plane在operation外选择`InterfaceId + source IPv4`。Stack验证opaque mapping与Endpoint
  admission；selection failure、stale/unknown interface、unsupported source、oversize和bounded TX full均在成功
  commit前返回typed stack-local outcome；
- 只有selected external interface的pump可以消费该datagram。先pump其它interface必须保持Endpoint queue、
  owner与success/failure fact不变；之后selected interface经真实smoltcp UDP egress形成frame；
- local interface使用stack-private IP-medium software device/port，不修改shared `FrameProvider`并不伪造Ethernet
  facts。egress进入有界software-link storage，后续bounded pump从同一storage进入同一Stack normal IP/UDP ingress；
  不允许直接调用目标Endpoint enqueue/process或从source Socket复制到peer Socket；
- 两个external interface与local link共享同一个pump serialization boundary；每次interface/local round有限，
  blocked provider或full local link不能让其它ready interface永久饥饿；
- provisional Endpoint retire/remove必须从同一owner撤销它聚合的全部private engine resource、pending datagram与
  local-link association；随后使用stale provisional identity或继续pump都不能deliver/drive已撤销resource。本probe
  只证明aggregate teardown和no parallel truth，不证明File final release、port reuse或并发close race；
- private smoltcp `SocketHandle`、`SocketSet`、`Interface`、IP-medium device和buffer不得从crate public surface导出。

本checkpoint不预先要求literal `EndpointId`、`SelectedEgress`、`LocalLink`或`DomainPump`名称。若一个private type
同时混合Endpoint lifecycle、route policy、provider resource与test control，应在同一checkpoint内缩窄职责，
不能用`Manager`/`State`总对象掩盖owner边界。

**关闭记录：** 2026-07-29，0B以一个Stack-private Endpoint owner聚合每个external/local interface的private
smoltcp UDP resource；binding conflict、TX phase、receive ordering gate与retire均由aggregate owner推进。
operation-local selection在commit前验证interface mapping、source、destination、MTU与Endpoint capacity；只有
selected engine获得datagram，wrong-interface-first保持owner queue不变。新增bounded IP-medium local port经
protocol egress、下一bounded round的normal ingress完成delivery，provider blocked与local-link full均不阻塞其它
ready interface/Endpoint永久推进。retire先撤销aggregate identity，再删除全部engine resource及带owner的pending
local packet；host facade只暴露protocol-domain value/outcome与test-only observation。0A expected-wrong test已改写为
positive matrix。source、review与validation证据见
[transaction](../../devlog/transactions/2026-07-29-net-udp.md#2026-07-29---checkpoint-0b-implementation-review-validation-and-closure)。
本记录不选择0C的保留/cleanup路线，也不授权进入0C。

#### 7.4.1 Checkpoint 0B Feedback Correction — Closed

0B关闭后的独立软件工程审查确认以下反馈；它们不改变R0 target、owner、ABI、shared contract或acceptance
boundary，因此按保持target的Route Correction在原Stage 0内修复，不递增RFC修订：

- **Keter — receive backpressure owner粒度错误：** `blocked_receive: Option<InterfaceId>`把一个Endpoint的
  aggregate receive queue饱和提升成domain/interface级gate，使无关ready Endpoint在连续finite pump中永久无法
  推进；blocker没有Endpoint identity也使retire无法证明清除的是原阻塞事实。
- **Keter — TX admission与engine storage不一致：** admission只按interface MTU接受payload，没有把Endpoint
  private engine payload capacity纳入同一owner predicate；合法MTU内但超过engine storage的send先返回成功，随后
  pump在`send_slice`处触发correctness assertion，而不是commit前typed failure。
- **Euclid — host validation与production owner文件混合：** conditional DTO、error conversion、fixture control和
  observation集中在`stack.rs`，使test seam反向塑造owner orchestration文件；module-wide dead-code bridge仍只允许
  保留到0C决定。

本correction的交付与边界：

- receive queue容量与pending engine datagram保持Endpoint-local；一个Endpoint饱和不得阻止同一或其它interface上
  无关Endpoint的normal ingress。不得以domain-wide排序gate、busy recheck或第二receive truth修复；同Endpoint已有
  engine datagram在aggregate credit恢复后仍先于该engine后续可接受datagram进入aggregate queue。
- TX commit前的单一admission predicate同时覆盖selected interface IP MTU与Endpoint engine payload capacity；
  accepted payload进入private engine时只能成功，capacity failure返回既有typed stack-local outcome，不新增panic
  fallback或第二credit truth。
- 把`#[cfg(feature = "host-test")]`validation types/facade/conversion按同一Stack owner拆入独立
  `src/stack/host_validation.rs` child module；ordinary owner operation保持module-private，conditional surface与kernel
  `default-features = false`边界不变。拆分不得扩大production public API或shared API。
- 增加focused host regression：小engine/大MTU的pre-commit拒绝；Endpoint A receive饱和时Endpoint B在同一local
  interface继续推进；existing topology、local recovery和retire cases继续通过。

**Resolved correction write set：**

- `anemone-kernel/crates/anemone-smoltcp-stack/src/{lib,pump,udp}.rs`与`src/stack/mod.rs`；
- 新建`anemone-kernel/crates/anemone-smoltcp-stack/src/stack/host_validation.rs`；
- `anemone-kernel/crates/anemone-smoltcp-stack/tests/udp_topology.rs`；
- 本RFC `index.md` / `implementation.md`、net-udp transaction、RFC/transaction索引与当前biweekly devlog。

`local_link.rs`、Cargo feature、kernel、`anemone-net-api`、vendored smoltcp、current contracts、register、build/config
与其它RFC保持只读。若修复需要改变receive overflow语义的accepted boundary、Endpoint/control-plane owner、
public/shared API或上述write set，立即停止并回到RFC review；不得自动进入0C。

验证至少重跑`udp_topology`、stack/net-api host suite、no-default compile/check、`just fmt kernel --check`、RV64
release compile integration、`mdbook build docs`与whitespace检查；runtime/rootfs/QEMU/LTP/LA64仍不由本correction
外推。独立subagent必须在最终source上按软件工程审查等级review；Apollyon/Keter未清零不得重新关闭0B。

**Correction result：** domain/interface级`blocked_receive`与关联cleanup已经删除；饱和Endpoint只在自己的engine
保留最早datagram，不阻塞其它Endpoint或制造Immediate busy recheck。TX pre-commit maximum现在同时取interface
MTU payload ceiling与Endpoint engine capacity的下界。ordinary Stack owner收归`src/stack/mod.rs`，conditional
DTO/facade/conversion位于同owner child module `src/stack/host_validation.rs`；ordinary create/send/retire保持
module-private，`interface_mut`只在unit test或`host-test`下编译。

两个focused regression与原有matrix通过；stack/net-api host suite、no-default compile/check、RV64 release compile、
mdBook和whitespace gate完成，formatter只剩三处未触及的vendored smoltcp baseline。最终独立复审为
Apollyon 0、Keter 0、Euclid 0。完整source/review/validation证据见
[transaction](../../devlog/transactions/2026-07-29-net-udp.md#2026-07-29---checkpoint-0b-feedback-correction-implementation-review-validation-and-closure)。
0B重新Closed；本结果不选择0C路线、不关闭Stage 0，也不授权进入0C。

### 7.5 Checkpoint 0C — Decision closure与probe cleanup

成功路线：

- 保留证明target所需的最小ordinary stack code和long-term deterministic tests；删除0A的错误期望、packet dump、
  unrestricted inspection与不再需要的host-only method；只有独立integration test仍需要时才保留窄
  `host-test` facade，并在定义旁说明它不进入kernel dependency，以及在fixture可回到crate-private或accepted
  production control-plane owner替代后删除；
- 在transaction记录concrete topology decision、为何不需要vendored/shared API变化、仍未证明的kernel/runtime边界；
- 把Stage 1 Outline所需的route input写回本文，但不在0C解析或激活Stage 1。

负面路线：

- 若0B无法在manifest内满足hypothesis，删除无法形成独立长期价值的candidate/probe-only code；有长期价值时保留
  最小stable characterization/regression，否则只在transaction保留cleanup前的exact command/output/source point，
  不能用文字判断替代可复现证据；
- transaction必须指出失败发生在何处：非目标interface不可避免地dequeue、single Endpoint只能复制成
  per-interface truth、local medium必须修改shared frame API、或现有smoltcp缺少必要filter/dispatch seam；
- target保持不变时将结果分类为Route Correction，并由`0 -> 1`gate比较“narrow vendored smoltcp seam”与
  “Stack-owned aggregate engine resources”等候选；Stage 0不得自行扩大manifest尝试第二方案；
- 若证据要求移动control-plane/binding owner、允许success后silent drop、Socket fast path或削弱normal-ingress
  acceptance，停止cutover并进入Target Renegotiation Gate。agent只能提出，不能批准。

**关闭记录：** 2026-07-29，0C选择positive route。保留`anemone-smoltcp-stack`内最小ordinary aggregate
Endpoint、显式selection admission、bounded local-link与finite pump candidate，以及5项长期deterministic topology
tests；这些代码继续保持private、`no_std + alloc`可编译且没有kernel consumer。integration test仍需conditional
facade，故本stage保留的UDP-specific surface只传protocol-domain value/outcome与窄owner observation；kernel的
`default-features = false`dependency不编译该surface。没有保留packet dump、raw smoltcp handle/buffer/queue
inspection或0A expected-wrong test。既有frame-path host fixture继续拥有其长期test-only packet injection，未被
本stage扩大。临时dead-code/test-facade注释的退出条件改为Stage 1由accepted production
owner接管，或Stage 1选择替代路线并删除candidate。

concrete topology decision是：一个Stack-private aggregate Endpoint owner可以为多个interface维护private engine
resource，同时保持binding、admission、receive ordering与retire单一真相；operation-local selected
`InterfaceId + source`在engine enqueue前完成验证，只有selected engine取得datagram；IP-medium local port可在同一
`&mut Stack`边界内经bounded software handoff与下一finite round的normal ingress推进。该结果无需修改vendored
smoltcp、`anemone-net-api`、kernel或current contracts。Stage 1 route input因此是优先评估该aggregate engine
candidate如何接入唯一global Stack access window与异构provider；local port只作为Stage 2 production loopback
resolution的证据输入。0C不解析Stack access/worker/attach路线，不冻结Stage 1 manifest，也不授权`0 -> 1`gate。

### 7.6 审计

Stage 0必须执行并在transaction记录以下source audit：

- `Stack`内所有`SocketSet`/UDP socket/handle owner与lookup，确认没有跨crate handle、per-interface Linux
  Endpoint或按interface分裂的binding truth；
- 每个TX enqueue/dequeue、selected-interface check和provider/local-link transfer，确认commit前后只有一个
  datagram owner；
- vendored UDP `dispatch`的no-source drop分支，确认Stage 0成功路径不能在syscall-equivalent success后进入；
- 所有local-delivery入口，确认没有direct Endpoint process/enqueue、Socket-to-Socket copy、unbounded queue或
  synchronous unbudgeted reentry；
- Endpoint retire/remove及所有private engine handle/resource lookup，确认withdraw先于stale lookup失败，pending
  datagram/local-link association不会在aggregate owner之后继续deliver或形成第二lifecycle truth；
- Cargo feature/public surface/dependency audit，确认kernel的`default-features = false`dependency不获得host
  control，conditional public facade只暴露validation input/outcome而不暴露smoltcp object/handle/buffer/queue
  identity，stack不依赖kernel/task/fs，`anemone-net-api`未被修改；
- currentframe host tests，确认multi-instance isolation、frame ownership、bounded progress和deadline没有因
  global endpoint experiment退化。

允许保留的旁路只有`#[cfg(feature = "host-test")]`下的fixture injection/inspection。独立integration test需要时，
该fixture可通过窄`pub` facade调用，但必须由Cargo target的`required-features = ["host-test"]`隔离，且no-default/
kernel dependency不编译或导出test control。ordinary UDP owner logic不得依赖host-test-only fact。

### 7.7 反馈假设与停止条件

本stage要验证的具体假设只有一个：current stack/vendored seam是否足以支持single Endpoint owner下的显式
interface/source admission。以下任一项出现即停止当前route，不允许通过扩展scope或降低断言继续：

- 非目标interface会peek/dequeue/consume属于另一interface的datagram；
- missing source/route/interface只能在smoltcp dequeue后发现，operation已无法同步失败；
- 必须让kernel/control plane保存smoltcp handle、SocketSet、queue depth或binding mirror；
- 必须修改`anemone-net-api`、vendored smoltcp、kernel或current contract才能继续；
- local delivery只能用unbounded queue、direct Endpoint injection或socket fast path完成；
- logical Endpoint必须复制为per-interface binding/readiness/lifecycle owner，且无法由一个Stack owner证明聚合；
- pump需要并发`&mut Stack`、持provider全局锁跨protocol callback，或普通负载无法有限推进；
- host-only API越过显式validation facade、进入no-default/kernel dependency或暴露private representation，或测试
  只能靠fake protocol path自证。

发生上述failure时，结果先写transaction。保持target的scope/route变化写回本文；设计issue如影响owner、stage
order或acceptance，另建/更新tracking issue；改变target/contract/ABI进入RFC review；target内已实现但错误的行为
进入open issue，不能登记为accepted limitation。

### 7.8 Contract cutover

None。Stage 0无论成功或得到负面结论，都不修改`docs/src/contracts/`、不增加pending-successor生效状态，也不
宣称`NET-IFACE-DOMAIN-001`、`NET-CONTROL-PLANE-001`、`NET-PROTOCOL-BOUNDARY-001`、
`NET-SOCKET-ENDPOINT-001`、`NET-UDP-TRANSACTION-001`或`NET-SOCKET-WAIT-001`已实现。

Stage 0失败时current per-netdev Stack和所有现有contracts保持baseline。Stage 0成功的private ordinary code也只
是尚未接入kernel的implementation input；没有production activation、syscall或runtime evidence前不能cut over。

### 7.9 模块边界预检

进入Stage 0时`anemone-smoltcp-stack/src/stack.rs`混合interface mapping、SocketSet ownership与host validation control，
`pump.rs`混合per-interface scheduling与socket egress。Stage 0若继续把Endpoint transaction、local software device
和test control全部塞入这两个文件，会固化新的混合边界。

因此本stage允许在同一crate/同一Stack owner内新建`src/udp.rs`与`src/local_link.rs`：

- `udp.rs`只承载provisional UDP Endpoint/operation ownership与stack-private engine conversion；
- `local_link.rs`只承载bounded IP-medium packet handoff与smoltcp device adaptation；
- `stack/mod.rs`保留interface/endpoint mapping和owner-level orchestration；
- `pump.rs`保留finite progression与outcome组合；
- `adapter.rs`保留smoltcp device/token adaptation，不取得route/Endpoint policy。

这是same-owner、private、behavior-preserving-or-probe-local的结构维护，不扩大public API或shared contract。
若实际证据需要不同文件名但仍满足上述职责与manifest目录边界，可在activation preflight中冻结更窄清单；若需要
移动owner surface、导出public API、修改shared/vendored crate或触碰kernel，必须停止并申请write-set expansion，
不能包装成模块整理。

### 7.10 Scope envelope

参与owner：

- concrete `anemone-smoltcp-stack::Stack`：private interface/Endpoint/engine resource与唯一protocol mutation；
- deterministic external provider：test-ownedframe/resource truth；
- test-owned control plane：immutable route/source/interface selection input；
- stack-private local software-link：in-flight IP packet/capacity；
- host test harness：只拥有case orchestration与observation。

本stage不创建kernel `NetworkDomain` object，不决定production lock/worker/mailbox，不定义Linux errno或readiness，
不决定port allocator、SystemTarget schema或KernelConfig数值。本stage新增的ordinary UDP owner类型和operation
方法保持crate-private。独立integration test允许增加最窄的`#[cfg(feature = "host-test")] pub` validation facade；
其参数和返回值只表达protocol-domain input/outcome与test observation，不得暴露smoltcp object/handle、buffer/
queue identity或production authority。它是conditional test surface，不是production API；Cargo `required-features`
和kernel `default-features = false`dependency必须共同证明它不进入kernel可见surface。

### 7.11 Resolved Write Set Manifest

允许production/provisional source写入：

- `anemone-kernel/crates/anemone-smoltcp-stack/Cargo.toml`，只用于启用UDP/IP-medium所需smoltcp feature与声明
  `udp_topology` host test target；
- `anemone-kernel/crates/anemone-smoltcp-stack/src/lib.rs`；
- `anemone-kernel/crates/anemone-smoltcp-stack/src/{pump.rs,adapter.rs}`与`src/stack/mod.rs`；
- 计划新建`anemone-kernel/crates/anemone-smoltcp-stack/src/{udp.rs,local_link.rs}`。

允许test/validation写入：

- `anemone-kernel/crates/anemone-smoltcp-stack/tests/support/mod.rs`；
- 计划新建`anemone-kernel/crates/anemone-smoltcp-stack/tests/udp_topology.rs`；
- Cargo/formatter生成的untracked build output；这些不是source、contract或freshness authority。

允许执行期文档写回（R0接受后由transaction引用）：

- `docs/src/rfcs/net-udp/implementation.md`，只记录Stage 0反馈、Route Correction与Stage 1 resolution；
- `docs/src/rfcs/net-udp/{index.md,invariants.md,tracking-issues.md}`，只有对应影响分类确实需要时；
- 当前net-udp transaction与transactions index/biweekly devlog的必要执行事实；
- target内缺陷或target外accepted gap确有结论时，分别写register open issue或current limitation。

validation-only只读输入：

- `anemone-kernel/crates/anemos/smoltcp/src/{socket/udp.rs,iface/interface/mod.rs,iface/interface/ipv4.rs,phy/mod.rs}`；
- `anemone-kernel/crates/anemone-net-api/src/**`；
- `anemone-kernel/src/{net,device/net,driver/net}/**`；
- 现有`anemone-smoltcp-stack/tests/{frame_path.rs,bounded_progress.rs,multi_instance.rs}`；
- 本RFC target、current network/opened-description/iomux/epoll contracts和register。

明确禁止写入：

- vendored`anemone-kernel/crates/anemos/smoltcp/**`；
- `anemone-kernel/crates/anemone-net-api/**`；
- kernel、device、driver、VFS、syscall、iomux、task与architecture source；
- root `Cargo.toml`/`Cargo.lock`、Justfile、xtask、`conf/**`、anemone-apps、current contracts和public navigation，
  除非后续独立docs review明确批准；
- 与net-udp无关的tracked或private file。

若manifest内无法完成Stage 0，不得先修改再追认。扩展申请必须说明失败证据、拟新增文件/owner、对target/
contract/gate/验证的影响，以及批准后在本文和transaction的记录位置。

### 7.12 可观测性

Stage 0只增加test-owned observation：

- 每个interface/provider的TX submission与RX consumption；
- Endpoint queue ownership、selected interface/source与typed admission outcome；
- aggregate Endpoint retire后private engine resource、pending datagram和local-link association均已撤销；
- local-link当前occupied credit、full/recovery/recheck和normal-ingress completion；
- bounded pump round与哪个owner仍有work的case-local记录。

这些字段不得参与production行为决策，必须在定义旁标明diagnostic/test-only。Stage 0不增加production per-packet
log、global diagnostic registry或长期counter。owner-local correctness使用常开`assert!`；昂贵的test扫描可以使用
test assertion，不把`debug_assert!`用于release correctness。

### 7.13 验证

Stage 0先按0C选择positive或negative路线，再在对应final source上执行分支验证和共同gate。

Positive branch：

1. `cargo test -p anemone-smoltcp-stack --test udp_topology`，覆盖：
   - wrong-interface-first不consume，selected interface唯一egress；
   - 两个不同prefix/source的external selection与同Endpoint identity；
   - no-selection/stale-interface/no-source/oversize/TX-full在commit前typed fail；
   - local egress -> bounded link -> normal ingress -> same Endpoint namespace；
   - local-link full/recovery/recheck、external provider blocked isolation与finite pump；
   - no-source vendored drop分支不在accepted admission后发生；
   - aggregate Endpoint retire撤销全部private engine resource/pending handoff，stale provisional identity不能继续
     deliver或drive；这不外推为fd final release、port reuse或concurrent close证明；
2. final diff若保留ordinary stack code或修改kernel dependency会编译的smoltcp feature，运行repository-owned
   `just build --preset qemu-virt-rv64-release --bind smp=8 --bind memory=1G`。它只证明RV64 kernel compile integration，
   不形成runtime、syscall或network behavior证据。

Negative branch：

1. candidate cleanup前，以transaction记录的exact test name/filter重新运行0A characterization，记录exact command、
   output、vendored/source decision point和0B失败分类；不能只保留文字判断；
2. cleanup后，若保留具有长期价值的窄characterization/regression test，则在final source上运行并PASS；若删除全部
   candidate/target test code，则保留cleanup前的transaction evidence，并用下面共同gate证明baseline恢复。Positive
   `udp_topology` capability matrix对此结果标为`Not Applicable — negative decision`，不能记为PASS或`Not Run`；
3. source audit确认无法形成独立价值的candidate/probe-only ordinary code、feature和conditional public facade均已
   删除；有意保留的regression必须逐项说明proof scope和退出条件。若negative final diff仍保留kernel dependency会
   编译的ordinary code/feature，也必须运行上述repository-owned RV64 build；否则以final diff记录该build为
   `Not Applicable — cleaned to host-only/baseline surface`。

共同gate（在positive或negative final source上均执行）：

1. `cargo test -p anemone-net-api -p anemone-smoltcp-stack`，沿用现有crate-owned host conformance gate并确认
   frame-path三个长期integration target继续实际执行；
2. `cargo test -p anemone-smoltcp-stack --no-default-features --no-run`与
   `cargo check -p anemone-smoltcp-stack --no-default-features`，证明host target由feature metadata隔离且ordinary
   stack code保持`no_std + alloc`；
3. `just fmt kernel --check`；任何新增formatter diff阻塞本stage，既有baseline只可原样记录；
4. dependency/public-surface/SocketHandle/SocketSet/source-selection/local-link/Endpoint-retire/queue-bound/pump-budget
   source audit；
5. `git diff --check`，以及新文件逐个`git diff --no-index --check -- /dev/null <file>`。

Stage 0不运行rootfs、QEMU、LTP或双架构runtime。它们记为`Not Run`，不能以host PASS或compile PASS外推。若Stage 0
需要kernel runtime才能证明hypothesis，说明scope已经扩大，应停止并申请新的Ready definition，不把broad runtime
当作替代证据。

### 7.14 退出条件

Stage 0独立Closed需要同时满足：

- Checkpoint 0A已经用真实smoltcp UDP path刻画candidate seam risk，并明确它不是current production baseline
  defect；
- Checkpoint 0B得到满足全部protected invariant的positive result，或者得到可复现、可分类且已清理partial code的
  negative result；
- 最终source没有fake UDP path、unbounded queue、per-interface Linux Endpoint truth、host control leak、
  Socket-to-Socket copy、vendored/shared/kernel write或未记录scope expansion；
- branch-specific与共同host/no-default/conditional-build/format/whitespace gates按positive或negative final code
  通过，`Not Applicable`与`Not Run`没有冒充PASS；
- transaction记录result、code disposition、remaining uncertainty和feedback route；
- current contracts保持不变；conditional RV64 build只按compile evidence记录，runtime/UAPI/architecture behavior
  claims明确`Not Run`；
- Stage 0先Closed。Stage 1是否、何时达到Ready由后续独立resolution gate决定，不是本stage closure条件。

## 8. Stage 0 -> Stage 1 Implementation Resolution Gate — Completed 2026-07-29

前置条件：

- Stage 0已按7.14独立Closed；
- transaction已有exact diff、validation、review与positive/negative topology evidence；
- 当前RFC target、Contract Impact和current contracts仍有效。

只读preflight：

- 读取live Stack/interface/socket/pump、kernel `net::{mod,worker}`、`device/net` registry/provider、Stage 0 diff、
  review findings、host evidence和module-boundary pressure；
- 若Stage 0 positive，审计candidate能否在不泄露smoltcp object的前提下由kernel global Stack access window与
  heterogeneous external provider自然调用；
- 若Stage 0 negative，比较至少以下target-preserving路线：为vendored smoltcp增加最窄egress-selection seam；
  由Stack聚合多个private engine resource但保持单一Endpoint/binding/capacity truth；调整pump admission而不复制
  route policy。不能只按已写代码沉没成本选择；
- 核对Stage 1 Outline的logical-interface owner、global Stack、attach rollback、old wiring removal和frame-path
  regression是否仍构成可达路径。

解析输出：

- 选择并说明Stage 1的concrete Stack access/provider/worker路线；
- 把Stage 1展开为完整Ready：交付/checkpoint、internal API、module split、audit、observability、validation、
  failure/exit、temporary bridge删除点、candidate contract cutover与exact Resolved Write Set Manifest；
- 只改变implementation preference、physical files、stage order或validation时更新本文并在transaction记录Route
  Correction；
- 需要修改`anemone-net-api`或vendored smoltcp时，必须在Stage 1 manifest中明确新增的窄surface、真实consumer、
  object fence、no-default proof和退出条件，不能从Stage 0自动继承权限；
- 改变global Stack、Endpoint/control-plane owner、send-success、normal-ingress、ABI或acceptance boundary时停止，
  进入RFC review / Target Renegotiation Gate。

授权边界：

- Stage 1完整定义和manifest冻结后才达到Ready；必须另行授权进入Active，不得由Stage 0 closure、positive probe
  或transaction记录自动启动。

Resolution结果：

- Stage 0 positive route在live source中仍可由一个kernel domain-owned Stack access window和heterogeneous
  per-provider worker自然消费；不需要vendored smoltcp、`anemone-net-api`、route/control plane或第二Stack。
- authoritative Stage 1 Ready定义、单一Checkpoint 1A、`NET-UDP-DOMAIN-CUTOVER`与exact manifest已经冻结在
  [6.1](#61-stage-1-closed--initial-domain--global-stack-walking-skeleton)；具体preflight、Route Correction分类和
  Not Active边界记录在[transaction](../../devlog/transactions/2026-07-29-net-udp.md)。
- 本resolution保持R0 target、owner、ABI、Contract Impact与acceptance boundary，不增加RFC修订，不更新current
  contract或register。Stage 1为Ready / Not Active；Stage 2 resolution与任何implementation均未授权。

## 9. Stage 1 -> Stage 2 Implementation Resolution Gate — Completed 2026-07-29

前置条件：

- Stage 1已按6.1独立Closed，transaction保存final diff、review、host/no-default、RV64 compile/runtime和
  `NET-UDP-DOMAIN-CUTOVER`证据；
- current `NETDEV-LIFE-001`、`NET-ATTACH-001`和`NET-IFACE-DOMAIN-001`已经与live source一致；
- R0 target、SystemTarget/current Network contracts、register与Stage 0 topology conclusion没有漂移。

只读preflight：

- 读取Stage 1 exact source/commit、`net::{mod,domain,worker}`、Stack/local-link/pump/UDP candidate、current contract、
  register与transaction，确认global Stack access、logical/protocol identity、attach rollback和shutdown边界；
- 读取live SystemTarget loader/resolver、kernel build generated-def materializer、clean、selected QEMU SystemTarget/
  Platform/Preset/KConfig和双架构wrapper，确认network deployment的唯一配置owner与runtime input path；
- 审计module pressure：`domain.rs`已混合logical/Stack/KUnit，`worker.rs`混合control与provider progression，
  stack root/pump同时承载external与dormant local/UDP role，build root同时承载orchestration和defs rendering；
- 核对Stage 0 local candidate能否由production DomainStack和local worker直接消费，是否需要vendored/shared seam、
  新probe或target变化；核对`127/8`、self-external local route、bounded recovery与conditional KUnit proof是否可达。

解析输出：

- 采用“2A same-owner split -> 2B atomic static-control/loopback cutover”，不建立新的probe或transitional current
  contract；2A不改变behavior，2B才一次发布config/control plane/local path；
- SystemTarget使用optional single-external `[network.ipv4]` typed input；ordinary QEMU RV64/LA64 target显式声明
  `eth0 10.0.2.15/24`和`10.0.2.2` gateway，loopback保持implicit，missing/mismatch fail closed且无fallback；
- control plane是route/source/interface唯一policy owner，Stack只保存address/default-route/AnyIP projection；
  production local worker复用bounded pump/control，KConfig只增加真实独立的packet capacity和IP MTU；
- Stage 0 candidate、conditional facade和no-default object fence足以形成production route；只需在
  `anemone-net-api`增加IPv4 address/CIDR value，不需要vendored smoltcp change或generic protocol trait；
- Stage 2 cutover proof只覆盖control-plane/local-path closure；真实remote external UDP仍由Stage 5 final
  acceptance强制证明，不从本stage的Not Run降低或外推；
- authoritative Stage 2 Ready、两个checkpoint、`NET-UDP-CONTROL-CUTOVER`、audit/validation/stop boundary与exact
  manifest已经冻结在[6.2](#62-stage-2-ready--static-control-plane与production-loopback)。

授权与文档结果：

- 本resolution保持R0 target、state owner、ABI/visible semantics、Contract Impact分类与acceptance boundary；只是
  解析implementation preference、module layout、stage order、validation和cutover arrangement，因此不增加R1、
  tracking issue或register条目；
- current contracts、source、SystemTarget/KConfig/generated defs均未修改。只同步canonical RFC/transaction/
  navigation/devlog状态并运行docs gate；文档/source audit不能冒充未来2A/2B build、host、KUnit或runtime evidence；
- Stage 2为**Ready / Not Active**。下一步只能在新的明确授权下激活Checkpoint 2A，并先在transaction重新核对
  branch/HEAD、dirty state、live owners和2A frozen manifest；不得由本resolution修改source、执行2B/cutover、
  更新current contract、解析Stage 3或把conditional KUnit写成用户态UDP能力。

## 10. 旁路审计清单

后续每个Ready stage都必须按实际scope细化本清单；至少持续检查：

- `Stack::new()`、`SocketSet::new()`、smoltcp socket construction与handle storage，确认production只有一个domain
  protocol owner且没有hidden per-netdev namespace；
- netdev ifindex/name lookup与logical-interface lookup，确认cutover后只有一份行为truth；
- route/source/interface selection入口，确认kernel Socket、Stack private iteration与provider没有第二policy；
- local-address、loopback与packet injection入口，确认没有backend hairpin、Socket fast path或host fixture进入
  production dependency；
- syscall、File private state、fd table与final-release callsites，确认不按fd/memory lifetime retire Endpoint；
- poll/register/notify paths，确认route publication/current predicate原子、notify在guard外且final recheck存在；
- UDP enqueue/dequeue/copyout/fragment paths，确认single owner、send success与receive consume linearization；
- `cfg(test)`/feature/log/diagnostic fields，确认validation-only state不驱动production behavior；
- temporary bridge/fallback/legacy owner，确认有日志/注释、唯一behavior authority和明确删除gate。

## 11. 可观测性清单

实现期可观测性服务于owner、handoff、failure与validation，不建立第二truth：

- boot/attach摘要：logical identity、external publication origin、Stack-private mapping仅以opaque/diagnostic形式
  关联；不打印或暴露driver backing/smoltcp handle；
- endpoint lifecycle：create/bind/retire/reject可使用opaque diagnostic ID，但ID不得驱动lookup之外的行为；
- admission failure：unsupported ABI、bind conflict、no route/source/interface、oversize、normal capacity与
  would-block应能区分；静默兼容或未支持flag必须按syscall准则注释并记录；
- wait：只记录predicate transition/recheck/cancel类别，不把wake count或event payload当ready proof；
- validation marker必须标注host、RV64 agent-run、LA64 user-run与Not Run，不从一个轨道生成另一轨道结论；
- production per-packet dump、长期queue mirror和unbounded日志不作为默认方案。

## 12. 停止边界

以下情况继续在implementation层解析，不重开target：private type/方法名、module placement、lock/actor/worker、
queue或allocator形状、port选择算法、capacity数值、test case拆分、vendored narrow seam与Stack-private aggregate
之间的路线选择，只要保持owner/ABI/contract/acceptance。

以下情况立即停止当前gate并上报：

- 需要第二domain/Stack、per-interface Endpoint namespace或双route/binding/readiness truth；
- 需要扩大或降低五项R0用户可见决定、first-version syscall/flag/copy/fragment行为；
- 需要改变`NETDEV-LIFE-001`/`NET-ATTACH-001`分类、SystemTarget owner、OPENED-DESC/IOMUX/EPOLL contract；
- 需要Socket fast path、success后silent drop、copy fault requeue、unbounded queue、busy-poll或无法退出的compat bridge；
- 需要把LA64 mandatory closure改成可选、把external proof替换为local/host proof，或把Not Run写成PASS；
- Ready/Active manifest需要越界且尚无批准。

若issue追查后只剩Safe级实现偏好，不继续为“更通用”而扩展设计；按当前Ready gate执行并把未来可能性留给
真实consumer或后续RFC。

## 13. 实现期反馈记录

- `2026-07-29`：Checkpoint 0A / Execution Fact。真实UDP path确认naive shared-engine topology缺少
  selected-interface admission seam；wrong-interface-first可消费queue并通过错误provider egress。结论保持R0，
  不是production baseline defect；0A characterization保留到0B正确路径形成时删除或改写。没有Route Correction、
  Target Renegotiation、Contract Impact、tracking issue或register写回；精确source/review/validation证据见
  [transaction](../../devlog/transactions/2026-07-29-net-udp.md#2026-07-29---checkpoint-0a-implementation-source-audit-and-closure)。
- `2026-07-29`：Checkpoint 0B / Positive Execution Fact。Stack-private aggregate Endpoint为每个interface维护
  private engine projection，但binding、admission、TX phase、receive order gate与retire只有一个owner；显式
  selection阻止wrong-interface dequeue，bounded IP-medium local port经normal ingress交付。结论保持R0且不修改
  shared/vendored/kernel/current contract。0C仍须独立决定candidate/facade/test的长期保留与cleanup，并在final
  source上重跑Stage 0 branch/common gates；精确证据见
  [transaction](../../devlog/transactions/2026-07-29-net-udp.md#2026-07-29---checkpoint-0b-implementation-review-validation-and-closure)。
- `2026-07-29`：Checkpoint 0C / Positive Decision Closure。保留最小private ordinary candidate、5项长期topology
  matrix与必要conditional validation facade；没有vendored/shared/kernel/current-contract变化。positive/common
  gates在final source通过，Stage 0独立Closed。Stage 1只获得aggregate-engine/global-Stack与local-link证据输入，
  `0 -> 1`resolution与Stage 1 activation均未授权；精确证据见
  [transaction](../../devlog/transactions/2026-07-29-net-udp.md#2026-07-29---checkpoint-0c-positive-decision-closure-review-and-validation)。
- `2026-07-29`：`0 -> 1` Implementation Resolution / Route Selection。live source证明Stage 0 aggregate
  private-engine candidate可由initial-domain唯一Stack access window和per-provider worker直接消费；Stage 1不需要
  vendored/shared seam。resolution只改变implementation route、physical manifest、validation与cutover安排，保持
  R0 target/owner/ABI/Contract Impact/acceptance不变；Stage 1达到Ready / Not Active，精确preflight见
  [transaction](../../devlog/transactions/2026-07-29-net-udp.md#2026-07-29---stage-0---stage-1-implementation-resolution-gate)。
- `2026-07-29`：Stage 1 / Execution and Contract Cutover。Checkpoint 1A按resolved route迁移到initial-domain唯一
  global Stack与logical-interface owner；未要求shared/vendored surface、target、owner、ABI、visible semantics或
  acceptance变化。`NET-UDP-DOMAIN-CUTOVER`原子生效，Stage 1 Closed；Stage 2仍Outline且`1 -> 2`gate未执行。
  精确source、review、validation与Not Run边界见
  [transaction](../../devlog/transactions/2026-07-29-net-udp.md#2026-07-29---stage-1-checkpoint-1a-implementation-validation-and-domain-cutover)。
- `2026-07-29`：`1 -> 2` Implementation Resolution / Module and Route Selection。live source确认Stage 0 local-link
  candidate可由Stage 1 DomainStack直接激活，不需要新probe或vendored change；同时确认kernel domain/worker、stack
  root/pump和xtask build materializer都需要在加入新职责前做same-owner split。resolution把Stage 2解析为2A
  split-only与2B atomic static-control/loopback cutover，保持R0 target/owner/ABI/Contract Impact/acceptance不变；
  Stage 2达到Ready / Not Active。精确preflight见
  [transaction](../../devlog/transactions/2026-07-29-net-udp.md#2026-07-29---stage-1---stage-2-implementation-resolution-gate)。
- `2026-07-29`：Checkpoint 2A / Same-owner Split Closure。kernel domain/worker、stack root/pump与xtask generated-def
  materializer已按resolved roles目录化，获批的`resolve.rs`扩展仅修正与canonical `.defconfig`不一致的test fixture。
  final diff没有新增type/method/config field、扩大public surface、改变owner/behavior/generated text或触碰current
  contract；全部2A validation gate与先前独立source review通过，开发者在closure时明确取消额外final exact-diff
  review。Contract Impact为None，Checkpoint 2B保持Ready / Not Active。精确证据见
  [transaction](../../devlog/transactions/2026-07-29-net-udp.md#2026-07-29---stage-2-checkpoint-2a-activation-and-same-owner-split)。
- `2026-07-30`：Checkpoint 3C / Correctness Feedback and Closure。初次独立source review发现RX aggregate credit
  recovery、fresh oversize implicit-bind retention与specific `127/8` source三项Apollyon；全部在Stack/File operation/
  control-plane既有owner内修复，并以无手工pump host recovery、fresh oversize `getsockname` retention与specific
  loopback runtime补齐证据。修复后final validation与复审通过，Stage 3独立Closed；Contract Impact为None，candidate
  contracts继续Pending。精确证据见
  [transaction](../../devlog/transactions/2026-07-29-net-udp.md#2026-07-30---stage-3-checkpoint-3c-implementation-review-repair-and-closure)。
- `2026-07-30`：`3 -> 4` Implementation Resolution / Source and Wait Route Selection。live source确认Stage 4可在
  现有Endpoint/Socket/iomux owner上形成完整复数route source，并让blocking syscall复用同一predicate与shared wait
  loop；不需要修改ABI/library、opened-description、epoll或vendored smoltcp。resolution把Stage 4解析为4A complete
  plural-route source、4B blocking syscall与4C race/fragment/evidence closure；multi-waiter是4A/4B固有能力，不是4C
  capability expansion。类型名、event batching、helper落点、容器与checkpoint内部顺序保留有边界的Route Correction，
  但single-waiter、second wait loop、duplicate readiness truth或降低evidence floor不在弹性范围。Contract Impact为
  None，candidate contracts继续Pending；Stage 4达到Ready / Not Active，精确preflight见
  [transaction](../../devlog/transactions/2026-07-29-net-udp.md#2026-07-30---stage-3---stage-4-implementation-resolution-gate)。

## 14. Target Renegotiation Gates

当前没有proposed gate。只有真实接口、代码、测试或集成证据表明R0 target代价/可行性需要重新判断时才增加。
提案不等于批准；决定前当前stage保持停止且不得cut over。correctness invariant不能作为reduced target妥协项。

## 15. Write Set扩展记录

- `2026-07-30`：final review继续要求把每个interface的private `SocketStorage` layout纳入KernelConfig predicate，
  并为external/local `SocketSet`按Endpoint capacity eager reserve。复核证明该条件只在约`4.6e16`个Endpoint起与
  Endpoint layout分离；没有物理系统能够成功publication该数量的Endpoint，要求通过eager allocation证明某个private
  container一定先失败也不能增加可用保证。预分配还会把UDP namespace policy传播到interface/local-link owner，
  或把逻辑上限变成Stack启动期内存承诺，并把lazy storage改成更早OOM的eager allocation。开发者据此要求恢复自然实现形状：
  不批准`stack/interfaces.rs`或`local_link.rs`扩集，删除cross-crate private-layout predicate，kernel只验收semantic与
  direct payload-byte arithmetic；Endpoint collection与SocketSet都按实际publication lazy growth。原finding按
  Neutralized处理；Contract Impact为None，R0 target、
  normal capacity semantics、owner、ABI与3C Not Active边界不变。

- `2026-07-30`：3B final review发现anonymous UDP inode仍以`InodeType::Regular`创建，`fstat/statx`错误投影
  `S_IFREG`；修复还要求把namespace-wide capacity/range收归Stack构造，并让kernel compilation通过stack-private
  actual allocation layout验收全部UDP参数语义。worker按6.3.10停止并报告后，开发者明确回复`approved`，批准：
  `anemone-kernel/src/fs/inode.rs`、`fs/api/getdents64.rs`、`fs/ext4/{mod.rs,inode.rs,superblock.rs}`进入3B manifest；
  原manifest内的`anemone-net-api`/stack public shared surface可以最小调整为构造时namespace policy与无私有类型
  泄漏的const layout predicate。`fs/socket/udp.rs`改用Socket inode，`anemone-rs`既有manifest surface增加窄
  `statx` wrapper供`udp-test`同时验证`fstat/statx`。Contract Impact为None：不改变anonymous/VFS lifecycle、
  opened-description owner、current contract、R0 ABI visible semantics或acceptance。validation必须覆盖temporary
  invalid-KernelConfig kernel compile failures、host ownership/layout regressions、KUnit与`udp-test` S_IFSOCK、
  双架构build、RV64 fresh-disk runtime、write-set/source audit和独立exact-diff review；3C保持Not Active。

- `2026-07-30`：后续backend admission复核中，开发者指出ramfs等backend是否纳入取决于实际需求。live source
  证明ramfs/proc创建入口只接收各自hard-coded类型，无Socket输入能力；devfs public `DevfsNodeAttr.ty`配合
  “leaf只要不是Dir”负向检查会在新增枚举后意外接纳Socket。为保持本stage Socket仅由anonymous VFS承载，
  `anemone-kernel/src/fs/devfs/mod.rs`加入3B manifest，只允许publish入口显式拒绝Socket及对应owner-local KUnit；
  ramfs/proc保持只读。Contract Impact仍为None，不改变public owner、ABI visible semantics、lifecycle或acceptance，
  并继续要求全部3B validation和final exact-diff review。

- `2026-07-29`：2A mandatory `just xtask-test`发现HEAD的
  `resolved_selection_owns_all_snapshot_inputs`仍期待`max_logical_cpus = 16`，而canonical `conf/.defconfig`已为`1`；
  两文件均无2A diff。worker按停止合同上报后，开发者明确批准把`scripts/xtask/src/config/resolve.rs`加入2A
  manifest，仅修正该test fixture/expected snapshot。Contract Impact为None，不改变owner、public API、ABI、
  visible semantics或acceptance；扩展后必须重跑全部2A validation与final review。
- `2026-07-29`：2B generated-output audit发现authoritative manifest要求ignored
  `anemone-kernel/src/network_defs.rs`，但live `anemone-kernel/.gitignore`没有对应规则。worker按停止合同上报后，
  开发者明确批准把`anemone-kernel/.gitignore`加入2B manifest，只增加`src/network_defs.rs`一行。Contract Impact
  为None，不改变owner、public API、ABI、visible semantics或acceptance；扩展后必须用`git check-ignore -v`证明
  四份generated kernel input均命中明确规则，并继续执行全部2B validation与final review。

Stage 1按冻结manifest完成；`1 -> 2`resolution为future Outline首次解析并冻结Stage 2 manifest。除上述获批2A
test-only修正外，2A/2B Active后任何manifest外tracked write仍须先上报。

## 16. 结构维护记录

Stage 0在0B Feedback Correction中执行same-owner private module split：ordinary Stack owner位于
`src/stack/mod.rs`，conditional facade/DTO/conversion位于`src/stack/host_validation.rs`；0C确认该拆分长期保留到
Stage 1已经由production initial-domain唯一Stack接管ordinary aggregate candidate owner。`0 -> 1`resolution确认Stage 1不替代
host-only control/observation，因此conditional facade继续只服务长期deterministic matrix，待Stage 2/3真实
control-plane/Endpoint consumer出现时按方法逐项删除或保留；它仍不进入kernel dependency。拆分没有改变public
production API、owner或shared contract。

`1 -> 2`resolution确认继续向现有flat files加入职责会扩大proof surface，因此把2A冻结为独立split-only
checkpoint：kernel domain按membership/Stack composition拆分，worker按control/external progression拆分，stack
root/pump按interface/UDP/local progression拆分，xtask build把generated-def rendering移入child module。该决定只
改变同owner physical layout，不建立新facade、public API或contract；是否成功必须由2A final diff和validation证明。

`2 -> 3`resolution同样把3A冻结为split-only checkpoint。3A现已把protocol UDP按Endpoint resource、bounded
namespace与datagram ownership phase目录化，把Stack UDP operation与DomainStack KUnit-only probe window移入各自
同owner child module；为恢复原flat-module访问域只使用parent-scoped `pub(super)`与crate-private re-export，没有
新增public surface、semantic owner、consumer或behavior。3B随后已按冻结边界引入真实Endpoint/File/address纵切，
并删除temporary probe；3C仍须在独立授权下增加nonblocking datagram transaction。
