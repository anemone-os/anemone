# Agent Orchestration

本文定义 `The Anemone Book` 的 agent 写作编排规则。它只约束书稿生产协作，不
替代 Anemone 的 RFC、devlog、register、current limitations，也不记录章节进度。

## 目标

书稿可以由多个 agent 协作，但必须保持一个稳定的叙述中心和事实边界。

- 全书定位、章节结构、术语和风格由主编 agent 串行维护。
- 章节 agent 可以并行取材和起草，但只能在明确 write set 内工作。
- review agent 只审事实、边界、遗漏和文风风险，不顺手改全书结构。
- 任何设计事实以代码、RFC、devlog、register / current limitations 和外部原始
  来源为准；book 只做叙述聚合。

## 角色

### Editor

Editor 是全书主编，负责：

- 维护 `positioning.md`、`outline.md`、`style.md` 和本文件。
- 分配章节 write set 和 source pass 任务。
- 决定哪些章节可以并行，哪些必须等待前置章节定稿。
- 统一术语、图题、callout、引用格式和 chapter opening。
- 合并各章正文，并处理跨章节冲突。

Editor 不应把自己变成进度账本。进度可以在对话、临时工作区或 issue 中短期存在，
但不进入 `anemone-book/meta/`。

### Chapter Writer

Chapter Writer 负责一个章节或一个明确的小节。默认 write set 包括：

- 对应的 `chapters/*.typ`。
- 该章直接使用的图源或图片。
- `sources.md` 中和该章相关的材料入口。

Chapter Writer 可以提出 outline、style 或术语修改建议，但不能直接扩大 write set。
如果发现章节结构或事实边界不成立，应停止并向 Editor 报告。

### Reviewer

Reviewer 负责章节审查，重点看：

- 事实是否能回到代码、RFC、devlog、register / current limitations 或外部原始
  来源。
- 章节是否越界成实现计划、RFC、测试分数报告或功能清单。
- 关键模块是否遗漏。
- owner boundary、ABI boundary、lifetime、wait / wake、VFS / device bridge 等
  叙述是否混乱。
- 语气是否过度宣传、过度 AI 腔，或过度教程化。

Reviewer 的输出应先列阻塞问题，再列可选润色。没有阻塞问题时，明确说明剩余风险。

## Dependency Topology

不要严格按照目录串行，也不要把所有章节一次性并行。默认采用“主编串行锁 +
依赖拓扑 + 有界并行”。

### Anchor Seed Phase

以下内容先由 Editor 串行完成 seed 版本：

- `§0 Preface`。
- `§1 Design Philosophy and System Map`。
- 术语表初版。
- Typst 模板和章节骨架。

seed 版本只定义全书的读者契约、对象词汇和版式约束，避免后续章节各写各的。它不
追求文采、完整叙事、最终图表或精致开场；尤其不要在模块章节完成 source pass 前
替后文做过满总结。

seed 稳定前，不启动大规模章节正文并行。`§0` 和 `§1` 的最终版本必须等模块章节
完成 draft / review 后再回写。

### Parallel Draft Phase

seed 稳定后，可以按依赖关系并行：

- `§2 ABI Boundary and Syscall Layer` 可较早启动，因为它定义 Linux ABI 与
  Anemone internal contract 的边界。
- `§3 Tasks, Processes, and Execution Context` 与
  `§4 Scheduling, Waiting, and Time` 可以并行取材，但成稿时必须互审 task /
  scheduler / wait-core 的 ownership 边界。
- `§5 VFS, Namespace, and Pseudo Filesystems` 与
  `§6 Device Driver Model and I/O Objects` 可以并行取材，但 devfs、future sysfs
  和 control surface 的叙事由 Editor 统一。
- `§7 Memory Management and Memory Object` 可独立推进，但要回连 `§1` 的折中路线、
  `§2` 的 ABI 边界，以及 VFS / page cache 的相关叙述。
- `§8 Architecture, Traps, and Platform Boundaries` 独立性较高，可以较早并行。
- `§9 Engineering Feedback Loop` 后写，因为它应吸收前面章节的工程证据，而不是
  反过来驱动内核设计叙事。

### Spine Rewrite Phase

模块章节完成主要 draft / review 后，Editor 回头重写 `§0` 和 `§1`。

- `§0 Preface` 负责人味、读者契约、影响来源和全书非目标。
- `§1 Design Philosophy and System Map` 负责把已经成型的模块章压缩成系统地图。
- 早期 seed 中过强、过虚或与后文章节不贴合的判断必须删除或改写。
- 关键图、epigraph 和跨章节引用在这个阶段补齐，而不是在 seed 阶段强行定死。
- 术语表根据各模块章的实际用词更新。

如果 spine rewrite 发现模块章支撑不了 `§1` 的某个全局判断，优先修正 `§1`，
而不是要求模块章迎合早期总论。

### Freeze Phase

版本冻结前，Editor 做全书级 pass：

- stale claim 检查。
- accepted limitation / current limitation 边界检查。
- 术语表和正文首次定义检查。
- 图题是否表达技术结论。
- 引用和 epigraph 来源检查。
- Typst 编译与 PDF 版面检查。

冻结后只修事实错误、错别字、引用错误和排版问题。新的设计进展进入下一版。

