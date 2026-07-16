# 2026-07-15 - Sched Dynamic Attributes

**Status:** Completed
**Owners:** doruche, Codex
**Area:** scheduler / dynamic attributes / syscall ABI / IPI / affinity
**Canonical Plan:** [RFC-20260714-sched-dynamic-attributes](../../rfcs/sched-dynamic-attributes/index.md), [不变量需求](../../rfcs/sched-dynamic-attributes/invariants.md), [迁移实施计划](../../rfcs/sched-dynamic-attributes/implementation.md)
**Canonical Revision:** R1
**Current Phase:** Checkpoint 2B、Stage 3、Stage 4、Stage 5与Stage 6全部Closed；R1 Completed

## Scope

本事务从 R0 启动并在 Checkpoint 2B 接受 R1 ownership/failure-contract 修订，继续按同一阶段顺序实现第一版 dynamic scheduler attributes：先建立 dormant value-carrying one-shot，再完成 typed config、owner-CPU reconfigure、existing priority 原子切换、affinity remote vertical slice、legacy scheduler ABI、`sched_attr` ABI 与最终旁路审计。

阶段之间严格执行 canonical write set、停止条件、验证 floor 与独立 review gate。worker 不得未经批准越界修改；真实 owner boundary 需要扩张时，先在本事务和 implementation plan 记录批准结果。本事务不修复 wait-core synchronous remote placement，也不把 IRQ-off allocation 风险误写为已关闭。

## Invariants

- published task 的 policy、parameters、nice、reset flag 与 affinity 最终只有一个 `SchedConfig` truth；Phase 2B 切换前不安装第二 storage。
- local 与 remote setter 汇合到固定 owner CPU 的同一 `ApplyConfigPatch` transaction；syscall adapter 不以 stale snapshot 拼完整 config。
- one-shot persistent phase 是 receive 的唯一返回依据；Force 只结束并重建当前 Latch round，不关闭 receiver、不释放 remote gate。
- `SenderClosed` 只有在唯一 sender 与未来 mutation/complete capability 同时消失时才是 scheduler request 的确定失败。
- `REMOTE_SCHED_REQUEST_GATE` 只串行 remote scheduler request，handler 不获取它；wait-core placement owner不变。
- Linux userspace representation、layout与共享常量由`anemone-abi::process::linux::sched`统一拥有；kernel raw copy、parse、ordering与semantic conversion只存在于`sched/api`，class/core不持raw ABI。
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
| 阶段 5 | shared `SchedAttr` ABI、`policy/attr/{mod,sched_setattr,sched_getattr}` adapter、syscall numbers、focused app；profile仅validation-only | Codex 总控或一个受限 worker | 独立 Linux 6.6 size/copy/errno reviewer | agent：build / focused probes；用户或明确授权的 agent：targeted LTP | Closed |
| 阶段 6 | 既有 owner 文件中的审计修正、focused asset、current-revision docs/devlog/nav；register仅真实状态变化时更新 | Codex 总控 | 独立最终 reviewer | agent：build / source / format / docs；用户或明确授权的 agent：SMP=2、ABI matrix、schedule profile、必要的 la64 smoke | Closed |

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

### 2026-07-15 - Stage 3 Affinity ABI / Remote Gate Vertical Slice 启动

**Phase:** 阶段 3；Implementation In Progress，independent review / runtime Not Run。Checkpoint 2A和2B已经由提交`7616b39b`与`1b5f2eb8`关闭，Stage 3不重新打开core storage、owner transaction或request/gate设计。

**Preflight:** live `sched/config.rs`已经提供UAPI-independent `CpuMask`、online normalization、fixed-owner validation与affinity-only patch，`Task::sched_config()`提供coherent snapshot，`sched/request.rs`提供local/remote同入口、typed transport/transaction result与现有全局`Mutex<()>` gate。`sched/api`可以新增独立affinity adapter而无需读取class runtime、RunQueue guard或credential lock跨wait；rv64/la64 syscall number owner、raw user-slice helper、focused app manifest、rv64 pretest rootfs与local-test入口均在canonical write set内。当前tracker没有未neutralize的Apollyon或Keter，preflight未发现需要migration、第二affinity truth、wait-core/IPI修改或write-set扩张的停止信号。

**Write Set Lock:** implementation严格限于新增`anemone-kernel/src/sched/api/affinity/{mod,sched_setaffinity,sched_getaffinity}.rs`，窄改`sched/api/mod.rs`、`anemone-abi/src/process.rs`中的`process::linux::sched` CPU-set ABI，以及`anemone-abi/src/syscall/{riscv,loongarch}.rs`，新增`anemone-apps/sched-attr-test/**`，并修改`conf/rootfs/pretest-rv64.toml`和`anemone-apps/user-test/src/main.rs`完成focused app接线。affinity ABI遵守one-syscall-per-file；`mod.rs`只保留两个入口真实共享的native-word常量与target lookup，copy、permission、mask conversion、errno mapping和phase ordering归各自syscall文件。`CpuSet`是Linux userspace `cpu_set_t`的1024-bit layout；kernel raw syscall仍按caller `cpusetsize`处理最小kernel-domain前缀，不能要求完整struct。`conf/platforms/qemu-virt-rv64-pretest.toml`只允许临时把`smp`改为2，`anemone-apps/user-test/ltp/profile.txt`只允许临时选择schedule group；两者runtime后必须恢复并确认最终无diff。不得修改core transaction/request/IPI/wait-core、Task storage、procfs、minimal或la64 rootfs；用户已明确允许必要时扩张`anemone-rs`，但当前raw ABI focused probes无需公共wrapper，若后续出现真实owner需求仍须先记录扩张影响。

