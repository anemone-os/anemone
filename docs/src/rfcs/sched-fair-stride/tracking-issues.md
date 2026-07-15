# Sched Fair / Stride Tracking Issues

**状态：** Closed
**最后更新：** 2026-07-14
**父 RFC：** [RFC-20260713-sched-fair-stride](./index.md)
**事务日志：** [2026-07-13-sched-fair-stride](../../devlog/transactions/2026-07-13-sched-fair-stride.md)

本文只跟踪 confirmed design issue。accepted contract 必须修回 [RFC 入口](./index.md)、
[不变量需求](./invariants.md) 或 [迁移实施计划](./implementation.md)；本文不替代 canonical
正文，也不记录实现进度或运行事实。

## Apollyon

- 暂无。

## Keter

- 暂无 open Keter。

## Euclid

- 暂无 open Euclid。

## Safe

- 暂无；本轮在剩余观察只属于 Safe 时停止 issue hunting。

## Neutralized

### KETER-STRIDE-001：transaction 中间态不能推进 placement floor

**状态：** Neutralized / Canonical contract repaired / 2026-07-13

**原问题：** `pick_next_task()` pop selected task 后若只按 remaining heap 刷新，`0, 100` 场景会
把 floor 从 `0` 错推到 `100`。独立复核进一步确认同一根因也存在于复合 requeue：若
`current = 0, peer = 100` 时在 clear-current 后、入堆前刷新，preempt/handoff 会制造
`queued pass = 0 < floor = 100`。

**关闭依据：** [RFC 入口](./index.md) 与 [不变量需求](./invariants.md) 现已统一规定 floor 只按完整
lifecycle transaction 的最终 post-state 刷新；中间 pop/clear 不刷新，pick 保持 floor，set-next
建立 current 后刷新，requeue 在入堆后刷新。刷新使用 `min_visible >= old_floor` 常开断言，不用
`max()` 隐藏违例，也不增加 selected/in-flight state。live `local_pick_next()` 的唯一 caller 与
owner-CPU IRQ-off pair 被写入 source-audit/stop contract。

**验证 gate：** [迁移实施计划](./implementation.md) 已加入 `0, 100` pick/set-next、fresh/wake
后续放置，以及 preempt/handoff 中间态反例的 focused KUnit；若 pick/set 出现独立 caller、中间
admission/callback 或需要新 state，Checkpoint 1 必须停止。

### KETER-STRIDE-002：RT 测试与 global selector owner 越界

**状态：** Neutralized / Canonical contract repaired / 2026-07-13

**原问题：** Checkpoint 1 要把 centralized precedence 扩展为 `Realtime > Fair > Idle`，却把
`rt.rs` 排除在 write set；live RT KUnit 仍精确断言 Realtime 是 Idle 上方唯一 class，既会在 Fair
接线后失败，也把 global class graph 错放在 RT owner。相邻的 default-constructor KUnit 同样把
global compile-time selector 解释留在 RT module。

**关闭依据：** implementation 已把 `rt.rs` 纳入 Checkpoint 1 的 test-only write set，直接删除该
无意义 precedence KUnit 且不迁移等价 exact-array 测试；`class/mod.rs` 继续持有 centralized code truth，
实际跨 class pick/arrival 行为由 Fair/RunQueue 集成 KUnit 验证。独立实现 review 进一步确认集成
KUnit 不能用 `SchedEntity::new_default()` 偶然构造当前 RT default，否则 Checkpoint 2 切换到 `fair`
后会重新耦合 global selector；因此 RT owner 提供仅在 `cfg(kunit)` 下可见的 explicit fresh RT entity
factory，测试固定构造 RT payload，不进入 production constructor surface；
Checkpoint 2 明确删除 `RtEntity::new_default()` 和 RT-local default assumption，把 selector dispatch
及对应 constructor KUnit 移到 `entity.rs`。不借机修改 RT queue、policy 或 runtime semantics。

**验证 gate：** source audit 确认 `rt.rs` 不再复制 global precedence 或匹配
`SchedDefaultPolicy`；跨 class KUnit 的 RT helper 不调用 `SchedEntity::new_default()`；三种 selector build
与 owner-local KUnit 共同验证 facade。

### KETER-STRIDE-003：`on_runq` 只能在完整 RunQueue transaction 边界一致

**状态：** Neutralized / Canonical contract repaired / 2026-07-13

**原问题：** 原不变量把 `on_runq` 描述成 class dispatch 每个中间步骤都与 heap physical
membership 同步，但 live `RunQueue` 实际先调用 class enqueue/requeue 再置位，先调用 class
pick/dequeue 再清零。若 Fair 在中间态强制两者相等，会对合法 transaction panic。

**关闭依据：** index/invariants 现将 `on_runq` 定义为 `RunQueue` 唯一发布的 generic membership：
完整 transaction 入口/出口必须与 Fair heap 一致，中间 class-local mutation 按既有发布顺序执行，
且始终处于同一 owner-CPU IRQ-off transaction 内。不得增加第二个 queued/member flag。

**验证 gate：** positive KUnit 覆盖 enqueue/requeue/pick/dequeue 返回边界；source audit 确认
class dispatch 与 generic flag 发布之间没有 admission、callback、unlock 或 remote observation。

### KETER-STRIDE-004：普通 KUnit 不能把 expected panic 当作通过项

