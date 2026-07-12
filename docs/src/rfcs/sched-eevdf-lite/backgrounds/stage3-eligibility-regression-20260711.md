# Stage 3 eligibility 与整体吞吐回归证据（2026-07-11）

本文抽取 Stage 3 用户运行与只读 probe 中已经稳定的事实，用于解释为什么 RFC 必须停止在阶段 3 并重开 Checkpoint 2C / 2D。本文不定义修复计划或最终 contract；correction gate 见 [迁移实施计划](../implementation.md)，当前设计问题状态见 [Tracking Issues](../tracking-issues.md)。

## 证据边界与 provenance

- 公共实现基线为 `a76a00ac`。
- evidence-only probe 为 `d0d4196f`，只用于关联 explicit yield、紧邻 dispatch、actual eligibility 分类和同一 entity snapshot 上的 weighted-fair-clock counterfactual。
- probe commit 同时包含临时 instrumentation、profile 选择和关机汇总，不是 feature commit，不应整体合入产品分支。
- 原始用户日志不进入公共仓库；本文只保留运行环境、决定性计数、自校验结果和结论强度。
- 所有 runtime 数据均为用户运行证据，不是 agent 在本轮文档修订中复跑的结果。

## 运行环境与整体回归

除特别说明外，运行环境为 rv64 单核 QEMU TCG，`system_hz = 100`，base slice、wake clamp window 和 yield penalty window 均为 `3000us`。`read-write` profile 覆盖 glibc / musl 共 118 个 case；所有对照运行最终结果均为：

```text
attempted=118 passed=96 failed=16 infra_failed=0 skipped=6
```

统一 profile 区间从 `profile_start` 到 musl `fsync01` 开始，适合横向比较，但不包含最后一个 case 的完整收尾：

| 运行 | 变化 | Profile 区间 |
| --- | --- | ---: |
| RR-A | RR baseline | 56.855s |
| RR-B | RR repeated baseline | 58.260s |
| EEVDF-A | default EEVDF | 194.116s |
| EEVDF-B | repeated default EEVDF | 201.960s |
| EEVDF-C | 不运行用户态 `eevdf-test` | 192.762s |
| EEVDF-D | 同时禁用本轮标注的 set-nice 行为 | 194.461s |
| EEVDF-E | 三个 slice / window 均改为 `30000us` | 192.038s |
| EEVDF-F | `system_hz = 500` | 217.239s |

两份 RR 的平均区间约为 `57.56s`。除单次 `500Hz` 运行外，EEVDF 区间集中在 `192.04s` 至 `201.96s`，约为 RR 的 3.3 至 3.5 倍。相同 case / failure multiset 排除了“多跑了不同测试集合”这一解释；它仍不能单独证明某个 scheduler mechanism 对全部差值的因果贡献。

## Signal profile 的 exact-yield 关联

相同 signal profile 的 EEVDF 与 RR 运行最终结果均为：

```text
attempted=74 passed=60 failed=10 infra_failed=0 skipped=4
```

