# RFC-20260803-clock-timekeeping-posix-timers

**状态：** Closed
**修订：** R0
**负责人：** doruche, Codex
**最后更新：** 2026-08-04
**领域：** time / timer / task / signal / syscall ABI / RTC
**影响契约：** Gate 1--3 已 Introduce `TIMEKEEPER-CLOCK-001`、`SOFT-TIMER-REQUEST-001` 与
`TIMEKEEPER-STEP-001`；Gate 5 已 Introduce `POSIX-TIMER-001` 并 Refine `SIGNAL-PENDING-001`
**执行记录：** [2026-08-04 Clock Timekeeping 与 POSIX Timers transaction](../../devlog/transactions/2026-08-04-clock-timekeeping-posix-timers.md)

## 本 RFC 包括哪些系统调用

本 RFC 只实现 Linux 在 RV64 和 LA64 原生 64 位 ABI 上公开的 10 个调用。

时钟调用共 5 个：

| 编号 | 系统调用 | 本轮能力 |
| --- | --- | --- |
| 112 | `clock_settime` | 直接设置 `CLOCK_REALTIME` |
| 113 | `clock_gettime` | 读取指定 clock ID |
| 114 | `clock_getres` | 读取指定 clock ID 的真实分辨率 |
| 115 | `clock_nanosleep` | 按指定 clock ID 相对或绝对睡眠 |
| 266 | `clock_adjtime` | 查询时间状态，或立即增加 realtime 偏移 |

POSIX timer 调用共 5 个：

| 编号 | 系统调用 | 本轮能力 |
| --- | --- | --- |
| 107 | `timer_create` | 创建当前线程组拥有的 POSIX timer，返回 timer ID |
| 108 | `timer_gettime` | 读取剩余时间和周期 |
| 109 | `timer_getoverrun` | 读取最近一次已交付通知的超限次数 |
| 110 | `timer_settime` | 设置、替换或解除 timer |
| 111 | `timer_delete` | 删除 timer |

这些调用在 64 位架构上的结构本身已经使用 64 位秒字段，名字没有 `64` 后缀，但就是 native time64 ABI。
Linux 的 403--409 是 32 位或 compat time64 入口，不属于 RV64/LA64 native syscall 表；本 RFC 不注册这些
编号，调用时保持 `ENOSYS`。syscall 编号依据见
`xref:linux-6.6.32:include/uapi/asm-generic/unistd.h#__NR_timer_create` 和
`xref:linux-6.6.32:include/uapi/asm-generic/unistd.h#__NR_clock_gettime64`。

本 RFC 不新增 timerfd syscall。现有 `timerfd_create`、`timerfd_gettime` 和 `timerfd_settime` 是新时间管理和
soft timer 的既有消费者，改造后必须继续正确工作。

## 摘要

当前内核已经能读取硬件计数、安排周期时钟中断，也有 clock 读取骨架、soft timer、timerfd、
`ITIMER_REAL` 和 Goldfish RTC 驱动骨架。但是 `CLOCK_REALTIME` 实际返回 monotonic，coarse clock 没有快照，
`clock_getres()` 固定报告 1ns，`clock_nanosleep()` 忽略 clock ID 和 flags；soft timer 也不能删除已排队事件。
因此不能只补七个缺失 handler 就声称支持这些 ABI。

本 RFC 建立一套不依赖 RTC 持续运行的时间管理。monotonic 由统一硬件计数和 Hertz 现场计算；
`CLOCK_MONOTONIC_RAW` 首版使用同一计算，但保留独立接口；realtime 等于 monotonic 加内存中的日历偏移；
RTC 将来只在启动时读取一次，为该偏移提供初值，不在运行期读取或写回。在这套时间之上，soft timer 管理
一次未来执行请求，`ThreadGroup` 管理 POSIX timer 对象和 timer ID，signal 子系统管理 `SI_TIMER` 的 pending
与用户态交付。

## 背景与当前基线

### 当前时间源和驱动

| 平台或设备 | 当前读数来源 | 频率来源 | 当前用途与位置 |
| --- | --- | --- | --- |
| RV64 clock source/event | `time` CSR | 设备树 `/cpus/timebase-frequency` | 读取计数并通过 SBI `set_timer()` 安排中断；`anemone-kernel/src/arch/riscv64/time.rs` |
| LA64 clock source/event | `rdtime()` | 优先设备树；否则由 CPUCFG 4/5 的基础频率、倍频和分频计算 | 读取计数并写 TCFG 安排中断；`anemone-kernel/src/arch/loongarch64/time.rs` |
| 公共架构接口 | 架构实现提供 | `LocalClockSourceArch::monotonic_freq_hz()` | `anemone-kernel/src/time/hal.rs` |
| Goldfish RTC | 64 位 Unix Epoch 纳秒寄存器 | 设备自身 | 只有 platform driver 和寄存器读取函数，尚未把读数交给时间管理；`anemone-kernel/src/driver/rtc/goldfish.rs` |

当前 `timekeeper.rs` 为每个 CPU 保存一次 `BOOT_MONO`，再用 BSP 的 `BSP_BOOT_MONO` 把本 CPU 计数平移到
同一数值域。当前精确公式是：

```text
当前统一计数
= 当前 CPU 的当前硬件计数
- 当前 CPU 的 BOOT_MONO
+ BSP_BOOT_MONO
```

这个结果仍位于 BSP 的硬件计数域，并不从零开始。`Instant` 保存的就是这种硬件计数，不是纳秒；转成
`Duration` 时才按频率执行整数换算：

```text
纳秒 = 硬件计数 * 1_000_000_000 / 硬件频率
```

