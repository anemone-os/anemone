# Sched RT Class Tracking Issues

**状态：** Active
**适用修订：** R1
**最后更新：** 2026-07-14
**父 RFC：** [RFC-20260711-sched-rt-class](./index.md)
**事务日志：** [R0](../../devlog/transactions/2026-07-12-sched-rt-class.md)、[R1](../../devlog/transactions/2026-07-14-sched-rt-class-r1.md)

本文只跟踪 confirmed design issue。实现阶段、write set、review gate 与验证 floor 以 [迁移实施计划](./implementation.md) 为准；执行事实只写入事务日志。

## Apollyon

- 暂无。

## Keter

- `KETER-RT-007`：R0 让 scheduler core 把 Tick / RunnableArrival cause snapshot 传到 `requeue_preempted_current()`，RT 再在 request 产生后的另一个 transaction 中据此决定队头/队尾。这把 core request cause 变成 class-policy continuation，却没有编码 request-time current segment、peer/higher-candidate condition 与 consumption-time lifecycle 的跨事务合同；full pick 延迟或 queue condition 变化时，class 只能依赖隐式调用顺序和过期前提。

  R1 已接受的 canonical 修正是：删除 `ReschedCause`，把 `PendingResched` 收窄为 core-only single-bit pending-pick snapshot；RR entity 增加 `rotation_due`，在 expiry 且存在 same-priority peer 时提交，并在 preempt/yield/handoff/block/exit lifecycle 中消费或清除。消费阶段不重新证明 request-time peer / candidate condition，多个 expiry 合并为一个 bool。Fair/Idle 只做 trait 机械适配。

  **状态：** Active / accepted repair pending implementation。关闭要求见 [R1 增量实施计划](./implementation.md)：focused KUnit、三个 selector build、RT/RR QEMU KUnit、source audit 和独立 review 全部通过后才可移入 Neutralized。C1 只允许修复该 Keter；若需要 core generation/token、改变 pending acknowledgement、把 rotation 移出 RT owner、重做 current/runqueue membership 或修改 Fair 算法，立即停止并回到 RFC review。

## Euclid

- 暂无。

## Safe

- 暂无。

## Neutralized

- `APOLLYON-RT-001`：首轮 RT requeue 把 Tick pending 错当成“刚到期且 budget 仍为 full”的证明，会在 deferred consumption 后对合法 remainder panic。现实现只验证 Tick source 为有效 RT/RR policy 并保留当前 remainder，delayed Tick focused KUnit 已通过；独立 review 确认关闭。
- `KETER-RT-005`：首轮 owner split 曾把 RT state/config 与 focused tests 放入共享文件，并把 `SchedEntity` facade 放错 owner。现 RT type/state/quantum/payload factory/算法/KUnit 全部限制在 `rt.rs`，`SchedEntity::{new_default,new_idle}` facade 位于 `entity.rs`，共享文件只保留 opaque storage、identity、contract 与 wiring；独立 review 确认关闭。
- `KETER-RT-006`：首轮 published entity 仍可通过 broad mutable closure 被普通 crate caller 替换。现 `SchedEntity` / class payload 不实现 `Clone`，mutable bridge 必须按值消费只可由 scheduler-class owner 构造的 token，scheduler core 只使用窄只读 membership observation；source audit 与独立 review 确认关闭。
- `KETER-RT-001`：原 ckpt1 只写 entity/rt/mod，却把 class identity 与 RunQueue dispatch 留到后续，无法从 `Arc<Task>` 取得唯一 RT payload。已将 identity、dispatch、legacy owner 删除和 constructor switch 合并为 Checkpoint 1 原子 write set；依据见 [迁移实施计划](./implementation.md) 与事务日志前置审计。
- `KETER-RT-002`：原 ckpt2 删除 legacy owner，却把 production constructor switch 留给 ckpt3，无法形成可编译原子状态。constructor 与 default selector 现已并入 Checkpoint 1。
- `KETER-RT-003`：RR full quantum 原先晚于 ckpt1 才接入配置。`sched_default_policy` 与 `rt_rr_timeslice_ms` 已前移到 Checkpoint 1 的配置 owner，并由生成的 typed kernel constant 消费。
- `KETER-RT-004`：canonical 状态、checkpoint write set、独立 review gate 与验证 floor 已同步收敛；后续执行事实只写 transaction。
- `EUCLID-RT-001`：`RtPolicy` 原先只提供 `round_robin()`，FIFO 依赖直接使用 enum variant，fresh policy API 不对称；已增加 `RtPolicy::fifo()` 并让 default construction/test fresh paths 使用成对入口。
- `EUCLID-RT-002`：首轮实现把跨 99 个 bucket 的 duplicate membership 扫描放在普通 enqueue/dequeue `assert!` 中。该扫描只服务昂贵诊断，已收窄为 `debug_assert!`；`on_runq`、expected-bucket lookup 与 missing-dequeue 的常开正确性检查保留。
- `EUCLID-RT-003`：xtask test 原先硬编码 `.defconfig` 当前必须是 `rt_rr`，混淆默认值与 selector 类型合同。测试已改为分别验证 `rt_rr` / `rt_fifo` 合法、未知值非法和零 timeslice 拒绝。
