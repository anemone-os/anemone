# Threaded Timer Event 不变量需求

**状态：** Draft / Canonical for RFC Review
**最后更新：** 2026-06-20
**父 RFC：** [RFC-20260620-threaded-timer-event](./index.md)

本文定义第一版 threaded timer event 必须满足的上下文、状态所有权、生命周期、队列和迁移验收边界。后续实现按 [迁移实施计划](./implementation.md) 推进；任何实现反馈都不得削弱本文不变量来换取局部通过。

## 闭合条件

迁移完成后必须同时满足：

1. timer IRQ 仍是 deadline 到期检测中心；threaded worker 只执行已到期投递的 completion。
2. IRQ timer 和 threaded timer 是显式并存的两个 API；调用点必须选择上下文，不能透明替换。
3. IRQ handler 不执行 threaded callback，不拿普通锁，不阻塞；threaded 投递路径只允许使用当前 noirq-capable heap / page allocator 做简单、bounded allocation，不能进入 reclaim、等待或用户可见错误恢复。
4. threaded callback 运行在 IRQ enabled 的 process context，可以获取普通锁并短暂让出调度器，但只能做 bounded timer completion。
5. timer core 不提供 per-object identity、per-object 串行化、periodic timer、物理取消、drain 或 lifetime pinning。
6. schedule 成功后事件可能晚到；调用者必须用 weak/generation/validness 或等价状态过滤 stale callback。
7. 第一版不把 allocation failure 作为 recoverable schedule 失败；普通路径不能发布 armed-but-unscheduled 状态。
8. `timerfd` 和 `ITIMER_REAL` 迁移只改变 completion context，不改变用户可见 timer 语义或扩大功能面。
9. wait-core timeout 第一版不迁移；其 wait identity / timeout / signal / force / cancel race 语义不被 threaded timer RFC 重写。
10. backlog 不通过丢弃、合并或阻塞 IRQ 隐藏；第一版只要求观测和 bounded callback 约束。
11. `Late` initcall 只表达晚期启动窗口，不表达 kthread/service policy；`kthreadd` 不得迁入 initcall。

如果任一条件不成立，当前实现只能视为止血或中间态，不能声明 threaded timer event 第一版闭合。

## 非目标

本文不定义：

1. 通用 workqueue、worker pool、优先级、负载均衡或 CPU hotplug 策略。
2. blocking I/O、长时间 reclaim、等待用户态或任意可睡眠后台任务语义。
3. hard cancel、drain、`synchronize_timer`、可取消 handle 或 callback 正在执行时的同步停止。
4. timer core 层面的 periodic timer、missed-tick accounting 或 overrun accounting。
5. wait-core timeout 迁移。
6. `ITIMER_VIRTUAL`、`ITIMER_PROF`、POSIX timer、alarm clock 或 time namespace 语义。
7. 专门用户态测试体系；除非后续有适合 KUnit 的小单元，否则验证复用 LTP。
8. kthread-specific / worker-specific initcall level，或 late initcall 之间的隐式依赖排序。

## 状态所有权

threaded timer core 只拥有 timer event 的排队、deadline 到期投递和 worker 执行状态。对象 timer 的 armed/disarmed、generation、expiration counter、signal state、interval、poll/read trigger 和 ABI 语义都由调用者 owner 拥有。

硬性要求：

1. `timerfd` 的单一真相源仍是 timerfd 对象 state，包括 schedule、generation、expiration counter、read/poll wait queues 和 missed-tick accounting。
2. `ITIMER_REAL` 的单一真相源仍是 thread-group itimer state，包括 `expire_at`、`interval` 和 `validness` 或后续等价 stale token。
3. timer core 不读取或解释对象 generation / validness，只负责执行 callback。
4. timer core 不知道两个 events 是否属于同一对象，因此不能承担 per-object ordering、merge、cancel 或 duplicate suppression。
5. backlog 计数、queue length、event id、worker wake count 等可以作为诊断字段；诊断字段不得反向驱动 timerfd / itimer 行为。

如果实现为了重排 publish 顺序需要临时构造 pending state，该 pending state 不能对用户可见为 armed timer，除非对应 event 已经提交到 timer core。

## 身份与能力模型

