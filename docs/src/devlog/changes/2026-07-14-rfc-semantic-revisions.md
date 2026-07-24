# ANE-CHG-20260714-rfc-semantic-revisions

**Type:** Documentation / Workflow
**Status:** Completed
**Date:** 2026-07-14
**Authors:** doruche, Codex
**Area:** docs / RFC workflow / agent workflow

## Problem

现有 RFC feedback loop 允许实现证据回写 canonical 文本，但 Closed RFC 在后续框架适配中多次改变不变量、owner boundary 或实现 gate 时，缺少明确的语义版本边界。仓库 Git 已经保存逐 commit 文本历史，但读者不能仅靠 diff 判断哪些变化是经过 review 接受的新 contract，哪些只是措辞、证据或执行记录更新。

为每个 RFC 建立独立 Git 仓库会拆散代码、RFC、transaction 和 register 的共同历史；新增通用 patch / amendment 文件又会要求读者回放增量才能恢复当前 contract，并形成第二套真相源。

## Scope

本轮只更新共享文档工作流和 agent 执行规则：

- RFC Git / semantic revision 模型；
- RFC 与 transaction 模板；
- Closed RFC 后续修订的 transaction 边界；
- RFC 入口说明、开发日志规则、agent 指令和 repo-local skill；
- 本小迭代、索引、双周 devlog 与 mdBook 导航。

本轮不修改内核代码，不批量给既有 RFC 补 `R0`，也不重写既有 transaction 历史。

## Solution

采用“仓库 Git 保存物理历史，RFC 修订号保存 accepted semantics”的两层模型：

- RFC 继续位于 Anemone 主仓库，不建立 per-RFC 仓库，也不创建 `index-v1.md`、默认 amendment 文档或并列 canonical 副本。
- Draft 修订写 `Draft`；第一次 accepted contract 形成 `R0`。只有目标、非目标、accepted contract、不变量、状态所有权、ABI / 可见语义或接受边界发生已接受变化时才递增 `R1`、`R2`。
- 拼写、链接、措辞、证据，以及保持 contract 的阶段顺序、write set 或验证调整不递增修订。
- `index.md` / `invariants.md` 原地维护当前 consolidated contract；`implementation.md` 保留已完成阶段并追加修订实施段；`tracking-issues.md` 保留问题来源、状态迁移和 neutralize 依据。
- RFC 页首状态描述当前修订：后续修订已接受但尚待实现时回到 `Accepted for Implementation`，收口后再回到 `Closed`；旧修订状态保留在修订记录和原 transaction 中。
- Closed RFC 的新语义修订需要代码工作时建立引用该修订的新 transaction；纯文档修订可以只链接 review / 小迭代证据，不制造空事务。
- 核心目标、主要 owner、整体方案或大部分证明边界变化时，新建 follow-up RFC 并记录 supersede / successor 关系。

## Change

- `docs/src/rfc-workflow.md` 定义 Git 与语义修订职责、修订触发条件、canonical / 增量文档边界和 follow-up RFC 阈值。
- `docs/src/rfc-template.md` 增加 `修订`、修订记录、子文档适用修订和修订实施记录模板。
- `docs/src/rfcs.md` 增加读者入口和 transaction 修订规则。
- `docs/src/development-log.md`、`docs/src/templates.md` 增加 transaction 目标修订字段和 post-close 新事务边界。
- `AGENTS.md` 与 `.agents/skills/anemone-rfc-doc-workflow/SKILL.md` 同步 agent 执行约束。

## Validation

- `git diff --check` 通过。
- RFC workflow skill YAML frontmatter 解析通过。
- `mdbook build docs` 通过。

## Tracking Issues

None.

## Risk / Follow-up

既有 RFC 不批量 retrofit。后续某份既有 RFC 第一次发生语义修订时，应先从其 accepted / closure 文本与 Git 历史建立可验证的 `R0` baseline；无法确认的日期或结论不得猜测补齐。

如果实际使用中 `R<n>` 与某份 RFC 自己的阶段编号冲突，应在该 RFC 的显示文本中写完整的“RFC 修订 `R<n>`”，但不另造 SemVer 层级。

## Supersession

2026-07-16 的 [current contract layer](./2026-07-16-current-contract-layer.md) 保留“Git 保存物理历史、RFC `R<n>` 标记 accepted target 修订”的结论，但取代了“每份 RFC `index.md` / `invariants.md` 持续维护跨 RFC current consolidated contract”的部分。后续共享规则按 owner / contract surface 提取到 `docs/src/contracts/`，RFC 保存 target 与 delta；本文继续作为旧两层模型的历史决策记录。

## Links

- Biweekly devlog: [2026-07-06 至 2026-07-19](../2026-07-06_to_2026-07-19.md)
- RFC workflow: [RFC 工作流](../../rfc-workflow.md#git-历史与语义修订)
- RFC template: [RFC 模板](../../rfc-template.md)
- Register / limitations: None
