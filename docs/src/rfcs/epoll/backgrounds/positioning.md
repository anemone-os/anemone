# Epoll RFC 前设计定位共识

**状态：** Historical Positioning Input / 已退出 target authority
**最后更新：** 2026-07-26
**范围：** epoll 与通用 I/O readiness subscription 的对象、owner 和模块边界
**当前 RFC：** [RFC-20260726-epoll](../index.md)

## 文档目的

本文保留 Anemone 展开 epoll 公共 RFC 前形成的设计定位共识。

本文不是 RFC canonical target，不是 current contract，不是 `invariants.md`，也不是
implementation plan。当前公共 Draft target 已展开到 [RFC 入口](../index.md)、
[不变量需求](../invariants.md) 和 [Tracking Issues](../tracking-issues.md)，实施路线由
[实施计划](../implementation.md) 单独拥有；当前 effective
baseline 只由仓库 `docs/src/contracts/` 拥有。若本文与 target proposal 或 current
contract 冲突，分别以后两者为准。

本文只保存形成公共 Draft 的定位背景、被拒绝路线和仍有参考价值的待讨论问题。具体 Rust
类型编码、锁实现、阶段顺序、write set 和验证 gate 已在开发者授权后进入
[实施计划](../implementation.md)；不能把本文继续当作并列计划层。

2026-07-26 的后续 Draft review 已接受六项覆盖本文早期候选的边界：每个 `Epoll` 的
ctl、teardown 与 harvest 使用一个 sleepable operation mutex；source notification 只需
callback-safe 地发布可合并、可延后的 sticky recheck obligation；Linux UAPI parser 位于
`fs::api` 而不是 epoll core；首版保留 `EpollFile` 普通 pollability，但显式拒绝 epoll
target，把 nested epoll 留给 future target；readiness registration 不预设通用同步
cancel，consumer-owned validity/lifetime 决定旧 hint 是否还能产生行为，source registry
只保存非拥有 route 并负责有界清理；opened description 通过 `task::files` 提供的
non-owning、terminal identity/liveness capability 被 ctl/harvest 验证，final close 不同步
进入 epoll。以下历史讨论若与这些决定冲突，以
[RFC 入口](../index.md) 和 [不变量需求](../invariants.md) 为准。

## 当前基线与问题

### 当前 latch poll 的语义

现有 `sched::Latch` 是建立在 wait core 上的一轮 OR wait：一个 waiter-owned `Latch` 对应当前 task 的一个 wait identity，同一轮可以派生多个 `LatchTrigger`，任意第一发有效 trigger 完成本轮等待，旧 trigger 在本轮 finish 后只能 stale / retired，不能影响下一轮等待。

现有 `poll` / `select` 采用以下结构：

```text
snapshot scan
begin latch
register scan
schedule
finish
final snapshot scan
```

source 在自己的状态锁下检查 readiness。当前已经 ready 时返回 `Ready(events)`，不保存 `LatchTrigger`；未 ready 且支持等待时保存本轮 trigger 并返回 `Armed`。状态变化时，source 在锁内更新 predicate 并 detach 对应 trigger，释放 source lock 后再触发。

该协议适合 `poll` / `select` 的 syscall-local wait round，但不能直接表达 epoll 的长期 interest relationship。

### epoll 引出的缺口

epoll 至少需要同时表达：

- interest relationship 跨越多次 `epoll_wait()` 持续存在；
- `EPOLL_CTL_ADD` 安装持久订阅时，同时取得注册点的当前 readiness；
- 即使注册时已经 ready，后续 subscription 仍然保留；
- source notification 只表示需要重新检查，不是稳定 readiness 事实；
- LT、ET、`EPOLLONESHOT`、ready queue 去重和 user data 属于 epoll owner，而不是 source；
- `EPOLL_CTL_DEL`、`MOD`、target final close 和并发 callback 必须有明确生命周期协议。

当前 `Ready / Armed / Unsupported` 的互斥结果不能表示“已经持久订阅，并且当前 ready”。当前 source 保存的 `LatchTrigger` 又绑定某个 task 的单次 wait identity，不能作为长期 epoll interest handle。

## 已形成的总体定位

### 不新增 scheduler 级 PollNotifier

epoll 不需要一个位于 `sched`、与 `Event` / `Latch` 同级的新调度原语。

source 到 epoll 的长期关系不是 task wait。该关系不应携带 `Task`、`WakeToken`、wait identity 或 runqueue placement 能力。真正的 task sleep 仍由 `sched::wait` 和 `sched::Latch` 负责。

