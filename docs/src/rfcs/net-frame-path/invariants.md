# Network Frame Path 目标与不变量

**状态：** R1 / Closed / Effective via `NFP-FINAL-CUTOVER`
**最后更新：** 2026-07-27
**父 RFC：** [RFC-20260726-net-frame-path](./index.md)
**适用修订：** `R1`

本文保留`net-frame-path` R1的accepted contract delta、target invariants与RFC-local proof obligations。
`NFP-FINAL-CUTOVER`已经完成；当前共享规则以[Network current contracts](../../contracts/net/index.md)与
[System Power shutdown lifecycle](../../contracts/power/shutdown-lifecycle.md)为唯一权威，本页不成为并列
current contract。

2026-07-27 Stage 4已关闭post-close反馈，使live implementation重新符合已经生效的owner、publication与attach
规则；它没有改变本页target invariant、Contract Impact或`NFP-FINAL-CUTOVER`的历史生效事实。

## 规则分类

- **Correctness Invariant：** owner、ownership、并发、lifecycle、cleanup、DMA safety、object
  visibility 和状态真相源规则；违反即实现不正确，不能以工程成本为由接受。
- **Target Guarantee / Capability：** 多设备、RV64 production vertical slice、bounded pump、host/QEMU proof 等
  本修订承诺；可以通过 `Target Renegotiation Gate` 形成更窄的新修订，但在此之前保持约束力。
- **Implementation Preference：** trait 拆分、type name、pool/backing、worker 数量、budget 算法、
  lock primitive 和模块布局；除非它们会改变上述 owner 或 correctness，否则不写成 invariant。

## Contract Impact

