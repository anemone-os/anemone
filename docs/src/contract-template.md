# 当前契约模板

完整职责、增量迁移和 RFC cutover 规则见 [当前契约](./contracts.md)。模板只固定 owner、作用域、稳定 ID 和生效证据所需的外壳；状态机、ABI、锁序、算法证明等正文按实际问题组织。

## 目录形状

```text
docs/src/contracts/<owner>/
  index.md
  <contract-surface>.md
```

目录按稳定 owner 命名，文档按共同变化、共同证明的 contract surface 命名，不机械镜像源文件。

## Owner 索引

```md
# <Owner> 当前契约

**Owner：** 子系统、协议对象或明确的架构 owner
**覆盖范围：** 本目录负责的状态、能力和协议边界
**不覆盖：** 相邻但由其它 owner 负责的范围
**最后核验：** YYYY-MM-DD

本目录只登记已经迁移到 contract 层的共享规则，不声称已经枚举本领域全部不变量。

## Owner-wide Invariants

只有真正适用于整个 owner、无法自然归入更窄 surface 的小规则才放在这里。规则一旦形成独立依赖、生命周期或验证边界，就拆到具名 surface 文档。

## Contract Surfaces

- [Surface A](./surface-a.md)：覆盖范围。
- [Surface B](./surface-b.md)：覆盖范围。

## 邻接契约

- [OTHER-OWNER](../other-owner/index.md)：依赖或 handoff 范围。
```

## Contract surface

```md
# <Contract Surface> 当前契约

**Contract ID：** SCHED-PICK / WAIT-SOURCE / ...
**状态：** Active / Transitional / Retired
**Owner：** 唯一协议或状态 owner
**参与领域：** scheduler / task / fs / ...
**覆盖范围：** 本页定义什么
**不覆盖：** 明确排除什么
**实现位置：** 当前主要实现文件或模块；只作定位，不定义 owner
**依赖：** 其它稳定 contract ID；没有则写 `None`
**Pending Successor：** accepted-but-not-effective RFC；没有则写 `None`
**最后核验：** YYYY-MM-DD

## 术语

只定义理解本页规则所必需、且不能直接链接到其它 contract 的术语。

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| state A | owner A | token / snapshot / weak handle | 用途 |

纯诊断字段需要明确标注，不得反向驱动协议。

## <PREFIX>-001 — 简短规则标题

**规则：** 使用规范性语言写出必须成立的原子规则。

**违反表现：** 可观察失败、禁止行为、第二套真相源、lost wake、stale decision 或其它破坏形式。

**验证 / Enforcement：** assertion、source audit、production-path test、ABI test、模型或其它证据。

**最初来源：** 首次引入本规则的 RFC / ADR / change record，以及初始 cutover 证据。

**当前来源：** 最近一次改变本规则语义的 RFC，以及实际 cutover transaction / change record；从未改变时可以与最初来源相同。

以下字段默认继承文档头；只有不同时才增加：

- **Owner：** 条目级 owner。
- **适用范围：** 条目级窄化范围。
- **依赖：** 条目级 contract IDs。

## 跨领域局部义务（按需）

| Obligation ID | 参与方 | 必须完成的动作 | Handoff / 线性化点 | 失败 / Cleanup 责任 |
| --- | --- | --- | --- | --- |
| WAIT-SOURCE-001A | source owner | 更新 predicate 后发布 trigger | source lock / publication point | teardown 前 detach |

端到端规则只在本页完整定义；参与领域的其它文档只引用 obligation ID，不复制完整协议。

## Transitional Contract（按需）

只在 staged migration 的中间态会被其它代码真实依赖时使用。必须写明进入 gate、允许共存的旧/新路径、禁止扩展项、验证和删除 gate。

## Retired ID 映射

| Retired ID | Successor / Removal | 来源 |
| --- | --- | --- |
| PREFIX-000 | PREFIX-001 / Removed | RFC / transaction |

这里只保留短映射，不复制旧规则正文。
```

## RFC `Contract Impact`

RFC `invariants.md` 在涉及共享 contract 时加入：

```md
## Contract Impact

| Contract ID | 变化 | 当前规则 | Target 摘要 | 生效 Gate |
| --- | --- | --- | --- | --- |
| JOBCTL-STATE-001 | Introduce | None（尚未生效） | 新增 ThreadGroup job-control phase | Gate 3 |
| SCHED-PICK-001 | Replace | [当前规则](../../contracts/scheduler/pick-request.md#sched-pick-001) | pending 改为 core-only full-pick request | Gate 3 |
| WAIT-WAKE-004 | Preserve | [当前规则](../../contracts/wait-core/wake-publication.md#wait-wake-004) | 不变 | 全程 |

变化类型只使用 `Introduce`、`Preserve`、`Refine`、`Replace`、`Remove` 或 `Scoped Exception`。`Introduce` 只用于此前没有 effective 规则的新 ID，current rule 写 `None（尚未生效）`，并在 cutover 时创建 Active 条目；已有行为只是尚未提取时，应先建立 minimum effective baseline。Draft 和 accepted-but-not-effective 阶段不能把 target 写成当前 effective 规则。

## Target Invariants

- 精确定义本 RFC 接受但尚未 cutover 的目标规则。

## RFC-local Invariants

- 只服务本方案、probe、迁移桥、阶段原子性或验收的规则。
- 明确哪些临时规则在什么 gate 删除，不能自然沉淀成长期 contract。
```

## 拆页检查

不要按“一条不变量一个文件”拆分。出现以下情况时才建立或拆出独立 surface：

- 有独立状态或协议 owner；
- 有自己的线性化点、生命周期、锁序或 ABI；
- 会被后续 RFC 作为整体替换；
- 有独立验证 gate；
- 需要多个代码位置共同维护。

如果一条小规则属于既有 surface，追加稳定 ID 条目即可。如果没有自然归属，不创建 `misc` 容器；先确认它是否真的需要跨 RFC 权威，或者是否暴露了尚未解决的 owner boundary。
