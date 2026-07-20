# RFC-20260720-unix-jobctl

**状态：** Accepted for Implementation
**修订：** R0
**负责人：** doruche, Codex
**最后更新：** 2026-07-21
**领域：** signal / task / process group / user entry / wait ABI / procfs
**事务日志：** [2026-07-20-unix-jobctl](../../devlog/transactions/2026-07-20-unix-jobctl.md)
**影响契约：** `SIGNAL-PENDING-*`、`SIGNAL-ACTION-*`、`SIGNAL-TEMP-MASK-*`、`PROCFS-TASK-STATE-*`、`PGRP-SIGNAL-*`、`TASK-LIFE-*`、`CHILD-WAIT-*`、`USER-ENTRY-*`；完整 delta 见[目标与不变量](./invariants.md#contract-impact)。
**开放问题：** 最后一轮 target review 发现的 global-init stop admission 与 stopped / continued `si_uid` 边界已经折入本文；当前没有已确认的 target blocker。implementation stage、write set、验证 floor 与 `UJ-CUTOVER` 已写入[迁移实施计划](./implementation.md)。
**下一步：** R0 的 Stage 0 与 Stage 1 checkpoint 已关闭并记录在事务日志；Stage 2 未开始且未获授权。`UJ-CUTOVER` 前不更新 effective contract。

## 摘要

本 RFC 定义 Anemone 协作式 Unix job control 基础能力：四种 default-stop signal 使通过action admission的user ThreadGroup停止再次进入用户态，global init保持不可stop，`SIGCONT`无条件恢复group，并通过`wait4`、`waitid`、`SIGCHLD`和procfs暴露stopped / continued状态。

这里的 stop 是 user-mode execution barrier，不是 scheduler hold 或任意内核栈 freeze。task 可以收口已经进入的 syscall、exception 和 ordinary wait；只有在再次执行用户指令前，才必须经过统一 Signal / lifecycle / jobctl arbitration。ThreadGroup 是 phase、completion progress、stop reason和 parent report 的唯一 owner；procfs 只读取由这些 owner state 派生的窄 snapshot。

## 背景

现有内核已经具有：

- task-private 与 ThreadGroup-shared signal pending；
- ProcessGroup membership 及 `kill(0, sig)` / `kill(-pgid, sig)` 广播；
- `ThreadGroupLifeCycle::Alive / Exiting / Exited`；
- exit-only `wait4` / `waitid`，以及 `WNOWAIT` peek；
- RV64 / LA64 ordinary trap-return 的 Signal arbitration。

当前缺少实际 stop / continue default action、stop/continue pending cleanup、完整 user-entry closure、stopped / continued child report、`CLD_STOPPED` / `CLD_CONTINUED` 和 procfs stopped projection。现有 [positioning 共识](./backgrounds/positioning.md)已经拒绝通过 scheduler execution barrier、generic wait cancellation、typed unwind 或通用 signal final-consumption framework 来填补这些缺口。

本 R0 target 相对已经提取的 current contract 描述 target delta；current behavior 继续以仓库 `docs/src/contracts/` 为准。本目录不是 effective authority。

## 目标

- 为 `SIGSTOP`、`SIGTSTP`、`SIGTTIN`、`SIGTTOU` 建立统一 ThreadGroup stop engine：普通 user ThreadGroup 的 `SIGSTOP` 在 generation transaction 中直接提交，另外三种信号只在 Signal 最终选择 `DefaultStop` 后提交；global init 保持 Linux 式不可 stop 边界。
- 让 task-directed 与 ThreadGroup-directed `SIGCONT` 在 generation time 无条件恢复整个 ThreadGroup，同时保留普通 pending / handler 语义。
- 以 `Running / Stopping / Stopped` 闭合 stop / continue phase，不引入 scheduler-owned stop state。
- 在 parent 可观察 `Stopped` 前，证明没有 member 可以在不再次经过 jobctl gate 的情况下执行用户指令。
- 保持 ordinary wait 的 predicate、timeout、source registration与真实result原样；stop不完成 active wait或制造`EINTR`，waiter按原条件醒来后在 user entry 前接受jobctl gate。
- 补齐 stopped / continued report、`wait4`、`waitid`、`WNOWAIT`、`SIGCHLD` 和 procfs projection。
- 让 clone / fork / exec / dethread、member exit、`SIGKILL` 与 group exit 不产生 user-entry bypass 或悬挂 completion responsibility。
- 尽早形成贯通真实 signal、trap-return、ThreadGroup、wait syscall 和 procfs 的 production vertical slice。

## 非目标

- 不在任意内核调用栈即时冻结 task。
- 不增加 scheduler `Requested / Held`、runqueue suppression 或通用 wait admission。
- 不取消、替换或改造 ordinary wait publication。
- 不用 `SIGSTOP` force notification完成 active wait，也不把jobctl stop映射为人造 `EINTR`。
- 不为 `Mutex`、`Event`、`Latch` 或其它同步原语引入 typed unwind。
- 不建立通用 Signal final-consumption / carrier / reservation framework。
- 不实现 controlling TTY、foreground/background process group 或 terminal access check。
- 不实现 orphaned-process-group stop suppression 及 `SIGHUP` / `SIGCONT` policy。
- 不实现 ptrace stop、tracer wait status 或 `PTRACE_CONT / PTRACE_LISTEN`。
- 不承诺 stop completion 的硬实时或固定时间上界。

## 文档地图

公开 RFC target：

- [目标与不变量](./invariants.md)：Contract Impact、target contract、状态机、owner、线性化与 proof obligations。
- [迁移实施计划](./implementation.md)：module split、Stage 顺序、write set、验证 floor、停止条件与 `UJ-CUTOVER`。
- [RFC 前定位共识](./backgrounds/positioning.md)：路线来源和已拒绝方向；已归档为 background，不覆盖本 RFC target。

Current contracts：

- [Signal pending routing](../../contracts/signal/pending-routing.md)
- [Signal temporary-mask delivery handoff](../../contracts/signal/temporary-mask-delivery.md)
- [Procfs TGID task-state projection](../../contracts/procfs/task-state-projection.md)
- [Process-group signal targeting](../../contracts/task/process-group-signaling.md)
- [ThreadGroup lifecycle](../../contracts/task/thread-group-lifecycle.md)
- [Child wait](../../contracts/task/child-wait.md)
- [Ordinary user entry](../../contracts/task/user-entry.md)

本目录是公开 RFC 的 canonical target source。R0 acceptance 只接受 target、实施边界和验证计划；它不更新 effective contract。Stage 0 与 Stage 1 的执行事实、review、验证和 checkpoint 由事务日志记录。

## 方案

### 数据模型边界：target 与实现基线

本 RFC 固定的是 owner、行为状态、线性化关系和外部语义，不要求未来实现逐字复制某组 Rust 字段。下面的数据结构是当前首选的最小实现基线，用于证明设计可以自然落入现有 `ThreadGroup` / Signal / wait owner；它不是 ABI，也不冻结文件布局、私有 helper、字段命名、enum 嵌套、具体容器或锁类型：

```rust
struct ThreadGroup {
    child_status_changed: Event,
    jobctl_unblocked: Event,
    inner: NoIrqRwLock<ThreadGroupInner>,
}

struct ThreadGroupInner {
    status: ThreadGroupStatus,
    members: BTreeMap<Tid, UserExposure>,
    // parent / children / pgid / sid / cpu_usage / shared pending ...
    job_control: Option<UserJobControl>,
}

enum UserExposure {
    Unexposed,
    Exposed,
}

struct UserJobControl {
    phase: JobControlPhase,
    continue_epoch: ContinueEpoch,
    report: Option<JobControlReport>,
}

enum JobControlPhase {
    Running,
    Stopping(StopEpisode),
    Stopped(StopEpisode),
}

struct StopEpisode {
    reason: StopSignal,
    // 只服务诊断，不参与 completion、report 或 signal ordering。
    started_at: Instant,
}

enum JobControlReport {
    Stopped,
    Continued,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
struct ContinueEpoch(u64);

struct Signal {
    // existing signal number / siginfo fields ...
    // 仅供 SIGTSTP / SIGTTIN / SIGTTOU 的条件性 DefaultStop handoff。
    // SIGSTOP 不进入 ordinary pending，也不携带 epoch。
    default_stop_epoch: Option<ContinueEpoch>,
}

enum StopAuthority {
    Sigstop,
    SelectedDefault {
        expected_continue_epoch: ContinueEpoch,
    },
}

struct ChildWaitOutcome {
    tgid: Tid,
    status: ChildWaitStatus,
    cpu_usage: ThreadGroupCpuUsage,
}

enum ChildWaitStatus {
    Exited(ExitCode),
    Stopped(StopSignal),
    Continued,
}
```

首选基线把 exposure 作为 live membership 的值，而不是另建 membership set、participant ledger或派生 count；`JobControlReport::Stopped` 不复制 stop reason，wait 在同一 owner snapshot中从 `Stopped(StopEpisode)` 取得 reason。user / kthread 的 `Option<UserJobControl>` presence必须由构造与常开 shape assertion约束，不能成为 `ThreadGroupType` 的可分叉第二真相。

`ContinueEpoch` 的首选物理表示是由 `ThreadGroup` control transaction保护的 checked monotonic `u64`：每个已经通过 target / permission validation的 concrete `SIGCONT` generation都推进一次；`SIGTSTP` / `SIGTTIN` / `SIGTTOU` occurrence在同一 generation transaction捕获当前值，Signal最终选择 `DefaultStop` 后只携带窄 authority进入jobctl并做 equality validation。global init之外的`SIGSTOP` generation与stop request位于同一个control transaction；所有`SIGSTOP`都不进入ordinary pending，也不需要epoch。该epoch不是stop round、report或时间顺序 identity，不使用 `Ord`，不得 wrapping / alias，也不应改成每次 `SIGCONT` 分配 `Arc<()>` 的 allocation-backed capability。Signal 中的首选携带形状只是一个窄的 `default_stop_epoch: Option<ContinueEpoch>`，不能扩张成通用 carrier。

`child_status_changed` 属于当前 ThreadGroup 作为 parent 时的 child-status predicate notification；`jobctl_unblocked` 只供本 ThreadGroup 内已经停驻在 mandatory gate 的成员重新仲裁。二者都位于 owner state之外，只能 guards-out publish并触发 predicate rescan，不携带 phase、report、selected child或 user-entry permit；具体字段名和所用 wake facade可以调整。

wait core首选返回一个统一的 `ChildWaitOutcome`，让 `wait4` / `waitid` 从同一个 typed status序列化 exited / stopped / continued；CPU usage等共同 snapshot放在status enum之外，避免 exit-only形状反向限制 stopped / continued ABI。具体 snapshot字段仍应由当时已经承诺的 wait / rusage scope决定，不要求实现预先填充尚未纳入 target的统计。

本 RFC 不为 stopped / continued report新增 credential truth。现有 exited-child `waitid` bridge 对 `si_uid` 使用 stage-1 `0`；本次新增的 `waitid` stopped / continued结果与对应job-control `SIGCHLD`同样写`si_uid = 0`。这是明确的scoped ABI limitation，不得通过缓存leader credential、从任意live member猜测UID或在`ThreadGroup`中建立第二份credential state来填补。未来只有在credential owner能够提供跨leader exit与wait/report生命周期稳定的child identity snapshot后，才能由后续contract统一修正exit、stopped与continued的`si_uid`。

实现可以在不改变 target 的前提下调整这些物理细节，例如将 exposure 放入等价的 membership record、改变 owner-private container、通过测量后增加带一致性断言的派生 count，或证明条件性stop从fetch到commit已被同一个control transaction完整串行化而不再需要显式epoch字段。`StopAuthority`也不冻结为public enum或固定API signature；它只表达通过global-init admission的`SIGSTOP`无条件authority与另外三种信号的epoch-validated DefaultStop authority不能混淆。此类调整必须保持唯一owner、相同线性化、无stale candidate、无第二真相和无allocation-backed protocol；若需要改变owner、锁方向、可见语义、accepted limitation或引入跨Signal的persistent machinery，则必须停止实现并回到RFC review。

### ThreadGroup job-control state

ThreadGroup 持有三态 phase：

```text
Running
  -- admitted stop request --> Stopping(reason)

Stopping(reason)
  -- no user-exposed member --> Stopped(reason)
  -- SIGCONT generation -----> Running

Stopped(reason)
  -- SIGCONT generation -----> Running + Continued report
```

通过action admission的`SIGSTOP` generation与Signal已经最终选择的`SIGTSTP` / `SIGTTIN` / `SIGTTOU` `DefaultStop`都进入同一个stop engine；global init在该边界前被排除。重复stop request在`Stopping` / `Stopped`中合并，不替换首个stop reason，也不建立新report。`Stopping`被`SIGCONT`取消时不产生Stopped、Continued或对应SIGCHLD notification；只有已经提交的`Stopped`才能产生一次`Continued`。

`Exiting / Exited` 继续只属于 `ThreadGroupLifeCycle`，不是第四种 jobctl phase。jobctl phase只在 lifecycle为 `Alive` 时具有行为意义；terminal transaction在同一 owner域发布 `Exiting(first_code)`、清 exposure与report并释放 parker，此后 user-entry / procfs / wait必须先服从 terminal truth。实现可以把不再生效的 phase归一为 `Running`，也可以保留最后 phase作为明确标注的 inert diagnostic snapshot，但它不得在 terminal lifecycle下反向驱动任何行为。

### User exposure，而不是 participant ack

ThreadGroup 维护一个 owner-local user-exposure relation：成员在执行用户态，或已经通过最终 user-entry gate 但尚未重新进入内核时，才标记为 exposed。首选实现把它直接放在 live membership value中；target不要求必须使用 `BTreeMap`、独立 set或某个固定字段。

- user-to-kernel trap entry 清除当前 member exposure；
- Running 上的最终 user-entry gate 先登记 member exposure再允许 architecture transition；
- Stopping / Stopped 上的 gate保持 unexposed并 park；
- detach / exit 在移除 membership 前清除 exposure；
- fresh / clone / exec entry 在通过 gate 前保持 unexposed。

因此 stop completion 不需要扫描 scheduler state、等待 ordinary waiter ack 或保存 per-round participant ledger。`Stopping` 中不存在 exposed member即证明：所有仍在用户态的 member 都已经进入内核；其它 member 下一次执行用户指令前必经 gate。已经处于ordinary wait的member保持原active wait，predicate、timeout、source registration与result均不因stop改变；source或timeout在Stopped期间完成时，task可以正常收口syscall，但最终user entry必须park。`SIGCONT`的resume side effect只唤醒jobctl parker，不负责唤醒仍在原ordinary wait中的task；单独被接纳的custom `SIGCONT` occurrence仍按普通Signal notification语义处理，因此只有后者可以按既有Signal规则影响可中断wait。

这意味着parent观察到`Stopped`后，child已经进入的内核路径仍可继续执行并产生原syscall允许的内核侧副作用；本RFC只承诺停止user execution，不承诺kernel execution freeze。

### Control-signal transaction

stop-class signal 与 `SIGCONT` generation 进入一个窄的 Signal/jobctl transaction；四种stop signal在取得不同的admission authority后共享同一个ThreadGroup stop engine：

这里的 generation 只发生在 concrete nonzero occurrence 已通过 sender-specific target / permission validation之后；signal-0 probe或拒绝的发送不产生 cleanup、epoch或 resume side effect。global init是action admission的特殊边界，不伪装成target或permission failure：合法stop-class generation仍执行Linux式ordinary opposite-class pending cleanup，但`SIGSTOP`不得取得unconditional stop authority，另外三种signal在live action仍为default-stop时也不得取得conditional authority。caught / masked等ordinary occurrence继续服从Signal规则。

- 四种stop-class signal generation都清理全部ordinary private / shared pending `SIGCONT`；已经从ordinary queue中dequeue并由具体task独占的reserved-delivery `SIGCONT`不再受后来stop-class generation撤销。reservation只提交occurrence claim并终结pending competition，不冻结后续live disposition或action selection；
- `SIGSTOP`因不可捕获、不可忽略、不可屏蔽，对global init之外的普通user ThreadGroup在该generation transaction中直接请求stop，不进入private / shared ordinary pending，不建立reserved delivery，也不调用完成active wait的force notification；若live exposure已经为空，同一transaction直接提交`Stopped`与parent report。发送给global init的合法`SIGSTOP`在完成opposite cleanup后被消费，不进入pending也不改变jobctl phase；
- `SIGTSTP` / `SIGTTIN` / `SIGTTOU`捕获当前continue epoch后按普通disposition、mask与pending规则接纳；只有Signal最终选择`DefaultStop`、目标不是global init且epoch仍匹配时，才请求同一个ThreadGroup stop。caught、ignored或masked occurrence不提前停止group；global-init immunity与未来orphaned-pgrp suppression都属于该action-selection/admission边界，不进入stop engine；
- `SIGCONT` generation 递增 continue epoch、清理全部ordinary private / shared stop-class pending、无条件执行一次group resume side effect，再按普通 disposition / mask / handler 规则处理 occurrence；已经reserved的stop-class occurrence同样不被撤销，其中旧`DefaultStop` candidate由epoch mismatch取消jobctl effect；
- `SIGCONT` resume只在线性化该concrete generation时发生一次；reserved delivery、ordinary fetch、同步消费和default-action consume都只处理ordinary occurrence，绝不能重放resume side effect；
- 若条件性stop occurrence在fetch或action selection后与并发`SIGCONT`交错，stop request必须在ThreadGroup transaction中验证captured epoch；stale candidate只放弃jobctl effect，不得重新停止group、重新发布occurrence或改写Signal顺序。

该transaction只覆盖control-signal generation、opposite-class cleanup、eligible `SIGSTOP`直接stop、global-init action admission、条件性DefaultStop authority validation、candidate invalidation与jobctl phase，不扩展到其它signal。target / permission可在进入前校验，但transaction仍须重验ThreadGroup lifecycle以及sender-specific exact target / membership仍然有效；signal-0、permission failure、target失效或terminal lifecycle不得产生cleanup、epoch或stop / resume副作用。

user-entry 的 Signal arbitration 是 phase-aware 的：`Stopping / Stopped` 上只允许 `SIGKILL`、已提交的 terminal lifecycle、kernel-generated synchronous signal 的 no-return terminal action、能够合并当前 stop 的条件性 `DefaultStop` control action，以及此前已经完成occurrence claim的reserved `SIGCONT`在重新进入jobctl gate前收口；其它ordinary asynchronous signal和非`SIGCONT` reserved delivery保持pending，不能因为唤醒ordinary wait或jobctl park就提前执行handler或terminal default action。

reserved `SIGCONT`仍按action-selection时的live disposition处理。它可以在`Stopping / Stopped`期间提交custom handler frame，或通过ignore / default no-frame consume完成temporary-mask cleanup；但frame、trapframe和mask commit都不携带user-entry permit，最终architecture transition仍必须被live jobctl gate阻止。一次reserved retirement结束当前ordinary scan；随后只能继续检查支配性的terminal / control truth并进入gate，不能顺带消费其它普通异步信号。handler frame构造失败继续服从既有no-return terminal path；`SIGKILL`或已经提交的terminal lifecycle即使在frame提交后到达也必须在user entry前支配。jobctl park以live phase为durable predicate，在wait publication前后重验，避免与`SIGCONT` / terminal transition丢wake。`SIGSTOP`不需要走该fetch/action-selection路径；任意`SIGCONT` occurrence的后续action都不得重放generation-time resume。

### Parent report 与观察面

child ThreadGroup 持有一个有界 report slot：owner-local存储抽象为 `None | Stopped | Continued`，其中 Stopped reason只从同一 snapshot里的 `Stopped(StopEpisode)` 取得；wait对外形成 `Stopped(reason)` typed status。slot与phase在同一 owner线性化域内更新；parent Event与SIGCHLD都只通知重扫。

- `Stopped` commit 建立或覆盖为 Stopped report；
- `SIGCONT` 从 Stopped 恢复时使任何未消费 Stopped report 失效并建立 Continued；
- `WNOWAIT` 只 peek；consuming waiter在 topology / parent relation -> child owner transaction中重新检查 selector、phase与当前 slot，并原子取走当下仍 eligible 的 report，至多一个能消费；
- exit 清除 job-control report并在 wait selection 中最高优先；
- `SA_NOCLDSTOP` / ignored SIGCHLD 只抑制 signal occurrence，不删除 report，也不抑制 parent predicate wake。

scan阶段取得的 report snapshot不携带 claim authority。若进入 claim transaction时 slot已消失或变为不符合 options的 kind，waiter丢弃旧选择并重扫；若 replacement当前仍 eligible，claim返回并消费该 current report。target不要求 `ReportId`、generation或 allocation-backed token；实现证据若确实要求跨 unlock exact identity，必须先回到 RFC review说明为什么 current-state claim不能闭合。

report transition在 commit后使用一个 current-parent snapshot发送通知；child若带着非空 report并发 reparent，topology adoption必须唤醒 new parent重扫，但不重放历史 SIGCHLD。

procfs 只通过单次 ThreadGroup derived snapshot在 committed `Stopped` 时投影 `T`；observable leader `Zombie` 仍优先为 `Z`，`Stopping / Running` 继续显示当前底层 task state，`status` character / name由同一 snapshot序列化。本 RFC不改变 binding / leader resolution失败边界。

## ABI 与兼容边界

- `wait4(WUNTRACED / WCONTINUED)` 和 `waitid(WSTOPPED / WCONTINUED)` 返回 ordinary child job-control status。
- `waitid(..., WNOWAIT)` 可以重复观察未变化的 current report。
- stopped status 携带触发 stop 的 signal；continued status 携带 `SIGCONT`。
- stopped / continued `waitid`与对应job-control `SIGCHLD`的`si_uid`本阶段固定为`0`，不声称已经具备Linux credential projection。
- task-directed default-stop 仍停止整个 ThreadGroup；不存在 thread-local job stop。
- global init不因四种default-stop signal进入`Stopping / Stopped`；合法generation是否产生ordinary occurrence仍服从上述Signal边界。
- sender 在 occurrence 被接受后即可返回，不等待 stop completion。
- fork child 不继承 pending stop occurrence、phase 或 parent report；同一 ThreadGroup 的新线程与 exec new image不能逃过当前 stop。
- parent观察`Stopped`只证明没有member可以继续执行user instruction；已经进入的syscall或ordinary wait可以继续在内核收口。

本 RFC 明确不复制 Linux incomplete group-stop 加 `SIGCONT` 的 synthetic `CLD_STOPPED` / continued wait-state corner。该差异是 accepted scoped ABI deviation，必须由确定性竞态测试固定。

本 RFC 还接受一个与temporary-mask reservation绑定的窄竞态差异：已经reserved的opposite-class occurrence不再被后来control generation撤销。尤其是旧`SIGCONT` reservation可以在新的stop生效后完成live action selection和handler-frame / no-frame收口，但handler在下一次真实`SIGCONT`恢复group前不能进入用户态；恢复用的新occurrence可能因此与旧reservation形成额外一次handler观察。每个pre-existing reservation至多贡献一个额外occurrence；普通handler mask通常串行化两次观察，`SA_NODEFER`可能形成嵌套或反转观察顺序，`SA_RESETHAND`或期间发生的live disposition变化也可能使新occurrence不再进入handler。这里接受的是更早的occurrence-claim finality，不是resume side effect重放。

## Contract Impact

完整逐 ID delta 与 `UJ-CUTOVER` 定义见[目标与不变量](./invariants.md#contract-impact)。本 RFC 不在 Draft / Accepted 阶段改写 current contract；初始 job-control core、user-entry closure、child report / wait ABI、SIGCHLD 和 procfs projection 必须作为一个 integrated cutover unit 生效。

## 接受边界

接受本 RFC 表示接受：

- cooperative user-entry barrier，而不是 scheduler freeze；
- membership-bound exposure completion proof；
- ordinary wait保持原predicate、timeout、source registration和result；Stopped允许既有kernel execution继续，但不允许任何user execution越过gate；
- `SIGSTOP` generation直接参与ThreadGroup control ordering；另外三种stop signal的stale DefaultStop candidate必须被后来`SIGCONT`失效。首选基线使用窄`ContinueEpoch`，但target不把等价owner-local串行化强制成固定字段布局；
- global init参与合法stop-class generation的opposite cleanup，但永不向stop engine提交`SIGSTOP`或conditional `DefaultStop` authority；
- ordinary opposite-class pending在generation transaction中清理，而已经reserved的occurrence保持task-local claim finality；reservation不冻结live action，且`SIGCONT` resume只在generation发生一次；
- ThreadGroup phase / report 的唯一 owner，以及 procfs derived snapshot 边界；
- stopped / continued `si_uid = 0`的stage-1 ABI limitation，以及不得为此新增ThreadGroup credential truth；
- incomplete `Stopping` cancellation 对 parent 不可见；
- TTY、orphaned process group 和 ptrace 的后续 RFC 边界。

接受不等于实现开始、transaction 创建、current contract cutover 或 limitation 关闭。任何实现证据若要求 scheduler stop state、generic wait cancellation、通用 signal carrier，或无法封闭 user-entry path，必须回到 RFC review。

## 工程取舍准则

Linux / POSIX 已明确、常用并能在现有owner边界内自然实现的语义仍是默认兼容义务，不能仅因实现麻烦就降级。只有当语义属于偏僻corner或极难稳定复现的并发race，并且精确复制会产生明显不成比例的实现代价、跨越既有owner，或迫使尚未稳定的基础路径先做高风险重构时，才允许采用受限降级。

此时优先保持现有owner、核心状态不变量和可诊断性：能在已有typed error、no-user-entry gate或既有no-return terminal边界内安全拒绝时选择fail-close；不能自然fail-close时，诚实记录trigger、外部可见差异、影响范围、验证方式和退出条件，并停止为该corner继续扩张协议。fail-close不得把原本recoverable的ABI corner升级为kernel panic、hang、data corruption或新造的terminal outcome。不得以“Linux corner”为名静默fail-open、伪造成功、降低mandatory user-entry / terminal precedence等核心不变量，也不得为未复现的race随意修改稳定路径、添加case hack或启动跨owner重构。若无法判断降级是否安全，当前 Stage 必须停止并回到RFC review，而不是由实现自行猜测。

## 备选方案

### Scheduler execution barrier

该方案可以在更早物理位置 suppress task，但会把 owner 扩张到 scheduler placement、generic wait、同步原语 cleanup 和 terminal unwind。它已被拒绝。

### 所有 member ack 的 participant round

该方案要求 ordinary waiter 也响应 stop，因而需要 wake / cancellation 或长时间阻塞 completion。membership-bound exposure直接表达外部承诺，避免建立第二套 participant ledger，故不采用 ack round。

### 复制 Linux incomplete-stop notification corner

该方案会让 phase、wait report 与 SIGCHLD notification出现难以解释的 synthetic state。本 RFC选择 parent-invisible cancellation。

### 用 generation-time disposition / mask snapshot 提前提交条件性 stop

该方案可以让`SIGTSTP` / `SIGTTIN` / `SIGTTOU`更早进入jobctl，但会让generation后安装handler、改变ignore/default disposition或解除mask的ordinary Signal语义失效，并使process-directed member selection依赖陈旧snapshot。本RFC只允许通过global-init admission的`SIGSTOP`在generation time直接stop；另外三种信号必须等Signal最终选择`DefaultStop`并通过相同admission后再进入共享stop engine。

## 风险

- 任一 user-to-kernel 或 kernel-to-user path 漏掉 exposure transition都会破坏 `Stopped` barrier；必须逐架构审计并用 production runtime 验证。
- control-signal transaction若不能覆盖`SIGSTOP`直接stop、条件性stop的pending publication与epoch capture，late publication可能逃过`SIGCONT`cleanup或stale candidate可能重新停止group；这是document / implementation hard stop。
- 任一路径若能让global init取得`SIGSTOP`或conditional `DefaultStop` authority，init可能进入`Stopping / Stopped`并停摆系统控制链；这是document / implementation hard stop。
- exposed-user latency优化若只持有task snapshot而不重验live exposure，guards-out kick可能误中task后来建立的active wait；第一版不需要kick，未来若增加只能使用不完成active wait的stale-safe user-execution kick / IPI。
- reserved-delivery retirement或stopped-phase special case若丢失 temporary-mask restore responsibility、继续消费其它普通异步信号或绕过最终gate，task可能带错误mask返回用户态或在Stopped期间执行handler；这是Signal/jobctl handoff hard stop。
- report claim 若不在 topology / parent relation -> child owner transaction中重新读取 selector、phase与当前 slot，可能产生 stale status、消费不再 eligible 的状态或多 waiter double-consume。
- stop latency 取决于 exposed user task 到达 kernel entry；RFC 只要求可诊断，不承诺时间上界。

## 收口

当前为 R0 Accepted for Implementation。逻辑数据模型与首选实现基线已经形成，但具体字段布局、owner-private API、container、锁实现和后续 implementation stage 仍允许在上述 target 边界内根据源码与 probe 证据调整。Stage 0 行为保持型 Signal module split 与 Stage 1 dormant ThreadGroup/user-entry foundation 已关闭；尚未发生 `UJ-CUTOVER`、production stop / continue ingress、runtime job-control 可见语义或 current effective rule 变化，Stage 2 未开始。

## 修订记录

| 修订 | 日期 | 状态 | 语义变化 | Review / 事务 |
| --- | --- | --- | --- | --- |
| R0 | 2026-07-20 | Accepted for Implementation | 初始 accepted target；定义 ThreadGroup-owned stop / continue、mandatory user-entry barrier、child report、Signal ordering、procfs projection 与 `UJ-CUTOVER`。 | [2026-07-20-unix-jobctl](../../devlog/transactions/2026-07-20-unix-jobctl.md) |