threaded timer event 的身份只服务 timer core 排队和诊断。对象身份由调用者的 weak reference、generation、validness 或对象锁下状态检查表达。

要求：

1. schedule 成功不返回可取消 handle。第一版 schedule API 不返回 recoverable allocation failure；正常返回即表示 event 已提交。
2. callback closure 可以携带 weak reference、generation、validness token 或其它对象私有能力，但 timer core 不解释这些能力。
3. stale callback 必须能安全 no-op；对象释放不得要求 drain timer core。
4. callback 不应默认强持有目标对象。若某调用者确实需要 strong ref，必须在调用者文档中说明生命周期理由和退出影响。
5. 同一对象的多个到期事件可能晚到或交错；对象锁和 stale token 必须决定哪个 callback 仍有效。
6. worker placement proof 由 timer core-owned per-CPU worker slot 表达。slot 中的 CPU id 来自 `KThreadBuilder::cpu()` 创建请求和成功返回 handle 的发布记录，只用于证明 timer core 按本 CPU worker wake。它不是 scheduler policy 入口；timer core 不得通过 worker handle 读取底层 `Task`、scheduler state 或 kthread 控制状态。

## 线性化点

本文需要区分四个线性化点：

1. schedule 提交线性化点：event 资源准备完成并进入 timer queue。
2. 对象 armed 发布线性化点：用户可见 timer 状态变为 armed。
3. deadline 到期投递线性化点：timer IRQ 判断 event 到期并把 threaded event 放入 threaded-ready queue。
4. 对象 completion 线性化点：threaded callback 在对象锁下确认 token 有效并推进对象状态。
5. `ITIMER_REAL` signal action commit 线性化点：callback 在 itimer state lock 下确认 stale token 仍有效并生成 `SIGALRM` / interval rearm 动作时，`ITIMER_REAL` completion 已提交。

硬性要求：

1. 对象 armed 发布不得早于可保证 event 已提交；第一版不通过 recoverable allocation-failure rollback 证明这个关系。
2. 替换类操作不能在普通路径先取消旧 timer，再留下 disarmed 或 armed-but-unscheduled 假状态。
3. deadline 只在 IRQ 到期出队点判断。worker 不重新判定 deadline，也不把当前时间未满足作为丢弃 event 的理由。
4. callback 执行晚是合法状态；对象 completion 必须按对象状态和当前时间自行计算 missed ticks 或 signal 投递条件。
5. worker 不阻塞后续 timer IRQ。后续 IRQ 可以继续投递 event；timer core 只保证投递顺序和 worker FIFO 执行意图，不保证前一个 callback 完成后才投递后一个事件。
6. `ITIMER_REAL` 已提交的 signal action 在释放 itimer state lock 后必须无条件执行。cancel / replace 只能阻止尚未通过 token 检查的 stale callback；若后续希望撤回已生成 action，必须单独设计 pending-signal 撤销语义，不能混在 threaded timer 迁移中。

## 锁序与上下文规则

### IRQ 投递路径

IRQ handler 必须保持最小工作集：

1. 只允许使用当前 noirq-capable heap / page allocator 做简单、bounded allocation；不得进入 reclaim、阻塞等待、普通锁或用户态 errno 映射。
2. 不执行 threaded callback。
3. 不获取普通 sleepable lock。
4. 不等待 worker、对象锁、用户态或 I/O。
5. 只投递 event、更新 timer core 队列状态，并唤醒本 CPU worker；不能丢弃、合并或回退用户可见 timer state。
6. IRQ handler 必须按 `cur_cpu_id()` 选择 ready queue 和 timer core-owned worker slot，并在 `wake()` 前断言 `slot.cpu == cur_cpu_id()`；该断言失败是 worker 发布/CPU 绑定不变量破坏，不能 fallback 到 remote wake、worker 重选或 blocking placement。

如果 ready queue 或 event node 需要资源，实现应优先在 schedule 阶段准备；若选择在 IRQ handler 中做简单 allocation，该路径依赖当前 heap / page allocator 的 noirq contract，并必须证明不会阻塞、reclaim、拿普通锁或产生 recoverable timer API 错误。第一版不把 allocation failure 建模成普通 timer API 错误，也不能因资源不足丢弃或合并 event。若 allocator noirq contract 不成立或后续发生变化，必须回到 RFC review 收紧资源准备模型。