这两份 signal 运行的端到端用时分别为 EEVDF `78s`、RR `57s`。这是各一份用户运行值，不构成独立的波动范围或最终性能预测；后续 direct-causality 验证规则见 [Gate R1](../implementation.md#gate-r1---direct-weighted-fairclock-repair)。

probe 对每次 explicit yield 与紧邻 dispatch 做全量关联：

| 指标 | EEVDF | RR |
| --- | ---: | ---: |
| explicit yield | 1,494,290 | 184,858 |
| 总 dispatch | 1,496,129 | 186,624 |
| 非 yield dispatch | 1,839 | 1,766 |
| yield self-pick | 1,339,030 | 278 |
| yield handoff | 155,260 | 184,580 |

EEVDF 的 explicit yield 和总 dispatch 均约为 RR 的 8 倍，差异几乎全部来自 same-task dispatch；有效 handoff 反而比 RR 少。EEVDF actual pick 的分类为：

| actual 分类 | 次数 | 结果 |
| --- | ---: | --- |
| `no_peer` | 216 | self-pick |
| `yielding_ineligible` | 154,468 | handoff |
| `self_only_eligible` | 1,338,814 | self-pick |
| `self_earliest_deadline` | 0 | self-pick |
| `peer_earlier` | 786 | handoff |
| `peer_equal` | 6 | handoff |

`penalty_raised=1,493,899`，占全部 EEVDF yield 的 99.974%；`self_earliest_deadline=0`。因此大量 self-pick 不是 penalty 未生效，也不是 yielding task 在 eligible peers 中仍有最早 deadline，而是 peers 在 deadline 比较前已经被 min-floor eligibility 排除。

这一路径不会进入“non-empty queue 无 eligible task”的 fallback，所以已有 `NoEligibleTask` anomaly 不会报告该反馈环。

## Weighted fair clock counterfactual

counterfactual 在 actual pick 使用的同一 entity / weight snapshot 上计算精确加权平均虚拟时间。以最小 `vruntime` 为无符号原点 `v0`：

```text
A = sum((v_i - v0) * w_i)
W = sum(w_i)
entity i eligible iff A >= (v_i - v0) * W
```

这与 `v_i <= sum(v_j * w_j) / sum(w_j)` 等价，但不需要直接构造绝对加权和。所有乘加使用 checked `u128`。

在 min-floor actual path 产生的 `1,338,814` 次 `self_only_eligible` 中：

| Weighted-fair-clock 重新判断 | 次数 | 占比 |
| --- | ---: | ---: |
| 至少一个 peer eligible | 552,494 | 41.267% |
| 没有 peer eligible | 786,320 | 58.733% |
| 当前 Anemone penalty 会 handoff | 496,415 | 37.079% |
| Linux 风格 additive penalty 会 handoff | 532,281 | 39.758% |

由这些数据可以确定：

1. monotonic minimum-`vruntime` floor 不是正确的 eligibility fair clock；它在至少 41.267% 的 actual singleton snapshots 中错误排除了已经有资格竞争的 peer。
2. 修复不能退化为“只要 current yield 就强制 handoff”。同一批 snapshots 中仍有 58.733% 在 weighted fair clock 下没有 eligible peer；无条件跳过 current 会伪造 owed-service / lag 状态。
3. penalty 形式是次级变量。两种 penalty 在这批 snapshots 上只相差 35,866 次 handoff，不能解释 1,338,814 次 min-floor singleton set。
4. `552,494` 是单步 counterfactual 计数，不是修复后的性能预测。改变 fair clock 会继续改变后续 pick、accounting、placement 和 lag 轨迹。

## Probe 自校验

本次 `1,494,290` 个 EEVDF yield samples 中：

```text
invalid=0
mismatch=0
pending_overwrite=0
missing_yielding=0
missing_pick=0
```

actual probe 的守恒关系成立：

```text
1,339,030 self-pick + 155,260 handoff = 1,494,290 yield
```

因此，上述 eligibility 结论来自全量 exact correlation，而不是抽样估计。它足以否定现有 min-floor contract；它尚不能在 intervention 前证明这一 mechanism 对端到端 3.3 至 3.5 倍回归贡献了多少。

## 已排除或降级的解释

- 用户态 `eevdf-test`：移除后整体区间没有改善；它是 progress / fairness smoke，不是吞吐 benchmark。
- 本轮标注的 set-nice 行为：禁用后没有改善，不是必要触发条件。
- `3ms` slice 小于 `10ms` tick：存在量化压力，但把三个窗口都改为 `30000us` 没有改善。
- 提高 `system_hz`：单次 `500Hz` 运行没有改善，不能作为当前修复方向。
- penalty 经常 no-op 或太小：99.974% 的 yield 已抬高 deadline，且 `self_earliest_deadline=0`。
- ready queue 的渐进复杂度：观测到的最大 ready 长度为 24；线性扫描可能放大成本，但不是百万级 self-pick feedback 的第一原因。
- 更多有效 context switch：EEVDF handoff 更少，额外 dispatch 几乎全部是 self-pick。

runtime 换算、软件除法、线性扫描、entity lock 和 QEMU TCG 仍可能是 feedback 修复后的次级成本。只有 correction intervention 消除 singleton feedback 后仍有显著回归，才有证据重新提升这些方向。

## 公开结论边界

本证据包支持以下 RFC 动作：停止阶段 3、撤销 min-floor eligibility 的 neutralized 结论、禁止 forced-handoff / penalty-tuning 窄修，并为单变量 fair-clock correction 建立 runtime intervention gate。

本证据包不证明最终 lag / placement / accounting 表示已经闭合，也不证明 fair-clock correction 一定消除全部吞吐差距。后续结论必须由 [迁移实施计划](../implementation.md) 中各 correction gate 的独立证据关闭。

## R1 intervention 结果与最终处置（2026-07-12）

R1 在独立 validation 环境中把 actual eligibility 从 min-floor 替换为 weighted FairClock，并保持同一 signal case set 与 probe 观察语义。用户运行结果仍为：

```text
attempted=74 passed=60 failed=10 infra_failed=0 skipped=4
yield=1,393,625
yield self-pick=1,233,143
self_only_eligible=1,232,735
handoff=160,482
invalid=0 mismatch=0 pending_overwrite=0 missing_yielding=0 missing_pick=0
```

与修复前相比，yield 从 `1,494,290` 降至 `1,393,625`，yield self-pick 从 `1,339,030` 降至 `1,233,143`；公式替换产生了有限变化，但 actual trajectory 仍由约 123 万次 singleton eligibility / same-task feedback 主导。这明确命中 Gate R1 的“百万级重复 self-pick 仍主导”失败信号，因而不能把 weighted FairClock 公式落地解释成 R1 runtime closure，也不能自动进入 R2。

最终决定是延期关闭 RFC、恢复 RR 为 production default，并保留 EEVDF 作为实验实现。R2 / R3a / R3b 未执行；eligibility、competition membership、sleep/wake lag 与 accounting contract 仍未闭合。`EEVDF-001` / `EEVDF-018` / `EEVDF-004` / `EEVDF-020` 保持未解决 Keter。本文仅保留用户运行的稳定计数与结论，不链接私有原始日志或 validation 分支路径。
