# ANE-CHG-20260809-positive-dentry-residency

**Type:** Small Feature / VFS lifecycle optimization
**Status:** Completed
**Date:** 2026-08-09
**Authors:** doruche, Codex
**Area:** VFS namei / dentry lifecycle / superblock / mount cleanup / ext4 policy

## Problem / Context

`Dentry` 的 child 强持有 parent，而 parent child map 只保存 `Weak<Dentry>`。filesystem backend 因而继续是
持久 namespace 真相源，parent map 只是 live dentry 索引；但 pathname caller 释放最后一个正 dentry 后，后续 walk
必须重新进入 backend lookup。

本轮给明确 opt-in 的 filesystem 增加有界额外 residency，以减少已 materialize 正 dentry 的重复 backend lookup。
它不引入 negative cache、完整 pathname cache、global dcache、shrinker 或用户可见 cache guarantee。既有 backend
lookup 与 VFS materialization、backend mutation 与 weak-child unpublication 之间仍有并发窗口；本轮只保证额外
residency 不会把 mutation 前开始的旧工作延长为长期 membership，不重新定义完整 namespace linearizability。

## Decision

- residency 由 `SuperBlock` 生命周期唯一拥有；filesystem type 只通过 declarative flag 选择参与，初始仅 ext4 opt in。
  backend 不接收 hit、eviction 或 invalidation callback。
- `residents` 是额外 membership 的唯一真相，FIFO 只记录同一 membership 的 insertion order。每个 opt-in superblock
  的固定逻辑容量由 `vfs_positive_dentry_residency_capacity` Kconfig 拥有，production 默认值为 1024；容量必须非零。
- lookup/create 在 backend 工作前取得 generation ticket。成功 unlink、rmdir 或 rename 在 backend commit 后推进
  generation；parent map unpublish 后，再按返回的 exact dentry identity forget residency。
- admission 是 best effort。未参与、stale ticket 或容器 reserve 失败都只产生 no-retention，不改变 backend 结果、
  pathname 可见性、syscall errno 或 caller 已取得的 `Arc<Dentry>`。
- eviction、forget 和 drain 先在 residency lock 内摘除强引用，再在锁外 drop；residency lock 不跨 backend I/O、
  parent child-map mutation、inode eviction 或 mount-tree mutation。
- 同步 unmount 只在 attached mount inventory 中确认最后一个 superblock view 后 drain；ordinary shared view 和 bind view
  都保持同一 per-SB residency。drain 发生在既有 `has_alive_inode()` 与 `try_evict_all()` 之前。

## Implementation Boundary

**Target:** 为 opt-in filesystem 增加 per-SuperBlock、有界、best-effort 的正 dentry residency，只延长已 materialize
对象的强引用生命周期；保持 backend namespace truth、child-to-parent strong / parent-to-child weak 模型、inode identity、
mount sharing、syscall ABI/errno 与 visible pathname semantics。

**Owners / handoff:** backend 唯一拥有持久 namespace；`Dentry` parent map 拥有 live publication；superblock-local
residency component 唯一拥有额外 membership、容量、FIFO 与 freshness；namei/VFS mutation 和 MountTree 只提供窄的
ticket/admit、invalidate/forget 与 last-view drain handoff。

**Failure / cleanup:** admission failure 不回滚或覆盖 backend 结果。成功 remove/replace 的顺序是 backend commit、
generation invalidation、weak-child unpublication、membership forget；所有 detached `Arc<Dentry>` 在 owner lock 外释放。
last-view unmount 若仍有真实 external/live reference，可以在 cache 已 drain 后返回 busy，后续 lookup 可自然重新填充。

**Protected surface / Contract Impact:** 不扩大 public Rust API、shared visibility、backend callback、syscall ABI 或 current
contract；`Contract Impact` 为 `None`。本轮不关闭 create publication、rename 后 live `PathRef` relocation、lazy detach/
final cleanup 或完整 namespace linearizability。

## Change

- 将原 `fs/dentry.rs` 按同一 owner 内稳定职责整理为 `dentry/mod.rs` 与 `dentry/residency.rs`；原 Dentry 对象模型保持，
  新文件只承载 owner-local residency lifecycle 与三个 inline KUnit。
- `SuperBlock` 按 filesystem flag 可选持有 residency owner，并提供 ticket/admit、invalidate、forget 与 drain 的 private
  handoff。ext4 是唯一 opt-in filesystem；ramfs、procfs、devfs 与 anonymous filesystem 均保持 opt out。
