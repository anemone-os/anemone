# Sched Latch Tracking Issues

**状态：** Protocol Closed / Implementation Gates
**最后更新：** 2026-06-03
**父 RFC：** [RFC-20260603-sched-latch](./index.md)
**事务日志：** [2026-06-03 - Sched Latch](../../devlog/transactions/2026-06-03-sched-latch.md)

本文只跟踪当前仍影响实现顺序、review gate、停止边界或验收判断的问题。历史 review 材料放在 `backgrounds/`；canonical 不变量以 [不变量需求](./invariants.md) 为准。

## 分类总览

当前没有 Still open plan gap。所有原 Keter 项都已由 RFC text 在协议层闭合，并作为未来实现阶段的验收 gate 保留；Euclid 项只保留会影响阶段验收或诊断证据的 gate；Safe 项不作为阻塞项。

| Issue | 当前分类 | 收口依据 |
| --- | --- | --- |
| KETER-001 | Accepted implementation gate | [不变量需求](./invariants.md) 的 “Poll / Select 注册协议”；[迁移实施计划](./implementation.md) 的阶段 2、4、5 |
| KETER-002 | Accepted implementation gate | [不变量需求](./invariants.md) 的 “Poll / Select 注册协议” / “source wake 侧要求”；[迁移实施计划](./implementation.md) 的阶段 2、3 |
| KETER-003 | Accepted implementation gate | [不变量需求](./invariants.md) 的 “单 consumer 与多 producer”；[迁移实施计划](./implementation.md) 的阶段 1 |
| KETER-004 | Accepted implementation gate | [不变量需求](./invariants.md) 的 “状态所有权” / “线性化点”；[迁移实施计划](./implementation.md) 的阶段 1、4、5 |
| KETER-005 | Accepted implementation gate | [不变量需求](./invariants.md) 的 “闭合条件” / “线性化点”；[迁移实施计划](./implementation.md) 的阶段 1 前置条件 |
| KETER-006 | Accepted implementation gate | [不变量需求](./invariants.md) 的 “Source 队列与生命周期”；[迁移实施计划](./implementation.md) 的阶段 1 退出条件和阶段 2 前置条件 |
| KETER-007 | Accepted implementation gate | [不变量需求](./invariants.md) 的 “单 consumer 与多 producer” / “禁止退化项”；[迁移实施计划](./implementation.md) 的阶段 1 |
| KETER-008 | Accepted implementation gate | [不变量需求](./invariants.md) 的 syscall outcome mapping；[迁移实施计划](./implementation.md) 的阶段 4、5 |
| EUCLID-001 | Accepted implementation gate | [迁移实施计划](./implementation.md) 的阶段 4、5 |
| EUCLID-002 | Accepted implementation gate | [不变量需求](./invariants.md) 的 “可观测性”；[迁移实施计划](./implementation.md) 的阶段 2 可观测性 |
| EUCLID-003 | Accepted implementation gate | [不变量需求](./invariants.md) 的 “可观测性”；[迁移实施计划](./implementation.md) 的分阶段可观测性与总清单 |
| SAFE-001 | Closed by RFC text | [迁移实施计划](./implementation.md) 的阶段 1 类型草案采用 `make_trigger()` / `LatchTrigger::trigger()` 角色拆分 |

## Still Open Plan Gaps

None。

如果后续审查发现会改变 wait identity、completion 线性化点、owner boundary、source register gate、source wake detach、cleanup 非正确性支柱、placement 责任或 `ppoll` / `pselect6` outcome mapping 的缺口，必须在这里新增 Still open plan gap，并先修 RFC 文本。

## Accepted Implementation Gates

### KETER-001：register scan 必须 fail closed

**状态：** Gate

**问题：** 半迁移 source 如果只做 snapshot、忽略 trigger 或静默返回 not-ready，会让 `ppoll` / `pselect6` 睡在永远不会触发它的 source 上。

**协议收口：** RFC 要求 typed register result：`Ready(events)`、`Armed`、`Unsupported` / `NotRegistered` 或等价状态。syscall 只有在所有参与阻塞判定的 source 都 ready、armed 或明确 unsupported/fallback 后，才允许进入 latch schedule。

