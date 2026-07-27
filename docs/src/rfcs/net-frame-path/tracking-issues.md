# Network Frame Path Tracking Issues

**状态：** Closed / 0 Apollyon / 0 Keter / 0 Euclid
**最后更新：** 2026-07-27
**父 RFC：** [RFC-20260726-net-frame-path](./index.md)
**事务日志：** [2026-07-26 net-frame-path](../../devlog/transactions/2026-07-26-net-frame-path.md)已Completed；
[Stage 4 transaction](../../devlog/transactions/2026-07-27-net-frame-path-stage4.md)已Completed

本文只跟踪会影响 implementation readiness、owner boundary、停止条件或最终验收的 design / feasibility
问题。普通类型选择、stage TODO、case inventory 和未运行证据不进入本页。

R1、Stage 1-3与`NFP-FINAL-CUTOVER`的历史closure保持；post-close review新增的NFP-008/009/010已由单一
Stage 4 neutralize，R1重新Closed。neutralized项继续保留原问题、依据与重新打开条件，不成为current contract
或进度账本。

## Apollyon

当前无项。

## Keter

当前无项。

## Euclid

当前无项。

## Safe

当前无项。pool/backing 类型、worker 数量、budget 数值、test fixture 形状和精确 case inventory 是
后续 stage 解析输入，不作为 Safe issue 堆放。

## Neutralized

### NFP-008 — network activation依赖同级Late initcall的偶然顺序

