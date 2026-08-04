# POSIX Timer Thread-ID Notification 实施路线

**状态：** Accepted
**最后更新：** 2026-08-05
**父 RFC：** [RFC-20260804-posix-timer-thread-id-notification](./index.md)
**当前修订：** R0

## 全局 Implementation Boundary

- **Target / non-goals：** 实现 native `SIGEV_THREAD_ID` 的同 `ThreadGroup` exact-task signal
  delivery；raw `SIGEV_THREAD`、CPU-time timer、compat ABI 与 kernel-side callback execution 不在目标内。
- **Owner / handoff / failure / cleanup：** timer state 继续由创建者 `ThreadGroup` POSIX timer owner 持有；
  exact task private pending owner持 registration/occurrence；topology解析同组target；task exit在detach前关闭
  admission并锁外retire private pending。未到期与已经dequeue/rearm的物理arm在后续expiry得到typed
  target-exited outcome后停止，pending/flush状态不产生新物理arm；timer delete以immutable identity完成。
- **Protected ABI / contract / acceptance：** 保持 native `SigEvent` layout、`SIGEV_SIGNAL` shared route、
  ordinary signal/job-control/temporary-mask规则与现行timer deletion ordering。在
  `PT-THREAD-ID-CUTOVER` 前 current contract 与 unsupported ABI 均不变。
- **Validation claim：** owner-local KUnit、RV64/LA64 release build/KUnit、raw syscall oracle与
  source/lock-order audit共同构成语义验收；Vim只作为集成smoke，不能替代raw证据。
- **Stop conditions：** target/owner/handoff/errno/cleanup改变，出现维持`Task`可执行/成员状态的强lifetime、expiry TID lookup、
  private/shared双重pending、Signal私有表示外泄、锁内owner callback，或需要降低双架构runtime强度时，
  停止并回到RFC review/Target Renegotiation。

RFC接受或完成前一Gate都不会自动授权下一Gate。用户已在2026-08-05另行授权Gate 0；Gate 1--3仍为
**Not Authorized / Not Started**，Gate 0关闭后必须停在Gate 1前等待授权。

## Gate 0 — ABI、source 与 oracle baseline

**状态：** Authorized / Not Started
**Purpose：** 在不改变 production `timer_create()` 可见行为的前提下，冻结 native `_tid` ABI、Linux
same-thread-group/exact-delivery/lifecycle语义与可重复的raw测试入口。
**Prerequisites：** RFC target 已接受；旧 Clock/POSIX Timer RFC 保持 Closed；读取 live current contracts、
register 与固定 Linux 6.6.32 source。
**Protected Boundary：** `SIGEV_THREAD_ID` 仍返回 `EOPNOTSUPP`并打印notice；不增加Signal/timer core
capability，不修改current contract，不把host C ABI布局当native Linux ABI。
**Deliverable：**

- 为 native `SigEvent` 提供只表达 `_tid` 的 ABI accessor，固定 64-byte size、8-byte alignment及
  0/8/12/16 offsets；raw union不越过syscall boundary。
- source audit 固化 Linux `good_sigevent()` 的strict notify/TID/同组校验、`posix_timer_event()` 的exact
  PID route与failed-send no-rearm、`send_sigqueue()`/`dequeue_signal()` 的task-private pending和锁外rearm、
  `posix_timer_fn()` 的ignored immediate rearm、`__exit_signal()` 的private flush、`common_timer_get()` 的
  periodic投影，以及delete后preallocated queued signal的存续语义。
- 准备raw syscall oracle：显式构造 `_tid`，让target与decoy thread同时等待同一signal，通过
  `rt_sigtimedwait`返回的`SI_TIMER`字段和errno证明exact delivery；冻结invalid/foreign target、strict notify、
  target-exit三阶段、`timer_gettime()`投影和delete-after-queue的Linux可见预期。Gate 0 baseline只验证
  notification type 4仍以notice和`EOPNOTSUPP`拒绝，oracle的success分支留待Gate 2 cutover。

**Validation：** ABI layout/accessor KUnit；oracle source/build review；RV64/LA64 baseline各复现一次明确的
unsupported notice和raw errno。按用户指令不运行mdBook。
**Cutover：** None。
**Stop / Exit：** ABI/source预期与oracle代码能区分“syscall被接受”“exact task通过同步wait收到signal”
“target-exit阶段与timer表示符合Linux”，且两架构baseline均明确得到unsupported notice/errno后关闭；
positive target/lifecycle语义只标记Ready for Gate 2，不在Gate 0宣称已运行。
若两架构布局不同、Linux源码不能支持冻结语义或测试只能依赖未固定host状态，停止并回到RFC review。

