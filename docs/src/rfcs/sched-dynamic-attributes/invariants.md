# Dynamic Scheduler Attributes 不变量需求

**状态：** Canonical
**最后更新：** 2026-07-15
**父 RFC：** [RFC-20260714-sched-dynamic-attributes](./index.md)
**适用修订：** R1
**事务日志：** [2026-07-15-sched-dynamic-attributes](../../devlog/transactions/2026-07-15-sched-dynamic-attributes.md)

本文定义 dynamic scheduler attributes、scheduler request、value-carrying one-shot、owner-CPU transaction、snapshot getter、fixed affinity 与 fork inheritance 的状态所有权和证明义务。实施阶段、checkpoint、write set与验证floor由[迁移实施计划](./implementation.md)拥有，不进入本文件的长期contract。

## 闭合条件

文档协议至少同时满足：

1. published task 的 policy、policy parameters、nice、reset flag 与 affinity 只有一个 configured truth。
2. 所有 published-task setter 都进入固定 owner CPU 的 IRQ-off `RunQueue` transaction。
3. local 与 remote setter 使用同一个 mutation entry；remote transport 不产生第二套业务队列。
4. setter syscall 只有在 transaction 成功提交或确定失败后才返回。
5. remote waiter park，不 busy-spin，并关闭 producer-before-park race。
6. getter 返回单 entity coherent snapshot，不使用 IPI，也不承诺未来时刻仍新鲜。
7. current、queued、detached 与 zombie 的 transaction 都有唯一、可断言的 post-state。
8. discipline change 不复制旧 class-private runtime，只通过 target class owner 构造 fresh payload。
9. Fair pass、RT budget、rotation obligation、queue membership 与 configured attributes 不形成并列真相源。
10. fixed affinity 的保存、read-back、clone inheritance、`/proc` projection 与 immutable `cpuid` 一致。
11. permission snapshot 不要求 hardirq 获取 credential lock，latest-config privilege boundary 仍由 owner 检查。
12. fork/clone 不继承 runtime，并完整实现 reset-on-fork contract。
13. 全局 `Mutex<()>` remote request submission gate 保证任意时刻最多一个 published remote scheduler request 仍持有开放 receiver、其 completion 仍可能进入 wait-core placement，且不被误述为配置锁或 multi-target transaction。
14. `Box<SchedRequest>` 是唯一transport owner，`IpiPayload`不提供包含request的通用clone能力；request仍只有一个可`take()`的body，duplicate execute不能复制mutation或completion capability。
15. Linux UAPI layout、size negotiation、field projection、copy ordering、errno precedence 与 unsupported-feature boundary 在 `sched/api` 内闭合，不改变 core patch shape。

任一条件缺失时，只能称为 draft 或 migration intermediate，不能声明动态调度属性实现闭合。

## 非目标

- task migration、owner handoff、load balancing、hotplug 与 NUMA。
- unsupported scheduler policy、deadline/RT bandwidth、PI 与 cgroup scheduling。
- interruptible/timeout/multi-party one-shot channel。
- 全局 atomic multi-target `setpriority()`。
- allocation-free scheduler path 证明。
- wait-core synchronous remote placement 的通用修复；该问题由公开 wait-core tracker 拥有。
- 把现有 IPI allocation 的 fatal OOM 改造成可恢复错误、预留消息或 rollback 协议。
- `SCHED_FLAG_KEEP_POLICY`、`SCHED_FLAG_KEEP_PARAMS`、util clamp、deadline flags 或其它未接受的 `sched_attr` extension。
- 把文件级write set或阶段顺序固化为本文件的长期contract；这些执行边界只由implementation plan和transaction维护。

## 状态所有权

### Configured attributes

`SchedEntity` 的 generic configured attributes 与 class-configured payload共同形成唯一 `SchedConfig`：

- `Nice` 是 nice 唯一真相源；不得保留 `AtomicNice`、cached nice 或 cached weight。
- `reset_on_fork` 只有一份长期状态。
- effective affinity mask 只有一份长期状态。
- Fair/RT/FIFO/RR configured identity 和 RT priority 不得在 `Task`、request cache、procfs cache 或 queue node 中再保存一份可驱动行为的副本。

snapshot 可以复制这些字段作为 observation value，但 snapshot 必须只读、允许 stale，且不能反向驱动 scheduler transaction。

### Class-private runtime

- Fair owner 独占 pass、fresh/placed state、placement floor、heap key snapshot、enqueue sequence 与 Fair current identity。
- RT owner 独占 remaining quantum、rotation obligation、priority bucket 与 RT placement semantics。
- processor owner 独占 current task pointer 与 `PendingResched`。
- `RunQueue` 独占 generic `on_runq` publication 与跨 class membership transaction。
- wait core 独占 task wait identity、completion、park state 与 stale-safe physical wake placement。

