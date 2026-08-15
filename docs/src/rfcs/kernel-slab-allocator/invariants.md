# Kernel Slab Allocator 目标与不变量

**状态：** R0 Closed / Effective via `KERNEL-SLAB-CUTOVER`
**最后更新：** 2026-08-15
**父 RFC：** [RFC-20260815-kernel-slab-allocator](./index.md)
**适用修订：** R0

本文保存父RFC R0的历史target与contract proof obligations。`KERNEL-SLAB-CUTOVER`后，当前effective规则以
[Kernel Heap current contract](../../contracts/mm/kernel-heap.md)及live source为准。

## 规则分类

- **Correctness Invariant：** free-object唯一owner、allocation/deallocation memory safety、span publication、
  boot/runtime domain、锁序、IRQ/reentry、failure与cleanup；不能通过内存充足或性能收益降低。
- **Target Guarantee / Capability：** eligible small-object local hit不碰shared lock、任意CPU释放、bounded passive
  central exchange、Talc fallback与明确的no-reclaim边界；改变时必须Target Renegotiation。
- **Implementation Preference：** 类型、helper、文件布局、intrusive node表示、class数量、具体阈值、central lock
  sharding、batch表示与span数值；只要保持全部target/proof obligation，可以在实现内自然决定。

## Target Invariants

### MM-KMALLOC-001 — 唯一kernel heap domain与backing handoff

**规则：** `mm::kmalloc`唯一拥有每次合法`Layout`的small slab或Talc route，以及small-object boot/runtime同域、
private span publication、large fallback与allocation failure handoff。满足slab class的layout从第一次可发生kernel
allocation起就只由slab domain分配；per-CPU local capability不可用时使用central path，不得临时改走Talc并在随后
仅凭layout猜测allocation origin。

Talc继续拥有尚未交给slab的bootstrap / extended arena和全部large / exceptional-layout allocation。一次完整span
成功返回后，operation在private state完成class校验与slot carve；publication是该span永久转交目标slab class的唯一
commit。published span不返回Talc/frame、不改变class，也不创建独立frame accounting。

**Owner：** `mm::kmalloc`；Talc与frame allocator分别只拥有handoff前backing和物理页/accounting。

**依赖：** `KCONFIG-VALIDATION-001`、`MM-OOM-001`。

**违反表现：** 同一small layout在boot与runtime产生Talc/slab混合origin；deallocation依赖不可恢复的运行阶段；
Talc与slab同时把同一span视为free；span发布后改变class或归还backing；frame allocator保存slab private state；
allocation failure在持kmalloc guard时进入panic/log allocation递归。

**Cutover / Proof：** `KERNEL-SLAB-CUTOVER`；全部route/source audit、owner-local KUnit、双架构boot/KUnit及双架构
cross-CPU userspace smoke。

### MM-KMALLOC-002 — Current-CPU small-object fast path与有界滞留

**规则：** 当前CPU local capability可用且对应class有free object时，small allocation / deallocation只在当前CPU的
短noirq local transaction内完成，不获取shared central/Talc/frame lock，不读取其它CPU cache，也不调用可能分配、
阻塞、reclaim、格式化日志或执行callback的代码。

合法small object可以在任意CPU释放，并进入执行deallocation的当前CPU cache；allocator不保存allocation CPU、
span `owner_cpu`或remote-free state。local cache按class有固定build-time上界，只在empty/full边界与central domain
交换有界batch。低于上界的remote CPU free capacity不被steal，published span不reclaim；这些滞留规则是target
capability边界，不是遗漏的cleanup。

**Owner：** `mm::kmalloc` local cache / central class domain；`percpu`只提供当前CPU local capability。

**依赖：** `MM-KMALLOC-001` target、KUnit execution/proof current contract。

