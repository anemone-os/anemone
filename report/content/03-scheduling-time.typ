= 调度与时间

== 调度器

Anemone使用TaskSchedState来表示一个Task的调度状态。这里，与大部分往届作品不同的是，我们定义了高度层次化的状态机。TaskSchedState分为Runnable，Waiting，Zombie三态，而Waiting内部又分为PrePark和Parked两个子状态，同时它也记录了当前Waiting是否允许被信号中断。

[需要代码块] TaskSchedState的定义和状态转换图。

为了兼容用户态的视角，我们引入了一个投影：TaskState，它可以通过TaskSchedState的状态和信号中断标志来计算得出。用户态看到的TaskState近似就是Linux的TASK_RUNNING、TASK_INTERRUPTIBLE、TASK_UNINTERRUPTIBLE和TASK_ZOMBIE等状态。


== 等待与唤醒协议

等待路径是 Anemone 的关键工程边界。wait-core 负责阻塞协议和 wait identity，事件源只发布唤醒能力；timeout、signal interruption、poll/select OR wait 都应回到统一的完成分类。这里建议放一张 sleep / wakeup 流程图和一段核心接口代码。

== 时间触发

timer、itimer、timerfd 等时间对象在本报告中作为等待协议和文件对象的代表路径展开。正文应解释时间触发如何唤醒等待者，以及为什么 timer 不直接成为任务阻塞状态的 owner。

== 调度可观测性与验证

本节整理 sleep、timeout、信号打断、pipe blocking、poll/select 等路径的验证证据。正式正文应从 LTP、libcbench、本地日志或 devlog 中选择少量代表问题，而不是把调度章节写成完整调试流水账。
