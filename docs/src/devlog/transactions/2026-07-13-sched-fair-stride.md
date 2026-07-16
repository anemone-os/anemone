# 2026-07-13 - Sched Fair / Stride

**Status:** Completed
**Owners:** doruche, Codex
**Area:** scheduler / fair class / stride / nice / runqueue
**Canonical Plan:** [RFC-20260713-sched-fair-stride](../../rfcs/sched-fair-stride/index.md), [不变量需求](../../rfcs/sched-fair-stride/invariants.md), [迁移实施计划](../../rfcs/sched-fair-stride/implementation.md)
**Current Phase:** Completed；Checkpoint 3 runtime acceptance 已关闭

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

### 2026-07-13 - Checkpoint 1 只读 Preflight 与 Worker 边界

**Phase:** Checkpoint 1 - Fair Identity、Stride State 与 Heap Algorithm。
**Change:** 尚无 scheduler class 代码变化；总控与独立只读 reviewer 已按 canonical Checkpoint 1 合同复核 live class/core transaction surface，并将实现 worker 限定在 `fair/{mod,stride}.rs`、`class/{mod,entity,runqueue}.rs` 以及 `rt.rs` 的单个错误归属 precedence KUnit 删除。worker 不得修改 scheduler core、task、idle/EEVDF、Kconfig、architecture 或 ABI；若真实 owner boundary 需要扩张，必须停止并上报文件、理由、contract 与验证影响。
**Audit:** `local_pick_next()` 仍是唯一 production pick/set-next caller，二者在同一 owner-CPU IRQ-off closure 内连续执行；Tick 先调用 class `task_tick()`，再写入合并 pending latch；yield requeue 后必经 full pick；`RunQueue` 的 class-local physical mutation与 `on_runq` 发布顺序符合完整 transaction 边界合同。两轮审计均结论为 NO STOP，未发现 Checkpoint 1 write-set 扩张需求。
**Observability:** focused KUnit 必须分别覆盖 weight/arithmetic、heap/tie-break、floor 反例、tick/yield、current/lifecycle、fixed-set progress、transaction-boundary membership 与实际跨 class pick/arrival；当前 KUnit 不支持 unwind，非法输入与 snapshot mismatch 的常开断言继续由 source audit 关闭。
**Feedback:** None；原目标、不变量、阶段顺序、write set、验证 floor 与停止条件均保持不变。accepted IRQ-off `BinaryHeap` allocation 风险继续由 register 跟踪，本 checkpoint 不宣称修复。
**Validation:** 本条只记录实现前 preflight；build、focused KUnit runtime、format、docs 与独立 code review 尚未运行。
**Next:** 在上述 write set 内实现 Checkpoint 1；任何 canonical 停止条件出现时立即停止，不用兼容层绕过。

### 2026-07-13 - Checkpoint 1 Review Feedback：Cross-class KUnit RT Factory

**Phase:** Checkpoint 1 - independent review correction。
**Change:** 独立 review 发现 `runqueue.rs` 的跨 class KUnit 通过 `SchedEntity::new_default()` 构造所谓 RT task；该 helper 只在当前 `rt_rr` default 下成立，Checkpoint 2 切换到 `fair` 后会构造 Fair payload并使测试失败。批准在既有 `rt.rs` 文件 write set 内扩大 test-only action scope：由 RT owner 增加 `cfg(kunit)` explicit fresh RT entity factory，`runqueue` 测试改用该 factory；production constructor、selector、RT queue/policy/runtime 均不变。
**Audit:** 该问题定级为 Keter，因为它把 global default selector重新耦合进 RT identity 测试，并会在后续 checkpoint 产生必然失败。它不是 canonical production 停止条件，不需要扩大文件集合或改变目标/不变量；canonical `implementation.md` 已先回写修正后的 worker action scope 与 source-audit gate。
**Observability:** cross-class pick/arrival KUnit 将显式验证 RT payload，不再借当前默认配置偶然通过。
**Feedback:** Checkpoint 1 的 `rt.rs` action scope 与 source audit 已更新；原阶段顺序、production write set、验证 floor、Fair/Stride 合同和停止条件保持不变。
**Validation:** 修正尚未实现；完成后必须重跑 build、focused KUnit 与独立 review。
**Next:** 只允许 worker 修改 `rt.rs` 的 test-only factory 与 `runqueue.rs` 的测试 helper，然后返回 review gate。