**ABI / Stop Boundary:** setter保持copy-in、lookup、permission、online-mask semantics、patch submit顺序；getter保持len/alignment validation、lookup、snapshot、copy-out顺序。fixed owner不在normalized mask时返回`EINVAL`并保留诊断，不排队migration。若实现要求修改`Task::cpuid`、跨CPU RunQueue lock、raw mask进入core、procfs/clone/getter产生不同truth，或为了LTP静默接受migration-required mask，立即停止并回到RFC/write-set review。

**Planned Gate:** 完成后执行affinity conversion/ordering KUnit、`just build`、双架构focused app build、rv64 pretest全部enabled KUnit与单CPUfocused app。SMP=2双向remote setter stress和targeted affinity LTP按canonical runtime gate执行；无法稳定触发的Force、开放receiver计数或transport-offline窗口不以user-space smoke伪造，继续由阶段一/二决定性KUnit与request/gate source review承担，并在Stage 3证据中记为Not Run。随后由未参与实现的独立reviewer同时检查ABI ordering和remote-gate neutralization；所有validation-only文件在提交前恢复。

### 2026-07-15 - Stage 3 Affinity 模块形状修正

**Review Feedback:** 实现中间态把两个affinity syscall与共享helper合并进单个`sched/api/affinity.rs`，不符合当前`sched/api`一个syscall一个实现文件的模块规范，也会让setter与getter不同的copy/lookup/permission ordering难以独立review。

**Approved Correction:** 同一scheduler ABI owner内改为`affinity/{mod.rs,sched_setaffinity.rs,sched_getaffinity.rs}`；`mod.rs`只保存两个入口真实共享的native-word常量与target resolution，setter拥有copy-in、permission、decode、normalization、submit与errno mapping，getter拥有len validation、encode与copy-out。各syscall的phase order和ABI注释留在自己的文件。canonical implementation write set已同步；该结构修正不扩大core/public API，不改变R1 contract、验证floor或停止条件，旧单文件中间态不得进入冻结diff。

### 2026-07-15 - Stage 3 `cpu_set_t` ABI Owner 扩张批准

**Review Feedback:** kernel affinity adapter与focused app虽然按native-word raw syscall工作，但都直接自建`usize`/byte mask，`anemone-abi`没有Linux userspace `cpu_set_t`对应类型。继续保留会让ABI layout、容量和后续wrapper各自猜测，违反单一ABI truth。

**Approved Expansion:** 将`anemone-abi/src/process.rs`加入Stage 3 write set，只在`process::linux::sched`定义1024-bit `CpuSet`、native-word layout常量和纯位操作。kernel adapter复用word常量，但仍按caller `cpusetsize`接受zero/short/exact/long输入并只复制kernel domain所需前缀；focused app改用`CpuSet`存取mask。该扩张不修改`SchedConfig`、core owner、syscall集合、fixed-owner affinity contract或LTP接受边界，也不要求此阶段新增`anemone-rs`公共wrapper。

### 2026-07-15 - Stage 3 实现、审查与 Gate 关闭

**Change:** 在`anemone-abi::process::linux::sched`建立1024-bit、native `unsigned long` word布局的`CpuSet`及当前focused调用需要的纯位操作；kernel affinity adapter只复用word layout，raw syscall继续按caller `cpusetsize`复制kernel-domain前缀，不把固定struct或raw mask带入scheduler core。affinity按目录拆为`mod.rs`、`sched_setaffinity.rs`和`sched_getaffinity.rs`，分别保持setter的copy-in、lookup、kernel-task拒绝、permission、normalization、patch顺序与getter的len/alignment、lookup、coherent snapshot、copy-out顺序。rv64/la64增加asm-generic syscall 122/123，focused app、rv64 pretest rootfs和local-test入口完成接线；没有新增`anemone-rs`公共wrapper。

**Independent Review:** 未参与实现的独立reviewer检查最终冻结diff、request/gate owner和两份runtime log。首轮Apollyon 0、Keter 0；两个Euclid仅为implementation旧单文件路径和transaction阶段记录位置/Closure不一致，均在本次状态同步中修正，窄复核后最终Apollyon 0、Keter 0、Euclid 0。review确认一个syscall一个文件、`CpuSet` ABI owner、raw zero/short/exact/long语义、errno ordering、fixed-owner migration rejection、single affinity truth、publish前到terminal receive的既有全局gate、handler不取gate以及focused stress均符合R1；本RFC producer graph对KETER-DYNATTR-001的neutralization成立，wait-core KETER-WAIT-001仍保持Open。

