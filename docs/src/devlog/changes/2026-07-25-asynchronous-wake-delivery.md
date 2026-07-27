# ANE-CHG-20260725-asynchronous-wake-delivery

**Type:** Bugfix / Workflow Improvement
**Status:** Completed
**Date:** 2026-07-25
**Authors:** doruche, Codex
**Area:** scheduler / wait core / IPI / Event / Latch / documentation workflow

## Problem

wait core 已经在 task sched-state 的唯一事务中完成 wait identity、mode、armed state、
completion reason 与 `Waiting -> Runnable` 的 logical completion，但 remote physical placement
仍使用 synchronous IPI 等 owner CPU 返回 `WakeEnqueueResult`。producer 不需要这个瞬时
placement 分类；若 producer 本身位于 IPI hardirq，双 CPU 反向 completion 会形成同步互等边。

dynamic scheduler request 曾用全局 `REMOTE_SCHED_REQUEST_GATE` 串行所有 remote setter，
只为避免两个 handler 同时完成对方的 oneshot wait。这个桥证明了组合风险，也限制了统一
wake capability；它不是 scheduler transaction lock，异步 delivery 完成后必须删除。

现有 effective 行为分散在 live source、Closed `sched-wait-refactor` / `sched-latch` RFC 和
历史 transaction，没有 current contract surface。变化本身只有一个原子 cutover，但旧
workflow 又把任何需要 current contract 的小迭代一律升级 RFC，工件体量超过这次已完整
解析、无过渡协议的局部修复。

## Scope

本轮保持 wait identity、logical completion 线性化点、park latch、stale-safe placement、
Event exclusive / mode-blocked requeue 和 Latch no-return producer 语义不变，只完成：

- remote placement 改为 strong `Arc<Task>` single-target async IPI obligation；
- `WakeResult::Woke` 收窄为 logical completion + scheduler 已接管 obligation；
- physical placement result 收入 scheduler owner-local 诊断；
- 删除 wake 专用 synchronous result transport 和 scheduler request 临时串行 gate；
- 提取 scheduler wake-delivery 最小 current contract 闭包；
- 允许严格满足单一原子 cutover 条件的 contract-bearing small change。

本轮不引入 workqueue / softirq、预分配 inbox、allocation-free transport、task migration、
CPU hotplug、Event 固定 hardirq latency 或可恢复的 post-commit transport error。通用
synchronous IPI 继续服务真实 barrier；`ANE-20260622-IRQ-OFF-HEAP-ALLOCATION` 保持开放。

## Solution

logical completion 成功后，wait core 只调用 scheduler 的 no-result placement handoff。
本地 task 立即做 stale-safe placement；remote task 的强 capability 先进入 owner CPU IPI
queue，再 ring IPI。handler 在 transport lock 外直接调用 owner-local placement，不做 TID
lookup，也不重新选择 remote route。allocation / offline submission failure发生在不可 rollback
的 post-commit 边界，以常开 fatal invariant 暴露，不进入 consumer result。

strong payload 的直接析构风险由 live lifecycle 闭合：user task 与 kthread 都在 topology
detach / unpublish 前先把强引用交给 deferred-disposal queue；disposer 只在
`Arc::strong_count() == 1` 时释放，否则重新排队。因此 payload outstanding 时 disposer 不会
取走 task，而 handler 释放 message 时 deferred owner 仍在。这个结论不泛化到既有
`Weak::upgrade()` 窗口或 IRQ-off allocation，它们仍由现有 register issue 跟踪。

文档采用 contract-bearing small change：本页保存 baseline、local target、cutover 与证据，
[scheduler wake-delivery current contract](../../contracts/scheduler/wake-delivery.md) 保存唯一
effective 正文。没有独立 invariants、implementation、tracking 文件或 transaction；代码、
workflow 和 contract 在本 commit 同时生效，任一 gate 失败则不提交、旧 contract 保持有效。

## Change

