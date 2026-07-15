# 2026-07-15 - Sched Dynamic Attributes

**Status:** Active
**Owners:** doruche, Codex
**Area:** scheduler / dynamic attributes / syscall ABI / IPI / affinity
**Canonical Plan:** [RFC-20260714-sched-dynamic-attributes](../../rfcs/sched-dynamic-attributes/index.md), [不变量需求](../../rfcs/sched-dynamic-attributes/invariants.md), [迁移实施计划](../../rfcs/sched-dynamic-attributes/implementation.md)
**Canonical Revision:** R1
**Current Phase:** Checkpoint 2B Closed；Stage 3 Not Started / Not Run

## Scope

本事务从 R0 启动并在 Checkpoint 2B 接受 R1 ownership/failure-contract 修订，继续按同一阶段顺序实现第一版 dynamic scheduler attributes：先建立 dormant value-carrying one-shot，再完成 typed config、owner-CPU reconfigure、existing priority 原子切换、affinity remote vertical slice、legacy scheduler ABI、`sched_attr` ABI 与最终旁路审计。

阶段之间严格执行 canonical write set、停止条件、验证 floor 与独立 review gate。worker 不得未经批准越界修改；真实 owner boundary 需要扩张时，先在本事务和 implementation plan 记录批准结果。本事务不修复 wait-core synchronous remote placement，也不把 IRQ-off allocation 风险误写为已关闭。

## Invariants

- published task 的 policy、parameters、nice、reset flag 与 affinity 最终只有一个 `SchedConfig` truth；Phase 2B 切换前不安装第二 storage。
- local 与 remote setter 汇合到固定 owner CPU 的同一 `ApplyConfigPatch` transaction；syscall adapter 不以 stale snapshot 拼完整 config。
- one-shot persistent phase 是 receive 的唯一返回依据；Force 只结束并重建当前 Latch round，不关闭 receiver、不释放 remote gate。
- `SenderClosed` 只有在唯一 sender 与未来 mutation/complete capability 同时消失时才是 scheduler request 的确定失败。
- `REMOTE_SCHED_REQUEST_GATE` 只串行 remote scheduler request，handler 不获取它；wait-core placement owner不变。
- raw Linux ABI 只存在于 `sched/api`；class/core 不持 raw layout、policy number或 errno ordering。
- Fair、RT、RunQueue 与 task lifecycle owner不因阶段拆分而移动；accepted allocation issue与 limitation保持 Open / Active。

## Checkpoint Authority

下表只登记各后续 checkpoint 的初始协作边界；完整文件列表、验证命令和停止条件仍以 [迁移实施计划](../../rfcs/sched-dynamic-attributes/implementation.md) 为唯一 canonical authority。任何扩张必须先回写该计划和本事务。