## Gate 1 — Task-private timer signal protocol

**状态：** Not Authorized / Not Started
**Purpose：** 在Signal owner内建立per-registration task-private `SI_TIMER` slot、exact-target enqueue/wake
与task-exit cleanup，但不让`timer_create()`提前接受新ABI。
**Prerequisites：** Gate 0关闭；PT-TID-002/003/004/006/007可从live task/signal锁序闭合；现有shared timer
slot、`rt_sigtimedwait` synchronous fetch与temporary-mask reserved-delivery路径已审计。
**Protected Boundary：** 普通private/shared signal和`SIGEV_SIGNAL` timer route不变；Signal独占pending/
mask/disposition/job-control/wake/frame；timer不读取Signal私有容器；本Gate不发布新的public API、timer ID
行为或current contract。

**Deliverable：**

- Signal内部提供target-task private timer registration capability与独立slot，预分配失败可在future
  `timer_create()`路径返回，expiry path不为首个notification临时分配。
- exact target enqueue应用live ignored disposition与现行job-control generation，返回typed queued/
  already-pending/ignored/consumed/target-exited outcome，只唤醒选定task。queued/already-pending等待dequeue，
  ignored不提交最近交付overrun但立即继续periodic arm，consumed只服务现行`SIGSTOP` scoped exception并提交
  control consumption/overrun后继续periodic arm，flushed/target-exited不rearm。
- task exit在membership detach前关闭private registration admission，摘除registration/pending/reserved
  delivery，并在所有Signal/membership guard外完成pending episode callback与destructor；未到期arm不提前
  disarm，pending flush不rearm，已经dequeue/rearm的arm继续到failed exact expiry。
- ordinary fetch/frame、`rt_sigtimedwait`同步fetch与temporary-mask handoff都消费同一个private timer
  occurrence并完成同一slot handoff；shared timer slot与ordinary signal路径保持回归。
- Signal锁外completion显式区分`Dequeued`与`Flushed`；只有ordinary/synchronous dequeue提交最近交付
  overrun，target-exit/exec/disposition flush只retire episode，不能触发delivery commit/rearm。
- private realtime timer slot与同号ordinary task-directed realtime queue保持Linux arrival ordering。

**Validation：** owner-local KUnit覆盖同号多timer、同registration重复expiry、masked/ignored及恢复disposition、control
signal、ordinary/同步fetch、dequeue-vs-flush overrun、flush/reserved delivery、same-number realtime
arrival ordering、create-vs-exit admission、exit-vs-callback、target-exit三阶段、callback re-entry与slot reuse；
source/lock-order audit证明无Signal -> timer -> Signal回环。按用户指令不运行mdBook。
**Cutover：** None。`SIGNAL-PENDING-001` 在真实POSIX timer consumer接入前保持current。
**Stop / Exit：** internal capability能fail closed且不改变现有route后关闭。它必须由Gate 2消费；若Gate 2
不cut over，删除capability与任何临时probe/test facade。若需要维持`Task`生命周期的强引用、shared fallback、pending容器外泄
或改变job-control/temporary-mask contract，立即停止并review。

## Gate 2 — POSIX timer integration 与 PT-THREAD-ID-CUTOVER

**状态：** Not Authorized / Not Started
**Purpose：** 解码target TID，把Gate 1 capability接入`timer_create()`和timer lifecycle，并以双架构
raw/Vim证据原子切换两项current contract。
**Prerequisites：** Gate 0/1关闭；PT-TID-001--007均有source/KUnit证据；raw oracle已能在两架构
识别exact target、errno和lifecycle状态，Vim smoke case可重复运行。
**Protected Boundary：** timer仍为creator `ThreadGroup`所有；target必须是caller同组可注册member；无
retarget fallback或`Task`强lifetime；`SIGEV_SIGNAL`、raw `SIGEV_THREAD`、delete/exec/last-member ordering与现行
timekeeper/soft-timer contract不变。

**Deliverable：**

- `timer_create()`在ABI boundary strict decode `_tid`；值4以外的unknown/mixed notify、稳定不存在/
  异组/closed target返回`EINVAL`且不发布timer ID。concurrent exit允许commit或`EINVAL`，但成功只能绑定
  原identity，不能产生半发布timer或TID reuse retarget。
- POSIX timer notification表示消费task-private registration capability；expiry、periodic overrun、
  typed dequeue/flush、replace/disarm/delete、exec/last-member exit与target-exit completion都按
  generation/episode identity闭合。
