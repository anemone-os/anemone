# 2026-06-15 - OOM Killer

**Status:** Active
**Owners:** EDGW, Codex
**Area:** mm / frame allocator / user-space VMO / task / signal / kthread
**Canonical Plan:** [RFC-20260615-oom-killer](../../rfcs/oom-killer/index.md), [Invariants](../../rfcs/oom-killer/invariants.md), [Implementation Plan](../../rfcs/oom-killer/implementation.md)
**Current Phase:** runtime validation pending

## Scope

本事务实现第一版 OOM killer：

- `oom_kill_threshold` kconfig gate；
- frame allocator 成功分配后的 OOM wake hook；
- `oom-killer-0` kthread worker；
- `UserSpace` 独占物理页 snapshot；
- 按最大独占物理页选择 eligible thread group；
- kernel-origin `SIGKILL` 终止 victim。

非目标保持 RFC 边界：不实现 badness score、memcg、OOM reaper、direct reclaim、swap、page cache reclaim、稳定 RSS 账本或 `/proc` RSS ABI。

## Invariants

- allocator path 只做 semantic threshold check 和 kthread wake，不扫描 task，不发送 signal。
- `FrameAllocatorStats` 只公开 `exceeds_io_shrink_threshold()` 与 `exceeds_oom_kill_threshold()`；裸 percentage helper 保持私有。
- victim score 是释放该 thread group address space 时最可能归还给 frame allocator 的独占物理页数。
- `ShadowObject` overlay 中 refcount 为 1 的页计入；parent/backing VMO 只有当前 shadow 链持有时递归计入 parent 链。
- topology lock 只覆盖 thread group `Arc` snapshot，不覆盖 `UserSpace` lock 或 VMO traversal。
- active victim 在 `Alive` / `Exiting` 期间不被清空，避免连续杀多个进程。

## Phase Log

### 2026-06-15 - threshold API correction

**Phase:** implementation correction

**Change:** `FrameAllocatorStats` 增加 `exceeds_io_shrink_threshold()` 和 `exceeds_oom_kill_threshold()`，二者共用私有 `used_pages_exceeds_percent()`。inode shrinker 改为调用 IO shrinker 语义函数，OOM path 调用 OOM killer 语义函数。

**Audit:** 不公开裸 percentage helper，避免 inode shrinker、OOM killer 或 allocator hook 绕开 policy 名称直接拼公式。

**Validation:** 增加 KUnit 覆盖严格大于阈值、等于阈值不触发和零总页数不触发。完整构建验证待本事务收口记录。

### 2026-06-15 - exclusive physical page metric

**Phase:** implementation correction

**Correction:** victim score 从早期“字面内存 / resident-like 总量”收窄为“独占物理页数”。该值只服务 OOM victim 排序，不成为 RSS、`/proc` 或长期 accounting 真相源。

**Change:** VMO trait 增加 `exclusive_physical_pages(range)` 默认 0。`AnonObject`、`FixedObject` 和 `ShadowObject` 统计 refcount 为 1 的 resident frames；shared/file/cache/default backing 不计入。`UserSpaceHandle::exclusive_physical_pages_snapshot()` 在 user-space lock 下遍历非 guard、非 shared VMA，并把 VMA range 转成 VMO pidx range。

**Correction:** `ShadowObject` 不能只统计 overlay。当前实现先统计 overlay 独占页；如果 parent VMO 的 strong ref 只剩当前 shadow 持有，则递归统计 parent 链。递归前释放 overlay read lock，保持 parent-before-overlay 锁序。

**Validation:** 目前为源码级实现；专门的 VMO/UserSpace KUnit 仍待补。

### 2026-06-15 - OOM worker and allocator hook

**Phase:** implementation

**Change:** 新增 `mm::oom`，启动 `oom-killer-0` kthread。worker 等待 kthread wake 或 threshold predicate，醒后重查 `frame_allocator_stats().exceeds_oom_kill_threshold()`，再执行 thread group snapshot、score、SIGKILL 和 active victim yield/recheck。

**Change:** `alloc_frame()` 与 `alloc_frames()` 成功分配后调用同一个 `wake_oom_killer_if_needed()` helper。helper 在 allocator lock 释放后读取 stats，超过 OOM threshold 时调用 `mm::oom::wake_oom_killer()`。

**Audit:** OOM worker 构造 `SiCode::Kernel` 的 `SIGKILL`，sender 来自当前 OOM kthread；不直接调用 victim exit helper。

**Validation:** Docker `gallant_lamarr` 内以 `ubuntu` 用户运行 `just build` 通过。首次运行因本地 `kconfig` 指向不存在的 `pre-test-la64` platform 失败；随后在容器内执行 `just defconfig` 恢复 tracked `conf/.defconfig` 后构建通过。

### 2026-06-15 - user-app-test wiring

**Phase:** implementation / test wiring

**Change:** 新增 `anemone-apps/oom-killer-test`。测试进程 clone 出非 `CLONE_VM` 子进程；子进程循环匿名 `mmap` 50 MiB chunk 并逐页 volatile write，父进程 `wait4` 子进程，只有观测到 `WStatus::Signal(SIGKILL)` 才退出 0。

**Change:** `anemone-apps/user-test` 暂时注释其它 local tests、competition test 入口和 LTP 入口，只执行 `/bin/oom-killer-test`。本地忽略的 `rootfsconfig-rv` 增加 `oom-killer-test` app，使 Docker rootfs 构建会把测试二进制安装到 `/bin`。

**Validation:** Docker `gallant_lamarr` 内以 `root` 用户运行 `just rootfs mkfs -c rootfsconfig-rv` 通过；日志确认 staging/build 了 `oom-killer-test` 并生成 `build/rootfs/minimal-rv/rootfs.img`。

## Open Items

- 运行 `git diff --check` 和需要时的 format check。
- 补独占物理页 snapshot 的 KUnit，尤其是 `ShadowObject` parent shared/独占递归边界。
- 运行 QEMU / user-test runtime：父进程保持低 footprint，子进程按 50 MiB chunk 分配并逐页触碰，父进程观察子进程被 `SIGKILL`。
- 如果 runtime 发现 victim `SIGKILL` 后内存释放等待过短，再按 RFC 扩展 wait-for-victim-exit 事件或更精确的退出观察点。
