# RFC-20260620-threaded-timer-event

**状态：** Accepted for Implementation
**负责人：** doruche, Codex
**最后更新：** 2026-06-20
**领域：** time / timer / scheduler / kthread / timerfd / signal
**事务日志：** [2026-06-20-threaded-timer-event](../../devlog/transactions/2026-06-20-threaded-timer-event.md)
**开放问题：** 无实现 blocker；见 [Tracking Issues](./tracking-issues.md) 中的 Safe non-closure 和 Gate P1 证据项。
**下一步：** 按 [迁移实施计划](./implementation.md) 推进阶段 0/1；阶段推进和验证证据写入事务日志。

## 摘要

当前 soft timer 只有 IRQ callback 路径：`schedule_local_irq_timer_event()` 把 callback 放入本 CPU timer heap，timer interrupt 到期后直接在 IRQ context 执行 callback。这个形态适合真正短小、IRQ-safe、不会阻塞的 timeout completion；但 `timerfd` 和 `ITIMER_REAL` 已经把对象状态推进、wait trigger 触发、signal 投递和周期 rearm 等逻辑塞进 IRQ callback。为了避免这些路径继续扩大 IRQ lock-order 和死锁风险，本文提出第一版 threaded timer event。

第一版 threaded timer 不是通用 workqueue。它只提供一个非 IRQ 的 timer completion lane：deadline 仍由本 CPU timer IRQ 检测，到期事件被投递给本 CPU threaded timer worker，worker 在 process context 执行调用者提供的 bounded completion callback。调用者仍负责对象状态、stale 过滤、生命周期、周期 rearm 和 ABI 语义。

## 背景

当前实现状态：

- `time/timer.rs` 已有 `IRQ_EVENT_QUEUE`，timer interrupt 按 ticks 到期顺序弹出事件并直接运行 callback。
- `schedule_threaded_timer_event()` 只有占位实现。
- `timerfd` 当前通过 `schedule_local_irq_timer_event()` 安排到期 callback；callback 在对象 state lock 下核算到期次数、detach read/poll triggers，并在周期 timer 场景重新安排下一次事件。
- `ITIMER_REAL` 当前通过 IRQ timer callback 拿 thread-group itimer lock、检查 `validness`、投递 `SIGALRM`，并对 interval timer 重新安排下一次事件。
- wait-core timeout 也使用 IRQ timer，但它与 `WakeToken`、stale-safe placement、signal/force/cancel race 和 `finish()` outcome mapping 紧密绑定；本 RFC 第一版不迁移该路径。

已有 timerfd 小迭代把 IRQ callback 限制记录成 stage-1 bridge：周期 timer 的 successor event 必须保持 O(1)，后续应迁移到 threaded / cancellable timer infrastructure。本文只覆盖 threaded completion 上下文，不定义完整 cancellable timer core。

## 目标

- 新增 threaded timer event 作为非 IRQ timer completion lane，用于迁移不适合继续在 IRQ 中执行的 timer callback。
- 保留 IRQ timer API，要求调用点显式选择 IRQ context 或 threaded context。
- 第一批迁移对象限定为 `timerfd` 和 `ITIMER_REAL`。
- deadline 到期检测仍由 timer IRQ 执行；IRQ handler 只做到期事件出队、threaded-ready 投递和 worker 唤醒，不执行 threaded callback。
- 第一版 threaded timer 使用 per-CPU local queue 和 per-CPU worker。
- timer core 不扩大 `KThreadHandle` public surface；worker 本地性由 timer core 自己的 per-CPU worker slot 记录 `KThreadBuilder::cpu()` 创建请求和成功返回的 handle 绑定关系。
- 第一版实现同时引入通用 `Late` initcall level，用于启动已经依赖 ordinary kthread / 全 CPU online / 基础 probe 完成的晚期内核服务。
- threaded callback 运行在 IRQ enabled 的 process context，可以获取普通锁并在短 lock contention 中让出调度器，但仍必须是 bounded completion。
- 继续使用 one-shot event；周期语义由调用者在对象锁下根据自身状态和当前时间决定是否 rearm。
- schedule 阶段优先完成需要 process context 的资源准备；当前内核 heap / page allocator 是 noirq-capable，IRQ 投递路径允许简单、bounded 的 noirq allocation，但不得进入阻塞、reclaim、普通锁或用户可见错误恢复路径。
- 第一版 threaded schedule API 不把 allocation failure 作为 recoverable ABI 错误处理；OOM 或 timer core 未初始化都属于 panic / assertion 边界，不作为运行时错误返回。
- 调用者使用 weak / generation / validness 等逻辑失效模型处理 stale callback；第一版不提供物理取消、drain 或 per-object 串行化。
- 迁移后 `timerfd` 必须保留 missed-tick accounting，不退化成 worker 每跑一次只累计一次到期。
- 迁移后 `ITIMER_REAL` 只改变 completion context，不扩大 itimer 功能面。

