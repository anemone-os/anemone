# 冻结候选 Patch

本目录保存不能仅用 Git commit 表达、但被正文性能结论引用的冻结候选源码差异。

- [`user-fault-local-tlb.patch`](./user-fault-local-tlb.patch)：应用于 baseline `2b2b2a1de57d28a9ec9bdf4847582af51234a081`；SHA-256 为 `3edab6a1649773cd4fd6bcdf64f5cb55a3556f04938074c145353cb389a7b529`。

Patch 只复现实验候选，不替代后续正式实现的 commit 链与多核 correctness closure。
