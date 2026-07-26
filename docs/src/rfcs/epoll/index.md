# RFC-20260726-epoll

**状态：** Accepted for Implementation / Stage 0 Active / Not Effective
**修订：** R0
**负责人：** doruche
**最后更新：** 2026-07-26
**领域：** fs / iomux / epoll / task files / scheduler wait
**事务日志：** [2026-07-26-epoll](../../devlog/transactions/2026-07-26-epoll.md)
**影响契约：** Preserve `SCHED-LATCH-001..003`、`SIGNAL-TEMP-MASK-001..003`、`IOMUX-POLL-003`、`OPENED-DESC-003`、`TTY-TERM-001`、`TTY-INPUT-001`；Refine `IOMUX-POLL-001`、`OPENED-DESC-001/002`；Replace `IOMUX-POLL-002`；Introduce `OPENED-DESC-LIVENESS-001`、`EPOLL-WATCH-001`、`EPOLL-READY-001`、`EPOLL-FILE-001`。完整 delta 与 cutover 见 [Contract Impact](./invariants.md#contract-impact)。
**开放问题：** [Tracking Issues](./tracking-issues.md) 当前无开放 Keter。
**下一步：** 执行已授权的 Stage 0 Checkpoint 0D TTY/closure；0D 关闭后停止，不自动进入 Stage 1 resolution gate。

## 文档状态

本文是 epoll R0 accepted target 与 target delta 的公共 canonical source。它把已经形成的
epoll 定位共识整理为已接受的实现目标，并通过
[Contract Impact](./invariants.md#contract-impact) 区分 current effective contract、
尚未生效的 target delta 与 RFC-local proof obligations。[实施计划](./implementation.md)
已经解析三阶段滚动路线与首个 Ready stage；R0 acceptance、transaction bootstrap 与本轮开发者授权
已在 [事务日志](../../devlog/transactions/2026-07-26-epoll.md) 中记录。current contract 在对应
cutover 前保持不变；初始授权覆盖 Stage 0 的 0A、0B，后续明确授权覆盖 0C、0D，且 Stage 1
resolution gate 尚未授权。

## 摘要

Anemone 已有 `PollRequest` / `PollRegisterResult` 与 `sched::Latch`，能够为
`ppoll` / `pselect6` 表达一次 syscall-local OR wait，但 source 当前保存的
`LatchTrigger` 绑定单个 task wait identity，且 `Ready / Armed / Unsupported`
不能同时表达“订阅已经安装”和“注册点当前 ready”。这套协议不能直接承载跨越多次
`epoll_wait()` 的长期 interest relationship。

本 RFC 提议引入通用 readiness registration 合同。具体 source 在同一个状态锁临界区内
安装非拥有的 observer route，并返回注册点的 current readiness；consumer owner
通过 `PollObserver` 接收 recheck hint，并单独拥有旧 hint 是否仍能产生行为的
validity/lifetime 边界。source registry 的物理 entry 只是通知路由，不是 consumer
关系仍有效的第二份真相。

这里统一的是 source-facing protocol，而不是强制所有 source 共用一个 registry
container、锁、容量策略或 notification batch。普通 task-context source、noirq source 与
具有预分配 handoff 的 source 可以保留各自 owner-local storage 和执行上下文约束；只要它们
共同满足 subscribe/snapshot、observer capability、consumer retirement 与有界 cleanup
合同，就不构成第二套 poll 协议。通用 helper 或 registry 只有在不制造第二份状态真相、
不要求 feature-specific downcast 且不削弱困难 source 的上下文边界时才值得采用。

poll/select 通过 `IomuxWaitRound` 把该合同适配到单轮 `Latch`；epoll 则由
`EpollWatch` 长期拥有 callback acceptance 与 policy，并由 `Epoll` 统一拥有 ready queue。
target 不要求通用、同步、带 drain 语义的 source-side `cancel()`；consumer 关系失效后，
旧 route 由 source 按有界资源规则惰性清理。

每个 `Epoll` 还拥有一个 sleepable operation mutex，串行该 instance 的 ctl、teardown
与 harvest 决策。source notification 不获取该 mutex，也不要求立即操作 ready queue；
它只需通过 callback-safe 的 sticky pending/dirty handoff 发布“可能需要重检”。多个通知
可以合并，callback 次数不对应用户可见 event 次数。

首版明确接受同一 epoll instance 内 ctl 与并发 harvest 的吞吐损失，以换取更窄的状态机、
锁序和 proof boundary；没有实际瓶颈证据时不把拆锁或并行 harvest 作为 target 能力。

## 背景

现有 poll/select 路径采用：

```text
snapshot scan -> begin latch -> register scan -> schedule
              -> finish -> final snapshot scan
```

该路径已经闭合单轮 wait identity、timeout/signal 竞争、source 锁内检查与注册，
以及锁外 trigger；它仍是 epoll 的 task-wait 基础，而不是需要替换的旧架构。

epoll 额外要求：

- `EPOLL_CTL_ADD` 建立的 interest 跨越多次 wait 持续存在；
- subscription 在注册点已经 ready 时仍必须保留；
- source hint、epoll ready candidate 和最终 target readiness 必须彼此区分；
- LT、ET、`EPOLLONESHOT`、user data 与 ready coalescing 由 epoll owner 解释；
- ADD/MOD/DEL、target final close、harvest 与并发 callback 必须能失效旧 watch；
- epoll file 自身也必须是普通 pollable source，以支持 `poll(epfd)` 和
  `epoll_wait()` 只等待自身 ready protocol；首版不因此开放 nested epoll。

此前的定位讨论保留在 [epoll 设计定位共识](./backgrounds/positioning.md) 中，作为本 RFC 的
背景材料；若背景文字与本文或 [不变量需求](./invariants.md) 冲突，以当前 RFC
文件和 [Tracking Issues](./tracking-issues.md) 为准。

## 目标

- 定义 source-neutral 的 `PollObserver` / readiness registration 合同。
- 统一 poll/select 与 epoll 的 source-facing subscription protocol，而不强制统一具体 source
  的 registry container、锁、容量、分配和 guard-out handoff 形状。
- 保持 source state 对实际 readiness 与 registry 容器的唯一所有权，同时让 consumer
  owner 单独拥有 callback acceptance / generation。
- 让 poll/select 通过 `IomuxWaitRound` 复用 subscription，而不长期保留第二套
  `LatchTrigger` source protocol。
- 定义 `Epoll`、`EpollWatch`、`EpollFile` 的状态、身份和生命周期边界。
- 支持 Linux asm-generic 的 `epoll_create1`、`epoll_ctl`、`epoll_pwait` 和
  `epoll_pwait2` ABI，并由 libc wrapper 承接 `epoll_create` / `epoll_wait`。
- 为 LT、ET、`EPOLLONESHOT`、ready queue、temporary signal mask 和 target close
  建立可审查的证明边界。
- 在可接受性能下实现完整核心语义，不把 Linux 的红黑树或完整 waitqueue 形状
  作为兼容要求。

## 非目标

- 不把 `Latch` 扩展成跨 wait round 的 permit 或长期 subscription。
- 不新增 scheduler 级 `PollNotifier`，也不让 `sched` 理解 poll/epoll。
- 不把 `Event` 改造成 epoll source registry。
- 不让 source 识别 poll/select/epoll consumer 类型或解释 ET/ONESHOT。
- 不以“所有 source 必须套用单一通用 registry/wrapper”作为统一成功标准，也不为追求代码
  复用抹平 timerfd noirq、TTY 预分配 handoff 等 owner-local 执行上下文约束。
- 不把 callback payload 或 ready queue entry 当作 target readiness truth。
- 不通过遍历所有 target 或 busy polling 实现 epoll wait。
- 不用 epoll 反向实现现有 poll/select syscall。
- 首版不支持把 epoll fd 加入另一个 epoll。该能力保留为后续实现反馈验证与
  follow-up target；首版 `EPOLL_CTL_ADD` 对 epoll target 返回 `EINVAL` 并记录 notice，
  不能让尚未验证 cycle/depth 与并发 admission 的 nesting 因对象可组合而意外生效。
- 不要求复制 Linux `eventpoll` 的红黑树、RCU、slab 或 `ovflist` 具体结构。
- R0 acceptance 与 Stage 0 activation 不替代 checkpoint gate；0C、0D 由后续明确授权单独激活，
  任何 Stage 0 checkpoint 都不授权后续 Stage。

## 文档地图

Target proposal：

- [不变量需求](./invariants.md)
- 本文

Current effective baseline：

- [Scheduler Latch wait round](../../contracts/scheduler/latch-wait-round.md)
- [Signal temporary-mask delivery handoff](../../contracts/signal/temporary-mask-delivery.md)
- [Poll wait 与 source registration](../../contracts/iomux/poll-wait.md)
- [Opened-description lifecycle](../../contracts/task/opened-description-lifecycle.md)
- [Serial TTY data plane](../../contracts/tty/data-plane.md)

Review 状态：

- [Tracking Issues](./tracking-issues.md)

实施路线：

- [实施计划](./implementation.md)

背景材料：

- [背景材料索引](./backgrounds/index.md)
- [epoll 设计定位共识](./backgrounds/positioning.md)

[实施计划](./implementation.md) 已按开发者授权补充 rolling stages，并把 Stage 0 解析为
0A terminal liveness、0B observer/pipe、0C timerfd noirq、0D TTY/closure 四个顺序
checkpoint，分别冻结 write subset、proof-first validation floor、review 与停止/恢复条件。当前所有
Keter 均已在 R0 target 中 neutralize；Stage 0 已通过 transaction preflight 进入 Active，但每个
checkpoint 仍需按自己的交付、review、验证和停止/恢复条件独立关闭。

## 修订记录

| 修订 | 日期 | 状态 | 语义变化 | Review / 事务 |
| --- | --- | --- | --- | --- |
| R0 | 2026-07-26 | Accepted for Implementation | 初始 accepted target；定义 source-neutral subscription、terminal opened-description liveness、epoll owner/ready/file 语义与三个 cutover unit。 | [初始事务](../../devlog/transactions/2026-07-26-epoll.md) |

## 方案

### 通用 subscription

`fs::iomux::subscription` 或等价 owner surface 定义：

- `PollEvent`：内部 readiness snapshot / recheck hint 表达；
- `PollObserver`：consumer 提供的 no-return、fail-closed callback capability；
- readiness registration：source-local 的非拥有通知路由；
- subscribe result：同时确认 route 已建立并返回 current readiness。

source-local registry 继续位于 pipe、eventfd、timerfd、fanotify、未来 socket 和
epoll file 自身的状态 owner 内。subscribe 在 source lock 下安装 entry 并读取
current readiness；source 状态变化时锁内筛选 observer，锁外调用 callback。consumer
round 或 watch 一旦失效，旧 callback 只能成为 stale hint；source 对失效 route 的
pruning 是有资源上界的 registry hygiene，不参与用户可见 correctness。

公共面只约束 observer capability、subscribe + current snapshot 的事务语义、consumer
retirement 后的 fail-closed 行为与 cleanup 上界。predicate 计算、entry storage、锁类型、
容量/分配策略、interest candidate 选择和锁外 batch/handoff 继续由具体 source owner
决定。因而多数 source 的最终迁移可以复用相同协议骨架，但 timerfd 的 noirq/fixed-capacity
路径和 TTY 的预分配 dirty handoff 仍是必须单独证明的 owner-local 路径，不能被“只是换
wrapper”这一工程估算隐藏。

`PollSubscription` 可以继续作为这段关系的工作名，但 target 不预设它必须表现为
consumer-side cancellation handle，也不预设同步 unlink、callback drain、引用表示或
具体 prune 算法。若实现提供 eager unlink，它只能是资源回收优化，不能成为正确性条件。

### poll/select wait round

`IomuxWaitRound` 持有一个 `Latch`、一个共享 `LatchPollObserver`，并拥有本轮 callback
acceptance lifetime。任意 source hint 可以完成本轮 Latch；返回用户态前必须 retire
本轮 wait identity / observer acceptance，并重新 snapshot 实际 readiness。它不需要
同步进入每个 source 撤销 registry entry。

`LatchTrigger` 只存在于 `LatchPollObserver` 内部，不能泄漏到 source-facing
subscription API。现有 `PollRequest::register(&LatchTrigger)` 只能作为带删除条件的
迁移桥。

### epoll owner

`Epoll` 拥有 watch table、ready candidate queue、epoll-file readiness 与 harvest
流程。`EpollWatch` 拥有 target identity、用户 fd key、internal interest、user data、
LT/ET/ONESHOT policy、source observer relationship、non-owning opened-description
identity/liveness capability、active/generation 与 ready protocol state。

target source state 始终拥有实际 readable/writable/error/hangup predicate。
source callback 只要求 watch 重新检查；ready queue 只保存候选 watch identity。
事件交付用户态前必须重新 snapshot target，并按 watch 当前 policy 生成结果。

`EpollFile` 只是 anonymous file 外壳，内部持有 `Arc<Epoll>`，自身通过相同的
subscription contract 暴露 readability。epoll wait 在没有可交付 candidate 时只
订阅 epoll file 自身，而不临时重新订阅全部 target。

每个 `Epoll` 拥有一个 private、sleepable operation mutex，串行该 instance 的 ADD / MOD /
DEL、instance teardown 与 ready harvest。syscall adapter 在进入
operation 前完成 UAPI copyin、fd 解析与基础校验；mutex 可以覆盖 target subscribe /
snapshot 与 watch/ready commit 或 rollback。它只提供 operation serialization，不保存
watch、generation、readiness 或 lifecycle 的第二份真相。

source readiness notification、IRQ/noirq handoff 与 task wait 不获取该 mutex。notification
只需 callback-safe 地发布 sticky pending/dirty recheck obligation；实际 observer callback、
ready-queue insertion 或 deferred processing 可以稍后发生并合并。首版不开放 nested epoll；
future nesting 若被接受，需要独立解析跨 instance graph admission，不能假设单 instance
operation mutex 已经提供全局 graph serialization。

### 生命周期

watch table 以 Linux 语义上的 opened-file-description identity 与用户 fd key
共同区分 entry。`Epoll` 拥有 watches；watch 不反向拥有 owner `Epoll`；source
registry 的 observer route 不拥有 consumer lifetime，禁止形成 owner 生命周期环。

`task::files` 继续拥有 dup/fork sharing 与最后一个 published fd reference 的
release 事实，并提供 opaque、non-owning 的 opened-description identity/liveness
capability。target lifecycle 单调推进 `Unpublished -> Live(n) -> Retired`；首次最终
`1 -> 0` 后旧 capability 永久不可复活。

watch 不长期强持 target。ctl/harvest 可以取得短生命周期 live target lease，并在
watch/event commit 前最终验证 liveness；retirement 先发生则丢弃并惰性移除，commit
先发生则结果在线性化顺序上先于 close。`close()` 不进入 epoll、不获取 operation mutex、
不要求唤醒 `epoll_wait()`。dynamic final-release observer registry 只在 capability probe
失败、资源上界无法闭合或 accepted target 新增 close-driven 义务时重新讨论。

### ABI 边界

Linux `epoll_event`、ctl opcode、`EPOLL_*` bits、timeout layout 与 user pointer
校验只存在于 `anemone_abi` 和 syscall/API 层。`fs::epoll` 内部只使用 Anemone
语义类型；source subscription API 不接收 Linux UAPI struct，也不解释
`EPOLLET`、`EPOLLONESHOT` 或 `EPOLLEXCLUSIVE`。

## Contract Impact 概览

本 RFC 不把 `sched-latch` 历史 RFC 或本文的不变量直接当作 current contract。
当前有效规则只由 `docs/src/contracts/` 下已经提取的 `SCHED-LATCH-*`、
`IOMUX-POLL-*` 与 `OPENED-DESC-*` 拥有；完整 delta、变化分类和未来 cutover unit 见
[不变量需求](./invariants.md#contract-impact)。

R0 target 要求：

- Preserve `SCHED-LATCH-*`、`SIGNAL-TEMP-MASK-*` 与 final readiness recheck；
- Refine / Replace 当前 one-round iomux source registration，使 poll/select 经 adapter
  使用 source-owned persistent observer routes；
- Refine published-ref truth 为 terminal retirement，并 Introduce non-owning
  opened-description identity/liveness capability；保留现有静态 final-release hook，
  不预设 dynamic observer registry；
- Introduce epoll watch、ready protocol 与 epoll-file pollability 的长期规则；nested
  epoll 不属于首版 `EPOLL-CUTOVER`。
- Preserve `TTY-TERM-001` / `TTY-INPUT-001` 的 Terminal/input readiness truth；TTY
  representative slice 只替换 poll route，不改变 record boundary。

以上 target 在对应 cutover 完成前都不是 effective behavior。R0 acceptance 与 Stage 0
production-shaped slice 不修改 current contract 语义；Stage 0 的 contract cutover 固定为 `None`。

## 接受边界

本文已经作为 R0 Accepted for Implementation，但不表示任何 `Contract Impact` 已经 cut over。
当前文档层已经闭合七项边界：统一只约束
source-facing subscription protocol，不强制统一 source-local registry/lock/handoff；每个
`Epoll` 使用一个 sleepable operation mutex 串行 ctl、teardown 与 harvest；source
notification 只发布
可合并、可延后的 sticky recheck obligation；Linux UAPI adapter 留在 `fs::api` 边界；
首版明确不开放 nested epoll；readiness registration 不要求通用同步 cancel，
consumer-owned validity/lifetime 与 source-owned 有界 route cleanup 分别承担正确性和
资源卫生；opened description 通过 non-owning terminal liveness capability 被 epoll
验证，final close 不同步进入 epoll。

[实施计划](./implementation.md) 已把首个 proof-first slice 解析为 Stage 0，并在本轮授权下进入
Active：0A 先
fail-fast 验证 opened-description terminal liveness，0B 用 pipe 建立公共 observer/route 与
ordinary task-context slice，0C/0D 再分别证明 timerfd noirq/fixed-capacity 与 TTY 预分配
handoff，最后由 0D 完成一次 Stage 级 runtime/review closure。Stage 0 不执行 contract cutover，
任一 checkpoint 都不能单独合入。后续全量 source cutover 与 epoll ABI 仍保持 Outline，只在前一
阶段独立关闭后解析。影响 owner、ABI、可见语义或接受边界的反馈仍必须回到 RFC review。

## 备选方案

### Dynamic final-release observer registry

不作为首选 target。现有证据只说明创建时固定的单 `FileDescOps::final_release` 不能动态
组合 epoll watches，没有证明 final close 必须主动通知 epoll。首版先使用 non-owning
identity/liveness capability 与 ctl/harvest lazy retirement；只有 capability probe、资源
上界或新增 close-driven 语义失败时，才把 registry 作为 target renegotiation 选项。

### 长期保存 `LatchTrigger`

拒绝。它绑定某个 task 的单次 wait identity，跨轮次保存会破坏 stale-safe
completion 边界。

### source callback 后自动删除 registration，再由 epoll 重订阅

拒绝作为通用合同。它把 rearm 窗口和 ET correctness 推给 consumer，也不利于
`EPOLL_CTL_ADD` 在当前 ready 时持续安装 observer route。route 不因一次 callback 自动
删除；consumer 是否仍接受 hint 由自己的 round/watch state 决定。

### 复用 `Event`

拒绝。`Event` listener 是 task-bound wait listener；source 到 epoll watch 是
非 task consumer 的长期关系。

### epoll 私有 source hook

拒绝。它会让 pipe/eventfd/timerfd 等 source 识别 epoll，并与 poll/select 的
注册协议形成并列真相源。

### 完全照搬 Linux eventpoll 数据结构

拒绝。兼容目标是 ABI 和外部语义，不是红黑树、RCU 或 ready-list 内部形状。

## 风险

- poll/select source 合同迁移会触碰多个共享 source；必须保持现有 final scan、
  timeout/signal outcome 和 source-lock/wake 顺序不回退。
- source 初始改动可能大量表现为 typed wrapper/entry 替换，但 persistent route 在“注册点
  已 ready 时仍安装”、notification 后不自动删除、consumer retirement 后允许晚到 callback
  三处改变了生命周期证明。future resolution 必须把 eventfd/pipe/fanotify 类普通路径与
  timerfd noirq、TTY 预分配 handoff 等困难路径分开证明，不能按 diff 形状把整个 cutover
  归类为机械重命名。
- persistent registration 增加长期 routing entry；IRQ-off notification、容量上界、
  allocation failure 和惰性 pruning 的停止边界必须在未来实现规划前明确。
- target final close、watch replacement 与飞行中 callback 交错容易制造双重状态；
  opened-description terminal liveness 只能由 `task::files` capability 判断，watch
  active/generation 不能替代它或 source registry truth。
- epoll file 的普通 pollability 会为 future nesting 保留自然组合路径，但首版必须
  fail closed 拒绝 epoll target；cycle/depth、并发 graph admission 与 callback propagation
  只在后续 target 接受后解析，不能反向阻塞当前对象模型。

## 收口

R0 已接受，transaction 已建立，Stage 0 已按开发者授权进入 Active。当前 Keter 已 neutralize，
target / current / RFC-local 分层和 [实施计划](./implementation.md) 保持权威；0A-0C 已独立关闭，
0D 已获授权但尚未关闭。Stage 0 closure 不会自动进入 Stage 1，也不会修改 current contract。
