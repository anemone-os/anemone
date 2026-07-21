# Unix Job Control 迁移实施计划

**状态：** Completed / R1
**最后更新：** 2026-07-21
**父 RFC：** [RFC-20260720-unix-jobctl](./index.md)
**目标与不变量：** [Unix Job Control 目标与不变量](./invariants.md)
**当前修订：** R1
**事务日志：** [2026-07-20-unix-jobctl](../../devlog/transactions/2026-07-20-unix-jobctl.md)；Stage 0至Stage 5全部关闭。
**Contract Cutover：** `UJ-CUTOVER`已在Stage 5原子生效。

当前 effective contract：

- [Signal pending routing](../../contracts/signal/pending-routing.md)
- [Signal temporary-mask delivery handoff](../../contracts/signal/temporary-mask-delivery.md)
- [Procfs TGID task-state projection](../../contracts/procfs/task-state-projection.md)
- [Process-group signal targeting](../../contracts/task/process-group-signaling.md)
- [ThreadGroup lifecycle](../../contracts/task/thread-group-lifecycle.md)
- [Unix job control](../../contracts/task/job-control.md)
- [Child wait](../../contracts/task/child-wait.md)
- [User entry](../../contracts/task/user-entry.md)

## 1. 计划角色与当前边界

本文是 RFC target 到实现 checkpoint 的协调契约，负责阶段顺序、write set、验证 floor、停止条件和反馈回写，不重新定义 target invariant。发生冲突时：

1. current contract 继续描述已经生效的规则；
2. `index.md` 与 `invariants.md` 描述尚未生效的 accepted / proposed target；
3. 本文只决定如何验证并迁移到该 target；
4. transaction 在实现开始后记录执行事实、review、证据、修正和 cutover 结果。

R1在当前事务中接受时，接受与transaction创建本身不表示：

- current contract 已完成cutover；
- 任何代码、测试配置或 runtime 语义已经获准修改；
- 任一`JOBCTL-*`或`USER-ENTRY-002`已经生效。这些边界后来只由Stage 5 `UJ-CUTOVER`关闭。

Stage 0仅是行为保持型Signal module split checkpoint；Stage 1至Stage 4完成candidate实现与验证，
Stage 5冻结docs-only resolved manifest并将全部target delta作为`UJ-CUTOVER`原子切换。所有Stage
已经关闭，后续TTY、orphaned-pgrp或ptrace工作不属于本计划的自动下一gate。

### 1.1 废弃来源隔离

本计划只以当前工作树中的 `unix-jobctl` target、current contract、register、live code、
Linux 6.6.32 与当前测试源码为依据。`dev/drc/sigstop` 分支及该分支上的
`signal-group-stop` 已经停止并被放弃，不得作为设计输入、实现模板、write set、阶段
顺序、验证证据或 patch 来源，也不得从中复制或 cherry-pick 内容。若当前工作树与其
存在相似代码，只能按当前代码和当前文档独立审计，不能以废弃分支历史为正确性依据。

## 2. 迁移原则

- 以 ThreadGroup 为 phase、first stop reason、control ordering、live exposure 与 parent report 的唯一 owner。Signal 只拥有 occurrence、pending、mask、disposition 与 action selection；scheduler 只提供既有 wait / wake / placement 能力。
- stop 是 mandatory user-entry barrier，不是 scheduler hold。不得新增 scheduler-owned stop state、runqueue suppression、generic wait admission、typed unwind 或 ordinary-wait cancellation。
- ordinary wait 的 predicate、timeout、source registration 与真实 result 全程由原 owner 保持。stop / resume side effect 不得完成 active wait、制造 `EINTR` 或消费 `WakeToken`。
- module split 先于 semantic growth。split-only checkpoint 只能移动同一 Signal owner 内的现有职责、收窄 visibility、保持调用面和行为不变。
- Signal generation 与 delivery 是两条不同边界：generation 负责已验证 occurrence 的 admission 与 generation-time control ordering；delivery 负责 source selection、live action selection、handler frame 与 no-frame cleanup。
- jobctl 不放入 `task::sig`。ThreadGroup-owned phase / exposure / report 与 mandatory gate 使用独立 `task::jobctl` 目录模块，并只向 Signal、wait、lifecycle、procfs 暴露窄 API 或 immutable snapshot。
- 第一条 production vertical slice 必须使用最终通用 ThreadGroup 路径。单线程只是验证输入，不得形成 singleton 特殊实现、多线程 fallback、feature flag 或 old/new runtime 分流。
- 从第一条可达 jobctl semantic path 开始，候选 train 在 `UJ-CUTOVER` 前不可独立发布。中间 Stage 可以构建、review 和运行 candidate，但不得更新 current contract 或让其它代码依赖 transitional behavior。
- implementation feedback 可以改变阶段顺序、物理字段、私有 helper、container 与 owner-local lock 形状，但不能静默改变 owner、target invariant、ABI、accepted limitation 或 `UJ-CUTOVER` 原子边界。
- 每个 Stage 开始前把默认 write set 解析为逐文件的 `Resolved Write Set Manifest`。若自然实现需要扩大范围，先停止并报告原因、拟新增文件、受影响 contract、验证与回写位置；批准后再继续。
- 构建验证统一通过仓库入口 `just build`；运行时验证统一通过
  `./scripts/run-user-test-rv64.sh etc/sdcard-rv.img <log-path>`。计划与执行者不得直接调用
  `just xtask qemu`；QEMU、rootfs、kernel build 与日志收集由 RV64 端到端 wrapper 统一编排。
- 定向 LTP 输入冻结为 `anemone-apps/user-test/ltp/profile.txt` 中仅启用 `signal` 与
  `wait`。每个 runtime Stage 开始前都要核对这一输入；扩大、缩小或临时替换 profile
  必须先停止并记录批准、预期结果与恢复方式，不能把测试集合漂移当作阶段进展。
- 不为反馈默认创建 `feedback.md`、`probe.md` 或 `experiments.md`。计划留在本文，执行事实写 transaction；只有长证据包才进入具体命名的 `backgrounds/` 文件。

## 3. 物理模块边界

### 3.1 Signal split-only 目标形状

当前 `task/sig/mod.rs` 同时承载 mask、pending source、generation、delivery、temporary-mask handoff 与 trap-return frame commit。Stage 0 将其按下列稳定职责拆分：

```text
task/sig/
  mod.rs
  mask.rs
  pending.rs
  generation.rs
  delivery.rs
  disposition.rs
  info.rs
  set.rs
  altstack.rs
  hal.rs
  api/
```

| 模块 | 拥有的职责 | 明确不拥有 |
| --- | --- | --- |
| `mod.rs` | module docs、声明、窄 re-export、共享领域名词 `SigNo` / `Signal` | `Task` / `ThreadGroup` 行为、pending policy、trap-return loop |
| `mask.rs` | `TaskSigMaskState`、temporary-mask token、current / restore slot、mask mutation | occurrence claim、disposition、jobctl phase |
| `pending.rs` | `PendingSignals` 与 private/shared source 的 queue、fetch、flush、reserved storage primitive | sender validation、notification、action selection、jobctl transition |
| `generation.rs` | 已通过 sender-specific target / permission validation后的 occurrence admission；task/group receive；generation-time control ordering | syscall ABI parsing、handler frame、wait result |
| `delivery.rs` | private/shared source selection、temporary-mask classifier、live action selection、handler frame / no-frame cleanup、ordinary user-entry Signal arbitration | target permission、ThreadGroup phase truth、wait claim |

Stage 0 不新建 `types.rs`、`model.rs` 或 `occurrence.rs`。`SigNo` 与 `Signal` 是整个 Signal 领域共用的根名词，留在 module root；以后只有在它们形成独立 owner 或显著职责时才重新评估。

temporary-mask classifier 留在 delivery：它会读取 pending、mask 与 disposition，并建立具体 occurrence handoff；把它放入 `mask.rs` 会让 mask owner 反向拥有 delivery policy。`PendingSignals` 只提供 source primitive；跨 private/shared 的选择与 action ordering 不下沉到容器。

Stage 0 保持现有 `Task::recv_signal()`、`ThreadGroup::recv_signal()`、mask API、pending snapshot、`handle_signals()` 与 crate/public re-export 形状。命名调整、semantic return type、control authority 和 jobctl ingress 统一留给后续 Stage。

### 3.2 Jobctl 目标形状

jobctl 使用独立目录模块：

```text
task/jobctl/
  mod.rs
  group.rs
  user_entry.rs
  report.rs
```

- `mod.rs` 保存领域类型、共享 facade 与子模块声明；不成为所有逻辑重新聚集的 catch-all。
- `group.rs` 保存 ThreadGroup owner-local phase、exposure、continue ordering、stop / continue / lifecycle transition。
- `user_entry.rs` 保存 user-to-kernel exposure clear、before-user-entry gate、park 与 wake 后重新仲裁。
- `report.rs` 保存 child-attached report snapshot / claim、parent predicate notification 与 guards-out effect。

文件按 Stage 实际创建，不预建空模块。`ThreadGroupInner` 中的 membership record、job-control storage 与既有 lifecycle 保持同一 owner transaction；不会为了目录整洁把 state 搬到另一个锁或建立 manager object。

推荐依赖方向：

```text
signal api / kernel producer
          -> sig::generation
               -> sig::pending leaf
               -> task::jobctl narrow owner API

architecture user entry
          -> sig::delivery arbitration
          -> task::jobctl user-entry gate

task::wait / lifecycle / procfs
          -> task::jobctl report or derived snapshot API
```

禁止 `task::jobctl` 读取 pending queue、mask、disposition或architecture trapframe内部表示；禁止 Signal、wait、procfs、Event 或 scheduler 直接写 phase / exposure / report。

## 4. Stage 总览与发布边界

| Stage | 目标 | 当前状态 | Contract Cutover | 发布边界 |
| --- | --- | --- | --- | --- |
| Stage 0 | Signal module split-only | Closed | None | 行为保持型 checkpoint |
| Stage 1 | ThreadGroup owner 与 mandatory-entry dormant foundation | Closed | None | target readiness；无 stop ingress |
| Stage 2 | 单线程 integrated production vertical slice | Closed | None | non-publishable candidate train 开始 |
| Stage 3A | conditional control signal、reservation 与 temporary-mask closure | Closed | None | non-publishable candidate checkpoint |
| Stage 3B | 多成员 exposure、lifecycle 与 topology closure | Closed | None | non-publishable candidate checkpoint |
| Stage 4A | terminal exposure / observability owner-local repair | Closed | None | repair checkpoint；candidate仍不可发布 |
| Stage 4B | ABI、竞态、旁路与 production validation closure | Closed | None | verified candidate；仍不可发布 |
| Stage 5 | current contract 与完整实现原子生效 | Closed | `UJ-CUTOVER` Effective | integrated publishable unit |

