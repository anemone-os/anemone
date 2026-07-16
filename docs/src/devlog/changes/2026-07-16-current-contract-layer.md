# ANE-CHG-20260716-current-contract-layer

**Type:** Documentation / Workflow
**Status:** Completed
**Date:** 2026-07-16
**Authors:** doruche, Codex
**Area:** docs / current contract / RFC workflow / agent workflow

## Problem

现行 RFC semantic revision 规则把每份 RFC 的 `index.md` / `invariants.md` 同时当作决策历史和 current consolidated contract。该模型可以处理同一 RFC 内的 R0 -> R1 修订，但当新的 RFC 扩展或替换多个旧 RFC 共同依赖的不变量时，会要求逐份回写历史正文和 supersession 提示；维护扇出本身容易遗漏，并让多份旧 RFC 同时表现为当前权威。

另一方面，为解决该问题一次性盘点整个 scheduler、VFS、task 等领域的全部不变量，会制造一份无法证明完整的新目录，并把当前 RFC 扩大成仓库级文档迁移。

## Scope

本轮只建立 current contract 文档层和配套治理规则：

- current contract、RFC target、transaction evidence 与 Git history 的权威边界；
- 按 owner / contract surface 组织、稳定 ID、最小闭包和按触达迁移；
- 小不变量、RFC-local invariant 与跨 RFC shared invariant 的分流；
- 跨领域依赖、接口 contract、唯一协议/state owner 和局部义务；
- RFC `Contract Impact`、accepted target、pending successor 与 cutover 生命周期；
- docs、模板、AGENTS、repo-local skill、导航与开发日志同步。

本轮不修改内核代码，不批量搬运既有 RFC 不变量，也不为当前设计中的具体 RFC 猜测 contract ID 或 owner。

## Solution

采用三层语义模型：

- `docs/src/contracts/` 保存已经生效、需要跨 RFC / 模块引用的共享规则；对已登记 ID，它是当前文档权威。
- RFC 保存 accepted target、相对 current contract 的 delta，以及只服务本方案或迁移的 proof obligations。
- transaction devlog 保存实际执行、review、验证与 contract cutover 证据；Git 保存物理文本历史。

迁移按触达进行。后续 RFC 第一次复用、扩展或替换既有共享规则时，只提取直接受影响规则、唯一 owner 和必要直接依赖构成的最小闭包；旧 RFC 正文保持历史原貌，不要求批量 backlink。Contract 文档按稳定 owner 与共同变化/共同证明的 surface 组织，小规则聚合为稳定 ID 条目；跨领域 handoff 必须明确唯一协议 owner、每份状态的唯一 owner、局部义务、线性化点和 cleanup。

RFC acceptance 只形成 target，不提前覆盖 effective contract。每个影响项用 `Preserve` / `Refine` / `Replace` / `Remove` / `Scoped Exception` 和生效 gate 表达；只有 docs-only 或 implementation cutover 达到验证和停止条件后，才更新 current contract。

## Change

- 新增 `docs/src/contracts.md`，定义 contract 权威、组织、增量迁移、跨域规则、文档格式、RFC 生命周期和 supersession。
- 新增 `docs/src/contract-template.md`，提供 owner index、contract surface、稳定 ID、局部义务、transitional contract、retired-ID 和 RFC impact 模板。
- 更新 RFC workflow、RFC template 与 RFC 入口，移除 RFC 作为跨 RFC current consolidated contract 的旧职责。
- 更新文档框架、ADR 边界、开发日志和 transaction 模板，使 contract cutover 有明确记录位置。
- 更新 `AGENTS.md`、`.agents/skills/anemone-rfc-doc-workflow/SKILL.md` 与 `.agents/skills/anemone-code-review-principles/SKILL.md`，约束 agent 做最小闭包提取、effective/target 分离、跨域 owner 审查，并让 reviewer 从 effective contract / RFC target / cutover evidence 判断当前语义。
- 更新 mdBook 导航、小迭代索引和双周 devlog；旧 semantic revision 记录追加显式 supersession，而不改写其历史结论。

## Validation

- `git diff --check` 通过。
- RFC workflow 与 code-review skill 的 YAML frontmatter 解析通过。
- `mdbook build docs` 通过；只保留既有 large search-index warning。
- 变更 Markdown 的本地相对链接审计通过。
- 活动 workflow / template / AGENTS / skill 文本的旧模型搜索通过；旧两层模型只保留在 2026-07-14 历史记录及本轮 supersession 说明中，不再作为当前规则。

## Tracking Issues

None.

## Risk / Follow-up

当前 contract registry 有意为空；这表示尚未按新规则提取 shared invariants，不表示仓库不存在不变量。首个实际需要跨 RFC supersession 的 RFC 应建立最小 contract surface，并用真实 owner / code / RFC / transaction 证据填充，不能把模板示例当成已接受的 scheduler 或 wait-core 规则。

如果实际使用中发现 contract surface 边界过粗或过细，应优先调整文档分组而保持稳定 ID；只有规则语义变化才走 RFC delta 与 cutover。

## Links

- Biweekly devlog: [2026-07-06 至 2026-07-19](../2026-07-06_to_2026-07-19.md)
- Current contracts: [当前契约](../../contracts.md)
- Contract template: [当前契约模板](../../contract-template.md)
- RFC workflow: [RFC 工作流](../../rfc-workflow.md#当前契约与-rfc-的边界)
- Previous workflow record: [RFC semantic revision workflow](./2026-07-14-rfc-semantic-revisions.md)
