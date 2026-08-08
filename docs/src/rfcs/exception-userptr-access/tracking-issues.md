# Exception-backed User Pointer Access Tracking Issues

**状态：** Active；R0已关闭，本页保留仍有效的validation boundary与已neutralize依赖的导航。
**最后更新：** 2026-08-09
**父 RFC：** [RFC-20260801-exception-userptr-access](./index.md)
**事务日志：** [2026-08-01-exception-userptr-access](../../devlog/transactions/2026-08-01-exception-userptr-access.md)；
R0实现和验收早于公共RFC，历史代码由`4ed73b76`、`70f422a0`、`97e02808`保存。

本文只跟踪仍会影响future user-access/MM修改顺序、review gate或验收解释的问题。R0 exception window、copy
和partial语义已经由用户验收；Active条目不能被解释为R0未实现。

## Apollyon

当前无RFC-local Apollyon。

## Keter

当前无项。

## Euclid

当前无项。

## Safe

### UACCESS-SAFE-001 - Runtime userptr app未接入默认runner

**状态：** Active / accepted validation boundary。

`anemone-apps/userptr`覆盖bad address、lazy/cross-page、partial、permission/unmap和side-effect ordering，
但`anemone-apps/user-test/src/main.rs`中的启动代码当前被注释。R0依据用户已完成的历史验收关闭；默认构建或
普通LTP运行不等于自动执行该oracle。

**Exit Condition：** future若需要持续回归，在不改变production semantics的前提下把app接入明确的focused
profile/runner，并分别保存RV64/LA64结果。该项不要求为了“测试自动化”重新打开R0。

## Neutralized

### UACCESS-KETER-001 - Remote fence 仍在 UserSpace mutex 内完成

**状态：** Neutralized by
[`USER-TLB-COMPLETION-CUTOVER`](../user-tlb-completion/index.md#closure)，2026-08-09。

历史`RemoteUspFenceGuard`会让exception userptr在持有`UserSpace` mutex时同步广播IPI。当前architecture accessor
只持有`UserSpaceGuard`窄能力；page fault通过`run_tlb_transaction()`在mutex内完成PTE mutation与local invalidation，
随后释放mutex、同步等待allocation-free user-TLB transport ack，再重新取得mapping owner并retry。ordinary copy、
explicit `fault_in_page()`、non-active probe及其它production mutation caller已由独立source review闭合；没有raw
`with_usp()`/mutex路径丢弃completion obligation。

每个address space在发布前准备transport storage；当前boot-fixed online set使post-mutation completion不返回
recoverable allocation/offline failure。COW/replacement retirement活到ack之后。RV64/LA64 SMP=8 release QEMU的
597项KUnit与六组`userptr`均通过；这些运行结果是被执行路径的集成/回归证据，owner、lifetime与happens-before
closure由源码审查承担，不把并发测试未暴露问题写成穷举证明。

当前规则由[`MM-TLB-REMOTE-001`](../../contracts/mm/user-fault-local-tlb.md#mm-tlb-remote-001--destructive-user-mapping在dependent-continuation与retirement前完成remote-ack)
拥有；本页只保留原issue到修复来源的导航。