Stage 只能按表中顺序进入；Stage 3A 与 Stage 3B 各自冻结 manifest、执行 review、记录
checkpoint 并满足退出条件，3A 未关闭时不得进入 3B。某个 Stage / checkpoint 的 build、
局部测试或单线程成功不能授权跳过下一项，也不能提前宣称 effective。Stage 2 之后任何
发现若使 candidate 无法继续到 `UJ-CUTOVER`，整条 candidate train 保持 Not Cut Over。

## 5. 实施入口（不计入 Stage）

target review 与公开 RFC promotion 属于 implementation stage 之前的文档工作，不进入
Stage 编号。只有满足以下条件后，才能冻结 Stage 0 manifest 并请求实现授权：

- target review 已完成，且用户明确授权提升；
- RFC 已提升到 `docs/src/rfcs/unix-jobctl/`，R0 状态为 `Accepted for Implementation`；
- transaction 已创建，并记录 canonical revision、受影响 contract IDs、`UJ-CUTOVER`
  和 Stage 0 初始状态；
- RFC / SUMMARY / transaction / devlog 导航已同步，current contract 只增加获准的
  pending-successor 导航，没有提前修改 effective rule；
- `git diff --check`、`mdbook build docs` 与 public link / anchor audit 通过；
- Stage 0 的逐文件 `Resolved Write Set Manifest`、执行者、reviewer 与验证命令已冻结。

提升只改变文档 authority，transaction 创建只建立执行记录入口；二者都不是
implementation Stage，也不触发 contract cutover。条件完成后仍须等待用户明确授权
Stage 0，不得因提升完成自动进入代码实现。

## 6. Stage 0：Signal module-boundary split-only checkpoint

### 前置条件

- 第 5 节实施入口条件已经完成，并明确授权 Stage 0。
- transaction 已冻结 Stage 0 manifest。
- 当前 `task::sig` public/crate API、symbol usage、lock order 与 direct field access inventory 已记录。

### 交付

- 按第 3.1 节创建 `mask.rs`、`pending.rs`、`generation.rs` 与 `delivery.rs`，将现有职责机械移动到对应 owner-local 模块。
- `mod.rs` 只保留 module docs、declaration、narrow re-export、`SigNo` 与 `Signal`。
- 收窄只被 sibling Signal modules 使用的 helper；外部调用面保持不变。
- 不新增 jobctl state、continue epoch、control cleanup、stop/resume side effect、report 或 user-entry gate。

### 审计

- 对比移动前后的 `Task` / `ThreadGroup` inherent method、pub/crate symbol、pending fetch order、temporary-mask cleanup 与 trap-return action loop。
- 搜索 `sig_pending`、`sig_mask`、`sig_disposition`、`reserved_delivery`、`handle_signals`、`recv_signal`、`fetch_signal`、`fetch_specific_signal` 的全部调用者。
- 确认 pending leaf 不读取 disposition / jobctl，mask leaf 不 claim occurrence，generation 不构造 handler frame，delivery 不做 sender permission。

### 反馈假设

现有 Signal 职责可以在不移动 owner surface、改变 lock order 或扩大 public API 的情况下按稳定角色拆分。

出现以下信号立即停止 Stage 0：

- 机械移动要求改变 pending ownership、reservation semantics 或 temporary-mask restore protocol；
- 需要修改 architecture、wait、scheduler 或 topology 行为才能编译；
- 需要新增 generic context / manager / carrier 才能避免模块循环；
- visibility 只能通过扩大到 `pub` 解决，而不是 sibling / crate-local 窄接口。

停止结果写 transaction；若只是 write set / stage shape 变化，更新本文；若暴露 owner 或 contract 问题，回 RFC review。

### Write Set

- `anemone-kernel/src/task/sig/mod.rs`
- `anemone-kernel/src/task/sig/{mask,pending,generation,delivery}.rs`
- `anemone-kernel/src/task/sig/disposition.rs` 仅允许必要的 sibling visibility 调整。
- `anemone-kernel/src/task/mod.rs` 只允许 import / re-export 路径调整。
- 其它 kernel、architecture、apps、tests 与 docs 只读；transaction 记录执行事实除外。

### 可观测性

- 不增加 runtime log 或新状态。
- 保留现有 temporary-mask leak、invalid mask、frame failure 与 pending debug 信息。

### 验证

```sh
just fmt kernel --check
just build
```

- `git diff --check`
- old/new symbol、visibility、direct-field 和 callsite source audit。
- 本 Stage 不运行 QEMU / LTP；任何可见语义变化都表示 Stage 0 失败，而不是需要 runtime 接受的新结果。

### 退出条件

- build 与 format check 通过。
- review 确认每一行改动都属于移动、module declaration、import、visibility 收窄或必要路径修正。
- Signal module root 不再混合四类行为职责。
- Stage 0 transaction checkpoint 关闭后才可冻结 Stage 1 manifest。

### 当前结果（2026-07-20）

Stage 0 已按冻结 manifest 完成并关闭：`mask.rs`、`pending.rs`、`generation.rs` 与
`delivery.rs` 已建立，`mod.rs` 保留根领域名词与声明，既有调用面、visibility、pending /
reservation、temporary-mask cleanup、handler-frame loop 与锁序保持不变。`just build`、
format、whitespace、diff 和 source audit 均通过；首次 sandbox build 的 `lwext4` `Bad
system call` 已通过同一仓库入口的批准 escalation 重试并通过。QEMU/LTP/LA64 按本阶段
边界未运行。未发现需要回写 RFC target、contract 或 register 的问题。

### Contract Cutover

None；全部 current Signal contract 保持原样。

## 7. Stage 1：ThreadGroup owner 与 mandatory-entry dormant foundation

### 前置条件

- Stage 0 split-only review 关闭。
- Stage 1 resolved manifest 点名 ThreadGroup construction、membership join/detach、两架构 trap entry / ordinary return、fresh task、clone child 与 exec new-image 的全部物理入口。
- Stage 1 manifest 已冻结 RV64 wrapper 输入、修改前 baseline log，以及仅包含 `signal` / `wait`
  的 LTP profile；退出时按同一输入比较结果，不能用已知无关失败掩盖新增回退。

### 交付

- 创建 `task/jobctl/`，先落入实际需要的 `mod.rs`、`group.rs` 与 `user_entry.rs`；没有 report consumer 前不创建空 `report.rs`。
- 为 user ThreadGroup 建立 owner-local `Running` baseline、continue ordering identity、membership-bound exposure 与 `jobctl_unblocked` wake capability；kthread 保持无 jobctl behavior，并通过构造与 shape assertion保证 presence。
- membership value 直接承载 exposure，或采用经 review 证明等价的 membership record；不得另建 task-local ack、participant set 或派生 behavioral counter。
- RV64 / LA64 user-to-kernel trap entry 在继续内核处理前清除 exposure。
- ordinary trap return、fresh user task、clone child、exec new image 在 architecture transition 前进入相同逻辑 gate；Running gate 登记 exposure。
- park API 与 re-arbitration loop 可以形成最终 owner-local形状，但本 Stage 没有 production stop ingress，所有 user ThreadGroup 都保持 Running。

### 审计

- 按 `on_prv_change(Privilege::User)`、raw user transition、trap entry symbol、clone / exec direct entry逐项建立 closure table。
- 审计 `ThreadGroupInner::members` 的全部 construct / insert / remove / iterate / snapshot caller。
- 审计 `ThreadGroupType::User / KThread` construction，证明 `Option<UserJobControl>` 或等价 presence不是第二真相源。
- 审计 gate 调用时不存在外层 lock、wait registration、temporary-mask token或未收口资源事务。
- scheduler path只读；不得新增 stop state、special wake reason或 placement policy。

### 反馈假设

membership-bound exposure与mandatory entry可以在现有ThreadGroup owner内闭合，而不侵入scheduler-core、generic wait或architecture policy。

以下任一情况停止 Stage 1 并回 RFC review：

- 存在无法放置 exposure clear / final gate 的 user transition；
- Running gate 与 stop request无法在同一个 ThreadGroup ordering domain形成先后关系；
- park 必须持有外层 guard、线性 token或 active wait registration；
- 实现要求 task-local stop flag、scheduler hold、participant ack或跨owner shared mutable truth。

### Write Set

- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/task/jobctl/{mod,group,user_entry}.rs`
- `anemone-kernel/src/task/topology/{mod,thread_group}.rs`
- `anemone-kernel/src/task/api/clone/mod.rs`
- `anemone-kernel/src/task/api/execve/kernel.rs`
- `anemone-kernel/src/arch/{riscv64,loongarch64}/exception/trap/utrap.rs`
- `anemone-kernel/src/arch/{riscv64,loongarch64}/sched.rs`
- lifecycle、wait、procfs、Signal semantic path、apps 与 rootfs 只读。

### 当前 Resolved Write Set Manifest（2026-07-21）

Stage 1 preflight 已按 live source 将默认 write set 解析为以下逐文件 manifest：

- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/task/jobctl/{mod,group,user_entry}.rs`（新建）
- `anemone-kernel/src/task/jobctl/api/{mod,getpgid,getsid,setpgid,setsid}.rs`（从
  `task/api/jobctl/` 行为保持迁移）
- `anemone-kernel/src/task/api/mod.rs` 与
  `anemone-kernel/src/task/api/jobctl/{mod,getpgid,getsid,setpgid,setsid}.rs`（只允许删除旧
  module declaration / 旧物理副本）
- `anemone-kernel/src/task/topology/{mod,thread_group}.rs`
- `anemone-kernel/src/task/api/clone/mod.rs`
- `anemone-kernel/src/task/api/execve/kernel.rs`
- `anemone-kernel/src/arch/{riscv64,loongarch64}/exception/trap/utrap.rs`
- `anemone-kernel/src/arch/{riscv64,loongarch64}/sched.rs`
- 本实施计划与对应 transaction 的 Stage 1 执行事实更新

`task/api/jobctl -> task/jobctl/api` 是同一 task/jobctl owner 内的行为保持型物理迁移：
syscall registration、ABI、可见语义和逻辑 policy 不变，不引入 facade 或扩大 public API。
若迁移要求改变这些边界，Stage 1 立即停止并按 write-set 扩展 / RFC review 路径上报。
`clone/mod.rs`、`execve/kernel.rs` 与两架构 `sched.rs` 即使最终只需作为 closure 证据保持
只读，也保留在 resolved manifest 中，避免把未修改误写成未审计。

