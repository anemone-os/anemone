# Epoll 实施计划

**状态：** Active / Stage 0
**适用修订：** R0
**最后更新：** 2026-07-26
**父 RFC：** [RFC-20260726-epoll](./index.md)
**目标不变量：** [Epoll 与 Poll Subscription 不变量需求](./invariants.md)
**当前契约：** [`SCHED-LATCH-*`](../../contracts/scheduler/latch-wait-round.md)、[`SIGNAL-TEMP-MASK-*`](../../contracts/signal/temporary-mask-delivery.md)、[`IOMUX-POLL-*`](../../contracts/iomux/poll-wait.md)、[`OPENED-DESC-*`](../../contracts/task/opened-description-lifecycle.md)、[`TTY-TERM-001` / `TTY-INPUT-001`](../../contracts/tty/data-plane.md)
**开放问题：** [Tracking Issues](./tracking-issues.md) 当前无开放 Keter
**事务日志：** [2026-07-26-epoll](../../devlog/transactions/2026-07-26-epoll.md)

本文把公共 R0 中已经闭合的 accepted target 解析成滚动实施路线。Stage 0 已解析为
Ready，并在 R0 acceptance、transaction bootstrap 与开发者明确授权后进入 Active。
初始授权覆盖 0A、0B；后续授权覆盖 0C、0D。前一 checkpoint 的交付、review 与验证关闭后
才能进入后一项，且 Stage 0 closure 不会自动授权后续 Stage。

## 实施原则

- 只设置三个阶段和两个阶段解析 gate。Stage 0 在同一 feasibility boundary 内按 0A-0D
  checkpoint 顺序执行；checkpoint 只提供实现、review、验证和可恢复停止边界，不形成新的
  Stage、contract cutover 或 partial feature。build、KUnit、源码审计、userspace/LTP 与 review
  仍是 Stage closure 的证据，不再各自拆成形式化子 gate。
- Stage 0 先证明最危险的 owner boundary，不执行 contract cutover，也不能作为可单独合入的
  partial feature。Stage 1 原子完成 source subscription 与 poll/select cutover；Stage 2
  一次完成 epoll core、ABI 和 `EPOLL-CUTOVER`。
- KUnit 只覆盖用户态难以稳定命中的、可通过真实 production transition 确定性证明的内核
  不变量。纯 getter/setter、同义 helper、test-only 状态机副本、逐 source 模板测试和只证明
  “代码执行过”的用例不进入计划。
- ABI、errno、LT/ET/ONESHOT、dup/close/fd reuse、temporary signal mask 与 copyout 主要由
  focused userspace 和 LTP 证明；KUnit 不伪装成 ABI 验收。
- 每个阶段先独立关闭，再解析下一阶段。解析 gate 只在阶段边界发生；普通 review 反馈在
  当前 closure 内修正，不另建 gate。
- `Target Renegotiation Gate` 只在命中本文停止条件时触发，不是例行阶段。实现者不得用
  per-source escape hatch、日志兼容或弱化验证绕过 target。

## 全局受保护边界

- `SCHED-LATCH-001..003` 保持有效；persistent subscription 不进入 scheduler，source 不保存
  `Task`、`WakeToken` 或 runqueue capability。
- `SIGNAL-TEMP-MASK-001..003` 保持有效；`IomuxWaitRound` 不取得 current mask、reserved
  delivery target 或 restore responsibility，ppoll/pselect 继续由现有 Signal classifier 收口。
- readiness truth 与 routing storage 始终由具体 source state 拥有；consumer acceptance、
  watch generation、policy 与 ready protocol 不进入 source。
- source lock 内只更新 readiness、安装/筛选 route；observer notification、wait completion、
  epoll operation 和用户 copyout 都发生在 source lock 外。
- source route 不拥有 consumer lifetime；consumer retirement 后的晚到 notification 必须
  内存安全并 fail closed。eager unlink / lazy pruning 只承担有界资源卫生。
- opened-description identity、published ref 与 terminal retirement 只由 `task::files` 拥有；
  epoll 不缓存第二份 alive truth，也不让 final close 同步进入 epoll。
- 每个 `Epoll` 只有一个 sleepable operation mutex 串行 ctl、teardown 与 harvest；source
  notification、IRQ/noirq handoff 和 task sleep 不获取它。
- ready entry 与 callback 只表示 recheck obligation；交付前重新读取 target predicate，并在
  commit 前验证 opened-description liveness。
- Linux UAPI 只位于 `anemone_abi` 与 `fs::api`。`fs::epoll` 不读写用户指针，不保存
  Linux-shaped event/flag state。
- 首版拒绝 nested epoll。对 epoll target 的 `EPOLL_CTL_ADD` 固定返回 `EINVAL` 并记录
  notice；不为通过 Linux nesting 用例暗中开放 cycle/depth 路径。
- `TTY-TERM-001` 与 `TTY-INPUT-001` 保持有效；TTY representative slice 只替换 poll
  routing capability，不复制 Terminal readiness/input truth，也不改变 record boundary。

