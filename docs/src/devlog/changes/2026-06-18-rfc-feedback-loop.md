# ANE-CHG-20260618-rfc-feedback-loop

**Type:** Documentation / Workflow
**Status:** Completed
**Date:** 2026-06-18
**Authors:** doruche, Codex
**Area:** docs / RFC workflow / agent workflow

## Problem

现有 RFC 工作流已经要求 implementation 发现 RFC 不变量或边界错误时回到文档层修正，但规则过于隐含。实际使用时容易被解释成“设计阶段必须尽量清空所有 tracking issues 后才能写代码”，从而让大型 RFC 偏向前馈式设计内循环。

大型实现中的接口摩擦、状态机细节、错误路径、性能约束和模块集成问题，很多只有真实编码或最小端到端路径才能暴露。流程需要明确允许实现阶段反向修正设计，同时防止不确定性变成无约束漂移。

## Scope

本轮只更新共享工作流规则和可追溯记录：

- RFC 工作流、RFC 模板、RFC 索引说明；
- transaction / devlog 模板中的反馈字段；
- probe / feedback 的默认承载格式；
- 顶层 agent 指令与 repo-local RFC/devlog skill；
- 当前小迭代索引、双周 devlog 和 mdBook Summary。

本轮不重写既有 RFC，不 retroactively 清理已有 tracking issues，也不改变任何内核代码。

## Solution

保留“文档层先闭合 contract”的原则，但把闭合定义为：accepted contract、验证 floor、停止条件和反馈入口足够明确，而不是所有未知都在编码前消失。

新规则要求：

- 进入实现不必清空所有 `tracking-issues.md` 条目；仍改变 accepted contract、状态所有权、ABI 边界、阶段顺序或验收判断的 Apollyon / Keter 必须先 neutralize，或明确转成某个实现 gate 的停止条件。
- 高风险点可以进入 probe / vertical slice gate，但必须写清假设、最小 write set、验证 floor、失败信号、删除条件和 RFC 回写位置。
- 默认不新增通用 `feedback.md`、`probe.md` 或 `experiments.md`；probe 计划写在 `implementation.md`，执行反馈写在 transaction devlog，较大的证据包才进入 RFC `backgrounds/` 下的具体命名文件。
- 反馈只能优化实现路线，不能篡改目标或私自削弱不变量；若实现暴露出目标、不变量、ABI 边界或验收条件本身有问题，必须停止当前 gate 并回到 RFC review。
- 实现期反馈按影响归属：执行事实写 transaction devlog；阶段计划变化回写 `implementation.md`；不变量、ABI、owner boundary 或接受边界变化回写 RFC canonical 文本和 `tracking-issues.md`；接受限制或开放缺陷进入 register / current limitations。

## Change

- `docs/src/rfc-workflow.md` 新增受控反馈、probe / vertical slice gate、实现期反馈分流和 agent 约束。
- `docs/src/rfc-template.md` 在 RFC 入口、tracking issues 和 implementation 模板中加入不确定性、反馈假设、probe gate 和实现期反馈记录字段。
- `docs/src/rfcs.md` 对齐 tracking issues 与实现期反馈的职责说明。
- `docs/src/development-log.md` 和 `docs/src/templates.md` 明确 transaction devlog 如何记录实现期反馈。
- `AGENTS.md` 和 `.agents/skills/anemone-rfc-doc-workflow/SKILL.md` 同步未来 agent 的默认执行规则。
- 2026-06-18 follow-up 明确 probe / feedback 不默认新建通用 md 文件，并把标准字段写入 RFC 模板。
- 2026-06-18 follow-up 追加 anti-hacking 边界：不得以反馈为名缩小目标、调低验证集合、隐藏失败路径、削弱不变量，或在 RFC canonical 文本更新前让代码接受更弱语义。

## Validation

- `git diff --check` 通过。
- `git diff --no-index --check -- /dev/null docs/src/devlog/changes/2026-06-18-rfc-feedback-loop.md` 无 whitespace warning 输出；命令退出码为 `1`，这是 no-index diff 在文件存在差异时的预期结果。
- `mdbook build docs` 通过。

## Tracking Issues

None.

## Risk / Follow-up

已有 RFC 不在本轮批量改写。后续只有在具体 RFC 被继续推进、review 或收口时，才按新规则补充 probe gate、反馈分流或 tracking issue 状态。

## Links

- Biweekly devlog: [2026-06-08 至 2026-06-21](../2026-06-08_to_2026-06-21.md)
- RFC workflow: [RFC 工作流](../../rfc-workflow.md)
- RFC template: [RFC 模板](../../rfc-template.md)
- Register / limitations: None