## 非目标

- 不实现通用 workqueue、后台任务框架、worker pool、任务优先级、负载均衡或 CPU hotplug 策略。
- 不承诺 callback 可以执行任意阻塞工作、blocking I/O、长时间 reclaim、等待用户态、等待另一个 timer completion，或承载复杂后台任务。
- 不迁移 wait-core `schedule_wait_with_timeout()`，不改变 wait timeout 与 signal / force / event trigger 的 race 语义。
- 不提供物理取消、hard cancel、drain、`synchronize_timer` 或可取消 handle。
- 不提供 timer core 层面的 per-object identity、per-object FIFO、per-object 串行化或事件合并。
- 不提供 periodic timer core 原语；timer core 只提供 one-shot delayed completion。
- 不丢弃、不合并 threaded completion event，也不在 IRQ handler 中对 backlog 反向阻塞。
- 不补齐 `ITIMER_VIRTUAL`、`ITIMER_PROF`、POSIX timer、`timer_create`、timer overrun 或 alarm clock 语义。
- 不新增专门用户态测试体系；验证优先复用 LTP。只有能立即验证核心队列或上下文不变量、且适合 KUnit 的单元测试，才作为后续可选项。
- 不新增 kthread-specific / service-specific initcall level；initcall level 只表达启动时刻，不表达“是否会 spawn kthread”。

## 文档地图

Canonical：

- [不变量需求](./invariants.md)
- [迁移实施计划](./implementation.md)

Review tracking：

- [Tracking Issues](./tracking-issues.md)

## 已接受方向

### 上下文边界

threaded timer 是非 IRQ completion surface，不是任意可睡眠 workqueue。它的 callback 可以在 process context 获取普通 `Mutex`，并允许短 lock contention 进入调度，但只能执行 timer 语义所需的 bounded state advancement / notification。需要重活的子系统必须让 timer callback 唤醒自己的 owner / worker，而不是把重活塞进 threaded timer callback。

### API 分层

`schedule_local_irq_timer_event()` 和 threaded timer API 长期并存。调用点必须显式选择上下文：

- IRQ timer：用于真正 IRQ-safe、O(1)、不拿普通锁、不睡眠的短路径；若路径需要内存分配，只允许当前 noirq allocator contract 下的简单、bounded allocation，不能进入 reclaim、阻塞等待或用户态 errno 映射。
- threaded timer：用于需要 process-context completion 的对象 timer。

不做透明替换，也不把 threaded timer 设为默认路径。

### 第一批迁移对象

第一版只接受以下迁移对象：

1. `timerfd`：迁移对象状态推进、read/poll trigger 触发和周期 rearm 的 completion context。
2. `ITIMER_REAL`：迁移 `SIGALRM` 投递和 interval rearm 的 completion context。

wait-core timeout 暂不迁移。它仍通过现有 wait identity 和 IRQ timeout callback 与 source trigger、signal、force、cancel 竞争同一 wait round；是否迁移需要单独证明，不属于第一版目标。

