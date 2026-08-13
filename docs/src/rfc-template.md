# RFC 模板

完整分级和生命周期见[开发工作流](./development-workflow.md)。跨 RFC 已生效规则见[当前契约](./contracts.md)，外部源码证据遵循[公共引用规则](./external-source-references.md)。模板只提供最小形状；没有真实内容的可选章节和文件直接删除。

## 目录形状

RFC 默认只有一个入口文件：

```text
docs/src/rfcs/<short-slug>/
  index.md
```

按需增加：

```text
  invariants.md          # 非平凡协议、状态机、锁序、生命周期或 contract proof
  implementation.md      # 多阶段、probe、不安全中间态或多个 cutover
  tracking-issues.md     # 仍影响实现、停止边界或 acceptance 的设计问题
  backgrounds/           # 正文无法扫读的事实证据包或历史材料
    index.md
```

不要默认创建 transaction。只有长期 RFC、多 checkpoint、多 cutover、probe/renegotiation 证据确实需要独立执行历史时，才在 `docs/src/devlog/transactions/` 建立记录。

RFC 不要求先创建 positioning 或 backgrounds。target 已经闭合时直接编写 `index.md`；只有事实证据、历史上下文或被拒绝方案会妨碍正文扫读时，才增加 `backgrounds/`。

RFC 文本历史由整个仓库 Git 保存，不创建 per-RFC 仓库、版本化 canonical 副本或默认 amendment。
RFC 修订只发生在设计、实现和验收尚未整体 Closed 的生命周期内；Closed 后整个 RFC 目录冻结为历史资料。

## `index.md`

```md
# RFC-YYYYMMDD-short-slug

**状态：** Draft / Accepted / Review Hold / Closed / Superseded / Terminated
**修订：** Draft / R0 / R1 / ...
**负责人：** name1, name2
**最后更新：** YYYY-MM-DD
**领域：** scheduler / fs / mm / ...
**影响契约：** 实际变化的 contract IDs 与链接；没有则写 `None`
**执行记录：** commit / PR / optional transaction；Draft 阶段可写 `None`

`Terminated`只用于维护者永久取消尚未满足acceptance/closure的RFC。它没有active gate或current-contract
cutover，supporting pages只能保留historical状态，且不得恢复。未来相关工作必须作为独立任务重新分类并取得
新的授权/Implementation Boundary；只有仍命中RFC分级时才新建RFC。

`Closed`是不可重新打开或修订的完成终态。不得把它恢复为`Accepted`/`Review Hold`、增加`R<n>`、追加gate，
或用新transaction续跑。后续相关工作从live source、current contract和register建立新的Implementation Boundary，
按当前规则独立分类；旧RFC只作为provenance，不能规定新任务必须修订它或建立follow-up RFC。

## 摘要

用一到两段说明问题和提议方向。

## 背景

记录 current baseline、已观察失败和为什么需要共享决策。当前 effective 规则只链接 current contract，不复制正文。

## 目标

- 目标 1。
- 目标 2。

## 非目标

- 明确排除的范围 1。
- 明确排除的范围 2。

## Owner 与协议边界

- Protocol/state owner：唯一 owner。
- Handoff / 线性化点：跨 owner 时说明。
- Failure / cancellation / cleanup：最终负责方和失败后的状态。

## ABI 与可见语义

说明 user-visible ABI、errno、兼容策略、显式不支持项；没有则写 `None`。

## Contract Impact

只列语义发生变化的 ID。未变化规则放在 `Dependencies`，不登记 `Preserve`。

| Contract ID | 变化 | 当前规则 | Target 摘要 | Cutover |
| --- | --- | --- | --- | --- |
| JOBCTL-STATE-001 | Introduce | None（尚未生效） | 新规则摘要 | 原子 checkpoint / named gate |
| SCHED-PICK-001 | Replace | [当前规则](../../contracts/scheduler/pick-request.md#sched-pick-001) | 新规则摘要 | named gate |

变化类型只使用 `Introduce`、`Refine`、`Replace`、`Remove`、`Scoped Exception`。若没有 contract delta，删除表格并写 `None`。

### Dependencies

- [UNCHANGED-ID](../../contracts/<owner>/<surface>.md#unchanged-id)：仅作为前提，不复制正文。

## Implementation Boundary

- 允许改变：本轮 target、owning subsystem 和 visible surface。
- 必须保持：owner、handoff、ABI、current contract、acceptance 或其它受保护边界。
- 实现提示（可选）：预计模块/目录；非穷举、非逐文件授权。
- 停止条件：哪些发现必须回到 RFC review / Target Renegotiation。

## Acceptance 与 Validation

- 接受本 RFC 代表什么。
- 需要的 source/build/runtime/architecture 证据。
- 明确用户运行、agent 运行和 Not Run 边界。

## 风险与反馈

- 只能靠实现验证的风险、失败信号和回写位置。
- 需要 probe 或多阶段时链接 `implementation.md`；否则直接删除此句。

## 文档与证据

- [目标与不变量](./invariants.md)（如果存在）
- [实施路线](./implementation.md)（如果存在）
- [Tracking Issues](./tracking-issues.md)（如果存在）
- [背景材料](./backgrounds/index.md)（如果存在）
- commit / PR / optional transaction：
- 外部源码证据：`xref:<source-id>:<repo-relative-path>#<locator>` / `None`

