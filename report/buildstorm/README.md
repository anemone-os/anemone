# Anemone BuildStorm 内核设计与优化报告

> 当前状态：Editorial outline。确认本大纲后，正文直接在本文件中扩写，不另建并列的长期规划文档。

## 写作约束

- 面向竞赛评委，成果与系统价值优先，避免展开代码级实现和内部工作流术语。
- 主文围绕“发现了什么问题、采用了什么设计、获得了什么收益”组织。
- 关键结论使用图、表和少量路径示意表达；完整样本、commit 和复现身份下沉到附录。
- 微负载、Cargo 构建与 BuildStorm 分别承担机制、候选确认与最终端到端结论，不混合不同实验口径的百分比。
- 主文目标约 12–16 页；研究附录不计入主叙事篇幅。

## 摘要

- **核心主张：** Anemone 面向真实 Rust 软件构建负载，建立了从原生观测、瓶颈归因、候选验证到正式实现验收的完整性能优化闭环。
- **主要内容：** BuildStorm 运行结果、三组代表性优化、性能工程能力和总体收益。
- **视觉对象：** 复用最终结果表 `T04` 的核心行；正文完成后反写本节，不维护第二份数据。
- **篇幅：** 约 1 页。

## 1. BuildStorm：从复杂软件构建到内核综合负载

### 1.1 赛题与工作负载

- **核心主张：** BuildStorm 不是单一 syscall 测试，而是同时覆盖动态链接、进程线程、虚拟内存、文件系统和多核调度的真实复杂软件构建负载。
- **主要内容：** 离线 Rust 构建链、全量 crate 编译、产物校验和八核运行环境。
- **视觉对象：** 可选构建负载图 `D01`。
- **篇幅：** 约 0.5 页。

### 1.2 Anemone 的运行基础

- **核心主张：** 完整运行 BuildStorm 建立在 Anemone 已有的 Linux ABI、glibc、VFS、MM、进程和多核能力之上，性能优化不以削弱正确性或减少构建工作量为代价。
- **主要内容：** 只概括与复杂构建直接相关的系统能力，不重复内核技术报告。
- **视觉对象：** 不单独制图；若采用 `D01`，在其中标出相关内核能力域。
- **篇幅：** 约 0.5 页。

### 1.3 基线与主要瓶颈

- **核心主张：** 先建立可重复、结果正确的构建基线，再由系统级和 owner-local 观测逐层缩小热点范围。
- **主要内容：** 用户态与内核态占比、syscall/缺页/文件系统等顶层归因，以及最终选出的优化方向。
- **视觉对象：** 复用性能工程闭环图 `D02` 的前半段表达归因漏斗。
- **篇幅：** 约 1 页。

## 2. Anemone 性能工程方法

### 2.1 从宏观负载到具体机制

- **核心主张：** 微负载回答“为什么”，Cargo 构建回答“方向是否成立”，BuildStorm 回答“最终系统是否更快”。
- **主要内容：** hypothesis、定向负载、counterfactual、独立 confirmation 和 production acceptance 的最小流程。
- **视觉对象：** 必需的性能工程闭环图 `D02`。
- **篇幅：** 约 0.5 页。

### 2.2 内核原生性能观测能力

- **核心主张：** Anemone 使用可发现、可聚合、默认关闭的原生性能观测数据面，而不是依赖散落的临时日志猜测热点。
- **主要内容：** Counter、Histogram、Elapsed、per-CPU 聚合、native `perf_observe` 和 syscall profiling；不展开 wire layout 与内部类型。
- **视觉对象：** 复用 `D02` 中的 native observation 数据通路，不再单独制图。
- **篇幅：** 约 0.75 页。

### 2.3 `perfctl` 与 `anemone-bench`

- **核心主张：** `perfctl` 提供统一控制与结果读取，`anemone-bench` 提供能够隔离线程、虚拟内存和 libc 机制的可重复微负载。
- **主要内容：** 简短命令示例，以及 pthread、VM 和 CPU control 如何服务 TLB 等研究。
- **视觉对象：** 工具、负载与结论强度表 `T01`。
- **篇幅：** 约 0.75 页。

### 2.4 ABBA 与优化接受标准

- **核心主张：** 主要性能结论来自 recording-disabled 的 fresh-boot 交错对照；enabled metrics 只解释机制。
- **主要内容：** `A1 -> B1 -> B2 -> A2`、正确性 oracle、构建产物校验和候选采用门槛。
- **视觉对象：** 复用 `D02` 的 confirmation 阶段，不再单独绘制 ABBA 时间线。
- **篇幅：** 约 0.5 页。

## 3. 关键优化

### 3.1 基于范围与转换语义的 TLB 优化

