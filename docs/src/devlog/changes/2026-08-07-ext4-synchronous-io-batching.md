# ANE-CHG-20260807-ext4-synchronous-io-batching

**Type:** Small optimization
**Status:** Completed
**Date:** 2026-08-07
**Authors:** doruche, Codex
**Area:** inode address space / ext4 / synchronous I/O

## Problem / Context

ext4 regular-file I/O 已由 inode `AddressSpace` 统一拥有 resident page map、publication、dirty、logical size 与
backing-cache accounting，但原 backend handoff 仍逐页执行。连续 ordinary/direct-user read 每个未驻留页分别取得
ext4 transaction 并调用 lwext4；`sync_all()`、`sync_range()` 也对每个 dirty page 分别 writeback。lwext4 已能从较大
连续 buffer 形成 multi-block callback，因此这些逐页 transaction 和 filesystem call 是可以在 owner-local 边界内消除的
固定开销。

本轮只改变同步请求形状。它不引入异步 I/O、readahead、block scheduler merging、dirty retirement 或新的 durability
保证；extent、洞和非对齐边界仍可由 lwext4/设备层继续拆分。

## Decision

- `AddressSpace::read()` / `read_user()` 只在调用者本次 requested range 内发现连续 miss run；resident page、EOF、请求
  边界和 batch cap 都会切断 run。mmap/ELF 单页 fault 仍自然退化为单页请求，不产生 speculative readahead。
- `sync_all()` / `sync_range()` 按连续 dirty run writeback；clean resident page、非 resident page、range、EOF 与 cap 都会
  切断 run。每个 run 完成后重新读取 live page map，不保留 operation-wide frame authority。
- `AddressSpace` 继续唯一拥有 run discovery、frame allocation/publication、dirty、size admission 与 accounting；ext4
  backend 只接收 `(offset, contiguous byte buffer)`，并在一个既有 read/write transaction 中调用 lwext4。
- 非连续 frame 使用 operation-local bounded staging。`ext4_sync_io_batch_pages = 16` 由 KernelConfig/ext4 consumer
  拥有；内核以常开/compile-time assertion 拒绝零值和 allocation-size 越界，xtask 只 materialize，不 clamp 或解释该值。
- writeback 的 logical-size snapshot 必须先于对应 frame snapshot，并在整个 backend call 中保持固定。这样并发
  truncate-grow 不能让旧 EOF page 中已经 clone 的 tail 因新 size 而进入写回范围。
- writeback 成功后 dirty 仍故意保持 sticky；在没有 writable-PTE write-protect/TLB shootdown/redirty 协议前清除它会
  造成后续 mmap store 漏写。

## Implementation Boundary

**Target:** ordinary/direct-user requested-range read 对每个最大有界连续 miss run 至多提交一次 ext4 fill；
`sync_all()` / `sync_range()` 对每个最大有界连续 dirty run 至多提交一次 ext4 writeback，同时保持现有 ABI、errno、
partial-copy、EOF、truncate、sticky-dirty 与 durability 语义。

**Owners / handoff:** inode `AddressSpace` 拥有 page/cache/dirty/size 事实并提交 offset + bounded byte range；
`Ext4AddressSpaceBackend` 只拥有 ext4 inode identity 与 transaction capability；lwext4/`BlockDev` 继续拥有物理请求拆分。

**Failure / cleanup:** staging/run metadata 使用可失败预留并映射为现有 `SysError`。failed read batch 不发布该 batch 的
frame，先前完成的 batch/user-copy prefix 保持提交；failed writeback 不清除 dirty。page-map lock 不覆盖 backend I/O，
backend lock 不覆盖 user copy。

**Protected surface:** 不修改 public/shared API、syscall ABI、errno、current contract、`BlockDev`、fsync/sync syscall 接线、
dirty retirement 或 register 范围。本轮 consumer 是 ordinary/direct-user read、VMO range sync、inode sync/eviction、
truncate 前同步与 unmount；不能把结果扩大为设备 request 数或严格 durability 声明。

**Contract Impact / Cutover:** `None`。这是 fs-private `AddressSpace` / ext4 backend handoff 的 owner-local implementation
cutover。

## Change

- `AddressSpaceBackend` 从单页 fill/writeback 改为 bounded range capability，并由 backend 提供正数 page cap。
- read path 为每个 miss run 一次分配 bounded staging 和 frame set，完整 backend fill 成功后才逐页 publication；已有
  concurrent publication winner 不被覆盖，也不重复 cache accounting。
- writeback path 流式发现一个 bounded dirty run、snapshot 一次 pre-frame logical size、提交一次 backend call，再从
  live map 发现下一 run；不再为整个 dirty set 建立无界 snapshot。
- ext4 backend 对每个 range 只取得一次既有 transaction/fs lock；Kconfig/default/generated definition 增加 owner-local
  16-page cap。
- 增加 7 个 inline KUnit，覆盖 request/resident/cap/EOF 分界、batch failure publication、已完成 prefix、publication
  winner/accounting、clean gap、后一 writeback run failure、sticky dirty、`sync_range` errno/shape，以及 clone 后 grow 仍
  使用旧 EOF write length。

## Validation

- `just fmt kernel --check`、`git diff --check`、`just test xtask`（83/83）与 `just test lwext4`（2/2）通过。
- RV64 single-HART wrapper 重新完成 rootfs、release discovery/final build、6338-symbol verification、boot 与
  508/508 KUnit；11 个 `AddressSpace` KUnit 全部通过。
- focused `address-space` LTP group：glibc 8/8、musl 8/8，合计
  `attempted=16, passed=16, failed=0, infra_failed=0`；结论来自逐 group/case summary，不以通用
  `All tests passed!` marker 代替 LTP 证据。guest 随后完成 filesystem/network/device shutdown 并进入 orderly
  PowerOff。
- 临时 LTP profile 已恢复为 `socket`，没有进入最终 diff。独立 change review 在 size-before-frame 修正后确认
  Apollyon、Keter、Euclid 均为 0；direct-user 已有 copy progress 转 short success 的行为由 `File` wrapper source audit
  与 focused syscall run 共同支撑，没有为测试扩大 production API。
- **Not Run:** LA64 build/runtime、full LTP、final harness、实体硬件、SMP、性能 A/B、write/truncate/sync 并发 stress、
  direct-user copy-fault injection 与 crash/power-loss durability。

## Remaining Risk / Links

- [`ANE-20260523-TRUNCATE-MMAP-COHERENCY`](../../register/current-limitations.md#ane-20260523-truncate-mmap-coherency) 与
  [`ANE-20260523-EXT4-TRUNCATE-CACHE-INVALIDATION`](../../register/current-limitations.md#ane-20260523-ext4-truncate-cache-invalidation)
  保持 Active。当前 run snapshot 后的 concurrent shrink/invalidation 仍属于既有 page-cache/truncate 并发限制；本轮未
  引入 generation/in-flight protocol，也不把该限制写成已关闭。
- [`ANE-20260528-OPEN-STATUS-FLAGS-STAGE1`](../../register/current-limitations.md#ane-20260528-open-status-flags-stage1)
  保持 Active；本轮 batching 不实现 `O_SYNC` / `O_DSYNC` 或其它同步写 durability 语义。
- current contract：None。未来第二个 persistent filesystem 若需要同类 batching，应基于真实 consumer 重新判断 policy
  owner，不能把当前 ext4 cap 自行提升成 generic filesystem truth。
