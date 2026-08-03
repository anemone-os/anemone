# Clock Timekeeping 与 POSIX Timers 事务日志

**状态：** Active / R0 / Gate 0--1 Closed / Gate 2 Authorized
**日期：** 2026-08-04
**负责人：** doruche, Codex
**RFC：** [RFC-20260803-clock-timekeeping-posix-timers R0](../../rfcs/clock-timekeeping-posix-timers/index.md)
**实施计划：** [Gate 0--6](../../rfcs/clock-timekeeping-posix-timers/implementation.md)
**适用修订：** R0
**Contract Cutover：** `TC-CLOCK-CUTOVER` Completed；`TIMEKEEPER-CLOCK-001` Active；其它 R0 contract delta pending

## 边界

本 checkpoint 关闭 Gate 0 与 Gate 1：冻结 native ABI、clock operation、architecture source 和 caller domain
baseline，并切换统一 architecture counter、integer Hertz conversion、timekeeper clock read truth、八个 get/res
route 和 calendar consumer。它不实现或声明 realtime mutation、完整 clock sleep、可删除 soft-timer request、
timerfd cancel-on-set、POSIX timer 或 RTC seed。

开发者已验收 Gate 1 实现，并授权其关闭后直接进入 Gate 2。Gate 2 仍使用独立 review、validation、contract
write-back 和 `clock:` commit；该授权不把 Gate 2 代码或 `SOFT-TIMER-REQUEST-001` 混入本 checkpoint。

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