### 2026-07-13 - Checkpoint 1 实现、验证与 Review Gate 收口

**Phase:** Checkpoint 1 - Fair Identity、Stride State 与 Heap Algorithm。
**Change:** 新增稳定 `fair` module 与当前唯一 `Fair = Stride` / `FairEntity = StrideEntity` alias；接入 opaque Fair payload、`Realtime > Fair > Idle` 集中 precedence 和 `RunQueue` 全生命周期 dispatch。Stride class 以 `Option<u128>` pass、反序 `(pass_snapshot, enqueue_seq)` `BinaryHeap`、Weak current、placement floor 和 checked sequence 实现 fresh/wake、tick、yield、preempt、handoff、block、exit、pick/set-next 与 same-Fair arrival。production `new_default()`、Kconfig 和 RT runtime 均保持 RT/RR；Fair 只由 class-local KUnit fresh helper 构造。
**Audit:** source audit 确认 production graph 无 `SchedClassKind::Stride`、runtime backend tag、EEVDF payload、cached nice/weight、`initialized` 双状态或第二份 membership truth；`enqueue_new()` 是唯一 `None -> Some(pass)` 转换，queued pass只保留不可变排序 snapshot。floor 只在完整 lifecycle post-state 刷新，pick 不刷新；Tick transaction 当场且仅收费一次，preempt requeue不补记；yield 有 peer 时按单次 nice observation charge并至少抬到最小 peer pass。独立 review 初次发现 cross-class KUnit 依赖 global default selector 的 Keter；按上一条 canonical correction 改为 RT-owner test-only explicit factory 后复核 PASS，无未关闭 Apollyon、Keter 或 Euclid，也未命中 canonical STOP。
**Observability:** 新增 13 项 Stride focused KUnit 与 3 项 RunQueue 集成 KUnit，覆盖 Linux weight / arithmetic、heap/tie-break、fresh/wake/floor、`0, 100` pick/set gap、preempt/handoff最终态、placed lifecycle、weak nice observation、equal/unequal progress、delayed Tick、yield handoff、current lifecycle、transaction-boundary membership和实际 cross-class pick/arrival。`BinaryHeap::push()` 处明确保留 IRQ-off allocation limitation 与独立移除 gate。
**Feedback:** 仅扩大既有 `rt.rs` 的 test-only action scope并同步 `implementation.md` / `tracking-issues.md`；文件 write set、production owner boundary、目标、不变量、验证 floor 与停止条件未改变。
**Validation:** `just build` 通过。`./scripts/run-user-test-rv64.sh etc/sdcard-rv.img build/stride-ckpt1-pretest-rerun.log` 重新生成 clean pretest rootfs、构建当前 rv64 kernel并运行 127 项 KUnit；16 项新增 scheduler 测试逐项 `ok`，最终打印 `All tests passed!`。KUnit 后已进入 init/user-test，总控通过 QEMU monitor 退出；随后只发生的 partial LTP 输出不作为本 checkpoint evidence，full LTP / Fair runtime 明确 Not Run。一次直接复用已被前序 KUnit 修改的 rootfs 因既有 openat fixture `AlreadyExists` 提前 panic，该次运行判为脏 guest 无效证据，不是 Stride failure。`git diff --check`、两个新增 Rust 文件的 `git diff --no-index --check` 与 `mdbook build docs` 通过；`just fmt kernel --check` 只报告 build flow 重新生成的 ignored `kconfig_defs.rs` / `platform_defs.rs` whitespace drift，未报告本 checkpoint 触碰文件，生成文件未手工修改或提交。
**Next:** Checkpoint 1 退出条件关闭；Checkpoint 2 才能修改 global default selector、constructor facade、Kconfig default并运行三 selector build / Fair default pretest gate。

### 2026-07-13 - Checkpoint 2 启动与 Owner Audit

