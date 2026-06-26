= 时间

时间子系统在 Anemone 中不只是调度器的附属功能。它同时承担硬件时钟抽象、内核单调时间线、用户态 clock API、sleep timeout 和文件对象定时通知等职责。因此，本章把时间从调度章节中拆出，单独说明它和调度、IPC、文件对象以及架构层之间的边界。

== 时钟与时间线

Anemone 的时间线以架构层提供的本地单调计数器为基础。架构层只暴露“读取当前单调计数”和“按绝对单调计数重新设置下一次时钟事件”这两个能力；通用时间子系统负责把硬件计数换算成 `Duration`、维护启动基线，并向上提供 `Instant`、`clock_gettime`、`clock_getres`、`gettimeofday`、`times` 等接口。

目前 `CLOCK_MONOTONIC`、`CLOCK_REALTIME`、coarse clock、进程 CPU 时间和线程 CPU 时间都通过统一的 `Clock` 接口进入系统调用层。这里仍有阶段性限制：真实 RTC、NTP 校准、suspend/resume accounting 和 time namespace 尚未完成，因此 `CLOCK_REALTIME` 和 `CLOCK_BOOTTIME` 中的部分语义仍以单调时间线作为兼容基础。正式正文需要结合 current limitations 或 devlog 补齐这些限制的最终表述。

== Tick 与定时中断

定时中断到来时，通用时间子系统先推进 tick 计数，再按照系统频率重新编程下一次本地时钟事件。这个路径连接架构层和通用内核：具体平台负责接收 timer interrupt 和设置硬件 deadline，通用层负责维护内核可见的 tick、单调 uptime 和到期 timer event。

这个边界使第九章的架构硬件抽象层只需要解释 RISC-V 与 LoongArch 如何接入 timer interrupt，而不需要把 `nanosleep`、`timerfd` 或 `ITIMER_REAL` 的用户可见语义放进架构章节。

== 软定时器

软定时器是时间子系统和等待协议之间的关键连接。Anemone 当前的 timer core 只提供 one-shot completion，不在 timer core 内保存调用者对象身份、取消句柄、周期语义或生命周期所有权。调用者需要自己维护对象状态，并用 generation、validness 或 weak reference 过滤过期 callback。

软定时器有两条显式 lane：

- IRQ timer lane：到期 callback 在 timer interrupt context 运行，只适合短小、IRQ-safe、不睡眠、不获取普通锁的 completion。
- Threaded timer lane：timer IRQ 只负责把已到期事件投递给本 CPU 的 timer worker，callback 在 process context 中执行，但仍必须是有界的 timer completion，而不是通用后台任务。

这个拆分来自 threaded timer event RFC。第一版迁移对象限定为 `timerfd` 和 `ITIMER_REAL`：它们都需要在到期后推进对象状态、触发等待者或投递信号，不适合继续扩大 IRQ context 中的锁顺序风险。wait-core timeout 仍保留在 IRQ lane，因为它和 wait identity、signal / force / source trigger 的竞争关系绑定，迁移需要单独证明。

== 用户可见时间对象

`nanosleep` 和 `clock_nanosleep` 通过 wait-core timeout 表达“当前任务睡到超时或被信号打断”。这部分属于调度等待协议的使用者：时间子系统给出 timeout，调度层负责把任务置入可被信号打断的等待状态，并在完成时返回剩余时间或中断错误。

`timerfd` 则把时间变成文件对象。它的 fd 身份由 VFS 暴露，但到期次数、read/poll readiness、周期 rearm 和 stale callback filtering 都由 timerfd 私有状态维护。threaded timer callback 只是到期通知来源；如果 reader 或 poller 在 callback 获得 CPU 前已经观察到 timer 过期，timerfd 仍会在自身状态锁下按当前时间推进 missed-tick accounting。

`ITIMER_REAL` 把时间和信号连接起来。它的真实 timer 状态属于线程组；到期后发送 `SIGALRM`，周期 timer 在对象状态下决定是否重新安排下一次 one-shot timer event。当前实现只覆盖 real itimer，virtual/prof itimer、POSIX timer 和更完整的取消/drain 语义不属于这一阶段。
