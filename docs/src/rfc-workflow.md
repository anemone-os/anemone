# RFC 工作流

本文定义中型及以上功能迭代的文档生命周期。目标是让计划、不变量、review 结论和实现证据都有稳定归属，避免实现阶段依赖个人草稿、聊天记录或临时上下文。

RFC 工作流不是要求实现前消除所有不确定性。文档层 review 负责先闭合已知 contract、边界、验证 floor 和停止条件；实现阶段负责用真实接口、状态流转、错误路径和集成结果反向校正 RFC。允许带着受约束的不确定性进入实现，但每个不确定点必须有归属、验证方式、停止条件和回写路径。

反馈机制只能优化实现路线，不能篡改目标或私自削弱不变量。实现反馈可以暴露原 RFC 的目标、不变量或接受边界存在错误，但这类发现必须停止当前 gate 并回到 RFC review，由 canonical 文本显式修正；在修正前，agent 不能以“反馈”为名缩小目标、降低验证 floor、绕过不变量、删除失败路径或写临时 hack 来通过 gate。

## 适用范围

满足以下任一条件时，应走 RFC 工作流：

- 改动跨多个子系统，或会改变 shared contract；
- 改动涉及 ABI、兼容性、调度、等待、生命周期、锁序或资源所有权；
- 方案需要多轮 review，且 review 结论会影响实现顺序；
- 后续实现预计跨多天、多个 agent 或多个 checkpoint；
- devlog、register 或后续 RFC 需要长期引用该计划。

简单 bugfix、局部清理、一次性实验和不影响公共契约的小补丁不需要 RFC。它们可以只写普通 devlog、register 条目、小迭代记录，或不写正式文档。小迭代记录可以在单页内写清局部问题、解决方案和本迭代内部 tracking issues；如果这些内容开始需要仓库级 contract、不变量、阶段 gate 或多轮 review，再升级为 RFC。

## 生命周期

### 1. 私有草案

早期探索可以放在开发者自己的 gitignored 草稿区。私有草案可以提前采用 [RFC 模板](./rfc-template.md) 的目录形状，便于后续提升，但它不是公共 canonical source。

私有草案阶段允许快速重写、丢弃和拆分。公共文档、devlog 和 register 不应把私有草稿路径作为长期引用目标。

### 2. 草案成形

当方案开始稳定时，应按目录级 RFC 的角色拆分：

- `index.md`：入口状态、背景、目标、非目标、文档地图、方案摘要、接受边界和下一步；
- `invariants.md`：协议、不变量、状态所有权、身份模型、线性化点、锁序、生命周期和禁止退化项；
- `implementation.md`：阶段、前置条件、交付、审计、可观测性、验证、probe / vertical slice gate 和退出条件；
- `tracking-issues.md`：当前 design review 或实现反馈发现，且仍影响实现顺序、review gate、停止边界或验收判断的设计问题；
- `backgrounds/`：历史材料、旧问题清单、旧计划、被拒绝方案和运行证据索引。

不是每个 RFC 都需要 `invariants.md` 或 `tracking-issues.md`。如果正确性不依赖复杂协议，可以省略不变量页；如果 review 没有发现需要持续跟踪的设计缺陷，可以不创建 tracking 页。

### 3. 文档层 Review

中型及以上迭代默认先在文档层闭合协议，再进入实现。这里的闭合指当前阶段已经有足够明确的 accepted contract、边界和反馈入口，不表示所有未知都必须在编码前被消除。review 应分清三类内容：

- 不变量和系统语义是否自洽；
- 子系统边界、状态所有权、生命周期、可观测性和失败路径是否合理；
- 实现顺序、review gate、验证 floor 和停止条件是否足够明确。

review 发现的 confirmed design issue 应写入 `tracking-issues.md`，并使用当前等级名：`Apollyon`、`Keter`、`Euclid`、`Safe`、`Neutralized`。不要把实现进度、普通 TODO 或历史讨论塞进 tracking issues。

进入实现不要求 `tracking-issues.md` 中所有条目清空。仍会改变 accepted contract、状态所有权、ABI 边界、阶段顺序或验收判断的 Apollyon / Keter 必须先 neutralize，或者明确转成某个实现 gate 的停止条件。Euclid / Safe、已接受延期项、以及只能通过实现证据验证的风险，可以带入实现阶段，但必须在 `implementation.md` 或 transaction devlog 中写明验证点和回写路径。

修复 design issue 时，必须把修复折回 `index.md`、`invariants.md` 或 `implementation.md` 的 canonical 文本。`tracking-issues.md` 只记录问题状态、修复依据和链接，不能替代主文档。

### 4. 提升为公开 RFC

当方案进入共享决策流程，或后续 devlog/register 需要引用计划时，必须把已收口草案提升到：

```text
docs/src/rfcs/<short-slug>/
```

提升不是简单复制。公共 RFC 应重写标题、状态字段、文档地图和接受边界，让 `docs/src/rfcs/<short-slug>/` 立即成为 canonical source。公共文档不应继续暗示私有草案才是权威。