**Phase:** Checkpoint 2 - Compile-time Fair Default Cutover。
**Change:** 尚无代码或配置变化；总控复核 global selector、generated kernel enum、default constructor facade、Fair/RT fresh payload owner、tracked default、ignored live config 和全部 production constructor caller，锁定 Checkpoint 2 write set。实现由总控直接完成，只另起一个独立 reviewer；不再分派 implementation worker。
**Audit:** xtask `SchedDefaultPolicy` 仍只接受 `rt_rr | rt_fifo` 并生成同名 kernel enum；`SchedEntity::new_default()` 仍把 global selector解释委托给 `RtEntity::new_default()`。Fair owner 已有唯一 `FairEntity` alias和 class-local fresh constructor，RT owner已有显式 FIFO/RR fresh state。ordinary task、clone、bootstrap、`kthreadd` 与普通 kthread 仍统一调用 `new_default()`，idle 只调用 `new_idle()`；clone 只在发布前继承 typed nice，不复制 parent entity。现有 owner surface 足以完成三分支 facade，无需 runtime policy tag、published migration、双 payload或 write-set 扩张，未命中 Checkpoint 2 停止条件。
**Observability:** selector branch 由 `entity.rs` owner-local KUnit 与 `fair` / `rt_rr` / `rt_fifo` 三次 repository build共同覆盖；cross-class KUnit继续使用 Checkpoint 1 建立的 selector-independent explicit RT factory。
**Feedback:** None；原目标、不变量、stage order、write set、验证 floor 与停止条件保持不变。
**Validation:** 本条只记录实现前 owner audit；三 selector build、invalid selector parse、Fair default build/pretest、format、docs 与独立 review尚未运行。
**Next:** 只在 canonical Checkpoint 2 write set 内实现 compile-time Fair default cutover；若 shared facade被迫拥有 class-private算法/state，立即停止并回到 RFC review。

### 2026-07-13 - Checkpoint 2 实现、验证与 Review Gate 收口

**Phase:** Checkpoint 2 - Compile-time Fair Default Cutover。
**Change:** xtask 受约束 selector与 generated kernel enum增加稳定 `Fair` 分支，repository tracked default和 ignored live selector切换为 `fair`。`SchedEntity::new_default()` 成为 global selector唯一解释点，只在 `Fair | RtRr | RtFifo` 间选择 owner-local opaque fresh payload factory；Fair owner构造 fresh Stride payload，RT owner只保留显式 RR/FIFO payload construction，不再解释 global default。default-constructor KUnit从 RT module迁到 entity facade owner；ordinary task、clone、bootstrap与 kthread caller保持不变。
**Audit:** source audit确认只有 `entity.rs` 匹配 `SCHED_DEFAULT_POLICY`，RT/Fair owner均不解释对方 selector；production fresh non-idle caller继续统一使用 `new_default()`，idle只使用 `new_idle()`。clone在发布前继承 typed nice但不复制 parent entity/pass。production graph没有 `sched_fair_policy`、runtime policy tag、published migration API、双 payload或 `stride` selector值，archived EEVDF仍不进入 production module graph；未命中 canonical停止条件。
**Observability:** entity-owner KUnit按当前 build selector验证 class identity，并由 RT owner test-only assertions检查 RR/FIFO fresh policy；三种 selector repository build共同覆盖全部 constructor branch。Fair default clean pretest运行 127 项 KUnit，其中 default-constructor与全部 16 项 Fair/RunQueue focused gate通过并打印 `All tests passed!`，随后正常进入 init/user-test。
**Feedback:** 用户直接审查本 checkpoint diff，结论为改动合理、可以接受，并明确无需 subagent复审；总控据此取消尚未产出结论的 reviewer任务。review gate无未关闭 Apollyon、Keter或 Euclid；原目标、不变量、stage order、write set、验证 floor与停止条件不变。
**Validation:** live Fair `just build`通过；从同一配置仅改变 selector的临时 `fair`、`rt_rr`、`rt_fifo` 配置分别通过 `just xtask build -k <temporary-kconfig>`。非法 selector在 prebuild前按 serde enum contract拒绝，错误列出 `fair, rt_rr, rt_fifo`。恢复 live Fair后最终 `just build`通过，generated selector确认为 `SchedDefaultPolicy::Fair`。`./scripts/run-user-test-rv64.sh etc/sdcard-rv.img build/stride-ckpt2-fair-pretest.log`通过上述 pretest gate；QEMU由 monitor主动退出，随后 partial LTP不作为本 checkpoint evidence，full LTP与用户侧 Fair/RT-RR A/B仍为 Not Run。`git diff --check`与 `mdbook build docs`通过；`just fmt kernel --check`只报告 build flow生成的 ignored `kconfig_defs.rs` / `platform_defs.rs` whitespace drift，未报告 authored文件，生成文件未手工修改或提交。
**Next:** Checkpoint 2退出条件关闭；Checkpoint 3只执行 canonical runtime acceptance与最终收口，用户侧 `fair-test` 和同 checkout RT/RR A/B结果不得提前推定为通过。

