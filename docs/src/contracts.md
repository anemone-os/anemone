# 当前契约

本页定义 Anemone 跨 RFC、跨模块长期生效的不变量如何组织和维护。契约层只保存当前已经生效的共享语义；RFC 保存一次变化的目标、理由、contract delta 和迁移证明，事务日志保存实际 cutover 与验证证据。

契约层不是全仓库不变量百科。既有 RFC 不批量迁移；只有当后续 RFC 第一次复用、扩展或替换某条既有共享不变量时，才提取本次变化所需的最小 contract 闭包。

可复制的文档形状见 [当前契约模板](./contract-template.md)。RFC 如何声明和切换 contract delta，见 [RFC 工作流](./rfc-workflow.md#当前契约与-rfc-的边界)。

## 权威边界

- `docs/src/contracts/` 下的条目只表达已经生效的共享规则。对已经登记的 contract ID，它是当前语义的唯一文档权威。
- RFC `index.md` / `invariants.md` 表达 accepted target、相对当前契约的 delta，以及只服务该方案或迁移的 RFC-local proof obligations。它们不能在 cutover 前把目标规则写成当前事实。
- transaction devlog 记录阶段事实、review、验证和 contract cutover 证据，不重新定义规则。
- Git 保存所有物理文本历史。契约文档不建立 `v1` / `v2` 副本，也不维护第二套修订号；语义变化由来源 RFC 和生效 transaction 解释。
- register / current limitations 继续保存当前开放问题和已接受缺口，不承担 contract 或迁移计划。

对于尚未提取到契约层的既有 RFC-local 不变量，原 Closed RFC 仍可作为该方案的历史 accepted source；一旦后续 RFC 要跨文档依赖或改变它，必须先把受影响的最小闭包提取为 contract。新的 contract 条目优先于旧 RFC 中同范围的历史规则，旧 RFC 不需要逐份反向改写。

## 按 owner 和 contract surface 组织

目录按稳定的状态或协议 owner 组织，不机械镜像 Rust 文件或模块：

```text
docs/src/contracts/
  <owner>/
    index.md
    <contract-surface>.md
```

- `<owner>/index.md` 定义 owner 边界、owner-wide invariants 和领域内 contract surface 导航。
- `<contract-surface>.md` 保存一组共同变化、共同证明的规则，例如 pick request、wait publication、task exit/reaping 或 file-offset semantics。
- 文件名按协议或能力命名，不使用 `misc.md`、`small-invariants.md`、`state.md` 这类无法说明边界的容器。

是否拆成同一页，优先判断：是否由同一个 owner 推进状态转换、是否共享线性化点或生命周期、是否通常一起修改、是否使用同一组验证闭合。代码位置只作为实现定位，不决定文档身份。

## 不变量放置分级

| 不变量范围 | 规范落点 |
| --- | --- |
| 只约束一个函数、类型或局部实现 | `assert!`、关键注释和定向测试 |
| 只服务某个 RFC 方案或迁移 | RFC `invariants.md` / `implementation.md` |
| 会被多个 RFC、模块或后续设计引用 | 对应 contract surface 的稳定 ID 条目 |
| 具有独立 owner、生命周期、ABI 或验证边界 | 独立 contract surface 文档 |

一条很小但需要跨 RFC 引用的规则通常只是现有 contract surface 下的一条条目，不为每条不变量单独建文件。若它尚无自然归属，先判断它是否真的需要成为共享 contract；若确有独立 owner 或变更轴，即使当前只有一条也可以建立窄文档，不能把无关小规则堆进通用杂项页。

## 最小闭包与按触达迁移

新 RFC 不需要整理整个领域，只需要闭合它的实际影响面：

1. 找出本 RFC 直接 Introduce、Preserve、Refine、Replace、Remove 或 Scoped Exception 的共享规则。
2. 确认每条规则的 owner，以及描述它所必需的直接依赖：状态来源、能力、线性化点、生命周期、ABI 或失败边界。
3. 把这些规则和直接依赖提取到窄 contract surface；明确写出不覆盖的相邻领域。
4. 在 RFC 中用稳定 ID 声明 `Contract Impact`，未触及的领域不做顺手整理。
5. 若无法在不引入大量其它规则的情况下给出唯一 owner，或多个结构都自称同一状态的 owner，停止文档 gate，先修正 owner boundary 或拆分 RFC。

最小闭包的完成标准不是“整个子系统已枚举完”，而是本 RFC 影响的规则都有唯一 owner、直接依赖已定义，且没有另一份当前权威对同一范围给出冲突定义。

## 跨领域不变量

跨领域不等于共同拥有。先区分普通依赖与真实 handoff：

- 如果领域 A 只把领域 B 的规则作为前提，A 的 contract 通过稳定 ID 引用 B，不复制 B 的正文。
- 如果正确性依赖两个领域之间的顺序、能力移交、原子性、取消或 teardown，建立一个接口级 contract。优先放在主导协议 owner 的目录；没有自然主导目录时可以放在 `contracts/interfaces/`，但仍必须声明唯一协议 owner。

接口 contract 必须列出参与领域、每份状态的唯一 owner、各方持有的是状态还是 snapshot/token/capability、handoff 或线性化点、失败与 cleanup 责任，以及各方局部义务。端到端规则只在接口 contract 中完整定义；领域文档只引用 contract ID 并说明自己承担的局部义务。

如果两个领域都能独立推进同一状态、都缓存同一可变真相源，或 cleanup 没有最终负责方，这属于设计 blocker，不能用“共同 owner”文字掩盖。

## 固定外壳与自由正文

契约文档采用固定外壳和受限自由正文：

- 文档头固定给出 contract ID、状态、owner、参与领域、覆盖范围、不覆盖范围、实现位置、依赖、最后核验和 pending successor。
- 每条跨 RFC 不变量使用稳定 ID；ID 不因文件移动或标题变化而改变，retired ID 不复用。
- 每条规则至少说明规范性规则、违反表现、验证/enforcement、最初来源和当前来源。owner、scope 等字段可以从文档头继承，只有不同时才覆盖。
- 状态机、锁序、线性化点、ABI/errno 表、算法证明和生命周期按问题自由组织，不强迫每页保留空章节。
- 规则、理由、实现位置、验证方式和来源必须分开，避免把当前实现偶然形状写成长期 contract。

## 当前契约与 RFC 的边界

RFC 的 `invariants.md` 仍有独立职责，但不再维护整个领域的 current consolidated contract：

1. `Contract Impact`：按稳定 ID 声明 Introduce / Preserve / Refine / Replace / Remove / Scoped Exception、目标摘要和生效 gate；Preserve 项只链接，不复制正文。
2. `Target Invariants`：精确定义尚未生效的新规则。
3. `RFC-local Invariants`：只服务当前方案、probe、迁移桥、阶段原子性或验收的规则；它们不会自动进入长期 contract。

`Introduce` 表示 RFC 新增此前不存在的 effective contract ID。该 ID 在 cutover 前只存在于 RFC target，current rule 为 `None（尚未生效）`；达到 cutover gate 后才在契约层创建 Active 条目。若规则已经由 live code 或 Closed RFC 生效、只是尚未迁入契约层，应先提取最小 baseline，再按真实 delta 分类，不能使用 `Introduce` 把既有行为伪装成新增语义。

生命周期如下：

| 阶段 | RFC | 当前契约 | Transaction |
| --- | --- | --- | --- |
| Draft | 提议 delta 和 target | 保持当前 effective 规则 | 不存在 |
| Accepted for Implementation | accepted target | 保持 effective 规则；可增加 pending successor 链接 | 记录目标修订与计划 cutover |
| Cutover gate | 保留目标和理由 | 原子更新受影响 ID、来源和生效证据 | 记录实际生效与验证 |
| Closed | 作为决策和迁移历史 | 继续作为当前权威 | 保留执行证据 |

若一个 RFC 分阶段切换多个独立 contract ID，可以逐项 cutover；如果中间阶段形成可被其它代码依赖的长期可见规则，它本身必须被明确记录为当前 transitional contract，并带删除 gate。纯文档语义校正、或 RFC/实现/验证在同一原子变更中完成时，接受点可以同时是 cutover，但仍需保留变化原因和证据入口。

## Supersession 与链接

- RFC 指向当前 contract ID，并声明 delta；不复制未改变的规则。
- contract 在 cutover 后记录最初引入来源、最近一次语义改变的 RFC 和生效 transaction，不维护所有引用者 backlink。
- Closed RFC 正文不因后续 contract 变化逐份回改。必要的旧页提示可以是轻量导航，但不是 current truth 的维护条件。
- contract 被替换时原地维护当前规则；旧规则由 Git 与来源 RFC 恢复。retired ID 只保留 `ID -> successor / removal source` 的短映射，不复制旧正文。

## 当前登记

契约层从本规则生效后按触达迁移。当前不批量把既有 RFC 的不变量搬入 `docs/src/contracts/`；首个需要跨 RFC 修改或复用既有共享规则的 RFC，应按本页提取最小 contract 闭包，并把新入口加入本节和 `docs/src/SUMMARY.md`。

- [Signal 当前契约](./contracts/signal/index.md)
  - [Pending routing 与 ordinary action selection](./contracts/signal/pending-routing.md)
  - [Temporary-mask delivery handoff](./contracts/signal/temporary-mask-delivery.md)
- [Procfs 当前契约](./contracts/procfs/index.md)
  - [TGID task-state projection](./contracts/procfs/task-state-projection.md)
- [Task 当前契约](./contracts/task/index.md)
  - [Process-group signal targeting](./contracts/task/process-group-signaling.md)
  - [ThreadGroup lifecycle](./contracts/task/thread-group-lifecycle.md)
  - [Child wait](./contracts/task/child-wait.md)
  - [Ordinary user entry](./contracts/task/user-entry.md)
