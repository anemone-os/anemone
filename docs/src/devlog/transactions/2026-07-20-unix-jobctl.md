# 2026-07-20 - Unix Job Control

**Status:** Completed
**Owners:** doruche, Codex
**Area:** signal / task / process group / user entry / wait ABI / procfs
**Canonical Plan:** [RFC-20260720-unix-jobctl](../../rfcs/unix-jobctl/index.md), [目标与不变量](../../rfcs/unix-jobctl/invariants.md), [迁移实施计划](../../rfcs/unix-jobctl/implementation.md)
**Canonical Revision:** R1
**Current Phase:** Stage 5 Closed / `UJ-CUTOVER` Effective

## Scope

本事务执行RFC R0/R1的完整Stage 0至Stage 5。Stage 0完成Signal行为保持型目录化拆分；Stage 1建立
ThreadGroup-owned dormant job-control与统一user-entry gate；Stage 2贯通generation-time control、
child report、wait ABI、SIGCHLD、procfs与单线程production slice；Stage 3A/3B关闭conditional
control、reservation、temporary-mask、多成员、lifecycle与topology；Stage 4完成owner-local repair、
完整production validation与review；Stage 5将current contracts、RFC、register与导航作为同一个
`UJ-CUTOVER`原子切换。当前全部Stage关闭，candidate已成为integrated effective implementation。

## Contract and register boundary

`UJ-CUTOVER`已经更新`SIGNAL-PENDING-*`、`SIGNAL-ACTION-*`、`SIGNAL-TEMP-MASK-*`、
`PROCFS-TASK-STATE-001`、`PGRP-SIGNAL-*`、`TASK-LIFE-*`、`CHILD-WAIT-*`与`USER-ENTRY-*`，并在
`contracts/task/job-control.md`创建全部`JOBCTL-*` Active规则。register中的process-group stage-1
限制只保留TTY/foreground/orphaned边界；ptrace、stopped/continued `si_uid = 0`与guards-out
SIGCHLD publication ordering分别保持Active limitation，不伪装成已实现或已修复。

## Stage 0 preflight and resolved write-set manifest

R0 acceptance、transaction 创建、RFC/SUMMARY 导航和 pending-successor 导航均已完成。
Stage 0 的逐文件 manifest 冻结如下：

- `anemone-kernel/src/task/sig/mod.rs`
- `anemone-kernel/src/task/sig/mask.rs`（新建）
- `anemone-kernel/src/task/sig/pending.rs`（新建）
- `anemone-kernel/src/task/sig/generation.rs`（新建）
- `anemone-kernel/src/task/sig/delivery.rs`（新建）
- `anemone-kernel/src/task/sig/disposition.rs`（仅在 sibling visibility 必要时收窄）
- `anemone-kernel/src/task/mod.rs`（仅 import/re-export 路径调整）
- 本事务日志及 RFC/contract 导航的执行事实更新

其它 kernel、architecture、scheduler、wait、topology、apps、tests、rootfs、LTP profile
和 effective contract 正文为只读。任何需要超出此 manifest、扩大 public API、改变 owner、
lock order、ABI、visible semantics 或 target invariant 的情况，均按 implementation plan
停止条件停止，不在本事务内绕行。

## Inventory evidence

Stage 0 前的 source inventory 记录了当前 `sig/mod.rs` 约 1380 行的职责边界：

- `TaskSigMaskState`、temporary-mask token 与 mask mutation 位于既有 mask owner；
- `PendingSignals` 及 reserved-delivery queue primitive 位于 pending leaf；
- `Task::recv_signal`、`ThreadGroup::recv_signal`、pending flush 和 notification admission
  位于 generation；
- private/shared source selection、temporary-mask classifier、handler frame、no-frame
  cleanup、`handle_signals` 与 ordinary action loop 位于 delivery；
- `SigNo` 与 `Signal` 保留在 module root，既有 `disposition`、`info`、`set`、`altstack`、
  `hal`、`api` 子模块不移动。

外部调用面、direct field access 和锁序已核对：现有 `Task` / `ThreadGroup` inherent method、
`handle_signals`、temporary-mask types、pending snapshots、signal constructors 与 syscall/
architecture callers 保持 root symbol 形状；现有 `sig_pending -> sig_mask -> disposition`
顺序、shared pending 的 topology guard 和 notification guards-out 关系只做机械搬迁。

## Stage 0 execution log

### 2026-07-20 - Split-only implementation

**Change:** 将 mask、pending、generation、delivery 的既有实现移动到 manifest 指定模块；
`mod.rs` 保留 module docs、声明、窄 re-export、`SigNo` 与 `Signal`。跨 sibling 使用的
helper 只收窄为 `pub(super)`，没有新增 public API。没有创建 `task/jobctl`，没有修改
architecture、wait、scheduler、topology 或 syscall 行为。

**Review focus:** 逐项核对 root export、Task/ThreadGroup inherent method、pending fetch
顺序、temporary-mask restore responsibility、reserved-delivery finality、handler-frame
commit/no-frame cleanup、notification 与锁序。若发现 ownership、reservation、temporary
restore、generic carrier 或 visibility 需要语义扩张，Stage 立即停止。

**Validation:**

- `just fmt kernel --check` 通过。
- `git diff --check` 通过；四个新文件分别通过 `git diff --no-index --check` 空白检查。
- 首次在 sandbox 中运行 `just build` 时，`lwext4` 编译子进程因 sandbox `Bad system call` 失败；随后以仓库入口、经批准的 escalation 重跑同一 `just build`，构建通过。该环境失败不被记录为代码失败。
- source audit 通过：79 个函数体 hash 未发现删除或改变；root 不再包含 `impl Task`、`impl ThreadGroup`、`handle_signals` 或 `perform_signal_action`；调用者闭包、sibling visibility、direct-field 使用及既有锁序保持不变。
- 按 Stage 0 规定未运行 QEMU/LTP，也未运行 LA64；本阶段不产生 runtime 证据。

**Review:** 未另行启动独立 reviewer；完成冻结 review focus 的 self-review，未发现
Apollyon、Keter 或 Euclid 级别的 owner、生命周期、锁序、ABI 或可观测语义问题。每一行
代码改动均限于职责移动、module declaration、import 或 sibling visibility 收窄。

**Result:** Stage 0 split-only checkpoint closed。行为保持型模块拆分完成；没有新增
jobctl state、runtime ingress、scheduler/wait/topology 变化或 public API，`UJ-CUTOVER`
仍为 None，Stage 1 Not Started。

## Stop conditions and feedback

本 checkpoint 未命中 Stage 0 停止条件。没有 tracking issue 文件；当前没有已确认的
target blocker。若后续阶段发现无法保持 pending ownership、reservation semantics、
temporary-mask restore protocol、lock order 或现有 visibility，则停止并回写 implementation
plan / RFC review，不通过兼容桥或额外 manager/carrier 继续。

## Handoff

Stage 0 关闭后，下一 gate 只能是 Stage 1 manifest 冻结与 dormant ThreadGroup/user-entry
foundation preflight。Stage 1 不在本事务当前变更中执行，也不因本 checkpoint 自动开始。

## Correction note - 2026-07-21

收口复核发现总 RFC 索引、双周 devlog 和 invariants 页仍保留 Stage 0 启动期措辞；本条
补充后已同步为 Stage 0 closed、Stage 1 Not Started。该修正只澄清文档状态，不改变 R0
target、current contract、register、write set 或 validation floor。

## Stage 1 preflight - 2026-07-21

**Authority and transaction correction:** 用户明确授权完成 Stage 1，并反馈现有
`task/api/jobctl` 可以迁移到 `task/jobctl/api`。本 transaction 对应仍在实现中的同一 R0，
页首 `Closed` 只正确描述 Stage 0 checkpoint、错误地关闭了整条 R0 transaction；现已更正
为 `Active`，同时保留 Stage 0 closed 的全部历史事实。该状态修正不重开 Stage 0、不改变
R0 target，也不创建并列 transaction。

**Resolved Write Set Manifest:** Stage 1 manifest 已冻结为：

- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/task/jobctl/{mod,group,user_entry}.rs`
- `anemone-kernel/src/task/jobctl/api/{mod,getpgid,getsid,setpgid,setsid}.rs`
- `anemone-kernel/src/task/api/mod.rs` 与旧
  `anemone-kernel/src/task/api/jobctl/{mod,getpgid,getsid,setpgid,setsid}.rs`
- `anemone-kernel/src/task/topology/{mod,thread_group}.rs`
- `anemone-kernel/src/task/api/clone/mod.rs`
- `anemone-kernel/src/task/api/execve/kernel.rs`
- `anemone-kernel/src/arch/{riscv64,loongarch64}/exception/trap/utrap.rs`
- `anemone-kernel/src/arch/{riscv64,loongarch64}/sched.rs`
- RFC implementation plan 与本 transaction 的 Stage 1 执行事实更新

旧 syscall 目录到 `task/jobctl/api` 的移动只允许改变物理归属和 module declaration；syscall
registration、ABI、policy、可见语义与 public API 不得扩大。lifecycle、wait、procfs、Signal
semantic path、scheduler、apps、rootfs、LTP profile、LA64 runtime 和 effective contract 正文
保持只读。

**Entry and membership inventory:** live source 中的 ThreadGroup construction 位于
`topology/mod.rs` 的 root、user leader 与 kthread 三条路径；user member join 位于同文件的
`TaskBinding::Member`，detach 与 dethread membership key replacement 位于
`topology/thread_group.rs`。两架构 ordinary return 位于各自 `rust_utrap_entry()`，fresh task
位于各自 `sched.rs::user_task_entry_secondary()`，clone child 经
`enter_cloned_user_task() -> TrapArch::load_utrapframe()`，exec new image 经
`load_context(TaskContext::from_user_fn(...))` 落入相同 fresh path。两架构 raw
`utrap_return_to_task()` wrapper 是 fresh / clone / exec 的共同最终 user-transition facade。

**Frozen baseline:** 输入冻结为仅含 `signal` / `wait` 的现有 LTP profile、
`etc/sdcard-rv.img`、`./scripts/run-user-test-rv64.sh` 和日志
`build/unix-jobctl-stage1-baseline-rv64.log`。修改前 wrapper 已完成完整 rootfs、kernel、QEMU
链；182 项 KUnit 全部通过。glibc 与 musl 各自结果均为
`attempted=56 passed=49 failed=5 infra_failed=0 skipped=2`，合计
`attempted=112 passed=98 failed=10 infra_failed=0 skipped=4`。Stage 1 退出运行必须用相同输入
对比 exact case/result classification；这些既有 failure 不得掩盖新增回退。

**Cutover / register:** `UJ-CUTOVER` 仍为 None；current contract 与
`ANE-20260527-PROCESS-GROUP-SESSION-STAGE1` 保持不变。Stage 1 只建立 dormant readiness，
不得出现 production stop / continue ingress，也不得进入 Stage 2。

## Stage 1 execution log - 2026-07-21

**Change:** 新建 `task/jobctl/{mod,group,user_entry}.rs`。user ThreadGroup 构造 dormant
`Running` phase、continue ordering identity与 predicate-only `jobctl_unblocked` capability；
membership wrapper直接在每个 live user member value中承载 `Unexposed / Exposed`，没有
task-local flag、participant set或派生 behavioral counter。kthread construction保留
`job_control=None`，user/kthread presence由 `ThreadGroupType` construction与 shape assertion
约束。旧 `task/api/jobctl` 同 owner物理迁移到 `task/jobctl/api`，四个 syscall实现文件内容
逐字不变，registration、ABI与 policy不变。

**Entry closure:** RV64 与 LA64 的 `rust_utrap_entry()` 在继续内核处理前通过
`on_user_trap_entry()`清 exposure。两架构 ordinary return与 raw
`utrap_return_to_task()` facade均在 FPU restore前调用 `before_user_entry()`；fresh task经
`sched.rs::user_task_entry_secondary()`、clone child经 `TrapArch::load_utrapframe()`、exec new
image经 `TaskContext::from_user_fn()`进入该 facade。gate只在 ThreadGroup owner下登记
exposure；FPU restore完成后才发布 `Privilege::User`，因此 future park不会跨 context switch
保留错误 FPU ownership。gate没有外层 guard、active wait registration或线性 token；Event
只发布 predicate rescan，不携带 phase或 permit。

**Membership / owner audit:** root、user leader、kthread三条 construction，user member join、
ordinary detach、dethread rekey与 kthread unpublish均已闭合。user detach / rekey要求
`Unexposed`，trap entry与 Running gate分别断言 `Exposed -> Unexposed` 与
`Unexposed -> Exposed`；generic removal只接受 kthread variant。`Option<UserJobControl>`不参与
ThreadGroup type决策，diagnostic-only phase timestamps在字段旁明确不驱动行为。未发现
Signal ingress、scheduler stop state、wait/lifecycle/procfs semantic change或旧 syscall路径残留。

**Review:** 独立 reviewer 初检发现 task-internal visibility与 FPU/gate顺序两个 Keter，均在
runtime前修复；后续 Euclid 指出的 removal escape、幂等 exposure写入、diagnostic字段标注与
状态导航分叉也已 neutralize。最终复核未留 Apollyon、Keter或 Euclid。未为 dormant gate
强造 KUnit；真实 trap entry与 lifecycle closure由 mandatory RV64 wrapper及源码审计覆盖。

**Validation:**

- `just fmt kernel --check` 通过。
- `just build` 通过；sandbox内首次 lwext4 C build因 `Bad system call`被阻止，随后使用同一
  repository入口经批准在 sandbox外构建通过，该环境失败不记为代码失败。
- `./scripts/run-user-test-rv64.sh etc/sdcard-rv.img build/unix-jobctl-stage1-rv64.log`
  完整通过并正常关机；KUnit 182 项全部通过。
- glibc与musl各自为
  `attempted=56 passed=49 failed=5 infra_failed=0 skipped=2`，合计
  `attempted=112 passed=98 failed=10 infra_failed=0 skipped=4`。
- 以 libc、testcase名与exit code归一化比较
  `build/unix-jobctl-stage1-baseline-rv64.log` 和最终日志，diff为空；没有新增结果分类回退。
- RV64 / LA64 source closure、syscall registration uniqueness、新文件 whitespace与
  `git diff --check` 通过；按 Stage 1 floor未运行 LA64 runtime。

**Docs-only closure write set:** 为同步唯一 canonical 状态，closure增加
`docs/src/rfcs/unix-jobctl/index.md`、`docs/src/rfcs.md` 与当前双周 devlog。该扩展只写
Stage 1 closed / Stage 2 Not Started及已有验证证据，不修改 R0、invariants、current contract、
register、ABI、visible semantics或 `UJ-CUTOVER=None`；验证为 stale wording / link audit和
`mdbook build docs`。

**Result:** Stage 1 checkpoint closed。所有 production user ThreadGroup仍为 `Running`，没有
signal可触发 stop / continue。Stage 2 manifest未冻结、未获授权且未进入实现；下一步只能在
新的明确授权下执行 Stage 2 preflight。

## Stage 2 preflight and resolved write-set manifest - 2026-07-21

**Authority:** 用户明确授权完成 Stage 2，并在preflight发现typed child siginfo owner越出默认
write set后，批准纳入`anemone-kernel/src/task/sig/info.rs`；若现有枚举或`CLD_*`表示不足，
同时允许纳入`anemone-abi`。live ABI已定义`CLD_STOPPED = 5`、`CLD_CONTINUED = 6`与
`SA_NOCLDSTOP`，所以本checkpoint只需要扩展typed kernel representation，不修改ABI crate。
该批准不改变R0 target、owner、visible semantics、acceptance、current contract或
`UJ-CUTOVER=None`。

**Lock / call graph probe:** kill / tkill / tgkill / rt_sigqueueinfo 与process-group broadcast在
signal-0、target和permission检查成功后都可汇入统一generation入口。控制事务按
`exact identity / topology -> ThreadGroup owner -> at most one private/shared Signal leaf`顺序闭合；
process-group只提供ThreadGroup snapshot selector，每个ThreadGroup独立接受generation，不需要
ProcessGroup-wide phase或rollback。phase/report commit在ThreadGroup owner内完成，notify、
`jobctl_unblocked`、parent child-status Event与SIGCHLD publish全部在释放owner guard后执行。
wait consuming path按`topology / parent relation -> child ThreadGroup owner`重新验证selector和当前
report slot；scan snapshot不携带claim authority。因此probe未发现反向锁、guards-in wake / user
copy、post-commit recoverable failure、ReportId、第二truth或singleton分流需求。

**Resolved Write Set Manifest:** Stage 2逐文件manifest冻结为：

- `anemone-kernel/src/task/sig/{generation,delivery,pending,mod,disposition,info}.rs`
- `anemone-kernel/src/task/sig/api/{kill,tkill,tgkill,rt_sigqueueinfo}.rs`
- `anemone-kernel/src/task/jobctl/{mod,group,user_entry,report}.rs`
- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/task/topology/{mod,thread_group,parent_child}.rs`
- `anemone-kernel/src/task/topology/process_group.rs`（2026-07-21 review扩展批准；仅control
  generation的expected-PGID重验）
