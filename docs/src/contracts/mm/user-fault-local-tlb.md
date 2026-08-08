# User Fault Local TLB Completion 当前契约

**Contract ID：** `MM-TLB-LOCAL`
**状态：** Active
**Owner：** MM user-space fault-resolution policy；Mapper拥有leaf PTE commit fact，UserSpace resolver拥有local completion decision
**参与领域：** MM paging / VMA与VMO / RV64与LA64 user trap / kernel userptr / futex与explicit fault-in / architecture TLB primitive
**覆盖范围：** operation-local leaf PTE commit relation、access continuation分类与current-core local TLB completion policy
**不覆盖：** in-flight remote predecessor下的mapping-identity强一致性、remote shootdown、IPI failure与CPU hotplug、retired-frame cleanup、COW/VMO resolve语义、kernel page table、ASID/PCID、TLB batching或性能保证
**实现位置：** `anemone-kernel/src/mm/{paging/mapper.rs,uspace/}`、`anemone-kernel/src/arch/{riscv64,loongarch64}/exception/`
**依赖：** `USER-ENTRY-001/002`
**Pending Successor：** None
**最后核验：** 2026-08-08

## 状态与能力所有权

| 事实 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| leaf PTE与同一次commit的old/new relation | Mapper | VMA取得operation-local relation | 区分`Added`、unchanged、permission relaxation及replacement/restriction |
| access continuation | fault caller选择，UserSpace resolver解释 | `UserReturn`或`Immediate`窄值 | 说明resolver返回后由user hardware retry，还是kernel立即访问/retry |
| current-core local completion decision | MM UserSpace fault resolver | relation与continuation的单次输入 | 决定本轮是否执行local TLB invalidation |
| architecture local invalidation primitive | RV64 / LA64 paging architecture | MM只调用通用能力 | 执行具体current-core TLB instruction |
| remote completion与retired frame | MM remote-fence protocol | local policy不取得其内部状态 | 保持既有remote guard、IPI failure与cleanup边界 |

PTE始终是page-table truth；commit relation不保存为address-space字段、pending bit、epoch或cache，也不由diagnostic/
performance counter驱动。`Added`是operation-local fact，不是“所有CPU均不存在旧translation”的证明。

## MM-TLB-LOCAL-001 — Actual user fault按operation-local Added延迟local completion

**规则：** Mapper必须在唯一leaf mutation point、同一次page-table walk内推导operation-local commit relation；caller
不得为分类再次walk或建立第二份PTE truth。本次walk没有替换valid ancestor且目标leaf在commit前invalid时为`Added`；
同frame同flags为`Unchanged`；只增加permission为`Relaxed`；frame replacement、permission restriction、leaf kind变化
或valid ancestor replacement均为`ReplacedOrRestricted`。

ordinary RV64/LA64 user-mode fault使用`UserReturn` continuation。`UserReturn + Added`延迟本轮current-core local
invalidation；hardware若保留invalid或更restrictive translation、已安装leaf保持且原access再次fault，本次commit不再
是`Added`，resolver必须在再次返回user mode前完成local invalidation。`UserReturn`的其它relation均立即完成；若
intervening destructive mutation让同一access再次得到`Added`，仍按current limitation处理，不属于non-`Added` refault
guarantee。

kernel userptr recovery、futex、explicit fault-in与non-active address-space probe使用`Immediate` continuation；任一
relation都必须在resolver返回、kernel access或retry前完成local invalidation。policy只依赖commit relation与
continuation，不得按QEMU、machine或architecture选择。existing remote fence guard、IPI failure policy与retired-frame
cleanup保持独立，local policy不得删除、延迟或接管remote obligation。

该规则明确受
[`ANE-20260808-MM-LAZY-LOCAL-TLB-REMOTE-PREDECESSOR-WINDOW`](../../register/current-limitations.md#ane-20260808-mm-lazy-local-tlb-remote-predecessor-window)
约束：如果另一个CPU的destructive mutation已经让page table呈现invalid、但remote guard尚未完成，当前CPU仍可能持有
predecessor translation；Signal arbitration还可能在原access refault前重定向到handler。本契约不承诺该窗口内的
mapping-identity强一致性，也不把operation-local `Added`描述为全局fresh mapping。

**违反表现：** 为分类执行第二次walk或缓存relation；把同一次commit中替换的valid leaf/ancestor分类为`Added`；
`Unchanged` refault、Immediate或其它present/destructive relation跳过local completion；按platform/test分叉policy；
或借local classification改变remote guard、failure与frame-retirement顺序。accepted predecessor窗口本身不是对本规则的
违反，只能由current limitation定义和约束。

**验证 / Enforcement：** Mapper owner-local KUnit覆盖additive、unchanged、permission relaxation、restriction、frame
replacement与valid-ancestor replacement；resolver policy KUnit覆盖唯一lazy组合和全部eager组合；RV64/LA64 ordinary
trap、四个architecture userptr recovery点及全部`fault_in_page()` consumer source audit；两个architecture单HART
588/588 KUnit boot与六组focused `userptr` runtime。SMP predecessor forced interleaving、实体硬件、性能、full LTP与
final harness不属于当前证明。

**最初来源：** [User fault local TLB completion小迭代](../../devlog/changes/2026-08-08-user-fault-local-tlb.md)。

**当前来源：** [User fault local TLB completion小迭代](../../devlog/changes/2026-08-08-user-fault-local-tlb.md)；
2026-08-08 R1 CKPT 2 closure提交。
