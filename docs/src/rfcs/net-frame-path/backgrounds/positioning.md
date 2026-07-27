# 网络帧路径定位共识

**状态：** Superseded / RFC Background
**最后更新：** 2026-07-26
**范围：** `net-frame-path` RFC 形成前的定位共识与来源材料

> 本文已由 [RFC-20260726-net-frame-path](../index.md) 取代，不再承担活动 target 或开放问题权威。
> 它仍不是 current contract 或 implementation plan，也不表示任何阶段已经获得实现授权。
> 本文形成时对 global shutdown owner/hook 的描述已由 System Power R0 supersede；当前顺序与失败边界
> 以 [System Power current contract](../../../contracts/power/shutdown-lifecycle.md) 和本 RFC
> Draft 为准，本文相关段落只保留历史定位价值。

## 文档目的

`net-frame-path` 是网络领域三份 sibling RFC 中的第一份。它以网络设备与帧路径为
主轴，为后续 `net-udp` 和 `net-tcp` 铺设最小的跨层地基，但不提前拥有 endpoint、
socket ABI 或 transport semantics。

本文先稳定以下内容：

- 本 RFC 要交付的能力边界；
- `driver/net`、`device/net` 与四层网络架构之间的 owner 和 object fence；
- frame identity、ownership、resource exhaustion、通知与推进的大方向；
- host testing 与 QEMU 证据各自承担什么责任；
- 在进入 RFC-shaped draft 前必须继续裁决的 target、owner、lifecycle 与 acceptance
  boundary 问题。

本文形成时还参考了三份 sibling RFC 共用的宽泛网络定位、能力调查和 host-testing rationale。
这些共享材料继续作为私有设计输入，不是公共 citation authority；本页只保留已经折入
`net-frame-path` Draft 的定位历史。

## 当前范围

### 目标能力

`net-frame-path` 应建立一条可验证的最小链路层路径：

1. 现有 bus / `Device` / `Driver` 框架通过同一个 concrete VirtIO-Net driver 发现并 probe
   一个或多个 boot-time NIC；
2. `driver/net` 把 NIC、DMA、queue、IRQ 和 completion 收敛为帧级能力；
3. `device/net` 注册稳定 netdev identity，并发布 link facts 与 driver-provided frame I/O
   capability；
4. `anemone-smoltcp-stack` 通过最小共享边界消费 frame capability，把它适配为 smoltcp
   device token，并以 bounded pump 推进真实 smoltcp interface；
5. `anemone-kernel::net` 完成 netdev、concrete stack、worker/wake 与 monotonic time 的最小
   wiring，但不建立 kernel socket object；
6. host test 与真实 VirtIO/QEMU 分别证明确定性语义和硬件集成。

设备被发现或 driver probe 成功不是充分验收。单向 TX 也不是充分验收。最终必须能证明
真实 RX/TX、frame ownership、资源耗尽后的可恢复进展、bounded pump 和跨层 wiring。

### 架构与多设备验收边界

首个 concrete NIC driver 是 transport-agnostic VirtIO-Net driver。最终 runtime acceptance
同时覆盖 RV64 QEMU 的 virtio-mmio 与 LA64 QEMU 的 virtio-pci；两条路径可以分阶段落地，
但不能只凭其中一个 architecture 的 runtime 成功关闭本 RFC。每个 architecture 的 QEMU
runtime floor 可以只使用一张 NIC，具体链路层观察路径留到 validation gate。

多个 boot-time netdev 的注册、独立 identity、分别 attach 到 protocol interface，以及
frame-path 资源和状态互不混淆，是本 RFC 的正式能力，不只是避免全局 singleton 的代码
形状。第一版不因此引入跨 interface route、自动出接口选择、用户配置面或其它 IP control
plane；是否在 host、QEMU 或两者组合中证明多设备能力，由后续 validation 解析决定。

### 明确不进入本 RFC