- target退出时，未到期arm继续到failed exact expiry；pending occurrence被flush且不产生新物理arm；已经
  dequeue/rearm的arm继续到failed exact expiry。periodic timer在flush/failed send后保留Linux
  `timer_gettime()`未来投影但没有物理request；stale callback不影响新generation/reused ID。
- timer delete不recall已由Signal拥有的private occurrence；普通delivery或同步wait仍可取得旧`SI_TIMER`，
  completion不得rearm已删除timer。
- 保留`SIGEV_THREAD`的`EOPNOTSUPP` notice并注明这是相对Linux raw syscall的有意差异；更新
  `SIGEV_THREAD_ID`诊断，使unsupported、invalid与runtime target-exit可区分。
- 在同一cutover中Refine `POSIX-TIMER-001`与`SIGNAL-PENDING-001`，记录旧/新规则、source/KUnit、
  RV64/LA64 raw oracle和Vim smoke证据。
- 执行 `ANE-20260606-RT-SIGTIMEDWAIT-ASYNC-WAITED-SIGNAL-EINTR` 要求的precheck-after-arrival定向
  用例以及LTP `rt_sigtimedwait01`/`sigtimedwait01`；只有满足该项Exit Condition后才从register移除，
  否则Gate 2保持Not Cut Over。

**Validation：** 双架构release build/KUnit与source audit证明physical request/outcome转换；raw oracle覆盖
target/decoy同步wait、`SI_TIMER`字段、同号多timer、
blocked/unblocked、ignored/control signal、unknown/mixed notify、invalid/foreign TID、concurrent-exit
closure、dequeue-vs-exit-flush的`timer_getoverrun()`、target-exit三阶段的可见投影、无retarget与无后续
delivery、ignored后恢复disposition、TID reuse、delete后queued delivery、同号ordinary task-directed realtime
arrival ordering、periodic overrun与delete/replace/in-flight竞态；Vim smoke在两架构均不再出现`E1286`。
普通signal、`SIGEV_SIGNAL`、exec/exit与
timer regression保持通过。按用户指令不运行mdBook。
**Cutover：** `PT-THREAD-ID-CUTOVER` 原子 Refine `POSIX-TIMER-001` 与 `SIGNAL-PENDING-001`。任何一项
证据缺失时两项都保持旧current contract，不能部分cut over。
**Stop / Exit：** code、两项contract和证据在同一可审查状态下切换后关闭。若只能接受ABI但无法证明exact
delivery/cleanup，或Vim仅因隐藏错误继续运行，保持Not Cut Over并回到review。

## Gate 3 — Final audit 与 RFC closure

**状态：** Not Authorized / Not Started
**Purpose：** 对cutover后的live实现做最终架构摩擦、lifecycle与双架构回归审计，并在无blocking finding时
关闭RFC。
**Prerequisites：** Gate 2关闭且`PT-THREAD-ID-CUTOVER`真实生效。
**Protected Boundary：** 不引入新target/contract ID，不扩大到raw `SIGEV_THREAD`、CPU-time timer或相邻
Signal重构；发现target/owner/ABI/acceptance变化必须停止，不能借closure追认。

**Deliverable：**

- 审计timer只持non-rebinding exact-target identity capability，数值TID仅在ABI/诊断边界，该capability
  不维持`Task`可执行/成员生命周期；不存在第二份pending truth、shared fallback、Signal私有表示泄漏或
  锁内owner callback。
- 审计create/exit admission、membership detach、target-exit三阶段、periodic `timer_gettime()`投影、timer delete后queued
  delivery、ordinary/同步fetch、typed dequeue/flush、pending/reserved retirement与generation callback
  顺序，确认cleanup不依赖`Task::Drop`偶然发生。
- 复跑双架构raw/Vim与普通signal/POSIX timer回归；将实际验证、Not Run边界、仍开放问题和有证据的
  Architecture Friction写回唯一closure证据。
- 更新RFC为Closed；若发现当前target内缺陷则保持打开并修复，不登记为accepted limitation。

**Validation：** bounded periodic engineering audit、source/lock-order review、RV64/LA64最终runtime matrix、
`git diff --check`；按用户指令不运行mdBook。
**Cutover：** None；本Gate只核验Gate 2已经生效的contract，不重复cutover。
**Stop / Exit：** Apollyon/Keter立即停止closure并报告current diff、模型偏差与所需owner/RFC决定；Euclid可在
不改变边界时修复或带证据报告。所有acceptance满足且无blocking finding后关闭RFC并停止。
