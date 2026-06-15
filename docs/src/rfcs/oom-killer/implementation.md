# OOM Killer 迁移实施计划

**状态：** Active implementation
**最后更新：** 2026-06-15
**父 RFC：** [RFC-20260615-oom-killer](./index.md)
**不变量：** [OOM Killer 不变量需求](./invariants.md)

本文定义第一版 OOM killer 的实现阶段。该实现以物理页使用率为触发源，以用户地址空间独占物理页 snapshot 为 victim 排序依据，以 `SIGKILL` 作为终止动作。

## 迁移原则

- allocator fast path 只做轻量 threshold check 和 wake。
- OOM policy 属于 OOM killer worker，不属于 frame allocator。
- `FrameAllocatorStats` 只提供通用 threshold 判断，不保存 OOM 状态。
- victim selection 使用独占物理页 snapshot，不追求实时最优。
- task topology 全局锁只做短 snapshot，不能覆盖 user-space/VMO scan。
- kill victim 走 signal/exit 现有路径。

## 阶段 1：kconfig 与 stats threshold helper

前置条件：

- `FrameAllocatorStats` 已有 `total_pages`、`free_pages` 和 `used_pages()`。
- kconfig 生成器已支持新增参数。

交付：

- `conf/.defconfig` 增加 `oom_kill_threshold = 90`。
- `scripts/xtask/src/config/kconfig.rs` 的 `Parameters` 增加 `oom_kill_threshold: Option<u8>`。
- 生成 `OOM_KILL_THRESHOLD: u8`。
- `FrameAllocatorStats` 增加 OOM 专用 threshold 判断函数，例如：

```rust
pub fn exceeds_oom_kill_threshold(&self) -> bool
```

审计：

- `OOM_KILL_THRESHOLD <= 100` 使用 `const_assert!` 保证，私有百分比 helper 仍用 `assert!` 防御错误调用。
- `total_pages == 0` 返回 false。
- 等于 threshold 不触发。
- `FrameAllocatorStats` 可以公开 `exceeds_io_shrink_threshold()` 和 `exceeds_oom_kill_threshold()` 这类语义方法，但裸百分比 helper 必须保持私有，避免调用点绕过 policy 名称。

write set：

- `anemone-kernel/src/mm/frame/allocator.rs`
- `scripts/xtask/src/config/kconfig.rs`
- `conf/.defconfig`
- ignored local `kconfig` 只在需要本地运行时同步，不作为提交入口。

验证：

- KUnit 覆盖 90% 等于不触发、91% 触发、0 total 不触发。
- `git diff --check`。

退出条件：

- OOM killer 和 allocator wake hook 可以通过 `frame_allocator_stats().exceeds_oom_kill_threshold()` 判断是否超过 OOM killer 阈值；inode shrinker 继续通过 `exceeds_io_shrink_threshold()` 判断自己的回收阈值。

## 阶段 2：OOM killer worker skeleton

前置条件：

- [kthread](../kthread/index.md) 已可用于长期 worker。
- signal 层支持向 thread group 投递 `SIGKILL`。

交付：

- 新增 `anemone-kernel/src/mm/oom.rs` 或等价 mm-owned module。
- 新增 `init_oom_killer()`，boot 后创建 `oom-killer-0` ordinary kthread。
- 新增 `wake_oom_killer()`，只 publish wake event。
- worker wait predicate 同时观察 stop/park 和当前 threshold。

审计：

- worker 被唤醒后必须先重查 `FrameAllocatorStats`。
- threshold 不满足时不选择 victim。
- worker stop / park safe point 明确。
- `wake_oom_killer()` 不分配 heap，不扫描 task。

write set：

- `anemone-kernel/src/mm/mod.rs`
- `anemone-kernel/src/mm/oom.rs`
- `anemone-kernel/src/main.rs`

验证：

- boot 后创建 `oom-killer-0` 不 panic。
- 通过日志或 KUnit/定向 hook 确认 threshold 不满足时 worker 不 kill。

退出条件：

- OOM killer worker 可被唤醒并按 threshold gate 进入或跳过 victim selection。

