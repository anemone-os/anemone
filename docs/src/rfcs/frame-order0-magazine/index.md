# RFC-20260816-frame-order0-magazine

**状态：** Closed
**修订：** R0
**负责人：** doruche
**最后更新：** 2026-08-16
**领域：** mm / frame allocator / SMP
**影响契约：** `MM-FRAME-001`、`MM-FRAME-002`（Introduced）
**执行记录：** None（单一cutover直接由本页记录closure）

## 摘要

本 RFC 在 frame allocator owner 内为 order-0 frame 引入有界 current-CPU magazine，使本地命中的单页
分配与释放只进入当前 CPU 的短 noirq transaction，不再获取共享 buddy 锁。magazine empty / full 时通过
operation-local bounded batch 与 buddy 交换；任何通过buddy order与算术admission的分配首次进入buddy后因当前
availability失败时，allocator逐个sweep本次boot已注册的全部逻辑 CPU magazine、把detached frame归还buddy，并
重试一次。

本 RFC 只建立结构性并行能力：不同 CPU 命中各自 magazine 时不竞争同一把 buddy 锁，共享锁访问被收敛到
batch boundary 与失败恢复路径。它不设置吞吐、加速比或容量调优门槛，也不把普通 SMP boot 或串行 KUnit
外推为完整并发交错证明。

## 背景

当前 `mm::frame` 使用一把 `NoIrqSpinLock` 包裹整个 buddy allocator。`alloc_frame()`、`alloc_frames()`、
最终 `FrameHandle` / `Folio` 释放和 `frame_allocator_stats()` 都经过这把共享锁；即使不同 CPU 只分配或释放
互不相关的单页 frame，也会在同一个 global critical section 串行化。

现有 owner 分层本身保持直接：buddy 管理可分配物理块，`managed` 模块用 RAII 与 frame refcount 表达对外
ownership，`FrameAllocatorStats` 是 OOM、`/proc/meminfo` 与 `sysinfo` 消费的 global pressure truth。本 RFC
不重写 buddy，也不移动 RAII、OOM 或 observer owner，只在 allocator 内部增加 order-0 free-frame shard。

