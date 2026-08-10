# TLB 优化研究

## 研究问题

软件构建会频繁创建线程、分配用户页并修改页表。早期实现对这些不同性质的操作统一采用立即、逐页的 TLB 失效，正确但保守。研究首先区分两类成本：大范围内核映射的重复指令，以及新增用户映射完成后并非总有必要的立即失效。

## 内核栈范围失效

`pthread.createjoin_minimal1` 的路径分解显示，512 KiB 内核栈包含 128 个映射页和一个 guard page。一次完整的创建与回收会执行 257 次逐页本地失效；2506 个生命周期形成 5,012 次 range operation，累计覆盖 644,042 个页面。

反事实实验保持栈大小、guard page 和 PTE 工作量不变，只将大范围逐页失效替换为一次全地址空间本地失效。正式实现随后把选择策略收归架构分页层：小范围仍逐页处理，大范围由 Kconfig 阈值选择一次 full flush。RV64 正式策略在 recording-disabled 微负载中，相对逐页基线的两个可比位置分别改善 16.7% 和 14.8%。机制窗口精确记录 644,042 个 range pages、5,012 次 full flush 和 0 次 page flush，证明收益来自失效策略，而不是缩小内核栈。

公开身份：正式实现 `0e68c9736b0fa6f95374127308dd1790e4f559c3`；RV64、SMP=1、QEMU release；基准为 2500 个 serial create/join，五次重复。LA64 的对应大范围策略由 `b267d2bc6ce89194da8502700d42dd6165542432` 实现，但上述百分比只属于 RV64 实验。

## 用户缺页的延迟本地完成

第二项研究把页表提交关系分为 `Added`、`Unchanged`、`Relaxed` 和 `ReplacedOrRestricted`，并区分返回用户态和内核立即重试两种 continuation。只有“actual user fault + `Added` + 返回用户态”延迟本地失效；替换、权限收紧、显式 fault-in、futex 和 userptr 恢复仍立即完成。若硬件缓存了无效或受限 translation，同一指令再次 fault 时关系不再是 `Added`，内核会立即失效后重试。

冻结的 RV64/QEMU TCG Cargo confirmation 严格按 `A1 -> B1 -> B2 -> A2` 执行。same-boot Cargo 的 baseline 为 5.34/6.14 s，candidate 为 3.78/3.92 s，中位数从 5.740 s 降至 3.850 s，改善 32.93%。机制窗口中 `Added` 占 actual fault 的 97% 以上，且与 deferred completion 一一对应。

该数字证明优化方向和 QEMU TCG 下的收益，不等同于最终 BuildStorm 增益。正式内核由 `777b031a` 建立 local completion policy，`503d6522` 闭合 destructive mapping 的 completion、ack 与资源退休顺序，随后由 `23a9172496d4cdf662444f598b1581e296b3ba95` 将 residency 确立为 remote target 的唯一真相源。residency 是正确性与可扩展性闭环，不单独声明未经测量的性能数字。

## 结论边界

- 内核栈数据来自 pthread 微负载；user-fault 数据来自 Cargo 工作负载，两者不可相加。
- recording-disabled 结果承担性能结论；enabled counter 只解释机制。
- RV64/QEMU/SMP=1 的百分比不外推为 LA64、八核或实体硬件的固定收益。
- 正式实现不通过缩小栈、减少构建工作或放宽正确性 oracle 获得加速。