这套代码没有证明各 CPU 在不同启动时刻采样基准后，task 跨 CPU 迁移的读数一定连续。目标实现要求架构
clock source 先向上层提供跨 CPU 不后退的统一计数；如果平台计数天然不同步，校正属于架构 source，不能让
timekeeper 再保存一套 per-CPU 时间真相。

### 当前八个 clock

`anemone-kernel/src/time/clock/` 当前注册 clock ID 0--7：

| Clock ID | 当前实际返回 | 当前缺口 |
| --- | --- | --- |
| `CLOCK_REALTIME` | `Instant::now()` 换算值 | 不是 Unix Epoch，不能设置或调整 |
| `CLOCK_MONOTONIC` | 统一硬件计数换算值 | 启动零点和跨 CPU 连续性没有严格建立 |
| `CLOCK_PROCESS_CPUTIME_ID` | 线程组累计 CPU 计数换算值 | 已有独立含义 |
| `CLOCK_THREAD_CPUTIME_ID` | 当前线程累计 CPU 计数换算值 | 已有独立含义 |
| `CLOCK_MONOTONIC_RAW` | 直接复用 monotonic 对象 | 首版允许同值，但没有独立路由 |
| `CLOCK_REALTIME_COARSE` | 普通 monotonic 读数 | 既不是 realtime，也没有快照 |
| `CLOCK_MONOTONIC_COARSE` | 普通 monotonic 读数 | 没有快照 |
| `CLOCK_BOOTTIME` | 直接复用 monotonic 对象 | 当前没有 suspend，因此可同值，但仍缺独立路由 |

`Clock::resolution_ns()` 默认固定返回 1ns。这个值既没有根据硬件 Hertz 计算，也没有根据 coarse 更新周期
计算。当前只有 113--115 已注册；其中 `clock_nanosleep()` 把所有请求都当成 monotonic 相对等待。

CPU 时间由 `anemone-kernel/src/task/cpu_usage.rs` 管理。task 运行、切出以及用户态/内核态切换时，内核用
同一个硬件计数器累计计数差，查询时才换算成纳秒。该状态继续属于 task 和 thread group，不移入 timekeeper。

### 当前 timer 和 signal

| 机制 | 当前对象归属 | 当前工作方式 | 本 RFC 必须解决的问题 |
| --- | --- | --- | --- |
| 通用 soft timer | 每 CPU `BinaryHeap<TimerEvent>` | 100 Hz 中断弹出到期事件，IRQ callback 当场运行，threaded callback 交给 timer worker | heap 不能删除指定事件，反复重设远期 timer 会积累失效事件 |
| timerfd | 每个 timerfd 文件的 `TimerFdCore` | generation 让旧 callback 不生效，周期按原目标推进 | 旧事件仍留在 heap；`TFD_TIMER_CANCEL_ON_SET` 只有日志，没有 `ECANCELED` 语义 |
| `ITIMER_REAL` | `ThreadGroup::RealITimer` | threaded callback 通过通用 signal 发送 `SIGALRM` | 旧事件不能物理删除；周期从 callback 实际运行时间重算，会漂移 |
| CPU usage | task / thread group | 保存硬件计数差 | 只能支持读取；CPU-time sleep 和 timer 需要 scheduler 驱动，本轮不做 |

`ITIMER_REAL` 已经证明 timer 通知可以走通用 signal 路径。但当前普通非实时 signal 在
`PendingSignals` 中只有一个 slot；两个 POSIX timer 即使使用同一个普通 signal，也必须保留各自的 timer ID、
`sigval` 和 overrun，不能互相覆盖。

## 目标

- 以一个 timekeeper 管理 monotonic 零点、realtime 偏移、realtime 跳变通知和 coarse 快照。
- 八个 clock ID 都通过独立路由返回与名称一致的值，并只开放本轮明确支持的操作。
- `clock_getres()` 对普通 clock 根据硬件 Hertz 计算，对 coarse clock 根据实际快照更新间隔计算；全程只用
  整数，不使用浮点数。
- `clock_settime()` 和首版 `clock_adjtime()` 只修改 realtime 偏移，不改变 monotonic 的走速或数值。
- `clock_nanosleep()` 正确区分相对/绝对请求、clock ID、signal 中断和 remaining time。
- soft timer 能按排队句柄删除仍在队列中的请求；generation 只处理已经出队的旧处理函数。
- `ThreadGroup` 拥有 POSIX timer ID 表，实现五个 POSIX timer syscall、周期推进、overrun 和生命周期清理。
- POSIX timer 使用通用 signal 的 mask、选择、唤醒和 frame 路径，同时保留每个 timer 的独立通知身份。
- timerfd、`ITIMER_REAL`、`gettimeofday()`、文件时间和其它既有消费者改读同一套时间事实。
- 未来 RTC 只在启动时读取一次，不建设运行期轮询、alarm 或写回。

## 非目标

- 不实现 32 位 time ABI、compat syscall 或 403--409。
- 不实现 `CLOCK_TAI`、alarm clock、动态设备 clock 或 time namespace。
- 不实现 CPU-time sleep 和 CPU-time POSIX timer。
- 不实现 monotonic correction、频率修正、渐调、NTP discipline、PLL/FLL、PPS、闰秒或 TAI；
  `CLOCK_MONOTONIC_RAW` 和 `CLOCK_MONOTONIC` 首版同值。
- 不支持 `SIGEV_THREAD` 或 `SIGEV_THREAD_ID`，也不把它们静默降级成进程级 signal。
- 不实现 RTC alarm、周期读取或写回。
- 不同时改造成 tickless 或 high-resolution timer；首版仍由 100 Hz 中断发现普通到期请求。
- 不复制 Linux 的全局 timer hash、RCU 对象图或 hrtimer 内部类型；Linux 源码只用于核对 ABI 和可见语义。

