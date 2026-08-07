# Kthread Timed Wait 当前契约

**Contract ID：** `KTHREAD-WAIT`
**状态：** Active
**Owner：** `task::kthread` cooperative wait adapter；wait identity与completion truth仍由scheduler wait core拥有
**参与领域：** task kthread / Event / scheduler wait core / threaded timer
**覆盖范围：** nonzero-duration `KThreadCtx::wait_for()`的stop/deadline完成条件、ordinary-wake重查、每轮cleanup与stale timeout isolation
**不覆盖：** signal-interruptible sleep、generic periodic-worker、业务request predicate、精确timer cadence、scheduler physical placement
**实现位置：** `anemone-kernel/src/task/kthread/{ctx,control}.rs`、`anemone-kernel/src/sched/{event,wait,mod}.rs`
**依赖：** `SCHED-WAKE-001..004`
**Pending Successor：** None
**最后核验：** 2026-08-05

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| `Running / StopRequested / Exited` phase | `KThreadControl` | handle只有request/wake/wait-exit capability，entry持`KThreadCtx` | cooperative stop truth |
| wake notification | `KThreadControl`内的`Event` | `KThreadHandle::wake()`只有pure wake capability | 促使consumer重查owner predicate |
| 当前wait identity、outcome与task wait state | scheduler wait core | Event adapter持本轮active-wait/listener/token | 裁决Event、Force、Timeout与cancel竞争 |
| 本次调用的remaining duration | `wait_for()` operation-local adapter | timer只接收本轮duration | 保持同一次调用的deadline，不形成persistent timer state |
| timeout callback lifetime与validity | scheduler wait core的weak task + wait token | kthread/OOM不可观察token或callback result | late callback只能尝试完成它携带的旧identity |

Event、timer callback、wake edge和diagnostic wait/task ID都不是stop truth，也不得反向驱动kthread phase。

## KTHREAD-WAIT-001 — Timed wait只由stop truth或deadline完成

**规则：** 对nonzero duration，`KThreadCtx::wait_for()`只有在`KThreadControl`已经发布cooperative stop truth，或本次
调用的remaining duration耗尽时才返回。`request_stop()`必须先提交`StopRequested`再发布wake；stop在listener
registration之前发生时由predicate precheck观察，在precheck之后发生时由同一个Event listener促使重查，因此wake
edge本身可以丢失，stop truth不能丢失。

ordinary Event publication与forced wake只允许完成当前内部wait round并重新检查stop/deadline，不得把wake当作
`wait_for()`成功条件。每次recheck必须建立新的wait-core identity；本轮都按begin、listener registration、
schedule/cancel、listener cleanup、finish/retire exactly once闭合。timer callback只持weak task与对应round token，
旧、已完成或已retire token不能完成新round，也不能因早期ordinary wake把task强持到原deadline。

`KThreadCtx`只公开duration能力；consumer不得取得Event、Task、wait token、completion reason或physical placement
结果。业务request truth仍由具体consumer拥有，不能缓存进timed-wait adapter。

**违反表现：** ordinary `wake()`让nonzero wait提前返回；先wake后发布stop造成永久睡眠；多个round复用token；晚到
timeout完成后续round；listener/active wait未finish；timer强持已经提前结束的task；或kthread/OOM保存第二份
deadline/stop generation。

**验证 / Enforcement：** Event/kthread source audit；常开wait-core identity/lifecycle assertions；owner-local KUnit
覆盖timeout、长wait的stop interruption、ordinary wake和同一调用内late/stale timeout；RV64 431/431 KUnit boot。

**最初来源：** [OOM Periodic Sampling小迭代](../../devlog/changes/2026-08-05-oom-periodic-sampling.md)。

**当前来源：** 同上；2026-08-05 closure提交。