冻结的 baseline 输入为 `anemone-apps/user-test/ltp/profile.txt` 中仅启用 `signal` 与
`wait`、`etc/sdcard-rv.img`、RV64 wrapper 和
`build/unix-jobctl-stage1-baseline-rv64.log`。修改前 wrapper 全链路完成；KUnit 182 项全部
通过，glibc 与 musl 各自都是 `attempted=56 passed=49 failed=5 infra_failed=0 skipped=2`。
退出时必须使用相同输入和结果分类比较，不得把这 10 个既有 LTP failure 当作新增回退。

### 可观测性

- 轻量 assertion 覆盖 user/kthread shape、membership exposure、Running gate与detach前 exposure cleanup。
- diagnostic snapshot 至少能输出 phase、first reason、exposed count与phase age的占位形状；本 Stage 不对procfs暴露这些字段。
- member identity若进入debug output，明确标注只服务诊断，不参与completion。

### 验证

```sh
just fmt kernel --check
just build
./scripts/run-user-test-rv64.sh etc/sdcard-rv.img build/unix-jobctl-stage1-rv64.log
```

- RV64 / LA64 entry source closure review。
- 构建证据只使用 `just build`；不直接调用 `just xtask qemu`。
- RV64 wrapper 是 mandatory baseline gate；它必须完成完整 rootfs / kernel / QEMU 链，并证明
  冻结的 `signal` / `wait` 结果相对修改前 baseline 没有新增回退。该结果只证明 current
  behavior 未回退，不证明 jobctl target。

### 退出条件

- 每条 user-to-kernel 与 kernel-to-user路径均进入closure table，未解析项为零。
- user ThreadGroup 的 exposure 是 membership owner的一部分，kthread不参与。
- 所有 production group保持 Running；没有 signal 可以触发 stop / continue。
- RV64 mandatory baseline wrapper完成，且冻结结果集相对修改前baseline没有新增回退。
- Stage 1 review 关闭后才可冻结 Stage 2 manifest。

### 当前结果（2026-07-21）

Stage 1 已按 resolved manifest 完成并关闭。ThreadGroup membership 直接承载 user exposure，
user group 具有 dormant `Running` phase、continue ordering identity 与 predicate-only wake
capability；kthread 不具有 user job-control state。RV64 / LA64 trap entry 清 exposure，ordinary、
fresh、clone 与 exec user return 都在 architecture-local FPU restore 之前进入同一 gate，并在
restore 完成后才发布 user privilege。所有 production group 仍保持 `Running`，Signal、wait、
lifecycle、procfs 与 scheduler 均未获得 stop / continue ingress或第二份 truth。

RV64 wrapper 使用冻结输入完成，KUnit 182 项全部通过；glibc 与 musl 各自保持
`attempted=56 passed=49 failed=5 infra_failed=0 skipped=2`，逐 libc、逐 testcase 的 exit
classification 与修改前 baseline 完全一致。独立 review 的可见性、FPU ordering、exposure
transition assertion、kthread-only removal 与 diagnostic-field 标注 finding 已 neutralize，最终
未留 Apollyon、Keter 或 Euclid。Stage 2 manifest 未冻结，也未进入实现。

### Contract Cutover

None；`USER-ENTRY-002`、`JOBCTL-STATE-001` 与 `JOBCTL-STOP-001` 仅达到 dormant target readiness。

## 8. Stage 2：单线程 integrated production vertical slice

### 前置条件

- Stage 1 mandatory-entry closure review通过。
- Stage 2 先用最小 lock / call graph probe 确认 `topology / exact identity -> ThreadGroup owner -> one Signal leaf` 的控制事务可行；反向锁、跨 guard wake / user copy / complex drop、post-commit recoverable failure 或第二 truth 都立即停止本 Stage。
- Stage 2 resolved manifest同时覆盖 Signal generation、jobctl、parent report、wait ABI、SIGCHLD、procfs、deterministic test app与RV64 wrapper；不得只实现 producer 或 consumer一侧。
- Stage 2 使用最终通用 ThreadGroup路径，不允许 `ntasks() == 1` feature branch。

### 交付

- 在 `sig::generation` 建立已验证 concrete `SIGSTOP` / `SIGCONT` generation入口：signal-0、permission failure、target mismatch 与 terminal group不得产生side effect。
- 对global init执行target规定的action admission；合法stop-class generation可以清理ordinary opposite pending，但init不得取得stop authority。
- 普通user ThreadGroup的`SIGSTOP`不进入ordinary pending、不建立reservation、不force-complete active wait，直接调用唯一stop engine。
- `SIGCONT` generation推进continue ordering、清理ordinary stop-class pending、执行一次group resume，再按ordinary disposition / mask处理occurrence。
- jobctl owner提交 `Running -> Stopping -> Stopped` 与 `Stopped -> Running + Continued report`；incomplete `Stopping -> Running`不生成parent report。
- 创建 `task/jobctl/report.rs`，把`child_exited` predicate event扩展为child-status predicate event；Event与SIGCHLD只触发重扫，不携带report truth。
- wait core返回typed `ChildWaitOutcome`，让wait4 / waitid从同一snapshot序列化Exited / Stopped / Continued；WNOWAIT只peek，consume在topology / parent relation -> child owner下重验current report。
- procfs从单次derived snapshot生成state character / name：terminal Zombie优先，committed Stopped显示T，Stopping不伪装Stopped。
- 新建专用 `anemone-apps/jobctl-test/`，覆盖单线程SIGSTOP、Stopped report、WNOWAIT、SIGCONT、Continued、wait4 / waitid与procfs。不要把该状态机测试继续堆入一般Signal action测试。

### 审计

- concrete syscall target / permission成功点到generation入口的所有路径：kill、tkill、tgkill、rt_sigqueueinfo与process-group broadcast。
- `SIGSTOP` path上不存在ordinary pending publication、reserved delivery或`notify(..., true)`等generic force-wake。
- report commit先于parent Event publish；SIGCHLD occurrence与report truth解耦。
- wait scan snapshot不携带claim authority；report replacement、reparent或exit后必须重扫。
- `waitid` stopped / continued与对应SIGCHLD固定`si_uid = 0`，不得新增credential cache。

### 反馈假设

真实Signal、ThreadGroup gate、parent wait/report与procfs可以形成一条production vertical slice，而不需要scheduler stop、generic wait cancellation、report identity token或credential副本。

以下任一情况停止 Stage 2：

- control transaction需要反向锁、同时持有多个无序Signal leaf或guards-in wake/Event/user copy；
- parent观察Stopped后仍存在未登记user entry；
- wait report claim只能依赖stale scan snapshot或新增ReportId才能闭合；
- incomplete Stopping取消必须伪造Stopped / Continued才能满足实现；
- vertical slice只能通过singleton分流、feature flag或兼容wrapper运行。

### Write Set

- `anemone-kernel/src/task/sig/{generation,delivery,pending,mod}.rs`
- 需要识别control signal的`task/sig/disposition.rs`
- `anemone-kernel/src/task/sig/info.rs`，只允许补齐job-control `SIGCHLD`所需的typed
  `CLD_STOPPED / CLD_CONTINUED` child code、Linux映射与字段一致性校验