**Agent-run Validation:** `just build`通过；`just app build --arch riscv64 sched-attr-test`与loongarch64对应构建均通过。单CPU`build/stage3-singlecpu-cpuset.log`记录167项enabled KUnit全部通过，4项affinity KUnit、fair-test以及focused app的local、errno和permission case通过，remote stress明确`SKIP single CPU`；取得本阶段证据后在无关full-profile LTP期间结束QEMU，该profile不记为完整运行。SMP=2 targeted日志`build/stage3-smp2-cpuset-schedule.log`同样记录167项KUnit全部通过，focused app在CPU `(1,0)`间完成128轮双向remote priority/affinity submission；glibc和musl的`sched_setaffinity01`、`sched_getaffinity01`各4项TPASS且case exit为0，运行最终正常shutdown。schedule group中Stage 4/5尚未实现的policy/attr syscall产生的TCONF/TFAIL不归为Stage 3回归，也不把整个schedule profile写成PASS。

**Static / Docs Validation:** `just fmt sched-attr-test --check`通过；`just fmt kernel --check`只报告未手工维护的generated `kconfig_defs.rs`和`platform_defs.rs`既有whitespace drift，未报告本阶段authored source。`git diff --check`与新文件no-index whitespace检查无告警。runtime后`conf/platforms/qemu-virt-rv64-pretest.toml`和`anemone-apps/user-test/ltp/profile.txt`均精确恢复并确认零diff。状态同步后`mdbook build docs`通过，仅报告既有large search-index warning。

**Not Run:** la64 runtime、完整all-profile LTP、Force/open-receiver计数以及IPI allocation/target-offline窗口的用户态触发未运行；前两项不属于Stage 3最低runtime gate，后两类不以不稳定smoke或test-only hook伪造，继续由阶段1/2决定性KUnit和本阶段request/gate source review承担。Stage 4及后续ABI均保持Not Started / Not Run。

**Source / Worktree Boundary:** 最终实现只落在Stage 3批准write set；没有修改`SchedConfig`、request/IPI/wait-core、Task storage、procfs、clone、minimal/la64 rootfs、Kconfig或competition testcase。validation-only platform/profile不进入提交；工作树中的`AGENTS.md`属于用户改动，继续保持unstaged。

**Stop Conditions:** 最终没有task migration、`Task::cpuid`修改、跨CPU RunQueue lock、raw mask进入core、第二affinity truth、handler互等、gate泄漏、lost completion或为LTP静默接受migration-required mask。Stage 3 Implementation、独立review、双架构build、rv64单CPU/SMP=2 runtime与targeted affinity LTP gate关闭；RFC整体保持Active，Stage 4尚未开始。

### 2026-07-15 - Stage 4 Legacy Policy / Parameter / Query ABI 启动

**Phase:** 阶段 4；Implementation In Progress，independent review / runtime Not Run。Stage 3已经由双CPUremote gate stress、targeted affinity LTP与独立review关闭；本阶段不重新打开core transaction、request/gate、affinity或wait-core contract。

**Preflight:** 当前tracker没有未neutralize的Apollyon或Keter。live `SchedConfig` snapshot、semantic discipline patch、latest-config permit与local/remote `submit_config_patch()`能够承载legacy policy/parameter setter和getter；RR interval只允许通过class/config owner提供full configured quantum的窄只读投影，不能读取`RtEntity` remaining budget、rotation或RunQueue。若实现需要credential进入owner handler、class-private runtime进入ABI、current新schedule entry、wait-core变化或write-set外owner surface，立即停止并回写RFC/write-set review。

**Write Set Lock:** kernel实现严格限于`sched/api/mod.rs`与新增`sched/api/policy/{mod,sched_setscheduler,sched_getscheduler,sched_setparam,sched_getparam,sched_get_priority_min,sched_get_priority_max,sched_rr_get_interval}.rs`，仅在需要窄configured interval投影时修改`sched/config.rs`和`sched/class/{mod,rt,fair/mod}.rs`；`class/mod.rs`只能转发RT owner的configured full-quantum ticks，不能暴露`RtEntity`或runtime。ABI允许修改`anemone-abi/src/process.rs`中既有`process::linux::sched` owner的shared `SchedParam`/policy/reset常量，以及`anemone-abi/src/syscall/{riscv,loongarch}.rs`；focused runtime只修改`anemone-apps/sched-attr-test/**`并复用同一ABI定义。`anemone-apps/user-test/ltp/profile.txt`仅允许targeted schedule验证时临时选择，运行后必须恢复并确认无diff。按用户明确要求，一个syscall独占一个`.rs`文件；`policy/mod.rs`只承载legacy family真实共享的byte-copy、target/permission helper与typed errno映射，各入口独有的copy/lookup/validation/projection顺序不得上提。该同owner目录化和两项最小owner扩张不改变R1 contract或syscall语义。