### 到期检测与执行模型

deadline 到期检测仍在 timer IRQ 中执行。IRQ handler 只做最小工作：

1. 从本 CPU timer queue 中按 deadline 弹出到期 event。
2. 对 threaded event，把 event 投递到本 CPU threaded-ready queue；实现可以移动已准备好的节点，也可以在当前 noirq allocator contract 下执行简单、bounded allocation，但不得阻塞、reclaim、丢弃或合并 event。
3. 唤醒本 CPU threaded timer worker。

worker 不重新判定 deadline，只执行已经到期投递的 completion。callback 运行晚是允许的；对象需要按 `now` 计算 missed ticks 或剩余时间时，由对象 callback 自己处理。

### Locality

第一版使用 per-CPU local queue 和 per-CPU worker。它继承现有 local timer event 的基本形态，避免第一版引入全局 queue、跨 CPU 投递、远程唤醒和 worker 负载均衡。

timer core 为每个 CPU 维护一个只属于 timer owner 的 worker slot，slot 内保存创建请求使用的 CPU id 和成功返回的 `KThreadHandle`。IRQ 投递路径按 `cur_cpu_id()` 选择 ready queue 与 worker slot，并在调用 `wake()` 前断言 `slot.cpu == cur_cpu_id()`。该 CPU 字段是 timer core 对自身 per-CPU 发布表的 proof，不是 scheduler policy，也不要求 `KThreadHandle` 暴露放置查询。若实现发现必须查询 worker task 的实际 placement，必须先回到 kthread-core RFC 更新 handle contract，不能在本 RFC 中临时扩大 kthread public surface。

若断言失败，这是 timer worker 发布或 CPU 绑定不变量破坏，不能 fallback 到 remote wake 或 worker 重选。Gate P1 还必须审计 `handle.wake()` 下游不会在该本地 wake 路径走 remote IPI、blocking placement 或复杂分配。

callback 不保证跑在原 task 所在 CPU；第一批对象只要求非 IRQ context、对象 stale check 和对象锁下状态推进正确。

### Late Initcall

本轮实现接受新增通用 `Late` initcall level，属性形态为 `#[initcall(late)]`。`Late` 的语义是：基础 filesystem / driver / probe 初始化完成，`kthreadd` 已经手动建立，所有 CPU 已完成 local init 并 online，用户态 `init` 尚未 exec。

`Late` 只表达启动窗口，不表达 kthread policy。适合挂到 `Late` 的是各子系统自己的晚期初始化入口，例如 threaded timer worker、inode shrinker worker 和 OOM killer worker。`kthreadd` 自身不是 late initcall：它是 ordinary kthread spawn 的固定拓扑锚点和前置条件，必须继续由 boot path 手动初始化。

`Late` initcall 之间不提供相对顺序合同。timer worker、inode shrinker 和 OOM killer 不能依赖彼此先后；如果后续某个 consumer 需要依赖另一个 late consumer 的完成，必须显式表达依赖或回到文档层讨论新增阶段，而不是依赖 link order。

### 取消与生命周期

第一版不提供物理取消。schedule 成功后事件可能晚到；调用者必须使用 weak reference、generation、validness 或等价对象状态判断 stale callback。对象释放不需要 drain timer core；晚到 callback 应安全 no-op。

timer core 不替调用者强持有目标对象，也不提供 drop-time drain。若某个调用者需要 strong lifetime pinning，必须在自己的对象 contract 中说明理由，不能作为 threaded timer 默认模式。

### 失败语义

threaded schedule API 第一版不返回 allocation failure。资源准备优先发生在 process context；IRQ 投递路径若使用当前 noirq heap / page allocator 做简单 allocation，失败也按当前内核不可恢复错误或 assertion 边界处理，而不是向 `timerfd_settime()` / `setitimer()` 映射 `ENOMEM`、丢弃 event 或驱动 rollback 语义。timer core 未初始化同样属于内核 bug，用 assertion 暴露，不作为普通错误分支。若后续 allocator 引入可睡眠 reclaim 或不再满足 noirq 使用条件，必须先回到 RFC review 收紧 threaded-ready 资源准备模型。

