# Anemone TTY 当前实现边界

**状态：** 归档调查；不覆盖 [RFC target](../index.md) 或 current contract。

TTY core 本身可以从干净模型开始，但端到端并不独立。现有 unix-jobctl 是很好的基础；真正的历史包袱集中在 bootstrap
console、通用 CharDev、VFS open、UART IRQ 和 session 拓扑的交界处。

当前没有发现 Apollyon 级问题，但有几个必须先处理的 Keter 级边界。

### 现状判断

1. Keter：现在没有可用的交互输入链路。

- 启动 stdio 是匿名 console；stdin 永久返回 EOF，stdout 还要求数据是合法 UTF-8：anemone-kernel/src/device/
  console.rs:168、anemone-kernel/src/main.rs:131。

- NS16550A 的 CharDev::read() 是 unimplemented!()，TX 是轮询忙等：anemone-kernel/src/driver/serial/ns16550a.rs:160。
- UART 确实已经申请 IRQ、打开 RX interrupt，但 handler 只是读取并丢弃所有 RX 字节：anemone-kernel/src/driver/serial/
  ns16550a.rs:285。

- UART 注册成了字符设备，却没有发布为 /dev/ttyS0 一类 devfs 节点：anemone-kernel/src/driver/serial/ns16550a.rs:504。

所以当前的“UART 中断 I/O”准确说只是 IRQ plumbing 和清中断，不是可消费的中断输入。

2. Keter：TTY 不适合直接塞进现有通用 CharDev。

现有字符设备抽象：

- read/write 看不到 FileIoCtx，所以无法自然处理 O_NONBLOCK。
- ioctl 上下文刻意不暴露 caller/session。
- 通用 char devfs 的 poll 固定 NYI。
- 每次 open 都使用 NilOpaque，没有 TTY-specific opened-file 状态。

见 anemone-kernel/src/device/char/mod.rs:28 和 anemone-kernel/src/device/char/devfs.rs:20。

TTY 应拥有专用 FileOps/devfs open 实现；UART 驱动只实现窄的 TtyPort 式字节传输能力。不要为了 TTY 把所有字符设备都扩展
成能观察 Task/session 的宽接口。

3. Keter：terminal job control 是明确保留的跨子系统缺口。

当前 jobctl 已经具备：

- ThreadGroup 唯一 stop/continue truth；
- SIGTSTP/SIGTTIN/SIGTTOU default-stop 路径；
- process-group signal selector；
- stopped/continued wait report 和 SIGCHLD。

但 contract 明确不包含 controlling TTY、foreground pgrp、terminal-generated signals 和 orphan policy：docs/src/
contracts/task/job-control.md:1。

目前 SessionInner 只有 process-group 集合，没有 controlling TTY；ProcessGroup 也没有 terminal/orphan 状态：anemone-
kernel/src/task/mod.rs:213。

好消息是 TTY 不需要再发明 stop 状态机。它应只负责：

- 验证 foreground/background；
- 向目标 ProcessGroup 生成 SIGINT/SIGQUIT/SIGTSTP/SIGTTIN/SIGTTOU/SIGHUP；
- 让现有 Signal → ThreadGroup jobctl 路径完成真正 stop/continue。

4. Keter：UART、TTY 输出和 printk 的共享端口所有权尚未定义。

console::output() 持有全局 IRQ-save console 锁调用后端，UART console 和字符设备写又分别直接访问同一 MMIO，没有统一 TX
serialization：anemone-kernel/src/device/console.rs:113。

UART IRQ handler 还会对每个 RX IRQ 执行 kdebugln!()，这会重新走同一个 UART console：anemone-kernel/src/driver/serial/
ns16550a.rs:556。实际接入键盘输入前必须消除这个 IRQ→printk→UART 的递归/长临界区风险。