**Validation / Review Plan:** focused KUnit覆盖field-to-patch、reset encoding、policy/range和Fair/FIFO/RR interval；focused app覆盖Fair、FIFO、RR transition、priority raise/lower、reset read-back、errno/permission和coherent getter projection。按global floor执行`just build`、双架构app build、全部enabled KUnit、focused app与targeted Stage 4 LTP supported subset；profile恢复后由未参与实现的独立reviewer检查ABI ordering、permission/latest-config、reset owner、interval projection、一个syscall一个文件和write-set。上述实现、验证与review当前均未完成，不预记为通过。

**Approved Preflight Expansions:** 独立reviewer指出，若kernel和focused app各自定义`sched_param`/policy常量会形成ABI并列truth，因此总控批准`anemone-abi/src/process.rs`最小扩张；同理，RR quantum继续由`class/rt.rs`唯一拥有，批准`class/mod.rs`只转发窄configured ticks accessor，禁止adapter/config重复计算或暴露runtime。两项扩张已先同步canonical implementation write set，worker随后才获准写入；R1 accepted contract、阶段顺序、停止条件和验证floor不变。

**Ordering Preflight:** `sched_setparam()`的family validation与submit-side permission并不构成停止条件。入口在copy与lookup后只把raw priority分类为`Fair`或`Realtime`参数，并用一次coherent snapshot做family mismatch早拒绝；该snapshot不复制mode、旧priority、nice、reset或affinity，也不生成complete replacement。permission随后产生窄permit，owner transaction仍以`ReconfigureParameters`对latest config重复family validation并在permit check前返回typed `InvalidParameters`。focused probes必须区分missing target + bad range、unauthorized family mismatch与family-matching permission denial。

### 2026-07-16 - Stage 4 实现、审查与 Gate 关闭

**Change:** 在`anemone-abi::process::linux::sched`增加shared `SchedParam`以及Linux policy/reset常量，rv64/la64接入asm-generic syscall 118至121和125至127。kernel legacy family按`policy/`目录拆为七个syscall独占文件；shared module只保留byte-oriented `sched_param` copy、target/credential permit和typed errno映射。setter保持top-level scalar/null、copy-in、lookup、positive semantic validation、permission、owner latest-config validation与commit顺序；`sched_setscheduler()`提交discipline replacement并独占reset，`sched_setparam()`只提交`ReconfigureParameters`并保持nice/reset/mode。getter从单个`SchedConfig` snapshot投影。Fair interval为一个effective tick、FIFO为zero、RR经class-owned窄facade读取full configured quantum，不读取remaining budget、rotation、membership或RunQueue。

**Independent Review:** 未参与实现的独立reviewer对最终冻结diff完成只读审查，覆盖ABI ordering/errno precedence、unaligned byte copy、kernel-task fail-closed、credential到窄permit的边界、owner latest-config重验、reset clear denial、coherent getter、interval owner、一个syscall一个文件、focused app前向进展和write set。最终结论Apollyon 0、Keter 0、Euclid 0；没有Stage 4停止条件。runnable external target保持Fair，blocked target只在父进程完成RR/priority mutation后由pipe唤醒，测试握手不会让RT child饿死producer。

**Agent-run Validation:** rv64 `just build`通过；随后`./scripts/run-user-test-rv64.sh etc/sdcard-rv.img build/stage4-schedule-rv64.log`重建rootfs/kernel并正常运行至guest shutdown。日志记录`Running 174 tests...`与`All tests passed!`；新增policy projection/range/reset、`sched_setparam()` family以及configured interval KUnit均为`ok`。focused `sched-attr-test`的Fair/FIFO/RR、priority raise/lower、reset read-back/clear denial、errno/permission、unaligned copy、queued runnable和pipe-blocked detached target case全部通过；既有remote-gate stress在本次单CPU运行明确`SKIP single CPU`，其双CPUtransport/gate证据仍由已关闭Stage 3拥有。

**Targeted LTP:** 同一日志中的glibc与musl schedule group各自运行Stage 4指定的`sched_setscheduler01/02/04`、`sched_setparam01..05`、`sched_getscheduler01/02`、`sched_getparam01/03`、priority min/max和`sched_rr_get_interval01..03`；这些supported case均为zero failed / zero broken，RR interval 03各有一个libc variant TCONF而raw syscall variant通过。整组summary为attempted 62、passed 52、failed 9、infra_failed 0、skipped 1，不能写成全组PASS：未列入Stage 4 target的`sched_setscheduler03`因缺少`getrlimit()`而TBROK，`getcpu01`因未实现`getcpu()`失败，Stage 5才实现的`sched_setattr/getattr`因ENOSYS失败。这些结果不通过legacy兼容映射隐藏，也不归为Stage 4回归。