提升时至少同步：

- `docs/src/rfcs.md`；
- `docs/src/SUMMARY.md`；
- RFC 目录内的相对链接；
- 必要的 `backgrounds/index.md`。

### 5. 引入事务 Devlog

RFC 进入实现阶段时，必须建立事务日志：

```text
docs/src/devlog/transactions/YYYY-MM-DD-<short-slug>.md
```

同时建立双向链接：

- RFC `index.md` 页首的 `事务日志` 字段链接到 transaction；
- transaction 页首的 `Canonical Plan` 链接回 RFC；
- `docs/src/devlog/transactions/index.md` 加入入口；
- 当前双周 devlog 追加一条入口摘要；
- `docs/src/SUMMARY.md` 加入 transaction 导航。

RFC 记录 accepted contract、边界和计划。transaction devlog 记录实际执行、checkpoint、review 结论、验证证据、更正说明、剩余限制和 handoff。

### 5.1 跨 RFC 功能入口

有些能力会被多个 RFC 分段覆盖。此时不要为“功能进度”新建一份并行账本，也不要把 transaction devlog 的阶段事实复制到另一个状态文件中。

推荐做法是提供一个轻量导航入口：

- 如果某个 RFC 是 umbrella / parent RFC，在该 RFC 的 `index.md` 中列出后续 RFC、事务日志、register / current limitations 链接；
- 如果没有明确 parent RFC，在 [公开草案与 RFC](./rfcs.md) 的“当前 RFC”或领域分组中聚合相关 RFC 链接；
- 导航入口只说明“该看哪些文档”和每个链接覆盖的范围，不重新记录阶段完成度、验证证据或剩余问题。

跨 RFC 功能的当前事实仍按原职责归属：accepted contract 写在对应 RFC，执行进展写在 transaction devlog，开放问题和接受限制写在 register / current limitations。这样可以给读者一个 feature 级入口，同时避免多重真相源。

### 5.2 受控反馈与探针阶段

对高风险设计点，RFC 可以在正式语义阶段前安排 probe / vertical slice gate。它用于验证接口形状、owner boundary、状态机闭合、错误路径、性能或集成风险，而不是绕开 RFC 的长期实现。

probe / vertical slice gate 必须写清：

- 要验证的假设和失败信号；
- 受保护的目标、不变量和验收边界，说明哪些内容不得在 probe 中削弱；
- 最小 write set、保持不变的 public API 和不得沉淀的临时路径；
- 验证 floor，例如定向 build、source audit、smoke、LTP 子集或用户运行证据；
- 退出路径：删除探针、把结果折回 RFC、升级为正式阶段，或登记为 limitation / open issue；
- 若探针发现 accepted contract 错误，必须先更新 RFC canonical 文本和必要的 tracking issue，再继续扩大实现。

默认不要为反馈机制新建通用 `feedback.md`、`probe.md` 或 `experiments.md`。probe / vertical slice gate 的计划格式写在 `implementation.md`，执行结果写在 transaction devlog 的阶段条目中。只有当 probe 产生的 Linux/LTP 对照、日志摘要、被拒绝方案或证据包已经让 `implementation.md` 难以扫读时，才在该 RFC 的 `backgrounds/` 下增加具体命名的证据文件，例如 `backgrounds/<topic>-probe-YYYYMMDD.md`。这类文件仍是证据材料，不承担阶段计划、反馈状态或 accepted contract。

探针代码不能因为“已经能跑”就自然变成长期抽象。只有当 transaction devlog 记录了证据，且 RFC 已接受对应 contract 变化时，探针形状才可以进入后续正式阶段。

### 6. 实现阶段

实现必须按 RFC 和 transaction 中的阶段推进。每个阶段至少说明：

- write set 和不应触碰的边界；
- 模块边界预检：当前文件/模块是否已经混合多个职责，继续追加代码是否会强化错误 owner boundary；
- review gate 和停止条件；
- 验证 floor，例如 `just build`、用户运行的 LTP、或只读审计；
- 临时兼容层、旁路路径和后续删除点。

实现阶段可以安排独立的结构维护 gate。这个 gate 只做同一 owner 内的行为保持型拆分、模块目录化、可见性收窄、导入路径调整和调用面不变的文件搬移；不应顺手改变 syscall 语义、状态机、不变量或 ABI。推荐验证 floor 是 `git diff --check`、`just build`，以及必要的 `rg` 检查，确认外部调用面没有被扩大、旧兼容入口没有被误保留。

当拆分需要移动 owner surface、改变 public API、引入新的 facade、调整 shared contract，或扩大原 write set 时，不能把它包装成普通整理；应走 write set 扩展申请，并在 transaction devlog、阶段说明或 agent 编排文档中记录批准后的结构边界。

阶段推进、review 结论和验证证据写入 transaction devlog。RFC 只在 accepted contract 变化时更新；如果实现发现 RFC 的不变量或边界错误，应先回到 RFC 文档层修正，再继续实现。

实现期反馈按影响分流：