- `anemone-kernel/src/task/api/wait/{mod,wait4,waitid}.rs`
- `anemone-kernel/src/task/api/exit/mod.rs`
- `anemone-kernel/src/arch/{riscv64,loongarch64}/exception/trap/utrap.rs`（2026-07-21 final
  owner review扩展批准；仅统一final user-entry重新仲裁）
- `anemone-kernel/src/fs/proc/tgid/{stat,status}.rs`
- `anemone-apps/jobctl-test/{Cargo.toml,Cargo.lock,app.toml,src/main.rs}`
- `anemone-apps/user-test/src/{main,guest}.rs`
- `conf/rootfs/pretest-rv64.toml`
- `docs/src/rfcs/unix-jobctl/{index,invariants,implementation}.md`
- `docs/src/register/current-limitations.md`
- `docs/src/rfcs.md`
- `docs/src/devlog/2026-07-06_to_2026-07-19.md`
- 本transaction文件

`anemone-abi`已获条件授权但不在当前resolved manifest，因为现有常量足以表示target；只有后续
实现证明确有ABI representation缺口时才停止并记录实际扩展。`task/sig/api/mod.rs`、
Signal syscall以外API、LA64 runtime、scheduler/wait-core、
generic active-wait machinery、LTP profile/groups、TTY、orphaned-pgrp、ptrace与current
contract正文保持只读。

**Frozen validation floor:** `just fmt kernel --check`、`just fmt jobctl-test --check`、
`just build`、`./scripts/run-user-test-rv64.sh etc/sdcard-rv.img
build/unix-jobctl-stage2-rv64.log`与`git diff --check`。focused app必须覆盖wait4 stop/continue、
waitid `WSTOPPED / WCONTINUED / WNOWAIT`、job-control SIGCHLD、procfs committed `T`、SIGSTOP
no-pending与global-init immunity；冻结的signal/wait profile相对Stage 1不得新增回退。源码审计
必须确认没有group-size runtime分流、scheduler hold、generic wait completion、credential truth或
Event-carried report identity。Stage 2关闭前须完成独立review并清零Apollyon、Keter与Euclid；
本checkpoint不进入Stage 3A。

## Stage 2 runtime harness correction - 2026-07-21

**Evidence:** 首次integrated RV64日志中`wait4-stop-continue-procfs`、
`waitid-wnowait-sigchld`与`global-init-immunity`全部通过；focused app退出并开始进入competition
root后，内核在`proc_root_lookup: pde exists but its inode does not exist`处panic。用户确认当前
procfs虽然设计上允许多次挂载，但实现只可靠支持单次挂载；focused app在boot root自行建立
procfs mount、competition environment随后再次mount的route触发了该已知限制。这不是job-control
producer、wait ABI或procfs stopped projection失败，也不授权修改procfs。

**Approved write-set expansion:** 用户批准把`anemone-apps/user-test/src/guest.rs`纳入Stage 2
resolved manifest。该文件只在chroot前把boot root的`/bin/jobctl-test`复制到competition image；
`user-test/src/main.rs`改为先chroot并由既有environment建立系统唯一procfs mount，再运行focused
app，随后进入LTP。`jobctl-test`删除自行mount fallback，只要求harness已经提供`/proc`。原manifest
其余边界不变；不修改procfs、R0 target、owner、ABI、visible semantics、acceptance、register、
current contract或`UJ-CUTOVER=None`。

**Validation plan:** 重新执行`just fmt user-test --check`、`just fmt jobctl-test --check`、
`just build`与RV64 wrapper；focused三组case必须再次通过，competition environment只能建立一次
procfs mount，LTP完成后必须走到user-test normal shutdown。最终Stage 2 validation floor与独立
Apollyon / Keter / Euclid review仍保持不变。

## Stage 2 control-target revalidation correction - 2026-07-21

**Finding:** 独立owner review确认两个Apollyon。第一，`kill(0, ...)` / `kill(-pgid, ...)`从
ProcessGroup取得snapshot后，目标可能并发`setpgid`离组；原generation未重验snapshot PGID，
因此仍会清pending并误停/误续目标。第二，`rt_sigqueueinfo`按exact PID/TID解析Task后只保留
shared occurrence route，若该Task并发detach / dethread，control side effect可能落到不再包含该
identity的ThreadGroup。另有last-member已detach而lifecycle尚未terminal的短窗口，空group上的
SIGCONT会panic、SIGSTOP会伪造report。

**Approved expansion and repair:** 用户批准把`task/topology/process_group.rs`纳入Stage 2
manifest。control generation现在分别携带whole-ThreadGroup、exact private member、exact member +
shared occurrence或expected PGID route，并在任何opposite cleanup、epoch、phase或pending副作用前
重验live nonempty membership及source-specific identity。`ProcessGroup::recv_signal()`与kill的PGID
branches使用同一路径；ordinary signal仍保持existing snapshot semantics。`rt_sigqueueinfo`保留
shared pending publication，同时携带exact resolved member用于generation revalidation。该修复不
改变PGRP-SIGNAL owner、process-group-wide atomicity、public API、R0 target、ABI、acceptance、
current contract或`UJ-CUTOVER=None`。

## Stage 2 parent-effect ordering review - 2026-07-21

**Finding and bounded repair:** 独立并发review确认，原实现先在child owner内提交report，释放guard
后才重新解析parent；若旧parent在两步之间退出并把child adopt给init，历史Stopped / Continued
transition会把SIGCHLD错误发送给new parent。当前实现改为`topology -> child ThreadGroup owner`窄事务：
在phase/report commit的同一snapshot中固定current-parent `Arc`，释放全部guard后再向该Arc发布
predicate Event与可选SIGCHLD。该Arc只服务一次guards-out effect，不标识report、不参与wait claim或
child状态机。last-exposure closure使用两段式重验，只在可能完成Stopping时进入topology transaction；
并发SIGCONT / terminal transition会在report commit前fail closed。live ThreadGroup / exact member继续
使用廉价`Arc` identity重验；用户明确裁决数值TID / PGID复用属于既有边界，本RFC不引入generation ID、
稳定身份表或第二truth。