**Cross-architecture / Static / Docs Validation:** `just app build --arch riscv64 sched-attr-test`与loongarch64对应构建通过；临时切换`qemu-virt-la64-pretest`、用`conf/rootfs/pretest-la64.toml`生成rootfs后，la64 `just build`通过，再恢复`qemu-virt-rv64-pretest`。`just fmt sched-attr-test --check`、`git diff --check`和`mdbook build docs`通过；kernel format check只报告未手工维护的generated `kconfig_defs.rs` / `platform_defs.rs`既有whitespace drift，未报告Stage 4 authored source。runtime后`anemone-apps/user-test/ltp/profile.txt`逐字节恢复并确认零diff。

**Not Run:** la64 runtime、完整`all` profile和独立Stage 4 SMP=2 policy stress未运行；它们不属于本阶段最低runtime gate。Stage 3已经用SMP=2双向remote priority/affinity stress关闭shared request/gate vertical slice，本阶段没有用单CPUapp冒充新的跨CPU证据。

**Source / Worktree Boundary:** 最终实现只落在批准后的Stage 4 write set；没有修改core transaction、request/IPI、wait-core、Task storage、clone、procfs、rootfs manifest、runner、Kconfig或competition testcase。validation-only profile和platform选择均已恢复；工作树中的`AGENTS.md`属于用户改动，提交时继续保持unstaged。

**Stop Conditions:** 最终不需要读取class-private runtime、直接操作queue、为unsupported policy做静默映射、把credential带入owner handler、增加current schedule entry或修改wait-core。Stage 4 Implementation、双架构build、rv64全部enabled KUnit、focused app、targeted supported LTP与独立review gate关闭；Stage 5保持Not Started / Not Run。

### 2026-07-16 - Stage 5 `sched_attr` ABI / Size-Ordering Gate 启动

**Phase:** 阶段 5；Implementation In Progress，independent review / runtime Not Run。Stage 4已经证明supported policy transition、latest-config permission和coherent snapshot projection；本阶段只增加attr ABI adapter，不重新打开core transaction、request/gate、class runtime或legacy policy contract。

**Preflight:** 当前tracker没有未neutralize的Apollyon或Keter。live `UserReadSlice<u8>`能够按size分别验证/复制known prefix与future tail，`UserWriteSlice<u8>`能够先验证getter完整`usize`范围再只写`min(usize, 56)`，setter的best-effort size write-back也能局限在attr adapter；因此当前不需要修改global user-copy contract，P3停止条件未命中。既有`policy` target、credential-to-permit与typed submit helper可以复用，但setter/getter不同的top-level、copy、lookup与projection顺序不能上提成通用wrapper。

**Approved Write Set Correction:** 用户明确指出原Stage 5单文件`sched/api/attr.rs`布局不合适，随后进一步把最终形状收窄为现有`sched/api/policy/`下的独立`attr/`子目录。canonical kernel write set现为`sched/api/mod.rs`、`policy/mod.rs`与`policy/attr/{mod,sched_setattr,sched_getattr}.rs`：`attr/mod.rs`只拥有共享纯size/copy helper，setter拥有size/tail/version/policy/permission/patch顺序，getter拥有usize/lookup/snapshot/full-range/copy-out顺序；两个syscall各自独占一个文件，不与legacy sibling平铺共享helper。该同scheduler ABI owner目录化不改变R1 contract、syscall集合、验证floor或停止条件，也不授权修改core patch、global user access、wait-core或class runtime。

**Approved ABI Owner Expansion:** 用户进一步明确批准Stage 5必要时扩张到`anemone-abi`定义`sched_attr` Rust结构。总控确认kernel adapter与focused app都需要同一Linux 6.6 layout、known sizes和attr flags，批准将`anemone-abi/src/process.rs`加入write set，只在`process::linux::sched`定义shared `SchedAttr`、VER0/VER1 known-size与flag常量；不得加入syscall wrapper或scheduler行为。kernel `policy/attr/mod.rs`继续独占copy/tail/size negotiation，raw ABI不得进入config/request/class。该扩张避免并列ABI truth，不改变R1可见语义、验证floor或停止条件。

**Remaining Write Set Lock:** rv64/la64 syscall number owner可增加asm-generic 274/275；focused runtime只修改`anemone-apps/sched-attr-test/**`并复用shared `SchedAttr`，`anemone-apps/user-test/ltp/profile.txt`只允许targeted schedule验证时临时选择并必须恢复。不得修改core config/request、IPI、Task storage、clone、procfs、global user access、rootfs/runner、Kconfig或competition testcase；出现其它owner surface真实需求时必须先上报并同步canonical write set。

**Validation / Review Plan:** focused KUnit与app覆盖size 0/47/48/55/56/future/PAGE_SIZE边界、zero/nonzero tail、size write-back attempt、tail fault、util field-presence、missing-target cross-error、Fair/FIFO/RR projection、reset、inactive fields、dormant nice与failure-no-mutation。按global floor执行`just build`、双架构focused app build、全部enabled KUnit、focused app和targeted LTP supported branches；profile恢复后由未参与实现的独立reviewer逐行对照Linux 6.6 matrix审查ABI ordering、raw containment、permission/latest-config与publication前failure。上述实现、验证和review当前不预记为通过。

