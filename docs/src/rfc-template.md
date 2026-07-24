# RFC 模板

创建、review、提升和实现期记录的完整流程见 [RFC 工作流](./rfc-workflow.md)。跨 RFC 已生效规则见 [当前契约](./contracts.md)，契约文档形状见 [当前契约模板](./contract-template.md)。外部源码证据使用[公共引用规则](./external-source-references.md)，不得链接私人 checkout 路径。本页只定义 RFC 目录结构和单页内容模板。

RFC 默认使用目录结构，而不是单个 Markdown 文件。

目录名使用稳定 slug：

```text
docs/src/rfcs/<short-slug>/
```

最小结构：

```text
docs/src/rfcs/<short-slug>/
  index.md
  implementation.md
  invariants.md          # 可选
  tracking-issues.md     # 可选
  backgrounds/           # 可选
    index.md
```

`index.md` 与 `implementation.md` 必须存在。`invariants.md` 在 RFC 改变共享 contract，或协议、不变量、锁序、生命周期、迁移证明义务复杂时创建；它保存 contract impact、target invariants 和 RFC-local proof obligations，不维护整个领域的 current contract。`tracking-issues.md` 在 design review 或实现反馈发现会影响实现顺序、review gate、停止边界或验收判断的设计问题时创建。`backgrounds/` 用于历史背景、旧问题清单、被拒绝方案和旧计划归档；背景材料不能覆盖 accepted target 或 current contract。

RFC 的历史版本由整个仓库的 Git 保存，不创建独立仓库或 `index-v1.md` 一类并列副本。页首 `修订` 使用单调的 `R0`、`R1`、`R2`：Draft 写 `Draft`，第一次 accepted target 记为 `R0`，之后只有 accepted semantics 变化才递增。拼写、链接、措辞、证据补充和不改变 target 的实现计划调整不递增修订。Current contract 不使用 RFC 修订号；它只在实际 cutover 时原地更新 effective 规则和来源。

RFC 一旦进入实现阶段，必须创建对应事务日志：

```text
docs/src/devlog/transactions/YYYY-MM-DD-<short-slug>.md
```

并在 RFC `index.md`、事务日志索引、当前双周 devlog 和 mdBook Summary 中建立链接。

## `index.md`

```md
# RFC-YYYYMMDD-short-slug

**状态：** Draft / Accepted for Implementation / Superseded / Closed
**修订：** Draft / R0 / R1 / ...
**负责人：** name1, name2
**最后更新：** YYYY-MM-DD
**领域：** scheduler / fs / mm / ...
**事务日志：** Draft 阶段可写 `None`；进入实现阶段后必须链接对应 transaction；存在后续修订时列出各修订事务。
**影响契约：** contract IDs 与链接；没有则写 `None`。
**开放问题：** 简短列出当前 blocker、可带入实现的 gated item 或待决问题；没有则写 `None`。
**下一步：** 下一次 review、probe / vertical slice、迁移阶段、验证或收口动作。

## 摘要

用一到两段说明问题是什么，以及 RFC 提议的方向。

## 背景

记录当前实现状态、已观察到的失败模式、已有约束，以及为什么现在需要共享评审。

## 目标

- 目标 1。
- 目标 2。

## 非目标

- 明确排除的范围 1。
- 明确排除的范围 2。

## 文档地图

RFC target：

- [目标与不变量](./invariants.md)
- [迁移实施计划](./implementation.md)
- [Tracking Issues](./tracking-issues.md)（如果存在）

Current contracts：

- [CONTRACT-ID](../../contracts/<owner>/<surface>.md#contract-id)：当前 effective 规则；没有则写 `None`。

背景材料：

- [背景材料索引](./backgrounds/index.md)

公共外部源码证据（如果使用；详细 citation 放在所支持的 claim 附近）：

- `xref:<source-id>:<repo-relative-path>#<locator>`，或 `None`。

## 修订记录

只记录已经接受的语义版本。普通文字修正和未接受的提案不增加一行；旧文本通过 Git 历史恢复，不在 RFC 目录复制快照。

| 修订 | 日期 | 状态 | 语义变化 | Review / 事务 |
| --- | --- | --- | --- | --- |
| R0 | YYYY-MM-DD | Closed | 初始 accepted target；列出 contract delta。 | [初始事务](../../devlog/transactions/YYYY-MM-DD-short-slug.md) |
| R1 | YYYY-MM-DD | Accepted for Implementation / Closed | target invariant / owner / ABI / 接受边界与 contract delta 变化摘要。 | [R1 事务](../../devlog/transactions/YYYY-MM-DD-short-slug-r1.md)，或 docs-only review / 小迭代记录。 |

