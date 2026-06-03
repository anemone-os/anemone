# RFC 模板

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
  backgrounds/           # 可选
    index.md
```

`index.md` 与 `implementation.md` 必须存在。`invariants.md` 在协议、不变量、锁序、生命周期或证明义务复杂时创建。`backgrounds/` 用于历史背景、问题清单、被拒绝方案和旧计划归档；背景材料不能覆盖 canonical 结论。

RFC 一旦进入实现阶段，必须创建对应事务日志：

```text
docs/src/devlog/transactions/YYYY-MM-DD-<short-slug>.md
```

并在 RFC `index.md`、事务日志索引、当前双周 devlog 和 mdBook Summary 中建立链接。

## `index.md`

```md
# RFC-YYYYMMDD-short-slug

**状态：** Draft / Accepted for Implementation / Superseded / Closed
**负责人：** name1, name2
**最后更新：** YYYY-MM-DD
**领域：** scheduler / fs / mm / ...
**事务日志：** Draft 阶段可写 `None`；进入实现阶段后必须链接对应 transaction。
**开放问题：** 简短列出待决问题；没有则写 `None`。
**下一步：** 下一次 review、原型、迁移阶段、验证或收口动作。

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

Canonical：

- [不变量需求](./invariants.md)
- [迁移实施计划](./implementation.md)

背景材料：

- [背景材料索引](./backgrounds/index.md)

## 方案

概括最终方向。如果细节很多，这里保持短摘要，并链接到 canonical 子文档。

## 接受边界

说明本 RFC 被接受意味着什么，哪些内容仍需审查，以及哪些变化必须回到本 RFC 或 follow-up RFC。

## 备选方案

记录考虑过的合理备选方案，以及为什么拒绝或延期。

## 风险

- 风险 1 及控制方式。
- 风险 2 及控制方式。

## 收口

完成后记录最终验证、剩余限制，以及 register / devlog 链接。
```

## `invariants.md`

```md
# <标题> 不变量需求

**状态：** Draft / Canonical / Superseded
**最后更新：** YYYY-MM-DD
**父 RFC：** [RFC-YYYYMMDD-short-slug](./index.md)

## 闭合条件

- 必须满足的条件 1。
- 必须满足的条件 2。

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

- 声明协议闭合的标准。
- 只能声明为迁移中间态的标准。
```

## `implementation.md`

```md
# <标题> 迁移实施计划

**状态：** Draft / Active / Completed
**最后更新：** YYYY-MM-DD
**父 RFC：** [RFC-YYYYMMDD-short-slug](./index.md)
**不变量：** [不变量需求](./invariants.md)

## 迁移原则

- 原则 1。
- 原则 2。

## 阶段 1：简短阶段名

前置条件：

- 开始前必须满足的条件。

交付：

- 具体交付 1。
- 具体交付 2。

审计：

- 本阶段需要执行的搜索、review 或分类。

可观测性：

- 本阶段要求的 debug / trace / assertion 证据。

验证：

- 命令、测试、stress profile 或证明材料。

退出条件：

- 进入下一阶段的标准。

## 旁路审计清单

列出精确代码搜索、分类方式和允许保留旁路的理由。

## 可观测性清单

列出后续 review 必须能依赖的 logs / traces / assertions。

## 停止边界

说明什么时候应继续追查 issue，什么时候应停止实现形状争论。
```

## `backgrounds/index.md`

```md
# <RFC 标题> 背景材料

本目录保存 [RFC-YYYYMMDD-short-slug](../index.md) 的历史上下文。

Canonical：

- [不变量需求](../invariants.md)
- [迁移实施计划](../implementation.md)

历史材料：

- [问题简述](./problem-brief.md)
- [被否决的窄化方案](./rejected-narrow-fix.md)
```
