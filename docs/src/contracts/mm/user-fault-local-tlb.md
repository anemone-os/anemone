# User Address-Space TLB Completion 当前契约

**Contract ID：** `MM-TLB-LOCAL` / `MM-TLB-REMOTE`
**状态：** Active
**Owner：** MM user address-space completion policy；Mapper拥有leaf PTE commit fact，`UserSpaceHandle`拥有destructive completion ordering，IPI transport拥有delivery与acknowledgement
**参与领域：** MM paging / VMA与VMO / RV64与LA64 user trap / kernel userptr / futex与explicit fault-in / user-TLB IPI transport / architecture TLB primitive
**覆盖范围：** operation-local leaf PTE commit relation、access continuation分类、current-core local completion、destructive remote completion ordering与retirement lifetime
**不覆盖：** runtime CPU hotplug、address-space residency/active mask、ASID/PCID、kernel page table、TLB batching或数值性能保证、COW/VMO自身resolve语义
**实现位置：** `anemone-kernel/src/mm/{paging/mapper.rs,uspace/}`、`anemone-kernel/src/exception/ipi/user_tlb.rs`、`anemone-kernel/src/arch/{riscv64,loongarch64}/exception/`
**依赖：** `USER-ENTRY-001/002`
**Pending Successor：** None
**最后核验：** 2026-08-09

## 状态与能力所有权

| 事实 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| leaf PTE与同一次commit的old/new relation | Mapper | VMA取得operation-local relation | 区分`Added`、`Unchanged`、`Relaxed`及replacement/restriction |
| access continuation | fault caller选择，UserSpace resolver解释 | `UserReturn`或`Immediate`窄值 | 说明resolver返回后由user hardware retry，还是kernel立即访问/retry |
| current-core local completion decision | MM UserSpace fault resolver | relation与continuation的单次输入 | 决定本轮是否执行local TLB invalidation |
| address-space destructive completion ordering | `UserSpaceHandle::completion_ordering` | `UserSpaceGuard`持有线性访问能力 | 串行化destructive commit、remote ack、dependent continuation与retirement |
| user-TLB message、target delivery与ack | `exception::ipi::user_tlb` | MM持有prepared/committed completion capability | 对boot-fixed online target执行allocation-free同步shootdown |
| retired page table、frame与backing | MM destructive transaction | IPI不读取retirement内容 | 保持旧translation可达资源活到remote ack之后 |
| architecture invalidation primitive | RV64 / LA64 paging architecture | MM与IPI只调用通用能力 | 执行current-core或target-core TLB instruction |

PTE始终是page-table truth；commit relation不保存为address-space字段、pending bit、epoch或cache，也不由diagnostic/
performance counter驱动。`completion_ordering`只表达同一address space的串行能力，不缓存PTE、VMA、CPU residency或
generation。IPI phase只决定transport lifecycle，不反向决定MM mutation分类。

## MM-TLB-LOCAL-001 — Actual user fault按operation-local Added延迟local completion

**规则：** Mapper必须在唯一leaf mutation point、同一次page-table walk内推导operation-local commit relation；caller
不得为分类再次walk或建立第二份PTE truth。本次walk没有替换valid ancestor且目标leaf在commit前invalid时为`Added`；
同frame同flags为`Unchanged`；只增加permission为`Relaxed`；frame replacement、permission restriction、leaf kind变化
或valid ancestor replacement均为`ReplacedOrRestricted`。

ordinary RV64/LA64 user-mode fault使用`UserReturn` continuation。`UserReturn + Added`延迟本轮current-core local
invalidation；hardware若保留invalid或更restrictive translation、已安装leaf保持且原access再次fault，本次commit不再
是`Added`，resolver必须在再次返回user mode前完成local invalidation。`UserReturn`的其它relation均立即完成。

kernel userptr recovery、futex、explicit fault-in与non-active address-space probe使用`Immediate` continuation；任一
relation都必须在resolver返回、kernel access或retry前完成local invalidation。policy只依赖commit relation与
continuation，不得按QEMU、machine或architecture选择。

`Added`、`Unchanged`和`Relaxed`不创建自己的remote round，但这不允许它们越过较早的destructive completion。
所有published address-space mutation与continuation先取得同一个`completion_ordering`能力；因此successor只有在
predecessor remote ack和retirement release之后才能进入resolver、kernel retry、user-entry arbitration或syscall
success路径。operation-local `Added`不再承担、也不需要伪装成全局fresh-mapping证明。