## 文档地图

RFC canonical target：

- [目标与不变量](./invariants.md)：统一计数、timekeeper、Hertz 分辨率、realtime step、soft timer 删除、
  POSIX timer 生命周期和 `SI_TIMER` 的 correctness rules 与 proof obligations。
- [实施计划](./implementation.md)：Gate 0--6 的依赖、独立安全状态、contract cutover、双架构验证和停止条件。

当前契约依赖：

- [Signal pending routing](../../contracts/signal/pending-routing.md)
- [ThreadGroup lifecycle](../../contracts/task/thread-group-lifecycle.md)
- [Scheduler latch wait round](../../contracts/scheduler/latch-wait-round.md)

本 RFC 没有 `tracking-issues.md`。Gate 0--1、后续多个独立 cutover 与用户逐 Gate 授权由上述 transaction
保存执行证据，不在本页复制验证流水。

## 方案

### 系统启动后时间怎样运行

架构 clock source 先发布两个事实：当前统一硬件计数和频率 `F`，其中 `F` 的单位是 Hertz，表示硬件计数器
每秒增加多少次。timekeeper 初始化时读取一次计数并保存为 `boot_counter`；这一读取瞬间被定义为
`CLOCK_MONOTONIC == 0`。以后不保存一个周期递增的“当前时间”，每次读取都重新读取硬件计数并现场计算：

```text
counter_delta = counter_now - boot_counter

monotonic_now_ns
= counter_delta * 1_000_000_000 / F

monotonic_raw_now_ns = monotonic_now_ns

realtime_now_ns
= monotonic_now_ns + realtime_offset_ns
```

乘法使用 `u128`，结果回到内部纳秒类型和用户 `timespec` 前检查范围。除法是整数除法，不使用浮点。
`CLOCK_MONOTONIC_RAW` 保留独立读取入口，但首版不做 correction，所以调用同一个计算。100 Hz 的 `TICKS`
不参与普通 clock 读取；两个时钟中断之间，硬件计数变化仍会使普通 clock 读数变化。

无 RTC 时，`realtime_offset_ns` 初始为零，因此启动时 realtime 等于 monotonic，日期不准确但接口可用。
未来 RTC 接入时，在同一启动初始化窗口读取 RTC 和 monotonic：

```text
realtime_offset_ns = rtc_unix_epoch_ns - monotonic_sample_ns
```

此后 RTC 不再参与普通 clock 读取。`clock_settime()` 和 `clock_adjtime()` 也只改内存，不写回 RTC。

### timekeeper 保存什么

| 字段 | 含义 | 怎样变化 |
| --- | --- | --- |
| `boot_counter` | timekeeper 建立时的统一硬件计数，定义 monotonic 零点 | 初始化一次，之后不变 |
| `realtime_offset_ns` | realtime 与 monotonic 的非负差；内部宽度必须覆盖允许的 Unix 时间范围 | 启动时由零或 RTC 建立；`clock_settime()` / `ADJ_SETOFFSET` 只有在结果仍不小于 monotonic 时修改 |
| `realtime_change_seq` | realtime 最近一次实际跳变的计数，不是时间值 | 每次 offset 发生跳变时加一；只用于识别“从某次快照后是否发生过跳变” |
| `coarse_mono_ns` | 最近一次 BSP 周期节拍保存的 monotonic 快照 | BSP 每个系统节拍更新；允许落后真实 monotonic |

`realtime_change_seq` 解决的是快照与注册之间的竞态。例如 timerfd 读取 realtime 并准备登记
`TFD_TIMER_CANCEL_ON_SET` 时，可以同时记住当时的 change seq；登记完成后若序号已经不同，就知道中间发生过
一次 realtime 跳变，不能错过取消。它不是 clock ID，不参与时间公式，也不表示 offset 改了多少。

`coarse_mono_ns` 是明确允许陈旧的性能快照，不是第二份 monotonic 真相。realtime coarse 每次读取时计算：

```text
CLOCK_MONOTONIC_COARSE = coarse_mono_ns
CLOCK_REALTIME_COARSE = coarse_mono_ns + realtime_offset_ns
```

因此不保存可直接推导的 `coarse_real_ns`。realtime 跳变会立即反映到 realtime coarse；其中 monotonic 部分
仍只在 BSP 的系统节拍更新。

### 每个 clock 支持什么

| Clock ID | 本轮数值 | 允许的操作 |
| --- | --- | --- |
| `CLOCK_REALTIME` | `monotonic + realtime_offset` | get/res、set、首版 adjust、相对/绝对 sleep、POSIX timer |
| `CLOCK_MONOTONIC` | 从 `boot_counter` 开始的硬件计数换算值 | get/res、相对/绝对 sleep、POSIX timer；set/adjust 拒绝 |
| `CLOCK_PROCESS_CPUTIME_ID` | 当前线程组累计 CPU 计数换算值 | get/res；set/adjust 拒绝，sleep/timer 本轮返回不支持 |
| `CLOCK_THREAD_CPUTIME_ID` | 当前线程累计 CPU 计数换算值 | get/res；set/adjust 拒绝，sleep/timer 本轮返回不支持 |
| `CLOCK_MONOTONIC_RAW` | 首版与 monotonic 同值 | 仅 get/res |
| `CLOCK_REALTIME_COARSE` | `coarse_mono + realtime_offset` | 仅 get/res |
| `CLOCK_MONOTONIC_COARSE` | `coarse_mono` | 仅 get/res |
| `CLOCK_BOOTTIME` | 当前没有 suspend，因此与 monotonic 同值 | get/res、相对/绝对 sleep、POSIX timer；set/adjust 拒绝 |