## 阶段路线图

| 阶段 | 成熟度 | 目的 | contract cutover | 解析触发点 |
| --- | --- | --- | --- | --- |
| Stage 0 | Active | 用 production-shaped vertical slice 证明 observer route、consumer retirement、terminal liveness 与三类 source context 可以共存 | None | Ready 解析与 activation preflight 已完成 |
| Stage 1 | Outline | 迁移全部 pollable source 与 poll/select，删除 `LatchTrigger` source bridge，并原子切换 subscription / opened-description contract | `SUBSCRIPTION-CUTOVER`、`OPENED-DESC-CAPABILITY-CUTOVER` | Stage 0 独立关闭后 |
| Stage 2 | Outline | 实现 epoll core、anonymous file、syscall ABI、focused tests 与 LTP，完成首版 epoll cutover | `EPOLL-CUTOVER` | Stage 1 独立关闭后 |

## Stage 0 Ready：Subscription 与 Liveness Proof-First Slice

### 前置条件

以下前置条件已于 2026-07-26 满足，证据见
[事务日志](../../devlog/transactions/2026-07-26-epoll.md)；它们不替代各 checkpoint 自己的
关闭条件：

- 公共 Draft 已完成文档层 review 并被接受为 R0；accepted target、Contract Impact 与本文三阶段
  路线没有语义变化。
- 已建立引用该 RFC 修订的新 transaction；transaction 只链接本文，不复制 Ready 定义。
- 启动前重新核对 live branch、HEAD、dirty state、current contracts 与下列代码 owner；若
  source layout 或 `FileOps::poll` / fd publication surface 已漂移，先更新本文 manifest，
  不在 worker 中猜测兼容层。
- 开发者明确授权 Stage 0 从 Ready 进入 Active。RFC acceptance 本身不构成启动授权。

### 阶段内 checkpoint 规则

- Stage 0 是 observer route、consumer retirement、三类 source context 与 terminal liveness
  共同可行性的闭合单元；0A-0D 只是同一 Stage 内的顺序执行边界。任何单个 checkpoint
  关闭都不能写成 Stage Closed、contract 已 cut over 或 epoll capability 已形成。
- Stage 0 activation 不自动越过 checkpoint gate。前一个 checkpoint 的交付、定向验证、review
  与恢复处置闭合后，才能按既有授权协议进入下一个；transaction 只追加实际 activation、结果和
  证据，不复制本文的计划 authority。
- 每个 checkpoint 只能修改下文冻结的 write subset。若 finding 需要回改前一个 subset，重新打开
  对应 checkpoint；若需要 Stage manifest 外文件，先按 write-set expansion 流程更新本文并记录批准。
- 0A-0C 的 build 证明 production source 与 KUnit test body 可编译，但不冒充 KUnit 已运行；全部
  enabled KUnit、focused iomux userspace/LTP 与正常关机只在 0D 的 Stage closure wrapper 中统一
  运行。最终 runtime 暴露早期 checkpoint 问题时，重新打开对应 checkpoint 并重跑受影响证据。

| Checkpoint | 目的 | 前置依赖 | 主要 write subset | 关闭边界 |
| --- | --- | --- | --- | --- |
| 0A | fail-fast 证明 opened-description terminal liveness | Stage 0 Active | `task/files.rs` | capability、publication audit、KUnit 编译与 lifecycle review 闭合 |
| 0B | 建立 observer core、`IomuxWaitRound` 与普通 pipe vertical slice | 0A Closed | `fs/iomux*`、`fs/api/iomux/{wait,ppoll,pselect6}.rs`、`fs/pipe.rs` | late notification、ready-at-subscribe、final-scan 与 bridge review 闭合 |
| 0C | 证明 timerfd noirq / fixed-capacity route | 0B Closed | `fs/timerfd.rs` | noirq allocation、guard-out notify/drop、容量回收与 owner KUnit 编译闭合 |
| 0D | 证明 TTY 预分配 dirty handoff 并完成 Stage closure | 0C Closed | `device/tty/terminal.rs` | TTY review、全量 audit、一次 RV64 wrapper、完整 diff review 与 Stage write-back 闭合 |

### 要证明的假设

1. 一个 source-neutral、non-owning observer route 可以由 `IomuxWaitRound` 持有 consumer
   lifetime，而 source registry 只保存通知能力；晚到或已选择的 callback 在 round retire
   后能够 fail closed，无需同步 drain。
2. 同一 subscribe 事务可以在 route 已安装的前提下返回 current readiness；ready-at-subscribe
   不会把 persistent route 降级成一次性 trigger。
3. 同一 consumer protocol 能分别容纳：
   - pipe 的普通 task-context、动态 `Vec` registry；
   - timerfd 的 `NoIrqSpinLock`、固定容量和 guard-out batch；
   - TTY 的预分配 poll storage / spare handoff。
   三者可以保留不同 owner-local container 与 handoff，而不泄漏 `LatchTrigger` 或 consumer
   private type。
