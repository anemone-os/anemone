# ANE-CHG-20260807-ext4-single-mutex

**Type:** Cleanup / concurrency simplification
**Status:** Completed
**Date:** 2026-08-07
**Authors:** doruche, Codex
**Area:** task filesystem context / VFS namei / ext4 / lwext4 synchronization

## Problem / Context

`Ext4Sb` 原先同时用 `fs_lock: SpinLock<()>` 串行化每次 lwext4 mutable access，并用
`tx_lock: RwLock<()>` 包围复合 filesystem operation。lwext4 的 lookup、read、stat 和 readdir 也会修改内部
cache、reference 或 iterator 状态，因此 read transaction 并没有形成真实并行能力；普通路径反而连续取得两把
spin-based lock，复合路径还需要分别证明 transaction span 与 mutable-access span。

同步 block I/O 可能发生在这些 guard 内。继续使用 spin lock 会让竞争方主动等待，也让 `Ext4FsCell` 的 unsafe
`Send + Sync` 证明依赖两条独立的调用约定。本轮把已发布 lwext4 filesystem 的 access 与 transaction owner 收敛为
ext4 superblock 内一把可睡眠、独占的 `Mutex`。

实现期 source/runtime feedback 还发现一条上层锁序：产品配置启用 `spin_lock_irqsave` 时，Task pathname helper
持有 `FsState` 的 `RwLock` read guard 跨完整 namei，使 ext4 backend 在 IRQ disabled 状态取得新 Mutex。它不是 boot
特例，而是 Task filesystem context 到任意可睡眠 backend 的错误 handoff。因此本轮在 closure 前把该 guard span 一并
收窄为 operation-local namespace snapshot；没有为 ext4 或通用 Mutex 增加逃生路径。

## Decision

- `Ext4Sb` 只保留一把 `Mutex<GuardedExt4Fs>`。未发布的局部 `Ext4Fs` 可以完成构造和 root inode 校验；发布后每个
  lwext4 access 都经同一 guard。
- lookup/create/make-node/symlink/link/unlink/rmdir/rename/truncate/inode sync 等复合 operation 每次只取得一次
  ext4 guard。guard-held helper 显式接收 `&mut Ext4Fs`，不递归取得同一 Mutex。
- ext4 guard 内只执行 lwext4 及其向下 block I/O。`iget()`、inode/dentry cache publication、address-space orchestration、
  user copy、`DirSink` handoff 和跨 owner drop 均在 guard 外。
- Task filesystem context 在一次 read guard 内成对 clone `root/cwd`，形成不可变 `FsPathSnapshot`；释放 guard 后才进入
  namei。显式 dirfd/from 入口只 snapshot logical root。`PathRef` clone 是单次 operation 的稳定 lifetime capability，
  不是第二份 namespace truth。
- 并发共享 `CLONE_FS` 的 `chroot/chdir` 只会线性化在 snapshot 前或后；单次 lookup 不会观察混合 root/cwd，也不再
  用 IRQ-save `FsState` guard 覆盖可能睡眠的 backend path walk。

## Implementation Boundary

**Target:** 用一个 sleepable exclusive Mutex 唯一串行化已发布 lwext4 access 与复合 backend transaction，并让
Task filesystem-context guard 在进入 namei/backend 前转换为 coherent operation-local path capability；保持现有 ABI、
errno、visible path semantics、failure/cleanup、current contract 与 register 范围。

**Owners / handoff:** Task `FsState` 继续唯一拥有 root/cwd/umask 以及 fork/`CLONE_FS`/exec 生命周期，只向 VFS
namei 移交 cloned `PathRef`；VFS 继续拥有 path walk、dentry 与 mount topology；ext4 superblock 唯一拥有 lwext4
filesystem object 和同步；lwext4 wrapper与 block device继续拥有各自 FFI/resource 与 I/O 生命周期。

**Failure / cleanup:** snapshot 是 infallible `Arc` capability clone。lwext4 error mapping、partial progress、metadata/
flush 和 best-effort cleanup 顺序不变；任何 early return 都通过 guard lifetime 释放 Mutex，不新增 retry、poisoning、
rollback、fallback 或双路径。

