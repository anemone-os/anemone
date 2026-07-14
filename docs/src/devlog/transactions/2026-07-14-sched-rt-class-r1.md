# 2026-07-14 - Sched RT Class R1

**Status:** Active
**Owners:** doruche, Codex
**Area:** scheduler / realtime / RR rotation / scheduler core contract
**Target Revision:** R1
**Canonical Plan:** [RFC-20260711-sched-rt-class](../../rfcs/sched-rt-class/index.md), [不变量需求](../../rfcs/sched-rt-class/invariants.md), [R1 增量实施计划](../../rfcs/sched-rt-class/implementation.md), [Tracking Issues](../../rfcs/sched-rt-class/tracking-issues.md)
**Current Phase:** C1 ready / not started

## Scope

本事务独立实现 `sched-rt-class` R1。R0 已由 [2026-07-12-sched-rt-class](./2026-07-12-sched-rt-class.md) 完成并关闭；本事务不重开旧 transaction，也不改写其 phase log。

R1 修复 scheduler core 与 RT class 之间的跨事务 cause continuation：core pending 只保留一次 full-pick request，RR expiry 的队尾 rotation 由 RR entity 在 request 产生时提交，后续 lifecycle transaction 消费。Fair / Idle 只做 trait 机械适配，算法不变。

非目标包括：改变 pending take / restore / acknowledgement、wait-core、current-on-ready-queue 设计、动态 policy / priority mutation、SMP migration、调度 ABI、Fair 算法或 archived EEVDF 算法。

## Invariants

- `RtPolicy::RoundRobin { remaining_ticks, rotation_due }` 是 RR budget 与 committed rotation obligation 的唯一行为真相源。
- 只有 active current 可以携带 `rotation_due == true`；fresh、queued、blocked 与 exiting task 必须 clear。
- expiry 且存在同 priority peer 时先提交 rotation，再请求 full pick；延迟、arrival 和 request-time peer 消失不撤销已提交义务。
- preempt 原子消费 rotation：true 入队尾，false 入队头；yield/handoff 消费并入队尾，block/exit clear 且不 refill budget。
- `PendingResched` 是 scheduler-core-owned typed single bit；保留 destructive take、caller-owned union restore 和 successful full-pick acknowledgement，不进入 class transaction。
- `Realtime > Fair > Idle`、priority-first ordering、FIFO no rotation、Fair pass / heap / floor / yield 语义保持不变。

## Handoff

**Last Updated:** 2026-07-14

**Current Branch:** `dev/drc/sched-params`

**Completed:** R1 repair contract 已由主审与独立 reviewer 交叉校对；R0 accepted / closure Git evidence 已确认。D0 canonical R1 docs、cross-RFC alignment、transaction、navigation、whitespace / mdBook validation 与独立 review 已关闭。

**In Progress:** 无。C1 kernel implementation 尚未开始。

**Open Blockers:** `KETER-RT-007` 在 C1/C2 实现、验证和独立 review 前保持 Active。

**Next Action:** 用户启动实现后，仅按 RFC C1 原子 write set 修改 scheduler core / trait / RT，并对 Fair / Idle 做机械适配。

**Do Not Redo:** 不恢复 `ReschedCause`；不把 rotation 放入 core pending、Task sibling field或 queue node；不为 RT 复制 current identity；不改 pending acknowledgement；不借 trait 适配修改 Fair 算法。

## Phase Log

### 2026-07-14 - R1 触发与协议裁定

**Phase:** D0 前置 design review。

**Trigger:** R0 的 `task_tick()` / arrival decision 先向 processor 登记带 cause 的 resched request，之后 `requeue_preempted_current()` 再读取 cause 决定 RT current 的队头 / 队尾 placement。该形状要求 request 产生与消费之间的 current execution segment、class state 和 queue condition 保持隐式一致，却没有由 owner state 或类型合同表达。

**Adjudication:** cause 不是 core 需要保留的事实。Fair 在 `task_tick()` 内完成 pass charge，preempt requeue 不需要知道 trigger；RT/RR 唯一需要延续的是“本次 active segment 已提交队尾 rotation”。该义务归属 `RtPolicy::RoundRobin::rotation_due`。core 删除 Tick / RunnableArrival taxonomy，pending 收窄为 typed single-bit pending-pick snapshot。

**Cross-review:** 独立 reviewer 确认不需要 core epoch/token、RT-local `Weak<Task>` 或 current-on-ready-queue 改造；`Processor::running_task` 与 current-only class method entry 足以提供 lifecycle proof。review 同时确认 peer 在消费前消失不撤销已提交 rotation，多个 expiry 可以安全合并为一个 bool。

**Feedback:** 该修正改变 state ownership、scheduler trait 与 accepted invariant，因此建立 R1 semantic revision 和独立 transaction；R0 Completed 事务保持不变。

**Validation:** 本条只记录 accepted design；kernel build、KUnit、QEMU 和 LTP 均未运行。

**Next:** 完成 D0 canonical docs、cross-RFC alignment、navigation、whitespace / mdBook validation 与独立文档 review。

### 2026-07-14 - D0 Canonical Revision 与独立 Review 收口

**Phase:** D0 - R1 文档与事务闭合。

**Change:** `sched-rt-class` 建立可验证 R0 baseline 和 consolidated R1 `index.md` / `invariants.md`；`implementation.md` 保留 R0 Completed 阶段并追加独立 D0/C1/C2；tracker 新增 active `KETER-RT-007`。建立本 R1 transaction，并同步事务索引、当前双周 devlog、`rfcs.md` 与 `SUMMARY.md`。Fair / Stride、wait-preempt 与 Closed/deferred EEVDF 的 shared scheduler contract 已统一为 core-only single-bit pending；旧 implementation / issue 历史通过 supersession banner 保留，不改写 completed evidence。

**Review:** 独立 reviewer 首轮发现 EEVDF implementation 顶层迁移原则仍把多 bit cause / class pending 参数写成当前合同，定级 Keter；修正为 core-only pending。后续复审发现 EEVDF canonical/current navigation 仍把 RR 写成现行 default，定级 Euclid；已明确区分“EEVDF 关闭时恢复 RR”与“后来 Fair / Stride supersede 为 Fair”。最终复审确认先前三项均 neutralized，当前工作树无剩余 Apollyon、Keter 或 Euclid。

**Validation:** `git diff --check` clean；新建 transaction 的 `git diff --no-index --check` 无 whitespace 报告；`mdbook build docs` 通过，只输出既有 large search-index warning。旧 R0 transaction 的 `git diff --numstat` 为空，确认未被重开或改写。

**Boundary:** 本阶段只修改 docs；未运行 kernel build、format、KUnit、QEMU 或 LTP，也不把 C1/C2 写成已执行。`KETER-RT-007` 在 C1/C2 implementation、validation 和独立 review 前保持 Active。

**Next:** 仅在用户启动实现后进入 C1 原子 code gate；先按 canonical write set 删除 class-visible pending cause 并实现 RT-owned `rotation_due`，不得自动推进 C2 或扩大范围。

## Open Items

- `KETER-RT-007`：accepted repair pending C1/C2 implementation。

## Closure

Active；R1 尚未实现或关闭。