### Threaded callback 路径

threaded callback 运行在 process context，但仍是 timer completion：

1. 允许 IRQ enabled。
2. 允许获取普通 `Mutex`，并允许短 lock contention 慢路径进入调度。
3. 允许触发 latch、投递 signal、推进 timerfd/itimer 对象状态和安排下一次 one-shot event。
4. 不允许执行 blocking I/O、长时间 reclaim、等待用户态、等待另一个 timer completion 或承担复杂后台任务。
5. callback 如果发现需要重活，必须唤醒对应子系统 owner / worker 并返回。

### Worker 生命周期

第一版 worker 是启动期创建、运行期常驻的 per-CPU 基础设施：

1. 普通调用者不可 stop / drain / restart worker。
2. schedule 成功后不能因为 worker 被普通路径停止而永远不执行。
3. worker 初始化失败属于启动期或内核 bug 边界，不作为普通 timer API 错误。
4. 对象释放不等待 worker drain；晚到 callback 依赖 stale check no-op。
5. ready queue slot、worker slot 和 wake target 必须按 CPU 绑定；IRQ 投递路径必须通过 timer core-owned slot CPU 断言或等价源码证据证明只唤醒本 CPU worker。

## Initcall 边界

第一版 threaded timer worker 使用通用 `Late` initcall 启动。`Late` 的硬性语义：

1. `Fs` / `Driver` / `Probe` initcall 已完成。
2. `kthreadd` 已由 boot path 手动初始化，ordinary kthread spawn 已合法。
3. 所有 CPU 已完成 local init 并 online，pinned per-CPU worker 可以发布。
4. 用户态 `init` 尚未 exec。

边界要求：

1. `Late` 是通用启动阶段，不能命名或设计成 `kthread` / `service` / `worker` 专用阶段。
2. `kthreadd` 不走 initcall；它是 `Late` consumer 的前置锚点，不是 consumer。
3. threaded timer worker、inode shrinker 和 OOM killer 可以分别由所属子系统挂 `#[initcall(late)]`。
4. late initcall 之间不提供相对顺序。任何 late consumer 都不能依赖 timer worker、inode shrinker 或 OOM killer 已经先于自己启动，除非该依赖在独立 contract 中显式表达。
5. timer core 未初始化但调用 threaded schedule 属于内核不变量破坏，应由 assertion 暴露，不作为普通 timer API 错误返回。

## 队列与 Backlog 规则

第一版使用 per-CPU local timer queue 和 per-CPU threaded-ready queue。

要求：

1. 到期 event 按 deadline 从 timer queue 出队。
2. threaded-ready queue 按 IRQ 投递顺序执行 FIFO 意图。
3. worker backlog 不能反向阻塞 IRQ handler。
4. timer core 不丢弃、不合并 event。
5. backlog 超过阈值时应保留诊断日志或计数；诊断不能改变语义。
6. 如果后续证据显示需要 worker pool、合并、丢弃、backpressure 或优先级，必须回到 RFC review。

同一对象的顺序语义不由 timer core 提供。即使 ready queue FIFO，callback 睡在对象锁上时也可能与后续事件交错；对象 owner 必须用 generation/state lock 处理。

## 调用者迁移规则

### Timerfd

`timerfd` 迁移必须满足：

1. generation 仍用于 stale callback 过滤，或替换为等价对象-owned token。
2. expiration counter 和 missed-tick accounting 仍在 timerfd state lock 下按 `now` 和 schedule state 计算。
3. 周期 timer 不能退化为 worker 每执行一次只累计一次到期。
4. read/poll trigger detach 仍遵守现有“对象锁下 detach，锁外 trigger/drop”的边界。
5. `TFD_TIMER_CANCEL_ON_SET`、`CLOCK_BOOTTIME` stage-1 限制不因 threaded timer 迁移而扩大或改写。
6. `timerfd_settime()` 替换旧 timer 时，普通路径不得发布没有 queued event 的新 armed state。

### ITIMER_REAL

`ITIMER_REAL` 迁移必须满足：