## 阶段 3：allocator wake hook

前置条件：

- `wake_oom_killer()` 已存在。
- `FrameAllocatorStats` threshold helper 已存在。

交付：

- 修改 `alloc_frame()`：成功从 `FRAME_ALLOCATOR.alloc_one()` 取得 frame 后，读取 stats 并判断 `exceeds_oom_kill_threshold()`。
- 修改 `alloc_frames()`：成功取得 contiguous folio 后走同一个 OOM wake helper。
- 超过 threshold 时调用 `wake_oom_killer()`。
- `alloc_frame_zeroed()` 通过现有调用链自然继承该行为。

审计：

- stats check 必须发生在 frame allocator lock 释放后。
- allocation 失败不新增语义。
- `alloc_frame()` 与 `alloc_frames()` 只调用同一个 helper，避免两个 allocation path 分裂出不同 threshold 判断。

write set：

- `anemone-kernel/src/mm/frame/mod.rs`
- `anemone-kernel/src/mm/oom.rs`

验证：

- KUnit 或源码审计确认 hook 不在 allocator lock 内。
- user-app-test 的匿名页 fault 路径会经 `alloc_frame_zeroed()` 触发 hook。

退出条件：

- frame allocation 后，超过 threshold 会唤醒 OOM killer。

## 阶段 4：独占物理页 snapshot 与 victim selection

前置条件：

- OOM worker skeleton 已可运行。
- task topology 提供 thread group / task 枚举能力。

交付：

- 增加短 snapshot helper，复制 eligible thread group handles 后释放 topology lock。
- 增加 `UserSpaceHandle::exclusive_physical_pages_snapshot()` 或等价 helper。
- 独占物理页 snapshot 从 `UserSpace` 当前 VMA/VMO backing 推导杀掉该地址空间最可能立即释放的物理页，不引入长期 RSS 字段。
- victim selection 按独占物理页 pages 最大值选择 thread group。

实现建议：

- VMO 层增加 read-only traversal / count helper，统计当前 backing 中独占的物理页；shared backing、共享零页和文件页缓存不计入。
- `ShadowObject` 始终统计自身 overlay 中 refcount 为 1 的物理页；如果 parent/backing VMO 的 strong ref 只剩当前 shadow 链持有，则递归统计 parent 链，因为释放该地址空间会释放这些祖先页。
- `UserSpace::exclusive_physical_pages_snapshot()` 遍历非 guard VMA，并按 VMA 映射到的 VMO range 统计独占物理页。
- thread group snapshot 只复制 `Arc<ThreadGroup>`；scoring 时再读取 status、leader 和 user-space handle。

审计：

- topology lock 不得覆盖 `UserSpaceHandle` mutex 或 VMO traversal。
- stale candidate 已退出时跳过。
- kernel / idle / init / OOM killer / kthreadd 不可选。
- 只用独占物理页 snapshot 排序，不把它暴露成 `/proc` RSS。

write set：

- `anemone-kernel/src/task/topology/mod.rs`
- `anemone-kernel/src/mm/uspace/mod.rs`
- `anemone-kernel/src/mm/uspace/vmo/*`
- `anemone-kernel/src/mm/oom.rs`

验证：

- KUnit 覆盖独占物理页 snapshot 对 guard / 不可达 VMA 不计数。
- KUnit 覆盖 mapped private anonymous pages 计入独占物理页 snapshot。
- KUnit 覆盖 shared backing 不计入独占物理页 snapshot。
- KUnit 覆盖 `ShadowObject` parent shared 时不递归、parent 独占时递归计入。
- 源码审计确认 topology lock scope 只覆盖 snapshot。

退出条件：

- OOM worker 能在不长时间持有全局 topology 锁的情况下选出当前独占物理页最多的 eligible process。

## 阶段 5：SIGKILL 与 active victim race control

前置条件：

- victim selection 可返回 TGID / thread group handle。
- signal 层 `ThreadGroup::recv_signal()` 可用。

交付：