4. `task::files` 能提供 opaque、non-owning identity/liveness capability 与 operation-local
   live lease；最后一个 published ref 退休后旧 capability 不可复活，且不需要 dynamic
   final-release observer registry。

### 交付

#### Iomux consumer owner 与窄 route surface

- 在 `fs::iomux` owner 下增加 `subscription` 与 `wait` 子模块。`subscription` 定义内部
  `PollObserver` capability、
  non-owning route 与 subscribe result。具体 Rust 表示由该模块封装；source 只能 clone/store
  route、判断是否可 prune、以及在锁外发布 no-return notification，不能取得 consumer、
  `Latch` 或 epoll private state。
- `fs::iomux::wait::IomuxWaitRound` 成为 register scan 的 linear owner：持有本轮
  `Latch`/trigger、observer lifetime 与 callback acceptance；每个 begin 必须 exactly-once
  retire + finish。当前 `fs::api::iomux::wait` 继续拥有 temporary signal mask、errno/outcome
  mapping 与 syscall orchestration，不把 Linux-facing policy 下沉到 core wait owner。
- `PollRequest` 在 Stage 0 暂时同时携带新 observer route 与旧 `LatchTrigger` bridge：三类
  representative source 只消费新 route，其余 source 继续走 current effective bridge。
  bridge 必须有明确注释说明只活到 Stage 1 `SUBSCRIPTION-CUTOVER`，不得新增调用者。
- register scan 无论 current predicate 是否 ready，都先完成 source-local route publication
  与 snapshot；consumer 在 ready/error/timeout/signal 返回前 retire acceptance，随后按现有
  latch contract finish 并执行 final snapshot。

#### 三类 representative source

- `pipe.rs`：只替换 poll routing entry；blocking read/write 与 pipe buffer/readiness owner 不变。
  subscribe 前 prune stale route，锁内安装并 snapshot，锁外通知；ordinary dynamic allocation
  失败必须在 route publication 前返回。
- `timerfd.rs`：只替换 poll route，blocking read trigger 不在本阶段迁移。poll route 继续使用
  预留容量；prune/detach 在 noirq guard 内只移动到 caller-owned batch，route drop 与 notify
  在 guard 外发生。容量耗尽保持可观察的 register failure，不能在 noirq path 分配补救。
- `terminal.rs`：复用现有预分配 `poll_triggers` / `poll_spare` handoff 的容量与 guard-out
  约束，只改变 entry 内的 consumer capability。不得把预分配 TTY 路径改造成通用动态
  registry，也不得在 terminal guard 下 drop 最后引用或通知 observer。
- 三个 source 都必须在 current-ready 时保留 route；一次 notification 不自动删除 route。
  current poll/select round retire 后，stale route 由下一次 subscribe/notification/owner cleanup
  有界清理。

#### Opened-description capability probe

- `task::files` 在 `ProcFile` owner 内表达 terminal lifecycle；published-ref 仍是唯一 ref truth，
  capability 不暴露 `ProcFile`、refcount、fd-table lock 或底层 `File` 存活细节。
- capability 只允许 identity equality、尝试取得当前 operation 的短 live lease、以及 commit 前
  重新验证同一 identity 仍未退休。lease 不能逃出 operation，也不能被 watch 长期保存。
- 全量审计 `open_fd*`、`FdReservation::commit`、dup/dup3、fork/CLONE_FILES、unshare、
  close/close_range、close-on-exec 与 exit cleanup。任何可能在 terminal `1 -> 0` 后重新
  publication 的路径必须通过 owner API 排除，并用常开 `assert!` 守住内部不变量。
- 保留 `FileDescOps::final_release` 的单静态 hook 语义；本阶段不增加 observer list，也不让
  capability 的存在触发 epoll callback。

### KUnit 边界

本阶段最多形成以下三项 focused evidence，不按 source 数量复制模板：

1. 改写 TTY 既有 `poll_register_before_after_notification_and_stale_cleanup`，让它经过真实
   observer route，覆盖 register-before-notify、ready-at-subscribe 仍安装 route，以及 round
   retire 后的 late notification/stale cleanup。它是既有测试迁移，不新增一份平行测试。
2. timerfd 增加一项 owner-local KUnit，直接覆盖 fixed-capacity route 在 noirq guard 内被选择、
   guard 外 notify/drop，以及 stale route 回收后容量可再用。若只能通过 test-only callback
   通道或复制一份 batch 状态机才能测试，则不写该 KUnit，改由 source audit 记录证明边界。
3. `task::files` 增加一项 owner-local KUnit，使用真实 publication/ref owner 覆盖两个 alias
   尚存时 capability 可取得 live lease、最后一个 published ref 退休后旧 capability 无法再
   取得 lease。不可测试的 illegal revival 由 production `assert!` 与 publication caller audit
   证明，不为捕获 panic 增加 test hook。

不为 pipe 增加 entry push/pop 单元测试；pipe 的真实 register/wake/final-scan 由 focused
iomux userspace 路径覆盖。不为 route getter、identity equality 或 debug id 单独写测试。

### 审计与可观测性