`Event` 也不作为 epoll source subscription 的基础。`Event` 的 listener 是 task-bound
wait listener；epoll 需要的是 source 与非 task consumer 之间可独立退休的长期通知
关系。把所有 source 通过临时 `Event` 转发会重新混淆 source owner、epoll owner 和
task wait round。

### 以 PollSubscription 为核心，而不是 PollNotifier

`PollSubscription` 可以继续作为 registration relationship 的工作名，而不是一个独立的
`PollNotifier` owner；当前不把它预设为具体 consumer-side cancellation handle。

`Notifier` 容易被理解成另一个拥有锁、observer registry 或 readiness 状态的中心对象，从而在 source state 旁边制造第二个 owner。subscription registry 应继续由具体 source state 拥有，使“安装 registration 并取得当前 readiness”能够在同一个 source lock 临界区内线性化。

当前倾向的通用角色是：

- `PollObserver`：consumer 提供给 source 的窄 callback capability；
- readiness registration：source 保存的非拥有 notification route；
- consumer validity/lifetime：由 wait round 或 watch owner 持有的 callback 接受边界；
- `PollEvent`：一次 readiness snapshot，或 callback 携带的过滤 / recheck hint。

`notify` 是 source 对 observer 执行的一次动作，不需要成为独立的公共 owner 类型。

### poll waiter 收敛为 wait-round-scoped subscription

现有 source-facing poll waiter / `LatchTrigger` 注册路径不作为长期并列协议保留。

从语义上看，`poll` / `select` 的每轮等待可以安装普通 observer routes，并使用一个由
`LatchTrigger` 适配出来的 `PollObserver`。本轮任意 callback 第一次有效触发 Latch；
该轮结束时，wait-round owner retire 本轮 wait identity 与 callback acceptance，不要求
同步进入所有 source 删除 routing entry。

一次性语义属于 observer 和 wait round，不属于 source subscription：

```text
consumer callback acceptance
├── 由 IomuxWaitRound 拥有：本轮结束时 retire
└── 由 EpollWatch 拥有：DEL / close 时失效，MOD 按当前 watch protocol 更新
```

source 不应保存 `OneShot / Persistent` consumer policy，也不应根据 callback 的返回值
决定是否删除 registration。route 可以在 consumer retirement 后暂时留在 registry，
但旧 callback 只能通过 consumer generation / active / terminal-liveness boundary fail closed；source 的
eager unlink 或惰性 pruning 只服务有界资源卫生。

代码命名中不使用 `OneShotPollSubscription`。poll wait round 的 first-completion-wins 与 `EPOLLONESHOT` 是不同语义，不能共享 `oneshot` 名称。

### 协议统一不等于实现容器统一

poll/select 与 epoll 应统一 source-facing subscription protocol，但统一不要求所有 source
套用同一个 `PollRegistry`、container、锁或 notification batch。公共面只负责：

- source-neutral observer capability；
- subscribe publication + current readiness snapshot；
- consumer retirement 后 late callback fail closed；
- source-local stale route 的有界 cleanup 合同。

predicate、registry storage/capacity、IRQ/noirq 约束、allocation policy 与 guard-out handoff
仍由具体 source owner 决定。共享 helper 只是 implementation preference；如果它需要访问
source 私有锁、缓存 readiness、提供 feature-specific downcast，或迫使 timerfd/TTY 放弃
自身执行上下文边界，它就已经越过了通用协议层。

### epoll 是 readiness owner 之上的组合 owner

epoll 不拥有 target 当前是否 readable / writable 的真相。target source state 仍是 readiness 的唯一 owner。

epoll 拥有的是：

- interest set；
- user data；
- LT / ET / `EPOLLONESHOT` policy；
- watch 是否 active 及 callback generation；
- ready queue membership / coalescing；
- epoll file 自身是否可能 readable；
- `epoll_wait()` 的 harvest 和 recheck 流程。

epoll ready queue 是候选 / 调度提示，不是 target readiness cache。事件向用户交付前必须重新检查 target source 的实际 predicate。

## 工程 ROI 结论

当前 pollable source 已经普遍具备“source 锁内检查/注册，锁内更新 predicate 与选择
trigger，锁外触发，wake 后重新 snapshot”的共同骨架。因而在 observer/route 核心协议
稳定后，snapshot-only file 和普通 task-context source 的大部分迁移预计是 typed API、
entry 与 callback handoff 的机械替换。