页首 `状态` 始终描述当前修订。后续修订已接受但仍需实现时，状态写 `Accepted for Implementation`；该修订验证收口后再写 `Closed`。旧修订的状态保留在本表和对应事务中。

## 方案

概括最终方向。如果细节很多，这里保持短摘要，并链接到 target 子文档和 current contract IDs。

## 接受边界

说明本 RFC 被接受意味着什么，哪些内容仍需审查，以及哪些变化必须回到本 RFC、`Target Renegotiation Gate` 或 follow-up RFC。明确 acceptance 只形成 target 还是同时完成 docs-only cutover；区分 correctness invariant、target guarantee / capability 和 implementation preference。若允许带着不确定性进入实现，必须列出对应验证点、停止条件和 RFC / contract 回写路径。

## 备选方案

记录考虑过的合理备选方案，以及为什么拒绝或延期。

## 风险

- 风险 1 及控制方式。
- 风险 2 及控制方式。

## 收口

完成后记录最终验证、剩余限制，以及 register / devlog 链接。
```

## `tracking-issues.md`

```md
# <标题> Tracking Issues

**状态：** Active / Closed
**最后更新：** YYYY-MM-DD
**父 RFC：** [RFC-YYYYMMDD-short-slug](./index.md)
**事务日志：** [YYYY-MM-DD-short-slug](../../devlog/transactions/YYYY-MM-DD-short-slug.md)

本文只跟踪当前仍影响实现顺序、review gate、停止边界或验收判断的设计问题。问题可以来自文档层 review，也可以来自实现期反馈；普通实现进度、TODO、历史问题清单和旧 review 材料不放在这里，历史材料放在 `backgrounds/`。

## Apollyon

- 当前必须修复的错误结果、数据损坏、安全问题、崩溃或严重不可恢复状态。

## Keter

- 当前必须修复的架构方向、状态所有权、边界或后续开发阻塞问题。

## Euclid

- 通常值得修，但不阻塞主线的问题。

## Safe

- 记录即可，默认不修的问题。

## Neutralized

- 已处理完成的问题、neutralize 依据和对应事务日志条目。
```

## `invariants.md`

```md
# <标题> 目标与不变量

**状态：** Draft / Accepted Target / Superseded
**最后更新：** YYYY-MM-DD
**父 RFC：** [RFC-YYYYMMDD-short-slug](./index.md)
**适用修订：** Draft / R0 / R1 / ...

本文定义本 RFC 的 contract delta、尚未 cutover 的 target invariants，以及只服务本方案或迁移的 proof obligations。当前已经生效的共享规则以 `docs/src/contracts/` 中的稳定 ID 为准。

## 规则分类

- **Correctness Invariant：** 状态唯一 owner、并发、生命周期、cleanup、内存安全和 ABI 诚实性等“违反即不正确”的规则；不得作为工程降级项接受。
- **Target Guarantee / Capability：** 当前修订承诺的功能范围、兼容覆盖、原子性或性能边界；在新修订接受前保持约束力，只能通过 `Target Renegotiation Gate` 调整。
- **Implementation Preference：** 类型、helper、内部模块、算法或数据结构偏好；不写成 invariant，在保持 target 时由滚动阶段解析决定。

## Contract Impact

