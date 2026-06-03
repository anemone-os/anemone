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
  implementation.md
  invariants.md          # 可选；协议、不变量或证明义务复杂时使用
  tracking-issues.md     # 可选；实现期需要持续分级跟踪问题时使用
  backgrounds/           # 可选；保存历史背景、问题清单和被拒绝方案
    index.md
```

`index.md` 是总入口，负责说明状态、范围、文档地图、接受边界和下一步。`implementation.md` 是实现计划，负责记录阶段、审查合同、验证和停止边界。`invariants.md` 只在正确性依赖明确协议、不变量或证明义务时创建；调度、等待、锁序、生命周期等子系统通常需要它。`tracking-issues.md` 只在实现期存在一组需要持续 review、分级和关闭的问题时创建。`backgrounds/` 只保存背景材料，不作为当前 canonical 结论的来源。

大型重构应把不变量、实现顺序、历史材料拆成同一目录下的子文档，避免 devlog 或 register 直接引用个人 `etc/` 草稿。

每个 RFC 入口都应在页首明确给出：

- `状态`
- `负责人`
- `最后更新`
- `领域`
- `开放问题`
- `下一步`

可直接复制的草案结构见 [RFC 模板](./rfc-template.md)。

## 实现期事务日志

RFC 一旦进入实现阶段，必须建立对应的事务级 devlog：

```text
docs/src/devlog/transactions/YYYY-MM-DD-<short-slug>.md
```

同时更新：

- RFC `index.md` 页首的 `事务日志` 字段；
- `docs/src/devlog/transactions/index.md`；
- 当前双周 devlog，只追加该事务的入口摘要；
- `docs/src/SUMMARY.md`，让 RFC 和事务日志都出现在 mdBook 导航中。

事务日志记录实际执行、checkpoint、review 结论、验证证据、剩余限制和更正说明；RFC 记录计划、边界和 accepted contract。事务日志应链接回 RFC，RFC 也应链接到事务日志。

## Tracking Issues

不是每个 RFC 都需要 `tracking-issues.md`。只有当问题清单会影响实现顺序、review gate、停止边界或验收判断时，才在 RFC 根目录创建它。

`tracking-issues.md` 是当前问题跟踪页，不是历史归档：

- 当前仍影响实现或验收的问题放在 `tracking-issues.md`；
- 已过期的旧问题清单、被否决方案和历史 review 材料放在 `backgrounds/`；
- 实际阶段推进、checkpoint、验证证据和更正说明仍写入事务日志；
- 不要用它替代 GitHub issue、PR 讨论或双周 devlog。

问题等级必须使用当前 review skill 的名称：

- `Apollyon`：错误结果、数据损坏、安全问题、崩溃或严重不可恢复状态，必须修。
- `Keter`：不会马上爆炸，但会阻塞后续开发或把核心抽象带错方向，必须修。
- `Euclid`：通常值得修，但不阻塞主线。
- `Safe`：记录即可，默认不修，除非局部且低成本。
- `Neutralized`：已经处理完成的问题；必须保留 neutralize 依据和对应事务日志条目。

旧文档可能仍出现 `P0/P1/P2/P3` 历史称呼；新增 RFC、review 输出和 tracking issue 不再使用这些旧等级名。

## 当前 RFC

- [RFC-20260602-cred-merge](./rfcs/cred-merge/index.md)：credentials feature merge 的 canonical 执行计划和审查合同。
- [RFC-20260603-sched-latch](./rfcs/sched-latch/index.md)：`poll` / `select` OR wait 所需的 wait-core latch 原语和 iomux 迁移计划。
- [RFC-20260601-sched-wait-refactor](./rfcs/sched-wait-refactor/index.md)：已完成的 scheduler wait/wake 协议重构 RFC。

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