**Protected surface:** 不扩大 public Rust API、syscall ABI、visibility或 shared contract；不改变
[`VFS-MAKE-NODE-001`](../../contracts/vfs/make-node.md#vfs-make-node-001--make-node-admission-与-backend-publication-只有一个-handoff)
的 backend publication serialization，也不改变
[`SYSTEM-POWER-ORDERLY-001`](../../contracts/power/shutdown-lifecycle.md#system-power-orderly-001)
的 filesystem sync handoff。

**Contract Impact / Cutover:** `None`。Task snapshot 是 owner-local lock/handoff correction，ext4 Mutex 是 filesystem
adapter 内部同步 cutover；两项 dependency 的 effective rule 均未改变。

## Change

- 删除 `fs_lock`、`tx_lock`、`read_tx()`、`write_tx()`、`UnsafeCell` 和 unsafe `Sync`；新增只实现 unsafe `Send` 的
  `GuardedExt4Fs`，由 `Ext4Sb::fs` 的 Mutex guard 唯一暴露 `&mut Ext4Fs`。
- 把所有 ext4 callsite 收敛到 `with_fs()`；unlink/rmdir/rename 等 helper 消费已持有的 `&mut Ext4Fs`，复合 backend
  mutation 保持连续。
- readdir 每次在 ext4 guard 内 snapshot 一个 entry 和 next offset，释放 guard 后调用 `DirSink::push()`；只有 sink
  `Accepted` 后才提交 caller offset，`Stop` 保持原 entry 可在下次调用重试。
- 新增 private `FsPathSnapshot`，统一修正六个 Task lookup/parent variant；absolute/relative ordinary path 使用同一
  root/cwd snapshot，显式 start-path variant 只持有 root snapshot。

## Validation

- `just fmt kernel --check`、`git diff --check` 与 `just test lwext4` 通过；host lwext4 callback tests 为 2/2。
- 原实现使用同一 RV64 single-HART preset、preliminary image 和 `open` / `fs` / `read-write` / `address-space`
  profile取得 baseline：571/571 KUnit；LTP合计
  `attempted=292, passed=224, failed=58, infra_failed=0, skipped=10`。
- 首次集成运行在 initial exec 暴露 `FsState` IRQ-save guard跨 namei 的 Mutex context assertion。符号化调用链为
  `bsp_kinit -> exec_initial_program -> Task::lookup_path -> namei -> ext4_lookup -> Mutex::lock`；本轮没有用 boot
  特判或 lock bypass 掩盖该证据，而是收窄 filesystem-context owner handoff。
- 最终同配置 RV64运行重新完成571/571 KUnit。glibc各组为
  `open 24/18/5/0/1`、`fs 55/36/18/0/1`、`read-write 59/52/6/0/1`、
  `address-space 8/8/0/0/0`；musl除`read-write 59/48/6/0/5`外对应一致。总计仍为
  `292/224/58/0/10`，全部 group summary 与 `FAIL LTP CASE`记录和 baseline 精确相同，没有新增 TFAIL、TBROK、
  timeout 或 infra failure。
- guest 没有出现 Mutex context/recursive assertion、panic、ext4 I/O/flush/sync error或停机卡死，并按 filesystem、
  network、device 顺序完成 shutdown 后进入 PowerOff。临时 LTP profile 已恢复为 `socket`，未进入最终 diff。
- 一位独立 agent 在修正前识别出上述 IRQ-save outer-guard Apollyon；最终复审确认该 finding 已关闭，Apollyon、
  Keter、Euclid、Safe 均为 0。Architecture Friction Scan 未发现第二状态真相、owner 穿透、private representation
  泄漏、caller/boot/test 特判、临时桥或新的无 consumer 抽象。
- **Not Run:** LA64 build/runtime、full LTP、SMP、多轮并发 stress、kernel-preempt-on 对照、实体硬件、final harness、
  Mutex contention/performance A/B、arbitrary I/O failure、crash/power-loss 与 journal recovery。

## Remaining Risk / Links

- [`ANE-20260801-VFS-MAKE-NODE-LWEXT4-ATOMICITY`](../../register/current-limitations.md#ane-20260801-vfs-make-node-lwext4-atomicity)
  与
  [`ANE-20260523-EXT4-TRUNCATE-CACHE-INVALIDATION`](../../register/current-limitations.md#ane-20260523-ext4-truncate-cache-invalidation)
  保持 Active；本轮没有扩大 transaction rollback、truncate/cache invalidation 或 durability guarantee。
- `FsPathSnapshot` 只证明一次 operation 的 coherent root/cwd capability；本轮没有运行共享 `CLONE_FS` 下并发
  `chroot/chdir` stress，也不声明新的 namespace race guarantee。
- Current contract / RFC / transaction：None。执行证据由本记录摘要与最终 Git commit 持有。
