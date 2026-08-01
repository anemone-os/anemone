# 开发工作流

本文定义 Anemone 的开发分级、RFC 生命周期和实现反馈边界。目标不是让每项开发都留下完整过程档案，而是只记录无法从代码、测试和 Git 低成本恢复的决策，同时保护 owner、ABI、生命周期、current contract 和接受边界。

跨 RFC 长期生效的规则见[当前契约](./contracts.md)，可复制的最小形状见 [RFC 模板](./rfc-template.md)和[当前契约模板](./contract-template.md)。

## 核心原则

- 一个事实只有一个权威落点：代码和测试表达实际行为，current contract 表达已生效共享规则，RFC 表达 accepted target 与 delta，执行证据放在最接近实现的记录或 Git/PR 中，register 只表达当前开放问题和接受限制。
- 文档成本与不可逆性、跨 owner 风险和长期复用价值成比例，不按改动天数、文件数或 commit 数升级流程。
- 编码前先闭合用户可见 target、非目标、owner、handoff、failure、cleanup、ABI 和接受边界；类型、helper、文件布局和内部 API 在这些边界内由实现自然决定。
- 只有 contract cutover、ABI 发布、owner 迁移、高风险 probe、不安全中间态或明确人工授权点才需要正式 gate。普通 commit 不需要 resolution、activation、closure 三套动作。
- 验证证据只记录一次，并区分 agent 运行、用户运行和 Not Run；其它页面只链接，不复制整套矩阵。
- Git 保存文本和实现历史。Closed RFC、Completed transaction 与历史 change record 不因新规则批量改写。

## 三档开发分级

### Patch

Patch 是最小开发单位。它实现、恢复或机械落实已经能从 current contract、现有代码/测试或明确任务目标确定的行为，不创造新的语义决策。

Patch 可以跨多个直接相关文件，也可以增加回归测试、内部 import/re-export、模块注册、同 owner 新文件或自然的行为保持型模块拆分。它不按行数或文件数判断。

Patch 默认只产生代码、测试、验证和 Git/PR 证据，不建立 devlog、change record、transaction 或逐文件 write set。必要的关键注释和对现有 register/current limitation 的维护不属于额外过程文档。

出现以下任一情况时不再是 Patch：需要选择新的 ABI/errno/兼容策略，移动状态或协议 owner，改变 shared contract 或 acceptance boundary，引入无法局部证明的并发/生命周期规则，或需要 probe、多个语义 cutover、target renegotiation。

### 小迭代

小迭代用于保存值得长期追溯、但不需要 RFC 的局部决策或事实，例如不明显的根因、兼容取舍、局部 ABI 判断、可复用调查结论、小功能或一次原子 contract cutover。

默认产物只有一份自描述 change record：Problem/Context、Decision、Change、Validation、Remaining Risk/Links。`Tracking Issues`、`Architecture Friction`、背景目录和 `Contract Impact / Cutover` 都只在确有内容时出现。小迭代不再强制同步双周日志。