## Chapter Workflow

一个章节默认按以下流水线推进。

### 1. Brief

Editor 给 Chapter Writer 一份短 brief，至少包含：

- 章节标题和目标读者。
- 本章希望读者相信什么。
- 必须覆盖的重要模块。
- 代表路径 / case study。
- 必须核对的代码、RFC、devlog、register / current limitations 或外部资料。
- 明确不写什么。
- write set。

brief 是启动指令，不是长期维护文件。除非内容沉淀为 outline 或 style 规则，否则
不进入 `meta/`。

### 2. Source Pass

Chapter Writer 先提交材料摘录和事实清单，不急着写正文。

Source pass 应回答：

- 这一章有哪些稳定事实？
- 哪些事实只能说成当前实现状态？
- 哪些地方存在 accepted limitation 或 open issue？
- 哪些代码路径适合作为代表路径？
- 哪些图或 listing 真正能降低理解成本？

不确定事实必须标注来源缺口，不能凭记忆补齐。

### 3. Thesis Pass

Editor 和 Chapter Writer 收束本章的 2-4 个设计判断。

好的 thesis 不是“本章介绍了什么”，而是“本章为什么这样组织系统”。例如：

- `syscall adapter` 只翻译 Linux ABI，不拥有内部对象语义。
- `devfs` 暴露设备节点，但设备语义仍由 driver owner 决定。
- `wait-core` 拥有阻塞协议，event source 只发布唤醒能力。

若 thesis 与 `outline.md` 冲突，先修 outline，再写正文。

### 4. Draft Pass

Chapter Writer 写正文草稿。规则：

- 中文正文为主，保留必要英文术语。
- 不罗列功能清单。
- 不把测例、分数或比赛环境写成叙事核心。
- 不把 RFC 的实现计划复制进书稿。
- 只展示能支撑设计判断的代码片段。
- 图题必须能写出明确技术结论。
- draw.io 图必须同时保留 `.drawio` 源文件和导出的 SVG。
- 遇到边界、限制和临时兼容层时，用中性工程语言说明，不写成道歉或营销。

### 5. Review Pass

Reviewer 审查章节草稿。最少检查：

- 技术事实是否可核对。
- 模块覆盖是否满足 outline。
- 是否误把 Linux ABI 兼容写成 Linux 内核复刻。
- 是否误把 Rust 写成自动正确性叙事。
- 是否混淆 device model、VFS、devfs / pseudo fs 的边界。
- 是否遗漏重要 trade-off 或 accepted limitation。
- 图、listing 和 callout 是否服务正文。
- 图源、导出物和 Typst 引用是否一致。

Review finding 应带章节位置、问题、影响和建议修复方向。不要只给泛泛评价。

### 6. Editorial Pass

Editor 合稿并统一：

- 标题大小写和 `§` 编号。
- 术语首次定义。
- callout 标签。
- figure / table / listing 编号和图题。
- epigraph 来源。
- 跨章节引用。
- 语气密度。

如果 editorial pass 发现章节事实不稳，退回 source pass，而不是用文风掩盖。

### 7. Build Pass

涉及 Typst 正文或模板时，至少运行：

```sh
typst compile anemone-book/main.typ anemone-book/build/anemone-book.pdf
```

只改 `meta/*.md` 时，至少运行：

```sh
git diff --check -- anemone-book
```

书稿变更默认不运行 QEMU、LTP 或内核 build，除非正文事实需要新的验证证据。

## Write Set Rules

默认不允许 agent 静默扩大 write set。

- 章节 agent 需要改 `outline.md`：先报告原因、影响章节和建议改法。
- 章节 agent 需要改 `style.md`：先说明这是局部例外还是全书规则。
- 章节 agent 需要改 `positioning.md`：停止当前章节任务，交由 Editor 判断。
- 章节 agent 需要新增图表资产：说明图要证明的技术结论，并提交可编辑源文件。
- 章节 agent 发现 canonical 文档与代码不一致：不要在 book 中自行裁决，回到
  对应 RFC、devlog、register / current limitations 或源码核对。

如果多个 agent 同时修改同一章，必须指定一个 owner；另一个 agent 只做 review
或 source pass。

## Handoff Format

章节 agent 交付时，最少说明：

- 修改了哪些文件。
- 本章 thesis 是什么。
- 覆盖了哪些模块。
- 依赖了哪些 source。
- 新增或修改了哪些图源和导出物。
- 哪些事实仍需 Editor 或领域 owner 核对。
- 是否有 write set expansion 建议。
- 已运行的检查。

Reviewer 交付时，最少说明：

- 阻塞问题。
- 非阻塞建议。
- 事实核对缺口。
- 版面或 Typst 风险。
- 是否建议进入 editorial pass。

## Conflict Handling

常见冲突按以下方式收束：

- 定位冲突：回到 `positioning.md`。
- 章节职责冲突：回到 `outline.md`。
- 表达规则冲突：回到 `style.md`。
- 材料来源冲突：回到 `sources.md` 或 canonical 文档。
- 实现事实冲突：回到源码、RFC、devlog、register / current limitations。

不要为单个冲突新建长期协调文件。能沉淀为规则的，进入现有 meta 文件；不能沉淀的，
在对话或短期任务说明中解决。
