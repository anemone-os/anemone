# Clock Timekeeping 与 POSIX Timers 事务日志

**状态：** Active / R0 / Gate 0--4 Closed / Gate 5 Authorized
**日期：** 2026-08-04
**负责人：** doruche, Codex
**RFC：** [RFC-20260803-clock-timekeeping-posix-timers R0](../../rfcs/clock-timekeeping-posix-timers/index.md)
**实施计划：** [Gate 0--6](../../rfcs/clock-timekeeping-posix-timers/implementation.md)
**适用修订：** R0
**Contract Cutover：** `TC-CLOCK-CUTOVER`、`ST-REQUEST-CUTOVER`、`TC-STEP-CUTOVER` Completed；`TIMEKEEPER-CLOCK-001`、`SOFT-TIMER-REQUEST-001`、`TIMEKEEPER-STEP-001` Active；其它 R0 contract delta pending

## 边界

本 transaction 记录四个独立 checkpoint。Gate 0--1 冻结 native ABI、clock operation、architecture source 和
caller domain baseline，并切换统一 counter、integer Hertz conversion、timekeeper clock read truth、八个
get/res route 和 calendar consumer。Gate 2 随后建立可物理删除的 soft timer request，并迁移 timerfd、
`ITIMER_REAL` 和 wait timeout。Gate 3 开放 realtime mutation、完整 clock sleep 与 timerfd realtime/
cancel-on-set。Gate 4 在不创建 timer 对象或注册 syscall 的前提下建立 signal-owned `SI_TIMER` pending、预分配
slot、锁外 handoff 和双架构 frame oracle。四个 checkpoint 各自保留 review、validation 和 commit 边界；
Gate 4 没有 contract cutover。

本 transaction 尚不实现或声明 POSIX timer 对象、107--111 syscall 或 RTC seed。Gate 4 内部能力只有在 Gate 5
接入真实 timer consumer 并完成 `PT-SIGNAL-CUTOVER` 后才成为 current contract；开发者已授权继续 Gate 5。

## Gate 0 baseline

### Syscall 与 native ABI

| Syscall | Gate 0/1 current state | R0 route |
| --- | --- | --- |
| 107--111 `timer_*` | 未注册 | Gate 5 |
| 112 `clock_settime` | 未注册 | Gate 3 |
| 113 `clock_gettime` | 已注册；Gate 1完成八个 clock read | Gate 1 closed |
| 114 `clock_getres` | 已注册；Gate 1改为真实 source/coarse resolution | Gate 1 closed |
| 115 `clock_nanosleep` | 既有 monotonic-relative 实现，完整 clock/absolute ABI 未宣称完成 | Gate 3 |
| 266 `clock_adjtime` | 未注册 | Gate 3 |
| 403--409 compat time64 | RV64/LA64 native table均不注册 | R0 non-goal，保持 `ENOSYS` |

native layout audit：`timespec=16`、`itimerspec=32`、`sigevent=64`、`siginfo=128`、
`__kernel_timex=208` bytes。10 个 R0 syscall 使用 64-bit seconds 的 native ABI；403--409 不作为 native alias。

### Clock operation matrix

| Clock | Gate 1 effective get/res | 尚未 cut over 的操作 |
| --- | --- | --- |
| realtime | monotonic + zero realtime offset；source resolution | set/adjust、完整 relative/absolute sleep、POSIX timer |
| monotonic | BSP boot counter 起点；source resolution | 完整 relative/absolute sleep、POSIX timer |
| process CPU | thread-group CPU usage；source resolution | CPU-time sleep/timer 为 R0 non-goal |
| thread CPU | task CPU usage；source resolution | CPU-time sleep/timer 为 R0 non-goal |
| monotonic raw | 与 monotonic 同值的独立 route；source resolution | sleep/timer 不支持 |
| realtime coarse | BSP coarse snapshot + zero offset；coarse resolution | sleep/timer 不支持 |
| monotonic coarse | BSP coarse snapshot；coarse resolution | sleep/timer 不支持 |
| boottime | 与 monotonic 同值的独立 route；source resolution | 完整 relative/absolute sleep、POSIX timer |

