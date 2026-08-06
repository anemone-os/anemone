# POSIX Timer Thread-ID Notification 实施路线

**状态：** Closed
**最后更新：** 2026-08-06
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
- **Validation claim：** owner-local KUnit、RV64 release build/KUnit、RFC专属 raw syscall oracle与
  source/lock-order audit共同构成语义验收；raw oracle必须能排除shared fallback、确认target admission已经
  关闭，并比较dequeue-finalized `si_overrun`/`timer_getoverrun()`；LA64 runtime本轮按维护者授权为
  `Not Run / waived`，不伪写成通过。Vim只作为集成smoke，不能替代raw证据。
- **Stop conditions：** target/owner/handoff/errno/cleanup改变，出现维持`Task`可执行/成员状态的强lifetime、expiry TID lookup、
  private/shared双重pending、Signal私有表示外泄或锁内owner callback时，停止并回到RFC review/Target Renegotiation。

用户已在2026-08-05授权依次完成Gate 0--3。后续Gate虽然已获授权，但只能在前一Gate完成验证、review与
独立commit后进入；不得跳跃或并行推进。

## Gate 0 — ABI、source 与 oracle baseline

**状态：** Closed — 2026-08-05
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

### Gate 0 Closure — 2026-08-05

- `SigEvent`增加只读取native `_tid` union member的ABI accessor；layout checks固定size/alignment与
  0/8/12/16 offsets，raw union不进入timer core。
- raw user oracle使用当前线程真实TID构造notification type 4，明确断言Gate 0仍返回`EOPNOTSUPP`；
  Gate 2以exact-delivery/lifecycle cases替换该临时expectation。
- local Linux 6.6.32 source audit冻结strict create validation、exact private route、queued/dequeue/ignored/
  flush/target-exited rearm矩阵、target-exit三阶段、`timer_gettime()`投影与delete-after-queue语义。
- `just fmt all --check`通过；RV64/LA64 release app与kernel build通过。RV64 469/469、LA64 472/472 KUnit
  全通过，两个架构的raw oracle均打印`SIGEV_THREAD_ID baseline errno=EOPNOTSUPP`并完成既有POSIX timer
  checks。default QEMU console policy过滤Notice，notice路径由source/KUnit与`etc/log-board-rv-6.log`的实际
  type-4记录证明。
- 两个测试盘均在Gate 0 marker之后因缺少`/musl`/`/glibc` static BusyBox退出；该外部fixture不参与Gate 0
  acceptance，未把其后环境/LTP阶段写成已运行。全部mdBook检查均按用户指令跳过。
- production仍不接受`SIGEV_THREAD_ID`，Signal/timer core与current contract未变。

### Gate 0 Post-closure Oracle Correction — 2026-08-06

Gate 2 pre-cutover review确认，同时释放target与decoy进入`rt_sigtimedwait`不能证明exact private route：错误的
shared publication仍可能被target竞争取得而让decoy timeout。Gate 0没有ABI或contract cutover，因此保持
Closed；其positive oracle设计由Gate 2替换为“decoy独占expiry窗口、target延后fetch”。这项更正不降低
Gate 0已验证的layout/unsupported baseline，也不能被既有双wait结果当作Gate 2证据。

## Gate 1 — Task-private timer signal protocol

**状态：** Closed — 2026-08-06
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

### Gate 1 Closure — 2026-08-06

- Signal owner新增weak shared/private pending capability；private registration、occurrence、arrival identity与
  admission都由exact task的`sig_pending`唯一拥有，expiry不重新解析TID，也不为首个notification分配。
- private普通/同步fetch与temporary-mask reservation消费同一slot；job-control generation在同一ThreadGroup
  transaction内清理opposite-class shared/private occurrence，锁外以typed `Dequeued`/`Flushed`完成owner handoff。
- task exit在membership detach前关闭private admission并摘除pending/reserved occurrence；callback与registration
  destructor均在Signal/ThreadGroup guard外运行。未到期arm与dequeue后的后续arm仍留给Gate 2 timer consumer处理。
- owner-local KUnit补齐同registration ignored后恢复、private `SIGCONT`经stop-class cleanup后旧episode
  `Flushed`且新episode `Dequeued`、三阶段target exit、callback re-entry、reserved delivery、slot reuse与
  private realtime ordering。`just fmt kernel --check`通过；RV64 release build与477/477 KUnit通过，LA64
  release build与478/478 KUnit通过，两个新增case双架构均为`ok`。
- 两次QEMU均在完整KUnit与既有clock/timer/signal用户检查通过后，因本轮只挂载pretest rootfs、未提供第二块
  test disk而在`/dev/vdb` mount处退出；该fixture边界不属于Gate 1 acceptance。按用户指令未运行mdBook。