### 2026-07-16 - Stage 5 实现、审查与 Gate 关闭

**Change:** 在`anemone-abi::process::linux::sched`增加Linux 6.6 `SchedAttr`、VER0/VER1 known size和attr flag常量，rv64/la64接入asm-generic syscall 274/275。kernel沿既有policy owner增加`policy/attr/`子目录：`mod.rs`只承担两个入口共享的byte-oriented size读取、known-prefix copy、future-tail检查、getter range validation与best-effort size write-back；`sched_setattr.rs`独占copy-in、tail/version、lookup、util field-presence、policy/range、permission和patch submit顺序，`sched_getattr.rs`独占unsigned `usize`、lookup、snapshot、完整range validation和projection/copy-out顺序。Fair nice按Linux范围归一，FIFO/RR保持dormant nice；inactive deadline fields不进入core，unsupported deadline policy和util flags保持显式`EINVAL`。

**Independent Review:** 未参与实现的独立reviewer逐行检查最终source形状、Linux 6.6 size/copy/errno matrix、raw containment、permission/latest-config、projection与publication前failure。首轮发现getter把Linux `unsigned int usize`误建模为Rust `usize`，以及若干预期失败probe没有证明config不变；修正为入口接收`u32`后再扩为host `usize`，增加高32位caller值被ABI截断的probe，并在47/PAGE_SIZE/read-only write-back/nonzero tail/tail fault等整组失败完成后统一核对snapshot。同步修正implementation中的旧单文件路径。窄复核后Apollyon 0、Keter 0、Euclid 0，未命中Stage 5停止条件，允许进入runtime gate。

**Agent-run Validation:** rv64 `just build`通过；`./scripts/run-user-test-rv64.sh etc/sdcard-rv.img build/stage5-schedule-rv64.log`重建rootfs/kernel并正常运行至guest shutdown。日志记录`Running 180 tests...`与`All tests passed!`；三项新增setter semantic/field-presence、两项共享size/prefix及一项getter projection KUnit均为`ok`。focused `sched-attr-test`的attr size/tail、errno、Fair/FIFO/RR projection、permission及failure-no-mutation case全部通过，完整focused suite打印`END all available cases passed`；既有remote-gate stress在本次单CPU运行明确`SKIP single CPU`，其SMP=2证据继续由已关闭Stage 3拥有。

**Targeted LTP:** 同一日志中glibc与musl的`sched_getattr02`各4项全部TPASS；`sched_setattr01`各有missing pid、null attr和nonzero flags三个supported subtest TPASS，唯一TFAIL是用`SCHED_DEADLINE`要求成功，而deadline明确不在R1支持集合，因此该case整体仍计为failed；`sched_getattr01`也只因setup先请求unsupported deadline而TFAIL。runner整组case summary为attempted 62、passed 54、failed 7、infra_failed 0、skipped 1，不能写成全组PASS；其余失败还包含既有`getcpu()`/`getrlimit()`缺口。相对Stage 4同profile的group-level passed case count 52，本阶段glibc与musl两侧的`sched_getattr02`从failed变为passed，使该计数增加到54；没有把`sched_setattr01`的三个supported subtest误算成完整case通过，也没有用静默deadline映射隐藏失败。

**Cross-architecture / Static / Docs Validation:** `just app build --arch riscv64 sched-attr-test`与loongarch64对应构建通过；临时切换`qemu-virt-la64-pretest`、用`conf/rootfs/pretest-la64.toml`生成rootfs后，la64 `just build`通过，再恢复`qemu-virt-rv64-pretest`并重跑`just build`通过。`just fmt sched-attr-test --check`与`git diff --check`通过；kernel format check只报告未手工维护的generated `kconfig_defs.rs` / `platform_defs.rs`既有whitespace drift，未报告Stage 5 authored source。runtime后`anemone-apps/user-test/ltp/profile.txt`与platform选择均精确恢复并确认零diff。状态同步后`mdbook build docs`通过，仅报告既有large search-index warning。

**Not Run:** la64 runtime、完整`all` profile和独立Stage 5 SMP=2 attr setter stress未运行；它们不属于本阶段最低runtime gate。Stage 3已经用SMP=2双向remote priority/affinity stress关闭shared request/gate vertical slice，本阶段没有用单CPUapp冒充新的跨CPU证据。

**Source / Worktree Boundary:** 最终实现只落在批准后的Stage 5 write set；没有修改core config/transaction、request/IPI、wait-core、Task storage、clone、procfs、global user access、rootfs/runner、Kconfig或competition testcase。validation-only profile和platform选择均已恢复；工作树中的`AGENTS.md`属于用户改动，提交时继续保持unstaged。

**Stop Conditions:** 最终不需要改变global user-copy contract、把raw `SchedAttr`带入scheduler core、增加第二config truth、读取class-private runtime、绕过latest-config permission或为LTP静默接受deadline/util。Stage 5 Implementation、双架构build、rv64全部enabled KUnit、focused app、targeted supported LTP与独立review gate关闭；Stage 6保持Not Started / Not Run，RFC整体仍为Active。