## 修订记录

只记录 RFC Closed 前已接受的 target 语义版本；普通文字、证据、实现路线、文件布局和验证命令调整不增加修订。

| 修订 | 日期 | 语义变化 | Review / Evidence |
| --- | --- | --- | --- |
| R0 | YYYY-MM-DD | 初始 accepted target 与 contract delta。 | commit / PR / optional transaction |

## Closure

完成时记录实际交付、验证、contract cutover/Not Cut Over、仍开放问题/限制，以及有证据的 Architecture Friction。该次 closure 同时冻结 RFC；后续事实进入 live source、current contract、register 或新的独立任务，不再回写本 RFC。没有架构摩擦时不写占位结论。
```

## `invariants.md`（按需）

只有正确性依赖非平凡协议、状态机、锁序、生命周期或 contract proof 时才创建。

```md
# <标题> 目标与不变量

**状态：** Draft / Accepted Target / Superseded / Terminated Historical
**最后更新：** YYYY-MM-DD
**父 RFC：** [RFC-YYYYMMDD-short-slug](./index.md)
**适用修订：** Draft / R0 / R1 / ...

本文只定义本 RFC 的 target/contract proof obligations。当前 effective 规则以 `docs/src/contracts/` 为准。

## 规则分类

- Correctness Invariant：唯一 owner、并发、生命周期、cleanup、内存安全和 ABI 诚实性；不可通过工程妥协降低。
- Target Guarantee / Capability：当前修订承诺的能力；只能通过 Target Renegotiation 改变。
- Implementation Preference：类型、helper、内部模块、算法和文件布局；不写成 invariant。

## Target Invariants

### TARGET-001 — 简短标题

**规则：** 规范性目标。
**Owner：** 唯一 owner。
**依赖：** current contract IDs 或本 RFC target。
**违反表现：** 错误、双重真相源、ABI 偏差或不可闭合路径。
**Cutover / Proof：** 何时、用什么证据证明；RFC-local 时写 `N/A`。

## 状态所有权与生命周期

只写理解协议所必需的 owner、状态、能力、线性化点、锁序和 cleanup。

## RFC-local Proof Obligations

- 迁移、probe 或阶段原子性规则。
- 临时桥必须写明原因、可见边界和删除条件。

## 禁止退化项

- 会制造第二套真相源、绕过 owner 或降低 ABI 诚实性的模式。
```

Contract Impact 默认放在 `index.md`；只有表格和证明义务过长时才移到本页，不能两边复制。

## `implementation.md`（按需）

只有多阶段、不安全中间态、probe、多个 cutover 或需要长期保存的实施路线时才创建。普通单次实现、commit 顺序和文件列表不需要本页。

```md
# <标题> 实施路线

