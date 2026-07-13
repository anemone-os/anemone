# Sched Fair / Stride 迁移实施计划

**状态：** Draft；公开 RFC 已提升，尚未开始实现
**最后更新：** 2026-07-13
**父 RFC：** [RFC-20260713-sched-fair-stride](./index.md)
**事务日志：** None；RFC 接受进入实现时创建。

## 实施原则

- 本文是公开 RFC 的 planned gates，不记录已执行事实。
- 本文所有阶段（包括阶段 0）都只在公开 RFC review 关闭、RFC 接受进入实现且 transaction devlog 已建立后执行；本次 promotion 已完成，但不算作本文内部 checkpoint。
- checkpoint、review、验证和更正事实只追加到 transaction。
- 每个 checkpoint 只处理一个主要风险，前一 checkpoint 未退出不得自动进入下一 checkpoint。
- write set 是协调合同。若真实 owner boundary 需要扩张，先报告原因、文件、contract 和验证影响，批准并回写后再继续。
- 不修改 scheduler core、wait-core、trap/IPI、pending-resched 或 schedule entry，除非前置 gate 证明现有 method-first surface 无法表达 Stride；此时停止并回到 RFC review。
- 不让 implementation feedback 把 fixed-tick Stride 演化为 runtime-accounted scheduler，也不引入 EEVDF/CFS 状态通过 gate。
- 代码 checkpoint 最终需要独立 review；有未关闭 Apollyon 或 Keter 时不得提交或推进。Euclid 必须有明确处置、owner、验证点和回写路径，但不自动阻塞 checkpoint。
- QEMU/LTP/runtime 责任必须区分 agent-run、user-run 和 Not Run；不得用 build 冒充运行验收。

## 阶段 0：Live Source 与 Fair Test 资产前置 Gate

### 目标

在 scheduler class 代码修改前确认本 RFC 所依赖的 scheduler class surface、Kconfig owner、constructor path 和 yield path 与 live tree 一致；同时把已有通用公平性 workload 从 `eevdf-test` 正式迁移为 `fair-test` 并装入公共测试 rootfs，为后续用户运行准备稳定资产。

本阶段内部先完成只读 source audit；若命中停止条件，不开始 rename 或 rootfs 修改。只有 live surface 与 canonical contract 一致后，才执行 `fair-test` 资产迁移。

### Write set

- `anemone-apps/eevdf-test/**` -> `anemone-apps/fair-test/**` 的单一 rename；同步 package、app、artifact 和输出前缀，不保留旧 wrapper 或第二份 binary
- `conf/rootfs/minimal.toml`
- `conf/rootfs/pretest-rv64.toml`
- `conf/rootfs/pretest-la64.toml`
- canonical RFC 与 transaction devlog 的阶段 0 write-back

用户运行期间对 live 配置和 `anemone-apps/user-test/src/main.rs` 的临时修改由用户拥有，不属于本阶段 production write set；agent 不得覆盖、提交或把这些修改记为 agent-run evidence。

### Source audit