但 persistent subscription 相对当前一次性 `LatchTrigger` 有三项真实的生命周期变化：

- 注册点已经 ready 时仍必须安装 route；
- notification 后 route 不自动删除，consumer policy 不进入 source；
- consumer retirement 后允许已选择 callback 晚到，但必须内存安全、fail closed 且物理
  route 最终受资源上界约束。

证明负担因此集中在公共 observer/retirement 协议和困难 source，而不与 diff 体量成正比。
普通 eventfd/pipe/fanotify 类路径主要复用现有锁内检查、锁外 trigger 形状；timerfd 还要
保留 noirq、fixed-capacity 与锁外 drop/trigger 约束；TTY 还要保留预分配双容器和
handoff-active/dirty 的可重入通知协议。后两类不能被“只是换 wrapper”的估算掩盖。

ROI 需要把两类成本分开：opened-description liveness、watch generation、ready harvest、
LT/ET/ONESHOT 与 DEL/MOD/close 失效是正确 epoll 无论是否统一 poll 都必须承担的成本；
统一额外承担的是现有 source 与 `IomuxWaitRound` 的一次迁移。若不统一，只省去这次迁移，
却仍要支付 epoll 核心证明并长期维护两套 source registration/wake/cleanup 协议。因此当前
接受的工程方向仍是统一协议，但不追求统一 owner-local storage。

[实施计划](../implementation.md) 的 Stage 0 已先安排 proof-first vertical slice，再授权全量
`SUBSCRIPTION-CUTOVER`：普通 task-context 使用 pipe，noirq/fixed-capacity 使用 timerfd，
TTY 的预分配 dirty handoff 作为 adversarial source。本文仍只固定假设、风险类别与停止方向；
具体 write set、验证 floor、失败信号和代码去留以实施计划为准。

## 模块关系

当前接受的总体模块关系是：

```text
                           用户 syscall ABI
                                  |
                                  v
                        fs::api::iomux::epoll*
                                  |
                                  v
                +--------------------------------+
                | fs::epoll                     |
                | Epoll + EpollWatch + ready queue|
                +--------------------------------+
                         | subscribe_poll()
                         v
+---------------------------------------------------------+
| fs::iomux::subscription                                 |
| PollEvent / PollObserver / PollSubscription              |
+---------------------------------------------------------+
             ^                              ^
             |                              |
  pipe/eventfd/timerfd/...          fs::iomux::wait
  source state + registry           IomuxWaitRound
             |                              |
             | on_poll_hint()               | wraps
             +----------------------> LatchPollObserver
                                            |
                                            v
                                      sched::Latch
                                            |
                                            v
                                      sched::wait
```

`task::files` 位于该图侧面，向 epoll 提供 opened-file-description identity 和 final published-fd release 生命周期能力，但不得依赖 `fs::epoll` 私有类型。

### `sched::wait`

`sched::wait` 继续拥有：

- task 当前 wait identity；
- completion / cancel / finish；
- timeout、signal、force 与 producer wake 的竞争；
- stale-safe runnable placement。

它不理解 poll source、subscription、epoll watch、interest 或 ready queue。

### `sched::latch`

`sched::latch` 继续只表示一个 task 的一轮 OR wait。

`Latch` 是 waiter-owned linear guard；`LatchTrigger` 是本轮 producer capability。它们不扩展成跨轮次 permit，也不成为 source 的长期接口。

### `fs::iomux::subscription`

该层定义与 consumer 类型无关的 readiness subscription 语义：

- `PollEvent`；
- `PollObserver`；
- source-local observer registration；
- subscribe 时的 current readiness result；
- consumer retirement、stale callback 与 route cleanup 的窄合同。

该层不应依赖 `Task`、`LatchTrigger` 或 epoll 私有类型。

### `fs::iomux::wait`

该层负责把通用 subscription 适配到当前 task 的一轮等待。

当前倾向用 `IomuxWaitRound` 表达 syscall-local owner。它持有：

- 一个 `Latch`；
- 一个共享的 `LatchPollObserver`；
- 本轮 callback acceptance lifetime；
- timeout / signal / final snapshot 的控制流。

`LatchPollObserver` 内部可以持有 `LatchTrigger`，但这一事实不能泄漏到 source-facing subscription API。

### pollable source modules

pipe、eventfd、timerfd、fanotify、未来 socket 等 source 各自拥有：

