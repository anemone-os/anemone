# ANE-CHG-20260817-signal-blocked-ignored-admission

**Type:** Signal bugfix / contract-bearing small iteration
**Status:** Completed
**Date:** 2026-08-17
**Authors:** doruche, Codex
**Area:** signal / task / signalfd

## Problem / Context

Signal generation原先只读取disposition，因而在写入private / shared pending前丢弃所有显式ignore或default-ignore
occurrence。child exit产生的default-ignore `SIGCHLD`即使已被parent block也会消失，signalfd与同步signal wait没有可消费的
Signal occurrence；这与Linux“blocked signal不能在generation按ignored丢弃”的规则相反，也暴露了
[`SIGNAL-ACTION-001`](../../contracts/signal/pending-routing.md#signal-action-001--ignored-generation-admission-同时读取-live-mask)
自身的错误baseline。

这不是signalfd缺少一个child-exit入口。signalfd只消费Signal pending，child status继续由wait-core拥有；在signalfd内部合成
`SIGCHLD`会建立第二份occurrence truth，并绕过普通signal generation、timer与job-control路径共同面对的admission问题。

## Decision

generation discard统一为`action.is_ignored() && !admission_mask.contains(signal)`。private occurrence使用exact target；shared
occurrence使用已有live member snapshot中的一个member提供当下mask与共享disposition视图。该member只服务本次判定，不写入
pending、不成为delivery identity，也不引入ThreadGroup mask cache或全组聚合。ordinary、job-control、普通timer与
job-control timer都使用同一个纯predicate。

admission与publication在原有pending owner锁内完成，再按既有`sig_pending -> sig_mask -> sig_disposition`锁序读取状态。
这样`rt_sigaction`先发布ignored disposition、随后flush pending的既有序列与并发generation原子闭合；设置ignored仍会无条件
flush已经pending的同号occurrence。ordinary delivery继续在dequeue后读取live action。

## Implementation Boundary

**Target:** blocked explicit-ignore与default-ignore occurrence不在generation丢失，仍进入其自然private / shared pending owner，
可被signalfd / synchronous wait消费；unblocked ignored occurrence继续在generation丢弃。

**Owners / handoff:** `Task::sig_pending`与`ThreadGroupInner::sig_pending`继续是occurrence唯一owner；per-task mask与共享
`SignalDisposition`只提供live admission snapshot；signalfd只按原路径dequeue pending；wait-core继续独占child status。

**Failure / cleanup:** standard / realtime coalescing、timer slot与锁外callback、`rt_sigaction` flush、pending teardown、
job-control opposite-class cleanup和`SIGSTOP` direct consumption保持既有owner与顺序。没有新allocation、wake source、
lifecycle或rollback owner。

**Protected surface / non-goals:** 不改变signalfd UAPI、pending routing、ordinary action selection、job-control phase/report、
timer registration identity或child wait ABI。不处理`rt_sigtimedwait`既有异步完成缺口，不实现`SA_NOCLDWAIT`或显式
`SIG_IGN` SIGCHLD autoreap，不承诺Linux mixed-thread mask下的exact target选择，也不在signalfd伪造SIGCHLD。若实现需要
保存parent task identity、聚合跨task mask或增加新的ThreadGroup lifecycle state，本小迭代停止并升级RFC。

## Change

- `SignalAction`新增generation-specific纯predicate，保留`is_ignored()`作为delivery、terminal与`rt_sigaction`语义。
- ordinary private/shared、job-control ordinary/timer、POSIX timer private/shared在pending锁内读取mask、disposition并发布。
- owner-local KUnit覆盖predicate truth table、private/shared default-ignore、explicit-ignore job-control及timer admission；测试恢复
  修改过的mask、disposition与pending。
- `signalfd-test`覆盖blocked explicit-ignore `SIGWINCH`和blocked default-ignore child-exit `SIGCHLD`，并在signalfd dequeue后
  再用`wait4`回收相同child status。

## Contract Impact / Cutover

| Contract ID | 变化 | Cutover前 baseline | Effective rule | 生效证据 |
| --- | --- | --- | --- | --- |
| [`SIGNAL-ACTION-001`](../../contracts/signal/pending-routing.md#signal-action-001--ignored-generation-admission-同时读取-live-mask) | Refine | ignored action无条件在pending前丢弃 | 仅unblocked ignored occurrence在generation丢弃；blocked occurrence进入自然pending owner，ordinary delivery仍读取live action | owner-local KUnit、双架构build、RV64 signalfd acceptance与source audit |

## Validation

- `just fmt kernel`与`just fmt signalfd-test`通过；`just app build --arch riscv64 signalfd-test`和对应
  `loongarch64`命令通过。
- `just build --preset qemu-virt-rv64-release`与`just build --preset qemu-virt-la64-release`串行通过。首次并行尝试因两个
  preset共享`build/generated`，LA64编译读到RV64 projection并报`EARLYCON_REG`缺失；未修改源码或手工生成文件，串行重跑
  闭合两套最终tuple。
- RV64、SMP=2、1 GiB、default KernelConfig / release的signalfd acceptance boot通过695/695 KUnit；新增
  disposition、ordinary private/shared、job-control与timer KUnit均执行，app输出
  `SIGNALFD:CASE:blocked-ignored-sigchld:pass`和`SIGNALFD:PASS`，随后orderly poweroff。
- LA64、SMP=1、1 GiB、default KernelConfig / release的同一acceptance boot通过698/698 KUnit和全部signalfd app case；
  guest完成`SIGNALFD:PASS`及orderly shutdown步骤后停在该平台既有的无poweroff handler末态，由host `Ctrl-C`结束QEMU。
- RV64 preliminary LTP signalfd profile通过同一最终源码的695/695 KUnit；glibc与musl各通过`signalfd01`、
  `signalfd4_01`、`signalfd4_02`，合计attempted=6、passed=6、failed=0、infra_failed=0、skipped=0并正常关机。该组只作
  ABI回归，不替代新增SIGCHLD product oracle。
- `mdbook build docs`与`git diff --check`通过。
- **Not Run:** LA64 LTP、full LTP、mixed-thread mask target stress、`rt_sigtimedwait`异步完成与穷尽并发交错。

## Remaining Risk / Links

- shared occurrence沿用一个live member作为瞬时admission view；本轮不引入exact Linux mixed-thread target identity。若真实
  multi-thread workload证明必须跨topology / reparent lifecycle保存parent task identity，应独立升级RFC，而不是缓存第二份
  mask或在signalfd补偿。
- [`ANE-20260722-SIGNAL-SIGTIMEDWAIT-ASYNC-COMPLETION`](../../register/open-issues.md)保持独立Open；本轮没有改变
  `rt_sigtimedwait`的wake/completion协议。
