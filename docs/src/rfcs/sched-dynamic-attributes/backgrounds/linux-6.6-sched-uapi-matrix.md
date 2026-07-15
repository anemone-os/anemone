# Linux 6.6 Scheduler UAPI Matrix

**状态：** Reviewed evidence for accepted R0
**最后更新：** 2026-07-15
**父 RFC：** [RFC-20260714-sched-dynamic-attributes](../index.md)
**对应问题：** [KETER-DYNATTR-002](../tracking-issues.md#keter-dynattr-002调度-uapi-的精确-abi-matrix-尚未闭合)

本文记录 R0 scheduler UAPI contract 的 Linux 6.6 与 LTP 依据、逐 syscall 行为和验证映射。binding boundary 仍由 [RFC 入口](../index.md) 与 [不变量需求](../invariants.md) 拥有；本文不定义 scheduler core state、implementation stage 或 write set。

## 权威与取舍

主要依据：

- Linux 6.6.32 `include/uapi/linux/sched.h`
- Linux 6.6.32 `include/uapi/linux/sched/types.h`
- Linux 6.6.32 `kernel/sched/core.c`
- Linux 6.6.32 `kernel/sys.c`
- LTP 20240524 `testcases/kernel/syscalls/`
- LTP 20240524 `testcases/open_posix_testsuite/`

R0 使用 Linux 6.6 的 ABI layout、size negotiation、pointer ordering、legacy reset encoding、raw return encoding 和 errno 分类，但只接受 RFC 已声明的功能子集：

- setter policy：`SCHED_OTHER`、`SCHED_FIFO`、`SCHED_RR`；
- `sched_attr.sched_flags`：仅 `SCHED_FLAG_RESET_ON_FORK`；
- syscall 自身的 `flags`：仅 0；
- `SCHED_BATCH`、`SCHED_IDLE`、`SCHED_DEADLINE` 和其它 policy：setter 返回 `EINVAL`；
- reclaim、deadline overrun、keep-policy、keep-params、util-clamp 和未知 attr flag：返回 `EINVAL`。

拒绝 `SCHED_FLAG_KEEP_PARAMS` 是有意的 R0 边界。完整支持它需要 owner CPU 基于 latest config 执行“换 policy 但保留当前参数”的新 semantic patch；syscall adapter 不得先读 snapshot 再提交完整 config。R0 不为未要求的 flag 扩张 `SchedConfigPatch`。

## Raw layout 与 size negotiation

### `sched_param`

Native 64-bit rv64/la64 使用 Linux layout：

| Field | Type | Size |
|---|---|---:|
| `sched_priority` | signed 32-bit integer | 4 |

负数经过 typed validation 后为 `EINVAL`。

### `sched_attr`

Linux 6.6 layout：

| Offset | Field | Type |
|---:|---|---|
| 0 | `size` | `u32` |
| 4 | `sched_policy` | `u32` |
| 8 | `sched_flags` | `u64` |
| 16 | `sched_nice` | `i32` |
| 20 | `sched_priority` | `u32` |
| 24 | `sched_runtime` | `u64` |
| 32 | `sched_deadline` | `u64` |
| 40 | `sched_period` | `u64` |
| 48 | `sched_util_min` | `u32` |
| 52 | `sched_util_max` | `u32` |

已知 version：

| Version | Size | R0 behavior |
|---|---:|---|
| VER0 | 48 | 接受；缺失的 util fields 补零 |
| VER1 | 56 | 接受；util fields 只作为 ABI tail，R0 不支持对应 flags |

Anemone 的 advertised kernel size 固定为 56。即使 R0 不支持 util clamp，也保留 Linux 6.6 struct version；feature support 由 flags 决定，不由 advertised size 暗示。

### `sched_setattr()` size

按以下顺序处理：

1. 先读取用户 `size`；读取失败为 `EFAULT`。
2. `size == 0` 兼容为 48。
3. `size < 48` 或 `size > PAGE_SIZE`：尝试把 56 写回用户 `size`，最终返回 `E2BIG`；写回失败不把结果改成 `EFAULT`。
4. `48 <= size < 56`：复制 `size` bytes，kernel tail 补零。
5. `size == 56`：复制完整 known struct。
6. `56 < size <= PAGE_SIZE`：先检查 unknown tail；全零则只复制 known 56 bytes，非零为 `E2BIG` 并尝试写回 56，tail 访问失败为 `EFAULT`。
7. effective size 小于 56 且设置任一 util-clamp flag 时，在 target lookup 前返回 `EINVAL`；这是 version/field presence check，不表示 R0 支持 util clamp。
8. `sched_nice` 在 ABI boundary clamp 到 `[-20, 19]`。

### `sched_getattr()` usize

- null output、negative pid、nonzero syscall flags、`usize < 48` 或 `usize > PAGE_SIZE` 在 lookup 前返回 `EINVAL`。
- `48 <= usize <= PAGE_SIZE` 合法，不要求 8-byte 对齐。
- target lookup 与 snapshot 成功后，先验证完整 `usize` user range 可写，再复制 `min(usize, 56)` bytes。
- 输出 `attr.size = min(usize, 56)`。
- `usize > 56` 时用户 tail 保持不变。

## Policy、field 与 flag 投影

| Requested policy | Parameter rule | Nice rule | Result |
|---|---|---|---|
| `SCHED_OTHER` | priority 必须为 0 | `sched_setattr` clamp 后写 configured nice | Fair |
| `SCHED_FIFO` | priority `1..=99` | attr nice inactive，不修改 dormant nice | Realtime/FIFO |
| `SCHED_RR` | priority `1..=99` | attr nice inactive，不修改 dormant nice | Realtime/RR |
| BATCH/IDLE/DEADLINE/unknown | 不消费 | 不消费 | `EINVAL` |

在 supported policy 下，deadline tuple 没有激活 feature，按 Linux inactive-field 行为忽略；它们不会进入 core，也不会从 getter 回显。选择 `SCHED_DEADLINE` 或使用 deadline/reclaim flags 才构成 unsupported feature，并返回 `EINVAL`。

`sched_attr.sched_flags`：

| Flag | R0 |
|---|---|
| `SCHED_FLAG_RESET_ON_FORK` | 支持 |
| RECLAIM / DL_OVERRUN | `EINVAL` |
| KEEP_POLICY / KEEP_PARAMS | `EINVAL` |
| UTIL_CLAMP_MIN / UTIL_CLAMP_MAX | `EINVAL` |
| unknown bits | `EINVAL` |

除“short struct携带util-clamp flag”这个version check外，unsupported attr flags 与 positive policy/range validation 在 target lookup 后执行，保留 Linux 的 `ESRCH` precedence；struct size、copy、util field-presence和signed-policy sanity仍在lookup前完成。

## 逐 syscall matrix

### Legacy policy/parameter calls

| Syscall | Input/ordering | Success projection | Main errors |
|---|---|---|---|
| `sched_setscheduler(pid, policy, param)` | negative policy -> null/negative pid -> copy-in -> lookup -> policy/priority -> permission -> patch | 返回 0；replace discipline，preserve dormant nice；reset 取 legacy policy bit | `EINVAL`, `EFAULT`, `ESRCH`, `EPERM` |
| `sched_setparam(pid, param)` | null/negative pid -> copy-in -> lookup -> latest discipline parameter validation -> permission -> patch | 返回 0；preserve discipline、nice、reset | 同上 |
| `sched_getscheduler(pid)` | negative pid -> lookup -> snapshot | 返回 policy OR legacy reset bit | `EINVAL`, `ESRCH` |
| `sched_getparam(pid, param)` | null/negative pid -> lookup -> snapshot -> copy-out | Fair priority 0；RT configured priority | `EINVAL`, `ESRCH`, `EFAULT` |

`sched_setscheduler()` 只在该入口把 `SCHED_RESET_ON_FORK=0x40000000` 从 policy 中剥离。没有该 bit 表示请求清除 reset；无 privilege caller 不能清除已经置位的 reset。错误或 nonzero priority 不能夹带 reset side effect。

`sched_setparam()` 不接受 legacy reset encoding，也不清除 reset。Fair 只接受 priority 0；RT 只接受 `1..=99`。

### Attribute calls

| Syscall | Input/ordering | Success projection | Main errors |
|---|---|---|---|
| `sched_setattr(pid, attr, flags)` | top-level scalar/null -> size/copy/tail -> util version check -> signed policy -> lookup -> policy/attr-flag/range -> permission -> patch | 返回 0；reset 取 attr flag | `EINVAL`, `E2BIG`, `EFAULT`, `ESRCH`, `EPERM` |
| `sched_getattr(pid, attr, usize, flags)` | top-level scalar/null/usize -> lookup -> one config snapshot -> full-range access check/copy-out | 返回 0；按下表投影 | `EINVAL`, `ESRCH`, `EFAULT` |

`sched_getattr()` projection：

| Config | policy | flags | nice | priority | deadline tuple | util tuple |
|---|---:|---:|---:|---:|---|---|
| Fair | OTHER | reset only | configured nice | 0 | 0 | 0 |
| RT/FIFO | FIFO | reset only | 0 | configured priority | 0 | 0 |
| RT/RR | RR | reset only | 0 | configured priority | 0 | 0 |

RT dormant nice 只能通过 `getpriority()` 观察；`sched_getattr()` 不把它投影为 RT active parameter。`sched_setattr()` 进入 RT 时也不得用 inactive `sched_nice` 覆盖 dormant nice。

### Static policy queries 与 yield

| Syscall | Input | Result |
|---|---|---|
| `sched_get_priority_min(policy)` | FIFO/RR | 1 |
| `sched_get_priority_max(policy)` | FIFO/RR | 99 |
| min/max | OTHER/BATCH/IDLE/DEADLINE | 0 |
| min/max | unknown 或附带 reset bit | `EINVAL` |
| `sched_yield()` | none | class transaction 后返回 0 |

min/max 是 policy-domain query；对 unsupported setter policy 返回 0 不表示该 policy 可安装。

### `sched_rr_get_interval()`

调用顺序为 negative pid -> lookup/snapshot -> interval derivation -> copy-out。不存在 target 优先 `ESRCH`；只有 target 存在且 output fault 才返回 `EFAULT`。

| Configured discipline | Observable interval |
|---|---|
| Fair/Stride | 一个 effective scheduler tick |
| RT/FIFO | zero timespec，表示无自动 timeslice |
| RT/RR | `RT_RR_FULL_QUANTUM_TICKS` 对应的 effective duration |

Fair 不读取 ready queue，也不复制 Linux CFS 的 load-dependent slice。当前 Fair backend 以一个 timer tick 为完整 service unit，因此一个 effective tick 是稳定且可由 config/scheduler constants 投影的 interval。

RR 返回 full configured quantum，不返回 `remaining_ticks`，也不根据 peer、`rotation_due` 或当前 segment 改变。duration conversion 由 class/config owner 提供窄值，UAPI adapter 只转换为 native `__kernel_timespec`。

### Priority calls

| Syscall | Selection/normalization | Success | Errors |
|---|---|---|---|
| `setpriority(which, who, nice)` | validate which；nice clamp `[-20,19]`；snapshot target set；逐 target commit | 返回 0 | `EINVAL`, `ESRCH`, `EACCES`, `EPERM` |
| `getpriority(which, who)` | validate which；snapshot target set；逐 target nice snapshot | raw syscall 返回 selected maximum `20 - nice` | `EINVAL`, `ESRCH` |

Selectors：

- `PRIO_PROCESS`：`who == 0` 为 current，否则按 TID；negative/missing 为 `ESRCH`。
- `PRIO_PGRP`：`who == 0` 为 current process group；negative/missing/empty 为 `ESRCH`。
- `PRIO_USER`：`who == 0` 为 caller real UID；negative/missing/empty 为 `ESRCH`。
- kthread 不进入 user-facing target set。

Multi-target setter accumulator 与现有 Linux/Anemone 行为一致：

1. 初值为 `ESRCH`。
2. 第一个成功把 `ESRCH` 变为 success。
3. target failure 把 accumulator 设为该 errno。
4. 后续成功不能清除已经记录的非-`ESRCH` failure。
5. 已完成的 target mutation 不回滚。

因此它不是 topology-wide atomic transaction。

### Affinity calls

令 `KERNEL_CPU_MASK_BYTES` 为按 native `unsigned long` 对齐、足以表示 compile-time CPU domain 的 mask size。

`sched_setaffinity(pid, len, mask)`：

1. 先复制 `min(len, KERNEL_CPU_MASK_BYTES)` bytes；短输入其余位补零，长输入的高 tail 不检查。
2. copy fault 返回 `EFAULT`，先于 target lookup；`len == 0` 不触碰 pointer。
3. `pid == 0` 为 current；negative/missing pid 返回 `ESRCH`。
4. lookup 后检查 ownership/CAP，再与 online CPU domain 取交集。
5. 空交集返回 `EINVAL`。
6. 交集不包含 immutable owner `cpuid` 时需要 migration；R0 返回 `EINVAL` 并记录诊断。
7. 否则提交完整 effective mask，返回 0。

`sched_getaffinity(pid, len, mask)`：

1. lookup 前要求 `len * 8` 足以覆盖 kernel CPU domain，且 `len` 是 native `unsigned long` size 的整数倍；否则 `EINVAL`。
2. negative/missing pid 返回 `ESRCH`。
3. snapshot 只保留 online bits。
4. copy-out fault 返回 `EFAULT`。
5. raw syscall success 返回 `min(len, KERNEL_CPU_MASK_BYTES)`，不是 0；libc wrapper 可以把它转换为 0。
6. 返回长度之外的用户 buffer 不修改。

## Permission 与 errno 映射

Submit-side owner check使用 caller effective UID 对 target real/effective UID；不匹配且没有 effective `CAP_SYS_NICE` 为 `EPERM`。Getter 不要求相同 UID。

没有 `RLIMIT_NICE` / `RLIMIT_RTPRIO` 时，same-owner non-escalating permit 允许：

- numeric nice 增大或不变；
- 同一 RT mode 下 priority 降低或不变；
- RT 退出到 Fair；
- 设置 reset-on-fork；
- exact no-op。

它不允许：

- numeric nice 降低；
- Fair 进入 RT；
- FIFO/RR 互换；
- RT priority 提高；
- 清除已经置位的 reset-on-fork。

`CAP_SYS_NICE` permit 可以执行所有 supported transition。Owner CPU 对 latest config 应用 permit；并发更新导致 transition 变成 escalation 时必须在 detach 前拒绝。

内部 transaction 只返回 typed permission denial，ABI adapter按入口映射：

- `setpriority()` 的 nice escalation denial -> `EACCES`；
- scheduler policy/parameter/attr escalation denial -> `EPERM`；
- wrong-owner 或 affinity permission denial -> `EPERM`。

## Error precedence probes

Focused syscall tests至少固定以下组合，防止实现把 validation 都堆在 handler 开头而改变 Linux ordering：

| Probe | Expected |
|---|---|
| legacy setter：bad non-null param + missing pid | `EFAULT` |
| legacy setter：valid param + missing pid + invalid positive policy | `ESRCH` |
| `sched_setattr`：invalid size + missing pid | `E2BIG` |
| `sched_setattr`：nonzero unknown tail | `E2BIG` and size 56 write-back attempt |
| `sched_setattr`：size 48 + util-clamp flag + missing pid | `EINVAL` |
| `sched_setattr`：valid struct + missing pid + unsupported attr flag/policy | `ESRCH` |
| `sched_getparam`：null output + missing pid | `EINVAL` |
| `sched_getparam`：bad non-null output + missing pid | `ESRCH` |
| `sched_getattr`：bad output + missing pid | `ESRCH` |
| `sched_rr_get_interval`：bad output + missing pid | `ESRCH` |
| affinity set：bad input + missing pid | `EFAULT` |
| affinity get：invalid len + missing pid | `EINVAL` |
| affinity get：valid len + bad output + missing pid | `ESRCH` |
| scheduler policy/parameter invalid against unauthorized existing target | `EINVAL` before `EPERM` |
| affinity empty/migration-required mask against unauthorized existing target | `EPERM` before mask-semantic `EINVAL` |

所有 failure 都必须发生在 config publication 前；copy-out failure不回滚 getter前已经取得的 snapshot，也不修改 scheduler state。

## LTP/POSIX 验证映射

Supported-subset gate：

- `sched_setscheduler01/02/04`：pointer/range、permission、legacy reset；
- `sched_setparam01..05`：Fair/RT parameter、pid、permission；
- `sched_getscheduler01/02`、`sched_getparam01/03`；
- `sched_getattr02`：pid、null、minimum size、top-level flags；
- `sched_get_priority_{min,max}01/02`：包含 unsupported policy 的 static-query result；
- `sched_rr_get_interval01/02/03`：RR full quantum、FIFO zero、pid/copy errors；
- `setpriority01/02`、`getpriority01/02`：selectors、clamp、raw result、`EACCES` / `EPERM`；
- `sched_setaffinity01`、`sched_getaffinity01`：mask copy、empty mask、len、pid、permission。

Stock LTP 中以下 success expectation 与 R0 非目标冲突，必须记录为 expected unsupported coverage，不能把整 case 写成 completion gate：

- `sched_setattr01` 的 success branch 安装 `SCHED_DEADLINE`；
- `sched_getattr01` 先安装并读取 `SCHED_DEADLINE`；
- `sched_setscheduler03` 的 BATCH / IDLE branches 要求成功。

这些 case 的其它 errno branch仍可作为局部证据。R0 需要额外 focused tests覆盖 stock LTP 缺少的 size 0、48/56、zero/nonzero future tail、reset read-back、unsupported attr flags、cross-error precedence、Fair interval和 affinity raw return length。

## 结论

该 matrix 不要求新增 configured field、permission state或 transaction owner：

- userspace structs、layout与共享常量由`anemone-abi::process::linux::sched`统一拥有；kernel raw copy、flags解释、size negotiation、errno mapping与semantic conversion都留在`sched/api`；
- `sched_setscheduler`、`sched_setparam`、`sched_setattr`、`setpriority` 与 affinity只生成既有 semantic patch维度；
- permission denial保持typed internal result，由每个ABI入口映射；
- getter从一个 `SchedConfig` snapshot投影，不读取RunQueue或class-private runtime；
- 若未来要支持KEEP_PARAMS、util clamp、migration或Deadline，必须作为后续contract扩张重新review。