generic configuration 不得吸收 class-private runtime。class-private payload也不得缓存 generic nice、affinity、reset flag 或 permission state。

### Nice consumption

只有 Fair class 可以把 nice 映射为 weight/delta。RT/Idle 可以携带 dormant nice snapshot，但不得读取它决定 pick、preempt、budget、bucket 或 placement。

Fair nice update：

- 保留已有 pass；
- queued 时不改变 immutable pass snapshot；
- 后续实际推进 pass 的 transaction 只读取一次新 nice；
- 不回溯重算历史 service。

## Identity 与 capability

### Target identity

- syscall 在 submit 前把数字 pid/TID 解析为 strong `Arc<Task>`。
- request 与 snapshot 使用该 object identity，不在 owner 重新按 TID lookup。
- TID 仅用于 ABI lookup 和诊断，不能作为异步 request correctness key。
- target 进入 zombie 后 setter 返回 `ESRCH`；strong Arc 只保证 lifetime，不让 zombie 重新可配置。

### Owner identity

- `Task::cpuid()` 是 published lifetime 内 immutable owner CPU truth。
- request 只能发往该 CPU；local owner inline 执行。
- affinity mask 不改变 owner identity，也不能授权在其它 CPU 直接修改 entity。
- v1 不存在 request forwarding、owner epoch 或 migration generation。

### Mutation capability

- ordinary task/syscall/procfs caller不能构造 whole-entity mutation capability。
- class-private payload只能由对应 class owner解释和构造。
- `RunQueue` reconfigure transaction 是 published entity mutation token 的唯一生产者。
- unpublished child construction 使用独立的 pre-publish capability，不经过 IPI，也不能复用于 published task。
- remote `SchedRequest` 的 `NoIrqSpinLock<Option<SchedRequestBody>>` 是 transport envelope 内唯一的 execute capability；handler 在 transaction 前 take body，第二次 take 必须常开断言失败。

### Permission capability

`SchedChangePermit` 是由 credential snapshot 派生的窄 transition capability：

- 不包含可变 `CredentialSet` guard；
- 不允许 scheduler 读取 UID 或 capability；
- owner 对 latest old/new `SchedConfig` 应用 permit；
- permit 不能被 request clone 扩张；
- transition denial是typed internal result，不携带Linux errno；syscall adapter按入口映射 `EACCES` 或 `EPERM`；
- diagnostic privilege label 不能反向驱动权限决定。

## UAPI containment 与 failure phase

逐 syscall 的事实表与 testcase mapping见 [Linux 6.6 Scheduler UAPI Matrix](./backgrounds/linux-6.6-sched-uapi-matrix.md)。以下 phase order属于 R0/R1 binding contract，不是implementation建议。

### Boundary representation

- raw `sched_param`、`sched_attr`、policy、flag、selector、native timespec与CPU mask layout只能存在于 `sched/api`。
- advertised `sched_attr` known size固定为56；VER0 48、VER1 56、zero future tail、short-copy zero-fill和`E2BIG` size write-back都在进入semantic patch前完成。
- syscall-level `flags`只接受0；attr flags只接受`SCHED_FLAG_RESET_ON_FORK`。
- unsupported policy/flag、invalid parameter和permission failure必须可区分，不能静默映射到Fair/RT或成功no-op。
- `SCHED_FLAG_KEEP_PARAMS`不得用“getter snapshot -> complete config write”模拟；未来若支持，必须先接受owner-side latest-config semantic patch。

### Setter phase

Legacy scheduler setter按以下顺序：

```text
top-level signed/null validation
-> copy-in
-> target lookup
-> supported policy/parameter validation
-> credential snapshot and permit construction
-> owner-side latest-config validation
-> commit
```

`sched_setattr()`按以下顺序：

```text
top-level null/pid/syscall-flags validation
-> size read, zero-tail check and copy-in
-> util-clamp field-presence check
-> signed-policy sanity
-> target lookup
-> supported policy/attr-flag/parameter validation
-> credential snapshot and permit construction
-> owner-side latest-config validation
-> commit
```

effective size小于56且携带util-clamp flag时，field-presence check在target lookup前返回`EINVAL`。除此之外，positive unknown/unsupported policy与unsupported attr flag在target lookup后分类，因此valid input buffer命中missing target时保持`ESRCH` precedence；invalid size/tail或copy fault仍先返回`E2BIG`/`EFAULT`。scheduler policy/parameter invalidity在permission前返回`EINVAL`。

