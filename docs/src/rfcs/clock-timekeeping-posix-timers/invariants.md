# Clock Timekeeping 与 POSIX Timers 目标与不变量

**状态：** Accepted Target
**最后更新：** 2026-08-03
**父 RFC：** [RFC-20260803-clock-timekeeping-posix-timers](./index.md)
**适用修订：** R0

本文只保存实现和 review 必须共同证明的规则。syscall 范围、完整 clock 操作矩阵、背景事实和 ABI 说明以
[RFC 正文](./index.md)为准；实施 gate 与验证路线见[实施计划](./implementation.md)。

## 规则分类

- **Correctness Invariant：** 唯一时间真相、状态 owner、锁外通知、请求删除、在途处理失效、对象生命周期和
  signal pending 身份。违反这些规则会造成时间错误、资源无界增长、旧 callback 修改新对象、重复通知或
  use-after-free，不能通过缩小功能范围接受。
- **Target Guarantee / Capability：** 本轮 10 个 native syscall、clock 操作矩阵、按 Hertz 报告分辨率、
  realtime step、POSIX timer 周期与 overrun。这些能力只能经 RFC review 修改。
- **Acceptance / Scope Boundary：** 不做 correction、CPU-time timer、高精度 timer、32 位入口和 RTC 写回。
- **Implementation Preference：** Rust 类型名、模块拆分、队列具体容器、锁类型、event ID 分配算法、跨 CPU
  通知采用 IPI 还是 worker。这些可以在不改变规则的前提下由实现决定。

## Contract Impact

