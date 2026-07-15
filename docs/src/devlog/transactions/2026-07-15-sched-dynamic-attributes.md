# 2026-07-15 - Sched Dynamic Attributes

**Status:** Active
**Owners:** doruche, Codex
**Area:** scheduler / dynamic attributes / syscall ABI / IPI / affinity
**Canonical Plan:** [RFC-20260714-sched-dynamic-attributes](../../rfcs/sched-dynamic-attributes/index.md), [不变量需求](../../rfcs/sched-dynamic-attributes/invariants.md), [迁移实施计划](../../rfcs/sched-dynamic-attributes/implementation.md)
**Canonical Revision:** R0
**Current Phase:** 阶段 0 已完成；阶段 1 尚未开始

## Scope

本事务按 R0 的阶段顺序实现第一版 dynamic scheduler attributes：先建立 dormant value-carrying one-shot，再完成 typed config、owner-CPU reconfigure、existing priority 原子切换、affinity remote vertical slice、legacy scheduler ABI、`sched_attr` ABI 与最终旁路审计。

阶段之间严格执行 canonical write set、停止条件、验证 floor 与独立 review gate。worker 不得未经批准越界修改；真实 owner boundary 需要扩张时，先在本事务和 implementation plan 记录批准结果。本事务不修复 wait-core synchronous remote placement，也不把 IRQ-off allocation 风险误写为已关闭。

## Invariants

- published task 的 policy、parameters、nice、reset flag 与 affinity 最终只有一个 `SchedConfig` truth；Phase 2B 切换前不安装第二 storage。
- local 与 remote setter 汇合到固定 owner CPU 的同一 `ApplyConfigPatch` transaction；syscall adapter 不以 stale snapshot 拼完整 config。
- one-shot persistent phase 是 receive 的唯一返回依据；Force 只结束并重建当前 Latch round，不关闭 receiver、不释放 remote gate。
- `SenderClosed` 只有在唯一 sender 与未来 mutation/complete capability 同时消失时才是 scheduler request 的确定失败。
- `REMOTE_SCHED_REQUEST_GATE` 只串行 remote scheduler request，handler 不获取它；wait-core placement owner不变。
- raw Linux ABI 只存在于 `sched/api`；class/core 不持 raw layout、policy number或 errno ordering。
- Fair、RT、RunQueue 与 task lifecycle owner不因阶段拆分而移动；accepted allocation issue与 limitation保持 Open / Active。

## Checkpoint Authority

下表只登记各后续 checkpoint 的初始协作边界；完整文件列表、验证命令和停止条件仍以 [迁移实施计划](../../rfcs/sched-dynamic-attributes/implementation.md) 为唯一 canonical authority。任何扩张必须先回写该计划和本事务。

| Checkpoint | 初始 write set | Implementation Owner | Review Owner | Runtime Owner | 初始状态 |
| --- | --- | --- | --- | --- | --- |
| 阶段 1 | `sched/{oneshot,mod}.rs` 与 oneshot owner 内 KUnit | Codex 总控或一个受限 worker | 与实现者不同的只读 reviewer | agent：build / pretest KUnit | Implementation Not Started；review/runtime Not Run |
| Checkpoint 2A | `sched/config.rs`、sched class typed foundation、priority 目录机械搬迁、`exception/ipi.rs` Clone 适配及同 owner KUnit | Codex 总控或一个受限 worker | 独立 reviewer 分别审 priority move、IPI Clone、dormant model | agent：build / pretest KUnit；priority runtime 未授权时由用户运行 | Implementation Not Started；review/runtime Not Run |
| Checkpoint 2B | request/config/processor/class/task/clone/priority/IPI/procfs final cutover 与同 owner KUnit | Codex 总控或一个受限 worker；不得拆分唯一 truth 切换 | 独立 reviewer 覆盖 config、role、request/gate、clone 与 2A 隔离 | agent：build / pretest KUnit；priority LTP由用户或明确授权的 agent | Implementation Not Started；review/runtime Not Run |
| 阶段 3 | affinity adapter、rv64/la64 syscall numbers、`sched-attr-test`、rv64 pretest routing；SMP/profile仅validation-only | Codex 总控或一个受限 worker | 独立 ABI 与 remote-gate reviewer | agent：双架构 build、pretest、单CPU app；用户或明确授权的 agent：SMP=2 stress / targeted LTP | Implementation Not Started；review/runtime Not Run |
| 阶段 4 | policy adapter、窄 interval accessor、syscall numbers、focused app；profile仅validation-only | Codex 总控或一个受限 worker | 独立 policy/permission/interval reviewer | agent：build / pretest / focused app；用户或明确授权的 agent：targeted LTP | Implementation Not Started；review/runtime Not Run |
| 阶段 5 | attr adapter、syscall numbers、focused app；profile仅validation-only | Codex 总控或一个受限 worker | 独立 Linux 6.6 size/copy/errno reviewer | agent：build / focused probes；用户或明确授权的 agent：targeted LTP | Implementation Not Started；review/runtime Not Run |
| 阶段 6 | 既有 owner 文件中的审计修正、focused asset、R0 docs/devlog/nav；register仅真实状态变化时更新 | Codex 总控 | 独立最终 reviewer | agent：build / source / format / docs；用户或明确授权的 agent：SMP=2、ABI matrix、schedule profile、必要的 la64 smoke | Implementation Not Started；review/runtime Not Run |

