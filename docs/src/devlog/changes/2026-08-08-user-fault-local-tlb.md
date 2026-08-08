# ANE-CHG-20260808-user-fault-local-tlb

**Type:** MM local-TLB capability / contract-bearing small iteration
**Status:** Completed / R1
**Date:** 2026-08-08
**Authors:** doruche, Codex
**Area:** MM / user address space / page fault / paging / RV64 / LA64

## Problem / Context

actual user page fault原先在VMA安装或更新leaf PTE后无条件执行eager local TLB invalidation。Mapper不报告同一次
commit的old/new relation，统一fault入口也没有区分“返回user mode后由hardware retry”和“kernel在函数返回后立即
访问”。直接对统一入口延迟completion会让kernel userptr recovery在没有完成invalidation时立刻retry，因而不成立。

本轮先建立operation-local transition与显式continuation，再只对actual user fault的`Added`启用lazy local
completion；remote shootdown、COW、failure和cleanup保持原边界。

## Decision

- Mapper继续唯一拥有leaf PTE mutation，并在原page-table walk和commit point返回`Added`、`Unchanged`、
  `Relaxed`或`ReplacedOrRestricted`。relation只描述本次operation，不缓存为address-space state。
- RV64/LA64 ordinary user trap进入`UserReturn`；四个architecture userptr recovery点以及futex、explicit fault-in与
  non-active probe统一进入`Immediate`。
- 只有`UserReturn + Added`延迟local invalidation。已安装leaf保持时，原access若因invalid/restrictive translation
  refault，下一次relation不再为`Added`并必须eager；intervening destructive mutation造成的再次`Added`属于R1 limitation。
- `Immediate`和所有present/destructive relation保持eager。policy不使用architecture、machine、QEMU或test特判。

## R1 Target Renegotiation

CKPT 2 review发现：另一个CPU完成destructive PTE mutation和自己的local invalidation、释放`UserSpace` mutex，但
`RemoteUspFenceGuard`尚未Drop时，当前CPU可能仍持old restrictive translation；它对新mapping fault并提交
operation-local `Added`后，user-entry Signal arbitration可能在原instruction refault前改写trapframe进入handler。
因此operation-local `Added`不证明全局fresh mapping，handler可能在remote completion前短暂观察predecessor mapping。