## Open Items

- 无 RFC blocker。
- 动态调度属性、第二个 Fair backend、actual-runtime accounting 与 noirq allocation-free ready queue 仍在本 RFC 非目标或既有 register 边界外；如需继续，另开 follow-up。

## Closure

### 2026-07-14 - Checkpoint 3 用户验证与 RFC 收口

**用户侧正确性证据：** Fair default 的 `build/user-test-rv64.log` 完整运行当前 `all` profile。启动时 127 项 KUnit 全部通过并打印 `All tests passed!`，default-constructor、13 项 Stride focused KUnit 与 3 项 RunQueue 集成 KUnit均为 `ok`。随后 `fair-test` 的 equal-progress、nice-direction、bounded-yield 与 sleep-wake-progress 四组 workload全部通过。

**用户侧 LTP 证据：** 当前 validation case-set 的 glibc 侧汇总为 attempted 832、passed 475、failed 253、infra_failed 0、skipped 104；musl 侧汇总为 attempted 831、passed 457、failed 263、infra_failed 2、skipped 110。整体汇总为 attempted 1663、passed 932、failed 516、infra_failed 2、skipped 214，并正常打印 `all competition tests finished` 与关机结束标记。这里的“完整运行”表示 `all` profile覆盖的全部 registered group 已跑到末尾，不表示每个 LTP case 都通过，也不把 validation-only case exclusions写成 production contract。

**性能裁定：** 用户报告 Fair 相比 RT/RR baseline 整体约慢 13–14%，并明确接受该结果、要求收口。该差距仍属于同一量级，没有命中“重复、稳定的多倍 whole-profile 退化”停止条件。事务只记录用户提供的比例与接受结论；RT/RR baseline 的绝对耗时、独立日志路径和更细 measurement interval未提供给 agent，因此不补造精确数字或宣称 agent复现了 A/B。

**证据裁定：** 阶段 0 与 Checkpoint 1-2 已用双架构 `fair-test` build、三 selector build、非法 selector拒绝、focused KUnit、Fair default pretest、source audit和 review关闭算法、owner与配置合同；本次用户运行补齐真实 Fair default、yield、block/wake、process/thread lifecycle及长链路 LTP集成证据，不替代上述理论/KUnit proof。未发现 starvation、稳定 yield self-pick、pass/snapshot/current assertion、wake catch-up或 resched storm停止信号。

**验证接线边界：** `fair-test` 临时调用和少量 LTP case exclusions仍是用户工作树中的 validation-only改动，agent未修改、恢复或提交这些文件；production kernel无临时 instrumentation或 probe。当前 IRQ-off `BinaryHeap` allocation风险继续由 `ANE-20260622-IRQ-OFF-HEAP-ALLOCATION` 跟踪，本 RFC不宣称修复。

**结论：** Checkpoint 3关闭，transaction更新为 Completed，RFC第一版收口。Fair/Stride作为已验收 compile-time default；RT/RR与RT/FIFO selector build证据继续有效。动态调度属性、第二 Fair backend、actual-runtime accounting、跨 CPU fairness和 allocation-free ready queue均留给独立 follow-up。

**Agent-run 收口验证：** Fair live config的最终 `just build`、`git diff --check`、`mdbook build docs` 与 default/alias/precedence/constructor source audit均通过。agent不重复运行 QEMU或LTP；运行时证据使用上述用户侧完整运行结果以及 Checkpoint 1-2 已记录的 pretest证据。build flow只更新 ignored generated definitions，未手工修改或提交它们。
