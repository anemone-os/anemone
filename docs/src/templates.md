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