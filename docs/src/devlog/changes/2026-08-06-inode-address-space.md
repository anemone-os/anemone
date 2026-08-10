# ANE-CHG-20260806-inode-address-space

**Type:** Internal owner consolidation
**Status:** Completed
**Date:** 2026-08-06
**Authors:** doruche, Codex
**Area:** VFS inode / filesystem address space / ramfs / ext4 / MM VMO

## Problem / Context

ramfs 与 ext4 原先各自维护 regular inode 的 resident page map、logical-size mirror 和
filesystem-specific `VmObject`。VFS inode metadata 同时保存另一份 size，普通写和 truncate 必须依赖调用顺序同步
多个值；page fill、跨页 copy、dirty、sync、cache accounting 与 mmap backing 也在两个 filesystem 中重复实现。

`VmObject` 还允许 backing 覆写可由 `resolve_frame` 完整推导的 byte-copy method。generic partial-page write 因此可能
对同一页先 read-resolve、再 write-resolve，扩大了调用次数和未覆盖字节的证明面。

## Decision

- regular inode 唯一拥有 identity、一个 `Arc<AtomicU64>` logical-size truth，以及可选的 `AddressSpace` capability；
  完整 metadata projection 继续叫 `InodeMeta`，锁内不含 size 的私有字段叫 `InodeMetaFields`。
- filesystem owner 内的 `AddressSpace` 唯一拥有 resident pages、dirty、backing-cache accounting 和 file-backed
  `VmObject`。ramfs 使用 volatile mode；ext4 只提供接收 page offset/buffer 的窄 fill/writeback backend。
- backend 不接收完整 inode、user buffer、page map 或 filesystem private lock，也不保存 size mirror、resident frame
  或 dirty。fill 在 page-map lock 外执行，并发 fill 最终只发布和计数一个 frame。
- `VmObject` 只保留 `resolve_frame`、`sync_range`、`discard_range` 与
  `exclusive_physical_pages` 等真实多态能力；`read_bytes` / `write_bytes` 成为 nonvirtual helper，每个 touched page
  只 resolve 一次。
- syscall ABI、errno、fault error domain、mmap signal、truncate/shared-mmap、ROFS 和 sticky-dirty 可见语义保持不变。
  本轮没有 current-contract cutover，也不关闭 register limitation。

## Change

- 增加 fs-private `AddressSpace` 与 `AddressSpaceBackend`，统一 ramfs/ext4 的 page publication、copy、dirty、sync、
  invalidation 和 cache accounting；删除两套 `RegState` / `RegMapping`、backend-local page map/size mirror 与
  filesystem-specific `impl VmObject`。
- inode size 从 metadata lock 中窄化为唯一 atomic cell；metadata snapshot、stat/EOF、file I/O、mmap admission 和
  truncate commit 都读取或更新该 cell。`InodeMeta` 保留原有完整聚合含义，没有改名为 `ResidentInodeMeta`。
- ordinary/direct-user I/O 在取得稳定 `FrameHandle` 后才执行 copy，不持有 page-map 或 backend transaction lock；
  ext4 writeback 失败保留 dirty，成功后也保持 sticky，truncate 继续在持久化成功后才 invalidate 并提交 size。
- 增加 3 个 `AddressSpace` inline KUnit 和 2 个 VMO byte-helper inline KUnit；增加可复用的 8-case
  `address-space` LTP group。默认 active profile 已恢复，不因本轮改变测试选择。

## Validation

- `just fmt kernel --check`、`git diff --check` 通过。source-shape audit 未发现 ramfs/ext4 `RegState`、`RegMapping`、
  filesystem-specific `impl VmObject`、backend-local resident map 或 size mirror；backend 参数与 user-copy lock
  boundary 也符合上述 owner 模型。
- 当前 RV64 单 HART运行完成 481/481 KUnit，5 个新增测试全部通过；boot、local pretests、focused LTP 与 orderly
  shutdown 完成。
- 同一 RV64 双 libc 前后对照实际运行了 9 个 case：baseline 每套 6/9，当前每套 8/9。`ftruncate01` 从
  SIGSEGV/TBROK 变为通过，`truncate02` 从 content TFAIL 变为通过；其余 case 没有新增 TFAIL、TBROK、timeout 或
  infrastructure failure。
- `mmap001 -m 32` 在 baseline 与当前都于 touch mapped memory 时收到 SIGSEGV/TBROK，对应既有
  [`ANE-20260529-FILE-BACKED-MMAP-FAULT-STAGE1`](../../register/current-limitations.md#ane-20260529-file-backed-mmap-fault-stage1)，
  不是本次回归。按维护者决定，最终 focused group 排除该 case；因此同一日志可逐项证明最终 8-case subset 为
  baseline 6/8、当前 8/8，但这不表示 `mmap001` 或该 limitation 已关闭。
- baseline 为解除无关 `blocked-connect-signal-cancel` 对执行的遮蔽，只在临时 baseline clone 中取消了该 socket
  case；它没有进入最终 diff，也不构成 socket 验证。wrapper 的 `All tests passed!` 可被 KUnit marker 满足，所以上述
  LTP 结论只来自逐 group/case 摘要，不使用 wrapper exit code 代替。
- LA64 release build 完成 discovery/final pass、symbol table verification 与 postbuild。独立 change review确认
  Apollyon、Keter、Euclid 均为 0；其唯一 Safe 注释路径问题已修正。
- **Not Run:** LA64 runtime、full LTP、final harness、SMP stress、实体硬件、write/truncate/sync 并发 stress，以及
  direct-user partial-copy fault injection。`mmap001` 在最终 focused group 中 Excluded；上面的旧限制证据不升级为
  本轮 acceptance。

## Remaining Risk / Links

- [`ANE-20260523-TRUNCATE-MMAP-COHERENCY`](../../register/current-limitations.md#ane-20260523-truncate-mmap-coherency)、
  [`ANE-20260523-EXT4-TRUNCATE-CACHE-INVALIDATION`](../../register/current-limitations.md#ane-20260523-ext4-truncate-cache-invalidation)、
  [`ANE-20260529-FILE-BACKED-MMAP-FAULT-STAGE1`](../../register/current-limitations.md#ane-20260529-file-backed-mmap-fault-stage1)
  与 [`ANE-20260528-ROFS-DIRECT-WRITE-STAGE1`](../../register/current-limitations.md#ane-20260528-rofs-direct-write-stage1)
  均保持 Active，本轮不改变其范围或退出条件。
- current contract：None。此次变化只收敛 fs 内部 state owner 与 Rust implementation shape，不改变 shared rule、
  syscall ABI 或可见语义。
- direct-user partial-copy 的不跨锁性质主要由 source audit 证明；若后续需要更强证据，应在 user-access fault
  injection owner 下增加定向测试，而不是扩大 `AddressSpaceBackend`。