- source readiness predicate；
- 保护 predicate 与 subscription registry 的 source lock；
- source-local subscription entries；
- 状态变化时的 observer 筛选和 lock-after-callback handoff。

source 不理解 poll/select/epoll consumer 类型，不保存 `Task`，不调用 scheduler lifecycle，也不把 callback hint 当作 readiness truth。

### `fs::epoll`

`fs::epoll` 是 epoll 语义的唯一 owner，当前预计至少包含：

- `Epoll`：watch table、ready queue、自身 readiness subscription registry；
- `EpollWatch`：每个 interest relationship 的 policy 与生命周期；
- `EpollFile`：anonymous file / `FileOps` 外壳；
- core-facing typed operations；
- 后续独立 target 需要时的 nested-epoll graph / cycle validation。

`Epoll` 自身也应实现普通 pollable source 合同。首版让 `poll(epfd)` 与 `epoll_wait()`
等待自身 ready queue 复用同一套 subscription 机制，而不是建立 epoll 私有的
`LatchTrigger` 队列。该对象模型为外层 epoll 观察内层 epoll 保留自然组合路径，但首版
`EPOLL_CTL_ADD` 对 epoll target 显式返回 `EINVAL` 并记录 notice；future nesting 在
cycle/depth、并发 graph admission 与传播验证完成前不能意外生效。

### `task::files`

`task::files` 继续拥有 fd table、dup/fork sharing、opened-description status flags 和最后一个 published fd reference 的 release 语义。

epoll watch 持有 target `Arc<File>` 只能保证对象内存继续存在，不能替代 Linux 的
opened-file-description final-close 语义。`task::files` 应提供 opaque、non-owning 的
identity/liveness capability；opened description 从 `Unpublished -> Live(n) -> Retired`
单调推进，首次最终 `1 -> 0` 后旧 capability 不可复活。

watch 不长期强持 target。ctl/harvest 可以通过 capability 取得短生命周期 live target
lease，并在 watch/event commit 前最终验证 liveness。final close 不扫描 epoll、不获取
epoll mutex，也不要求主动唤醒 waiter；stale watch 由后续 operation 或 teardown 惰性移除。
dynamic final-release observer registry 只在 capability 或资源上界 probe 失败后重新讨论。

### ABI 与 syscall API

Linux `epoll_event`、`EPOLL_*` flag、ctl opcode 和 syscall argument layout 只存在于 `anemone_abi` 和 syscall/API 边界。

`fs::epoll` 内部使用 Anemone 语义类型表达 interest、mode、user data 和 watch identity。source subscription API 不接收 Linux UAPI struct，也不解释 `EPOLLET` / `EPOLLONESHOT`。

## 核心对象关系

### `PollObserver`

`PollObserver` 是 consumer 提供给 source 的窄 callback capability。

当前接受的语义要求：

- callback 是 no-return / fail-closed；
- callback 只表示需要重新检查；
- callback 可以被合并、重复或与 consumer retirement 并发；
- source 不能根据 callback 结果补偿、重试或改变 readiness；
- observer 不允许反向访问 source 私有锁或状态。

具体采用 trait object、函数指针加 opaque context，还是其它 no-std 形状仍未决定。

### `PollSubscription`

`PollSubscription` 当前只表示 source 与 observer 之间已经建立的 registration
relationship，不预设最终一定存在同名 Rust handle。

source registry membership 只说明 routing entry 仍物理存在；wait round 或 watch owner
的 validity/lifetime 才决定 callback 是否还能产生行为。普通 DEL、MOD、round finish 和
teardown 不要求通用同步 cancel 或 callback drain。具体引用形状、retirement encoding、
eager unlink 与 lazy pruning 由后续实现解析，但 cleanup 必须有界且不参与 correctness。

### `IomuxWaitRound`

`IomuxWaitRound` 是 poll/select/epoll_wait 的 syscall-local wait owner，不是 source object。

它把一个 `Latch` 与多个 source observer routes 组合成一轮 OR wait。任意 source hint
可以完成 Latch，但返回用户态前仍必须 retire 本轮 callback acceptance、finish wait
identity，并重新 snapshot 实际 predicate。

### `Epoll`

`Epoll` 持有所有 `EpollWatch` 的强引用和 ready queue。

它负责按稳定 watch identity / generation 去重 ready candidate，并在从“无可用 candidate”转为“可能有 candidate”时向观察 epoll file 的 subscriptions 发布 recheck hint。

