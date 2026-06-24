#import "../template/components.typ": *
#import "../template/figures.typ": *

= 调度、等待与时间

#epigraph(attribution: [Butler Lampson, @lampson1983hints])[
  Make actions atomic or restartable.
]

#thesis[
  Anemone 的 scheduler 拥有 runnable state、CPU placement 和运行队列；wait-core 拥有阻塞协议、wait identity、completion 线性化点和 stale-safe wake placement；timer 提供时间触发，但不接管等待状态。task 是这些协议作用的对象，event source 只发布受限 wake capability。这个拆分让 sleep / wake、poll/select OR wait、signal interruption、timeout 和 timerfd / itimer completion 可以共享同一组不变量，而不是各写一套 task 状态机。
]

调度和等待经常被写在一起，因为最终效果都像是“这个 task 不运行了”或“这个 task 又能运行了”。但对内核设计来说，这两个动作的 owner 完全不同。调度器决定哪个 runnable task 进入 run queue、当前 task 是否 requeue、跨 CPU wake 是否走 IPI；wait-core 决定一轮等待何时发布、由谁完成、旧 wake 为什么不能完成新一轮、完成后如何退役。timer 只是一个 producer，它可以在 deadline 到来时触发 timeout 或对象状态推进，但不能替 task 或 wait-core 决定等待是否仍有效。

== Scheduler runnable state

Anemone 的 scheduler 明确以 per-CPU owner 为中心。`Processor` 保存本 CPU running task、run queue、scheduler context 和 `need_resched`；`Task::cpuid()` 在创建后表示该 task 的 owner CPU；`local_enqueue()` 要求 task 内部状态已经是 runnable，并断言 task 属于当前 CPU；`remote_enqueue()` 只是 strict non-wait-tail placement，通过 IPI 把已经 runnable 的 task 送到目标 CPU。

这不是完整负载均衡，也不是 Linux CFS 复刻。`pick_next_cpu()` 只是新 task 创建时的 CPU 选择策略入口；调度类拥有 run queue policy，timer tick 通过 `RunQueue::on_tick()` 触发 reschedule 请求。更关键的边界是：裸 enqueue API 只接受“已经 runnable”的 task；wait completion tail 必须走 `wake_enqueue()`，因为旧 wake、late timeout 或 stale trigger 需要重新验证 task 是否仍处在同一轮等待的完成结果上。

#listing([普通 enqueue 与 wait completion placement 是两条不同入口])[
```rust
pub fn local_enqueue(task: Arc<Task>) {
    assert!(task.cpuid() == cur_cpu_id());
    assert!(task.is_sched_runnable());
    ...
}

pub fn local_wake_enqueue(task: Arc<Task>, park: ParkState) -> WakeEnqueueResult {
    if !matches!(task.sched_state(), TaskSchedState::Runnable) {
        return WakeEnqueueResult::Stale;
    }
    ...
}
```
]

runtime accounting 也在这条边界内。`TaskCpuUsage` 记录 running flow 属于 user 还是 kernel，并在 switch-in、switch-out 和 privilege change 时 settle 到 task 自身的 user/kernel 时间；`ThreadGroupCpuUsage` 再把成员和已 reap child 的时间聚合起来。这些数据可用于 `/proc`、`getrusage`、`times` 等观察面，也可以为更复杂调度策略提供材料；它们目前构成的是 runtime accounting，不是已经存在的 EEVDF/CFS 级调度语义。

#boundary[
  当前 scheduler 的承诺是 owner boundary：runnable placement 属于 scheduler，runtime accounting 是独立观测与统计状态，wait completion 不能绕过 wait-core 直接塞进 run queue。Linux 级调度策略、实时调度类和跨 CPU 负载均衡仍不属于这条已收束边界。
]

== Wait-core 阻塞协议

wait-core 的核心边界，是把一轮阻塞从 task 的观察状态中分离出来：每轮等待都有稳定 `WaitState` identity；source 持有 `WakeToken` 或受限 trigger；event wake、timeout、signal、force 和 caller cancel 竞争同一个 wait state；成功 wake 后，wait-core 负责 stale-safe physical placement。`TaskStatus` 被降级为观察投影，`TaskSchedState` 才是 scheduler/wait 的内部状态。

