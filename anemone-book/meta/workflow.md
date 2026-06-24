# Anemone Book 写作工作流

本文定义 `anemone-book` 内部的轻量写作工作流。它只约束书稿生产，不替代
Anemone 仓库已有的 RFC、devlog、register 或 current limitations。

## 文件角色

- `positioning.md`：写作契约。记录读者、目的、非目标、语气、章节深度和
  artifact 边界。全书方向变化先更新这里。
- `outline.md`：章节结构。记录每章目标、核心论点、代表路径、图表候选、
  代码片段候选和模块覆盖矩阵。
- `sources.md`：材料入口。记录书稿可引用的 RFC、devlog、register、代码模块、
  外部系统、论文或文档入口。
- `style.md`：风格指南。记录编号、标题、术语、callout、图表、代码片段、
  字体、页眉页脚和语气规则。
- `agent-orchestration.md`：agent 编排规则。记录主编、章节作者、审稿者的职责、
  章节流水线、并行边界和 write set 规则。
- `workflow.md`：本文件。记录书稿内部推进规则。
- `main.typ`：正式排版入口。只在定位和章节结构稳定后承载正文。
- `refs.bib`：正式引用数据。外部引语、论文、项目文档进入正文前应落到这里或
  在正文旁保留可核对来源。

默认不创建 `tracking.md`、`todo.md`、`devlog.md`、`rfc.md`。短期 TODO 放在
`outline.md` 的章内备注；项目事实仍回到仓库 canonical 文档层。

## 推进顺序

1. 若改变全书定位、读者、语气、非目标或 artifact 边界，先更新
   `positioning.md`。
2. 若改变章节结构、章节职责、覆盖范围或代表路径，先更新 `outline.md`。
3. 每章正文写作前，先在 `sources.md` 确认可引用材料，避免凭记忆写事实。
4. `main.typ` 只承载已经过 `positioning.md` 和 `outline.md` 收束的正文。
5. 大段重排前先更新 outline，再改 Typst，避免排版文件同时承担计划和正文职责。

## 版本维护

书稿源保持单一，版本通过 release snapshot 表达。

- `main.typ` 和 `meta/*` 是持续演进的 authoring source。
- 初赛版、决赛版通过 git tag、提交点和 PDF artifact 固化，不长期维护两套正文目录。
- 开发过程中不实时追每个小改动；重要 RFC accepted / closed、transaction 收口、
  register 限制变化、核心模块设计变化时，做一次书稿影响检查。
- 每个双周 devlog 周期结束后，快速扫 `outline.md`、`sources.md` 和相关正文，
  确认没有明显 stale claim。
- 版本冻结前做一次事实核对、限制边界核对、引用核对、Typst 编译和 PDF 检查。
- 冻结后只修事实错误、错别字、引用错误和排版问题；新的设计进展进入下一版。
- 决赛版在同一书稿源上继续演进，替换过时章节、加入新模块、新验证证据和新架构
  取舍，而不是复制初赛版后分叉维护。

书稿不维护自己的进度账本。版本事实由 git tag、提交点、release artifact 和
必要的 README 说明表达。

## Typst 项目结构

当前 `@preview/ilm` 模板只是早期占位，不作为必须维护的依赖。正式书稿改用
repo-local 轻量模板；可以借鉴 `ilm` 的简约、留白、书籍感和克制装饰，但不
依赖外部模板包。

目标结构：

```text
anemone-book/
  main.typ
  template/
    book.typ
    components.typ
    figures.typ
  chapters/
    00-preface.typ
    01-design-map.typ
    ...
  appendices/
    glossary.typ
    agentic-coding-workflow.typ
  assets/
    figures/
    images/
    sources/
  meta/
    positioning.md
    workflow.md
    agent-orchestration.md
    outline.md
    sources.md
    style.md
  refs.bib
  build/
```

规则：

- `main.typ` 保持很薄，只导入模板并 include 章节。
- 正文直接写 Typst；`meta/` 继续使用 Markdown。
- 每章一个 `.typ` 文件，附录也拆文件，便于 review 和后续版本演进。
- Typst 正文不要按源码列宽硬换行；中文自然段优先一段一行，必须换行时只能在标点或结构边界之后换行，避免编译产物出现句中空格。
- `template/book.typ` 负责全局页面、字体、标题、目录、页眉页脚和参考文献设置。
- `template/components.typ` 负责 epigraph、principle、boundary、tradeoff、note
  等克制 callout。
- `template/figures.typ` 负责图、图题、图源和 diagram wrapper 的统一样式。
- 源文件不和产物混放；默认 PDF 输出到 `build/`。
- 暂不引入 Typst package / `typst.toml`；如果后续工具链需要，再单独讨论。
- 暂不接入仓库根 build system；先在 README 或本目录脚本中记录 Typst 命令。

主标题固定为 `The Anemone Book`。正文标题中文为主，可保留必要英文关键词。
书稿采用偏技术书的版式，而不是论文报告版式。

构建命令默认形态：

```sh
mkdir -p anemone-book/build
typst compile anemone-book/main.typ anemone-book/build/anemone-book.pdf
```