- 审计 `sched/class/mod.rs` 的 `Scheduler` trait、centralized precedence、`TickAction`、`PreemptDecision` 和 `PendingResched`。
- 审计 `sched/class/{entity,runqueue,rt,idle}.rs` 的 payload owner、capability token、dispatch、`on_runq` / `ntasks` 更新顺序和 idle fallback；确认 `on_runq` 只在完整 `RunQueue` transaction 边界与 class-local physical membership 一致。
- 审计 `rt.rs` 中跨 class precedence 与全局 default constructor 测试，确认前者不应继续由 RT owner 保留，后者必须在 Checkpoint 2 随 selector owner 一起迁出 RT module。
- 审计 `sched/mod.rs`、`sched/processor.rs` 和 `sched/api/sched_yield.rs`，确认用户 `sched_yield()` 与内核 `yield_now()` 统一进入 `requeue_yielded_current()`，且 requeue 后执行 full pick。
- 审计 `local_pick_next()` 是 `pick_next_task()` / `set_next_task()` 的唯一 production scheduler-core caller，且两者在同一 owner-CPU IRQ-off full-pick transaction 中无 admission、无 callback 地成对执行。
- 审计 Tick pending 的 producer/consumer，确认 `task_tick()` 可以在 request 产生时完成 pass charge，requeue 不需要补记。
- 审计 `SchedEntity::new_default()` 的所有 production callers，确认 ordinary task、clone、bootstrap 和 kthread 不绕过 facade。
- 审计 `Task::nice()`、clone inheritance 和现有 weak setter，确认 tick 与 charged yield 每个 transaction 只读一次 typed nice，queued pass/key 不被追溯修改，且不需要新增调度属性 transaction。
- 审计 `scripts/xtask/src/config/kconfig.rs`、`conf/.defconfig`、live root `kconfig` 和 generated `kconfig_defs.rs`，确认 selector 是受约束 enum 而不是多个 boolean；generated `kconfig_defs.rs` / `platform_defs.rs` 只作为 build output，不属于手工 authored write set。
- 审计 archived `sched/class/eevdf.rs` 没有进入 production module graph。
- 审计 register 中 IRQ-off queue allocation 限制仍然有效；本 RFC只链接/保留，不宣称修复。
- 审计 `eevdf-test` 的四组 workload 与阈值在 rename 后保持不变，只删除 EEVDF-specific package、artifact 和输出身份；nice-direction 必须在 measurement barrier 前完成 set/get，测量区间保持 fixed-nice，其阈值不作为动态 renice、精确 share 或线性化 proof。
- 审计所有安装 `user-test` 的公共 rootfs manifest 都安装 `fair-test`；本阶段不增加永久自动执行 hook。

### 前置假设

- 现有 method-first `Scheduler` trait 足以表达 Stride 全部生命周期 transaction。
- `Task::with_sched_entity_mut(SchedEntityMutToken)` 足以让 Fair owner 访问 payload，同时阻止普通 crate caller 替换 published entity。
- same-class arrival 可以保持 current；下一 tick 只需产生已有 Tick resched request，不需要新 resched cause，也不声称 full pick 具备 one-tick 上界。
- yield guarantee 可以完全在 `requeue_yielded_current()` 内通过 pass/sequence 表达。
- heap queued key 在 owner-CPU transaction 间保持 immutable，不需要 scheduler core 支持。

### 停止条件

命中以下任一项时停止阶段并回到 RFC review：

- yield path 在 requeue 与 pick 之间存在会让 peer/order 失效的 unlock、remote mutation 或 no-pick 分支；
- Tick pass charge 必须依赖 scheduler-core pending slot 的持久计数；
- fresh/wake placement 必须读取 wait-core private identity；
- current identity 无法由现有 set-next/switch-out transaction 完整建立和清除；
- pick/set-next 出现独立 caller、可中断分支、中间 admission/callback，或需要 selected/in-flight state 才能维持 floor；
- default Fair 构造必须增加 published-task class setter；
- Kconfig owner 无法用单一 `fair | rt_rr | rt_fifo` selector 表达；
- 实现需要 runtime backend tag或混合 Fair payload。
- rename 被迫保留 `eevdf-test` wrapper、双 binary 或 EEVDF-specific runtime contract；
- 安装 `fair-test` 必须修改 rootfs owner 之外的公共测试入口。

### 验证

- `just app build --arch riscv64 fair-test`
- `just app build --arch loongarch64 fair-test`
- `just fmt fair-test --check`
- rootfs manifest source audit，确认三份公共 manifest 只新增同一个 `fair-test` app identity
- `git diff --check`
- `mdbook build docs`
- public docs、register 和 devlog 无 private path 链接

### 退出条件

- live surface 与草案假设一致；
- `fair-test` rename、双架构 build、公共 rootfs 安装与 source audit 关闭；
- accepted contract、write set、验证 floor、runtime owner 和停止条件闭合。

## Checkpoint 1：Fair Identity、Stride State 与 Heap Algorithm

### 前置条件

- 阶段 0 关闭。
- 公开 RFC 与 transaction devlog 继续是 canonical plan / execution record。
- production default 暂时保持当前 RT/RR；本 checkpoint 不切换 ordinary task。

### 交付

