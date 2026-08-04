# Clock Timekeeping 与 POSIX Timers 实施计划

**状态：** Accepted / Gate 0--4 Closed / Gate 5 Authorized
**最后更新：** 2026-08-04
**父 RFC：** [RFC-20260803-clock-timekeeping-posix-timers](./index.md)
**当前修订：** R0

本文只定义实施依赖、独立安全状态、contract cutover、验证和停止条件。目标语义以
[RFC 正文](./index.md)为准，必须证明的 owner/并发/生命周期规则见[目标与不变量](./invariants.md)。文件路径
只是当前实现提示，不是逐文件写入清单。

## 全局 Implementation Boundary

- **Target：** 实现 RFC 开头列出的 10 个 native 64 位 syscall；建立单一 timekeeper、真实 clock
  resolution、可物理删除的 soft timer 请求、`ThreadGroup` POSIX timer 和通用 `SI_TIMER` 交付。
- **Non-goals：** 403--409、CPU-time timer/sleep、高精度或 tickless timer、correction/NTP、
  `SIGEV_THREAD(_ID)`、RTC 写回或运行期轮询。
- **Owner：** 架构 source、timekeeper、soft timer queue、task/thread group、timerfd、signal 分别保持
  [owner 表](./invariants.md#状态与能力所有权)中的唯一状态；本计划不授权迁移 owner。
- **Handoff：** timekeeper 锁外通知 realtime step；soft timer 用句柄返回请求删除能力；POSIX timer 用窄
  `SI_TIMER` 入队和锁外交付回告接入 signal。
- **Failure / cleanup：** 输入 copyin、参数/权限校验失败无副作用；output copyout 的副作用顺序按各 Linux
  syscall 单独定义，不能套用统一回滚规则；queue 插入失败不发布 armed 状态；替换、解除、删除、exec 和
  退出物理删除仍排队请求；在途处理只通过 generation/identity 退出。
- **Protected ABI：** syscall 编号、clock 操作矩阵、支持 flag/mode/notification、errno 和 native time64
  struct 不能在实现中自行改变。
- **Validation claim：** 每个 gate 只声明本 gate 的独立能力；RFC 保持 Accepted，尚未完成的 contract/gate在
  对应双架构 runtime 和用户可见矩阵完成前保持 Not Cut Over。
- **Stop conditions：** 需要改变 target、owner、handoff、failure/cleanup、signal pending 规则、RTC 策略、
  public ABI、contract delta 或验证强度时停止，回到 RFC review。发现 Keter/Apollyon 架构摩擦时不得进入
  下一个 gate 或执行 cutover。

## Gate 0 — 基线冻结与 oracle 准备

**状态：** Closed（2026-08-04）；baseline matrix 与证据见
[transaction](../../devlog/transactions/2026-08-04-clock-timekeeping-posix-timers.md#gate-0-baseline)。

**Purpose：** 在改变时间真相前固定当前调用者、ABI 和架构计数事实，准备不依赖 LTP 的定向语义 oracle。

**Prerequisites：** RFC Draft 和 [TC/ST/PT proof obligations](./invariants.md#rfc-local-proof-obligations) 已可
评审；不要求 accepted target 或实现授权。

**Protected Boundary：** 只增加 validation 或 source evidence，不改变 production 行为，不注册新 syscall，
不修改 current contract。

**Deliverable：**

- 建立八个 clock ID × get/res/set/adjust/sleep/timer 的当前/目标矩阵，确认 107--115、266 和 403--409 的
  双架构 syscall table 状态。
- 列出所有 `Instant::now().to_duration()`、`uptime()`、timerfd、`ITIMER_REAL`、文件/IPC 时间调用者，并按
  日历时间、经过时间、CPU 时间分类；每个 caller 只有一个目标时间域。
- 对 RV64/LA64 source 的计数同步、频率来源和 `F >= SYSTEM_HZ` 条件形成 source evidence；若不能证明跨 CPU
  连续，先将修正限定在架构 source。
- 增加纯整数换算、resolution 和 clock matrix 的 owner-local KUnit/host oracle 入口；不为测试创建无生产
  consumer 的 public probe facade。
- 确认 native `timex`、`timespec`、`itimerspec`、`sigevent` 和 `siginfo` 布局以及 syscall copy 边界。

**Validation：** source audit；换算/resolution 定向测试在可用 host/KUnit 环境编译；`git diff --check`。
本 gate 不运行 QEMU 不构成缺口，因为没有 production 语义改变。

**Cutover：** None。

**Stop / Exit：** 所有当前 consumer 已分类、跨 CPU source 结论明确、ABI 结构和目标矩阵可供后续 gate 使用后
关闭。若发现不同 CPU 频率或计数无法在架构层统一，停止并回到 RFC review，不把 per-CPU offset 塞入
timekeeper。

## Gate 1 — Timekeeper 与 clock 读取

**状态：** Closed / `TC-CLOCK-CUTOVER` Completed（2026-08-04）；
[`TIMEKEEPER-CLOCK-001`](../../contracts/time/clock-derivation.md) Active。

**Purpose：** 建立 RTC-independent 的唯一时间推导链，先让所有 clock get/res 返回真实值，但不开放
realtime mutation、realtime absolute sleep 或 POSIX timer。

**Prerequisites：** Gate 0 关闭；TC-SOURCE-001、TC-TIMEKEEPER-001、TC-RES-001 可由实现直接证明。

**Protected Boundary：** 不改变 scheduler CPU usage owner；不开放 `clock_settime()` / `clock_adjtime()`；不
改变 soft timer queue；不提前注册 POSIX timer syscall。

**Deliverable：**

- 架构 source 向上提供跨 CPU 不后退的统一计数和稳定 Hertz；移除用户 clock 对当前 per-CPU boot baseline
  纪元的依赖。
- timekeeper 建立 `boot_counter`、`realtime_offset_ns=0`、`realtime_change_seq` 和 BSP 更新的
  `coarse_mono_ns`；monotonic/raw 使用同一计算但独立路由。
- `clock_gettime(113)` 对八个 clock ID 返回 RFC 定义的值；CPU clock 继续读取 task/thread group 累计计数。
- `clock_getres(114)` 按 TC-RES-001 使用 `u128` 整数计算，普通/coarse 分开；删除默认 1ns placeholder。
- `gettimeofday()` 和所有日历时间 consumer 切到同一 realtime；uptime/timeout/CPU usage consumer 保持各自
  正确时间域。若不能在本 gate 一次完成 consumer cutover，则不得把 `CLOCK_REALTIME` 新值公开为已切换。
- `CLOCK_BOOTTIME` 保留独立路由；当前无 suspend 时与 monotonic 同值并带关键 ABI 注释。

**Validation：**

- 不同 Hertz 的整数换算/resolution KUnit，覆盖不可整除、频率高于 1GHz、`F == SYSTEM_HZ`、溢出和会让
  realtime offset 变负的输入拒绝。
- 双架构 build；双架构 QEMU 读取八个 clock，验证 monotonic/raw 不后退、coarse 只按 tick 更新、CPU time
  只在运行时增加。
- 多 CPU 配置下做 task migration 连续性测试；如果当前 runner 无法提供 SMP runtime，Gate 1 保持 Not Cut
  Over，不能用单核证据替代 TC-SOURCE-001。
- source audit 证明无浮点、无固定 1ns/10ms、无第二份 realtime。

**Cutover：** `TC-CLOCK-CUTOVER` 激活 `TIMEKEEPER-CLOCK-001`。该 ID 只固定统一计数、整数换算、timekeeper
字段和 clock 读取真相；Gate 3 的 mutation/sleep ABI 仍是未完成 RFC target，不能因本 cutover 写成已实现。

**Stop / Exit：** 八个 clock get/res 和全部现有 consumer 使用一致时间域，双架构 + SMP source 证据成立后
关闭。任何跨 CPU 后退、重复 realtime 或无法原子切换日历 consumer 都阻止关闭。

## Gate 2 — 可物理删除的 soft timer

**状态：** Closed（2026-08-04）；`ST-REQUEST-CUTOVER` 已激活
[`SOFT-TIMER-REQUEST-001`](../../contracts/time/soft-timer-request.md)。

**Purpose：** 把 soft timer 从“只能等待旧 callback 到期”改成拥有排队句柄、可物理删除、在途处理安全失效
的一次请求服务；先迁移不依赖 realtime step 的现有请求路径。

**Prerequisites：** Gate 0 关闭；ST-OWNER-001、ST-CANCEL-001、ST-LOCK-001 已冻结。可与 Gate 1 并行实现，
但后续 consumer cutover 以 Gate 1 timekeeper 读取能力为前提。

**Protected Boundary：** soft timer 不取得 POSIX timer、timerfd、itimer 或 wait 状态所有权；不同时实现
tickless/high-resolution；不把容器类型扩大成 public contract。

**Deliverable：**

- queue 支持 `(到期时刻, event identity)` 有序取出和按排队句柄删除；event identity 回绕不能删除新请求。
- 明确 queue ownership、remote cancel、IRQ 出队、threaded ready queue 和 callback 执行的线性化点与锁序；
  queue 锁内不运行外部 callback。
- 保留 IRQ/threaded 两条执行 lane；threaded lane 中已出队请求用 generation/identity 检查旧处理是否仍有效。
- timerfd 重设、解除和最后引用关闭物理删除旧请求；monotonic/相对请求的周期从原目标推进。本 gate 不提前
  宣称 absolute realtime 或 `TFD_TIMER_CANCEL_ON_SET` 已完成。
- `ITIMER_REAL` 重设、解除和线程组 teardown 物理删除旧请求；周期从原目标推进，继续通过通用 signal 发送
  `SIGALRM`。
- 现有 `nanosleep()` / `clock_nanosleep()` 相对 timeout 请求迁移到新句柄；syscall 离开时删除仍排队请求，
  wait outcome 仍由 wait-core 独占。完整 clock 路由和 absolute sleep 留到 Gate 3。

**Validation：**

- owner-local KUnit 覆盖同目标多请求、删除、重复删除、event ID 冲突/回绕、IRQ 出队与 cancel 竞争、threaded
  stale callback、remote CPU cancel 和锁序断言。
- 压力反复 set/delete 很远期 timerfd/itimer/sleep，证明队列 live entry 与实际 armed 请求有界一致；不能只
  证明旧 callback 无效果。
- 双架构 timerfd monotonic/relative、`ITIMER_REAL`、nanosleep runtime；明确不把 absolute realtime 或
  cancel-on-set 记为本 gate 通过。
- source audit timer IRQ/IRQ-off return path，确认新 queue/ready 交接没有引入 blocking lock、普通日志或无界
  allocation 副作用。

**Cutover：** `ST-REQUEST-CUTOVER` 激活 `SOFT-TIMER-REQUEST-001`。该 ID 固定一次请求 owner、排队句柄、
物理删除和在途失效；timerfd realtime/cancel-on-set 仍明确等待 Gate 3。

**Stop / Exit：** 物理删除、在途竞态、现有 consumers 和双架构 runtime 全部闭合，旧不可删除接口没有
对应已迁移 production caller 后关闭。若 queue 需要同时持多 CPU 锁、回调在 queue 锁内执行或资源仍随历史
操作数增长，Gate 2 不得关闭。

## Gate 3 — Realtime mutation 与完整 clock sleep

**状态：** Closed；`TC-STEP-CUTOVER` Completed。

**Purpose：** 在可物理删除请求已经成立后，开放 `clock_settime(112)`、`clock_adjtime(266)` 和完整
`clock_nanosleep(115)`，并闭合 timerfd realtime/cancel-on-set 对已生效 timekeeper/soft-timer contract 的消费。

**Prerequisites：** Gate 1、Gate 2 关闭；TC-STEP-001、TC-CLOCK-OPS-001 已冻结；timerfd、itimer 和 sleep
已经能持有并删除排队句柄。

**Protected Boundary：** 不注册 POSIX timer syscall；不把周期 polling 当作 realtime step 通知；不允许
generation-only 重新成为取消路径。

**Deliverable：**

- `clock_settime()` 实现 `CLOCK_REALTIME`、`CAP_SYS_TIME`、完整输入/溢出检查、不得早于当前 monotonic 和
  offset/change-seq 原子提交。
- `clock_adjtime()` 实现查询与 `ADJ_SETOFFSET`，`ADJ_NANO/MICRO` 只作单位修饰；其它模式按 RFC 明确拒绝并
  打日志，查询报告未同步状态和真实 precision。
- realtime step 锁外发布跨 CPU 通知；consumer 通过 change seq 或等价协议不能漏掉 snapshot/登记并发。
- `clock_nanosleep()` 实现 clock 路由、`TIMER_ABSTIME`、相对请求的 monotonic 固化、absolute realtime 的
  双向 step 行为、signal 中断和 remaining time；其它 flag 位按 Linux legacy ABI 静默忽略并做限频日志；
  复用 wait-core 单轮 owner和 Gate 2 排队句柄。
- timerfd absolute realtime 接入 realtime queue；`TFD_TIMER_CANCEL_ON_SET` 在 step 时物理删除请求、进入
  cancelled 状态并真实返回 `ECANCELED`。
- `nanosleep(101)` 继续调用 monotonic 相对 sleep；`ITIMER_REAL` 相对运行不受 realtime step 影响。

**Validation：**

- clock_set/adj 的 bad pointer、非法 timespec、permission、clock ID、mode/flag、溢出和 input-validation
  no-side-effect 测试。
- 相对/绝对 realtime/monotonic/boottime sleep 的正常到期、signal interruption 和 remaining；realtime 向
  前/向后 step 并发测试。
- timerfd absolute realtime/cancel-on-set、change seq snapshot/登记竞态、所有 CPU realtime queue 重检。
- source audit 证明 timekeeper 锁内没有 queue/wait/signal callback；双架构 build/runtime。

**Cutover：** `TC-STEP-CUTOVER` 激活 `TIMEKEEPER-STEP-001`，固定 offset/change seq 原子提交、timekeeper
锁外通知、跨 CPU realtime 重检和 timerfd cancel-on-set 无丢失协议；同时继续满足 Gate 1/2 已生效规则。

**Stop / Exit：** 三个 clock syscall、timerfd realtime/cancel-on-set、相对/绝对 sleep 和双架构 step 行为都
成立后关闭。任何漏掉跨 CPU step、无法物理删除请求或相对 sleep 受日期修改影响都阻止 cutover。

## Gate 4 — `SI_TIMER` signal 协议

**状态：** Closed（2026-08-04）；实现、review 与双架构证据见
[transaction](../../devlog/transactions/2026-08-04-clock-timekeeping-posix-timers.md#gate-4-implementation)。

**Purpose：** 在创建 POSIX timer 对象前，先让通用 signal owner 能保存每个 timer 的独立 pending identity，
并提供锁外入队结果和交付回告。

**Prerequisites：** Gate 3 的 timekeeper/soft-timer contract 已 cut over；signal current contract 已读取；
PT-SIGNAL-001 和 ST-LOCK-001 可证明。

**Protected Boundary：** 普通 `kill()` standard signal 继续单 slot 合并；signal 仍拥有 mask/disposition/target/
frame；本 gate 不创建 timer ID 表，不注册 timer syscall，也不让 signal 保存 timer schedule。

**Deliverable：**

- signal 内部增加来源为 POSIX timer 的 `SI_TIMER` pending 项，携带 timer ID、generation、`sigval` 和必要
  episode identity；即使 signum 是普通 signal，不同 timer 也独立保存。
- 窄入队入口先应用 live ignored disposition，再返回 queued/already-pending/ignored/target-exited 等明确
  outcome；调用者不读取 signal 私有 bit/queue。
- pending 取出后保留不可变 identity，释放 signal 锁，再回告 timer owner；目标不存在或 generation 不符时
  只完成 signal cleanup。
- signal frame 在 RV64/LA64 正确生成 `SI_TIMER` 的 `si_code`、timer ID、overrun snapshot 和 `sigval`；ABI
  struct 只存在于 signal copy 边界。
- preallocated notification resource 或等价机制的分配失败必须在未来 `timer_create()` 时可返回，不允许 timer
  到期时静默丢失首个通知。具体资源表示属于实现选择；Linux 的语义依据见
  `xref:linux-6.6.32:kernel/time/posix-timers.c#alloc_posix_timer` 和
  `xref:linux-6.6.32:kernel/signal.c#send_sigqueue`。

**Validation：**

- signal owner-local KUnit：同 signum 不同 timer、同 timer 重复 expiry、ignored disposition、删除后交付、
  generation stale、普通 kill 合并不变、realtime signal FIFO 不退化。
- 双架构 signal-frame 用户态 oracle 检查 `SI_TIMER` 字段；普通 signal regression。
- 锁序/source audit 证明 pending lock 外回告，不存在 signal lock -> timer object lock 回环。

**Cutover：** None。`SIGNAL-PENDING-001` 尚不能只凭内部能力改变 current contract；它与真实 POSIX timer
consumer 在 Gate 5 原子切换。

**Stop / Exit：** signal 协议能独立 fail closed、普通 signal 语义不变、双架构 frame 已证明后关闭。若需要
timer 子系统自建 signal queue、signal 私有容器外泄或普通 kill 改成按来源排队，停止并回到 RFC review。

## Gate 5 — POSIX timer 对象与五个 syscall

**状态：** Authorized / Not Started。

**Purpose：** 建立 `ThreadGroup` timer ID 表、对象生命周期、周期/overrun 和五个 syscall，消费 Gate 2/4
已经完成的 soft timer 与 signal 能力。

**Prerequisites：** Gate 3、Gate 4 关闭；PT-ID-001、PT-PERIOD-001、PT-SIGNAL-001 可证明；native UAPI layout
已在 Gate 0 冻结。

**Protected Boundary：** 只支持 realtime/monotonic/boottime 和 `SIGEV_NONE`/`SIGEV_SIGNAL`；不实现 CPU-time
timer、`SIGEV_THREAD(_ID)`、全局 ID namespace 或 fd 化 timer。

**Deliverable：**

- `ThreadGroup` 内 timer ID 表和对象；ID publication/removal、数字复用、锁和引用生命周期符合 PT-ID-001。
- `timer_create(107)` 校验 clock/sigevent，预先取得通知所需资源，分配尚未对 syscall lookup 生效的 ID/object，
  先 copyout ID，再完成对象初始化并发布到 `ThreadGroup` 表；任一步失败都撤销 ID/object/resource。该顺序
  对应 `xref:linux-6.6.32:kernel/time/posix-timers.c#do_timer_create`。
- `timer_settime(110)` 实现相对/绝对目标、旧值 snapshot、disarm/replace、generation、排队句柄和周期。
  flags 只解释 `TIMER_ABSTIME`，其它位按 Linux legacy ABI 静默忽略并限频日志。
  Linux 先修改 timer，再 copyout old value；old-value copyout 失败返回 `EFAULT`，但新设置保持生效，不回滚。
  该可见顺序必须由定向 oracle 固定，见
  `xref:linux-6.6.32:kernel/time/posix-timers.c#SYSCALL_DEFINE4(timer_settime)`。
- `timer_gettime(108)` 从对象原目标/周期和当前 clock 推导 snapshot；`timer_getoverrun(109)` 返回最近一次已
  交付值并钳位；两者不读取 soft timer 私有队列作为 timer truth。
- `timer_delete(111)` 先移除 ID 可见性，再推进 generation、物理删除请求和释放对象；pending `SI_TIMER` 可按
  signal 生命周期交付但不能 rearm。
- 周期 timer 在 notification pending 时不重复入队同一 timer 通知；按原目标累计 overrun，交付或 ignored
  后推进到下一未来目标。
- fork 不继承；成功 exec 和最后成员退出批量执行同一删除协议；线程组任意成员可按 ID 操作。

**Validation：**

- owner-local KUnit：ID 分配/复用、create copyout rollback、settime old-value copyout `EFAULT` 后新设置仍
  生效、disarm/replace/delete、相对/绝对、周期延迟、overrun 钳位、pending notification、ignored signal、
  删除/expiry/交付竞争、fork/exec/exit。
- 用户态 Linux-semantics oracle：五个 syscall 的 bad pointer、invalid ID/clock/flag/sigevent/timespec、默认
  sigevent、`SIGEV_NONE`、同 signal 多 timer、interval 和 overrun。
- 双架构 build/runtime；确认 107--111 注册且 403--409 未注册。
- signal、timerfd、`ITIMER_REAL`、sleep 回归；队列资源压力继续满足 Gate 2 有界证明。

**Cutover：** `PT-SIGNAL-CUTOVER` 原子激活 `POSIX-TIMER-001`，并 Refine `SIGNAL-PENDING-001` 使
`SI_TIMER` 独立 pending 语义生效。
如果五个 syscall 任何一个仍是 stub/partial success，两个 ID 都不得 cut over。

**Stop / Exit：** 五个 syscall、对象生命周期、signal/overrun 和双架构 runtime 全部闭合后关闭。任何
generation-only cleanup、timer 自建 signal queue、fork 继承、ID 跨线程组可见或 unsupported feature 假成功都
阻止 cutover。

## Gate 6 — 最终消费者审计与 RFC 收口

**Purpose：** 证明 10 个 syscall 和现有时间消费者共同使用一套时间/请求/signal 事实，完成最终 contract
与文档收口。

**Prerequisites：** Gate 1--5 关闭，Gate 1/2/3/5 的适用 cutover 已完成；没有 active Keter/Apollyon finding。

**Protected Boundary：** 不在收口阶段加入 CPU-time timer、高精度 timer、RTC 接入或新 clock ID；新能力走
follow-up RFC。

**Deliverable：**

- 重新审计全部日期/经过/CPU-time consumer，确认没有旧 boot-relative realtime、不可删除 production timer
路径或第二份 POSIX pending。
- 确认所有关键 ABI 取舍、静默兼容、unsupported flag/mode 和临时桥都有源内注释、可观测日志和删除条件；无
临时桥则不创建占位说明。
- 按实际 cutover 更新 current contract 和 RFC closure；未实现能力只登记真实 current limitation，不把
target 内 bug 降格成 limitation。
- 完成 Architecture Friction Scan：第二份状态、owner 穿透、private representation 泄漏、调用者/架构/测试
特判、隐含 cleanup 顺序和无真实义务的抽象层。

**Validation：**

- RV64/LA64 repository-owned kernel build、targeted QEMU 用户态 matrix 和现有 time/timer/signal regression。
- SMP runtime；反复 stress；所有 KUnit/host oracle；`git diff --check`、mdBook build。
- LTP clock/timer/timerfd/itimer 可以作为额外回归和兼容证据，Vim 可以作为集成 smoke；两者都不能替代
  Linux/POSIX 语义 oracle。

**Cutover：** 确认 Gate 1/2/3/5 已完成五项 Contract Impact，没有遗漏或提前生效的 current rule；本 gate 本身
不引入新 contract ID。

**Stop / Exit：** 证据、current contract、RFC closure 和 register/current limitation 一致后关闭 RFC。
双架构或 SMP 必需证据缺失时保持 Not Cut Over；不得用“能启动 Vim”替代 timer 生命周期和 ABI 证明。

## 跨 Gate 验证矩阵

| 能力 | 最早生产 gate | 必须验证 |
| --- | --- | --- |
| 普通/raw/realtime/coarse/boottime get/res | Gate 1 | 多 Hertz 整数计算、双架构、SMP 不后退 |
| realtime set/adj | Gate 3 | 权限、输入校验失败无副作用、跨 CPU step |
| relative/absolute clock sleep | Gate 3 | clock/flag、句柄删除、signal/remaining、双向 step |
| timerfd/`ITIMER_REAL` 可删除请求 | Gate 2 建立，Gate 3 闭合 realtime | 资源有界、出队竞态、周期不漂移、cancel-on-set |
| `SI_TIMER` pending/frame | Gate 4 内部能力，Gate 5 对外 cutover | 同 signal 多 timer、普通 signal 回归、双架构 frame |
| 五个 POSIX timer syscall | Gate 5 | ID/lifecycle、周期/overrun、signal、fork/exec/exit |

## Target Renegotiation 触发器

以下证据不能由实现阶段自行“简化”，必须停止并回 RFC review：

- 平台无法提供跨 CPU 统一计数，只能在 timekeeper 保存 per-CPU 时间真相；
- 可删除队列要求改变 IRQ/threaded timer 的 owner 或允许无界 lazy cleanup；
- `SI_TIMER` 无法在 signal owner 内按 timer 身份保存，只能建立 timer 私有 pending queue；
- native ABI 的 errno、struct、支持 clock/notification 或 syscall 编号需要改变；
- 物理取消、timer lifecycle、overrun 或双架构/SMP validation 只能形成比 RFC 更弱的保证；
- 需要把 CPU-time timer、高精度 timer、RTC 接入或 32 位 ABI 纳入当前 acceptance 才能继续。

review 只能选择保持 target 并改路线、接受一个自洽的修订 target、拆 follow-up RFC 或 Not Cut Over；agent 不
能自行批准 reduced target。