| Contract ID | 变化 | 当前规则 | Target 摘要 | 生效 Gate |
| --- | --- | --- | --- | --- |
| JOBCTL-STATE-001 | Introduce | None（尚未生效） | 新增 ThreadGroup job-control phase | Gate 3 |
| SCHED-PICK-001 | Replace | [当前规则](../../contracts/scheduler/pick-request.md#sched-pick-001) | 新规则摘要 | Gate 3 |
| WAIT-WAKE-004 | Preserve | [当前规则](../../contracts/wait-core/wake-publication.md#wait-wake-004) | 不变 | 全程 |

变化类型只使用 `Introduce`、`Preserve`、`Refine`、`Replace`、`Remove` 或 `Scoped Exception`。`Introduce` 只用于此前没有 effective 规则的新 ID，当前规则栏写 `None（尚未生效）`，cutover 时才创建 current contract；已有行为尚未文档化时，应先提取 baseline，不能借 `Introduce` 跳过。Preserve 项不复制 contract 正文；Draft 与 accepted-but-not-effective 阶段不能把 target 写成当前规则。

如果本 RFC 第一次跨 RFC 触及尚未提取的既有不变量，先从 live owner、Closed RFC 和执行证据提取最小 contract 闭包；不批量整理整个领域，也不逐份回改旧 RFC。

## Target Invariants

### TARGET-001 — 简短标题

**规则：** 本 RFC 接受后、cutover 前仍只是 target 的规范性规则。

**Owner：** 唯一状态或协议 owner。

**依赖：** contract IDs 或本 RFC 的其它 target。

**违反表现：** 会导致的错误、双重真相源、ABI 偏差或不可闭合路径。

**Cutover：** 在哪个 gate、以什么验证证明后写入 current contract；若只属于 RFC-local 则写 `N/A`。

## RFC-local Invariants

- 只服务本方案、probe、迁移桥、阶段原子性或验收的规则。
- 临时规则必须写明保留原因、可见边界和删除 gate，不能自然沉淀为长期 contract。

## 非目标

- 非目标 1。
- 非目标 2。

## 状态所有权

定义单一真相源，以及每个状态转换由哪个子系统拥有。

## 身份与能力模型

定义稳定身份、token、guard、permit 或其他 capability，并说明哪些比较有效、哪些比较禁止。

## 线性化点

定义外部可见状态变化在哪个事务或锁边界上成立。

## 锁序与生命周期规则

定义锁序、引用所有权、cleanup 责任和 teardown 行为。

## 禁止退化项

- 会破坏证明的模式。
- 会制造第二套真相源的模式。

## 完成标准

- 声明 RFC target 闭合的标准。
- 列出每个 contract ID 的 cutover / pending / Not Cut Over 结果。
- 声明只能作为迁移中间态、不得进入 current contract 的规则。
```

## `implementation.md`

```md
# <标题> 迁移实施计划

**状态：** Draft / Active / Completed
**最后更新：** YYYY-MM-DD
**父 RFC：** [RFC-YYYYMMDD-short-slug](./index.md)
**目标与不变量：** [目标与不变量](./invariants.md)
**当前契约：** 受影响 contract IDs 与链接；没有则写 `None`。
**当前修订：** Draft / R0 / R1 / ...

## 迁移原则

- 原则 1。
- 原则 2。
- 允许带入实现的不确定性、验证方式、停止条件和 RFC 回写路径。
- 多阶段 RFC 默认只把下一个可执行阶段完整解析为 Ready；更远阶段保持 Outline，不伪造尚无实现证据支持的精确设计。
- 实现反馈不得自行降低 accepted target；工程代价需要改变目标时，先停止 cutover 并进入 `Target Renegotiation Gate`。

## 阶段成熟度与滚动解析

- `Outline`：未来阶段；只有概括目的、依赖、受保护边界和解析触发点。
- `Ready`：交付、实现策略或 probe、审计、可观测性、验证、停止/退出条件、contract cutover 与 `Resolved Write Set Manifest` 已完整解析，但尚未自动获得执行授权。
- `Active`：已经通过当前授权协议并开始执行。
- `Closed`：已经按本阶段自身条件独立关闭。
- 第一个可执行阶段在实现开始前达到 Ready；后续阶段在前一阶段关闭后通过独立的 `N -> N+1 Implementation Resolution Gate` 从 Outline 解析为 Ready。
- `implementation.md` 是 Ready 阶段与 resolved manifest 的唯一权威；transaction devlog 只记录 preflight 证据、批准事实、生效点和本节链接。
- 只有 Ready / Active 阶段冻结后的 manifest 越界才是 write set 扩展；future Outline 的自然收窄、扩大、拆分、合并或重排属于滚动解析。
- 不新建通用 `stage-plan.md`、`manifest.md`、`write-set.md` 或并列计划文件。

## 阶段路线图

| 阶段 | 成熟度 | 概括目的 | 前置依赖 | 受保护边界 | 解析触发点 |
| --- | --- | --- | --- | --- | --- |
| Stage 0 | Ready | 第一个可执行阶段目的 | RFC acceptance | target / contract IDs | 实现开始前 |
| Stage 1 | Outline | 后续阶段目的 | Stage 0 Closed | target / contract IDs | Stage 0 独立关闭后 |

## Stage 1 Outline：简短阶段名

概括目的：

- 本阶段最终要贡献什么能力或证明。

前置依赖：

- 哪些阶段、contract 或外部条件必须先成立。

受保护边界：

- 不得改变的 correctness invariant、target guarantee、owner、ABI、contract IDs 和 acceptance boundary。

解析触发点：

- 前一阶段关闭后的只读 preflight；具体类型、函数、算法、逐文件路径和精确测试命令在该 gate 解析，当前缺失不构成 finding。

预计范围（可选）：

- 预计参与的 owner、subsystem、目录、模块或验证类别；这些内容不是写入授权。

## Stage 0 Ready：简短阶段名

阶段成熟度：

- Ready / Active / Closed；Ready 表示整个阶段已经解析完成，但尚未自动获得执行授权。

前置条件：

- 开始前必须满足的条件。

交付：

- 具体交付 1。
- 具体交付 2。

审计：

- 本阶段需要执行的搜索、review 或分类。

反馈假设：

- 本阶段要用真实实现验证的假设。
- 失败信号、停止条件，以及结果应写回 transaction devlog、RFC target / `Contract Impact`、current contract、`implementation.md`、`tracking-issues.md` 还是 register / current limitations。

contract cutover：

- 本阶段只验证 target，还是会切换具体 contract IDs。
- cutover 前置验证、原子边界、生效范围，以及失败时保持的 effective 规则。
- 如果本阶段不改变 current contract，明确写 `None`。

模块边界预检：

- 当前文件/模块是否已经混合 syscall ABI、核心状态机、后端实现、兼容桥、锁/生命周期规则或 UAPI/internal 转换。
- 继续实现前是否需要同一 owner 内的行为保持型拆分；如果需要，列出 split-only checkpoint、预期移动的文件和应保持不变的 public API。
- 若 Ready / Active 阶段的拆分会改变 owner surface、public API、shared contract 或 resolved manifest，先走扩展申请，不在本阶段静默完成。

scope envelope：

- 参与的 protocol/state owner、subsystem、contract IDs。
- 已解析的内部模块/API 边界及其与 RFC target 的关系。
- 不得跨越的语义、owner、public API、shared contract、ABI 和 acceptance boundary。

Resolved Write Set Manifest：

- 精确列出允许修改的现有文件、计划新建的文件或目录、文档回写面和 validation-only 输入。
- 列出不应触碰的物理边界，以及 integrator / reviewer 责任。
- 如果更合适的架构需要扩大已冻结 manifest，应停止并上报扩展申请；申请需说明原因、拟新增范围、contract/gate/验证影响和批准后的记录位置。

可观测性：

- 本阶段要求的 debug / trace / assertion 证据。

验证：

- 命令、测试、stress profile 或证明材料。

退出条件：

- 本阶段独立关闭的标准；不得把“下一阶段已达到 Ready”作为本阶段完成事实的一部分。

## Stage 0 -> Stage 1 Implementation Resolution Gate

前置条件：

- Stage 0 已按自身 review、验证和退出条件独立关闭。

只读 preflight：

- 读取 live source、Stage 0 实际 diff、review finding、模块边界、验证证据、RFC target 和 current contracts。
- 核对 Stage 1 Outline 的目的、依赖、受保护边界和 target 可达性。

解析输出：

- 把 Stage 1 从 Outline 展开为完整阶段：精确交付、实现路径或 probe、审计、可观测性、验证、停止/退出条件、兼容桥删除点和 contract cutover。
- 在 Stage 1 的 `Resolved Write Set Manifest` 中精确列出文件、新建路径、文档回写面、validation-only 输入、不应触碰边界和集成责任。
- 若只改变 future Outline、物理范围、stage order、validation 安排、review gate 或停止条件，更新本文并在 transaction 记录原因和生效点。
- 若改变 target invariant、protocol/state owner、public API、shared contract、ABI、visible semantics 或 acceptance boundary，停止并进入 RFC review / `Target Renegotiation Gate`。

授权边界：

- 完整阶段和 manifest 冻结后 Stage 1 才达到 Ready；按当前 RFC / transaction 的授权协议另行启动，不自动进入实现。

## 旁路审计清单

列出精确代码搜索、分类方式和允许保留旁路的理由。

## 可观测性清单

列出后续 review 必须能依赖的 logs / traces / assertions。

## 停止边界

说明什么时候应继续追查 issue，什么时候应停止实现形状争论。

## Probe / Vertical Slice Gates

默认不要为 probe / feedback 新建通用 `feedback.md`、`probe.md` 或 `experiments.md`。计划写在本节；执行结果写入 transaction devlog。只有证据包过长时，才在 `backgrounds/` 下增加具体命名的证据文件，并从本节链接。

### Gate P1 - 简短标题

**Hypothesis:** 要验证的设计假设。
**Protected Goal / Invariant:** 本 gate 不得削弱的目标、不变量、ABI 边界或验收条件。
**Contract Impact:** 受影响 contract IDs；说明本 gate 只验证 target，还是会执行 cutover。
**Minimum Write Set:** 允许触碰的最小文件、模块或文档。
**Non-goals:** 明确不沉淀的长期抽象、兼容层或 public API。
**Validation Floor:** build / source audit / smoke / LTP 子集 / 用户运行证据。
**Failure Signals:** 出现什么现象就停止当前 gate，并回到 RFC 或人工决策。
**Write-back:** 结果应写回 transaction devlog、RFC target / `Contract Impact`、current contract、`implementation.md`、`tracking-issues.md` 还是 register / current limitations。
**Exit:** 删除探针 / 升级为正式阶段 / 登记 limitation 或 open issue / 回到 RFC review。
**Evidence:** 可选；只有证据包较长时链接 `backgrounds/<topic>-probe-YYYYMMDD.md`。

## 实现期反馈记录

- YYYY-MM-DD：反馈来源、影响分类（Execution Fact / Route Correction / Target Renegotiation）、是否保持 accepted target、更新位置、transaction devlog 链接。

## Target Renegotiation Gates

只有工程证据表明 accepted target 的代价或可行性需要重新判断时才增加本节条目。提案不等于批准；决定前当前阶段保持停止且不得 cutover。

### YYYY-MM-DD - 简短标题

**Status:** Proposed / Accepted / Rejected / Superseded。
**Trigger / Cost Evidence:** 真实接口、实现、测试或集成暴露的成本与失败信号；不能只写“实现太难”。
**Original Target:** 受影响的目标、target guarantees、contract IDs、ABI / acceptance boundary 和原验证要求。
**Correctness Invariants:** 无论采用何种 reduced target 都不得违反的 owner、并发、生命周期、cleanup、内存安全和 ABI 诚实性边界。
**Completed Slice / Code Disposition:** 已完成能力，以及现有代码应保留、保持 dormant、删除还是拆入独立阶段。
**Options:** Route Correction / Accepted Reduced Target / Follow-up RFC / Not Cut Over，以及各自成本和架构影响。
**Candidate Reduced Target:** 若提出降级，说明它为何独立有用、ABI 诚实、可验证且不固化错误 owner / abstraction；未支持输入如何稳定拒绝，临时桥如何观测和退出。
**Revision / Contract Impact:** 保持当前修订，或更新的 RFC revision、`Contract Impact` 和后续 cutover gate；决定前 effective contract 不变。
**Validation:** reduced target 自己必须满足的验证，不得复用原目标的部分通过结果冒充 closure。
**Decision / Authority:** reviewer / owner 的明确结论和证据链接；agent 提案不能自行批准。
**Remaining Gap:** 进入 follow-up RFC、current limitations 或 open issues 的事项；新 target 范围内的错误不能登记为 limitation。

## 修订实施记录

已完成阶段保留原结论；后续修订追加独立段落。若新修订使旧计划不再成立，在旧段标记 superseded 关系，不能让冲突计划同时表现为当前有效。

### R1 - 简短修订标题

**Trigger:** 触发修订的框架、证据或约束变化。
**Semantic Delta:** 受影响的 target invariant / owner / ABI / 接受边界和 contract IDs；具体 target 已折回 RFC `index.md` / `invariants.md`，effective 规则只在 cutover gate 更新。
**Write Set / Gates:** 本修订的实施范围、review gate 和停止条件。
**Validation Floor:** 本修订必须完成的验证。
**Transaction:** [R1 事务](../../devlog/transactions/YYYY-MM-DD-short-slug-r1.md)；纯文档修订写 `None` 并链接 review / 小迭代证据。

## Write Set 扩展记录

- 只记录 Active / Ready 阶段已冻结 manifest 的扩展；future Outline 的 scope estimate 变化不记录为扩展。
- YYYY-MM-DD：原 resolved manifest、申请原因、批准后的新增范围、对应 review/验证 gate、transaction devlog 链接。

## 结构维护记录

- YYYY-MM-DD：拆分前职责混杂点、split-only checkpoint 范围、保持不变的 public API、验证命令和后续语义阶段链接。
```

## `backgrounds/index.md`

```md
# <RFC 标题> 背景材料

本目录保存 [RFC-YYYYMMDD-short-slug](../index.md) 的历史上下文。

RFC target：

- [目标与不变量](../invariants.md)
- [迁移实施计划](../implementation.md)
- [受影响的当前契约](../../../contracts/<owner>/<surface>.md)

历史材料：

- [问题简述](./problem-brief.md)
- [被否决的窄化方案](./rejected-narrow-fix.md)
```