Affinity setter先复制mask，再lookup target和检查permission，最后normalize online domain并提交；因此bad input优先`EFAULT`，negative/missing affinity pid为`ESRCH`，而unauthorized existing target即使mask为空或需要migration也先返回`EPERM`。这些顺序差异不能被一个改变errno的通用“validate all args” wrapper抹平。

### Getter phase

- getter先完成各自top-level pid/null/usize/flags/len validation，再lookup并取得一个coherent snapshot，最后copy-out。
- `sched_getparam()`的null output在lookup前为`EINVAL`；bad non-null output只在存在target时为`EFAULT`，missing target优先`ESRCH`。
- `sched_getattr()`先验证`usize in 48..=PAGE_SIZE`，lookup后验证完整usize user range，再复制`min(usize,56)`。
- `sched_rr_get_interval()`先lookup和派生interval，再copy-out；missing target优先`ESRCH`。
- `sched_getaffinity()`先验证len覆盖kernel CPU domain且按native word对齐，再lookup、snapshot和copy-out；raw success返回实际copy bytes。
- copy-out failure不回滚、不重取snapshot，也不产生scheduler mutation。

### Reset encoding

- `sched_setscheduler()`独占legacy policy bit `SCHED_RESET_ON_FORK` 的parse和 `sched_getscheduler()` read-back。
- `sched_setattr()` / `sched_getattr()`只使用`SCHED_FLAG_RESET_ON_FORK`。
- `sched_setparam()`保持reset；其它setter只修改自己拥有的patch维度。
- 请求清除已置位reset必须由latest-config permit检查，不能在submit-side stale snapshot上决定。

## `sched::oneshot` channel 协议

### 角色

- `Sender<T>`：single producer，`Send`，不 `Clone`；`send(self, T) -> Result<(), T>` 消耗 endpoint且不等待。
- `Receiver<T>`：single consumer，不 `Clone`，可以在 receive 前移动；`recv_uninterruptible(self) -> Result<T, RecvError>` 消耗 endpoint，并把实际 wait round 绑定到调用时的 current task。
- shared channel state：拥有 payload slot、endpoint lifecycle与至多一个receive-time trigger registration。
- receive-local `Latch`：只在 receiver 确认 channel 仍 empty 后创建，拥有本次阻塞所需的一轮 wait-core active wait。

`Arc<Shared<T>>` 只保活 channel phase storage，不能复制 `Sender<T>` 或产生第二份 producer capability。scheduler request transport 本身由 single-owner `Box` 承载。

### Dormant channel 与状态所有权

`sched::oneshot::channel()` 只构造 shared channel state 和两个 endpoint：

- 不读取 current task；
- 不调用 `Latch::begin_current()` / `ActiveWait::begin()`；
- 不发布 `TaskSchedState::Waiting`；
- 不要求 receiver 固定在 constructor task；
- sender capability 可以在 receiver 首次 receive 前逸出并完成。

channel phase 至少能区分：

- payload 尚未发布；
- payload 已发布；
- sender closed without value；
- receiver closed；
- payload 已消费。

这些状态只拥有 value/endpoint lifecycle。channel另有至多一个`Option<LatchTrigger>` registration slot；该slot只说明“若channel随后terminal，应尝试完成哪一轮wait”，不拥有task是否Waiting、是否Parked、是否已加入runqueue或哪个wake reason胜出。

wait core / Latch 只拥有 wait round lifecycle。它们不拥有 payload 是否初始化，也不能以 `Triggered` 直接推导 value 一定存在。

receive wait adapter 固定为 `Latch`，不使用 `Event`。`Event::listen_uninterruptible()` 在 wait-core `Force` 后重试predicate的方向与本channel一致，但其API要求调用时不持有lock/guard，而remote setter刻意跨receive持有唯一submission gate。Event还维护独立的可复用listener `VecDeque`、exclusive/non-exclusive与quota policy；首次注册可能在`NoIrqSpinLock`临界区扩容，并为single-consumer channel增加第二同步域。one-shot已有hardirq-safe phase lock，只需在同一owner内保存一个bounded `Option<LatchTrigger>`；不得为本channel扩展Event contract或IRQ-off allocation面。constructor dormancy由persistent channel phase与receive-time registration保证，不依赖选择哪一种wait adapter。

channel state transaction必须使用hardirq-safe、bounded synchronization；sender可能在IPI handler执行，不能获取sleepable mutex。trigger必须在释放channel state lock后执行，避免channel lock进入wait-core/placement路径。

### Receive-time registration

`recv_uninterruptible()` 按以下协议运行：