“本轮返回不支持”表示该 Linux 能力需要本轮没有的 scheduler-driven CPU timer，不能用 wall timer 假装；
实现应返回 `EOPNOTSUPP` 并记录一次可观测日志。未知 clock ID 和该操作从定义上不允许的 clock 返回
`EINVAL`。如果 review 决定采用不同 errno，属于 ABI target 变化，必须在接受前明确，不能留给 handler 随意选择。

### `clock_getres()` 怎样根据 Hertz 计算

`clock_getres(clock_id)` 报告该 clock 的读数能分辨的时间单位，不报告 soft timer 多久检查一次到期。
普通 clock 每次读取硬件计数；soft timer 当前约每 10ms 才检查一次。这是两个不同事实。

设硬件频率为 `F`。一个硬件计数所代表的纳秒数用整数向上取整：

```text
source_resolution_ns
= max(1, (1_000_000_000 + F - 1) / F)
```

实现先把 `F` 和常量转成 `u128` 再计算，不使用浮点。向上取整是因为 `timespec` 不能表示小数纳秒；向下
取整会报告硬件实际达不到的更细精度。`max(1, ...)` 保证 `timespec` 的最小表示单位仍是 1ns。

普通 monotonic、raw、realtime 和 boottime 都从同一个计数器读取；realtime offset 不改变相邻读数的计数
粒度。进程和线程 CPU time 保存的也是该计数器的计数差。因此它们都返回 `source_resolution_ns`，不能写死
1ns 或 10ms。

coarse clock 的更新间隔先按当前 timer programming 的整数方式换成硬件计数，再换回纳秒：

```text
coarse_interval_counts = F / SYSTEM_HZ

coarse_resolution_ns
= max(1,
      (coarse_interval_counts * 1_000_000_000 + F - 1) / F)
```

乘法和加法同样使用 `u128`。启动时必须验证 `F >= SYSTEM_HZ`，避免更新间隔为零。当前
`SYSTEM_HZ == 100`；只有 `F` 能被 100 整除时，coarse 分辨率才正好是 10,000,000ns，不能把 10ms 写成
所有平台的固定返回值。

| Clock ID | `clock_getres()` 来源 |
| --- | --- |
| `CLOCK_REALTIME` | `source_resolution_ns` |
| `CLOCK_MONOTONIC` | `source_resolution_ns` |
| `CLOCK_PROCESS_CPUTIME_ID` | `source_resolution_ns` |
| `CLOCK_THREAD_CPUTIME_ID` | `source_resolution_ns` |
| `CLOCK_MONOTONIC_RAW` | `source_resolution_ns` |
| `CLOCK_REALTIME_COARSE` | `coarse_resolution_ns` |
| `CLOCK_MONOTONIC_COARSE` | `coarse_resolution_ns` |
| `CLOCK_BOOTTIME` | `source_resolution_ns` |

Linux 按 clock ID 分派不同 `clock_getres` 实现，入口可参考
`xref:linux-6.6.32:kernel/time/posix-timers.c#SYSCALL_DEFINE2(clock_getres)`。Anemone 按自己的硬件计数和
快照更新方式计算，不照抄 Linux 某个 high-resolution 或 low-resolution 配置的常数。

### 设置和调整 realtime

`clock_settime()` 只允许 `CLOCK_REALTIME`，调用者需要 `CAP_SYS_TIME`。它完整校验用户 `timespec` 后，在
timekeeper 锁内读取当前 monotonic，计算新的 `realtime_offset_ns`；offset 实际变化时增加
`realtime_change_seq`。释放 timekeeper 锁后，再通知 soft timer 和 timerfd 处理 realtime 跳变。锁内不能
运行 timer 到期处理函数、唤醒 task 或发送 signal。目标 realtime 小于当前 monotonic 时返回 `EINVAL`，保留
Linux 防止 realtime 落到 monotonic 之前的检查。

跳变通知必须让 soft timer 重新检查所有 CPU 的 realtime 队列，不能只检查执行 syscall 的当前 CPU。向前跳
后越过目标的请求转入各自到期处理路径；向后跳只改变以后判断到期的时间。这个跨 CPU 扫描或通知发生在
timekeeper 锁外，具体 IPI/worker 形状属于实现选择。

首版 `clock_adjtime()` 仍操作同一个 timekeeper：

- `modes == 0` 查询当前状态，不要求 `CAP_SYS_TIME`；
- `ADJ_SETOFFSET` 立即给 realtime 增加指定偏移，需要 `CAP_SYS_TIME`；
- `ADJ_NANO` / `ADJ_MICRO` 只解释 `ADJ_SETOFFSET` 输入单位；
- 频率修正、渐调、PLL/FLL、PPS、闰秒、TAI、status 和 tick 修改返回 `EOPNOTSUPP` 并记录日志；
- 未知 mode bit 或非法组合返回 `EINVAL`，不能成功但不生效，也不能把渐调静默改成立即跳变。

查询返回的 frequency 和 pending offset 为零，precision 使用 `source_resolution_ns`。因为没有 NTP
discipline，status 报告 `STA_UNSYNC`，调用返回 `TIME_ERROR`。首版只接受 `CLOCK_REALTIME`。
`ADJ_SETOFFSET` 应用后若 realtime 会小于当前 monotonic，同样返回 `EINVAL` 且不修改 offset 或 change seq。

### `clock_nanosleep()` 怎样提交一次睡眠