每个 `Epoll` 拥有一个 sleepable operation mutex，串行该 instance 的 ctl、teardown 与
harvest。callback/IRQ handoff 不获取该 mutex，只发布可合并的 pending/dirty recheck
obligation；实际 ready-queue 处理可以在后续 task context 完成。

首版接受同一 instance 内 ctl 与 harvest 的串行化成本；没有实际瓶颈证据时，不把拆锁、
并行 harvest 或逐 callback 精确记账作为 target 能力。

`Epoll` 不应在持有自身状态锁时调用 target source snapshot、subscribe、observer
callback 或用户内存 copyout。

### `EpollWatch`

当前倾向让每个 `EpollWatch` 自己实现或持有一个 `PollObserver` capability，使 source callback 可以直接定位到 watch，而不是先定位 epoll instance 再扫描所有 watches。

一个 watch 至少关联：

- target opened file handle / identity；
- 用户提交时的 fd key；
- internal interest；
- user data；
- LT / ET / `EPOLLONESHOT` policy；
- source observer relationship；
- non-owning opened-description identity/liveness capability；
- ready queue membership；
- active / publishing / generation 类协议状态；
- 指向 owner `Epoll` 的弱引用。

`Epoll` 拥有 watch；watch 不反向拥有 `Epoll`；source registry 的 observer route 不拥有
consumer lifetime。具体引用表示留待实现解析，但不能形成 owner 生命周期环。

### `EpollFile`

`EpollFile` 是 VFS anonymous file 外壳，内部持有 `Arc<Epoll>`。

其 read/write/seek/ioctl 默认行为、pollability、final release 和 fdinfo 等属于后续 RFC/API 设计；但它不能成为另一套 epoll 状态 owner。

## 状态所有权共识

| 状态或事实 | 唯一 owner |
| --- | --- |
| target 当前是否 readable / writable / error / hangup | target source state |
| source 当前有哪些 observer routing entries | target source registry |
| 某个 task 当前 wait identity 与 completion | `sched::wait` |
| 一轮 iomux wait 持有哪些临时 subscriptions | `IomuxWaitRound` |
| epoll interest、user data、LT/ET/ONESHOT | `EpollWatch` |
| watch 是否 active、callback generation 是否有效 | `EpollWatch` / `Epoll` protocol |
| watch 是否已经位于 epoll ready queue | `Epoll` ready queue protocol |
| epoll file 当前是否可能 readable | `Epoll` ready queue predicate |
| opened-description identity、terminal liveness 与 final release | `task::files` |

`PollEvent` callback payload、epoll ready queue entry、diagnostic watch id 和 wait id 都不是 readiness 真相。

## 运行时交互共识

### source subscribe 线性化

持久 subscribe 必须在同一个 source lock 临界区内完成：

```text
安装 source-local registration
读取注册点的当前 readiness
返回 current readiness + established observer route
```

即使 current readiness 非空，registration 仍然保留。

若 subscribe 中途失败，不能返回成功或留下仍能产生行为的半发布 route。具体
allocation / publish / rollback 顺序仍待后续设计。

### source notification

任何可能改变 poll predicate 的 source 状态变化，应按以下方向组织：

```text
source lock 内：
    更新 source readiness truth
    找出相关 observer route candidates
释放 source lock
callback-safe 地发布 sticky recheck obligation
可立即 kick，也可把 ready processing 延后到 task context
```

source 不能持锁进入 epoll lock、wait core 或 task placement。notification 可以携带可能
相关的 `PollEvent` bits 用于过滤和诊断，但 consumer 必须重新 snapshot；notification
publication 必须完成逻辑 handoff，observer callback 与 ready-queue processing 不要求立即
执行。

### poll / select wait round

当前接受的目标形态是：

```text
snapshot scan
如果 ready：直接返回

begin IomuxWaitRound / Latch
创建共享 LatchPollObserver
逐 source subscribe_poll()
保留本轮 observer acceptance lifetime

如果 register scan 发现 current ready 或 error：
    retire 本轮 observer acceptance
    cancel current Latch round / finish Latch
    final snapshot
    返回

否则 schedule Latch

wake / timeout / signal 后：
    retire 本轮 observer acceptance
    finish Latch
    final snapshot
    映射 syscall outcome
```

现有 `PollRequest::register(&LatchTrigger)` 和 source-local `*_poll_triggers` 可以作为迁移桥存在，但不能成为与 `PollSubscription` 长期并列的第二套 source protocol；迁移桥必须有明确删除条件。

### `EPOLL_CTL_ADD`

