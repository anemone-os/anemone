# Dynamic Scheduler Attributes 迁移实施计划

**状态：** Active
**最后更新：** 2026-07-16
**父 RFC：** [RFC-20260714-sched-dynamic-attributes](./index.md)
**不变量：** [不变量需求](./invariants.md)
**当前修订：** R1
**事务日志：** [2026-07-15-sched-dynamic-attributes](../../devlog/transactions/2026-07-15-sched-dynamic-attributes.md)

本文只定义 planned gates、write set、review、验证与停止条件，不记录已经执行的 checkpoint 事实。实现开始后，执行结果只追加到对应 transaction devlog；只有阶段顺序、write set、验证 floor 或停止条件变化才回写本文，contract / owner / ABI / 接受边界变化必须先回写 `index.md`、`invariants.md` 与 tracking issue。

## 迁移原则

- 实现顺序固定为：文档与 live-source 前置 gate、dormant one-shot、Phase 2A mechanical/dormant preparation、Phase 2B core transaction 与既有 priority 原子切换、affinity 与双 CPU vertical slice、legacy scheduler ABI、`sched_attr` ABI、旁路审计与收口。
- 每个代码 checkpoint 必须保持可构建；前一 checkpoint 未通过独立 review 和验证 floor，不扩大下一阶段 write set。
- `SchedConfig`、class-private runtime、physical membership、wait state、IPI transport 与 UAPI 各自只有一个 owner。不得为分阶段方便增加并列 policy/nice/affinity truth、task-local request state或第二 queue。
- `AtomicNice` 删除、新 `SchedConfig` storage 安装、existing `getpriority()` / `setpriority()`切到owner transaction、clone/procfs切到新snapshot与remote submission接线必须在Phase 2B同一个原子checkpoint完成。Phase 2A只能准备不驱动production behavior的typed model、behavior-preserving module move与IPI `Copy -> Clone` mechanical变化；不得提前安装第二份nice/config truth或发布request path。
- local、remote、single-target 与 multi-target setter 最终只调用同一个 owner-CPU `ApplyConfigPatch` transaction；syscall adapter 不读取 stale snapshot 拼完整 config。
- one-shot 只复用公开 `Latch` capability，不修改 wait identity、completion、parkability 或 placement contract。若现有 `Latch` 无法支撑 accepted one-shot 协议，停止并回到 RFC review，不越过 wait-core visibility。
- one-shot 不使用 `Event`：Event的Force predicate retry方向可复用为语义参考，但其API要求调用时不持guard，且listener `VecDeque`会为single-consumer channel增加独立同步域和潜在IRQ-off扩容；阶段1只在channel phase lock内保存bounded trigger slot，不扩展Event contract或allocation面。
- `REMOTE_SCHED_REQUEST_GATE` 明确复用现有 `Mutex<()>`，不新增自定义 `AtomicBool + Event` gate。它是临时 syscall-domain 约束；实现必须保留退出条件和 KETER-WAIT-001 链接，不能把 stress 通过写成 wait-core 已修复。
- 现有 IPI message、Fair heap与 RT bucket 的 IRQ-off allocation继续服从 fatal OOM / register 接受边界。本 RFC 不增加预留、rollback、fallible queue或 allocation-free transport，也不能扩大 IRQ-off allocator side effect。
- raw Linux layout、flag、selector、errno mapping 和 pointer ordering只存在于 `sched/api`。scheduler core只接收 typed config、patch、permit、mask和 target identity。
- write set 是协调合同。需要新增 owner surface 时先记录原因、文件、contract / gate / 验证影响并等待批准；不得在原 write set 内制造长期 adapter。
- agent-run、user-run 与 Not Run 证据必须分开。build、KUnit 与 source audit不能替代双 CPU stress或用户侧 LTP。

## 全局阶段 Gate

每一阶段开始前确认：

1. [Tracking Issues](./tracking-issues.md) 没有未 neutralize 的 Apollyon / Keter 阻塞当前 gate。
2. 当前阶段依赖的前序 transaction entry 已记录 review、验证结果和未运行项。
3. write set 与模块 owner 一致；发现更自然的 owner 需要扩张时先停止上报。
4. 当前阶段不触碰 `sched/wait.rs`、`sched/latch.rs` 的 accepted contract，也不修改 synchronous remote wake placement。
5. 所有可恢复 UAPI、permission、target、mask与首次 request transport failure 都能在 detach 前结束。
6. 任何测试临时路径都有删除条件，不能留下 test-only setter、隐藏 policy switch、第二 completion flag或 broadcast scheduler request。

每一代码checkpoint退出前至少完成：

- `just build`；
- `git diff --check`；
- 对阶段 write set 的独立 review；
- transaction devlog 追加 agent-run、user-run、Not Run、finding 与修正证据。

新增或修改in-kernel KUnit的checkpoint必须启动pretest并运行全部enabled KUnit，在log中点名本checkpoint新增的focused case；当前runner没有单独的focused runtime filter。只有无法由runtime直接触达、且计划明确列出不变量与caller audit的性质，才允许用source proof代替对应focused case。双CPU并发、用户态ABI与LTP结果不能由source proof替代。

公开 RFC 的导航或 mdBook 页面发生变化时另加 `mdbook build docs`。`just fmt kernel --check` 作为格式 floor运行，但必须把未触碰 generated drift 与本阶段文件分开报告。阶段1至5的验证小节只列本checkpoint相对上述global floor新增的证据，避免重复抄写同一命令。

## 目标模块形状

live `sched/api/mod.rs` 当前只有 `sched_yield`，而本 RFC 会引入多组 UAPI、core config和 remote request lifecycle。实现前按 owner 做目录边界预检，目标形状为：

```text
sched/
  config.rs             # typed config、patch、snapshot、CpuMask、permit、error
  oneshot.rs            # dormant single-producer value channel
  request.rs            # local/remote submission、gate、SchedRequest lifecycle
  api/
    mod.rs
    sched_yield.rs
    priority/            # getpriority/setpriority 与 selector snapshot
    affinity/
      mod.rs              # cross-entry native-word constants与target lookup
      sched_setaffinity.rs
      sched_getaffinity.rs
    policy/
      mod.rs              # policy family shared target/permission helpers
      sched_setscheduler.rs
      sched_getscheduler.rs
      sched_setparam.rs
      sched_getparam.rs
      sched_get_priority_min.rs
      sched_get_priority_max.rs
      sched_rr_get_interval.rs
      attr/
        mod.rs            # shared sched_attr size/copy helper
        sched_setattr.rs
        sched_getattr.rs
```

具体文件可在同一 owner 内收窄，但必须保持：

- `config.rs` 不引用 raw syscall structs/constants；
- `request.rs` 不解析 credential、UAPI 或 class runtime；
- `api` 不取得 entity mutation token、RunQueue guard或 wait-core token；
- `class/runqueue.rs` 拥有 published-task reconfigure transaction，Fair / RT 文件只拥有各自 runtime transition和 placement hook；
- `task/sched.rs` 只保留 TCB private storage bridge、snapshot accessor与 unpublished construction handoff；
- `exception/ipi.rs` 只 transport/forward request，不解释 patch、permit、target state或 result；
- helper 只有在两个以上 ABI family 确实共享相同 ordering 时才上提；不得预先建立通用 syscall framework。

