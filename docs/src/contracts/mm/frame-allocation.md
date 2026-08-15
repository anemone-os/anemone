# Frame Allocation 当前契约

**Contract ID：** `MM-FRAME`
**状态：** Active
**Owner：** `mm::frame` physical-frame allocation / RAII final release / placement handoff / global pressure accounting
**参与领域：** buddy allocator / per-CPU storage / KernelConfig / kernel heap / OOM与memory observers
**覆盖范围：** order-0 current-CPU magazine、buddy backing、bounded batch handoff、admitted miss recovery、RAII final release与global frame stats
**不覆盖：** reclaim、compaction、swap、memcg、NUMA、CPU hotplug、multi-order cache、zeroed cache、OOM victim policy、用户ABI或数值性能保证
**实现位置：** `anemone-kernel/src/mm/frame/`、`conf/kconfs/`、`scripts/xtask/src/config/kconfig.rs`
**依赖：** `MM-KMALLOC-001/002`、`MM-OOM-001`、`KCONFIG-VALIDATION-001`、`KUNIT-EXEC`、`KUNIT-CONCURRENCY`、`KUNIT-PROOF`、`KUNIT-SHAPE`
**Pending Successor：** None
**最后核验：** 2026-08-16

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| allocated frame / folio lifetime | `OwnedFrameHandle` / `OwnedFolio`与frame refcount | caller持RAII capability | 独占、共享与最终释放 |
| current-CPU order-0 free domain | `mm::frame`对应logical-CPU magazine | `percpu`只提供稳定current `CpuId` | local allocation与final release fast path |
| detached free frames | 当前allocator operation的fixed batch | magazine与buddy均已撤销membership | guard外handoff与failure cleanup |
| global free block domain | `mm::frame` buddy | allocator front持窄alloc/dealloc能力 | order-0 backing与higher-order contiguous allocation |
| total/free page pressure | `mm::frame` global accounting | OOM与observers取得一次copy snapshot | global physical-memory pressure truth |
| capacity / batch值 | KernelConfig | `mm::frame` consumer拥有semantic predicate | bounded retention与handoff policy |

每个allocator-managed order-0 frame在稳定状态只属于allocated lifetime、一个logical-CPU magazine或buddy；跨锁
移动时只属于当前operation batch。magazine数组的initialized prefix及同一guard下的精确长度是该slot唯一membership
表示，不建立bitmap、`owner_cpu`、remote-free queue或可独立变化的诊断表。global accounting只表达pressure，不判断
具体frame placement。

## MM-FRAME-001 — 唯一frame owner、RAII handoff与global pressure truth

**规则：** `mm::frame`唯一拥有physical-frame allocation、buddy与magazine placement、RAII final-release入口及global
`FrameAllocatorStats`。`alloc_frame*()`与`alloc_frames*(1)`共享order-0 route；任意单页`OwnedFrameHandle`或
`OwnedFolio`的最后一个ref撤销后进入同一order-0 deallocation route。`npages > 1`继续由buddy提供按既有order
rounding形成的contiguous block，最终释放不拆页进入magazine。

global `total_pages`表示PMM实际加入allocator的全部页，`free_pages`表示未被外部RAII allocation占用的实际buddy
charge。order-0 allocation commit恰好减一，最终release恰好加一；higher-order按实际rounded block charge更新。
buddy、operation batch与magazine之间的placement transfer不改变pressure accounting，因此magazine驻留不能被
`/proc/meminfo`、`sysinfo`或OOM投影为已用内存。counter使用checked update并暴露underflow、overflow或
`free_pages > total_pages`，不得clamp、saturate或反向驱动membership。

Kernel heap只作为frame allocator consumer取得backing；magazine、batch、sweep与stats不得调用可能进入
`mm::kmalloc`的容器增长、formatting或callback，也不触发reclaim、OOM hint、pending bit或wake edge。

**违反表现：** buddy stats成为runtime observer的buddy-only projection；同一frame同时存在于两个free domain；
RAII类型而非page count决定单页释放路径；higher-order folio被拆入magazine；pressure counter决定具体placement；
frame path递归进入kernel heap或向OOM提交allocation-edge side effect。