- **核心主张：** Anemone 根据失效范围和页表转换语义消除无必要的 TLB flush，并通过多核 residency 协议保护 destructive mapping。
- **主要数据：**
  - kernel-stack workload 的正式 range-invalidation 策略改善约 14.8%–16.7%；
  - user-fault candidate 的 Cargo same-boot 中位数改善 32.93%；
  - 两组结果来自独立实验，不相加为总体收益。
- **视觉对象：** TLB 机制对比图 `D03`；TLB 独立实验结果图 `P01`。
- **篇幅：** 约 3 页。

#### 3.1.1 内核栈批量 TLB 失效

- **问题：** 大内核栈映射和回收触发大量逐页 local invalidation。
- **设计：** 小范围逐页处理，大范围由 architecture-owned policy 选择单次 full local flush；不通过缩小内核栈换取收益。
- **结果：** 展示 owner interval、完整 workload 和策略计数守恒，不展开具体阈值实现。

#### 3.1.2 用户缺页的延迟 local completion

- **问题：** 新增用户页后无条件 eager invalidation，在构建负载中形成显著重复成本。
- **设计：** 新映射返回用户态后允许硬件直接重试；替换、权限收紧和内核立即访问仍同步完成 TLB 失效。
- **结果：** 展示 Cargo、boot-first 和 direct `rustc` 的核心对比，省略完整 transition enum 与 caller audit。

#### 3.1.3 多核 residency 与 destructive completion

- **作用：** 说明正式多核内核如何选择真正可能持有旧 translation 的 CPU，并在释放旧页表或物理页前完成同步失效。
- **表达边界：** 作为 correctness 与 scalability closure，不单独包装为未经测量的性能数字。

### 3.2 Positive dentry residency

- **核心主张：** Anemone 通过保留高价值 positive dentry，显著减少真实编译负载中的重复 pathname backend lookup。
- **主要数据：** production Cargo 改善 8.48%，direct `rustc` 同向改善 11.21%。
- **主要内容：** 从 namei 归因、容量反事实到 production 默认策略；突出正式实现没有直接照搬研究用大容量。
- **视觉对象：** 两项 production 优化结果图 `P02` 的 dentry 面板。
- **篇幅：** 约 2 页。

### 3.3 Page-bounded C-string copy

- **核心主张：** Anemone 在保持跨页 fault 和 NUL 边界语义的前提下，将 pathname 等 C-string 读取由逐字节访问改为页内批量复制。
- **主要数据：** production Cargo 改善 3.71%。
- **主要内容：** 逐字节 user-access window 热点、page-bounded batch 和 256-byte production cap；不展开所有参数探索过程。
- **视觉对象：** 两项 production 优化结果图 `P02` 的 C-string 面板；机制由正文中的简短 before/after 路径表达，不单独制图。
- **篇幅：** 约 1.5–2 页。

### 3.4 其它优化与候选筛选

- **核心主张：** Anemone 只将同时通过机制、正确性和端到端收益门槛的候选纳入正式内核。
- **主要内容：**
  - 简述 Signal empty-return fast path 等其它已采用优化；
  - 用 page-table range walker 和 userptr wide access 说明内部工作量下降不自动等于系统收益。
- **视觉对象：** 候选筛选与处置表 `T02`。
- **篇幅：** 约 1 页；详细材料进入研究附录。

## 4. BuildStorm 最终结果

### 4.1 实验环境

- **核心主张：** 最终结果使用赛方 BuildStorm 工作负载、八核配置和完整产物 oracle。
- **主要内容：** baseline/candidate commit、架构、内存、命令和统计口径；具体身份链接到 evidence。
- **视觉对象：** 最终实验环境表 `T03`。
- **篇幅：** 约 0.5 页。

### 4.2 RV64 结果

- **主要数据：** 待最终 BuildStorm 验收后填写运行成功、耗时、加速比和稳定性结果。
- **视觉对象：** 最终 BuildStorm 结果图 `P03` 的 RV64 面板；精确数字进入 `T04`。

### 4.3 LA64 结果

- **主要数据：** 待最终 BuildStorm 验收后填写运行成功、耗时、加速比和稳定性结果。
- **视觉对象：** 最终 BuildStorm 结果图 `P03` 的 LA64 面板；精确数字进入 `T04`。

### 4.4 总体收益分析

- **核心主张：** 用最终累计 BuildStorm 结果承担赛题结论，不把不同 profile 下的局部百分比机械相加。
- **主要内容：** 双架构结果、Linux 基线/赛方计分关系和关键优化对总体负载的作用。
- **视觉对象：** 汇总 `P03` 与 `T04`，不增加新的总体排名图。
- **篇幅：** 第 4 章合计约 1.5–2 页。

## 5. AI 使用与可复现性

### 5.1 AI 在开发中的作用

- **核心主张：** AI 用于材料检索、候选生成、代码审查、实验编排和文档整理；设计接受、边界判断与结果裁决由开发者负责。
- **主要内容：** 选取一两个具体工作流示例，不复制完整对话。
- **篇幅：** 约 0.5 页。

### 5.2 复现步骤与版本身份