**实现 gate：** 阶段 2 必须提供 typed register API；阶段 4/5 必须证明未 armed source 不会进入 schedule。

### KETER-002：wake-side source 临界区线性化点必须硬化

**状态：** Gate

**问题：** source wake 如果先 detach、后发布 ready，或在 predicate update 与 detach 之间释放锁，register scan 可能在窗口内看到 not-ready 并注册新 trigger，而 wake 路径已经错过它。

**协议收口：** RFC 要求任何使 predicate 变 ready / hangup / error 的状态变更，必须在同一 source lock 临界区内完成 predicate update 与对应 trigger 队列 detach；释放 source lock 后才能执行 detached trigger。

**实现 gate：** 阶段 2/3 必须逐 source 审计 predicate update + detach 的临界区，并证明 source 不在持锁时进入 wait core wake。

### KETER-003：`Latch` consumer capability 必须是 owner-bound linear guard

**状态：** Gate

**问题：** consumer handle 如果跨 task 使用，可能由错误执行上下文 retire 另一 task 的 active wait，破坏 single-consumer、状态所有权和 `wake_active_wait()` 的当前 active wait 假设。

**协议收口：** RFC 要求 `Latch` 字段私有、不可 clone、不可伪造，`finish(self)` 消耗对象，并要求 `!Send` / `!Sync` 或所有 waiter-owned 方法校验 `current_task == owner_task` 并 fail closed。

**实现 gate：** 阶段 1 必须用类型边界或 runtime guard 执行 owner boundary，并审计 double finish、drop-without-finish 和 owner 校验失败。

### KETER-004：cancel / finish 线性化和 reason 边界必须固定

**状态：** Gate

**问题：** cancel 覆盖 winning outcome 会破坏 first-completion-wins；cancel 后未 retire 会让 old trigger fail-closed 依赖 source cleanup；任意 `WaitReason` 会污染日志和 syscall outcome 分类。

**协议收口：** RFC 要求 waiter-owned `cancel()` 是同一 wait core transaction 上的一次可输 completion attempt；每个 `begin*()` 后必须 exactly-once `finish()` 或等价 retire；consumer cancel 使用受限 `LatchCancelReason` / `ConsumerCancelReason` 或等价枚举。

**实现 gate：** 阶段 1 必须确认 cancel 不覆盖已有 outcome；阶段 4/5 必须证明所有 begin 后路径 exactly-once finish/retire。

### KETER-005：非 producer completion 也必须共享 stale-safe placement 合同

**状态：** Gate

**问题：** timeout、signal、force 与 producer trigger 面对同一类 park race。如果只做逻辑完成而没有 stale-safe physical placement，wait round 可能已完成但 task 不可运行。

**协议收口：** RFC 把 placement 写成通用 completion 不变量：producer trigger、timeout、signal、force 和 consumer cancel 都竞争同一个 `WaitState`，完成后由 wait core 负责逻辑完成和 stale-safe placement。

**实现 gate：** 阶段 1 前置条件必须确认 `wake_wait()` / `wake_active_wait()` 的 `Woke` 语义包含 post-commit stale-safe placement，且 timeout、signal、force 入口不会绕过该合同。

### KETER-006：weak/strong trigger 生命周期策略必须在 source 注册协议落地前闭合

**状态：** Gate

**问题：** source 队列如果保存 strong `WakeToken`，每轮 `poll` / `select` 残留 trigger 可能长期保活已结束 wait round。weak vs strong 不能等 `ppoll` 迁移后再补。

**协议收口：** RFC 允许 weak trigger 或 strong trigger + lazy pruning / 显式 cleanup，但要求正确性不依赖 cleanup，并要求资源上界和 pruning gate 在 source 注册协议落地前确定。

**实现 gate：** 阶段 1 退出必须记录 weak/strong 策略作为阶段 2 前置约束；阶段 2 前置条件必须已有资源策略；阶段 2/3 退出必须能说明 cleanup / pruning 上界。

