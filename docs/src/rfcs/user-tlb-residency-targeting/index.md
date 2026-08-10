# RFC-20260809-user-tlb-residency-targeting

**状态：** Closed
**修订：** R1
**负责人：** doruche, Codex
**最后更新：** 2026-08-09
**领域：** MM / scheduler / user address-space activation / TLB shootdown / SMP
**影响契约：** Refine
[`MM-TLB-REMOTE-001`](../../contracts/mm/user-fault-local-tlb.md#mm-tlb-remote-001--destructive-user-mapping在dependent-continuation与retirement前完成remote-ack)
**执行记录：** 本次提交

## 摘要

当前destructive user mapping mutation在本核完成local invalidation后，向全部其它boot-online CPU同步发送TLB
shootdown并等待ack。这个target在没有address-space residency truth时是正确的保守边界，但会中断并flush从未安装、
或已经安全切离该页表的CPU；CPU数量增加时，每轮remote completion仍按全部online CPU扩张。

本RFC为user address space定义一个窄的activation/residency协议。只有仍可能通过当前硬件mapping观察旧translation的
CPU才承担remote completion obligation；已经完成切离证明的CPU不再是稳定target。scheduler是主要activation参与者，
exec image replacement与procfs temporary activation等直接切换user page table的路径也必须服从同一协议。现有
destructive completion ordering、IPI delivery/ack、retirement与dependent continuation边界保持不变。

RFC只固定target coverage、owner/handoff、failure/cleanup与验收，不固定mask或per-CPU表示、锁或atomic方案、具体Rust
类型、activation API形状、allocation/preallocation策略或短暂保守多发的内部处理。实现可以在不漏失效、不制造第二份
truth且不改变受保护contract的前提下适度松弛和适配。

## 背景

当前[`MM-TLB-REMOTE-001`](../../contracts/mm/user-fault-local-tlb.md#mm-tlb-remote-001--destructive-user-mapping在dependent-continuation与retirement前完成remote-ack)
要求destructive mutation在dependent continuation与retirement前同步完成其它boot-online CPU的remote invalidation。
此前的[User TLB Completion RFC](../user-tlb-completion/index.md)已经关闭predecessor/successor窗口，使transport在
`UserSpace` mutex外完成ack，并把retired page table/frame/backing保持到ack之后；本RFC不重新设计这些边界。

RV64与LA64当前在安装不同address space时都完成本核full TLB invalidation。因此，一个CPU如果已经切离某user page
table并完成对应local destruction，就不再需要接收该address space之后的shootdown。反之，只观察scheduler current
task不足以描述硬件mapping：exec会原地安装新image，procfs读取其它进程的cmdline/environ时还会临时切换目标page
table后恢复。residency closure必须覆盖全部真实user activation caller，而不是在scheduler旁建立一个不完整cache。

本轮性能目标是结构性的target缩减，不设置wall-clock、吞吐或评分门槛。稳定resident set是online set的子集；数值收益
由后续用户测量决定，不是本RFC设计或cutover的前置条件。

## 目标

- 为每个published user address space建立唯一的TLB residency truth，使remote target来自仍可能观察旧translation的
  CPU，而不是无条件来自全部boot-online CPU。
- 为user page-table activation、切离与destructive target snapshot建立同一协议，使并发join/leave既不会漏掉必须
  完成的invalidation，也不会要求永久保留已经安全切离的CPU。
- 让scheduler、exec与temporary activation caller只持有窄activation/residency能力，不读取或改写MM、IPI或其它
  caller的私有状态。
- 保持现有destructive mutation、current-core local completion、锁外remote ack、retirement与dependent
  continuation顺序。
- 允许实现选择清晰、自然、可审查的内部表示和适度分配策略，不把allocation-free、精确瞬时mask或某种具体同步原语
  提升为target guarantee。

## 非目标

- 不引入ASID/PCID、ASID generation、lazy remote generation、TLB epoch、batching或deferred user-entry flush。
- 不支持runtime CPU hotplug，也不重定义CPU online/offline lifecycle。
- 不改变kernel page table shootdown、generic IPI consumer、architecture TLB primitive或全局membarrier协议。
- 不改变PTE/VMA/VMO/COW truth owner、page-fault commit分类、syscall ABI/errno、Signal/user-entry语义或partial progress。
- 不承诺精确消除每一个过渡期多余IPI；有证明依据的保守superset与短暂多发可以接受，但不得成为静默、永久的
  all-online fallback。
- 不冻结mask布局、per-CPU或per-address-space存储、锁/atomic、guard/callback、函数签名、模块布局或
  allocation/preallocation选择。
- 不要求随机并发stress、调度交错穷举、数值performance A/B、完整LTP/final harness或实体硬件作为cutover证明。

## Owner 与协议边界

- **PTE/VMA truth owner：** `UserSpace`与Mapper继续唯一拥有mapping topology、leaf commit及其分类。residency不得缓存
  PTE、VMA、commit relation或“可能需要flush”的mapping诊断bit。
- **Residency owner：** MM user address-space activation/residency domain唯一拥有“哪些CPU仍可能通过当前硬件mapping
  观察该address space translation”的协议状态。它是TLB target行为真相，不是日志、统计或从scheduler task state
  松散推导的cache。
- **Activation参与者：** scheduler mapping switch、exec image replacement与temporary user-space activation通过窄能力
  推进join/leave；参与者不直接写residency表示，也不决定destructive policy。
- **Architecture owner：** RV64/LA64 activation继续拥有安装page-table root与完成本核TLB destruction的架构义务。
  residency协议只消费“activation/local destruction已经完成”的能力，不读取architecture-private CSR表示。
- **Completion owner：** `UserSpaceHandle`现有completion ordering继续串行化destructive predecessor、dependent
  continuation与retirement。residency只决定target coverage，不反向决定PTE mutation或completion是否destructive。
- **Transport owner：** user-TLB IPI transport继续拥有message delivery与ack。target一旦提交给transport，只有remote
  ack或协议明确承认的等价local TLB-destruction proof才能解除该CPU的本轮obligation。
- **Cleanup owner：** MM destructive transaction继续持有retired page table/frame/backing；完整target completion之前
  不得释放或复用。temporary activation还必须保证目标与原address space在切换及恢复期间保持live。

## Target Invariants

### Residency语义

一个CPU属于某user address space的resident set，当且仅当协议尚不能证明该CPU在后续执行或user return前已经无法再
使用该address space的旧translation。CPU正在该进程的syscall/interrupt内执行时，仍可能携带相同hardware mapping，
不能只因暂时不在user mode就退出residency。

稳定状态下，destructive remote target应等于resident online CPU扣除由本轮current-core local completion覆盖的source
CPU。实现可以在activation切换、snapshot或transport handoff的短暂窗口保守包含额外CPU；只要不漏target、不会把
diagnostic数据变成行为truth，并且完成切离后能够收敛到稳定resident set即可。

### Join、leave与snapshot

destructive mapping commit与target snapshot之间必须建立足够的同步和内存顺序，使每个并发activation满足以下二分：

- join在线性化target snapshot之前完成时，该CPU必须进入本轮target或持有等价completion obligation；
- join在线性化target snapshot之后完成时，activation必须在允许该CPU使用mapping或返回user mode前，观察本次commit
  并完成足以排除旧translation的local completion。

leave只有在协议已经证明CPU不能再使用旧translation后才能完成。leave先于snapshot完成时，CPU可以不进入target；
snapshot先选中CPU时，后续leave不得无证明地撤销obligation。实现可以保守等待原IPI ack，也可以消费与本轮有明确
happens-before的local destruction proof；RFC不规定采用哪一种。

同一address-space之间的task切换、user/kernel mapping切换、exec no-return handoff与temporary activation/restore都必须
保持上述语义。实现不必把它们编码成相同函数或状态机，但不能存在绕过唯一residency truth的直接activation路径。

### Completion与lifetime

本RFC只缩小remote target，不削弱现有顺序：

```text
destructive PTE/VMA commit
    -> current-core local completion
    -> residency-covered remote completion
    -> dependent continuation / exposure

residency-covered remote completion
    -> retired backing release or reuse
```

target snapshot的具体位置、是否和mutation guard连续持锁、是否复制snapshot以及transport何时准备内部storage属于实现
选择；但不能持有会被remote handler或activation路径反向需要的锁等待同步ack，也不能让snapshot、leave或transport
phase成为第二份mapping truth。

## Allocation、Failure 与 Cleanup

- allocation-free不是本RFC的target或验收条件。IRQ/IRQ-off、Drop、decommit及completion相关路径可以使用与一次有界
  操作相称的适度分配；不得仅为排除分配而引入侵入式对象、镜像状态、固定容量第二真相源或不自然的预分配协议。
- 当前工程阶段承认heap allocation OOM为kernel-fatal。普通分配失败不需要转换成新的errno、Signal、rollback或
  fail-close leak；实现也不能在destructive commit后把allocation failure作为recoverable result返回并继续执行。
- 允许分配不等于允许blocking/synchronous reclaim、普通sleep lock、remote placement、复杂callback/log formatting或
  复杂对象析构进入IRQ/noirq临界区。实现必须根据真实上下文保持操作适度、有界、不可睡眠且锁序可审查。
- Drop可以承担局部资源释放并允许自然分配，但不得重新成为唯一remote completion/failure owner。正常路径必须显式
  建立target completion，retired资源仍在completion之后释放。
- 当前不支持runtime CPU hotplug。target在没有ack或等价destruction proof时消失、residency重复/遗漏转换、最终释放
  address space时仍有无解释resident等情况属于correctness invariant violation，不能伪装成普通成功或由OOM policy
  吸收。

## ABI 与可见语义

本RFC不增加syscall、flag、UAPI structure、errno或新的Linux-visible capability。成功的mapping operation仍满足现有
`MM-TLB-LOCAL-001 / MM-TLB-REMOTE-001` completion语义；用户只可能观察到不必要remote interruption减少及由此带来的
时序/性能变化，不能观察到较弱的mapping、COW、Signal、userptr或retirement保证。

## Contract Impact

| Contract ID | 变化 | 当前规则 | Target 摘要 | Cutover |
| --- | --- | --- | --- | --- |
| [`MM-TLB-REMOTE-001`](../../contracts/mm/user-fault-local-tlb.md#mm-tlb-remote-001--destructive-user-mapping在dependent-continuation与retirement前完成remote-ack) | Refine | Destructive user mapping同步完成全部其它boot-online CPU；每个address space固定预备allocation-free transport | Remote target由唯一residency协议覆盖；稳定非resident CPU不再接收本address space的shootdown；保留同步ack、retirement和continuation顺序，并取消allocation-free作为普遍contract要求 | `USER-TLB-RESIDENCY-CUTOVER` |

### Dependencies

- [`MM-TLB-LOCAL-001`](../../contracts/mm/user-fault-local-tlb.md#mm-tlb-local-001--actual-user-fault按operation-local-added延迟local-completion)：commit分类与current-core local policy保持不变。
- [IRQ-off heap allocation开放问题](../../register/open-issues.md#ane-20260622-irq-off-heap-allocation)：允许适度有界allocation，但blocking/reclaim/复杂副作用风险仍需按真实上下文审查。

## Implementation Boundary

- **允许改变：** user address-space residency state与窄activation handoff、destructive target selection、直接user
  activation caller接入、必要的transport内部表示、owner-local assertion/KUnit，以及同owner、行为保持的模块拆分。
- **必须保持：** PTE/VMA/VMO/COW唯一truth、`MM-TLB-LOCAL-001`、destructive completion ordering、IPI ack、retirement
  lifetime、dependent continuation、generic IPI consumer、kernel page-table行为、RV64/LA64 ABI与所有syscall可见语义。
- **工程余地：** 实现可以选择per-address-space或per-CPU表示、mask或其它有限集合、锁或atomic、scoped guard或显式
  handoff、保守snapshot、复用现有预分配transport或按需适度分配。RFC不把这些候选提升为accepted设计，也不要求为
  未来ASID/hotplug提前抽象。
- **实现提示：** 预计触达MM user-space completion/activation、scheduler mapping switch、exec/temporary activation与
  user-TLB transport；这些是非穷举owner提示，不是逐文件write set。
- **停止条件：** 需要ASID/generation、lazy或async/best-effort remote completion、runtime hotplug、改变architecture
  activation guarantee、扩大public API/ABI、引入第二份residency/mapping truth、无法闭合真实activation caller、多个
  semantic cutover、production probe或降低source-review验收时，必须回到RFC review / Target Renegotiation。

## Acceptance 与 Validation

- 接受本RFC只批准target、owner/handoff、failure/cleanup、Contract Impact、Implementation Boundary与单次
  `USER-TLB-RESIDENCY-CUTOVER`；不批准某个具体mask、lock、API、allocation或模块方案，也不自动授权实现。
- 并发正确性主要由bounded source review验收。review必须闭合全部production user activation caller，证明唯一
  residency truth、join/leave/snapshot二分、source local completion、target obligation、temporary restoration、
  address-space lifetime、锁序和`completion -> retirement/continuation`关系。
- architecture source audit必须确认RV64/LA64 user mapping activation在完成join之前提供本RFC依赖的local TLB
  destruction与必要内存顺序；不能仅凭trait注释或一次runtime通过推断硬件保证。
- owner-local KUnit只覆盖高判别力的production状态转换与target selection，例如稳定单resident、multiple resident、
  source exclusion、join/leave边界、same-mapping与temporary activation restore。KUnit不追求数量，不要求调度线程、
  随机stress或为测试建立独立production facade。
- 双架构release build与SMP focused runtime负责普通调度切换、exec、destructive completion及无死锁/断言失败的
  集成/回归。temporary activation的caller闭包、CPU pin、lifetime与Drop restore由bounded source review验收；不要求为
  该显然的scoped handoff另造跨进程procfs runtime oracle。runtime不被解释为对全部并发交错“不存在错误”的证明。
- 稳定单resident address space的destructive operation不应产生remote target；稳定multi-resident场景只覆盖resident
  remote CPU；all-resident场景允许退化为现行all-online target。该结构性结果由source/KUnit或等价直接oracle证明，
  不要求wall-clock阈值。
- 数值performance A/B由用户后续按需执行，不属于cutover gate。full LTP、final harness、实体硬件、runtime hotplug、
  ASID与调度交错穷举均为Not Run，除非实现反馈证明其中某项已成为真实acceptance依赖。
- `USER-TLB-RESIDENCY-CUTOVER`必须原子完成production caller接入、target selection、source review、必要KUnit/build/runtime
  evidence与`MM-TLB-REMOTE-001` Refine。任一surface未闭合时保持Not Cut Over，不能用all-online兼容旁路声明完成。

## 风险与反馈

- 最大风险是某个直接activation caller绕过residency handoff，或把scheduler current task、CPU-private root snapshot、
  diagnostic mask与address-space resident set并列为多个行为truth。source audit必须从所有`activate_addr_space()`与
  user-space activation facade caller出发闭合，而不是只检查scheduler。
- noirq activation与process-context destructive completion可能形成锁序或IPI wait环。实现必须在发送/等待同步IPI前
  退出residency临界区；若自然实现做不到，真实问题已经扩张到scheduler/MM completion owner，必须停止review。
- temporary activation的目标lifetime、原mapping恢复和错误/提前退出路径可能暴露新的cleanup问题。实现可以选择
  scoped capability、显式handoff或其它直接形状，但不能依赖调用者记忆协议或留下无退出条件的兼容桥。
- 保守多发是允许的实施余量，不是逃避target的永久fallback。若稳定nonresident CPU仍无法从target移除，或优化只能在
  特定caller/architecture/test上成立，应回到RFC review而不是把较弱能力写成closure。
- 如果实现证据要求改变target、residency owner、architecture activation dependency、failure/cleanup、Contract Impact
  或validation claim，必须在cutover前触发Target Renegotiation；内部类型、同步原语、allocation与文件布局调整不触发。

## 文档与证据

- Current contract：[`MM-TLB-LOCAL-001 / MM-TLB-REMOTE-001`](../../contracts/mm/user-fault-local-tlb.md)
- Baseline RFC：[User TLB Completion](../user-tlb-completion/index.md)
- Allocation boundary：[IRQ-off heap allocation开放问题](../../register/open-issues.md#ane-20260622-irq-off-heap-allocation)
- commit / PR / optional transaction：本次提交
- 外部源码证据：None

## 修订记录

2026-08-09，维护者接受本文 target、owner/handoff、failure/cleanup、Contract Impact、
Implementation Boundary、acceptance 与单次 `USER-TLB-RESIDENCY-CUTOVER` 为 R0，并另行授权开始实现。

2026-08-09，维护者接受R1 acceptance修订：temporary activation由bounded source review验收，不再要求双架构focused
runtime构造跨进程procfs oracle；实现target、owner、ABI、contract与其它validation floor不变。

## Closure

R1 implementation与`USER-TLB-RESIDENCY-CUTOVER`已经完成。`UserSpaceHandle::tlb_residency`以一个owner-local
resident CPU集合成为唯一target truth；`activate_mapping_transition()`在同一边界内编排user/kernel、user/user与
same-mapping切换，architecture root安装和本核full TLB invalidation先于join，旧mapping的leave后于该destruction。
scheduler与exec只提交old/new handle，procfs cmdline/environ使用preemption-pinned scoped capability完成temporary
activation与Drop restore；原始`activate()`和未使用的root PPN暴露面已经移除。

destructive transaction在mutation与current-core completion之后通过residency锁准备target，随后仍在释放`UserSpace`
mutex后发送并等待同步IPI；锁内只完成有界resident snapshot与既有per-CPU message preparation，不在residency锁内等待
ack。target一旦进入transport便不受后续leave撤销，join若发生在snapshot之后则通过同一锁序看到commit并在发布
residency前完成本核full invalidation。现有`completion_ordering`、retirement、dependent continuation与transport ack
lifecycle保持不变；实现继续复用allocation-free预分配transport，没有引入generation、fallback或新分配路径。

最终bounded source review从两个architecture `activate_addr_space()`实现向上闭合kernel bootstrap、scheduler、exec与
procfs temporary caller，核对single residency truth、same-mapping稳定性、join-new-before-leave-old保守窗口、temporary
lifetime/restore、snapshot后message ownership、boot-fixed online invariant，以及`mutation -> local completion ->
snapshot -> unlock -> ack -> retirement/continuation`锁序；未发现剩余属于本RFC的production-reachable
Apollyon/Keter/Euclid。

RV64与LA64的SMP=8 KUnit-enabled release build和QEMU focused runtime均通过：两边600项KUnit全部为`ok`，其中新增两项
residency状态转换与一项source-only零remote target/selected target transport coverage；两边`/bin/userptr`六组均通过，
并由正常init/exec与scheduler运行覆盖production activation主路径。focused rootfs随后因未提供`/dev/vdb`在competition
mount阶段失败，这不覆盖已经完成的KUnit/userptr evidence。competition-final RV64/LA64 release build也通过。

cutover原子Refine[`MM-TLB-REMOTE-001`](../../contracts/mm/user-fault-local-tlb.md#mm-tlb-remote-001--destructive-user-mapping在dependent-continuation与retirement前完成remote-ack)：
remote target改由唯一residency协议覆盖，并取消allocation-free作为普遍contract guarantee，同时保留同步ack、retirement
和dependent continuation。full LTP、final harness、实体硬件、runtime hotplug、ASID、数值performance A/B与调度交错
穷举均为Not Run，且不属于R1 closure floor。按R1 acceptance，跨进程procfs temporary activation runtime oracle同样为
Not Run；其caller闭包、CPU pin、handle lifetime与Drop restore由上述bounded source review完成验收。
