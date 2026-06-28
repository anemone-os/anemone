#import "../components/figure.typ": code-block, report-figure

= 调度

调度的核心是Task的调度状态机。Anemone 的调度设计没有把“用户态看到的状态”和“调度器内部状态”混成同一个字段，而是让内部状态先给出正确的调度转换，再投影成 Linux 风格的 task state。这样，我们在实现的时候，首先考虑的不是兼容Linux，而是自洽的逻辑和证明，这种思路为我们提供了很大帮助。

== 调度状态机

Anemone 使用 `TaskSchedState` 记录一个 `Task` 的内部调度状态。它的顶层状态只有三类：Runnable、Waiting、Zombie。等待态内部再记录等待轮次、是否允许被信号打断，以及当前是尚未真正 park 的 `PrePark`，还是已经从运行队列上脱离的 `Parked`。

#code-block(
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

  pub enum ParkState {
      PrePark,
      Parked,
  }
  ```.text,
  caption: [`TaskSchedState` 保存调度器内部状态，`ParkState` 区分 wait 发布后到真正 park 之间的窗口],
  lang: "rust",
)

这个状态机的核心作用是把*逻辑完成*和*物理入队*分开。一个 task 开始等待时，状态先从 `Runnable` 变成 `Waiting { park: PrePark }`。如果在它真正调用调度器、准备从 CPU 上切走之前，等待条件已经被其它 CPU 或中断路径满足，那么唤醒方只需要把状态改回 `Runnable`，后续调度器会发现本轮等待已经完成，并把它作为普通可运行 task 继续处理。如果 task 已经进入 `Parked`，唤醒方则在完成逻辑唤醒后再把它放回目标 CPU 的运行队列。

我们之所以要特地引入 `PrePark`，是因为阻塞路径必须先把“我正在等什么”发布出去，事件源才有机会唤醒它；但发布等待状态和真正切出 CPU 之间一定存在窗口。如果把这两个动作当成一个不可分状态，早到的 wakeup 很容易变成旧轮次唤醒、重复入队或 lost wake。Anemone 让等待轮次拥有独立身份，旧 token 只能完成它所属的那一轮等待，不能误伤下一轮等待。实际上，早期开发过程中，这样的竞态就曾经导致过一个难以复现的 bug：一个 task 在等待信号时被唤醒，但它还没有真正 park，结果在 wakeup 之后又被调度器再次 park，导致它永远无法被唤醒。

如前所述，Anemone最后还是得考虑兼容Linux的语义。比如`/proc/<pid>/stat`、`/proc/<pid>/status`，它们都有稳定的Linux UAPI。 所以我们使用观察性投影：`Runnable` 映射为 running，`Waiting` 根据 `interruptible` 映射为 interruptible 或 uninterruptible sleep，`Zombie` 映射为 zombie。这个投影可以近似 Linux 的 `TASK_RUNNING`、`TASK_INTERRUPTIBLE`、`TASK_UNINTERRUPTIBLE` 和 `TASK_ZOMBIE`。

== 调度循环

Anemone没有像Linux那样，不存在一个中间的调度上下文。这的确牺牲了部分性能，但是我们认为这是值得的——我们换来了更清晰的边界。

#report-figure(
  image("../assets/central-scheduler.png", width: 100%),
  caption: [来自rCore，一致的中心化调度上下文设计],
)

在Anemone，每个 CPU 都有自己的调度器上下文、当前运行 task 和本地运行队列。调度循环从本地运行队列选出下一个 task，切换地址空间与内核执行上下文。

时钟中断负责推进抢占请求。每次 tick 到来时，本地调度类可以返回 `Resched`，处理器状态中的 `need_resched` 被置位。用户态 trap 返回前会检查该标志；如果需要重新调度，就在返回用户态之前进入调度器。

*Anemone支持内核抢占*。它由 `kernel_preempt` 配置控制：当该配置打开且当前 CPU 允许抢占时，内核 trap 处理路径也可以在合适位置执行调度。临界区通过关中断或 `PreemptGuard` 禁止这类抢占，从而保护 per-CPU 数据和短临界区。

与大部分往届内核不太一样的是，我们没有在时钟中断上下文就立即schedule，而是把“请求重新调度”和“立刻切换上下文”分开。tick、中断和远程唤醒可以只设置调度请求；真正切换发生在 trap 返回、显式 `yield`、阻塞等待或调度器安全点。这样既避免在任意锁持有位置强行切换，又能让长时间运行的用户态程序和可抢占内核路径及时让出 CPU。

== 多调度类共存

Anemone 的运行队列借鉴 Linux sched class 的分层思路：每个 task 持有一个调度实体，调度实体记录它属于哪个调度类；每个 CPU 的运行队列按调度类组织 ready task。调度器挑选下一个 task 时，先从普通调度类取 task，若没有可运行 task，再落到 idle task。

#code-block(
  ```rust
  pub trait Scheduler: Send + Sync {
      fn enqueue(&mut self, task: Arc<Task>);
      fn dequeue(&mut self, task: &Arc<Task>) -> bool;
      fn pick_next(&mut self) -> Option<Arc<Task>>;
      fn on_tick(&mut self, cur_task: &Arc<Task>) -> Option<OnTickAction>;
  }

  pub struct RunQueue {
      ntasks: usize,
      rr: RoundRobin,
      idle: Idle,
  }

  pub enum SchedClassPrv {
      RoundRobin(()),
      Idle(()),
  }
  ```.text,
  caption: [调度类接口、每 CPU 运行队列和当前已落地的调度类],
  lang: "rust",
)

目前我们已经实现了`RoundRobin` 和 `Idle` 两个调度类。`RoundRobin` 是简单的FIFO，而`Idle` 是兜底调度类；当普通运行队列为空时，本地 idle task 被选中执行。

这里值得一提的是，idle 循环也必须检查 `need_resched`，这样即使内核抢占关闭，也能在有新 task 变为 ready 后回到调度器。否则内核会整个死在idle上下文，不再让出Cpu！

这个框架具备不弱的扩展能力，我们可以在后续实现 CFS、EEVDF、Deadline 或实时调度类。每个调度类只需要实现自己的 `enqueue`、`dequeue`、`pick_next` 和 `on_tick`，不需要关心其它调度类的内部逻辑。CPU 运行队列只负责按调度类顺序挑选下一个 task。

== Event 与条件等待

`Event` 是 Anemone 中最常用的条件等待原语，功能接近 Linux wait queue。它本身不传递数据，只表达“某个条件可能已经变化”。等待方注册 listener 后必须再次检查 predicate；发布方唤醒 listener 后，等待方醒来也必须重新检查 predicate。因此，Event 的 wakeup 是提示，不是条件已经永久成立的证明。

`Event` 支持可中断等待、不可中断等待、带 timeout 的等待，以及 exclusive / non-exclusive listener。non-exclusive listener 会在一次 publish 中全部尝试唤醒；exclusive listener 则按指定 quota 唤醒，用来避免某些资源只释放一个单位时把所有等待者都叫醒。`Mutex` 的慢路径就是一个代表例子：快速路径先用原子操作抢锁，失败后通过 `Event` 不可中断等待锁释放；解锁时发布一个 exclusive wakeup，让一个等待者重新竞争锁。

Event 适合“源对象拥有等待队列”的场景，例如 mutex、futex、pipe close、child exit、vfork completion 等。它不适合直接表达 `poll` / `select` 这类“一次 syscall 同时等待多个源，只要任意一个源 ready 就返回”的场景，因为后者的等待队列不属于某一个长期事件源，而属于本次 syscall。

从一定程度上来讲，`Event`其实是一个弱化版的Linux wait queue。它没有 Linux wait queue 那样的复杂性和多态性，但也因此更容易理解和使用。这是一种权衡，我们在开发过程中认为，在Anemone中，Event的简单性和可理解性更为重要。

== Latch 与 I/O 多路等待

为了解决 `poll` / `select` 的 OR 等待，Anemone 创造性地实现了一个新同步源于： `Latch`，意为闸门。 `Latch` 表示一轮的等待：等待方创建一个不可复制的 `Latch`，再派生出*多个可复制*的 `LatchTrigger` 分发给 fd source。任意 source 的第一发有效 trigger 都可以完成本轮等待；旧 trigger 迟到时，将不会发生任何效果。

最初，我们引入`Latch`的核心诉求是正确地实现 `ppoll` 和 `pselect6`，替换了早期的 busy polling。这里可以用pipe举个例子。VFS对pipe进行轮询/poll时， pipe会检查读写是否可行；如果暂时无数据，就保存对应方向的 trigger；读写端状态变化或关闭时，pipe就通过trigger唤醒正等待在latch上的task。

== 多核分配与负载均衡

Anemone 是多核内核，调度器采用“创建时分配 CPU，运行期固定 CPU”的绑核策略，这可以减少线程在不同CPU之间切换的开销，提高缓存命中率。每个 task 创建时会选择一个目标 CPU；task 之后只在这个 CPU 的本地运行队列中被调度。跨 CPU 唤醒不是迁移 task，而是向目标 CPU *发送 IPI*，让目标 CPU 把已经属于它的 task 放回自己的运行队列。

如果唤醒方CPU直接访问目标CPU的运行队列，会破坏 per-CPU 的数据一致性。使用IPI正是为了避免竞态，从而维护调度队列的一致性。

我们实现了负载均衡策略。每当新task创建时，我们会使用负载均衡算法决定将其分配到哪个CPU上运行。负载均衡算法会考虑当前各个CPU的负载情况，尽量将新task分配到负载较低的CPU上，以提高系统整体的性能和响应速度。

#report-figure(
  image("../assets/task-load-balacing.png", width: 100%),
  caption: [当前 task 创建时的 CPU 分配与本地运行队列关系。新 task 选择一个目标 CPU 后进入对应运行队列，之后等待唤醒也回到该 CPU。],
)
