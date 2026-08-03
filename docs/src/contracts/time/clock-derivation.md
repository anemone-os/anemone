# Clock Derivation 当前契约

**Contract ID：** `TIMEKEEPER-CLOCK-001`
**状态：** Active
**Owner：** architecture clock source 与 common timekeeper
**参与领域：** RV64 / LA64 architecture、timekeeper、clock route、task CPU usage、filesystem / procfs / SysV IPC calendar consumer
**覆盖范围：** clock ID 0--7 的 get/res、统一 counter-to-nanoseconds 推导、realtime read projection 与 coarse snapshot
**不覆盖：** `clock_settime()`、`clock_adjtime()`、完整 `clock_nanosleep()`、realtime step notification、soft timer、timerfd cancellation、POSIX timer、RTC seed/writeback 和 suspend accounting
**实现位置：** `anemone-kernel/src/{arch/riscv64/time.rs,arch/loongarch64/time.rs,time/timekeeper.rs,time/clock}`
**依赖：** None
**Pending Successor：** RFC-20260803 Gate 2/3/5 分别引入独立 contract，不替换本规则
**最后核验：** 2026-08-04

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| raw/stable counter、跨 CPU correction、Hertz | architecture clock source | timekeeper 只读 counter/frequency | 提供一个不随 task migration 后退的计数域 |
| `boot_counter`、frequency、realtime read snapshot、coarse monotonic snapshot | timekeeper | clock/calendar consumer 读取值 | 推导 wall/elapsed clock |
| process/thread CPU counter accumulation | task / thread group | CPU clock 读取 snapshot | 保持 CPU time 与 wall time 分离 |
| clock ID 到 read/resolution 的投影 | clock route | syscall adapter 取得 narrow `Clock` capability | 拒绝未知 ID，避免不同 clock 偶然 alias |

`realtime_change_seq` 在本 contract 下是 Gate 3 预留且 dormant 的协议字段；它不参与 Gate 1 读值，也不能反向
决定 offset。`coarse_mono_ns` 是明确允许陈旧的性能 snapshot，不是第二份 monotonic truth。

## TIMEKEEPER-CLOCK-001 — 所有 clock 读取来自一条整数推导链

**规则：** 每个 online CPU 向 common timekeeper 暴露同一个不后退的 architecture counter domain 和一个
nonzero、boot-stable Hertz `F`。平台 raw counter 需要 correction 时，correction 只属于 architecture
source；timekeeper 不保存 per-CPU baseline 或 offset。

BSP 在 common timekeeper 初始化时采样唯一 `boot_counter`。普通 wall clock 按以下公式现场推导，乘法先提升
到 `u128`，不使用浮点：

```text
counts = counter_now - boot_counter
monotonic_ns = counts * 1_000_000_000 / F
raw_ns = monotonic_ns
realtime_ns = monotonic_ns + realtime_offset_ns
boottime_ns = monotonic_ns
```

当前没有 RTC seed 或 mutation，`realtime_offset_ns == 0`。offset 必须非负；以后 Gate 3 改变 offset 时必须
通过独立 `TIMEKEEPER-STEP-001` cutover，不能只修改本读值 contract。`raw` 与 `monotonic`、`boottime` 当前
同值但保留独立 clock route，不通过对象 alias 表达 ABI identity。

BSP 的每个 system tick 更新一次 `coarse_mono_ns`；其它 CPU 不发布 coarse snapshot。`MONOTONIC_COARSE`
直接读取该 snapshot，`REALTIME_COARSE` 在读取时加同一个 realtime offset；不得保存 `coarse_real_ns`。

process/thread CPU clock 读取 task/thread-group owner 的累计 counter，并使用同一个 Hertz 换算；timekeeper 不
取得 CPU usage 状态。

普通 clock 与 CPU clock 的 resolution 是：

```text
source_resolution_ns = max(1, ceil(1_000_000_000 / F))
coarse_counts = F / SYSTEM_HZ
coarse_resolution_ns = max(1, ceil(coarse_counts * 1_000_000_000 / F))
```

启动必须保证 `F >= SYSTEM_HZ`。加法、乘法和 ceil division 在 `u128` 中完成并检查回到 internal/user range；
production route 不固定返回 1ns 或 10ms，也不把 100 Hz soft-timer delivery 周期当作普通 clock resolution。

| Clock ID | 当前读值 | Resolution owner |
| --- | --- | --- |
| `CLOCK_REALTIME` | monotonic + realtime offset | source |
| `CLOCK_MONOTONIC` | counter - boot counter | source |
| `CLOCK_PROCESS_CPUTIME_ID` | thread-group CPU usage | source |
| `CLOCK_THREAD_CPUTIME_ID` | current-task CPU usage | source |
| `CLOCK_MONOTONIC_RAW` | monotonic，独立 route | source |
| `CLOCK_REALTIME_COARSE` | coarse monotonic + realtime offset | coarse update interval |
| `CLOCK_MONOTONIC_COARSE` | BSP coarse snapshot | coarse update interval |
| `CLOCK_BOOTTIME` | monotonic，独立 route；当前无 suspend | source |

calendar-valued consumers（`gettimeofday`、filesystem/procfs inode timestamp、SysV IPC timestamp）必须读取同一
realtime projection；uptime、scheduler/device timeout 和 CPU accounting 必须继续使用 monotonic 或 CPU owner。

**违反表现：** common timekeeper 保存 per-CPU boot baseline；raw/realtime/boottime 保存可独立推进的 current
time；calendar consumer 继续使用 boot-relative `Instant`；coarse realtime 被缓存成第二份值；读值使用浮点；
`clock_getres()` 固定返回 1ns/10ms；process/thread CPU time 在 blocked sleep 期间按 wall time推进。

**验证 / Enforcement：** owner-local KUnit 覆盖多个 Hertz、不可整除 ceil、`F == SYSTEM_HZ`、`F > 1GHz`、
overflow、negative-offset 拒绝、BSP coarse publication、八个独立 route 和双 CPU ordered reads。2026-08-04
RV64/LA64 SMP QEMU 分别通过 395/395 与 396/396 KUnit；同源 user oracle 覆盖八个 ID、resolution class、
non-regression、derivation、blocked CPU time、invalid ID 与 `clock_getres(NULL)`。LA64 QEMU 的 stable counter 本身
跨 CPU shared 且不应用 CNTC，因此 CNTC correction 的符号与 AP write ordering由 RFC-pinned Linux 6.6.32
source audit证明，不把 QEMU结果扩大为物理 CNTC runtime evidence。

**最初来源：** [Clock Timekeeping 与 POSIX Timers RFC R0](../../rfcs/clock-timekeeping-posix-timers/index.md) 的
`TC-CLOCK-CUTOVER`。

**当前来源：** [2026-08-04 Gate 0/1 transaction](../../devlog/transactions/2026-08-04-clock-timekeeping-posix-timers.md#gate-1-closure-与-tc-clock-cutover---2026-08-04)。