## Phase Log

### 2026-07-15 - 阶段 0 R0 Acceptance 与 Source Audit

**Phase:** 阶段 0 - 文档、Live Source 与 R0 Acceptance 前置 Gate。

**Change:** RFC 从 Draft 接受为 `R0 / Accepted for Implementation`；canonical invariants 转为 `Canonical / R0`，implementation plan 转为 `Active / R0`。建立本事务并接入 RFC、transaction index、当前双周 devlog、RFC index 与 mdBook 导航。阶段 0 未修改 kernel、ABI crate、app、rootfs、runner或live build配置。

**Document Review:** 首轮独立 review 发现 Force 关闭 receiver 并释放 gate 后，旧 request 仍可提交 mutation 的 Keter，因此当时未接受 R0。修订后的协议把 persistent channel phase恢复为唯一返回依据：Force只完成当前 Latch round，receiver清除旧 registration、锁外 drop、finish并在empty时rearm；`SenderClosed`同时证明未来 mutation capability消失。复审确认 `KETER-DYNATTR-006` 已 neutralize，最终无 Apollyon、Keter 或 Euclid。

**Source Audit:** live Fair / RT / RunQueue owner surface可以增加 dedicated reconfigure，不需要伪装成 yield、block、wake或preempt；`NCPUS` 在 production task construction前初始化，clone在publish前有完整config/affinity窗口。Generic IPI可以保持async transport并使用独立 scheduler one-shot，queue lock在业务handler前释放，`IpiPayload Copy -> Clone`可机械迁移。rv64/la64 syscall owner、raw user-copy helper、schedule LTP group和pretest入口均可承载后续write set。`AtomicNice`、direct setter与procfs all-online mask仍是Phase 2B必须原子替换的旧truth。

**Stop Conditions:** `implementation.md:121-124` 四项均未命中：class lifecycle可表达reconfigure；boot/clone可在publish前建立合法config；request variant不要求改变generic IPI completion/placement owner；UAPI matrix与canonical errno/field contract无矛盾。

**Register Boundary:** [IRQ-off heap allocation](../../register/open-issues.md#ane-20260622-irq-off-heap-allocation) 保持 Open；[RT noirq bucket allocation](../../register/current-limitations.md#ane-20260713-sched-rt-noirq-bucket-allocation) 保持 Active。本阶段不宣称修复。

**Validation:** `git diff --check` 通过；新增 transaction 的 `git diff --no-index --check /dev/null docs/src/devlog/transactions/2026-07-15-sched-dynamic-attributes.md` 无 whitespace 告警；`mdbook build docs` 通过。mdBook 仅报告既有 search index 较大警告。

**Not Run:** kernel build、format、KUnit、QEMU、focused app、SMP=2 stress、LTP与la64 runtime均未运行；阶段 0 是 docs/source-audit checkpoint。

**Next:** 先提交阶段 0。阶段 1 只能在该提交后按 canonical write set实现 dormant `sched::oneshot`，并通过全部 enabled KUnit与独立 review gate后再进入2A。

## Open Items

- 本 RFC owner内当前无开放 Apollyon、Keter或 Euclid。
- wait-core [KETER-WAIT-001](../../rfcs/sched-wait-refactor/tracking-issues.md#keter-wait-001synchronous-remote-placement-不能组合进-cross-cpu-ipi-completion) 继续 Open；R0 remote gate只neutralize scheduler request producer graph。
- 后续所有实现、review与runtime项保持 Not Started / Not Run，直到对应 checkpoint 明确启动。

## Closure

事务 Active；R0 尚未实现或关闭。