- deferred probe、probe 失败后的通用回滚协议、driver unbind 和 runtime hotplug；
- 为上述能力预留的通用设备框架扩展；
- `EndpointId`、TCP/UDP endpoint、kernel socket object、socket syscall 和 iomux readiness；
- address、route、neighbor、DHCP、netlink 或完整 IP control plane；
- 把 frame、protocol socket storage 和 userspace I/O 统一成一个 buffer object；
- 通用 mbuf、`Lease`、fragment chain、clone/COW 或 packet metadata framework；
- e1000e、板载 GMAC 等第二种 concrete NIC driver；
- runtime removal 所要求的 generation reuse、stale-handle recovery 和 teardown protocol。

第一版只讨论 boot-time discovered、注册后在本次 boot 生命周期内保持存在的 netdev。
这允许身份不复用，也避免未来 hotplug 需求反向塑造当前工程形状。以后若引入 hotplug，
应由独立 RFC 明确 Refine 或 Replace 当前生效的 identity/lifecycle contract。

## 当前基本共识

### 复用现有设备框架

现有 `Device` / `Driver` / platform / PCIe / VirtIO bus 是本 RFC 的既有基础，不是待重写
对象。`net-frame-path` 只新增网络 subsystem 和 concrete net driver 所必需的 owner-local
结构。

除非当前目标中的正常 boot-time attach、IRQ、frame I/O 或 shutdown 无法表达，否则不
扩大通用设备框架。deferred probe、失败回滚和 hotplug 的缺失不构成本 RFC 的扩展理由。

### driver 与 device 保持帧级

`driver/net` 拥有具体硬件机制：

- NIC feature negotiation、queue/ring、DMA backing 和硬件 token；
- IRQ ack/mask/unmask 与 RX/TX completion；
- concrete RX refill、TX submission、资源回收和硬件错误观测；
- 从硬件取得的 MAC、MTU、link observation 和统计事实。

`device/net` 拥有网络设备 subsystem 语义：

- netdev identity、ifindex/name 分配与 registry；
- MAC、MTU、link availability 等规范化 link facts 的对上层发布；
- driver-provided frame I/O capability 的发布和查找；
- 多个 boot-time netdev 的独立注册、identity 与 frame capability 隔离。

两者都不理解 TCP、UDP、port、endpoint、socket、fd、poll、waiter 或 Linux errno。
`smoltcp::phy::Device` 不是 Anemone 的公共 driver trait；它只能在
`anemone-smoltcp-stack` 的 object fence 内出现。

### 四层架构只建立 frame slice

`net-frame-path` 不试图一次完成四层。它只在每层建立后续 frame capability 必需的最小
切面：

| 层 | 本 RFC 需要建立的最小内容 | 本 RFC 不引入 |
| --- | --- | --- |
| `smoltcp` | 可 host build 的 fork、Ethernet/interface/device token 与 bounded poll 所需能力 | Anemone kernel object |
| `anemone-net-api` | `InterfaceId`、link/interface facts、frame handoff、显式 monotonic time、pump/recheck 的纯语义边界 | endpoint、socket、Linux errno/readiness |
| `anemone-smoltcp-stack` | interface resource owner、netdev-to-interface mapping、smoltcp device adaptation、bounded pump | fd/task/wait、kernel socket state |
| `anemone-kernel::net` | concrete stack wiring、net worker/wake/timer 接入、boot-time netdev attach | socket ABI、transport state |

本地维护的第三方 fork 归入 `crates/anemos`，crate 保留 upstream 名称，因此这里使用
`smoltcp`；`anemone-net-api` 与 `anemone-smoltcp-stack` 仍是带 Anemone 前缀的项目 crate。

`anemone-net-api` 是共同依赖的语义边界，不是 runtime 中介层，也不是第二个 net core。
它只能承载两侧必须共同理解的 values、capabilities 和 outcomes，不能暴露 concrete driver
buffer、smoltcp object 或 kernel object。

### 身份域保持分离

以下身份不能互相替代：

- 通用 `Device` identity；
- netdev identity / ifindex；
- protocol `InterfaceId`；
- smoltcp interface object identity；
- hardware queue / descriptor token。

