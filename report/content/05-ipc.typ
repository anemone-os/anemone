= IPC

== 信号机制

信号机制涉及目标选择、pending set、signal mask、用户自定义 handler、signal frame、sigreturn 和 blocked syscall interruption。报告应按“目标选择 -> 递送时机 -> 用户态返回 -> sigreturn 恢复”说明完整路径。

== Pipe 与文件对象

pipe 是典型的文件对象与等待协议结合点。本节说明 pipe buffer、读写端生命周期、阻塞/非阻塞行为、poll/select 可见性和关闭语义。

== 共享内存与 System V IPC

共享内存同时属于 IPC 和内存管理能力域。本节只说明 IPC API、key/id/permission 和用户可见生命周期；具体页映射和 fault path 放在内存管理章节。

== Event 类对象

eventfd、timerfd、signalfd 或相邻事件对象如果已实现，应在本节说明文件接口、等待协议和 poll/select 交互；未实现的对象应明确写入限制或未来工作。