Gate 1 只 cut over get/res。既有 syscall 115 的窄行为不作为完整 sleep matrix 证据，未知 clock/operation 的
最终 errno 与 legacy flags 仍以 Gate 3/5 target 为准。

### Architecture source matrix

| Architecture | Counter | Hertz | Cross-CPU conclusion |
| --- | --- | --- | --- |
| RV64 | `time` CSR | `/cpus/timebase-frequency`，boot-stable `MonoOnce` | platform timebase 是 shared domain；timekeeper 不再作 per-CPU correction |
| LA64 | `RDTIME` | firmware，缺失时由 CPUCFG 4/5 计算并存入 `MonoOnce` | architecture source 在每 CPU timekeeping readiness 前写 BSP-owned CNTC correction |

LA64 correction逐式匹配 RFC 固定的 Linux 6.6.32：BSP 计算 `-(drdtime() - CNTC)`，随后 BSP/AP 写相同
CNTC。QEMU 10/11 的 `cpu_loongarch_get_constant_timer_counter()` 直接返回 shared virtual counter并忽略 CNTC，
因此 SMP QEMU证明 cross-CPU route/order，不证明物理 CNTC application；符号与实机模型由 pinned source audit
承担。`RDTIME.D` 的第二 operand 已修正为 architectural output counter ID，不再错误作为 input。

### Caller domain matrix

| Consumer | Domain after Gate 1 | Audit boundary |
| --- | --- | --- |
| `gettimeofday` | realtime/calendar | `time/api/gettimeofday.rs` |
| file/inode create、write、chmod/chown、`utimensat`、boot metadata | realtime/calendar | `boot.rs`、`fs/{file.rs,inode,api/fchmod,api/fchown,api/utimensat,vfs/ops.rs}` |
| procfs inode metadata | realtime/calendar | `fs/proc/{pde.rs,meminfo.rs,root,tgid,uptime.rs}`；`/proc/uptime` payload仍是 monotonic |
| SysV shared-memory timestamps | realtime/calendar | `mm/uspace/shm/segment.rs` |
| printk timestamp、`sysinfo.uptime`、`times()`、`/proc/uptime` payload | monotonic elapsed | `debug/printk`、`debug/api/sysinfo.rs`、`time/api/times.rs`、`fs/proc/uptime.rs` |
| scheduler、iomux/epoll、device I/O、network、soft timer、itimer timeout | monotonic elapsed | 所有 `Instant::now()`/`uptime()` caller保持 elapsed-time domain |
| process/thread CPU accumulation | task/thread-group CPU counts | `task/cpu_usage.rs`；timekeeper不保存第二份 CPU usage |
| futex `FUTEX_CLOCK_REALTIME` | pre-existing unsupported flag path | `task/api/futex/futex.rs`仍记录 warning并按既有 monotonic timeout执行；未迁移为 calendar semantics，也不计入 Gate 1完成面 |

全仓 caller audit未发现其它 `Instant::now().to_duration()` calendar consumer。RTC driver没有进入普通 read path。

## Gate 1 implementation

- common timekeeper 只保存 BSP `boot_counter`、stable frequency/counts-per-tick、nonnegative realtime read
  snapshot和BSP coarse snapshot；删除旧 per-CPU `BOOT_MONO` / `BSP_BOOT_MONO` baseline。
- monotonic/raw/realtime/boottime按单一 counter chain推导，coarse只由BSP tick发布；八个 ID 保持独立 route。
- source/coarse resolution用 `u128` integer ceil计算；没有 production fixed 1ns/10ms 或浮点换算。
- calendar consumer原子切换到 `realtime()`；elapsed/CPU consumer保持原 owner。
- user-test直接调用113/114，覆盖八个 ID、ordered reads、resolution class、derivation、blocked CPU time、
  invalid ID和`clock_getres(NULL)`。

