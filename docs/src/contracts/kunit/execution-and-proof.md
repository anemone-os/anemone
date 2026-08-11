# KUnit Execution and Proof 当前契约

**Contract ID：** `KUNIT-EXEC` / `KUNIT-CONCURRENCY` / `KUNIT-PROOF` / `KUNIT-SHAPE`
**状态：** Active
**Owner：** KUnit boot runner与repository validation policy；被测状态和并发协议仍由各production subsystem拥有
**参与领域：** KUnit framework / boot / scheduler / task kthread与kworker / timer与timekeeping / IPI / filesystem与global registry / validation claims
**覆盖范围：** 普通registered case的执行上下文、cleanup和panic边界、live scheduling准入与同步、运行结果可证明范围、KUnit conditional代码形状
**不覆盖：** 各subsystem correctness rule、host-side timeout实现、case filter/skip/fixture framework、expected-panic runner、用户态integration或性能测试
**实现位置：** `anemone-kernel/src/debug/kunit.rs`、`anemone-kernel/src/main.rs`、owner-local inline KUnit modules
**依赖：** 各测例触达的production owner contract
**Pending Successor：** None
**最后核验：** 2026-08-11

## 状态与能力所有权

| 事实 / 能力 | 唯一 Owner | 测例持有什么 | 行为用途 |
| --- | --- | --- | --- |
| suite启动时点、串行case dispatch与terminal panic边界 | KUnit boot runner | 普通`fn()` registration | 在共享live kernel中执行一次suite |
| 被测production状态、线程和并发协议 | 对应subsystem owner | production capability、token、handle或snapshot | 走真实路径并由owner裁决结果 |
| test-local phase、fixture与observation | 当前case / owner-local conditional module | 不进入production decision的私有值 | 建立确定性输入、握手和断言 |
| validation claim | 声明该claim的change/RFC/contract owner | 本次真实architecture/topology/path的运行证据 | 限定可报告的证明范围 |

KUnit不拥有被测subsystem状态。test-local phase只用于编排测试双方，不能变成production状态机的并列真相；一次
case PASS也不能改变production contract或补出本次运行没有实际执行的architecture、CPU topology或device path。

## KUNIT-EXEC-001 — Boot-integrated执行上下文

**规则：** registered cases在BSP `kinit` task上串行执行一次。开始执行时，配置CPU已完成local initialization，
Late initcalls、device attachment与rootfs mount已经完成，interrupt、scheduler、timer、allocation和`kthreadd`可用；
initial userspace task尚未prepare。current task只能依赖明确存在的kernel-task能力，不能假设普通用户进程的user memory、
files、filesystem state、credentials、signals或userspace trap frame。

**违反表现：** 把BSP boot task当作ordinary process；依赖case顺序或per-case隔离；在provider ready前伪造同类环境；
或把detached task fixture当作已发布、可调度的真实task。

**验证 / Enforcement：** boot调用顺序与runner源码审查；owner-local case只使用明确可用的上下文；双架构build和实际
KUnit boot核对runner时点。

**最初来源：** [KUnit execution boundary小迭代](../../devlog/changes/2026-08-02-kunit-execution-boundary.md)。

**当前来源：** 同上；本页由[KUnit execution and proof小迭代](../../devlog/changes/2026-08-11-kunit-execution-proof.md)
提取既有effective baseline，未改变其语义。

## KUNIT-EXEC-002 — Shared-kernel cleanup与terminal failure

**规则：** cases共享live kernel、global registries与rootfs，没有执行顺序或自动rollback contract。case成功返回前必须
撤销自己发布的对象、恢复修改的global state、删除filesystem fixture，并stop/join全部自行创建的kthread。任何panic
都终止kernel和当前suite；普通runner不得在可能部分修改的共享状态上unwind后继续。expected-panic只能使用独立
single-case boot和host-side terminal oracle。

**违反表现：** case返回后遗留线程、timer/request、listener、mount/path、registry entry或global configuration；后续case
依赖前序残留；把panic记为ordinary negative assertion后继续suite；或以host timeout替代case拥有的cleanup协议。

**验证 / Enforcement：** owner-local teardown/source audit；正常结束路径的join、cancel、unpublish和fixture removal断言；
runner在suite后同步filesystem mutation。

**最初来源：** [KUnit execution boundary小迭代](../../devlog/changes/2026-08-02-kunit-execution-boundary.md)。

**当前来源：** 同上；本页只提取既有effective baseline。

## KUNIT-CONCURRENCY-001 — Live scheduling是窄例外且必须逻辑闭合