1. 只迁移 real itimer completion context。
2. `validness` 或等价 token 仍用于 stale callback 过滤。
3. `SIGALRM` 仍通过现有 signal / thread-group 路径投递。
4. itimer state lock 内只做 stale token 检查、状态推进、interval rearm 决策和本地动作生成；`SIGALRM` 投递应在释放 itimer state lock 后执行，除非实现提供显式锁序证明。
5. interval rearm 仍由 thread-group itimer state 决定。
6. 不补 `ITIMER_VIRTUAL`、`ITIMER_PROF`、POSIX timer、timer overrun 或 `timer_create`。
7. `setitimer()` 替换旧 timer 时，普通路径不得发布不可触发的新 state。

### Wait-Core Timeout

wait-core timeout 保持第一版非目标：

1. 不迁移 `schedule_wait_with_timeout()`。
2. 不改变 `WakeToken`、timeout callback、signal/force/cancel/source trigger 的竞争规则。
3. 不通过 threaded timer 引入新的 wait outcome 或 timeout-delivery 延迟语义。
4. 如果后续必须迁移，必须单独讨论 wait-core accepted contract。

## 失败语义

threaded schedule API 的失败边界必须可审计：

1. 第一版不返回 recoverable allocation failure。
2. 资源准备发生在 process context；真实 OOM 按当前内核不可恢复错误处理，不映射到用户态 errno。
3. 内部不变量破坏，例如 timer worker 未初始化或 per-CPU timer core 不存在，应使用 assertion 暴露，而不是返回普通错误。
4. 对用户 ABI 的 errno 映射不包含 threaded timer allocation failure；timer core 不替 timerfd / itimer 映射 ABI。

如果实现发现必须把 allocation failure 纳入用户可见 ABI 或 rollback 合同才能保持对象语义，必须停止并回到文档层；这超出第一版边界。

## 禁止退化项

以下做法不能作为最终关闭条件：

1. 用 threaded timer 替换所有 IRQ timer，导致调用点不再显式选择 context。
2. 在 IRQ handler 中执行 threaded callback。
3. 在 IRQ 投递 threaded event 时执行复杂/阻塞分配、reclaim、获取普通锁，或把 allocation failure 映射成用户可见 errno。
4. callback 执行 blocking I/O、长时间 reclaim、等待用户态或等待其它 timer completion。
5. 在 timer core 中保存对象 key，并用它做 per-object merge、cancel 或串行化。
6. schedule 成功后要求调用者 drain 才能释放对象。
7. 普通路径留下 armed-but-unscheduled。
8. 替换类操作先取消旧 timer，再留下没有 queued event 的新 armed state。
9. 周期 timer 语义下沉到 timer core，导致 timerfd missed-tick accounting 退化。
10. worker backlog 时丢弃、合并或静默跳过 event。
11. 为了第一版迁移顺手实现 `ITIMER_VIRTUAL`、POSIX timer、alarm clock 或 time namespace。
12. 把 wait-core timeout 迁移作为 timerfd / itimer threaded migration 的附带改动。
13. 删除 IRQ 桥注释后不补 bounded threaded-completion 注释，让后续读者误以为 callback 可任意阻塞。
14. 把 `Late` 设计成 kthread-specific 阶段，或让 late initcall 之间形成隐式顺序依赖。

## 完成标准

文档层完成标准：

1. 本文和 `index.md` 的目标、非目标、上下文合同、失败语义和迁移对象已被 review 接受。
2. [迁移实施计划](./implementation.md) 只讨论满足本文的实现路线，不重新打开已接受边界。
3. 如需改变 wait-core timeout、物理取消、worker pool、periodic core、backpressure 或对象生命周期 pinning，必须先更新本文或新增 follow-up RFC。

实现层最低语义验证应包含：

1. `timerfd` LTP 相关测例复用验证，尤其周期 timer 和 poll/read readiness 不退化。
2. `ITIMER_REAL` / `SIGALRM` 相关 LTP 或已有测例复用验证。
3. 源码审计确认 IRQ handler 不执行 threaded callback、不做复杂/阻塞分配、不拿普通锁。
4. 源码审计确认 wait-core timeout 未被隐式迁移。
5. 如添加 KUnit，只覆盖核心 queue/context 不变量，不新建大用户态测试体系。