调用者的对外可见 armed 状态仍必须与普通路径上的 event 提交保持一致。替换类操作不能在非 panic 路径留下 armed-but-unscheduled，也不能让 staged settime / setitimer 重排把对象状态推进到没有对应 queued event 的状态。

### Backlog 与观测

第一版不丢弃、不合并、不阻塞 IRQ。threaded-ready backlog 超过阈值时应有日志或计数作为后续设计证据；吞吐不足只能作为后续 worker pool、合并或 backpressure 设计的输入，不能在第一版中通过破坏 timerfd / itimer 语义隐藏。

### 注释迁移

迁移后应替换现有 IRQ 临时桥注释，而不是删除约束。新的注释应说明：

- callback 运行在 threaded process context；
- callback 仍是 bounded timer completion；
- stale 过滤由对象 generation / validness 负责；
- 周期 rearm 和 missed-tick accounting 由对象状态负责；
- 不得把 threaded timer callback 当成通用后台任务。

## 接受边界

本文被接受表示：threaded timer event 第一版的目标是迁移不适合 IRQ context 的现有 timer completion，且只覆盖 bounded process-context completion。实现方案必须满足 [不变量需求](./invariants.md)，并按 [迁移实施计划](./implementation.md) 的 gate 推进。

本文被接受不表示：

- 可以把 threaded timer 变成通用 workqueue。
- 可以迁移 wait-core timeout 或改变 wait-core timeout race 语义。
- 可以把物理取消、drain、periodic timer core、per-object 串行化或 worker pool 纳入第一版。
- 可以把 `Late` 当作 kthread-specific initcall，或把 `kthreadd` 隐藏进 initcall。
- 可以让 late initcall 之间形成隐式顺序依赖。
- 可以把 allocation failure 当作普通用户态 errno 或 rollback 驱动项。
- 可以为了 timer worker locality 临时扩大 `KThreadHandle` placement API；若需要 kthread handle 暴露放置查询，必须先更新 kthread-core contract。
- 可以丢弃、合并或延迟隐藏 timerfd / itimer 的用户可见语义。
- 可以绕过本 RFC 或继续把旧私人草稿当作 canonical source；公共权威文本是本目录下的 RFC 文件。
- 可以隐式关闭 `ANE-20260616-LTP-POST-SUMMARY-HANG`。本 RFC 只能作为 timer 相关排查线索；只有后续证据证明根因属于 timerfd / itimer threaded migration 范围时，才可回写 register。

如果实现阶段发现以下情况，必须回到文档层更新本 RFC 或新建 follow-up RFC：

- wait-core timeout 也必须迁移才能关闭已知死锁或锁序风险；
- 第一版需要物理取消、drain、hard cancel 或可取消 handle；
- per-CPU worker 无法满足安全性或可观测性，需要全局 worker / worker pool；
- backlog 需要合并、丢弃或 backpressure 才能保持系统可用；
- `timerfd` 或 `ITIMER_REAL` 迁移要求改变用户可见 ABI、itimer 功能范围或 timerfd missed-tick 语义。

## 备选方案

### 让 threaded timer 替代所有 IRQ timer

拒绝。IRQ timer 仍适合真正短小、IRQ-safe 的 completion。透明替换会模糊调用点上下文选择，让后续锁序和 latency 难以审计。

### 做成通用 workqueue

拒绝作为第一版。通用 workqueue 需要 worker pool、backpressure、长任务隔离、drain、stop、优先级和死锁策略。当前问题是 timer completion context 错位，不需要一次性引入完整后台任务框架。

### 第一版迁移 wait-core timeout