1. 先以 Acquire 检查 terminal phase；已发布 value 或 sender-closed时直接consume/返回，不建立wait round。
2. phase仍empty时，为current task begin一轮uninterruptible `Latch`，并生成唯一trigger。
3. 在channel state transaction中重查phase；仍empty才安装trigger。
4. 若value/closed已在fast path与registration之间发布，不安装trigger；receiver在锁外drop未安装trigger，cancel + finish刚建立的latch，再由persistent terminal phase决定no-switch返回。Force若抢先完成该round，只改变finish outcome，不能覆盖terminal result。
5. trigger成功安装后才允许schedule；wake返回后，receiver先在channel state transaction中take仍存在的registration。sender/close可能已经take，因此`None`是合法竞争结果；任何取出的trigger都在锁外drop。
6. receiver随后finish当前Latch，再以Acquire检查channel phase。terminal phase决定consume/返回；phase仍empty时只允许outcome为`Force`，此时loop并为同一receiver建立下一轮Latch。普通trigger、signal、cancel或unexpected outcome下phase仍empty是常开断言级bug。
7. 新round begin前旧registration必须已经清空；single receiver保证slot中若仍有trigger，只能属于当前round，不增加generation、pending-Force或第二份wait identity。

这组check -> begin -> recheck/register -> schedule -> detach -> finish -> phase recheck关闭send-before-receive、send-between-begin-and-register、send-after-register和Force各窗口。channel value本身是持久结果，不依赖trigger保存跨时间permit；Force只要求重建wait round，不改变channel lifecycle。

### Publication order

成功 send 在channel state transaction中写入payload，以Release发布terminal phase，并detach已注册trigger。registered waiter路径的happens-before contract：

```text
producer writes T
-> Release publish channel phase
-> detach trigger and release channel lock
-> trigger Latch if registered
-> receiver finishes wait round
-> Acquire observes channel phase
-> receiver moves T out exactly once
```

- 没有registered trigger时，send不进入wait core；未来receiver通过terminal fast path的Acquire观察payload。
- payload write 必须先于 trigger。
- receiver 不得在 finish/Acquire 前读取 payload。
- unsafe payload cell 如被采用，所有 unsafe `Send`/`Sync` 证明必须局限于 one-shot module。
- debug-only borrowed flag 不能替代 release/acquire publication。

### Close 与 Force rearm

- sender drop without send持久发布`SenderClosed`，detach并在锁外trigger已注册receiver；尚未receive的receiver随后走terminal fast path。
- receiver在调用receive前drop时只发布`ReceiverClosed`并exactly-once drop pending payload；dormant receiver没有active wait需要cancel/finish。
- `recv_uninterruptible()` 消耗receiver；一旦它创建latch，每一轮都必须在rearm或返回前显式cancel/finish，不能触发`Latch` drop assertion。
- receiver 已关闭时 `send(self, value)` 返回原 value，不写入 abandoned slot。
- 普通信号不能完成uninterruptible receive。
- wait-core Force可以先于channel terminal phase结束当前wait round，但不写channel phase、不关闭receiver、不drop payload，也不进入`RecvError`。receiver清除旧registration、finish Latch并重查phase；仍empty时内部rearm，直到value或sender-closed真正terminal。
- sender drop without value返回`RecvError::SenderClosed`；triggered/terminal不等于payload一定存在。
- scheduler request只有在唯一sender连同未来mutation/complete capability都已不可恢复地消失时，才能把`SenderClosed`解释为确定失败；handler不能在仍可能提交mutation时提前drop sender。
- 任何 endpoint close race 都必须 exactly-once drop payload。

## IPI request protocol