- OOM worker 对 victim 投递 `SIGKILL`，`SigInfo` 标记为 kernel-origin。
- worker 记录 active victim TGID。
- active victim 尚存在且未完成 `Exited` 时，worker 不选择新的 victim，只让出调度器后重查。
- active victim 已 `Exited` 或不存在时，清空 active victim 并继续 threshold loop。

审计：

- 不在持有 victim selection lock / active victim lock 时发送 signal。
- 不直接调用 victim 的 exit helper。
- kill 后必须重新检查 threshold，不假设 memory 已释放。
- 没有 eligible victim 时记录 notice/warning 并 `yield_now()`，避免 tight spin。

write set：

- `anemone-kernel/src/mm/oom.rs`
- 必要时新增 narrow task/signal helper，避免 OOM module 构造过宽 signal internals。

验证：

- 定向运行中父进程保持 alive，子进程收到 `SIGKILL`。
- 日志包含 victim TGID、exclusive pages、threshold 和当前 physical usage。

退出条件：

- worker 能按 “check threshold -> select one victim -> SIGKILL -> yield/recheck” 循环工作，直到 threshold 不满足。

## 阶段 6：user-app-test 回归

前置条件：

- OOM killer 已能在匿名页 fault 压力下杀进程。
- user-test 或 standalone user app 构建路径明确。

交付：

- 新增 user-app-test：父进程 clone/fork 子进程，子进程进入无限循环：
  - 每轮申请 50 MiB；
  - 逐页写入，强制匿名页 materialize；
  - 保留指针，避免被优化或释放。
- 父进程等待子进程退出，带 timeout。
- 父进程断言子进程是 signaled exit，信号为 `SIGKILL`。
- 测试输出明确区分：
  - child malloc/mmap 提前失败；
  - child 正常退出；
  - child 被非 SIGKILL 杀死；
  - parent 被错误选中或 wait timeout。

落位选择：

- 如果仓库已有专门 user-app-test runner，新增对应 case。
- 如果没有，按现有结构新增 `anemone-apps/oom-killer-test` 或接入 `anemone-apps/user-test` profile，并在 rootfs config 中加入该 app。

审计：

- child 不使用 `CLONE_VM`，避免父子共享地址空间导致 parent 也成为最大 victim。
- parent 自身保持低独占物理页 footprint。
- chunk 大小固定为 50 MiB，且每页至少写 1 字节。

write set：

- `anemone-apps/user-test/src/*` 或新增 `anemone-apps/oom-killer-test/*`
- `anemone-apps/*/app.toml`
- 必要的 rootfs/test profile 配置

验证：

- `just fmt <app> --check`。
- `just xtask app build <app> --arch riscv64`。
- QEMU/user-test 运行对应 profile，确认 parent 观测到 `SIGKILL`。

退出条件：

- 测试能稳定证明 OOM killer 会杀掉内存压力子进程，而不是杀掉父进程或让系统 panic。

## 旁路审计清单

后续 review 至少确认：

- `alloc_frame()` hook 没有在 frame allocator lock 内执行复杂逻辑。
- 没有其它 path 在未重查 threshold 的情况下直接 kill victim。
- topology lock 没有覆盖 VMO 独占物理页扫描。
- 独占物理页 snapshot 没有被复用成 `/proc` RSS ABI。
- SIGKILL 走现有 signal/exit path。
- active victim 机制不会因为 stale TGID panic。

## 停止边界

需要回到 RFC 层的情况：

- 要把 OOM policy 扩展成 badness score、memcg、OOM reaper、direct reclaim、swap 或 page cache reclaim。
- 要把独占物理页 snapshot 变成稳定 RSS 账本或 `/proc` ABI。
- 要让 allocator 失败 path 同步等待 victim 退出。
- 要在 OOM killer 中直接释放 victim address space 或绕过 signal/exit。

可以作为普通小改继续推进的情况：

- 调整日志字段。
- 调整 threshold 默认值，仍保持 kconfig 百分比和严格大于语义。
- 增加独占物理页 snapshot 的 KUnit 覆盖。
- 把 user-app-test 从 standalone app 移入现有 user-test runner，前提是测试语义不变。
