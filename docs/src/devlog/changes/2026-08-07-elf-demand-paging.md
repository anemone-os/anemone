# ANE-CHG-20260807-elf-demand-paging

**Type:** ELF demand-paging small iteration
**Status:** Completed
**Date:** 2026-08-07
**Authors:** doruche, Codex
**Area:** execve / ELF loader / userspace MM / inode address space

## Problem / Context

ELF loader 原先在 `exec` 临界路径把主程序和 `PT_INTERP` interpreter 的全部 `PT_LOAD` body 复制到匿名
`LoadChunk`。这让未被访问的代码和数据也提前发生文件读取、页面分配与复制，并且 read-only text 无法直接复用
inode `AddressSpace` 已拥有的 file page。

regular inode 已通过 `AddressSpace` 统一 file-backed page fill、publication 和 cache ownership。此次迭代因此只需让
ELF owner 把 image layout 表达为 lazy backing；不需要第二套 page cache，也不让 generic mmap owner 接管 ELF 的
segment composition、BSS、permission 或 auxv policy。

## Decision

- 主 executable 与 `PT_INTERP` interpreter 使用同一条 lazy load path。构造阶段仍同步读取并校验 ELF header、
  program headers 与 interpreter pathname，但不读取 `PT_LOAD` body。
- inode `AddressSpace` 保持唯一 file-page source/cache。完整 read-only file page 可直接复用其 frame；write fault、
  partial file page、BSS 和 overlap page 才物化为 process-image-local frame。
- ELF loader 保持 image-layout owner：它负责 file/virtual offset translation、page permission union、program-header
  顺序下的 overlapping bytes、zero fill、load bias/range validation 与 private/COW 行为。
- 当前缺少 executable-vs-writer accounting，因此 backing 仍是 live file view。本轮不实现 snapshot/version pinning、
  eager fallback 或 `ETXTBSY`，并发 write/truncate 的可见性继续由既有开放问题覆盖。

## Implementation Boundary

**Target:** kernel 直接装载的全部 ELF image 只在构造期读取必要 metadata，`PT_LOAD` body 在首次访问时按页解析；
无并发 writer 时保持既有 image bytes、permissions、BSS、entry/PHDR/interpreter metadata 与 fork/private-write 行为。

**Owners / handoff:** ELF loader 唯一拥有 segment layout 与 page recipe；inode `AddressSpace` 唯一拥有 file page；MM
VMA/fault owner 只消费 `VmObject` capability。metadata 校验完成后 ELF loader 向新 `UserSpace` 交付 lazy VMA，fault
时再把必要的 file-page request 交给 inode mapping。

**Failure / cleanup:** metadata 中可发现的 overflow、越界、page incongruence 和 malformed range 继续在 exec 提交前
失败。尚未发布的 recipe 随构造失败回收；image-local materialized frame 随 VMO 生命周期回收；discard 只删除这些
private frame，不修改 inode mapping。

**Protected surface:** syscall ABI、exec errno、auxv、load bias、page permissions、zero fill、private writes、fork/COW、
普通 `DT_NEEDED` library 的 userspace-rtld owner 和 current contract 均不改变。并发 writer/truncate snapshot 语义不在
本轮 target 中。

**Contract Impact / Cutover:** `None`。这是 exec/ELF owner-local implementation cutover，不修改 shared rule、register
范围或退出条件。

## Change

- 将 `PT_LOAD` planning/backing 从 `parse.rs` 拆到 owner-local `segment.rs`；`load_image` 与 `load_interpreter` 都调用
  `map_load_segments`，原 eager `collect_load_chunks(file, ...)` 路径被删除。
- 增加 `ElfLoadObject`。完整 file page 的 read/execute fault 直接返回 inode mapping frame 并强制只读；write fault、
  partial/BSS/overlap page 按 recipe 物化 zeroed private frame，且 overlapping file bytes 保持 program-header 后写覆盖。
- VMA 沿用 `ForkPolicy::CopyOnWrite` 与现有 `ShadowObject`；ELF-local `discard_range` 只丢弃 materialized page，后续
  fault 重新从唯一 source 解析。
- metadata validation 增加 checked file/address arithmetic、file-size bound、`filesz <= memsz`、page congruence、
  userspace boundary 与 load-bias/page-align overflow 检查。
- ELF owner inline KUnit 覆盖 lazy source resolution、direct read-only reuse、partial/BSS/overlap composition、private
  writes、fork shadow isolation 与 discard/refault。

## Validation

- `just fmt kernel --check` 与 `git diff --check` 通过。RV64 release build 的 discovery/final pass 与 symbol verification
  通过；sandbox 内同一 build 曾在 vendored lwext4 遇到 `Bad system call`/SIGSYS，完全相同的命令在 sandbox 外通过，
  因此归类为环境限制而非代码失败。
- source/owner audit 确认构造期 file read 只剩 ELF header、program headers 与 `PT_INTERP` pathname；main/interpreter
  共用 lazy helper；没有第二份 file-page cache、filesystem-direct I/O、eager fallback、kernel `DT_NEEDED` 解析、
  architecture/libc 特判或 public/shared API 扩张。
- RV64 单 HART运行完成 487/487 KUnit。新增 ELF tests 覆盖的 helper/backing、fork shadow 与 discard/refault 均通过。
- focused wrapper 中 glibc 与 musl 各自执行 `execve01`、`execve05`、`execveat01`，每套均为
  `attempted=3, passed=3, failed=0, infra_failed=0`，合计 6/6，并 orderly PowerOff。结论来自逐 case/group summary，
  不使用 wrapper 的通用 `All tests passed!` marker 代替 LTP 证据。
- 第一次 wrapper 在进入 focused LTP 前被既有 `socket-test` 的 `blocked-connect-signal-cancel` 中断，不是 ELF
  acceptance failure。经维护者授权，成功运行时临时关闭 `socket-test`；相关 user-test/profile/group 改动随后全部
  恢复，没有进入最终 diff，也不构成 socket 验证或行为变更。
- 独立 change review 未发现 Apollyon 或 Keter。review 提出的 fork/discard Euclid coverage 已补齐；真实
  `load_image` / `load_interpreter` 入口仍由 source audit 证明，没有为了 test injection 扩大 production visibility 或
  建立 probe。这一 residual validation gap 不改变 owner/ABI/contract closure。
- Architecture Friction Scan 未发现第二份状态真相、owner 穿透、private representation 泄漏、public API 扩大、
  调用者/架构/libc 特判、无退出条件 bridge、隐含 cleanup 顺序或 validation 降级；上述入口测试缺口是唯一 Euclid。
- **Not Run:** LA64 runtime、full LTP、final harness、实体硬件、SMP/exec-writer stress、`execve04`、malformed-metadata
  fault injection、普通 shared-library relocation/lazy-binding 专项与性能 A/B。

## Remaining Risk / Links

- [`ANE-20260528-EXEC-ETXTBSY-WRITER-ACCOUNTING`](../../register/open-issues.md#ane-20260528-exec-etxtbsy-writer-accounting)
  保持 Open：resident frame 与 future fault 仍可能观察并发 write/truncate，本轮不承诺 executable snapshot。
- [`ANE-20260529-FILE-BACKED-MMAP-FAULT-STAGE1`](../../register/current-limitations.md#ane-20260529-file-backed-mmap-fault-stage1)
  保持 Active：本轮不改变 generic file-backed fault 的 hole/EOF error 与顶层 signal 分类。
- current contract：None。若后续需要关闭并发 writer/truncate 风险，必须由 executable-vs-writer protocol owner
  实现并证明，不能在 ELF backing 中增加私有 cache 或 eager snapshot workaround。
