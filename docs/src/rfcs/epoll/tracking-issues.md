# Epoll RFC Tracking Issues

**状态：** Active
**最后更新：** 2026-07-26
**父 RFC：** [RFC-20260726-epoll](./index.md)
**事务日志：** None

本文只跟踪当前仍影响 Draft target、实现顺序、review gate、停止边界或验收判断
的 confirmed design issues。普通实现 TODO、具体 Rust encoding 选择和尚未验证的
性能猜测不放在这里。

2026-07-26 workflow migration 已把 current effective baseline 提取到
`SCHED-LATCH-*`、`IOMUX-POLL-*` 与 `OPENED-DESC-*`，并在
[Contract Impact](./invariants.md#contract-impact) 中记录 target delta。该迁移没有
创建 `implementation.md` 或启动 transaction。2026-07-26 后续 review 已通过 per-`Epoll`
operation mutex neutralize K1，通过现有 `fs::api` 边界 neutralize E2，并通过
consumer-owned validity/lifetime 与 source-side 有界惰性 pruning 的方向 neutralize E1。
K3 已进一步收敛为 callback-safe sticky recheck handoff，不再要求逐 callback 记账。
K2 也已收敛为 non-owning、terminal opened-description liveness capability；dynamic
final-release observer registry 不再是 target 前置条件。K4 通过“统一 source-facing
protocol、保留 source-local registry/lock/handoff”与 future proof-first gate 的方向
neutralize，不再把统一误判为单一通用 wrapper 或纯机械迁移。
开发者随后授权创建 [实施计划](./implementation.md)；该文件只把 Stage 0 解析为 Ready，并在
后续 review 中按 owner 与恢复边界拆成 0A terminal liveness、0B observer/pipe、0C timerfd
noirq、0D TTY/closure 四个 checkpoint；这不改变本页 issue 结论，也不形成 accepted R0、
transaction 或代码执行授权。

## Apollyon

None.

## Keter

None.

## Euclid

None.

## Safe

None.

## Neutralized

### EPOLL-DRAFT-K4 - 协议统一被误判为单一 wrapper / registry

**状态：** Neutralized in Draft target / 2026-07-26
**影响范围：** readiness subscription / source owner / implementation resolution / ROI
**来源：** 2026-07-26 工程 ROI review

**原问题：** 草案要求 poll/select 与 epoll 离开两套 source protocol，但“统一”可能被误读为
所有 source 必须套用一个通用 registry/container，或把现有 `LatchTrigger` 换成
`PollObserver` 就完成了机械迁移。前一种理解会让通用层侵入 predicate、锁、容量和
IRQ/noirq handoff owner；后一种理解又忽略了 ready-at-subscribe 仍安装 route、notification
后不自动删除、consumer retirement 与晚到 callback 的新生命周期证明。

**关闭决策：** target 统一 source-facing subscription protocol：observer capability、
subscribe + current snapshot 事务、consumer-owned retirement/fail-closed 与 source-owned
有界 cleanup。具体 registry storage、锁类型、容量/分配、interest filtering 和 guard-out
batch/handoff 继续由 source owner 决定；不同 owner-local 实现不构成并列协议，单一通用
registry 也不是 ROI 或 acceptance 成功条件。

future implementation planning 必须在全量 `SUBSCRIPTION-CUTOVER` 前设置 proof-first gate，
覆盖普通 task-context、noirq/fixed-capacity 与预分配 dirty handoff 三类约束。若代表性
source 只能依赖 feature-specific downcast、同步 callback drain、第二份 readiness/liveness
truth，或破坏原有执行上下文合同，必须停止并进入 Target Renegotiation Gate；不能用
per-source escape hatch 维持名义统一。具体 probe 计划已经写入 [实施计划](./implementation.md) 的
Stage 0 Ready，并由 0B-0D 分别证明 ordinary、noirq/fixed-capacity 与预分配 dirty handoff；
在 R0 acceptance、transaction 与开发者启动授权前不得进入 Active。

**修复位置：** [RFC 通用 subscription 与风险](./index.md#通用-subscription)、
[协议统一与 source-local 实现边界](./invariants.md#协议统一与-source-local-实现边界)、
[工程 ROI 结论](./backgrounds/positioning.md#工程-roi-结论)和
[RFC-local Invariants](./invariants.md#rfc-local-invariants)。

**重新打开条件：** future owner surface 要求所有 source 共享一个行为型 registry/lock；
poll/select 与 epoll 仍长期保留不同 register/wake/cleanup 协议；公共 helper 需要 source 或
consumer private downcast；或困难 source 无法在不新增第二份状态真相、不破坏上下文边界的
条件下实现共同 subscription contract。

### EPOLL-DRAFT-K2 - opened-description retirement 被过早绑定到动态 observer

**状态：** Neutralized in Draft target / 2026-07-26
**影响范围：** `task::files` / target final close / watch identity / lazy retirement
**来源：** 2026-07-14 文档层 review；2026-07-26 owner/capability 复核

**原问题：** 当前 [`OPENED-DESC-001..003`](../../contracts/task/opened-description-lifecycle.md)
明确 `ProcFile::description_refs` 是最后一个 published fd reference 的唯一真相，而
`FileDescOps::final_release` 只是 opened description 创建时固定的单 hook。早期 K2 从
“该 hook 不能动态组合多个 epoll watches”直接推出“必须增加 dynamic final-release
observer registry”，把一种主动 cleanup 机制误当成了 epoll 可见语义。

**关闭决策：** `task::files` 提供 feature-neutral、opaque、non-owning 的
opened-description identity/liveness capability。它只允许比较 opened-description identity、
尝试取得本次 operation 使用的短生命周期 live target lease，并在 event/watch commit 前
重新验证 terminal liveness；不暴露 `ProcFile`、published-ref count 或 fd-table 私有状态。

opened-description lifecycle 在 target 中单调推进
`Unpublished -> Live(n) -> Retired`。最后一个 published ref 的 `1 -> 0` 线性化为
`Retired`，旧 capability 永久不可复活；dup/fork aliases 尚存时仍保持 `Live`。当前 live
source 的 `FileDesc::clone()`、public `FdReservation::commit()` 与普通 acquire 路径尚未从
类型上排除延迟 `0 -> 1` publication，因此 future implementation resolution 必须通过
最小 source audit/probe 选择 lifecycle encoding 或 publication assertion，不能假设现状
已经满足 terminal retirement。

`EpollWatch` 不长期强持有 target `File` / opened description。ctl/harvest 可以在
per-`Epoll` operation mutex 下临时取得 live lease，但必须在 watch/event commit 前最终
验证 liveness：retirement 先发生则丢弃并惰性移除；commit 先发生则该结果在线性化顺序上
先于 close，不追溯撤回。final close 不获取 epoll mutex、不扫描 epoll、不要求唤醒
`epoll_wait()`；stale watch、ready entry 与 source route 在后续 ctl/harvest/teardown 中
惰性清理，物理 cleanup 不参与用户可见正确性。

dynamic final-release observer registry 只保留为 fallback：只有 live-source probe 证明
non-owning capability 无法安全取得 transient target lease、惰性清理无法满足明确资源上界、
或 accepted target 新增 close-driven wake/cleanup 义务时，才回到 RFC review 讨论。

**修复位置：** [RFC 生命周期与 Contract Impact](./index.md#生命周期)、
[watch identity / liveness](./invariants.md#watch-identity-与-opened-description-liveness)、
[Ctl、Retirement 与 Final Release](./invariants.md#ctlretirement-与-final-release)和
[引用与 Teardown](./invariants.md#引用与-teardown)。

**重新打开条件：** capability 依赖强持 target 才能判断 semantic final close；terminal
retirement 可以从 `Retired` 复活；harvest 无法在 commit 前形成线性化 liveness validation；
lazy stale metadata/route 超出明确资源上界；或实现/ABI 证据证明 final close 必须主动进入
epoll 才能保持 accepted semantics。

### EPOLL-DRAFT-E1 - cancel 返回边界尚未集中定义

**状态：** Neutralized in Draft target / 2026-07-26
**影响范围：** readiness registration / wait round / DEL / MOD / teardown
**来源：** 2026-07-14 文档层 review；2026-07-26 严重度复核与对象模型修正

**原问题：** 早期定位把 `PollSubscription` 预设为 consumer-side linear cancellation
handle，于是必须回答 cancel 返回后 source 能否继续选择 callback、飞行中 callback
是否允许晚到，以及 cancel 是否承担 drain。该问题一度提升为 Keter，因为 poll round、
epoll ctl、final close 与 teardown 不能各自发明不同合同。

**关闭决策：** syscall 只要求 interest / wait round 在 consumer owner 处逻辑失效，
不要求内核提供通用、同步、带 drain 语义的 source-side cancel。source registry entry
只保存非拥有 notification route；`IomuxWaitRound` 或 `EpollWatch` 的 validity/lifetime
决定旧 hint 是否还能产生行为。consumer retirement 后，已选择或飞行中的 callback
可以晚到，但必须内存安全并 fail closed。source 对 stale route 的 eager unlink 或
惰性 pruning 只服务资源卫生，并必须有界，不能参与用户可见 correctness。

本次只接受 owner 与语义方向；`PollSubscription` 是否保留为类型名、引用表示、validity
encoding、registry 容器和 prune 算法仍留给 future implementation resolution。

**修复位置：** [RFC 摘要与通用 subscription](./index.md#通用-subscription)、
[Target Invariants](./invariants.md#target-invariants)、
[PollSubscription](./invariants.md#pollsubscription) 和
[Ctl、Retirement 与 Final Release](./invariants.md#ctlretirement-与-final-release)。

**重新打开条件：** future implementation 让 source route 拥有 consumer lifetime、
让 validity 与 ready publication 无法形成一致的 fail-closed 顺序、依赖同步 drain 才能
保证正确性，或 stale route 可无界增长而没有 source-owned cleanup boundary。

### EPOLL-DRAFT-K1 - 并发 ctl 缺少事务 owner

**状态：** Neutralized in Draft target / 2026-07-26
**影响范围：** `Epoll` watch table / ADD / MOD / DEL / instance teardown / harvest
**来源：** 2026-07-14 文档层 review

**原问题：** unpublished watch 的 subscribe -> publication 序列无法阻止两个并发 ADD
同时通过 duplicate check，也没有统一 MOD、DEL、instance teardown 与 final-close watch
removal 的 control transaction owner。

**关闭决策：** 每个 `Epoll` 使用一个 private、sleepable operation mutex，串行该
instance 的 ADD/MOD/DEL、teardown 与 harvest。final close 只推进 `task::files` terminal
liveness，不进入该 mutex；其 stale watch 由后续 operation 惰性移除。syscall adapter
在进入 operation 前完成 UAPI copyin、fd 解析和基础校验；mutex 可以覆盖 target
subscribe/snapshot、watch/ready commit 或完整 rollback。source readiness notification、
IRQ/noirq handoff 与 task wait 不获取该 mutex。

该 mutex 只提供 operation serialization，不保存 watch、generation、readiness 或
lifecycle 的第二份真相。不同 epoll instances 不为首版无 nesting 的目标支付全局串行化；
同一 instance 内则明确接受 ctl 与并发 harvest 的吞吐损失，以换取更小的状态机与证明
边界。future nesting 若被接受，必须独立解析跨 instance graph admission。

**修复位置：** [RFC epoll owner](./index.md#epoll-owner)、
[Epoll Watch Publication](./invariants.md#epoll-watch-publication)、
[Ctl、Retirement 与 Final Release](./invariants.md#ctlretirement-与-final-release) 和
[锁序与执行上下文](./invariants.md#锁序与执行上下文)。

**重新打开条件：** source readiness callback、IRQ/noirq notification 或 task wait 被迫
获取该 sleepable mutex；某个 ctl/teardown/harvest path 可以绕过 mutex 推进同一
instance 的行为状态；或实现证据表明 operation serialization 无法覆盖 commit/rollback
而必须改变 owner、ABI 或 acceptance boundary。

### EPOLL-DRAFT-K3 - ready harvest 被错误提升为逐 callback 记账合同

**状态：** Neutralized in Draft target / 2026-07-26
**影响范围：** ready queue / harvest / LT / ET / ONESHOT / notification handoff
**来源：** 2026-07-14 文档层 review；2026-07-26 target 边界复核

**原问题：** 早期 K3 要求对每个 callback/harvest 交错证明该 hint 被本轮 snapshot
覆盖或保留为下一轮 candidate，并预设类似
`Idle / Queued / Harvesting { pending } / Disabled` 的精确状态机。这把内部 callback
记账、copyout failure recovery 与未来优化形状抬成了用户可见语义和文档层前置条件。

**关闭决策：** callback 次数不对应 ABI event 次数。每个 `Epoll` 的 operation mutex
串行 ctl、teardown 与 harvest；source notification 不获取该 mutex，只需 callback-safe
地发布一个 sticky pending/dirty recheck obligation。多个 notification 可以合并，实际
observer callback、ready-queue insertion 与 deferred processing 可以延后。harvest 在
宣布无 candidate 并进入 task wait 前，只需完成一次无丢 wake 的 pending handoff：已
发布 obligation 要么被当前 operation 吸收，要么保持可见并触发后续重检。
source notification path 在返回前仍必须完成 pending publication 或等价的持久 handoff；
这里允许延后的是物理处理，不是允许通知义务消失。

LT、ET 与 `EPOLLONESHOT` 的用户可见 policy 仍由持有 operation mutex 的 epoll owner
根据当前 target snapshot 与 watch state 决定；逻辑 DEL/MOD 仍必须立即使旧 generation
fail closed，target final close 则由 terminal opened-description liveness 使 candidate
不可交付。具体 atomic、queue、worker、epoch、状态枚举以及 copyout
partial-progress recovery 留给 future implementation / ABI resolution，不作为逐 callback
proof obligation。

**修复位置：** [RFC 摘要与 epoll owner](./index.md#epoll-owner)、
[Source Notification 线性化](./invariants.md#source-notification-线性化)、
[Ready Queue 与 Harvest](./invariants.md#ready-queue-与-harvest)和
[锁序与执行上下文](./invariants.md#锁序与执行上下文)。

**重新打开条件：** implementation 只能依赖 callback 逐次、立即执行才能避免永久睡眠；
pending handoff 允许已发布 recheck obligation 在无后续 source transition 时永久丢失；
或放松内部记账实际降低了 LT/ET/ONESHOT、logical retirement 或 stale user-data 的
用户可见语义。

### EPOLL-DRAFT-E2 - syscall API 路径与 core owner 边界不清

**状态：** Neutralized in Draft target / 2026-07-26
**影响范围：** module layout / Linux ABI containment
**来源：** 2026-07-14 文档层 review

**原问题：** 历史定位图使用 `fs::epoll::api` 并把 syscall parser 列入 core role，可能
让 Linux struct、flag 与用户指针处理进入 epoll state owner。

**关闭决策：** Linux UAPI conversion、pointer access、flag/opcode/timeout/sigmask
validation 固定在 `fs::api::iomux::epoll*` 或保持同一依赖方向的现有 `fs::api` 边界；
`fs::epoll` 只暴露 typed operations 与 anonymous file backend。具体文件拆分可以留给
future implementation resolution，但不得改变该 owner direction。

**修复位置：** [ABI 边界](./index.md#abi-边界)和
[ABI 边界不变量](./invariants.md#abi-边界)。

**重新打开条件：** future implementation 需要 core 直接读写用户指针、保存 Linux
UAPI struct/bit layout，或让 source subscription API 解释 epoll-specific Linux policy。