#listing([`WaitState` identity 和 `WakeToken` 把一轮等待从 task 状态投影中分离出来])[
```rust
pub enum TaskSchedState {
    Runnable,
    Waiting {
        state: Arc<WaitState>,
        interruptible: bool,
        park: ParkState,
    },
    Zombie,
}

pub(super) struct WakeToken {
    state: Arc<WaitState>,
}
```
]

这条设计解决的不是某个单独的 panic，而是一个协议问题：如果旧 wake tail 晚于 waiter 进入下一轮等待，裸 `task_enqueue()` 会把旧一轮的物理 placement 强行作用到新一轮状态上。wait-core 用 wait identity 让旧 token 失效，用 `ParkState::PrePark/Parked` 处理 schedule 与 wake 的交错，用 `wake_enqueue()` 把逻辑完成和物理 placement 绑在一起。scheduler 仍然负责 run queue，但它不再需要猜测某个 wake 是否属于当前等待轮次。

#invariant[
  一轮阻塞的安全顺序是：caller 先通过 wait-core 发布等待身份和 wake capability，再让出 CPU；producer 只能用 capability 完成本轮 wait；返回用户可见结果前，waiter 必须 finish/retire 本轮等待，并按 owner 的 predicate 重新分类结果。
]

`Event` 是 wait-core 上的一个 adapter，而不是另一个状态真相源。它维护 listener 队列、exclusive quota 和 mode-blocked requeue，但 listener 里保存的是 wait-core token；`Event::publish()` detach listener 后在 event lock 外调用 wait-core wake，不直接写 task 调度状态，也不补做 runqueue placement。signal/force 的 `notify()` 同样走 `wake_active_wait()`，只是在没有 active wait 时记录 stale/no-active-wait，而不是恢复旧的 legacy waiting 分支。

#book-figure(
  "../assets/figures/ch04/wait-core-boundary.png",
  [wait-core 拥有阻塞协议，event source 只发布 wake capability。],
  width: 100%,
)

== Latch OR wait

`ppoll` / `pselect6` 的难点不是“等待某个 Event”，而是一个 syscall-local consumer 同时等待多个 fd source，任意一个 source ready 都应该完成本轮 wait。`sched::latch` 在 wait-core 上提供这层 OR 组合：consumer 持有不可 clone、owner-bound 的 `Latch`；producer 只拿 cloneable `LatchTrigger`；旧 trigger 晚到时必须通过 wait identity 失败，而不是依赖 source queue cleanup 及时发生。

fd source 的注册协议也因此变得可审。`PollRequest::snapshot()` 只看当前 readiness；`PollRequest::register()` 带本轮 trigger。source 必须在自己的状态锁下同时检查 readiness 并保存 trigger，状态变化时在同一 source lock 临界区 detach trigger，释放 source lock 后再 trigger。`ppoll` / `pselect6` 的 shared helper 采用 snapshot scan -> begin latch -> register scan -> schedule -> finish -> final snapshot scan；wake 只是 readiness hint，返回前必须重扫 predicate。

这也解释了 snapshot-only source 的边界。source 如果没有 register / wake 能力，就只能参与 readiness snapshot，不能把 caller 放进一轮真正可完成的阻塞等待。helper 在这种情况下 cancel + finish 后做 final snapshot scan；如果仍无 ready，就按 fail-closed 处理。这个选择牺牲了部分尚未迁移 source 的阻塞兼容性，但保住了等待协议的单一真相源。

#book-figure(
  "../assets/figures/ch04/latch-or-wait.png",
  [`ppoll` / `pselect6` 的 OR wait 是一轮 latch，而不是多个 source 自己调度 task。],
  width: 100%,
)

== Signal、timeout 与 wait completion

signal interruption、timeout 和普通 event wake 在用户态看起来是不同 errno 或返回值，在 wait-core 内部却都是同一轮等待的 completion 竞争者。`clock_nanosleep()`、`rt_sigtimedwait()` 和 Event timeout 都通过受限 current-wait adapter 或 `ActiveWait` 创建本轮 wait；timeout callback 用同一 `WakeToken` 调 `wake_wait(..., Timeout, AnyWait)`；signal/force 用 `wake_active_wait()`。完成后，caller 按自己的 syscall 语义解释 `Completed(Timeout)`、`Completed(Signal)`、`Completed(Force)` 或 predicate-ready。