相对睡眠表示“从现在起再经过多久”，不应受以后修改日期影响：

- realtime、monotonic 和 boottime 的相对请求都换成 `当前 monotonic + 用户时长`，排入 monotonic 队列；
- monotonic/boottime 的 `TIMER_ABSTIME` 直接保存该 clock 的绝对目标；
- realtime 的 `TIMER_ABSTIME` 保留用户给出的日历目标，排入 realtime 队列，realtime 跳变后重新判断。

flags 只解释 `TIMER_ABSTIME` 位；其它位按 Linux legacy 行为静默忽略并做限频日志，不返回 `EINVAL`。这不是
临时 bridge：现有 legacy syscall 持续保留该行为，只有未来新增独立严格接口时，严格接口才可以拒绝未知位。
绝对睡眠被 signal 中断时不写 remaining time；相对睡眠被中断时用“原目标 monotonic - 当前 monotonic”写回
剩余时间。task 是否仍在等待、是被 signal 打断还是正常到时，继续由 wait-core 管理；soft timer 只保存本次
唤醒请求，不复制 task 等待状态。syscall 离开前必须用排队句柄删除仍在队列中的唤醒请求。

### soft timer 管理的是什么

soft timer 管理的不是 POSIX timer 对象，也不是 timerfd 文件或 task。它只管理“一次未来执行请求”：
到某个 clock 的目标时刻后，运行一个到期处理函数。

| 提交者 | 长期状态由谁保存 | 一次请求到期后做什么 |
| --- | --- | --- |
| `clock_nanosleep()` | 当前 task 的 wait round | 唤醒本次睡眠 |
| POSIX timer | `ThreadGroup` timer ID 表中的对象 | 更新该 timer，并按通知方式产生 `SI_TIMER` |
| timerfd | 该文件的 `TimerFdCore` | 增加到期次数，使 fd 可读并通知 poll/epoll |
| `ITIMER_REAL` | `ThreadGroup::RealITimer` | 通过通用 signal 发送 `SIGALRM` |

相对请求和 monotonic/boottime 绝对请求进入 monotonic 队列；realtime 绝对请求进入 realtime 队列。每个请求
至少保存目标时刻、唯一 `event_id`、执行种类和到期处理函数。队列插入成功后返回排队句柄：

```text
owner_cpu  请求在哪个 CPU 的队列
clock_kind monotonic 队列或 realtime 队列
event_id   请求在该队列中的唯一编号
```

目标队列必须支持按该句柄删除仍在队列中的请求。当前只支持弹出最早元素的 `BinaryHeap` 不满足要求；具体
使用何种有序、可索引容器属于实现选择，但不能继续把“让旧 callback 不生效”冒充资源删除。跨 CPU 删除必须
取得目标队列的 IRQ-safe 锁，不能只依赖调用 CPU 关中断。

各操作删除的是对象当前排入队列的那一次请求：

- 睡眠被 signal 中断：删除本次唤醒请求；
- `timer_settime()` 替换或解除：删除该 POSIX timer 原来的下一次到期请求；
- `timer_delete()`、成功 `exec` 或线程组最后成员退出：删除所属 POSIX timer 的当前请求；
- timerfd 重设、解除或最后引用关闭：删除该文件当前请求；
- `setitimer()` 重设或解除：删除原来的 `SIGALRM` 请求。

删除可能和到期同时发生：请求也许已经从定时队列移到 timer worker 的 ready queue，此时排队句柄找不到它。
因此每个长期对象还保存 generation。重设、解除或删除时先增加 generation；请求记住提交时的值；到期处理
函数运行前比较，不相等就退出。物理删除负责立即释放仍在队列中的远期请求，generation 只负责阻止已经出队
的旧处理函数修改新状态，两者不能互相替代。

每次 100 Hz 中断分别检查当前 CPU 的 monotonic 和 realtime 队列。请求不得早于目标时刻执行，但可能等到
下一个周期中断才被发现。以后改用最早目标动态设置硬件中断时，可以复用现有
`LocalClockEventArch::program_next_timer()`；这不是本 RFC 的首版要求。

realtime 向前跳后，已经越过目标的绝对 realtime 请求立即成为到期请求；向后跳后，它们继续等待 realtime
重新走到目标。普通绝对 realtime 请求不会因为设置时间而被删除。只有使用 `TFD_TIMER_CANCEL_ON_SET` 的
绝对 realtime timerfd 删除当前请求、进入 cancelled 状态，并让下一次 `read()` 返回 `ECANCELED`。

### POSIX timer 对象和 timer ID

timer ID 是当前线程组 POSIX timer 表中的整数键。它不是文件描述符，不是内核指针，也不是 signal 编号。
线程组内任意线程可以用该 ID 操作同一个 timer；其它线程组的同数值 ID 与它无关。删除 timer 时，先从 ID
表移除键，使新的 syscall 查找立即失败；旧数字只有在旧对象不再可见后才允许复用。

每个 POSIX timer 对象保存：所用 clock、`SIGEV_NONE` 或 `SIGEV_SIGNAL` 参数、下一次到期时刻、周期、
generation、当前排队句柄、是否已有本 timer 的通知等待交付、当前通知累计的 overrun，以及最近一次已交付
通知的 overrun。这些状态在同一个对象锁下修改。soft timer 只持弱引用和本次请求的 generation；signal
pending 项保存 timer ID、generation 和 `sigval`，不成为第二份 timer schedule。

五个 syscall 的目标语义是：

- `timer_create()` 支持 realtime、monotonic 和 boottime；空 `sigevent` 使用
  `SIGEV_SIGNAL + SIGALRM`，`sival_int` 是新 timer ID；首版通知方式只支持 `SIGEV_NONE` 和
  `SIGEV_SIGNAL`。
