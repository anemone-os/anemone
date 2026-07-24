# PulseOS、MyGO 与 KernelX 的 TTY 支持调查

**状态：** pre-RFC 背景材料；不覆盖 [RFC target](../index.md) 或实现计划。

调查日期：2026-07-22。

调查快照：

- PulseOS：`49a70067ca9d742f09786d4971b33394a50b6c79`
- MyGO：`921cf97c31c5bbcf1d494a8405628a6f442a498d`
- KernelX：`e2f38ac9285fd3eeb36f5e254b4f6a297adea479`

本次只做静态源码、仓库内文档、配置、截图和历史记录核查，没有构建或启动三个内核。因而本文把证据分为：

- **代码确认**：当前快照中存在完整调用路径；
- **仓库材料确认**：当前仓库包含截图、日志、rootfs 配置或测试记录；
- **项目声称**：README 或设计文档声称支持，但当前仓库没有足够运行证据；
- **未找到**：在约定范围内没有找到对应实现或证据，不等价于数学意义上的不存在。

## 1. 先区分“能跑 shell”和“有 TTY”

BusyBox `ash` 在 stdin/stdout 只是普通字节流、`isatty()` 失败或拿不到 controlling tty 时，仍可运行脚本，并能进入功能降级的交互模式。因此以下现象都不能单独证明 TTY 已实现：

- 内核成功 `exec /bin/sh`；
- 出现 shell prompt 并能执行简单命令；
- BusyBox shell 的非交互测试通过；
- rootfs 中存在 `vi` 或 `vim` 二进制。

本调查把能力拆为四层：

1. **shell 可执行**：fork/exec/read/write 足以运行 shell 或脚本；
2. **交互字节流**：控制台输入可阻塞、唤醒并供 shell 消费；
3. **TTY 数据语义**：`isatty`/termios、canonical/raw、echo、控制字符、winsize、nonblock 和 iomux 基本成立；
4. **终端 job control**：controlling tty、session、foreground process group、`TIOCGPGRP/TIOCSPGRP`、后台读写规则和终端信号成立。

KernelX 的仓库截图 `docs/static/result_basic_0.png` 是最直接的反例：它展示了可交互 BusyBox shell，同时明确打印：

```text
sh: can't access tty; job control turned off
```

这证明“能交互运行 shell”和“终端 job control 已闭合”是两件不同的事。

## 2. 总体结论

| 内核 | shell/交互 | TTY 数据语义 | UART RX | iomux | controlling tty / job control | PTY | 综合判断 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| PulseOS | 能直接启动 `/bin/sh`，有历史交互 shell 记录 | 主要是 termios/ioctl 外壳；没有真正 line discipline | timer tick 轮询 console；没有 UART IRQ 路径 | 匿名 stdin 有 wait/poll，重开 `/dev/tty*` 行为不一致 | 基本没有；全局 PGID 和启发式 Ctrl-C | 未找到 | shell 定向兼容层，不是 TTY 子系统 |
| MyGO | rootfs 明确提供交互 BusyBox shell 入口 | 三者中最完整：canonical/raw、echo、控制字符、部分 `VMIN`、winsize | RX IRQ 只唤醒；kthread 延后运行行规程，但没有软件 RX ring | poll/select 可用；epoll 和若干 readiness 语义有缺口 | 有 PGID/terminal signal 雏形，但 controlling tty 是 no-op，并有 BusyBox 定向 fallback | 明确未实现 | 最接近可用串口 TTY vertical slice，但 job control 未闭合 |
| KernelX | 有历史交互 shell 截图；README 声称可运行 vim | 有独立 `TtyState`、canonical/raw、echo、部分 termios/winsize | RX IRQ 中直接 drain 并执行 line discipline；TX polling | 串口和 PTY 都接入 poll/epoll | 基本未接入；截图明确 job control 关闭 | 已有 `/dev/ptmx` 和 `/dev/pts/N` | 数据通道和全屏交互基础较好，但 IRQ 边界和终端信号语义问题明显 |

成熟度排序不能简单写成一条总排名：

- 若只看 **串口 TTY + shell 定向兼容**，MyGO 最完整；
- 若看 **PTY 数据通道和 iomux**，KernelX 明显领先；
- 若看 **真实 controlling tty 和 shell job control**，三者都没有完成；
- PulseOS 的 shell 证据最不应被误读为 TTY 已实现。

