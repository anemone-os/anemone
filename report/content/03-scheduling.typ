= 调度

== 调度协议

Anemone使用TaskSchedState来表示一个Task的调度状态。这里，与大部分往届作品不同的是，我们定义了高度层次化的状态机。TaskSchedState分为Runnable，Waiting，Zombie三态，而Waiting内部又分为PrePark和Parked两个子状态，同时它也记录了当前Waiting是否允许被信号中断。

[需要代码块] TaskSchedState的定义和状态转换图。

为了兼容用户态的视角，我们引入了一个投影：TaskState，它可以通过TaskSchedState的状态和信号中断标志来计算得出。用户态看到的TaskState近似就是Linux的TASK_RUNNING、TASK_INTERRUPTIBLE、TASK_UNINTERRUPTIBLE和TASK_ZOMBIE等状态。

== 多种调度策略共存

Anemone参考了Linux的sched类的设计，从架构上支持不同task可以使用不同的调度策略。

[调度类 trait 的代码]

目前，我们已经实现了FIFO和Idle两种调度策略。FIFO策略是最简单的，它按照任务的优先级顺序调度任务，优先级高的任务会先被调度。Idle策略则是为系统空闲时提供的调度策略，当没有其他任务需要运行时，Idle任务会被调度执行。

这个灵活的架构允许我们此后扩展到更多的调度策略，比如EEVDF，CFS，Deadline等。每种调度策略都可以有自己的调度算法和优先级计算方式，从而满足不同应用场景的需求。

== 同步原语

Anemone提供了多种同步原语来支持任务之间的协作和资源共享。有一些是基于自旋的，比如SpinLock，RwLock，它们适用于短时间的临界区保护。还有一些是基于*调度阻塞*，比如Mutex。

这里，我们创造性地引入了两种新的同步原语：Event和Latch。

Event是一种用于任务间事件通知的机制，它允许一个任务等待某个事件的发生，并在事件发生时被唤醒。

Latch是OR门型的同步原语，它仅仅能够提供一次等待，这一次等待允许一个task同时侦听多个事件的发生。Latch的设计灵感来源于硬件中的锁存器，它在软件中实现了类似的功能，允许任务在等待多个条件时，只要其中一个条件满足，就可以继续执行。Anemone的poll，select等IO Multi-plexing的实现都依赖于Latch来实现多路事件等待。

== 负载均衡

Anemone是一个多核系统，它支持多核之间的负载均衡。当前，我们默认一个线程只能运行在一个固定CPU上，这是一种绑核策略，它可以减少线程在不同CPU之间切换的开销，提高缓存命中率。

同时，每当新task创建时，我们会使用负载均衡算法决定将其分配到哪个CPU上运行。负载均衡算法会考虑当前各个CPU的负载情况，尽量将新task分配到负载较低的CPU上，以提高系统整体的性能和响应速度。

[负载均衡的图片]