- `WakeResult::Woke` 移除 placement 字段；Event 只消耗 quota，Latch 只记录 logical outcome。
- scheduler owner-local 保存 `WakeEnqueueResult`，remote submission 使用
  `send_ipi_async(...).expect(...)`，handler 直接 stale-safe revalidate。
- wake IPI payload 从 `Tid` 改为 `Arc<Task>`，禁止 sync/async broadcast；移除
  `send_ipi_wait_result()` 和 `IpiMsg::wake_result`，保留其它 synchronous IPI。
- 删除 `REMOTE_SCHED_REQUEST_GATE`；remote request 的 async transport + parked oneshot
  terminal completion 不变；本轮以 source proof 和普通 boot 验证，不把未完成的 SMP stress
  写成 cutover 证据。
- 新增 `SCHED-WAKE-001..004` current contract 和 scheduler owner index。
- 同步 development log、RFC workflow、contract docs/templates、repository `AGENTS.md` 与
  RFC workflow skill 的 contract-bearing small-change 资格边界。

## Contract Impact / Cutover

| Contract ID | 变化 | Cutover 前 effective baseline | 新 effective 规则 | 生效证据 |
| --- | --- | --- | --- | --- |
| `SCHED-WAKE-001` | Refine | wait core 同步提交 logical completion，随后同步执行一次 placement 并返回诊断 | logical completion 不变；返回前 scheduler 已本地终结或接管 obligation | wait / processor source audit，历史 park/stale closure，当前 build / boot |
| `SCHED-WAKE-002` | Replace | remote wake 用 synchronous IPI 等 owner placement result | remote obligation enqueue-before-ring 后异步返回，不建立反向 completion 边 | IPI source audit，当前 build / 普通 boot |
| `SCHED-WAKE-003` | Replace | producer 的 strong task 引用跨同步等待保活，payload 只传 TID | payload 自持 strong task capability；single-target；post-commit failure fatal、无 rollback | payload/broadcast 与 exit/deferred lifecycle audit |
| `SCHED-WAKE-004` | Refine | owner CPU stale-safe placement 分类可由 wait consumer观察 | 分类只留 scheduler-local；`Woke` 只表达 logical success + obligation accepted | 全树 consumer/source audit，Event/Latch 边界 |

四项在本 commit 原子切换；current contract 的唯一正文是
[`SCHED-WAKE`](../../contracts/scheduler/wake-delivery.md)。实现或验证失败时不提交该 checkpoint，
不会留下 target 已写成 current truth 的中间态。

## Validation

- `./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/async-wake-pretest-rv64.log`
  通过。脚本使用 tracked `qemu-virt-rv64-release` preset、`smp=1` / `memory=1G`，重建
  pretest rootfs、构建 RV64 release kernel 并正常完成 QEMU 用户态流程。
- kernel KUnit 运行 255 项并打印 `All tests passed!`；`ioctl-test`、`fair-test`、
  `sched-attr-test` 与 `jobctl-test` 均打印各自完成标记。单 CPU 下
  `remote-submission-stress` 按设计 SKIP。
- 脚本现有 `signal wait` LTP profile 同时完成：120 attempted / 106 passed / 10 failed /
  0 infra failed / 4 skipped。它是周边回归记录，不是本轮 contract acceptance；失败集合未在
  本轮诊断或归因。
- `just fmt kernel --check`、`just fmt sched-attr-test --check`、`git diff --check` 与
  `mdbook build docs` 通过；mdBook 仅报告既有 large search-index warning。
- source audit 确认 `WakeEnqueueResult` 只存在于 scheduler-local `processor.rs`，生产代码无
  `send_ipi_wait_result`、`IpiMsg::wake_result`、consumer-visible placement 或
  `REMOTE_SCHED_REQUEST_GATE`；wake payload 持有 `Arc<Task>` 且被 sync / async broadcast
  preflight 拒绝。exit / kthread exit / deferred disposal audit 闭合 payload lifecycle。