- 新增 `sched/class/fair/mod.rs` 和 `sched/class/fair/stride.rs`。
- `fair/mod.rs` 定义 Linux nice weight contract，并直接 alias `Stride as Fair`、`StrideEntity as FairEntity`。
- `StrideEntity` 只保存 `pass: Option<u128>`；`Stride` 只保存 heap、current、placement floor 和 sequence。
- `SchedClassPrv` 增加 opaque `Fair(FairEntity)`；`SchedClassKind` 增加稳定 `Fair`。
- `RunQueue` 增加 `fair: Fair` 和所有 exhaustive method-first dispatch。
- centralized precedence 扩展为 `Realtime > Fair > Idle`。
- 删除 `rt.rs` 中只断言“Realtime 是 Idle 上方唯一 class”的无意义 KUnit，不迁移等价的 exact-array 测试。`class/mod.rs` 继续是 precedence 唯一代码真相，实际跨 class pick/arrival 行为由本 checkpoint 的集成 KUnit 覆盖。
- 实现 fixed-tick pass arithmetic、heap order、floor、yield、preempt、handoff、block、exit、pick、set-next 和 same-Fair arrival semantics。
- production `new_default()` 和 Kconfig 仍使用 RT/RR；Fair task 只由 class-local KUnit 的 fresh helper 构造。
- 不创建 `SchedClassKind::Stride`、backend enum、runtime wrapper、heap index 或 Fair selector。

### 建议 write set

- 新增 `anemone-kernel/src/sched/class/fair/mod.rs`
- 新增 `anemone-kernel/src/sched/class/fair/stride.rs`
- `anemone-kernel/src/sched/class/mod.rs`
- `anemone-kernel/src/sched/class/entity.rs`
- `anemone-kernel/src/sched/class/runqueue.rs`
- `anemone-kernel/src/sched/class/rt.rs`，仅删除错误归属的全局 precedence KUnit；不修改 RT queue/policy/runtime semantics
- canonical RFC 与 transaction devlog 的本 checkpoint write-back

明确不修改：

- `anemone-kernel/src/sched/{mod,processor,switch,wait}.rs`
- `anemone-kernel/src/sched/class/{idle,eevdf}.rs`
- `anemone-kernel/src/task/**`
- architecture trap/bootstrap
- Kconfig owner、`.defconfig`、live root `kconfig`
- user ABI / priority syscall

若 KUnit fresh construction 不能在 class owner 内表达而需要扩大 constructor surface，必须停止并提交 write-set/owner 扩展；不能增加普通 crate caller 可用的 published entity replacement。

### Focused KUnit

- 40 项 weight 表长度、正值、nice direction 和 nice 0 weight。
- stride delta nice 0 精确值、两端边界、ceil rounding、单调方向、checked add。
- heap min order、same-pass sequence order和合法 queued snapshot/entity 一致性。
- fresh floor placement、wake stale-credit clamp、debt preserve、empty-set floor persistence。
- `0, 100` pick/set-next gap：pick 后 floor 不变，set-next 后仍为 `0`，随后 fresh/wake placement 不跳到 `100`。
- `current = 0, peer = 100` 的 preempt/handoff：完成 requeue 后 floor 仍为 `0`，不得在 clear-current 中间态推进。
- `pass: Option<u128>` 的合法初始化与 placed lifecycle：正向测试只允许 `enqueue_new()` 完成 `None -> Some(floor)`，后续 transaction 保持 `Some(pass)`。
- queued weak nice update 不修改 pass/snapshot；pick/set 后下一次 tick 与有 peer yield 分别从单次 nice observation 使用新 delta。
- equal-weight 多任务 round order。
- unequal-weight deterministic service count 和 eventual progress；测试使用足够小的 synthetic weight/scale helper避免长循环，同时生产 weight mapping 单独锁定。
- Tick 有 peer时 charge + resched、无 peer时 charge + continue。
- delayed full pick 前多个 Tick 各自收费且 requeue 不重复收费。
- Yield 有 peer时至少 peer first、无 peer时 pass 不变并允许 self-pick。
- higher-weight current yield 时，即使普通 delta 后仍小于 peer，也通过 floor-to-peer 阻止立即 self-pick。
- preempt/handoff/block/exit/current identity 和 membership lifecycle。
- same-Fair arrival keep-current，cross-class precedence 的三类组合。
- same-Fair arrival 验证 active current 为 `Some(pass)` / not queued，candidate 为 `Some(pass)` / queued。
- `RunQueue` facade 的 transaction-boundary membership：enqueue/requeue 返回后 `on_runq == true`，pick/dequeue 返回后 `on_runq == false`；不要求 class dispatch 中间态同步。