contract-bearing small change 只适用于 target 已完整解析、protocol/state owner、handoff、failure、cleanup 与验证明确，且代码和 contract 只有一个原子 cutover 的局部变化。它不需要逐文件清单；只要求 [Implementation Boundary](#implementation-boundary) 能在同一 checkpoint 闭合，失败时保持旧 contract。

如果需要 probe、transitional contract、多个语义 checkpoint、target renegotiation，或存在本轮无法关闭的 Apollyon/Keter，应升级 RFC。

### RFC

满足以下任一条件时使用 RFC：

- 尚未闭合 protocol/state owner、跨 owner handoff、failure 或 cleanup；
- 改变 shared contract、公共 ABI、外部可见语义或 acceptance boundary；
- 正确性依赖非平凡并发、生命周期、锁序或状态机证明；
- 需要 probe、target renegotiation、多个独立 cutover 或不安全中间态；
- 方案需要多轮共享 review，且结论会改变上述边界。

跨多个文件、跨多天或拥有多个普通 commit 本身不触发 RFC。多 agent 也不作为默认触发条件；仓库约定一个 branch/worktree 同时只推进一项开发，迭代完成后再 merge。

## Implementation Boundary

Implementation Boundary 是实现授权边界，取代默认逐文件 write set。它说明：

- 本轮 target 与 non-goals；
- owning subsystem，以及状态/协议 owner、handoff、failure 和 cleanup；
- 允许改变和必须保持不变的 public API、ABI、visible semantics 与 current contract IDs；
- acceptance、验证 claim 和真正需要停止的条件。

预计目录、模块或文件可以作为非穷举实现提示，但不是授权清单，也不需要在实现结束后回写成实际 diff。实际修改范围由 Git diff 审查；每一行仍必须直接服务本轮 target。

在 Implementation Boundary 内，agent 可以直接完成实现闭包，包括内部 import/re-export、模块注册、同 owner 新文件/模块、定向测试，以及保持行为与公共 surface 不变的同 owner 拆分。不得为了旧路径提示把逻辑塞进不自然文件，也不得顺手清理无关代码。

出现以下情况必须停止并报告，而不是先改后追认：

- 改变 target、non-goals 或 acceptance；
- 移动 protocol/state owner，改变 handoff、failure 或 cleanup；
- 扩大 public API、ABI、visibility contract 或 shared contract；
- 降低验证要求、隐藏失败或把不支持行为伪装为成功；
- 需要把无关问题纳入当前迭代；
- 当前工作实际需要升级为小迭代或 RFC。

若用户明确给出严格的逐文件写入限制，它作为本次任务的显式附加约束继续有效；这不是工作流默认值。

## Current contract 与 RFC target

`docs/src/contracts/` 只保存已经生效、会被多个 RFC/模块依赖的共享规则。RFC 保存 target 和实际 contract delta，不能在 cutover 前把目标写成当前事实。

`Contract Impact` 只列语义真实发生变化的 ID：`Introduce`、`Refine`、`Replace`、`Remove` 或 `Scoped Exception`。未变化规则放在 `Dependencies` 中链接，不登记 `Preserve` 流水，也不复制正文。

`Introduce` 只用于此前不存在的 effective ID。已有行为只是尚未提取时，应从 live owner、Closed RFC 和实现证据提取本次需要的最小 effective baseline，再按真实 delta 分类。不要批量整理整个领域。

accepted target 默认由 RFC 单向链接 current baseline；current contract 不为每个 pending RFC 维护反向链接。只有 cutover 达到验证与停止条件时，才原子更新受影响 ID、来源和生效证据。证据可以来自同一原子 change record/RFC closure、Git/PR 或按需 transaction，不要求 transaction 必然存在。

correctness invariant 约束唯一 owner、并发、生命周期、cleanup、内存安全和 ABI 诚实性，不能作为工程妥协项。target guarantee/capability 可以经 `Target Renegotiation` 形成新修订；类型、helper、内部模块和数据结构只是 implementation preference。

## RFC 最小形状

RFC 默认只有：

```text
docs/src/rfcs/<short-slug>/
  index.md
```

`index.md` 负责 target、non-goals、owner/handoff/failure/cleanup、ABI/visible semantics、contract delta、acceptance、validation、风险和停止边界。

只在出现真实需要时增加：

- `invariants.md`：非平凡 protocol、contract proof、锁序、生命周期或状态机；
- `implementation.md`：多阶段、不安全中间态、probe、多个 cutover 或需要长期保存的实施路线；
- `tracking-issues.md`：仍未解决且会影响实现、停止边界或 acceptance 的设计问题；
- `backgrounds/`：正文无法扫读的事实证据包、历史材料或被拒绝方案；
- transaction devlog：长期、多 checkpoint、多 cutover、probe/renegotiation 证据需要独立执行历史时。

resolved finding 折回 canonical target/implementation；普通 neutralized finding 的历史交给 Git/review，不要求永久保留在 tracker。Supporting pages 通过 RFC `index.md` 导航，不建立第二份状态总表。

## Git 历史与 RFC 修订

整个仓库 Git 保存物理文本历史。不要创建 per-RFC 仓库、`index-v1.md`、默认 amendment 或并列 canonical 副本。

Draft 修订写 `Draft`，第一次接受记为 `R0`。只有目标、非目标、target invariant、owner、ABI/visible semantics、contract delta 或 acceptance boundary 的已接受变化才递增 `R<n>`；措辞、证据、内部路线、文件布局和验证命令调整不递增。

RFC 状态使用 `Draft`、`Accepted`、`Review Hold`、`Closed`、`Superseded`。状态表达当前修订，不代替用户对当前任务的实现授权。Closed RFC 的新语义修订原地更新当前 target；只有确实需要独立长期执行历史时才建立新 transaction。核心目标、主要 owner、总体方案或大部分证明边界改变时，新建 follow-up RFC。

## 生命周期

### 私有草案

早期探索可以放在 gitignored 私有区域，允许快速重写或丢弃。公共文档、register 和 devlog 不把私人路径作为稳定引用。

### 公共 RFC 与 review

方案进入共享决策或需要公共长期引用时，提升到 `docs/src/rfcs/<short-slug>/`。公共 RFC 立即成为提案/accepted target 的 canonical source，但不会覆盖 current contract。

公共入口只同步必要导航：`docs/src/rfcs.md`、`docs/src/SUMMARY.md` 和 RFC 内链接。导航只提供链接与简短范围，不复制阶段、验证和问题状态。

文档层 review 检查 target 自洽、owner、ABI、并发、failure、cleanup、observability、acceptance 和 contract delta。Apollyon/Keter 在接受前必须 neutralize，或明确成为实施中的硬停止条件。Euclid 可以带入实现并在收口摩擦扫描中复核；Safe 默认不记录。

### 实现、checkpoint 与 stage

普通 RFC 实现不必建立 transaction，也不必给每个 commit 维护 Ready/Active/Closed。Git/PR 与 RFC 的最终 closure 足以保存单次实现证据。

只有当一个中间状态独立安全、需要独立 cutover/probe/review，或用户明确要求 checkpoint/stage 时，才把它写成正式 gate。每个 gate 说明目的、前置依赖、受保护边界、验证、退出和停止条件；未来阶段只需概括目的、依赖和受保护边界，不提前冻结类型、算法、文件列表或精确命令。

如果用户只授权某个 checkpoint/stage，完成后必须停止，不得因为后续计划已存在而自动进入下一 gate。

transaction devlog 是按需执行记录。使用时只保存 checkpoint、review、验证、cutover、更正、target renegotiation 和 handoff 事实，不复制 RFC target 或实施计划。加入 transaction index/SUMMARY 只是导航，不要求再同步双周日志。

### Probe / vertical slice

高风险假设可以先做最小 probe。计划写在按需 `implementation.md` 中，至少说明 Hypothesis、Protected Boundary、Non-goals、Validation、Failure Signals、Write-back 和 Exit；不需要 `Minimum Write Set`。

probe 代码不能因“已经能跑”自然沉淀。长期保留前必须把证据折回 RFC，接受相应 target/contract delta，并完成 cutover。不要新建通用 `feedback.md`、`probe.md`、`experiments.md` 或并列计划家族。

## 实现反馈与 Target Renegotiation

保持 accepted target 和 Implementation Boundary 的路线修正可由 agent 直接完成。若证据要求改变 target invariant、owner、ABI、visible semantics、contract delta、acceptance 或验证 claim，必须在 cutover/完成声明前停止。

Target Renegotiation 至少记录真实成本/失败证据、已完成 slice 与代码处置、受影响 target/contract/acceptance、correctness invariants，以及可比较的处理路线。review 只形成：

- `Route Correction`：保持 target，调整实现路线；
- `Accepted Reduced Target`：接受较弱但独立有用、ABI 诚实的新修订；
- `Follow-up RFC`：核心目标、owner 或证明边界已经改变；
- `Not Cut Over`：部分实现不能形成安全、诚实、独立能力。

agent 可以提案，不能批准自己的 reduced target。新修订接受并完成对应 cutover 前，不能把更弱行为写成当前事实、限制或原 target closure。

## 架构摩擦扫描

每次 Patch、小迭代、checkpoint、stage 或 RFC 实现收口前，agent 必须在内部扫描本轮是否暴露架构摩擦。检查至少包括：第二份状态真相或 stale 派生状态、owner 穿透或私有表示泄漏、为局部需求扩大 public API、调用者/架构/测试特判、无退出条件的临时桥或双路径、failure/cancellation/cleanup 依赖隐含顺序、无真实义务的新抽象层，以及通过降低 oracle/validation/ABI 诚实性换取“跑通”。

摩擦必须有具体代码路径、状态/owner/lifecycle 模型或已接受下一步作为证据。“不够优雅”、文件较长、普通 import/module 注册、编译错误、工具链/验证环境问题和未被本轮恶化的相邻技术债不构成报告。

- 没有具体摩擦，或只剩 Safe：不输出“无摩擦”占位结论。
- Euclid：可以完成；若没有在当前边界内消除，收口时简短报告证据、模型偏差、影响和最小修正方向。
- Keter/Apollyon：立即停止，不得声明 checkpoint/stage 完成或执行 cutover；报告当前 diff/代码处置和需要的 owner/RFC/target 决策。

Patch 中发现需要长期保留的摩擦，是升级为小迭代的信号；小迭代中发现未决 owner/contract/protocol 摩擦，是升级 RFC 的信号。只有确有摩擦时才在 change record、RFC closure 或按需 transaction 中加入 `Architecture Friction`；不要建立 `friction.md` 或全局摩擦台账。

## 收口与证据

收口只更新真实受影响的权威面：

- 代码、测试和实际验证；
- RFC 当前修订与 closure（如果存在）；
- 实际发生语义变化的 contract IDs 与 cutover 来源；
- 仍开放的问题/接受限制；
- 按需 change record 或 transaction；
- 有证据的架构摩擦。

不要为了形式同时更新 RFC、transaction、双周日志和多个状态索引。导航页只维护可达链接，不复制完成度、验证矩阵或剩余问题。

## 外部源码证据

公共 RFC、change record、transaction 或背景材料引用外部源码时，遵循[外部源码引用规则](./external-source-references.md)。长期公共参考使用 tracked `xref/sources.toml` 的固定 source ID/commit；私人 checkout 路径、branch 和 `HEAD` 不能成为公共 citation authority。外部源码只证明对应快照事实，不能替代 Anemone target、current contract、live source 或执行验证。

## Artifact 边界

| Artifact | 职责 | 默认性 |
| --- | --- | --- |
| Patch | 已确定语义的实现、测试和验证 | 默认最小单位；无过程文档 |
| Small change record | 局部决策、调查结论、原子 cutover 与证据 | 仅有长期追溯价值时 |
| Current contract | 已生效的跨 RFC/模块共享规则 | 按真实复用/变化提取 |
| RFC `index.md` | accepted target、delta、边界、acceptance 与 closure | RFC 唯一默认文件 |
| `invariants.md` | 非平凡 target/contract proof obligations | 按需 |
| `implementation.md` | 多阶段/probe/cutover 实施路线 | 按需 |
| `tracking-issues.md` | 仍影响实现或 acceptance 的设计问题 | 按需；不保存普通历史 |
| Transaction devlog | 长期、多 checkpoint/cutover 的执行证据 | 按需 |
| 双周 devlog | 人工选择的时间线摘要 | 可选；不是 workflow gate |
| Register/limitations | 当前开放问题和接受限制 | 只维护当前项 |

## 新旧规则边界

本规则适用于新任务，以及活跃 RFC 的下一个尚未开始 gate。既有 RFC、Completed transaction、历史 manifest、change record 和 devlog 均作为 legacy history 保留，不补写、不重排、不批量迁移。

Agent 处理具体任务时先读取 live source、current contracts、活动 RFC/记录和 register；历史文档只作为来源证据。若用户显式给出 checkpoint、停止条件或更窄写入限制，以该任务约束为准。
