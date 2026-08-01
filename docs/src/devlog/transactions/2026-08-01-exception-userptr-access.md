# 2026-08-01 - Exception-backed User Pointer Access

**Status:** Completed
**Owners:** EDGW, Codex
**Area:** syscall / user memory / exception / MM / RISC-V64 / LoongArch64
**Canonical Plan:** [RFC-20260801-exception-userptr-access](../../rfcs/exception-userptr-access/index.md)
**RFC Revision:** R0
**Contract Impact:** None
**Current Phase:** Closed

## Scope

本事务回顾性记录已经完成并由用户验收的exception-backed userptr迁移。R0用RV64/LA64 architecture-owned
bytewise assembly、per-CPU exact-PC recovery window和一次page-fault retry替代旧whole-range MM预检查后
直接解引用raw user pointer的路径；generic typed API统一为fallible copy，VFS cursor继续拥有ordinary I/O
partial progress。

本事务不重演历史代码修改，不引入Linux全局exception table、page pin、zero-copy、word/vector优化或新的
MM shootdown协议。`RemoteUspFenceGuard`锁内同步IPI问题早于R0，并按用户决定留给follow-up。

## Baseline

- 旧`UserReadPtr`/`UserWritePtr`构造时逐页`inject_page_fault()`，随后通过raw pointer/reference访问user bytes。
- typed pointer要求natural alignment，不能自然覆盖packed/unaligned syscall ABI。
- expected user access fault缺少只绑定具体copy instruction的kernel fixup identity。
- ordinary stream I/O需要partial prefix，scalar/record和external-side-effect transaction需要exact/pre-fault，
  不能由一套whole-range eager validator替代。
- RV64与LA64 kernel trap encoding不同，但需要共享相同的generic copy/fault contract。

## Phase Log

### 2026-07-29 - Architecture recovery core

**Change:** `4ed73b76`增加`UserPtrAccessorArch`和`UserPtrAccessError`，实现RV64/LA64 bytewise read/write
assembly、fault/fixup labels、per-CPU `UserPtrValidation`和`UserAccessWindow`。kernel synchronous trap入口只在
exact fault PC命中时记录fault并rewrite trap PC；hardware interrupt入口断言slot未发布。

**Invariant:** IRQ disable先于record publication，slot withdrawal先于IRQ restore；read只恢复user load，write
只恢复user store；其它kernel fault继续fail closed。

### 2026-07-29 - Generic typed API与fault/retry

**Change:** `syscall/user_access.rs`删除raw pointer callback和typed alignment gate，scalar/slice/string access
统一返回`Result`。copy按page boundary切分；access fault立即`EFAULT`，page fault退出window后由`UserSpace`
resolve并只retry一次；error携带已完成prefix。

**Invariant:** architecture只报告fault/progress事实；typed exact failure和VFS partial success分别由调用owner
决定。`try_new()`只表达checked range，需要完整pre-fault的transaction显式调用`fault_in()`。

### 2026-07-29 - Special route closure

**Change:** `70f422a0`补齐read/getdents/clone/exit/futex等路径。VFS user-buffer cursor按page/iovec聚合partial；
getdents和exact transaction在external side effect前pre-fault；non-active clone child使用显式MM probe，真正
child store仍走exception accessor；exit/robust-list cleanup不再因坏用户指针panic。

**Invariant:** ordinary backend不获得raw user memory capability；non-active address space不伪装成hardware
accessor；cleanup user fault不能升级为kernel fault。

### 2026-07-29 - Runtime oracle

**Change:** `97e02808`增加`anemone-apps/userptr`，覆盖bad address、lazy/cross-page、partial second-page fault、
permission/unmap和side-effect-before-EFAULT场景。

**Boundary:** `anemone-apps/user-test`中的启动项当前仍被注释，因此该app是手工focused oracle，不是默认持续
回归入口。

### 2026-08-01 - RFC promotion与用户验收记录

**Validation:**

- RV64/LA64各自保留`validation_matches_only_exact_user_access_instruction` KUnit；RV64额外拒绝错误access
  direction。
- source assertions覆盖nested window、nested fault、hwirq entry、full-success progress和cleanup ordering。
- runtime app包含五组行为case；默认runner状态明确记录为manual。
- 用户确认exception-backed validator早已验收通过。agent没有原始验收日志，因此不补写未提供的架构、
  case count、时序或console输出。

**Closure:** R0与本事务Completed/Closed；没有current contract cutover。

## Implementation Feedback

2026-08-01 source/history audit确认`RemoteUspFenceGuard`及`out of mutex lock` TODO来自`84227f55`
（2026-05-11），旧validator当时已经在`UserSpace` mutex内析构guard。`4ed73b76`继承并扩展了触发路径，但
不是问题起源。

用户决定本轮不修复。future route必须在PTE update/local invalidate后释放mutex，再同步完成必要remote
shootdown，重新加锁后才retry；不能简单async化或延迟到整次copy结束。该风险保留在
[Tracking Issues](../../rfcs/exception-userptr-access/tracking-issues.md)。

## Final Invariants

- architecture accessor唯一拥有可恢复raw user load/store和trap fixup identity。
- 每CPU最多一个IRQ-off recovery window；exact PC不命中时不接管kernel fault。
- 一个window最多一页，bytewise progress是已经对destination可见的连续prefix。
- access fault立即失败；page fault最多resolve/retry一次。
- typed exact、VFS partial和explicit pre-fault由各自调用owner表达。
- hardware accessor只操作current active address space。

完整规则见[目标与不变量](../../rfcs/exception-userptr-access/invariants.md)。

## Remaining Boundaries

- remote fence锁边界与IPI failure policy未修复。
- runtime userptr app未接入默认runner。
- copy loop未做word/vector性能优化。
- 不提供page pin、zero-copy、content snapshot或全syscall side-effect rollback证明。
- software unaligned access模拟器的独立内存破坏问题不属于本事务closure。

## Links

- [RFC](../../rfcs/exception-userptr-access/index.md)
- [Implementation map](../../rfcs/exception-userptr-access/implementation.md)
- [Tracking Issues](../../rfcs/exception-userptr-access/tracking-issues.md)
- [Biweekly devlog](../2026-07-20_to_2026-08-02.md)
