# 2026-07-12 - Sched RT Class

**状态：** Completed
**负责人：** doruche, Codex
**领域：** scheduler / realtime / FIFO / RR / scheduler class
**权威计划：** [RFC-20260711-sched-rt-class](../../rfcs/sched-rt-class/index.md), [不变量需求](../../rfcs/sched-rt-class/invariants.md), [迁移实施计划](../../rfcs/sched-rt-class/implementation.md), [Tracking Issues](../../rfcs/sched-rt-class/tracking-issues.md)
**当前阶段：** Completed；Checkpoint 2 用户态集成验证已关闭

## 范围

本事务跟踪 `sched-rt-class` RFC 的 staged implementation：

- 先证明现有 `PendingResched` full-pick acknowledgement、deferred restore 和 wait no-switch 边界可直接被 RT class 消费；
- 原子引入共享 `Realtime` class、typed priority、FIFO/RR policy、99 个 priority bucket、RR quantum、`RunQueue` dispatch、default constructor 和 Kconfig selector；
- 删除 legacy `RoundRobin` identity / queue owner，不保留双 dispatch 或 identity alias；
- 通过 source proof、focused KUnit、两个 compile-time selector build 和用户态同 priority smoke 关闭第一版。

非目标仍以 RFC 为准：本事务不实现调度属性 syscall、published-task policy/priority mutation、RT bandwidth、跨核迁移、不同 priority 用户态 workload 或 hard realtime guarantee。

## 不变量