### 2026-07-16 - Stage 6 旁路审计、Runtime Acceptance 与收口启动

**Phase:** 阶段6；source audit与runtime acceptance开始，final review / closure Not Run。Stage 1至5均已由各自implementation、validation与独立review gate关闭；Stage 3已经提供SMP=2双向remote setter / gate contention证据，Stage 5最新rv64日志已经覆盖180项enabled KUnit、单CPUfocused app和两套libc schedule profile，因此本阶段先核对这些证据是否完整匹配最终acceptance，不机械重复同一运行。

**Preflight / Register Boundary:** `ANE-20260622-IRQ-OFF-HEAP-ALLOCATION`仍为Open，`ANE-20260713-SCHED-RT-NOIRQ-BUCKET-ALLOCATION`仍为Active，wait-core `KETER-WAIT-001`仍Open；Stage 6不得把任何一项误写为已修复。focused `sched-attr-test`已是rv64 pretest长期回归资产，Stage 3接线不是temporary routing；validation-only SMP platform/profile仍必须保持零diff。当前RFC owner内没有未neutralize的Apollyon/Keter。

**Write Set Lock:** source-audit finding默认只允许落在Stage 1至5既有owner文件；当前已确认的validation gap只允许在`anemone-apps/sched-attr-test/**`补齐clone/reset继承矩阵。closure文档限于public RFC四页、当前transaction与索引、当前双周devlog、`docs/src/rfcs.md`和必要导航；register只在真实状态变化时修改。不得新增kernel capability、syscall、compat、Kconfig、test-only setter或扩大owner surface；若audit发现semantic缺口，立即停止并回到对应RFC gate。

**Planned Evidence:** 执行canonical八组`rg`旁路审计并逐类核对storage/accessor、ABI adapter、diagnostic/assertion、unpublished constructor、local/remote submission、unsupported rejection与non-production archived source；补齐focused clone/reset matrix后重跑focused app。rv64/la64 kernel/app build、180项KUnit、单CPUABI/error-ordering、SMP=2 remote stress和schedule supported subset若其日志、source revision和validation-only恢复状态仍匹配当前HEAD，则作为本阶段可复用证据；任何覆盖不足再补跑。最后由未参与实现的独立reviewer审查完整R1 diff、runtime分类、register边界和冻结closure文档。

### 2026-07-16 - Stage 6 Source Audit 与 Runtime Acceptance 完成

**Source Audit:** canonical八组`rg`全部执行并逐项分类。`AtomicNice`、`set_nice()`与`inherit_nice_before_publish`零命中；production `SchedEntity` mutation token只在scheduler class / `RunQueue` owner内构造，唯一configured publication位于`RunQueue::apply_config_patch()`。`sched/class/eevdf.rs`中的旧signature属于未接入module graph的archived source，不是live bypass，也不在本RFC越界清理。`SchedConfig`命中只属于唯一storage、coherent accessor、typed patch/request、owner transaction、unpublished constructor、ABI adapter和owner-local KUnit；syscall number只在rv64/la64 ABI owner和对应一个syscall一个文件的handler接线中出现。scheduler IPI只发送single-owner`Box<SchedRequest>`，broadcast在任何复制、分配或enqueue前常开断言拒绝；request body和sender各自只有一次消费。remote gate只存在于`request.rs`的remote publish-to-terminal-receive窗口，local/getter/handler不取gate；one-shot Force只结束receive-local Latch round，不改变persistent terminal phase。affinity命中只属于config truth、clone/procfs/getter projection、fixed owner assertion与ABI adapter；unsupported policy/flag命中只属于静态query domain、显式拒绝或focused test。

**Owner Proof / Corrections:** `Task::sched_config()`提供单锁coherent snapshot，priority/affinity/policy/attr getter与procfs都从该snapshot投影且不发送IPI；clone只读取一次parent snapshot，再由scheduler unpublished constructor应用reset规则和fresh class payload。`RunQueue::apply_config_patch()`在detach前完成zombie、projection与latest-config permit检查，按current / queued / detached分类，识别exact no-op，publication后只执行不可失败attach tail并保持post-state assertion。预审发现Stride KUnit保留了直接`publish_config()`的test-only nice setter；该setter已删除，class test改为只消费构造时配置，dynamic nice的pass/membership保持证明继续由真实`RunQueue` transaction KUnit拥有。ABI owner审计同时修正canonical、tracker、transaction与Linux matrix的旧措辞：userspace representation/layout/constants由`anemone-abi::process::linux::sched`统一拥有，kernel raw copy/parse/ordering/conversion只在`sched/api`，raw类型不进入config/request/class。为闭合这一跨页漂移，Stage 6 closure docs write set最小扩到`backgrounds/linux-6.6-sched-uapi-matrix.md`；不改变R1 contract、ABI值、kernel owner或验证floor。