**违反表现：** local hit取得任意shared/global lock；free读取origin CPU并修改remote cache；cache容量由allocator偶然
失败决定；background worker主动均衡；full/empty路径静默越过central直接逐对象调用Talc；用bitmap/free count复制
free-list行为truth；把single-CPU KUnit报告为SMP proof。

**Cutover / Proof：** `KERNEL-SLAB-CUTOVER`；local-hit call graph、lock/IRQ review、KUnit object-conservation proof及
双架构`smp > 1` cross-CPU ownership/free runtime。

## Free Object 唯一所有权

每个slab slot在任意完整transaction边界恰处于以下一种状态：

```text
PrivateUnpublished
Allocated
LocalFree(cpu, class)
CentralFree(class)
```

- `PrivateUnpublished`只存在于成功取得但尚未commit的完整span；该operation唯一拥有全部slots。
- `Allocated` slot不属于任何free structure；allocator不额外保存可独立修改的allocated bit或owner CPU。
- `LocalFree` slot只属于一个current-CPU class cache；同一slot不得同时出现在另一个CPU或central。
- `CentralFree` slot只属于一个shared class domain；任何CPU都可以通过bounded refill取得。

允许实现使用free slot自身存储intrusive link，但该表示只是上述唯一free ownership的物理载体。若实现选择额外metadata，
它只能编码publication/class/lifetime所需事实，不能与free structure形成两份都可驱动allocation的truth。

完整转换只有：

```text
PrivateUnpublished -> Allocated + LocalFree/CentralFree
LocalFree           -> Allocated
CentralFree         -> Allocated
Allocated           -> LocalFree/CentralFree
LocalFree           -> CentralFree       (bounded drain)
CentralFree         -> LocalFree         (bounded refill)
```

首版不存在`* -> TalcFree`、跨class转换、remote CPU direct mutation或span retirement。违反`GlobalAlloc` safety
contract的double free / wrong layout不要求target提供recoverable outcome，但合法调用不得因并发或内部handoff产生
duplicate、lost slot或use-after-free。

## Layout 与 Class

一次dispatch必须从完整`Layout`同时考虑size与alignment：

- class slot size必须不小于有效payload、intrusive free representation所需最小尺寸及requested alignment；
- class base与每个slot地址都必须满足原layout alignment；不能只按size选择class再静默返回低对齐pointer；
- class arithmetic、round-up、span metadata/slot起点、slot count和address end必须checked，不能wrap或saturate后继续；
- 超过slab object upper bound、alignment不能由class/span自然满足或其它明确不适用layout必须走Talc；
- 对同一合法layout，allocation和matching deallocation必须得到同一route，且不依赖CPU、cache occupancy或boot phase。

具体class count、power-of-two policy和small upper bound属于implementation preference；一旦作为重要capacity/policy
选定，值进入KernelConfig或由更少的KernelConfig值纯派生，不散落为多份magic constants。

## Local Transaction 与 IRQ

local cache只允许由当前CPU访问。local mutation必须形成不可被同CPU hard IRQ重入、也不可迁移到其它CPU的短transaction；
普通`PerCpu::with_mut()`仅关闭preemption是否足够，必须由实现结合实际hard-IRQ allocation caller证明，不能仅因类型名
包含per-CPU就假设安全。

local guard内只允许：

- 读取/写入当前class free representation和有界count；
- pop/push一个slot；
- detach或attach一个已形成的有界batch；
- 常开、O(1)或与固定batch上界成比例的局部correctness assertion。

local guard内禁止取得central/Talc/frame/ordinary mutex，禁止span allocation/carve、日志格式化、callback、Drop复杂对象、
blocking wait或synchronous reclaim。miss/full必须先完成local状态转换并释放guard，再进入下一个owner。

## Central Batch Handoff

central domain只拥有按class可全局复用的free objects，不拥有per-CPU cache或Talc internals。实现可以按class shard lock，
也可以使用其它直接同步表示；无论形状如何，必须满足：