**违反表现：** 为分类执行第二次walk或缓存relation；把同一次commit中替换的valid leaf/ancestor分类为`Added`；
`Unchanged` refault、`Immediate`或其它present/destructive relation跳过local completion；按platform/test分叉policy；
或任何production caller绕过address-space ordering，使dependent continuation越过未完成的destructive predecessor。

**验证 / Enforcement：** 源码审查闭合Mapper classification、published `UserSpaceHandle` mutation caller和
`UserSpaceGuard` continuation路径，并核对同一ordering owner覆盖mutex unlock、remote ack与relock。owner-local KUnit
覆盖relation/local policy及五条代表性ordering路径；2026-08-09 RV64/LA64 SMP=8 release QEMU中597项KUnit与六组
`userptr`均通过。运行结果只说明这些路径未回归，不替代owner/caller/happens-before源码审查，也不穷举所有并发交错。

**最初来源：** [User fault local TLB completion小迭代](../../devlog/changes/2026-08-08-user-fault-local-tlb.md)。

**当前来源：** [User TLB Completion RFC R2 closure](../../rfcs/user-tlb-completion/index.md#closure)，
`USER-TLB-COMPLETION-CUTOVER`（2026-08-09）。

## MM-TLB-REMOTE-001 — Destructive user mapping在dependent continuation与retirement前完成remote ack

**规则：** replacement、permission restriction、unmap、discard/decommit、COW/fork write restriction及其它无法在
唯一commit point证明为monotonic的user mapping mutation，必须在`UserSpace` mutex内完成mutation和current-core
local invalidation，然后释放该mutex，在仍持有address-space `completion_ordering`时同步完成其它boot-online CPU的
remote invalidation。只有ack全部完成后，才允许释放retired page table/frame/backing、重新取得`UserSpace` mutex并
返回dependent kernel retry、user/Signal continuation、syscall success或fork child publication。

```text
prepare transport
  -> lock/mutate UserSpace
  -> current-core local completion
  -> unlock UserSpace
  -> remote acknowledgement
  -> release retirement
  -> relock/return dependent continuation
```

每个address space在发布前构造固定的`UserTlbShootdownSet`。round preparation只snapshot当前boot-fixed online set，
commit后enqueue预先拥有的per-CPU message并等待ack；post-mutation路径不分配、不返回recoverable failure，也不把
completion藏在Drop中。当前内核不支持runtime CPU hotplug：已经进入target的CPU在ack前变为offline属于correctness
invariant violation，不能作为`TargetOffline`成功返回。普通owner-local collection增长继续遵循内核的allocation-failure
panic policy，不为避免该policy按容量上限预分配。

`UserTlbRetirement`是destructive change到ack之间的唯一cleanup owner。完全移除的VMA、detached page-table frame和
被backing撤销的resident frame必须移入该retirement；surviving/successor VMA继续持有的backing无需复制第二份lifetime
状态。permission relaxation可以清leaf以保留private/COW refault语义，但不得在没有remote obligation时detach并释放
空page-table frame。

**违反表现：** 持有`UserSpace` mutex发送同步IPI；destructive commit后向caller返回allocation/offline failure或继续
user execution；ack前释放/复用retired资源；child在parent COW restriction ack前发布；caller绕过唯一ordering owner；
或让IPI transport phase、diagnostic字段、PTE snapshot或CPU residency cache成为第二份MM truth。

**验证 / Enforcement：** 独立bounded source review核对production caller闭包、唯一owner、transport phase、boot-fixed
target invariant、fork/SysV/Anon/Shadow/page-table retirement及`mutation -> unlock -> ack -> release/retry`顺序，未发现
剩余production-reachable Apollyon/Keter/Euclid。IPI KUnit覆盖online snapshot和构造失败不发布transport；ordering KUnit
覆盖fixed replace、permission restriction、fork COW、heap decommit及monotonic/no-change路径；双架构SMP=8 runtime和
`userptr`作为集成/回归证据。KUnit、forced interleaving与runtime都不被解释为对所有调度交错“不存在错误”的证明。

**最初来源：** [User TLB Completion RFC R2 closure](../../rfcs/user-tlb-completion/index.md#closure)，
`USER-TLB-COMPLETION-CUTOVER`（2026-08-09）。

**当前来源：** 同上。