第一版 netdev 在 boot 生命周期内不移除，因此 netdev identity 与 `InterfaceId` 不需要为
hotplug 预建 reuse/generation 机制。`device/net` 拥有 netdev identity；
`anemone-smoltcp-stack` 拥有 `InterfaceId` 的分配、映射和 interface resource。
`InterfaceId` 的值类型放在 `anemone-net-api`，但它不等于 ifindex，也不能被调用者解引用
成 smoltcp object。

### 注册、attach 与 shutdown lifecycle

第一版允许四类可持续的 owner facts：generic bus 已发现但尚未绑定的 `Device`；driver 已
完成 frame capability 并发布、但尚未 attach 的 netdev；已经完成 protocol mapping 与
kernel wiring、允许 pump 的 active path；以及 system shutdown 后不再接受工作的 quiesced
path。这里的分类只描述跨 owner 的合法可观察关系，不要求实现 `NetdevLifecycle` enum、
集中式状态机或并列缓存字段。实现应优先从 device binding、netdev registry publication、
active `InterfaceId` mapping 和各 owner 的 quiesce publication 直接判断；不得为了方便查询
复制一份可能 stale 的综合生命周期真相。

driver probe 必须先完成 owner-local hardware/frame 准备，最后才发布 netdev。probe 失败时
不能留下已发布 netdev，generic device 可以保持 unbound/inert；对本次尝试创建的 owner-local
资源只要求 best-effort cleanup。若完整回收会迫使通用 device/bus framework 引入不自然的
rollback 形状或扩大修改面，第一版接受少量、仅属于本次失败尝试的资源泄露，不要求 generic
rollback、deferred probe 或自动重试。这一让步只针对资源回收：不得释放或复用仍可能被失败
设备访问的 backing，无法证明访问已经停止时应保留资源。

已发布但尚未 attach 的 netdev 是合法状态。唯一的 kernel-side attach authority 负责发起
protocol mapping、worker/wake/time 准备，并只在这些准备全部成功后发布 active path；
`anemone-smoltcp-stack` 仍然独占 `InterfaceId` 分配与私有 mapping。准备中的
`stack attached` 只是 transaction 内部事实，不形成第二个长期公开状态。attach 失败必须
撤销 stack-local 部分结果并保留原 netdev 的 published/unattached 状态；不同 netdev 独立
失败，不能互相回滚或阻塞。第一版不要求自动重试；若后续允许 retry，也只能由同一 attach
authority 发起。目标平台的预期 NIC 未进入 active path 时，runtime acceptance 失败。

唯一的 kernel-side attach authority 同时拥有 active path 的 quiesce barrier；concrete driver
不得为了协调关闭而反向访问 protocol stack、kernel worker 或 attach authority。system power
path 在调用通用 `device::shutdown()` 前，必须先经过该 barrier 发布不再接受新的 pump、
timer/wake 和 frame-token acquisition，并按顺序 best effort 地让已经进入 protocol/frame 访问的
执行退出。随后 concrete driver 的 shutdown 只负责 owner-local 的 IRQ mask、queue/DMA 停止、
设备 reset/quiesce 与安全资源回收，不能承担跨层 active-path 协调。

quiesced 是本次 boot 的终态，shutdown 不要求完整 teardown、runtime unregistration、identity
reuse 或重新激活。若安全回收要求为通用框架引入不自然的关闭形状，可以把仍可能被执行或设备
访问的资源保留到 reset/power-off，不能冒险提前释放。具体 publication primitive、精确线性化点、
retry API、power-path hook 形状、等待原语与超时策略留到 RFC stage 解析。

### link facts 与 link-down 语义

link-up 不是 attach 前提。netdev 即使当前不能证明 link 可用，也可以完成 `InterfaceId`
mapping 并进入 active path；link availability 不反向拥有 netdev lifecycle。

concrete driver/frame owner 提供其实际能够观察的硬件 link facts 与更新机制，`device/net`
负责把这些事实规范化并对上层发布。发布责任不等于复制一份并列真相：上层收到的通知只
表示重新读取，不能成为第二份 link state；若为了稳定 snapshot 必须缓存，后续设计必须
说明来源、允许的 stale 窗口和失效点。硬件或 transport 不能证明的 link fact 不得为了方便
伪造成 link-up；完整字段集合、是否需要 unknown 表达以及具体 Rust 编码留到 RFC stage。