- `timer_settime()` 的 flags 同样只解释 `TIMER_ABSTIME` 位，其它位按 Linux legacy 行为静默忽略并限频日志；
  `it_value == 0` 解除 timer并删除旧请求；替换时先保存需要返回的旧设置，再删除旧请求、增加 generation、
  写入新目标和周期，最后保存新排队句柄。
- `timer_gettime()` 返回周期和相对于所选 clock 的剩余时间。周期通知已 pending 时，也要按原周期目标计算
  下一次未来到期，不能永久返回过期值。
- `timer_getoverrun()` 返回最近一次已经交付给用户的 timer signal 对应的 overrun，超过 `INT_MAX` 时钳住。
- `timer_delete()` 先从 ID 表移除对象，再增加 generation、删除仍排队请求并释放对象。已经进入 signal
  pending 的通知按 signal 自己的生命周期处理；它可以被交付，但不能重新 arm 已删除对象。

周期 timer 的下一目标从原目标增加整数个 interval，不能从到期处理函数实际运行时间重新起算，否则每次
调度延迟都会累积成永久漂移。

Linux 6.6.32 的可见行为参考
`xref:linux-6.6.32:kernel/time/posix-timers.c#do_timer_create`、
`xref:linux-6.6.32:kernel/time/posix-timers.c#do_timer_gettime`、
`xref:linux-6.6.32:kernel/time/posix-timers.c#do_timer_settime` 和
`xref:linux-6.6.32:kernel/time/posix-timers.c#SYSCALL_DEFINE1(timer_delete)`。

### POSIX timer 怎样走通用 signal

`SIGEV_SIGNAL` 到期时，POSIX timer 对象构造带真实 timer ID、generation 和 `sigval` 的 `SI_TIMER`，通过
signal 子系统的窄入口提交到当前线程组 shared pending。signal 子系统继续负责 disposition、mask、选择可接收
线程、唤醒和用户 signal frame；timer 子系统不自建 signal queue。

signal pending 必须按 timer 身份保存 `SI_TIMER`。即使两个 timer 选择同一个普通非实时 signal，它们也是
两份独立通知；普通 `kill()` 产生的非实时 signal 仍保持现有单 slot 合并语义。同一个 timer 在通知已经 pending
期间不再追加第二份通知，而是根据原周期目标累计错过的完整 interval。

`SI_TIMER` 真正被取出交付时，signal 子系统用 timer ID 和 generation 回告 POSIX timer 对象。对象把当前累计
值固化为 `last_overrun`，清除 pending 标记，把下一目标推进到未来，再提交下一次到期请求。如果 disposition
使 signal 在入队前被忽略，对象直接按原周期推进，不能让周期 timer 永久停止。

新进程不从 `fork` 继承 POSIX timer。成功 `exec` 和线程组最后成员退出都删除该线程组全部 timer：先让 ID
不可查，再删除仍排队请求，最后释放对象。已经出队的旧处理函数只能通过弱引用和 generation 退出。

### 其它时间消费者

建立真正 realtime 后，所有表示日历日期的现有消费者必须改读同一个 realtime：`gettimeofday()`、
`UTIME_NOW`、文件 atime/mtime/ctime 和 SysV IPC 日期字段。`/proc/uptime`、`sysinfo.uptime`、调度 timeout 和
纯内核 elapsed-time consumer 继续使用 monotonic/raw。CPU usage 继续由 task/thread group 累计。

timerfd 和 `ITIMER_REAL` 必须迁移到可物理删除的 soft timer 请求；`ITIMER_REAL` 的周期也改为按原目标推进。
实施时逐个分类现有 `Instant::now().to_duration()` 调用，不能用全局搜索替换把日期和经过时间混在一起。

## Owner 与协议边界

| Owner | 唯一负责的事实 | 向其它模块提供什么 |
| --- | --- | --- |
| 架构 clock source/event | 统一硬件计数、Hertz、设置下一硬件中断 | 读计数、读频率、安排绝对硬件计数中断 |
| timekeeper | monotonic 零点、realtime offset/change seq、coarse monotonic 快照 | 按 clock ID 读取、设置或调整时间；锁外发布跳变通知 |
| clock 路由 | clock ID 的操作能力和 ABI 校验 | 把合法请求交给 timekeeper、CPU usage、wait 或 POSIX timer owner |
| soft timer | 一次排队请求、物理删除、到期交接 | 排队句柄；不拥有 POSIX timer ID、周期、overrun 或 signal |
| `ThreadGroup` POSIX timer 表 | timer ID、对象到期安排、通知参数、overrun 和生命周期 | 五个 timer syscall 的对象操作；向 soft timer/signal 做窄交接 |
| signal | pending、disposition、mask、目标线程选择、唤醒和 frame | `SI_TIMER` 入队结果与交付回告 |
| RTC provider | 启动时一次 Unix Epoch 读数 | 可选 boot seed；运行期不参与 clock |

timekeeper 锁内不能调用 soft timer、wait 或 signal。POSIX timer 对象锁内不能获取 signal 的全局 pending 锁后
再回调任意 timer syscall 路径；具体锁序必须在实现 review 中证明。任何模块都不能缓存一份可独立修改的
realtime、POSIX schedule 或 pending 真相。

## ABI 与错误语义

- `clock_gettime()` / `clock_getres()` 未知 clock ID 返回 `EINVAL`；`tp == NULL` 的
  `clock_getres()` 仍只验证 clock ID，不写用户内存。