- 本Gate没有ABI或current contract cutover；`SIGEV_THREAD_ID`仍保持unsupported，internal capability必须由
  Gate 2真实POSIX timer consumer接入，否则按既定退出条件删除。

## Gate 2 — POSIX timer integration 与 PT-THREAD-ID-CUTOVER

**状态：** Closed — 2026-08-06
**Purpose：** 解码target TID，把Gate 1 capability接入`timer_create()`和timer lifecycle，并以双架构
raw/Vim证据原子切换两项current contract。
**Prerequisites：** Gate 0/1关闭；PT-TID-001--007的source/KUnit baseline已建立；raw oracle入口与Vim
smoke case可重复运行。positive exact-route、exit-stabilization与nonzero-overrun oracle必须按本Gate
pre-cutover review更正后才能成为cutover证据。
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
- live periodic occurrence在dequeue时必须先完成timer-owner handoff：把pending期间错过的周期合入同一个
  delivery提交，同时更新即将copyout/frame的`siginfo.si_overrun`和`timer_getoverrun()`最近交付snapshot，
  再执行physical rearm。没有replace/republication的单episode中两者必须是相同非零钳位值；只更新timer
  snapshot而保留enqueue-time siginfo不满足Linux可见语义。
- ignored expiry不产生occurrence、不提交最近交付snapshot，但必须保留timer-local overrun accrual并立即
  periodic rearm；恢复disposition后的首次真实dequeue才把累计值暴露到`si_overrun`和最近交付snapshot。
- target退出时，未到期arm继续到failed exact expiry；pending occurrence被flush且不产生新物理arm；已经
  dequeue/rearm的arm继续到failed exact expiry。periodic timer在flush/failed send后保留Linux
  `timer_gettime()`未来投影但没有物理request；stale callback不影响新generation/reused ID。
- timer delete不recall已由Signal拥有的private occurrence；普通delivery或同步wait仍可取得旧`SI_TIMER`，
  completion不得rearm已删除timer。
- 保留`SIGEV_THREAD`的`EOPNOTSUPP` notice并注明这是相对Linux raw syscall的有意差异；更新
  `SIGEV_THREAD_ID`诊断，使unsupported、invalid与runtime target-exit可区分。
- 在同一cutover中Refine `POSIX-TIMER-001`与`SIGNAL-PENDING-001`，记录旧/新规则、source/KUnit与
  RV64 RFC专属 raw oracle；通用 LTP 及其清理路径不属于本 RFC 验收。

**Validation：** RV64 release build/KUnit与source audit证明physical request/outcome转换；raw oracle覆盖
target/decoy均blocked、decoy独占expiry窗口且target延后fetch的exact-route、`SI_TIMER`字段、同号多timer、
blocked/unblocked、ignored/control signal、unknown/mixed notify、invalid/foreign TID、concurrent-exit
closure、以新registration稳定`EINVAL`确认admission关闭、dequeue-vs-exit-flush的`timer_getoverrun()`、
无replace/republication的延迟多周期dequeue中`si_overrun`与最近交付snapshot取得相同非零值、
target-exit三阶段的可见投影、无retarget与无后续
delivery、ignored后恢复disposition、TID reuse、delete后queued delivery、同号ordinary task-directed realtime
arrival ordering、ignored期间多周期accrual延迟到恢复后的首次delivery、periodic overrun与
delete/replace/in-flight竞态；RV64 Vim smoke不再出现`E1286`。通用 signal-wait LTP 不作为本 RFC 验收。
LA64 runtime按维护者授权记为`Not Run / waived`。按用户指令不运行mdBook。
**Cutover：** `PT-THREAD-ID-CUTOVER` 原子 Refine `POSIX-TIMER-001` 与 `SIGNAL-PENDING-001`。任何一项
证据缺失时两项都保持旧current contract，不能部分cut over。
**Stop / Exit：** code、两项contract和RV64 RFC专属证据已在同一可审查状态下切换；LA64 waiver与通用LTP
不作为本 RFC closure blocker。Gate 2 已关闭。

### Gate 2 Pre-cutover Source Review — 2026-08-06

本地固定 Linux 6.6.32 source确认target/owner方向成立，但当前Gate 2 diff和证据仍有以下blocker，不能执行
`PT-THREAD-ID-CUTOVER`：

- `dequeue_signal()` 在Signal锁外调用`posixtimer_rearm()`，后者同时更新timer最近交付snapshot和待交付
  `info->si_overrun`。当前Anemone completion只携带immutable identity/reason；timer owner能计算新的
  `last_overrun`，却没有路径修正已经取出的siginfo。必须闭合双向窄handoff，并增加非零delayed-dequeue oracle。