- refill / drain在central guard内只执行有界pointer/list transfer和local correctness checks；
- 不在central guard内循环调用Talc alloc/free、逐对象buddy/Talc algorithm、frame allocation或span carve；
- central empty observation不是永久reservation。释放guard后申请span期间其它CPU可以先行发布capacity；并发额外span
  可以被接受，但每个operation至多发布自己唯一取得的完整span，且growth不能形成无界retry storm；
- refill取得的slot在central owner撤销后才可发布给local；drain在local owner撤销后才可发布给central；中间batch由当前
  operation唯一拥有，不通过第二份membership flag驱动行为。

batch大小、cache high-water与central lock sharding属于implementation preference；它们的build-time关系必须使任何
detach/attach临时存储和循环有明确上界。

## Span Publication 与 Failure

一次新span路径按以下顺序闭合：

1. local与central guard均已释放；
2. Talc按完整span layout返回allocation，或失败返回null；
3. successful allocation保持private，完成全部class、alignment、slot count和checked arithmetic校验；
4. private carve形成互不重叠的slot ownership；
5. 一次commit把一个slot交给当前allocation，其余slot交给local或central；
6. commit后span永久属于该class，不存在rollback或Talc deallocation。

任何publication前失败必须保持shared state不变，并按自然owner cleanup处理尚未commit的backing。若实现无法在不调用
allocator的cleanup路径中安全返回private Talc allocation，必须调整prepare/commit形状；不得泄漏半初始化span、把失败
变成success或在local / central guard内panic后遗留published chain。

当前infallible kernel allocation的最终OOM可以kernel-fatal，但`GlobalAlloc::alloc`自身必须在释放kmalloc guards后
返回null，使上层failure path不会递归等待本次持有的allocator lock。本RFC不引入errno、pressure wake、reclaim callback
或另一个OOM state owner。

## Boot 与 Per-CPU Readiness

slab central domain必须在任何可能的small kernel allocation前可用，且不依赖已经初始化的per-CPU storage。当前CPU local
capability不可用时，small alloc/free直接使用central domain；一旦当前CPU满足canonical per-CPU readiness，后续operation
可以使用local cache。

readiness必须由percpu/boot owner的canonical事实派生，或通过一次明确的boot handoff提交；不得在kmalloc与percpu分别
保存可独立变化的ready truth。若采用一次global enable，source proof必须确认所有AP在任何allocator caller前已经完成
自己的per-CPU initialization；否则必须使用current-CPU-specific capability，而不是依赖偶然boot顺序。

无论readiness如何变化，同一small layout始终属于slab domain，因此早期central allocation可在运行期释放到current CPU
local cache。不得以early Talc allocation加runtime pointer猜测、地址区间heuristic或永不释放假设建立兼容桥。

## Memory Retention 与 Cleanup Boundary

首版memory model为：

```text
per-class retained spans ~= that class historical high-water backing
per-CPU stranded free     <= CPUs * classes * configured local bound
```

published span不返回Talc/frame，free objects只在同class复用。central free capacity对全部CPU可用；低于local上界的
capacity允许暂时滞留，不提供fairness或availability deadline。kernel shutdown不drain local cache，因为整个heap与
backing同为kernel-lifetime owner。

这些规则不允许真实slot泄漏出`Allocated/LocalFree/CentralFree` union，也不允许unbounded local growth；它们只接受
跨class reuse与物理页回收能力缺席。若真实验收因class-stranded memory失败，必须Target Renegotiation，不得在当前target
内静默增加bitmap、span owner、remote-free或shrinker。

## Kconfig 不变量

重要slab参数由KernelConfig选择，`mm::kmalloc` consumer在编译期至少闭合实际实现需要的：