- `IpiPayload::SchedulerRequest(Box<SchedRequest>)` 是唯一拥有的transport envelope，不是scheduler state。
- `SchedRequest` 只保存一个 `Option<SchedRequestBody>`；body拥有target、patch、permit与non-clone sender，`Some -> None` 是 execute/complete capability 的唯一消费点。
- existing per-CPU IPI queue 是唯一 pending queue。
- `IpiPayload`不提供包含request的通用`Clone`；broadcast只显式复制eligible variant。
- scheduler request进入broadcast是内核调用错误，必须在任何allocation/enqueue前断言失败；不能增加可恢复`NotBroadcastable` transport error。
- exception handler pop 后释放 queue lock，再进入 scheduler owner transaction；不能持 IPI queue lock 修改 RunQueue。
- exception layer只转发，不检查 permission、entity、task state 或 patch。
- request completion先写 result，再 send one-shot。
- request 只执行一次；duplicate enqueue/double complete 是常开断言级 kernel bug。
- 首次 request IPI 的可恢复 allocation/target-online failure发生在发布前，不能留下等待一个永不执行 request 的 receiver。
- wait-core remote wake completion继承现有IPI allocation的fatal OOM边界；它不是request transaction可恢复错误，本RFC不增加pre-reservation或rollback。
- 所有 remote request 在发布前获取同一个 `Mutex<()>` `REMOTE_SCHED_REQUEST_GATE`，并持有到 `recv_uninterruptible()` 返回；因此全系统最多一个 published remote request 仍有开放 receiver、其 completion 仍可能触发 `LatchTrigger` 并进入wait-core placement。
- Force只完成当前Latch round；receiver必须detach旧trigger、锁外drop、finish并在phase仍empty时rearm，不能返回或释放gate。gate release因此证明channel已经terminal，且`SenderClosed`必须同时证明request不再拥有未来mutation capability；terminal后的transport envelope可以短暂存活，但不能保留未消费的execution body。不得据此断言所有时序下物理上最多一个request envelope in flight。
- gate 不被 local setter、getter、unpublished child、IPI handler 或 owner transaction获取，也不产生配置或multi-target atomicity。

同步语义属于 syscall transaction，不属于 IPI transport：async send + parked receive 是允许的，busy-spin completion 不是。

## 线性化点

### Setter

setter 的 configured-state 线性化点是 owner CPU IRQ-off `RunQueue::apply_config_patch` 或等价 transaction 中，一次性把完整 old configured view 替换为完整 new configured view 的 publication 点。所有可失败检查必须发生在该点之前；publication 后只允许不可失败的 physical attach tail。

- owner CPU 在 publication 与 attach tail 之间保持 local IRQ disabled，不能进入 scheduler 或处理另一条 RunQueue transaction。
- getter 可以在 publication 后观察完整 new config，但不能观察逐字段中间态；snapshot 不暴露中间 membership。
- successful one-shot result 必须发生在 attach tail 和全部 post-state assertion 之后。
- validation、permission与首次request transport的可恢复failure发生在commit前，不能改变target。
- 一旦 detach old membership，不得在没有 rollback 的情况下返回普通错误。
- local 与 remote path 使用同一个 commit definition。

### Getter

getter 的线性化点是持有 entity observation guard 时完成 `SchedConfig` copy 的时刻。

- user copy 可以在线性化点之后执行。
- user copy fault 不回滚或改变 scheduler state。
- getter 不等待 pending setter；锁获取顺序自然决定它观察 old 或 new完整版本。
- multi-target getter没有统一线性化点。

### Affinity

affinity setter 的 commit 点与其它 patch 相同。保存 mask 时必须同时满足 `cpuid in affinity`；不存在“先成功返回、稍后迁移或稍后修正 mask”的异步尾巴。

## Lock、IRQ 与生命周期

- RunQueue mutation 只依赖 owner CPU + local IRQ disabled，不增加跨 CPU RunQueue lock。
- existing entity storage guard 可以作为 owner transaction 内的短临界区，但不能代替 owner serialization。
- `REMOTE_SCHED_REQUEST_GATE` 的具体类型是 `sched/request` 私有的全局 `Mutex<()>`；现有Mutex已经以CAS fast path和内部Event slow path实现sleepable竞争，本RFC不增加自定义`AtomicBool + Event` gate。Mutex内部Event只用于request发布和Latch建立前的不可中断lock acquisition；它和one-shot都不把Force暴露成普通返回，取得guard后不再进入该Event wait。gate只串行remote request的submit-to-terminal-receive窗口，不保护scheduler state。
- gate 必须在 task context、IRQ enabled、preemption allowed时，于request发布前获取；dormant channel可以在gate前或gate后创建，但一旦`recv_uninterruptible()`建立active wait，就不得再获取gate或其它sleepable lock。
- gate guard刻意跨越one-shot全部receive rounds持有；这是合法的唯一前置guard。其安全性依赖receive内不再获取sleepable lock、IPI handler/completion path不获取gate，以及receive先finish当前Latch、观察terminal phase后才返回并drop guard。
- request submit/receive 期间除该 gate 外，不持 credential、topology、entity、RunQueue、user-space、IPI queue 或 source lock。
- 首次request transport失败发生在receive前；调用侧只关闭或丢弃dormant endpoints，再释放gate，不得伪造一个需要cancel/finish的wait round。
- 一旦进入`recv_uninterruptible()`，normal value、sender closed与Force路径都由receive负责当前round的trigger cleanup和Latch finish；Force后的empty phase必须在函数内部rearm，调用侧只在terminal返回后释放gate。
- hardirq handler不能获取可能正被中断 task 持有的 credential/task普通锁。
- hardirq handler绝不能获取remote request gate；否则会把task-context serialization变成handler等待自身caller的锁环。
- one-shot trigger不能在持有 channel-internal payload borrow 时进入会重入 channel 的路径。
- IPI queue lock在业务处理前释放。
- task strong reference由 request持有到 completion/abort；completion 不得形成 `Task -> request -> Task` 永久环。
- cleanup/drop 先 retire wait、释放 publication，再以常开断言暴露 lifecycle bug。
- wait-core hardirq-safe remote placement落地并通过移除gate后的双向remote setter stress后，删除该临时gate；删除不授权改变RunQueue owner或multi-target ABI语义。