**Current-diff Build / KUnit:** 当前冻结候选上`just build`通过；临时切换`qemu-virt-la64-pretest`并生成`conf/rootfs/pretest-la64.toml`对应rootfs后，la64 `just build`通过，再恢复`qemu-virt-rv64-pretest`并重跑`just build`通过。`just app build --arch riscv64 sched-attr-test`与loongarch64对应构建均通过。`build/stage6-kunit-rv64.log`记录180项enabled KUnit全部通过，点名修正后的`test_weighted_charge_preserves_ready_snapshot`与owner transaction的`test_config_patch_current_and_queued_fair_roles`均为`ok`；同次focused app的clone/reset和全部available case通过。该次使用原始`all` profile，在KUnit/focused证据完成后通过QEMU console结束，随后已开始的full-profile LTP不作为Stage 6结果。

**Runtime Acceptance:** `build/stage6-schedule-rv64.log`在当前focused app上正常运行至guest shutdown：180项enabled KUnit全部通过，完整ABI/error-ordering/permission/projection及新增clone/reset matrix打印`END all available cases passed`，single-CPU remote stress按设计明确skip。glibc与musl schedule group合计attempted 62、passed 54、failed 7、infra_failed 0、skipped 1，与Stage 5分类一致：supported legacy scheduler、priority、affinity、RR interval及attr error branches继续通过；Deadline成功分支、`getrlimit()`和`getcpu()`等未支持边界不伪装为成功。Stage 3的`build/stage3-smp2-cpuset-schedule.log`继续提供双CPU `(1,0)` 之间128轮双向remote priority/affinity submission、gate contention与正常shutdown；Stage 3之后request、oneshot、processor、RunQueue及SMP platform没有production变化，因此该证据仍覆盖当前shared remote path。

**Static / Docs Validation:** closure同步后`just fmt sched-attr-test --check`、`git diff --check`与`mdbook build docs`通过；mdBook只报告既有large search-index warning。`just fmt kernel --check`只报告xtask生成且由`.gitignore`排除的`kconfig_defs.rs` / `platform_defs.rs`既有whitespace drift，未报告Stage 6 authored kernel source；不手工修改generated文件。

**Register / Asset Boundary:** `ANE-20260622-IRQ-OFF-HEAP-ALLOCATION`保持Open，`ANE-20260713-SCHED-RT-NOIRQ-BUCKET-ALLOCATION`保持Active，wait-core `KETER-WAIT-001`保持Open；没有register状态变化。`sched-attr-test`由`conf/rootfs/pretest-rv64.toml`和user-test local routing长期拥有，继续作为scheduler ABI pretest资产；validation-only profile与SMP platform均已逐字节恢复且零diff。工作树中的`AGENTS.md`仍是用户改动，不属于本事务。

**Not Run:** la64 runtime、Stage 6新的SMP=2 attr/legacy-policy专项stress与完整`all` LTP收口运行未执行。la64由当前kernel/app build覆盖；shared remote request/gate由Stage 3双CPUvertical slice与当前source proof覆盖；`all` profile不替代本RFC的schedule supported-subset分类。Force精确窗口、target-offline与fatal IPI allocation仍不以不稳定userspace smoke或test-only hook伪造，分别由owner-local KUnit/source proof和accepted limitation承担。

**Independent Final Review:** 未参与Stage 6实现的reviewer审查从Stage 0基线到当前冻结候选的完整R1实现线，而非只看最后修正；同时核对canonical八组审计、两份Stage 6日志、Stage 3 SMP=2复用证明、双架构current-diff build、ABI owner跨页一致性、register/Not Run/asset边界和closure候选。pre-closure结论为Apollyon 0、Keter 0、Euclid 0，未命中Stage 6或R1停止条件，允许只做状态与导航同步。

**Closure Narrow Review:** 首轮状态/导航窄审为Apollyon 0、Keter 0、Euclid 3，暂不允许暂存：R1修订接受日期被closure日期覆盖、tracker残留未来时态、transaction引用了不存在的后续记录。本候选已恢复R1接受日期、把focused验证改为Completed transaction事实，并以本段替代悬空引用；final recheck结论为Apollyon 0、Keter 0、Euclid 0，未命中R1 / Stage 6停止条件，允许暂存提交。

## Open Items

- 本 RFC owner内当前无开放 Apollyon、Keter或 Euclid。
- wait-core [KETER-WAIT-001](../../rfcs/sched-wait-refactor/tracking-issues.md#keter-wait-001synchronous-remote-placement-不能组合进-cross-cpu-ipi-completion) 继续 Open；R1 remote gate只neutralize scheduler request producer graph。
- Checkpoint 2A、2B、Stage 3、Stage 4、Stage 5与Stage 6全部关闭。

## Closure

事务Completed；R1全部阶段、runtime acceptance与完整实现线独立review已经关闭，RFC状态同步为Closed。wait-core `KETER-WAIT-001`与两项allocation register边界继续由各自owner保持Open / Active，不阻塞本事务完成。