## 3. PulseOS

### 3.1 实际结构

PulseOS 的路径是“平台 console -> 全局 stdin 队列 -> 匿名 stdio 或 devfs 别名”，没有独立 TTY core。

RISC-V console 使用 SBI DBCN 读写，读操作是非阻塞探测：

- `crates/axplat-riscv64-qemu-virt/src/console.rs:8-35`
- `crates/axplat-riscv64-qemu-virt/src/console.rs:42-90`

LoongArch64 直接轮询 `ns16550a::Uart`，TX 等待 `put()`，RX 调用 `get()`：

- `crates/axplat-loongarch64-qemu-virt/src/console.rs:9-21`
- `crates/axplat-loongarch64-qemu-virt/src/console.rs:24-67`

没有找到 UART RX/TX IRQ 注册或 ISR。输入由 timer tick 周期性调用 `poll_stdin()`：

- `pulse_core/src/task/mod.rs:163-195`

`poll_stdin()` 每次读取最多 64 字节，写入全局无界 `VecDeque` 并唤醒 wait queue：

- `pulse_core/src/fd_table.rs:275-276`
- `pulse_core/src/fd_table.rs:328-347`

### 3.2 TTY/termios 只是兼容外壳

实际输入变换只有无条件 `CR -> NL`：

- `pulse_core/src/fd_table.rs:260-267`

Ctrl-C 被硬编码截获；它不读取 `ISIG` 或可配置 `VINTR`：

- `pulse_core/src/fd_table.rs:288-315`
- `pulse_core/src/fd_table.rs:328-343`

没有 canonical 行聚合、erase/kill/EOF、echo、raw/cbreak 或 `VMIN/VTIME`。termios 虽可读写，却只是整个系统共享的一份快照：

- `pulse_core/src/fd_table.rs:363-461`

`TCSETS`、`TCSETSW` 和 `TCSETSF` 最终只是更新同一份快照，没有 drain/flush 差异；winsize 固定为 24x80：

- `pulse_core/src/fd_table.rs:510-563`

syscall 层自己把这条路径称为 `tty compatibility stub`，并提示语义被简化：

- `pulse_syscalls/src/impls/fs/control.rs:175-211`
- `pulse_syscalls/src/impls/fs/control.rs:309-395`

### 3.3 阻塞和 iomux

匿名 fd 0 的空读会等待 `STDIN_WAIT_QUEUE`，pending signal 返回 `EINTR`；它也有 poll/waker 注册：

- `pulse_core/src/fd_table.rs:573-643`

但 `O_NONBLOCK` 对匿名 stdin 没有真实作用。重新打开 `/dev/tty*` 后又走通用 `FileObject`，其 nonblock、poll 和 epoll 行为与 fd 0 不一致：

- `pulse_core/src/fd_table.rs:112-114`
- `pulse_core/src/fd_table.rs:926-950`
- `pulse_syscalls/src/impls/fs/epoll.rs:89-105`

### 3.4 job control 缺口

前台 PGID 是全系统一个 `AtomicU64`，没有 TTY、session 或权限边界：

- `pulse_core/src/fd_table.rs:278-285`
- `pulse_core/src/fd_table.rs:528-545`

若没有设置 PGID，Ctrl-C 会选择 PID 最大的非 init 进程组，是明显的 shell 兼容启发式：

- `pulse_core/src/fd_table.rs:288-313`

session 也未真实建模：`setsid()` 是成功桩，`getsid()` 退化为 PGID。没有 controlling tty、后台 read/write、`SIGTTIN/SIGTTOU`、hangup 或 orphaned process group 规则：

- `pulse_syscalls/src/impls/task/user.rs:270-273`
- `pulse_syscalls/src/impls/misc.rs:682-703`

### 3.5 shell/vi 证据

内核正常配置直接加载 `/bin/sh`：

- `src/main.rs:37-65`

devfs 创建 `/dev/tty`、`/dev/console` 和 `/dev/ttyS0`，但三者只是同一全局 console 回调的别名；`/dev/tty` 不是“解析当前进程 controlling tty”的设备：

- `arceos/modules/axfs/src/fs/devfs.rs:304-323`
- `arceos/modules/axfs/src/fs/devfs.rs:779-833`