### KETER-007：producer capability 必须 no-return、fail-closed、隐藏 `WakeToken`

**状态：** Gate

**问题：** source 如果根据 stale/retired/woke 分支补 enqueue、重试其他 wait round 或改变 readiness 行为，会重新制造第二套 completion 事实。

**协议收口：** RFC 要求 producer-side trigger 可 clone，但只暴露 no-return / fail-closed trigger 能力；普通 source 不得取得 `WakeToken`、强 `Task` 引用、`WakeResult`、`WakeEnqueueResult` 或 wait-core waiter lifecycle API。

**实现 gate：** 阶段 1 审计必须确认普通 source 无法取得 token 或 wake 诊断结果，且 producer 触发失败不会 panic、补 enqueue 或重试其他 wait round。

### KETER-008：`ppoll` / `pselect6` outcome mapping 必须统一

**状态：** Gate

**问题：** wait core 线性化只解决“谁完成本轮 wait”，不会自动决定 syscall 返回 ready count、0、`EINTR` 或错误。两个 syscall 分别解释 race 会造成 Linux-compatible 行为分裂。

**协议收口：** RFC 固定最低 outcome mapping：finish 后先做 final readiness scan；ready 可见则返回 ready；final scan 仍无 ready 时，再按 winning outcome 映射 timeout、signal/force、cancel/error。`ppoll` 与 `pselect6` 必须共享同一套 mapping。

**实现 gate：** 阶段 4 必须在 `ppoll` 中固定 final scan 与 outcome mapping；阶段 5 必须复用同一套 loop/helper 或同一条注释规则，不允许分裂解释。

### EUCLID-001：timeout-zero / signal precheck 顺序需要保持一致

**状态：** Gate

**问题：** zero-timeout poll/select 本质是非阻塞 snapshot；已有 signal precheck 或 deadline expired 时不应先创建 `Latch` 再 cancel。

**协议收口：** 阶段计划规定 snapshot 后先处理 signal/deadline/zero-timeout；只有确实需要阻塞时才 begin/register。

**实现 gate：** 阶段 4/5 审计早返回路径，避免无意义 stale trigger 和 cleanup 压力。

### EUCLID-002：source 队列 entry 的诊断身份需要落到注册协议

**状态：** Gate

**问题：** 这不参与 correctness；但出现 stale pruning、late trigger 或 lost wake 疑似问题时，需要知道是哪类 source、哪次注册、哪个 interest 留下的 trigger。

**协议收口：** RFC 的可观测性要求记录 wait id、source 注册点、trigger 结果、placement 结果、finish outcome 和 final scan。

**实现 gate：** 阶段 2 可观测性必须让 source 队列 entry 至少能记录 wait id、source/debug id、interest 和注册点。

### EUCLID-003：可观测性清单需要变成阶段验收

**状态：** Gate

**问题：** 缺少 trace gate 时，stale / retired / placement / final scan 的 fail-closed 行为只能靠代码推理。

**协议收口：** RFC 把可观测性列为诊断要求，并明确日志不能成为协议正确性依赖。

**实现 gate：** 阶段 1、2、3、4、5 均有可观测性要求；阶段 6 总清单必须能关联 registration、trigger、placement、finish outcome、final readiness scan 和 cleanup / pruning 上界。

## Closed By RFC Text

### SAFE-001：`Latch::trigger()` 与 `LatchTrigger::trigger()` 命名混淆角色边界

**状态：** Closed

**问题：** 如果 consumer-side API 也叫 `trigger()`，会混淆“派生 producer capability”和“实际触发”的角色边界。

**收口：** 阶段 1 类型草案采用 `make_trigger()` 派生 producer handle，并保留 `LatchTrigger::trigger()` 作为唯一 fire 动词。该项不改变协议，不作为实现阻塞项；后续只需避免把 consumer-side API 命名成 producer fire 动词。

## Apollyon

当前未发现 Apollyon。草稿没有把单轮 wait identity、old trigger fail-closed、cleanup 非正确性支柱、source detach 后 wake 或 wait core placement 这些核心方向写反。