- **核心主张：** 每项正式结论都能回到 baseline/candidate commit、运行配置、命令、oracle 和结果摘要。
- **主要内容：** 给出最短复现流程；完整身份进入 [复现与结果证据](./evidence/)。
- **视觉对象：** 结论与复现身份索引表 `T05`。
- **篇幅：** 约 0.5 页。

## 6. 总结

- **核心主张：** Anemone 不仅完成了 BuildStorm 复杂软件构建，也形成了能够持续定位、验证和交付内核优化的性能工程能力。
- **主要内容：** 三组代表性优化、最终 BuildStorm 收益和工程方法；正文完成后反写本节。
- **篇幅：** 约 0.5 页。

## Figure inventory

### 发布与数据规则

- draw.io 图保留可编辑的 `.drawio` 源文件，正文引用同名 SVG 发布物。
- Python 统计图保留绘图脚本，统一从 `evidence/results.csv` 读取公开、中性的结果数据，正文引用 SVG 发布物。
- baseline 使用中性灰，candidate 使用统一的 Anemone 主色；accepted 与 screened-out 只作为辅助状态色。
- 小样本优先显示原始点、范围和中位数；只有适合从零比较的量才使用柱状图。
- 不使用 3D、饼图、双纵轴或跨 profile 排名；所有图直接标注单位与关键数值。
- 预览 PNG 只用于生成过程中的视觉检查，默认不作为正文 publication artifact。

### Draw.io diagrams

| ID | 状态 | 章节 | 内容与目的 | 预计文件 |
| --- | --- | --- | --- | --- |
| `D01` | Optional | 1.1–1.2 | BuildStorm 简化构建链及其涉及的进程、MM、VFS 与调度能力；只有开篇文字不足以快速建立负载印象时采用 | `assets/diagrams/buildstorm-workload.drawio` / `.svg` |
| `D02` | Required | 1.3、2.1–2.4 | 将宏观负载、顶层归因、native observation、`perfctl`、`anemone-bench`、counterfactual、ABBA 与 production acceptance 串成一张性能工程闭环图 | `assets/diagrams/performance-workflow.drawio` / `.svg` |
| `D03` | Required | 3.1 | 双面板表达 kernel-stack 逐页失效到 range policy，以及 user-fault eager completion 到 additive mapping 延迟 completion | `assets/diagrams/tlb-optimization.drawio` / `.svg` |

### Python plots

| ID | 状态 | 章节 | 内容与目的 | 数据与预计文件 |
| --- | --- | --- | --- | --- |
| `P01` | Required / data available | 3.1 | 两个独立子图分别展示 kernel-stack production range-invalidation 结果和 user-fault Cargo ABBA；不得共用坐标轴或累加百分比 | `evidence/results.csv`；`assets/plots/tlb-results.py` / `.svg` |
| `P02` | Required / data available | 3.2–3.3 | 两个独立子图展示 positive dentry 与 C-string production A/B；明确各自 baseline，不制作优化收益排行榜 | `evidence/results.csv`；`assets/plots/production-optimizations.py` / `.svg` |
| `P03` | Required / final data pending | 4.2–4.4 | 分 RV64、LA64 展示 BuildStorm baseline 与 final；有重复样本时显示原始点、范围和中位数，只有单次正式结果时退化为点图 | `evidence/results.csv`；`assets/plots/buildstorm-results.py` / `.svg` |

### Tables

| ID | 状态 | 章节 | 内容与目的 | 权威数据落点 |
| --- | --- | --- | --- | --- |
| `T01` | Required | 2.3 | `anemone-bench`、Cargo 与 BuildStorm 的负载层级、用途和能够承担的结论 | 正文内表格 |
| `T02` | Required | 3.4 | Signal fast path、page-table walker、userptr wide access 等候选的机制结果、端到端结果与最终处置 | 公开研究附录的结论摘要 |
| `T03` | Required / final identity pending | 4.1 | BuildStorm baseline/final commit、架构、核数、内存、workload、统计口径与 oracle | `evidence/` 中的最终复现身份 |
| `T04` | Required / final data pending | 摘要、4.2–4.4 | RV64/LA64 构建成功、精确耗时、加速比和结果范围；摘要只复用其核心行 | `evidence/results.csv` |
| `T05` | Required | 5.2 | 每项正式结论到 baseline/candidate commit、命令、配置、oracle 和证据入口的索引 | `evidence/` 中围绕具体结论组织的复现记录 |

### 预计素材布局

```text
assets/
├── diagrams/
│   ├── *.drawio
│   └── *.svg
└── plots/
    ├── *.py
    └── *.svg
evidence/
└── results.csv
```

## 附录

- [研究附录](./appendices/)：公开、可独立阅读的代表性研究与候选筛选材料。
- [复现与结果证据](./evidence/)：围绕具体结论组织的 commit、配置、命令、oracle 与结果摘要。
