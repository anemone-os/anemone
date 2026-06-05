# 模板

以下模板可直接复制使用。条目应保持简短、事实化。活动登记册默认只写最小必填字段；只有在可选字段能明显降低沟通成本时，才把它们加上。

## 开发日志条目

```md
## 2026-05-22 - 简短任务标题

**Date:** 2026-05-22
**Authors:** name1, name2
**Area:** scheduler / futex

**Summary:** 用一句话说明这次改了什么。

**Motivation / Symptom:** 触发这次工作的失败现象、任务目标或观察。

**Change:** 实际发生的行为或结构变化。

**Validation:** 实际运行的命令、测试或复现步骤。

**Follow-up:** 仍然开放、存在风险或明确延期的内容。

**Related:** issue ID、决策记录、公开调查材料。
```

## 小迭代记录

默认使用单文件。如果需要背景材料，可以使用同名目录，并把下面的记录本体放在 `index.md`。

```md
# ANE-CHG-20260522-short-slug

**Type:** Bugfix / Small Feature / Cleanup / Investigation
**Status:** Completed / Reverted / Superseded / Follow-up
**Date:** 2026-05-22
**Authors:** name1, name2
**Area:** scheduler / futex

## Trigger

触发这次工作的失败现象、任务目标或观察。

## Scope

本次只改什么；明确不改什么。

## Change

实际发生的行为或结构变化。必要时列出关键文件、commit 或语义边界。

## Validation

实际运行的命令、测试、复现步骤，或说明验证由用户运行 / 尚未运行。

## Risk / Follow-up

仍然开放、存在风险、明确延期，或需要 register / current limitations 记录的内容。

## Links

- Biweekly devlog:
- Register / limitations:
- RFC / transaction:
- Issue / PR / commit:
```

目录版小迭代记录可以使用以下形态：

```text
docs/src/devlog/changes/2026-05-22-short-slug/
  index.md
  backgrounds/
    ltp-evidence.md
    linux-reference.md
```

`backgrounds/` 只保存证据摘要、Linux / LTP 对照、历史材料或运行记录；不要在小迭代记录中维护 accepted contract、不变量、阶段计划或 review tracking issues。

## 事务日志

```md
# 2026-05-22 - 简短事务标题

**Status:** Active / Blocked / Completed
**Owners:** name1, name2
**Area:** scheduler / futex / timer
**Canonical Plan:** 计划、不变量文档或 RFC 链接。
**Current Phase:** 阶段名或阶段编号。

## Scope

这次事务要完成什么，不包含什么。

## Invariants

- 必须一直保持的不变量。
- 阶段性交付不能破坏的边界。

## Phase Log

### 2026-05-22 - 阶段标题

**Phase:** 阶段编号或名称。
**Change:** 本阶段实际推进的内容。
**Audit:** 旁路审计、关键命中分类或 review 结论。
**Observability:** 新增或验证的 debug / trace / 断言 / 日志证据。
**Validation:** 实际运行的命令、测试或复现步骤。
**Next:** 下一阶段入口条件。

## Open Items

- 仍未完成但属于本事务范围的事项。

## Closure

事务完成时记录最终验证、剩余限制和相关 register / devlog 链接。
```

## 问题条目

```md
## ANE-0001

**Type:** Issue
**Status:** Open
**Area:** VFS / procfs

**Symptom / Trigger:** 简洁的复现条件。

**Impact:** 对用户或开发者造成的可见影响。

**Owner:** name
**Last Verified:** 2026-05-22
**Exit Condition:** 满足什么条件后可以关闭该条目。
**Related:** 开发日志、GitHub issue / PR、决策记录、调查笔记。
```

问题条目可按需补充：

```md
**Severity:** High
**Workaround:** 临时规避手段，或 `None`。
**First Seen:** 2026-05-22
**Tracker:** GitHub issue / PR / 其他长期讨论入口。
```

## 限制条目

```md
## ANE-0002

**Type:** Limitation
**Status:** Active
**Area:** VFS / openat

**Summary:** 简洁说明当前阶段接受的能力缺口或语义缩减。

**Owner:** name
**Last Verified:** 2026-05-22
**Exit Condition:** 满足什么条件后可以取消该限制。
**Related:** 开发日志、GitHub issue / PR、决策记录、调查笔记。
```

限制条目可按需补充：

```md
**Severity:** Medium
**Workaround:** 当前存在的临时路径；如果没有可写 `None`。
**First Seen:** 2026-05-22
**Tracker:** GitHub issue / PR / 其他长期讨论入口。
```

## 功能性测例状态行

```md
| basic | basic_testcode.sh | 待填写 | 待填写 | 待填写 | |
```

## 性能 Bench 状态行

```md
| cyclictest | cyclictest_testcode.sh | 待填写 | 待填写 | 待填写 | 待填写 | |
```

## 决策记录

```md
# ADR-20260522-short-slug

**Status:** Accepted
**Owners:** name1, name2
**Related:** 开发日志、问题条目、调查笔记。

## Context

是什么问题或权衡迫使我们做这个决策？

## Decision

最终选择了什么？

## Consequences

这个决策让什么事情更容易、更困难，或变成了必须？

## Rejected Alternatives

考虑过哪些合理方案，为什么没有选它们？

## Invalidation Signals

未来出现什么证据时，这个决策应被认为错误或过时？
```

## RFC 模板

RFC 使用单独页面模板，见 [RFC 模板](./rfc-template.md)。
