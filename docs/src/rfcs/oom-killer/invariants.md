# OOM Killer 不变量需求

**状态：** Active implementation
**最后更新：** 2026-06-16
**父 RFC：** [RFC-20260615-oom-killer](./index.md)

本文定义第一版 OOM killer 的阈值、唤醒、victim selection、锁序和竞态边界。实施顺序见 [迁移实施计划](./implementation.md)。

## 闭合条件

OOM killer 被视为闭合时，必须同时满足：

1. `oom_kill_threshold` 进入 kconfig，默认 90，且被约束为 0..=100 的百分比。
2. `FrameAllocatorStats` 提供 threshold 判断函数，使用严格大于语义。
3. `alloc_frame()` / `alloc_frames()` 成功分配后检查一次 threshold，超过时只唤醒 OOM killer。
4. allocation fast path 不扫描 task、不锁 user address space、不发送 signal。
5. OOM killer worker 醒后必须重新读取 stats 并重查 threshold。
6. threshold 不满足时 worker 不选择 victim。
7. victim selection 不在 task topology 全局锁内扫描 VMO 或 user-space state。
8. victim 必须是 eligible user thread group，不能是 kernel / idle / init / OOM killer / kthreadd。
9. worker 通过 `SIGKILL` 终止 victim，不直接调用 victim 的 exit path。
10. worker 避免重复杀同一个尚未完成退出的 active victim。
11. worker 重复执行直到 threshold 不满足；没有 eligible victim 时记录诊断并让出调度器。
12. user-app-test 能稳定制造子进程内存压力，并由父进程观察到子进程被 `SIGKILL`。

## 状态所有权

### Frame allocator stats

`FrameAllocatorStats` 是物理页使用率的观察来源：

- `total_pages` / `free_pages` 仍由 frame allocator 统计；
- threshold 判断函数只计算 `used_pages` 与 threshold 的关系；
- stats 不拥有 OOM worker lifecycle，不保存 pending wake，也不选择 victim。

要求：

1. `FrameAllocatorStats` 只公开 `exceeds_io_shrink_threshold()` 和 `exceeds_oom_kill_threshold()` 这类语义函数；裸百分比 helper 保持私有，避免调用点绕过 policy 名称。
2. `total_pages == 0` 返回 false。
3. 阈值判断使用 saturating arithmetic 或等价方式避免乘法溢出。
4. 等于 threshold 不触发 OOM killer。

### OOM killer worker

OOM worker 是 OOM 策略 owner：

- 全局 worker slot 只保存 kthread weak handle，用于防止重复初始化；
- wake event 只表示“需要重查 threshold”；
- active victim 只表示“已经向某个 thread group 投递过 OOM SIGKILL，等待它进入退出/释放路径”。

要求：

1. worker 每轮先检查 stop。`kthread-core` 阶段 1 已移除 park/unpark，OOM policy 不再依赖 park 状态。
2. worker 被唤醒后先读取 `frame_allocator_stats()` 并判断 `oom_kill_threshold`。
3. active victim 已经 `Exited` 或不存在时才能清空并选择新 victim。
4. active victim 仍 `Alive` / `Exiting` 时，worker 应让出调度器，而不是继续杀其它进程。

### Victim candidate

victim candidate 是 thread group，不是单个 task：

- 选择单位使用 TGID；
- memory score 来自该 thread group 代表的用户地址空间独占物理页 snapshot；
- kill 动作投递给 thread group shared pending signal。

要求：

1. thread group 已经 `Exiting` 或 `Exited` 时不可选。
2. `Tid::INIT` 不可选。
3. 没有 user-space handle 的 kernel task / kthread 不可选。
4. 多线程共享地址空间只按 thread group 计一次。

## 线性化点

### Alloc path wake

`alloc_frame()` 的 OOM wake 线性化点是成功获得 `OwnedFrameHandle` 后、返回调用者前的一次 stats check；`alloc_frames()` 对 contiguous folio 分配使用同一个 wake helper。

要求：

1. allocation 失败不负责唤醒 OOM killer；失败语义仍由调用者处理。
2. stats check 必须发生在 frame allocator 内部锁释放后。
3. wake 只能 publish event 或设置轻量原子状态。

### Worker threshold check

worker 是否进入 victim selection 的线性化点是醒后读取到的 `FrameAllocatorStats`。

要求：

1. allocator 侧的 check 只是 hint。
2. worker 侧 check 是执行 kill policy 的必要条件。
3. victim kill 之后必须重新回到 threshold check；不能假设一次 kill 足以解除 OOM。

### Victim selection

victim selection 分两阶段：

1. topology snapshot 阶段：在 topology read lock 下复制 thread group `Arc` 或等价轻量 handles。
2. scoring 阶段：释放 topology lock 后，逐个 candidate 读取 status、leader/user-space handle 和独占物理页 snapshot。

要求：

1. topology lock 不得覆盖 VMO traversal、user-space mutex、signal delivery 或 logging 中的大规模格式化。
2. scoring 期间 candidate 可能退出；stale candidate 必须 fail closed。
3. 选择结果只是本轮 snapshot 最大值，不承诺全局实时最优。

### Kill

kill 线性化点是向 victim thread group 投递 `SIGKILL`。

要求：

1. signal 使用 `SiCode::Kernel` 或等价 kernel-origin 标识。
2. OOM killer 不直接持有 victim user-space lock 发送 signal。
3. kill 后记录 active victim，再让出调度器。

## 锁序与生命周期规则

1. frame allocator lock 不得包住 OOM wake 以外的逻辑。
2. topology lock 只用于短 snapshot，不得包住 user-space lock。
3. user-space lock 可以包住 VMO 独占物理页扫描，但不能在该窗口内反向获取 topology lock。
4. OOM worker 不得在持有 active-victim state lock 时执行 signal delivery。
5. worker stop safe point 必须位于 wait loop、active victim wait/yield 边界和每轮 kill 后。
6. victim 的地址空间释放仍由普通 exit path 和 `Drop`/RAII 资源释放负责。

## 禁止退化项

以下模式会破坏证明：

1. 在 allocator fast path 中扫描 task、VMO 或 user-space。
2. 在 topology 全局锁内计算每个进程的独占物理页。
3. 用 VMA virtual size 替代独占物理页 snapshot 作为 canonical victim policy。
4. 直接修改 victim thread group lifecycle 或调用 victim 的 `kernel_exit_group()`。
5. 连续向多个 thread group 投递 OOM `SIGKILL` 而不让 victim 有机会进入退出路径。
6. 把 active victim 当成内存释放完成的证明。
7. 把 OOM 独占物理页 snapshot 暴露成 `/proc` RSS ABI。

## 完成标准

文档层完成标准：

- `index.md`、`invariants.md` 和 `implementation.md` 明确阈值、唤醒、worker、victim selection、锁序和测试边界。

运行完成标准：

1. `git diff --check` 通过。
2. `just fmt kernel --check` 通过。
3. `just build` 通过。
4. user-app-test：父进程 clone/fork 子进程，子进程按 50 MiB chunk 分配并触碰内存，父进程观察到子进程被 `SIGKILL`，且父进程自身未被 OOM killer 选中。