link-down 不撤销 netdev identity、ifindex、`InterfaceId` mapping 或 frame capability，也不
等于 runtime removal、detach 或 shutdown。frame capability 可以因当前 link/resource facts
暂时无法推进新的收发，但已经存在的 completion、in-flight resource 和回收路径仍必须继续
处理。link 恢复后只触发 recheck，原 stack instance 沿原 mapping 继续推进，不重新分配
identity 或重新执行 attach transaction。精确 unavailable outcome、变化线性化点与并发
corner cases 留到后续解析。

### 只统一 frame ownership / handoff 语义

当前只统一帧的独占所有权转换和跨层可观察结果，不预选一个跨 driver 的 concrete buffer
类型。

RX 至少需要区分：

```text
可供投递 -> device/DMA owned -> CPU-visible frame -> protocol consume -> 可重新投递
```

TX 至少需要区分：

```text
可填充 -> submitted -> device/DMA owned -> completed -> 可回收复用
```

提交给设备后的 bytes 不能继续被 CPU 访问；TX completion 前不得复用 backing；RX 只有在
protocol consumer 释放后才能重新投递。普通 frame 通过 ownership move 流转，不以
`Arc<Mutex<_>>` 或 `Arc<SpinLock<_>>` 作为默认模型。

frame 资源必须有界。RX exhaustion、TX backpressure 和 queue full 都是正常状态，不是
panic 或 busy-spin 条件。资源恢复和 TX completion 只产生“重新检查”的通知；可发送、可
接收与资源数量的 durable truth 仍由 netdev/driver owner 保存。

普通软件负载不能因为 frame 资源分配或 pump 调度策略，使 RX/TX 中一个方向无限期饿死
另一个方向。真实 device queue saturation 或 backing/submit credit 耗尽可以造成暂时停顿，
但 completion 或资源归还发生后必须能够重新检查并恢复推进。这是跨方向的进展性边界，
不是“TX 耗尽时仍保证消费至少一个 RX frame”之类的资源数量承诺。

第一版向 smoltcp 暴露连续的有效 Ethernet frame 区域。VirtIO header、descriptor chain、
DMA padding 和 queue token 保持在 concrete driver 内部。是否需要公共 frame wrapper、
associated token、driver-private buffer 或其它接口，仍需由 host frame test implementation
与 VirtIO-Net 的共同需求决定。

frame capability 的访问语义采用 **move-only 资源 token + owner-controlled consume
scope**。token 不是 netdev identity、durable state 或 hardware queue token，而是调用者对
一个已预留 frame 资源的一次性独占访问能力：

- RX token 只在 `consume` 回调期间暴露有效 frame 的 `&[u8]`；回调返回后由 owner recycle
  backing，使其重新进入可供设备投递的状态；
- TX token 只在 `consume(len, ...)` 回调期间暴露指定有效区域的 `&mut [u8]`；回调返回后
  由 owner 提交，直到 completion 前上层都不能再次访问 backing；
- token 不可复制；未被 consume 的 token 在 `Drop` 时取消预留并把资源归还原 owner，不能
  依赖调用者额外执行 cleanup；
- acquire、consume finalization 和 completion 可以短暂进入 owner-local 临界区，但不得在
  protocol callback 期间持有 device-wide lock。smoltcp 可能在 RX consume 内立即消费配对
  TX token，因而 RX/TX 预留必须允许该嵌套路径而不重入同一把全局锁。

这一语义同时约束 host frame test implementation 与 VirtIO-Net，防止 DMA/CPU 并发访问、
double recycle、double submit 和 use-after-submit，又不要求共享边界公开 concrete
backing。token 的具体 Rust 表示仍未冻结：它可以是 stack-local adapter token，也可以包含
窄类型擦除或 owner-private slot handle；选择时必须保持上述一次性访问、object fence、取消和
锁边界，不能把 descriptor、DMA 地址或 driver-private buffer identity 暴露给 stack。

### host frame test implementation 的产品边界

