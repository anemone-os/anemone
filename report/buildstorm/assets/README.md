# 图表清单（Figure Inventory）

本清单管理 BuildStorm 优化报告正文中的视觉对象。它只决定图表是否进入正文、承担什么结论以及如何复现，不保存第二份实验数据。统计图的数值统一来自 [`../evidence/results.csv`](../evidence/results.csv)，结构图的可编辑源统一为 Draw.io 文件。

状态约定：

- **正文已采用**：源文件、发布用 SVG 与正文引用均已存在；
- **不制作**：已评估但当前正文不需要，避免重复表达或增加阅读负担。

## Draw.io 结构图

| ID | 状态 | 正文位置 | 视觉内容与承担的结论 | 交付文件 |
| --- | --- | --- | --- | --- |
| `D01` | 不制作 | 第 1 章 | 原计划展示 BuildStorm 构建链及其涉及的进程、MM、VFS 与调度能力。现有文字和能力列表已经足以建立负载印象，不再增加一张开篇总览图。 | — |
| `D02` | 正文已采用 | 第 2 章开头 | 展示从完整编译定位开销、用定向测试验证原因、执行四次独立启动交错对照，再决定是否进入正式内核的全过程。 | [`performance-workflow.drawio`](./diagrams/performance-workflow.drawio) / [`performance-workflow.svg`](./diagrams/performance-workflow.svg) / [`performance-workflow.png`](./diagrams/performance-workflow.png) |
| `D03` | 正文已采用 | 第 3.1 节 | 双面板对比内核栈大范围刷新与新增用户页映射两项 TLB 优化，明确展示优化前后的动作和正确性条件。 | [`tlb-optimization.drawio`](./diagrams/tlb-optimization.drawio) / [`tlb-optimization.svg`](./diagrams/tlb-optimization.svg) / [`tlb-optimization.png`](./diagrams/tlb-optimization.png) |

## Python 统计图

| ID | 状态 | 正文位置 | 图形与承担的结论 | 数据、脚本与交付文件 |
| --- | --- | --- | --- | --- |
| `P01` | 正文已采用 | 第 3.1 节末尾 | 两个独立面板展示内核栈刷新和新增用户页映射优化的对照结果。面板使用各自单位和纵轴，不合并不同测试负载的百分比。 | [`results.csv`](../evidence/results.csv) / [`tlb-results.py`](./plots/tlb-results.py) / [`tlb-results.svg`](./plots/tlb-results.svg) |
| `P02` | 正文已采用 | 第 3.3 节末尾 | 展示目录项复用与用户字符串批量读取的全部 Cargo 样本和每次启动中位数，避免只展示单个最好结果。 | [`results.csv`](../evidence/results.csv) / [`production-optimizations.py`](./plots/production-optimizations.py) / [`production-optimizations.svg`](./plots/production-optimizations.svg) |
| `P03` | 不制作 | 第 4 章 | 每个架构只有一次有效 BuildStorm 结果，精确耗时和产物信息用紧凑表格表达更清楚；LA64 完整日志只作为成功跑完的支持性证据，不单独形成视觉叙事。 | — |

## 正文表格

表格承担精确映射和边界说明，不再为相同信息制作额外示意图。

| ID | 状态 | 正文位置 | 内容与作用 | 权威落点 |
| --- | --- | --- | --- | --- |
| `T01` | 正文已采用 | 摘要 | 四项代表性优化的验证负载、证据层级、核心结果和正式处置。 | 正文摘要；具体身份由复现索引拥有 |
| `T02` | 正文已采用 | 第 2.1 节 | `anemone-bench`、Cargo、BuildStorm 与 native observation 分别能够承担和不能承担的结论。 | 正文表格 |
| `T03` | 正文已采用 | 第 3.4 节 | 已采用与已归档候选的机制结果、端到端结果和最终处置。 | 正文表格；详细材料见研究附录 |
| `T04` | 正文已采用 | 第 4 章 | 主结果的源码、赛题版本、内核配置、八核/8 GiB 参数、计时范围与成功条件。 | [`../evidence/reproducibility.md`](../evidence/reproducibility.md) |
| `T05` | 正文已采用 | 摘要、第 4 章 | RV64 主结果与 LA64 完整运行支持记录的构建状态、虚拟机内计时、产物大小与样本数；摘要只复用主结果。 | [`../evidence/results.csv`](../evidence/results.csv) 与复现记录 |

## 明确不新增的图

- 不为 native perf/debug 单独制作组件架构图：`D02` 已经表达它在整体闭环中的位置，正文表格负责能力边界。
- 不为 ABBA 单独制作时间线：`D02` 与正文顺序已经足够清楚。
- 不为 C-string 单独制作代码级机制图：其 page-bounded、NUL、fault 和长度边界用短段落更易读。
- 不为最终 BuildStorm 制作单次样本统计图：`T04` 与 `T05` 直接给出配置、耗时和产物证据。
- 不制作局部优化收益排行榜、饼图、3D 图或双纵轴图；不同测试负载不直接排名或合并。
- 不为已归档候选制作统计图：`T03` 足以表达“机制改善不自动等于端到端收益”。

## 生成与发布规则

- Draw.io 图保留 `.drawio` 源、light-theme `.svg` 发布件和供 Markdown/PDF 稳定渲染的 `.png` 发布件。
- Python 图保留 `.py` 脚本与 `.svg` 发布件，脚本只能读取公开的 `results.csv`，不得依赖外部环境路径。
- PNG 用于 Markdown 正文和生成过程中的视觉检查；SVG 保留为可缩放版本，`.drawio` 是可编辑源文件。
- 小样本优先显示原始点、中位数和必要的范围；只有适合从零比较的量才使用柱状图。
- 图题或替代文本必须说明测试负载、单位和关键比较范围，不能让局部百分比看起来像最终 BuildStorm 加速比。
- 只有通过产物检查的有效运行才能进入 `T04` 和 `T05`；后续新增样本时同步更新数据与样本数，不回填占位数值。
