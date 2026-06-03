# 公开草案与 RFC

公共仓库中的草案只用于共享评审尚未定稿、但已经需要协作讨论的方案。

不是所有个人草稿都需要进入仓库；只有当一个问题已经进入共享决策流程时，才需要公开草案页面。

## 什么时候需要公开草案

满足以下任一条件时，适合创建公开草案：

- 方案影响多个子系统；
- 方案会改变 ABI、兼容性或外部契约；
- 方案需要跨人、跨时间异步评审；
- 方案预计会经历多轮讨论，且结论需要长期追踪。

## 存放方式

公开草案统一放在 `docs/src/rfcs/` 下，并默认使用目录级 RFC：

```text
docs/src/rfcs/<short-slug>/
  index.md
  invariants.md
  implementation.md
  background/
    index.md
```

`index.md` 是总入口，负责说明状态、范围、文档地图、接受边界和下一步。大型重构应把不变量、实现顺序、历史材料拆成同一目录下的子文档，避免 devlog 或 register 直接引用个人 `etc/` 草稿。

每个 RFC 入口都应在页首明确给出：

- `状态`
- `负责人`
- `最后更新`
- `领域`
- `开放问题`
- `下一步`

可直接复制的草案结构见 [RFC 模板](./rfc-template.md)。

## 当前 RFC

- [RFC-20260602-cred-merge](./rfcs/cred-merge/index.md)：credentials feature merge 的 canonical 执行计划和审查合同。
- [RFC-20260601-sched-wait-refactor](./rfcs/sched-wait-refactor/index.md)：scheduler wait/wake 协议重构的 canonical 不变量和迁移计划。

## 目录级 RFC 何时必需

满足以下任一条件时，必须使用目录级 RFC，而不是单文件草案：

- 迁移跨多个子系统，且需要阶段性实施计划；
- 方案正确性依赖明确不变量或协议证明；
- 需要保留历史备选、问题清单、review 结论或验证证据；
- devlog 事务日志需要引用该计划作为 canonical source。

## 如何避免误导 agent

只要边界清楚，公开草案不会误导 agent。

关键在于：

- 当前事实仍写在主文档、活动记录和已接受的决策记录中；
- 草案只陈述提议、问题与待决事项；
- 草案一旦被接受，其结论应迁移到当前事实页面、决策记录，或在 RFC 目录内标记为 canonical implementation source。

换句话说，草案是输入材料，不是当前事实本身。