本文中的 host frame test implementation 专指 host harness 内、test-only 的 concrete frame
capability provider。它实现与 VirtIO-Net 相同的正式共享语义，用确定性输入控制 RX/TX、
completion、资源耗尽、link change 和 recheck，并可以实例化多个彼此隔离的 frame providers
来验证多 netdev / interface mapping；它不是另一套测试专用 API，也不能与 fake protocol
stack 两侧互相自证。

这个 test implementation 不进入内核 `device/net` registry，不参与 boot attach，也不成为
第一版生产 netdev。注包、强制 completion、手动推进时间等 harness 控制能力保持在 test-only
边界，不能为了测试方便进入 production frame capability。本文不再把它称为 software
netdev，以免与 kernel loopback、veth、TUN/TAP 或其它生产软件网卡混淆；这些设备若以后有
真实需求，由最先需要它们的 transport RFC 或独立 follow-up 重新定义 target、lifecycle、
link facts 与控制面。

### IRQ、通知与协议推进分离

一个 concrete protocol-stack instance 是其 protocol resources 的唯一逻辑推进 owner。
该 instance 拥有的多个 interface、`InterfaceId` mapping、protocol deadline，以及后续
transport RFC 引入的 `SocketSet` / endpoint resources，都必须通过同一个独占访问边界串行
推进；同一 instance 任一时刻最多有一个 pump 正在执行。多 netdev 与多 interface 仍是正式
能力，但第一版不承诺同一 stack instance 内跨 interface 并行执行 smoltcp。

这里的唯一 owner 是访问协议，不要求 `ProtocolOwner` enum、owner-id 缓存、固定线程亲和性、
singleton manager 或单一物理 worker。实现可以使用一个或多个 worker；即使多个 worker 能够
竞争同一 instance，也只能有一个取得推进能力。未来若性能证据要求分片，可以建立多个彼此
独立的 stack instances，每个 instance 仍各自保持唯一逻辑推进 owner；这类 target 变化不由
本 RFC 提前设计。

IRQ 不直接调用 socket 逻辑，也不发布协议或 Linux-visible readiness。它只处理
driver-owned 硬件事实并触发 net worker 重新检查。

worker 消费的是 durable predicate：RX completion、TX completion、link change、timer
deadline 或显式 work request。通知只是 wake edge；worker 被唤醒后必须重新读取 owner
状态。

协议推进必须有 budget，不能把可能无界处理 device queue 的 `Interface::poll()` 暴露为
普通调用。一次 pump 至少同时考虑 ingress、egress、work remaining、immediate repoll 和
next monotonic deadline。具体 worker 形状、budget 单位和 `PumpReport` 字段尚未确定。

## 候选技术路径

下面是当前用于继续讨论的工程路径，不是 implementation stages 或冻结 write set。

### 先收敛共享语义，再决定 concrete 类型

先从两个 concrete provider 的共同需求反推最小 frame capability：

- 可确定性驱动的 host frame test implementation；
- 基于 `virtio-drivers` non-blocking raw queue API 的 VirtIO-Net driver。

两者共同回答 RX acquire/consume/recycle、TX acquire/submit/complete/reclaim、queue
exhaustion、有效 bytes、capacity 和 recheck；只有共同需要的语义才进入共享边界。
VirtIO queue token、host fixture queue 和具体 backing 都留在各自 owner 内。

RX/TX reusable backing 是否分别供给、submit credit 如何预留，以及 smoltcp paired
RX/TX token 如何映射，属于后续 RFC 设计与必要 probe 要回答的具体问题。它们必须证明
已接受的有界资源和跨方向进展性边界，但 positioning 不冻结 pool 类型、资源数量、
response reserve 或 smoltcp adapter 的具体机制。

### 让 frame capability 可被 host 组合

frame handoff 的共享 surface 倾向放在 host-buildable 的窄边界中，使
`anemone-smoltcp-stack` 不依赖 kernel module，也使 host harness 能提供遵循正式 frame
capability 的 test-only concrete implementation。