**规则：** 普通KUnit默认不触达live scheduling、阻塞等待或新建kthread。只有当scheduler、wait、kthread、kworker、
timer/timekeeping、IPI等live并发语义本身就是被测对象，而且纯状态机/owner-local deterministic test不能形成所需证据时，
才允许通过production接口构造并发。测试双方必须以具名phase、predicate、Event、wait token、completion或等价
owner-visible事实完成握手；每个worker/request/listener必须在case返回前由真实lifecycle闭合。

禁止把固定次数的`yield_now()`、`schedule()`、tick等待、wall-clock sleep或“给另一个线程若干调度机会”当作某事件已发生、
未发生或不可能发生的证明。`yield_now()`只能作为等待显式phase/predicate变化的退让动作。除非timer/timekeeping、
timed-wait或scheduler tick accounting本身就是被测语义，duration/timeout只能作为failure bound，不能成为成功或排序
oracle；时间语义例外也不得外推到无关的并发顺序。

**违反表现：** `schedule N次后仍未变化`被当作互斥/ordering证据；依靠机器速度、tick phase或偶然interleaving PASS；
worker没有ready/done握手或join；用远期timer掩盖缺失的事件发布；或只为制造交错而给production路径增加暂停点。

**验证 / Enforcement：** 全量source audit搜索registered case中的kthread、yield、schedule、wait、timeout、tick和SMP路径；
review要求每个live并发case说明被测并发语义、握手事实、failure bound与cleanup。2026-08-11 cutover删除五个依赖
KUnit pause hook及固定32次yield的MM ordering cases，并删除Event与mount retry的test-driven observation hook。

**最初来源：** [KUnit execution and proof小迭代](../../devlog/changes/2026-08-11-kunit-execution-proof.md)。

**当前来源：** 同上；2026-08-11 closure checkpoint。

## KUNIT-PROOF-001 — PASS只覆盖实际执行的环境与路径

**规则：** 一个passing case只证明本次实际执行的architecture、CPU count/topology、feature tuple、device/filesystem和
production path。因缺少SMP、device或其它前提而提前返回的case对该缺席路径不形成证据；suite总PASS数不能把no-op
case外推成SMP、跨架构、硬件、failure-path或完整并发交错证明。detached fixture、test-local model和synthetic state
transition只证明其明确覆盖的owner-local语义，不能替代真实publication、transport或lifecycle。

**违反表现：** SMP=1的全绿结果被报告为SMP proof；RV64结果外推到LA64；QEMU外推到硬件；早退case仍被列为该路径
通过；synthetic callback或fixture被写成production transaction evidence；或以KUnit存在为由省略唯一owner、caller、
lifetime和happens-before源码审查。

**验证 / Enforcement：** validation记录必须报告实际tuple和Not Run；claim owner核对case是否进入目标production path；
并发correctness仍以owner/caller/lifetime/happens-before审查为主，KUnit只作为定向回归证据。

**最初来源：** [KUnit execution boundary小迭代](../../devlog/changes/2026-08-02-kunit-execution-boundary.md)。

**当前来源：** 同上；本页由2026-08-11小迭代提取既有effective baseline并明确no-op case不形成缺席topology证据。

## KUNIT-SHAPE-001 — Production不得理解KUnit测试协议

**规则：** production object state、control flow、owner API和ordinary-build visibility不得因KUnit而改变。禁止在production
transaction中调用KUnit hook、保存test probe/phase、按test状态分支，或只为强制交错/观察内部步骤而泛化production helper。
若某项语义只能依赖这种seam验证，应删除该测例并如实撤回相应proof claim，或另行建立有明确owner、consumer、failure、
cleanup和退出gate的validation/probe边界，不能让测试反向塑造长期实现。

owner-local `cfg(kunit)` fixture constructor、detached model和只读observation可以保留，但必须仅有真实测试consumer、
不参与production behavior decision、不复制可变truth，且不进入ordinary build/public API。production本来就需要的窄
capability或observation不能仅因KUnit也使用它而判定违规；其owner和consumer必须独立于测试成立。

**违反表现：** production struct携带probe字段；正常路径检查KUnit flag或回调test module；为了一个case增加generic hook、
暂停点或第二份状态；conditional helper被production依赖；或删除测例后遗留无真实consumer的测试抽象。

**验证 / Enforcement：** 全仓`cfg(kunit)`、`for_kunit`、hook/probe和production caller source audit；普通feature build证明
conditional surface不泄漏；Architecture Friction Scan核对第二份truth、owner穿透和test special case。2026-08-11
cutover恢复MM completion、Event timed wait和namei mount retry三条production路径的直接形状。

**最初来源：** [KUnit execution and proof小迭代](../../devlog/changes/2026-08-11-kunit-execution-proof.md)。

**当前来源：** 同上；2026-08-11 closure checkpoint。
