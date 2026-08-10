# RFC-20260808-user-tlb-completion

**状态：** Closed
**修订：** R2
**负责人：** doruche, Codex
**最后更新：** 2026-08-09
**领域：** MM / user address space / page table / TLB shootdown / SMP / user access
**影响契约：** Refine
[`MM-TLB-LOCAL-001`](../../contracts/mm/user-fault-local-tlb.md#mm-tlb-local-001--actual-user-fault按operation-local-added延迟local-completion)，
Introduce `MM-TLB-REMOTE-001`
**执行记录：** None

## 摘要

cutover前，user address-space mutation 在 `UserSpace` mutex 内修改 PTE 并完成 current-core local
invalidation，随后返回 `RemoteUspFenceGuard`，由 guard 在 mutex 外 Drop 时无条件同步广播 remote
TLB shootdown。这个模型同时存在两个问题：每次 page fault 即使只安装新 leaf、保持原 leaf 或放宽权限也会
广播全部 remote CPU；而 destructive mutation 释放 mutex 后到 guard 真正完成之间，又没有 address-space
protocol state 阻止 successor operation 越过尚未完成的 predecessor。

本 RFC 把 remote fence 从“每次 fault 返回一个 Drop guard”收敛为 user address-space completion ordering。
只有 replacement、restriction、unmap、discard、decommit 等 destructive commit 才引入同步 remote completion
义务；`Added`、`Unchanged` 与同 frame permission relaxation 不产生自己的 remote obligation，仍服从现行
current-core local completion policy。任何依赖 destructive commit 后 mapping state 的 continuation/exposure，以及
retired backing 的释放或复用，都必须发生在该 obligation 完成之后。RFC 只固定这个 happens-before、唯一 owner、
failure 与 cleanup，不固定 operation 在锁前等待、继承/join obligation、coalesce，还是采用粗粒度串行。

## 背景

[`MM-TLB-LOCAL-001`](../../contracts/mm/user-fault-local-tlb.md) 当前把 Mapper 同一次 walk 得到的 leaf
commit 分为 `Added`、`Unchanged`、`Relaxed` 与 `ReplacedOrRestricted`，并只让
`UserReturn + Added` 延迟 current-core local invalidation。这个 operation-local relation 是正确且需要保留的
PTE fact，但它不携带 address-space 历史：`Added` 只说明本次 commit 前 leaf invalid，不能证明其它 CPU 已完成
更早的 destructive remote invalidation。

cutover前的`ANE-20260808-MM-LAZY-LOCAL-TLB-REMOTE-PREDECESSOR-WINDOW`记录了具体SMP窗口。
CPU A 完成 destructive mutation 和自己的 local invalidation、释放 `UserSpace` mutex，
但尚未 Drop remote guard；CPU B 携带 predecessor translation 取得 mutex，并把新 mapping 的 invalid leaf 提交为
`Added`。如果 ordinary user-entry Signal arbitration 在原 instruction refault 前重定向到 handler，CPU B 可能在
CPU A 的 remote completion 前短暂使用 predecessor translation。

旧guard还把completion failure隐藏在Drop中；cutover前的
`ANE-20260807-MM-REMOTE-FENCE-FAIL-CLOSE-RETENTION`只能在broadcast failure后永久保留retired frame，避免stale
translation访问已经复用的物理页；它不能阻止不携带
retired frame 的 mapping operation 继续，也不能建立 predecessor/successor 顺序。与此同时，exception userptr 的
[`UACCESS-KETER-001`](../exception-userptr-access/tracking-issues.md#uaccess-keter-001---remote-fence-仍在-userspace-mutex-内完成)
已经要求 synchronous remote shootdown 不能在 `UserSpace` mutex 内完成。因此修复不能退回“持锁广播”，也不能只在
fault 或 Signal owner 中增加一次局部检查。

## 目标

- 为每个 user address space 建立唯一的 remote-completion ordering owner，使依赖 post-destructive mapping state 的
  user return、Signal redirect、kernel retry、syscall success、fork child publication与backing reuse不能越过
  尚未完成的 obligation。
- 保留 Mapper 同一次 walk 的 operation-local commit relation，并把 remote policy 收敛为：monotonic change 不广播，
  destructive change 在 dependent continuation/exposure 和 backing retirement 前同步完成 remote invalidation。
- 让 destructive mutation、local invalidation、锁外 remote acknowledgement、failure 与 retired-frame cleanup 形成
  可证明的 ordering/lifecycle relation；Drop 不再是 correctness completion 和 failure policy 的唯一承载者。
- 在没有 per-address-space CPU residency truth 的方案中，对 destructive obligation 继续覆盖全部其它 online CPU；
  优化来自“只对 destructive change 广播”，而不是未经证明地缩小 target CPU 集合。
- 在 RV64 与 LA64 的 SMP execution 中关闭 predecessor/Signal redirect 窗口，同时保持 existing
  immediate userptr、futex/fault-in、COW、VMA operation 与 syscall ABI。

## 非目标

- 不引入 per-address-space active CPU mask、ASID/PCID generation、TLB epoch、lazy remote generation 或
  scheduler context-switch flush protocol。
- 不改变 Signal pending/action、user-entry arbitration、trapframe redirect 或“原 instruction 必须先 refault”的
  语义；remote predecessor 必须由 MM address-space protocol 关闭。
- 不把 generic IPI transport、CPU hotplug 或 architecture TLB primitive 重写成新的公共框架；若当前 transport
  无法满足本 RFC 的 post-mutation completion 要求，只允许增加 MM 所需的窄 preparation/completion capability。
- 不把所有 remote target 缩减到当前运行过该 address space 的 CPU，也不为性能目标引入可 stale 的 residency cache。
- 不规定每个 operation 必须在锁定 `UserSpace` 前取得 gate，也不规定 successor 必须等到 predecessor completion
  后才能读取或修改内部 PTE/VMA；只约束依赖 post-destructive state 的外部 continuation/exposure 与 cleanup。
- 不改变 COW/VMO resolve、VMA ownership、syscall flags/errno、partial user-copy progress 或 kernel page-table fence。
- 不承诺数值性能门槛、实体硬件 TLB latency 或完整 LTP/final score；本 RFC 只要求证明不再为 monotonic fault
  发送 remote shootdown，并关闭 correctness window。

## Owner 与协议边界

- **PTE/VMA truth owner：** `UserSpace` 在其现有 mutex 内继续唯一拥有 VMA topology、page table 与 mutation。
  Mapper 只从同一次 walk/commit 返回 operation-local relation；不得增加第二次 classification walk、pending PTE bit
  或 address-space relation cache。
- **Completion-ordering owner：** MM user-address-space completion domain 唯一拥有 outstanding destructive
  obligation或等价completion proof，与dependent continuation/retirement的顺序关系。RFC不固定它由`UserSpaceHandle`、shared inner、
  gate、sequence 还是 linear transaction 承载；同一 address space 的全部参与路径必须观察同一个 ordering truth，
  不能按 handle/caller 复制。该 truth 不能缓存当前 PTE、VMA、CPU residency 或 generation，也不能由 diagnostic
  字段驱动。
- **Remote transport owner：** IPI transport 继续拥有 message、target delivery 与 acknowledgement。MM 只持有
  prepared/synchronous completion capability，不读取 IPI queue 或 CPU-private state。
- **Retirement owner：** destructive completion obligation或由它线性移交的cleanup capability持有retired backing；
  只有target set完整确认旧translation已失效后才能释放或复用。VMO/frame allocator不读取completion-ordering
  state，也不建立平行retirement truth。
- **Continuation owner：** actual user fault、immediate userptr/futex/fault-in 与 VMA syscall 各自保留现有 continuation
  和返回语义；当它们依赖post-destructive mapping state时，只有覆盖该continuation的remote completion已被观察后
  才能继续retry、user-entry arbitration、syscall success或fork child runnable publication。backing reuse受上面的
  retirement owner约束。

## Target invariants

本文的`obligation`是correctness proof obligation：实现必须证明remote completion与dependent continuation/retirement
之间的顺序；它不要求存在同名runtime object、token、counter或state machine。

### Destructive completion ordering

destructive operation在唯一PTE/VMA owner内完成mutation与必要current-core local invalidation后，必须由一个覆盖
受影响translation的remote completion obligation或等价ordering proof承接。remote completion在不持有
`UserSpace` mutex时同步完成，并建立以下happens-before：

```text
destructive PTE/VMA mutation
    -> current-core local completion
    -> remote completion
    -> dependent continuation / exposure

remote completion
    -> retired backing release or reuse
```

dependent continuation/exposure 包括 actual fault 返回后的 user execution与Signal handler redirect、Immediate
userptr/futex retry、依赖新 mapping 的 syscall success、fork child runnable publication，以及任何把 post-destructive
mapping 当作已经完成的外部 handoff。单纯在 owner 内读取或修改 PTE/VMA 不自动构成 exposure；successor 可以在
mutation 前等待 predecessor，也可以先完成内部 mutation后继承/join predecessor obligation，只要相关 remote
completion 覆盖其依赖，并且上述 continuation/exposure仍在completion之后。

PTE mutation 是 address-space owner 的内部提交点，不等同于 remote completion。mutation与acknowledgement之间，
predecessor CPU 仍可能执行旧 translation；这是同步 shootdown 正在关闭的正常窗口。已经在 destructive operation
之前取得 continuation 的 predecessor access可以与它竞争，但不能在 remote completion 后继续使用旧translation。
实现可以使用 owner-local gate、sequence、显式 transaction、obligation propagation或coalescing；不得通过持有
`UserSpace` mutex发送同步IPI来满足顺序，也不得让architecture accessor解锁它不拥有的raw mutex。

### Commit 分类与 remote policy

| Commit / operation | Target 分类 | Current-core local completion | Remote completion |
| --- | --- | --- | --- |
| `Added`，且没有 valid ancestor replacement | Monotonic | 保持 `MM-TLB-LOCAL-001`：仅 `UserReturn` 可延迟，`Immediate` eager | 不产生自己的 obligation；若依赖未完成的destructive predecessor，continuation必须被其completion proof覆盖 |
| `Unchanged` | No change | 保持现行 eager policy | 不产生自己的obligation；若依赖未完成的destructive predecessor，continuation必须被其completion proof覆盖 |
| 同 frame、同 leaf kind、只增加 permission 的 `Relaxed` | Monotonic | 保持现行 eager policy | 不产生自己的obligation；若依赖未完成的destructive predecessor，continuation必须被其completion proof覆盖 |
| frame replacement、permission restriction、leaf kind 或 valid ancestor replacement | Destructive | Eager | 同步完成全部其它 online CPU |
| unmap、`MAP_FIXED`/remap clobber、COW write replacement、fork write restriction、discard/decommit 与 backing retirement | Destructive | Eager | 同步完成全部其它 online CPU |
| 无法在唯一 owner/commit point 证明 monotonic | Destructive | Eager | 同步完成全部其它 online CPU |

本 RFC 可以把 range operation 保守分类为 destructive，不要求逐 leaf 证明“实际没有 present PTE”。transport 可以使用
range或full flush，也可以在不削弱coverage与continuation ordering的前提下合并/coalesce obligation；RFC不规定
broadcast round数量。普通non-fixed VMA insertion在没有替换PTE时不产生自己的remote obligation。

“本轮不广播”只表示 monotonic/no-change operation 不创建新的 remote obligation，不能解释为丢弃它所依赖的
destructive predecessor。实现可以在mutation前等待，也可以把predecessor obligation传播到本轮continuation；
`Added`的remote省略建立在这个ordering proof上，而不是把operation-local relation扩张成address-space generation。

### Remote target 与 CPU online 边界

本 RFC 不维护 address-space residency，因此每个 destructive obligation 的 target 是 commit CPU 之外、已经发布 online
的全部 logical CPU。current CPU 由本轮 local completion 覆盖。从未发布 online 的 CPU，或由 offline owner
已经证明旧 TLB state 被摧毁、且尚未重新发布的 CPU，不属于本轮 target；CPU 一旦进入 target，在 acknowledgement
或同等 TLB-destruction proof 之前不能从本轮移除。

如果 live CPU topology 允许 target 在 commit 与 acknowledgement 之间 offline，offline owner 必须参与同一个
completion protocol；如果当前平台没有 runtime hotplug，则实现必须把 boot-fixed online set 作为明确 invariant。
`TargetOffline` 不能在 destructive commit 后作为 recoverable result 交还 caller。

### Failure 与 cleanup

- remote message、target snapshot或其它软件可见的fallible preparation必须在第一项destructive mutation前完成。
  preparation失败时不得修改PTE/VMA、留下pending completion/cleanup state或接管retired frame；caller只能观察
  现有operation所允许的pre-mutation failure。
- destructive mutation后，针对已确定target set的路径不得再把可恢复`TargetOffline`、allocation error或其它
  software-visible failure交还caller，也不得继续user execution来伪装完成。普通owner-local collection growth沿用
  当前内核的全局allocation policy：分配失败直接panic，不要求仅为消除该panic边界而提前按容量上限预留；能够返回
  `Result`的fallible preparation仍必须在mutation前完成。
- 本RFC不建设system-wide fail-stop、retry queue、persistent poisoned address space或failed generation。实现必须通过
  pre-mutation preparation与当前boot/online invariant使post-mutation completion不可失败；如果做不到则不得cutover，
  而不是扩张到panic、power或CPU-lifecycle新协议。
- Drop可以断言“对应remote completion已经被显式观察”或执行不影响语义的cleanup，但不能是唯一
  completion/failure路径。正常路径必须显式观察remote acknowledgement，再允许dependent continuation与
  retired-frame release。
- actual user fault 不得把 remote transport resource pressure转换为新的 `SIGSEGV`；如果同一次 Mapper walk 无法在
  commit 前取得 fallible preparation，transport 必须提供 reserved/infallible capability，或实现必须停止并回到
  RFC review。

## ABI 与可见语义

本 RFC 不增加 syscall、flag、UAPI structure 或新的 Linux-visible capability，也不改变现有成功操作的 mapping、
permission、COW、Signal、partial-copy 与 errno 语义。可见变化只来自 correctness 收紧：成功的 destructive operation
返回、actual fault continuation、immediate kernel retry 和 backing reuse 都证明 predecessor remote completion 已完成。

实现若需要新增 pre-mutation errno、把 IPI allocation failure映射为新的 user-visible signal/error、增加 retry 次数或改变
partial progress，必须停止并回到 RFC review。post-mutation failure 不得通过普通 errno 返回。

## Contract Impact

| Contract ID | 变化 | Cutover前规则 | Target 摘要 | Cutover |
| --- | --- | --- | --- | --- |
| [`MM-TLB-LOCAL-001`](../../contracts/mm/user-fault-local-tlb.md#mm-tlb-local-001--actual-user-fault按operation-local-added延迟local-completion) | Refine | `UserReturn + Added` 延迟 local completion，但明确排除 in-flight remote predecessor 下的 mapping-identity guarantee | local policy组合不变；`Added`不产生自己的remote obligation，其dependent continuation必须被尚未完成的destructive predecessor completion proof覆盖，因此不再受predecessor/Signal redirect limitation约束 | `USER-TLB-COMPLETION-CUTOVER` |
| `MM-TLB-REMOTE-001` | Introduce | None；remote guard 对每次 fault/operation Drop broadcast，且没有 address-space predecessor ordering | destructive commit由唯一ordering owner可观察的锁外同步completion obligation或等价proof覆盖；monotonic/no-change不创建自己的obligation；dependent continuation/exposure与retirement发生在覆盖它们的remote ack之后 | `USER-TLB-COMPLETION-CUTOVER` |

### Dependencies

- [`USER-ENTRY-001/002`](../../contracts/task/user-entry.md)：Signal/lifecycle/jobctl arbitration 顺序保持不变；MM
  destructive continuation 必须在进入该 arbitration 前完成。
- [`MM-TLB-LOCAL-001`](../../contracts/mm/user-fault-local-tlb.md)：Mapper relation 与 current-core policy 是本 RFC
  的baseline；R2 closure已完成本表所列Refine。

## Implementation Boundary

- **允许改变：** MM user-space completion-ordering boundary、remote fence linear capability或等价owner-local ordering、
  fault/VMA operation的typed commit outcome、retired-frame handoff、narrow IPI preparation/completion API、exception
  userptr / explicit fault-in的锁外completion与revalidation、inline KUnit及双架构focused runtime oracle。
- **必须保持：** Mapper/PTE/VMA/VMO 的唯一 truth owner，现有 local completion relation与continuation，Signal 与
  user-entry owner，syscall ABI/errno/partial progress，COW与permission语义，RV64/LA64 architecture invalidation
  primitive，以及 generic IPI consumer 的现行行为。
- **实现提示：** 预计主要位于 `anemone-kernel/src/mm/{paging,uspace}/`、两个 architecture user-access/fault caller
  与 `exception/ipi/` 的窄 MM capability；这些位置不是逐文件 write set，也不授权重构 generic IPI framework。
- **停止条件：** 需要 active CPU mask/ASID/generation、runtime hotplug或system-wide fail-stop新协议、Signal retry
  token、持`UserSpace` mutex同步IPI、async/best-effort shootdown、第二份mapping truth、新user-visible failure、多个
  semantic cutover、production probe 或较低验证强度时，必须回到 RFC review / Target Renegotiation；若真实路线需要
  多阶段或 probe，再按需增加 `implementation.md`，不能在单次 cutover 中隐藏过渡态。

## Acceptance 与 Validation

- 接受本 RFC 只批准 Draft 中的 target、owner、failure/cleanup、contract delta 与单次 cutover，不自动授权实现。
- owner、lifetime与happens-before由源码审查承担验收：必须从published `UserSpaceHandle`的production caller闭合唯一
  completion-ordering owner，核对destructive mutation与dependent continuation串行，核对`UserSpace` mutex unlock、
  remote ack、retirement release/relock顺序，并核对page-table/backing在ack前保持live。
- transport源码审查必须确认fallible storage在address space发布/首次mutation前准备，post-mutation round不分配、不返回
  recoverable failure；当前不支持runtime hotplug，因此boot-fixed online target与“target进入round后必须ack”是correctness
  invariant。若live topology改变该前提，必须回到RFC/CPU-lifecycle review，不能把`TargetOffline` warning算作完成。
- exception userptr source audit必须 neutralize `UACCESS-KETER-001`：synchronous remote completion不在`UserSpace`
  mutex内发生，COW/replacement retry只在completion后继续，ordinary copy、explicit fault-in与non-active probe没有通过
  raw mutation路径绕过ordering或丢弃obligation。
- owner-local KUnit、forced interleaving和SMP runtime只负责覆盖代表性路径并捕捉回归。它们可以观察fixed replacement、
  permission restriction、fork COW、heap decommit、monotonic/no-change与userptr行为，但“测试没有暴露问题”不能替代
  caller/lifetime/ordering审查，也不被写成对全部调度交错不存在错误的证明。
- 不强制为pending Signal redirect、原instruction refault、真实old translation或尚未出现的transport/hotplug failure制造
  预测性测试、probe或抽象。现有可真实执行的transport construction failure KUnit保留；新增failure injection只有在出现
  对应production failure owner或可观测语义时才形成义务。
- RV64与LA64 release build、SMP>1 KUnit/runtime、existing userptr oracle与文档验证是cutover integration floor。
  full LTP、final harness、实体硬件和数值performance A/B不是R2 closure必需证据，未运行时必须明确Not Run。
- `USER-TLB-COMPLETION-CUTOVER` 必须原子完成实现、Refine `MM-TLB-LOCAL-001`、Introduce
  `MM-TLB-REMOTE-001`、移除 predecessor limitation、关闭 remote failure/retention limitation（或把仍真实存在的
  更窄问题重新登记）并 neutralize exception-userptr tracking issue。任一 surface 未闭合时不得声明 cutover。

## 风险与反馈

- 最大实现风险是caller绕过completion ordering，继续直接取得`&mut UserSpace`后在内部触发page fault并丢弃
  predecessor/destructive obligation。实现审计必须
  从 `UserSpaceHandle::{with_usp,lock}`、architecture userptr、`fault_in_page()`、VMA syscall 与 fork/exec/exit
  consumer 出发闭合，而不是只替换 ordinary trap path。
- 新增owner state只能表达outstanding destructive completion或等价ordering。若实现开始缓存PTE relation、CPU
  residency、range generation或“可能需要flush”的诊断bit，它会成为第二份truth，必须停止并重新设计。
- all-online broadcast 仍可能有较高 tail latency，但 destructive operation 本来就承担同步 correctness obligation；
  本 RFC 不以 active-mask 优化换取新的 scheduler/MM handoff。后续若测量证明需要缩小 target，单独 RFC 定义 residency
  owner与CPU lifecycle。
- 如果窄 IPI capability 无法同时做到mutation前preparation和mutation后infallible completion，真实问题已扩张到
  IPI/CPU-lifecycle owner，不能把 warning、frame leak或测试特判留作长期桥。

## 文档与证据

- Current contract：[`MM-TLB-LOCAL-001 / MM-TLB-REMOTE-001`](../../contracts/mm/user-fault-local-tlb.md)
- 触发本 RFC 的小迭代：[User fault local TLB completion](../../devlog/changes/2026-08-08-user-fault-local-tlb.md)
- cutover前的predecessor与remote failure/retention limitation已从active register移除，历史由Git保存。
- Exception userptr neutralized issue：
  [`UACCESS-KETER-001`](../exception-userptr-access/tracking-issues.md#uaccess-keter-001---remote-fence-仍在-userspace-mutex-内完成)
- commit / PR / optional transaction：None
- 外部源码证据：None

## 修订记录

2026-08-09，维护者接受本文 target、owner、failure/cleanup、contract delta、acceptance 与单次 cutover 边界为 R0，
并另行授权开始实现。

2026-08-09，维护者接受 R1 failure-boundary 修订：post-mutation 仍不得向caller暴露可恢复failure或越过remote
completion，但普通堆分配沿用当前内核alloc-failure panic policy；不得仅为绝对禁止allocation而引入不自然的容量预留。
target、owner、ABI、contract delta、ordering、retirement与其它acceptance保持不变。

2026-08-09，维护者接受R2 acceptance-boundary修订：唯一owner、production caller闭包、lifetime与happens-before
由源码审查验收；KUnit、forced interleaving与runtime只作为代表性路径的集成/回归证据，不伪装成并发正确性的穷举
证明。R2不再强制构造pending Signal redirect、原instruction refault、真实old translation或预测性failure injection；
target、owner、ABI、contract delta、ordering、retirement与cutover surface保持不变。

## Closure

R2 implementation与`USER-TLB-COMPLETION-CUTOVER`已经完成。`exception/ipi/{mod.rs,user_tlb.rs}`保留generic
IPI facade，并把MM transport收窄为crate-private capability；per-address-space `completion_ordering`串行化destructive
predecessor与dependent continuation，remote acknowledgement在`UserSpace` mutex外完成，retired page-table/frame/
backing ownership保留到ack之后；`Added`、`Unchanged`与`Relaxed`不创建自己的remote round。实现只为真实fallible
transport construction预备storage；VMA/backing mutation保持owner-local直接形状，没有容量上限预留、完整registry clone
或为预测性failure建立的新抽象。

最终bounded independent review先发现并修复一项production-reachable问题：non-heap `mprotect` relaxation清leaf时曾
detach空page-table却不创建remote obligation；当前relaxation仍清leaf以保持private/COW refault语义，但使用
`try_unmap_keep_page_tables()`保留branch lifetime。复核随后闭合published mutation callers、fork、SysV、Anon/Shadow、
page-table retirement、transport phase与boot-fixed target invariant，未发现剩余属于本RFC的production-reachable
Apollyon/Keter/Euclid。该source review是owner、lifetime与happens-before的验收证据。

最终RV64与LA64的SMP=8 release build均通过，并在两边QEMU中取得focused evidence：597项KUnit全部通过，其中
5项completion-ordering、2项user-TLB transport以及page-table retirement/heap flags相关定向项均为`ok`；两边
`/bin/userptr`六组均通过。focused
rootfs在这些marker之后进入competition阶段时因本轮未提供`/dev/vdb`而失败，随后由timeout终止；该失败不覆盖或撤销
已经完成的RFC runtime evidence。KUnit/runtime只记录被执行路径符合预期，不被表述为全部并发交错的证明。

cutover原子Refine[`MM-TLB-LOCAL-001`](../../contracts/mm/user-fault-local-tlb.md#mm-tlb-local-001--actual-user-fault按operation-local-added延迟local-completion)、
Introduce[`MM-TLB-REMOTE-001`](../../contracts/mm/user-fault-local-tlb.md#mm-tlb-remote-001--destructive-user-mapping在dependent-continuation与retirement前完成remote-ack)，
移除register中的predecessor与remote failure/retention limitation，并neutralize
[`UACCESS-KETER-001`](../exception-userptr-access/tracking-issues.md#uaccess-keter-001---remote-fence-仍在-userspace-mutex-内完成)。
full LTP、final harness、实体硬件、数值performance A/B与额外system-wide/post-mutation failure injection均为Not Run，
且不属于R2 closure floor。
