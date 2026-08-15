# ANE-CHG-20260816-user-fault-ahead

**Type:** MM performance / contract-bearing small iteration
**Status:** Completed
**Date:** 2026-08-16
**Authors:** doruche, Codex
**Area:** MM / user fault / VMA / VMO / inode address space / ELF

## Problem / Context

真实用户缺页原先只解析并安装 fault 所在的一页。顺序访问因此对每页重复进入 trap、VMA/VMO resolve、页表 walk；
文件页 fault 还把 inode `AddressSpace` 已有的 bounded range fill 退化成单页 ext4 请求。已有
[Ext4 synchronous I/O batching](./2026-08-07-ext4-synchronous-io-batching.md)刻意不做 speculative readahead，
因为当时没有来自 MM fault owner 的有界 locality request。

本轮以性能提升为目的，但不设数值门槛。正确性是 cutover 硬门槛：demand page 的 errno、permission、COW、dirty、
stack growth 与 TLB completion 不能因 speculation 改变。BuildStorm 由维护者在本轮提交后独立评估，不构成本次
completion oracle。

## Decision

- 一次 actual user fault 先按原路径解析 demand page；仅在成功后，把它扩展为最多
  `user_fault_window_pages` 页、向高地址、同一 VMA 的同步 locality transaction。该 Kconfig 常量包含 demand page，
  当前默认值为 8。
- `UserSpace` 拥有窗口与 reservation admission：ordinary VMA 截止于 VMA end，heap 还截止于 `brk.page_up()`；
  stack 本轮保持单页，避免 speculation 参与 stack-growth policy。`Immediate` continuation 仍严格单页。
- `VmObject::resolve_frame_ahead()` 是默认拒绝的窄能力。每次只返回当前 follower 的 frame authority；exclusive
  `request_end` 只允许 VMO owner 合并自己的 clean work，不能把 initiating Write 当成 follower 已真实写入。
- VMA 只安装此前 absent 的连续 follower leaf；遇到 present leaf、VMO `None`/error 或 OOM 即停止。每个成功 commit
  必须是 `LeafPteCommit::Added`，已成功的 demand page、page-cache publication 与 PTE prefix 不回滚。
- `AddressSpace` 复用已有 EOF、resident boundary 与 backend batch cap，一次 follower request 可以自然填充 clean
  file-page run。Write fault 的 follower 以 Read 解析，因此不提前发布 sticky dirty，也不获得 writable file PTE。
  ELF 只对连续 direct-file read/execute recipes 透传该 clean range；write、partial/overlap/BSS 未 materialized 页拒绝。
- Anon/Fixed 可以返回自然 frame；SysV shm 只返回 already-resident frame，避免提前改变 `shm_rss`；Shadow write 只返回
  already-resident overlay，不能 speculative COW 或消费 decommit marker。Shadow read/execute 保持 parent-before-overlay
  lock order并允许 parent owner决定是否 opt in。

## Implementation Boundary

**Target:** 在不改变 demand-fault correctness 的前提下，把 actual user fault 转换为 bounded synchronous same-VMA
locality transaction，减少顺序访问的 fault/PTE 固定开销，并让 inode range fill 成为真实 fault consumer；本轮不要求
BuildStorm 或 microbenchmark 达到指定百分比。

**Owners / handoff:** `UserSpace`/VMA 拥有 virtual window、heap/stack bounds、PTE admission与TLB completion；VMO拥有
speculation safety、frame与writability；inode `AddressSpace`拥有page-cache publication、dirty、EOF、resident boundary与
backend batch cap；ext4 backend仍只执行 range I/O。

**Failure / cleanup:** exact demand error原样传播。follower refusal、error或OOM只终止 speculation；page-cache/PTE成功
前缀保留，page-table中间branch继续由`PageTable`拥有并可复用。speculation不建立异步任务、rollback owner或额外状态。

**Protected surface:** syscall ABI/errno、permission、COW/decommit、sticky dirty、stack growth、brk、remote completion、
frame retirement、`Immediate` callers和current owner split。非目标包括异步队列、自适应 predictor、persistent access
history、reclaim/eviction、huge page、lwext4 parallelization、COW ancestry问题和BuildStorm特判。

## Change

- KernelConfig增加包含demand page的actual-user-fault window bound；xtask只materialize该正数常量。
- VMA增加absent-only follower commit，actual `UserReturn` fault在exact success后按ordinary/heap边界调用；stack与
  `Immediate`不执行ahead。
- VMO opt-in按各自可见状态和COW/dirty边界实现；`AddressSpace`和ELF direct-file path复用既有clean range fill。
- owner-local deterministic KUnit覆盖窗口/continuation、absent/present/decline prefix、AddressSpace clean batching、
  Shadow write/decommit与ELF direct-file range forwarding；没有production test hook或live scheduling。
- 修正Mapper的过期OOM注释：已发布的中间page-table frame由`PageTable`持有并可复用，best-effort caller也无需回滚。

## Contract Impact / Cutover

| Contract ID | 变化 | Cutover前 baseline | Effective rule | 生效证据 |
| --- | --- | --- | --- | --- |
| [`MM-TLB-LOCAL-001`](../../contracts/mm/user-fault-local-tlb.md#mm-tlb-local-001--actual-user-fault按operation-local-added延迟local-completion) | Refine | actual user fault只commit demand leaf | exact success后可提交bounded same-VMA absent followers；followers仅`Added`且failure不改变demand result；`Immediate`仍单页 | focused KUnit、owner/caller/source review、双架构build与用户态smoke |

## Validation

- `just test xtask`：108/108通过，覆盖新增KernelConfig字段的解析与generated constant materialization。
- `just build --preset qemu-virt-rv64-release`与`just build --preset qemu-virt-la64-release`：最终源码形状、default
  KernelConfig、release profile均构建通过。
- RV64、SMP=1、release QEMU使用fresh ext4 rootfs与default feature tuple；validation-only log ceiling恢复为5后，
  boot-integrated KUnit 672/672通过。focused coverage包括fault window、absent/present/decline prefix、clean
  AddressSpace batching、Shadow write/decommit和ELF direct-file forwarding。
- 同一最终RV64 validation kernel通过`rlimit-test`用户态smoke：11/11 case输出`RLIMITTEST:PASS`，随后完成filesystem、
  network与device shutdown并进入PowerOff。
- `just fmt kernel`、`mdbook build docs`与`git diff --check`通过。格式化器触及的无关`proc/pde.rs` import排版未纳入
  本轮diff。
- 本地working tree中维护者已有的default log ceiling `0`修改会使既有Nemophila logging KUnit无法构造`Err`级策略；
  它不是fault-ahead失败，也不属于本轮commit。validation恢复原有ceiling `5`后全套KUnit通过。
- **Not Run / user-owned:** BuildStorm与其性能A/B、optimize分支after study。

## Remaining Risk / Links

- 固定向前窗口是bounded policy，不是自适应顺序性证明；随机访问可能产生无用resident page/PTE。本轮只要求代码形状
  确实减少连续fault固定工作，具体收益与窗口调优由BuildStorm/后续性能研究决定。
- `ANE-20260727-MM-COW-SHADOW-ANCESTRY-STACK-OVERFLOW`保持独立Open；本轮Shadow opt-in不改变其parent ancestry。
- 当前契约：[User Address-Space TLB Residency 与 Completion](../../contracts/mm/user-fault-local-tlb.md)。