## Physical role transaction

owner CPU 的 role classification 顺序必须以 physical truth 为准：

1. 与 processor current object identity 比较；
2. 检查 generic/class-local queued membership；
3. 检查 detached task sched state；
4. zombie fail closed。

不得仅凭 `TaskSchedState::Runnable` 推断 queued；wait completion/new task publication都允许 logically runnable 但 physical enqueue pending 的 detached 窗口。

### Current

- active scheduling dimension变化前，旧 class必须结束对应 current segment。
- payload replacement前旧 class current identity必须清除。
- new class必须建立合法 current runtime identity。
- active dimension变化后只设置 `PendingResched`，不在 IPI handler直接 context switch。
- generic-only change不得无故消费 RR rotation、重置 RR budget、推进 Fair pass或改变 current identity。

### Queued

- 若 patch 改变 queue key、bucket或 class，必须先按 old payload detach。
- detach 与 attach 之间 `on_runq == false`，且该中间态不能对外 admission/callback。
- post-state 只能在一个 class queue；generic `on_runq` 与 physical membership一致。
- nice-only Fair change不修改 pass/key，因此保持现有 heap entry。
- generic-only change保持原位置。

### Detached

- 不执行 dequeue，也不伪装为 blocked/yield/preempt。
- discipline replacement可以安装 fresh payload。
- target Fair payload必须在未来 wake前拥有合法 pass；RT -> Fair transition由 owner Fair class按当前 placement floor初始化。
- logically runnable / enqueue-pending task的后续 enqueue必须自然读取 new config，不能使用 request-side cached class。

### Zombie

- 不修改 config、runtime、membership或 affinity。
- result 为 `ESRCH`。
- request 持有的 strong target `Arc<Task>` 只延长对象生命周期，不改变 zombie 语义。

## Runtime transition 不变量

### Discipline replacement

- Fair <-> RT 与 FIFO <-> RR 使用 target owner 构造的 fresh private payload。
- replacement发生时旧 queue/current identity已经detach。
- 不复制旧 payload字段到不相关的新 payload。
- generic nice/reset/affinity按 patch和旧 configured value保留，不随 class payload丢失。

### Fair

- nice update保留 pass。
- RT -> Fair fresh pass使用 transaction 时刻的 class-owned placement floor。
- fresh placement不能低于 placement floor。
- queued heap snapshot继续等于 entity pass。
- dynamic update不引入 cached weight、historical recompute或 dormant Fair payload。

### Realtime

- RT priority范围始终为 `1..=99`，数值越大优先级越高。
- RR priority-only update保留 `remaining_ticks`。
- FIFO -> RR产生 full quantum且无 rotation obligation。
- RR -> FIFO销毁 RR runtime。
- `rotation_due` 只属于 active RR current；queued、detached、fresh与FIFO必须为 false/无该状态。
- 结束 active segment的 scheduling reconfigure清除旧 rotation obligation；reset/affinity/dormant nice等 generic change不结束 segment。

### Exact no-op

old/new config完全相同且没有语义变化时：

- 不 detach/attach；
- 不替换 payload；
- 不改变 pass、budget、rotation或enqueue sequence；
- 不设置 pending resched；
- completion仍可以正常返回成功。

## Placement 不变量

queued RT reconfigure：

- priority提高：destination bucket tail；
- priority降低：destination bucket head；
- FIFO/RR mode切换且priority不变：same bucket tail；
- Fair -> RT：destination bucket tail。

queued RT -> Fair：

- pass初始化为当前 Fair placement floor；
- 使用新的 enqueue sequence；
- 不从旧 RT priority构造 Fair ordering credit。

Fair nice update不重建 heap位置；generic-only patch不改变任何 ready order。

current active scheduling change一律请求一次full pick。full pick仍由现有class precedence、pass/bucket与requeue规则决定；reconfigure handler不得预选candidate或携带class-visible resched reason。

