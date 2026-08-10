# Positive dentry residency

## 研究问题

Cargo 和 `rustc` 会重复访问大量源码、元数据和构建产物。Anemone 的 filesystem backend 始终是 namespace 真相源，而 parent child map 只保存 live dentry 的弱引用；当最后一个强引用释放后，相同 pathname 的下一次访问必须重新进入 backend lookup。

研究阶段先用有界强引用队列验证机制。在固定 Cargo workload 中，backend lookup 从 4,418 次降至 638 次，下降 85.56%；`live hit + backend lookup` 总量保持 13,520，说明工作没有被删除，只是更多请求在 VFS 层命中。该反事实只用于确认方向，没有直接成为生产设计。

## 正式设计

正式实现 `4214f95c9b04689f8bd922aec4e9a31ec8ac2c96` 使用 per-SuperBlock、有界、best-effort 的 positive residency：

- ext4 显式 opt in，production 默认容量为 1024；
- backend 继续拥有持久 namespace，residency 只延长已 materialize dentry 的生命周期；
- lookup/create 使用 generation ticket，成功的 unlink、rmdir 和 rename 会推进 generation 并忘记精确 identity；
- admission 失败只退化为不保留，不改变 syscall 结果；
- eviction、forget 和 drain 在 owner lock 内摘除引用、在锁外 drop；最后一个 mount view 卸载时 drain。

这套设计避免把研究用 16K FIFO、临时 metric 或 backend callback 带入正式内核，也不宣称 negative cache、全局 dcache 或用户可见缓存保证。

## Production A/B

性能 A/B 固定为 `825d4cc6541687ee750131ad2161d58505ba27d4` 与 `44abcddac727c8f74b953bd8ff41f37de3bfd954`；两者之间的源码差异由正式提交 `4214f95c9b04689f8bd922aec4e9a31ec8ac2c96` 交付。实验在 RV64、SMP=1、8 GiB、QEMU release 上执行 fresh-boot `A1 -> B1 -> B2 -> A2`，每个 boot 在 warmup 后运行七次 recording-disabled clean Cargo，并以 boot median 裁决：

| 状态 | 两个 boot median | 趋势中位数 |
| --- | ---: | ---: |
| baseline | 4.22 / 3.92 s | 4.070 s |
| production | 3.72 / 3.73 s | 3.725 s |

Cargo 改善 8.48%，两个 production median 均低于 baseline 区间。direct `rustc` 从 2.810 s 降至 2.495 s，同向改善 11.21%。四个 boot 均通过完整 KUnit、构建产物 hash、recording 恢复、timer 外 sync 和 orderly PowerOff。

该结论证明默认容量 1024 在冻结 workload 中保留了研究收益；容量、replacement 与 memory-pressure 行为继续作为实现策略，而不是对外性能保证。