- Linux ignored expiry的`hrtimer_forward()`继续累加`it_overrun`，直到后续真实dequeue才提交snapshot。当前
  Anemone thread-specific ignored completion清除pending episode并立即rearm，但没有保留该accrual；必须补齐
  timer-owner累计，并用“delivery前snapshot不变、恢复后首次delivery为非零”的oracle证明。
- 当前raw exact-route case让target与decoy同时等待，shared fallback仍可能由target竞争取得；两者必须保持
  signal blocked，target延后wait，让decoy在expiry窗口成为唯一shared consumer。当前exit case的`done`发生在
  `SYS_exit`前，固定20ms不能证明registration admission已经关闭；必须改用type-4 create稳定`EINVAL`的
  lifecycle probe。
以上为 pre-cutover review 历史记录；前三项已由当前代码和 RFC 专属 RV64 oracle 修复，通用
LTP 与外围清理路径不属于本 RFC，未写入本 RFC closure。

### Gate 2 Closure — 2026-08-06

- 板上运行通过，480/480 KUnit 通过；RFC 专属 native oracle 通过 exact
  target/decoy、strict notify/TID validation、dequeue-finalized nonzero overrun、ignored recovery、target-exit
  三阶段、periodic projection 与 delete-after-queue。
- source/lock-order audit确认private occurrence唯一由exact task pending owner持有，timer只通过typed handoff
  接收completion；没有TID expiry lookup、shared fallback、强Task生命周期或锁内owner callback。
- `PT-THREAD-ID-CUTOVER` 已原子 Refine `POSIX-TIMER-001` 与 `SIGNAL-PENDING-001`。LA64 runtime由维护者明确
  waiver，记为`Not Run / waived`；通用 LTP 及 zombie 清理路径不属于本 RFC。

## Gate 3 — Final audit 与 RFC closure

**状态：** Closed — 2026-08-06
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
  delivery、ordinary/同步fetch、typed dequeue/flush、dequeue-finalized siginfo/snapshot、ignored accrual、
  pending/reserved retirement与generation callback顺序，确认cleanup不依赖`Task::Drop`偶然发生。
- 对照 `etc/linux-6.6.32` 固定源码完成 THREAD_ID 语义、owner、锁序和生命周期静态审计；将实际验证、
  Not Run边界、仍开放问题和有证据的 Architecture Friction写回唯一closure证据。
- 更新RFC为Closed；若发现当前target内缺陷则保持打开并修复，不登记为accepted limitation。

**Validation：** bounded static engineering audit、Linux 6.6.32 source review、Anemone source/lock-order
review、Gate 2 RV64 板上证据复核与 `git diff --check`；本 Gate 不新增 runtime 测试，按用户指令不运行mdBook。
**Cutover：** None；本Gate只核验Gate 2已经生效的contract，不重复cutover。
**Stop / Exit：** Gate 3 static audit、closure write-back 和 architecture-friction scan 已完成；RFC 可关闭并停止。

### Gate 3 Closure — 2026-08-06

- Linux 6.6.32 `kernel/time/posix-timers.c` 静态核对：`good_sigevent()`（377--399）对
  `SIGEV_THREAD_ID` 走精确 notify 分支并要求同组 target；`posix_timer_event()`（280--300）以
  `PIDTYPE_PID` exact 投递且 failed send 不 rearm；`posix_timer_fn()`（310--374）保留 ignored 周期的
  overrun accrual 并立即 rearm；`common_timer_get()`（637--690）保留 requeue-pending 的未来投影。
- Linux `kernel/signal.c` 静态核对：`dequeue_signal()`（635--710）在 siglock 外调用 `posixtimer_rearm()`；
  `send_sigqueue()`（1978--2039）将 `PIDTYPE_PID` occurrence 放入目标 task private pending；
  `kernel/exit.c::__exit_signal()`（143--215）在 siglock 下 flush task-private queue 后再释放锁。
- Anemone 对应 owner 路径：`time/posix_timer/api.rs` 只在 syscall ABI 边界解码 `_tid`；
  `task/posix_timer.rs` 只保存 weak exact identity、timer-owned overrun/arm state 和 typed completion；
  `task/sig/{pending,timer,delivery}.rs` 由 private pending owner 独占 occurrence，并在 Signal/ThreadGroup
  guard 外完成 timer callback；`task/api/exit/mod.rs` 在 topology detach 前关闭 admission 并 flush。
- Architecture Friction Scan 未发现第二份 pending truth、owner 穿透、expiry TID lookup、强 `Task` 生命周期、
  shared fallback 或锁内 callback；syscall 中的 `Arc<Task>` 仅是 registration 建立期间的 transient snapshot，
  已发布 registration 只持 non-rebinding weak capability。未运行 LA64 runtime，不把它写成通过；Gate 2 的维护者
  waiver 继续有效。