- `clock_settime()` 只接受 realtime，校验完整 `timespec` 和 `CAP_SYS_TIME`；其它 clock 返回 `EINVAL`。
- `clock_adjtime()` 的支持 mode、权限与拒绝规则以方案正文为准；不能静默接受未实现 adjustment。
- `clock_nanosleep()` 只允许 realtime、monotonic 和 boottime；CPU clock 返回本轮明确的不支持错误；
  raw/coarse 和未知 clock 返回 `EINVAL`。
- `timer_create()` 只允许 realtime、monotonic 和 boottime；CPU clock 返回本轮明确的不支持错误；其它 clock
  返回 `EINVAL`。
- timer ID 不存在、已删除或属于其它线程组时，timer syscall 返回 `EINVAL`。
- POSIX timer 首版只接受 `SIGEV_NONE` 和 `SIGEV_SIGNAL`；其它通知方式返回 `EOPNOTSUPP` 并记录日志。
- `clock_nanosleep()` 和 `timer_settime()` 只解释 `TIMER_ABSTIME`，未知 flag 位按 Linux legacy ABI 静默忽略；
  该永久兼容选择必须有源内注释和限频日志，不能以后顺手收紧成 `EINVAL`。
- 所有用户 `timespec` / `itimerspec` 先检查负秒、`tv_nsec` 范围和内部换算溢出，再产生可见副作用。

Linux ABI 表达只停在 syscall 和 signal copy 边界；timekeeper、soft timer 和 POSIX timer 对象不长期保存
Linux UAPI struct。

## Contract Impact

以下是 R0 contract delta。Gate 1--5 已完成全部五项 cutover：

