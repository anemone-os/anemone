# 2026-07-14 - Sched RT Class R1

**Status:** Completed
**Owners:** doruche, Codex
**Area:** scheduler / realtime / RR rotation / scheduler core contract
**Target Revision:** R1
**Canonical Plan:** [RFC-20260711-sched-rt-class](../../rfcs/sched-rt-class/index.md), [不变量需求](../../rfcs/sched-rt-class/invariants.md), [R1 增量实施计划](../../rfcs/sched-rt-class/implementation.md), [Tracking Issues](../../rfcs/sched-rt-class/tracking-issues.md)
**Current Phase:** R1 closed

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

**Current Branch:** `dev/drc/rt`

**Completed:** D0 canonical contract、C1 `39ba07a9` implementation、三个 selector build、clean-rootfs RT/RR QEMU `131/131` KUnit、source audit、独立 review 与 R1 canonical closeout 均已关闭。

**In Progress:** 无。

**Open Blockers:** 无；`KETER-RT-007` 已 neutralize。

**Next Action:** R1 无剩余动作；动态调度属性、RT bandwidth 或 archived EEVDF 重开必须进入独立 RFC / transaction。

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

### 2026-07-14 - C1 Core-Only Pending 与 RT Rotation 实现

**Phase:** C1 - 原子 code gate。

**Change:** `39ba07a9` 删除 production `ReschedCause` 和 class-visible pending continuation。`PendingResched` 移入 `processor.rs`，收窄为 scheduler-core-owned single-bit pending-pick snapshot；`schedule_preempt(pending)` 仍只用 snapshot 证明 entry 合法，pending 不再进入 `ScheduleMode`、`ScheduleDecision`、`local_requeue_preempted_current()`、`RunQueue` 或 class trait。`request_resched()` 不再接收 cause。

RT/RR 的 `rotation_due` 与 `remaining_ticks` 一起由 `RtPolicy::RoundRobin` 单独拥有。quantum expiry 且存在 same-priority peer 时先提交 obligation，再向 core 请求 full pick；delayed / repeated expiry 保持或合并该 bool。preempt 原子消费并决定 head / tail，yield 与 handoff 消费后入队尾，block / exit 清除但不 refill remainder；消费时不重新要求 request-time peer 仍存在。fresh、queued、woken 与 selected state 均要求 obligation clear。

**Boundary:** Fair / Stride 只删除 trait 参数与对应 KUnit 调用，pass、placement floor、heap、yield、nice 与 tick charge 均未变化；Idle 只做机械适配。archived `eevdf.rs` 继续不在 production graph，等待未来重开时按 R1 contract 迁移。未修改 architecture trap、wait-core、Task owner、ABI、跨 CPU 或配置 owner；没有扩大 C1 write set，也没有触发 R1 停止条件。

**Focused Tests:** RT KUnit 增加 committed rotation 的 clear/set head/tail、延迟和重复 expiry、peer 消失、yield/handoff/block/exit/wake 清除及合法 remainder 保留；processor KUnit 直接覆盖 empty/request/destructive take/union restore state。full-pick acknowledgement 与四个 architecture trap-tail 的 caller-owned deferred restore 由 source audit 证明，没有用 KUnit 修改全局 processor / trap 状态来模拟。

### 2026-07-14 - C2 验证、独立 Review 与收口

**Phase:** C2 - validation / review / closeout。

**Build:** 在被忽略的根 `kconfig` 中临时切换 selector，最新 C1 代码的 `fair`、`rt_rr`、`rt_fifo` 三次 `just build` 均通过；完成 RT selector 验证后由 `just defconfig` 恢复 repository default，最终 `fair` build 再次通过。tracked default 未修改。

**Runtime:** RT/RR build 通过 `just xtask qemu -p qemu-virt-rv64-pretest -i build/anemone.elf` 运行。首轮复跑因可变 pretest 镜像残留 fixture 在既有 openat KUnit 触发 `AlreadyExists`；按 rootfs owner 使用 `just rootfs mkfs -c conf/rootfs/pretest-rv64.toml --sudo` 重建镜像后，最新工作树 `Running 131 tests... All tests passed!`，新增 processor / RT KUnit 全部为 `ok`。

KUnit 完成后 pretest 固定继续启动 `/bin/fair-test`；RT/RR compile-time selector 会把该 workload 的普通 task 全部构造成 RT/RR，因而 Fair nice-share 专项前提不成立并失败。该 workload 不属于 R1 focused KUnit gate，也不反证 scheduler KUnit；确认结果后终止 QEMU，未扩大 rootfs / user-test write set，也未把该段写成用户态 integration pass。R0 已有的用户 RT/RR 整套 LTP 证据仍由 R0 事务拥有，R1 不重复要求 LTP。

**Source / Format:** production graph 无 `ReschedCause`；`request_resched()` 无 cause；`PendingResched` 不进入 class、RunQueue、ScheduleMode 或 ScheduleDecision；`rotation_due` 只位于 `rt.rs` 的 RR entity；四个 RV64/LoongArch kernel/user trap caller 仍只在 `SchedulePreemptResult::Deferred` 后 restore；successful full pick 仍在 `pick_next_task()` 与 `set_next_task()` 之间直接清空 slot。`git diff --check` 通过；`just fmt kernel` 后 `just fmt kernel --check` 通过，且只规范化被忽略的生成产物，没有产生 tracked 源码 diff；`mdbook build docs` 通过，只输出既有 large search-index warning。

**Review:** 未参与写入的独立 reviewer 检查完整七文件 diff。首轮唯一 Euclid 是为 full-pick clear 增加单次使用的 `acknowledge_full_pick()` helper，既弱化 owner-local 邻接，也让 KUnit 只测试脱离真实路径的包装；提交前已删除 helper，恢复直接 clear，并把 KUnit / source proof 边界写实。最终复审确认无剩余 Apollyon、Keter 或 Euclid，并明确确认 rotation 单一 owner、lifecycle clear/consume、peer disappearance、core-only pending、caller-owned restore、acknowledgement 线性化、Fair 算法不变和 archived EEVDF 边界。

**Result:** `KETER-RT-007` neutralized；C1/C2 关闭，RFC R1 返回 Closed。

## Open Items

- 无。

## Closure

Completed；R1 由 `39ba07a9` 实现并完成 C2 validation / review，canonical RFC 已返回 Closed。