两个 Alpine base archive 包含 BusyBox `sh` 和 BusyBox `vi`；历史记录也显示曾进入 shell prompt。但没有找到 `vi` 的实际运行记录，也没有找到 GNU vim 或 bash 的运行证据。

### 3.6 判断

PulseOS 可证明的是：它具备 shell 可执行和基础交互字节流，并用若干 ioctl 桩帮助 libc/BusyBox 继续运行。它不适合作为 Anemone TTY 状态模型、IRQ 边界或 job-control 集成的参考实现。

可借鉴的局部只有：

- 初始 stdio 安装和 devfs 节点闭环；
- stdin wait queue 和 poll 接口；
- 把 tty ioctl 限制在实际终端节点，而不是让所有字符设备假装 TTY。

## 4. MyGO

### 4.1 UART、IRQ 和延后处理

16550 UART 直接暴露原始字节和 readiness。RX IRQ 注册后只检查硬件可读并唤醒 waiters，不在硬 IRQ 中运行行规程：

- `drivers/uart16550/src/driver.rs:394-447`
- `drivers/uart16550/src/driver.rs:540-547`
- `drivers/uart16550/src/driver.rs:573-603`

timer/IRQ hook 只设置原子请求位；低优先级 kthread 在进程上下文调用 `poll_tty_input()`，并以 10ms timeout 兜底：

- `kernel/src/tty_poll.rs:14-40`
- `kernel/src/tty_poll.rs:70-83`

这条“IRQ top half 不运行 line discipline”的方向是三者中最接近 Anemone 所需边界的。但它仍有明显缺口：RX 没有软件 ring，字节继续留在 16-byte 硬件 FIFO，IRQ 只负责唤醒；高输入速率下缺少溢出和线路错误统计。

TX 有 32 KiB 软件缓冲，但靠 write/flush/poll 主动 kick，没有 TX-empty IRQ：

- `drivers/uart16550/src/driver.rs:55-67`
- `drivers/uart16550/src/driver.rs:369-389`
- `drivers/uart16550/src/driver.rs:449-470`

### 4.2 真实但 shell 定向的 line discipline

TTY 状态按底层设备共享，`/dev/console` 和 `/dev/uartN` 的多个 open fd 可共享 termios、winsize、foreground PGID 和行缓冲：

- `general/src/vfs/devtmpfs.rs:675-711`
- `general/src/vfs/devtmpfs.rs:745-763`

canonical 模式实现了 ICRNL、IXON、ISIG、VERASE、VKILL、VEOF、换行提交、ECHO/ECHOE/ECHOK；也有 raw/noncanonical 路径和 OPOST/ONLCR：

- `general/src/vfs/devtmpfs.rs:814-831`
- `general/src/vfs/devtmpfs.rs:958-1085`

但其中存在明确的 BusyBox 定向兼容桥：异步 pump 即使发现 `ISIG` 关闭，也会强制把 VINTR/VQUIT/VSUSP 解释为信号，避免 shell 启动前台程序后 Ctrl-C 滞留：

- `general/src/vfs/devtmpfs.rs:925-955`

这个 fallback 会破坏真实 raw-mode 语义，可能直接影响 vi/vim 等需要精确控制字符行为的程序。

### 4.3 termios、blocking 和 iomux

TTY ioctl 覆盖面在三者中最广，包括 termios/termios2、winsize、foreground PGID、SID、队列长度、flush/drain/break 和 line discipline 0：

- `general/src/vfs/user_api/tty.rs:18-46`
- `general/src/vfs/user_api/tty.rs:321-450`

真正影响数据路径的仍只是部分常见 flags。`VMIN` 被强制至少为 1，`VTIME` 没有实际 deadline，因此 noncanonical read 的四种 POSIX 组合没有闭合：

- `general/src/vfs/devtmpfs.rs:1118-1144`

blocking/O_NONBLOCK 和 poll/select 有完整竖切：

- `general/src/vfs/devtmpfs.rs:1190-1265`
- `kernel/src/syscalls/fs.rs:3588-3600`
- `kernel/src/syscalls/fs.rs:3845-3868`

但 canonical 模式可能因硬件 FIFO 有半行字节而提前报告 `POLLIN`；`POLLOUT`、nonblocking TX、`FIONREAD` 和 epoll 也存在语义缺口。TTY 虽声称 `is_epollable=true`，却没有对应 poll source：

