# 模板

以下模板可直接复制使用。条目应保持简短、事实化。活动登记册默认只写最小必填字段；只有在可选字段能明显降低沟通成本时，才把它们加上。RFC、small-change 或调查证据引用仓库公共参考源码时，使用[外部源码引用规则](./external-source-references.md)；不得把私人 checkout 路径写入公共文档。

## 双周开发日志条目（可选）

双周日志不是 workflow gate；只有确需人工时间线时才使用。

```md
## 2026-05-22 - 简短任务标题

**Area:** scheduler / futex
**Summary:** 一句话说明发生了什么。
**Validation:** 已运行、用户运行或 Not Run。
**Related:** change record / RFC / issue / commit。
```

## 小迭代记录

默认使用单文件。没有真实内容的可选章节直接删除。

```md
# ANE-CHG-20260522-short-slug

**Type:** Bugfix / Small Feature / Cleanup / Investigation
**Date:** 2026-05-22
**Authors:** name1, name2
**Area:** scheduler / futex

## Problem / Context

触发本轮工作的症状、目标、证据或不明显根因；说明为什么需要长期记录，而不是保持 Patch。

## Decision

局部方案、关键语义或兼容取舍，以及为什么不需要 RFC。

## Change

实际行为或结构变化；必要时列出 Implementation Boundary、commit 或受影响 surface。

## Validation

实际运行的命令、测试或复现；区分 agent 运行、用户运行和 Not Run。

## Remaining Risk / Links

- Current contract / register / limitation：
- RFC / optional transaction：
- 外部源码：`xref:<source-id>:<repo-relative-path>#<locator>` / `None`
- Issue / PR / commit：

## Contract Impact / Cutover（仅原子 contract-bearing small change）

| Contract ID | 变化 | Cutover 前 baseline | 新规则 | 生效证据 |
| --- | --- | --- | --- | --- |
| SCHED-WAKE-001 | Replace | 当前规则与来源 | 新规则摘要 | source/runtime/commit |

说明代码与 contract 的单一原子 cutover、失败时保持旧规则，以及 current contract 唯一正文链接。

## Architecture Friction（仅存在时）

- 等级：Euclid / Keter / Apollyon。
- 证据：具体代码路径、状态/owner/lifecycle 模型。
- 模型偏差与影响：局部可逆，还是阻碍当前/下一步。
- 路由：当前边界内修正、Patch/小迭代、RFC review 或停止。
```

`Tracking Issues` 只在当前小迭代确有尚待关闭的局部 concern 时增加；修复后把结论折回正文。不要拆出独立 `tracking-issues.md`、`invariants.md` 或 `implementation.md`。需要 probe、transitional contract、多个语义 checkpoint、target renegotiation 或本轮无法关闭的 Apollyon/Keter 时升级 RFC。

## 事务日志（按需）

只有长期、多 checkpoint、多 cutover、probe/renegotiation 或 handoff 需要独立执行历史时才使用。

```md
# 2026-05-22 - 简短事务标题

**Status:** Active / Blocked / Completed
**Owners:** name1, name2
**Canonical Target:** RFC 与修订链接。
**Contract Delta:** 实际变化的 IDs；没有写 `None`。

## Scope

说明本 transaction 为什么需要独立存在，以及它不承担的 target/计划正文。

## Checkpoint Log

### 2026-05-22 - Checkpoint 标题

**Change:** 实际完成内容。
**Review / Feedback:** review、Route Correction 或 Target Renegotiation；没有写 `None`。
**Contract Cutover:** 实际切换的 IDs 与证据；没有写 `None`。
**Validation:** 已运行、用户运行和 Not Run。
**Architecture Friction:** 仅存在时记录 Euclid；Keter/Apollyon 必须停止并记录代码处置。
**Next / Stop:** 下一入口或停止条件。

## Closure

最终交付、验证、cutover/Not Cut Over、剩余问题/限制和证据链接。
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

## 当前契约模板

跨 RFC 已生效规则使用单独页面模板，见 [当前契约模板](./contract-template.md)。不要在小迭代或 transaction 中复制 current contract 正文。