拒绝。wait-core timeout 与 wait identity、stale-safe placement、signal/force/cancel race 和 `finish()` outcome mapping 绑定。threaded worker 的调度延迟会改变 timeout 与其它 completion 的 race 表面；该迁移需要单独 RFC 级证明。

### 提供物理取消

延期。当前 `timerfd` generation 和 `ITIMER_REAL` validness 已经能表达 stale callback。物理取消会牵扯 IRQ queue、ready queue、worker-running callback 和对象生命周期之间的 remove/drain 协议，超出第一版。

### timer core 提供 periodic timer

拒绝作为第一版。timerfd 和 itimer 的周期语义都依赖对象状态，尤其 timerfd 需要按 `now` 计算 missed expirations。core 只提供 one-shot event，周期 rearm 由对象 owner 负责。

### 全局 threaded timer worker

拒绝作为第一版。现有 timer event 是 local CPU 形态；全局 worker 会提前引入跨 CPU 投递、远程唤醒和全局队列锁。per-CPU worker 更适合先关闭 IRQ context 风险。

## 风险

- callback 虽然不在 IRQ 中运行，但如果被误用为后台任务，仍可能造成 worker backlog 或新的锁等待链。控制方式是 bounded completion contract、日志/计数观测和 review gate。
- 不提供物理取消意味着 stale callback 一定会存在。控制方式是调用者必须使用 weak/generation/validness，并在对象锁下检查状态。
- 第一版不处理 allocation failure，意味着真实 OOM 会走 panic / assertion 边界。控制方式是只允许当前 noirq allocator contract 下的简单、bounded allocation，并把普通路径上的 armed-but-unscheduled 以及 IRQ 投递失败后丢弃/合并 event 明确列为禁止退化项。
- per-CPU worker 可能在 backlog 下延迟通知。控制方式是第一版不承诺低延迟，只要求 timerfd/itimer 按对象状态保持语义正确，并暴露 backlog 证据。
- 不迁移 wait-core timeout 可能留下另一个 IRQ callback 使用面。控制方式是把它列为显式非目标；若后续证据显示它也必须迁移，回到 RFC review。

## 验收边界

后续实现的最低验收语义应覆盖：

1. `timerfd` 迁移后仍按对象状态和当前时间计算 missed expirations，周期 timer 不退化成每次 worker 执行只累计一次。
2. `timerfd` 的 blocking read / poll readiness 仍由对象状态和 latch trigger 语义决定，threaded callback 只作为 readiness hint 的投递来源。
3. `ITIMER_REAL` 迁移后仍只覆盖 real itimer，仍通过 existing signal / thread-group exit 路径投递 `SIGALRM`，不扩大到 virtual/prof 或 POSIX timer。
4. 普通路径上不发布 armed-but-unscheduled 状态；替换类操作不能把旧 timer 取消后留下没有 queued event 的新 armed state。
5. IRQ handler 不执行 threaded callback；threaded 投递路径若分配内存，只能走当前 noirq allocator contract 下的简单、bounded allocation，不能阻塞、reclaim、拿普通锁或丢弃 event。
6. 现有 IRQ timer API 保留，wait-core timeout 未被隐式迁移。
7. 不新增专门用户态测试体系；验证优先复用 LTP timerfd / itimer 相关测例。若实现期添加 KUnit，只能覆盖核心队列或上下文不变量。

## 收口

当前公开 RFC 的收口顺序建议是：

1. 关闭、接受或转成实现 gate 的 [Tracking Issues](./tracking-issues.md) active 项。
2. 若决定进入实现流程，把本文状态改为 Accepted for Implementation，并建立 transaction devlog。
3. 按 [迁移实施计划](./implementation.md) 推进 staged gate；实现反馈按影响回写 RFC、tracking issue、transaction devlog 或 register。
4. 实现收口后，替换 `timerfd` / `ITIMER_REAL` 中的 IRQ 临时桥注释，保留 bounded threaded-completion 注释。
