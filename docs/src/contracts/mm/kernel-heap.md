# Kernel Heap 当前契约

**Contract ID：** `MM-KMALLOC`
**状态：** Active
**Owner：** `mm::kmalloc` layout dispatch / slab local-central domain / Talc backing handoff
**参与领域：** per-CPU storage / Talc / frame allocator / KernelConfig
**覆盖范围：** kernel `GlobalAlloc` 的small-object route、boot/runtime同域、span publication、current-CPU cache与bounded central exchange
**不覆盖：** userspace allocator、syscall ABI、generic reclaim、span shrink/reclaim、NUMA、CPU hotplug、remote-free、OOM victim policy或数值性能保证
**实现位置：** `anemone-kernel/src/mm/kmalloc/`、`anemone-kernel/src/percpu.rs`、`conf/kconfs/`、`scripts/xtask/src/config/kconfig.rs`
**依赖：** `KCONFIG-VALIDATION-001`、`KUNIT-EXEC`、`KUNIT-CONCURRENCY`、`KUNIT-PROOF`、`KUNIT-SHAPE`、`MM-OOM-001`
**Pending Successor：** None
**最后核验：** 2026-08-15

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| small / large layout route | `mm::kmalloc` | caller只提交完整合法`Layout` | allocation与matching deallocation选择同一domain |
| current-CPU class free list | `mm::kmalloc` local cache | `percpu`只提供当前CPU local capability | 完成small-object steady-state hit与bounded retention |
| shared class free list | `mm::kmalloc` central class | local transaction只移交detached batch | 在CPU间被动交换同class free slots |
| unpublished slab span | 当前grow operation | Talc已撤销该allocation，central/local尚不可见 | 私有校验、slot carve与publication前cleanup |
| published slab span | `mm::kmalloc`目标class | Talc/frame不保存class或free mirror | kernel lifetime内固定服务一个class |
| backing arena与large allocation | Talc | `mm::kmalloc`只调用窄alloc/free边界 | bootstrap、span backing与large/special-layout fallback |
| physical pages与global frame accounting | frame allocator | Talc只在claim成功后接收folio | 物理内存分配与pressure truth |
| per-CPU storage publication | `percpu` / architecture boot order | `kmalloc`读取一次性ready capability | ready前走central，ready后允许current-CPU local path |

free slot自身的intrusive link是local、central或operation batch membership的唯一物理表示。精确长度与tail只在同一
list owner下作为有界transfer和O(1) splice索引维护，不构成span accounting、allocated bitmap或另一份free owner。

## MM-KMALLOC-001 — 唯一kernel heap domain与backing handoff

**规则：** `mm::kmalloc`必须从完整`Layout`确定small slab或Talc fallback；allocation与matching deallocation使用同一
确定性predicate，不能依赖CPU、cache occupancy或boot phase。eligible small layout从最早可能的kernel allocation起
只属于slab domain：per-CPU storage尚不可用时使用central path，不能临时走Talc再凭地址或阶段猜测origin。large、
超过class alignment能力及其它special layout继续完整归Talc所有。

slab grow只有在释放local与central guard后才能向Talc申请完整span。成功返回的span先由当前operation私有拥有，完成
alignment、class、slot count与checked arithmetic验证并形成不重叠slots；publication是把一个slot交给当前allocation、
其余slots一次交给目标central class的唯一commit。publication前失败不得改变shared slab state，并由当前owner把backing
归还Talc；publication后span永久属于该class，不返回Talc/frame、不跨class复用，也不建立独立frame accounting。

Talc allocation failure只在Talc guard释放后返回global allocation failure结果；本contract不增加reclaim callback、
errno或第二个failure owner。

**违反表现：** 同一small layout产生Talc/slab mixed origin；deallocation需要地址或boot阶段猜测；同一span同时归
Talc与slab所有；private carve未完成即发布；failure在持kmalloc guard时进入可能递归分配的诊断；published span被归还、
改class或投影为frame allocator私有状态。

**验证 / Enforcement：** layout/class、alignment、span carve、private failure与publication owner-local KUnit；
GlobalAlloc、boot/AP order、Talc/frame handoff与failure source audit；非法bootstrap配置在xtask传输完成后由
`mm::kmalloc::slab`编译期predicate拒绝；RV64 `646/646`、LA64 `649/649` exact-source KUnit boot及双架构release build。

**最初来源：** [Kernel Slab Allocator RFC](../../rfcs/kernel-slab-allocator/index.md)；2026-08-15
`KERNEL-SLAB-CUTOVER`。

**当前来源：** [Kernel Slab Allocator RFC](../../rfcs/kernel-slab-allocator/index.md)；2026-08-15 closure提交。

## MM-KMALLOC-002 — Current-CPU small-object fast path与有界滞留

**规则：** current-CPU local capability可用且目标class命中时，small allocation只在当前CPU的短noirq transaction内
完成，不获取central、Talc、frame或其它shared lock，也不执行blocking、reclaim、callback或日志格式化。合法small
object可以在任意CPU释放，并进入执行deallocation的当前CPU cache；allocator不保存allocation CPU、`owner_cpu`、
remote-free queue或remote mutable cache capability。

每CPU每class local cache具有build-time固定容量，只在empty/full边界与对应central class交换固定上界batch。完整owner
转换为`Allocated <-> LocalFree(cpu,class) <-> operation batch <-> CentralFree(class)`；detach先从旧owner撤销，attach再向
新owner发布，同一slot不能同时存在于两个free domain。锁序只能是local transaction结束后取得central，central结束后
才可进入Talc；local、central与Talc guard不得嵌套或形成反向边。

低于上界的remote local capacity不被steal，central empty允许并发operation各自grow一个span，已发布span不reclaim。
按class保留历史峰值与bounded per-CPU stranded-free是本contract的明确能力边界，不构成low-memory robustness保证。

**违反表现：** local hit取得shared lock或backing allocation；free修改allocation CPU的cache；bitmap、owner字段或
free count成为第二份membership truth；local/central/Talc guard嵌套；central guard内进行span carve、逐对象backing
allocation、callback或无界扫描；background worker主动均衡或回收span。

**验证 / Enforcement：** free-list和refill/drain对象守恒KUnit、local empty/full边界KUnit、lock/IRQ与AP readiness
source audit；tracked acceptance配置的每CPUstranded-free上界为`130,816` bytes，8 CPU为`1,046,528` bytes；RV64与
LA64 8 CPU runtime分别记录`1 -> 2`及`4 -> 5`的cross-CPU allocation/final-close路径各128轮，PTY均`17/17`。

**最初来源：** [Kernel Slab Allocator RFC](../../rfcs/kernel-slab-allocator/index.md)；2026-08-15
`KERNEL-SLAB-CUTOVER`。

**当前来源：** [Kernel Slab Allocator RFC](../../rfcs/kernel-slab-allocator/index.md)；2026-08-15 closure提交。
