# Kworker 当前契约

**Contract ID：** `KWORKER`
**状态：** Active
**Owner：** `task::kworker` boot topology、system queue ownership 与 worker drain protocol
**参与领域：** task::kworker / task::kthread / scheduler / timer core / hard IRQ timer delivery
**覆盖范围：** boot-fixed per-CPU system worker、anonymous `FnOnce` submission、queue ownership/FIFO、IRQ enqueue handoff与process-context execution
**不覆盖：** Linux CMWQ compatibility、dynamic worker management、backpressure、physical cancellation、completion/flush/join、CPU hotplug、strict latency/fairness或reclaim forward progress
**实现位置：** `anemone-kernel/src/task/kworker.rs`、`anemone-kernel/src/task/mod.rs`、`anemone-kernel/src/main.rs`、`anemone-kernel/src/time/timer/threaded.rs`
**依赖：** [`SCHED-WAKE-001..004`](../scheduler/wake-delivery.md)、kthread lifecycle/wait capabilities与timer core deadline/cancellation owner
**最后核验：** 2026-08-07

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| boot activation phase、per-CPU slot与sealed topology | `task::kworker` | BSP只有一次 activation capability；consumer只有已发布的 current/target-CPU `SystemWorker` submission capability | 固定 worker 拓扑与 queue target |
| queue entries与accepted closure ownership | `task::kworker` | producer在线性化前拥有closure，之后不再拥有 | FIFO handoff与at-most-once execution |
| kthread stop/exited phase | `task::kthread` | worker entry只有 `KThreadCtx`；submission capability只有pure wake | cooperative lifecycle |
| runnable/physical placement | scheduler | kworker只保留boot选定的 queue target并做 locality assertion | scheduler placement |
| deadline、timer event与cancellation | timer core / timer event owner | kworker只接收expiry后已经转交的callback | timer semantics |
| callback业务validity或logical cancellation | 具体consumer owner | callback捕获owner-local `Weak`、generation或validity capability | 不把queue presence或wake edge当作业务truth |

## KWORKER-001 — Boot-fixed topology

**规则：** BSP必须在所有CPU完成local init并online之后、unordered `Late` initcall window之前调用一次
system-worker activation。activation为每个boot-online CPU创建且发布一个绑定该CPU的system worker，随后seal
topology。重复activation、activation前submission或缺失slot都属于implementation invariant violation并以fatal
assert暴露；本阶段没有runtime create/destroy/resize/rebind/restart或CPU hotplug路径。

**违反表现：** `Late` consumer在worker发布前提交；一个CPU没有slot或出现重复slot；运行期改变worker数量、placement
或lifecycle；或者用`Late` initcall链接顺序伪造provider/consumer ordering。

**验证 / Enforcement：** `main.rs` source ordering audit；activation与slot publication assertions；owner-local
topology KUnit；RV64 QEMU SMP2显示HART count为2、KUnit完整运行并正常PowerOff。

**最初来源：** boot-fixed system-worker implementation target。

**当前来源：** [2026-08-05 kworker change record](../../devlog/changes/2026-08-05-kworker.md)及其Git cutover commit。

## KWORKER-002 — Submission ownership与FIFO

**规则：** `SystemWorker::submit(F)`是infallible、one-shot submission。queue `push_back`是ownership transfer与
成功提交的linearization point；同一queue按该点的顺序FIFO，worker `pop_front`后至多调用一次。只要worker最终获得
调度、前序callback返回且kernel未进入fatal failure，成功提交的work conditional eventual执行一次。不同worker之间不
提供ordering、barrier或completion guarantee；本阶段不提供work handle、physical cancellation、flush、drain、join
或admission result。callback对同一worker的自提交形成新的后继queue entry。

**违反表现：** closure在线性化前后出现双重owner；同一entry重复或静默丢弃；callback重用旧pending identity；或调用者
依赖不存在的同步完成/取消语义。

**验证 / Enforcement：** `push_back`/`pop_front` source audit；owner-local FIFO、at-most-once与callback self-submit
KUnit；RV64 QEMU SMP2中对应测例通过。

**最初来源：** boot-fixed system-worker implementation target。

**当前来源：** [2026-08-05 kworker change record](../../devlog/changes/2026-08-05-kworker.md)及其Git cutover commit。

## KWORKER-003 — IRQ handoff与callback discipline

**规则：** producer在`NoIrqSpinLock`内只把closure `push_back`到目标CPU queue；unlock之后才发布既有no-result wake。hard
IRQ路径不执行callback、不做普通日志、不取得普通锁、不等待worker或同步completion；允许既有`Box`、`VecDeque`扩容
及wake transport的allocation，分配失败沿kernel fatal allocator policy处理。worker先pop并释放queue lock，再在
process context调用callback；每个callback之后显式`yield_now()`。callback必须短小、有界，不得blocking I/O、长时间
reclaim、user wait或依赖同一worker后继entry；本契约不承诺allocation-free、bounded latency、strict fairness或
forward progress。

**违反表现：** queue lock持有期间wake/callback/drop；IRQ中执行复杂副作用；callback不返回而永久占用共享lane；或把
heap allocation的允许边界误写成实时保证。

**验证 / Enforcement：** source lock-scope audit；timer callback与burst KUnit断言interrupt enabled、not hwirq、
preemption allowed；RV64 QEMU SMP2完整KUnit通过。`ANE-20260622-IRQ-OFF-HEAP-ALLOCATION`仍是shared register问题。

**最初来源：** boot-fixed system-worker implementation target与既有scheduler wake contract。

**当前来源：** [2026-08-05 kworker change record](../../devlog/changes/2026-08-05-kworker.md)及其Git cutover commit。

## KWORKER-004 — Threaded timer handoff

**规则：** timer core继续唯一拥有deadline、`TimerEvent`、expiry判定与cancellation；到期threaded callback转交请求
所属CPU的system worker queue。本地timer IRQ自然命中当前CPU；realtime step的全CPU重检可以在锁外取得目标CPU的窄
submission capability，但不得读取kworker queue、slot或kthread表示。timer不再拥有private ready queue、worker slot、
entry、initcall或drain/wake loop。callback仍只在expiry或realtime step选定唯一terminal cause后、interrupts enabled且
允许抢占的process context执行；timer cancellation/stale validity仍由各timer object owner闭合。system-worker queue
ordering或delay不成为timer ABI。

**违反表现：** timer重新建立第二套worker/queue truth；expiry在hard IRQ执行process-context callback；或kworker改变timer
deadline/cancellation语义。

**验证 / Enforcement：** timer residual search；threaded timer callback/burst与remote owner-CPU dispatch KUnit；source
call-chain audit确认`ITIMER_REAL`、timerfd与network deadline仍经`schedule_threaded_timer_event()`进入timer core，且
validity、generation、deadline与recheck truth仍由各consumer owner持有。2026-08-07 integration在RV64/LA64 QEMU
SMP4分别通过565/565 KUnit、POSIX timer用户态oracle与socket LTP 6/6，并完成orderly shutdown。

**最初来源：** threaded timer event implementation与本次system-worker migration target。

**当前来源：** [2026-08-05 kworker change record](../../devlog/changes/2026-08-05-kworker.md)及其Git cutover commit；
2026-08-07 `github/main` integration merge对target-CPU submission与realtime step handoff的原子refinement。
