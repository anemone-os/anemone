# ANE-CHG-20260805-oom-periodic-sampling

**Type:** Small Feature / contract-bearing local cutover
**Date:** 2026-08-05
**Authors:** doruche, Codex
**Area:** mm / frame allocator / task kthread / scheduler wait / Kconfig

## Problem / Context

原OOM实现把物理页分配成功事件反向接到OOM worker：`alloc_frame()`与`alloc_frames()`读取水位并唤醒
global kthread handle，worker随后再次读取同一`FrameAllocatorStats`。这条edge不是pressure truth，却让任意
allocation caller继承scheduler/Event side effect；kmalloc OOM或IRQ-off tail中的frame allocation还可能递归进入
OOM wake路径。

frame allocator已经唯一拥有total/free page truth，OOM worker执行policy前本来就必须读取live stats。因此本轮把
采样时机交给OOM owner，以固定delay取代allocation edge，并在kthread owner内补充一个复用现有Event/wait-core
timeout identity的窄timed-wait能力。2026-06-15 OOM RFC及transaction从未闭合其runtime acceptance；它们作为
未完成历史终止，而不被本轮追认为Closed或Completed。

## Decision

- `oom-killer-0`每次从上一轮sample/victim工作完成后等待一个完整
  `oom_kill_sample_interval_ms`，默认50 ms；不追赶wall-clock deadline，也不在压力期间busy yield。
- timeout只决定何时读取一次live `FrameAllocatorStats`。该snapshot同时服务threshold判断和本轮日志；系统不保存
  `pressure_pending`、allocation hint、task-exit hint或其它第二份pressure truth。
- 使用率必须严格大于`oom_kill_threshold`才进入victim policy；一次sample至多执行一次victim round，active victim
  尚未退出或没有eligible victim时也必须重新等待完整interval。
- `KThreadCtx::wait_for(Duration)`只以cooperative stop truth或nonzero deadline完成。`request_stop()`先发布stop再
  wake；ordinary wake或forced wake只触发stop/deadline重查。每个内部recheck使用新的wait identity，晚到timeout
  不能完成后续round。
- victim eligibility、独占物理页score、kernel-origin `SIGKILL`与既有signal/exit cleanup保持不变；不新增用户ABI、
  generic periodic-worker框架、reclaim或allocation-failure恢复承诺。

## Implementation Boundary

frame allocator唯一拥有allocation与`FrameAllocatorStats`；OOM owner唯一拥有sample loop、threshold policy、active
victim与victim round；task/kthread owner唯一拥有stop/wake/timed-wait adapter；scheduler wait core唯一拥有wait
identity、completion competition和timer-token stale判断；Kconfig/build owner只负责物化和传输threshold/interval。

OOM向`KThreadCtx`提交duration，wait返回后先检查stop，再取得一个live stats snapshot；只有该snapshot超过阈值时
才执行一次victim round。timeout、Event和日志都不是pressure state。每轮listener/token必须在进入下一轮前完成
cleanup/retire；victim signal与资源释放仍由既有Signal/ThreadGroup exit owner终结。

本轮保护现有用户ABI、strict-greater threshold、victim范围/score、`SIGKILL` origin和ordinary exit path。若需要
allocator/task-exit edge、第二份pending truth、OOM-local timer协议、generic worker abstraction、victim/exit policy
变化或更弱runtime oracle，则必须停止并重新分类；本轮没有触发这些条件。

## Change

- 在KernelConfig schema、default materialization与generator中增加
  `oom_kill_sample_interval_ms: u64`，tracked默认值为50；OOM consumer以`static_assert!`拒绝零值。
- Event增加crate-local uninterruptible timed predicate wait；kthread control只以stop predicate使用它，
  `KThreadCtx`不暴露Event、Task、wait token或scheduler placement result。
- 增加owner-local KUnit，覆盖timeout完成、stop打断、ordinary wake不提前返回及同一调用内的late/stale timeout
  isolation；frame stats KUnit覆盖total为0、等于90%不触发及严格超过才触发。
- 删除allocation-time threshold/wake helper、OOM global handle和公开wake入口；`alloc_frame()` / `alloc_frames()`
  恢复为只返回allocator结果。
- OOM worker改为fixed-delay loop，每轮只读一次stats并至多执行一次victim round。连续no-eligible-victim日志由一个
  明确标注为diagnostic-only的flag抑制；该flag不参与threshold、sampling或victim选择。