- span size非零、满足page/backing alignment且能自然容纳metadata与至少一个最大class slot；
- minimum class可承载free representation，maximum class与span/align policy一致；
- local capacity、refill / drain batch非零且彼此关系有效，所有临时batch storage有固定上界；
- `CPU count * class count * local capacity * class size`等验收所需上界计算不溢出；
- bootstrap backing能够满足首个合法span acquisition；若该predicate依赖真实boot layout，必须由对应owner共同证明，
  不能由xtask clamp到另一数值。

xtask只传输resolved values；不得复制上述predicate、修正非法值或在runtime fallback到另一套cache/span policy。

## Validation Proof Obligations

### Source review

- 完整`GlobalAlloc` route、boot call order、per-CPU readiness、hard IRQ/IRQ-off callers与Talc/frame handoff；
- free ownership union、batch ownership transfer、span prepare/commit/failure、large fallback与OOM guard release；
- local / central / Talc / frame allocator lock graph；local / central guard内无复杂allocation、reclaim、ordinary lock、
  callback或日志格式化。Talc guard内既有`Talc -> frame allocator` backing / OOM edge单独审计，不受本条local / central
  guard禁止项约束，也不得被slab改造成新的反向lock edge或第二份failure owner；
- production无KUnit hook/probe、remote cache mutation、diagnostic state驱动behavior或validation-only public API。

### KUnit

- 真实kmalloc owner helper覆盖layout/class/alignment、span carve、slot唯一性、cache empty/full、refill/drain conservation、
  private publication与failure、large fallback和Kconfig边界；
- case返回前不遗留修改后的global cache、span、thread或request；需要修改global allocator state的case必须使用明确fixture
  和cleanup，不能依赖case顺序；
- 普通serial BSP KUnit只报告实际architecture/topology/path。没有真实进入SMP path的case不得记为SMP通过。

### Runtime

- RV64与LA64 ordinary/release build和实际KUnit boot；
- RV64与LA64分别运行`smp > 1`有界userspace并发smoke，以affinity或等价owner-visible证据证明至少两个不同online CPU
  实际执行目标workload；每个架构至少有一次由CPU A触发object取得/shared ownership建立、由不同CPU B完成final
  close / drop或等价最终释放的合法cross-CPU路径；
- cross-CPU oracle不得依赖production KUnit hook、allocator pause point、allocation CPU字段或第二份owner truth；
- 对closure实际使用的每份tracked KernelConfig记录每CPU及全机最坏stranded-free上界与bootstrap首span headroom，并由
  维护者在final review确认可接受；
- 保存完整日志并记录actual feature、CPU identity/topology、目标操作顺序、退出、shutdown边界与Not Run，使维护者能够
  结合上下文直接判断结果；配套runner只编排repository-owned flow、保存日志并传播host command status，不得以硬编码
  expected string作为guest语义oracle；不从另一architecture、SMP=1、仅配置多CPU但worker未跨CPU执行或external
  model外推。

性能A/B未进入本次closure。cutover前它可以触发保持target、缩小eligible class、Target Renegotiation或Not Cut Over，
但不得替代上述correctness proof，也不能把混合其它MM优化的结果全部归因于slab；RFC Closed后的性能工作另立边界。

## 禁止退化项

- local hit重新进入任何shared global lock或Talc/frame path；
- 为span reclaim预建`owner_cpu`、remote-free queue、bitmap/free count、后台worker或CPU stealing；
- 以large cache掩盖central guard内逐对象backing allocation或长扫描；
- 让frame allocator、scheduler、caller或通用PerCpu API理解slab private class/cache/span representation；
- 让boot与runtime对同一small layout产生不可判定origin，或用地址heuristic、silent fallback、永不释放假设兜底；
- KUnit production hook、pause point、test-only state carrier或固定yield/sleep并发oracle；
- 用性能结果降低slot唯一owner、memory safety、failure cleanup、lock/IRQ correctness或双架构SMP validation；
- 在`KERNEL-SLAB-CUTOVER`前把`MM-KMALLOC-001/002`写入current contract或宣称effective。