若 `sched/api` 继续堆在单个文件会混合 raw ABI、target selection、permission、remote waiting与copy ordering，则先做上述同 owner 目录化；该拆分与语义阶段可以在同一 checkpoint 内原子提交，但 review必须能分辨 mechanical move和semantic delta。

## 阶段 0：文档、Live Source 与 R0 Acceptance 前置 Gate

### 前置条件

- 本 RFC R0 的 `index.md`、`invariants.md`、UAPI matrix和 tracking issues 已完成文档层 review。
- 当前只有 KETER-WAIT-001 作为下层开放问题，KETER-DYNATTR-001 已由本 RFC 的 gate neutralize。

### 交付

- 逐项核对 live owner、调用路径、锁边界、constructor、syscall number、IPI queue与测试入口。
- R0 接受时原地更新公开 RFC 的状态、修订记录和接受边界；不创建并列 canonical 版本，也不让任何私有工作材料成为公共依赖。
- 建立新的 transaction devlog，并同步 RFC 入口、transaction index、当前双周 devlog、`docs/src/rfcs.md` 与 `docs/src/SUMMARY.md`。
- 在 transaction 中登记后续每个 checkpoint 的初始 write set、review owner、runtime owner与 Not Run 项。

### Source audit

- `Task` 当前仍单独保存 `AtomicNice`，`task/sched.rs` 仍有 direct published setter；确认它们只能在阶段 2 原子切换中删除。
- `SchedEntity`、Fair / Stride、Realtime和`RunQueue`的 current / queued / detached lifecycle hook能否表达 dedicated reconfigure；不得复用 yield、block、wake handoff或普通 preempt伪装。
- `Processor` 能否在同一 local IRQ-off transaction中以 current identity、membership与`TaskSchedState`完成role classification，并在current active dimension变化后只请求full pick。
- `SchedEntity::new_default()` 的调用时刻是否已经能取得online CPU domain；若 boot ordering不能证明，unpublished constructor必须显式接收由caller提供的typed initial affinity，不能读取未初始化`NCPUS`。
- clone在task publish前是否能先取得parent coherent config snapshot、按reset规则构造fresh entity并从继承mask选择fixed CPU。
- 阶段0 source audit时`IpiPayload`仍是`Clone + Copy`，R0最初计划增加`Arc<SchedRequest>`后降为`Clone`；该所有权计划已由R1 supersede。Checkpoint 2B改为single-owner`Box<SchedRequest>`、无通用payload clone，并继续审计全部match/broadcast caller和禁止scheduler request broadcast。
- IPI handler pop message后是否已经释放queue lock；业务transaction和one-shot trigger不得在queue lock内执行。
- `Mutex::lock()` 已拒绝hwirq、IRQ-off、preemption-disabled和active-wait context；gate acquire顺序必须直接复用这些断言。
- `/proc/<pid>/status` 当前从`ncpus()`重建allowed mask；阶段 2 必须切到同一config snapshot，不能保留第二观察truth。
- rv64 / la64 syscall number表、raw user access helper、现有LTP schedule group与pretest rootfs入口是否满足后续write set。
- [IRQ-off heap allocation](../../register/open-issues.md#ane-20260622-irq-off-heap-allocation) 与 [RT noirq bucket allocation](../../register/current-limitations.md#ane-20260713-sched-rt-noirq-bucket-allocation) 条目仍为Active；本 RFC只链接和避免扩大。

### Write set

- 当前公开 RFC 的四份canonical文件、backgrounds索引及UAPI evidence；
- R0接受并启动实现后才新增transaction及其索引、双周devlog和SUMMARY入口。

阶段0不修改kernel、ABI crate、app、rootfs、runner或live build配置。

### 验证

- 公开 Draft 与 R0 acceptance 阶段：`git diff --check`、`mdbook build docs`；
- source audit结果写入transaction，不把假设写成已验证实现事实。

### 停止条件

- live class lifecycle无法在不改变Fair/RT accepted contract的情况下表达reconfigure；
- boot / clone无法在publish前建立合法config与affinity；
- request variant要求改变generic IPI completion/placement owner；
- UAPI matrix与本RFC canonical errno/field contract矛盾。

任一命中都先回到文档层，不启动阶段1。

## 阶段 1：Dormant `sched::oneshot` 原语

### 前置条件

- 阶段0退出。
- live `Latch` 仍提供begin、trigger、cancel、schedule与finish的窄能力，且不需要公开raw wait token。

### 交付

- 新增`sched::oneshot::channel::<T>() -> (Sender<T>, Receiver<T>)`。
- `channel()`只分配shared phase和endpoints，不读取current task、不建立Latch、不发布Waiting。
- `Sender<T>` single-use、non-clone、可跨CPU；`send(self, T) -> Result<(), T>`在channel lock内Release发布terminal phase并detach trigger，锁外触发。
- `Receiver<T>` non-clone、receive前可移动；`recv_uninterruptible(self)`先走terminal fast path，empty时才begin Latch并执行recheck/register。
- shared phase明确覆盖empty、value、sender closed、receiver closed和consumed；payload exactly-once publish/consume/drop。
- `RecvError`在v1只包含`SenderClosed`；普通signal和wait-core Force都不能成为channel terminal error。
- registered wake后固定执行`channel lock内take旧trigger -> 锁外drop -> finish Latch -> 重查persistent phase`；terminal决定返回，empty只允许outcome为Force并loop rearm。
- Force不写channel phase、不关闭receiver、不drop payload；不增加generation、pending-Force或request cancellation state。
- receive wait adapter只使用receive-local `Latch`；不得直接调用要求无guard的`Event` wait，也不得扩展Event API或listener storage。
- channel state使用hardirq-safe bounded lock；任何`LatchTrigger`调用或drop都发生在释放channel lock之后。
- unsafe payload cell、`Send` / `Sync`证明和drop顺序如果需要，只能局限在`oneshot.rs`。

### 模块边界预检

- `sched/mod.rs`只做窄module/re-export，不承载phase logic。
- 不修改`Latch`实现来迎合channel；若公开能力不足，触发停止条件。
- 不把gate、IPI request、scheduler error或UAPI result放入generic channel。

### Write set

- 新增`anemone-kernel/src/sched/oneshot.rs`；
- `anemone-kernel/src/sched/mod.rs`；
- focused KUnit留在`oneshot.rs`或同owner test module。

不得修改`sched/wait.rs`、`sched/latch.rs`、`sched/event.rs`、`exception/ipi.rs`、task fields或syscall代码。

### 可观测性

- terminal fast path、begin、recheck/register、trigger detach、Force round finish/rearm、send/close、consume/drop与phase violation可关联channel debug identity；
- debug identity只用于日志/assertion，不驱动phase或wait决策；
- 普通trigger后phase仍empty、double send/consume、drop漏回收为常开断言级bug。

### 验证

- focused phase tests覆盖send-before-receive、send-between-begin-and-register、send-after-registration；
- sender-drop-before-receive、receiver-drop-before-send、send-after-receiver-close返回原value；
- Force在begin、register、pre-park与park窗口内部retry，repeated Force后最终value/sender-close，Force与terminal竞争时persistent phase决定返回；
- 每轮旧trigger在新Latch begin前清空且锁外drop，payload exactly-once publish/consume/drop；
- trigger在channel lock外执行的source audit；
- pretest运行全部enabled KUnit并在log中点名本阶段新增的one-shot case。

### Review Gate

独立review必须确认：无constructor scheduler side effect、无第二wait truth、无payload/Latch outcome互相推导、无锁内trigger/drop、无clone sender、每条receive slow path都finish自身Latch、Force不关闭receiver且empty只会rearm。存在未关闭Apollyon/Keter不得进入阶段2。

### 停止条件

- 必须扩展wait-core token/placement public surface；
- hardirq sender需要sleepable lock、unbounded scan或锁内allocation；
- Force round cleanup无法在单一phase owner下证明旧trigger已清空，或terminal/empty分支需要额外generation、pending-Force或cancellation truth；
- `channel()`必须返回可恢复allocation error才能继续。

最后一项超出已接受API与OOM边界，应回到RFC，不在实现中改签名。

## 阶段 2：Config、Owner Transaction 与 Priority 原子切换（2A / 2B）

### 前置条件

- 阶段1通过review与runtime验证。
- 阶段0已经证明class lifecycle可增加dedicated reconfigure hook。

### 调整理由

本阶段分成两个独立可review checkpoint。2A只隔离mechanical move与不驱动production behavior的typed foundation；2B才执行唯一真相源与production path的原子切换。这样可以先证明目录owner、IPI clone语义和纯config validation，不把所有风险压进P1。

2B仍不可再按storage、request、priority或clone拆开提交。`SchedConfig`一旦安装为storage，就必须在同一checkpoint删除`AtomicNice`和published direct setter，并让existing priority、owner transaction、remote submission、clone与procfs全部切到final path；否则会留下两份nice truth或可观察的半协议。

### Checkpoint 2A：Mechanical 与 Dormant Foundation

#### 交付

- 新增typed`SchedConfig`、`SchedDiscipline`、`RtMode`、`SchedConfigPatch`、`DisciplineChange`、`SchedParameters`、`CpuMask`、`SchedChangePermit`、`SchedError`和纯validation/projection helper，但不把它们安装进`Task`或published`SchedEntity` storage。
- `CpuMask`先闭合compile-time CPU domain、online normalization和`cpuid in mask`的纯typed操作；raw native-word layout仍留待affinity adapter。
- 将`task/api/priority/**` behavior-preserving搬到`sched/api/priority/**`，继续使用当前唯一`AtomicNice` truth和既有selector/result folding；该现有weak setter只允许存活到紧邻的2B，不增加新caller或adapter。
- 将`IpiPayload`从`Copy`收窄为`Clone`并修正现有TLB、wake、KUnit、stop与broadcast caller；本checkpoint不增加`SchedulerRequest` variant或request handler。
- Fair / RT只能增加不发布production mutation entry的class-local transition validation/factory与focused KUnit；不得从syscall、Task或RunQueue接线。

#### Write set

- 新增`anemone-kernel/src/sched/config.rs`；
- `anemone-kernel/src/sched/mod.rs`；
- `anemone-kernel/src/sched/class/{mod,entity,rt}.rs`；
- `anemone-kernel/src/sched/class/fair/{mod,stride}.rs`；
- `anemone-kernel/src/task/api/mod.rs`；
- `anemone-kernel/src/task/api/priority/**` -> `anemone-kernel/src/sched/api/priority/**`；
- `anemone-kernel/src/sched/api/mod.rs`；
- `anemone-kernel/src/exception/ipi.rs`；
- 对应owner文件内focused KUnit。

不得修改Task storage、`AtomicNice`、RunQueue transaction、clone、procfs、wait-core、rootfs、LTP profile或user ABI。

#### 验证与 Review Gate

- config/patch/permit纯矩阵覆盖exact no-op、supported transition、rejection、affinity normalization与latest-config permit predicate；
- existing priority ABI与selector行为保持；
- source audit确认`SchedConfig`尚未成为第二storage，且没有production request/reconfigure caller；
- pretest运行全部enabled KUnit并点名2A新增case；
- 独立review分开检查priority mechanical move、IPI `Copy -> Clone`与dormant typed model。任何production行为变化、第二truth或提前发布request path都阻塞2B。

### Checkpoint 2B：Final-Shape Atomic Cutover

#### 交付：稳定模型与storage

- 将2A的typed model安装为`SchedEntity`唯一coherent configured storage和snapshot来源。
- `SchedEntity`成为configured attributes、class-private payload与membership metadata的唯一storage；Idle保持非UAPI特殊实体，不伪装为Linux policy。
- 删除Task独立`AtomicNice`字段和`AtomicNice`类型；`Nice`继续是typed value。
- RT configured mode/priority只在config；`RtEntity`只保留RR budget/rotation等private runtime。runtime union tag只能作为payload shape，并以常开断言与config discipline一致，不能反向成为policy truth。
- Fair pass、RT budget/rotation、on_runq和current identity不进入snapshot。

#### 交付：Owner-CPU transaction

- `RunQueue::apply_config_patch()`或等价owner-local入口在local IRQ-off下完成latest-config permit check、exact no-op、role classification、detach、publication与不可失败attach tail。
- role顺序固定为processor current identity、queued physical membership、detached sched state、Zombie fail closed；不能仅凭Runnable推断queued。
- 为Fair / RT增加dedicated current/queued/detached reconfigure hook；不得复用yield、block、wake handoff或preempt入口。
- generic-only与Fair nice patch不改变physical position；active dimension变化按RFC placement表detach/attach。
- RT -> Fair detached/queued/current都由Fair owner按transaction时刻placement floor建立placed pass；不开放普通fresh-pass setter。
- current active dimension变化只设置processor-ownedpending full pick；transaction不直接switch、不携带class-visiblereason。
- publication之后只允许不可失败physical attach tail；existing heap/bucket allocation发生OOM仍是fatal，不转成普通`SchedError`。
- successful completion只在post-state assertion之后发送。

#### 交付：Request、Gate 与 IPI transport

- 新增`SchedRequest`与single-use`Option<SchedRequestBody>`；body拥有target、patch、permit与non-clone sender。
- local target直接调用同一owner transaction，不建channel、不拿gate、不发self-IPI。
- remote target在task context取得全局`Mutex<()>` `REMOTE_SCHED_REQUEST_GATE`，创建request并async publish，随后`recv_uninterruptible()`；Mutex guard持有到channel terminal返回。现有Mutex的CAS fast path与Event slow path就是完整gate实现，不增加第二套atomic/event状态；内部Event只发生在publish/Latch之前的lock acquisition，不承担one-shot completion。
- request transport failure发生在receive前，drop dormant endpoints后释放gate；进入receive后，Force只由oneshot内部finish当前Latch并rearm，不能关闭receiver或释放gate。`SenderClosed`只有在唯一sender连同未来mutation/complete capability都已消失时才是确定失败；handler不能在仍可能提交mutation时提前drop sender。
- 在2A已从`Copy`收窄的`IpiPayload`中增加`SchedulerRequest(Box<SchedRequest>)`，并移除payload的通用`Clone`；`Box`是唯一request owner，single-use body继续防御duplicate execute。
- broadcast只显式复制eligible variant；scheduler request进入broadcast时在任何allocation/enqueue前断言失败，不增加可恢复`NotBroadcastable`错误。
- IPI handler pop后释放queue lock，借用payload并直接执行request method，不clone request、不经过一层语义转发函数，也不取credential、gate或user lock。
- handler第二次execute、empty body或double completion由常开断言暴露。

#### 交付：既有Priority与unpublished child

- 将2A已搬迁的priority handler切到final config/request path，保留现有selector snapshot与multi-target partial-progress folding。
- `getpriority()`逐target读取coherent config snapshot；`setpriority()`生成nice-only patch和narrow permit，不再调用Task setter。
- wrong-owner与nice escalation继续分别映射`EPERM` / `EACCES`；owner CPU按latest config再次检查non-escalating permit。
- clone在publish前读取parent coherent config snapshot，经scheduler unpublished-child factory应用reset-on-fork，构造fresh target payload并清除child reset flag。
- child继承effective affinity并从mask内选择fixed CPU；不复制parent pass、budget、rotation、membership、current或pending state。
- ordinary task/kthread constructors继续走single scheduler facade；任何无法取得合法initial affinity的caller必须显式上报，不硬编码all-CPU第二truth。
- `/proc/<pid>/status`的`Cpus_allowed` / list改从同一snapshot投影；不再由`ncpus()`独立猜测task mask。

#### 模块边界预检

- `class/runqueue.rs`若因role transaction继续增长到同时承载config type、UAPI或request lifecycle，应先把config/request放到独立owner文件；不把核心transaction移到syscall helper。
- `rt.rs`与`fair/stride.rs`只新增runtime transition hook和focused tests；raw policy number、permission、request id不得进入class。
- `task/sched.rs`只做private storage access和unpublished handoff；普通crate caller不能构造mutation token或replace published entity。
- `exception/ipi.rs`必须单独review：确认2A的`Copy -> Clone`机械结果在2B只按request唯一所有权所需进一步收窄为显式broadcast复制，TLB、wake、KUnit与stop payload语义未改变，handler不再无条件clone payload。

#### Checkpoint 2B Write set

- 新增`anemone-kernel/src/sched/request.rs`；
- `anemone-kernel/src/sched/config.rs`；
- `anemone-kernel/src/sched/{mod,processor,nice}.rs`；
- `anemone-kernel/src/sched/class/{mod,entity,runqueue,rt}.rs`；
- `anemone-kernel/src/sched/class/idle.rs`，仅在shared lifecycle trait的exhaustiveness需要同步实现时作behavior-preserving调整；
- `anemone-kernel/src/sched/class/fair/{mod,stride}.rs`；
- `anemone-kernel/src/task/{mod,sched}.rs`；
- `anemone-kernel/src/task/api/{mod,clone/mod}.rs`；
- `anemone-kernel/src/sched/api/mod.rs`；
- `anemone-kernel/src/sched/api/priority/**`；
- `anemone-kernel/src/exception/{mod,ipi}.rs`；`mod.rs`仅允许re-export既有`IpiError`，不改变IPI transport、completion或error contract；
- `anemone-kernel/src/fs/proc/tgid/status.rs`；
- 对应owner文件内focused KUnit。

不得修改wait-core、`sync::Mutex`、`sched::Event`、architecture trap、Kconfig、scheduler default policy、user ABI crate、rootfs或LTP profile。若compile证明architecture IPI caller或额外constructor必须改，先提交write-set扩展。

#### Checkpoint 2B 验证

- config/patch/permit matrix：exact no-op、latest-config escalation、all supported transition与rejection；
- current / queued / detached / Zombie role与post-state assertion；
- Fair pass preserve/fresh placement、RT priority head/tail、RR budget/rotation lifecycle；
- local/remote priority path、multi-target partial progress、request exactly-once与completion-after-commit；
- clone reset矩阵、fresh runtime、affinity-contained CPU选择；
- source audit确认无`AtomicNice`、`Task::set_nice`、published entity replace、第二policy/priority/affinity truth；
- source audit确认scheduler request不可broadcast且误用只会在发送前panic、payload无通用clone、IPI queue lock不跨业务handler、gate不进入handler；
- pretest运行全部enabled KUnit并点名2B新增role/config/request case；existing priority LTP验证local用户态路径；remote runtime留给阶段3的SMP=2 gate，不在2B用source proof写成runtime PASS。

#### Checkpoint 2B Probe / Review Gate

Phase 2B以existing`setpriority()`作为final-shape vertical slice：不增加test-only request variant或临时completion flag，证明ABI target snapshot -> permit -> local/remote submit -> owner transaction -> one-shot return完整贯通。独立review必须确认单一config truth、role transaction atomicity、current resched、request/gate lifecycle、clone inheritance，以及2A mechanical结果没有在2B被重新混入难以review的move。

#### 停止条件

- transaction需要持entity guard跨IPI wait或取得credential/user lock；
- detach后出现普通可恢复allocation/error且没有RFC接受的preflight/rollback；
- class replacement必须复制dormant runtime或从lossy`SchedClassKind`重建config；
- current reconfigure必须直接context switch或携带class-visiblepending reason；
- clone必须先publish再修正config/cpuid；
- request需要修改wait-core remote placement才能工作。

## 阶段 3：Affinity ABI 与双 CPU Remote Gate Vertical Slice

### 前置条件

- Checkpoint 2A与2B均完成，final cutover通过独立review。
- config snapshot和affinity patch已由core KUnit证明，不再在ABI阶段决定storage语义。

### 交付

- 在rv64 / la64 syscall number owner中增加`sched_setaffinity`与`sched_getaffinity`常量。
- `sched/api/affinity/{mod,sched_setaffinity,sched_getaffinity}.rs`实现raw native-word mask copy、short-input zero extension、long-input high-tail ignore、online normalization、permission、migration-required rejection与raw copied-byte return；一个syscall一个实现文件，共享项才进入`mod.rs`。
- setter严格保持copy-in -> lookup -> permission -> mask semantics -> patch ordering；getter保持len validation -> lookup -> snapshot -> copy-out ordering。
- fixed owner `cpuid`不在normalized mask时返回`EINVAL`并记录诊断，不排队migration。
- 增加`anemone-apps/sched-attr-test`作为本RFC focused user-space资产；首阶段只覆盖priority/affinity、errno ordering、snapshot read-back和双CPU remote submission。
- app安装到pretest rootfs，并由现有local-test入口执行；不修改competition testcase、judge或`etc`资产。
- rv64双CPU运行使用validation-only平台配置差异；若临时把pretest`smp`改为2，必须在transaction记录并在提交前恢复，不把个人live配置作为canonical证据。

### 双 CPU stress

focused app至少执行：

1. 通过self singleton affinity尝试识别task固定CPU，并确认getter返回保存mask；
2. 建立位于两个不同owner CPU的task；
3. barrier后由CPU A task修改CPU B target，同时CPU B task修改CPU A target；
4. 在priority与affinity no-op / allowed patch间重复，制造gate contention、send-before-receive与registered receive时序；
5. 断言所有syscall返回、config read-back一致、没有两个仍持有开放receiver且completion仍可能触发wait-core placement的published remote request；
6. 验证transport/target failure在receive前释放gate，后续request仍可推进。

单CPU环境可以运行ABI和local transaction部分，但双向stress必须明确记为Not Run，不能以skip写成pass。

### Write set

- `anemone-kernel/src/sched/api/mod.rs`与`sched/api/affinity/{mod,sched_setaffinity,sched_getaffinity}.rs`；一个syscall一个实现文件，`affinity/mod.rs`只保留两个入口真实共享的native-word常量与target lookup，copy/permission/mask conversion/errno mapping归各自syscall文件；
- `anemone-abi/src/process.rs`，只在`process::linux::sched`定义Linux userspace `cpu_set_t`对应的`CpuSet` layout、native-word常量与无syscall副作用的位操作；raw syscall仍按caller提供的`cpusetsize`处理kernel-domain前缀；
- `anemone-abi/src/syscall/{riscv,loongarch}.rs`；
- 新增`anemone-apps/sched-attr-test/**`；
- `conf/rootfs/pretest-rv64.toml`；
- `anemone-apps/user-test/src/main.rs`，仅增加focused app调用；
- `conf/platforms/qemu-virt-rv64-pretest.toml`，仅作为validation-only `smp = 2`输入，提交前必须恢复并验证无diff；
- `anemone-apps/user-test/ltp/profile.txt`，仅在targeted schedule profile运行时临时选择group，提交前必须恢复并验证无diff。

不修改`anemone-rs`公共wrapper；focused app可直接使用`anemone-abi` syscall number，避免为测试扩大用户库API。`minimal`与`pretest-la64`rootfs不属于P2 minimum write set；只有阶段6确认focused app作为跨架构长期资产保留时才另行接入。若后续真实app需要stable wrapper，另行上报。

### 验证

- affinity mask单元测试：zero/short/exact/long、native-word alignment、online bits、raw return bytes；
- background matrix列出的cross-error precedence probes；
- `just app build --arch riscv64 sched-attr-test`与loongarch64对应build；
- pretest运行全部enabled KUnit并点名affinity/config新增case；单CPU focused app；
- 用户侧或明确授权的agent侧rv64 SMP=2双向stress；
- targeted LTP`sched_setaffinity01` / `sched_getaffinity01`；
- runtime后确认platform与profile validation-only差异已经恢复。

### Review Gate

独立review必须同时检查ABI ordering和KETER-DYNATTR-001 neutralization：具体gate是现有`Mutex<()>`，在request发布前获取并持有到terminal receive返回；handler不取gate；stress没有依赖per-target/per-CPU锁、第二completion channel或自定义CAS/Event gate。Force路径不在本阶段做不稳定的user-space smoke；其`detach旧trigger -> 锁外drop -> finish Latch -> phase empty则rearm`顺序、receiver保持开放和guard不释放由阶段1决定性KUnit与本阶段source review证明。

### 停止条件

- mask需要触发task migration、修改`Task::cpuid`或跨CPU RunQueue lock；
- 双向stress出现handler互等、active-wait Mutex assertion、gate泄漏或lost completion；
- raw mask被存入core，或procfs/clone/getter出现不同truth；
- 为通过LTP而把migration-required mask静默接受。

## 阶段 4：Legacy Policy、Parameter 与 Query ABI

### 前置条件

- 阶段3的remote gate vertical slice在双CPU环境通过；若双CPU验证尚未运行，本阶段不得把remote policy setter声明完成。

### 交付

- 增加`sched_setscheduler`、`sched_getscheduler`、`sched_setparam`、`sched_getparam`、`sched_get_priority_min/max`和`sched_rr_get_interval` syscall number与handler。
- Linux `sched_param` layout与policy/reset常量由`anemone-abi::process::linux::sched`作为userspace ABI单一真相源；kernel侧raw copy、parse与legacy reset解释只存在于`sched/api/policy/`。
- setter按UAPI matrix完成scalar/null、copy-in、lookup、positive policy/parameter、permission、owner latest-config validation与patch提交顺序。
- `sched_setscheduler()`独占legacy reset bit；`sched_setparam()`保持reset。
- getter从一个coherent snapshot投影；`sched_rr_get_interval()`只读configured discipline和class/config常量，不读remaining budget、rotation或runqueue。
- focused app扩展Fair <-> FIFO/RR、FIFO <-> RR、priority raise/lower、reset read-back、current/queued/detached target与permission路径。
- 使用当前已覆盖scheduler syscall cases的LTP schedule group验证supported subset；BATCH/IDLE success expectation明确保留为expected unsupported，不通过兼容映射伪造。

### 模块边界预检

- policy adapter只生成既有semantic patch，不新增`SetScheduler`一类core enum。
- interval conversion若需要class-ownedfull quantum accessor，只增加返回typed duration/ticks的窄只读接口，不暴露`RtEntity`。
- target lookup / credential helper只在ordering完全一致时共享；null/copy/error precedence留在各handler。

### Write set

- `anemone-kernel/src/sched/api/mod.rs`与`sched/api/policy/{mod,sched_setscheduler,sched_getscheduler,sched_setparam,sched_getparam,sched_get_priority_min,sched_get_priority_max,sched_rr_get_interval}.rs`；每个syscall独占一个实现文件，`policy/mod.rs`只保留本legacy family真实共享的byte-copy helper、target/permission helper与typed errno映射，不吸收各入口不同的copy、lookup、validation或projection顺序；
- `anemone-kernel/src/sched/config.rs`，仅在需要窄readonly interval投影时；
- `anemone-kernel/src/sched/class/{mod,rt,fair/mod}.rs`，仅窄configured interval accessor/facade与focused test；`class/mod.rs`不得暴露`RtEntity`或class-private runtime；
- `anemone-abi/src/process.rs`，仅在`process::linux::sched`增加shared `SchedParam` layout与Linux policy/reset常量，不增加syscall wrapper或scheduler行为；
- `anemone-abi/src/syscall/{riscv,loongarch}.rs`；
- `anemone-apps/sched-attr-test/**`；
- `anemone-apps/user-test/ltp/profile.txt`，仅作targeted schedule profile的validation-only选择，提交前恢复。

默认不修改core transaction、request、IPI、wait-core、clone或procfs。若adapter暴露core contract缺口，停止并回写RFC，不在ABI文件补逻辑。

### 验证

- legacy setter/getter field-to-patch与reset encoding focused tests；
- policy/range/null/pointer/missing-target/permission cross-error probes；
- current active change请求full pick，generic-only不请求；
- RR full quantum、FIFO zero、Fair one tick interval；
- focused app运行transition与read-back matrix；
- targeted LTP：`sched_setscheduler01/02/04`、`sched_setparam01..05`、`sched_getscheduler01/02`、`sched_getparam01/03`、priority min/max、RR interval cases；
- 双架构kernel / app wiring build；pretest运行全部enabled KUnit并点名policy/config新增case；runtime后确认profile validation-only差异已经恢复。

### 停止条件

- legacy handler需要读取class-private runtime或直接操作queue；
- unsupported policy必须静默映射才能通过case；
- permission依赖owner handler读取credential；
- current reconfigure需要新的schedule entry或wait-core行为。

## 阶段 5：`sched_attr` ABI 与 Size/Error-Ordering Gate

### 前置条件

- 阶段4证明policy transition与snapshot投影正确。
- UAPI matrix仍是Linux 6.6 size/ordering evidence，canonical R0 feature subset未变化。

### 交付

- 增加`sched_setattr`与`sched_getattr` syscall number和handler。
- Linux `sched_attr` layout、VER0/VER1 size与flag常量由`anemone-abi::process::linux::sched`作为userspace ABI单一真相源；`sched/api/policy/attr/mod.rs`只定义两个attr入口共享的纯size/copy/tail helper，`attr/sched_setattr.rs`与`attr/sched_getattr.rs`各自拥有本入口的phase ordering、semantic conversion或projection。raw struct只在ABI crate、kernel adapter与focused userspace调用边界出现，不进入scheduler core。
- setter实现size 0、48/56、future zero tail、nonzero tail + size write-back、util field-presence、signed-policy sanity、lookup后flag/policy validation与semantic patch。
- getter验证完整user range，只复制`min(usize, 56)`，保持future tail不变，并把output size设为实际known copy size。
- R0只接受reset flag；KEEP_POLICY、KEEP_PARAMS、reclaim、deadline overrun、util clamp和unknown bits返回`EINVAL`。
- inactive deadline tuple忽略但不回显；RT attr nice不覆盖dormant nice，getter对RT nice投影0。
- focused app扩展全部size/tail/error-ordering probe，并检查失败不产生partial mutation。

### Write set

- `anemone-kernel/src/sched/api/mod.rs`、`sched/api/policy/mod.rs`与`sched/api/policy/attr/{mod,sched_setattr,sched_getattr}.rs`；两个syscall各自独占一个实现文件，`attr/mod.rs`只保留两者真实共享的纯size/copy helper，不吸收setter/getter不同的lookup、validation、permission、projection或publication顺序；
- `anemone-abi/src/process.rs`，只在`process::linux::sched`定义shared Linux 6.6 `SchedAttr` layout、VER0/VER1 known-size与attr flag常量，不增加syscall wrapper或scheduler行为；kernel和focused app不得复制该layout/常量；
- `anemone-abi/src/syscall/{riscv,loongarch}.rs`；
- `anemone-apps/sched-attr-test/**`；
- `anemone-apps/user-test/ltp/profile.txt`，仅作targeted schedule profile的validation-only选择，提交前恢复。

不得修改core patch维度来支持KEEP_PARAMS/util/deadline；这些是future contract，不属于R0。

### 验证

- size 0、47、48、55、56、57..PAGE_SIZE、>PAGE_SIZE；
- short known prefix zero-fill、future zero/nonzero tail、size write-back attempt和tail fault；
- util flag + short struct、missing target + unsupported flag/policy、bad output + missing target等matrix probes；
- Fair/RT/FIFO/RR attr projection、reset read-back、inactive tuple与dormant nice；
- targeted LTP`sched_setattr01` / `sched_getattr01/02`的supported branches；Deadline success branch记录expected unsupported；
- focused app、双架构kernel / app wiring build；runtime后确认profile validation-only差异已经恢复。

### Review Gate

独立ABI review逐行对照background matrix。不能用“总体像Linux”替代size、copy和errno precedence检查；所有ordinary failure必须证明发生在config publication前。

### 停止条件

- user-access helper无法表达required partial copy / full-range validation而需要改变全局user-copy contract；
- handler需要stale snapshot拼完整config；
- LTP success expectation要求扩大到Deadline/BATCH/IDLE/KEEP_PARAMS；
- size/tail failure在owner transaction后才可见。

## 阶段 6：旁路审计、Runtime Acceptance 与收口

### 前置条件

- 阶段1至5全部通过各自review gate。
- focused app已覆盖全部R0 ABI和双CPUremote path。

### 旁路审计

至少执行并分类：

```text
rg -n "AtomicNice|set_nice\(|inherit_nice_before_publish" anemone-kernel/src
rg -n "sched_entity|with_sched_entity_mut|SchedEntityMutToken" anemone-kernel/src
rg -n "SchedConfig|SchedConfigPatch|ApplyConfigPatch|SchedChangePermit" anemone-kernel/src
rg -n "SYS_(SCHED|GETPRIORITY|SETPRIORITY)" anemone-kernel anemone-abi
rg -n "IpiPayload|SchedulerRequest|send_ipi_async|send_ipi_wait_result" anemone-kernel/src
rg -n "REMOTE_SCHED_REQUEST_GATE|oneshot::channel|recv_uninterruptible" anemone-kernel/src
rg -n "Cpus_allowed|cpus_allowed|affinity|cpuid\(" anemone-kernel/src
rg -n "SCHED_BATCH|SCHED_IDLE|SCHED_DEADLINE|KEEP_PARAMS|UTIL_CLAMP" anemone-kernel/src/sched
```

每个命中分类为：唯一storage/owner accessor、ABI adapter、diagnostic/assertion、unpublished constructor、local/remote submission、legacy bypass、unsupported feature rejection或必须删除的temporary test path。

### 必须证明

- published scheduler mutation只有owner transaction；
- getter只从coherent config snapshot投影，不发IPI；
- raw ABI不进入core/class；
- no-op、generic-only、active dimension change与discipline replacement按矩阵执行；
- request、sender和body exactly-once；scheduler request不可broadcast；
- gate是现有`Mutex<()>`且只覆盖remote request publish-to-terminal-receive窗口，local/getter/handler不依赖gate；Force不释放gate，channel返回时request已提交结果或确定失去未来mutation capability；
- clone、procfs、priority getter与affinity getter读取同一truth；
- wait-core KETER仍Open，temporary gate删除条件仍在；
- IRQ-off allocation register和RT bucket limitation未被误写为修复。

### Write set

- source-audit finding默认只在阶段1至5已经批准的owner文件内修正；需要新owner surface时先走扩展申请；
- `anemone-apps/sched-attr-test/**`、pretest rootfs和`user-test` local routing，只用于补齐或收敛已批准的focused验证资产；
- public RFC的`index.md`、`invariants.md`、`implementation.md`、`tracking-issues.md`；
- R0 transaction、transaction index、当前双周devlog、`docs/src/rfcs.md`、`docs/src/SUMMARY.md`；
- 仅在真实状态变化时更新register / current limitations。

本阶段默认不新增kernel capability、syscall、兼容路径、Kconfig项或测试专用setter。audit暴露semantic缺口时停止并回到对应RFC gate，不以“收口修正”为由静默扩大实现。

### Agent-run验证

- `just build`；
- rv64与la64 kernel / focused app build；
- pretest运行全部enabled KUnit，并从log确认本RFC各阶段新增case全部执行；
- focused app单CPU路径；
- source audit与`git diff --check`；
- `just fmt kernel --check`，分开报告unrelated generated drift；
- 公开RFC/transaction/navigation更新后的`mdbook build docs`。

### User-run或明确授权的Runtime验证

- rv64 SMP=2双向remote setter stress，覆盖gate contention、send-before-receive、registered receive与重复迭代；
- focused app完整ABI / error-ordering / clone-reset matrix；
- schedule profile中的priority、affinity、legacy scheduler、attr与RR interval supported subset；
- 必要时la64 targeted smoke；未运行必须明确记录。

stock LTP中Deadline、BATCH、IDLE success expectation不作为整case completion gate。必须记录具体supported branch结果和expected unsupported branch，不能把TFAIL统一归因或为通过整case扩大R0。

### 独立最终 Review Gate

review覆盖完整RFC diff而非只看最后阶段。pass要求无未关闭Apollyon / Keter属于本RFC owner；Euclid有明确处置、owner、验证和回写路径。reviewer必须显式确认config/state owner、role transaction、failure atomicity、IPI/request lifecycle、one-shot lock/drop、gate neutralization、ABI containment、clone/procfs truth与accepted OOM边界。

### 收口

- transaction追加最终agent-run、user-run、Not Run、LTP分类与review结论；
- RFC状态从Accepted for Implementation更新为Closed；
- tracking issues保留KETER-DYNATTR-001 neutralized依据，并继续链接Open的KETER-WAIT-001；
- register只更新真实受影响条目，不删除IRQ-off allocation限制；
- 如果focused app是长期回归资产则保留并记录owner；任何temporary user-test routing、SMP配置或debug instrumentation在提交前删除/恢复。

## 可观测性清单

实现期日志/trace/assertion必须能回答：

1. request route是local还是remote，caller/target/owner CPU分别是谁；
2. patch修改哪些semantic dimension，permit是unrestricted还是non-escalating；
3. owner role是current、queued、detached还是Zombie；
4. detach/attach使用哪个old/new class、RT bucket placement或Fair pass初始化；
5. config publication、post-state assertion与completion的先后；
6. one-shot走terminal fast path、begin-to-register recheck还是registered trigger；
7. sender-close、receiver-close、Force round cleanup/rearm与payload drop由谁拥有；
8. `Mutex<()>` gate acquire/release、contention，以及仍有开放receiver并可能触发placement的request count；
9. request body take、duplicate execute/double complete异常；
10. ABI failure属于parse/copy/lookup/permission/owner validation/transport/transaction哪一阶段。

日志字段只服务诊断，不参与role、permission、placement、phase或completion决策。request id、channel id等诊断字段旁必须注明这一点。

## Probe / Vertical Slice Gates

### Gate P1 - Existing Priority Final-Shape Cutover

**Hypothesis:** existing `setpriority()`可以在不保留`AtomicNice`或direct setter的情况下，经final config/request/owner transaction完成local与remote target。

**Protected Goal / Invariant:** 单一nice truth、owner-CPU mutation、multi-target partial progress、`EACCES` / `EPERM`映射与completion-after-commit不得削弱。

**Minimum Write Set:** Checkpoint 2B完整原子write set；2A只提供已经独立review的mechanical/dormant foundation，不增加test-only transport或临时policy setter。

**Validation Floor:** pretest全部enabled KUnit并点名focused core case、existing priority LTP local路径、local/remote source proof、build与independent review；remote runtime由阶段3 P2关闭。

**Failure Signals:** 第二nice truth、remote bypass、credential lock进入handler、detach后ordinary error或request completion早于post-state。

**Write-back:** 执行事实进transaction；阶段/write set变化进本文；owner/invariant变化进`index.md` / `invariants.md`与tracking issue。

**Exit:** final shape成为阶段2正式实现；失败则回到RFC review，不保留probe adapter。

### Gate P2 - Two-CPU Mutual Remote Setter

**Hypothesis:** 全局task-context `Mutex<()>` gate足以让scheduler remote request一次只保留一个仍有开放receiver、completion仍可能触发wait-core placement的请求，从而在wait-core修复前排除双向IPI handler completion环。

**Protected Goal / Invariant:** wait-core继续拥有placement；IPI handler不取gate；one-shot使用Latch内部消费Force outcome并只按persistent phase返回；gate复用现有Mutex而不复制AtomicBool/Event状态；不复制wait state、不增加第二mailbox、不改变fatal OOM边界。

**Minimum Write Set:** 阶段3 affinity adapter、focused app、rv64 pretest rootfs入口、validation-only SMP平台与profile选择；不修改wait-core或IPI remote wake。

**Validation Floor:** SMP=2多轮A -> B / B -> A同步stress、wake-capable request count assertion、request/read-back一致性与正常shutdown。Force内部retry、terminal竞争与gate保持由阶段1/2B决定性KUnit和阶段3 source review覆盖，不要求本阶段user-space Force smoke。

**Failure Signals:** 两条published request同时保有开放receiver并可触发placement、Force导致receiver关闭或gate提前释放、terminal phase之后request仍能提交新mutation、handler互等、active-wait lock assertion、lost completion、gate未释放或必须用per-target lock补救。

**Write-back:** 结果进transaction；若gate不能neutralize，重开KETER-DYNATTR-001并停止后续remote setter；共享placement问题仍写回wait-core tracker。

**Exit:** 保留final gate并进入阶段4；wait-core future修复后删除gate并复跑同一stress。

### Gate P3 - `sched_attr` Size/Error Ordering

**Hypothesis:** current user-access primitives足以在`sched/api/policy/attr/{mod,sched_setattr,sched_getattr}.rs`局部表达Linux 6.6 known-prefix、future-tail与cross-error ordering，不需改变全局copy contract。

**Protected Goal / Invariant:** raw ABI containment、failure-before-publication、known size 56与R0 unsupported feature boundary不得削弱。

**Minimum Write Set:** `anemone-abi`中的shared attr representation、policy目录内的size/copy helper与两个独立syscall文件、syscall numbers和focused app；默认不修改global user-access。

**Validation Floor:** background matrix全部focused probes、supported LTP branches、build与ABI review。

**Failure Signals:** 必须全局改变user-copy semantics、future tail被覆盖、errno precedence漂移或unsupported flag进入core。

**Write-back:** ordering实现事实进transaction；ABI contract变化必须回到RFC canonical文本与KETER-DYNATTR-002复审。

**Exit:** 通过后成为正式attr adapter；失败则删除/回退未闭合handler并停止阶段5。

## 总体停止边界

以下任一情况出现时停止当前gate并回到RFC、tracking issue或write-set review：

- 需要修改wait identity、Latch contract、logical completion或stale-safe placement owner；
- 需要让IPI handler取得sleepable gate、credential、user lock或跨CPU RunQueue lock；
- 需要保留AtomicNice、第二policy/priority/affinity truth或task-localrequest state；
- 需要在detach后返回普通可恢复错误但没有accepted preflight/rollback contract；
- 需要为了LTP支持Deadline/BATCH/IDLE、KEEP_PARAMS、util clamp、migration或bandwidth control；
- 需要把current reconfigure伪装成yield/block/wake/preempt或直接在handler切换；
- 需要通过per-target/per-CPU gate、第二mailbox、busy-spin flag或broadcast request替代accepted remote protocol；
- 需要把Force暴露成one-shot terminal error、跨remote gate直接调用Event wait、修改Event listener/allocation contract，或绕开现有`Mutex<()>`另建`AtomicBool + Event` gate；
- 需要把fatal OOM改成syscall error或新增侵入式allocation recovery；
- 需要修改shared public API、owner surface或write set但尚未批准。

以下情况不继续扩大设计争论：

- 同一owner内具体private helper名称或文件细分，不改变target module boundary；
- Safe级日志字段、format或test helper命名；
- stock LTP明确要求R0非目标feature成功；
- wait-core future transport具体实现尚未确定，但本RFC gate仍满足neutralization；
- 没有runtime执行授权或可用环境；把验证记为Not Run并等待用户证据即可。

## 实现期反馈记录

- 2026-07-15 Checkpoint 2B review反馈指出：`NotBroadcastable`不是可恢复transport failure，因为broadcast caller全部是内核；同时以`Arc<SchedRequest>`满足全payload `Clone`会把broadcast需求反向泄漏为request共享所有权。R1改为single-owner`Box<SchedRequest>`、无通用`IpiPayload::Clone`、handler借用payload、broadcast仅显式复制eligible variant，并在任何发送副作用前断言scheduler request误用。目标、owner、UAPI、completion、gate与exactly-once目标保持不变，但明确的request ownership invariant和broadcast failure contract发生变化，因此递增R1；执行与复审证据追加当前transaction。

## 修订实施记录

### R1 - Scheduler request 单所有权与 broadcast fail-fast

**Trigger:** Checkpoint 2B pre-commit review确认scheduler request进入broadcast只能来自内核调用方误用，且为了通用payload clone而把request包装为`Arc`错误表达了共享所有权。

**Semantic Delta:** canonical request ownership从`Arc<SchedRequest>`改为single-owner`Box<SchedRequest>`；`IpiPayload`不再通用`Clone`，broadcast只显式复制eligible variant；scheduler request误用在任何allocation/enqueue前panic，不进入可恢复`IpiError`。R0的目标、owner、UAPI、remote gate、completion和exactly-once接受边界不变。

**Write Set / Gates:** 保持Checkpoint 2B kernel write set；docs同步`index.md`、`invariants.md`、本文、tracker、transaction与RFC导航。旧Arc diff的review结论作废，Box冻结diff重新完成独立review。

**Validation Floor:** `just build`、全部enabled KUnit、fair-test local priority vertical slice、IPI/request source audit、`git diff --check`、kernel format check与`mdbook build docs`。

**Transaction:** [当前R1实施事务](../../devlog/transactions/2026-07-15-sched-dynamic-attributes.md)。R0事务尚未Completed，因此本修订在同一活动事务内独立记录，不创建post-close事务。

## Write Set 扩展记录

- 文档层review已批准Phase 2A / 2B使用`sched/class/mod.rs`；`idle.rs`只在shared lifecycle trait exhaustiveness要求时作behavior-preserving同步。
- 文档层review已批准Phase 3把`conf/platforms/qemu-virt-rv64-pretest.toml`和`anemone-apps/user-test/ltp/profile.txt`列为validation-only write set；两者不得进入最终提交，runtime后必须恢复并验证无diff。
- 2026-07-15 Stage 3 implementation review要求affinity ABI遵守`sched/api`现有的one-syscall-per-file模块形状。批准把原单文件`affinity.rs`细化为同owner目录`affinity/{mod,sched_setaffinity,sched_getaffinity}.rs`；该结构维护不改变UAPI、owner、visibility、验证floor或停止条件，`mod.rs`只保留两个入口真实共享的native-word常量与target lookup，不吸收setter/getter各自的copy、permission、mask conversion、errno mapping或ordering逻辑。
- 2026-07-15 Stage 3 implementation review进一步指出，focused app和kernel adapter直接各自以`usize`/byte array猜测Linux CPU-set layout会让ABI truth分裂。批准把`anemone-abi/src/process.rs`加入write set，在`process::linux::sched`定义1024-bit Linux userspace`CpuSet`及native-word常量；kernel只复用word layout并继续按`cpusetsize`复制最小kernel-domain前缀，不能把raw syscall错误收窄为必须传完整`CpuSet`。focused app必须改用该类型，不再自建mask layout。该扩张不改变fixed-owner、normalization、raw copied-byte return或migration rejection contract。
- 2026-07-15 Checkpoint 2B compile integration证明`sched/api`需要按transport error variant完成typed mapping。批准把`anemone-kernel/src/exception/mod.rs`加入2B最小write set，仅re-export`ipi.rs`中已经公开且由`send_ipi_async()`返回的`IpiError`；同时删除为绕过不可命名返回类型而添加的单用途predicate wrapper。该扩展不改变IPI error、transport、ABI或scheduler contract；review需确认scheduler只匹配现有variant，且`exception/ipi.rs`不保留语义重复的薄wrapper。
- 2026-07-15 Stage 4只读preflight确认kernel policy adapter与focused app都需要Linux `sched_param` layout和policy/reset常量。批准把`anemone-abi/src/process.rs`加入Stage 4 write set，在既有`process::linux::sched` ABI owner内定义shared `SchedParam`与常量；kernel继续只在`sched/api/policy/`解释raw bytes、ordering和semantic patch，app不得复制layout/常量。该扩张不改变R1 syscall集合、ABI值、core patch、owner或验证floor。
- 2026-07-15 Stage 4只读preflight确认RR full quantum当前由`class/rt.rs`私有常量唯一拥有，而`sched/api`不能绕过private sibling边界读取。批准把`anemone-kernel/src/sched/class/mod.rs`加入Stage 4 write set，只允许转发`rt.rs`提供的窄configured full-quantum ticks accessor；不得暴露`RtEntity`、remaining budget、rotation或queue state。该扩张避免在config/API重复quantum truth，不改变R1 interval语义、class owner或停止条件。
- 2026-07-16 Stage 5启动前确认原单文件`sched/api/attr.rs`会把setter与getter不同的size/copy/lookup/permission ordering混在一起，并把attr policy family放在legacy `policy/` sibling之外。按用户批准，Stage 5归入`sched/api/policy/attr/{mod,sched_setattr,sched_getattr}.rs`：两个syscall各自独占一个实现文件，`attr/mod.rs`只保存共享纯size/copy helper，`policy/mod.rs`只增加子模块声明并继续拥有跨policy family共享的target/permit/submit边界。该同owner目录化不改变R1 UAPI、core patch、public API、验证floor或停止条件。
- 2026-07-16 用户进一步明确批准Stage 5在需要时扩张到`anemone-abi`定义`sched_attr` Rust结构。为避免kernel adapter与focused app复制Linux layout、size和flag truth，批准把`anemone-abi/src/process.rs`加入write set，只在既有`process::linux::sched` owner内定义shared `SchedAttr`、VER0/VER1 known-size和attr flag常量；kernel继续在`policy/attr/mod.rs`解释copy/tail/size negotiation，各syscall文件拥有ordering与semantic conversion，ABI type不得进入config/request/class。该扩张不改变R1可见ABI、core patch、验证floor或停止条件。

## 结构维护记录

- 计划：Checkpoint 2A将空壳`sched/api`扩为按priority/affinity/policy/attr划分的同owner目录，并behavior-preserving搬迁existing priority handler；Checkpoint 2B再把它切到final config/request path。两者保持syscall ABI不变，不建立shared generic syscall framework。执行证据写入R0 transaction。
- 2026-07-15：Stage 4按用户明确要求把原计划中的单文件`policy.rs`具体化为同一`sched/api` owner内的`policy/`目录，并保持一个syscall一个`.rs`文件。`mod.rs`只承载legacy family真实共享的layout、常量和窄helper；各syscall独有的ABI ordering、copy、validation和projection留在对应文件。该结构维护不改变R1 contract、public API、owner、验证floor或停止条件。
- 2026-07-16：Stage 5继续使用现有`policy/` owner，并增加`attr/`子目录；`attr/mod.rs`保存共享copy/tail helper，`sched_setattr.rs`与`sched_getattr.rs`作为独立syscall实现文件，Linux raw layout由`anemone-abi`单独拥有。该布局让legacy与attr入口共同复用policy target/permit/submit边界，同时让attr family内部共享项不与其余legacy syscall平铺，不建立新的syscall framework。