- `general/src/vfs/devtmpfs.rs:1230-1269`
- `libs/vfs/src/epoll.rs:209-214`

### 4.4 job control 仍未闭合

MyGO 已有真实 ProcessGroup、Session、`setpgid/setsid` 和按进程组发信号的基础：

- `libs/sched/src/group.rs:206-284`
- `libs/sched/src/operation.rs:100-211`

TTY 能把 VINTR/VQUIT/VSUSP 变成 `SIGINT/SIGQUIT/SIGTSTP` 并发给记录的 PGID：

- `general/src/vfs/user_api/tty.rs:198-210`
- `general/src/vfs/devtmpfs.rs:867-923`

但 `Session` 明确还没有 controlling-terminal 字段；`TIOCSCTTY/TIOCNOTTY` 无条件成功，`O_NOCTTY` 也没有进入控制终端逻辑：

- `libs/sched/src/group.rs:276-284`
- `general/src/vfs/user_api/tty.rs:451-452`
- `kernel/src/syscalls/fs.rs:3421-3439`

`TIOCSPGRP` 只校验正数，不校验 PGID 存在、同 session 或权限。若 shell 没有正确设置 foreground PGID，TTY 会记住最近 reader，并可能同时给 stored PGID 和 current reader PGID 补发信号：

- `general/src/vfs/devtmpfs.rs:785-811`
- `general/src/vfs/devtmpfs.rs:867-891`

没有找到后台 read/write 的 `SIGTTIN/SIGTTOU`、TOSTOP、terminal hangup、session leader exit 或 orphaned process group 规则。

### 4.5 shell/vi/PTY 证据

RISC-V 和 LoongArch rootfs 都提供 Ctrl-C 后进入 `exec /bin/sh -i` 的路径，并在 inittab 中配置 console shell：

- `userland/rootfs-rv/etc/init.d/rcS:39-49`
- `userland/rootfs-rv/etc/inittab:1-5`
- `userland/rootfs-la/etc/init.d/rcS:30-40`
- `userland/rootfs-la/etc/inittab:1-5`

仓库测试记录能确认 BusyBox `ash -c exit` 和 `sh -c exit`，但这是非交互证据。BusyBox 1.36.1 的默认配置会构建 ash、line editing 和 vi，然而没有找到 vi 的实际启动、编辑和退出日志，也没有 GNU vim 证据。

PTY 被明确列为 missing functionality：

- `userland/rootfs-la/etc/ltp-skip.tsv:13-17`

### 4.6 判断

MyGO 是三者中最值得研究的串口 TTY 样本，特别是：

- UART 只暴露原始字节、readiness、waiter 和 typed control；
- line discipline 在上层共享 TTY object 中拥有；
- IRQ top half 只通知，kthread 在进程上下文推进；
- 同一 port 的 console 别名共享一份 TTY 状态；
- TTY 使用专属 FileOps 处理 ioctl、blocking/nonblocking 和 poll。

不应照搬的部分是：

- RX 不进入固定容量软件 ring；
- TTY 大量逻辑直接堆在 `devtmpfs.rs`；
- 用 foreground-PGID fallback 和双重投递弥补 controlling tty 缺失；
- raw mode 强制信号化；
- `VMIN/VTIME`、epoll 和若干 readiness/queue ioctl 语义不完整。

## 5. KernelX

### 5.1 TTY core、串口和 IRQ

KernelX 有独立、可同时复用于串口和 PTY 的 `TtyState`。它持有 attr、4096-byte input ring、1024-byte canonical line buffer 和 winsize：

- `kernelx/src/driver/char/tty.rs:12-13`
- `kernelx/src/driver/char/tty.rs:168-182`

canonical 模式实现 DEL erase、Ctrl-D EOF、换行提交和基本 echo；关闭 `ICANON` 后字节立即进入 input ring；输出实现 OPOST/OCRNL/ONLCR：

- `kernelx/src/driver/char/tty.rs:234-336`

NS16550 初始化只打开 RX IRQ，TX 仍是寄存器 polling：

- `kernelx/src/driver/char/serial/ns16550a.rs:71-110`
- `kernelx/src/driver/char/serial/ns16550a.rs:153-161`

最大的结构问题是：`Stty::handle_interrupt` 在硬 IRQ 中直接 drain UART、运行 line discipline、echo、投递信号、唤醒 waiters 并通知 epoll：