- 所有普通 namespace child materialization 收敛到唯一 `materialize_child_dentry` admission：backend lookup、touch、
  make-node、mkdir 与 symlink 共 5 个 production caller。其它 `Dentry::new` 只构造 mount/devfs root、anonymous path
  或 detached KUnit fixture，不形成 opt-in child bypass。
- unlink、rmdir、rename source 与 rename replacement target 在 backend 成功后统一 invalidate，并通过
  `take_child` 先 unpublish、再 forget exact identity。`take_child` 同时避免在 parent map lock 内 drop live child。
- MountTree 在 writer transaction 固定的 last-view plan 上 drain；其 attached inventory 按 `Arc::ptr_eq(m.sb(), &sb)`
  同时覆盖 ordinary shared-SB mount 与 bind view。

## Bounded Source Review

三组关键交错由 residency mutex 下同一个 generation/membership 同步边界闭合：

1. **mutation first:** lookup/create 在 mutation 前取得的 ticket 会在成功 mutation 推进 generation 后变 stale；即使旧
   backend result 随后 materialize，也只能由当前 caller/既有 weak publication 暂时持有，不能进入长期 residency。
2. **admission first:** 已进入 membership 的 dentry 会在后续 mutation 中先从 parent map unpublish，再以同一 identity
   forget；resident strong reference 在 residency lock 外释放。
3. **forget 后 fast hit:** namei fast path只读取 parent map，不执行 re-admission；unpublish 后该 identity 已不可命中。
   此前已进入 backend 的 materialization仍持有旧 ticket，也不能重新 resident。

锁序审计确认：backend lookup/create/mutation均不持 residency lock；parent publication/unpublication 与 residency 操作
分段执行；eviction/forget/drain 以及 parent `take_child` 返回的 capability均在相关 owner lock 外 drop；MountTree inner
guard 在 drain 前已释放，writer transaction只维持既有 teardown 编排，不形成 residency/parent/inode-cache 到 MountTree
的反向锁序。

一位独立 reviewer 审阅完整 diff、caller/mutation inventory、上述交错、last-view 判断和验证证据，结论为 0 Apollyon、
0 Keter、0 Euclid。Architecture Friction Scan 未发现第二份 namespace/membership 真相、owner 穿透、private
representation 泄漏、backend/caller/test 特判、public surface 扩张、无 consumer 抽象或无退出条件临时桥。

## Validation

- `just test xtask`：83/83 通过，覆盖新 Kconfig 参数的配置解析与生成。
- `just build --preset competition-final-rv64-release --bind smp=8 --bind memory=8G`：通过。
- `just build --preset competition-final-la64-release --bind smp=8 --bind memory=8G`：通过；这里只形成 LA64 compile/link
  证据，不外推 runtime。
- `./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img
  build/positive-dentry-residency-rv64.log`：603/603 KUnit通过；新增 retention/FIFO、stale admission/forget 与
  drain/ticket invalidation 三项均为 `ok`。当前 socket profile 的 glibc、musl 各 3/3 case通过，guest 正常进入
  PowerOff。
- `just fmt all --check` 与 `git diff --check` 通过。独立 review 后只增加本记录与导航，因此未浪费时间重跑
  kernel build 或 QEMU。
- **Not Run:** LA64 runtime、forced SMP interleaving或并发用户态 namespace stress、full LTP、final harness、实体硬件、
  性能/hit-rate/capacity A/B、allocation fault injection、shrinker/memory-pressure 与 runtime resize。

## Remaining Risk / Links

- [`ANE-20260801-VFS-CREATE-PUBLICATION-ATOMICITY`](../../register/open-issues.md#ane-20260801-vfs-create-publication-atomicity)
  保持 Open；本轮 freshness只阻止旧工作取得长期 residency，不为 backend commit 到 dentry publication 建立 rollback
  或完整 linearization。
- [`ANE-20260528-RAMFS-RENAME-STAGE1`](../../register/current-limitations.md#ane-20260528-ramfs-rename-stage1)
  保持 Active；本轮不迁移 rename 后已经返回的 live `PathRef`/dentry，也不补额外 rename flags。
- [`ANE-20260619-MOUNT-UNMOUNT-CLEANUP-STAGE1`](../../register/current-limitations.md#ane-20260619-mount-unmount-cleanup-stage1)
  保持 Active。drain 只使此前取得的 ticket stale；drain 后才开始并进入既有同步 unmount/recheck 窗口的 lookup，不由
  本轮建立新的 admission-closed final-cleanup 状态。lazy detach、retry/reaper、observer 与 `MNT_EXPIRE` 仍属后续工作。
- Current contract / RFC / transaction：None。performance signal不属于本轮 closure，容量 1024 与 FIFO 也不构成
  用户可依赖的性能或 replacement contract。