维护者明确接受该窗口并要求坚持lazy路线。R1把target guarantee收窄为本文的operation-local completion policy，不
承诺该cross-CPU predecessor / Signal redirect窗口内的mapping-identity强一致性；限制登记于
[`ANE-20260808-MM-LAZY-LOCAL-TLB-REMOTE-PREDECESSOR-WINDOW`](../../register/current-limitations.md#ane-20260808-mm-lazy-local-tlb-remote-predecessor-window)。
本轮不设计remote-fence predecessor handoff、pending generation或user-entry retry capability，也不把该限制写成已经
由单核runtime证明安全。

## Implementation Boundary

Mapper唯一拥有PTE truth与commit classification；VMA/VMO继续拥有frame resolve、COW、permission与mapping flags；
MM UserSpace resolver唯一拥有relation加continuation到local-completion的decision；architecture只交付trap identity并
实现具体TLB primitive。relation来自同一次commit，不引入第二walk、pending bit、epoch、cache或performance counter。

受保护边界是syscall ABI、errno、signal、一次retry、partial prefix、COW、permission、unmap/mprotect/discard/brk、
remote fence、IPI failure、frame retirement及RV64/LA64 primitive；R1 limitation是唯一明确的visible guarantee缩减。
本轮不改变
[`ANE-20260807-MM-REMOTE-FENCE-FAIL-CLOSE-RETENTION`](../../register/current-limitations.md#ane-20260807-mm-remote-fence-fail-close-retention)
或exception userptr RFC记录的remote-fence lock handoff。

## Checkpoints / Change

### CKPT 1 — Transition与continuation准备

`1ab08325`在不改变local completion行为的前提下增加Mapper commit relation与valid-ancestor保守分类，收紧fault owner
surface，并让actual trap、architecture userptr及`fault_in_page()` consumer显式选择continuation。全部组合仍eager，
current contract不变。独立source review确认该checkpoint安全且没有finding。

### CKPT 2 — Actual-fault local completion cutover

MM resolver只让`UserReturn + Added`延迟local completion，并以关键注释和inline KUnit固定non-`Added` refault completion、
Immediate eager及R1 limitation。代码、[`MM-TLB-LOCAL-001`](../../contracts/mm/user-fault-local-tlb.md#mm-tlb-local-001--actual-user-fault按operation-local-added延迟local-completion)、
current limitation、本记录与导航在第二个focused commit原子生效；没有research counter、platform cfg、persistent state
或临时双路径。

## Contract Impact / Cutover

| Contract ID | 变化 | Cutover前baseline | Effective rule | 生效证据 |
| --- | --- | --- | --- | --- |
| [`MM-TLB-LOCAL-001`](../../contracts/mm/user-fault-local-tlb.md#mm-tlb-local-001--actual-user-fault按operation-local-added延迟local-completion) | Introduce | None；actual user fault无条件eager local invalidation | `UserReturn + Added`延迟；non-`Added` refault、Immediate及其它relation eager；remote predecessor/intervening mutation窗口按current limitation排除 | relation/policy KUnit、caller audit、RV64/LA64单核runtime、R1独立review |

[`USER-ENTRY-001/002`](../../contracts/task/user-entry.md)是未变化Dependency。remote shootdown与frame retirement没有
新增contract delta。

## Validation

- `just fmt kernel --check`与`git diff --check`通过。RV64首次sandbox build在lwext4 C编译处因SIGSYS报告
  `Bad system call`；同一个repository-owned build命令在非sandbox环境通过，该失败是执行环境限制而非source failure。
- RV64与LA64的SMP=1 release kernel build均通过。两个architecture分别以临时focused rootfs完成588/588 KUnit，
  包括Mapper relation与local-completion policy；`/bin/userptr`的scalar bad-address、lazy/cross-page、partial
  cross-page fault、permission/unmap、EFAULT side effect和wait4 copyout ordering六组case全部通过。
- RV64完成filesystem、network与device shutdown并由platform PowerOff退出QEMU，exit code 0。LA64完成相同orderly
  shutdown顺序；该QEMU machine没有成功的power-off handler，kernel进入terminal halt后由控制台结束，QEMU exit code 0。
- CKPT 2初次独立review识别remote predecessor / Signal redirect窗口；维护者将其接受为R1 current limitation。R1复核
  确认实现、contract与validation不再把该窗口写成已证明correctness，其余owner/caller/cleanup边界没有finding。
- `mdbook build docs`、最终whitespace/navigation检查与Architecture Friction Scan通过。operation-local relation与
  cross-owner predecessor obligation的差异保留在current limitation中，没有伪造成第二份PTE truth。
- **Not Run:** production performance A/B、Cargo/direct rustc benchmark与性能门槛；optimize分支合流或after study；
  RV64/LA64 SMP>1、remote predecessor / Signal redirect forced interleaving、IPI/remote-shootdown stress与CPU-hotplug；
  实体硬件及实体平台performance；full LTP、BuildStorm、final harness与final score。

## Remaining Risk / Links

- [`ANE-20260808-MM-LAZY-LOCAL-TLB-REMOTE-PREDECESSOR-WINDOW`](../../register/current-limitations.md#ane-20260808-mm-lazy-local-tlb-remote-predecessor-window)
  是R1 accepted limitation；单核PASS、普通refault reasoning或Mapper relation KUnit不能替代其future SMP proof。
- local policy不关闭
  [`ANE-20260807-MM-REMOTE-FENCE-FAIL-CLOSE-RETENTION`](../../register/current-limitations.md#ane-20260807-mm-remote-fence-fail-close-retention)，
  也不修正exception userptr RFC中的
  [`UACCESS-KETER-001`](../../rfcs/exception-userptr-access/tracking-issues.md#uaccess-keter-001---remote-fence-仍在-userspace-mutex-内完成)。
- [User Fault Local TLB Completion当前契约](../../contracts/mm/user-fault-local-tlb.md)是effective policy的唯一正文。
  后续性能研究需要独立授权和新baseline，不能用历史结果替代production implementation的after evidence。