- `kernelx/src/driver/char/serial/stty.rs:53-69`

这与 Anemone 所需的 IRQ 边界相反。它还在 echo 时只尝试一次 `putchar` 并忽略失败；ring 满时覆盖旧数据且没有 overflow/error counter：

- `kernelx/src/driver/char/serial/stty.rs:56-59`
- `kernelx/src/klib/ring.rs:20-27`

阻塞 read、O_NONBLOCK、poll waiter 和 epoll notifier 已接通：

- `kernelx/src/driver/char/serial/stty.rs:82-139`

但 read 的“检查为空 -> 注册 waiter”横跨不同锁，存在 missed-wakeup 窗口。

### 5.2 termios 是最小子集

KernelX 支持 `TCGETS/TCSETS*`、`TCGETA`、termios2 和 winsize：

- `kernelx/src/driver/char/tty.rs:359-430`

真正保存和执行的 flags 很少。`ISIG`、`ECHONL` 和 `IEXTEN` 虽定义，却没有进入有效 `TtyAttr`；用户设置的 control characters 基本不保存，`Termios2` 的速度和控制字符也会丢失：

- `kernelx/src/driver/char/tty.rs:42-51`
- `kernelx/src/driver/char/tty.rs:189-199`
- `kernelx/src/driver/char/tty.rs:433-494`

没有 `VMIN/VTIME`，`TCSETSW` 没有 drain，`TIOCSWINSZ` 不生成 `SIGWINCH`。

### 5.3 terminal signal 语义存在严重偏差

KernelX 已有 PCB 级 PGID/SID syscall 和 stop signal 基础，但没有把它们接到 TTY：

- `kernelx/src/kernel/task/pcb/identity.rs:25-43`
- `kernelx/src/kernel/syscall/task.rs:47-134`
- `kernelx/src/kernel/ipc/signal/signalnum.rs:38-40`

`TtyState` 没有 session、controlling owner 或 foreground PGID，ioctl 也没有 `TIOCSCTTY/TIOCNOTTY/TIOCGPGRP/TIOCSPGRP`：

- `kernelx/src/driver/char/tty.rs:168-173`
- `kernelx/src/driver/char/tty.rs:359-371`

Ctrl-C 路径尤其值得警惕：line discipline 只在 canonical 模式把 `0x03` 变成 `Interrupt`，串口 IRQ 随后向“中断发生时当前正在 CPU 上运行的 PCB”发送 `SIGQUIT`，而不是向 foreground process group 发送 `SIGINT`：

- `kernelx/src/driver/char/tty.rs:275-283`
- `kernelx/src/driver/char/serial/stty.rs:53-62`

PTY master 写入时又直接丢弃这个 event，不生成任何信号：

- `kernelx/src/fs/devfs/inode/pty/inner.rs:144-167`

因此不能因为 KernelX 已有 PGID/SID syscall 和 stop signals，就认为 terminal job control 已实现。

### 5.4 PTY

KernelX 固定创建 `/dev/ptmx` 和 `/dev/pts`；每次打开 ptmx 动态分配 `/dev/pts/N`：

- `kernelx/src/fs/devfs/superblock.rs:58-74`
- `kernelx/src/fs/devfs/inode/pty/node.rs:70-84`

PTY master/slave 有 blocking/nonblocking、poll、hangup 和 epoll notifier：

- `kernelx/src/fs/devfs/inode/pty/inner.rs:188-296`
- `kernelx/src/fs/devfs/inode/pty/file.rs:122-147`
- `kernelx/src/fs/devfs/inode/pty/file.rs:243-268`

这是一套有实际价值的数据通道 vertical slice，但它和串口一样没有 controlling-terminal/job-control 语义，且 slave read 也存在相似的 waiter 注册竞态。

### 5.5 shell/vim 证据

init 会打开 bootarg `tty=` 指定的字符设备，并把同一 file 安装为 fd 0/1/2：

- `kernelx/src/kernel/main.rs:51-56`
- `kernelx/src/kernel/task/tcb.rs:308-325`

默认 init tty 是 `/dev/serial@10000000`；文档给出 `/bin/busybox` 加 `initargs=sh` 的启动方式：

- `kernelx/src/kernel/config.rs:63-74`
- `docs/latex/sections/quick-start.tex:36-52`

README 声称支持 BusyBox shell 和 vim：

- `README.md:15-22`
- `kernelx/README.md:3`

