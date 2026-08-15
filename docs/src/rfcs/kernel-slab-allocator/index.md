# RFC-20260815-kernel-slab-allocator

**状态：** Accepted
**修订：** R0
**负责人：** doruche
**最后更新：** 2026-08-15
**领域：** mm / kmalloc / kernel heap / SMP
**影响契约：** `MM-KMALLOC-001`、`MM-KMALLOC-002`（均为 pending Introduce）
**执行记录：** None

## 摘要

本 RFC 在 `mm::kmalloc` owner 内为 kernel heap 引入按 size class 切分的 slab 小对象路径，并以有界
per-CPU cache 消除稳态小对象 `alloc` / `dealloc` 对共享全局锁的依赖。Talc 不被整体删除：它继续承担
bootstrap backing、slab span backing，以及大对象和特殊对齐请求的通用慢路径。

首版只解决 SMP kernel heap common path。它不实现主动负载均衡、remote-free queue、span reclaim、
slab shrink、NUMA 或 CPU hotplug。释放可以发生在任意 CPU；满足 slab class 的对象进入执行释放的当前
CPU cache，并只通过有界 refill / drain 与 shared central class domain 被动交换。该取舍接受按 size class
保留历史峰值内存和有界 per-CPU 滞留，以避免为当前不需要的低内存回收能力引入 span owner、remote mutation、
bitmap accounting 与额外生命周期协议。

本 RFC 的性能目标是结构性的：小对象 local-cache hit 不得获取共享全局锁或进入 backing allocator。数值
性能比较由维护者另行完成，不设置吞吐或加速比 closure 门槛；没有实测证据时不得宣称具体性能收益。

## 背景

当前 `KernelAllocator` 以一把 `NoIrqSpinLock<Talc<HeapOomHandler>>` 串行化全部 kernel heap allocation 与
deallocation。Talc 首次使用时 claim bootstrap heap，后续扩容从 frame allocator取得 folio，并在 claim
成功后把整段内存永久交给 kernel heap。该实现具有单一 owner 与直接失败边界，但所有 CPU 的 Box、Arc、Vec
及其它 ordinary kernel object 都共享同一锁。

已有外部实现经验表明，只在 per-CPU magazine miss 时持全局锁逐个执行一批通用 heap allocation，仍可能把
refill / drain 变成长共享临界区。本 RFC 只吸收其中可由 Anemone live owner独立验证的结构结论：global
backing 一次交接完整 span，slot carve 在尚未发布的私有 span 上完成，local / central 只交换已经形成的
同 class free objects。外部报告不定义 Anemone target，也不证明本 RFC 的性能收益。

当前 [MM current contract](../../contracts/mm/index.md) 只覆盖 user TLB completion、frame pressure sampling与
OOM policy，没有 kernel heap operational contract。本 RFC 计划在唯一 cutover 时引入最小 `MM-KMALLOC-*`
规则；Draft / Accepted target 在 cutover 前不覆盖 live implementation 或 current contract。

## 目标

- `mm::kmalloc` 继续作为 kernel global allocator、small / large layout dispatch、slab cache、central class、
  span handoff 与 allocation failure 的唯一 protocol owner。
- 对满足 slab class 的小对象，per-CPU local-cache hit 在当前 CPU 的短 noirq transaction 内完成，不读取或
  修改 shared central state，不获取 Talc / frame allocator，也不访问其它 CPU cache。
- 小对象在 per-CPU cache 尚不可用的启动阶段也属于 slab allocation domain，只使用 central path；运行阶段
  local cache 可用后，早期对象与后续对象仍按同一 slab class 规则释放，不形成 Talc / slab mixed-origin
  ambiguity。
- 任意 CPU 可以释放合法 slab 对象。普通释放进入当前 CPU 的对应 local cache，不保存 allocation CPU、
  `owner_cpu` 或 remote-free state，也不要求返回原 CPU。
- local cache 使用固定 build-time 上界。cache empty 时从 central class 取得有界批次，超过上界时向 central
  class 归还有界批次；不从其它 CPU 偷取低于上界的保留对象。
- central refill / drain 只做有界 free-object chain transfer。central class 没有可用对象时，不持 local / central
  guard向 Talc取得一个完整、符合对齐要求的 span；span 在私有状态完成 slot carve 后才原子发布。