这条边界也说明，wait-core 不能替每个 syscall 完成最终语义分类。同步 signal wait 这类路径在 wait completion 后还必须回到 signal syscall owner，重新检查 waited set、pending signal 和 interrupted 状态；wait-core 只报告 completion cause，不能把所有 caller predicate 都压成统一 errno。

同理，一个外部观察到的卡住现象也不能默认归因于 wait4、waitid 或 wait-core。正确的边界是先区分 task exit、cleanup、timer、I/O object 和 IRQ context，再决定哪个 owner 应该解释失败信号。wait-core 是核心协议边界，但不是每个阻塞现象的默认答案。

== Time trigger boundary

Anemone 的 clock 层提供 `Clock::now_ns()` 和固定 clock table。`CLOCK_BOOTTIME` 当前映射到 monotonic timeline，使 timerfd、sleep 和 timeout 路径共享同一组 clock domain 和 deadline 处理。这个取舍让等待路径保持简单；代价是 suspend/resume accounting、time namespace 和 Linux `CLOCK_BOOTTIME` 的完整差异化语义不在当前承诺内。

timer core 则分成两条 lane。`schedule_local_irq_timer_event()` 保留 IRQ callback，要求 callback 不睡眠、不拿 ordinary mutex、不依赖 process context；wait-core timeout 留在这条 lane，因为它和 wait identity、signal/force/source trigger race 以及 `finish()` outcome mapping 紧密绑定。`schedule_threaded_timer_event()` 提供 per-CPU threaded completion lane：timer IRQ 仍负责 deadline 检测，到期后只把 callback 投递到本 CPU ready queue 并 wake 本 CPU timer worker，callback 在 process context 运行，但仍必须是 bounded timer completion，不是通用 workqueue。

timerfd 和 `ITIMER_REAL` 使用 threaded lane，但对象状态仍归各自 owner。timerfd 的 `TimerFdState` 保存 generation、schedule、expirations、read/poll triggers 和 cancel-on-set 兼容状态；到期 callback 在对象锁下推进 missed-tick accounting、detach triggers，并由 generation 过滤 stale callback。`ITIMER_REAL` 的 thread-group itimer state 保存 expire time、interval 和 validness；callback 在 state lock 下提交 signal/rearm action，释放锁后才投递 `SIGALRM`。

#listing([threaded timer 是 bounded completion lane，不是对象状态 owner])[
```rust
pub fn schedule_threaded_timer_event(
    expire: Duration,
    callback: Box<dyn FnOnce() + Send + 'static>,
) {
    assert!(threaded_worker_ready());
    push_timer_event(TimerEvent::new_threaded(expire_ticks_after(expire), callback));
}
```
]

#tradeoff[
  threaded timer 把不适合 IRQ context 的 timerfd / ITIMER_REAL completion 移到 process context，降低锁序和 IRQ 上下文风险。代价是它不提供取消、drain、worker pool、periodic core 或 per-object serializing；stale filtering、missed-tick accounting、interval rearm 和用户可见语义仍必须留在对象 owner 内。
]

== TradeOff: 统一 wait-core 与阶段化兼容

这个设计的主要收益是组合性。Event、Latch、timeout、signal 和 timer 都可以作为 producer，但它们不需要也不能各自发明 task 状态机；scheduler 可以继续维护 per-CPU run queue 和 placement 断言，而不用理解每种等待源的生命周期；time subsystem 可以提供 deadline 和 completion context，而不把 timerfd 或 itimer 的对象状态下沉到 timer core。

剩余缺口也更容易分类。`pselect6` 的 exception / POLLPRI 属于 fd readiness 语义边界，不影响 READABLE / WRITABLE OR wait 的协议闭合；wait-core timeout 留在 IRQ lane，说明时间触发路径仍按 wait identity 和 outcome mapping 收束；外部卡住现象也会落回 task exit、cleanup、timer、I/O object 或 IRQ context 等具体 owner。一个能解释失败归属的边界，比一个声称“一切都统一了”的抽象更有价值。