| Contract ID | 变化 | 当前规则 | Target 摘要 | 生效 Gate |
| --- | --- | --- | --- | --- |
| `SYSTEM-POWER-ORDERLY-001` | Refine | [Active：`filesystem -> network -> device`静态plan](../../contracts/power/shutdown-lifecycle.md#system-power-orderly-001) | 在`device`前显式调用network owner的有限best-effort cleanup facade；保持single episode、静态plan与fail-forward | `NFP-FINAL-CUTOVER`（Effective） |
| `NET-BOUNDARY-001` | Introduce | [Active](../../contracts/net/frame-path.md#net-boundary-001--frame-slice依赖方向与object-fence) | 固定frame slice的依赖方向、共享语义与object fence | `NFP-FINAL-CUTOVER`（Effective） |
| `NETDEV-LIFE-001` | Introduce | [Active](../../contracts/net/netdev-lifecycle.md#netdev-life-001--boot-time-identity与publication是单向transaction) | 固定boot-time netdev identity、publication、link facts与lifecycle owner | `NFP-FINAL-CUTOVER`（Effective） |
| `NET-FRAME-OWN-001` | Introduce | [Active](../../contracts/net/frame-path.md#net-frame-own-001--frame-backing只有一个访问owner) | 固定RX/TX backing的独占ownership、consume scope与completion handoff | `NFP-FINAL-CUTOVER`（Effective） |
| `NET-FRAME-PROGRESS-001` | Introduce | [Active](../../contracts/net/frame-path.md#net-frame-progress-001--有界资源normal-backpressure与durable-recheck) | 固定有界resource、normal backpressure、recheck与跨方向进展边界 | `NFP-FINAL-CUTOVER`（Effective） |
| `NET-STACK-PUMP-001` | Introduce | [Active](../../contracts/net/frame-path.md#net-stack-pump-001--stack-instance唯一推进protocol-state) | 固定stack instance唯一逻辑推进owner、显式时间与bounded pump | `NFP-FINAL-CUTOVER`（Effective） |
| `NET-ATTACH-001` | Introduce | [Active](../../contracts/net/attach-lifecycle.md#net-attach-001--attach-publicationrollback与best-effort-shutdown) | 固定netdev attach publication、rollback与best-effort shutdown cleanup handoff | `NFP-FINAL-CUTOVER`（Effective） |

六个network规则在R1 acceptance时没有effective baseline，因此delta仍记为`Introduce`；它们现已Active。
System Power已有current contract，本RFC只Refine显式orderly participant plan；episode、fail-forward、
emergency与machine semantics保持不变。

## Target Invariants

### NET-BOUNDARY-001 — frame slice 依赖方向与 object fence

**分类：** Correctness Invariant

**规则：** `anemone-net-api` 只定义 kernel side 与 concrete protocol stack 共同需要的 opaque
identity、frame capabilities/outcomes、link/interface facts、monotonic time 和 pump/recheck 语义。
它不得依赖 `anemone-kernel` 或 concrete smoltcp object；`anemone-smoltcp-stack` 可以依赖
`anemone-net-api` 与 `smoltcp`，但不得依赖 kernel task/fd/wait object；driver/device 层只停留在
frame/link 层，不得依赖 endpoint、socket 或 protocol object。

**Owner：** `anemone-net-api` 拥有共享 semantic surface，但不拥有 runtime mutable state。runtime
state 仍由 netdev、frame provider、stack instance 与 kernel attach authority 分别拥有。

**参与方局部义务：**

- driver 不公开 descriptor、DMA address、VirtIO header 或 hardware queue token；
- `device/net` 不公开 driver backing 或 transport state；
- stack 不公开 smoltcp object identity，也不接收 fd/task/wait capability；
- kernel attach layer 不复制 driver queue truth 或 `InterfaceId -> smoltcp object` mapping；
- `anemone-net-api` 不解释 Linux errno/readiness，也不形成第二个 net core。

**违反表现：** driver 直接实现并公开 `smoltcp::phy::Device`；shared API 返回 concrete kernel/
smoltcp object；kernel 根据 smoltcp handle 做状态决策；API crate 持有并行 runtime registry。

**Cutover：** source/dependency audit 与 host build 证明 object fence；真实 stack/provider 组合证明
共享 surface 足够完成 frame slice，而不是靠 downcast 或私有旁路汇合。

### NETDEV-LIFE-001 — boot-time netdev identity、publication 与 link lifecycle

**分类：** Correctness Invariant + Target Capability

**规则：** `device/net` registry 是 netdev identity、ifindex/name、规范化 link facts 和已发布 frame
capability 的唯一 subsystem owner。driver 先完成 owner-local hardware/frame 准备，最后一次性发布
netdev；probe 失败不得留下已发布条目。已发布 identity 在本次 boot 内稳定且不复用。

**Owner：**

- `device/net`：netdev registry、identity、规范化 link/interface snapshot publication；
- concrete driver/frame provider：hardware queue、DMA、completion、实际可观察的 link/resource truth；
- generic bus：device binding fact；它不拥有 netdev lifecycle。

**规则细化：**

- generic `Device` identity、netdev identity/ifindex、`InterfaceId` 与 hardware token 不等价；
- published/unattached netdev 是合法状态，不能用另一个综合 lifecycle cache 覆盖该事实；
- link-up 不是 publication 或 attach 前提；link-down 不撤销 identity、frame capability 或
  `InterfaceId` mapping；
- 通知只要求重新读取 owner fact，不能成为第二份 link/resource state；
- provider 无法证明的 link fact 必须表示为 unavailable/unknown 或省略，不能伪造成 link-up；
- 第一版只支持 boot-time persistent netdev；shutdown admission closure/cleanup attempt 后不移除、不复用
  identity、不重新激活。

**失败与 cleanup：** probe failure 可以让 generic device 保持 unbound/inert。若完整回收需要扩大
generic bus rollback，允许保留本次失败尝试的 owner-local resource；仍可能被 device 访问的 backing
绝不能提前释放或复用。

**违反表现：** 先注册 netdev 再准备 queue/IRQ；失败后留下半初始化 registry entry；用 link-down
触发 identity 重建；ifindex 与 `InterfaceId` 共用同一个可解引用 ID；多个 registry 缓存不同 lifecycle。

**Cutover：** registry/source audit、host 双实例隔离，以及 RV64 的 probe/publication/link fact runtime
evidence。

### NET-FRAME-OWN-001 — frame backing 的独占 ownership 与 handoff

**分类：** Correctness Invariant

**规则：** 每个 frame backing 在任一时刻只有一个有权访问或推进它的 owner。frame provider 通过
move-only token 授予一次性 consume scope；共享边界只暴露有效 Ethernet frame 区域，不暴露
backing/descriptor/DMA identity。

RX ownership：

```text
provider available -> device/DMA owned -> completion committed
-> CPU-visible token -> protocol consume scope -> provider recyclable
```

TX ownership：

```text
provider reservable -> CPU fill scope -> submit committed
-> device/DMA owned -> completion committed -> provider reclaimable
```

**Owner：** concrete frame provider instance。host provider 与 VirtIO-Net 各自拥有自己的 backing；
共享 contract 只约束一致的 handoff 语义。

**规则细化：**

- RX token 只在 `consume` 期间提供 `&[u8]`，callback 返回后由 provider recycle；
- TX token 只在 `consume(len, ...)` 期间提供有效 `&mut [u8]`，callback 返回并提交后，上层失去访问权；
- 未 consume 的 token 在 `Drop` 时取消 reservation 并归还 owner，不依赖额外 cleanup call；
- TX completion 前不得读写或复用 backing；RX completion commit 前不得由 CPU 读取，consumer
  释放前不得重新投递；
- acquire、consume finalization 与 completion 可以短暂进入 owner-local critical section，但
  protocol callback 期间不得持有 device-wide lock；
- RX consume 中可能取得配对 TX token；该嵌套路径不得重入同一把全局锁或依赖共享可变 slice；
- 普通 frame 默认以 ownership move 流转，不通过共享锁引用维持并列访问者。

**违反表现：** use-after-submit、double recycle、double completion、DMA 与 CPU 并发访问同一
mutable bytes、callback 返回后保存 raw slice、stack 持有 descriptor token、drop token 泄露 credit。

**Cutover：** host deterministic ownership/failure injection、VirtIO source audit，以及 RV64 QEMU
runtime 的双向 RX/TX/completion 证据。任何 unsafe bridge 必须有局部 safety proof，说明 buffer
identity、lifetime、sync point 与 error cleanup。

### NET-FRAME-PROGRESS-001 — 有界资源、normal backpressure 与 recheck

**分类：** Correctness Invariant + Target Guarantee

**规则：** RX backing、TX backing、submit descriptor/credit、同时存活的 DMA mapping 与 queued
completion 都有明确上界。frame/queue 资源暂不可用返回 normal outcome；不得因这类 exhaustion
panic、无限 spin 或隐式创建无界资源。completion、resource return 或 link/resource fact 变化后
必须发布或保留可被 worker 重新观察的 durable predicate，使停顿方向能够恢复。

kernel heap / physical allocator 的全局 OOM 不属于 frame/queue normal exhaustion。当前
`virtio-drivers::Hal::share()` 无 fallible return，`VirtIOHalImpl` 的 bounce allocation 失败可能导致
kernel panic；本修订接受该系统级失败边界，不承诺把它转换为网络 backpressure。

**Owner：** frame provider 拥有 resource count、queue truth 与 completion；worker 只消费 recheck
edge 并重新读取，不缓存并列 resource truth。

**规则细化：**

- production frame admission 可以包含简单、适度的 per-operation allocation；同时存活的 backing、
  mapping 与 metadata 必须受 frame/queue credit 约束，并由 completion 或取消路径确定回收；
- allocation-free、统一预分配 pool 或 `virtio-drivers` fork 不是 correctness 要求；capacity reserve、
  owner-local reuse 与 high-water 观测只在保持局部和简单时作为 best effort；
- hard IRQ 只处理 owner hardware facts、最小 ack/mask/unmask 与 recheck publication，不承担 frame
  submit/recycle 或 protocol callback；简单有界的 IRQ-safe allocation 可以发生，但不得引入无界增长、
  blocking/reclaim protocol、普通锁依赖、复杂 drop/callback 或第二套资源 truth；
- queue full、RX empty、TX credit exhausted 与 link unavailable 不是 fatal error；
- wake edge 可以 coalesce 或丢失重复边沿，但 durable predicate 不能丢；worker 每次醒来重新读取；
- 普通软件 budget/credit 策略不能让 ingress 或 egress 无限期饿死另一侧；真实 device saturation
  可以造成暂时停顿，但资源恢复后必须重新参与调度；
- 本规则不承诺每次 pump 的具体 packet 数、不承诺固定 response reserve，也不要求无条件 RX/TX
  同时进展。

**依赖：** `NET-FRAME-OWN-001`。

**违反表现：** queue full 触发 panic；live bounce mapping 超过 credit 上界或 completion 后泄漏；
为了消除适度 allocation 引入跨层 DMA identity、侵入式 frame 或镜像 credit truth；TX exhaustion 后
completion 无法唤起重查；notification 自身被当作唯一 state；一侧在普通 workload 下永久饥饿。

**Cutover：** host real-stack + deterministic provider 的 exhaustion/completion/recheck/fairness tests、IRQ
source audit、VirtIO mapping identity/lifetime/reclaim audit，以及 QEMU bounded burst 的真实 TX/RX
completion、IRQ recheck、outstanding 上界、live mapping 回落与正常关机 evidence。QEMU 若自然观察到
exhaustion，必须额外证明其后恢复提交和 completion；未自然观察到不替代、也不否定 host exhaustion proof。

### NET-STACK-PUMP-001 — stack instance 的唯一逻辑推进 owner

**分类：** Correctness Invariant + Target Guarantee

**规则：** 一个 concrete stack instance 独占它的 interface resources、private smoltcp objects、
`InterfaceId` mapping、protocol deadline 和后续 transport resources。同一 instance 任一时刻最多
一个 pump 持有推进能力；所有 protocol mutation 通过该独占边界串行化。

**Owner：** `anemone-smoltcp-stack` 的 concrete stack instance。worker、IRQ、timer 与 kernel attach
authority 只持有 wake/work capability 或 opaque identity，不成为 protocol state owner。

**依赖：** `NET-FRAME-OWN-001`、`NET-FRAME-PROGRESS-001`。

**规则细化：**

- 多 interface 可以属于同一 instance，但第一版不承诺 instance 内并行 pump；
- 多个物理 worker 可以竞争推进 capability，但不能同时进入；不要求固定 CPU/thread affinity；
- pump 使用调用者提供的 monotonic instant，不读取 wall clock 或隐藏的另一条时间线；
- 一次 pump 的 ingress、egress 与 maintenance 工作有明确 finite budget；不得调用无界
  `Interface::poll()` 作为普通 worker action；
- pump outcome 至少能表达 work remaining/immediate recheck 与 next monotonic deadline；精确类型和
  字段不是 invariant；
- timer/wake/IRQ 只请求重新推进，不能在 owner 外修改 smoltcp object 或发布 Linux readiness。

**违反表现：** 两个 worker 同时 poll 同一 `SocketSet`/interface；IRQ 直接进入 smoltcp；kernel
缓存 smoltcp handle 并修改 protocol state；pump 在 queue 持续有包时无界运行；deadline 使用 wall time。

**Cutover：** host concurrency/serialization、budget/deadline tests，source audit 与 QEMU worker/timer
wiring evidence。

### NET-ATTACH-001 — attach publication、rollback 与 best-effort shutdown cleanup

**分类：** Correctness Invariant

**规则：** `anemone-kernel::net` 的唯一 attach authority 拥有 netdev 到 concrete stack 的 attach
transaction、active-path publication 与 network-local shutdown cleanup。System Power 继续唯一拥有
global terminal episode 和 orderly participant 顺序；`anemone-smoltcp-stack` 仍独占 `InterfaceId`
分配和 private mapping；driver 不反向访问 attach authority 或 protocol stack。

**合法 durable facts：**

- generic device discovered/unbound；
- netdev published/unattached；
- active mapping 已发布并允许 pump；
- shutdown admission 已关闭/cleanup 已尝试，作为本次 boot 的终态。

这些是不同 owner fact 的合法组合，不要求一个长期 `NetdevLifecycle` enum 或综合 cache。

**Attach handoff：**

1. authority 取得 published netdev capability，但不复制 driver queue truth；
2. stack owner 分配 `InterfaceId` 并建立 private mapping；
3. authority 准备 worker/wake/time wiring；
4. 全部成功后一次性发布 active path；此前的 stack-attached 只属于 transaction-local fact；
5. 失败时撤销 stack-local partial result，netdev 回到 published/unattached；其它 netdev 不回滚。

**Orderly shutdown handoff：**

1. `power` 在静态 plan 中、`device::shutdown()` 前调用唯一 network owner facade；
2. authority 关闭或抑制新的 pump、timer/wake 与 frame-token acquisition，并用既有 owner/token/worker
   机制做一次有限 cancel/drain attempt；
3. cleanup 不要求新增 shutdown-only lifecycle cache、通用 refcount/drain/timeout framework或重塑
   driver/frame 对象模型，也不保证全部访问退出、全部资源回收或 callback 有界返回；
4. driver 只完成 owner-local IRQ mask、queue/DMA/device reset/quiesce，不反向协调 stack/worker；
5. 任一 owner 无法证明 resource 已不再被 CPU/device 访问时，保留到 reset/power-off，不提前释放或复用；
6. emergency 路径按
   [`SYSTEM-POWER-EMERGENCY-001`](../../contracts/power/shutdown-lifecycle.md#system-power-emergency-001)
   跳过全部 ordinary participant，不调用 network cleanup。

**失败边界：** 第一版不要求 attach retry、runtime detach、完整 teardown 或 shutdown progress/
reclamation guarantee。cleanup callback 可以因已停止 CPU、在途访问或 owner-local lock 不前进；若它
选择返回，后续 driver 必须能在不完整前态上安全跳过不支持的回收。预期平台 NIC 未进入 active path
仍是 runtime acceptance failure，但不授权另一个 owner 绕过 attach authority。

**违反表现：** driver probe 直接推进 smoltcp；kernel 与 stack 各存一份 mapping；active path 在
worker/time 未准备好前可见；attach failure 注销 netdev；driver shutdown 反向协调 stack/worker；为了
shutdown 新建平行生命周期真相；在仍可能访问时释放/复用 DMA；第二张网卡失败回滚第一张 active path。

**Cutover：** attach rollback/independence host tests、publication source audit、
`SYSTEM-POWER-ORDERLY-001` 静态 plan Refine，以及 RV64 的 active attach、network cleanup 在 driver
shutdown 前被调用和不安全资源不回收 evidence；不要求证明完整 teardown 或全部资源回收。

## RFC-local Invariants

### NFP-PROOF-001 — Host evidence 必须由真实两侧汇合

host frame tests 必须至少组合真实 `anemone-smoltcp-stack` 与实现正式 frame contract 的真实 test
provider。fake/fake 只可以做局部 negative test，不能证明 shared API 足以支撑纵切。

### NFP-PROOF-002 — RV64 production runtime 不可由 host 证据替代

RV64 virtio-mmio 是本修订唯一 production runtime target。RV64 未运行、只 probe 未收发、只 TX 未 RX
或只 polling 未 IRQ 时，production runtime 结论保持 Not Run / Not Cut Over。LA64 / virtio-pci 不属于
本修订 build、runtime 或 cutover floor，不能被顺带写成 coverage。

### NFP-PROOF-003 — 多设备是隔离能力，不是容器形状

至少两个 provider/netdev/interface 实例必须证明 identity、frame credit、completion、link facts、
mapping、pump 和 failure 隔离。`Vec`/map 能存两个元素或避免 global static 本身不是证据。

第一版允许由 host 完成双实例证明、RV64 QEMU 验证单实例 hardware vertical slice；不因此引入
route/control plane。

### NFP-PROOF-004 — 实现反馈不得静默弱化 target

如果 `VirtIOHalImpl`、IRQ routing、shutdown hook、smoltcp token 或 budget 行为使目标无法以合理
成本落地，当前 gate 必须在 cutover 前停止。证据回写 RFC review 后，由有权 reviewer 选择 route
correction、accepted reduced target、follow-up RFC 或 Not Cut Over；不能用更强 fake、缩小 case 或
只验单向收发/无 IRQ 路径冒充 closure。

### NFP-PROOF-005 — Exhaustion 与 production runtime 使用互补证据

normal exhaustion/recovery 必须由真实 stack 与实现正式 frame contract 的 deterministic host provider
确定性制造：test owner 可以控制自己的有限 credit、matching completion、link 与 recheck edge，但这些
control 不得进入 production capability。RV64 QEMU 必须独立证明真实 VirtIO queue/DMA/IRQ/worker 路径的
bounded completion/reclaim；不能因为 TCG 回收足够快而把 `queue-full == 0` 判为 production failure，也不能
通过延迟 completion、降低 production capacity、伪造 queue state 或 diagnostics 驱动行为来制造 exhaustion。

若 RV64 自然出现 exhaustion，验收必须证明之后仍有新 submission、matching completion、IRQ recheck 与
最终 mapping baseline；若没有出现，只能结论为“本次 production burst 未观察到 exhaustion”，host 的确定性
proof 仍必须独立通过。

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| generic device binding | generic bus / `Device` | driver capability | 是否已绑定 driver |
| netdev identity / registry entry | `device/net` | opaque netdev handle / snapshot | lookup、publication、attach input |
| hardware queue、DMA、completion | concrete driver/provider | one-shot frame token、recheck edge | frame I/O 与资源回收 |
| normalized link facts | `device/net`，来源为 provider observation | snapshot + recheck edge | 上层重查；不拥有 lifecycle |
| `InterfaceId` allocation / mapping | concrete stack instance | opaque `InterfaceId` | protocol interface addressing |
| smoltcp interface/resource | concrete stack instance | 无 direct handle | protocol progression |
| active attach publication | kernel attach authority | active capability / recheck handle | 允许 worker/pump 进入 |
| global orderly participant 顺序 | System Power | network owner facade | 在 device shutdown 前调用 network cleanup |
| network-local shutdown cleanup | kernel attach authority | best-effort cancel/drain attempt | 关闭/抑制新入口并尝试收口现有工作 |
| worker wake edge | wake owner | edge only | 请求重查，不是 durable truth |

诊断统计、queue high-water、provider label 或 hardware token string 不参与 owner 决策；若实现保留这类
字段，必须明确它们可 stale 且只服务诊断。

## 身份与能力模型

- netdev identity 与 ifindex/name 在 boot 内稳定，但不承诺跨 boot 稳定；
- `InterfaceId` 由 stack owner 分配，只在该 owner 的有效 boot lifecycle 内解释；
- frame token 表示一次 reservation，不表示 device、netdev、interface 或 queue identity；
- wake/recheck capability 只允许请求 owner 重查，不允许调用者读取或修改 private state；
- active-path capability 只在 attach publication 后取得，quiesce publication 后不再签发；
- 没有 runtime removal，因此第一版不定义 generation、stale handle recovery 或 identity reuse。

## 线性化点

这里只固定必须存在的可见边界，不预选具体 lock/atomic/type：

- **netdev publication：** driver owner-local frame/IRQ 准备完成后，registry entry 首次可见；
- **RX CPU ownership：** completion 被 provider 正式收割并完成必要 DMA sync 后，RX token 才可见；
- **TX device ownership：** filled bytes 与 descriptor/queue submit 一起提交后，上层访问权终止；
- **completion reclaim：** provider 确认 used completion 并完成必要 sync 后，backing 才可重用；
- **active attach publication：** mapping、worker、wake 与 time wiring 全部准备后，active capability 可见；
- **link fact publication：** provider observation 被 `device/net` 规范化后；notification 只指向重读；
- **network shutdown publication：** authority 关闭/抑制新进入并做一次有限 cleanup attempt；它不是完整
  drain/reclamation 证明，driver shutdown 在其后且不得回收仍可能被访问的 resource。

后续 implementation 必须为每个边界指定 live primitive 与 failure path。若无法用一个明确的 commit
point 表达，返回 RFC review，而不是增加第二套 lifecycle state。

## 锁序与生命周期规则

- protocol callback 期间不持有 device-wide queue/registry lock；
- frame provider critical section 不调用 kernel attach、worker scheduling 或 smoltcp owner 的复杂
  callback；先提交 owner fact，再发布 recheck edge；
- hard IRQ 不获取可能睡眠或普通锁语义的锁，不执行 frame submit、protocol callback、无界
  collection growth 或 complex drop；简单有界的 IRQ-safe allocation 不因此被禁止；
- stack pump 的独占边界不反向进入 driver registry mutation；frame access 只通过窄 capability；
- attach transaction 不能在持有 netdev registry write lock 时等待 worker、timer 或 protocol callback；
- cleanup / `Drop` 先撤销 publication/reservation，再用 `assert!` 暴露局部不变量；
- probe/shutdown 若不能安全回收，优先保留资源到 reset，而不是释放仍可能被 DMA/worker 访问的对象；
  不为提高 shutdown cleanup 完整性单独增加生命周期 cache、通用 drain/refcount 或 timeout 机制。

精确锁序、primitive、是否等待与局部 wait strategy 需要在第一个相关 Ready stage 结合 live source
解析；本页不为尚不存在的类型编造 lock graph，也不把 timeout framework 设为前提。

## 禁止退化项

- 不得建立能从 binding、registry、mapping 和 quiesce owner facts 推导出的综合 lifecycle cache；
- 不得把 recheck edge、IRQ bit、统计计数或 diagnostic owner label 反向用于 durable state 决策；
- 不得让 `device/net`、kernel attach layer 或 shared API 解引用 smoltcp private object；
- 不得把 `InterfaceId`、ifindex、descriptor token 与 frame token 合并为一个“通用 ID”；
- 不得因 queue exhaustion、temporary link-down 或 frame/queue credit shortage panic/busy-spin；全局
  allocator OOM 的 kernel-fatal 边界必须显式记录，不能伪装成可恢复；
- 不得在 IRQ/IRQ-off 路径引入无界增长、blocking/reclaim protocol、普通锁、日志格式化 heavy path、
  complex callback/drop 或第二套资源 truth；
- 不得仅为消除适度 IRQ-off allocation 引入侵入式 frame/pool、跨层 DMA token、镜像 credit 状态或
  `virtio-drivers` fork；
- 不得用一个 global netdev/stack singleton 满足多设备 target；
- 不得为了 trait-object 方便公开 concrete buffer，或为尚无 consumer 的 mbuf/lease framework 固定 API；
- 不得由 driver shutdown 反向协调 protocol owner，也不得释放/复用仍可能被访问的 frame/DMA resource；
- 不得把 endpoint/socket/readiness/control-plane 旁路塞进 frame RFC 的 shared API。

## 完成标准

R1 文档层 acceptance 已确认：

- 六个 proposed network contract IDs 与 `SYSTEM-POWER-ORDERLY-001` Refine 的 owner、依赖、failure 与
  cutover proof 已经 review；
- [Tracking Issues](./tracking-issues.md) 的 Keter 已 neutralize，或由单独授权的
  `implementation.md` 路由到带 protected boundary、resolution trigger、failure signal 与停止条件的
  明确 gate；相关阶段在执行前必须独立达到 Ready；
- target guarantee、correctness invariant 与 implementation preference 没有混写；
- 没有为未来 hotplug、transport、socket 或第二种 NIC 预建通用抽象。

RFC 最终完成至少要求：

- `NFP-PROOF-001` 到 `NFP-PROOF-005` 全部有可审计证据；
- RV64 runtime、host ownership/exhaustion/multi-instance 与 source audits 全部达到 floor；
- 六个 network IDs 与 `SYSTEM-POWER-ORDERLY-001` Refine 在 `NFP-FINAL-CUTOVER` 原子写入 current
  contract 并成为 Effective；
- 没有 Transitional、模糊 pending、host-only 或单向路径冒充 closure；
- 未进入 target 的 socket/control-plane/hotplug 仍由后续 RFC 明确拥有。