| Checkpoint | 初始 write set | Implementation Owner | Review Owner | Runtime Owner | 初始状态 |
| --- | --- | --- | --- | --- | --- |
| 阶段 1 | `sched/{oneshot,mod}.rs` 与 oneshot owner 内 KUnit | Codex 总控或一个受限 worker | 与实现者不同的只读 reviewer | agent：build / pretest KUnit | Implementation Not Started；review/runtime Not Run |
| Checkpoint 2A | `sched/config.rs`、sched class typed foundation、priority 目录机械搬迁、`exception/ipi.rs` Clone 适配及同 owner KUnit | Codex 总控或一个受限 worker | 独立 reviewer 分别审 priority move、IPI Clone、dormant model | agent：build / pretest KUnit；priority runtime 未授权时由用户运行 | Implementation Not Started；review/runtime Not Run |
| Checkpoint 2B | request/config/processor/class/task/clone/priority/IPI/procfs final cutover 与同 owner KUnit | Codex 总控或一个受限 worker；不得拆分唯一 truth 切换 | 独立 reviewer 覆盖 config、role、request/gate、clone 与 2A 隔离 | agent：build / pretest KUnit；priority LTP由用户或明确授权的 agent | Implementation Not Started；review/runtime Not Run |
| 阶段 3 | affinity adapter、rv64/la64 syscall numbers、`sched-attr-test`、rv64 pretest routing；SMP/profile仅validation-only | Codex 总控或一个受限 worker | 独立 ABI 与 remote-gate reviewer | agent：双架构 build、pretest、单CPU app；用户或明确授权的 agent：SMP=2 stress / targeted LTP | Implementation Not Started；review/runtime Not Run |
| 阶段 4 | policy adapter、窄 interval accessor、syscall numbers、focused app；profile仅validation-only | Codex 总控或一个受限 worker | 独立 policy/permission/interval reviewer | agent：build / pretest / focused app；用户或明确授权的 agent：targeted LTP | Implementation Not Started；review/runtime Not Run |
| 阶段 5 | attr adapter、syscall numbers、focused app；profile仅validation-only | Codex 总控或一个受限 worker | 独立 Linux 6.6 size/copy/errno reviewer | agent：build / focused probes；用户或明确授权的 agent：targeted LTP | Implementation Not Started；review/runtime Not Run |
| 阶段 6 | 既有 owner 文件中的审计修正、focused asset、current-revision docs/devlog/nav；register仅真实状态变化时更新 | Codex 总控 | 独立最终 reviewer | agent：build / source / format / docs；用户或明确授权的 agent：SMP=2、ABI matrix、schedule profile、必要的 la64 smoke | Implementation Not Started；review/runtime Not Run |

## Phase Log

### 2026-07-15 - 阶段 0 R0 Acceptance 与 Source Audit

**Phase:** 阶段 0 - 文档、Live Source 与 R0 Acceptance 前置 Gate。

**Change:** RFC 从 Draft 接受为 `R0 / Accepted for Implementation`；canonical invariants 转为 `Canonical / R0`，implementation plan 转为 `Active / R0`。建立本事务并接入 RFC、transaction index、当前双周 devlog、RFC index 与 mdBook 导航。阶段 0 未修改 kernel、ABI crate、app、rootfs、runner或live build配置。

**Document Review:** 首轮独立 review 发现 Force 关闭 receiver 并释放 gate 后，旧 request 仍可提交 mutation 的 Keter，因此当时未接受 R0。修订后的协议把 persistent channel phase恢复为唯一返回依据：Force只完成当前 Latch round，receiver清除旧 registration、锁外 drop、finish并在empty时rearm；`SenderClosed`同时证明未来 mutation capability消失。复审确认 `KETER-DYNATTR-006` 已 neutralize，最终无 Apollyon、Keter 或 Euclid。

**Source Audit:** live Fair / RT / RunQueue owner surface可以增加 dedicated reconfigure，不需要伪装成 yield、block、wake或preempt；`NCPUS` 在 production task construction前初始化，clone在publish前有完整config/affinity窗口。Generic IPI可以保持async transport并使用独立 scheduler one-shot，queue lock在业务handler前释放，`IpiPayload Copy -> Clone`可机械迁移。rv64/la64 syscall owner、raw user-copy helper、schedule LTP group和pretest入口均可承载后续write set。`AtomicNice`、direct setter与procfs all-online mask仍是Phase 2B必须原子替换的旧truth。

**Stop Conditions:** `implementation.md:121-124` 四项均未命中：class lifecycle可表达reconfigure；boot/clone可在publish前建立合法config；request variant不要求改变generic IPI completion/placement owner；UAPI matrix与canonical errno/field contract无矛盾。

