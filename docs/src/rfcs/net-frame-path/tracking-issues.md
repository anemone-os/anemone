# Network Frame Path Tracking Issues

**状态：** Active
**最后更新：** 2026-07-26
**父 RFC：** [RFC-20260726-net-frame-path](./index.md)
**事务日志：** [2026-07-26 net-frame-path](../../devlog/transactions/2026-07-26-net-frame-path.md)

本文只跟踪会影响 implementation readiness、owner boundary、停止条件或最终验收的 design / feasibility
问题。普通类型选择、stage TODO、case inventory 和未运行证据不进入本页。

## Apollyon

当前无项。

## Keter

当前无项。

## Euclid

当前无项。RV64 virtio-mmio 尚未取得 network runtime evidence 是后续 validation gap，不是已经确认的
设计缺陷；它由 RFC acceptance floor 和后续 implementation gate 负责。LA64 / virtio-pci 不属于当前
R0 target。如果实际证据要求改变 shared ownership、public semantic surface 或 acceptance boundary，
再新增对应 finding。

## Safe

当前无项。pool/backing 类型、worker 数量、budget 数值、test fixture 形状和精确 case inventory 是
后续 stage 解析输入，不作为 Safe issue 堆放。

## Neutralized

### NFP-002 — System Power R0 已提供显式 network cleanup route

**状态：** Neutralized by System Power R0 and accepted R0 best-effort cleanup boundary
**来源：** 2026-07-26 live source audit；2026-07-26 System Power R0 cutover；2026-07-26 NFP scope review
**影响：** `NET-ATTACH-001`、`SYSTEM-POWER-ORDERLY-001` Refine、shutdown acceptance
**依据：** [System Power current contract](../../contracts/power/shutdown-lifecycle.md#system-power-orderly-001)
与 [`NET-ATTACH-001`](./invariants.md#net-attach-001--attach-publicationrollback-与-best-effort-shutdown-cleanup)

原问题是 live power path 只有 filesystem/device 直接调用链，无法在 driver shutdown 前表达由 network
attach owner 先关闭 pump/timer/wake/frame acquisition 的跨层顺序；把协调塞进 VirtIO-Net driver 又会
反转依赖方向。

System Power R0 已由 `power` 唯一拥有 terminal episode，并把 orderly participant 固定为源码显式、
编译期静态 plan；emergency 则跳过全部 ordinary callback。当前 effective plan 仍是
`filesystem -> device`，因此 NFP 将 `SYSTEM-POWER-ORDERLY-001` Refine 为在 `device` 前调用唯一 network
owner facade。global order 属于 `power`，network-local admission/cancel/drain attempt 属于 attach
authority，driver 继续只拥有 IRQ/queue/DMA/device-local shutdown。

shutdown 不是本 RFC 的重点。第一版只要求复用既有 owner/token/worker 关系做一次有限、可观测的
best-effort cleanup：关闭或抑制新工作，尝试取消/排空已进入访问；不为提高 cleanup 完整性重塑对象
模型，不增加通用 lifecycle cache、drain/refcount/timeout/teardown framework，也不保证 callback 有界
返回、全部访问退出或全部资源回收。唯一不可放宽的安全边界是：无法证明 resource 已不再被 CPU/device
访问时，不得释放或复用，允许保留到 reset/power-off。

剩余工作属于后续 Ready stage 的普通实现与 proof obligation，而不是开放设计缺陷：

- 向 `power` 静态 plan 增加窄 network facade，并保持 network-before-device 字面顺序；
- 解析 owner-local admission closure、有限 cancel/drain 与诊断，不增加第二份 lifecycle truth；
- 证明 driver 不反向访问 stack/worker，且不完整 cleanup 下不会回收仍可能被访问的 resource；
- 用 source audit 与 RV64 runtime 证明 callback 次序和 best-effort 行为，不把正常关机写成完整 teardown。

若实现只能通过改变 frame/driver/worker 对象模型、generic device lifecycle 或 System Power 的静态
plan/episode/emergency contract 才能工作，必须重新打开 finding 并进入 owner/target review；不能在
implementation 中静默扩张。

### NFP-001 — 当前 VirtIO HAL sharing 路径尚不满足有界帧资源前提

**状态：** Neutralized by accepted R0 allocation boundary
**来源：** 2026-07-26 live source audit；2026-07-26 engineering tradeoff review
**依据：** [RFC 的适度 IRQ-off allocation 原则](./index.md#适度-irq-off-allocation-优先于扭曲对象模型)
与 [`NET-FRAME-PROGRESS-001`](./invariants.md#net-frame-progress-001--有界资源normal-backpressure-与-recheck)

`virtio-drivers::device::net::VirtIONetRaw` 提供所需的 non-blocking begin/poll/complete API，但
`VirtQueue::add()` 仍通过不可失败的 `Hal::share()` 映射每个 buffer。当前 `VirtIOHalImpl::share()`
每次调用 `dma_alloc(buffer.len()).expect(...)` 建立 bounce `DmaRegion`，再插入全局 `HashMap`；
`unshare()` 在 completion 时回收并 copy back。若不修改 dependency，这条路径不能把 allocator OOM
自然转换成可重试 outcome。

本 R0 接受适度 IRQ-off allocation 与该路径的 kernel-fatal OOM 边界。frame/queue exhaustion 仍是
normal backpressure；全局 allocator OOM 不属于该语义。默认不修改或 fork `virtio-drivers`，也不为
消除 allocation 引入侵入式 frame、跨层 DMA token、镜像 credit 状态或通用 packet pool。低成本的
capacity reserve、owner-local reuse 与 high-water 观测可以 best effort 收敛，但不是 acceptance
blocker。当前 workload 触发 OOM 的可能性较低只用于风险接受，不构成不会 OOM 的证明。

公共 register 的 IRQ/off-tail complex allocator side-effect 问题仍保持 Open，并继续由 scheduler/
task-lifecycle/allocator owner 收敛；本条 neutralization 不关闭该跨领域问题。除非 network vertical
slice 复现其具体 blocking、重入或泄漏后果，frame RFC 不以改造通用 allocator 或 wake path 作为
implementation readiness 前提。

后续首个 frame-provider Ready gate 仍必须证明 raw buffer identity 与 begin/complete lifetime、同时存活
mapping 受 frame/queue credit 约束、completion/cancel 确定回收，以及 descriptor/DMA backing 不泄漏到
shared API。这些是既有对象模型内的实现 proof obligations，不再要求先建立 allocation-free route 或
dependency adjustment。

### NFP-003 — 通用 packet/lease framework 被误当作 frame path 前提

**状态：** Neutralized by accepted R0 target
**依据：** [RFC 的 frame capability](./index.md#frame-capability) 与
[`NET-FRAME-OWN-001`](./invariants.md#net-frame-own-001--frame-backing-的独占-ownership-与-handoff)

早期讨论可能把“统一 ownership/handoff”扩大成跨 driver、protocol socket storage 和 userspace I/O
的统一 buffer/lease object。当前 R0 已将它收窄为 move-only resource token + owner-controlled
consume scope，并明确 concrete backing、pool、wrapper、clone/COW 与 fragment metadata 均不预建。

只有后续出现第二个真实 production provider 或 transport consumer，且证明共同机制会改变核心
ownership model 时，才可由对应 RFC 重新提出；不能从本条恢复一套默认通用框架。
