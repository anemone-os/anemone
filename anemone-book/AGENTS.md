# Anemone Book 目录规则

本目录是 `The Anemone Book` 的书稿源。修改、审阅或验证本目录时，先遵守仓库根
`AGENTS.md`，再遵守本文件。

## 必读入口

任何修改或审阅前，必须先读：

- `README.md`
- `meta/workflow.md`

再按任务类型补读：

- 全书定位、读者、非目标、artifact 边界：`meta/positioning.md`
- 章节结构、模块覆盖、代表路径、图和 listing 候选：`meta/outline.md`
- 文风、编号、标题、术语、callout、图题、Typst 换行：`meta/style.md`
- 技术事实、引用、外部材料、accepted limitations：`meta/sources.md`
- 多 agent 编排、章节 brief、source pass、review pass、write set：
  `meta/agent-orchestration.md`

## 边界

- `anemone-book` 只做设计叙述快照，不是 Anemone 的 canonical source。
- 代码行为以源码为准；accepted contract 以 RFC 为准；执行事实以 devlog /
  transaction devlog 为准；开放问题和已接受限制以 register / current
  limitations 为准。
- 不把私人工作路径、临时草稿或 `etc/` 写成公开书稿的稳定来源。
- 不新增并行的 `tracking.md`、`todo.md`、`devlog.md`、`rfc.md` 等进度或事实文件。
- 不静默扩大 write set。需要改 meta、模板、其它章节或资产时，先说明原因、
  影响范围和验证方式。

## 写作与验证

- 正文使用中文为主，保留必要英文技术术语。
- 每个正文章节应有 thesis paragraph；它说明本章立场，不写成教材式流程摘要。
- 正文优先解释 owner boundary、ABI boundary、不变量、trade-off 和工程闭环，
  不罗列 syscall、测例分数或功能清单。
- Typst 中文自然段优先一段一行；必须换行时，只在中文标点或结构边界之后换行。
- 图必须有可编辑源文件和明确技术结论；复杂技术图优先维护 `.drawio` 源文件。
- 只改 `meta/*.md` 时，至少运行 `git diff --check -- anemone-book`。
- 改 Typst 正文、模板、引用或正文使用的图时，至少运行
  `typst compile anemone-book/main.typ anemone-book/build/anemone-book.pdf`。
- 书稿改动默认不运行 QEMU、LTP 或内核 build，除非正文事实需要新的验证证据。