**Register Boundary:** [IRQ-off heap allocation](../../register/open-issues.md#ane-20260622-irq-off-heap-allocation) 保持 Open；[RT noirq bucket allocation](../../register/current-limitations.md#ane-20260713-sched-rt-noirq-bucket-allocation) 保持 Active。本阶段不宣称修复。

**Validation:** `git diff --check` 通过；新增 transaction 的 `git diff --no-index --check /dev/null docs/src/devlog/transactions/2026-07-15-sched-dynamic-attributes.md` 无 whitespace 告警；`mdbook build docs` 通过。mdBook 仅报告既有 search index 较大警告。

**Not Run:** kernel build、format、KUnit、QEMU、focused app、SMP=2 stress、LTP与la64 runtime均未运行；阶段 0 是 docs/source-audit checkpoint。

**Next:** 先提交阶段 0。阶段 1 只能在该提交后按 canonical write set实现 dormant `sched::oneshot`，并通过全部 enabled KUnit与独立 review gate后再进入2A。

### 2026-07-15 - 阶段 1 Dormant One-shot 启动

**Phase:** 阶段 1 - Dormant `sched::oneshot` 原语；Implementation In Progress，review/runtime Not Run。

**Preflight:** 阶段 0 已由前序提交关闭。live `Latch` 提供 `begin_current()`、`make_trigger()`、`cancel()`、`schedule_with_timeout()` 与 consuming `finish()`，不需要公开 raw wait token；`NoIrqSpinLock` 可以在固定 phase / trigger slot 上提供 hardirq-safe bounded transaction，锁内不需要 allocation、scan 或 trigger/drop。KUnit runner 位于 late init 之后，能够复用现有 kthread API验证真实 parked-window Force/rearm。当前没有未 neutralize 的 Apollyon / Keter 阻塞本 gate。

**Write Set Lock:** kernel write set严格限于新增 `anemone-kernel/src/sched/oneshot.rs`、窄改 `anemone-kernel/src/sched/mod.rs` 及 `oneshot.rs` owner内KUnit；不得修改 `sched/{wait,latch,event}.rs`、IPI、task fields或syscall代码。事务日志只记录协作和执行事实，不扩大kernel owner surface。

**Stop Conditions:** 启动时四项均未命中：不需要扩展wait-core public surface；hardirq sender不需要sleepable lock、无界scan或锁内allocation；accepted Force cleanup可以由single persistent phase加一个bounded registration slot表达；`channel()`继续服从现有infallible allocation / fatal OOM边界，不改变签名。

**Planned Gate:** 先实现dormant channel与决定性owner-local KUnit，再运行`just fmt kernel --check`、`just build`、`git diff --check`及rv64 pretest全部enabled KUnit；随后由未参与写入的独立reviewer检查完整阶段一diff。上述验证、review和任何修正当前均为Not Run / Not Started，不预记为通过。

### 2026-07-15 - 阶段 1 实现、修正与 Gate 关闭

**Change:** 新增 `sched::oneshot::channel<T>()`、non-clone single-use `Sender<T>`、non-clone consuming `Receiver<T>` 与仅含 `SenderClosed` 的 `RecvError`。channel constructor 只分配 `Arc<Shared<T>>` 和两个endpoint；persistent phase与一个bounded `Option<LatchTrigger>` registration共用`NoIrqSpinLock`，不读取current task或begin wait。send/close在锁内发布terminal phase并detach trigger，锁外trigger；receive先查persistent terminal，empty才建立receive-local Latch，注册竞争失败时cancel/finish，registered wake后先detach/drop旧trigger、finish并重查phase，empty只接受Force并内部rearm。endpoint/shared cleanup先撤销publication并锁外释放trigger/payload，再用常开断言暴露phase bug。

**Focused Tests:** `oneshot.rs`内6项KUnit覆盖dormant constructor与endpoint move能力、send-before-receive、begin/register/pre-park各发送窗口、sender-drop与receiver-drop、repeated Force、Force与value/sender-close竞争、真实Parked Force后新round rearm、timer hardirq sender，以及payload exactly-once drop。parked case最初使用同CPU helper持续yield等待第二轮，pretest稳定停在该case；修正为helper观察真实`Parked`后Force并退出，再由50ms local timer IRQ发送terminal value，receiver同时断言至少begin两轮。该修正只改变test coordination，不增加production phase、generation或completion flag。

**Review Corrections:** 未参与写入的独立reviewer首轮发现Drop/terminal异常路径在cleanup前断言的Keter；修正后所有相关路径均先锁内replace phase与take trigger、锁外drop trigger/payload/terminal，再断言。复审仅剩两个phase violation缺少channel debug identity的Euclid，补齐`channel_id`后最终复审确认完整阶段一diff无Apollyon、Keter或Euclid；constructor dormancy、single phase owner、锁外trigger/drop、Sender non-clone、每轮Latch finish、Force rearm、hardirq bounded sender与write set均通过source-review gate。

**Validation:** 最终`just build`通过。按用户要求使用`./scripts/run-user-test-rv64.sh etc/sdcard-rv.img`完成pretest rootfs、sdcard staging、kernel build与QEMU启动；`build/user-test-rv64.log`记录`Running 135 tests...`、6项新增one-shot KUnit全部`ok`和`All tests passed!`。KUnit完成后退出QEMU，不等待full-profile LTP；输出批次已经开始的少量post-KUnit LTP不作为本阶段证据，也未分类为PASS/FAIL。`git diff --check`无告警；新增`oneshot.rs`的`git diff --no-index --check /dev/null ...`无whitespace诊断。`just fmt kernel --check`只报告被忽略的generated `kconfig_defs.rs` / `platform_defs.rs`既有生成格式漂移，未报告阶段一tracked源码。`mdbook build docs`通过，仅有既有large search-index warning。

**Source / Worktree Boundary:** 最终tracked kernel diff只包含`sched/mod.rs`与新增`sched/oneshot.rs`；未修改`wait.rs`、`latch.rs`、`event.rs`、IPI、task field或syscall。producer lock内只有bounded phase/slot操作，无sleepable lock、scan、allocation、trigger或payload drop。工作树中的`AGENTS.md`改动不属于本阶段、未由本事务修改，提交时保持不暂存。

**Stop Conditions:** 阶段一四项停止条件最终均未命中；实现不需要扩展wait-core surface、不引入hardirq非bounded工作、不需要第二Force/cancellation truth，也不改变infallible channel API或accepted fatal OOM边界。阶段一Implementation、runtime KUnit与独立review gate关闭；Checkpoint 2A仍为Not Started。

### 2026-07-15 - Checkpoint 2A Mechanical / Dormant Foundation 启动

**Phase:** 阶段 2 / Checkpoint 2A；Implementation In Progress，review/runtime Not Run。

**Preflight:** 阶段一的实现、全部enabled KUnit与独立review gate已由前序提交关闭；阶段0 source audit确认class lifecycle可增加dedicated reconfigure。当前tracking issues没有未neutralize的Apollyon或Keter阻塞本checkpoint。

**Write Set Lock:** kernel write set严格限于`implementation.md`的Checkpoint 2A列表：新增`sched/config.rs`，窄改`sched/mod.rs`、class typed foundation、priority目录机械搬迁、`sched/api/mod.rs`与`exception/ipi.rs` Clone适配，以及对应owner内KUnit。不得修改Task storage、`AtomicNice`、RunQueue transaction、clone、procfs、wait-core、rootfs、LTP profile或user ABI；worker不得提交，发现真实owner需要扩张时必须停止上报。

**Behavior Boundary:** typed config、patch、mask、permit与class transition factory保持dormant，不安装进Task或published `SchedEntity` storage，不增加production request/reconfigure caller。priority只做behavior-preserving owner move并继续读取当前唯一`AtomicNice` truth；IPI只做`Copy -> Clone`机械收窄，不增加scheduler request variant。

**Planned Gate:** 完成后运行`just fmt kernel --check`、`just build`、`git diff --check`与rv64 pretest全部enabled KUnit，并在log中点名2A新增case；随后由未参与写入的独立reviewer分别检查priority move、IPI Clone与dormant typed model。existing priority用户态runtime未单独授权时记为Not Run，不用source proof替代。

### 2026-07-15 - Checkpoint 2A 实现、审查与 Gate 关闭

**Change:** 新增`sched/config.rs`中的typed `SchedConfig`、discipline/parameter、semantic patch、`CpuMask`、non-clone narrow permit与typed error；它们只提供纯projection、online normalization和latest old/new permit检查，没有安装进Task或published `SchedEntity` storage。将四个priority文件从`task/api`逐字节搬到`sched/api`，继续使用唯一`AtomicNice` truth与原selector/result folding。`IpiPayload`只从`Copy`收窄为`Clone`并机械适配broadcast/handler。Fair新增从owner placement floor构造placed payload的方法，RT复用typed priority/mode构造fresh FIFO/RR payload；均未增加published reconfigure caller、RunQueue入口或request path。

**Review:** 未参与写入的独立reviewer分别审查typed model、class factory、priority move与IPI Clone，并按补充要求单独检查模块边界和代码范式。最终无Apollyon或Keter。唯一Euclid是早期注释把整个typed value/RT factory称为dormant，但`RtPriority`和fresh factory已经由existing unpublished default constructor机械复用；修正后的注释只把完整`SchedConfig`/patch/mask/permit与published transition call site标为dormant。state-owning操作保持为owner type方法，跨两个snapshot的纯permit relation、priority target orchestration与IPI dispatch保持为私有/module自由函数；visibility没有扩大mutation capability或class-private payload surface。RFC入口的阶段一进度文案同时改为稳定指向本事务，不再维护第二份checkpoint进度。

**Validation:** 总控重跑`just build`通过，rv64 release使用`fs_ext4`、`spin_lock_irqsave`与`kunit`且无compiler warning。`./scripts/run-user-test-rv64.sh etc/sdcard-rv.img`重建pretest rootfs、kernel并启动QEMU；`build/user-test-rv64.log`记录`Running 142 tests...`，5项config/permit/mask、RT/Fair各1项transition factory及搬迁后的priority KUnit全部`ok`，最终`All tests passed!`。KUnit结束后QEMU进入常规fair-test/LTP，随后由总控结束；这些post-KUnit输出不作为2A LTP证据。四个priority文件与HEAD旧路径逐文件SHA-256一致。`git diff --check`及新文件no-index whitespace检查无告警；`just fmt kernel --check`只报告未触碰generated `kconfig_defs.rs` / `platform_defs.rs`既有漂移。公开RFC进度措辞调整后`mdbook build docs`通过，仅报告既有large search-index warning。

**Source / Write-set Audit:** `SchedConfig`、patch与permit production引用只存在于其owner测试；没有`SchedulerRequest`、`ApplyConfigPatch`或remote gate。`AtomicNice`、direct setter、Task storage、RunQueue、clone、procfs、wait-core、rootfs、profile与user ABI均未修改。工作树中的`AGENTS.md`改动继续属于用户，不进入本checkpoint。

**Not Run:** existing priority独立用户态runtime、targeted priority LTP、la64 build/runtime、SMP=2与后续ABI验证均未运行；2A只要求behavior-preserving move与rv64全部enabled KUnit。运行脚本后附带开始的普通LTP输出没有完整运行或分类，不作为PASS/FAIL证据。

**Stop Conditions:** 2A未安装第二config truth、未改变production mutation行为、未提前发布request path，也不需要write-set扩张；本checkpoint停止条件均未命中。Implementation、独立review与runtime KUnit gate关闭；2B仍为Not Started。

### 2026-07-15 - Checkpoint 2B Final-shape Atomic Cutover 启动

**Phase:** 阶段 2 / Checkpoint 2B；Implementation In Progress，review/runtime Not Run。Checkpoint 2A已由提交`7616b39b`关闭，机械搬迁不再混入本diff。

**Preflight:** 独立只读preflight逐项核对AtomicNice/Task storage、RunQueue role、Processor current/pending、Fair/RT lifecycle、clone、priority、procfs、IPI与request接线；canonical 2B write set足以完成final shape，当前未命中停止条件或扩张需求。实现保持零参数`SchedEntity::new_default()`，由scheduler owner构造online default affinity，避免把owner参数传播到write set外的architecture/kthreadd caller。

**Write Set Lock:** kernel write set严格限于新增`sched/request.rs`，以及`sched/config.rs`、`sched/{mod,processor,nice}.rs`、`sched/class/{mod,entity,runqueue,rt}.rs`、必要时仅作trait exhaustiveness同步的`class/idle.rs`、`sched/class/fair/{mod,stride}.rs`、`task/{mod,sched}.rs`、`task/api/{mod,clone/mod}.rs`、`sched/api/mod.rs`、`sched/api/priority/**`、`exception/{mod,ipi}.rs`、`fs/proc/tgid/status.rs`与对应owner KUnit。`exception/mod.rs`仅允许re-export既有`IpiError`。不得修改wait-core、`sync::Mutex`、Event、architecture trap、Kconfig、default policy、user ABI、rootfs或LTP profile；worker不得提交或修改文档，任何额外constructor/caller文件先停止上报。

**Atomic Cutover Boundary:** 本checkpoint必须同时安装唯一`SchedConfig` storage、删除`AtomicNice`与published direct setter、完成class role transaction、local/remote request/gate/IPI接线，并切priority、clone与procfs；不得按storage/request/ABI再拆提交，也不得保留第二nice/policy/priority/affinity truth或临时adapter。所有可恢复错误在detach前结束，publication后只允许不可失败attach tail。

**Review Dimensions:** 独立review除single truth、role/placement、current resched、request/gate、clone inheritance与2A隔离外，单独检查模块边界和代码范式：对象不变量逻辑应成为owner type方法，跨对象编排/纯关系才保留自由函数；visibility必须阻止syscall/request越过RunQueue和class owner，不得新增manager式泛化、shared mutation token或public payload构造面。

**Planned Gate:** 完成后运行`just fmt kernel --check`、`just build`、`git diff --check`与rv64 pretest全部enabled KUnit，并点名config/role/request/clone/priority case；source audit确认无`AtomicNice`、`Task::set_nice`、published entity replace、第二config truth、request broadcast或IPI queue-lock跨业务handler。existing priority local用户态runtime若没有独立focused资产则如实记录验证边界；remote SMP=2留到阶段3，不以source proof写成runtime PASS。

### 2026-07-15 - Checkpoint 2B Write Set 扩展批准

**Trigger:** 首次compile integration中，`sched/api`需要把`send_ipi_async()`的typed transport failure映射为`TransportAllocation`或`TargetOffline`。`IpiError`及其variants已经在`exception/ipi.rs`中公开，但private `ipi` module没有从`exception` facade re-export该返回类型；在原write set内绕行会迫使IPI owner增加两个只包一层`matches!`的单用途predicate方法。

**Approval:** 将`anemone-kernel/src/exception/mod.rs`加入Checkpoint 2B write set，只允许从现有IPI facade re-export既有`IpiError`。删除临时`is_allocation_failure()` / `is_target_offline()`薄wrapper，让scheduler caller直接匹配现有error variants。该变更不新增error variant，不改变transport、completion、ABI或accepted scheduler contract，也不修改architecture trap。

**Review / Validation Impact:** 最终独立review必须确认新增surface只暴露现有函数签名已经返回的transport error、scheduler没有解释transport之外的policy，并完成全树薄wrapper审计。原build、全部enabled KUnit、priority local runtime与source-audit floor保持不变。

### 2026-07-15 - Checkpoint 2B IPI Ownership 修正

**Feedback:** 用户在pre-commit review指出，scheduler request进入broadcast是内核调用错误，不应新增可恢复`IpiError::NotBroadcastable`；既然request不可broadcast，以`Arc<SchedRequest>`满足整个`IpiPayload: Clone`也错误暗示共享所有权。总控暂停旧final review并退回同一checkpoint修正。

**Accepted Correction:** `IpiPayload::SchedulerRequest`改为single-owner`Box<SchedRequest>`；payload删除通用`Clone`，IPI handler借用而不clone payload，broadcast只显式复制eligible variant，并在任何allocation/enqueue前对scheduler request误用常开断言失败。`SchedRequestBody`的`Some -> None`继续防御duplicate execute与double completion；Box保持payload尺寸和诊断地址稳定。删除`NotBroadcastable` variant及errno mapping，保留`IpiError` facade re-export供真实`Alloc` / `TargetOffline` typed mapping。

**Contract / Gate Impact:** canonical index、invariants、implementation与tracker同步为`sched/request` owner和single-owner payload。该反馈不改变R0目标、owner、UAPI、gate、completion或exactly-once语义，保持R0；2B write set、验证floor与停止条件不变。旧Arc diff上的review结论作废，新冻结diff必须重新完成独立review、build与全部enabled KUnit。

### 2026-07-15 - Checkpoint 2B RFC Revision Classification Correction

**Correction:** 上一条把ownership/failure-path修正归类为“保持R0”不符合仓库修订规则。R0 canonical已经明确接受`Arc<SchedRequest>` ownership invariant，而本次反馈把它改为single-owner `Box`，并把broadcast误用从可恢复transport rejection改为任何发送副作用前panic；即使目标、owner、UAPI、gate、completion与exactly-once高层边界不变，这仍是已接受的不变量和failure contract变化，因此当前canonical修订递增为R1。

**Transaction / Write Set:** R0事务仍为Active且从未Completed，本次R1继续由同一事务实现和验证，不创建post-close transaction。为保持当前RFC导航不漂移，批准把`docs/src/rfcs.md`加入本checkpoint的docs write set；kernel write set、stage order、review gate、验证floor与停止条件均不变。

### 2026-07-15 - Checkpoint 2B 实现、R1 修正与 Gate 关闭

**Change:** 原子安装`SchedEntity`唯一`SchedConfig` truth，删除`AtomicNice`和published direct setter；`RunQueue::apply_config_patch()`覆盖current、queued、detached和Zombie role，按Fair/RT owner处理payload replacement、placement、RR budget/rotation与current full-pick。priority local/remote submission、global remote gate、one-shot completion、clone reset/fresh runtime/affinity-contained CPU及procfs affinity projection全部切到同一truth。request/gate lifecycle位于`sched/request.rs`，`sched/api`只保留ABI selector、result folding和errno映射。

**R1 Ownership Correction:** 用户pre-commit反馈后，scheduler request从`Arc<SchedRequest>`改为single-owner`Box<SchedRequest>`；`IpiPayload`删除通用`Clone`，handler借用payload，broadcast只显式复制eligible variant，并在任何allocation/enqueue前对scheduler request误用panic。删除`IpiError::NotBroadcastable`，只保留真实`Alloc` / `TargetOffline` transport failure。canonical RFC按明确ownership invariant和broadcast failure contract变化递增R1；R0目标、owner、UAPI、gate、completion与exactly-once接受边界不变。

**Module / Paradigm Review:** 删除临时transport-error predicate wrapper以及Fair reconfigure的一行语义wrapper，调用点直接执行owner操作并在非显然顺序处保留inline invariant comment。最终复审又删除无收窄/校验价值的`new_round_robin_entity()` / `new_fifo_entity()`；fresh runtime构造归属`RtEntity::new_fresh(mode)` associated constructor，test-only显式runtime注入保持`rt.rs`私有。最终owner-type方法承载entity/class不变量，跨对象submission、纯config关系与ABI target编排保留module自由函数，没有新增manager式抽象、共享mutation token或public payload构造面。

**Independent Review:** 未参与写入的独立reviewer在旧Arc diff作废后重新审查最终冻结diff，覆盖config/state owner、role/placement、Fair/RT transition、current pending、request/gate/Box lifetime、broadcast pre-side-effect panic、handler borrow、transport failure cleanup、clone、priority、procfs、模块边界、OOP/free-function选择与薄wrapper。最终结论为Apollyon 0、Keter 0、Euclid 0，Checkpoint 2B source-review PASS。

**Agent-run Validation:** 最终`just build`通过且无compiler warning。`./scripts/run-user-test-rv64.sh etc/sdcard-rv.img`重建rootfs和kernel并启动rv64 pretest；`build/user-test-rv64.log`记录`Running 163 tests...`、新的broadcast-copy、`RtEntity` fresh constructor、request exactly-once、local setpriority以及config/role/clone/procfs focused KUnit全部`ok`，最终`All tests passed!`。随后`fair-test`的equal-progress、nice-direction、bounded-yield和sleep-wake-progress全部通过；其nice-direction路径实际调用并read-back `setpriority()` / `getpriority()`，因此只作为existing priority local userspace vertical slice，不升级为targeted priority LTP证据。KUnit/fair-test后启动的普通LTP输出是incidental且未完整运行或分类，不作为本checkpoint PASS/FAIL证据。

**Static / Docs Validation:** `git diff --check 7616b39b`通过；source audit对`AtomicNice`、direct nice setter、旧reconfigure wrapper、固定mode RT wrapper、`Arc<SchedRequest>`、`NotBroadcastable`、通用payload clone与handler `payload.clone()`均为零production命中。`just fmt kernel --check`只报告未手工维护的generated `anemone-kernel/src/kconfig_defs.rs`和`platform_defs.rs`既有whitespace drift，未报告authored 2B source。R1 canonical/navigation更新后`mdbook build docs`通过，仅有既有large search-index warning。

**Not Run:** targeted priority LTP、SMP=2 remote setter runtime、remote gate contention/dual-CPU stress、la64 build/runtime以及Stage 3 affinity ABI/runtime均未运行；SMP=2与affinity验证继续由Stage 3 canonical gate拥有，不能由2B source proof或单CPU KUnit替代。

**Source / Worktree Boundary:** kernel diff只落在Checkpoint 2B批准的owner文件；`exception/mod.rs`仅re-export既有`IpiError`，`docs/src/rfcs.md`仅因R1 current-revision导航同步进入批准的docs扩展。`AGENTS.md`仍是用户改动，未由本checkpoint修改并在提交时保持unstaged。没有wait-core、Mutex/Event、architecture trap、Kconfig、default policy、rootfs/profile或user ABI tracked修改。

**Stop Conditions:** 最终未命中第二config truth、wrong-owner mutation、detach后ordinary failure、request broadcast、handler获取gate/credential、completion早于commit、owner/write-set不闭合或review blocker。Checkpoint 2B Implementation、独立review与rv64 KUnit/local fair-test gate关闭；Stage 3保持Not Started / Not Run。

## Open Items

- 本 RFC owner内当前无开放 Apollyon、Keter或 Euclid。
- wait-core [KETER-WAIT-001](../../rfcs/sched-wait-refactor/tracking-issues.md#keter-wait-001synchronous-remote-placement-不能组合进-cross-cpu-ipi-completion) 继续 Open；R1 remote gate只neutralize scheduler request producer graph。
- Checkpoint 2A和2B已关闭；Stage 3及后续阶段保持Not Started / Not Run。

## Closure

事务 Active；R1的Stage 2已关闭，Stage 3及后续实现尚未开始，RFC整体尚未关闭。
