# OOM Policy 当前契约

**Contract ID：** `MM-OOM`
**状态：** Active
**Owner：** `mm::oom` sample / threshold / victim-round policy
**参与领域：** frame allocator / Kconfig / task kthread / UserSpace/VMO snapshot / Signal / ThreadGroup lifecycle
**覆盖范围：** global frame-usage sampling、strict threshold、active victim、eligible victim selection与kernel-origin `SIGKILL` handoff
**不覆盖：** synchronous allocation-failure recovery、reclaim、swap、OOM reaper、memcg、badness score、panic policy、用户RSS ABI或hard realtime response
**实现位置：** `anemone-kernel/src/mm/{oom.rs,frame/}`、`anemone-kernel/src/task/kthread/`、`conf/kconfs/default.toml`、`scripts/xtask/src/config/`
**依赖：** `KTHREAD-WAIT-001`、`KCONFIG-VALIDATION-001`、`TASK-LIFE-001..003`、`SIGNAL-PENDING-001/002`、`SIGNAL-ACTION-002`
**Pending Successor：** None
**最后核验：** 2026-08-05

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| total/free physical pages | frame allocator | OOM取得一次copy snapshot | 当前global frame usage truth |
| threshold与sample interval build policy | KernelConfig；kernel OOM consumer拥有semantic predicate | OOM读取generated constants | 决定何时进入victim round |
| fixed-delay loop、active victim与victim round | `mm::oom` | kthread只提供stop/deadline wait capability | 编排proactive best-effort policy |
| user address-space exclusive-page estimate | UserSpace/VMO owner | OOM持operation-local score snapshot | 提供可回收物理页估计，不成为RSS账本 |
| ThreadGroup type/lifecycle | task topology / ThreadGroup owner | OOM持短期`Arc` snapshot并读取live truth | 提供victim admission输入并观察active victim退出 |
| victim eligibility与排序policy | `mm::oom` | task/VMO只提供type、lifecycle与score snapshot | 排除kernel/init/idle/exiting targets并选择本轮victim |
| pending signal、terminal action与资源释放 | Signal与ThreadGroup exit owner | OOM只提交kernel-origin `SIGKILL` | 终止victim并走ordinary cleanup |
| no-eligible-victim suppression bit | OOM diagnostic only | 无 | 只限制重复notice，不参与任何policy decision |

timeout、allocation edge、wake notification、startup log与no-victim diagnostic都不是pressure truth。

## MM-OOM-001 — OOM worker自有fixed-delay采样与victim round

**规则：** OOM worker在每个sample或victim round完成后等待一个完整configured interval，再检查cooperative stop，
然后读取恰好一个live `FrameAllocatorStats` snapshot。tracked默认interval为50 ms且OOM consumer以compile-time
assertion拒绝零值；实际cadence可以因timer granularity、调度、victim scan与exit延迟而更晚，不追赶wall-clock
deadline。

该snapshot是本轮threshold与frame-usage日志的唯一输入。使用率必须严格大于`oom_kill_threshold`（tracked默认90%）
才进入victim policy；等于阈值或`total_pages == 0`不触发。allocator成功/失败、task exit或其它source都不提交
pressure hint、pending bit或wake edge，OOM也不缓存第二份pressure state。

一次sample至多执行一次victim round。active victim仍未到`Exited`时不选择新victim；否则只在alive user
ThreadGroup中选择exclusive physical-page snapshot为正且最大的eligible target，排除idle、init与kernel task。
victim kill只能提交kernel-origin `SIGKILL`，不得由OOM直接修改ThreadGroup terminal state或释放address space；
Signal与ordinary exit owner负责delivery和cleanup。没有eligible victim时只允许有界诊断并在下一完整interval后
重试，不能busy yield、panic或把diagnostic suppression反向变成policy state。

**违反表现：** allocation path读取threshold或唤醒OOM；timeout/wake/log变成pressure truth；同一sample为判断和日志
重复读取stats；等于threshold触发；一次sample连续杀多个victim；active victim未退出仍扩大杀伤面；OOM直接释放
victim资源；no-victim path tight loop/刷屏；或把50 ms写成exact cadence/hard response guarantee。

**验证 / Enforcement：** allocation-hook/global-handle与victim call-site residual audit；frame threshold KUnit；
interval零值的owner-local compile failure；kthread wait KUnit；RV64 431/431 KUnit boot；focused RV64 guest中pressure
child触达850 MiB后被`SIGKILL`、parent PASS并orderly shutdown。LA64、SMP、full LTP、hardware与exact cadence Not Run。

**最初来源：** 未提取的victim/Signal baseline来自[Terminated OOM RFC](../../rfcs/oom-killer/index.md)及live source；
periodic trigger与本ID的effective closure来自[OOM Periodic Sampling小迭代](../../devlog/changes/2026-08-05-oom-periodic-sampling.md)。

**当前来源：** [OOM Periodic Sampling小迭代](../../devlog/changes/2026-08-05-oom-periodic-sampling.md)；
2026-08-05 closure提交。