**状态：** Neutralized / Canonical contract repaired / 2026-07-13

**原问题：** 原计划要求 focused KUnit 执行 duplicate new、fresh wake/set-next、未初始化 arrival
和 snapshot mismatch 等预期断言，但当前 KUnit 不支持 unwind；任一 panic 都会终止 kernel，无法
成为普通 `All tests passed!` gate 中的 negative pass case。

**关闭依据：** focused KUnit 只运行合法 lifecycle、边界值与可返回结果的纯 arithmetic/helper；
expected-panic contract 改由 source audit 确认生产路径存在常开断言。按用户裁定，不增加 destructive
QEMU expected-crash test。

**验证 gate：** KUnit clean run、常开断言 source audit 和 review 共同关闭；不得用移除断言换取
KUnit 通过。

### KETER-STRIDE-005：RT/RR baseline 与 runtime 裁定责任必须显式

**状态：** Neutralized / Canonical contract repaired / 2026-07-13

**原问题：** 原 runtime stop condition 只写“相对 accepted RT/RR baseline 显著退化”，没有规定
baseline 是否来自同 checkout，也没有区分 agent/user owner，无法判断缺失对比结果时能否收口。

**关闭依据：** runtime acceptance 现规定用户在同 checkout、相同平台/CPU/config/rootfs/test
image/profile/case-set/测量区间下运行 `rt_rr` 与 `fair` A/B；RT/RR 是 baseline，预期 Fair 保持同一
量级且无重复、稳定的多倍退化。agent 不运行 whole-profile 对比，不预设未经用户接受的百分比 SLA，
最终可接受性由用户结合重复结果裁定。

**验证 gate：** transaction 必须记录用户提供的两侧配置身份、case/result 摘要、测量区间和接受
结论；未提供时保持 `User-run Not Run/Pending`，不得写成通过。

### EUCLID-STRIDE-001：weak nice 与 fixed-nice 验证边界

**状态：** Neutralized / Canonical contract repaired / 2026-07-13

**原问题：** 非目标把运行期 nice 修改整体排除，但 live published-task weak setter 与
`fair-test` nice-direction 都会在 task 发布后调用 `setpriority()`，容易被误读为忽略 renice 或已经
提供强 renice transaction。

**关闭依据：** canonical 文本现只排除 owner-CPU linearized renice、queued remove/reinsert、
current segment split、历史 pass 重算与动态公平证明。tick 和 charged yield 每个 transaction 只读
一次 `Task::nice()`；queued pass/key 不追溯修改。`fair-test` nice-direction 明确只是在 barrier 前
设置 nice、测量区间保持 fixed-nice 的 direction/integration smoke，不证明动态 renice、精确 share
或线性化语义。

**验证 gate：** implementation 已要求测试 queued weak update 不改 pass/snapshot，并验证后续 tick
与 charged yield 各自从单次 nice observation 计算 delta。

### EUCLID-STRIDE-002：fresh/placed 使用单一 Option 状态

**状态：** Neutralized / Canonical contract repaired / 2026-07-13

**原问题：** `pass + initialized` 可表达互相矛盾的组合，且 duplicate `enqueue_new()`、fresh wake、
current transaction 与 arrival decision 的非法输入没有完整失败合同。

**关闭依据：** `StrideEntity` 已收窄为 `pass: Option<u128>`。`new_fresh()` 只产生 `None`，
`enqueue_new()` 是唯一 `None -> Some(floor)` 转换，不存在 `Some -> None`；wake、queued/current
lifecycle 和 same-Fair arrival decision 都必须常开断言 `Some(pass)`。arrival 还必须分别验证 active
current/not-queued 与 queued candidate，不增加共享 state 或 scheduler-core API。

**验证 gate：** implementation 已加入 valid lifecycle 保持 placed coordinate 的 positive KUnit；
duplicate new、fresh wake/set-next 与未初始化 arrival 的常开断言由 source audit 关闭，不作为普通
KUnit expected-panic case。

### EUCLID-STRIDE-003：Euclid 不应自动阻塞 checkpoint

**状态：** Neutralized / Canonical contract repaired / 2026-07-13

**原问题：** 原 implementation 把任何未关闭 Euclid 都列为不得推进条件，与仓库 review 等级中
“Euclid 通常值得修、但不自动阻塞主线”的定义冲突，可能诱发无关清理和 scope creep。

**关闭依据：** 所有 review gate 现只要求无未关闭 Apollyon/Keter；Euclid 必须有明确处置、owner、
验证点和回写路径，但可按记录带入下一 checkpoint。

### EUCLID-STRIDE-004：selector build gate 必须走现有 xtask 入口

**状态：** Neutralized / Canonical contract repaired / 2026-07-13

**原问题：** 原验证只写非命令化的“`just build` with selector”和当前没有仓库入口的“xtask config
unit tests”，同时把 generated `kconfig_defs.rs` 混入 authored write set。

**关闭依据：** Checkpoint 2 现使用只改变 selector 的临时 config，分别执行
`just xtask build -k <temporary-kconfig>`，并用 invalid config 验证 parse failure；最后恢复 live/default
`fair` 后运行 `just build`。`kconfig_defs.rs` / `platform_defs.rs` 明确是 ignored build output，不手工
编辑或提交。