### Source audit

- production class graph 无 `SchedClassKind::Stride` / backend tag。
- `FairEntity` 只有一个 alias；production graph 无 EEVDF payload。
- `Task::nice()` 之外无长期 nice/weight truth。
- queued entity pass 只有 snapshot entry，queued lifetime无 pass mutation。
- `enqueue_new()` 是唯一 `None -> Some(pass)` 转换；production graph 无 `initialized` 双状态或 `Some -> None`。
- duplicate `enqueue_new()`、fresh `enqueue_woken()`、fresh `set_next_task()`、未初始化 arrival、snapshot mismatch 和错误 membership entry 均有常开断言；当前 KUnit 不支持 unwind，因此这些 expected-panic contract 只做 source audit，不进入普通 KUnit pass set，也不增加 destructive QEMU test。
- enqueue/requeue 先完成 class-local 入堆再由 `RunQueue` 发布 `on_runq = true`，pick/dequeue 先移除 entry 再清除 `on_runq`；完整 transaction 之外不存在第二份 membership truth。
- heap sequence/pass 使用 checked `u128`。
- `placement_floor` 只在 fresh/wake placement 被消费，不出现在 pick eligibility；所有刷新只读取完整 transaction 的最终 post-state，pick 不刷新。
- `local_pick_next()` 唯一且连续地成对调用 pick/set-next，不存在中间 admission/callback。
- `requeue_preempted_current()` 不因 Tick pending 再次收费。
- `yield_now()` 无 task-type 特例。

### 验证 floor

- `just build`
- focused KUnit compile/runtime；使用 repository pretest QEMU wrapper
- `git diff --check`
- `just fmt kernel --check`，将未触碰 generated drift 与本 checkpoint 文件分开报告
- `mdbook build docs`
- independent code review；无未关闭 Apollyon / Keter，Euclid 均有明确处置和回写路径

不运行 full LTP；Fair 尚未成为 production default。

### 停止条件

- heap key必须在 queued 状态变化；
- floor 无法只按完整 transaction 的最终 post-state刷新，或合法 post-state 出现 `min_visible < old_floor`；
- fresh/placed lifecycle 需要 `Option<u128>` 以外的第二份 initialized truth；
- floor 必须参与 eligibility 才能保证进展；
- yield guarantee 需要 scheduler-core skip flag或新 entry mode；
- Tick accounting 在 deferred preempt 下出现丢记/重复；
- Fair wiring改变 RT/FIFO、RT/RR 行为或 precedence truth出现复制；
- 需要触碰 wait-core、pending slot或调度属性 setter。

### 退出条件

- Fair/Stride algorithm、entity、heap 和 lifecycle 在非默认 graph 中完整可构造、可测试；
- focused proof/KUnit、build 和 review 通过；
- default selector 尚未变化。

## Checkpoint 2：Compile-time Fair Default Cutover

### 前置条件

- Checkpoint 1 关闭并有独立 review 结论。
- Fair class 的 KUnit/runtime pretest 无 correctness failure。

### 交付

- `SchedDefaultPolicy` 增加 `Fair`，serde 值为 `fair`。
- generated kernel selector 增加 `SchedDefaultPolicy::Fair`。
- `SchedEntity::new_default()` facade 根据 `Fair | RtRr | RtFifo` 选择 opaque payload factory。
- RT module 不再拥有 `RtEntity::new_default()` 或任何全局 default selector 解释；只拥有 policy-directed RT/RR、RT/FIFO fresh payload construction 和 validation。现有 default-constructor KUnit 迁到 `entity.rs`，由 facade owner 校验当前 build 的 selector/payload；三种 selector build 合起来覆盖所有分支。
- `conf/.defconfig` 与 live root `kconfig` 的 default 切换为 `fair`。
- ordinary task、clone、bootstrap、`kthreadd` 和普通 kthread 无需新增 class-specific caller；继续统一调用 `new_default()`。
- `rt_rr` / `rt_fifo` selector 仍能构建并保持当前 RT semantics。
- 不增加 `sched_fair_policy`；当前 source alias 仍唯一选择 Stride。