当前 [`MM-KMALLOC-001`](../../contracts/mm/kernel-heap.md#mm-kmalloc-001--唯一kernel-heap-domain与backing-handoff)
还规定 kernel heap 扩容通过 frame allocator 取得 backing。因此 magazine、batch、stats 与 recovery 自身不得
依赖可能再次扩容 kernel heap 的容器或 callback；这里的固定有界表示是 allocator recursion 约束，而不是追求
全内核 allocation-free。

## 目标

- `mm::frame` 继续唯一拥有 physical-frame allocation、buddy、order-0 magazine、batch handoff、failure recovery
  与 `FrameAllocatorStats`；magazine 不是新的 subsystem owner。
- `alloc_frame()`、`alloc_frame_zeroed()`、`alloc_frames(1)` 与 `alloc_frames_zeroed(1)` 共享 order-0 magazine
  route；任意单页 `OwnedFrameHandle` / `OwnedFolio` 的最终释放进入同一 deallocation route。
- current-CPU magazine 有可用 frame 时，order-0 allocation只在选择并固定当前CPU的短noirq transaction内完成，
  不获取 buddy、其它 CPU magazine 或其它 shared lock。
- 合法 order-0 frame 可以在任意 CPU 最终释放，并进入执行 final release 的当前 CPU magazine；allocator 不保存
  allocation CPU、`owner_cpu`、remote-free queue或per-frame CPU affinity。
- magazine 使用 build-time 固定容量，只在 empty / full boundary 与 buddy 交换固定上界 batch。capacity 与 batch
  进入 KernelConfig；frame allocator consumer以编译期检查拥有其非零、`batch <= capacity`、全机容量上界和算术
  安全语义。具体默认值不是本 RFC 的性能保证。
- refill允许partial batch。只要取得至少一个frame，本次order-0 allocation即可成功；未交付caller的其余frame
  发布到current-CPU magazine，无法容纳的余量归还buddy，不能因不足完整batch而泄漏或误报失败。
- magazine full 时先在local transaction内detach有界batch，释放local guard，再把batch归还buddy；不得持local
  guard逐页进入buddy。
- 任意order-0或`npages > 1` request只有在通过buddy order与算术admission、首次因当前availability allocation
  miss后，才执行一次all-magazine sweep并重试buddy一次。超过buddy支持order、算术无法安全形成request或其它
  admission failure保持现有直接失败语义，不触发sweep。magazine是可回收优化层，不能成为让buddy永久看不到
  全局free frame的reservation。
- `frame_allocator_stats()`继续提供一次coherent global snapshot。magazine与operation batch中的frame仍计入
  `free_pages`；buddy / batch / magazine之间的内部转移不改变global pressure accounting。
- 用源码审查、owner-local KUnit、代表性非法KernelConfig编译拒绝、RV64 / LA64 default KernelConfig release
  build、RV64 `smp = 2` focused cross-CPU production-path KUnit boot和ordinary non-KUnit boot smoke完成结构性
  验收；性能测量和参数调优由维护者另行执行。

## 非目标

- 为order-1及以上block建立magazine、按order分类的per-CPU block cache，或改变buddy对非二次幂`npages`的
  rounding / provenance / accounting语义；
- dirty / zeroed双cache、后台预清零，或让zeroing发生在magazine / buddy guard内；
- 稳态remote stealing、CPU间主动均衡、background drain、NUMA policy、CPU hotplug或offline drain；
- reclaim、compaction、watermark、swap、memcg、OOM reaper，或修改[`MM-OOM-001`](../../contracts/mm/oom-policy.md#mm-oom-001--oom-worker自有fixed-delay采样与victim-round)
  的periodic sampling与victim policy；
- 修改frame public API、RAII / refcount、zeroed allocation、higher-order contiguous allocation或用户可见ABI；
- per-frame free bitmap、mirrored state table、allocation-site diagnostics，或让diagnostic / stats反向决定
  membership与allocation route；
- 关闭[`ANE-20260622-IRQ-OFF-HEAP-ALLOCATION`](../../register/open-issues.md#ane-20260622-irq-off-heap-allocation)；
  本RFC只保证frame allocator自身的noirq工作简单、有界、不阻塞且不递归，不替代全部caller side-effect审计；
- 数值性能门槛、自动benchmark gate、full LTP、完整preliminary / final harness、physical hardware、极端OOM、
  长时碎片或完整并发interleaving proof。

## Owner 与协议边界

### 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 行为用途 |
| --- | --- | --- |
| `Allocated` | 现有`OwnedFrameHandle` / `OwnedFolio`与frame refcount | caller可访问的独占或共享frame lifetime |
| `MagazineFree(CpuId)` | `mm::frame`对应logical-CPU magazine | order-0 local allocation / final release fast path |
| `TransferBatch` | 当前allocator operation | local与buddy guard之间的detached handoff与failure cleanup |
| `BuddyFree` | `mm::frame` buddy | 全局order-0 backing与全部higher-order allocation |
| global `total_pages` / `free_pages` | `mm::frame` accounting | OOM与memory observer的single snapshot truth |
| current CPU identity与boot可用时点 | `percpu` / architecture boot order | 选择唯一logical-CPU magazine |
| capacity与batch选定值 | KernelConfig | 构建时policy输入；语义predicate由`mm::frame`拥有 |

每个allocator-managed order-0 frame在稳定状态只属于`Allocated`、一个`MagazineFree(CpuId)`或`BuddyFree`；
跨锁移动时只属于一个`TransferBatch`。detach先从旧owner撤销，attach再向新owner发布；frame不得同时存在于两个
free domain，也不得在early return后脱离全部owner。magazine内容与同一guard下的有效长度是membership表示，
不得另建可独立变化的bitmap、owner field或诊断表。

global accounting与free-frame placement是两个不同事实：前者唯一表达pressure snapshot，后者唯一决定某个frame
可从哪里取得。若实现为稳定snapshot保留derived counter，字段旁必须说明truth、更新点和允许的观察语义；该counter
不得用来判断具体frame membership或绕过buddy / magazine owner。

### Current-CPU transaction

选择current `CpuId`、定位唯一magazine和完成local pop / push / detach必须处于同一个不可迁移、不可被本CPU同路径
重入的短noirq transaction。local hit不得获取buddy lock、扫描其它CPU、执行blocking / reclaim、kernel heap allocation、
callback或日志格式化。不同logical `CpuId`必须映射到不同magazine，禁止clamp、fallback或CPU-ID alias。

具体表示可以使用cache-padded fixed logical-CPU table与每slot noirq lock，但类型、数组布局和LIFO / FIFO顺序属于
implementation preference；RFC只固定唯一CPU映射、有界容量、current-CPU transaction和remote sweep所需的受保护
访问能力。

### Allocation handoff

1. `npages == 1`先尝试current-CPU magazine；hit时从`MagazineFree(cpu)`直接commit为`Allocated`并建立现有RAII。
2. local miss先结束local transaction，再在一次buddy critical section内取得至多configured batch个order-0 frame。
3. operation从batch选择一个frame完成本次allocation；其余frame在新的local transaction内尽量发布到当前CPU
   magazine，无法容纳的余量在不持local guard时归还buddy。
4. 已通过buddy admission的request未取得任何frame时进入统一recovery；recovery之后只重试buddy一次，仍失败即
   返回现有`None`结果。admission failure不进入recovery。

`alloc_frame_zeroed()`与`alloc_frames_zeroed(1)`只在RAII ownership已经建立且全部allocator guard释放后清零，
不形成zeroed cache或新的free-frame状态。

### Deallocation handoff

最后一个frame ref撤销后，order-0 frame由当前deallocation operation独占并进入current-CPU magazine。若插入将超过
capacity，operation在local transaction内detach至多configured batch，确保local长度恢复到合法上界，再在local guard
外把detached batch归还buddy。frame在allocation CPU与deallocation CPU不同不改变协议，也不需要remote mutation。

`npages > 1`的最终释放仍直接归还buddy，不拆成order-0 frame填充magazine。`alloc_frames(1)`虽然返回`OwnedFolio`，
但其单页最终释放必须按order-0 route处理，不能仅凭RAII类型选择buddy。

### Buddy miss recovery

已通过buddy order与算术admission的order-0 refill或higher-order allocation首次因当前availability发生buddy miss后，
allocator按本次boot已经注册的logical CPU domain逐个检查magazine；未注册slot不能接收frame，也不构成可恢复owner。
每次只取得一个magazine guard，detach该slot当时可见的全部free frame，释放guard，再把batch归还buddy。不得同时持有两个
magazine guard，不得持magazine guard取得buddy lock，也不得把remote frame直接偷入requester magazine。全部slot各
检查一次后，allocator重试原buddy request一次；没有无界retry、等待其它CPU或OOM wake side effect。

sweep不是stop-the-world barrier或global instantaneous-empty snapshot。某CPU在其slot已经被检查后并发完成的新释放
可以留在该magazine，并按与当前allocation并发的顺序线性化；本RFC只保证sweep开始前持续驻留的free frame不会因
per-CPU cache而永久对buddy不可恢复。

### Accounting、failure 与 cleanup

- `total_pages`继续表示frame allocator实际管理的全部页；`free_pages`表示当前未由外部allocation占用的实际
  allocator charge，包括`MagazineFree`、free `TransferBatch`与`BuddyFree`。
- order-0 allocation commit使`free_pages`恰好减一，最终order-0 release恰好加一。higher-order allocation / release
  保持当前buddy按实际block charge形成的统计语义，本RFC不把任意请求`npages`误写为实际order大小。
- refill、drain和recovery sweep只改变placement owner，不改变`total_pages`或`free_pages`。轻量守恒检查使用普通
  `assert!`；accounting更新不得用saturating add / sub、clamp或silent fallback掩盖`free_pages > total_pages`
  或counter drift。既有threshold乘法的overflow处理不在此限制内。
- partial refill、full-drain、recovery和early return必须有唯一cleanup owner。已经detach的batch不得因retry failure
  丢失；已经commit给RAII的frame不得再次发布为free。
- magazine、batch、sweep与stats不得调用可能进入`mm::kmalloc`的容器增长、formatting或callback，避免
  `MM-KMALLOC -> frame allocator -> MM-KMALLOC`递归。
- allocation success / failure不提交OOM hint、pending bit或wake edge；OOM继续只读取一次live stats snapshot。

## ABI 与可见语义

本RFC不改变syscall ABI、errno、用户地址空间或frame allocator public Rust API。`alloc_frame*()`与
`alloc_frames*(1)`的返回类型、zeroing、独占访问和RAII最终释放语义保持不变；`npages > 1`仍要求buddy提供连续block。

可观察变化限于内部并发进展、allocation failure前的单次cache recovery，以及有界per-CPU free-frame placement。
magazine保留的页仍计入`free_pages`，因此`/proc/meminfo`、`sysinfo`和OOM不得把cache residency误投影为已使用内存。

## Contract Impact

| Contract ID | 变化 | 当前规则 | Target 摘要 | Cutover |
| --- | --- | --- | --- | --- |
| `MM-FRAME-001` | Introduce | None（live RAII / buddy / stats baseline尚未提取为effective ID） | `mm::frame`唯一拥有frame allocation、RAII final release与global accounting；placement transfer不改变pressure truth | 2026-08-16 `FRAME-MAGAZINE-CUTOVER`完成 |
| `MM-FRAME-002` | Introduce | None（live order-0 path仍由单一buddy lock串行化） | current-CPU bounded order-0 magazine、detached batch handoff、无guard嵌套与admitted buddy miss sweep/retry once | 2026-08-16 `FRAME-MAGAZINE-CUTOVER`完成 |

### Dependencies

- [`MM-KMALLOC-001/002`](../../contracts/mm/kernel-heap.md)：kernel heap继续以frame allocator作为backing consumer；
  frame magazine不得反向依赖kernel heap allocation或改变slab / Talc owner。
- [`MM-OOM-001`](../../contracts/mm/oom-policy.md#mm-oom-001--oom-worker自有fixed-delay采样与victim-round)：OOM
  继续只读取live `FrameAllocatorStats`，allocation edge不引入wake或第二份pressure truth。
- [`KCONFIG-VALIDATION-001`](../../contracts/configuration/kernel-parameter-validation.md#kconfig-validation-001--kernel-consumer-唯一定义参数语义合法性)：
  KernelConfig忠实传输capacity / batch，`mm::frame`唯一拥有并在消费点执行semantic predicate。
- [`KUNIT-EXEC` / `KUNIT-CONCURRENCY` / `KUNIT-PROOF` / `KUNIT-SHAPE`](../../contracts/kunit/execution-and-proof.md)：
  KUnit执行上下文、并发准入、证明外推与production code shape。

## Implementation Boundary

- **允许改变：** `mm::frame` allocator front、order-0 dispatch、buddy guard boundary、global stats implementation、
  owner-local tests，以及capacity / batch的KernelConfig传输与frame-side compile-time validation；同owner新文件、
  import / re-export、模块注册与行为保持型拆分可自然闭合。
- **必须保持：** frame public API、RAII / refcount、higher-order buddy ownership与连续分配语义、zeroed allocation、
  `MM-KMALLOC-001/002` backing方向、`MM-OOM-001` policy、architecture / percpu public owner surface、现有用户ABI与
  本文validation floor。
- **实现提示：** fixed-capacity magazine、operation-local fixed batch、logical-CPU table、stats counter与buddy内部
  批处理API均属于owner-local implementation preference；不得为未来multi-order cache提前建立generic framework、
  public trait或逐order配置族。
- **停止条件：** 若实现需要缓存order-1以上block、改变buddy rounding / provenance / accounting、增加steady-state
  remote stealing或generic remote-percpu mutation、让magazine / buddy guard嵌套、引入reclaim / OOM side effect、
  改变public API / ABI / shared contract、需要probe / 不安全中间态 / 多个正式stage或降低validation强度，必须在实现或
  cutover前停止并回到RFC review / Target Renegotiation。

R0实现使用一个连续implementation unit与唯一`FRAME-MAGAZINE-CUTOVER`。普通commit和owner内部工作片不构成
独立gate；closure没有创建`implementation.md`或transaction。

## Acceptance 与 Validation

### 源码审查

- 审计`alloc_frame*()`、`alloc_frames*(1)`、`npages > 1`、`FrameHandle` / `Folio` final Drop、pmm init与全部
  `FrameAllocatorStats` consumer，确认order-0 route完整且higher-order正常路径未被缓存。
- 建立magazine、buddy、stats、RAII、kmalloc backing与caller lock graph；确认current-CPU selection / local mutation
  在同一noirq transaction，local / remote-magazine / buddy guard不嵌套，guard内无blocking、reclaim、heap growth、
  callback或日志格式化。
- 审计RV64 / LA64 BSP与AP boot order，确认任何可达frame allocation caller都在current CPU identity可安全读取后
  进入magazine path；不得以偶然启动成功替代该source proof。
- 审计frame conservation、refcount commit/final release与global accounting更新点，确认stats不是buddy-only projection，
  transfer中没有double count / missing count，也不以counter反向决定membership。
- 审计all-magazine sweep的logical CPU domain、逐slot guard lifetime与retry bound，确认无CPU-ID alias、remote direct
  steal、遗漏detached cleanup或伪造global quiescence。

### KUnit与KernelConfig

- owner-local inline KUnit覆盖magazine empty / hit / full、push / pop、partial refill、bounded drain、batch conservation、
  duplicate / lost frame防护和capacity边界。
- detached allocator fixture覆盖order-0 buddy miss、逐magazine sweep、retry-once success / failure，以及higher-order
  admitted miss触发同一sweep但不进入magazine，并确认buddy admission failure不触发sweep。fixture只证明owner-local
  protocol，不能写成真实SMP或global allocator evidence。
- 真实frame API KUnit覆盖`alloc_frame()`、`alloc_frame_zeroed()`、`alloc_frames(1)`、单页两种RAII Drop、refcount与
  stats前后恢复；higher-order既有KUnit继续证明原连续分配与统计路径。
- RV64 `SMP = 2` focused case必须通过真实frame API让两个不同logical CPU都执行order-0 allocation / final release，
  并至少覆盖一次CPU A取得的`OwnedFrameHandle`由CPU B执行final Drop；case必须以具名phase、predicate、Event、
  completion或等价owner-visible事实握手，闭合全部worker lifecycle，验证stats恢复，并与源码/owner-local observation
  共同证明两个logical CPU没有magazine slot alias。缺少SMP前提而early return的case不形成该项证据。
- 除上述被测语义就是cross-CPU frame lifecycle的focused case外，普通KUnit不创建kthread，也不得用固定yield /
  schedule、tick或wall-clock sleep伪造并发。BSP串行PASS只作为deterministic regression，不形成cross-CPU proof；
  focused case只证明实际执行的ownership/free路径，不外推全部并发交错。
- RV64与LA64均以default KernelConfig完成release build；至少一个代表性非法capacity / batch配置必须由
  `mm::frame`消费点的编译期predicate拒绝，不能由xtask clamp、fallback或复制同一semantic check。

### Boot smoke

- RV64以`SMP = 2`完成一次KUnit-enabled真实boot，全部registered case通过、两个CPU正常完成boot initialization，
  且上述focused case实际进入两个不同logical CPU的production frame path。
- 同源ordinary non-KUnit RV64 `SMP = 2` kernel启动到init / ordinary userspace并orderly shutdown，无deadlock、panic、
  refcount或accounting assertion failure。
- focused case与boot smoke只证明production装配、AP初始化和已执行的cross-CPU allocation / final-release
  lifecycle可用；它们不声称证明contention、fairness、吞吐或全部并发交错。

### Not Run

除非实际执行，性能A/B、参数调优、allocation stress、full LTP、完整preliminary / final harness、LA64 runtime、
physical hardware、CPU hotplug、NUMA、极端OOM、长时碎片与完整interleaving proof均为Not Run / Not In Target。

`FRAME-MAGAZINE-CUTOVER`只有在上述source review、KUnit、KernelConfig rejection、双架构release build、focused
cross-CPU production path与两次RV64 `SMP = 2` boot闭合，final review无Apollyon / Keter，Architecture Friction Scan
未发现第二份frame membership truth、owner穿透、allocator递归、CPU-ID alias、隐含guard嵌套或validation降级后，
才能原子Introduce `MM-FRAME-001/002`并关闭RFC。

## 风险与反馈

- 若global stats无法在不扫描并嵌套全部magazine / buddy guard的情况下提供coherent snapshot，应停止并重新审查
  accounting owner与linearization；不得先以近似值、buddy-only值或saturating counter完成cutover。
- 若all-magazine sweep需要global quiescence、recovery epoch或新的fast-path shared gate才能保持目标failure语义，
  应先比较其对target并行能力与lock graph的影响，再由RFC review决定；不得在实现中静默增加第二把global fast-path锁。
- 若实现证据表明`npages > 1`现有provenance / accounting缺口阻塞本RFC，只能修复保持既有可见语义所必需的最小
  owner-local问题；任何multi-order cache或新folio ABI都命中停止条件。

## 当前执行事实

- `mm::frame`以cache-padded fixed logical-CPU table拥有order-0 magazine；capacity / batch默认`64 / 16`。local hit、
  partial refill、full drain、all-registered-slot sweep与retry once均已落入统一allocator front，single-page frame / folio
  final Drop共享该route；higher-order继续直接使用buddy。
- global atomic accounting是OOM与memory observer的pressure truth，magazine和operation batch中的free frame仍计入
  `free_pages`；buddy stats只在PMM add-range期间提供初始化增量，不参与runtime placement decision。
- KernelConfig consumer编译期拥有非零、`batch <= capacity`、全机retention checked arithmetic及capacity-sized sweep
  batch不超过一页stack storage的predicate。代表性`capacity = 65536`配置在`mm::frame`断言处按预期编译失败。
- owner-local magazine与detached-backend KUnit、real frame API KUnit及RV64 SMP2 cross-CPU case已落地。最终review关闭
  首轮stack/config与test-oracle问题后为0 Apollyon / 0 Keter / 0 Euclid；Architecture Friction Scan未发现第二份
  membership truth、owner穿透、allocator recursion、CPU-ID alias、guard嵌套、test-only production shaping或validation
  oracle降低。
- `just test xtask`为`108/108`；final source完成RV64与LA64 default KernelConfig release build。此前同一production
  实现的RV64 `SMP = 2` KUnit boot为`682/682`，focused case实际执行`source=0 target=1`并orderly shutdown；ordinary
  non-KUnit `SMP = 2` boot进入userspace并orderly shutdown。最终新增compile-time bound与收紧test setup后，按维护者
  指示未重跑QEMU；这些微调不改变production allocation / release path。
- `MM-FRAME-001/002`已由[`Frame Allocation当前契约`](../../contracts/mm/frame-allocation.md)原子Introduce，
  `FRAME-MAGAZINE-CUTOVER`完成。RFC目录仍只有本`index.md`，没有supporting page或transaction。
- 性能A/B、参数调优、allocation stress、full LTP、完整preliminary / final harness、LA64 runtime、physical hardware、
  CPU hotplug、NUMA、极端OOM、长时碎片与完整interleaving proof均Not Run / Not In Target。

## 修订记录

| 修订 | 日期 | 语义变化 | Review / Evidence |
| --- | --- | --- | --- |
| R0 | 2026-08-16 | 接受首版frame order-0 magazine target、owner/handoff/accounting边界、admitted buddy miss recovery、focused cross-CPU proof、双架构build floor与pending `MM-FRAME-001/002` delta。 | 维护者接受；RFC review为0 Apollyon / 0 Keter；`git diff --check`；`mdbook build docs` |

## Closure

2026-08-16 Closed。唯一`FRAME-MAGAZINE-CUTOVER`原子完成order-0 current-CPU magazine、bounded batch handoff、
admitted buddy miss sweep/retry once、global pressure accounting、KernelConfig predicate、owner-local与production-path
validation，并Introduce `MM-FRAME-001/002`。实现保持`mm::frame`单一owner、既有public Rust API / user ABI、RAII /
refcount、higher-order contiguous allocation、OOM policy与percpu owner surface。

独立final review为0 Apollyon / 0 Keter / 0 Euclid；无未决Architecture Friction finding。残余风险限于本target明确排除
且未执行的性能、压力、完整harness、LA64 runtime、hardware、hotplug / NUMA、极端OOM、长时碎片与完整并发交错，
以及最终test/config-only微调后未重跑QEMU这一已披露的验证时点差异。Closed后本RFC冻结为历史资料；后续工作从
live source、current contract与register重新分类。