`ADD` 需要同时闭合 watch publication 与 source observer-route publication：

```text
创建尚未公开的 EpollWatch
建立 source observer route 并取得 current readiness
把 watch 发布到 Epoll watch table
合并 subscribe 返回的 current readiness 与发布期间形成的 pending recheck obligation
必要时进入 ready queue
```

不能让 source notification 因 watch 尚未进入 epoll map 而被静默丢弃。publication 只需
保留一个 sticky pending/dirty obligation；多个 callback 可以合并，且不要求逐 callback
对应 snapshot、ready entry 或用户 event。具体 atomic、queue 或 deferred worker 形状留待
implementation resolution。

### source hint 到 epoll ready queue

典型路径是：

```text
SourceState
  -> callback-safe 地发布 watch pending/dirty
  -> 可选 kick / deferred task-context processing
  -> 持 Epoll operation mutex 吸收 pending 并按需 enqueue
  -> Epoll 发布自身 readiness hint
```

source callback 不能直接向用户生成 `epoll_event`。callback 次数也不是 ABI event 计数。
真正交付时，epoll harvest 必须在 operation mutex 下根据 watch 当前 policy 和 target 当前
snapshot 生成返回事件。

### `epoll_wait()`

`epoll_wait()` 不直接订阅所有 target source。长期 target subscription 已由 watches 持有。

当 ready queue 暂无可交付项时，`epoll_wait()` 只等待 epoll file 自身的 readiness：

```text
harvest / recheck ready candidates
若无可交付事件：
    通过 IomuxWaitRound 订阅 Epoll 自身 readiness
    schedule Latch
    wake 后重新 harvest
```

这样 `epoll_wait()` 与 `poll(epfd)` 可以使用同一个 epoll-file source contract；若未来
独立 target 接受 nested epoll，外层 epoll 观察内层 epoll 也应复用这条合同，而不是让
该能力在首版意外生效。

### `DEL`、`MOD`、close 与飞行中 callback

删除或替换 watch 不能假设 source callback 已经停止。至少需要：

- 在 epoll owner 边界内先使旧 watch identity / generation 失效；
- ready queue 中的旧 entry 后续只能被识别并丢弃；
- 已经离开 source lock 的 callback 到达时 fail closed；
- user-visible `DEL` / `MOD` 返回点与旧 callback 的可见边界明确。

source route 的物理 entry 可以随后 eager unlink 或惰性 pruning，但不参与上述正确性。
同一 `Epoll` 的 DEL、MOD、teardown 与 harvest 由 operation mutex 串行；callback 只访问
callback-safe pending/validity state。具体 generation 表示、pending handoff、`MOD` 更新
形状与 pruning 策略留给实现解析。

target final close 不获取 operation mutex，也不主动进入 epoll。ctl/harvest 在取得短期
target lease 后、watch/event commit 前最终验证 opened-description liveness；retirement
先发生则丢弃并惰性移除，commit 先发生则结果在线性化顺序上先于 close。

## 依赖与可见性共识

允许的依赖方向：

```text
fs::epoll -> fs::iomux::subscription
fs::iomux::wait -> fs::iomux::subscription + sched::latch
pollable source -> fs::iomux::subscription
sched::latch -> sched::wait
```

禁止的依赖和可见性：

- `sched` 不依赖 `fs::iomux` 或 `fs::epoll`；
- source 不依赖 `LatchTrigger`、`Task`、`Epoll` 或 `EpollWatch` 具体类型；
- `fs::iomux::subscription` 不依赖 epoll private state；
- `task::files` 不识别 epoll private type；
- syscall/API 层不直接操作 watch table、ready queue 或 source registry；
- epoll 不访问 source 私有锁或 predicate storage；
- source 不访问 epoll lock、ready queue 或 user data；
- Linux UAPI bits 不进入 source subscription contract。

## 当前接受的命名

以下名称当前用于表达已经接受的语义角色，具体 module path 和 Rust encoding 仍可在 rolling implementation resolution 中调整：

- `PollObserver`：接收 readiness recheck hint 的 consumer capability；
- `PollSubscription`：持久 registration relationship 的工作名，不预设具体 handle；
- `IomuxWaitRound`：一轮 syscall-local iomux wait owner；
- `LatchPollObserver`：把 `PollObserver` 适配到 `LatchTrigger` 的内部对象；
- `Epoll`：epoll instance 的状态 owner；
- `EpollWatch`：一个 target interest relationship；
- `EpollFile`：anonymous file 外壳。