### 建议 write set

- `anemone-kernel/src/sched/class/entity.rs`
- `anemone-kernel/src/sched/class/rt.rs`
- 必要时 `anemone-kernel/src/sched/class/fair/mod.rs`
- `scripts/xtask/src/config/kconfig.rs`
- `conf/.defconfig`
- ignored live root `kconfig`，只改 selector且不得提交
- canonical RFC 与 transaction devlog write-back

build flow 会覆盖 ignored `anemone-kernel/src/{kconfig_defs,platform_defs}.rs`；它们是由 xtask 生成的验证副作用，不得手工编辑、提交或作为 authored write set 领取。

普通 constructor call sites 默认只读审计，不应需要修改。若发现 caller 绕过 `new_default()`，停止并报告真实 owner/write-set 扩展，不能在 caller 中硬编码 `FairEntity`。

### Kconfig contract

受约束 selector 必须分别接受：

```text
fair
rt_rr
rt_fifo
```

其它值解析失败。当前不得出现 `stride` 作为 default policy 值，也不得出现 `sched_fair_policy`。未来第二个 Fair backend 的 selector 是独立 RFC/阶段，不属于本 checkpoint。

### Source audit

- 所有 production fresh non-idle task只调用 `SchedEntity::new_default()`。
- idle 只调用 `new_idle()`。
- `fair` build 中 production graph 无 fresh RT payload；`rt_rr` / `rt_fifo` build 中 production graph 无 fresh Fair payload。
- selector 只决定 fresh construction，不成为 per-task runtime policy slot。
- `entity.rs` 是 global default selector 与 constructor KUnit 的 owner；`rt.rs` 不再匹配 `SchedDefaultPolicy`，也不保留“default 必为 Realtime”的测试。
- 没有 published entity replacement、class setter 或 backend migration API。
- clone 获得 fresh pass/floor placement，只继承 nice，不复制 parent entity。

### 验证 floor

- 从同一 live config 生成只改变 selector 的临时 `fair` / `rt_rr` / `rt_fifo` 配置，分别运行 `just xtask build -k <temporary-kconfig>`
- 使用只把 selector 改为非法值的临时配置运行 `just xtask build -k <temporary-invalid-kconfig>`，确认在 build 前解析失败
- 恢复 live/default `fair` 后再次 `just build`
- Fair default pretest QEMU：全部 focused KUnit 通过并正常进入 init/user-test
- `git diff --check`
- `just fmt kernel --check`，区分 generated drift
- `mdbook build docs`
- independent review；无未关闭 Apollyon / Keter，Euclid 均有明确处置和回写路径

### 停止条件

- shared `entity.rs` 被迫拥有 Stride arithmetic、heap或RT policy state；
- RT module继续解释 `Fair` selector或 Fair module解释 RT selector；
- selector 需要运行时 policy tag、published migration或双 payload；
- Fair default boot/KUnit 出现稳定 self-pick、membership、floor或pass anomaly；
- RT selector build/semantics 被破坏。

### 退出条件

- repository default 为 `fair`；
- 三种 compile-time policy 构建和 source audit 通过；
- Fair production boot/KUnit通过；
- RT regression gate 清洁。

## Checkpoint 3：Runtime Acceptance 与收口

### 前置条件

- Checkpoint 2 关闭。
- clean Fair default build 已保存；临时诊断 instrumentation 不在 production commit 中。

### Agent-run 验证

- focused KUnit 与 pretest boot 复跑。
- `just build`、`git diff --check`、`mdbook build docs`。
- source audit 复核 default、alias、precedence、constructor 和 archived EEVDF 边界。
- agent 不运行 `rt_rr` / `fair` whole-profile 对比，不把缺失的用户结果推定为通过。