- 历史OOM RFC/transaction标记为`Terminated`并保留原有Not Run；register只把递归OOM wake分支标为已消除，
  IRQ/off-tail allocator side-effect问题整体仍保持Open。

## Contract Impact / Cutover

| Contract ID | 变化 | Cutover前baseline | 新规则 | 生效证据 |
| --- | --- | --- | --- | --- |
| [`KTHREAD-WAIT-001`](../../contracts/task/kthread-wait.md#kthread-wait-001--timed-wait只由stop-truth或deadline完成) | Introduce | None | kthread-owned stop/deadline wait、ordinary-wake recheck与stale-round isolation | wait lifecycle source audit、三项owner KUnit、RV64 KUnit boot |
| [`MM-OOM-001`](../../contracts/mm/oom-policy.md#mm-oom-001--oom-worker自有fixed-delay采样与victim-round) | Replace | live `mm::oom`与历史RFC中未提取的allocation-success wake baseline | worker-owned periodic stats sampling、单snapshot与单victim round | hook residual audit、threshold KUnit、focused RV64 OOM runtime |

[`KCONFIG-VALIDATION-001`](../../contracts/configuration/kernel-parameter-validation.md#kconfig-validation-001--kernel-consumer-唯一定义参数语义合法性)、
[`SCHED-WAKE-001..004`](../../contracts/scheduler/wake-delivery.md)、
[`TASK-LIFE-001..003`](../../contracts/task/thread-group-lifecycle.md)与
[`SIGNAL-PENDING` / `SIGNAL-ACTION`](../../contracts/signal/pending-routing.md)均为未变化Dependencies。
代码、两项current contract、本记录和历史状态在同一个closure提交中生效；提交未完成时不单独发布contract文本。

## Validation

- `just fmt kernel --check`通过；`just test xtask`为75/75；`git diff --check`与allocation-hook/global-handle/
  victim-round call-site residual search通过。
- 临时把ignored本地KernelConfig设为`oom_kill_sample_interval_ms = 0`后，RV64 release build在
  `mm::oom`的`static_assert!`以`E0080`拒绝；删除临时值后，同一完整low-level tuple重新生成值50并完成release
  build。沙箱内lwext4 C编译因`Bad system call`失败，宿主环境的同一repository command通过；这不是内核失败。
- RV64单HART QEMU boot通过431/431 KUnit；新增三项kthread wait测试和frame threshold测试均通过，guest随后正常
  PowerOff。
- focused RV64单HART guest中，OOM测试child逐页触碰到850 MiB后被`SIGKILL`，parent报告PASS，kernel完成orderly
  shutdown且无panic。临时focused user-test/rootfs接线随后删除；最终diff不改变测试选择。
- 独立change review确认无Apollyon、Keter、Euclid或blocking finding；stop publication、wait identity、timer weak
  lifetime、stats单一真相、hook removal和diagnostic-only flag边界均通过源码审查。
- **Not Run:** LA64 build/runtime、`smp>1`、full LTP、final harness、physical hardware、no-eligible-victim日志的定向
  runtime，以及跨两个独立`wait_for`调用的late-timer强化测试。本轮不测试或承诺exact 50 ms cadence。

## Remaining Risk / Links

- [Kthread timed wait当前契约](../../contracts/task/kthread-wait.md)与
  [OOM policy当前契约](../../contracts/mm/oom-policy.md)是effective语义的唯一正文。
- 实际OOM响应还包含timer granularity、worker调度、victim signal和exit释放延迟；两个sample之间仍可继续分配，
  本轮只承诺proactive best-effort sampling，不承诺allocation failure恢复或hard realtime bound。
- 持续高于阈值且没有eligible victim时，系统可能长期保持压力；日志保持有界，但reclaim/panic/backoff不在本轮。
- 若后续workload证明50 ms响应不足，allocation/task-exit edge hint必须作为独立工作重新分类，不能把hint预埋为
  parallel pressure truth。
- [`ANE-20260622-IRQ-OFF-HEAP-ALLOCATION`](../../register/open-issues.md#ane-20260622-irq-off-heap-allocation)
  的递归OOM wake分支已消除，hard IRQ/IRQ-off tail的其它复杂allocator side effect仍保持Open。
- [Terminated OOM RFC](../../rfcs/oom-killer/index.md)与
  [Terminated transaction](../transactions/2026-06-15-oom-killer.md)只保留历史target和未闭合证据，不定义current
  behavior。
