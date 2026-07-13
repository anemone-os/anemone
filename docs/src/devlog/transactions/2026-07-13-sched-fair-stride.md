# 2026-07-13 - Sched Fair / Stride

**Status:** Active
**Owners:** doruche, Codex
**Area:** scheduler / fair class / stride / nice / runqueue
**Canonical Plan:** [RFC-20260713-sched-fair-stride](../../rfcs/sched-fair-stride/index.md), [不变量需求](../../rfcs/sched-fair-stride/invariants.md), [迁移实施计划](../../rfcs/sched-fair-stride/implementation.md)
**Current Phase:** Checkpoint 1 待启动

## Scope

本事务按 canonical implementation 的阶段顺序实现第一版 Stride-backed `Fair` scheduler class。
阶段之间严格执行 write set、停止条件、验证 floor 和 review gate；未经批准不扩大 worker write set，
真实 owner boundary 需要扩张时先停止并回到 RFC review。

阶段 0 只审计 live scheduler/Kconfig/test surface，并把既有通用公平性 workload 单一迁移为
`fair-test` 后安装到公共 rootfs。它不修改 scheduler class 生产代码、默认 policy 或 runtime 接线。

## Invariants

- `Fair` 是稳定 class identity，当前唯一 backend 通过编译期 alias 指向 Stride。
- class precedence 只有 `class/mod.rs` 一份真相；阶段 0 不修改 production class graph。
- `Task::nice()` 是 Fair weight 的唯一长期来源，不增加 cached nice/weight 或调度属性 transaction。
- pass、heap、placement floor、current 和 membership 只由 owner CPU method-first transaction 修改。
- 阶段 0 source audit 命中任一 canonical 停止条件时，不进行 test rename 或 rootfs 修改。
- accepted IRQ-off allocation issue 保持有效，本事务不宣称修复。

## Phase Log

### 2026-07-13 - RFC 接受与事务建立

**Phase:** 阶段 0 前置。
**Change:** 公开 RFC 从 Draft 转为 Accepted / Implementing；建立 transaction devlog，并接入事务索引、mdBook 导航与当前双周 devlog。
**Audit:** Tracking Issues 当前无 open Apollyon、Keter 或 Euclid；阶段 0 write set、验证 floor、runtime owner 和停止条件使用 canonical implementation 原文，不在 transaction 中另建平行合同。
**Observability:** 本条仅建立执行记录；尚无代码或 runtime observability 变化。
**Feedback:** None；原目标、不变量、阶段顺序与 accepted contract 未改变。
**Validation:** 阶段 0 验证尚未运行；本条不把 transaction 建立写成阶段 0 完成。
**Next:** 先完成阶段 0 只读 source audit；未命中停止条件后才迁移 `fair-test` 资产。

### 2026-07-13 - 阶段 0 只读 Source Audit

**Phase:** 阶段 0 - source-audit gate。
**Change:** 无生产文件变化；完成 scheduler class/core、Kconfig、constructor、nice、归档 EEVDF、fair workload 与公共 rootfs owner 的只读审计。
**Audit:** `Scheduler` trait 已覆盖全部 Stride lifecycle transaction；`local_pick_next()` 是 `RunQueue::pick_next_task()` / `set_next_task()` 的唯一 production caller，并在同一 owner-CPU IRQ-off closure 内连续执行。yield 统一进入 `requeue_yielded_current()` 后必经 scheduler full pick；Tick 先执行 class `task_tick()`，再把 `Tick` 写入合并 pending latch。production fresh non-idle task 均调用 `SchedEntity::new_default()`；clone 只在发布前继承 typed nice，published setter 保持 weak update。Kconfig 使用单一受约束 enum selector。归档 `eevdf.rs` 未进入 production graph。三份 tracked 公共 rootfs 是全部安装 `user-test` 的 manifest，安装 `fair-test` 不需要修改公共测试入口。
**Observability:** `eevdf-test` 四组 workload、阈值与 fixed-nice barrier 已锁定；迁移只改变 package/app/artifact/output identity，并删除 EEVDF anomaly 专属 BEGIN 文案。
**Feedback:** None；未命中阶段 0 停止条件，原目标、不变量、write set 与验证 floor 均保持不变。
**Validation:** 两个只读审查任务分别覆盖 scheduler transaction surface 与 config/test/rootfs owner；均报告 PASS / no STOP。build、format 与文档验证尚未运行。
**Next:** 在阶段 0 write set 内执行单一 `fair-test` 资产迁移并安装到三份公共 rootfs。

### 2026-07-13 - 阶段 0 Fair Test 资产与 Gate 收口

**Phase:** 阶段 0 - Live Source 与 Fair Test 资产前置 Gate。
**Change:** 将 tracked `anemone-apps/eevdf-test` 单一迁移为 `anemone-apps/fair-test`；package、lock root、app、artifact 与输出前缀统一改为 `fair-test`，删除 EEVDF anomaly 专属 BEGIN 文案，不保留旧 wrapper 或第二份 binary。`minimal.toml`、`pretest-rv64.toml` 与 `pretest-la64.toml` 各安装一次同一 `fair-test` app；未修改 `user-test` 或其它公共测试入口。
**Audit:** 四组 workload、阈值与 fixed-nice barrier 保持不变；新 app production source 无 EEVDF 标识，三份 tracked 公共 rootfs 是全部安装 `user-test` 的 manifest。live scheduler/Kconfig source audit 与两个只读 reviewer 均未命中阶段 0 停止条件。旧 `anemone-apps/eevdf-test/target` 只包含 ignored 本地 build cache，不是 tracked app、wrapper、artifact contract 或提交内容。
**Observability:** 用户侧输出身份统一为 `fair-test:`；equal-progress、nice-direction、bounded-yield 与 sleep-wake-progress 的 case 边界继续独立可观察。
**Feedback:** None；阶段顺序、write set、验证 floor、runtime owner、accepted goal 与不变量均未改变。IRQ-off allocation issue 保持 Open，本阶段不宣称修复。
**Validation:** `just app build --arch riscv64 fair-test`、`just app build --arch loongarch64 fair-test` 与 `just fmt fair-test --check` 通过。target 产物分别识别为 RISC-V 与 LoongArch 静态 ELF；最终 exported artifact 与 LoongArch source artifact hash 一致。rootfs/source/private-link audit 通过。修复 `SUMMARY.md` transaction 同级缩进后，`git diff --check` 与 `mdbook build docs` 复跑通过；新建 transaction/app 文件的 `git diff --no-index --check` 无 whitespace 报告。
**Next:** 阶段 0 退出条件关闭；Checkpoint 1 必须按 canonical write set 实现非默认 graph 中的 Fair identity、Stride state、heap/lifecycle 与 focused KUnit，并在提交前完成独立 review gate。

## Open Items

- Checkpoint 1-3 尚未开始。
- 用户侧 `fair-test` 与同 checkout RT/RR A/B runtime evidence 属于 Checkpoint 3，本阶段不运行。

## Closure

事务尚未收口。