Contract delta 只在 [RFC 正文的 Contract Impact](./index.md#contract-impact) 维护。本文的 `TC-*`、`ST-*` 和
`PT-*` 是 RFC-local proof label，不是生效的 current contract ID。cutover 后跨 RFC 依赖必须引用
`TIMEKEEPER-CLOCK-001`、`TIMEKEEPER-STEP-001`、`SOFT-TIMER-REQUEST-001`、`POSIX-TIMER-001` 或
更新后的 `SIGNAL-PENDING-001`。

## Target Invariants

### TC-SOURCE-001 — 架构层只向上提供一条统一硬件计数

**分类：** Correctness Invariant

**规则：** `LocalClockSourceArch` 向 timekeeper 提供的当前计数必须在所有 online CPU 上属于同一数值域，
读取不能因为 task 迁移而后退；同时提供非零、稳定的频率 `F`。如果平台原始计数存在 per-CPU offset，offset
校准和应用由架构 clock source 独占，timekeeper 不再保存第二组 per-CPU 基准或校正状态。

本条所说的 source 统一只消除硬件计数域差异，不是 NTP、频率校正或 monotonic discipline。首版始终按架构
报告的标称 Hertz 换算。

**违反表现：** timekeeper 和架构层各自校正 CPU offset；同一 task 迁移后 raw/monotonic 后退；频率为零；
不同 CPU 使用不同频率却对上层宣称同一 source。

**证明：** 双架构 source audit；跨 CPU 迁移连续性 runtime；启动时对 `F > 0` 和需要的频率范围常开断言。

### TC-TIMEKEEPER-001 — 时间值只有一个推导链

**分类：** Correctness Invariant

**规则：** timekeeper 的 monotonic 唯一由统一硬件计数、不可变 `boot_counter` 和频率计算：

```text
monotonic = (counter_now - boot_counter) * 1_000_000_000 / F
raw = monotonic
realtime = monotonic + realtime_offset
```

`boot_counter` 的采样瞬间定义 monotonic 零点。乘法使用足够宽的整数并检查结果范围，不使用浮点。
`CLOCK_MONOTONIC_RAW` 首版保留独立接口但不保存独立数值。boottime 在没有 suspend 时从同一 monotonic
读取，不保存可独立修改的副本。

`realtime_offset` 必须保持非负，使 realtime 不小于 monotonic。`clock_settime()`、`ADJ_SETOFFSET` 或未来
RTC seed 若会违反该关系，必须在提交前失败，不得先写 offset 再修补。

timekeeper 唯一保存 `boot_counter`、`realtime_offset_ns`、`realtime_change_seq` 和
`coarse_mono_ns`。`coarse_mono_ns` 是明确允许陈旧的性能快照；realtime coarse 在读取时加 offset，不保存
`coarse_real_ns`。

**违反表现：** 每个 clock 周期递增自己的纳秒值；RTC 参与普通读取；raw/realtime/boottime 保存可独立漂移
的当前值；文件时间或 timerfd 保存第二份可修改的 realtime。

**依赖：** TC-SOURCE-001。

### TC-RES-001 — `clock_getres()` 必须来自真实计数或快照周期

**分类：** Correctness Invariant + Target Guarantee

**规则：** 普通 clock 和 CPU clock 的分辨率从硬件频率计算；两个 coarse clock 从实际 coarse 更新计数
间隔计算。所有加法和乘法先提升到 `u128`，使用整数向上取整，不使用浮点：

```text
source_resolution_ns = max(1, ceil(1_000_000_000 / F))

coarse_interval_counts = F / SYSTEM_HZ
coarse_resolution_ns =
    max(1, ceil(coarse_interval_counts * 1_000_000_000 / F))
```

实现中的 `ceil(a / b)` 必须使用经过溢出检查的整数除法。启动时保证 `F >= SYSTEM_HZ`。
realtime、monotonic、raw、boottime、process CPU time 和 thread CPU time 返回
`source_resolution_ns`；两个 coarse clock 返回 `coarse_resolution_ns`。

soft timer 的 100 Hz 到期检查周期不参与普通 clock 分辨率计算。它只影响请求何时被发现到期。

**违反表现：** 固定返回 1ns 或 10ms；根据 timer delivery 延迟报告普通 clock 分辨率；CPU clock 不根据其
实际累计计数来源计算；任一路径使用浮点。

**依赖：** TC-SOURCE-001、TC-TIMEKEEPER-001。

### TC-STEP-001 — Realtime 跳变在 timekeeper 内提交，在锁外通知

**分类：** Correctness Invariant

**规则：** `clock_settime()` 和支持的 `ADJ_SETOFFSET` 在 timekeeper 锁内完成参数对应的 offset 计算、范围
检查、offset 写入和 `realtime_change_seq` 增加。offset 没有实际变化时不增加序号。失败不得部分写入
offset 或序号。

锁释放后才允许通知 soft timer、timerfd、wait 或 signal。通知必须覆盖所有 CPU 的绝对 realtime 请求；
向前跳越过目标的请求进入正常到期路径，向后跳不删除普通请求。使用
`TFD_TIMER_CANCEL_ON_SET` 的绝对 realtime timerfd 必须通过 change seq 或等价的无丢失协议观察跳变、删除
当前请求并让下一次 `read()` 返回 `ECANCELED`。

`realtime_change_seq` 只表示“从旧快照后发生过一次实际跳变”，不参与时间计算，也不能反向决定 offset。

**违反表现：** 持 timekeeper 锁运行 callback、跨 CPU 扫描队列或发送 signal；offset 已改但序号/通知丢失；
普通绝对 realtime timer 被当成 cancel-on-set 删除；失败留下半次 step。

**依赖：** TC-TIMEKEEPER-001。

### TC-CLOCK-OPS-001 — Clock ID 的能力必须逐操作判断

**分类：** Target Guarantee + Acceptance / Scope Boundary

**规则：** clock 路由必须显式判断 get/res、set、adjust、sleep 和 POSIX timer 能力，不能因为 clock 可读取就
默认允许其它操作。完整矩阵以 [RFC 正文](./index.md#每个-clock-支持什么)为准。

CPU-time sleep/timer、raw/coarse sleep/timer 和 unsupported adjustment 必须按 RFC 的 errno 策略拒绝并保留
必要日志。`clock_nanosleep()` / `timer_settime()` 的 legacy flags 是明确例外：只解释
`TIMER_ABSTIME`，其它位静默忽略并限频日志。这个行为是 Linux legacy ABI，不是以后应删除的临时桥；只有
未来独立严格接口可以拒绝未知位。403--409 不注册。`CLOCK_MONOTONIC` 与 `CLOCK_MONOTONIC_RAW` 同值不能
通过静态数组偶然指向同一对象表达，必须保留独立路由身份。

**违反表现：** `clock_nanosleep()` 忽略 clock ID/flags；CPU timer 被 wall timer 代替；unsupported mode
成功但无效果；native 64 位架构注册 compat time64 编号。

**依赖：** TC-TIMEKEEPER-001、TC-RES-001。

### ST-OWNER-001 — Soft timer 只拥有一次未来执行请求

**分类：** Correctness Invariant

**规则：** soft timer 的权威状态只包括一次排队请求的目标 clock、到期时刻、唯一 event identity、执行种类
和到期处理能力。POSIX timer ID/周期/overrun、timerfd 到期计数、`ITIMER_REAL` 状态和 task wait outcome
分别留在原 owner；soft timer 不保存或推进这些长期对象的第二份状态。

队列插入返回能唯一定位该请求的排队句柄。event identity 在旧请求仍可能位于定时队列或 ready queue 时不得
与新请求冲突；ID 回绕必须检查冲突或 fail closed。

**违反表现：** timer core 保存 POSIX timer schedule；timerfd 与 timer core 各自累计 expiry；callback 持有
完整 `Task` 或 `ThreadGroup` 强引用导致对象无法清理；event ID 复用使旧句柄删除新请求。

### ST-CANCEL-001 — 物理删除和 generation 分别解决两个问题

**分类：** Correctness Invariant

**规则：** 替换、解除、删除和 owner teardown 必须用排队句柄物理删除仍在定时队列中的请求，立即释放队列
资源。请求已经出队时，长期对象 generation 或 wait-core identity 必须阻止旧到期处理函数产生效果。

generation 只能由长期对象 owner 在替换、解除或删除的同一状态转换中推进；timer core 只携带提交时的
snapshot，不得修改 generation。物理删除失败只有“请求已经出队或已被删除”这一种正常解释，不能静默掩盖
错误 owner/identity。

**违反表现：** 只增加 generation、让远期旧请求留在队列；删除方找不到请求却仍释放 callback 可访问对象；
timer core 自行推进对象 generation；反复 set/delete 使内存随操作次数无界增长。

**依赖：** ST-OWNER-001。

### ST-LOCK-001 — Queue、对象和 signal 的交接不在对方锁内回调

**分类：** Correctness Invariant

**规则：** timer IRQ 在 queue 锁内只把已到期请求移出有序队列并发布到适当 ready lane；释放 queue 锁后才
运行到期处理函数或获取长期对象锁。对象操作可以在自己的状态转换中调用窄 enqueue/cancel API，但 soft timer
不得在 queue 锁内回调对象。

POSIX timer 向 signal 提交通知时可以携带不可变输入；signal 在自己的 pending 锁内完成入队/取出后，必须
释放 pending 锁再回告 POSIX timer 对象。signal 不得持 pending 锁获取 timer 对象锁，timer 到期路径也不得
在持对象锁时接受会同步回调 timer 的 signal 操作。

跨 CPU queue 操作使用目标队列的 IRQ-safe 锁。禁止同时持有两个 CPU queue 锁；跨所有 CPU 的 realtime
重检逐队列完成，不形成全局多锁扫描。

**违反表现：** queue lock -> callback -> object lock；signal lock -> timer delivery callback -> object lock；
两个 CPU queue lock 互锁；timekeeper 锁嵌套 queue/object/signal 锁。

**依赖：** TC-STEP-001、ST-OWNER-001、ST-CANCEL-001。

### PT-ID-001 — `ThreadGroup` 唯一拥有 POSIX timer ID 和对象生命周期

**分类：** Correctness Invariant

**规则：** timer ID 是当前线程组 timer 表的整数键，不是 fd、指针或 signal number。表 publication/removal
是 syscall 查找可见性的唯一真相。线程组内任意线程可以操作对象；其它线程组的同数值 ID 不相关。

删除顺序是：先从 ID 表移除，使新查找失败；再推进 generation；再物理删除仍排队请求；最后释放对象。新
进程不从 `fork` 继承 timer；成功 `exec` 和最后成员退出执行同一 owner-local 批量删除协议。数字 ID 只有在
旧对象不再对 syscall 可见后才允许复用。

**违反表现：** 全局 timer ID namespace；timer core/signal 拥有 ID 表；对象释放后 ID 仍可查；fork 复制父
timer；exec/exit 只清 ID 表而不删除排队请求。

**依赖：** ST-CANCEL-001、[`TASK-LIFE-002`](../../contracts/task/thread-group-lifecycle.md#task-life-002--最后-member-detach-后才能发布-exited)。

### PT-PERIOD-001 — 周期和 overrun 都从原目标推进

**分类：** Correctness Invariant + Target Guarantee

**规则：** 周期 timer 的第 `n` 次目标只能由原目标加整数个 interval 得到，不从 callback 或 signal 实际运行
时间重新起算。通知已经 pending 时，同一 timer 不追加第二份 pending 通知；对象根据当前 clock 和原周期目标
计算完整错过周期数。`timer_getoverrun()` 只返回最近一次已交付通知固化的值，并在 `INT_MAX` 钳住。

`timer_gettime()` 即使在通知 pending 期间，也按原周期推导下一次未来目标和剩余时间。`SIGEV_NONE` 不产生
signal，但读取 timer 时仍必须得到按原目标推进的状态。

**违反表现：** `callback_now + interval` 造成漂移；全局统计代替每 timer overrun；pending signal 期间
`timer_gettime()` 永久返回零或过期目标；两个到期 callback 为同一 pending episode 重复累计。

**依赖：** PT-ID-001、TC-CLOCK-OPS-001。

### PT-SIGNAL-001 — `SI_TIMER` 使用通用 signal，但保留每个 timer 身份

**分类：** Correctness Invariant

**规则：** `SIGEV_SIGNAL` 通过 signal 子系统的 shared pending、disposition、mask、目标选择、唤醒和 frame
路径交付。POSIX timer 不自建 signal queue。每个 pending `SI_TIMER` 保留 timer ID、generation、`sigval`
和必要 overrun episode 身份；不同 timer 即使选择同一个普通 signal，也不能互相覆盖。

普通 `kill()` 产生的 standard signal 继续使用现有单 slot 合并规则。signal 入队入口必须返回“已入队、本
timer 已 pending、被 disposition 忽略、目标已退出”等可判定结果，timer owner 不读取 signal 私有容器猜测。
signal 取出 `SI_TIMER` 后在锁外按 ID/generation 回告；timer 已删除或 generation 不符时只完成 signal 自己的
生命周期，不重新安排对象。

Linux 6.6.32 通过每个 timer 自己预分配的 sigqueue 实现这个区别：
`xref:linux-6.6.32:kernel/signal.c#send_sigqueue` 在该 timer 的 queue 已经 pending 时只增加它自己的
`si_overrun`，否则把这一个 queue item 加入 shared pending；它不经过普通 standard signal 的
`legacy_queue()` 合并。Anemone 不必复制 Linux 对象图，但必须提供相同的用户可见身份和 overrun 结果。

**违反表现：** timer 自建 pending queue；不同 timer 的同号 signal 被合并；signal bit 反向决定 timer 状态；
删除后的 pending 通知重新 arm timer；普通 kill signal 被改成每来源排队。

**依赖：** PT-ID-001、PT-PERIOD-001、
[`SIGNAL-PENDING-001`](../../contracts/signal/pending-routing.md#signal-pending-001--directed-occurrence-只进入对应-pending-owner)、
[`SIGNAL-ACTION-001`](../../contracts/signal/pending-routing.md#signal-action-001--ignored-disposition-在-pending-publication-前生效)。

### TC-RTC-001 — RTC 只提供启动时一次 boot seed

**分类：** Acceptance / Scope Boundary

**规则：** 可选 RTC provider 只在用户态启动前读取一次 Unix Epoch，并用同一初始化窗口的 monotonic sample
建立 `realtime_offset_ns`。之后普通 clock 读取、`clock_settime()` 和 `clock_adjtime()` 不访问 RTC，也不
写回 RTC。无 RTC 时 offset 从零开始。

RTC alarm、运行期轮询、动态 POSIX clock 和写回均在本 RFC target 外。未来增加这些能力必须保持
TC-TIMEKEEPER-001 的单一时间推导链，不能让 RTC 成为第二个运行期 realtime owner。

**违反表现：** 每次读取 realtime 都访问 RTC；设置时间同步写设备；RTC 驱动保存另一份 current realtime；
无 RTC 时拒绝初始化 timekeeper。

**依赖：** TC-TIMEKEEPER-001。

## 状态与能力所有权

| 状态或能力 | 唯一 Owner | 其它参与方持有什么 |
| --- | --- | --- |
| 原始/统一硬件计数与 Hertz | 架构 clock source | timekeeper 只读取 |
| boot counter、realtime offset/change seq、coarse snapshot | timekeeper | clock/timer consumer 取得值或跳变通知 |
| CPU usage 累计计数 | task / thread group | CPU clock 读取 snapshot |
| 一次 soft timer 排队请求 | 对应 CPU queue | 提交者持排队句柄 |
| wait outcome | scheduler wait core | timer 只持唤醒能力 |
| timerfd 到期计数和 cancelled 状态 | `TimerFdCore` | soft timer 只触发一次处理 |
| `ITIMER_REAL` 周期和 armed 状态 | `ThreadGroup::RealITimer` | soft timer 只触发 `SIGALRM` 处理 |
| POSIX timer ID、周期、overrun、generation | `ThreadGroup` timer 表/对象 | soft timer 与 signal 持弱引用或 identity snapshot |
| signal pending、mask、action 和 frame | signal 子系统 | POSIX timer 取得入队结果和锁外交付回告 |

排队句柄、timer ID 和 generation 是协议身份，不是纯诊断字段。CPU ID、clock kind 和 event ID 如果构成物理
删除键，也必须作为明确请求 identity 使用；日志 label 不能反向驱动状态转换。

## 线性化与生命周期

- **读 clock：** 读取统一计数并完成整数换算的 snapshot 是本次读值；realtime offset 必须与所需一致性方式
  同时取快照，不能拼接不同 step 前后的 offset/seq。
- **提交 realtime step：** offset 与 change seq 在 timekeeper 锁内同时发布；锁外通知可以稍后执行，但
  consumer 通过 seq 或当前 realtime 不得漏掉 step。
- **排队：** 请求进入队列并返回句柄后才对提交者发布“当前已排队”；发布失败不留下半个 armed 对象。
- **物理删除：** 从队列移除成功即释放 queue ownership；已经出队则由 generation/wait identity 终止旧处理。
- **timer 删除：** ID 表 removal 先使 syscall 不可见，随后完成 generation、queue 和对象 cleanup。
- **signal 交付：** signal pending owner 先取出并保留不可变 identity，释放 signal 锁后再回告 timer owner。

## RFC-local Proof Obligations

### TC-PROOF-001 — 计数、Hertz 和 clock matrix

用多个可整除和不可整除的测试频率证明纳秒换算、普通/coarse resolution、向上取整和溢出；双架构运行证明
迁移不后退；逐 clock ID 验证每个操作成功或拒绝，不依赖静态数组偶然 alias。

### ST-PROOF-001 — 取消后资源有界

反复创建、设置很远期目标、替换、解除和删除 POSIX timer/timerfd/`ITIMER_REAL`，验证队列长度回到与 live
请求数一致的范围；制造 IRQ 出队与删除竞争，证明旧处理函数不改变新状态或访问已释放对象。

### PT-PROOF-001 — Timer、signal 与 overrun

至少覆盖两个 timer 使用同一普通 signal、一个 timer 通知长期 pending、signal ignored、timer 删除后 pending
交付、周期 callback 延迟、overrun 钳位、fork/exec/exit。普通 kill signal 的单 slot 行为必须回归不变。

### TC-PROOF-002 — Realtime step 没有丢失窗口

覆盖 timerfd snapshot/登记与 realtime step 并发、所有 CPU realtime queue 重检、向前/向后 step、absolute
sleep/POSIX timer/timerfd 的差异，以及 timekeeper 锁内无外部 callback 的 source audit。

## 禁止退化项

- 不允许为了让 syscall 返回成功而忽略 clock ID、flag、mode、通知方式或 CPU-time 语义。
- 不允许以 generation-only lazy cancellation 接受无界旧请求积累。
- 不允许把 queue、timer 对象、signal pending 或 RTC 变成 realtime/period/pending 的并列 owner。
- 不允许通过固定 1ns/10ms、浮点换算或 timer delivery 周期伪造 `clock_getres()`。
- 不允许把本 RFC 的 CPU-time、高精度 timer、RTC 写回和 32 位 ABI 非目标偷偷实现为兼容桥。
