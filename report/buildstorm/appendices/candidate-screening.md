# 候选筛选记录

我们的优化流程同时记录成功项与高价值的筛选结论，用同一套机制证据、正确性 oracle 和端到端门槛把实现预算集中到系统收益明确的方向。

| 候选 | 机制证据 | 端到端证据 | 处置 |
| --- | --- | --- | --- |
| Signal empty-return fast path | 99.79% 的 user-entry decision 走 fast skip | Cargo confirmation 改善 5.03% | 正式采用；作为辅助优化简述 |
| Page-table range walker | PTE slot 检查减少约 84.9% | 两个相邻 Cargo 比较均未改善 | 归档；确认局部工作量并非当时主瓶颈 |
| Userptr 8-byte wide access | 57.010% completed bytes 实际走 wide body；双架构 correctness 通过 | ABBA 未越过 3% 门槛且顺序漂移明显 | 暂缓；正确性成立，性能证据不足 |

其中 Signal 正式实现为 `343e8c15`。Page-table walker 和 userptr wide body 在完成研究并归档证据后结束候选生命周期，使正式代码继续聚焦于已经通过完整门槛的优化。

筛选的价值在于保护主线：减少某个计数器、循环次数或汇编指令，只能证明机制发生变化；只有 recording-disabled 的完整 workload 同时通过正确性 oracle 和预声明收益门槛，才形成生产采用建议。