仓库内历史截图 `docs/static/result_basic_0.png` 确认本机 BusyBox shell 能交互运行，同时明确显示 job control 已关闭。没有找到启动或操作 vim/vi 的截图、运行日志、测试脚本或跟踪的 guest 二进制；因此 vim 只能记为项目声称，不能记为本次确认。

另外，截图显示 KernelX 5.0.0，而当前比赛构建脚本配置 6.0，不能把截图当作当前 commit 的精确复现日志：

- `build-kernel.sh:6-10`

### 5.6 判断

KernelX 值得借鉴的部分是：

- 用窄 `SerialOps` 把 UART 接到共享 TTY core；
- TTY core 同时服务 serial 和 PTY；
- char file 到 blocking/O_NONBLOCK/poll/epoll 的完整竖切；
- init fd 0/1/2 绑定选定 terminal port 的简单闭环。

不应照搬的部分是：

- 在硬 IRQ 中运行 line discipline、echo、signal 和 wake policy；
- overwrite-on-full 且无统计；
- waiter 注册竞态；
- Ctrl-C 发给 current PCB 的 SIGQUIT；
- termios ABI 外形与实际状态严重脱节；
- 把 PTY 数据通道误当成 controlling tty/job control 已完成。

## 6. 对 Anemone 讨论最有价值的结论

### 6.1 三个项目共同证明了什么

1. **跑起 BusyBox shell 的门槛远低于真实 TTY。** PulseOS 和 KernelX 都能提供直接证据；KernelX 甚至同时展示 prompt 和 “job control turned off”。
2. **vi/vim 的最低门槛主要在 raw-ish input、escape-sequence output、winsize、阻塞/nonblock 和 iomux。** 它们不要求第一天就有完整 terminal job control，但错误的 `ISIG`/control-character 语义会制造难以诊断的问题。
3. **已有 process group/session syscall 不等于 TTY job control。** 三个内核都在这条集成边界上缺失或使用 fallback。
4. **PTY 也不等于 controlling tty。** KernelX 已有不错的 PTY 数据通道，却仍然没有 shell job control。

### 6.2 最值得借鉴的组合，而不是单个实现

三个项目中不存在可以整体照搬的实现。更合理的参考组合是：

- 采用 MyGO 的大方向：UART 只提供原始字节/readiness/control，line discipline 位于上层共享 TTY object，硬 IRQ 不运行 TTY policy；
- 补上三者都缺的部分：硬 IRQ 必须把 RX 字节 drain 到固定容量软件 ring，记录 overflow 和 line errors，再唤醒 deferred processing；
- 采用 KernelX 的复用方向：serial 和未来 PTY 可以共享 TTY core，但不能继承其硬 IRQ 执行 line discipline 的做法；
- 采用专属 TTY FileOps，统一 blocking/O_NONBLOCK/poll/ioctl，而不是让通用 CharDev 或匿名 boot stdio 各自形成不同语义；
- 直接利用 Anemone 已有 unix-jobctl 的 ThreadGroup/ProcessGroup/Session 基础实现真实 controlling tty 和 foreground PGID，不引入“最新进程”“最近 reader”“双重补发”等启发式真相源。

### 6.3 后续验证必须分层

后续 Anemone 的验收不应只写“能跑 shell”或“能跑 vim”，至少应分别记录：

- BusyBox `ash` 脚本模式；
- BusyBox `ash` 交互模式，以及是否出现 `can't access tty` / `job control turned off`；
- `stty`/`TCGETS`、canonical/raw、echo、Ctrl-C/Ctrl-Z、Ctrl-D、erase；
- O_NONBLOCK、poll/select/epoll；
- BusyBox `vi` 的启动、插入、移动、保存和退出；
- controlling tty、`tcsetpgrp`、`fg/bg/jobs`、后台 read 的 `SIGTTIN` 与 TOSTOP 写路径；
- PTY 作为独立后续能力，而不是串口 TTY 第一阶段的隐含前置条件。

## 7. 调查限制

- 没有运行三个内核，运行能力只按当前仓库内证据分级；
- README、历史截图和历史日志可能早于当前 commit；
- 外部测试盘或未跟踪 rootfs 中可能含有本文未观察到的 vim/vi 二进制；
- “未找到”只表示本次静态调查没有发现，不应替代后续针对性运行验证。
