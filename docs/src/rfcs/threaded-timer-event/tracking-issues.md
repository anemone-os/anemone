# Threaded Timer Event Tracking Issues

**状态：** Active
**最后更新：** 2026-06-20
**父 RFC：** [RFC-20260620-threaded-timer-event](./index.md)
**来源：** 2026-06-20 文档层审查 / 2026-06-20 用户裁定

本文只跟踪 design review 后确认的 threaded timer event RFC 缺陷、证明缺口、边界冲突或需要回到 RFC 修改的设计问题。

实现前已知缺口、当前基础设施状态、暂缓范围和阶段性交付项通常不写入本文；它们属于 [RFC index](./index.md) 的背景、非目标、风险，或 [迁移实施计划](./implementation.md) 的阶段内容。受控实现反馈不新建通用 feedback 文件；计划写在 [迁移实施计划](./implementation.md#probe--vertical-slice-gates)，执行结果进入 transaction devlog。审查中明确选择为 limitation 的问题可在本文记录决策，但 canonical limitation text 仍必须落回 RFC / implementation / register。

分级沿用 Anemone review 口径：

- **Apollyon**：当前必须修复的错误结果、数据损坏、安全问题、崩溃或严重不可恢复状态。
- **Keter**：会阻塞后续实现方向或导致核心抽象不可复审，必须修正或明确改边界。
- **Euclid**：值得修正，但通常不阻塞第一版实现。
- **Safe**：记录即可，除非顺手修正。

## Apollyon

- 暂无。

## Keter

- 暂无 active Keter。2026-06-20 审查中的 Keter 已按用户裁定折回 canonical 文本或移入下方 active Euclid proof gap。

## Euclid

### TTE-005：probe gate runtime evidence and exit path

**状态：** Active

**观察：** Gate P1 的 hypothesis 是 worker 可以在 process context 执行 callback，但当前 validation floor 仍以 `just build` 和源码审计为主。它能证明接口形状和不走 IRQ callback，却不能单独证明 callback 实际由 worker 执行、且执行时不在 IRQ context。P1/P2/P3 也只有 `Write-back Target`，还没有显式 `Exit` 字段说明成功、失败和临时 self-check 的收口归属。

**结论：** 这不阻塞 RFC 继续收口，但进入实现前需要决定 P1 是否要求 KUnit、boot smoke 或临时 self-check 之一作为最小运行证据，并为所有 probe gate 补齐 exit path。

**处理方向：** 若选择加入运行证据，把 [迁移实施计划](./implementation.md) 的 Gate P1 `Validation Floor` 和阶段 2 验证更新为：KUnit、boot smoke 或临时 self-check 中至少一种；临时 self-check 必须在 transaction devlog 记录证据并在收口前移除。为 P1/P2/P3 增加 `Exit` 字段：成功后进入正式阶段或 transaction 记录，失败后删除临时探针、回写 RFC / tracker，或登记 limitation / open issue。若选择不加入运行证据，需在本 issue 记录接受理由。

### TTE-006：ITIMER_REAL signal action commit point

**状态：** Active

**观察：** 草案要求 `SIGALRM` 投递不在 itimer state lock 内执行，但还没有定义 lock 内生成本地动作后，cancel / setitimer 与 expiry 竞争时哪个点算 signal action 已提交。

**结论：** 若实现直接把当前持锁 `recv_signal()` 改成锁外执行，而不定义 commit point，可能改变 `ITIMER_REAL` 在 expiry 与 cancel / replace 竞争时的可见语义。

**处理方向：** 在 [不变量需求](./invariants.md) 中补充 `ITIMER_REAL` action commit 线性化点：callback 在 itimer state lock 下确认 token 有效并生成 `SIGALRM`/rearm action 即 completion commit，释放锁后无条件执行该 action；若希望 cancel 仍可撤回已生成 action，则必须单独设计 pending-signal 撤销语义，不得混在 threaded timer 迁移中。

### TTE-007：ITIMER_REAL validation floor too broad

**状态：** Active

**观察：** 阶段 4 / Gate P3 当前只写复用 itimer 或 signal timer 相关 LTP / existing case。该描述不足以约束验证必须覆盖 stale no-op、interval rearm、real-only 范围和锁外 `SIGALRM` 投递。

**结论：** 泛化 signal profile 可能无法覆盖本 RFC 实际保护的不变量。

**处理方向：** 在 [迁移实施计划](./implementation.md) 阶段 4 和 Gate P3 的 `Validation Floor` 中列出具体 LTP/profile/case；若无现成 case，要求定向 smoke + 源码审计覆盖 real-only、interval rearm、锁外 `recv_signal()` 和 stale no-op。

## Safe

### TTE-004：register non-closure for post-summary LTP hang

**状态：** Active

**观察：** register 中 `ANE-20260616-LTP-POST-SUMMARY-HANG` 的根因范围包含 timer / wait-core / task exit。threaded timer event 可以作为后续排查线索，但本 RFC 不应在没有父/子状态和 cleanup 阶段证据的情况下隐式关闭该 register issue。

**结论：** 本 RFC 不关闭 `ANE-20260616-LTP-POST-SUMMARY-HANG`。除非后续证据证明根因就是 timerfd / itimer IRQ callback，否则该 register issue 仍按自身 exit condition 收敛。

**处理方向：** 公开 RFC 文本已保留该非闭合边界；后续只有专门调查证据证明根因属于本 RFC 范围时，才可回写 register 后关闭。

## Neutralized

### TTE-001：IRQ worker wake locality and placement proof

**状态：** Neutralized / Awaiting Gate P1 Evidence

**修复落点：**

- [RFC index](./index.md) 的 Locality 段改为 timer core-owned per-CPU worker slot，不要求扩大 `KThreadHandle` public surface。
- [不变量需求](./invariants.md) 要求 IRQ handler 按 `cur_cpu_id()` 选择 ready queue 和 timer core-owned worker slot，并在 `wake()` 前断言 `slot.cpu == cur_cpu_id()`。
- [迁移实施计划](./implementation.md) 阶段 0、阶段 2 和 Gate P1 把 worker slot proof、`handle.wake()` 下游本地性审计、remote IPI / blocking placement 禁止项纳入验证。

**结论：** 原先的 remote wake / blocking placement 风险已折回 canonical 文本。实现期仍必须在 Gate P1 证明 `on_timer_interrupt()` 投递 threaded event 后按本 CPU worker slot wake，存在 `slot.cpu == cur_cpu_id()` 断言或等价证明，且 wake path 不走 remote IPI、blocking wait、普通锁或复杂分配。

**原问题：** threaded timer worker 使用 per-CPU worker，但草案尚未证明 IRQ handler 唤醒 worker 时一定命中本 CPU worker，且不会通过 `KThreadHandle::wake()` / `Event::publish()` / wait-core placement 进入 remote IPI、阻塞等待或复杂分配路径。

### TTE-003：ready queue allocation contract

**状态：** Neutralized / Accepted noirq allocator premise

**修复落点：**

- 用户裁定当前内核 heap allocator 和 page allocator 明确是 noirq-capable，该前提保持。
- [RFC index](./index.md) 和 [不变量需求](./invariants.md) 已把 threaded-ready IRQ allocation 表述为依赖当前 noirq heap / page allocator 的简单、bounded allocation。
- [迁移实施计划](./implementation.md) 阶段 2 和 Gate P1 要求审计 allocation 不进入阻塞、reclaim、普通锁、用户可见 rollback、event loss 或 merge。

**结论：** 第一版不需要默认改成 IRQ 不分配。若未来 allocator contract 改变，或实现发现 IRQ 投递必须进入可睡眠 reclaim / recoverable allocation failure / event loss，必须回到 RFC review 收紧资源准备模型。

**原问题：** 草案允许 IRQ 投递路径做简单分配，但未说明 allocator 是否 IRQ-safe；这可能让实现误用复杂 heap growth / reclaim 路径，破坏 timerfd / itimer 可见语义。

### TTE-008：Phase 2 / Phase 3 infrastructure boundary split

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 已把原 Phase 2 timer lane skeleton 与 Phase 3 per-CPU worker 合并为 `阶段 2：Timer Core 双 Lane 与 Per-CPU Worker 基础设施`。

**结论：** 基础设施 gate 不再允许只投递 ready queue 却没有 worker 执行路径的中间关闭状态。阶段 2 退出条件必须同时证明 IRQ lane 保持、每个 online CPU 有 worker、worker slot 本地性断言成立、callback 能在 worker process context 执行。

**原问题：** 原 Phase 2 要求 IRQ 投递并 wake worker，但 worker 创建和 loop 到 Phase 3 才交付，导致 Phase 2 无法独立闭合。
