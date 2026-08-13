# User Address-Space TLB Residency 与 Completion 当前契约

**Contract ID：** `MM-TLB-LOCAL` / `MM-TLB-REMOTE`
**状态：** Active
**Owner：** MM user address-space activation/completion policy；Mapper拥有leaf PTE commit fact，`UserSpaceHandle`拥有destructive completion ordering与唯一residency truth，IPI transport拥有delivery与acknowledgement
**参与领域：** MM paging / VMA与VMO / scheduler mapping switch / exec / procfs temporary activation / RV64与LA64 paging / user trap / kernel userptr / futex与explicit fault-in / user-TLB IPI transport
**覆盖范围：** operation-local leaf PTE commit relation、access continuation分类、user mapping activation/residency、current-core local completion、destructive remote target与ack ordering、retirement lifetime
**不覆盖：** runtime CPU hotplug、ASID/PCID、kernel page table shootdown、TLB batching或数值性能保证、COW/VMO自身resolve语义
**实现位置：** `anemone-kernel/src/mm/{paging/mapper.rs,uspace/}`、`anemone-kernel/src/{sched/switch.rs,task/api/execve/kernel.rs,fs/proc/tgid/}`、`anemone-kernel/src/exception/ipi/user_tlb.rs`、`anemone-kernel/src/arch/{riscv64,loongarch64}/mm/`
**依赖：** `USER-ENTRY-001/002`
**Pending Successor：** None
**最后核验：** 2026-08-11

## 状态与能力所有权

| 事实 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| leaf PTE与同一次commit的old/new relation | Mapper | VMA取得operation-local relation | 区分`Added`、`Unchanged`、`Relaxed`及replacement/restriction |
| access continuation | fault caller选择，UserSpace resolver解释 | `UserReturn`或`Immediate`窄值 | 说明resolver返回后由user hardware retry，还是kernel立即访问/retry |
| current-core local completion decision | MM UserSpace fault resolver | relation与continuation的单次输入 | 决定本轮是否执行local TLB invalidation |
| address-space destructive completion ordering | `UserSpaceHandle::completion_ordering` | `UserSpaceGuard`持有线性访问能力 | 串行化destructive commit、remote ack、dependent continuation与retirement |
| user address-space TLB residency | `UserSpaceHandle::tlb_residency` | scheduler、exec与temporary caller只提交old/new handle | 以activation/full local destruction与同一锁内snapshot决定remote target |
| user-TLB message、target delivery与ack | `exception::ipi::user_tlb` | MM提交resident target snapshot并持有prepared/committed completion capability | 对已提交remote target执行同步shootdown与ack |
| retired page table、frame与backing | MM destructive transaction | IPI不读取retirement内容 | 保持旧translation可达资源活到remote ack之后 |
| architecture invalidation primitive | RV64 / LA64 paging architecture | MM与IPI只调用通用能力 | 执行current-core或target-core TLB instruction |

PTE始终是page-table truth；commit relation不保存为address-space字段、pending bit、epoch或cache，也不由diagnostic/
performance counter驱动。`completion_ordering`只表达同一address space的串行能力，不缓存PTE、VMA或generation；
`tlb_residency`是CPU target的唯一行为truth，不从scheduler task state或architecture-private root另建cache。IPI phase只
决定transport lifecycle，不反向决定MM mutation分类或residency。

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
`UserSpaceGuard` continuation路径，并核对同一ordering owner覆盖mutex unlock、remote ack与relock。owner-local
deterministic KUnit覆盖relation/local policy；dependent-continuation ordering由上述owner/caller/happens-before源码审查
enforce。2026-08-09 RV64/LA64 SMP=8 release QEMU中曾有五条forced-interleaving ordering KUnit与六组`userptr`通过；
这些历史运行事实保持，但对应KUnit因依赖production pause hook和固定yield次数已在2026-08-11删除，不再是当前回归
机制或并发证明。见[KUnit execution and proof小迭代](../../devlog/changes/2026-08-11-kunit-execution-proof.md)。

**最初来源：** [User fault local TLB completion小迭代](../../devlog/changes/2026-08-08-user-fault-local-tlb.md)。