## Review 与 validation

- 独立 Gate 0/1 review 最终无 Apollyon/Keter。初始 LA64 CNTC 符号 finding 经 RFC-pinned Linux 6.6.32
  逐式复核后撤回；review确认 QEMU CNTC proof boundary。
- 两项 Euclid oracle finding已在 cutover 前消除：coarse realtime改为 monotonic-coarse bracket read；blocked
  CPU-time test改为50ms sleep且CPU delta必须小于 wall elapsed 的四分之一。
- RV64 SMP=2 exact source：395/395 KUnit，两个 scheduler core均启动；user clock oracle通过，source
  resolution 100ns、coarse resolution 10ms。
- LA64 SMP=2 exact source：396/396 KUnit，两个 scheduler core均启动；user clock oracle通过，source
  resolution 10ns、coarse resolution 10ms。
- 双架构 pretest rootfs 由各自 manifest 重新构建，确保用户 oracle 是最新二进制；两次 QEMU 在成功标记后
  进入非 Gate 1 socket/pretest流程，外层 timeout不改变 clock结果。
- source audit：无 timekeeping float、无 production fixed resolution、无 duplicate realtime、无 common
  per-CPU baseline、403--409未注册；`just fmt kernel --check`、`just fmt user-test --check`和
  `git diff --check`通过。mdBook检查按开发者明确指示全部跳过，记为 Not Run。
- LTP Not Run：可用 competition test image 缺少 static BusyBox/LTP payload，runner在进入 LTP前停止。LTP是
  额外回归而非 Gate 1语义定义，不把 attempted=0伪装为通过。

## Gate 1 closure 与 TC-CLOCK-CUTOVER — 2026-08-04