## Affinity 不变量

- effective mask非空。
- effective mask只包含当前 kernel CPU domain内的CPU。
- setter复制`min(len, KERNEL_CPU_MASK_BYTES)`并对短输入零扩展；长输入的未知高tail不形成CPU，也不要求zero。
- getter要求len足以覆盖kernel CPU domain且按native `unsigned long`对齐；raw success返回实际copy bytes而不是0。
- affinity入口的negative/missing pid为`ESRCH`；setter copy-in先于lookup，getter copy-out后于lookup。
- immutable `cpuid` 永远在effective mask内。
- set mask包含多个CPU是合法的；固定在其中一个CPU运行不违反allowed-set语义。
- set mask不含current `cpuid`时不得保存、不得成功、不得排队延迟迁移。
- getter、`/proc`与clone读取同一mask truth。
- child cpuid必须从继承mask内选择。
- affinity更新不改变owner、不触发resched、不修改class runtime。

## Permission 不变量

- identity/credential check发生在submit侧，使用caller/target credential snapshot。
- hardirq owner transaction不获取credential lock。
- request只携带narrow permit，不携带guard或可变credential引用。
- config-dependent escalation基于owner看到的latest config。
- same-owner non-escalating permit允许numeric nice增大或不变、同一RT mode内priority降低或不变、RT退出到Fair、设置reset和exact no-op。
- non-escalating permit不能降低numeric nice、进入RT、切换FIFO/RR、提高RT priority或清除受保护reset flag。
- `CAP_SYS_NICE`生成unrestricted permit；wrong-owner、affinity permission或scheduler transition denial映射`EPERM`。
- `setpriority()` nice escalation denial映射`EACCES`，不能与其它scheduler setter的`EPERM`混用。
- permitted request发布后credential变化不撤销request。
- submit-side identity/credential permission failure不得产生IPI或partial mutation。
- owner-side latest-config permit denial可以通过request IPI返回，但必须发生在detach前且不修改target。

## Snapshot getter 不变量

- 一个single-target snapshot中的configured fields相互一致。
- getter只能观察完整old或完整new configured view，不能观察patch逐字段中间态。
- snapshot不包含class-private runtime、membership或pending state。
- getter不发送IPI、不等待full pick、不修改queue。
- `sched_getattr()` / legacy getter / procfs只是同一snapshot的不同projection。
- `sched_getattr()`对Fair投影configured nice和priority 0；对RT投影nice 0和configured priority；deadline/util fields固定为0。
- RT dormant nice只能通过`getpriority()`投影，inactive attr nice不得覆盖它。
- `sched_getscheduler()`使用legacy reset bit，`sched_getattr()`使用attr reset flag；两者来自同一reset truth。
- `sched_rr_get_interval()`对Fair返回一个effective tick、FIFO返回zero、RR返回full configured effective quantum；不得读取remaining budget、rotation obligation或ready queue。
- getter不能从lossy `SchedClassKind`重新猜policy/priority；必须由entity owner生成完整config snapshot。
- `getpriority()` target collection与per-target snapshot分离，不承诺topology-wide atomicity。

## Fork / clone 不变量

- child在publish前完成configured attribute构造。
- child不复制parent class-private runtime、membership、current identity、wait state或pending resched。
- no-reset时继承discipline、nice、affinity；reset flag按父配置处理。
- reset parent为RT时child为Fair/nice 0。
- reset parent为Fair且nice < 0时child nice 0；nice >= 0时继承。
- child reset flag永远清零；parent flag保持。
- child fixed CPU从继承affinity选择。
- fresh payload由target class owner构造，clone code不匹配private enum字段。

## Failure atomicity

commit前完成所有可恢复、可能返回用户错误的UAPI解析、range/policy validation、target resolution、permission、mask normalization和首次request transport allocation。

若owner-side latest-config validation仍可能失败，必须在detach前执行。detach后只允许：

- 完成已验证的commit；或
- 在同一IRQ-off transaction中恢复old payload、membership、position/runtime且证明外部不可观察。

不得：

- 失败后把task留在无queue、双queue或错误class；
- 用panic替代可预见用户输入错误；
- completion先于commit；
- send result失败后重复执行transaction；
- 通过读snapshot再写完整config规避owner-side validation。
- 为模拟`SCHED_FLAG_KEEP_PARAMS`或其它unsupported partial update，在syscall侧拼接stale complete config。

IRQ-off allocation是已接受限制，不是atomicity豁免。新引入且要映射为普通syscall error的fallible allocation若可能发生在detach后，implementation必须增加preflight或同一transaction rollback gate并回写RFC。现有IPI remote wake allocation失败继续进入fatal OOM边界，不映射为syscall error，也不要求本RFC侵入式预留或rollback。