- 大于 slab 上限、无法由 class size 满足对齐或其它明确不适用的 `Layout` 继续走 Talc；其成功、释放和
  allocation failure 语义不因小对象路径改变。
- slab span 一旦从 Talc 成功取得并发布，即在 kernel lifetime 内固定服务一个 size class，不返回 Talc / frame
  allocator，也不跨 class 复用。所有已释放 slot仍可由同 class 的 local / central path重用。
- span size、slab object upper bound、local cache capacity与batch上界等重要 build policy进入KernelConfig；
  `mm::kmalloc` 在消费点拥有并以编译期检查执行其非零、大小、对齐、幂次、容量关系与算术安全语义。
- 以源码审查、owner-local KUnit、双架构真实 boot/KUnit 与双架构 `smp > 1` 用户态 cross-CPU smoke 证明
  正确性与production-path integration；普通单核KUnit或仅配置多个CPU但未实际跨CPU执行的boot不外推为SMP proof。

## 非目标

- 完全移除或重写 Talc、改变 frame allocator / buddy algorithm、把 slab 直接变成新的物理页 owner；
- 数值性能门槛、自动 benchmark gate、在本 RFC closure 中归因具体加速比，或把外部结果外推到 Anemone；
- span empty detection、span reclaim、slab shrink、跨 size-class reuse、generic reclaim、swap、memcg或OOM reaper；
- span `owner_cpu`、remote-free queue、remote per-CPU mutation、CPU stealing、后台rebalancer、NUMA、CPU hotplug或
  offline drain；
- 为 invalid pointer、错误 `Layout`、double free 或其它违反 `GlobalAlloc` unsafe caller contract的行为建立完整
  runtime diagnostic / recovery协议；