`TC-CLOCK-CUTOVER` 原子激活
[`TIMEKEEPER-CLOCK-001`](../../contracts/time/clock-derivation.md#timekeeper-clock-001--所有-clock-读取来自一条整数推导链)。
该 current contract 只固定 architecture source、integer conversion、timekeeper read truth、clock get/res与
calendar caller domain。`TIMEKEEPER-STEP-001`、`SOFT-TIMER-REQUEST-001`、`POSIX-TIMER-001`和
`SIGNAL-PENDING-001` Refine继续 pending。

Architecture Friction Scan未发现第二份 mutable time truth、owner penetration、private ABI leakage、提前 Gate 2
capability或无真实义务的 abstraction。Gate 2 是下一已授权 gate，但其实现、review、validation与 cutover不属于
本 Gate 1 commit。

## Gate 2 implementation

- soft timer 使用每 CPU `NoIrqSpinLock<TimerQueue>` 保存单一最小堆；全局 event identity 不回绕，opaque
  `TimerHandle` 只携带 owner CPU 与 request identity。
- cancel 只锁本地或一个 remote CPU queue，物理移除仍排队请求；IRQ 到期批量出队、callback 执行和被取消
  callback 的析构都发生在 queue 锁外。
- wait-core 保存 timeout handle 并在任意 wait return 后删除请求；已经出队的 callback 继续由 `WakeToken`
  identity 拒绝旧 round。
- timerfd 在 replace、disarm、due refresh 和最后引用关闭时删除旧请求；`ITIMER_REAL` 在 replace、disarm 和
  owner teardown 时执行同一 cleanup。两者的 generation/validness 只拒绝已经出队的旧 completion。
- timerfd 与 `ITIMER_REAL` 的周期都从原目标推进；timerfd expiry count 与 `ITIMER_REAL` signal commit 继续由
  各自长期对象拥有。realtime step、absolute clock sleep、cancel-on-set、POSIX timer 与 RTC 均未进入本 gate。
- 用户态 oracle 反复 replace/disarm timerfd 与 `ITIMER_REAL`，读取周期 timerfd，到期投递 `SIGALRM`，并验证
  `SIGALRM` 提前中断 nanosleep。

## Gate 2 review 与 validation

- change review 未发现 Apollyon/Keter/Euclid。source audit 确认 queue 锁内无 callback/Drop、remote cancel 不
  同时持两个 CPU queue lock、production consumer 不以 generation-only 取消，也没有在新增 IRQ/IRQ-off return
  path 引入 blocking lock、普通日志或新的无界分配。
- 15 个新增 owner-local KUnit 覆盖 heap identity/order/remove、ID exhaustion、重复删除、IRQ dequeue、锁外
  callback drop、remote CPU cancel、远期请求有界、wait 提前唤醒、timerfd replace/disarm/refresh/close、
  `ITIMER_REAL` replace/disarm/teardown 和周期原目标推进。
- RV64 `log-acceptance.toml` release SMP=2 当前源码运行通过 410/410 KUnit、`All tests passed!` 和
  `soft-timer: timerfd, ITIMER_REAL, and interrupted nanosleep checks passed`。
- LA64 release SMP=2 的较早 Gate 2 运行通过 411/411 KUnit、`All tests passed!` 和同一用户 marker。最终源码
  exact build 通过；clean-rootfs 复跑中全部 15 个 Gate 2 新增 KUnit（包括 remote cancel）通过，随后既有
  `device::tty::file::kunits::set_modes_commit_after_drain_and_flush_only_for_tcsetsf` 在唤醒 worker 后断言 output
  仍 pending，因 worker 已消费而中止全局 marker。该 fixture 竞态不读取 timer request 状态，且不修改 TTY
  属于本 Gate 的明确边界；本记录不把这次全局复跑写成 411/411。
- 双架构 exact release build均通过；pretest rootfs 由对应 manifest 重新构建。QEMU 在用户 marker 后因没有
  competition `/dev/vdb` 测试盘停留，外层 timeout 只负责结束 guest，不作为失败归因。
- `just fmt kernel --check`、`just fmt user-test --check` 与 `git diff --check` 通过。mdBook 检查按开发者明确
  指示全部跳过，记为 Not Run。

## Gate 2 closure 与 ST-REQUEST-CUTOVER — 2026-08-04

`ST-REQUEST-CUTOVER` 原子激活
[`SOFT-TIMER-REQUEST-001`](../../contracts/time/soft-timer-request.md#soft-timer-request-001--排队句柄物理删除一次请求)。
该 current contract 只固定一次请求 owner、排队句柄、物理删除、queue/执行 lane 锁序和已出队 stale
completion 交接；realtime mutation、absolute clock sleep、timerfd cancel-on-set 与 POSIX timer 仍保持 pending。

Architecture Friction Scan未发现第二份 request/object 状态真相、owner penetration、private queue representation
泄漏、为局部 consumer 扩大 public API、无退出条件的临时桥、隐含 cleanup 顺序或无真实义务的 abstraction。
在该次 Gate 2 closure 时 Gate 3 尚未获授权；后续开发者验收 Gate 2 并单独授权 Gate 3。

## Gate 3 implementation

- RV64/LA64 native ABI 注册 `clock_settime(112)` 与 `clock_adjtime(266)`，增加显式 padding 的 208-byte
  `Timex` 和 `ECANCELED` 映射；root capability effective 集合开放 `CAP_SYS_TIME`。
- timekeeper 在唯一锁内提交 nonnegative realtime offset 与 nonwrapping change sequence，返回 crate-local
  must-use step token；publisher 只在锁外逐 CPU、一次一个 queue lock 重检 absolute realtime request。
- soft timer 保留原 monotonic heap并增加 private realtime heap。request 保存 absolute deadline 与可选
  cancel sequence；insert-side named recheck关闭 snapshot/登记窗口，observed sequence 单调前进，旧 scanner
  不能取消新 request 或用 stale calendar 值提前到期。
- `clock_nanosleep()` 显式路由 realtime/monotonic/boottime，CPU clock 返回 `EOPNOTSUPP`，raw/coarse/unknown
  返回 `EINVAL`。相对请求固定到 monotonic 纳秒 deadline；absolute realtime 使用窄 wait-core timeout trigger
  和可物理删除 request。legacy unknown flag bits按 Linux ABI 静默忽略并只记录一次日志。
- timerfd 继续由 `TimerFdCore` 独占 schedule、generation、expiry count 与 cancelled 状态。relative realtime
  固定为 monotonic；absolute realtime进入 realtime heap；cancel-on-set只接受 realtime + absolute，step物理
  移除请求并唤醒 read/poll，下一次 read 返回一次 `ECANCELED`，replacement拒绝旧 completion。
- 用户 oracle 覆盖 set/adj mode、permission、bad pointer、unsupported/invalid 错误，三种 clock 的相对/绝对
  sleep、signal remaining、realtime forward/backward 并发，以及 relative/absolute/cancel-on-set timerfd。

## Gate 3 review 与 validation

- change review 在 cutover 前修复两项 correctness finding：旧 step scanner 在 newer backward step 后不能用
  stale realtime 提前到期；`clock_adjtime(ADJ_SETOFFSET)` 在 mutation 前 fault-in 完整 copyout 范围，避免
  read-only/bad output pointer产生副作用。最终未发现 residual Apollyon/Keter/Euclid。
- owner-local KUnit新增 mutation no-side-effect、timex layout、realtime heap双向判断、cancel sequence、旧
  scanner、insert-side recheck、remote CPU recheck、timerfd invalid flags、relative fixation、physical cancel、
  一次 `ECANCELED` 与 stale replacement；既有 Gate 1/2 KUnit继续运行。
- RV64最终源码 release SMP=2通过425/425 KUnit、`All tests passed!`、soft-timer marker与
  `clock-step: realtime mutation, sleep, and timerfd checks passed`。
- LA64最终源码 release SMP=2通过426/426 KUnit、同一全量与用户 marker。两份 QEMU日志均在 marker后进入
  非 Gate 3 socket回归并通过，随后因未提供competition `/dev/vdb`按预期停止；该缺盘不归因于本 Gate。
- 双架构 pretest rootfs 通过 Docker `gallant_lamarr` 从 tracked manifest重新生成；双架构 exact release build
  通过。source audit确认timekeeper锁内没有queue/wait/timerfd callback、任一时刻只持一个CPU queue lock、
  relative sleep/timerfd/`ITIMER_REAL` 不消费 realtime offset。
- `just fmt kernel --check`、`just fmt user-test --check` 与 `git diff --check` 作为最终机械检查执行。所有 mdBook
  检查按开发者明确要求跳过，记为 Not Run；LTP不是本 Gate语义 oracle，本次 Not Run。

## Gate 3 closure 与 TC-STEP-CUTOVER — 2026-08-04

`TC-STEP-CUTOVER` 原子激活
[`TIMEKEEPER-STEP-001`](../../contracts/time/realtime-step.md#timekeeper-step-001--realtime-step-不能漏掉或误用旧-timeline)。
该 current contract 固定 offset/change-seq 原子提交、timekeeper锁外发布、全CPU realtime request重检、
snapshot/登记无丢失、旧scanner拒绝、relative monotonic fixation与timerfd cancel-on-set owner交接；Gate 1/2
current contract继续满足。

## Gate 4 implementation

- Signal shared pending 为每个未来 POSIX timer 预分配独立 slot；standard signal 不复用 ordinary 单 slot，
  realtime timer occurrence 与普通 realtime queue 共用 arrival identity，保持同 signum FIFO。
- registration 在创建时完成 fallible slot allocation；expiry 只提交 timer ID、generation、episode、overrun 和
  `sigval`，得到 `Queued / AlreadyPending / Ignored / TargetExited`，不读取 pending 私有容器。
- pending episode 可在仍排队时原子更新为最新 generation/overrun；dequeue 后用计数跟踪 immutable in-flight
  handoff，使新 generation 可以复用已释放的预分配 pending resource，同时阻止所有 callback 完成前复用 slot。
- 正常 delivery、同步 wait、disposition/job-control flush 和 task-private reservation exit 都先释放 pending 与
  ThreadGroup guard，再清 slot 并回告 timer owner。registration 删除只撤销未来 enqueue，已 pending identity
  继续按 signal 生命周期完成；stale generation callback 不取得 rearm authority。
- `Signal` 的 timer handoff 不可 clone；`rt_sigqueueinfo(SI_TIMER)` 只在 syscall copy 边界解析 timer union，并把
  `si_sys_private` 清零。Gate 4 用户态 oracle 暂用 Linux 允许的负 `si_code` 注入 frame；Gate 5 真实 expiry
  成为 primary oracle 后移除该测试桥的 primary 地位。
- 107--111 仍未注册。Gate 4 registration 暂时 fail closed 拒绝 `SIGKILL/SIGSTOP`；Gate 5 必须先把两者接入
  unmaskable/job-control generation 语义并删除 guard，不能把该内部阶段边界发布成 `timer_create()` 的 `EINVAL`。

## Gate 4 review 与 validation

- change review 修复一项 correctness finding：旧 bool `in_flight` 会把新 generation expiry 合并到已 dequeue 的
  stale episode，随后 stale callback 按 invariant 不 rearm，导致新 timer 停止。最终协议允许 dequeue 后立即
  queue 新 episode，并以 in-flight count 延迟 slot reuse；无 residual Apollyon/Keter/Euclid。
- 10 项新增 signal owner-local KUnit 覆盖同号不同 timer、pending episode/overrun 更新、ignored、删除/stale
  handoff、ordinary standard merge、ordinary/timer realtime FIFO、flush 锁外 callback、slot reuse、reserved
  task exit，以及 stale handoff 在途时新 generation 重新排队。
- RV64 release SMP=2 最终源码通过 435/435 KUnit、`All tests passed!`、
  `timer-signal: SI_TIMER frame and ordinary standard merge checks passed`；LA64 release SMP=2 通过 436/436 和同一
  marker。两架构 frame 均检查 `SI_TIMER`、非零 timer ID/overrun/`sigval` 和清零的 kernel-private 字段。
- 双架构 rootfs 由 Docker `gallant_lamarr` 从 tracked manifest重建，kernel 与 QEMU 由宿主 repository入口执行；
  一次把完整 wrapper 放进容器的尝试因 `lwext4` build script 无法访问宿主 Docker control plane而在 QEMU 前停止，
  不作为验证证据。最终两份 guest 均通过既有 clock、soft-timer 和 socket regression，随后只因测试盘缺 static
  BusyBox 停在 competition 初始化，该环境缺口不归因于 Gate 4。
- source audit确认 callback 只在 pending/ThreadGroup guard 外执行，slot reuse_generation与 in-flight count共同
  阻止 stale registration 访问新资源，普通 standard signal storage未改成按来源排队。`just fmt kernel --check`、
  `just fmt user-test --check`、双架构 exact release build与`git diff --check`通过；按开发者要求所有 mdBook检查
  跳过，记为 Not Run。LTP不是本 gate的语义 oracle，本次 Not Run。

## Gate 4 closure — 2026-08-04

Gate 4 只关闭内部 `SI_TIMER` signal protocol，没有创建 POSIX timer 对象、注册 107--111 或修改 current contract。
`SIGNAL-PENDING-001` 的 `SI_TIMER` refine 与 `POSIX-TIMER-001` 继续等待 Gate 5 真实 consumer、完整五 syscall 和
双架构生命周期/ABI matrix后原子 cut over。开发者授权 Gate 4 验收后继续 Gate 5，并要求开始前先同步合并
`origin/main`。