| Contract ID | 变化 | 当前规则 | Target 摘要 | Cutover |
| --- | --- | --- | --- | --- |
| `TIMEKEEPER-CLOCK-001` | Introduce | [Active current rule](../../contracts/time/clock-derivation.md#timekeeper-clock-001--所有-clock-读取来自一条整数推导链) | 架构计数/Hertz 是 monotonic/raw 来源；timekeeper 唯一保存零点、realtime offset 和 coarse 快照 | Gate 1 `TC-CLOCK-CUTOVER` Completed |
| `SOFT-TIMER-REQUEST-001` | Introduce | [Active current rule](../../contracts/time/soft-timer-request.md#soft-timer-request-001--排队句柄物理删除一次请求) | soft timer 只拥有一次排队请求，支持按句柄物理删除；长期对象状态仍由提交者拥有 | Gate 2 `ST-REQUEST-CUTOVER` Completed |
| `TIMEKEEPER-STEP-001` | Introduce | [Active current rule](../../contracts/time/realtime-step.md#timekeeper-step-001--realtime-step-不能漏掉或误用旧-timeline) | realtime offset/change seq 在 timekeeper 内原子提交，锁外无丢失通知所有 realtime consumer | Gate 3 `TC-STEP-CUTOVER` Completed |
| `POSIX-TIMER-001` | Introduce | [Active current rule](../../contracts/time/posix-timer.md#posix-timer-001--threadgroup唯一拥有timer对象id与通知episode) | `ThreadGroup` 拥有 timer ID、到期安排、overrun 与清理；soft timer 和 signal 只执行窄交接 | Gate 5 `PT-SIGNAL-CUTOVER` Completed |
| `SIGNAL-PENDING-001` | Refine | [ordinary signal 与 `SI_TIMER` 当前规则](../../contracts/signal/pending-routing.md#signal-pending-001--directed-occurrence-只进入对应-pending-owner) | 普通 signal 保持现有合并；`SI_TIMER` 按 timer 身份保存独立 pending 项 | Gate 5 `PT-SIGNAL-CUTOVER` Completed |

Dependencies：

- [`SIGNAL-ACTION-001`](../../contracts/signal/pending-routing.md#signal-action-001--ignored-disposition-在-pending-publication-前生效)：signal 子系统继续在 pending 前处理 ignored disposition。
- [`SIGNAL-ACTION-002`](../../contracts/signal/pending-routing.md#signal-action-002--ordinary-trap-return-才提交异步-action)：signal frame 仍由普通 return-to-user 路径提交。
- [`TASK-LIFE-002`](../../contracts/task/thread-group-lifecycle.md#task-life-002--最后-member-detach-后才能发布-exited)：线程组最后成员退出前完成 timer 清理，不改变 terminal owner。
- [`SCHED-LATCH-001`](../../contracts/scheduler/latch-wait-round.md#sched-latch-001--每个-latch-只拥有一个-wait-core-round)：`clock_nanosleep()` 复用现有 wait round。

## Implementation Boundary

- 允许改变：timekeeper、clock 路由和 syscall ABI 层、soft timer 队列、timerfd、`ITIMER_REAL`、task wait
  接入、`ThreadGroup` POSIX timer 表、`SI_TIMER` pending/交付，以及现有日期时间消费者。
- 必须保持：Hertz 由架构 clock source 提供；CPU usage 由 task/thread group 保存；signal mask、目标选择和
  frame 由 signal 管理；timerfd 和 `ITIMER_REAL` 不建立第二套时间真相。
- RTC 只允许增加启动时一次读取的窄 provider；普通 clock、设置和调整不访问或写回 RTC。
- 不增加 403--409，不扩大首版 clock、通知方式或 CPU-time timer 范围，不把 unsupported 成功返回。
- 替换、解除、删除、`exec` 和最后成员退出必须物理删除仍排队请求；已经出队的旧处理函数必须失效。
- 如果实现需要改变 syscall 范围、clock 操作矩阵、状态 owner、signal pending 规则、RTC 策略、errno 或验证
  强度，必须回到 RFC review，不能由实现自行降低 target。

## Acceptance 与 Validation

R0 target、owner 和 ABI 已按[实施计划](./implementation.md)的 Gate 0--6 实现并关闭；四个 cutover gate
已经使五项 contract delta 生效。最终 closure evidence 见对应
[transaction](../../devlog/transactions/2026-08-04-clock-timekeeping-posix-timers.md)，其证明范围包括：

- source/KUnit 证明全部时间换算只使用检查过的整数运算；用多个测试 Hertz 验证普通/coarse
  `clock_getres()`、向上取整、边界和溢出，确认没有浮点路径和固定 1ns/10ms 返回值；
- RV64 与 LA64 build/runtime 证明 monotonic/raw 不后退、跨 CPU 迁移连续、realtime 修改不改变 monotonic；
- 八个 clock ID 的 get/res/拒绝矩阵，以及 `clock_settime()`、`clock_adjtime()` 的权限、mode 和跳变验证；
- 相对/绝对 `clock_nanosleep()` 的正常到期、signal 中断、remaining time 和 realtime 双向跳变验证；
- 反复重设/删除远期 POSIX timer、timerfd 和 `ITIMER_REAL` 后，队列资源有界；并发出队时 generation 阻止旧
  处理函数生效；
- POSIX timer ID 隔离、默认 sigevent、两种支持通知、周期推进、overrun、同 signal 不同 timer、
  fork/exec/exit 和晚到通知验证；
- timerfd `CANCEL_ON_SET`、`ITIMER_REAL`、`gettimeofday()`、文件日期和 uptime 消费者回归；
- `git diff --check` 以及面向 Linux man-pages/6.6.32 可见语义的用户态验证；mdBook 检查按开发者明确指示
  Not Run。LTP 可以作为回归证据，但不是语义定义。

## 备选方案

### 每次 `clock_gettime(CLOCK_REALTIME)` 读取 RTC

拒绝。RTC 是低频、可能较慢的持久日期来源，不是系统运行期的单调走时源；持续读取会让 realtime 依赖设备
延迟和可用性，也无法自然支持 `clock_settime()` 的内存调整。启动时读取一次已经足够建立 epoch 偏移。

### 给每个 clock 保存独立的当前纳秒值

拒绝。realtime、raw、boottime 和 coarse 都能从统一硬件计数、offset 或快照推导；分别递增会制造多份可漂移
真相。首版只有一个计数时间线和一个 realtime offset。

### 继续使用 generation 作为 soft timer 取消

拒绝。generation 只能阻止旧处理函数产生效果，不能释放仍排在很远未来的事件；反复 set/delete 会让队列
无界增长。必须增加物理删除，generation 仅处理已经出队的竞态。

### 在 POSIX timer 内自建 signal queue

拒绝。mask、disposition、目标线程、唤醒和 frame 已由 signal 子系统拥有。timer 只需要带身份的窄入队和交付
回告；第二套 signal queue 会制造冲突的 pending 真相。

### 首版同时实现 CPU-time timer 和高精度 timer

延期。CPU-time timer 需要 scheduler 按实际 CPU 消耗触发，高精度 timer 需要按最早目标重编程硬件中断；
它们都不是补齐本轮 wall-clock/POSIX timer 语义的前置条件。

## 风险

- 当前 per-CPU boot baseline 未证明跨 CPU 连续；若架构计数不天然同步，必须先在架构 source 内解决。
- realtime 跳变同时影响 absolute sleep、POSIX timer 和 timerfd；锁内通知会造成 owner 穿透和潜在死锁。
- per-CPU 队列的远程删除与 IRQ 出队存在竞态；实现必须明确锁序、线性化点和 ready queue 交接。
- 普通 signal 单 slot 需要为 `SI_TIMER` 增加来源特例；该特例不能改变普通 `kill()` signal 的合并语义。
- 100 Hz 检查会让请求通常到下一个 tick 才被发现；threaded 到期处理还可能有额外调度延迟。
  `clock_getres()` 仍报告读时钟分辨率，不能用它伪装 timer delivery 精度。
- `ADJ_SETOFFSET` 之外的 `clock_adjtime()` 能力明确缺失；错误返回或日志不稳定会让用户误判为已支持校时。

## 收口

R0 在不修改 target 的前提下关闭。Gate 0--6 全部完成；`TC-CLOCK-CUTOVER`、`ST-REQUEST-CUTOVER`、
`TC-STEP-CUTOVER` 与 `PT-SIGNAL-CUTOVER` 已使本 RFC 的五项 contract delta 全部生效。

Gate 6 最终审计确认 calendar consumer 统一读取 realtime，scheduler、driver、network 与 uptime consumer 保持
monotonic，CPU clock 继续由 task/thread-group owner 累计；wait-core、timerfd、`ITIMER_REAL` 与 POSIX timer 都会
物理取消仍排队请求，`SI_TIMER` pending 继续由 signal owner 按 timer identity 持有。RV64/LA64 release SMP=2
分别通过 469/469 与 470/470 KUnit，10 个 syscall 及既有 clock、soft timer、futex、timerfd、itimer、signal
用户态 oracle 全部通过。Architecture Friction Scan 未发现第二份状态真相、owner 穿透、私有表示泄漏、隐含
cleanup 顺序或无真实义务的抽象层。

CPU-time timer/sleep、high-resolution/tickless timer、RTC seed/writeback、32 位 ABI 与新增 clock ID 仍是 R0
明确 non-goal，不是本 target 内缺陷，因此不新增 register limitation。详细执行证据由
[transaction](../../devlog/transactions/2026-08-04-clock-timekeeping-posix-timers.md)保存。