但这个 surface 不能为了 trait object 方便而泄漏 concrete buffer，也不能复制一套
smoltcp `Device` API。已接受的公共访问语义是 move-only 资源 token 加 owner-controlled
consume scope；后续只比较实现该语义所需的最小 Rust 表示，而不重新开放“直接返回公共
buffer”或“跨 callback 暴露 raw slice”的方向。选择标准是：独占所有权能否由类型形状
表达、是否支持 driver-private backing、多 netdev wiring 是否自然、是否可 host test，
以及是否会把 hardware token 泄漏给 stack。

### 由 kernel-side attach authority 完成 wiring

当前只固定 owner 方向：具体 driver probe 发布 netdev 与可唤醒的 frame capability，不创建
protocol endpoint，也不直接推进 smoltcp；等 kernel time、worker 与 wake 基础能力可用后，
由 `anemone-kernel::net` 一侧的 attach authority 发起 netdev 到 concrete stack interface 的
wiring，`anemone-smoltcp-stack` 继续拥有 `InterfaceId` 分配和到 smoltcp object 的私有映射。

attach authority 不拥有 driver queue truth，也不把 `InterfaceId` 到 smoltcp object 的映射
复制进 kernel。它最终是否表现为 singleton coordinator、是否使用统一 worker，以及 worker
与 interface 的数量关系都尚未冻结；这些形状必须服从后续接受的 lifecycle 与 protocol
progress owner 边界。

### 用互补证据证明同一条路径

Host validation 负责确定性地控制 frame、completion、exhaustion、budget 和 monotonic
time，至少组合真实 `anemone-smoltcp-stack` 与一个真实 host frame test implementation。
不能让共享接口两侧都由 fake 自证。

QEMU validation 负责证明真实 VirtIO transport、IRQ、DMA、RX/TX completion 和 kernel
worker wiring。Host 成功不替代 QEMU；QEMU 的单次收发也不替代 host 对 ownership、
exhaustion 和 bounded progress 的确定性证明。

## 定位层共识闭合

只有不同答案会改变 target、owner、lifecycle、跨层 contract 或 acceptance boundary 的
问题，才阻塞进入 RFC-shaped draft。当前目标/验收面、lifecycle（含 probe/shutdown
best-effort cleanup）、protocol progress owner、link-down 语义与 host frame test
implementation 的产品边界均已闭合，定位层不再保留尚待裁决的高层问题。

## 已后移的 RFC / stage 解析问题

以下问题仍然需要回答，但不属于 positioning 层必须现在定死的高层架构：

- netdev identity、ifindex 与 `InterfaceId` attach/mapping 的精确线性化点和具体编码；
- IRQ 到 worker 的具体 durable predicate 集合、wake aggregation 数据结构、worker 数量和
  interface 到 worker 的映射；
- bounded pump 的 budget 单位、数值、公平调度算法和 `PumpReport` 精确字段；
- link facts 的完整字段集合、统计项和内部缓存形状；
- probe/shutdown best-effort cleanup 的具体停止、诊断和资源保留机制；
- RX/TX backing、submit credit、pool、response reserve 与 smoltcp paired token 的具体适配；
- host validation 采用单 stack packet fixture、双 stack memory link 或其它组合；
- QEMU 链路层验收采用 raw Ethernet、ARP 或其它不引入正式 IP control plane 的可观察路径；
- 多 netdev 能力的具体 host/QEMU 证明组合，以及其它 case inventory、failure injection、
  运行命令和证据文件布局。

这些问题应按影响进入 RFC target/proof obligations、第一 Ready stage、后续 rolling stage
resolution 或必要 probe。后移不能放宽已经接受的 owner、ownership、bounded progress、
object fence 和 acceptance boundary；若真实证据表明这些边界需要改变，必须返回定位/RFC
review，而不是在 implementation 中静默选择较弱语义。

本文的定位结论已经折入 [RFC index](../index.md) 与[目标与不变量](../invariants.md)。后续 target、
owner、proof obligation 与 tracking issue 以 RFC 正文为准；本文只保留形成 Draft 前的讨论来源。
公共 RFC 已完成提升；`R0` acceptance、transaction 创建、代码实现和 contract cutover 仍需分别授权，
不得从本页历史材料自动推进。
