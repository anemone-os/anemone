#import "../components/figure.typ": code-block

= 时间

Anemone 的时间子系统同时服务调度器、超时等待和用户态时间接口。我们既要维护不会倒退的单调时间，也要给 shell、编译工具和标准库提供真实日历时间；在此之上，软定时器、POSIX timer、timerfd 和 itimer 又以不同方式把未来事件交付给内核对象或用户程序。

== 时间线与 tick

Anemone 把架构相关的计时能力抽象成 clock source 和 clock event。clock source 负责读取单调递增的硬件计数，clock event 负责把下一次 timer interrupt 编程到指定 deadline。

内核启动时会把各 CPU 的硬件计数投影到同一条单调时间线上。`MonotonicInstant` 表示自系统启动以来不会因校时而倒退的时间，适合调度、timeout 和性能计时；`RealtimeInstant` 则在单调时间上叠加日历偏移，供用户态观察现实时间。我们用不同类型区分它们，避免把可调整的 realtime deadline 误当作稳定的单调 deadline。

系统启动时，RTC 驱动读取一次硬件日历，为 realtime 建立初值。QEMU 平台可以使用 Goldfish RTC，Loongson 2K1000 则使用 LS7A RTC provider。运行过程中，`clock_settime` 与 `clock_adjtime` 调整的是 timekeeper 中的 realtime 映射，不会破坏 monotonic 的连续性。

周期 tick 由 timer interrupt 推进。每次中断到来时，timekeeper 更新全局 tick 计数，并重新编程下一次中断。调度器、软定时器和若干用户可见时间接口都建立在这条单调时间线上。

== 软定时器

软定时器负责在未来某个时刻执行一次回调。Anemone 的 timer core 使用按 deadline 排序的 per-CPU 队列保存请求；timer interrupt 到来后，内核取出已经到期的请求，再根据事件选择的路径执行或投递回调。请求可以被替换或取消，因此文件关闭、重新设定 timer 和进程退出不必等待一个已经失效的旧回调继续修改对象。

#code-block(
  ```rust
  enum TimerLane {
      Irq(Box<dyn FnOnce() + Send + 'static>),
      Threaded(Box<dyn FnOnce() + Send + 'static>),
  }

  pub unsafe fn schedule_local_irq_timer_event(
      expire: Duration,
      callback: Box<dyn FnOnce() + Send + 'static>,
  );

  pub fn schedule_threaded_timer_event(
      expire: Duration,
      callback: Box<dyn FnOnce() + Send + 'static>,
  );
  ```.text,
  caption: [软定时器事件显式区分 `IRQ` 和 `Threaded` 两种回调路径],
  lang: "rust",
)

我们明确区分了IRQ上下文的callback和线程上下文的callback，允许消费者更加灵活地选择。

=== IRQ

`IRQ` 路径用于真正适合在 timer interrupt 中完成的短回调。到期事件从本 CPU timer queue 中弹出后，回调直接在中断上下文运行。等待超时这类路径需要和调度器 wait identity 竞争同一轮唤醒，因此继续保留在 `IRQ` 路径中，由等待核心负责区分事件唤醒、信号、强制唤醒和 timeout。

这一路径的好处是延迟短、路径直接，但它要求调用者非常清楚中断上下文的约束。Anemone 因此没有把所有 timer callback 都统一塞进这条路径，而是让每个调用点按自己的锁、生命周期和外部可见行为选择合适的完成位置。同时，IRQ路径极易死锁或者竞态，这也是我们将接口标记为`unsafe`的缘故。

=== Threaded

`Threaded` 路径把到期检测和回调执行分开。时钟中断判断 deadline 是否已经到期，到期后只把回调投递到本 CPU 的 timer worker 队列，并唤醒对应内核线程；真正的对象状态推进、等待者唤醒和信号通知在内核线程上下文中完成。

这个拆分主要服务于对象定时器。`timerfd` 到期时需要推进 expiration counter、唤醒阻塞读者和 poll/select 等待者；进程定时器到期时需要更新线程组内的定时状态，并通过信号机制通知用户态。把这些动作从 timer interrupt 中移出后，对象可以在自己的锁下维护状态，再在合适的位置触发文件对象或信号路径。

== POSIX Clock

Anemone 为用户态暴露了 POSIX 风格的 clock 框架。每个 clock 对象提供时间分辨率和当前时间查询；系统调用层根据用户传入的 clock id 找到对应对象，再把纳秒时间转换成 Linux ABI 使用的 `timespec` 形式写回用户空间。

#code-block(
  ```rust
  pub trait Clock: Sync {
      fn resolution_ns(&self) -> u64 {
          1
      }

      fn now_ns(&self) -> u64;
  }
  ```.text,
  caption: [`Clock` trait 统一了用户可见 clock 的查询接口],
  lang: "rust",
)

这套框架覆盖常用的实时钟、单调钟、粗粒度 clock，以及进程和线程 CPU 时间等 clock id。`clock_gettime`、`clock_getres`、`clock_settime`、`clock_adjtime` 和 `clock_nanosleep` 复用统一的时间来源。相对休眠与绝对 deadline 都能按指定 clock 解释，因此 libc 和应用程序不需要依赖内核私有接口。

== 用户可见定时对象

在 clock 和软定时器之上，Anemone 实现了 POSIX timer、`timerfd` 和 `itimer`。POSIX timer 支持创建、查询、重新设定和删除，既可以向进程发送定时信号，也可以按 Linux ABI 选择目标线程；周期 timer 会统计错过的到期次数。`timerfd` 则以匿名文件描述符暴露定时事件，支持读取到期次数、非阻塞访问和 poll/epoll 可读事件，适合被事件循环统一管理。`itimer` 沿用传统进程定时器接口，继续通过信号通知线程组。

它们都没有把自己的语义下沉到时间系统的基础设施层。软定时器只负责在合适的时间投递回调；文件对象的可读状态、到期次数、周期重排，以及进程定时器的 signal 投递和重新计时，都由各自对象在自己的状态锁下维护。这样，我们的时间子系统既能支撑 Linux 兼容接口，又保持了和 VFS、信号、调度等待路径之间清晰的职责分工。