**当前来源：** [User TLB Completion RFC R2 closure](../../rfcs/user-tlb-completion/index.md#closure)，
`USER-TLB-COMPLETION-CUTOVER`（2026-08-09）。

## MM-TLB-REMOTE-001 — Destructive user mapping在dependent continuation与retirement前完成remote ack

**规则：** replacement、permission restriction、unmap、discard/decommit、COW/fork write restriction及其它无法在
唯一commit point证明为monotonic的user mapping mutation，必须在`UserSpace` mutex内完成mutation和current-core
local invalidation，然后通过该address space唯一residency owner提交仍可能观察旧translation的remote CPU target。
随后释放`UserSpace` mutex，在仍持有address-space `completion_ordering`时同步完成这些target的remote invalidation。
只有ack全部完成后，才允许释放retired page table/frame/backing、重新取得`UserSpace` mutex并返回dependent kernel
retry、user/Signal continuation、syscall success或fork child publication。

```text
lock/mutate UserSpace
  -> current-core local completion
  -> residency-covered target snapshot / transport preparation
  -> unlock UserSpace
  -> remote acknowledgement
  -> release retirement
  -> relock/return dependent continuation
```

activation在residency锁内安装user page-table root并完成本核full TLB invalidation，随后发布当前CPU的join；切换到
其它user/kernel root并完成同等本核destruction后，才从旧address space leave。destructive commit后的target snapshot
使用同一锁：join在线性化snapshot前完成则进入本轮target，join在线性化snapshot后完成则其local destruction发生在
commit之后；snapshot已提交的target不能因随后leave被无证明撤销。same-mapping switch保持现有residency，temporary
activation由preemption-pinned scoped capability恢复原mapping。

当前实现继续在address space发布前构造固定的`UserTlbShootdownSet`，并用预先拥有的per-CPU message承载resident
snapshot；allocation-free是当前实现形状，不再是本contract的普遍要求。任何未来适度有界allocation仍不得在
post-mutation路径返回recoverable failure，也不得把completion藏在Drop中。当前内核不支持runtime CPU hotplug：已经
进入target的CPU在ack前变为offline属于correctness invariant violation，不能作为`TargetOffline`成功返回。

`UserTlbRetirement`是destructive change到ack之间的唯一cleanup owner。完全移除的VMA、detached page-table frame和
被backing撤销的resident frame必须移入该retirement；surviving/successor VMA继续持有的backing无需复制第二份lifetime
状态。permission relaxation可以清leaf以保留private/COW refault语义，但不得在没有remote obligation时detach并释放
空page-table frame。

**违反表现：** user mapping caller绕过唯一activation/residency handoff；task state、architecture root或diagnostic mask
形成第二份target truth；join与snapshot竞态漏掉旧translation；未完成local destruction就leave；持有`UserSpace`或
residency锁发送/等待同步IPI；destructive commit后向caller返回allocation/offline failure或继续user execution；ack前
释放/复用retired资源；child在parent COW restriction ack前发布；或caller绕过唯一completion-ordering owner。

**验证 / Enforcement：** bounded source review从全部`activate_addr_space()`与user activation facade caller闭合scheduler、
exec、temporary restore、same-mapping、kernel/user handoff、唯一residency truth、join/leave/snapshot、锁序、boot-fixed target、
fork/SysV/Anon/Shadow/page-table retirement及`mutation -> snapshot -> unlock -> ack -> release/retry`顺序。owner-local/transport
KUnit覆盖稳定resident set、leave收敛、selected remote target与source-only零remote target。fixed replace、permission
restriction、fork COW、heap decommit及monotonic/no-change的dependent ordering当前由production-path source review enforce；
2026-08-09双架构SMP=8历史runtime曾执行对应forced-interleaving cases与六组`userptr`，但这些cases已在2026-08-11因
违反`KUNIT-CONCURRENCY-001`和`KUNIT-SHAPE-001`删除，不再计入current validation inventory。运行证据从不替代
owner/caller/happens-before源码审查。

**最初来源：** [User TLB Completion RFC R2 closure](../../rfcs/user-tlb-completion/index.md#closure)，
`USER-TLB-COMPLETION-CUTOVER`（2026-08-09）。

**当前来源：** [User TLB Residency Targeting RFC R1 closure](../../rfcs/user-tlb-residency-targeting/index.md#closure)，
`USER-TLB-RESIDENCY-CUTOVER`（2026-08-09）。
