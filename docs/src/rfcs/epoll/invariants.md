# Epoll 与 Poll Subscription 不变量需求

**状态：** Accepted Target / Not Effective
**最后更新：** 2026-07-26
**父 RFC：** [RFC-20260726-epoll](./index.md)
**适用修订：** R0

本文保存 epoll R0 相对 current effective contract 的 target delta，以及本 RFC
自己的 proof obligations。当前生效规则以 `docs/src/contracts/` 为唯一权威；本文是
accepted target，但在 contract cutover 前不是 current behavior。

## Contract Impact

下表中的 cutover 名称只是语义切换单元，不是 implementation stage，也不授权执行。
[实施计划](./implementation.md) 已把它们绑定到三阶段滚动路线、验证和回滚边界；
Stage 0 不切换 contract，Stage 1 / Stage 2 分别拥有 foundation 与 epoll cutover。

| Contract ID | 变化 | 当前规则 | Target 摘要 | 生效边界 |
| --- | --- | --- | --- | --- |
| [`SCHED-LATCH-001..003`](../../contracts/scheduler/latch-wait-round.md) | Preserve | 单轮 wait identity、owner-bound lifecycle、no-return stale-safe trigger | `Latch` 继续只承载 task-local 单轮等待；persistent subscription 不进入 scheduler | 全程 |
| [`SIGNAL-TEMP-MASK-001..003`](../../contracts/signal/temporary-mask-delivery.md) | Preserve | temporary mask、delivery reservation 与 restore responsibility 由 Signal owner 线性收口 | `IomuxWaitRound` 只替换 readiness registration owner，不取得或复制 mask/restore truth，现有 ppoll/pselect outcome classification 保持 | 全程 |
| [`IOMUX-POLL-001`](../../contracts/iomux/poll-wait.md#iomux-poll-001--阻塞前必须完成-snapshotregister-gate) | Refine | snapshot/register/final-scan 使用单轮 `LatchTrigger` register | poll/select 通过 `IomuxWaitRound` 建立 persistent observer routes，仍保留阻塞前完整 arm gate | `SUBSCRIPTION-CUTOVER` |
| [`IOMUX-POLL-002`](../../contracts/iomux/poll-wait.md#iomux-poll-002--source-锁拥有-readiness-与-trigger-publication) | Replace | source 锁内发布并 detach 单轮 `LatchTrigger`，锁外 trigger | source 锁内安装非拥有 observer route 并返回 current readiness；状态变化锁内选择 observer、锁外 callback；consumer retirement 使旧 hint fail closed，source 有界清理 stale routes | `SUBSCRIPTION-CUTOVER` |
| [`IOMUX-POLL-003`](../../contracts/iomux/poll-wait.md#iomux-poll-003--wake-只是-hint最终-predicate-决定返回) | Preserve | wake 只是 hint，final predicate 决定返回 | poll/select 与 epoll 都不得把 callback/queue payload 当成 target readiness truth | 全程 |
| [`OPENED-DESC-001`](../../contracts/task/opened-description-lifecycle.md#opened-desc-001--published-slot-refcount-是-final-release-的唯一真相) | Refine | published fd-slot refcount 决定 semantic final release | 保持 `task::files` 的唯一真相，并使 lifecycle 单调推进 `Unpublished -> Live(n) -> Retired`；首次最终 `1 -> 0` 后不可重新 publication | `OPENED-DESC-CAPABILITY-CUTOVER` |
| [`OPENED-DESC-002`](../../contracts/task/opened-description-lifecycle.md#opened-desc-002--dupfork-共享-descriptionfd-table-只拥有-publication) | Refine | dup/fork 共享 opened description；fd reuse 不保留旧 identity | 保持 sharing/identity truth，并提供不暴露 `task::files` 私有状态的 non-owning identity/liveness capability；watch key 组合该 identity 与用户 fd key | `OPENED-DESC-CAPABILITY-CUTOVER` |
| [`OPENED-DESC-003`](../../contracts/task/opened-description-lifecycle.md#opened-desc-003--当前-final-release-callback-是创建时固定的单-hook) | Preserve | 创建时固定的单 `final_release` hook | 保留现有 hook，不覆盖、不动态组合，也不把它扩张成 epoll observer registry | 全程 |
| `OPENED-DESC-LIVENESS-001` | Introduce | None（尚未生效） | `task::files` 提供 opaque、non-owning、terminal 的 identity/liveness capability 与短生命周期 live target lease | `OPENED-DESC-CAPABILITY-CUTOVER` |
| `EPOLL-WATCH-001` | Introduce | None（尚未生效） | `Epoll` / `EpollWatch` 唯一拥有 interest、generation、policy 与 publication；每个 `Epoll` 的 operation mutex 只串行 ctl、teardown 与 harvest，不保存第二份状态 | `EPOLL-CUTOVER` |
| `EPOLL-READY-001` | Introduce | None（尚未生效） | ready queue 只保存 candidate；notification 只发布可合并的 sticky recheck obligation；harvest/requeue/disable 由 epoll owner 线性化 | `EPOLL-CUTOVER` |
| `EPOLL-FILE-001` | Introduce | None（尚未生效） | epoll file 通过同一 source subscription contract 暴露 readability；首版不开放 nested epoll | `EPOLL-CUTOVER` |
| [`TTY-TERM-001`](../../contracts/tty/data-plane.md#tty-term-001--endpoint共享唯一terminal-semantic-truth) | Preserve | 共享 `Terminal` 唯一拥有 termios、input 与 readiness predicate | Stage 0/1 只替换 poll routing capability，不复制或下沉 Terminal readiness truth | 全程 |
| [`TTY-INPUT-001`](../../contracts/tty/data-plane.md#tty-input-001--input-ownershiprecord-boundary与readiness同源) | Preserve | input publication、read/poll predicate 与 durable recheck 同源 | TTY representative slice 保留 record/readiness owner 与预分配 dirty handoff，只迁移 consumer route | 全程 |

`SUBSCRIPTION-CUTOVER` 必须让所有参与阻塞的 source 与 poll/select adapter 同时离开旧
one-round source protocol；失败时保持当前 `IOMUX-POLL-*` effective rules，不能长期保留
两套 registry。`OPENED-DESC-CAPABILITY-CUTOVER` 保持 published-ref 与 dup/fork sharing
的 owner，增加 terminal retirement 与 non-owning identity/liveness capability；它不改变
`OPENED-DESC-003` 的静态 hook 语义，也不引入 dynamic observer registry。
`EPOLL-CUTOVER` 只有在依赖的两个 cutover 已有效、epoll ABI
与 target proof obligations 通过后才能开放 syscalls 和新 contract IDs。该 cutover
不包含 nested epoll；首版对 epoll target 的 `EPOLL_CTL_ADD` 返回 `EINVAL` 并记录
notice。future nesting 必须经独立 target review 与 cutover 才能把该拒绝改成成功。

## Target Invariants

### 目标闭合条件

以下条件同时成立后，文档协议才可以进入实现计划：

1. source readiness 与 registry 容器仍由具体 source state 单独拥有；registry entry
   只提供非拥有通知路由，不决定 consumer 是否仍接受 callback。
2. subscribe 能在一个 source-side 事务中安装 registration 并取得 current
   readiness，即使当前 ready 也保留 registration。
3. source callback 发生在 source lock 外，且不持有 task/runqueue completion
   capability。
4. poll/select 的单轮 task wait 继续由 `Latch` 表达，并由 wait-round owner 统一
   retire wait identity 与 callback acceptance；不要求同步进入每个 source unlink。
5. epoll policy、watch generation 与 ready protocol 只由 `Epoll` / `EpollWatch`
   拥有，不进入 source。
6. `task::files` 提供 non-owning opened-description identity/liveness capability；lifecycle
   单调推进 `Unpublished -> Live(n) -> Retired`，首次最终 `1 -> 0` 后旧 capability 永久
   不可复活。watch 不长期强持 target，ctl/harvest 只取得短生命周期 live lease。
7. ready queue 只保存候选；交付前重新检查 target 实际 predicate，并在 event commit 前
   最终验证 opened-description liveness。
8. 每个 `Epoll` 的 ADD/MOD/DEL、teardown 与 harvest 由一个
   sleepable operation mutex 串行化；source notification 不获取该 mutex，只发布可合并、
   可延后的 sticky recheck obligation。逻辑失效必须立即 fail closed，但不要求 callback、
   ready-queue insertion、source unlink 或 pruning 同步完成。
9. final close 不进入 epoll、不获取 operation mutex，也不要求唤醒 `epoll_wait()`；
   retirement 先于 commit 时必须 fail closed，commit 先发生时不追溯撤回结果。
10. epoll file 自身实现相同的 pollable-source contract；首版只承诺 `poll(epfd)` 与
   `epoll_wait()` 的自身等待，不接受 epoll fd 作为 watch target。
11. source、watch、epoll 与 opened description 之间没有强引用环。
12. Linux UAPI 表达留在 ABI/syscall 边界，内部 owner 不保存 Linux-shaped 状态。
13. poll/select 与 epoll 统一 source-facing subscription protocol；统一不要求具体 source
    共享 registry container、锁、容量、分配或 notification handoff 实现，也不能用单一
    通用 wrapper 的代码复用程度替代 owner-local correctness 证明。

任何一项未闭合时，只能称为 Draft 或受控 probe，不能声明 epoll 核心语义可实现。
当前 Keter 已在公共 R0 target 中 neutralize；future implementation resolution 仍需用 live
source probe 证明 terminal retirement 与 transient lease，但不要求提前伪造具体 Rust
encoding，也不自动获得执行授权。

### 非目标

- 不把 `Latch`、`Event` 或 wait core 扩展成 readiness subscription registry。
- 不引入完整 Linux waitqueue、RCU 或 eventpoll 数据结构兼容层。
- 不要求 source 理解 poll/select/epoll consumer policy。
- 不要求所有 source 使用一个通用 registry/container/lock；source-local storage 与 handoff
  只要实现同一 subscription contract，就不是并列协议。
- 不把 cleanup、Drop 或 lazy pruning 作为正确性唯一支柱。
- 不让 callback、ready candidate、diagnostic id 或 cached mask 成为 readiness 真相。
- 不要求 final close 同步进入 epoll、唤醒 epoll waiter 或调用 dynamic observer registry。
- 不让 watch 用长期强 `Arc<File>` / opened-description borrow 替代 semantic liveness。
- 不通过降低 ET/ONESHOT、close/dup 或并发 ctl 语义来绕过协议问题。
- 不把未经 cycle/depth 与并发 admission 验证的 nested epoll 当成首版隐含能力。

### 状态所有权

| 状态或事实 | 唯一 owner |
| --- | --- |
| target 当前 readiness predicate | target source state |
| source 当前 observer routing entries 与物理 pruning | target source registry |
| 当前 task wait identity、completion、timeout/signal/force 竞争 | `sched::wait` |
| 一轮 iomux wait 的 Latch、observer lifetime 与 callback acceptance | `IomuxWaitRound` |
| epoll interest、user data、LT/ET/ONESHOT | `EpollWatch` |
| watch active/generation 与 callback 接受边界 | `EpollWatch` / `Epoll` protocol |
| watch pending recheck obligation、ready candidate membership、harvest 与 requeue | `Epoll` / `EpollWatch` ready protocol |
| epoll file 是否可能 readable | `Epoll` ready predicate |
| ADD/MOD/DEL、instance teardown 与 harvest 的 operation 串行化 | 每个 `Epoll` 的 sleepable operation mutex；行为状态仍由 `Epoll` / `EpollWatch` 拥有 |
| opened description identity、terminal liveness、dup/fork sharing 与 final published release | `task::files` |

`PollObserver` 与 readiness registration 只携带 callback route，不拥有上述状态。

### 协议统一与 source-local 实现边界

统一的长期合同只包含：

- source-neutral 的 readiness snapshot 与 persistent-subscribe capability/result；
- `PollObserver` 的 no-return、recheck-only、late-safe callback 能力；
- subscribe publication 与 current readiness snapshot 的 source-lock 内事务；
- consumer-owned callback acceptance/retirement 与 source-owned 有界 route cleanup；
- source lock 外的 notification handoff，以及 callback/queue payload 不是 readiness truth。

以下形状继续由具体 source owner 决定：

- readiness predicate、interest filtering 与 source 状态转换；
- registry entry 的 container、容量、预留/分配和 pruning 算法；
- source lock 类型、IRQ/noirq 约束与 guard-out batch/handoff；
- 是否使用共享 helper。共享 helper 不得取得 source 私有锁、保存 readiness/cache truth、
  暴露 feature-specific downcast，或要求困难 source 为代码复用改变执行上下文合同。

因此，eventfd、pipe、fanotify、timerfd、TTY 等 source 可以保留不同的 owner-local registry
实现，但不能保留不同的 consumer protocol：source-facing API 不区分 poll/select/epoll，
不保存 `OneShot/Persistent` policy，也不把 `LatchTrigger` 或 epoll private type 泄漏回
registry。是否能抽出单一通用 registry 是 implementation preference，不是 target
guarantee 或统一工作的 ROI 成功条件。

### 身份与能力模型

#### `PollObserver`

`PollObserver` 是 consumer 交给 source 的窄 callback capability：

- callback no-return / fail-closed；
- callback 只要求 consumer recheck，不传递稳定 readiness；
- callback 允许重复、合并、并发和晚到；
- callback 调用次数不是 ABI event 计数；notification 可以先只发布 sticky pending/dirty，
  再由 task context 合并处理；
- source 不根据 callback 结果修改 readiness、重试或补偿；
- observer 不能访问 source 私有锁、registry 或 predicate storage。

具体 Rust encoding 尚未接受；trait object、函数指针与 opaque context 都不能把
任意闭包或 feature-specific downcast 变成公共逃生口。

#### `PollSubscription`

`PollSubscription` 只作为 readiness registration relationship 的工作名。target 不要求
它一定编码为 consumer-side handle，也不要求提供同步 source unlink、callback drain 或
通用 `cancel()`。source registry entry 是非拥有 routing record；consumer round/watch
state 才决定 callback 是否还能产生行为。

consumer retirement 后，已经选择或飞行中的 callback 可以晚到，但必须内存安全并
fail closed。source 可以 eager unlink，也可以惰性 prune stale route；两者只影响资源
卫生，不能改变用户可见正确性。具体引用表示、validity encoding、容器与 prune 算法留给
future implementation resolution。

#### `IomuxWaitRound`

`IomuxWaitRound` 是 syscall-local linear owner。它持有一个 `Latch` 与共享
`LatchPollObserver`，并拥有本轮 callback acceptance；本体不可跨 task 转移，
且每次 begin 必须 exactly-once finish/retire。

`LatchPollObserver` 可以持有本轮 `LatchTrigger`，但 source-facing API 不得暴露
该类型或 task identity。

#### watch identity 与 opened-description liveness

Linux-visible watch key 语义必须同时包含用户提交的 fd number 与对应的
opened-file-description identity。dup/fork 共享相同 opened description；fd reuse
不能命中已经失效的旧 watch。

具体 capability 由 `task::files` 提供。它必须 opaque、non-owning，只允许比较
opened-description identity、尝试取得本次 operation 使用的短生命周期 live target
lease，并在 commit 前验证 terminal liveness；不得暴露 `ProcFile`、published-ref count、
fd-table lock 或行为型逃生口。不得把 inode、path、raw fd、纯诊断指针、`Weak::upgrade()`
成功或底层 `File` 内存存活单独当成 semantic liveness。

opened-description lifecycle 单调推进：

```text
Unpublished -> Live(n) -> Retired
```

首次最终 `1 -> 0` 线性化为 `Retired`，之后任何旧 `FileDesc` clone、reservation 或延迟
publication 都不得使该 identity 复活。具体 lifecycle word、generation、assertion 与
publication API 由 future implementation resolution 选择，但 `description_refs` owner
不能与另一个 epoll-side alive bit 形成双重真相。

#### watch generation

watch generation 是 epoll protocol state，用于让已失效 watch 的 callback、ready
entry 和 liveness-retired candidate fail closed。它不是 target readiness generation，
也不能成为 source readiness 或 registry storage 的第二份真相。

### Source Subscription 线性化

subscribe 必须在同一个 source lock 临界区内完成：

```text
validate interests / reserve entry resources
publish source-local registration
snapshot current readiness
return current readiness + established observer route
```

即使 current readiness 非空，observer route 也保持已安装。任何 allocation、identity
分配或 observer route 构造失败都必须在 publish 前失败，或有不暴露半 entry 的
rollback；不能返回缺少可用 callback route 的成功结果。

snapshot-only source 与支持长期 subscription 的 source 必须可区分。regular file
可以对 poll snapshot 始终 ready，但 `EPOLL_CTL_ADD` 仍需根据 persistent-subscribe
capability 返回 Linux-compatible `EPERM`。

### Source Notification 线性化

任何可能改变已订阅 poll predicate 的状态转换必须遵循：

```text
source lock 内：
    更新 readiness truth
    选择相关 observer route candidates
source lock 外：
    callback-safe 地发布 sticky recheck obligation
    可立即 kick，也可把 ready processing 延后到 task context
```

source 不持锁进入 observer、epoll、wait core 或 task placement。observer capability
的 lock-after-callback handoff 必须在 consumer retirement race 下保持内存安全；
callback 晚到只能成为 consumer-side stale hint。

notification 的逻辑效果是发布一个可观察的 recheck obligation，而不是立即完成
observer callback 或 ready-queue insertion。source 可以把多个状态变化合并成一个
sticky pending/dirty 状态，并由 callback-safe kick 或 deferred task-context processing
推动 consumer。进入无 candidate 的 wait 前，consumer 必须与该 pending 状态完成一次
无丢 wake 的 handoff；不要求逐 callback 证明一一对应的 snapshot 或用户 event。
source notification path 在返回前必须完成 pending publication 或等价的持久 handoff，
但不必等待 observer callback、operation mutex、ready-queue insertion 或 target recheck。

observer route 不因 callback 自动删除。source 不保存
`OneShot/Persistent` policy，也不通过 callback 返回值决定 registration 生命周期。

### Poll / Select Wait Round

一轮阻塞 iomux 的目标控制流是：

```text
snapshot scan
begin IomuxWaitRound / Latch
subscribe all sources with the round observer
if current-ready or error:
    retire observer acceptance
    cancel current Latch round + finish Latch
    final snapshot
else:
    schedule Latch
    retire observer acceptance
    finish Latch
    final snapshot
```

timeout、signal、force、source hint 和 register failure 的竞争仍由 wait core 与共享
iomux outcome mapping 处理。retirement 顺序或 callback 晚到不能省略 final snapshot。

迁移期间的 `PollRequest::register(&LatchTrigger)` 必须标注为临时桥，并在所有
pollable sources 与 ppoll/pselect 完成 subscription 迁移后删除；不得新增第二套
长期 consumer。

### Epoll Watch Publication

ADD 必须在目标 `Epoll` 的 private、sleepable operation mutex 下把以下动作组织成一个
可回滚的 operation：

1. 按 `(opened description identity, fd key)` 检查或预留 watch identity；
2. 构造尚未对普通 lookup 可见的 watch；
3. 建立 source observer route 并取得 current readiness；
4. 合并 publication 窗口内的 pending recheck obligation；
5. 发布 active watch，并在需要时进入 ready queue。

source notification 不能因为 map 尚未发布而丢失；并发 ctl 也不能同时发布重复 watch。
operation mutex 在 syscall adapter 完成 UAPI copyin、fd 解析与基础校验后取得，并持有到
watch commit 或完整 rollback。并发 ADD 因而只能有一个成功；失败方必须在释放 mutex 前
使未发布 watch/observer relationship 永久失效，不能要求 callback 判断半发布状态。
mutex 可以覆盖 target subscribe，但 source notification 永不获取它，source lock 也不得
反向进入 epoll operation。source route 是否 eager unlink 不属于该 operation 的正确性条件。

### Ready Queue 与 Harvest

ready queue membership 是 epoll owner 的 coalescing state，不是 target readiness。
source notification 只按 watch identity/generation 发布 sticky pending/dirty recheck
obligation；它可以延后或合并，且不要求立即 enqueue。持有 operation mutex 的 harvest
负责吸收 pending、重新 snapshot target，并按当前 policy 过滤事件。

- LT 成功交付后，若实际 predicate 仍成立，应按 owner protocol 重新成为 candidate；
- ET 不因仍处于同一 ready level 而自动 requeue；
- ONESHOT 只在成功交付后 disable，直到有效 MOD rearm；
- stale watch、失效 generation 与 final-closed target 不得交付旧 user data；
- copyout partial-progress 与 failure recovery 属于 syscall ABI / implementation gate，
  不由 callback accounting 提前冻结为 ready protocol invariant。

多个 notification 可以折叠成一个 pending obligation。harvest 不需要逐 callback 证明
“本轮 snapshot 覆盖或下一轮保留”；它只需在宣布无可交付 candidate 并准备等待前，
完成 pending handoff，使已发布 obligation 被本轮吸收，或保持可见并触发后续重检。
具体 atomic、queue、worker、epoch 或状态枚举留给 implementation resolution。

### Epoll File Readiness

`EpollFile` 的 readable predicate 由 `Epoll` ready protocol 推导，并通过普通
subscription contract 暴露。`epoll_wait()` 在 harvest 无可交付事件时只建立对
epoll file 自身的一轮 subscription；它不能重新遍历并临时订阅全部 target。

epoll ready predicate 发生可能的 false/true 转换时，必须遵循普通 source 的锁外
callback 规则。该普通 pollability 为 future nesting 保留组合路径，但首版
`EPOLL_CTL_ADD` 必须在 publication 前识别 epoll target、返回 `EINVAL` 并记录 notice；
不能让对象模型的自然可组合性绕过尚不存在的 cycle/depth 与并发 graph admission。

### Ctl、Retirement 与 Final Release

ADD/MOD/DEL、epoll instance teardown 与 harvest 必须由同一 per-`Epoll` operation
mutex 串行化。该 mutex 是 operation permit，不是
watch-table、generation、ready membership 或 lifecycle truth。source readiness
notification 只能访问 callback-safe pending/validity state，不得获取该 mutex。

DEL 与 MOD replacement 至少遵循：

1. 在 epoll owner 边界先失效旧 watch identity/generation；
2. 让旧 ready entry 后续只能被识别并丢弃；
3. 释放 consumer 对旧 observer relationship 的行为接受；
4. 已离开 source lock 的 callback 到达时 fail closed，source route 随后按有界规则清理。

`task::files` 单独拥有 final published-fd release。`Live(1) -> Retired` 不进入 epoll、
不获取 epoll operation mutex，也不调用 epoll observer；现有
`FileDescOps::final_release` 保持创建时固定的静态 hook，不承担动态 watch cleanup。

持 operation mutex 的 ADD、MOD 与 harvest 必须使用 non-owning liveness capability：

1. 尝试取得只覆盖当前 operation 的 live target lease；
2. 在 lease 内执行 subscribe/snapshot 或 watch mutation；
3. 在 watch publication 或 event commit 前最终验证仍为同一 `Live` identity；
4. 若 retirement 先发生，丢弃 stale result、失效 callback acceptance，并惰性移除 watch；
5. 若 commit 先发生，结果在线性化顺序上先于 close，不要求 copyout 前追溯撤回。

ctl/harvest 可以在 operation 开始或处理 candidate 时扫描 retired watches。首版允许用
简单扫描换取证明清晰；若 epoll 长期不再执行 operation，stale watch metadata 可以保留到
teardown，但它不得强持 target/source，且不能继续交付或突破明确资源上界。dynamic
final-release observer registry 不是 target，只有 [EPOLL-DRAFT-K2](./tracking-issues.md) 的
重新打开条件成立时才重新评审。

### 锁序与执行上下文

以下负约束当前已经接受：

- source lock 下不调用 observer；
- epoll ready/state lock 下不调用 target snapshot、subscribe 或用户
  copyout；
- `task::files` opened-description lifecycle 不获取 epoll/watch lock，也不调用 epoll；
- source readiness notification 不获取 epoll operation mutex；
- operation mutex 只在 IRQ enabled、preemption allowed 的 task context 取得；可以覆盖
  target subscribe/snapshot 与 harvest 内部决策，但不跨 `epoll_wait()` task sleep 或
  source callback 持有；用户 copyout 是否纳入 operation 由 future ABI resolution 决定；
- 允许单向 `Epoll operation mutex -> target subscribe/snapshot owner lock`，禁止
  source、`task::files` lifecycle owner 或 callback-safe pending lock 反向获取 operation mutex；
- source 与 epoll callback 都不直接操作 task sched state 或 runqueue；
- IRQ-off/noirq source callback handoff 不得依赖不可用的动态分配或睡眠锁。

operation serialization、callback-safe pending handoff 与 non-owning terminal liveness
的 target 边界已经闭合；具体 encoding 与 live-source probe 留给 future implementation
resolution，cutover 前不得写入 current contract。

### 引用与 Teardown

- `Epoll` 强持有已发布 watch metadata，直到显式或惰性移除；terminal liveness 可以先使
  该 metadata 逻辑不可交付；
- watch 只弱持有 owner `Epoll`；
- source registry 只持有不拥有 consumer lifetime 的 observer route；
- watch 只长期持有 opaque、non-owning 的 opened-description identity/liveness capability；
  不长期强持 `File` / opened description，ctl/harvest 的 live target lease 不能逃出当前
  operation；
- final retirement 由 capability validation 使 watch fail closed；DEL、MOD replacement
  和 epoll close 由 epoll owner 显式失效对应 consumer relationship；
- lazy pruning 可以回收 stale routing entries，但不能决定 user-visible correctness，
  且必须服从明确资源上界。

teardown 必须先撤销发布状态和 callback acceptance，再执行可能暴露 bug 的轻量
`assert!`；不能在 logical retirement 前 panic 并遗留 active callback path。

### ABI 边界

`epoll_event`、`EPOLL_*`、ctl opcode 和 syscall timeout/sigmask layout 只存在于
`anemone_abi` 与 syscall adapter。内部 `PollEvent` 与 epoll policy type 不依赖 Linux
bit layout。

UAPI parser 位于 `fs::api::iomux::epoll*` 或保持同一依赖方向的现有 `fs::api` 边界；
`fs::epoll` core 只暴露 typed operations 与 anonymous file backend，不得读写用户指针，
也不得把 Linux struct 嵌入 watch。

### 禁止退化项

- source 长期保存 `LatchTrigger`、`Task`、`WakeToken` 或 runqueue capability；
- poll/select 与 epoll 永久保留两套 source registry；
- callback 返回值驱动 source registration 生命周期；
- source 或 `task::files` downcast epoll private type；
- final close 同步扫描 epoll、获取 epoll operation mutex 或默认依赖 dynamic observer registry；
- epoll 持有 target source 私有锁、容器或 predicate storage；
- 只按 raw fd、inode 或 path 标识 watch；
- 用强引用环替代显式 final-close/retirement protocol；
- 用 target 内存仍存活或 `Weak::upgrade()` 成功替代 opened-description terminal liveness；
- 用全表重扫 busy polling 替代 persistent subscription；
- 省略 target final recheck，直接把 callback payload 复制给用户；
- 为通过测试而把 ET、ONESHOT 或 close/dup race 降级成未记录弱语义；future nesting
  在独立 cutover 前只能保持显式拒绝，不能作为半支持路径泄漏。

## RFC-local Invariants

以下规则只约束本 RFC 的迁移、review 与验收，不自动成为长期 current contract：

- 三个 cutover unit 必须保持 effective / target 分离；任何 probe、partial integration
  或 R0 acceptance 都不能提前改变 `docs/src/contracts/`。
- `PollRequest::register(&LatchTrigger)` 是 `SUBSCRIPTION-CUTOVER` 前的 current
  effective path，不是已经被接受的长期 target；未来迁移桥必须带删除条件，且不得
  成为 poll/select 与 epoll 的永久并列 registry。
- [实施计划](./implementation.md) 的 Stage 0 必须先执行 proof-first vertical slice：至少覆盖
  普通非睡眠 source 路径与 noirq/fixed-capacity source 路径，并在全量 cutover 前审计 TTY
  一类预分配、可重入 dirty handoff。Stage 0 已按 0A terminal liveness、0B observer/pipe、
  0C timerfd noirq、0D TTY/closure 解析顺序、checkpoint write subset、定向验证、review、
  恢复和代码去留；本条继续保护“困难 source 不能被 wrapper-only 估算掩盖”的停止边界。
- 如果代表性 source 只能依赖 feature-specific downcast、同步 callback drain、第二份
  readiness/liveness truth，或要求 noirq/预分配 source 违反其上下文边界，必须在全量
  source 迁移前停止并进入 Target Renegotiation Gate，不能继续用 per-source escape hatch
  把名义上的统一协议拼出来。
- target owner、ABI、可见语义或 acceptance boundary 若因实现证据需要改变，必须进入
  Target Renegotiation Gate，不能在 future implementation stage 中静默降低语义。
- Stage 0 已在 document review 前解析为 Ready；R0 acceptance、transaction 与开发者启动授权
  已于 2026-07-26 完成，Stage 0 现为 Active。checkpoint authorization 仍逐项生效，本轮只覆盖
  0A、0B，不能自动进入 0C、0D 或后续 Stage。

### 文档层完成标准

R0 target 已由以下文档层证据闭合：

- 所有 Keter tracking issues 已 neutralize，或转成具有受保护 target、解析触发点、停止条件
  与回写路径的明确 stage gate；当前所有 Keter 已 neutralize，且
  [实施计划](./implementation.md) 已解析 Stage 0；R0 acceptance 与 Stage 0 activation 由
  [事务日志](../../devlog/transactions/2026-07-26-epoll.md) 独立记录，不授权后续 checkpoint / Stage；
- registration publication、consumer retirement、watch publication、ready harvest 与
  opened-description terminal retirement 的线性化点可组成无丢 wake、无旧数据交付的
  完整状态机；
- lock order、IRQ/noirq handoff、资源上界和 allocation failure 已形成可供未来
  implementation resolution 使用的停止边界；
- 协议统一与实现复用已经分离：公共合同不预设单一 registry，future resolution 又有
  普通 source、noirq/fixed-capacity source 与预分配 handoff 的代表性证明/审计边界；
- poll/select 迁移不会回退既有 latch final-scan/outcome contract；
- 首版对 epoll target 保持稳定 fail-closed，文档和实现都不宣称 nested epoll 已进入
  `EPOLL-CUTOVER`；
- 剩余不确定性已经能归入 future probe、accepted limitation 或公开 tracking issue，
  而不是隐藏在类型实现中。