- 修改 scheduler placement、task migration或通用 `PerCpu` remote mutation API；
- 关闭 [`ANE-20260622-IRQ-OFF-HEAP-ALLOCATION`](../../register/open-issues.md#ane-20260622-irq-off-heap-allocation)
  或 [`ANE-20260713-SCHED-RT-NOIRQ-BUCKET-ALLOCATION`](../../register/current-limitations.md#ane-20260713-sched-rt-noirq-bucket-allocation)；
  本 RFC 只保持 allocator自身简单、有界、不可阻塞的 noirq边界，不替代 caller side-effect审计或 RT queue 表示改造；
- full LTP、完整 preliminary / final harness、physical hardware、极端 OOM、长时内存碎片或低内存回收证明。

## Owner 与协议边界

### 状态与能力所有权

- **Kernel allocator owner：** `mm::kmalloc` 唯一拥有 layout classification、small / large dispatch、local cache、
  central class、private span publication与 allocator failure handoff。其它 subsystem只消费 `GlobalAlloc` 行为，
  不读取 slab private representation。
- **Per-CPU capability：** `percpu` 继续拥有 current CPU identity、local storage availability与访问约束。
  `kmalloc` 只消费当前 CPU local capability；不得为了 slab 增加 generic remote mutable access或复制另一份可独立
  变化的 CPU-ready truth。
- **Backing owner：** Talc在成功返回 span前拥有 backing allocation；成功返回后由 `kmalloc` 私有切分并在
  publication时永久转交给目标 slab class。大对象仍由Talc完整拥有直到匹配 deallocation。
- **Physical memory owner：** frame allocator继续拥有物理页分配与 `FrameAllocatorStats` truth；slab不读取frame
  private state，也不把class retention投影成另一份frame accounting。
- **Configuration owner：** KernelConfig拥有选定值，`mm::kmalloc`拥有这些值的semantic predicate。生成器只忠实
  传输，不clamp、fallback或复制consumer predicate。

### Allocation handoff

1. `KernelAllocator`只读取一次完整 `Layout`并决定slab class或Talc fallback；class必须同时满足size与alignment。
2. 当前CPU local capability可用时，先在短 noirq local transaction尝试pop。hit在此结束。
3. miss先释放local guard，再从对应central class取得有界batch；不得持local guard进入shared或backing owner。
4. central empty时先释放central guard，再向Talc申请一个完整span。span未发布期间只由当前operation拥有；slot
   carve、chain formation与全部可预判校验在private state完成。
5. publication把已完成的同class free-object chain一次交给local或central owner。并发grow可以产生额外完整span，
   但不得重复发布同一slot、覆盖已发布span或借此形成无上界retry / growth loop。

per-CPU尚不可用时跳过local步骤并使用同一central / span path。该降级只由canonical boot/per-CPU readiness决定，
不是runtime性能策略或可反复切换的behavioral mode。

### Deallocation handoff

合法small-object deallocation按原始匹配 `Layout`取得同一class。local capability可用时，slot进入当前CPU cache；
不可用时直接进入central class。local超过上界时，先在local transaction内detach有界batch，释放local guard后再
提交central。slot不返回allocation CPU，也不修改任何remote cache。

大对象和不适用slab的layout继续以匹配layout返回Talc。small / large route不得因运行阶段、CPU identity、cache
容量或allocation失败而对同一个layout静默改变，从而避免释放时无法确定owner。

### Failure 与 cleanup

- Talc span allocation失败时返回当前global allocation failure结果；所有kmalloc guard必须先释放，private state不得
  发布，已经取得但尚未commit的资源按自然RAII / owner-local cleanup返回。
- span publication之后没有rollback、shrink或shutdown drain；span与其中的free slots归kernel heap process lifetime
  所有。这是accepted target boundary，不是暂时等待补齐的cleanup缺口。
- cache batch transfer必须保持对象守恒；失败或early return不得让slot同时出现在两个free domain，也不得让已发布
  slot脱离所有allocated / free owner。
- ordinary allocation failure继续遵循当前kernel-fatal边界；本RFC不为既有infallible collection引入新的errno、
  reclaim callback或caller rollback协议。

## ABI 与可见语义

本 RFC 不改变syscall ABI、errno、用户地址空间或userspace allocator语义。它保持Rust `GlobalAlloc`要求的合法
layout对齐、非重叠live allocation、匹配deallocation与null-on-allocation-failure边界。

外部可观察差异仅限kernel heap并发进展、内存保留与极端OOM时点：slab按size class保留历史峰值span，并允许每CPU
每class滞留至配置上限的free objects，因此可比Talc更早耗尽尚未被其它class消费的backing。该差异属于本RFC明确
接受的能力边界；不得把它写成完整low-memory robustness、reclaim或内存效率保证。

## Contract Impact

| Contract ID | 变化 | 当前规则 | Target 摘要 | Cutover |
| --- | --- | --- | --- | --- |
| `MM-KMALLOC-001` | Introduce | None（尚未生效；live baseline为单一global Talc lock） | `mm::kmalloc`唯一拥有layout dispatch、boot/runtime同域、Talc backing handoff与失败/cleanup；small slab与large fallback保持可判定 | `KERNEL-SLAB-CUTOVER` |
| `MM-KMALLOC-002` | Introduce | None（尚未生效） | eligible small-object hit在current CPU local noirq path完成；任意CPU free进入当前CPU cache，bounded central exchange，不提供主动均衡或span reclaim | `KERNEL-SLAB-CUTOVER` |

### Dependencies

- [`KCONFIG-VALIDATION-001`](../../contracts/configuration/kernel-parameter-validation.md#kconfig-validation-001--kernel-consumer-唯一定义参数语义合法性)：
  kmalloc consumer拥有新增参数的semantic predicate。
- [`KUNIT-EXEC` / `KUNIT-CONCURRENCY` / `KUNIT-PROOF` / `KUNIT-SHAPE`](../../contracts/kunit/execution-and-proof.md)：
  KUnit执行、并发准入、证明外推与conditional code shape。
- [`MM-OOM-001`](../../contracts/mm/oom-policy.md#mm-oom-001--oom-worker自有fixed-delay采样与victim-round)：
  frame pressure仍由periodic OOM owner读取live frame stats；kmalloc allocation edge不引入pressure hint或wake truth。

## Implementation Boundary

- **允许改变：** `mm::kmalloc` 的small-object implementation、GlobalAlloc内部dispatch、bootstrap/runtime slab
  handoff、Talc backing integration、同owner模块拆分、owner-local tests，以及上述重要参数的KernelConfig传输与
  kmalloc-side semantic validation。
- **必须保持：** syscall ABI、Rust `GlobalAlloc`合法调用语义、frame allocator owner/accounting、OOM periodic
  policy、Talc large / exceptional-layout fallback、现有noirq可调用边界，以及percpu / scheduler的public owner surface。
- **实现提示：** `kmalloc`可以按allocator front、slab、backing等稳定角色做同owner目录化；intrusive free list、
  class数量、具体阈值、central lock sharding与batch表示属于implementation preference，不形成逐文件write set或public API。
- **停止条件：** 若实现需要让frame allocator感知slab class、增加remote per-CPU mutation、建立span reclaim / active
  rebalancing、无法让早期与运行期small allocation保持同一可判定owner、改变OOM/failure语义、扩大public API / shared
  contract、降低双架构SMP validation，或引入probe、不安全中间态、正式多阶段/多个cutover，必须在实现或cutover前
  停止并回到RFC review / Target Renegotiation。

实现按一个连续implementation unit推进，普通commit与owner内部工作片不构成独立gate，唯一语义发布点仍为
`KERNEL-SLAB-CUTOVER`。推荐依赖顺序为：先闭合layout route、boot/per-CPU readiness、lock graph与Kconfig predicate，
再完成owner-local基础表示和确定性测试；随后接通central/Talc backing与boot-safe路径，在该基础上接入per-CPU local
path；最后收集双架构source/KUnit/build/runtime证据并进行final review与单一cutover。这只是依赖顺序，不冻结具体类型、
模块布局、参数数值、内部算法、命令、commit顺序或validation harness；只要保持Implementation Boundary与证明义务，
实现可合并或重排内部工作。若实际工作要求probe、不安全中间态、正式stage或多个cutover，则命中上述停止条件并回到
RFC review，而不是在实现中自行扩展路线。

R0只接受本文target、non-goals、owner/handoff、failure/cleanup、Contract Impact与validation boundary；状态本身不授权
实现、当前契约更新或`KERNEL-SLAB-CUTOVER`。进入实现需要维护者另行明确授权。

## Acceptance 与 Validation

### 源码与模型证明

- 审计全部`GlobalAlloc` small / large route、size / align arithmetic、bootstrap readiness、Talc span handoff、
  deallocation owner与failure cleanup；确认同一合法layout不会产生不可判定mixed-origin。
- 建立local、central、Talc与frame allocator lock graph；确认local hit无shared lock，slow path不嵌套local / central /
  Talc guard。这里的guard内禁止项只指local / central guard：其中不得执行逐对象backing allocation、blocking/reclaim、
  普通lock、日志格式化或callback。Talc guard内既有`Talc -> frame allocator` backing / OOM edge另行审计，不把它误判为
  central violation，也不借slab静默改变large / exceptional-layout fallback。
- 审计hard IRQ、IRQ-off return tail、scheduler noirq与ordinary process callers，确认allocator自身没有扩大当前允许的
  simple bounded noirq allocation边界；该审计不关闭caller side-effect register issue。
- 审计free-object ownership与span publication，确认每个slot恰为allocated，或只属于一个local / central free domain；
  不存在bitmap、free count、diagnostic owner或Talc mirror反向形成第二份行为truth。

### KUnit与build

- owner-local inline KUnit覆盖layout到class映射、alignment、span carve无重叠、local / central refill-drain对象守恒、
  cache empty/full、private span publication、allocation failure不发布、large/special alignment fallback与重要参数边界。
- 普通KUnit不通过固定yield、schedule、tick或wall-clock sleep伪造SMP proof；若确有live scheduling case，必须证明
  allocator并发本身无法由deterministic owner-local test覆盖，并使用production lifecycle、显式phase/predicate与join闭合。
- 使用repository build wrapper完成RV64与LA64 ordinary/release build及真实KUnit boot；记录实际feature、CPU topology、
  case count与Not Run，不把single-CPU PASS外推为SMP证据。

### Runtime

- RV64与LA64分别以`smp > 1`启动真实kernel，运行有界userspace并发smoke；必须以affinity或等价owner-visible证据证明
  至少两个不同online CPU实际执行目标workload，不能只记录配置topology或依赖scheduler偶然placement。
- 每个架构至少覆盖一次合法cross-CPU ownership/free路径：由CPU A上的执行者触发kernel object取得或shared ownership
  建立，再由不同CPU B上的执行者完成final close / drop或等价最终释放。具体harness与observer属于implementation
  preference，但不得增加production KUnit hook、allocator pause point或第二份allocation-owner truth。
- smoke必须完成ordinary userspace退出与shutdown，并保存足以让维护者结合上下文判断结果的完整日志，包含实际CPU
  identity/topology、目标操作顺序、退出与shutdown边界。单纯`smp > 1` boot、多个worker仍全部运行在同一CPU或boot后
  立即shutdown均不形成cross-CPU allocator证据。
- 若增加配套验收脚本，脚本只负责编排repository-owned build/rootfs/QEMU flow、保存完整日志并传播host command status；
  不得搜索硬编码expected string来判定guest语义PASS/FAIL。最终验收由维护者直接审阅日志及上述证明上下文完成。
- 对closure实际使用的每份tracked KernelConfig，记录由configured CPU上界、class集合、local capacity与class size推导的
  每CPU及全机最坏stranded-free上界，以及bootstrap backing相对首个合法span及其alignment/metadata的headroom；这些事实
  由维护者在final review确认可接受，但本R0不引入统一数值性能或low-memory门槛。
- 性能A/B由维护者运行并单独保存；本RFC只以source proof确认local hit不触达shared lock，不从未运行或混合改动的
  benchmark推导具体加速比。

### Not Run

除非实际执行，full LTP、完整preliminary/final harness、BuildStorm数值门槛、physical hardware、CPU hotplug、NUMA、
极端OOM、长时内存碎片、span reclaim、低内存跨class再利用与完整interleaving proof均为Not Run / Not In Target。

`KERNEL-SLAB-CUTOVER`只有在上述source、KUnit、双架构build/boot与双架构cross-CPU SMP smoke全部闭合、final review无
Apollyon / Keter、Architecture Friction Scan未发现第二份free truth、owner穿透、remote per-CPU mutation、隐含锁序
或无退出条件bridge后，才能原子Introduce `MM-KMALLOC-001/002`并关闭RFC。

## 风险与反馈

- slab按class永久保留span会降低跨尺寸复用。若实现或验收证据表明contest workload也会因class-stranded memory出现
  allocator failure、不可接受增长或需要回收才能完成smoke，必须带实际峰值与失败证据进入Target Renegotiation；不得
  未经review悄然增加owner CPU、bitmap、remote-free或shrinker。
- tiny layout的class rounding、IRQ guard和local bookkeeping可能抵消单核收益。该结果不改变correctness invariant，
  但维护者性能A/B可以决定保持target、缩窄eligible class或Not Cut Over；agent不能自行用未测猜测修改target。
- bootstrap heap必须自然容纳首个合法span及其alignment/metadata。若配置约束无法在kmalloc owner内编译期闭合，或必须
  改变boot/frame owner才能取得首个span，应停止而不是引入另一套early allocator或不可释放compat bridge。
- 若central batch仍在central guard内执行与batch大小成比例的复杂allocator操作、长扫描或日志，说明实现退化回已拒绝的
  magazine慢路径，应在cutover前修正而不是靠更大cache掩盖。

## 文档与证据

- [目标与不变量](./invariants.md)
- 当前baseline：[MM current contract](../../contracts/mm/index.md)、[KUnit execution and proof](../../contracts/kunit/execution-and-proof.md)、
  [IRQ-off heap allocation open issue](../../register/open-issues.md#ane-20260622-irq-off-heap-allocation)。
- commit / PR / transaction：None。
- 外部源码证据：None；外部比较材料只用于提出候选方向，不定义Anemone target或validation claim。

## 修订记录

| 修订 | 日期 | 语义变化 | Review / Evidence |
| --- | --- | --- | --- |
| R0 | 2026-08-15 | 接受首版kernel slab target、local/central guard边界、cross-CPU ownership/free proof、retention事实验收与pending `MM-KMALLOC-001/002` delta。 | 维护者接受；RFC review；`git diff --check`；`mdbook build docs` |

## Closure

R0 target已接受，但尚未授权或进入实现、runtime validation与contract cutover。`MM-KMALLOC-001/002`仍为pending
RFC target，当前kernel heap与[MM current contract](../../contracts/mm/index.md)保持不变；`KERNEL-SLAB-CUTOVER`
Not Run，RFC未关闭。
