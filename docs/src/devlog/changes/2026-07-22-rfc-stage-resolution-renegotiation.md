# ANE-CHG-20260722-rfc-stage-resolution-renegotiation

**Type:** Documentation / Workflow
**Status:** Completed
**Date:** 2026-07-22
**Authors:** doruche, Codex
**Area:** docs / RFC workflow / implementation planning / agent review

## Problem

现有 RFC 工作流只把逐文件 write set 改为滚动解析，但仍要求接受时为每个未来阶段预先写出较完整的交付、验证和停止条件。模板中的 `Scope Only / Ready` 又只表达 manifest 是否冻结，容易让 reviewer 把未来阶段尚无实现证据支持的类型、函数、算法、corner case 和测试命令当成缺陷，重新制造“大型 RFC 必须一次写完 implementation.md”的压力。

同时，原 anti-hacking 规则把实现反馈概括为“只能优化路线，不能改变目标”。它能阻止 agent 静默降标，却没有明确说明真实工程代价过高时如何保留有价值的较小实现、重新接受 reduced target，或决定 Not Cut Over，容易把“停止当前 gate”误解为“必须丢弃整次实现”。

## Scope

本轮只更新共享 RFC / transaction 工作流、模板、agent/review 指令和对应历史入口：

- 阶段 `Outline / Ready / Active / Closed` 成熟度与滚动式阶段解析；
- `N -> N+1 Implementation Resolution Gate` 及其中的 resolved write-set 子步骤；
- future Outline 的 review finding 边界；
- correctness invariant、target guarantee / capability 与 implementation preference 的分类；
- `Target Renegotiation Gate`、reduced target 决策和部分代码处置；
- transaction 证据格式、历史 supersession、索引和导航。

本轮不修改内核代码，不批量重写既有 RFC、已完成阶段或历史 transaction，也不重新分类现有 register / current limitations。

## Solution

多阶段 RFC 接受时只要求第一个可执行阶段完整解析为 `Ready`。future `Outline` 只冻结概括目的、依赖、受保护的 target / contract / correctness 边界和解析触发点；Stage N 独立关闭后，`N -> N+1 Implementation Resolution Gate` 读取 live source、实际 diff、review、验证和 current contracts，再解析下一阶段的交付、实现路径或 probe、审计、可观测性、验证、停止/退出条件、contract cutover 和 resolved manifest。`Ready` 表示整个阶段已解析，但不自动授权进入 `Active`。

Review 必须按阶段成熟度判断精度。不得仅因 future Outline 缺少类型、函数、算法、逐文件路径、完整 corner-case 矩阵或精确测试命令而形成 finding；只有缺少必要依赖/受保护边界、target 路径不可达，或把潜在 owner / ABI / contract / acceptance 变化无 gate 地推迟时才构成问题。

实现反馈不能由 agent 自行降低 accepted target，但真实工程证据可以触发 `Target Renegotiation Gate`。当前阶段先停止 cutover 并记录成本、已完成 slice、受影响语义、备选方案和代码去向，再由 RFC review 选择：

- `Route Correction`：保持 target，只改实施路线；
- `Accepted Reduced Target`：同一 RFC 内形成新的 accepted revision；
- `Follow-up RFC`：核心目标、owner、方案或大部分证明边界变化；
- `Not Cut Over`：部分实现无法形成安全、诚实、独立能力，不因沉没成本激活。

correctness invariant 不能作为工程妥协项；target guarantee / capability 在当前修订中保持约束力，但可经 renegotiation 形成新修订；implementation preference 由滚动阶段解析决定。accepted limitation 必须位于新 target 之外，新 target 范围内的错误仍属于 open issue。

## Change

- `docs/src/rfc-workflow.md` 定义阶段成熟度、完整滚动解析、target renegotiation、review 边界和迁移规则。
- `docs/src/rfc-template.md` 增加 Future Stage Outline、Ready Stage、Implementation Resolution Gate、规则分类和 Target Renegotiation 模板。
- `docs/src/rfcs.md` 对齐公共入口中的阶段解析与工程妥协语义。
- `docs/src/development-log.md`、`docs/src/templates.md` 增加 transaction 的 renegotiation 证据和决定字段。
- `AGENTS.md`、`.agents/skills/anemone-rfc-doc-workflow/SKILL.md` 与 `.agents/skills/anemone-code-review-principles/SKILL.md` 同步执行和 review 约束。
- 2026-06-18 feedback-loop 记录追加 supersession，不改写当时的历史结论。

## Validation

- `git diff --check` 通过。
- 新文件 `git diff --no-index --check` 无 whitespace warning；退出码 `1` 是 no-index 文件存在差异的预期结果。
- 两个 repo-local skill 的 YAML frontmatter 解析通过。
- active workflow、模板、AGENTS 和两个 skill 中旧 `反馈只能优化路线`、`Write-set Resolution Gate`、`Scope Only / Ready` 等表述定向搜索零命中。
- 新增/修改导航与相对链接 source audit 通过。
- `mdbook build docs` 通过；只保留既有 large search-index warning。

## Tracking Issues

None.

## Risk / Follow-up

既有 RFC 和 transaction 不批量迁移。活跃 RFC 从下一个尚未解析的阶段开始使用新规则；历史 manifest 和阶段事实继续保留。已有 future stage 即使写得更详细也不要求删除，只是不再把这些未冻结内容当成执行授权或必须保持的精确设计。

## Links

- Biweekly devlog: [2026-07-06 至 2026-07-19](../2026-07-06_to_2026-07-19.md)
- RFC workflow: [RFC 工作流](../../rfc-workflow.md)
- RFC template: [RFC 模板](../../rfc-template.md)
- Previous feedback rule: [RFC workflow feedback loop](./2026-06-18-rfc-feedback-loop.md)
- Register / limitations: None
