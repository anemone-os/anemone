# Sched RT Class Tracking Issues

**状态：** Active
**最后更新：** 2026-07-12
**父 RFC：** [RFC-20260711-sched-rt-class](./index.md)
**事务日志：** [2026-07-12-sched-rt-class](../../devlog/transactions/2026-07-12-sched-rt-class.md)

本文只跟踪 confirmed design issue。实现阶段、write set、review gate 与验证 floor 以 [迁移实施计划](./implementation.md) 为准；执行事实只写入事务日志。

## Apollyon

- 暂无。

## Keter

- 暂无。

## Euclid

- 暂无。

## Safe

- 暂无。

## Neutralized

- `KETER-RT-001`：原 ckpt1 只写 entity/rt/mod，却把 class identity 与 RunQueue dispatch 留到后续，无法从 `Arc<Task>` 取得唯一 RT payload。已将 identity、dispatch、legacy owner 删除和 constructor switch 合并为 Checkpoint 1 原子 write set；依据见 [迁移实施计划](./implementation.md) 与事务日志前置审计。
- `KETER-RT-002`：原 ckpt2 删除 legacy owner，却把 production constructor switch 留给 ckpt3，无法形成可编译原子状态。constructor 与 default selector 现已并入 Checkpoint 1。
- `KETER-RT-003`：RR full quantum 原先晚于 ckpt1 才接入配置。`sched_default_policy` 与 `rt_rr_timeslice_ms` 已前移到 Checkpoint 1 的配置 owner，并由生成的 typed kernel constant 消费。
- `KETER-RT-004`：canonical 状态、checkpoint write set、独立 review gate 与验证 floor 已同步收敛；后续执行事实只写 transaction。