- 对所有 `PollRequest`、`PollRegisterResult`、`LatchTrigger` 与 `poll_triggers` caller 建立
  分类表，确认只有三类 representative source 进入新路径，其余仍处于明确 migration bridge。
- 审计三类 source 的每个 predicate-changing transition，确认 source lock 内只选择 route，
  notify/drop 在 guard 外；timerfd/TTY 路径不得引入 IRQ-off allocation 或 sleepable lock。
- observer acceptance、route prune 与 terminal liveness 依赖常开局部 `assert!`。日志只记录
  capacity exhaustion、unsupported source 或 impossible transition；diagnostic id 不参与行为。
- Stage 0 transaction 记录 probe 采用的具体 route/lifecycle encoding、每类 source 的资源
  上界、KUnit 实际保留/放弃原因，以及成功后代码由 Stage 1 吸收还是失败后删除。

### Checkpoint 0A - Opened-description Terminal Liveness

**执行状态：** Closed / 2026-07-26。实现与验证证据见
[transaction checkpoint log](../../devlog/transactions/2026-07-26-epoll.md#checkpoint-0a---opened-description-terminal-liveness---2026-07-26)；
本项关闭只解除 0B 的前置依赖，不表示 Stage 0 Closed 或任何 contract cutover。

**交付与 write subset：** 只修改 `anemone-kernel/src/task/files.rs`、本文与对应 transaction。
在 `ProcFile` owner 内实现 opaque identity/liveness capability、operation-local live lease 与 terminal
retirement；全量审计 publication/release caller，并增加前述 owner-local KUnit。不得修改
`FileDescOps::final_release` 的单静态 hook 语义或增加 dynamic observer。

**定向验证 / review：** publication/release source audit；确认旧 capability 在 terminal `1 -> 0`
后不能再次取得 lease，dup/fork alias 尚存时仍为 live；运行 `just fmt kernel --check`、
`git diff --check` 与
`just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`。build 只证明 KUnit 编译；
实际 KUnit 结果由 0D closure wrapper 给出。完成一次 task/files lifecycle review。

**停止 / 恢复：** capability 若允许 retirement 后复活、必须长期强持 target、要求 final close 主动进入
epoll，或只能用 dynamic final-release observer 保持 target semantics，停止 Stage 并进入 Target
Renegotiation Gate。局部 encoding 失败但 target 不变时，只回退到 0A 入口，记录 Route Correction
后重新执行 0A；不得带着未闭合 liveness API 进入 0B。

### Checkpoint 0B - Observer Core、IomuxWaitRound 与 Pipe Slice

**执行状态：** Closed / 2026-07-26。实现、source classification、concurrency review 与验证证据见
[transaction checkpoint log](../../devlog/transactions/2026-07-26-epoll.md#checkpoint-0b---observer-coreiomuxwaitround-与-pipe-slice---2026-07-26)；
Stage 0 仍为 Active，0C Not Started 且未获本轮授权。

**交付与 write subset：** 将 iomux owner 目录化为
`anemone-kernel/src/fs/iomux/{mod,subscription,wait}.rs`，修改
`anemone-kernel/src/fs/api/iomux/{wait,ppoll,pselect6}.rs`、`anemone-kernel/src/fs/pipe.rs`、本文与
对应 transaction。建立 source-neutral route、round-owned observer acceptance、ready-at-subscribe
结果与 Stage 1 删除的旧 `LatchTrigger` bridge；只迁移 pipe 这一 ordinary dynamic-registry source。

**定向验证 / review：** 分类全部 `PollRequest` / `PollRegisterResult` caller，确认只有 pipe 进入新
source path、其余 source 仍通过有删除条件的 bridge；审计 pipe predicate update、route publication、
锁外 notify/drop、late notification fail-closed 与 ppoll/pselect final-scan。运行与 0A 相同的 format、
diff 和显式 RV64 build floor，并完成一次 iomux/pipe concurrency review。

**停止 / 恢复：** route 若需要拥有 consumer、同步 drain、feature-specific downcast、source lock 内
wait completion，或 ready-at-subscribe 无法同时保留 persistent route，停止 Stage 并进入 Target
Renegotiation Gate。普通实现失败只回退 0B subset；0A 保持 Closed，但不能据此解析或激活 Stage 1。

### Checkpoint 0C - Timerfd Noirq / Fixed-capacity Route

**执行状态：** Closed / 2026-07-26。实现、noirq/resource review、KUnit 取舍与验证证据见
[transaction checkpoint log](../../devlog/transactions/2026-07-26-epoll.md#checkpoint-0c---timerfd-noirq--fixed-capacity-route---2026-07-26)；
本项关闭只解除 0D 的前置依赖，不表示 Stage 0 Closed 或任何 contract cutover。

**交付与 write subset：** 只修改 `anemone-kernel/src/fs/timerfd.rs`、本文与对应 transaction。
把 timerfd poll route 迁移到 0B protocol，保留 blocking-read trigger；保持 fixed-capacity storage，
在 noirq guard 内把 live route clone 到 caller-owned notify batch 并保留在 registry，只把 stale route
移入 drop batch，随后在 guard 外 notify/drop。按前述条件保留或放弃 owner-local KUnit，transaction
必须记录理由。

**定向验证 / review：** 审计 timerfd 每个 predicate-changing transition、容量耗尽和 stale route
回收；确认无 IRQ-off allocation、sleepable lock、最后引用 drop 或 observer notification。运行与
0A 相同的 format、diff 和显式 RV64 build floor，并完成一次 timerfd noirq/resource review；KUnit
runtime 仍由 0D closure wrapper 给出。

**停止 / 恢复：** 若共同 protocol 要求 noirq allocation、unbounded overflow queue、guard 内
notify/drop 或削弱容量耗尽的可观察失败，停止 Stage 并进入 Target Renegotiation Gate。局部问题只
回退 0C；0A/0B 保持 Closed，0D 不得开始。

### Checkpoint 0D - TTY Preallocated Handoff 与 Stage Closure

**执行状态：** Authorized / Not Started。0C 已独立关闭；0D 仍须在自己的 write subset、review、
runtime closure 与 write-back 全部闭合后才能关闭 Stage 0。

**0B correction 前置项：** 0D 的真实-route KUnit preflight 发现 `IomuxWaitRound` 与 private
`fs::iomux` façade 只允许 `crate::fs` 命名，`device::tty` 因而既不能保存 production `PollRoute`，
也不能让既有 KUnit 经真实 round 构造 route。开发者已批准把 `fs/mod.rs` 加入 Stage manifest，并
最小重开 0B subset 修改 `fs/iomux/{mod,wait}.rs`：只精确导出 crate-internal `PollRoute`，且只在
`kunit` 构建向 TTY 暴露 production `IomuxWaitRound`；observer、route constructor 与
`register_with_route` 继续保持原 owner-private。该 correction 必须独立验证、review、提交并重新关闭
0B，随后 0D code subset 仍只修改 `terminal.rs`。

**交付与 write subset：** 只修改 `anemone-kernel/src/device/tty/terminal.rs`、本文与对应 transaction。
把 TTY poll entry 迁移到 0B protocol，保持 `poll_triggers` / `poll_spare` 的预分配容量、
`poll_handoff_active` / `poll_dirty` 重入 handoff 与 guard-out notify/drop；改写既有 focused KUnit，
不增加平行状态机。随后完成 Stage 级 caller/source/publication audit 与 closure write-back。

**定向验证 / review：** 先审计 TTY register、dirty handoff、容量耗尽和每个 readiness transition，
确认 terminal guard 下不分配、不 drop 最后 route 引用、不通知 observer；运行 format/diff 和显式
RV64 build floor。再执行下文唯一一次 Stage closure wrapper，使 0A/0C/0D 的 enabled KUnit 与
focused iomux runtime 在同一 current image 中运行；最后 review 完整 Stage 0 diff。若 finding 需要
修改 0A-0C 文件，重新打开 owning checkpoint，不把修改静默归入 0D。

**停止 / 恢复：** TTY 若需要通用动态 registry、破坏预分配 dirty handoff、引入无界 storage，或
只能在 terminal guard 下 notify/drop，停止 Stage 并进入 Target Renegotiation Gate。局部问题回退
0D subset；完整 runtime/review 未通过时 Stage 0 保持 Active，不得运行 Stage 0 -> Stage 1 gate。

### Contract cutover 与代码去留

`None`。`SCHED-LATCH-*`、`SIGNAL-TEMP-MASK-*`、`IOMUX-POLL-*`、`OPENED-DESC-*`、
`TTY-TERM-001` 与 `TTY-INPUT-001` current contract 全部保持有效；0A-0D 任一
checkpoint 或其组合都不能单独合入有效分支，也不能被公共调用者解释为 epoll capability。

- 成功：production-shaped slice 可以保留在 transaction 分支，由 Stage 1 原子吸收；旧 bridge
  仍是 current contract，直到 Stage 1 cutover。
- 路线失败但 target 不变：删除或改写 probe 代码，在 transaction 记录 Route Correction，再
  重新解析 Stage 1。
- 命中停止条件：在任何 contract/public ABI cutover 前冻结代码去留，进入 Target
  Renegotiation Gate。未被接受的 partial slice 不得自然沉淀为第二套 protocol。

### Resolved Write Set Manifest

Stage 0 Active 时允许修改下列并集；每个 checkpoint 的实际 write subset 由上一节进一步收窄：

- `anemone-kernel/src/fs/iomux/mod.rs`
- `anemone-kernel/src/fs/iomux/subscription.rs`（新建）
- `anemone-kernel/src/fs/iomux/wait.rs`（新建）
- `anemone-kernel/src/fs/mod.rs`（0D preflight 后批准，仅精确导出 TTY production 所需的
  `PollRoute` 与 KUnit-only `IomuxWaitRound` test seam）
- `anemone-kernel/src/fs/api/iomux/wait.rs`
- `anemone-kernel/src/fs/api/iomux/ppoll.rs`
- `anemone-kernel/src/fs/api/iomux/pselect6.rs`
- `anemone-kernel/src/fs/pipe.rs`
- `anemone-kernel/src/fs/timerfd.rs`
- `anemone-kernel/src/device/tty/terminal.rs`
- `anemone-kernel/src/task/files.rs`
- 本 RFC 的 `implementation.md`、对应 transaction 条目、`index.md` 与 `invariants.md`。后两者由
  开发者在 0D 前显式批准加入，只用于同步已发生的 checkpoint authorization、Stage 0 closure 与
  Not Effective / no-cutover 边界；不得借此修改 R0 target、owner、ABI、visible semantics、
  acceptance boundary 或 contract delta。target 变化时仍必须先停止。

Validation-only 输入：

- `anemone-apps/user-test/ltp/groups/iomux.txt`
- `anemone-apps/user-test/ltp/profile.txt`：只允许为 0D closure 临时选择 `iomux`，Stage 0 关闭前
  恢复调用者原内容；不得把 profile 选择混入实现 diff。
- `conf/rootfs/pretest-rv64.toml`
- 调用者显式选择的初赛 RV64 sdcard master；它是 validation-only host input，只由 wrapper
  复制，不作为仓库路径或公共接口固定在 RFC 中。

不得修改：

- `anemone-kernel/src/sched/**`、wait core、architecture trap/IPI、`Event`；
- `FileOps` 中 epoll-specific hook、Linux UAPI、syscall number/handler；
- Stage 0 未列出的 pollable source；
- current contract 正文、public ABI、rootfs/build orchestration；
- dynamic final-release observer、epoll core、watch/ready queue 或 nested epoll。

若真实 owner boundary 必须触碰未列文件，worker 先报告文件、原因、contract/验证影响；批准后
先更新 authoritative manifest 和 transaction，再继续。不能在现有文件内塞 feature-specific
downcast 规避扩展。

### 验证

0A-0C 各自执行其定向 build/audit/review floor；Stage 0 仍只有一个由 0D 承担的最终 closure
bundle：

1. `rg` 分类全部 poll source、fd publication/release caller、source-lock 内 notify/drop 与
   `LatchTrigger` bridge；transaction 保存结论，不粘贴整段原始日志。
2. 运行 `just fmt kernel --check` 与 `git diff --check`。
3. 临时把 LTP profile 设为 `iomux`，运行
   `./scripts/run-user-test-rv64.sh <preliminary-rv64-sdcard-image> build/epoll-stage0-rv64.log`；
   wrapper 内的 repository-owned build 必须先成功，随后同一次运行必须看到全部 enabled
   KUnit 通过、`poll01/02`、`ppoll01` 与可适用的 `pselect*` 结果完成，并正常关机。随后
   恢复 profile，确认 master image 未被写入。build 成功与 KUnit runtime 成功分别记录，
   不相互冒充。
4. 对完整 Stage 0 diff 做一次 architecture/concurrency review；finding 在本阶段内修正并重跑
   受影响证据，不另建 review gate。

不运行 epoll LTP、LA64 QEMU 或 broad full profile；Stage 0 尚无 epoll ABI，运行这些不能增加
证明力。

### 停止条件

- representative source 需要保存 consumer 强引用、同步 drain、feature-specific downcast，
  或 notification 必须获取 sleepable operation lock；
- ready-at-subscribe 无法同时安装 route，或 late callback 只能靠 source 同步取消保证安全；
- timerfd/TTY 需要 IRQ-off allocation、sleepable lock、unbounded overflow queue，或破坏现有
  guard-out drop/notify 顺序；
- terminal retirement 允许旧 capability 复活，live lease 必须长期强持 target，或 final close
  必须主动进入 epoll 才能保证 target semantics；
- 当前 write set 会迫使新增第二份 readiness/liveness truth，或改变 scheduler、ABI、owner、
  accepted target / acceptance boundary。

### 退出条件

- 四项假设均由 live code、保留下来的 meaningful KUnit、focused iomux runtime 与 source audit
  证明；未保留的 KUnit 有明确理由，不能用“计划过但没写”计作证据。
- 三类 source 都遵守共同 consumer protocol 和各自 owner-local context/resource boundary；
  old bridge 范围已精确枚举且没有新调用者。
- opened-description capability 与 publication audit 证明 terminal retirement，不引入 dynamic
  observer 或第二 alive truth。
- 无未关闭 Apollyon / Keter / Euclid finding；Stage 0 代码去留与 `contract cutover: None`
  已写入 transaction。
- Stage 0 独立 Closed。Stage 1 是否已经解析不属于本阶段 closure。

## Stage 0 -> Stage 1 Implementation Resolution Gate

Stage 0 Closed 后执行一次只读 preflight：

- 读取 Stage 0 实际 diff、transaction 证据、review findings、KUnit 取舍、source 分类表与
  current `IOMUX-POLL-*` / `OPENED-DESC-*` contract。
- 决定 Stage 0 的 route/lifecycle encoding 是直接吸收、局部改写还是删除；不得把 probe
  临时 surface 默认为长期 public API。
- 枚举所有剩余 `FileOps::poll` / `PollRequest::register(&LatchTrigger)` source 和 snapshot-only
  source，解析各自 capacity、allocation、lock、notify、prune 与 unsupported/`EPERM` 路径。
- 把 Stage 1 展开为完整 Ready：精确 source/file manifest、bridge 删除点、poll/select final-scan
  回归、contract 原子 cutover、失败时旧 contract 保留边界，以及一次 closure bundle。
- 若需要改变 owner、public API、contract delta、ABI 或 acceptance boundary，停止在本 gate，
  进入 RFC review / Target Renegotiation Gate；Stage 1 不得自动 Active。

## Stage 1 Outline：全量 Subscription 与 Poll/Select Cutover

概括目的：

- 把 eventfd、fanotify、其余 TTY/file backend 与所有参与阻塞的 source 迁移到同一个
  source-facing subscription protocol；regular/snapshot-only source 明确返回 capability
  不支持，而不是伪造 persistent route。
- 让 ppoll/pselect 的 `IomuxWaitRound` 成为唯一 register consumer，删除 source-facing
  `LatchTrigger` bridge 与旧 trigger registries。
- 完成 Stage 0 terminal liveness capability 的全 publication audit，原子更新
  `IOMUX-POLL-001/002`、`OPENED-DESC-001/002` 与 `OPENED-DESC-LIVENESS-001`；保持
  `SCHED-LATCH-*`、`IOMUX-POLL-003`、`OPENED-DESC-003`。

前置依赖：Stage 0 Closed，且 route/lifecycle encoding、所有 remaining source 与 contract
cutover manifest 已在 resolution gate 中解析。

受保护边界：

- 不引入 epoll core/ABI；不让完整 source cutover 与 Stage 2 混成一次不可回滚变更。
- cutover checkpoint 中不能长期存在两套 source registry；任一 source 未迁移或 final-scan
  回归时，current contract 全部保持旧版本。
- 不为每个 source 复制 KUnit。优先复用 owner 既有测试；运行时用一次组合 profile 覆盖
  `iomux`、`eventfd`、`timerfd` 与 target 内 fanotify 路径，TTY 由既有 focused test/KUnit 与
  source audit 补充。
- Stage 1 closure 以一次 RV64 wrapper、一次 LA64 build、source audit、format/diff check 和
  review 为一组证据；不再拆 gate。

解析触发点：仅由上一节 resolution gate 解析为 Ready，不能因 Stage 0 成功自动实施。

## Stage 1 -> Stage 2 Implementation Resolution Gate

Stage 1 Closed 后执行一次只读 preflight：

- 读取最终 subscription/capability API、实际 source resource bounds、poll/select 回归、current
  contract cutover 与 `File`/anonymous-file live owner。
- 对照 Linux asm-generic ABI 与固定 LTP source，解析 `epoll_event` layout、syscall numbers、
  flag/opcode/errno、timeout/sigmask、copyout partial progress 和 fd/description key 行为。
- 用 live API 解析 `Epoll`/watch/ready/file 模块边界、operation mutex coverage、sticky pending
  handoff、LT/ET/ONESHOT commit 顺序、teardown 与 userspace test surface。
- 重新读取当时明确选择的初赛镜像中 glibc / musl 两套 `ltp/testcases/bin` 与固定 LTP source，
  刷新下文的 epoll case 基线；冻结 `epoll.txt`、group 注册、focused userspace test 与临时 profile
  选择所需的 test manifest，但不把测试盘路径写成公共运行接口。
- 冻结 Stage 2 精确 code/test/doc manifest 与一个最终 closure bundle。若只能弱化 target、开放
  nesting、要求 close-driven callback 或新增跨 instance owner，先进入 RFC review。

## Stage 2 Outline：Epoll Core、ABI 与最终 Cutover

概括目的：

- 在 `fs::epoll` 建立 watch table、generation、ready candidate/pending protocol、anonymous
  `EpollFile` 与 per-instance operation mutex；epoll wait 只订阅自身 file readiness。
- 在 `anemone_abi` / `fs::api::iomux` 实现 asm-generic `epoll_create1`、`epoll_ctl`、
  `epoll_pwait`、`epoll_pwait2`，由 libc wrapper 承接 `epoll_create` / `epoll_wait`。
- 完成 ADD publication rollback、MOD/DEL logical retirement、target final close lazy retirement、
  LT/ET/ONESHOT、temporary mask、timeout、copyout 与 epoll-file pollability。
- 新建 focused userspace epoll test，覆盖 LTP 不稳定命中的 poll(epfd)、dup/close/fd reuse、
  stale user data、MOD rearm 与 notification/harvest/wait 竞争；新建
  `anemone-apps/user-test/ltp/groups/epoll.txt`，并在
  `anemone-apps/user-test/src/ltp/config.rs` 注册独立 `epoll` group。
- 原子引入 `EPOLL-WATCH-001`、`EPOLL-READY-001`、`EPOLL-FILE-001` 并开放 syscall handlers。

前置依赖：Stage 1 Closed，两个 foundation cutover 已 effective，Stage 2 已由 resolution gate
完整解析为 Ready。

### LTP group 与 profile 角色

当前已挂载的初赛 RV 镜像在 glibc / musl 两套 `ltp/testcases/bin` 中提供相同的 22 个 epoll
可执行项。Stage 2 resolution gate 必须按当时镜像和固定 LTP source 复核；若资产未漂移，
`epoll.txt` 的首版可执行集合为：

```text
epoll_create01
epoll_create02
epoll_create1_01
epoll_create1_02
epoll01 epoll-ltp
epoll_ctl01
epoll_ctl02
epoll_ctl03
epoll_wait01
epoll_wait02
epoll_wait03
epoll_wait04
epoll_wait06
epoll_wait07
epoll_pwait01
epoll_pwait02
epoll_pwait03
epoll_pwait04
epoll_pwait05
```

同一文件用注释保留但不执行 `epoll_ctl04`、`epoll_ctl05` 与 `epoll_wait05`，分别说明它们依赖
首版 target 外的 nested epoll 或 socket persistent readiness。这样可以让后续 target 发现这些
资产，但不能把 TCONF、预期失败或为 TPASS 临时开放能力算作本阶段 closure。

profile 不新增 `epoll-iomux` 之类的组合 group，也不把选择固化进 tracked `profile.txt`：

```text
# 开发期快速定位 epoll ABI / policy
epoll

# Stage 2 最终组合回归
epoll
iomux
```

单独 `epoll` 运行只服务实现迭代和失败定位；最终 closure 使用一次 `epoll` + `iomux` 组合
profile，证明 epoll consumer 与完成 cutover 的 poll/select subscription 能在同一次启动中共存。
组合运行已经成功时不强制再跑一遍单组，避免重复 gate。运行后必须恢复调用者原 profile，
并确认选择变化没有进入 checkpoint。

受保护边界与验证原则：

- nested epoll 始终 `EINVAL` + notice。LTP `epoll_ctl04/05` 明确超出首版 target，不能为了
  获得 TPASS 暗开 nesting，也不能把它们的预期失败计作本阶段回归。
- socket-only `epoll_wait05` 在 socket persistent source 尚未进入 target 时单独分类，不冒充
  epoll core failure 或通过；其余 target 内 create/create1/ctl/wait/pwait/pwait2 case 必须形成
  清晰 PASS/FAIL 矩阵。
- KUnit 只考虑一项真正难由 userspace 确定性命中的 ready/pending handoff：在 harvest 宣布
  empty 与 wait publication 交错时，已发布 obligation 必须被当前轮吸收或保持可见。只有当
  测试能直接驱动 production transition、无需 test-only scheduler/callback 副本时才保留；
  否则用常开 assertion、source proof 与有界 userspace stress，不写伪 KUnit。
- LT、ET、ONESHOT、errno、sigmask、timeout、copyout、dup/close/fd reuse 与 poll(epfd) 由
  focused userspace + LTP 负责。最终至少运行一次选择 `epoll` + `iomux` 的 RV64 wrapper，要求
  glibc / musl 两套 target 内 case 都形成可归因的 PASS/FAIL 结果并正常关机；另运行 LA64 build。
  architecture-independent ABI 与 runtime evidence 不矛盾后才能执行 `EPOLL-CUTOVER`。
- `EPOLL-CUTOVER` 前任何 failure 都保持 syscall 不开放、三个新 contract ID 不生效；不能把
  partial core 登记为 limitation 后合入。

解析触发点：Stage 1 独立关闭后的上一节只读 preflight。

## 旁路审计

- `PollRequest` / `PollRegisterResult` / `LatchTrigger` / `poll_triggers`：区分 snapshot、blocking
  I/O trigger、poll subscription 与禁止残留的 source bridge。
- `description_refs` / publication / release：区分 published fd truth、临时 `Arc`、diagnostic
  identity 与 terminal liveness；任何 epoll-side alive cache 都是 blocker。
- source notification：查找 source lock 内 callback/drop、IRQ-off allocation、sleepable lock
  与直接 task wake。
- epoll code：查找 target strong hold、raw fd/inode/path identity、ready-as-truth、callback 进入
  operation mutex、用户指针进入 core，以及 epoll target 意外成功。

## 文档与反馈路由

- 执行事实、checkpoint、验证结果、KUnit 取舍与 review findings 只追加到 transaction。
- 保持 target 的 stage 顺序、manifest、验证或实现路线变化更新本文和 transaction，不增加
  target 修订。
- owner、correctness invariant、ABI、visible semantics、Contract Impact 或 acceptance
  boundary 变化先停止并进入 RFC review / Target Renegotiation Gate；接受后再更新 target。
- current contract 只在 Stage 1 / Stage 2 对应 cutover checkpoint 更新。失败时保留旧 effective
  文本，不把 partial implementation 写成当前事实。
- target 外、已接受的残余能力进入 current limitations；target 内错误进入 open issues。