- 不改变 accepted contract 的实现事实、checkpoint、review 结论和验证结果，只追加到 transaction devlog；
- 改变阶段顺序、write set、验证 floor、review gate 或停止条件时，更新 `implementation.md`，并在 transaction devlog 记录原因和生效点；
- 改变不变量、状态所有权、ABI 边界、外部可见语义或接受边界时，更新 `index.md` / `invariants.md`，并把对应 design issue 写入或更新 `tracking-issues.md`；
- 已接受但暂不补齐的能力缺口进入 current limitations；本应工作但当前不正确的事项进入 open issues；
- 无法分类的实现摩擦不能静默用兼容层绕过，应先停止在当前 gate，补充证据后再选择上述归属。

以下行为不属于有效反馈，必须停止并回报：为了让 gate 通过而缩小原目标、调低验证集合、隐藏失败路径、把必须满足的不变量改成建议项、用日志或静默兼容替代约定行为、把 Keter / Apollyon 重新命名成实现限制，或在未更新 RFC canonical 文本的情况下让代码接受更弱语义。

write set 是协作边界，不是架构边界。写入型 agent 不能静默越过已分配的 write set；但如果更合适的架构需要触碰新的 owner surface、移动 shared contract，或把 helper 放到更自然的子系统，agent 应停止并向总控或用户汇报扩展申请，而不是在原 write set 内做兼容性绕路。

write set 扩展申请至少说明：

- 为什么原 write set 会导致错误分层、重复状态、旁路路径或不可维护的适配层；
- 需要新增的文件、模块或子系统边界；
- 对 RFC accepted contract、阶段 gate、review gate 和验证 floor 的影响；
- 批准后由谁集成，以及 transaction devlog 或 orchestration 文档中的记录位置。

扩展通过后，应先更新 transaction devlog、阶段说明或 agent 编排文档中的 write set，再继续实现。扩展未通过前，worker 仍只能在原 write set 内修改，或保持停止状态等待人工决策。

### 7. 收口

事务完成时应更新：

- RFC 状态和收口说明；
- transaction devlog 的 `Status`、`Closure` 或最终阶段条目；
- 当前双周 devlog 的必要摘要；
- register 或 current limitations 中受影响的开放问题和限制；
- `tracking-issues.md` 中仍开放或已 neutralize 的问题状态。

收口记录必须区分：已实现能力、仍接受的限制、用户侧验证、agent 侧验证、未运行的验证。

## Artifact 边界

| Artifact | 职责 | 不应承担 |
| --- | --- | --- |
| 私有草案 | 快速探索、预 review、未公开方案打磨 | 公共 canonical source |
| RFC `index.md` | 状态、范围、方案摘要、接受边界、文档地图 | 阶段流水账 |
| `invariants.md` | 协议和不变量证明边界 | 实现步骤细节 |
| `implementation.md` | 阶段计划、gate、验证、停止条件 | 已执行 checkpoint 日志 |
| Probe / vertical slice gate | 验证高风险假设、接口形状和集成风险 | 没有 RFC 回写的长期抽象或隐藏实现阶段 |
| 结构维护 gate | 同一 owner 内行为保持的模块拆分、目录化、可见性收窄和调用面确认 | 借整理名义改变语义、移动 owner surface、扩大 public API |
| `tracking-issues.md` | 当前 design-review issue 与实现反馈暴露的 design issue 状态 | 普通 TODO、实现进度、历史归档 |
| `backgrounds/` | 历史材料、旧计划、被拒绝方案、证据索引 | 覆盖 canonical 结论 |
| Transaction devlog | 执行事实、checkpoint、review、验证、handoff | 重新定义 accepted contract |
| 跨 RFC 功能入口 | 相关 RFC / transaction / register 的导航索引 | 阶段进度账本、验证事实副本、第二套问题状态 |
| 双周 devlog | 入口摘要和重要结论 | 长篇阶段日志 |
| Register / limitations | 当前开放问题和接受限制 | 设计草案或实现计划 |

## Agent 约束

Agent 处理 RFC 工作流时应优先读取本文和 [RFC 模板](./rfc-template.md)。如果任务涉及 review 输出，还应使用当前 Anemone review 等级。

Agent 可以帮助整理私有草案、执行文档层 review、修复 RFC 文本、提升公开 RFC、建立 transaction devlog 和更新导航。但 agent 不应把私有草稿路径写入公共 canonical 链接，也不应在用户明确要求文档层闭合时提前开始代码实现。

Agent 不应把 “Accepted for Implementation” 解释成所有风险已经消失，也不应把 “tracking issues 仍有 Euclid / Safe / gated item” 解释成实现必须停止。正确动作是检查当前 gate 的 blocker 是否已 neutralize、剩余不确定性是否有验证点和回写路径，再按 transaction devlog 推进。

在实现文档或 agent 编排中，agent 应把 write set 视为默认协作合同。遇到必须越界的架构依赖时，正确动作是提交扩展申请并等待批准；不得自行扩大范围，也不得为了服从旧 write set 引入错误 owner boundary 或长期 compatibility layer。