- `RtEntity` 是 effective RT priority、policy 和 RR remaining budget 的唯一真相源；full quantum 只来自受约束的生成配置。
- `PendingResched` 只作为 pre-pick value snapshot 进入 preempted-current transaction；RT class 不保存 processor slot 或 task-local pending state。
- scheduler-class domain 集中定义跨 class precedence，`RunQueue` 单独消费该顺序并维护 physical membership；RT bucket node 不复制 priority。
- FIFO/RR 共享一个 `Realtime` identity 和同 priority FIFO 序列；不保留 legacy `RoundRobin` fallback。
- worker 未经批准不得越过当前 checkpoint write set；必要扩张先记录理由、owner surface、合同影响和验证 gate。
- bucket `VecDeque` 的 noirq allocation 风险继承 legacy RR，按用户裁定暂时接受，并继续由 [ANE-20260622-IRQ-OFF-HEAP-ALLOCATION](../../register/open-issues.md#ane-20260622-irq-off-heap-allocation) 跟踪；实现不得把它宣称为 allocation-free。

## Handoff

**Last Updated:** 2026-07-13

**Current Branch:** `dev/drc/rt`

**Canonical RFC:** [RFC](../../rfcs/sched-rt-class/index.md), [Invariants](../../rfcs/sched-rt-class/invariants.md), [Implementation Plan](../../rfcs/sched-rt-class/implementation.md), [Tracking Issues](../../rfcs/sched-rt-class/tracking-issues.md)

**Completed:** 公共 RFC 已提升。Scheduler-Core 前置 Gate source audit 已证明 full-pick acknowledgement、deferred restore、wait no-switch abort、parked handoff 和 removed abort-park surface。Checkpoint 1 已完成共享 `Realtime` class 原子切换、owner-boundary correction、两个 selector build、112 项 KUnit runtime、source/doc validation 与独立 review；`APOLLYON-RT-001`、`KETER-RT-005`、`KETER-RT-006` 已 neutralize。Checkpoint 2 由用户在 RT/RR 默认配置下完整运行整套 LTP 测例关闭。noirq bucket allocation 风险按用户裁定记录为 accepted limitation。

**In Progress:** 无。事务已完成。

**Open Blockers:** 无。FIFO 用户态专项验证未运行，但按用户裁定不属于本 RFC 的关闭 blocker。

**Next Action:** 无。若后续需要 FIFO 用户态专项证据、运行期 policy transaction 或不同 priority 验证，另开 follow-up。

**Do Not Redo:** 不重新设计 wait-core / pending acknowledgement；不把 RT payload 暂时伪装成 `RoundRobin`；不硬编码临时 quantum；不保留双 queue fallback；不通过 task-local state 或隐藏 setter 制造测试入口。

## Phase Log

### 2026-07-12 - Scheduler-Core 前置 Gate 与实施协议修正

**阶段：** 前置 Gate - source audit / document review。

**前置状态：**

- checkout root 为 `/home/doruche/dev/anemone`，分支 `dev/drc/rt`；阶段开始时 `git status --short` 为空，HEAD 为 `e7db92d7e1b5acfd509a267ef5d472017fdf7c92`。
- 已读取 repository `AGENTS.md`、`LOCAL.md`、RFC workflow/template、register open issues/current limitations、当前双周 devlog、相邻 scheduler transaction，以及 `sched-rt-class` 全部 canonical 文档。
- 私有 `etc/plans/sched-rt-class` 只用于 promotion 对照；公开 `docs/src/rfcs/sched-rt-class` 是唯一 canonical source。

**Scheduler-Core source audit：**

```sh
rg -n "PendingResched|take_pending_resched|restore_pending_resched|local_pick_next" anemone-kernel/src/sched anemone-kernel/src/arch
rg -n "schedule_preempt|DeferredPreempt|AbortWaitSleep|handoff_woken_current" anemone-kernel/src/sched anemone-kernel/src/arch
rg -n --glob '*.rs' "requeue_aborted_wait_current|aborted_wait_current|abort.*park.*class|abort.*wait.*requeue" anemone-kernel/src
rg -n "WaitState|WakeToken|WaitReason|ParkState|wait_id|wait_identity" anemone-kernel/src/sched/class
```

结果：

- `Processor::pending_resched` 是唯一长期 slot；`take_pending_resched()` 按值复制后清空，`restore_pending_resched()` 用 union 恢复 destructive-take snapshot 并保留并发新增 cause。
- `local_pick_next()` 在 IRQ disabled owner-CPU path 中先成功 `pick_next_task()`，再清 processor slot，随后调用 `set_next_task()`。Idle 兜底保证正常路径没有 selection `None`。
- rv64 / la64 的 user/kernel trap 四个 destructive-take caller 都只在 `schedule_preempt()` 返回 `Deferred` 时恢复同一 snapshot；PrePark deferred path 在 `switch_out()` 和 full pick 前返回。
- wait no-switch `AbortWaitSleep` 只返回 `DidNotSwitch`，不调用 class transaction、`switch_out()` 或 full pick，也不确认 processor slot。
- parked completion 的 production current path只通过 `local_handoff_woken_current()` 收口。
- production tree 无 `requeue_aborted_wait_current()` 或等价 abort-park class transaction；class 模块无 wait identity / park state 字段。

**前置 Gate 结论：** PASS。未命中 Scheduler-Core 停止条件；无需修改 `processor.rs`、wait-core、trap/IPI pending plumbing 或对应 scheduler-core transaction。

**文档层 review findings 与修正：**

- `KETER-RT-001`：原 ckpt1 无法在不修改 exhaustive `RunQueue` dispatch 的前提下让 `Arc<Task>` 取得唯一 `RtEntity`。修正为 Checkpoint 1 原子切换 class payload、identity、dispatch 与 legacy owner。
- `KETER-RT-002`：原 ckpt2 删除 legacy owner，却把 production constructor 迁移留给 ckpt3，无法形成独立可编译 checkpoint。constructor switch 已并入 Checkpoint 1。
- `KETER-RT-003`：RR refill 所需 full quantum 原先晚于算法接入。typed selector、timeslice config 和生成链已前移到 Checkpoint 1。
- `KETER-RT-004`：RFC entry、tracker 与 implementation 对 stage/write set 状态互相矛盾，且缺少独立 review pass rule / build floor。canonical 状态、write set、review gate 和验证 floor 已同步修正。
- noirq `VecDeque` allocation 风险不是 RT 新引入的问题；用户裁定沿用既有限制。RFC 与 transaction 链接 register，Checkpoint 1 实现必须加边界与删除条件注释。

**Checkpoint 1 worker contract：**

- 允许写入 `sched/class/{entity.rs,rt.rs,mod.rs,runqueue.rs,rr.rs}`、两架构 bootstrap、clone、`kthreadd`、`conf/.defconfig`、xtask kconfig owner和 build-generated `kconfig_defs.rs`；`rr.rs` 只允许删除。ignored 根 `kconfig` 是 live build input，只允许增加/切换本 checkpoint 的 selector 与 timeslice，必须保留其它开发者本地选项且不得提交。
- 禁止修改 `processor.rs`、wait-core、trap/IPI pending plumbing、task topology、调度属性 syscall 或其它 scheduler class 算法。
- implementation worker 与独立 reviewer 必须是不同 agent；review pass 要求无未关闭 Apollyon / Keter / Euclid。

**Review Gate：** 独立 reviewer 首轮发现 identity/write-set、atomic production cutover、full quantum source、canonical status/review floor 等 Keter；修正后又逐项补齐 live `kconfig`、Checkpoint 2 gated write set、class precedence/idle fallback、fresh-only constructor 与 published-task setter audit，以及 stable issue-ID / artifact-boundary 问题。最终 closure 未发现未关闭的 Apollyon、Keter 或 Euclid。

**Validation：** 在最终 closure edits 后，`git diff --check` clean；新增 transaction 的 no-index whitespace check clean；`mdbook build docs` 通过，只有既有 large search index warning。未运行 kernel build、KUnit、QEMU 或 LTP；Checkpoint 1 kernel implementation 尚未开始。

### 2026-07-12 - Checkpoint 1 Implementation 启动

**阶段：** Checkpoint 1 - RT Class 原子切换 implementation。

**Implementation worker：** `rt_ckpt1_impl`。worker 只允许修改：

- `anemone-kernel/src/sched/class/entity.rs`
- 新增 `anemone-kernel/src/sched/class/rt.rs`
- `anemone-kernel/src/sched/class/mod.rs`
- `anemone-kernel/src/sched/class/runqueue.rs`
- 删除 `anemone-kernel/src/sched/class/rr.rs`
- `anemone-kernel/src/arch/{riscv64,loongarch64}/bootstrap.rs`
- `anemone-kernel/src/task/api/clone/mod.rs`
- `anemone-kernel/src/task/kthread/kthreadd.rs`
- `conf/.defconfig`
- ignored live 根 `kconfig` 的新增 selector / timeslice 键
- `scripts/xtask/src/config/kconfig.rs`
- repository build flow 生成的 `anemone-kernel/src/kconfig_defs.rs`

worker 不修改 transaction/RFC；总控负责 execution facts。ignored 根 `kconfig` 必须保留已有本地选项且不得提交。worker 不运行用户态 smoke，也不提交 commit。

**Review / validation 状态：** 尚未执行。implementation 返回后再启动独立 reviewer；在 review closure 前不得提交 Checkpoint 1。

### 2026-07-12 - Checkpoint 1 Owner-Boundary Feedback

**来源：** 用户对 implementation worker 未提交代码的直接 review。

**Finding：** `KETER-RT-005` - 首轮实现把 full quantum、`RtPriority`、`RtPolicy`、`RtEntity`、state validation 和 default-policy construction 放进共享 `entity.rs`，并在 `mod.rs` 放置 RT-specific KUnit。虽然行为逻辑主体已在 `rt.rs`，但该形状让共享 class contract 承担 RT owner 的 state/config 责任，不符合窄接口和模块职责边界。

**修正合同：** write set 不扩张，功能/不变量不变。`rt.rs` 必须单独拥有 RT types/state/quantum/payload accessors/fresh constructor implementation/算法与 focused KUnit；`entity.rs` 只保留 generic storage、opaque class payload union、identity mapping 和窄 generic constructor；`mod.rs` 只保留 module/re-export、shared trait/pending contract 与 centralized precedence。修正完成后由独立 reviewer 检查 shared files 不再包含 RT policy/quantum/state logic，再决定是否 neutralize `KETER-RT-005`。

**状态：** Active；worker 已暂停，尚未执行修正或 review closure。

### 2026-07-13 - Checkpoint 1 Owner-Boundary Correction

**来源：** 用户继续 review 未提交实现，并要求后续修正由总控直接完成，不再交回原 implementation worker。

**Canonical correction：** 用户随后明确指出“`new_default` 这种门面构造函数”应位于 `entity.rs` 而不是 `rt.rs`。与此前“RT 相关逻辑限制在 `rt.rs`”合并后的边界是：`entity.rs` 拥有 `SchedEntity::{new_default,new_realtime,new_idle}` 公开 facade；`rt.rs` 拥有 selector、full quantum、`RtPolicy` / `RtEntity` 校验、fresh payload factory、RT accessors、class 算法和 focused KUnit。共享 facade 只包装 opaque payload，不实现 RT policy decision。

**新增 findings：**

- `KETER-RT-006`：`SchedEntity`、`SchedClassPrv` 与 `RtEntity` 的 `Clone` derive 保留了复制 published `on_runq` / RR remaining 的能力，与 fresh-only 和单一真相源边界冲突。
- `EUCLID-RT-001`：`RtPolicy` 只提供 `round_robin()`，FIFO fresh construction 直接使用 variant，API 形状不对称。
- `EUCLID-RT-002`：enqueue/dequeue 的普通正确性路径使用全 99 bucket membership 扫描，且以常开 `assert!` 执行；该扫描只服务昂贵的 duplicate 诊断，不应成为每次 noirq queue transaction 的 release 成本。
- `EUCLID-RT-003`：xtask selector test 硬编码 `.defconfig` 当前必须包含 `rt_rr`，把当前默认值与 typed selector 结构合同混在一起，无法独立证明两个合法 selector 和零 timeslice 拒绝。
- `APOLLYON-RT-001`：独立 reviewer 证明 Tick pending 作为合并 latch 可以延迟到后续 full pick 消费；在 `kernel_preempt=false` 的长内核执行期间，expiry refill 后的后续 timer tick 会继续递减 RR budget。首轮 `assert_refilled_rr()` 强制要求 full quantum，会对合法 remainder panic。

**直接修正：** 总控已把公开 `SchedEntity` facade constructors 移回 `entity.rs`；`rt.rs` 通过 `RtEntity::{new_default,new_fresh}` 暴露窄 payload factory；新增 `RtPolicy::fifo()` 并让 default/test fresh paths 使用对称 policy constructors；移除 `SchedEntity`、`SchedClassPrv` 与 `RtEntity` 的 `Clone` derive；将跨全部 bucket 的 duplicate 扫描收窄为 `debug_assert!`，同时保留 `on_runq`、expected-bucket lookup 和 missing-dequeue 的常开正确性检查；xtask test 改为分别证明 `rt_rr` / `rt_fifo` 合法、其它 selector 非法且零 timeslice 被拒绝。canonical RFC / invariants / implementation / tracker 已同步 owner 与 source-audit contract。

**Apollyon 修正：** 删除 full-quantum-only requeue 断言；Tick transaction 只断言 current 仍为有效 RT/RR policy，并保留 delayed consumption 时的合法 current remainder。新增 focused KUnit 覆盖“expiry 后 pending full pick 前又经过 timer tick”的路径，分别兼容 full quantum 为 1 和大于 1 的配置。

**Review adjudication：** reviewer 建议再次把 `SchedEntity::{new_default,new_realtime}` 移入 `rt.rs`，但该建议遗漏了用户后续关于 `new_default` 门面位置的明确反馈，因此未采纳。共享 facade 留在 `entity.rs`，RT selector/quantum/payload factory 继续只在 `rt.rs`。

**Review correction：** reviewer 后续证明 `<Realtime as Scheduler>::KIND` / `<Idle as Scheduler>::KIND` 是 shared precedence 与 class implementation identity 的必要 trait 连接；改为直接 enum 数组会让 `Scheduler::KIND` 变成无 consumer 的死 surface。总控撤回该中间改动，未将其保留为 finding。

**KETER-RT-006 correction：** 移除 `Clone` 仍不足以关闭 published entity mutation。`Task::with_sched_entity_mut` 公开返回完整 `&mut SchedEntity`，任意 crate caller 都可以赋值为 `SchedEntity::new_realtime(...)`，绕过 owner-CPU queue/class transaction。该 API 也是 implementation review gate 明确禁止的 direct entity mutation bridge。

**Write-set expansion request（待批准）：**

- 新增 `anemone-kernel/src/task/sched.rs`：删除无 capability 的 broad mutable closure；增加不可由普通 crate caller 构造的 class-owner mutation token/capability，并保留窄的 class identity / `on_runq` observation。
- 新增 `anemone-kernel/src/sched/processor.rs`：把 scheduler-core 的 `on_runq` 只读检查迁移到窄 observation，不获得 class payload mutation 能力。
- 继续使用已批准的 `sched/class/{entity.rs,rt.rs,runqueue.rs}`：class owner 内部通过 capability 访问 storage；RT payload mutation 仍只在 `rt.rs`，RunQueue 只维护 membership。
- 不修改 wait/pending contract、跨 CPU surface、Task layout、published policy setter 或 scheduler transaction 语义。

**验证计划：** `rg` 证明 production tree 无无 token 的 whole-entity mutable closure、无 published class/policy/priority replacement；RT/RR 与 RT/FIFO 两个 repository build；focused KUnit compile/runtime；`git diff --check`、kernel fmt（区分 generated drift）与 `mdbook build docs`；独立 reviewer 重跑 owner/fresh-only gate。

**状态：** Blocked on write-set approval；批准前不修改 `task/sched.rs` 或 `sched/processor.rs`，`KETER-RT-006` 保持 Active。

**验证：** 根 `kconfig` 保持 `sched_default_policy = "rt_rr"`；修正后 `git diff --check` clean，`just build` 通过。独立 code review 仍在进行，RT/FIFO selector build、KUnit runtime、LoongArch build、source audit 与最终文档验证尚未执行；本条不提前 neutralize active Keter。

### 2026-07-13 - Checkpoint 1 Write-set Expansion Approval

**批准：** 用户批准 `KETER-RT-006` 的最小 write-set expansion。Checkpoint 1 正式加入 `anemone-kernel/src/task/sched.rs` 与 `anemone-kernel/src/sched/processor.rs`；由总控直接集成，不交回原 implementation worker。

**Canonical correction：** `new_default()` 仍是 `entity.rs` 所有的共享 facade；此前把 public `new_realtime()` 也视为 facade 的表述经独立 review 证明过宽。custom priority/policy fresh construction、`RtPriority`、`RtPolicy` 和 RR runtime remainder 必须限制在 `rt.rs`，`mod.rs` 不公开 re-export RT runtime representation。`Task` 只提供 capability-gated entity lock bridge；capability 只能由 scheduler-class owner 构造，`processor.rs` 改用只读 membership observation。

**实施边界：** 不改变 `Task` layout、wait/pending contract、owner-CPU transaction、跨 CPU surface 或 scheduler semantics；只关闭 published entity replacement capability，并迁移现有只读 caller。RFC `index.md`、`invariants.md` 与 `implementation.md` 已先同步该 owner contract。

**状态：** Expansion approved；总控实施中。`KETER-RT-005/006` 与 `APOLLYON-RT-001` 在最终 validation 和独立复审前保持 Active。

### 2026-07-13 - Checkpoint 1 Expanded Implementation Validation

**实现：** `entity.rs` 定义 crate-visible、但 constructor 仅对 scheduler-class owner 可见的 `SchedEntityMutToken`；`Task::with_sched_entity_mut` 现在必须消费该 token。`RunQueue` 与 `rt.rs` 是 production graph 中唯一构造 token 的模块；`processor.rs` 的 membership 检查全部迁移到只读 `sched_on_runq()`。`SchedEntity::new_default()` / `new_idle()` 保留在 `entity.rs` facade，public `new_realtime()` 与 `mod.rs` 的 `RtPriority` / `RtPolicy` re-export 已删除；custom fresh construction 和 RT runtime representation 只留在 `rt.rs`。

**构建与 runtime KUnit：**

- 根 `kconfig` 为 `rt_rr` 时，扩展修正后的 `just build` 通过；切换为 `rt_fifo` 后 `just build` 通过；随后恢复 `rt_rr` 并再次 `just build` 通过，最终 generated kernel constant 与 live selector 均为 RT/RR。
- `timeout 25s just xtask qemu --platform qemu-virt-rv64-pretest --image build/anemone.elf` 启动当前 RT/RR kernel，运行 112 项 KUnit 并打印 `All tests passed!`。10 项 `sched::class::rt::kunits::*` 全部为 `ok`，包含 priority/bucket、mixed FIFO/RR ordering、strict higher-priority preempt、arrival head placement、Tick tail placement、delayed Tick remainder、FIFO no-rotation、RR budget/peer、class precedence 与 default constructor。KUnit 后已进入 init / user-test；外层 timeout 在无关的 read-write LTP 继续运行时终止 QEMU，退出码 124 不表示 KUnit 或 boot 失败。

**Source audit：** production scheduler graph 中所有 `with_sched_entity_mut` 调用都显式消费 `SchedEntityMutToken::new()`，constructor 只出现在 `sched/class/{runqueue,rt}.rs`；`processor.rs` 只调用 `sched_on_runq()`。排除未编译且已注明 archived 的 `eevdf.rs` 后，production source 对 `new_realtime`、`new_normal()`、`SchedClassKind::RoundRobin`、legacy `rr` module/import、public RT type/re-export 均为零匹配。`SchedEntity` / `SchedClassPrv` / `RtEntity` 不实现 `Clone`；effective `priority`、`policy` 与 `remaining_ticks` 只在 `rt.rs` 的 `RtEntity` / `RtPolicy` 中保存。

**文档与静态检查：** `git diff --check` 与 `mdbook build docs` 通过。`just fmt kernel --check` 仍只报告 repository generator 输出 `kconfig_defs.rs` / `platform_defs.rs` 的既有 trailing-whitespace drift，输出不包含本 checkpoint 手写 Rust 文件。accepted noirq `VecDeque` allocation risk 已在 `rt.rs` 注释和 `ANE-20260713-SCHED-RT-NOIRQ-BUCKET-ALLOCATION` current limitation 中记录，并继续链接 broad IRQ-off allocation issue。

**状态：** expanded implementation 与 agent-run validation 完成；`APOLLYON-RT-001`、`KETER-RT-005`、`KETER-RT-006` 等待未参与写入的 reviewer 最终裁定，本条不提前 neutralize。

### 2026-07-13 - Checkpoint 1 Independent Review Closure

**独立裁定：** 未参与写入的 reviewer 对 expanded implementation、owner boundary、pending/placement、constructor/config 和 validation evidence 完成只读复审。kernel implementation 未发现新的 Apollyon、Keter 或 Euclid；`APOLLYON-RT-001`、`KETER-RT-005`、`KETER-RT-006` 均满足关闭条件，已移入 Tracking Issues 的 Neutralized。

**确认边界：** RT type/state/quantum/accessor/算法/KUnit 均留在 `rt.rs`；`entity.rs` 只拥有 `SchedEntity` facade、opaque storage、identity 与 mutation capability，`mod.rs` 只保留共享 contract 和 precedence。token 不可由 scheduler-class owner 外安全构造，`processor.rs` 只使用只读 membership observation。queue placement、`on_runq` / `ntasks` 更新顺序、entity lock 不跨 class dispatch、FIFO/RR selector 和 delayed Tick remainder 均符合 canonical contract。

**限制与 residual：** fixed bucket 的 noirq `VecDeque` allocation 已在实现与 current limitations 记录。archived、未进入 production graph 的 `eevdf.rs` 仍保留旧的无 token 调用；未来重启 EEVDF 时必须机械迁移，但不影响本 checkpoint 的 production build 或 owner boundary。

**Gate：** PASS。Checkpoint 1 原子切换、agent-run validation 与独立 review gate 全部关闭；Checkpoint 2 用户态 smoke 尚未启动。

**最终复验：** 收口文档更新后，RT/RR、RT/FIFO、恢复 RT/RR 的三次 `just build` 均通过。首次 QEMU 复跑在既有 `openat` KUnit 创建固定路径时因 pretest ext4 映像残留同名文件触发 `AlreadyExists`；该测试在 block-backed rootfs 上运行，正常结束才删除路径，因此这不是 RT test failure。使用 repository-owned `just rootfs mkfs -c conf/rootfs/pretest-rv64.toml --sudo` 只重建 `build/rootfs/pretest-rv64/` 后，再次运行同一 25 秒 QEMU gate，112 项 KUnit 全部通过并打印 `All tests passed!`，10 项 RT KUnit 全部为 `ok`，随后正常进入 init / user-test。外层 timeout 只终止超出本 checkpoint 的 read-write LTP。

## Open Items

- 无 RFC blocker。FIFO 用户态 no-timeslice、explicit-yield 与 block/wake 专项 smoke 未运行；这不是已知缺陷，也不在本 RFC 的关闭条件内。

## Closure

### 2026-07-13 - Checkpoint 2 用户验证与事务收口

**用户侧证据：** 用户确认在 RT/RR 调度类作为 compile-time default 的配置下完整运行了整套 LTP 测例。该结果作为真实用户态 workload、yield、block/wake 和长链路调度集成证据；这里只记录整套运行完成，不宣称每个 LTP case 都通过。

**证据裁定：** Checkpoint 1 已有 RT/RR 与 RT/FIFO selector build、focused KUnit、source audit 和独立 review，分别覆盖 priority、bucket、FIFO no-timeslice、RR quantum、placement 和 owner boundary。用户的整套 RT/RR LTP 运行补齐 runtime integration 证据，不作为上述算法 proof 的替代。

**FIFO 边界：** 用户裁定本轮不要求 FIFO 用户态专项验证。RT/FIFO build 与 focused KUnit 证据保留；no-timeslice、explicit-yield 和 block/wake 专项 smoke 明确记为 `Not Run`，不写成 PASS，也不作为第一版收口 blocker。未来若需要该运行证据，另开 follow-up，不重开已关闭的 Checkpoint 2。

**结论：** Checkpoint 2 关闭，事务状态更新为 Completed，RFC 第一版收口。已登记的 noirq `VecDeque` allocation limitation 保持有效；ABI policy syscall、动态 policy transaction、bandwidth control、不同 priority runtime ordering 和 procfs observation 仍在 RFC 非目标边界外。

**Agent-run 文档验证：** 本次仅修改收口文档；`git diff --check` 与 `mdbook build docs` 均通过。未重复运行 kernel build、QEMU 或 LTP，运行时证据使用上述用户侧整套 RT/RR LTP 结果。