如果后续从 `anemone-book/` 内执行构建，可使用：

```sh
mkdir -p build
typst compile main.typ build/anemone-book.pdf
```

## Assets

- 项目 logo 放在 `assets/images/`，例如 `assets/images/anemone.png`。
- 最终正文引用的图放在 `assets/figures/`。
- 可编辑图源、生成脚本或说明放在 `assets/sources/`。
- 小型装饰性或排版性图形可以用 Typst 原生能力；内核对象关系、owner boundary、
  数据路径、状态机、VFS / device bridge 等技术结构图优先用 draw.io 维护源文件。
- draw.io 源文件放在 `assets/sources/`，导出的正式图默认用 PNG，放在
  `assets/figures/`，由 Typst 正文引用。
- SVG 暂不作为正文引用格式；Typst 对当前 draw.io SVG 的文字显示存在问题。
- PNG 可以作为正式正文图，但必须保留对应 `.drawio` 源文件。
- 截图、照片、运行界面等 bitmap 资产需要说明来源。
- 不提交只有不可编辑导出物、没有源或说明的复杂图。
- 文件命名应能看出章节和图意图，例如
  `assets/sources/ch05-vfs-object-model.drawio` 与
  `assets/figures/ch05-vfs-object-model.png`。

draw.io 导出命令默认形态：

```sh
drawio -x -f png --width 2000 \
  -o anemone-book/assets/figures/ch05-vfs-object-model.png \
  anemone-book/assets/sources/ch05-vfs-object-model.drawio
```

如果运行环境无法调用 draw.io CLI，可以先提交 `.drawio` 源文件和图题说明，再在可
运行 CLI 的宿主环境导出 PNG。正式版本冻结前必须确保正文引用的是已导出的稳定图。

## 引用与术语

- 正式外部参考资料进入 `refs.bib`，例如论文、书、官方文档、Linux docs、
  Zircon / Fuchsia docs、man-pages。
- 章节 epigraph 如果是原话，也应尽量进入 `refs.bib` 或至少保留可核对来源。
- 代码、RFC、devlog、register 和 current limitations 不进入 bibliography；
  正文按需要用路径、章节名或脚注说明。
- 不使用精确到行号、日志偏移、临时 diff 位置这类会随正常维护漂移的引用方式；
  需要指向代码或文档时，使用稳定路径、模块名、章节锚点、符号名或脚注说明可核对入口。
- 出处不稳的社区梗和临时网页不进正式 bibliography；能转述就转述，不能核对就不用。
- 术语表放附录；索引暂不优先，等术语和图表规模稳定后再决定是否增加。

## 事实与引用

- 代码行为以源码为准。
- 已接受设计 contract 以 RFC 为准。
- 执行事实、checkpoint、review 和验证证据以 devlog / transaction devlog 为准。
- 当前开放问题和接受限制以 register / current limitations 为准。
- `anemone-book` 只做叙述聚合，不反向定义 Anemone 当前事实。

外部系统、论文、名人引语、社区材料进入正文前必须能核对来源。出处不稳时，
改为转述或删除。不要把用户私人草稿路径写成公开书稿的稳定来源。

## 章节写作

每章优先写一个清晰论点，而不是罗列功能。一个功能只有在能解释设计原则、
关键 trade-off、代表路径、ABI 分层或 accepted limitation 时才进入正文。

每章至少在 outline 中明确：

- 本章要让读者相信什么；
- 本章覆盖哪些重要模块；
- 代表路径 / case study 是什么；
- 需要哪些图或代码片段；
- 需要核对哪些来源；
- 哪些边界或 accepted limitations 需要自然提及。

## 图与代码片段

图必须有论点。图题应写成明确技术判断，而不是“某某结构图 / 流程图”。
第一章图较多是可以接受的，后续模块章只放能降低理解成本的图。

draw.io 图进入正文前至少需要满足：

- `.drawio` 源文件存在于 `assets/sources/`。
- PNG 导出物存在于 `assets/figures/`。
- 图题能表达技术结论。
- 图中文字、箭头和分组与正文术语一致。
- Typst 能成功引用导出的图。

代码片段只展示类型、接口形状、不变量或边界，不放长函数实现。代码片段
必须服务正文论点。

## 审稿与验证

书稿元文件变更至少运行：

```sh
git diff --check -- anemone-book
```

Typst 正文或样式变更至少运行：

```sh
typst compile anemone-book/main.typ anemone-book/build/anemone-book.pdf
```

新增或修改 draw.io 图时，至少重新导出对应 PNG，并运行一次 Typst 编译。大图或
自动布局图还应检查是否有重叠、断线、文字截断、箭头穿过无关节点等问题。

需要检查文本结构时，可使用 `pdftotext` 或 Typst HTML / text 导出。需要检查
视觉布局时，再查看导出的 PNG 或 PDF 页面。书稿变更默认不运行 QEMU、LTP 或
内核 build，除非正文事实需要新验证。

## 收束规则

如果某个争议影响全书定位，回到 `positioning.md`。如果只影响章节结构，回到
`outline.md`。如果只是材料来源不清，回到 `sources.md`。不要为单个争议新建
并行的进度文件或事实文件。