- `anemone-kernel/src/task/jobctl/{mod,group,user_entry,report}.rs`
- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/task/topology/{mod,thread_group,parent_child}.rs`
- `anemone-kernel/src/task/topology/process_group.rs`只允许让control signal携带并重验
  snapshot选择时的PGID；不得把phase、rollback或全组完成点移入ProcessGroup
- `anemone-kernel/src/task/api/wait/{mod,wait4,waitid}.rs`
- `anemone-kernel/src/task/api/exit/mod.rs`
- `anemone-kernel/src/arch/{riscv64,loongarch64}/exception/trap/utrap.rs`只允许把最终
  user transition接入统一Signal / lifecycle / jobctl重新仲裁循环；不得修改trap ABI、
  exception policy或architecture-local FPU owner
- `anemone-kernel/src/fs/proc/tgid/{stat,status}.rs`及窄derived snapshot入口
- `anemone-kernel/src/task/sig/api/{kill,tkill,tgkill,rt_sigqueueinfo}.rs`只允许接入统一generation入口，不复制control policy
- `anemone-apps/jobctl-test/`
- `anemone-apps/user-test/src/{main,guest}.rs`只允许在chroot前把focused `jobctl-test`复制进
  competition root，并在既有environment完成唯一一次procfs挂载后、LTP开始前调用它
- `conf/rootfs/pretest-rv64.toml`只允许安装`jobctl-test`
- `docs/src/register/current-limitations.md`只允许记录用户在2026-07-21接受的job-control SIGCHLD
  guards-out publication ordering窗口、影响范围与退出条件
- LA64 runtime、orphaned-pgrp、TTY、ptrace与current contracts只读。

### 可观测性

- phase transition log / trace包含TGID、old/new phase、first reason、exposed count与phase age。
- Stopping长期不收敛时可以查询exposed count；member ID只作诊断。
- report publish / replace / consume记录kind与child TGID，不把Event sequence当identity。
- global init拒绝stop authority与SIGSTOP no-pending path有定向assert / test证据，避免依赖高频日志。

### 验证

```sh
just fmt kernel --check
just fmt jobctl-test --check
just build
./scripts/run-user-test-rv64.sh etc/sdcard-rv.img build/unix-jobctl-stage2-rv64.log
```

- `jobctl-test`单线程case必须全部通过。
- 源码审计证明 Stage 2 代码没有group-size runtime分流、scheduler hold、generic active-wait completion或credential副本。
- Stage 2 runtime只证明vertical slice candidate；不能据此cutover或关闭多成员/lifecycle obligations。

### 退出条件

- SIGSTOP / SIGCONT、gate、wait4、waitid、WNOWAIT、SIGCHLD与procfs在同一通用路径上贯通。
- 单线程production runtime通过，report / Event / wait claim顺序review通过。
- candidate train明确标记non-publishable；只有新的明确授权才能进入 Stage 3A。

### 执行结果

Stage 2 已按最终通用 ThreadGroup 路径关闭。generation-time `SIGSTOP / SIGCONT`、global-init
immunity、owner-local phase/report、统一 user-entry arbitration、wait4 / waitid / WNOWAIT、
job-control SIGCHLD与procfs committed `T`已贯通；focused harness复用competition root唯一一次
procfs mount，没有引入singleton分流、scheduler hold、generic wait completion、credential truth或
Event-carried report identity。

最终RV64 wrapper完成正常关机：182项KUnit全部通过，四个`jobctl-test` case全部通过；glibc与
musl的`signal`各为`attempted=37 passed=30 failed=5 infra_failed=0 skipped=2`，`wait`各为
`attempted=19 passed=19 failed=0 infra_failed=0 skipped=0`，总计
`attempted=112 passed=98 failed=10 infra_failed=0 skipped=4`，与冻结baseline一致。kernel、
user-test与jobctl-test format check、kernel build、LA64 user-test app build及diff whitespace检查通过；
LA64 runtime按本Stage边界未运行。

独立owner与ABI final review均未留下Apollyon、Keter或Euclid。explicit `SIG_IGN`、
`SA_NOCLDSTOP`未增加focused runtime case，LA64仅完成source/build closure；guards-out SIGCHLD
publication ordering与数值TID / PGID复用按已批准边界记录，不在本Stage引入临时协议。Stage 2
candidate继续保持non-publishable，`UJ-CUTOVER=None`，Stage 3A Not Started。

### Contract Cutover

None。Stage 2 开始形成可运行candidate，但全部current contract仍保持effective。

## 9. Stage 3：完整 control signal、多成员与 lifecycle closure

Stage 3 分成两个按顺序关闭的 checkpoint。3A 只验证 Signal owner 与 jobctl owner 之间的
control transaction；3B 才把已经稳定的 control semantics 扩展到多成员 membership、
lifecycle 与 topology。两个 checkpoint 都属于同一 non-publishable candidate train，均不
执行 contract cutover。

### Checkpoint 3A：conditional control signal、reservation 与 temporary-mask closure

#### 前置条件

- Stage 2 integrated slice关闭且candidate未发布。
- 3A resolved manifest冻结 private/shared pending、reserved delivery、temporary-mask、live
  disposition与全部可达control-generation producer的调用清单。
- Stage 2 已有 wait/report、procfs 与 lifecycle 路径在本 checkpoint 默认只读，作为回归消费者。

#### 交付

- 为SIGTSTP / SIGTTIN / SIGTTOU generation捕获current continue ordering，并按ordinary disposition / mask / pending规则接纳；只有最终live action为DefaultStop且authority仍有效时请求同一stop engine。
- stale DefaultStop candidate只取消jobctl effect，不重新发布、补偿或重排signal。
- 四种stop-class generation清理ordinary reserved之外的SIGCONT pending；SIGCONT清理ordinary stop-class pending；已经claimed的reserved occurrence保持finality。
- Stopping / Stopped arbitration允许reserved SIGCONT完成live custom / ignore / default no-frame action与temporary-mask cleanup，但不授予user-entry permit，也不在同一pass继续消费其它ordinary async signal。
- generation-time SIGCONT side effect在delivery、sync consume、default consume或handler action中绝不重放。

#### 审计

- private/shared、standard/realtime、task-directed/group-directed、multiple-member source competition与reservation全矩阵。
- temporary-mask restore责任在handler frame、no-frame consume与no-return terminal路径恰好一次收口。
- control cleanup不撤销已经claimed的reservation，reserved retirement不顺带消费其它ordinary async signal。
- Stage 2的wait/report与procfs消费者不保存epoch、reservation或pending truth。

#### 反馈假设与停止条件

完整control semantics可以在Signal与ThreadGroup owner之间闭合，不需要persistent Signal
carrier、second queue或generic final-consumption framework。以下任一情况停止3A并回写
transaction；若改变owner、target或ABI则回RFC review：

- reserved occurrence只能通过撤销claim或复制payload才能与control cleanup共存；
- temporary-mask责任无法在handler frame、no-frame或no-return terminal路径恰好一次收口；
- conditional DefaultStop需要跨Signal persistent carrier、第二pending queue或新的wait-core状态；
- 需要削弱global-init immunity、generation-only SIGCONT side effect或accepted reserved race才能通过测试。

#### Write Set

- `anemone-kernel/src/task/sig/{mod,pending,generation,delivery}.rs`
- `anemone-kernel/src/task/sig/disposition.rs`
- `anemone-kernel/src/task/sig/api/`中3A manifest点名的control-generation producer。
- `anemone-kernel/src/task/jobctl/{mod,group,user_entry}.rs`中control authority、phase arbitration与gate重仲裁的owner-local入口。
- `anemone-apps/jobctl-test/`中的conditional stop、reservation、temporary-mask与SIGCONT ordering case。
- 其它topology/lifecycle、wait/report、procfs、architecture、scheduler、rootfs和current contract正文只读。

#### 当前 Resolved Write Set Manifest（2026-07-21）

Stage 2关闭后已经完成独立只读transition preflight；用户本轮明确授权完成整个Stage 3，因此3A在
以下manifest冻结后进入Active：

- `anemone-kernel/src/task/sig/{mod,info,pending,generation,delivery,disposition}.rs`
- `anemone-kernel/src/task/jobctl/group.rs`
- `anemone-kernel/src/task/api/exit/mod.rs`（仅纠正既有child-exit siginfo code）
- `anemone-apps/jobctl-test/src/main.rs`
- `docs/src/rfcs/unix-jobctl/{index,invariants,implementation}.md`
- `docs/src/rfcs.md`
- `docs/src/devlog/2026-07-06_to_2026-07-19.md`
- `docs/src/devlog/transactions/2026-07-20-unix-jobctl.md`

全部control-generation producer已经闭合到`Task::recv_signal()`、`ThreadGroup::recv_signal()`、
exact-member与expected-PGID generation route；`kill / tkill / tgkill / rt_sigqueueinfo`和
`ProcessGroup::recv_signal()`保持只读，不需要producer-local policy。clone / clone3保存的任意合法
exit signal会在`task/api/exit/mod.rs`经parent `ThreadGroup::recv_signal()`生成；
`task/api/clone/`保持只读producer审计，`task/api/exit/mod.rs`只修改生成的child-exit siginfo code。
`task/sig/mask.rs`、
`rt_sigaction / rt_sigsuspend`与`ppoll / pselect6`只作为现有temporary-mask owner和调用者审计面，
不需要修改。Stage 2 wait/report、procfs、topology/lifecycle、architecture与rootfs wiring保持只读。

冻结的lock/call方向为`exact identity / topology -> ThreadGroup owner -> at most one private/shared
Signal leaf`。conditional occurrence只携带窄`ContinueEpoch`；reservation仍是task-private Signal
truth，live action selection与temporary-mask cleanup仍由Signal delivery owner收口。若实现要求
修改上述只读owner、引入second queue / persistent carrier / wait-core状态或改变target语义，立即
触发3A停止合同。

独立review发现`SiCode::Kernel`同时覆盖同步fault与child exit signal、SIGPIPE等异步kernel
producer，不能单独作为Stopped期间no-return同步action的authority。用户批准把
`task/sig/info.rs`加入本manifest；修复只从现有typed siginfo fields判定同步fault，不改producer
API或R0 target。用户随后批准对child-exit producer作窄ABI纠正，因此`task/api/exit/mod.rs`只把
现有child-layout siginfo从错误的`SI_KERNEL`改为对应`CLD_EXITED / CLD_KILLED`；不改变exit
状态机、通知时序或jobctl owner。`anemone-rs`虽获条件扩展授权，但当前不需要修改，仍不进入
exact manifest。

#### 可观测性

- temporary-mask / reserved SIGCONT case可以区分occurrence claim、live action selection、retirement、handler-frame commit与最终user-entry permit。
- stale epoch rejection与generation-only resume有低频assertion或定向测试证据，不在hot path持续打印。

#### 验证

```sh
just fmt kernel --check
just fmt jobctl-test --check
just build
./scripts/run-user-test-rv64.sh etc/sdcard-rv.img build/unix-jobctl-stage3a-rv64.log
```

- `jobctl-test`覆盖caught / ignored / masked条件性stop、task/private与group/shared竞争、reserved旧SIGCONT、普通mask、SA_NODEFER、SA_RESETHAND、frame failure与普通SIGKILL no-return路径。R1接受reservation-first导致的窄SIGKILL延迟，不再要求runtime证明later pending SIGKILL越过已claim target。
- 冻结的`signal` / `wait` profile相对Stage 2没有新增回退。
- LA64只做Signal delivery与user-entry source closure；除非用户另行授权，不增加LA64 runtime要求。

#### 退出条件

- `INV-CONTROL-TXN`有完整源码与RV64 production evidence。
- Signal、jobctl、wait与temporary-mask之间没有第二truth、persistent carrier或transitional fallback。
- 3A transaction checkpoint独立review关闭后，才可冻结3B manifest。

#### Contract Cutover

None。

#### Stage 3A closeout (2026-07-21)

Stage 3A 已完成并关闭。`ContinueEpoch`、phase-aware pending fetch、task-private reserved
delivery、opposite-class cleanup、temporary-mask handler/no-frame/no-return cleanup、live
disposition、`SA_NODEFER` / `SA_ONESHOT`、frame failure 与普通SIGKILL no-return路径均已在冻结
manifest 内收口；当前 contracts 保持 effective，`UJ-CUTOVER=None`。

独立 review 针对R1清零 Apollyon、Keter、Euclid。review 期间确认并修复两项窄问题：Stopped-phase
fetch 只能把 typed synchronous fault 作为 no-return terminal authority，child-exit siginfo
改用 `CLD_EXITED` / `CLD_KILLED`；conditional `DefaultStop` action 结束当前 ordinary scan
后回到 user-entry gate。两项修复均不改变 owner、ABI acceptance boundary 或
current contract。

review还确认current reservation-first顺序可让旧reserved `SIGCONT`先于later pending `SIGKILL`
retire；若真实的新`SIGCONT`已经恢复Running，custom handler可以执行到下一次mandatory kernel
entry。用户接受该既有Signal递送延迟，不要求本RFC扩张`jobctl/user_entry.rs`或改变pending lock
ordering；R1据此修正`USER-ENTRY-002`，但不削弱Stopped gate、terminal lifecycle precedence或
SIGKILL pending ownership。该精确竞态没有production test hook，证据边界是current contract、
source ordering、reservation KUnit和分别确定性的runtime路径，不声称已做单次可重复注入。

验证证据来自当前 candidate 的
`build/unix-jobctl-stage3a-rv64.log`：184 项 KUnit、全部 13 个 `jobctl-test` focused case
通过，glibc/musl `wait` 各 `19/19`，signal/wait profile 合计
`attempted=112 passed=98 failed=10 infra_failed=0 skipped=4`，正常关机；相对 Stage 2
没有新增 signal/wait 回退。`just fmt jobctl-test --check`、RV64 app build 与
`git diff --check` 通过；`just fmt kernel --check` 仅报告未手工维护的 generated
`kconfig_defs.rs` / `platform_defs.rs` whitespace。

### Checkpoint 3B：多成员 exposure、lifecycle 与 topology closure

#### 前置条件

- Checkpoint 3A关闭，control transaction不再作为本 checkpoint 的开放设计变量。
- 3B resolved manifest解析全部member join/detach、fork/clone、exec/dethread、ordinary exit、SIGKILL、exit_group、reparent、process-group broadcast与ordinary-wait测试路径。
- membership / lifecycle owner与jobctl cleanup的lock order、guards-out effect清单已冻结。

#### 交付

- 多成员ThreadGroup以membership exposure closure提交Stopped；runnable、syscall与ordinary-wait混合成员不建立participant ack。
- clone publication、fresh child、exec new image、dethread victim removal、ordinary detach与last-member exit完整维护exposure。
- terminal lifecycle清exposure / report并释放jobctl parker，但不复制或覆盖first terminal code；SIGKILL与Exiting / Exited支配jobctl phase。
- 带非空report的reparent唤醒new parent重扫，不重放历史SIGCHLD。
- process-group broadcast继续让每个ThreadGroup独立接受control generation，不建立ProcessGroup-wide phase或rollback。

#### 审计

- ordinary wait在stop前后保持原predicate、deadline、registration与result；jobctl Event不进入wait owner。
- clone/fork不继承pending stop、phase或report；同一ThreadGroup新member与exec image不能绕过live stop。
- detach、dethread、last-member exit与terminal cleanup不会遗留exposure、report或parker责任。
- reparent relation publication、new-parent Event与report claim继续遵守topology -> child owner方向。

#### 反馈假设与停止条件

multi-member与lifecycle closure可以保持ThreadGroup owner-local，不需要scheduler state、generic
wait adapter或per-round participant ledger。以下任一情况停止3B：

- multi-member completion需要主动取消ordinary wait或读取scheduler state；
- lifecycle cleanup出现ownerless exposure / report / parker责任；
- terminal与jobctl phase成为并列truth；
- topology / child owner只能通过反向锁或ReportId式第二identity闭合；
- 需要削弱entry closure、report precedence或terminal dominance才能通过测试。

#### Write Set

- `anemone-kernel/src/task/jobctl/{mod,group,user_entry,report}.rs`
- `anemone-kernel/src/task/topology/{mod,thread_group,parent_child,process_group}.rs`
- `anemone-kernel/src/task/api/{clone,execve,exit}/`中3B manifest点名文件。
- `anemone-apps/jobctl-test/`中的多成员、lifecycle、reparent、process-group与ordinary-wait case。
- 必要的RV64 pretest rootfs / harness wiring仅允许接线既有`jobctl-test`；不得扩大测试范围。
- Signal generation/delivery、wait ABI、procfs、architecture、scheduler wait/core、TTY、orphaned-pgrp、ptrace与current contract正文只读；若真实owner修复需要越界，先走write-set扩展申请。

#### Stage 3B Resolved Write Set Manifest (2026-07-21)

Stage 3A 已由 `76bd18f5` 独立关闭。随后执行的只读 transition preflight 逐项核对 live
membership publication、detach / dethread、clone、exec、ordinary / group exit、reparent、
process-group broadcast、jobctl gate / report owner与focused harness；用户此前对完整 Stage 3 的
明确授权在本 manifest 冻结后激活 3B。精确 write set 为：

- `anemone-kernel/src/task/jobctl/{mod,group,user_entry,report}.rs`
- `anemone-kernel/src/task/topology/{mod,thread_group,parent_child,process_group}.rs`
- `anemone-kernel/src/task/api/clone/mod.rs`
- `anemone-kernel/src/task/api/execve/kernel.rs`
- `anemone-kernel/src/task/api/exit/mod.rs`
- `anemone-kernel/src/task/sig/mod.rs`（仅增加kernel-private dethread victim purpose）
- `anemone-kernel/src/task/sig/pending.rs`（仅保证ordinary SIGKILL不会被该purpose合并覆盖）
- `anemone-kernel/src/task/sig/delivery.rs`（仅收口该purpose的per-task no-return action）
- `anemone-apps/jobctl-test/src/main.rs`
- `docs/src/rfcs/unix-jobctl/{index,implementation}.md`
- `docs/src/rfcs.md`
- `docs/src/devlog/2026-07-06_to_2026-07-19.md`
- `docs/src/devlog/transactions/2026-07-20-unix-jobctl.md`

preflight确认当前 membership value 已是 exposure 唯一真相；join保持Unexposed，ordinary detach与
dethread rekey只接受已经进入kernel的Unexposed member，fresh / clone / exec最终都重新经过统一
gate。`prepare_terminal()`已经在`exit_group`首次提交`Exiting(first_code)`与last-member `Exited`
publication前清report、归一phase并释放parker；后续member exit只复用first terminal code。
reparent在relation双方发布后只唤醒new parent Event重扫，不重放历史SIGCHLD；ProcessGroup仍只做
ThreadGroup selector。上述已有路径必须通过3B source audit和runtime证明，不能因“已经存在”跳过
验收。

validation-only输入冻结为仅启用`signal` / `wait`的
`anemone-apps/user-test/ltp/profile.txt`、`etc/sdcard-rv.img`、RV64 wrapper和
`build/unix-jobctl-stage3b-rv64.log`。除上述三个精确signal文件中的dethread victim purpose与
ordinary SIGKILL coalescing dominance外，
Signal generation/delivery、wait ABI、procfs、architecture、scheduler/wait core、rootfs/current
contract正文保持只读。若实现需要修改这些边界、增加scheduler
state、取消ordinary wait、引入participant ledger / ReportId、改变target/ABI或反转
topology -> ThreadGroup锁序，立即停止3B并上报，不能在本manifest内绕行。

首次RV64 focused run证明jobctl多成员与process-group路径通过，但既有multi-thread exec在
`Task::dethread()`向siblings发送普通kernel `SIGKILL`后，由default action误入整个
`kernel_exit_group()`，child以SIGKILL而不是新映像成功退出。该路径自原有multithreading实现即
存在，不是jobctl target delta。用户此前已授权顺手纠正相邻wrong-signal usage、且当前RFC不为
旧问题承担target责任。owner-identity probe经review被拒绝：process-directed SIGKILL是shared
occurrence，若由victim先claim，单靠owner identity可能吞掉真实group terminal action。最终manifest
只扩展`task/sig/{mod,pending,delivery}.rs`：`dethread()`产生的task-private SIGKILL携带
kernel-private victim purpose；pending owner保证该purpose不能合并覆盖ordinary task-directed
SIGKILL，reserved victim action也会在per-task退出前重验later ordinary SIGKILL；只有精确victim
occurrence在delivery中复用ordinary per-task `kernel_exit(0)`，任何外部或shared SIGKILL仍进入
既有`kernel_exit_group()`。ABI、public API、jobctl target、current contract与
`UJ-CUTOVER=None`均不改变。

#### 可观测性

- multi-member Stopping snapshot包含exposed count与phase age；不得把snapshot反向用于completion。
- terminal cleanup、report replacement与reparent wake有低频边界日志或assertion。
- membership identity若进入日志，只服务诊断，不参与completion或report claim。

#### 验证

```sh
just fmt kernel --check
just fmt jobctl-test --check
just build
./scripts/run-user-test-rv64.sh etc/sdcard-rv.img build/unix-jobctl-stage3b-rv64.log
```

- `jobctl-test`覆盖runnable + syscall + ordinary-wait多成员组合；parent在waiter未完成时观察Stopped；SIGCONT后waiter仍按原predicate / timeout返回。
- 覆盖clone / fork / exec / dethread / member exit / SIGKILL / exit_group / reparent。
- 覆盖task-directed、ThreadGroup-directed与process-group-directed四种stop signal的多成员路径。
- 冻结的`signal` / `wait` profile相对3A没有新增回退；LA64只完成source closure，除非用户另行授权，不增加LA64 runtime要求。

#### 退出条件

- `INV-ENTRY-CLOSURE`、`INV-LIFECYCLE`与多成员部分的`INV-REPORT-CLAIM`逐项有源码和runtime evidence。
- 无scheduler/generic wait侵入，无second truth、participant ledger或transitional fallback。
- 3B transaction checkpoint review关闭后，candidate继续保持non-publishable；Stage 4只有在独立
  transition preflight与明确授权后才能进入，当前保持Not Started。

#### Contract Cutover

None。

## 10. Stage 4：ABI、竞态、旁路与production validation closure

### 前置条件

- Stage 3A与3B都已独立review关闭，完整candidate review通过。
- Stage 4 validation manifest列出custom app case、LTP case/subcase、预期结果、允许的非目标failure、wrapper输入、log路径与profile状态。
- 不允许为通过 Stage 4 临时降低test集合、注释失败case或修改oracle。

### 交付

- 关闭所有target proof obligation与旁路审计。
- 验证wait4 `WUNTRACED / WCONTINUED`、waitid `WSTOPPED / WCONTINUED / WNOWAIT`、status word与`CLD_STOPPED / CLD_CONTINUED`映射。
- 验证`SA_NOCLDSTOP` / ignored SIGCHLD只抑制signal occurrence，不删除report或parent predicate wake。
- 验证deterministic `Stopping x SIGCONT`无report，以及committed Stopped后恰好一次Continued。
- 验证reserved旧SIGCONT、随后SIGSTOP与真正恢复用新SIGCONT的accepted race，包括普通mask、SA_NODEFER、SA_RESETHAND、frame failure、普通SIGKILL no-return路径与R1 reservation-first延迟边界。
- 验证global init全部可达producer；合法stop-class generation执行规定cleanup但永不进入Stopping / Stopped。
- 验证procfs `stat` / `status`同一snapshot的character / name、Stopping显示、Stopped T与Zombie Z优先。
- 核验stopped / continued waitid与job-control SIGCHLD的`si_uid = 0`，并搜索确认无新credential truth。

### 审计

- 第12节全部旁路搜索为零或有明确允许理由。
- Contract Impact每个ID都有对应源码、测试或source-audit evidence。
- 所有runtime logs来自candidate build与本次wrapper，不接受旧artifact或无provenance日志。
- register中TTY、orphaned-pgrp、ptrace与`si_uid = 0`边界的最终write-back已准备，但在cutover前不提前宣称关闭。

### Write Set

- candidate kernel code默认只读；Stage 4不是第二个宽泛实现阶段。
- `anemone-apps/jobctl-test/`
- `anemone-apps/user-test/ltp/groups/{wait,signal}.txt`只允许按manifest启用明确case；不得扩大为无关LTP整理。
- `anemone-apps/user-test/ltp/profile.txt`只读，并保持仅启用`signal`与`wait`。
- RV64 pretest rootfs / harness只允许manifest中的测试资产接线。
- 受影响current contracts、RFC、transaction与register在本 Stage只允许准备cutover diff和evidence索引，尚不生效。

Stage 4发现candidate代码缺陷时，立即停止validation checkpoint，不在上述write set内直接修复。
总控先冻结一个逐文件`Stage 4 Repair Manifest`，点名缺陷、唯一owner、最小代码write set、
受影响contract / proof obligation、reviewer与必须重跑的validation floor；批准后建立独立repair
checkpoint。repair只修复不改变target的缺陷，完成owner-local review和受影响floor后，重新进入
Stage 4并从受影响case开始重跑。若缺陷改变owner、target、ABI、accepted limitation或验收边界，
不建立repair checkpoint，直接回RFC review。

### Stage 4A Repair Manifest（2026-07-21）

Stage 3B closure后的只读transition preflight确认一个target-preserving candidate缺陷：
`kernel_exit_group()`可以在其它live member仍为Exposed时发布`Exiting`，但现有
`UserJobControl::prepare_terminal()`只归一phase并清report，没有按`JOBCTL-STATE-001`、
`JOBCTL-LIFE-001`和`INV-LIFECYCLE`要求在同一ThreadGroup owner transaction清除全部exposure。
lifecycle gate仍阻止terminal后的user entry，因此该finding不要求改变target、owner、ABI、
visible semantics、accepted limitation或`UJ-CUTOVER`；但Stage 4不能用不符合target的诊断状态
作为closure evidence。preflight同时确认现有diagnostic projection没有实际caller，长期
`Stopping`缺少phase age与remaining exposed progress的可用日志证据。

Stage 4A唯一权威resolved manifest冻结为：

- `anemone-kernel/src/task/jobctl/group.rs`
- `anemone-kernel/src/task/jobctl/user_entry.rs`
- `anemone-kernel/src/task/jobctl/report.rs`（只允许report transition / exact-once consume的低频诊断）
- `anemone-kernel/src/task/api/exit/mod.rs`
- 本实施计划与对应transaction的Stage 4A执行事实更新

允许的repair仅包括：terminal owner transaction清全组exposure；terminal之后到达trap entry的
member只验证该预清状态，`Alive`路径继续严格要求`Exposed -> Unexposed`；让现有diagnostic
projection进入低频phase transition / progress日志，并明确phase age、first reason与remaining
exposed count都不驱动行为；report文件只增加不携带truth的诊断输出。不得修改Signal、wait/report ABI、procfs、topology、scheduler/
wait-core、apps、LTP输入、current contract或register。

Stage 4A验证floor为`just fmt kernel --check`、`just build`、全部enabled KUnit、现有17个
`jobctl-test` case与RV64 wrapper
`./scripts/run-user-test-rv64.sh etc/sdcard-rv.img build/unix-jobctl-stage4a-rv64.log`；重点复核
multi-member external SIGKILL、exit_group、multithread exec / dethread、terminal first-code、
trap-after-terminal与membership detach assertion。独立review必须确认Apollyon / Keter / Euclid
均为0。若repair需要改变public API、锁方向、terminal lifecycle、user-entry visible behavior或
新增task-local flag / second exposure truth，Stage 4A立即停止并回RFC review。

Stage 4B的测试资产、validation manifest与文档write-back必须在4A独立review、验证并记录关闭后
重新读取live candidate再冻结；当前未解析、未激活，也不因4A通过自动开始。Stage 4A与4B各自
保留gate证据，但按用户最终要求只在整个Stage 4关闭后形成一个`jobctl:`提交。

Stage 4A于2026-07-21关闭。repair后的RV64 wrapper重新构建kernel并运行185个enabled KUnit、
17个既有`jobctl-test` case及冻结的`signal` / `wait` profile：KUnit与focused case全部通过，
glibc和musl各为`56/49/5/2`，合计`112/98/10/4`且`infra_failed=0`，正常关机。独立review先
发现无限`WNOWAIT` peek日志会在owner锁域内产生无界诊断，修正为只记录exact-once consume后，
最终Apollyon / Keter / Euclid均为0。`just fmt kernel --check`除两个构建生成文件的既有尾随空白外
无authored diff；独立`just build`在沙箱内被lwext4 C子进程的`Bad system call`限制阻断，但同一
wrapper中的candidate kernel build成功，故不把该环境失败归类为candidate failure。candidate继续
non-publishable，`UJ-CUTOVER=None`，current contract与register不变；Stage 4B仍为Not Started。

### Stage 4B Validation Manifest（2026-07-21）

Stage 4A关闭后的只读transition preflight重新核对repair diff、R1 proof obligations、全部Signal
producer、mandatory user-entry、wait/report、procfs、lifecycle/topology与forbidden scheduler/wait-core
bypass；未发现新的candidate defect或owner / ABI / target分叉。Stage 4B唯一权威write set冻结为：

- `anemone-apps/jobctl-test/src/main.rs`
- `anemone-kernel/src/task/jobctl/group.rs`（只增加owner-local `Stopping x SIGCONT` KUnit，不改production code）
- `anemone-rs/src/sys/linux.rs`与`anemone-rs/src/os/linux.rs`（只增加现有`tkill` syscall的窄测试调用接线）
- `anemone-apps/user-test/ltp/groups/wait.txt`
- 本实施计划与对应transaction的Stage 4B evidence / closure write-back

candidate kernel production code、`anemone-apps/user-test/ltp/groups/signal.txt`、`profile.txt`、rootfs harness、current
contracts与register正文保持只读。`profile.txt`必须继续只含`signal` / `wait`；不得注释既有case、
修改oracle、降低timeout或为测试引入kernel hook / counter。若新增case暴露candidate defect，立即停止
validation并冻结新的逐文件repair manifest；若需要改变target、owner、ABI、visible semantics或
accepted limitation，则回RFC review。

custom app case清单冻结为：

- `task-directed-default-stop-signals`：`tgkill`覆盖四种stop signal；
- `sigchld-suppression-report`：`SA_NOCLDSTOP`与explicit ignored `SIGCHLD`均不产生signal occurrence，
  但stopped report仍可由wait owner观察和消费；
- owner-local KUnit确定性构造exposed member并验证`Stopping -> SIGCONT -> Running`无Stopped /
  Continued report；`repeat-continue` production case验证committed stop在首次`SIGCONT`后只产生一次
  Continued，重复`SIGCONT`不重放；
- `wait4-stop-continue-procfs`：核对`stat` / `status`的T、`State: T (stopped)` pair与continue后非T；
- `global-init-immunity`：安全覆盖`kill`、`tkill`、`tgkill`、`rt_sigqueueinfo`的SIGSTOP producer及
  SIGSTOP admission，均不得把init置为T或遗留SIGSTOP pending；
- `multimember-mixed-execution`：在ordinary pipe source仍未满足时，`SIGCONT`只解除jobctl gate、不得
  完成pipe wait；source完成发生在Stopped期间时仍须等resume后才能执行首条用户指令。

Zombie precedence保持source-audit evidence：`proc_state()`在读取jobctl phase前先返回采样到的
`TaskStatus::Zombie`；黑盒SIGKILL后`/proc/<pid>`会先命中R1明确保留的binding / leader-resolution
失败边界，无法形成稳定Z观察窗口，因此不把该窗口误写成production oracle。conditional stop的
global-init immunity也保持source-audit evidence：合法ordinary occurrence不会取得stop authority，
但仍可按Signal语义打断PID 1的wait；当前init应用把`EINTR`当fatal，不能拿生产harness换黑盒
oracle。process-group对global init的发送会同时命中harness成员，global-init opposite-cleanup又无法在不
控制init mask / disposition的情况下建立可确认pending，因此两项保持source-audit evidence，不能为
Stage 4制造危险fixture。reserved旧`SIGCONT -> SIGSTOP -> 新SIGCONT`的完整复合时序没有userspace
可见的reservation-established acknowledgement；不增加kernel hook或声称确定性注入，改用
reservation finality / cleanup / live action / flags / frame failure / SIGKILL的既有split production
case，加source与KUnit evidence闭合R1边界。

同理，userspace没有“Stopping已建立但exposed target尚未trap”的确认；单CPU仍可在两个signal
syscall之间调度target，不能把back-to-back发送伪装成确定性proof。该transition改由上述最小
owner-local KUnit闭合；procfs对Stopping使用底层task state的行为由只在committed Stopped时返回T的
source audit闭合，不引入production hook。

LTP只在`wait.txt`启用`waitid07`、`waitid08`、`waitpid08`、`waitpid13`。四项在glibc与musl各预期
通过一次，wait group由19增至23 case；若既有signal分类不变，最终完整profile预期为每个libc
`60/53/5/2`，合计`120/106/10/4`、`infra_failed=0`。authoritative runtime为repair后candidate
build产生的`build/unix-jobctl-stage4-rv64.log`；验证floor是`just fmt kernel --check`、
`just fmt jobctl-test --check`、`just build`、全部enabled KUnit、全部custom case、上述完整LTP profile、
`git diff --check`、`mdbook build docs`与第12节旁路/Contract Impact evidence audit。LA64只做source
closure，不增加runtime要求。

Stage 4B于2026-07-21关闭。最终repair后candidate由RV64 wrapper重新构建；186个enabled KUnit、
19个`jobctl-test` focused case全部通过，四个新启用wait LTP case在glibc与musl各通过，最终每个
libc为`60/53/5/2`，合计`120/106/10/4`、`infra_failed=0`并正常关机。`just fmt kernel --check`、
`just fmt jobctl-test --check`、wrapper内`just build`、`git diff --check`与`mdbook build docs`通过。
第12节旁路搜索无scheduler / wait-core / compatibility bypass；全部Contract Impact ID和RFC-local
proof obligation已在transaction建立source / KUnit / production evidence索引。

两次过强黑盒oracle和一次不稳定时序注入均按反馈收敛：Zombie precedence、conditional-init /
process-group-init及Stopping procfs projection使用明确source evidence；`Stopping x SIGCONT`由
owner-local KUnit确定性证明；reserved复合时序继续使用split production + source / KUnit evidence，
没有添加kernel hook、second truth或危险harness发送。Stage 4关闭后current contracts、register与
RFC effective status仍不变，`UJ-CUTOVER=None`；Stage 5必须重新解析cutover resolved manifest，
不得从本closure自动进入。

Stage 4B独立closure review发现`repeat-continue`在消费首次Continued前连续发送两次`SIGCONT`，
可能被report / signal coalescing掩盖重复发布。该finding只要求加强既有validation oracle，不改变
candidate production code、target、ABI、visible semantics或acceptance。Stage 4B write set继续包含
`anemone-apps/jobctl-test/src/main.rs`，并为同步唯一canonical状态批准docs-only closure expansion：

- `docs/src/rfcs/unix-jobctl/index.md`
- `docs/src/rfcs.md`
- `docs/src/devlog/2026-07-06_to_2026-07-19.md`
- 本实施计划与对应transaction

扩展只记录首次Continued / SIGCHLD已消费后重复`SIGCONT`不再生成report或signal的review修复、
Stage 4 closed / Stage 5 Not Started及最终验证证据；current contracts、register、R1 target、
`UJ-CUTOVER=None`与Stage 5 resolved manifest均保持不变。修改后已重跑完整RV64 wrapper并正常关机；
186项KUnit、19个focused case与完整LTP floor通过，独立review确认该Euclid已neutralize且未留
Apollyon / Keter / Euclid blocker。format、whitespace、stale-status与mdBook验证作为最终提交前检查。

### 可观测性

- 收口phase、reason、exposed count、phase age、report transition与terminal precedence的诊断清单。
- 证明所有纯诊断字段不参与behavior；便宜的shape / transition invariant使用`assert!`。
- 移除只服务probe的临时log、counter与hook；保留的低频边界日志必须有明确诊断目的。

### 验证

基础命令：

```sh
just fmt kernel --check
just fmt jobctl-test --check
just build
./scripts/run-user-test-rv64.sh etc/sdcard-rv.img build/unix-jobctl-stage4-rv64.log
```

最低case集合：

- `jobctl-test`全部case；
- LTP `waitid07`、`waitid08`；
- wait profile既有exit-only回归，以及启用后仍相关的`waitpid08`、`waitpid13`；
- signal profile现有signal / kill / tgkill / tkill / rt_sigaction / rt_sigprocmask / rt_sigqueueinfo / rt_sigsuspend回归；
- RV64 wrapper完整rootfs、kernel、QEMU链。

LTP不足以证明multi-thread exposure、ordinary-wait preservation、reserved-delivery race与global-init immunity；这些必须由deterministic `jobctl-test`提供production evidence。KUnit如有，只补充局部transition，不替代wrapper。

### 退出条件

- 所有RFC-local proof obligation都有evidence索引；没有未归类failure。
- remaining gaps只可能是已接受并将写入register的limitation，不能是broken target behavior。
- 所有Stage 4 repair checkpoint均已关闭；最终validation evidence对应repair后的candidate commit。
- current contract / register / transaction cutover patch已形成一个原子manifest。
- Stage 4 通过仍不更新effective contract；等待 Stage 5 / `UJ-CUTOVER` 授权。

### Contract Cutover

None。

## 11. Stage 5：`UJ-CUTOVER` integrated effective switch

### 前置条件

- Stage 0、Stage 1、Stage 2、Stage 3A、Stage 3B与Stage 4全部关闭，transaction evidence完整。
- candidate没有old/new双路径、feature flag、singleton fallback、temporary bridge或未删除probe hook。
- Contract Impact中全部`Introduce / Refine / Replace / Scoped Exception`均有验证证据和current contract落点。
- 用户明确授权cutover与发布。

### 交付

- 原子更新受影响current contract：Signal pending/action、temporary-mask handoff、procfs task-state、child wait、ordinary user entry，并创建jobctl新增stable IDs的effective owner文档。
- transaction记录每个contract ID的old/new语义、evidence、effective范围与cutover commit。
- RFC状态、transaction状态、register/current limitations、devlog与导航同步关闭。
- `ANE-20260527-PROCESS-GROUP-SESSION-STAGE1`按实际已关闭范围重分类；TTY、foreground/background pgrp、orphaned-pgrp、ptrace与stopped/continued `si_uid = 0`继续作为明确后续边界，不伪装成已经实现。
- 发布candidate与contract docs作为同一个integrated unit；不存在只有core或只有wait/procfs先行生效的partial cutover。

### 审计

- current contract不残留与effective code矛盾的exit-only、ordinary-entry-only或no-jobctl描述。
- RFC target、transaction evidence与current contract三层角色清晰；不把长执行日志复制到contract。
- public navigation和anchors完整，private path不作为canonical链接。

### Write Set

- `docs/src/rfcs/unix-jobctl/{index,invariants,implementation}.md`，以及提升后实际存在的RFC-local `tracking-issues.md` / `backgrounds/index.md`；只更新状态、cutover结果、evidence入口和公开authority措辞。
- `docs/src/contracts/signal/{pending-routing,temporary-mask-delivery}.md`
- `docs/src/contracts/procfs/task-state-projection.md`
- `docs/src/contracts/task/{process-group-signaling,thread-group-lifecycle,child-wait,user-entry}.md`
- `docs/src/contracts/{signal,procfs,task}/index.md`中受cutover影响的owner覆盖范围与surface导航。
- Stage 5 resolved manifest点名的新job-control current contract文件，以及 `docs/src/contracts.md`。
- 对应transaction文件、`docs/src/devlog/transactions/index.md`和当前双周devlog。
- `docs/src/register/current-limitations.md`；只有最终结果产生broken expected behavior时，才经单独分类允许写`docs/src/register/open-issues.md`。
- `docs/src/rfcs.md`与`docs/src/SUMMARY.md`。
- candidate kernel、apps、LTP profile/group与rootfs/harness默认只读；若最终复核要求任何代码或测试资产变化，Stage 5立即停止，退回逐文件repair checkpoint并重跑受影响floor，不在cutover中顺手修改。

Stage 5开始前，以上默认范围必须展开成逐文件`Resolved Write Set Manifest`，点名新contract
文件、受影响owner index、transaction文件和当前双周devlog。未在manifest中的文档与全部
代码面只读；若自然的contract owner落点不同，先走write-set扩展申请，不能为了服从默认
路径建立错误contract分类。

### Stage 5 Resolved Write Set Manifest（2026-07-21）

Stage 5 transition preflight重新读取最终Stage 4 candidate commit `9d0308a7`、R1 target、Contract
Impact、Stage 4 proof/evidence index、current contracts、register与public navigation。candidate
production code和validation assets相对该commit无变化；未发现old/new双路径、feature flag、
singleton fallback、temporary bridge或probe hook。用户已明确授权完成Stage 5并免除重复final
runtime；本manifest只授权下列21个公共文档文件：

- `docs/src/rfcs/unix-jobctl/index.md`
- `docs/src/rfcs/unix-jobctl/invariants.md`
- `docs/src/rfcs/unix-jobctl/implementation.md`
- `docs/src/contracts/signal/index.md`
- `docs/src/contracts/signal/pending-routing.md`
- `docs/src/contracts/signal/temporary-mask-delivery.md`
- `docs/src/contracts/procfs/index.md`
- `docs/src/contracts/procfs/task-state-projection.md`
- `docs/src/contracts/task/index.md`
- `docs/src/contracts/task/process-group-signaling.md`
- `docs/src/contracts/task/thread-group-lifecycle.md`
- `docs/src/contracts/task/child-wait.md`
- `docs/src/contracts/task/user-entry.md`
- `docs/src/contracts/task/job-control.md`（新增；`ThreadGroup` job-control owner）
- `docs/src/contracts.md`
- `docs/src/devlog/transactions/2026-07-20-unix-jobctl.md`
- `docs/src/devlog/transactions/index.md`
- `docs/src/devlog/2026-07-06_to_2026-07-19.md`
- `docs/src/register/current-limitations.md`
- `docs/src/rfcs.md`
- `docs/src/SUMMARY.md`

全部kernel、apps、LTP profile/group、rootfs/harness、private draft、RFC backgrounds、open issues及
其它公共文档保持只读。新`JOBCTL-*`落在`contracts/task/job-control.md`，因为唯一协议owner是user
`ThreadGroup`；不新建并列top-level owner目录。若contract review要求修改任何代码/测试资产，或
发现target/owner/ABI/visible semantics/acceptance变化，Stage 5立即停止并保持Not Cut Over。

在manifest冻结前曾提前形成Signal/procfs/task contract未提交草稿；这些文本不构成已授权执行或
partial cutover。发现该流程Keter后已停止继续写contract，先完成本manifest与transaction activation
记录；随后只把manifest内草稿作为候选diff重新review、修正和验证。最终commit之前任何current
contract仍以HEAD旧文本为effective authority。

### 验证

- 复核 Stage 4 evidence来自最终candidate commit；代码变化后必须重跑受影响floor。
- `git diff --check`
- `mdbook build docs`
- final `just fmt kernel --check`、`just fmt jobctl-test --check`、`just build`。
- final runtime原计划使用
  `./scripts/run-user-test-rv64.sh etc/sdcard-rv.img build/unix-jobctl-stage5-rv64.log`。用户因
  Stage 5不改代码、Stage 4已经从最终candidate重新构建并跑完整floor而明确豁免本次重复runtime；
  本Stage复用`build/unix-jobctl-stage4-rv64.log`并核对candidate `9d0308a7` / artifact provenance，
  Stage 5 runtime明确记为user-waived / Not Run，不伪装成新运行证据。

### 退出条件

- 所有target contract ID为Effective或明确Not Cut Over；不得存在模糊pending状态。
- RFC Closed、transaction Completed、register状态与current contract来源一致。
- 没有要求自动进入TTY、orphaned-pgrp或ptrace follow-up。

### Contract Cutover

整个`UJ-CUTOVER`原子生效。任一参与domain失败则全部保持Not Cut Over，不允许部分ID先行effective。

### Stage 5结果（2026-07-21）

21文件resolved manifest已完成，current contract、RFC、transaction、register、双周devlog与导航在
同一个`jobctl: complete Stage 5 atomic cutover`提交中发布。全部`Introduce / Refine / Replace /
Scoped Exception` ID均有Active current-contract落点；Preserve ID的owner与语义保持。最终review为
Apollyon 0、Keter 0、Euclid 0，没有代码/测试资产、owner、public API、ABI、visible semantics、
acceptance或write-set扩张。

`git diff --check`、`mdbook build docs`、stale wording/link/anchor audit、
`just fmt kernel --check`、`just fmt jobctl-test --check`与final `just build`通过。首次sandbox build的
lwext4 C compiler `Bad system call`属于seccomp环境阻断；用户明确批准后在沙箱外重跑同一`just build`成功。
Stage 5没有修改production code，runtime按用户明确豁免记为user-waived / Not Run；最终runtime证据仍
由Stage 4 candidate `9d0308a7`及`build/unix-jobctl-stage4-rv64.log`拥有。

Stage 5与整个R1计划关闭，`UJ-CUTOVER` Effective。TTY、orphaned-pgrp、ptrace、credential
`si_uid`与SIGCHLD publication ordering只作为register/follow-up边界保留，不自动进入下一gate。

## 12. 旁路审计清单

每个 semantic Stage 都要重跑并在transaction记录差异：

- Signal ingress：`Task::recv_signal`、`ThreadGroup::recv_signal`、kill / tkill / tgkill / rt_sigqueueinfo、kernel-generated sync/fault signal与OOM SIGKILL。
- Signal source：所有`sig_pending` direct access、private/shared fetch、specific fetch、flush、reservation、pending snapshot与disposition reset。
- Temporary mask：token begin / restore / defer、classifier、reserved delivery、handler commit、no-frame cleanup与rt_sigreturn。
- User entry：两架构trap entry、ordinary trap return、fresh user task、clone child、exec new image、所有raw architecture transition与`on_prv_change(Privilege::User)`。
- Membership / lifecycle：ThreadGroup construction、member insert/remove/snapshot、clone join、dethread、ordinary detach、last-member exit、exit_group、SIGKILL与reparent。
- Wait/report：`child_exited`或后继Event的listen/publish、wait scanner、WNOWAIT、reap、wait4 / waitid serializer与SIGCHLD producer。
- Procfs：stat / status state character / name、leader/Zombie precedence与binding failure。
- Scheduler / wait-core：任何新增jobctl enum、stop flag、force wake、WaitState分支、WakeToken消费或generic admission均为默认违规。
- Compatibility：任何feature flag、group-size branch、old/new fallback、temporary queue、carrier、manager或worker obligation都必须有明确删除条件；本RFC默认不接受。

允许保留的旁路必须满足：不执行user transition、不参与user ThreadGroup jobctl、或是点名的kernel-thread/architecture HAL叶子，并在transaction写清理由。

## 13. 可观测性清单

- ThreadGroup diagnostic snapshot：TGID、Running / Stopping / Stopped、first stop reason、exposed count、phase age。
- phase transition：old/new phase、trigger class、是否形成Stopped / Continued report。
- user-entry gate：park、wake后重新仲裁与terminal dominance；不在hot path持续打印。
- control generation：global-init admission、SIGSTOP no-pending、SIGCONT generation-only side effect与stale epoch rejection。
- report：commit / replace / exact-once consume / exit-clear / reparent-wake；`WNOWAIT` peek由同slot
  source与production evidence证明，不在owner锁域逐次打印无界日志；Event只显示predicate notification。
- assertions：user/kthread shape、membership/exposure一致性、Stopped时exposed为空、terminal时report/park cleanup、report reason来自同一owner snapshot。
- 纯诊断member identity、timestamp或counter必须在字段旁说明不参与behavior；任何后续行为依赖都要先提升为协议状态并回RFC review。

## 14. 全局停止边界

以下任一情况发生，当前 Stage 停止，不通过缩小目标、降低测试或增加临时hack继续：

- 需要scheduler-owned stop state、runqueue suppression、generic wait admission或ordinary-wait cancellation；
- 任何user transition无法进入mandatory gate，或parent可观察Stopped后仍可能执行未登记user instruction；
- control generation无法在`exact identity / topology -> ThreadGroup owner -> one Signal leaf`方向闭合；
- 需要persistent Signal carrier、second pending queue、participant ledger、ReportId或allocation-backedepoch来维持正确性；
- global init可能取得SIGSTOP或conditional DefaultStop authority；
- reserved delivery无法在不丢失temporary-mask责任、不重放SIGCONT side effect的前提下收口；
- lifecycle、Signal、wait、procfs或scheduler形成第二份phase / report / terminal truth；
- report claim无法在parent relation -> child owner顺序重验selector与current slot；
- stopped / continued ABI要求缓存leader credential或猜测任意member UID；
- probe结果要求改变owner、target invariant、ABI、accepted limitation或`UJ-CUTOVER`原子边界。

反馈归属：

- 执行事实、checkpoint、review与runtime evidence写transaction；
- Stage 顺序、write set、validation floor或stop condition变化更新本文与transaction；
- owner、target invariant、ABI、visible semantics或acceptance boundary变化回`index.md` / `invariants.md`与tracking issue；
- broken expected behavior进入open issue，accepted gap进入current limitations；
- effective contract只在`UJ-CUTOVER`更新。

## 15. 实现期反馈记录

- 2026-07-20：Stage 0 source split 与验证完成；未产生 target、owner、ABI、停止条件或
  write set 扩展反馈。sandbox build 的 `Bad system call` 是环境限制，已记录于 transaction
  并通过批准的 repository build 重试解决。
- 2026-07-21：Stage 1 将最终 user-entry gate 放在 FPU restore 之前，gate 返回后再恢复
  architecture-local FPU state并发布 `Privilege::User`；这样 future park不会跨 context switch
  携带已经恢复的 FPU ownership。该调整保持 accepted target、ABI、owner与验证 floor不变。
- 2026-07-21：`ThreadGroupInner::members` 的 owner-local wrapper覆盖全部 construct / join /
  detach / dethread caller；exposure transition与user/kthread shape使用 release assertion，未引入
  task-local flag、participant set、behavioral counter或 scheduler state。
- 2026-07-21：Stage 3A review确认R0 `USER-ENTRY-002`要求reserved target不得阻塞later pending
  `SIGKILL`，与current `SIGNAL-TEMP-MASK-002`的reservation-first顺序冲突。用户接受该既有Signal
  递送延迟并拒绝为本RFC扩大jobctl owner；R1删除该额外强化，保持Stopped gate、terminal
  lifecycle precedence与SIGKILL pending ownership。实现代码不变，current contract与
  `UJ-CUTOVER=None`不变。

## 16. Write Set 扩展记录

- 2026-07-21：Stage 1 closure 增加 `docs/src/rfcs/unix-jobctl/index.md`、`docs/src/rfcs.md`
  与 `docs/src/devlog/2026-07-06_to_2026-07-19.md`，仅同步 Stage 1 closed / Stage 2 Not Started
  状态和验证摘要。该 docs-only 扩展不修改 R0 target、invariants、current contract、register、
  ABI、visible semantics或 `UJ-CUTOVER=None`；通过 stale wording / link audit 与
  `mdbook build docs` 验证。
- 2026-07-21：Stage 2 preflight确认typed siginfo owner位于
  `anemone-kernel/src/task/sig/info.rs`。用户批准将该文件纳入write set，并允许在表示能力确有
  缺口时纳入`anemone-abi`。live ABI已定义`CLD_STOPPED = 5`、`CLD_CONTINUED = 6`与
  `SA_NOCLDSTOP`，因此resolved manifest只扩展`info.rs`，不产生`anemone-abi` diff。该扩展只
  补齐既定R0 ABI结果的typed internal representation，不改变target、owner、visible semantics、
  acceptance、current contract或`UJ-CUTOVER=None`。
- 2026-07-21：首次RV64 integrated run中focused `jobctl-test`全部通过，但它在boot root自行挂载
  procfs后，competition environment再次建立procfs mount，触发当前procfs只可靠支持单次挂载的
  已知限制。用户批准把`anemone-apps/user-test/src/guest.rs`纳入write set：只在chroot前复制
  `jobctl-test`，随后复用competition environment的唯一procfs mount并在LTP前运行；test app不再
  自行挂载。该route correction不修改procfs、target、owner、ABI、visible semantics、acceptance、
  current contract或`UJ-CUTOVER=None`；验证要求RV64 wrapper运行至正常关机。
- 2026-07-21：独立Stage 2 review发现process-group snapshot与control generation之间并发
  `setpgid`时，已经离组的ThreadGroup仍可能收到cleanup与stop / continue side effect；公开
  `ProcessGroup::recv_signal()`也保留同类旁路。用户批准把
  `anemone-kernel/src/task/topology/process_group.rs`纳入write set，只让control occurrence携带
  snapshot PGID并在ThreadGroup owner下、任何副作用前重验。ordinary signal继续使用既有snapshot
  语义，ProcessGroup仍只拥有selector；不改变public API、target、owner、ABI、visible semantics、
  acceptance、current contract或`UJ-CUTOVER=None`。同轮review确认`rt_sigqueueinfo`必须把“共享
  ordinary occurrence route”与“exact resolved member authority”分开保存，修复位于既有manifest。
- 2026-07-21：Stage 2 final owner review确认，原jobctl gate在park wake后直接登记exposure，
  没有回到Signal / lifecycle arbitration；这会让custom `SIGCONT` handler延迟到下一次trap，
  也会让`SIGKILL`或已经提交的terminal lifecycle无法支配user entry。用户批准把
  `anemone-kernel/src/arch/{riscv64,loongarch64}/exception/trap/utrap.rs`纳入write set，只将
  ordinary return与fresh / clone / exec共同final transition接入同一个Signal-owned重新仲裁
  loop。该扩展不修改trap ABI、exception policy、FPU owner、scheduler、target、visible
  semantics、acceptance或`UJ-CUTOVER=None`；LA64只做source / format closure，runtime floor仍
  仅为RV64 wrapper。
- 2026-07-21：Stage 2 closure增加`docs/src/rfcs/unix-jobctl/index.md`、`docs/src/rfcs.md`与
  `docs/src/devlog/2026-07-06_to_2026-07-19.md`，只同步Stage 2 closed、Stage 3A Not Started、
  已有验证/review证据和non-publishable边界。该docs-only扩展不修改R0、invariants、current
  contract、ABI、visible semantics、acceptance或`UJ-CUTOVER=None`；验证为stale wording / link
  audit、`git diff --check`与`mdbook build docs`。
- 2026-07-21：用户批准Stage 3A以R1接受current reservation-first SIGKILL延迟；为原地修订accepted
  target，将`docs/src/rfcs/unix-jobctl/invariants.md`加入3A docs write set，并同步RFC index、
  implementation、transaction、RFC导航与双周devlog。该扩展不修改内核代码、current contract或
  `UJ-CUTOVER=None`。
- 2026-07-21：Stage 3B首次RV64 focused run确认multi-thread exec因既有dethread向siblings发送
  普通kernel `SIGKILL`而误入group exit。用户此前已授权顺手纠正相邻wrong-signal usage，且明确
  当前RFC不为旧问题承担target责任。初始`dethread_owner`方案经review发现可能让victim claim并
  吞掉process-directed shared SIGKILL，未保留。最终将
  `anemone-kernel/src/task/sig/{mod,pending,delivery}.rs`加入manifest，只为dethread producer的
  task-private SIGKILL携带kernel-private victim purpose并在delivery折为per-task
  `kernel_exit(0)`；pending owner保证它不能合并覆盖ordinary task-directed SIGKILL，且reserved
  victim在退出前重验later ordinary SIGKILL。外部/shared SIGKILL保持既有group terminal action。
  该修复不修改ABI、public API、jobctl target、current contract或`UJ-CUTOVER=None`；验证要求
  multi-thread exec child退出0、multi-member SIGKILL仍退出9。

## 17. 结构维护记录

- 2026-07-20：计划在 Stage 0 把当前混合mask、pending、generation、delivery与trap-return职责的`task::sig`根模块拆为`mask.rs`、`pending.rs`、`generation.rs`与`delivery.rs`；保持现有public/crate API、lock order、ABI与runtime行为不变。执行证据将在transaction中记录。
- 2026-07-21：按用户反馈将既有 `task/api/jobctl` 物理迁移到 `task/jobctl/api`。四个
  syscall实现文件逐字保持，registration各自唯一；仅 module declaration与领域归属变化，
  不改变 syscall ABI、policy、可见语义或 public API。