**状态：** Neutralized by Stage 4
**来源：** 2026-07-27 Stage 3 post-close software-engineering review
**影响：** `NET-ATTACH-001` time wiring publication obligation、boot ordering
**依据：** [Stage 4 closure](./implementation.md#107-closure)与
[Stage 4 transaction](../../devlog/transactions/2026-07-27-net-frame-path-stage4.md)

修正前network attach与threaded timer worker都使用`#[initcall(late)]`，但`Late`只表达共同启动窗口，不提供
consumer之间的相对顺序。attach会在worker/wake/time wiring准备后发布active path，后续future deadline可能
立即进入threaded timer；若network恰好先执行，timer未初始化会触发correctness assertion。当前ELF排列恰好
timer在前不构成contract。

Stage 4没有增加initcall level、priority或readiness framework，也没有调整linker section。network attach已经
退出`Late` initcall，由boot coordinator在`run_initcalls(InitCallLevel::Late)`完整返回后、用户态init前显式
调用；timer继续是普通`Late` provider。source audit、260/260 KUnit与RV64 active attach通过。若未来platform
绕开该boot coordinator顺序，必须重新打开本项。

### NFP-009 — published capability的pending handoff绕过device/net owner

**状态：** Neutralized by Stage 4
**来源：** 2026-07-27 Stage 3 post-close software-engineering review
**影响：** `NETDEV-LIFE-001`、`NET-ATTACH-001`、dependency direction与attach failure retention
**依据：** [Stage 4 closure](./implementation.md#107-closure)、
[netdev lifecycle current contract](../../contracts/net/netdev-lifecycle.md#netdev-life-001--boot-time-identity与publication是单向transaction)与
[attach lifecycle current contract](../../contracts/net/attach-lifecycle.md#net-attach-001--attach-publicationrollback与best-effort-shutdown)

修正前registry只保留`NetdevSnapshot`，真正的`PublishedNetdev<VirtIONetProvider>`由concrete VirtIO-Net driver
保存，并由kernel net通过driver-specific drain取得。这样publication record与pending frame capability分属两个
owner，kernel attach还反向依赖concrete driver discovery；attach失败消费capability后只保留registry snapshot，
不再有registry-owned可达handoff。

Stage 4已经把异构pending storage/drain归还`device/net`，与record在同一publication transaction提交。
dynamic dispatch只位于一次性pending attach边界；concrete `P`随即进入generic prepare，worker的
`PumpCore<P>`、token与data plane继续单态化。失败撤销mapping后把同一capability交还registry retention，当前
drain不自动重试；driver-owned slot和specific drain已删除。KUnit覆盖两个concrete provider、success/failure
retention与record isolation。若未来重新出现driver-specific drain、capability loss、第二份lifecycle truth或
`dyn FrameProvider` data path，必须重开本项。

### NFP-010 — 长期host conformance target缺少required-features

**状态：** Neutralized by Stage 4
**来源：** 2026-07-27 Stage 3 post-close software-engineering review
**影响：** production no-default feature proof与长期host conformance discoverability
**依据：** [Stage 4 closure](./implementation.md#107-closure)与
[Stage 4 transaction](../../devlog/transactions/2026-07-27-net-frame-path-stage4.md)

修正前`frame_path`已显式声明`required-features = ["host-test"]`，但同为长期host conformance的
`bounded_progress`与`multi_instance`仍由Cargo自动发现。两者调用只在`host-test`下存在的stack helper，导致
`cargo test -p anemone-smoltcp-stack --no-default-features --no-run`产生13个`E0599`。production kernel build
不受影响，因此定为Euclid而非产品blocker。

Stage 4已经为两个target补齐与`frame_path`一致的显式metadata。default host gate实际运行全部三个target；
no-default test compile与production check均通过，`bounded_progress`、`multi_instance`、host helper与production
feature graph没有删除或旁路。已删除的`icmp-validation-probe`保持删除。若长期host target再次依赖隐式
autodiscovery或在no-default下被错误编译，必须重开本项。

### NFP-007 — QEMU saturation proof 依赖非确定性 completion 时序

**状态：** Neutralized by R1 acceptance proof correction
**来源：** 2026-07-27 Stage 2 Checkpoint 1 negative evidence
**影响：** `NET-FRAME-PROGRESS-001` cutover proof、Stage 2 Checkpoint 1/3、validation floor
**依据：** [R1 acceptance](./index.md#r1-acceptance)、
[R1 Stage 2 Ready](./implementation.md#8-stage-2-readybounded-progress-conformance)与
[transaction](../../devlog/transactions/2026-07-26-net-frame-path.md)

原路线假设预排超过 TX slot 数的 ICMP burst 必然使 RV64 TCG 至少一次返回 normal exhaustion。真实运行没有
观察到`queue-full > 0`：总 packet 数大于 capacity 不等于 concurrent outstanding 超过 capacity，provider又会
在每次 admission 前回收已经完成的 TX。为制造 PASS 而延迟 completion、降低 production capacity或伪造queue
state都会污染被验证对象。

R1 保持 normal exhaustion/recovery target 不变，只修正 proof owner：host real-stack + deterministic provider
负责确定性 credit exhaustion、matching completion/recheck与恢复；RV64负责真实VirtIO bounded outstanding、
TX/RX completion、IRQ、mapping回落、有限worker action和正常关机。自然观察到的RV64 exhaustion形成附加恢复
证据，但`queue-full == 0`本身不再是失败。旧probe失败与删除事实保留，不改写为成功。

### NFP-004 — concrete VirtIO boundary 泄漏到通用 worker

**状态：** Neutralized by Stage 1 -> 2 Boundary Interlude
**来源：** 2026-07-26 Stage 1 post-close module-boundary review
**影响：** `NET-BOUNDARY-001`、`NET-FRAME-PROGRESS-001`、`NET-STACK-PUMP-001`
**依据：** [Boundary Interlude closure](./implementation.md#75-closure)与
[transaction](../../devlog/transactions/2026-07-26-net-frame-path.md)

原实现让`net::worker`直接import concrete VirtIO provider、VirtIO-named wake trait与diagnostic snapshot，
迫使driver module扩大crate-wide visibility。间章把durable predicate + stateless wake handoff放入kernel-local
`device/net` port；worker只依赖`NetdevFrameProvider` / `RecheckWake`并对provider泛型化，
`driver::net::virtio`恢复private。kernel wake没有进入shared API，task/driver private state也没有跨边界泄漏。

### NFP-005 — KUnit harness vocabulary 跨入 stack crate

**状态：** Neutralized by Stage 1 -> 2 Boundary Interlude
**来源：** 2026-07-26 Stage 1 post-close module-boundary review
**影响：** validation boundary、dependency direction、temporary-probe exit condition
**依据：** [Boundary Interlude closure](./implementation.md#75-closure)与
[transaction](../../devlog/transactions/2026-07-26-net-frame-path.md)

stack feature、type与method现只表达`icmp-validation-probe` / `IcmpEchoProbe` capability；两个first-party
crate中不再出现KUnit vocabulary，request/wait/assert全部留在kernel validation module。probe仍是临时
validation seam，正式control-plane出现或`NFP-FINAL-CUTOVER`审计时必须删除或替换，不能成为production
endpoint API。

### NFP-006 — stable module roles集中在少数文件

**状态：** Neutralized by Stage 1 -> 2 Boundary Interlude
**来源：** 2026-07-26 Stage 1 post-close module-boundary review
**影响：** Stage 2 implementation order、visibility与reviewability
**依据：** [Boundary Interlude closure](./implementation.md#75-closure)与
[transaction](../../devlog/transactions/2026-07-26-net-frame-path.md)

API、stack、device/net与VirtIO-Net均按已经存在的stable owner role做same-owner directory split，root
re-export和public API不变，也没有增加新crate/framework。source/unsafe/runtime audit确认begin/complete
window、drop order、publication与attach行为未变；结构拆分不再迫使concrete driver visibility向外扩张。

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
编译期静态 plan；emergency 则跳过全部 ordinary callback。R1 acceptance 时的 effective plan 仍是
`filesystem -> device`，因此 NFP target 将 `SYSTEM-POWER-ORDERLY-001` Refine 为在 `device` 前调用唯一
network owner facade；该 Refine 现已由 `NFP-FINAL-CUTOVER` 生效。global order 属于 `power`，
network-local admission/cancel/drain attempt 属于 attach authority，driver 继续只拥有
IRQ/queue/DMA/device-local shutdown。

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