如需临时探针，必须先在 canonical `implementation.md` 增加 bounded validation gate，说明 hypothesis、最小 write set、失败信号和删除条件。探针只在 validation branch 保存，不进入 production class。

### User-run 验证

- 用户按自己的 validation-only 配置与 `user-test` 接线运行 `fair-test`；四组 case 分别覆盖 equal-nice progress、fixed-nice direction、yield handoff 和 sleep/wake progress。nice-direction 不验收动态 renice、精确 share 或线性化语义。
- 用户在同一 checkout 下分别使用 `rt_rr` 与 `fair` selector 运行真实 user-test/LTP profile；两侧必须保持相同平台、CPU 数、`SYSTEM_HZ`、rootfs/test image、profile/case-set 和测量起止区间。
- `rt_rr` 是当前 A/B baseline。预期 Fair 与 RT/RR 保持同一量级，不出现重复、稳定的多倍 whole-profile 吞吐退化；具体运行次数和最终可接受性由用户结合结果裁定，不预设未经用户接受的百分比 SLA。
- 两侧记录 attempted/pass/fail/infra/skipped 或等价 case-set 摘要及测量区间；先做 failure-set diff，case/result multiset 不一致时不直接比较墙钟，不用单次墙钟波动判定回归。
- 至少包含 yield、signal、pipe/read-write、thread/process lifecycle 和 block/wake 密集路径。
- 若用户选择 full LTP，记录“完整运行完成”和结果摘要，不把所有 case 通过作为 Stride 算法 proof 的替代。
- 用户运行期间的 live 配置和 `user-test` 修改不进入 agent production write set；收口只记录用户提供的配置身份、case 摘要和是否已恢复临时接线，不把未提供结果写成通过。

### Runtime 停止条件

- 持续 runnable equal-nice task 出现稳定饥饿或数量级 share 偏离；
- yield 有 peer时仍稳定 self-pick；
- 用户 A/B 中 Fair 相比同 checkout RT/RR baseline 出现重复、稳定的多倍 whole-profile 吞吐退化，或用户据结果明确拒绝当前性能；
- pass/snapshot、duplicate membership、current identity 或 arithmetic assertion 触发；
- wake task携带陈旧低 pass 产生长时间 catch-up；
- deferred Tick 导致重复收费、漏收费或 resched storm；
- 为通过 workload需要降低 Linux weight、放宽 yield guarantee或引入特殊 service-kthread policy。

命中停止条件时：

- 停在当前 gate，不自动进入收口；
- execution facts 写 transaction devlog；
- 若改变 stage/write set/validation，回写 implementation；
- 若改变 fairness、placement、yield、identity 或接受边界，回到 index/invariants review并建立 tracking issue；
- 接受但延期的能力缺口进入 current limitations，本应正确却失败的事项进入 open issues。

### 收口动作

- RFC 状态更新为 Completed/Closed 的真实结果；
- transaction devlog 记录 agent/user validation、Not Run 项和 residual limitations；
- 用户提供的 RT/RR 与 Fair 配置身份、case/result 摘要、测量区间和接受结论写入 transaction；未提供的比较保持 `User-run Not Run/Pending`；
- 当前双周 devlog、RFC 导航和 transaction index同步；
- tracking issues 如存在，逐项保留关闭依据；
- IRQ-off allocation limitation 继续保留，除非独立 gate 已真实消除；
- 不把未来 dynamic scheduling attributes 或第二 Fair backend 写成当前完成项。

### 最终退出条件

- 文档、代码、Kconfig、default constructor、class graph 和 runtime evidence 一致；
- Fair/Stride 成为已验收 compile-time default；
- RT/FIFO 与 RT/RR selector仍可构建；
- 无未关闭 Apollyon 或 Keter；Euclid 均有明确处置、owner、验证点和回写路径；
- 所有临时探针已删除或单独归档为 evidence；
- 后续调度属性 RFC和可选 Fair backend RFC边界明确，但没有预埋未使用 runtime abstraction。
