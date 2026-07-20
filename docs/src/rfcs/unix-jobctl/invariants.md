# Unix Job Control 目标与不变量

**状态：** R0 / Accepted for Implementation / Not effective
**最后更新：** 2026-07-20
**父 RFC：** [RFC-20260720-unix-jobctl](./index.md)
**适用修订：** R0

本文定义 unix-jobctl 的 contract delta、尚未 cutover 的 target rules 和只服务本方案的 proof obligations。当前已经生效的共享规则以仓库 `docs/src/contracts/` 为准；本页所有 `JOBCTL-*` 与新增 `USER-ENTRY-*` 都是 proposed stable ID，不是 current authority。

## Contract Impact

`UJ-CUTOVER` 是本 R0 target 定义的语义 cutover unit，不单独充当 implementation stage 名称。`implementation.md` 将 Stage 5 映射到整个 `UJ-CUTOVER`；在此之前不能逐项宣称生效。

| Contract ID | 变化 | 当前规则 | Target 摘要 | 生效边界 |
| --- | --- | --- | --- | --- |
| [`SIGNAL-PENDING-001`](../../contracts/signal/pending-routing.md#signal-pending-001--directed-occurrence-只进入对应-pending-owner) | Scoped Exception | private / shared pending 各自持有 occurrence | ordinary occurrence仍由原owner持有；`SIGSTOP` admission直接消费为jobctl control input，不进入pending | `UJ-CUTOVER` |
| [`SIGNAL-PENDING-002`](../../contracts/signal/pending-routing.md#signal-pending-002--group-directed-publication-与-member-notification-分离) | Refine | shared publication先于advisory member notification；`SIGSTOP`使用强制notification | ordinary group occurrence保持publication / notification分离；`SIGSTOP`不发布pending且不force-complete active wait，可选user-execution kick必须guards-out且stale-safe | `UJ-CUTOVER` |
| [`SIGNAL-ACTION-001`](../../contracts/signal/pending-routing.md#signal-action-001--ignored-disposition-在-pending-publication-前生效) | Preserve | ignored occurrence 不进入普通 pending | control side effect 先独立线性化，occurrence 仍服从现有 ignored admission | 全程 |
| [`SIGNAL-ACTION-002`](../../contracts/signal/pending-routing.md#signal-action-002--ordinary-trap-return-才提交异步-action) | Refine | ordinary trap-return fetch 后选择 action | 普通user ThreadGroup的`SIGSTOP`在generation transaction直接stop；另外三种信号最终选择DefaultStop后用窄epoch authority提交同一ThreadGroup stop request；global init不接受stop authority；Stopped期间已经reserved的`SIGCONT`可以收口action但不能取得user-entry permit | `UJ-CUTOVER` |
| [`SIGNAL-TEMP-MASK-001`](../../contracts/signal/temporary-mask-delivery.md#signal-temp-mask-001--current-mask-与-restore-slot-只有一个-task-owner) | Preserve | current mask / restore slot 由 current task Signal state 唯一持有 | jobctl 不复制或终结 temporary-mask token state | 全程 |
| [`SIGNAL-TEMP-MASK-002`](../../contracts/signal/temporary-mask-delivery.md#signal-temp-mask-002--defer-必须先建立-task-private-delivery-handoff) | Refine | reserved target 优先于 ordinary pending delivery，但尚未提交action | reservation提交task-local occurrence claim并退出ordinary queue competition；control cleanup不撤销它，live action仍在后续选择 | `UJ-CUTOVER` |
| [`SIGNAL-TEMP-MASK-003`](../../contracts/signal/temporary-mask-delivery.md#signal-temp-mask-003--handler-commit-或-signal-no-frame-cleanup-终结-restore-responsibility) | Preserve | handler commit或Signal no-frame cleanup终结restore responsibility | jobctl不删除、复制或终结reservation；Signal owner仍通过handler frame、no-frame cleanup或no-return terminal teardown收口 | 全程 |
| [`PROCFS-TASK-STATE-001`](../../contracts/procfs/task-state-projection.md#procfs-task-state-001--当前-tgid-state-只投影-leader-taskstatus) | Refine | 成功读取时投影 leader `TaskStatus` 的 R / Z / S / D；status pair非原子 | 单次 derived snapshot保证character/name一致；Z优先，committed Stopped投影T | `UJ-CUTOVER` |
| [`PGRP-SIGNAL-001`](../../contracts/task/process-group-signaling.md#pgrp-signal-001--processgroup-只拥有成员选择) | Preserve | ProcessGroup 只拥有 membership selection | 不建立 process-group-wide jobctl phase | 全程 |
| [`PGRP-SIGNAL-002`](../../contracts/task/process-group-signaling.md#pgrp-signal-002--每个-threadgroup-独立接受-occurrence) | Preserve | 每个 ThreadGroup 独立接受 signal | 每个 ThreadGroup 独立 stop / continue / report | 全程 |
| [`TASK-LIFE-001..003`](../../contracts/task/thread-group-lifecycle.md) | Preserve | terminal lifecycle / last-member exit / notification ordering | terminal owner不变；jobctl 只承担自身exposure、report与parker cleanup | 全程 |
| [`CHILD-WAIT-001`](../../contracts/task/child-wait.md#child-wait-001--当前-wait-truth-只有-exited-child) | Replace | wait truth 只有 `Exited` | typed selection 同时观察 terminal 与 job-control report，exit 最高优先 | `UJ-CUTOVER` |
| [`CHILD-WAIT-002`](../../contracts/task/child-wait.md#child-wait-002--target-selection-每轮重读-child-relation) | Refine | scan 每轮读取 selector；reap claim只重验 relation与Exited | terminal / report claim都重验 relation、selector与 selected state | `UJ-CUTOVER` |
| [`CHILD-WAIT-003`](../../contracts/task/child-wait.md#child-wait-003--event-只触发-predicate-rescan) | Refine | `child_exited` 只触发 exited predicate rescan | 扩展为 child-status predicate，仍不携带 truth | `UJ-CUTOVER` |
| [`CHILD-WAIT-004`](../../contracts/task/child-wait.md#child-wait-004--peek-与-reap-使用同一-truth不同-claim) | Refine | exited child 支持 peek / reap | report 支持 WNOWAIT peek / exact-once consume，exit 仍 reap | `UJ-CUTOVER` |
| [`CHILD-WAIT-005`](../../contracts/task/child-wait.md#child-wait-005--non-exit-wait-abi-当前-fail-closed-或保持-exit-only) | Replace | stopped / continued 不支持 | wait4 / waitid 提供 stopped / continued ABI | `UJ-CUTOVER` |
| [`USER-ENTRY-001`](../../contracts/task/user-entry.md#user-entry-001--ordinary-trap-return-先完成-signal-arbitration) | Refine | ordinary trap-return 先做 Signal arbitration | ordinary path 增加 lifecycle / jobctl gate并登记 exposure | `UJ-CUTOVER` |
| `USER-ENTRY-002` | Introduce | None（尚未生效） | fresh / clone / exec 与 ordinary return 共享 mandatory arbitration contract | `UJ-CUTOVER` |
| `JOBCTL-STATE-001` | Introduce | None（尚未生效） | ThreadGroup 唯一拥有 phase、reason、continue epoch、exposure 与 report；procfs snapshot只读派生 | `UJ-CUTOVER` |
| `JOBCTL-STOP-001` | Introduce | None（尚未生效） | 不存在 exposed live member 时才提交 Stopped | `UJ-CUTOVER` |
| `JOBCTL-SIGNAL-001` | Introduce | None（尚未生效） | ordinary pending cleanup、reserved claim finality、global-init immunity、`SIGSTOP`直接stop、generation-only `SIGCONT` resume与条件性DefaultStop epoch同序 | `UJ-CUTOVER` |
| `JOBCTL-CONT-001` | Introduce | None（尚未生效） | incomplete cancellation 无 report；Stopped 才产生 Continued | `UJ-CUTOVER` |
| `JOBCTL-LIFE-001` | Introduce | None（尚未生效） | join / detach / exec / terminal 对 exposure 和 park 的局部义务 | `UJ-CUTOVER` |
| `JOBCTL-REPORT-001` | Introduce | None（尚未生效） | child-attached coalesced report、SIGCHLD、wait 与 procfs projection；stopped / continued `si_uid`保持stage-1 `0` | `UJ-CUTOVER` |

## Target 数据模型与实现余地

本页的 target约束逻辑 owner与状态关系，不把当前首选 Rust shape误写成不可调整的 ABI。首选基线由 RFC 正文的[数据模型边界](./index.md#数据模型边界target-与实现基线)给出；实现期可以改变字段名、私有 enum / struct嵌套、membership container、owner-local helper与等价锁实现，只要下列语义不变：

- lifecycle、membership、jobctl phase、first stop reason、exposure和report仍由 child `ThreadGroup` 的同一 owner transaction线性化；
- exposure绑定 live membership identity，不形成 task-local behavioral ack、独立 membership truth或 scheduler state cache；
- `SIGSTOP`直接参与control ordering；另外三种信号的stale DefaultStop不能越过后来`SIGCONT`。首选`ContinueEpoch`是non-allocating checked counter，但target真正保护的是这一ordering，不是固定字段偏移；
- report claim读取并消费当下 owner state，不需要预先选中的 snapshot持有 `ReportId`、generation或其它 claim capability；
- Event、notification、SIGCHLD和procfs只触发重扫或读取 derived snapshot，不携带 durable truth；
- terminal lifecycle始终先于 jobctl phase解释，`Exiting / Exited` 不得在 jobctl内复制为第二 terminal state。

owner-local物理替换必须在 implementation plan / transaction中记录证据与验证方式。若调整会改变 target invariant、owner、lock direction、ABI / visible semantics、accepted limitation或引入 allocation-backed / persistent cross-Signal protocol，则不是实现偏差，而是必须回到 RFC review 的语义变化。

## Target Invariants

### JOBCTL-STATE-001 — ThreadGroup 持有唯一 job-control truth

**规则：** 每个 user ThreadGroup 唯一拥有：`Running / Stopping(reason) / Stopped(reason)` phase、first stop reason、control ordering、live membership exposure和 parent-visible job-control report。首选实现使用 `ContinueEpoch` identity，owner-local等价串行化可以在不改变本 invariant的前提下由实现证据替换。procfs jobctl snapshot必须从该 owner state派生，不得缓存为另一份可变 truth。Signal、ProcessGroup、scheduler、waiter、Event 和 procfs 只能持有 occurrence、immutable snapshot或 wake capability，不能推进或重建 phase。

jobctl phase只在 `ThreadGroupLifeCycle::Alive` 时具有行为意义。`Exiting / Exited` 只由 `TASK-LIFE-*` owner发布；terminal transaction清 exposure与report并释放jobctl parker。实现可以归一化phase，也可以保留明确标注的 inert diagnostic snapshot，但 terminal之后不得再用该phase授权user entry、生成report或覆盖first terminal code。

**Owner：** child `ThreadGroup` job-control transaction。

**依赖：** `PGRP-SIGNAL-001`、`TASK-LIFE-001`。

**违反表现：** scheduler 保存并驱动 stop state、task-local ack 与 group progress 成为并列真相、SIGCHLD/Event payload决定 wait status，或 procfs缓存独立 jobctl state并反向改变 phase。

**Cutover：** `UJ-CUTOVER`。

### JOBCTL-STOP-001 — Stopped 由 user exposure closure 证明

**规则：** ThreadGroup 维护 live member 中“当前可能无需再次经过 mandatory gate 即执行用户态”的 exposure relation。user-to-kernel entry 必须先清除当前 member exposure；Running 上的最终 user-entry gate 必须在允许 architecture transition 前登记 exposure。`Stopping` 只能在不存在 exposed member时提交 `Stopped`。

`Stopped` commit 的含义是：没有 member 正在用户态或已经越过最终 gate；所有仍在内核、ordinary wait、fresh / clone / exec entry 或之后新加入的 member，再次执行用户指令前都必须观察当前 phase。它不表示内核执行已经静止：parent 观察到 Stopped 后，既有 syscall、exception 或 ordinary wait 仍可继续并产生正常的内核可见副作用，只是最终不得越过 user-entry gate。

ordinary wait 的 predicate、timeout、source registration和result owner不因stop改变。wait尚未满足时继续等待；在Stopping / Stopped期间满足或超时时按原协议收口，随后在最终user-entry gate park。`SIGCONT`的jobctl resume side effect只唤醒jobctl parker，不负责完成仍处于原ordinary wait的task；如果其ordinary occurrence本身按Signal规则可递送，由普通Signal notification独立产生原有影响。stop路径不得为ordinary wait制造`EINTR`或替换其真实返回结果。

**Owner：** `ThreadGroup` job-control transaction。

**依赖：** `USER-ENTRY-001`、`USER-ENTRY-002`。

**违反表现：** parent 观察 Stopped 后仍有 task 执行用户指令、completion 依赖 scheduler state snapshot、ordinary waiter 被stop取消或返回synthetic `EINTR`、`SIGCONT` resume side effect误完成原wait，或另设 participant ack / counter 与 exposure relation竞争。

**Cutover：** `UJ-CUTOVER`。

### JOBCTL-SIGNAL-001 — Control generation 与 stale candidate invalidation 同序

**规则：** stop-class 与 `SIGCONT` generation 进入同一 ThreadGroup control-signal transaction；四种stop signal在取得不同authority后共享唯一的ThreadGroup stop engine：

这里的generation指concrete nonzero occurrence已经通过sender-specific target resolution、thread-group relation和permission check，开始进入Signal owner。进入ThreadGroup transaction后仍须重验lifecycle和sender-specific exact target / membership relation；signal-0 probe、拒绝的发送、不存在或已失效的target不得执行cleanup、推进epoch或stop / resume。

global init immunity是generation内部的action-admission规则，不是伪造的permission failure。合法stop-class generation仍执行ordinary opposite-class pending cleanup；`SIGSTOP`随后不建立ordinary pending且不得取得unconditional stop authority，`SIGTSTP` / `SIGTTIN` / `SIGTTOU`仍可按ordinary mask / disposition路径被caught或pending，但live action为default-stop时不得取得conditional authority。`SIGCONT`在global init上仍按普通control generation执行cleanup、epoch推进和ordinary occurrence admission；live phase为Running时resume side effect自然不建立report。

1. 四种stop-class generation先清理全部member-private与shared ordinary pending `SIGCONT`；已经reserved的`SIGCONT`不再属于queue cleanup集合；
2. `SIGSTOP` generation因其不可捕获、不可忽略、不可屏蔽，对global init之外的普通user ThreadGroup在同一transaction中直接提交无条件stop request；它不进入private / shared ordinary pending，不建立reserved delivery，也不调用完成active wait的force notification；发送给global init时只保留第1项cleanup并消费occurrence，不提交stop request；
3. `SIGTSTP` / `SIGTTIN` / `SIGTTOU` generation捕获当前`ContinueEpoch`后按普通Signal disposition / routing接纳occurrence；其pending publication必须包含在ordering domain内，不能在cleanup后late publication；
4. `SIGCONT` generation先推进`ContinueEpoch`，清理全部member-private与shared ordinary stop-class pending，无条件执行一次整个ThreadGroup resume side effect，再按普通Signal语义接纳或丢弃occurrence；已经reserved的stop-class occurrence不被撤销，其中旧`DefaultStop` candidate由epoch mismatch取消jobctl effect；
5. 条件性stop occurrence只有在Signal最终选择`DefaultStop`、目标不是global init、未来适用的orphaned-pgrp suppression未否决且captured epoch仍匹配时，才能提交stop request；global-init immunity或epoch mismatch都只取消jobctl effect，不重排、重新发布或补偿该signal。

reservation是task-local occurrence dequeue / claim的finality，不是disposition或action commit。它终结ordinary private / shared pending competition，但仍按action-selection时的live disposition进入custom handler、ignore、default no-frame consume或default-stop epoch validation。为保持现有观察边界，本RFC不要求改变reserved target在task-private pending snapshot中的投影；该projection不把它重新变成generation cleanup可撤销的queue member。

首选`ContinueEpoch`是opaque、`Copy + Eq`、不提供ordering comparison的owner-local newtype，物理值为checked monotonic `u64`。每个concrete `SIGCONT` generation无论mask、ignore或custom handler都推进一次；counter不得wrapping / alias，不需要`AtomicU64`，也不得替换成每次generation分配的`Arc<()>` capability。只有`SIGTSTP` / `SIGTTIN` / `SIGTTOU` ordinary occurrence携带窄`default_stop_epoch: Option<ContinueEpoch>`或等价snapshot；`SIGSTOP`不携带epoch。该identity不是stop round、report、pending ownership或通用delivery carrier。

如果实现能够证明条件性stop occurrence从owner admission / fetch到DefaultStop commit始终与`SIGCONT`位于同一个不可插入的control transaction，并且该证明不扩大普通Signal锁域、不引入persistent reservation或第二真相，可以省略显式epoch字段；必须在implementation feedback中记录线性化证据。否则`ContinueEpoch`是默认且必须实现的stale-candidate closure。global init之外的`SIGSTOP` generation与stop request本来就在同一transaction内，不依赖该替代证明。

条件性`DefaultStop` commit必须结束当前ordinary signal scan并把控制权交给before-user-entry gate，不能在同一pass继续消费普通asynchronous signal。`Stopping / Stopped`上的ordinary asynchronous signal和非`SIGCONT` reserved delivery保持pending；只有`SIGKILL`、已提交的terminal lifecycle、kernel-generated synchronous signal的no-return terminal action、可合并当前stop的条件性DefaultStop control action，以及此前已经完成occurrence claim的reserved `SIGCONT`可以在重新进入jobctl gate前收口。

reserved `SIGCONT`仍按live disposition选择action：custom action可以提交handler frame，ignore或default no-frame consume可以完成no-frame cleanup。无论哪条路径，reserved retirement只终结该occurrence和temporary-mask responsibility，不授予user entry；一次retirement结束当前ordinary scan，随后只能继续检查支配性terminal / control truth并进入live jobctl gate。handler frame构造失败服从既有no-return terminal path；frame已提交也不能阻止后来`SIGKILL`或committed terminal lifecycle在architecture transition前取得支配。jobctl不删除reservation，也不接管restore slot；Signal owner仍按`SIGNAL-TEMP-MASK-003`通过committed handler frame、no-frame cleanup或no-return terminal teardown收口。

cleanup与`SIGCONT` resume side effect不受mask、explicit ignore或custom handler影响。task-directed与ThreadGroup-directed control signal都遵守相同group side effect。每个concrete `SIGCONT`只在generation transaction中执行一次resume；reserved delivery、ordinary fetch、同步消费和default-action consume都不得重放，default `SIGCONT` action因此只是ordinary no-frame consume。`SIGSTOP` admission直接消费为control input；其它stop-class和`SIGCONT`的ordinary occurrence仍进入原本的private / shared owner。当前RFC延后orphaned-pgrp suppression，因此它作为明确scoped limitation保留；未来只能在条件性DefaultStop admission前抑制，不能进入或分叉ThreadGroup stop engine。

从stop request开始，四种信号共享同一phase、exposure、report、continue、lifecycle与user-entry gate。具体API可以使用typed authority，也可以使用等价owner-private参数；但裸signo不得让未经Signal最终action selection的条件性stop绕过epoch validation，也不得让`SIGSTOP`误走ordinary pending。

**Owner：** ThreadGroup control-signal handoff protocol；pending、disposition和 phase 仍由各自 local owner 持有。

**依赖：** `SIGNAL-PENDING-001`、`SIGNAL-ACTION-001`、`SIGNAL-ACTION-002`、`SIGNAL-TEMP-MASK-001..003`、`JOBCTL-STATE-001`。

**违反表现：** `SIGSTOP`进入ordinary pending或force-complete active wait、global init取得任何stop authority、未经最终DefaultStop selection的条件性signal提前停止group、`SIGCONT`已线性化后旧candidate重新停止group、late ordinary occurrence逃过opposite cleanup、control generation撤销已经claimed的reservation、delivery重放`SIGCONT` resume、reserved retirement丢失temporary-mask restore responsibility或顺带消费其它ordinary asynchronous signal，或为了避免竞态引入所有signal共用的persistent carrier / reservation。

**Cutover：** `UJ-CUTOVER`。

### JOBCTL-CONT-001 — Continue 的 report 只来自 committed Stopped

**规则：** `SIGCONT` generation 的 resume side effect按 live phase执行：

- `Running -> Running`：不建立 job-control report；
- `Stopping -> Running`：取消未完成 stop、唤醒 jobctl parker，不建立 Stopped / Continued report或对应 SIGCHLD notification；
- `Stopped -> Running`：使未消费 Stopped report失效，建立一次 Continued report并唤醒 parker。

重复 `SIGCONT` 在已经 Running 时不重复建立 Continued。default-stop 在 `Stopping / Stopped` 中合并，不建立新 episode或替换 first reason。

上述wake只属于jobctl resume side effect，只释放jobctl parker，不完成ordinary wait。`SIGCONT` ordinary occurrence若被接纳并可递送，仍可通过既有Signal notification独立影响可中断wait；两者不得合并成一个generic force wake。

resume side effect的唯一线性化点是concrete `SIGCONT` generation。该occurrence此后无论被reserved、ordinary fetch、同步消费、ignore、default no-frame consume或交给custom handler，都不携带resume capability，也不得再次改变phase或report。

**Owner：** `ThreadGroup` job-control transaction。

**依赖：** `JOBCTL-STATE-001`、`JOBCTL-SIGNAL-001`、`JOBCTL-REPORT-001`。

**违反表现：** incomplete stop产生 synthetic Stopped / Continued、masked SIGCONT 不恢复 group、delivery重放resume side effect，或 consumed Stopped report使真正的 Stopped->Running 无法产生 Continued。

**Cutover：** `UJ-CUTOVER`。

### JOBCTL-LIFE-001 — Membership 与 terminal 不得遗留 exposure

**规则：** lifecycle / topology path 承担以下局部义务：

- fork child 建立新的 Running ThreadGroup，不继承 parent pending、phase、exposure 或 report；
- clone thread 在 membership publication 时保持 unexposed，首次进入用户态前接受 live group gate；
- exec 保留当前 ThreadGroup jobctl phase、control-ordering state（首选基线为 `ContinueEpoch`）和report；exec task在image replacement期间保持unexposed，新映像首次进入用户态前接受gate；
- dethread、ordinary member exit 和 detach 在移除 membership 前清除对应 exposure，必要时完成 Stopping；
- terminal lifecycle 在发布 `Exiting` 的同一 owner transaction 中取消 Stopping / Stopped、清除 job-control report并安排唤醒 parker；exit code 和 terminal signal truth仍由 `TASK-LIFE-*` owner决定。

普通 asynchronous、可屏蔽 terminal signal 在成为 committed terminal lifecycle 前不支配 Stopped，也不能仅因唤醒 ordinary wait / jobctl park而提前执行default action；`SIGKILL`、kernel-generated synchronous signal 的 no-return terminal action和 committed group exit无条件支配。

**Owner：** ThreadGroup lifecycle / topology handoff protocol；jobctl 只拥有自己的 cleanup obligation。

**依赖：** `TASK-LIFE-001..003`、`JOBCTL-STATE-001`、`JOBCTL-STOP-001`。

**违反表现：** detached TID 永久保持 exposed、exec new image越过 stop、terminal exit被 jobctl report阻塞，或 jobctl 覆盖 first exit code。

**Cutover：** `UJ-CUTOVER`。

### JOBCTL-REPORT-001 — Report 是 child-attached coalesced truth

**规则：** child ThreadGroup 在与 phase 相同的线性化域持有 report；owner-local slot是 `None | Stopped | Continued`。Stopped reason只由同一 snapshot内的 live `Stopped(reason)` phase提供，wait ABI形成 `Stopped(reason) | Continued` typed status，不在slot复制reason或保存opaque episode identity：

- Stopped commit 把 slot 设为 Stopped，覆盖更早的 Continued；
- Stopped->Running 把 slot 设为 Continued，无论此前 Stopped 是否已经被 consuming wait取走；
- Stopping 期间可以保留并消费上一 episode 的 Continued，直到新 Stopped、consume 或 terminal覆盖；
- Stopped report 只有在 live phase仍为 Stopped 时可返回；
- terminal lifecycle清除 slot，exit status在 selection中最高优先。

`WNOWAIT` 只读当前 snapshot；consuming wait按 topology / parent relation -> child owner顺序重新检查 relation、selector、phase和当前slot，并原子取走当下仍eligible的report，多个 waiter最多一个成功。scan snapshot不授予claim authority：slot已消失或变为不符合options时必须重扫；replacement当前仍eligible时可以由本次claim返回并消费。parent Event只触发rescan。`SA_NOCLDSTOP` / ignored SIGCHLD只抑制signal notification，不删除report或predicate wake。

report commit先于 guards-out parent notification。一次 stopped / continued transition对当前 parent使用一个 topology snapshot发送可选 SIGCHLD和必需的 child-status Event；若 child 带着非空 report并发 reparent，adoption path必须在 relation publication后唤醒 new parent重扫。reparent不清除、不复制 report，也不为历史 transition重放 SIGCHLD。

procfs 为单次 read从同一 owner取得一个只读 derived snapshot：observable leader `Zombie` 仍投影 `Z`；否则只有 committed Stopped投影 `T`，Stopping和Running继续使用 `PROCFS-TASK-STATE-001` 的底层映射。`status` 的 character与 name必须由同一 snapshot序列化；本 RFC不改变 binding / leader resolution失败边界。

stopped / continued typed status只承诺真实child identity、status kind与stop / continue reason；本阶段`waitid`结果和对应job-control `SIGCHLD`都写`si_uid = 0`，保持现有exited-child bridge的stage-1 credential projection边界。report、wait和Signal不得为填充该字段缓存leader credential、从任意live member猜测UID或在`ThreadGroup`中建立credential副本。未来修正必须先由credential owner定义跨leader exit及wait/report生命周期稳定的child identity snapshot，再统一替换exit、stopped与continued projection。

**Owner：** report / phase truth 归 child `ThreadGroup`；parent relation与 reparent handoff归 task topology；selection / claim protocol归 task/wait；SIGCHLD occurrence归 Signal；procfs只读。

**依赖：** `CHILD-WAIT-001..005`、`PROCFS-TASK-STATE-001`、`TASK-LIFE-003`、`JOBCTL-STATE-001`。

**违反表现：** SIGCHLD 成为 wait truth、WNOWAIT 消费 report、旧 scan snapshot绕过current-state重验、并发 waiter double-consume、带 report 的 child reparent后 new parent永久睡眠、Running child返回 Stopped、为`si_uid`新增无owner的credential cache，或 procfs直接读取汇合内部字段。

**Cutover：** `UJ-CUTOVER`。

### USER-ENTRY-002 — 所有 architecture user transition 共享同一逻辑 gate

**规则：** ordinary syscall / exception / trap return、clone child first entry、fresh task entry 和 exec new-image first entry 在执行用户指令前必须遵守同一逻辑循环：

1. 完成 phase-aware Signal / terminal lifecycle arbitration；Running 可以处理普通 signal，Stopping / Stopped只处理 `SIGKILL`、已提交的 terminal lifecycle、kernel-generated synchronous signal 的 no-return terminal action、可合并当前 stop的default-stop control action和已经claimed的reserved `SIGCONT`；
2. 进入 ThreadGroup before-user-entry gate；Running 时登记 exposure并允许，Stopping / Stopped 时保持 unexposed并 park；
3. gate允许后才执行 architecture transition。

jobctl park必须是对 live phase的 predicate wait：wait publication前后重验 phase，使 `SIGCONT` / terminal transition不能在 gate-check 与 park之间丢 wake。park被 `SIGCONT`、`SIGKILL` 或 terminal lifecycle唤醒后必须回到第 1 步，不能从 park直接进入用户态。普通 asynchronous signal和非`SIGCONT` reserved delivery只保持pending，不得释放jobctl park；ordinary wait即使因普通signal返回，也必须在phase-aware arbitration中保持该signal pending并重新park。reserved target尚未retire时不能阻塞对`SIGKILL`等支配性action的发现；已经提交的handler frame只改变后续user context与mask responsibility，不授权跳过重新仲裁或live gate。各架构可以有不同物理入口，但policy只定义一次。

**Owner：** user-task transition protocol。

**依赖：** `USER-ENTRY-001`、`JOBCTL-STOP-001`、`JOBCTL-LIFE-001`。

**违反表现：** clone / exec / fresh path绕过 gate、park wake直接执行 architecture return、或 architecture-specific code复制不同 Signal/jobctl policy。

**Cutover：** `UJ-CUTOVER`。

## 状态转换与可见副作用

| 起始 phase | 输入 | 下一 phase | Exposure / report | Guards-out effect |
| --- | --- | --- | --- | --- |
| Running | admitted stop request | Stopping 或 Stopped | 记录 first reason；若 exposure 空则同事务直接提交 Stopped + Stopped report | 可选 stale-safe user-execution kick；若直接提交则通知 parent |
| Stopping | user trap entry / detach | Stopping 或 Stopped | 移除 member exposure；最后一个关闭时建立 Stopped report | 最后关闭时通知 parent |
| Stopping | admitted stop request | Stopping | merge；不替换 reason / report | None |
| Stopping / Stopped | ordinary wait predicate / timeout / source completion | phase不变 | 不修改exposure / report或原wait state | 正常收口syscall；最终user-entry gate park，不制造`EINTR` |
| Stopping | SIGCONT generation | Running | 不建立 Stopped / Continued | 只唤醒jobctl parker；ordinary occurrence另走Signal notification |
| Stopped | admitted stop request | Stopped | merge；不建立新 report | None |
| Stopped | SIGCONT generation | Running | Stopped report失效，建立 Continued | 唤醒jobctl parker并通知parent；ordinary occurrence另走Signal notification |
| Running | SIGCONT generation | Running | report不变，不新增 Continued | jobctl无wake；ordinary occurrence另走Signal admission / notification |
| 任意 jobctl phase | committed terminal | —（lifecycle -> Exiting） | 清 exposure / jobctl report；phase归一化或转为inert snapshot | 唤醒 parker，进入 terminal owner |

phase transition、exposure mutation和 report commit 在线性化域内完成；notification、Event publish、IPI / task poke、park、user copy和复杂 drop 必须在相关 guards释放后执行。

## 身份与能力模型

- `ContinueEpoch` 是首选的 opaque、不可复用 control-ordering identity；默认使用 owner lock保护的 checked monotonic `u64`，只比较 equality，不提供 total ordering。在任何 stale candidate仍可能存在时不得 wrap / alias；等价单事务串行化只有满足 `JOBCTL-SIGNAL-001` 的证明与回写条件时才能省略该字段。
- exposure member key 必须绑定 live ThreadGroup membership identity；不能仅依赖可能复用的裸 numeric TID。detach 在 identity失效前删除 exposure。
- parent report不携带claim identity；wait通过 topology / parent relation -> child owner transaction读取并消费current slot。ABI不暴露scheduler state、participant identity或internal control-ordering identity，只观察reason / continued kind和child identity。
- diagnostic timestamp、CPU、tid formatting、phase age 和 blocked-location snapshot不得驱动 completion或report。

## 锁序与跨 owner 局部义务

目标逻辑顺序为：

```text
task topology / exact live identity
  -> ThreadGroup lifecycle + jobctl transaction
       -> one Signal pending / disposition leaf at a time

all guards out
  -> task notification / Event / IPI / park / scheduler wake / user copy / complex drop
```

具体 lock type 与 API signature 属于实现设计，但必须满足：

- control-signal generation必须在同一ordering domain闭合各自原子单元：`SIGSTOP`的ordinary opposite cleanup + direct stop，条件性stop signal的ordinary cleanup + epoch capture + ordinary pending publication，以及`SIGCONT`的epoch推进 + ordinary cleanup + resume side effect + ordinary occurrence admission；已经claimed的reservation不加入该锁域；
- conditional default-stop path 不得持有 Signal leaf再反取 ThreadGroup owner；应在Signal最终action selection后携带窄authority进入ThreadGroup transaction并重验epoch；
- before-user-entry gate 不得在持有任意外层 lock、wait registration、linear token或未收口资源事务时 park；
- wait report claim 必须按 topology / parent relation -> child owner 顺序重验，不能从 child report反取 topology；
- scheduler只执行现有 park / wake / placement capability，不读取或推进 jobctl phase。

第一版不要求任何stop latency kick。未来若增加，它只能在guards-out后使用“促使仍在用户态执行的task进入trap”的窄能力：即使task snapshot已经stale、目标已经进入ordinary wait，也必须no-op而不是完成active wait、消费WakeToken或改变wait result。`notify(..., true)`及语义等价的generic force wake不满足该边界。

无法在该方向闭合 lock graph时，必须回到 RFC review；不得用 dirty bit、worker obligation、第二 queue或 scheduler hold绕开。

## RFC-local Proof Obligations

### INV-ENTRY-CLOSURE — user / kernel exposure closure

- RV64 与 LA64 的 user trap entry 必须在任何可能继续内核处理前清 exposure。
- ordinary trap return、fresh task、clone child和exec new image必须逐条证明进入统一 arbitration。
- Running gate登记 exposure与 stop request读取 exposure必须有单一先后关系：gate先赢则stop等待下一次 kernel entry；stop先赢则gate不得放行。
- jobctl park的 wait publication与 `SIGCONT` / terminal transition必须通过 live-phase recheck闭合；wake只触发重新仲裁，不能直接授权 user entry。
- ordinary wait在stop前后的predicate、timeout、source registration和result必须由原owner完整保持；即使parent已经观察Stopped，其completion也只负责正常收口原syscall，随后再由user-entry gate阻止用户执行。
- 任何无法封闭的 architecture path 是 hard stop，不得标成 best-effort。

### INV-CONTROL-TXN — control-signal transaction closure

- task-private、ThreadGroup-shared、standard signal和多个 member竞争路径都必须覆盖generation-time ordinary opposite cleanup；只有三个条件性stop signal需要epoch capture与ordinary pending publication。
- signal-0 probe、permission failure、TGID/TID relation failure和 target-not-found路径不得进入 control transaction。
- global init的合法stop-class generation必须完成ordinary opposite cleanup，但`SIGSTOP`不得取得unconditional authority，三个条件性stop signal的live default action不得取得conditional authority；caught / masked ordinary occurrence与`SIGCONT`仍服从本invariant定义的普通路径。
- global init之外的`SIGSTOP`必须在generation transaction内直接取得unconditional authority并调用共享stop engine；不得发布pending、建立reserved delivery、等待member fetch或触发generic active-wait completion。
- opposite-class cleanup不得撤销已经claimed的reserved delivery；reservation仍须通过live action selection与handler-frame / no-frame / no-return terminal路径恰好一次收口temporary-mask responsibility。
- stopped-phase arbitration必须允许reserved `SIGCONT`收口而不授予user entry，并且在retirement前后都能找到`SIGKILL`、eligible default-stop或明确的synchronous no-return terminal action；非`SIGCONT` reservation保持reserved，不得继续消费其它ordinary asynchronous signal。
- `SIGCONT`在occurrence ignored、masked、caught、同步消费或异步消费时都只由generation完成一次group side effect，后续action不得重放。
- `SIGTSTP` / `SIGTTIN` / `SIGTTOU`只有在最终action selection得到`DefaultStop`、目标不是global init后才能形成conditional authority；captured epoch已stale时不得重新发布、补偿或重排为另一个signal，只取消jobctl effect。
- shared stop engine只接受unconditional `SIGSTOP` authority或已验证的conditional `DefaultStop` authority；从该边界开始四种信号复用相同phase、exposure、report、continue、lifecycle和user-entry逻辑。
- transaction不得扩大成普通 signal的 persistent prepare / finish protocol。

### INV-REPORT-CLAIM — parent report closure

- report commit先于 parent predicate notification。
- report transition使用单个 current-parent snapshot通知；带非空 report的 reparent在 relation publication后唤醒 new parent重扫，但不重放历史 SIGCHLD。
- wait4 / waitid selector、WNOWAIT和consuming waiter在并发 report replacement、SIGCONT、exit、reparent下必须重新验证；consuming claim不能沿用scan snapshot授权，而应在child owner下读取并claim当前eligible slot。
- `wait4` stopped / continued status word和 `waitid` `CLD_STOPPED / CLD_CONTINUED` siginfo映射必须由同一个 typed snapshot序列化。
- stopped / continued `waitid`与对应job-control `SIGCHLD`必须稳定写`si_uid = 0`；实现不得引入新的credential truth来伪装完整projection。
- `SA_NOCLDSTOP` / ignored SIGCHLD必须与 report truth、Event wake解耦。

### INV-LIFECYCLE — membership / terminal closure

- clone publication、dethread victim removal、ordinary detach、last-member exit与exec image replacement不得遗留 exposure。
- terminal清理必须先使 parked task能够离开 jobctl wait，再由现有 lifecycle完成 no-return路径。
- jobctl cleanup不得覆盖、推迟或重新解释 `TASK-LIFE-*` first terminal code。

### INV-OBSERVABILITY — Stopping 必须可诊断

实现必须提供有范围的 debug / trace snapshot，至少能区分 Running、Stopping、Stopped、first stop reason、exposed-member count和 phase age。member identity如果输出只服务诊断，必须明确不参与行为；procfs ABI不暴露 internal epoch或 exposure membership，并从单次 derived snapshot生成一致的 state character / name。

### INV-VALIDATION — production path validation floor

文档层收口后，未来 implementation至少必须覆盖：

- 单线程 stop / continue / wait4 / waitid / WNOWAIT；
- 多线程 user-running + syscall + ordinary-wait混合，并证明parent可在kernel waiter尚未完成时观察Stopped；
- active ordinary wait在stop前后的predicate、timeout、source registration与真实结果保持不变，stop本身不产生`EINTR`；
- `SIGSTOP`不进入private / shared pending、不等待member fetch且不调用generic force notification；
- `kill` / `tkill` / `tgkill` / `rt_sigqueueinfo`等可达路径中的global init immunity：stop-class generation仍清理ordinary opposite pending，但init永不进入`Stopping / Stopped`；条件性signal的caught / masked ordinary语义与`SIGCONT`路径保持有效；
- deterministic `Stopping x SIGCONT` cancellation无 report；
- completed Stopped 后 SIGCONT恰好一个 Continued；
- task-directed与process-group-directed四种 stop signal；
- caught / ignored / masked条件性stop signal与 ignored / custom / masked SIGCONT；包括generation后改变disposition再解除mask时按live action执行，而不是按generation snapshot提前stop；
- ordinary opposite-class pending被generation清理，而pre-existing reserved occurrence保持claim finality；覆盖reserved `SIGCONT`在Stopped期间的handler-frame、ignore / default no-frame与temporary-mask restore，以及reserved conditional-stop在后续Running阶段的live action / stale default-stop；
- `SIGCONT` resume side effect不完成ordinary wait且在delivery时绝不重放；custom `SIGCONT` occurrence仍可按普通Signal notification影响其可中断wait；
- reserved旧`SIGCONT`、随后`SIGSTOP`和真正恢复用的新`SIGCONT`的确定性竞态；覆盖额外handler观察、普通mask串行化、`SA_NODEFER`嵌套、`SA_RESETHAND`以及frame failure / `SIGKILL` terminal dominance；
- clone / fork / exec / dethread / member exit / SIGKILL / exit_group；
- RV64 production runtime，以及 RV64 / LA64 user-entry source closure；
- procfs `stat` / `status` Stopped、Stopping与terminal precedence；
- waitid stopped / continued与对应job-control SIGCHLD的`si_uid = 0`，并通过源码审计证明没有新增ThreadGroup credential cache或任意member UID猜测。

KUnit只能补充局部状态机证明，不能替代真实 trap entry、signal generation、ordinary wait和wait syscall路径。

## 工程降级准则

Linux / POSIX已经明确、常用且能在现有owner边界内自然实现的语义是默认兼容义务。只有某项行为属于偏僻corner或极难稳定复现的并发race，并且精确复制会造成明显不成比例的实现代价、跨越既有owner，或迫使尚未稳定的基础路径先做高风险重构时，才允许受限降级。

降级必须先保护唯一owner、核心状态不变量和可诊断性：已有typed error、no-user-entry gate或既有no-return terminal边界能够安全拒绝时优先fail-close；否则把trigger、可见差异、影响范围、验证方式和退出条件写入RFC accepted deviation、transaction反馈或register，并停止为该corner继续扩张协议。fail-close不得把原本recoverable的ABI corner升级为kernel panic、hang、data corruption或新造的terminal outcome。无法证明降级安全时停止当前 Stage 并回到RFC review。不得静默fail-open、伪造成功、降低mandatory user-entry / terminal precedence、用case hack隐藏失败，或仅凭未复现的race修改稳定路径和启动跨owner重构。

## Accepted Limitations 与 scoped deviations

- `Stopping x SIGCONT` 取消不复制 Linux synthetic incomplete group-stop notification corner。
- generation cleanup只删除ordinary opposite-class pending，不撤销pre-existing reserved occurrence。旧reserved `SIGCONT`可以在Stopped期间完成live action selection与handler-frame / no-frame收口，但只能在后续真实`SIGCONT`恢复后进入handler；新occurrence可能形成每个pre-existing reservation至多一次额外handler观察，`SA_NODEFER`可能嵌套或反转观察顺序，`SA_RESETHAND`或live disposition变化可能消除新handler观察。
- Stopped期间提交reserved `SIGCONT` handler frame允许写用户栈、更新handler mask / oneshot disposition；frame failure继续走既有no-return terminal path。这些是cooperative user-entry barrier允许的内核侧副作用，不得被解释为handler已经进入用户态。
- stop completion 无固定 latency bound；长期停留 Stopping必须可诊断。
- Stopped只承诺user execution barrier，不承诺内核quiescence；parent观察report后，既有syscall / exception / ordinary wait仍可能继续并产生正常内核副作用。
- controlling TTY、foreground/background pgrp和terminal-generated signal延后。
- orphaned process-group suppression及 `SIGHUP` / `SIGCONT` policy延后，并继续作为 register limitation。
- ptrace stop / tracer wait status延后；未来不得把 ptrace state塞入本 report或复用procfs display truth。
- report是有界 coalesced state，不提供无界 transition history或跨 child全局顺序。
- stopped / continued `waitid`与对应job-control `SIGCHLD`的`si_uid`固定为stage-1 `0`；未来只有credential owner提供稳定child identity snapshot后才能统一修正，不为本RFC新增ThreadGroup credential truth。公开提升后必须让该缺口在`UJ-CUTOVER`时继续留在register，直到后续credential contract关闭。

## 禁止退化项

- 不得新增 scheduler-owned stop phase、runqueue hold或generic wait admission。
- 不得要求 ordinary wait取消、转移到jobctl wait或发布 participant ack；不得改变其predicate、timeout、source registration、result，或为stop制造`EINTR`。
- 不得用force notification、generic task poke或WakeToken完成active ordinary wait；可选latency kick只能促使仍在用户态执行的task进入trap，并在目标已离开用户态时安全no-op。
- 不得让 Signal pending、task-local flag、Event、SIGCHLD、procfs或scheduler state成为第二份jobctl truth。
- 不得让global init取得`SIGSTOP`或conditional `DefaultStop` authority，也不得为stopped / continued `si_uid`缓存leader credential或从任意member猜测UID。
- 不得为 control signal竞态引入覆盖所有 Signal 的 persistent carrier / reservation / finalizer framework。
- 不得用“偏僻Linux语义”或“难复现race”包装常用ABI缺口、静默fail-open或未记录的弱语义；受限降级必须满足工程降级准则并保留退出条件。
- 不得在 parent可观察 Stopped 后允许任何未登记的 user entry。
- 不得让 implementation probe削弱 incomplete cancellation、report precedence或mandatory entry order。

## 完成标准

RFC target review完成需要同时证明：

1. `JOBCTL-*` / `USER-ENTRY-002` 的 owner、状态转换、线性化和failure boundary自洽；
2. current contract baseline与Contract Impact没有把 target写成effective事实；
3. exposure model覆盖全部user/kernel transition和membership lifecycle；
4. control-signal transaction覆盖global-init immunity、`SIGSTOP` direct admission、条件性DefaultStop authority、ordinary pending cleanup、reserved claim finality / temporary-mask handoff和generation-only `SIGCONT` resume，且不依赖通用 Signal重构；
5. report source、wait claim、SIGCHLD和procfs没有并列真相，stopped / continued `si_uid = 0`不引入credential副本；
6. ordinary wait保持原predicate、timeout、registration和真实result，stop / resume side effect都不把它当作jobctl ack；
7. accepted limitation、工程降级边界与后续RFC范围已明确；
8. 未来 implementation为全部 `Introduce / Refine / Replace` ID建立同一个 `UJ-CUTOVER` 映射、验证floor与停止条件。

`UJ-CUTOVER` 前，新增 ID全部保持 Not effective，现有 current contract不变；本 R0 不创建 transitional contract。