**验证 / Enforcement：** real frame API KUnit覆盖ordinary / zeroed frame、single-page folio、higher-order folio、
refcount与stats differential；源码审计覆盖全部RAII final Drop、stats consumer、PMM增量初始化及RV64 / LA64 BSP/AP
boot order；RV64与LA64 default KernelConfig release build通过。RV64 SMP2 KUnit boot在最终test/config-only微调前
`682/682`通过，ordinary non-KUnit SMP2 boot到userspace后orderly shutdown；最终微调后按维护者指示未重跑QEMU。

**最初来源：** [Frame Order-0 Magazine RFC](../../rfcs/frame-order0-magazine/index.md)；2026-08-16
`FRAME-MAGAZINE-CUTOVER`。

**当前来源：** [Frame Order-0 Magazine RFC](../../rfcs/frame-order0-magazine/index.md)；2026-08-16 closure提交。

## MM-FRAME-002 — Current-CPU order-0 magazine与admitted miss recovery

**规则：** current-CPU magazine命中时，order-0 allocation只在选择稳定current `CpuId`并操作对应slot的短noirq
transaction内完成，不获取buddy、其它CPU magazine或其它shared lock，也不执行blocking、reclaim、heap allocation、
callback或日志格式化。合法单页可以在任意CPU最终释放，并进入执行final release的current-CPU magazine；allocation
CPU不形成状态或affinity。

每CPU magazine具有build-time固定capacity，只在empty / full boundary通过operation-local fixed batch与buddy交换。
refill允许partial batch：operation交付一个frame，其余尽量发布到current slot，无法容纳的余量在local guard外归还
buddy。full release先在local transaction内detach至多configured batch，结束local guard后才逐页归还buddy。
capacity、batch非零且`batch <= capacity`；全机retention算术必须checked，recovery sweep的capacity-sized detached
batch不得超过一页stack storage。所有predicate由`mm::frame`在编译期拥有，xtask只忠实传输配置。

任意request只有在通过buddy order与算术admission、首次因当前availability miss后，才按本次boot已注册的dense
logical-CPU domain逐slot sweep。每次只持一个slot guard，原子detach该slot当时可见的全部frame，释放slot guard后
归还buddy；全部slot各检查一次后只重试原buddy request一次。admission failure不sweep；sweep不持有两个magazine
guard、不与buddy guard嵌套、不remote-steal到requester slot，也不声称形成global instantaneous-empty snapshot。

**违反表现：** current-CPU hit取得buddy/shared gate；不同logical CPU映射同一slot或使用clamp/fallback；保存
`owner_cpu`或remote-free queue；local / remote-magazine / buddy guard嵌套；partial refill泄漏余量；admission
failure触发sweep；retry无界；sweep遗漏已注册slot、把remote frame直接发布给requester，或把并发新release误写成
全局静止保证。

**验证 / Enforcement：** owner-local magazine/batch KUnit覆盖empty、hit、partial refill、full drain、batch=1与
conservation；detached-backend fixture覆盖order-0及higher-order admitted miss、all-slot sweep、retry-once success /
failure与admission bypass；`frame_magazine_capacity = 65536`由`mm::frame`单页sweep-batch predicate编译拒绝。
RV64 SMP2 focused KUnit通过真实frame API执行`CPU 0 -> CPU 1`cross-CPU final Drop、distinct-slot membership、stats
differential与joined worker lifecycle；独立final review为0 Apollyon / 0 Keter / 0 Euclid。该证据不外推contention、
fairness、吞吐或完整并发interleaving。

**最初来源：** [Frame Order-0 Magazine RFC](../../rfcs/frame-order0-magazine/index.md)；2026-08-16
`FRAME-MAGAZINE-CUTOVER`。

**当前来源：** [Frame Order-0 Magazine RFC](../../rfcs/frame-order0-magazine/index.md)；2026-08-16 closure提交。
