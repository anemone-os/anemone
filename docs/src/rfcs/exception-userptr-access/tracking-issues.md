# Exception-backed User Pointer Access Tracking Issues

**状态：** Active；R0已关闭，本页只保留不属于R0修复授权的继承风险。
**最后更新：** 2026-08-01
**父 RFC：** [RFC-20260801-exception-userptr-access](./index.md)
**事务日志：** [2026-08-01-exception-userptr-access](../../devlog/transactions/2026-08-01-exception-userptr-access.md)；
R0实现和验收早于公共RFC，历史代码由`4ed73b76`、`70f422a0`、`97e02808`保存。

本文只跟踪仍会影响future user-access/MM修改顺序、review gate或验收解释的问题。R0 exception window、copy
和partial语义已经由用户验收；Active条目不能被解释为R0未实现，也不能被R0 closure误写成已经neutralize。

## Apollyon

当前无RFC-local Apollyon。

## Keter

### UACCESS-KETER-001 - Remote fence 仍在 UserSpace mutex 内完成

**状态：** Active / inherited；按用户决定不在R0处理。

**历史：** `84227f55`（2026-05-11）引入`RemoteUspFenceGuard`时，源码已经说明它用于避免持有
`UserSpace` mutex发送同步IPI；同一提交也在旧user validator留下`TODO: tlb shootdown, out of mutex lock.`。
旧`validate_user_range()`在持锁状态下逐页调用`inject_page_fault()`并让guard析构，因此问题早于
`4ed73b76`的exception-based validator。

**当前路径：**

1. caller通过`UserSpaceHandle::lock()`/`with_usp()`持有address-space mutex；
2. architecture accessor发生page fault；
3. `UserSpace::handle_page_fault()`更新PTE，`VmArea`完成local TLB invalidation；
4. accessor立即`drop(fence)`；
5. `RemoteUspFenceGuard::drop()`同步`broadcast_ipi(TlbShootdown)`，此时mutex仍未释放；
6. IPI完成后才retry当前chunk。

显式`fault_in_user_range()`和少量non-active address-space probe也有同类guard lifetime，需要在follow-up中
一起审计，不能只改RV64/LA64两处ordinary copy。

**风险：** synchronous IPI把remote CPU completion依赖嵌入sleeping mutex临界区，扩大锁序和tail latency，
并可能在future remote CPU/MM路径交错后形成deadlock。guard的`Drop`只能记录IPI error，不能把failure返回给
当前copy transaction；这也需要由follow-up明确fail-closed或infallible policy。

**为什么不简单延迟到copy结束：** page fault可能执行COW或替换mapping。若当前CPU在remote CPU仍持旧
writable TLB时先retry/commit，新旧mapping可能并发可见，破坏COW隔离。remote invalidation必须在需要它的
retry之前完成，而不是在syscall末尾best-effort补做。

**目标handoff：**

```text
lock UserSpace
  -> resolve PTE
  -> local TLB invalidation
unlock UserSpace
  -> complete required synchronous remote shootdown
relock UserSpace
  -> revalidate/retry remaining bytes once
```

owner应是`UserSpaceHandle`或同等窄的locked-access transaction，而不是让拿到`&mut UserSpace`的architecture
accessor尝试解锁未知mutex。future方案可以引入可释放/重获的user-space access guard或outer retry step，但
不得暴露raw mutex操作、改成async shootdown、在锁内保留同步IPI或把fence推迟到完整copy之后。

**Exit Condition：**

- ordinary exception copy、explicit `fault_in()`和non-active MM probe全部完成lock/fence lifetime审计；
- PTE update/local invalidate发生在mutex内，必要remote shootdown在mutex外同步完成；
- retry只在shootdown成功并重新获得mapping owner后发生；
- IPI allocation/offline failure有明确、可返回或可证明infallible的policy；
- COW/remap、lazy fault、跨页copy和SMP并发测试证明没有stale writable mapping、deadlock或额外retry；
- source TODO删除，并通过独立MM/user-access review neutralize本条。

**R0关系：** 用户已经接受exception-backed validator R0，并明确本问题暂不处理。该决定只允许把问题作为
既有MM依赖留给follow-up，不允许文档声称当前锁序正确或问题已经修复。

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

当前无项。