**Accepted limitation:** 同一child的Stopped / Continued / terminal真实transition可以在owner guard
释放后交错，使较早transition的optional SIGCHLD occurrence晚于较晚transition入队。child-owned
current status、wait4 / waitid、procfs与predicate Event不受影响；严格串行化则需要跨guards-out
effect的bounded sequencer或持锁publication，前者会引入新persistent notification protocol，后者
违反R0 guards-out边界。用户接受该
罕见窗口不阻塞Stage 2，并授权只作记录；已新增
[`ANE-20260721-JOBCTL-SIGCHLD-PUBLICATION-ORDER`](../../register/current-limitations.md#ane-20260721-jobctl-sigchld-publication-order)。
这不削弱`JOBCTL-REPORT-001`的child-owned durable truth，也不向R0添加SIGCHLD total-order承诺。

**Approved write-set expansion:** `docs/src/register/current-limitations.md`只允许记录上述SIGCHLD
guards-out ordering窗口、影响范围与退出条件；RFC target/current contract仍不修改，
`UJ-CUTOVER=None`。

## Stage 2 user-entry arbitration correction - 2026-07-21

**Finding:** final owner review确认一个Apollyon：原gate在`jobctl_unblocked` Event或force wake后
直接用live `Running` phase登记exposure并返回，不重新执行Signal / lifecycle arbitration。
因此Stopped task收到custom `SIGCONT`后可以先返回用户态、延迟handler到下一次trap；`SIGKILL`
force wake会被uninterruptible Event loop吞掉；`kernel_exit_group`已经提交`Exiting`但尚未完成
member SIGKILL publication时，parker也可能在terminal truth之后取得user-entry permit。

**Approved expansion and repair:** 用户批准把RV64 / LA64的
`arch/*/exception/trap/utrap.rs`纳入Stage 2 manifest。两个架构的ordinary return与共同fresh /
clone / exec return facade只调用一个Signal-owned arbitration loop：Signal pass完成后在关中断下
检查lifecycle与jobctl gate；park wake只返回重新仲裁，只有`Alive + Running`才登记exposure。
`Exiting`直接进入既有`kernel_exit` no-return路径。该修复不增加task flag、
permit token、scheduler state或architecture policy分叉，不改变target、ABI、owner、visible
semantics、acceptance、current contract或`UJ-CUTOVER=None`。

**Validation plan:** 双架构source / format closure、`just build`、focused RV64 cases与冻结的
signal / wait profile；不运行LA64 runtime，不进入Stage 3A。

## Stage 2 closure - 2026-07-21

**Change:** Stage 2最终通用路径已经贯通generation-time direct `SIGSTOP`、generation-time
`SIGCONT` resume/opposite cleanup、global-init immunity、ThreadGroup-owned phase/exposure/report、
wait4 / waitid / WNOWAIT、job-control SIGCHLD、procfs committed `T`与统一final user-entry
arbitration。`SIGSTOP`不进入ordinary pending或完成active wait；masked default `SIGCONT`
occurrence保持pending并在unblock时按live action选择。focused app复制到competition root后复用
其唯一procfs mount，不再触发已知multi-mount冲突。

**Final review:** 独立owner与ABI reviewer最终均为Apollyon 0、Keter 0、Euclid 0。source audit
确认没有group-size runtime分流、scheduler hold、generic active-wait completion、credential truth、
Event-carried report identity或ordinary SIGSTOP pending；RV64 / LA64 ordinary与fresh / clone / exec
return均在FPU restore和`Privilege::User` publication前使用统一arbitration。explicit `SIG_IGN`与
`SA_NOCLDSTOP`没有独立focused runtime case，LA64只完成source/build closure；这些是Safe证据缺口，
不追加临时实现。guards-out SIGCHLD publication ordering、数值TID / PGID复用与既有final-gate到
hardware return窗口按已接受边界保留记录。

**Validation:**

- `just fmt kernel --check`、`just fmt user-test --check`、`just fmt jobctl-test --check`与
  `git diff --check`通过。
- `just build`通过；`just app build user-test --arch loongarch64`通过，LA64 runtime按Stage 2边界未运行。
- `./scripts/run-user-test-rv64.sh etc/sdcard-rv.img build/unix-jobctl-stage2-rv64.log`完整运行并
  正常关机；182项KUnit全部通过。
- `masked-default-sigcont-live-action`、`wait4-stop-continue-procfs`、
  `waitid-wnowait-sigchld`与`global-init-immunity`全部通过，`jobctl-test: all cases passed`。
- glibc与musl的`signal`各为
  `attempted=37 passed=30 failed=5 infra_failed=0 skipped=2`，`wait`各为
  `attempted=19 passed=19 failed=0 infra_failed=0 skipped=0`；总计
  `attempted=112 passed=98 failed=10 infra_failed=0 skipped=4`，与Stage 1冻结baseline一致。

**Docs-only closure expansion:** 用户批准补齐`docs/src/rfcs/unix-jobctl/index.md`；为保持唯一状态
入口一致，同时更新`docs/src/rfcs.md`与当前双周devlog。只写Stage 2 closed、Stage 3A Not Started、
已有validation/review证据与non-publishable边界，不修改R0 target、invariants、current contract、
ABI、visible semantics、acceptance或`UJ-CUTOVER=None`。

**Result:** Stage 2 checkpoint closed。candidate train保持non-publishable，current contracts仍为
effective，`UJ-CUTOVER=None`。Stage 3A未获授权且未进入。

## Stage 3A preflight and resolved write-set manifest - 2026-07-21

**Authority:** 用户本轮明确授权完成Stage 3；本checkpoint只激活3A，不提前进入3B。Stage 2已经
关闭且candidate保持non-publishable，current contracts继续effective，`UJ-CUTOVER=None`。

**Producer / owner inventory:** `kill / tkill / tgkill / rt_sigqueueinfo`、普通
`Task::recv_signal()`、`ThreadGroup::recv_signal()`与`ProcessGroup::recv_signal()`最终都经过
`generation.rs`的private、shared、exact-member或expected-PGID route。private/shared pending、
unreliable/realtime occurrence与task-private reservation由`PendingSignals`拥有；temporary-mask
linear token由`TaskSigMaskState`拥有，只有Signal classifier可以把一个已claim occurrence移入
reservation。live disposition、handler-frame commit、no-frame restore与no-return terminal action
仍由Signal delivery owner负责。ThreadGroup只拥有continue epoch、phase、exposure与report；wait、
procfs、Event与architecture entry不保存epoch或reservation truth。
clone / clone3还允许保存任意合法terminate signal；child exit在`task/api/exit/mod.rs`把该signal
交给parent `ThreadGroup::recv_signal()`，所以该链同样作为只读producer纳入审计，统一generation
facade已经覆盖它，不需要producer-local改动。

**Lock / call graph probe:** control generation保持`exact identity / topology -> ThreadGroup owner ->
at most one private/shared Signal leaf`；conditional occurrence在同一owner transaction捕获窄
`ContinueEpoch`后才进入ordinary pending。delivery claim在ThreadGroup phase read guard下进入
Signal leaf，避免已经处于Stopping / Stopped时claim普通async occurrence；claimed signal释放guard
后执行live action，最终user-entry gate仍重验terminal与phase。没有发现需要反向锁、second queue、
persistent carrier、generic final-consumption framework、wait-core或scheduler状态的证据。

**Resolved Write Set Manifest:** authoritative manifest已冻结在
[Stage 3A实施段](../../rfcs/unix-jobctl/implementation.md#当前-resolved-write-set-manifest2026-07-21-1)：

- `anemone-kernel/src/task/sig/{mod,info,pending,generation,delivery,disposition}.rs`
- `anemone-kernel/src/task/jobctl/group.rs`
- `anemone-kernel/src/task/api/exit/mod.rs`（仅纠正既有child-exit siginfo code）
- `anemone-apps/jobctl-test/src/main.rs`
- `docs/src/rfcs/unix-jobctl/{index,implementation}.md`
- `docs/src/rfcs.md`
- `docs/src/devlog/2026-07-06_to_2026-07-19.md`
- 本transaction文件

`task/sig/mask.rs`、control syscall producer文件、`rt_sigaction / rt_sigsuspend`、`ppoll /
pselect6`、topology/lifecycle、wait/report、procfs、architecture、scheduler、rootfs与current contract
正文只读。任何真实owner修复若需越界，必须先按停止合同报告并扩展manifest。

**Frozen validation / review floor:** `just fmt kernel --check`、`just fmt jobctl-test --check`、
`just build`、RV64 wrapper与`git diff --check`；focused case覆盖caught / ignored / masked conditional
stop、private/shared competition、opposite-class cleanup、reserved old SIGCONT、temporary-mask
handler/no-frame cleanup、SA_NODEFER、SA_RESETHAND、frame failure与普通SIGKILL no-return路径。冻结的signal /
wait profile相对Stage 2不得新增回退；LA64只做source/build closure。3A关闭前独立review必须为
Apollyon 0、Keter 0、Euclid 0。

## Stage 3A synchronous-fault authority correction - 2026-07-21

**Finding:** 独立代码review确认一个Apollyon：Stopped-phase fetch原先只检查
`SiCode::Kernel + default terminal`，但该code还被child exit signal、SIGPIPE、OOM与group-exit
producer使用。于是普通可屏蔽异步terminal signal可能在Stopped期间提前执行default action，违反
只允许kernel-generated synchronous fault no-return action支配jobctl gate的target。

**Approved write-set expansion and repair:** 用户批准把`task/sig/info.rs`加入3A exact manifest，
并允许必要时扩到`anemone-rs`。当前typed `SigInfoFields::Fault / Ill`已经完整标识现有同步fault
producer，因此修复只在Signal owner内组合`SiCode::Kernel`与typed fields形成窄判定；异步
`Kill / Chld`不再取得同步authority。`anemone-rs`无需修改，不进入实际write set。Linux 6.6的
child-exit siginfo使用`CLD_EXITED / CLD_KILLED / CLD_DUMPED`，而当前Anemone exit producer使用
`SiCode::Kernel + Chld`是既有ABI缺口；本checkpoint不修改exit producer，也不把该旧问题改写成
R0 target或3A责任。

**Validation:** 新增focused runtime case，使Stopped child只在收到`SIGCONT`后才处理grandchild
产生的异步clone exit signal；重新执行3A完整format、双架构build、RV64 wrapper、profile与独立
review floor。

## Stage 3A child-exit siginfo correction - 2026-07-21

**Correction to the prior boundary:** 用户随后明确批准修改exit producer，只要改动局限于它产生的
signal。3A exact manifest因此加入`task/api/exit/mod.rs`；该producer现有child-layout fields保持
不变，只按`ExitCode`选择Linux 6.6对应的`CLD_EXITED`或`CLD_KILLED`，不再错误使用
`SI_KERNEL`。这项既有ABI纠正不改变exit lifecycle、parent选择、notification ordering、R0 target、
current contract或`UJ-CUTOVER=None`；`anemone-rs`仍无实际修改需求。

## Stage 3A closeout - 2026-07-21

**Stage 3A result:** frozen manifest implementation is complete. The current candidate passes 184
KUnit tests and all 13 focused `jobctl-test` cases. The same RV64 wrapper input records glibc/musl
`wait=19/19` and aggregate `attempted=112 passed=98 failed=10 infra_failed=0 skipped=4`, followed by
normal shutdown. Independent review against R1 is Apollyon 0, Keter 0, Euclid 0. The generated-file-only
kernel formatting drift remains pre-existing and is not authored source. No current contract or
register effective rule changes; `UJ-CUTOVER=None`.

**Stage 3A closure:** `INV-CONTROL-TXN` is evidenced by generation-time opposite cleanup and epoch
capture, phase-aware delivery, reserved-delivery ownership, and the focused stale-epoch / reserved
SIGCONT / temporary-mask cases. The candidate remains non-publishable.

## Stage 3A R1 target correction - 2026-07-21

**Finding:** final review确认R0 `USER-ENTRY-002`要求reserved target不得阻塞later pending
`SIGKILL`，但current `SIGNAL-TEMP-MASK-002`与Stage 3前live code都规定reserved target优先。现有
candidate因此可能按`reserved old SIGCONT -> SIGSTOP -> pending SIGKILL -> real new SIGCONT`
顺序先提交旧SIGCONT handler frame；phase已经由真实新SIGCONT恢复Running时，handler可执行到
下一次mandatory kernel entry。该窗口原则上可以通过handler副作用观察，但不丢失SIGKILL、不
允许Stopped user entry，也不覆盖已经提交的terminal lifecycle。

**Decision and revision:** 用户明确接受这段异步递送延迟，不要求当前RFC为既有Signal优先级负责。
RFC原地升为R1：保留reservation-first顺序，删除R0额外的SIGKILL越过要求；
`docs/src/rfcs/unix-jobctl/invariants.md`加入3A docs write set。实现代码、owner、lock order、ABI、
current contract和`UJ-CUTOVER=None`均不改变，也不扩张到`task/jobctl/user_entry.rs`。

**Evidence boundary:** production app分别确定性覆盖reserved SIGCONT收口与普通SIGKILL no-return；
pending ordering由source与reservation KUnit固定。精确组合竞态没有test hook，当前证据不声称可
重复注入该单一交错。按R1复审后该finding neutralized；最终Apollyon 0、Keter 0、Euclid 0。

## Stage 3B transition preflight and activation - 2026-07-21

**Authority and boundary:** `76bd18f5`独立关闭Stage 3A后，按rolling write-set规则重新读取live
source、已提交3A diff、review与runtime证据。用户此前明确授权完成整个Stage 3，因此本次preflight
只解析3B物理write set，不扩大owner、public API、shared contract、ABI、target或acceptance；manifest
冻结后3B进入Active，Stage 4仍为Not Started，`UJ-CUTOVER=None`。并发落入工作树的R1 target
correction作为已接受基线保留，不属于3B owner扩张。

**Owner / lifecycle evidence:** membership publication由`topology/mod.rs`负责，ordinary detach与
dethread rekey由`topology/thread_group.rs`负责；clone与exec分别经`task/api/clone/mod.rs`和
`task/api/execve/kernel.rs`到达这些入口。`task/api/exit/mod.rs`拥有first terminal code、
`Exiting / Exited`与member exit；jobctl owner只在同一ThreadGroup transaction清exposure/report、
归一phase并发布parker wake。`parent_child.rs`保持topology -> child owner方向，reparent只在relation
发布后唤醒new parent Event；`process_group.rs`仍是selector而不是phase owner。ordinary wait、Signal、
architecture与scheduler不保存membership completion或terminal truth。

**Resolved manifest and validation:** authoritative逐文件清单、只读边界、stop conditions与冻结输入
见[Stage 3B Resolved Write Set Manifest](../../rfcs/unix-jobctl/implementation.md#stage-3b-resolved-write-set-manifest-2026-07-21)。3B必须用focused runtime证明runnable + syscall + ordinary-wait混合成员、task/group/process-group directed control、member exit、clone/exec/dethread、SIGKILL/exit_group和wait结果保持；reparent relation publication与new-parent wake仍需source audit。最终运行使用`build/unix-jobctl-stage3b-rv64.log`，冻结signal/wait profile不得新增回退。

## Stage 3B multi-thread exec baseline correction - 2026-07-21

**Runtime finding:** 首次RV64 focused run中，multi-member mixed execution与process-group四类stop
broadcast均通过；`multithread-exec-dethread`实际返回`Signal(9)`而不是`Exited(0)`。source与Git
history确认该行为来自既有`Task::dethread()`：它向siblings发送普通kernel `SIGKILL`，而default
action无exec-victim区分并进入整个`kernel_exit_group()`。该缺陷早于unix-jobctl，不是exposure、
phase、report或user-entry gate引入的回退。

**Authorized correction:** 用户此前允许顺手纠正相邻wrong-signal usage，并明确当前RFC不为旧问题
承担target责任。初始`dethread_owner`方案经review发现不能安全区分victim claim的
process-directed shared SIGKILL，未保留。authoritative 3B manifest最终精确增加
`anemone-kernel/src/task/sig/{mod,pending,delivery}.rs`：只有`dethread()`自产生的task-private
SIGKILL携带kernel-private victim purpose；pending owner保证该purpose不能合并覆盖ordinary
task-directed SIGKILL，reserved victim action也在per-task退出前重验later ordinary SIGKILL；任何
外部或shared SIGKILL仍走既有group terminal lifecycle。ABI、public API、accepted target、current
contract与`UJ-CUTOVER=None`均不改变。

**Validation boundary:** 修复后focused runtime必须同时证明multi-thread exec child `Exited(0)`与
multi-member external SIGKILL child `Signal(9)`，避免用dethread兼容分支削弱真实terminal dominance。

## Stage 3B closeout - 2026-07-21

**Change and proof:** production `jobctl-test`新增四个3B case，覆盖runnable userspace、ordinary
pipe wait与leader polling的混合成员，task-directed stop、ThreadGroup-directed conditional stop、
四种process-group broadcast、member exit、multi-thread exec / dethread，以及multi-member external
SIGKILL。source audit确认clone / exec重新经过统一entry gate，detach / dethread rekey只接受
Unexposed member；terminal cleanup保持first terminal code并清exposure / report / parker；reparent在
new-parent relation发布后只唤醒`child_status_changed`重扫，不重放历史SIGCHLD。没有scheduler
hold、ordinary-wait cancellation、participant ledger、ReportId或第二份lifecycle truth。

**Final review correction:** bounded adversarial review发现kernel-private dethread victim SIGKILL若与
ordinary task-directed SIGKILL共用unreliable slot，可能错误覆盖真实terminal action。3B manifest
因此精确加入`task/sig/pending.rs`：ordinary SIGKILL在coalescing时永远支配victim purpose；若victim
已经被temporary-mask路径reserved，delivery仍在per-task退出前重验later ordinary SIGKILL。该修正
不改变ABI、public API、jobctl target、current contract或R1 reservation-first边界。

**Validation:** `build/unix-jobctl-stage3b-rv64.log`记录184项KUnit全部通过、17个focused
`jobctl-test` case全部通过，其中multi-thread exec child退出0、multi-member external SIGKILL按9
终止；冻结signal/wait profile保持
`attempted=112 passed=98 failed=10 infra_failed=0 skipped=4`并正常关机。`just fmt jobctl-test
--check`与`git diff --check`通过；kernel format check仅报告未手工维护的generated
`kconfig_defs.rs` / `platform_defs.rs`既有whitespace。最终pending coalescing guard经source / format
review闭合；后续沙箱内build仍被lwext4 C子进程`Bad system call`阻止，用户明确确认构建可通过并
终止重复构建尝试。LA64不运行runtime。

**Review and result:** 最终复核为Apollyon 0、Keter 0、Euclid 0。Stage 3B checkpoint关闭；
candidate继续non-publishable，`UJ-CUTOVER=None`，current contract与register effective rule不变。
Stage 4保持Not Started，不因本checkpoint自动激活。

## Stage 4 transition preflight and Stage 4A repair activation - 2026-07-21

**Authority and checkpoint shape:** 用户明确授权完成Stage 4；后续澄清Stage 3才要求每个
checkpoint独立commit，本Stage只在4A repair与4B validation都关闭后形成一个`jobctl:`提交。
Stage 3B已由`5e947e03`独立关闭；本次rolling preflight重新读取R1 target、current
contracts、register、完整candidate diff、Stage 3A/3B review与RV64 evidence。Stage 4原validation
尚未开始，preflight先发现target-preserving candidate缺陷，因此按implementation停止合同将首个
checkpoint解析为Stage 4A owner-local repair；Stage 4B仍为Not Started。

**Finding:** `kernel_exit_group()`可以在其它live member仍处于userspace exposure时提交
`Alive -> Exiting`。现有terminal helper只归一jobctl phase并清report，未在同一ThreadGroup owner
transaction清除全组exposure；lifecycle gate仍阻止terminal后的user entry，但该状态不满足
`JOBCTL-STATE-001`、`JOBCTL-LIFE-001`与`INV-LIFECYCLE`的closure evidence。另有现成
diagnostic projection没有caller，无法实际提供长期`Stopping`的phase age与remaining exposed
progress。两项均能在既有owner内修复，不改变R1、ABI、visible semantics、public API、lock方向、
current contract或`UJ-CUTOVER=None`。

**Resolved manifest and validation:** 唯一权威逐文件清单、只读边界、停止条件与验证floor见
[Stage 4A Repair Manifest](../../rfcs/unix-jobctl/implementation.md#stage-4a-repair-manifest2026-07-21)。
本checkpoint只允许ThreadGroup membership/jobctl owner、terminal入口与transaction/plan write-back；
Signal、wait/report、procfs、topology、scheduler/wait-core、apps、LTP profile/groups、register与
current contract保持只读。Stage 4B manifest只能在4A独立review、验证和记录关闭后解析，不从
本次activation继承写授权；这两个内部gate不要求拆成两个Git commit。

**Pre-implementation review completion:** 在任何repair code写入前，独立review指出
`JobControlDiagnostic`无caller只是更大observability缺口的一部分；report consume也没有
边界诊断。authoritative manifest因此在冻结完成时加入`task/jobctl/report.rs`，只允许低频日志，
不改变report truth、Event、SIGCHLD、wait claim或ABI。Signal generation的现有receive日志与
group phase日志足以关联control occurrence，本checkpoint不扩大到Signal；procfs跨owner采样窗口
按R1只承诺character/name同一local enum与采样到Zombie时Z优先的窄文字归为Safe解释风险，不修改
procfs，也不把它写成broken target。

## Stage 4A repair closure - 2026-07-21

**Implementation result:** terminal helper现在在发布`Exiting` / `Exited`的同一个
`ThreadGroup.inner.write()` owner transaction中清除全部live user exposure、清report并归一phase；
后续`Exiting` trap只验证预清状态，`Alive` trap继续严格要求`Exposed -> Unexposed`。已有diagnostic
projection用于有界的Stopping progress、phase age、terminal与park / re-arbitration日志；report只在
exact-once consume和guards-out publication边界记录，不让诊断字段驱动behavior，也未引入task-local
flag、second truth、public API或shared contract变化。

**Validation evidence:** `build/unix-jobctl-stage4a-rv64.log`来自repair后的RV64 wrapper candidate build。
185个enabled KUnit全部通过，其中新增terminal exposure owner-local case通过；17个既有
`jobctl-test` case全部通过。glibc与musl各为`attempted=56 passed=49 failed=5 skipped=2`，合计
`112/98/10/4`、`infra_failed=0`，已知五个signal failure分类不变，系统正常关机。
`git diff --check`通过；`just fmt kernel --check`只报告两个构建生成文件的既有尾随空白。沙箱内独立
`just build`被lwext4 C子进程的`Bad system call`限制阻断，同一wrapper中的kernel build已成功，
因此记录为环境限制而非candidate failure。

**Review and result:** 独立review最初发现`WNOWAIT`可无限peek，同一report的日志不满足低频边界；
author随后删除peek日志，只保留exact-once consume。复核结论为Apollyon 0、Keter 0、Euclid 0。
Stage 4A关闭；candidate继续non-publishable，`UJ-CUTOVER=None`，current contract和register不变。
Stage 4B保持Not Started，必须由post-repair preflight解析独立manifest，且最终与4A形成单个Stage 4
`jobctl:`提交。

## Stage 4B validation activation - 2026-07-21

**Transition preflight:** Stage 4A closure后重新读取live candidate、R1 target、proof obligations、
Contract Impact和全部producer / user-entry / wait-report / procfs / lifecycle owner路径；repair没有引入
新的旁路、second truth或target分叉，candidate kernel因此恢复只读。Stage 4B validation manifest、
custom case、LTP预期、危险或不可确定注入边界、write set与stop condition已冻结在
[Stage 4B Validation Manifest](../../rfcs/unix-jobctl/implementation.md#stage-4b-validation-manifest2026-07-21)。

**Authority:** 用户授权完成整个Stage 4并最终形成一个`jobctl:`提交；4A / 4B仍分别保留review与
evidence gate，但不再要求两个Git commit。本checkpoint只写focused app、`tkill`测试接线、四个
明确wait LTP enablement和plan / transaction write-back。current contract、register、Stage 5和
`UJ-CUTOVER`均保持未授权、未生效。

**Validation feedback:** 首次4B wrapper在focused procfs case中按新增oracle尝试在SIGKILL后、wait
reap前读取`/proc/<pid>`观察Zombie Z，但terminal detach先触发R1明确保留的binding / leader
resolution失败边界。停止该次QEMU后确认这不是target defect：`proc_state()`源码已在jobctl T判断前
优先处理采样到的`TaskStatus::Zombie`，而userspace没有稳定可解析Zombie窗口。manifest因此保持
candidate kernel只读，只撤回该过强runtime oracle并把Z precedence归入source-audit evidence；
Stopped T / status pair / continue projection仍由production case验证，完整validation floor从头重跑。

第二次wrapper中安全的`kill` / `tkill` / `tgkill` / `rt_sigqueueinfo` SIGSTOP matrix均未停止init；
随后conditional stop ordinary occurrence按Signal语义打断了PID 1正在执行的wait，init应用因把
`EINTR`当fatal而panic。该signal没有取得jobctl stop authority，但黑盒本身破坏生产harness，不能
作为immunity oracle。Stage 4B再次保持kernel只读，移除conditional-init发送；其generation后在
global-init admission处拒绝conditional stop authority的证明改由producer/source audit闭合，与会
同时命中harness成员的process-group init发送一并记录为不安全黑盒边界。完整floor再次从头重跑。

第三次wrapper显示即使单CPU，target仍可能在两个userspace signal syscall之间被调度并完成
Stopped，因而无法稳定黑盒注入`Stopping x SIGCONT`或读取Stopping procfs projection；这不是
candidate state-machine failure，而是userspace没有“Stopping已建立且exposure尚未关闭”的确认。
manifest按反馈扩展`task/jobctl/group.rs`的test-only write set，以最小owner-local KUnit确定性证明
Stopping取消无Stopped / Continued report；production case只验证committed stop后重复SIGCONT恰好
一次Continued。procfs Stopping投影由`is_job_control_stopped()`只匹配committed Stopped的source
evidence闭合，不增加kernel hook、scheduler state或时序oracle。完整floor再次从头重跑。

## Stage 4B validation closure and Stage 4 completion - 2026-07-21

**Final candidate evidence:** `build/unix-jobctl-stage4-rv64.log`由最终repair后source、19个focused
case与启用四个wait LTP case的rootfs重新构建。wrapper内`just build`成功；186个enabled KUnit
全部通过，包括terminal exposure clear与`Stopping x SIGCONT`无report两个owner-local case；19个
`jobctl-test` case全部通过。glibc与musl signal均为`37/30/5/2`、wait均为`23/23/0/0`，每个libc
合计`60/53/5/2`，全profile为`120/106/10/4`、`infra_failed=0`，已知五个非target signal failure
分类不变，系统正常关机。`just fmt kernel --check`、`just fmt jobctl-test --check`、
`git diff --check`与`mdbook build docs`通过；LA64只完成source closure，未运行runtime。

**Proof and Contract Impact evidence index:**

| ID / obligation | Source authority | Runtime / local proof |
| --- | --- | --- |
| `SIGNAL-PENDING-001/002`、`SIGNAL-ACTION-001/002`、`JOBCTL-SIGNAL-001`、`INV-CONTROL-TXN` | `task/sig/{generation,pending,delivery}.rs`与kill / tkill / tgkill / rt_sigqueueinfo producer | task-directed四stop matrix、private/shared opposite cleanup、global-init安全SIGSTOP producer、conditional disposition / flag、temporary-mask与stale-epoch KUnit |
| `SIGNAL-TEMP-MASK-001..003` | `task/sig/{mask,pending,delivery}.rs`与`rt_sigreturn` | temporary default-stop / SIGCONT custom+default、SA_NODEFER、SA_RESETHAND、frame failure、SIGKILL dominance；完整reserved复合时序按无ack边界使用split evidence |
| `PROCFS-TASK-STATE-001` | `fs/proc/tgid/{stat,status}.rs`同一derived enum，源码先判Zombie、只对committed Stopped返回T | focused T / `T (stopped)` pair与continue后非T；Zombie binding与Stopping注入不建立不稳定oracle |
| `PGRP-SIGNAL-001/002` | `task/topology/process_group.rs`只snapshot selector，每个TG独立进入generation owner | 两个TG、四种stop signal的process-group broadcast；global-init group发送因会命中harness改用source evidence |
| `TASK-LIFE-001..003`、`JOBCTL-LIFE-001`、`INV-LIFECYCLE` | topology construct/join/detach/dethread、`task/api/exit` terminal owner与Stage 4A preclear | terminal exposure KUnit、multithread exec/dethread、multi-member SIGKILL、frame failure与wrapper正常teardown |
| `CHILD-WAIT-001..005`、`JOBCTL-REPORT-001`、`INV-REPORT-CLAIM` | `task/api/wait` selector/current-slot claim、`jobctl/report.rs`、wait4/waitid serializers | WNOWAIT双peek+consume、`si_uid=0`、SA_NOCLDSTOP / ignored SIGCHLD report survival、repeat-continue、waitid07/08与waitpid08/13 |
| `USER-ENTRY-001/002`、`JOBCTL-STOP-001`、`INV-ENTRY-CLOSURE` | RV64/LA64 ordinary/raw/fresh entry统一`arbitrate_user_entry()`；clone/exec汇入同一gate | multi-member runnable+ordinary pipe wait、source未满足时SIGCONT不完成wait、source完成后Stopped gate、exec new image |
| `JOBCTL-STATE-001`、`JOBCTL-CONT-001`、`INV-OBSERVABILITY`、`INV-VALIDATION` | `ThreadGroup.inner`唯一phase/exposure/report owner；diagnostic snapshot不参与behavior | cancellation KUnit无report、committed stop后exact-once Continued、全部focused/LTP floor和有界phase/progress/terminal/park/consume日志 |

**Bypass audit:** Signal ingress、pending / reservation / temporary mask、两架构user-entry、membership /
lifecycle、wait/report与procfs路径均重新搜索；所有producer进入既有exact target / ProcessGroup selector ->
ThreadGroup generation owner，所有user transition进入mandatory arbitration。scheduler与generic wait-core
不存在新增jobctl enum、stop flag、force-wake或admission分支；无feature flag、group-size branch、
old/new fallback、temporary carrier / queue / manager。current `profile.txt`仍只启用`signal` / `wait`，
`signal.txt`、current contracts与register均无diff。

**Accepted evidence boundaries:** Zombie Z只有在binding成功且采样到Zombie时适用，R1不改变binding
失败；conditional-init ordinary occurrence可合法打断PID 1 wait，process-group init发送会命中harness，
两者不做危险黑盒；userspace无法确认Stopping或reserved-delivery reservation建立，故分别使用
owner-local KUnit或split production + source evidence。guards-out SIGCHLD publication ordering继续由
`ANE-20260721-JOBCTL-SIGCHLD-PUBLICATION-ORDER`承载；numeric TID / PGID reuse和final gate到hardware
return窄窗口维持既有accepted boundary。以上均不是broken target behavior。

**Closure-review oracle correction and docs-only expansion:** Stage 4B独立review发现
`repeat-continue`在消费首次Continued前连续发送两次`SIGCONT`，错误的重复report / SIGCHLD可能被
coalescing掩盖。validation asset现改为先观察首次`CLD_CONTINUED`并消费Continued report，再发送
第二次`SIGCONT`，分别断言无新report且SIGCHLD计数不变。该修复仍在既有Stage 4B app write set内，
不修改candidate production code、target、ABI、visible semantics或acceptance。为同步canonical
closure状态，docs-only write set扩展为RFC `index.md`、`docs/src/rfcs.md`、当前双周devlog、
implementation plan与本transaction；只记录Stage 4 closed / Stage 5 Not Started、review结果和重跑
证据，current contracts、register、R1与`UJ-CUTOVER=None`保持不变。完整RV64 floor与独立review必须
在最终收口前重跑。

**Prepared Stage 5 atomic cutover manifest:** 本Stage不生成或应用effective diff；Stage 5 preflight必须
把以下候选集合重新解析为逐文件resolved manifest：RFC `index/invariants/implementation`状态与导航；
Signal `pending-routing` / `temporary-mask-delivery`、procfs `task-state-projection`、task
`process-group-signaling` / `thread-group-lifecycle` / `child-wait` / `user-entry` current contracts及owner
indexes；新增稳定job-control owner contract与contracts导航；本transaction、transaction index、当前
双周devlog、RFC/SUMMARY导航；register中
`ANE-20260527-PROCESS-GROUP-SESSION-STAGE1`只移除已闭合的stopped / continued wait缺口，同时保留TTY、
foreground/background pgrp、orphaned-pgrp、ptrace与`si_uid=0`后续边界，并保留
`ANE-20260721-JOBCTL-SIGCHLD-PUBLICATION-ORDER`。这些文件目前仍以旧current authority为真，不能把
本manifest解释为partial cutover。

**Review and result:** Stage 4A独立review为Apollyon 0、Keter 0、Euclid 0。Stage 4B首次closure review
只发现一个Euclid：`repeat-continue`在消费首次Continued前发送第二次`SIGCONT`，可能让错误重复发布被
coalescing掩盖；修复后已按上述oracle顺序重跑完整RV64 floor，独立复核确认该finding neutralize，
最终Apollyon / Keter / Euclid均为0。没有target defect、unclassified failure、owner / ABI / kernel
public API / shared contract扩张或stop-condition触发；`anemone-rs`只增加现有`tkill` syscall的窄调用
wrapper。Stage 4B与整个Stage 4关闭；candidate保持non-publishable，`UJ-CUTOVER=None`，current
contracts与register不变，Stage 5 Not Started，且整个Stage 4只形成一个最终`jobctl:`提交。

## Stage 5 transition preflight and activation - 2026-07-21

**Authority and preconditions:** 用户本轮明确授权完成Stage 5 / `UJ-CUTOVER`，并因本轮不改代码且
Stage 4已经完成最终runtime而免除重复Stage 5 wrapper。Stage 0至Stage 4均已关闭；HEAD
`9d0308a7`就是最终Stage 4 candidate commit，工作树开始本Stage时clean，Stage 4 evidence log仍对应
该candidate。preflight复核R1 Contract Impact、proof obligations、Stage 4逐ID evidence index、current
contracts、register和public navigation，没有发现old/new双路径、feature flag、singleton fallback、
temporary bridge、probe hook、target defect或未分类失败。

**Resolved manifest and activation:** authoritative逐文件清单冻结在
[Stage 5 Resolved Write Set Manifest](../../rfcs/unix-jobctl/implementation.md#stage-5-resolved-write-set-manifest2026-07-21)，
共21个公共文档文件。新增`contracts/task/job-control.md`承载`ThreadGroup`唯一owner下的全部
`JOBCTL-*` stable IDs；代码、测试资产、private draft、backgrounds与open issues保持只读。Stage 5
由此进入Active，`UJ-CUTOVER`在全部文档、review与validation退出条件通过前仍为None。

**Process correction:** manifest冻结前曾提前形成Signal/procfs/task current-contract未提交草稿。
独立流程审计将其定级Keter：rolling write-set workflow不允许用事后manifest追认执行。发现后立即
停止继续编辑contract；这些文本只按未授权draft处理，不代表effective change或partial cutover。
本preflight先独立冻结manifest并记录activation，后续再把manifest内draft作为candidate diff从头
review、修正和验证。最终cutover commit之前，HEAD中的旧current contract仍是唯一effective authority。

**Review / validation floor:** contract review逐项核对全部`Introduce / Refine / Replace / Scoped
Exception`落点、R1 reservation-first边界、terminal precedence、report/wait/procfs单一truth、
`si_uid = 0`与ptrace/TTY/orphaned follow-up；最终要求Apollyon/Keter/Euclid为0。执行
`git diff --check`、stale wording/link/anchor audit、`mdbook build docs`、
`just fmt kernel --check`、`just fmt jobctl-test --check`和`just build`。按用户明确豁免不重复QEMU /
LTP runtime；Stage 4日志与candidate hash/provenance必须继续一致。任何代码或测试资产变化、
owner/target/ABI/visible semantics/acceptance变化立即停止并保持Not Cut Over。

## Stage 5 `UJ-CUTOVER`与事务收口 - 2026-07-21

**Atomic cutover:** Stage 5按[authoritative resolved manifest](../../rfcs/unix-jobctl/implementation.md#stage-5-resolved-write-set-manifest2026-07-21)
只修改21个公共文档文件；kernel、apps、LTP profile/group、rootfs/harness、private draft、backgrounds与
open issues均保持只读。cutover commit是承载本条及全部contract diff的
`jobctl: complete Stage 5 atomic cutover`提交；其直接parent `9d0308a7`是最终Stage 4 candidate。
Git中的该单一提交同时发布current contract、RFC Closed、transaction Completed、register与导航，
不存在只能看到core或wait/procfs的partial effective状态。

**Contract result and evidence:** 下表的“旧规则”是R1 Contract Impact记录的切换前baseline；“Effective
规则”由对应current contract唯一拥有。runtime/source证据均来自最终candidate `9d0308a7`的Stage 4
proof index与`build/unix-jobctl-stage4-rv64.log`。

| Contract ID | 变化 | 旧规则 | Effective规则 / 落点 | Evidence |
| --- | --- | --- | --- | --- |
| `SIGNAL-PENDING-001` | Scoped Exception | private/shared pending拥有全部ordinary occurrence | ordinary owner不变；`SIGSTOP`直接作为jobctl control input消费 | pending/generation source；四stop matrix与opposite cleanup |
| `SIGNAL-PENDING-002` | Refine | group occurrence先publish pending；`SIGSTOP` force notification | ordinary publication不变；`SIGSTOP`不进pending、不完成active wait | producer/source与ordinary pipe-wait runtime |
| `SIGNAL-ACTION-001` | Preserve | ignored admission在pending publication前生效 | 保持；control generation side effect与ordinary occurrence admission分离 | generation/action source与conditional disposition cases |
| `SIGNAL-ACTION-002` | Refine | ordinary trap-return提交异步action | `SIGSTOP` generation direct stop；conditional DefaultStop使用epoch authority；Stopped-phase closure受限 | action/delivery source、stale-epoch与temporary-mask cases |
| `SIGNAL-TEMP-MASK-001` | Preserve | task唯一拥有mask/restore slot | 保持；jobctl不复制restore truth | mask owner source与temporary-mask regression |
| `SIGNAL-TEMP-MASK-002` | Refine | reserved target优先later pending但尚未提交action | claim finality保留；control cleanup不撤销reservation；R1 reservation-first顺序有效 | reservation source、SIGCONT custom/default与SIGKILL dominance |
| `SIGNAL-TEMP-MASK-003` | Preserve | handler/no-frame cleanup收口restore responsibility | 保持并明确no-return terminal前仍由Signal owner清理 | frame/no-frame/terminal source与frame-failure runtime |
| `PROCFS-TASK-STATE-001` | Refine | leader TaskStatus映射R/Z/S/D；status pair非原子 | read-local enum，Zombie优先，committed Stopped为T，单次status pair一致 | procfs source与focused T/status pair runtime |
| `PGRP-SIGNAL-001/002` | Preserve | ProcessGroup只选member；每个TG独立接受 | 保持；每个TG独立stop/continue/report，无pgrp-wide phase | selector source与双TG四stop broadcast |
| `TASK-LIFE-001..003` | Preserve | terminal owner、last detach与notification ordering | terminal owner不变；jobctl只承担exposure/report/parker cleanup | exit/topology source、terminal KUnit与SIGKILL/exec runtime |
| `CHILD-WAIT-001..005` | Replace / Refine | exit-only truth与claim；non-exit ABI缺失 | typed terminal/report selection，relation+selector重验，predicate-only Event，WNOWAIT peek/exact-once consume，stopped/continued ABI | wait/report source、WNOWAIT、waitid07/08、waitpid08/13 |
| `USER-ENTRY-001` | Refine | ordinary return只做Signal arbitration | ordinary path加入lifecycle/jobctl gate与exposure登记 | RV64/LA64 source与multi-member/ordinary-wait runtime |
| `USER-ENTRY-002` | Introduce | None | fresh/clone/exec与ordinary return共享mandatory gate；R1 reservation-first边界有效 | entry source、exec new image与temporary-mask/SIGKILL cases |
| `JOBCTL-STATE/STOP/SIGNAL/CONT/LIFE/REPORT-001` | Introduce | None | [ThreadGroup-owned Unix job control当前契约](../../contracts/task/job-control.md)全部Active | Stage 4逐ID proof index、186 KUnit、19 focused cases与wait LTP |

**Register result:** `ANE-20260527-PROCESS-GROUP-SESSION-STAGE1`已移除stopped/continued wait缺口，
继续保留TTY、foreground/background pgrp、terminal-generated signal与orphaned-pgrp policy。
`ANE-20260721-JOBCTL-PTRACE-DEFERRED`、`ANE-20260721-JOBCTL-SI-UID-ZERO`与
`ANE-20260721-JOBCTL-SIGCHLD-PUBLICATION-ORDER`均保持Active；没有broken expected behavior需要新增
open issue。

**Review:** 两路独立review核对current-contract owner/ID、cross-domain handoff、reparent no-lost-wake、
temporary-mask terminal cleanup、R1 reservation-first可见延迟、procfs read-local snapshot与全部public
anchor。manifest冻结前提前形成未提交contract草稿的流程Keter已通过先冻结manifest、再从头review
candidate diff而neutralize；其余finding逐项修正后最终Apollyon 0、Keter 0、Euclid 0。没有owner、
public API、ABI、visible semantics、acceptance或write-set扩张，也没有触发repair/target stop condition。

**Validation:** `git diff --check`、`mdbook build docs`、stale wording/link/anchor audit、
`just fmt kernel --check`与`just fmt jobctl-test --check`通过。用户明确批准后，final `just build`在沙箱外
通过，release kernel成功编译并导出；tracked code/test资产仍与`9d0308a7`一致。首次沙箱内build只因
lwext4 C compiler被seccomp以`Bad system call`阻断，不是源码失败。按用户明确豁免，本Stage没有重复
QEMU/LTP runtime；Stage 5 runtime记录为user-waived / Not Run，复用Stage 4最终candidate的完整日志与
artifact provenance，不把它写成新运行证据。

**Result:** 全部target contract ID为Effective，无Transitional、pending或Not Cut Over残留。
RFC R1 Closed，transaction Completed，`UJ-CUTOVER`原子生效。下一步只有TTY、orphaned-pgrp、ptrace、
credential `si_uid`与SIGCHLD ordering等独立follow-up；本事务不进入或授权其中任何一项。