当前不接受以下核心命名：

- `PollNotifier`：方向和 owner 不清，容易被误解成第二个 registry/state owner；
- `PollWake` / `PollWaker`：错误暗示 callback 直接唤醒 task；
- `PollListener`：容易与 `Event::Listener` 和 task waiter 混淆；
- `OneShotPollSubscription`：与 `EPOLLONESHOT` 语义冲突；
- `EpollSubscription`：会把 epoll 具体类型泄漏给通用 source。

## 已拒绝的方向

- 不把 `Latch` 改造成跨 wait round 的长期 notification permit。
- 不把 `Event` 扩展成 epoll source subscription registry。
- 不引入 scheduler 级 `PollNotifier`。
- 不让 source 同时长期维护 `LatchTrigger` queue 和 `PollSubscription` registry 两套协议。
- 不让 source 识别 poll/select/epoll consumer mode。
- 不把 `PollSubscription` 的 auto-remove policy 写成 source-side oneshot/persistent enum。
- 不把 callback payload 或 epoll ready queue 当作 target readiness truth。
- 不让 epoll 通过遍历所有 target 实现 busy polling。
- 不用 epoll 反向实现现有 `poll` / `select` syscall。
- 不在 `close()` syscall 中 downcast 或扫描 epoll private state。
- 不把 dynamic final-release observer registry 当成首版默认依赖。
- 不让 watch 长期强持 target，或用 target 内存存活替代 opened-description liveness。
- 不用强引用形成 source、watch、epoll 之间的生命周期环。
- 不在 source lock 内进入 observer callback、epoll lock 或 wait core。

## 候选约束

本节保留 promotion 前曾准备提升为 RFC invariant 的候选约束；当前 canonical target 以 [不变量需求](../invariants.md) 为准。

- target source state 是 readiness 的唯一真相源。
- source registry membership 只表示物理 routing entry，不决定 consumer callback acceptance。
- wait round 或 watch owner 的 validity/lifetime 是旧 hint 是否还能产生行为的唯一真相。
- notification 只提供 recheck hint，不承诺 callback 到达时 predicate 仍成立。
- subscribe 必须原子地安装 registration 并取得该线性化点的 current readiness。
- source callback 必须发生在 source lock 之外。
- poll/select/epoll_wait 的 task wait 继续由一轮 `Latch` 表达。
- poll/select 的 callback acceptance 由 `IomuxWaitRound` 统一 retire。
- epoll policy 只由 `Epoll` / `EpollWatch` 拥有，不进入 source。
- ready queue 只保存候选 watch identity；交付前重新检查 target readiness。
- 每个 `Epoll` 的 operation mutex 串行 ctl、teardown 与 harvest；source callback 不获取它。
- notification 可以合并、延后处理，但进入无 candidate 的 task wait 前必须闭合 sticky
  pending/dirty handoff，不能永久丢失已发布的 recheck obligation。
- callback 在 `DEL` / `MOD` 后必须通过 watch identity/generation fail closed；target final
  close 后必须通过 opened-description terminal liveness fail closed。
- `Epoll` 拥有 watches；watch 与 source route 都不反向拥有其 owner lifetime。
- epoll file 自身通过同一套 subscription contract 暴露 readability。
- opened-description identity/liveness 由 `task::files` 的 non-owning capability 提供；首次
  最终 `1 -> 0` 后 terminal，不允许旧 capability 因延迟 publication 复活。
- watch/event commit 前必须最终验证 liveness；physical watch cleanup 可以由后续
  ctl/harvest/teardown 惰性完成，final close 不同步进入 epoll。

## 待讨论的设计问题

以下问题尚未闭合，不应被本文其它段落误读成已经决定的实现合同。

- `FileOps` 最终采用 `poll_snapshot + subscribe_poll` 两个 hook，还是使用一个更窄的 poll-source ops facade。
- `PollObserver` 的具体 Rust 表示：trait object、函数指针加 opaque context，或其它 capability 形状。
- non-owning observer route 的具体表示，以及如何避免强引用环和任意行为型逃生口。
- source registry entry 的 eager remove 与 lazy pruning 边界、容量上界和失败信号。
- subscribe allocation failure、registration publish 和 rollback 的完整线性化顺序。
- notification batch 在 IRQ-off / noirq source 中的容量、分配和 handoff 约束。
- `IomuxWaitRound` retirement、late callback 与 Drop 安全网的最小实现形状。
- 当前 `PollRequest::snapshot/register` 的迁移顺序，以及旧 source `LatchTrigger` queue 的删除 gate。
- `EpollWatch` 的稳定 identity、generation 与最小 publishing/pending handoff encoding。
- watch key 是否严格按 Linux 的 opened-file-description identity + fd number 语义表达，以及 dup/fork/fd reuse 的精确行为。
- opened-description lifecycle 的最小 terminal encoding、non-owning identity/liveness
  capability 与 transient live target lease 形状；`FileDesc::clone()`、reservation commit
  和 publication path 必须证明不能在 final `1 -> 0` 后复活旧 identity。
