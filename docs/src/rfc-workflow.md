# RFC 工作流

本文定义中型及以上功能迭代的文档生命周期。目标是让计划、不变量、review 结论和实现证据都有稳定归属，避免实现阶段依赖个人草稿、聊天记录或临时上下文。

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
- `implementation.md`：阶段、前置条件、交付、审计、可观测性、验证和退出条件；
- `tracking-issues.md`：当前 design review 发现且仍影响实现顺序、review gate、停止边界或验收判断的问题；
- `backgrounds/`：历史材料、旧问题清单、旧计划、被拒绝方案和运行证据索引。

不是每个 RFC 都需要 `invariants.md` 或 `tracking-issues.md`。如果正确性不依赖复杂协议，可以省略不变量页；如果 review 没有发现需要持续跟踪的设计缺陷，可以不创建 tracking 页。

### 3. 文档层 Review

中型及以上迭代默认先在文档层闭合协议，再进入实现。review 应分清三类内容：

- 不变量和系统语义是否自洽；
- 子系统边界、状态所有权、生命周期、可观测性和失败路径是否合理；
- 实现顺序、review gate、验证 floor 和停止条件是否足够明确。

review 发现的 confirmed design issue 应写入 `tracking-issues.md`，并使用当前等级名：`Apollyon`、`Keter`、`Euclid`、`Safe`、`Neutralized`。不要把实现进度、普通 TODO 或历史讨论塞进 tracking issues。

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

### 6. 实现阶段

实现必须按 RFC 和 transaction 中的阶段推进。每个阶段至少说明：

- write set 和不应触碰的边界；
- review gate 和停止条件；
- 验证 floor，例如 `just build`、用户运行的 LTP、或只读审计；
- 临时兼容层、旁路路径和后续删除点。

阶段推进、review 结论和验证证据写入 transaction devlog。RFC 只在 accepted contract 变化时更新；如果实现发现 RFC 的不变量或边界错误，应先回到 RFC 文档层修正，再继续实现。

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
| `tracking-issues.md` | 当前 design-review issue 状态 | 普通 TODO、实现进度、历史归档 |
| `backgrounds/` | 历史材料、旧计划、被拒绝方案、证据索引 | 覆盖 canonical 结论 |
| Transaction devlog | 执行事实、checkpoint、review、验证、handoff | 重新定义 accepted contract |
| 双周 devlog | 入口摘要和重要结论 | 长篇阶段日志 |
| Register / limitations | 当前开放问题和接受限制 | 设计草案或实现计划 |

## Agent 约束

Agent 处理 RFC 工作流时应优先读取本文和 [RFC 模板](./rfc-template.md)。如果任务涉及 review 输出，还应使用当前 Anemone review 等级。

Agent 可以帮助整理私有草案、执行文档层 review、修复 RFC 文本、提升公开 RFC、建立 transaction devlog 和更新导航。但 agent 不应把私有草稿路径写入公共 canonical 链接，也不应在用户明确要求文档层闭合时提前开始代码实现。

在实现文档或 agent 编排中，agent 应把 write set 视为默认协作合同。遇到必须越界的架构依赖时，正确动作是提交扩展申请并等待批准；不得自行扩大范围，也不得为了服从旧 write set 引入错误 owner boundary 或长期 compatibility layer。