初始 sandbox build 在 lwext4 C 编译阶段被 host seccomp 以 `SIGSYS` 拒绝；同一仓库 build
入口和完整端到端脚本在 host 环境通过。另有两次 validation-only `smp=2` / two-logical-CPU
探索启动停在 userspace 前的 TTY `duplicate_identity_is_rejected_until_abort` KUnit，均人工终止；
它们不属于草案 acceptance，也不作为本次 cutover 证据。LA64、硬件、双 CPU race matrix、
allocation / fixed-latency stress 与 final harness Not Run。

## Tracking Issues

### CHG-001 - asynchronous payload 的最终析构上下文

**Status:** Neutralized
**Severity:** Keter

**Issue:** strong payload 若在 handler 中成为完整 `Task` 的最终 owner，会把复杂析构带入 hardirq。

**Resolution:** user exit 与 kthread exit 都在 unpublish 前先 `defer_to_dispose()`；disposer 在
payload outstanding 时看到至少两个 strong owners 并重新排队。payload 先释放时 deferred
queue 仍是 strong owner。因此 wake payload 本身不会成为最终 owner。既有 Weak-upgrade / IRQ-off
reclamation 风险不在此结论内，继续由 register issue 跟踪。

### CHG-002 - contract-bearing small change 不能形成第二套 RFC

**Status:** Neutralized
**Severity:** Keter

**Issue:** small-change record 若保存 pending 多阶段 target、transitional contract 或独立 proof
plan，会与 RFC 形成并列 proposal authority。

**Resolution:** canonical workflow、模板、repository rule 与 skill 同步限制为一个已完整解析的
原子 cutover；effective 正文只在 current contract。任何多阶段、probe、renegotiation、未决 owner
或本轮无法关闭的高等级 finding 都强制升级 RFC。

### CHG-003 - synchronous IPI 仍有真实使用者

**Status:** Neutralized
**Severity:** Euclid

**Issue:** wake 不需要同步 placement result，不能被扩张成内核不需要 synchronous IPI。

**Resolution:** 只删除 wake 专用 result surface；TLB shootdown、新 task publication和其它现有
barrier 保持同步语义。

### CHG-004 - async IPI 不等于严格 allocation-free IRQ-safe

**Status:** Deferred
**Severity:** Euclid

**Issue:** 当前 IPI message / queue node、scheduler queue 和邻接 cleanup 仍可能在 IRQ-off 路径分配。

**Resolution:** 本轮只消除 remote wake 的同步跨 CPU 互等并统一 consumer contract；不声明固定
hardirq latency 或 allocation-free，保留 register issue 的原 exit condition。

## Risk / Follow-up

- `Arc::strong_count() == 1` 是瞬时观察；其它 `Weak<Task>` 在 scan 后 upgrade 的既有风险未由本轮
  修复，也不得用本次 payload proof 关闭 IRQ-off reclamation issue。
- pre/post-park 与 stale-tail 继续由未改变的 wait-core identity、park latch、owner revalidation
  和历史 closure evidence 约束；本轮普通启动不构成双 CPU race matrix。
- LA64、硬件、专项 LTP 和 allocation/latency stress 不属于本轮 acceptance，不能从 RV64 证据外推。

## Links

- Biweekly devlog: [2026-07-20 至 2026-08-02](../2026-07-20_to_2026-08-02.md)
- Current contract: [Asynchronous wake delivery](../../contracts/scheduler/wake-delivery.md)
- Register / limitations: [IRQ-off heap allocation](../../register/open-issues.md#ane-20260622-irq-off-heap-allocation)
- Historical RFC: [Sched Wait Refactor](../../rfcs/sched-wait-refactor/index.md), [Sched Latch](../../rfcs/sched-latch/index.md), [Sched Dynamic Attributes](../../rfcs/sched-dynamic-attributes/index.md)
- External source evidence: None
- Issue / PR / commit: this change's Git commit
