# RFC-20260726-net-frame-path

**状态：** Accepted for Implementation
**修订：** `R1`
**负责人：** doruche
**最后更新：** 2026-07-27
**领域：** network-device / VirtIO / frame-path / smoltcp integration
**事务日志：** [2026-07-26 net-frame-path](../../devlog/transactions/2026-07-26-net-frame-path.md)
**影响契约：** `NET-BOUNDARY-001`、`NETDEV-LIFE-001`、`NET-FRAME-OWN-001`、
`NET-FRAME-PROGRESS-001`、`NET-STACK-PUMP-001`、`NET-ATTACH-001`（均为 proposed
`Introduce`；尚未生效）；
[`SYSTEM-POWER-ORDERLY-001`](../../contracts/power/shutdown-lifecycle.md#system-power-orderly-001)
（proposed `Refine`）
**开放问题：** [Tracking Issues](./tracking-issues.md) 当前没有 Apollyon、Keter 或 Euclid；NFP-007
已由 R1 proof-boundary correction neutralize
**下一步：** Stage 2 Checkpoint 1已关闭；Checkpoint 2仍为Not Activated / Unauthorized，后续只能在新的独立
授权下进入host ownership/progress/pump conformance，不得由本次closure自动激活

> 本目录是 `net-frame-path` R1 accepted target 的公共 canonical source。R1 尚未成为 current contract；
> 六个 network IDs 与 power Refine 只可在 `NFP-FINAL-CUTOVER` 原子生效。

## 摘要

`net-frame-path` 是网络领域三份 sibling RFC 中的第一份。它建立从 boot-time NIC discovery、
VirtIO-Net driver、netdev registry、frame handoff 到真实 smoltcp interface bounded pump 的最小
可验证路径，为后续 `net-udp` 和 `net-tcp` 提供可复用的设备、帧、时间与推进 contract。

本 RFC 不提前实现 endpoint、kernel socket object、socket syscall 或 IP control plane。它也不
以一套通用 packet/mbuf/lease framework 作为前提：只有会改变核心对象 owner、frame ownership、
跨层 object fence 或 lifecycle 的规则现在固定；具体 backing、pool、worker、budget 和内部类型
由实现证据反推。

## 背景与当前基线

当前仓库已经具备网络纵切所需的若干基础，但还没有可用的生产 frame path：

- generic `Device` / `Driver`、platform / PCIe / VirtIO bus 已能发现并匹配设备；
- 本修订的 production 验收平台 RV64 QEMU 已提供 `virtio-net-device`，virtio-mmio transport 能建立
  `VirtIODevice`；LA64 / virtio-pci 不属于本修订的 build 或 runtime target；
- `virtio-drivers` 0.13 的 `VirtIONetRaw` 提供 non-blocking RX/TX begin、completion polling、
  interrupt ack 与 queue notification control；
- 仓库已有本地 `smoltcp` fork，其 `poll_ingress_single()`、`poll_egress()` 和 `poll_at()` 可以作为
  bounded pump 的实现基础，但该 crate 尚未接入 kernel network owner；
- kernel 已有 monotonic time、IRQ、kthread 与 threaded timer 基础能力，但没有 `device/net`、
  VirtIO-Net driver、`anemone-net-api`、`anemone-smoltcp-stack` 或 `anemone-kernel::net` wiring。

live source 也暴露了不能在 RFC 中假装已经消失的工程事实：当前 `VirtIOHalImpl::share()` 会为
每次 virtqueue buffer sharing 分配 bounce DMA 并在失败时 panic。System Power R0 已建立由 `power`
唯一拥有的 orderly 静态 plan 与 emergency 绕过；当前 effective plan 仍是 `filesystem -> device`，
尚未列入 network participant。前者已由
[NFP-001](./tracking-issues.md#nfp-001--当前-virtio-hal-sharing-路径尚不满足有界帧资源前提)
记录为本 Draft 接受的 allocation boundary；后者不再是 owner/hook 设计缺口，并由
[NFP-002](./tracking-issues.md#nfp-002--system-power-r0-已提供显式-network-cleanup-route) 记录
neutralization 与剩余接入义务。

相关材料：

- [背景材料索引](./backgrounds/index.md)与 [定位共识](./backgrounds/positioning.md)：只保留本 RFC
  形成前的 frame-path 历史；
- [System Power shutdown lifecycle 当前契约](../../contracts/power/shutdown-lifecycle.md)；
- [当前 IRQ-off allocation 开放问题](../../register/open-issues.md#ane-20260622-irq-off-heap-allocation)。

提升前的网络宽泛定位、能力调查和共享 host-testing rationale 继续作为三份 sibling RFC 的私有
设计输入，不是公共引用目标。本 RFC 已把 frame-path 所需的 owner、object fence、互补证据责任与
test-support 提取边界折入正文、目标不变量和实施计划。

## 设计原则

### 最小机制，而不是最小正确性

本 RFC 只固定后续 sibling RFC 必须依赖、或错误选择会破坏核心对象模型的边界。它不固定 trait
拆分、manager 类型、pool 算法、worker 数量、budget 数值、统计集合或公共 buffer wrapper。

如果两个 concrete provider 只在某个内部机制上偶然相似，不因此提取公共抽象。只有
VirtIO-Net 与 host frame test implementation 都必须共同遵守的 ownership、handoff、outcome、
time 和 recheck 语义才能进入共享 API。

### 适度 IRQ-off allocation 优先于扭曲对象模型

当前工程阶段允许 IRQ-off owner transaction 做适度内存分配。适度表示一次操作的分配量有限、同时
存活数量受 frame/queue credit 约束，并由 completion 确定回收；它不要求所有 frame path 都预分配，
也不把 kernel 全局 allocator OOM 改造成网络 backpressure。当前 kernel 在该类 OOM 下可能 panic，
这是明确接受的系统级失败边界，不得写成网络资源耗尽已经可恢复。

默认不修改或 fork `virtio-drivers`，也不为了消除 allocation 单独引入侵入式 frame、跨层 DMA token、
镜像 credit 状态或通用 packet pool。capacity reserve、owner-local reuse 和 high-water 观测若能保持局部、
简单且不改变 owner，可以作为 best effort 收敛；它们不是本 RFC 的正确性门槛。

### 工程可行性可以收窄 target，但不能伪造 correctness

工程证据可以证明某个 target guarantee 成本过高，并触发 `Target Renegotiation Gate`。review
可以据此选择保持目标、接受一个更窄但自洽的新修订、拆 follow-up RFC 或保持 Not Cut Over。

以下内容不能用“工程优先”降级：DMA/CPU ownership 诚实性、唯一状态 owner、无第二套 durable
truth、已提交 frame 的生命周期安全、hard IRQ context 边界，以及对实际未支持能力的可观察拒绝。
若只能通过违反这些规则落地，当前方案不可接受。

## 目标

- 在现有 bus / `Device` / `Driver` 框架内，通过已有 `SomeTransport` capability 为 RV64
  virtio-mmio 建立 VirtIO-Net driver，不复制 transport owner。
- 建立 `device/net` owner：为一个或多个 boot-time netdev 分配稳定 identity，发布规范化 link
  facts 与 driver-provided frame capability。
- 建立最小 `anemone-net-api` frame slice：opaque `InterfaceId`、link/interface facts、move-only
  frame access、显式 monotonic time、bounded pump outcome 与 recheck semantics。
- 建立 `anemone-smoltcp-stack`：拥有 smoltcp interface resource、`InterfaceId` mapping、device
  token adaptation 与单 owner bounded pump。
- 由 `anemone-kernel::net` 的唯一 attach authority 完成 netdev、concrete stack、worker/wake/time
  wiring，并向 System Power orderly plan 提供一次 owner-local、best-effort shutdown cleanup facade。
- 用真实 stack + 真实 host frame provider 的确定性组合证明 ownership、resource exhaustion、
  completion、link recheck、budget 与多 interface 隔离。
- 用 RV64 QEMU 证明真实 virtio-mmio、IRQ、DMA、双向 RX/TX 和 kernel wiring。

## 非目标

- `EndpointId`、TCP/UDP endpoint、kernel socket object、socket syscall、fd/wait/iomux readiness；
- address、route、neighbor、DHCP、DNS、netlink、网络 namespace 或完整 IP control plane；
- runtime hotplug、unbind、remove、identity generation reuse、stale-handle recovery 或重新 attach；
- deferred probe、通用 probe rollback / retry framework，或为未来 driver 预留的 device-core 扩展；
- 通用 mbuf、skb、`Lease`、fragment chain、clone/COW、packet metadata framework；
- 把 driver frame、smoltcp socket storage 与 userspace I/O 统一成一个 buffer object；
- e1000e、GMAC、loopback、veth、TUN/TAP 或第二种 production NIC driver；
- LA64 / virtio-pci build、runtime、interrupt routing 或 architecture-parity acceptance；
- 多 interface routing、自动出接口选择、同一 stack instance 内跨 interface 并行推进；
- 完整 runtime teardown、资源回收后重启网络，或 shutdown 后重新激活；
- 为 shutdown 单独重塑 frame/driver/worker 对象模型，建立通用 drain/timeout/refcount/teardown
  framework，或保证所有在途工作都完成、所有资源都回收；
- panic/emergency 路径运行 network ordinary cleanup callback。

## 文档地图

RFC target：

- [目标与不变量](./invariants.md)：proposed contract IDs、owner、handoff、lifecycle 与 proof
  obligations；
- [Tracking Issues](./tracking-issues.md)：仍影响 implementation readiness 或 acceptance 的问题；
- [实施计划](./implementation.md)：Stage 1 walking skeleton 的 Ready 定义、后续 Outline、验证、停止条件
  与 resolved write set；Ready 不授予执行权限。

Current contracts：

- [System Power shutdown lifecycle](../../contracts/power/shutdown-lifecycle.md)：当前
  `SYSTEM-POWER-ORDERLY-001` 固定 `filesystem -> device` 静态 plan；本 RFC 只 proposed Refine 为在
  `device` 前显式调用 network owner facade。网络 frame path 尚无 effective contract，本 RFC 的六个
  network stable IDs 在 `NFP-FINAL-CUTOVER` 前都只是 proposed target。

背景材料：

- [背景材料索引](./backgrounds/index.md)；
- [定位共识](./backgrounds/positioning.md)。

公共外部源码证据：None。本 RFC 的当前基线与阶段 preflight 依赖 live repository source、锁定的
workspace dependency 和实际构建输入；本文不把私人 checkout 或移动的上游分支作为 citation authority。

## 目标架构

### 纵向边界

| 层 / owner | 本 RFC 建立的最小职责 | 禁止知识 |
| --- | --- | --- |
| concrete VirtIO-Net driver | feature negotiation、queue/DMA、IRQ、completion、RX refill、TX reclaim、MAC/MTU/link observation | smoltcp object、`InterfaceId`、socket/fd/wait |
| `device/net` | netdev identity/registry、规范化 link facts、frame capability publication | descriptor、DMA address、smoltcp object、transport state |
| `anemone-net-api` | 两侧共同理解的 values、capabilities、outcomes、time 与 recheck 语义 | kernel object、driver backing、smoltcp object、Linux errno/readiness |
| `anemone-smoltcp-stack` | `InterfaceId`、smoltcp resources、netdev-to-interface private mapping、token adaptation、bounded pump | fd/task/waiter、kernel socket state、driver token |
| `anemone-kernel::net` | attach authority、concrete wiring、worker/wake/timer、network-local shutdown cleanup | global shutdown phase、descriptor/DMA truth、smoltcp object identity、future socket state |
| System Power | global terminal episode、orderly participant 顺序、emergency 绕过与 machine handoff | network private object、worker、frame/DMA truth |

`anemone-net-api` 是共同依赖的语义边界，不是 runtime 中介层，也不是第二个 net core。它不拥有
一份和 driver、netdev 或 stack 并列的 mutable state。

### 身份域

以下身份彼此不等价，也不能相互解引用：

- generic `Device` identity；
- netdev identity、ifindex 与 name；
- protocol `InterfaceId`；
- private smoltcp interface object identity；
- VirtIO queue / descriptor token；
- 一次性 frame resource token。

`device/net` 拥有 netdev identity；`anemone-smoltcp-stack` 拥有 `InterfaceId` 分配和到 private
smoltcp object 的 mapping；`anemone-net-api` 只定义 opaque value type。第一版 identity 在本次
boot 内不复用，不为未来 hotplug 预建 generation。

### frame capability

共享边界只表达 frame ownership 与 owner-visible outcome，不冻结 concrete buffer：

```text
RX: available -> device/DMA owned -> CPU-visible -> protocol consume -> recyclable
TX: reservable -> CPU fill -> submitted/device-owned -> completed -> reclaimable
```

frame provider 以 move-only resource token 提供一次性 consume scope：RX 只在 callback 期间暴露
有效 `&[u8]`，TX 只在 callback 期间暴露请求范围的 `&mut [u8]`。token 未消费即销毁时由 owner
取消预留；TX submit 或 RX recycle 后，上层不得继续访问 backing。

VirtIO header、descriptor chain、DMA padding、queue token 与 concrete backing 始终留在 driver
owner 内。共享边界向 stack 提供连续 Ethernet frame 区域，但不要求跨 provider 共享一个 wrapper
类型。普通 frame 不以 `Arc<Mutex<_>>` 或 `Arc<SpinLock<_>>` 作为默认 ownership 模型。

frame resource 有界。RX exhaustion、TX backpressure、queue full 与暂时 link unavailable 都是正常、
可诊断且可重试的 outcome，不得转化为 panic 或 busy-spin。VirtIO HAL 可以在 submit/unshare 路径
分配和回收 bounce DMA；同时存活的 mapping 必须受 frame/queue credit 约束。全局 allocator OOM
不属于上述 normal outcome，在当前内核中可能 kernel-fatal。

### 协议推进与通知

每个 concrete stack instance 是其所有 protocol resources 的唯一逻辑推进 owner。同一 instance
任一时刻最多一个 pump；允许多个 worker 竞争推进权，不要求固定线程或 singleton manager。

IRQ 只确认 driver-owned hardware facts、完成最小 ack/mask/unmask，并发布 recheck edge；不在 IRQ
handler 调用 smoltcp、执行 protocol callback、承担 frame submit/recycle 或发布 Linux-visible
readiness。通知不是 durable truth，worker 醒来后必须重新读取 RX/TX completion、resource/link
facts、explicit work 和 deadline。IRQ 或 IRQ-off 路径中的简单有界 allocation 本身不构成违反，但
不能借此引入 blocking/reclaim protocol、普通锁依赖、复杂 drop/callback 或第二套资源 truth。

pump 使用显式 monotonic time 和有限 budget，分别观察 ingress、egress、work remaining、immediate
repoll 与 next deadline。普通软件调度策略不能让 RX/TX 一侧无限期饿死另一侧；真实 queue 或 backing
耗尽可以暂时停顿，但 completion / resource return 后必须重新获得推进机会。

### publication、attach 与 shutdown

driver probe 先完成 owner-local queue/DMA/IRQ/frame 准备，最后才向 `device/net` 发布 netdev。失败
不得留下已发布 netdev；若完整 rollback 会迫使 generic device core 引入不自然机制，可以保留本次
失败尝试的资源，但绝不能释放或复用设备仍可能访问的 backing。

published/unattached netdev 是合法状态。唯一 kernel-side attach authority 发起 stack mapping、
worker/wake/time 准备，并在全部成功后发布 active path；attach 失败撤销 stack-local partial result，
保留 netdev published/unattached。不同 netdev 的失败互不回滚或阻塞，第一版不要求自动 retry。

link-up 不是 attach 前提。link-down 不注销 identity、frame capability 或 `InterfaceId` mapping；它只
改变 owner facts 并触发 recheck。恢复后沿原 mapping 继续，不重新 attach。provider 无法证明的
link fact 使用 unavailable/unknown 语义，不伪造成 link-up。

orderly system shutdown 时，`power` 在静态 plan 中、`device::shutdown()` 前显式调用 network facade。
attach authority 只拥有 network-local cleanup：关闭或抑制新的 pump、timer/wake 与 frame-token
acquisition，并使用既有 worker/token/provider ownership 做一次有限的 cancel/drain attempt。shutdown
不是本 RFC 的重点；该 attempt 不要求为清理重塑对象模型，不保证等待所有访问、回收所有 backing 或
按时返回，也不引入 timeout/retry/通用 teardown framework。

随后 `device::shutdown()` 仍由 driver 完成 owner-local IRQ、queue/DMA 和 device quiesce。任何 owner
都不得释放或复用仍可能被 CPU/device 访问的 resource；不能低成本证明安全时允许保留到 reset/
power-off。System Power emergency path 跳过全部 ordinary participant，因此 panic 不调用 network
cleanup。shutdown admission closure/attempt 是本次 boot 的终态，不支持重新激活。

## 多设备与平台边界

多个 boot-time netdev 的独立 registration、identity、frame resource、link facts 与 interface
mapping 是正式 target，不只是“代码没有 singleton”。第一版不要求一个 architecture 的 QEMU
同时启动多张 NIC：host conformance/vertical-slice evidence 必须证明至少两个 provider/interface
实例互不混淆；RV64 QEMU 至少证明一张真实 NIC 的完整纵切。

concrete driver 必须消费现有 `SomeTransport` 或等价窄 capability，不能重新拥有或复制 virtio-mmio
transport state。本修订不要求为未验收的 PCI transport 预建抽象，也不以 LA64/virtio-pci coverage 作为
closure 条件。

## Contract Impact

完整规则见 [目标与不变量](./invariants.md#contract-impact)。六个 network stable IDs 当前均为
`Introduce`，current rule 为 `None（尚未生效）`；`SYSTEM-POWER-ORDERLY-001` proposed `Refine` 当前
`filesystem -> device` 静态 plan，在 NFP cutover 时加入显式 network owner facade。本 RFC 不提前创建
`docs/src/contracts/net/`，也不建立 RFC-local `contracts/` 子目录。

最终 `NFP-FINAL-CUTOVER` 是一个原子 contract cutover boundary。只有六项 network 规则与 power plan
Refine 都达到验证 floor，后续 sibling RFC 才能把它们作为 effective baseline；implementation 解析
可以安排多个 build/probe 阶段，但不能让未闭合的中间形状成为长期共享 contract。

## 接受边界

### R0 acceptance

2026-07-26 公共 review 确认 proposed target 足以进入 implementation，用户明确接受当前文本为
`R0 / Accepted for Implementation`、授权建立 transaction 并激活 Stage 1 Checkpoint 1。R0 acceptance
不让 proposed contract IDs 生效。

本次 acceptance 已确认：

- 文档层确认六个 network stable IDs 与 `SYSTEM-POWER-ORDERLY-001` Refine 的 owner、handoff、failure
  与 proof boundary 自洽；
- 剩余 Keter 已 neutralize，或路由到具备 protected boundary、resolution trigger、failure signal
  与停止条件的实施 gate；需要实际处理该问题的阶段在执行前必须达到 Ready；
- `implementation.md` 已建立，并且第一个可执行阶段独立达到 Ready；
- 任何 target reduction 经过明确 review，而不是把较弱实现写成原目标。

### R1 acceptance

2026-07-27 Stage 2 Checkpoint 1 的真实 RV64 128-packet probe 到达 exhaustion assertion，但没有观察到
`queue-full > 0`。累计 burst 大于 TX slot 数只证明总工作量，不保证同一时刻的 in-flight TX 超过 credit：
production provider 会在每次 admission 前回收已经完成的 TX，TCG/VirtIO 可以在相邻 admission 之间归还
credit。用户据此接受 R1，将错误的“QEMU 必须确定性制造一次 exhaustion”验收前提替换为互补 proof：

- host real-stack + deterministic provider 必须确定性制造 credit exhaustion、保持未发送工作、归还 matching
  completion/recheck 后恢复，并覆盖 budget、deadline、fairness 与 link matrix；
- RV64 QEMU 必须证明真实 virtio-mmio 路径上的 bounded outstanding、TX/RX completion、IRQ recheck、mapping
  回落、有限 worker action 与正常关机；若运行中自然观察到 exhaustion，还必须证明其后继续提交并完成；
- RV64 未自然观察到 exhaustion 不是失败，也不能替代 host 的确定性 exhaustion proof。validation 不得延迟/
  吞掉 completion、降低 production capacity、伪造 queue state 或让 diagnostics 驱动行为来制造 PASS。

R1 不改变 frame/progress 语义、owner、public API、contract delta、platform scope 或最终 cutover；它只修正
acceptance proof assignment。旧 Checkpoint 1 的失败、probe 删除与 Not Achieved 结论继续保留在 transaction。

### 最终 closure floor

`NFP-FINAL-CUTOVER` 至少要求以下互补证据：

- host：真实 `anemone-smoltcp-stack` 与真实 frame test provider 组合，覆盖 RX/TX ownership、
  unconsumed token cleanup、completion、exhaustion/recheck、budget、deadline、link change 与双实例隔离；
- RV64 QEMU：virtio-mmio 的 IRQ、DMA、RX/TX completion、active attach，以及 orderly plan 在 driver
  shutdown 前调用 network best-effort cleanup；
- source audit：object fence、single pump owner、IRQ ack/recheck-only、bounded mapping lifetime、
  publication ordering、attach rollback、network-before-driver 顺序与不安全资源不回收；
- acceptance audit：没有 endpoint/socket/control-plane 旁路，没有 fake/fake self-proof，没有把未验证的
  双向收发、IRQ 或 DMA 行为记为 PASS，也没有把 LA64/virtio-pci 写成本修订 coverage。

Host 成功不替代 RV64 QEMU，单次 packet smoke 不替代 production completion/reclaim proof；RV64 burst
成功也不替代 host 的确定性 exhaustion proof。未运行项目必须保留 Not Run。

## 备选方案与取舍

### driver 直接实现 `smoltcp::phy::Device`

拒绝。它把 smoltcp object 与 paired token semantics 泄漏进 hardware owner，也让 host provider、
其它协议 backend 和 kernel attach 依赖 concrete stack API。

### 先建立通用 mbuf / lease / packet framework

拒绝。当前只需要 Ethernet frame 的独占 handoff；clone/COW、fragment chain、metadata 与跨 subsystem
buffer sharing 没有真实 consumer。若后续 transport 或第二个 production driver 证明需要，再由最先
需要它的 RFC 提取。

### 默认改造或 fork `virtio-drivers`

拒绝作为第一路线。当前 `Hal::share()` 的不可失败接口使 allocation failure 无法自然上抛，但修改
dependency 会扩大维护、unsafe 与 dependency 分叉验证成本。第一版接受有界 live mapping 下的适度
allocation 与 kernel-fatal OOM；只有真实 workload、泄漏、延迟或 DMA correctness 证据证明现有路线
不可用时，才重新评估窄 dependency adjustment。

### 单一全局 netdev / interface

拒绝。它会把 first-device coincidence 固化为 core object model，并使第二张 NIC 需要重写 identity、
mapping、worker 与 shutdown owner。多设备隔离属于本 RFC 的最小模型正确性。

### IRQ 或无限 polling 直接推进 smoltcp

拒绝。IRQ context 不能承担 protocol work；无 budget polling 会占据 CPU 并掩盖 lost wake/resource
recovery 问题。

### 先重做 generic device lifecycle

拒绝。第一版 boot-only lifecycle 允许 unbound/inert device、published/unattached netdev 与资源保留到
power-off。只有 live frame path 无法在现有 framework 中安全表达时，才以具体证据申请最小扩展。

## 风险

- 当前 VirtIO HAL bounce sharing 会保留 per-submit allocation 与 kernel-fatal OOM 风险；
  [NFP-001](./tracking-issues.md#nfp-001--当前-virtio-hal-sharing-路径尚不满足有界帧资源前提)
  已按适度 IRQ-off allocation 偏向 neutralize，但后续 Ready gate 仍需证明 live mapping 上界、identity
  与 completion reclaim。
- System Power R0 已 neutralize 原 shutdown owner/hook 缺口；
  [NFP-002](./tracking-issues.md#nfp-002--system-power-r0-已提供显式-network-cleanup-route) 将剩余工作
  收窄为静态 plan 接入、owner-local best-effort cleanup 与不安全资源保留，不要求改变对象模型。
- RV64 virtio-mmio interrupt routing、link fact 能力与真实 RX/TX 仍需取得 runtime evidence；这是后续
  validation gap，不在本轮提前上升为 design finding。若证据要求改变 shared ownership 或
  acceptance boundary，再返回 RFC review。
- bounded pump 的精确 budget 若过早固定，可能不适配 smoltcp 与 queue 行为；正文只固定有限工作与
  公平性结果，数值和算法留给真实 probe。

## 修订记录

| 修订 | 日期 | 状态 | 摘要 | 事务 |
| --- | --- | --- | --- | --- |
| R1 | 2026-07-27 | Accepted for Implementation | 保持 R0 target/owner/contract delta，将 exhaustion 确定性证明归给 host provider，并把 RV64 验收修正为真实 bounded completion/IRQ/reclaim；Stage 2 Checkpoint 1 重新达到 Ready | [transaction](../../devlog/transactions/2026-07-26-net-frame-path.md) |
| R0 | 2026-07-26 | Accepted for Implementation | 接受 RV64-only frame path target、六个 proposed network IDs 与 System Power Refine；Stage 1 Checkpoint 1 激活 | [transaction](../../devlog/transactions/2026-07-26-net-frame-path.md) |

## 收口

当前尚未收口。Stage 1 已完成的 host/build/RV64 evidence、R0 Checkpoint 1 的失败证据与 R1 docs-only
renegotiation 均由 transaction 记录；R1 尚未运行任何新 host、build、QEMU 或 runtime validation。
current contract 只在 `NFP-FINAL-CUTOVER` 更新。