- lazy retired-watch scan 的资源上界与 probe；只有 capability/资源边界失败时才重新讨论
  dynamic final-release observer registry。
- `EPOLL_CTL_MOD` 如何原子更新 interest、user data 与 mode，避免更新窗口 missed-ready。
- LT ready requeue、ET edge policy 和 `EPOLLONESHOT` disable/rearm 的内部 encoding；不得把
  callback 次数提升成 ABI event 计数。
- 多个 `epoll_wait()` waiter 的 wake-one / wake-all 语义和 `EPOLLEXCLUSIVE` 边界。
- future nesting target 中 epoll file 被外层 epoll 观察时的 callback 嵌套、锁序和
  ready propagation；这不是首版实现 gate。
- future nesting target 的 cycle detection、最大深度和 wake-path resource bound；
  首版只需稳定拒绝 epoll target。
- target source 不支持持久 subscription 时，`EPOLL_CTL_ADD` 的 Linux-compatible errno 和 regular-file 行为。
- epoll anonymous file 的 read/write/seek/ioctl/fcntl/fdinfo/final-release 行为边界。
- epoll wait/pwait/pwait2 的 temporary signal mask、timeout、restart 和 copyout partial-progress 语义。
- 第一阶段 source 范围及 probe 顺序尚未冻结；implementation resolution 必须先覆盖普通
  task-context 与 noirq/fixed-capacity 两类代表性 source，并在全量 subscription cutover 前
  审计 TTY 预分配 dirty handoff。是否包含 pipe、fanotify、socket 属于当时的 live-source
  scope 解析，不能反向改变协议统一边界。
- source / watch / ready queue / wait round 的最小诊断 identity、counter 和 trace 形状。

## Promotion 前闭合清单

以下清单记录公共 RFC promotion 前用于判断定位共识是否足以形成 Draft target 与 Stage 0 Ready 的闭合边界；当前结论已经折回 [RFC 入口](../index.md)、[不变量需求](../invariants.md) 与 [实施计划](../implementation.md)，本节只保留历史推导：

- `PollObserver` / `PollSubscription` 的语义角色和依赖方向稳定；
- `FileOps` snapshot / subscribe owner surface 稳定；
- source subscribe、notification、consumer retirement 与 route cleanup 的边界稳定；
- poll/select 从 `LatchTrigger` source queue 迁移到 wait-round subscriptions 的目标形态稳定；
- `Epoll` / `EpollWatch` 的身份、生命周期方向和 ready queue owner 稳定；
- `EPOLL_CTL_ADD` 的 publishing + current-ready + pending-callback 协议稳定；
- `MOD` / `DEL` / final close 与飞行中 callback 的失效语义稳定；
- opened-description identity、terminal liveness、transient lease 与 lazy retirement
  边界稳定；
- LT / ET / `EPOLLONESHOT` 的状态 owner 和 recheck 规则稳定；
- epoll file 自身 pollability 稳定，首版对 epoll target 的 fail-closed 边界明确；
- IRQ-off notification、资源上界和 cleanup context 有明确接受边界；
- 协议统一与容器/锁/批处理复用已经分离，future proof-first gate 能在全量迁移前否决
  feature-specific escape hatch 或困难 source 上不成立的公共抽象；
- 主要候选约束已经折回 [不变量需求](../invariants.md)，所有 Keter 已 neutralize；
  [实施计划](../implementation.md) 已把 live-source 风险转成具有停止条件和回写路径的
  Stage 0 Ready；
- 剩余问题可以被分类为 implementation gate、accepted limitation 或 backgrounds evidence。

[实施计划](../implementation.md) 已从 [RFC 入口](../index.md) 和
[Tracking Issues](../tracking-issues.md) 展开阶段、write set、probe、验证 floor 与停止条件。
它仍是公共 Draft / Not Authorized，不表示 target 已接受为 R0、transaction 已创建或 Stage 0
已经 Active。