## 禁止退化项

- 保留`AtomicNice`或新增第二份nice/weight cache。
- syscall直接写Task字段或持entity guard跨等待。
- 让raw `sched_attr` / syscall enum进入RunQueue或class owner。
- 接受unknown/unsupported `sched_attr` flag后静默忽略，或把known size错误缩成48来隐藏util fields。
- 使用synchronous IPI/atomic flag spin表达setter completion。
- 在现有IPI queue旁增加无必要scheduler mailbox。
- 把wait-core Force暴露为one-shot terminal error、用它关闭receiver或释放remote gate。
- 在跨越remote `Mutex<()>` guard时直接调用要求无guard的`Event` wait，扩大Event listener/IRQ-off allocation contract，或在现有Mutex之外重复实现`AtomicBool + Event` remote gate。
- 使用per-CPU、per-target或仅单进程gate替代全局remote request gate；这些锁不能排除A -> B与B -> A同时保有开放receiver并形成双向completion边。
- 在one-shot active wait建立后获取remote request gate，或让IPI handler获取该sleepable gate。
- 将oneshot channel phase作为task runnable/park truth，或将Latch outcome作为payload initialized truth。
- clone `Sender<T>`、重复execute request或double-complete。
- owner IPI handler读取credential锁。
- queued task换payload前不detach旧queue。
- 通过普通yield/block/preempt伪装current reconfigure。
- discipline切换保留dormant class runtime。
- affinity set成功但不保存mask，或get返回与clone/procfs不同truth。
- 不含current cpuid的mask返回成功并承诺未来迁移。
- getter为追求“最新”发送同步IPI。
- `sched_getattr()`向RT回显dormant nice，或让RT attr nice修改generic dormant nice。
- `sched_rr_get_interval()`读取RunQueue load、RR remaining ticks或rotation state。
- 把multi-target setpriority描述为全局原子。
- 以诊断id、日志label或request id驱动correctness。

## 可观测性

常开断言至少覆盖：

- owner CPU与target cpuid一致；
- role classification唯一；
- detach前old class/key/bucket/membership匹配；
- attach后new class/key/bucket/membership匹配；
- queued/inactive RT无rotation obligation；
- Fair heap snapshot等于entity pass；
- affinity非空且包含cpuid；
- request exactly-once execute/complete；
- remote request gate保证仍有开放receiver并可能触发placement的published request count不超过一，Force不释放gate，且local/getter路径不依赖该gate；
- one-shot payload exactly-once publish/consume/drop；
- successful completion发生在commit后。

日志/trace至少记录request route、role、semantic patch维度、old/new discipline、placement、pending-resched decision、remote gate acquire/release/contention和失败分类。日志不记录不必要的credential细节，也不参与行为。

## 完成标准

文档协议可以声明闭合，仅当：

1. 本文与index对所有owner、state、linearization、placement、fork、permission与ABI scope无矛盾。
2. [Linux 6.6 Scheduler UAPI Matrix](./backgrounds/linux-6.6-sched-uapi-matrix.md) 的flag、size、errno、projection、copy ordering与testcase分类已折回本文和index。
3. RFC入口已明确与公开Fair/RT/Latch RFC的follow-up关系。
4. review没有未关闭Apollyon/Keter。
5. implementation plan通过文档层review、R0接受并建立transaction前，不开始kernel实现。

实现只能在未来满足以下证据后声明闭合：

- source audit确认没有published setter bypass和`AtomicNice`残留；
- 决定性one-shot KUnit覆盖send-before-receive、send-between-latch-begin-and-trigger-registration、send-after-registration、sender-drop-before-receive、receiver-drop-before-send、Force在begin/register/pre-park/park窗口的内部retry、repeated Force、Force与value/sender-close竞争、每轮旧trigger锁外清理与payload exactly-once drop；role matrix、payload transition、placement、permission与affinity由各自focused test覆盖；
- SMP=2 runtime覆盖两个CPU并发互调调度属性时被remote gate串行，且不存在两个仍有开放receiver、completion仍可能触发wait-core placement的published remote request；Force不释放gate、第二个request不能在第一个channel terminal前越过gate，由one-shot/request KUnit与request/guard source review证明，不要求不稳定的user-space Force smoke；
- build与architecture syscall wiring通过；
- targeted Linux/LTP scheduler/priority/affinity cases验证ABI；
- register limitation没有被误写为已修复；
- transaction devlog记录agent-run与user-run证据、剩余限制和任何RFC feedback。