**状态：** Draft / Active / Completed / Terminated Historical
**最后更新：** YYYY-MM-DD
**父 RFC：** [RFC-YYYYMMDD-short-slug](./index.md)
**当前修订：** Draft / R0 / R1 / ...

## 全局 Implementation Boundary

- Target / non-goals：
- Owner / handoff / failure / cleanup：
- Protected ABI / contract / acceptance：
- Validation claim：
- Stop conditions：

预计目录、模块或内部 API 可以作为非穷举提示；不要创建 `Resolved Write Set Manifest`、`write-set.md` 或逐文件扩展记录。

## 阶段或 Checkpoint（只有真实 gate 时）

### Gate 1 - 简短标题

**Purpose:** 本 gate 独立交付的安全能力或证明。
**Prerequisites:** 必须先成立的条件。
**Protected Boundary:** 不得改变的 target、owner、ABI、contract 和 acceptance。
**Deliverable:** 具体交付。
**Validation:** source/build/runtime/architecture 证据。
**Cutover:** 具体 contract IDs；没有写 `None`。
**Stop / Exit:** 何时停止、何时可以独立关闭。

未来 gate 只需要 Purpose、Prerequisites 和 Protected Boundary；到达前不冻结类型、算法、文件列表或精确命令。若用户只授权当前 gate，关闭后必须停止。

父RFC进入`Terminated`时，全部未完成gate必须标为Cancelled，正文改为明确的历史原计划/Not Run事实；不得保留
可执行前置、activation、cutover或从原RFC晋级的指令。

## Probe / Vertical Slice（按需）

### Probe P1 - 简短标题

**Hypothesis:** 要验证的假设。
**Protected Boundary:** 不得削弱的目标、不变量、ABI 或 acceptance。
**Non-goals:** 不沉淀的长期抽象、兼容层或 public API。
**Validation:** 最小 source/build/runtime 证据。
**Failure Signals:** 出现什么就停止。
**Write-back:** RFC / current contract / register / code disposition。
**Exit:** 删除、正式实现、RFC review 或 Not Cut Over。

## Target Renegotiation（按需）

**Trigger / Cost Evidence:** 真实接口、代码、测试或集成证据。
**Original Target:** 受影响的 target、ABI、contract、acceptance 和 validation。
**Correctness Invariants:** 不能降低的边界。
**Completed Slice / Code Disposition:** 保留、dormant、删除或拆分。
**Options:** Route Correction / Accepted Reduced Target / Follow-up RFC / Not Cut Over。
**Decision / Authority:** owner/reviewer 决定；agent 提案不能自行批准。

## Architecture Friction（仅存在时）

- 等级：Euclid / Keter / Apollyon。
- 证据：具体代码路径、状态/owner/lifecycle 模型。
- 模型偏差与影响：局部可逆，还是阻碍当前/下一步。
- 路由：当前边界内修正、Patch/小迭代、RFC review 或停止。
```

## `tracking-issues.md`（按需）

只保留仍影响实现、停止边界或 acceptance 的问题。普通 TODO、Safe、已修复问题和历史 review 交给 Git/PR；修复后的语义折回 canonical RFC。

```md
# <标题> Tracking Issues

**状态：** Active / Closed / Terminated Historical
**最后更新：** YYYY-MM-DD
**父 RFC：** [RFC-YYYYMMDD-short-slug](./index.md)

## ISSUE-001 - 简短标题

**等级：** Apollyon / Keter / Euclid
**状态：** Open / Neutralized / Superseded
**证据：** 具体路径、协议或验证。
**影响：** 对 target、owner、ABI、contract、实施或 acceptance 的影响。
**修复位置：** RFC canonical 文本、代码或 review 链接。
```

Neutralized 后若不再承担当前导航或停止作用，可以删除条目；历史由 Git/review 保存。

## `backgrounds/index.md`（按需）

```md
# <RFC 标题> 背景材料

本目录只保存 [RFC-YYYYMMDD-short-slug](../index.md) 的事实证据、历史上下文和被拒绝方案，不定义 accepted target、Implementation Boundary 或 current contract。

- [证据标题](./evidence.md)
```
