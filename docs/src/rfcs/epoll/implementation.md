# Epoll 实施计划

**状态：** Stage 0-1 Closed / Stage 2 Checkpoint 2C Closed / 2D Authorized, Not Started
**适用修订：** R0
**最后更新：** 2026-07-27
**父 RFC：** [RFC-20260726-epoll](./index.md)
**目标不变量：** [Epoll 与 Poll Subscription 不变量需求](./invariants.md)
**当前契约：** [`SCHED-LATCH-*`](../../contracts/scheduler/latch-wait-round.md)、[`SIGNAL-TEMP-MASK-*`](../../contracts/signal/temporary-mask-delivery.md)、[`IOMUX-POLL-*`](../../contracts/iomux/poll-wait.md)、[`OPENED-DESC-*`](../../contracts/task/opened-description-lifecycle.md)、[`TTY-TERM-001` / `TTY-INPUT-001`](../../contracts/tty/data-plane.md)
**开放问题：** [Tracking Issues](./tracking-issues.md) 当前无开放 Keter
**事务日志：** [2026-07-26-epoll](../../devlog/transactions/2026-07-26-epoll.md)

本文把公共 R0 中已经闭合的 accepted target 解析成滚动实施路线。Stage 0 已解析为
Ready，并在 R0 acceptance、transaction bootstrap 与开发者明确授权后进入 Active。
初始授权覆盖 0A、0B；后续授权覆盖 0C、0D。四个 checkpoint 已逐项独立关闭。开发者随后授权
执行 `0 -> 1` resolution gate；该 gate 把 Stage 1 完整解析为 Ready。后续独立授权完成了 Stage 1
代码、review、验证与两个 foundation cutover。开发者随后授权执行 `1 -> 2` resolution gate；
该 gate 已把 Stage 2 完整解析为 Ready。后续独立授权与精确 write-set expansion 批准已完成 2A，新的独立授权
也已完成 2B ready/wait protocol。当前目标授权覆盖2C与2D；2C已关闭，2D尚未激活，`EPOLL-CUTOVER`
仍未执行。

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
| Stage 0 | Closed | 用 production-shaped vertical slice 证明 observer route、consumer retirement、terminal liveness 与三类 source context 可以共存 | None | 0A-0D closure evidence 已记录 |
| Stage 1 | Closed | 迁移 eventfd/fanotify 两个剩余 poll bridge，删除 source-facing `LatchTrigger` / `Armed` 路径，并原子切换 subscription / opened-description contract | `SUBSCRIPTION-CUTOVER`、`OPENED-DESC-CAPABILITY-CUTOVER` 已同步生效 | closure evidence 已记录；Stage 2 gate 已独立完成 |
| Stage 2 | Active / 2C Closed / 2D Authorized, Not Started | 以2A-2D四个有序checkpoint实现 epoll core、anonymous file、syscall ABI、focused tests 与 LTP，完成首版 epoll cutover | `EPOLL-CUTOVER`，尚未生效 | 2A-2C closure evidence 已记录；2D等待激活 |

## Stage 0 Closed：Subscription 与 Liveness Proof-First Slice

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

**执行状态：** Closed / 2026-07-26。0C 已独立关闭，获批的 0B correction 已由独立 commit
`a693b628` 重新关闭；0D 的 TTY subset、Stage-level audit、RV64 closure wrapper、完整 diff review
与 write-back 已闭合。详细证据与 proof boundary 见对应
[transaction checkpoint log](../../devlog/transactions/2026-07-26-epoll.md#checkpoint-0d---tty-preallocated-handoff--stage-closure---2026-07-26)。

**0B correction 前置项：** 0D 的真实-route KUnit preflight 发现 `IomuxWaitRound` 与 private
`fs::iomux` façade 只允许 `crate::fs` 命名，`device::tty` 因而既不能保存 production `PollRoute`，
也不能让既有 KUnit 经真实 round 构造 route。开发者已批准把 `fs/mod.rs` 加入 Stage manifest，并
最小重开 0B subset 修改 `fs/iomux/{mod,wait}.rs`：只精确导出 crate-internal `PollRoute`，且只在
`kunit` 构建向 TTY 暴露 production `IomuxWaitRound`；observer、route constructor 与
`register_with_route` 继续保持原 owner-private。该 correction 必须独立验证、review、提交并重新关闭
0B，随后 0D code subset 修改 `terminal.rs`，并只在 `fs/mod.rs` 删除 TTY import 落地后已满足
退出条件的两个临时 unused allowance。

**交付与 write subset：** 只修改 `anemone-kernel/src/device/tty/terminal.rs`、`anemone-kernel/src/fs/mod.rs`
的上述 allowance cleanup、本文、对应 transaction，以及获批用于最终状态同步的 `index.md` /
`invariants.md`。
把 TTY poll entry 迁移到 0B protocol，保持 `poll_routes` / `poll_spare` 的预分配容量、
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

## Stage 0 -> Stage 1 Implementation Resolution Gate（Completed 2026-07-26）

Stage 0 Closed 后已执行一次只读 preflight：

- 读取 Stage 0 实际 diff、transaction 证据、review findings、KUnit 取舍、source 分类表与
  current `IOMUX-POLL-*` / `OPENED-DESC-*` contract。
- 决定 Stage 0 的 route/lifecycle encoding 是直接吸收、局部改写还是删除；不得把 probe
  临时 surface 默认为长期 public API。
- 枚举所有剩余 `FileOps::poll` / `PollRequest::register(&LatchTrigger)` source 和 snapshot-only
  source，解析各自 capacity、allocation、lock、notify、prune 与 unsupported/`EPERM` 路径。
- Stage 0 的 route / liveness encoding 直接吸收。live audit 证明 persistent route source 已是 pipe、
  timerfd 与 TTY；只剩 eventfd、fanotify 的 poll registry 保存 `LatchTrigger` 并返回 `Armed`。
  ext4、ramfs、procfs 等 regular/snapshot-only source 继续通过 `ready_or_unsupported()` 明确表达
  “当前 ready 可返回、未 ready 不可阻塞”，console/devfs/device stub 继续显式 unsupported。
- `PollRequest::register(&LatchTrigger)` 已无 production constructor caller；bridge 只通过
  `IomuxWaitRound::poll_request()` 内部同时携带 route/trigger，再由 eventfd/fanotify 读取 trigger。
  因而 Stage 1 可以一次删除 trigger 字段、legacy constructor/getter 与 `PollRegisterResult::Armed`，
  不需要 transitional contract、第二个 registry 或新的 probe。
- `task::files` publication/final-release surface 自 0D closure 后未漂移；
  `OpenedDescriptionCapability` / operation-local lease 保持 0A encoding，Stage 1 只完成再次 source audit
  与 current-contract cutover，不修改 `task/files.rs`。
- 下节已把 Stage 1 展开为完整 Ready：精确 source/file manifest、bridge 删除点、poll/select final-scan
  回归、contract 原子 cutover、失败时旧 contract 保留边界，以及一次 closure bundle。
- 若需要改变 owner、public API、contract delta、ABI 或 acceptance boundary，停止在本 gate，
  进入 RFC review / Target Renegotiation Gate；Stage 1 不得自动 Active。

## Stage 1 Ready：全量 Subscription 与 Poll/Select Cutover

### 阶段成熟度与授权边界

- `Closed / 2026-07-26`。Stage 0 已独立关闭；上一节 resolution gate 已核对 live source、实际
  Stage 0 diff、review/validation evidence、current contracts、register 与测试入口，并冻结本节完整
  deliverable、cutover 和 manifest。
- 本阶段是一个原子 checkpoint，不再拆 source-migration / bridge-removal 子 checkpoint。live tree 只剩
  两个普通 task-context dynamic registry，继续拆 gate 只会制造不可单独生效的混合协议中间态。
- 开发者已在新的明确授权中要求完成 Stage 1；transaction 已记录 branch、HEAD、clean state、current
  contracts 与 manifest 后激活本阶段。代码、验证、review、write-back 与两个 foundation cutover 已在
  同一 checkpoint 闭合；该授权不覆盖 Stage 2 resolution 或实现。

### 前置证据与受保护边界

- 直接吸收 Stage 0 的 `PollObserver` / non-owning `PollRoute`、`IomuxWaitRound`、pipe COW registry、
  timerfd fixed-capacity noirq batch、TTY preallocated dirty handoff，以及 opened-description terminal
  liveness encoding；不把其中任何 probe-only visibility 扩成 public API。
- 保持 `SCHED-LATCH-001..003` 与 `SIGNAL-TEMP-MASK-001..003`。`IomuxWaitRound` 内部可以继续持有
  本轮 `LatchTrigger`，但 source-facing `PollRequest`、source registry 与 `FileOps::poll` 不再看见它；
  ppoll/pselect 的 temporary-mask classifier、schedule/finish/final-scan 顺序保持不变。
- readiness truth、interest filtering 与 registry storage 仍由各 source state 拥有。route 是 non-owning
  recheck capability；source guard 内只安装/选择 route 和读取 predicate，observer notification、被替换
  registry 的最后 drop 与 wait completion 必须发生在 guard 外。
- eventfd blocking read/write 与 fanotify blocking read 仍是各自 owner 的 one-round `LatchTrigger`
  consumer，不属于 source-facing poll bridge，不能为了 `rg` 清零而迁移或改写。
- regular/snapshot-only source 不获得伪 persistent route。register scan 中当前 ready 可返回
  `Ready(events)`；未 ready 必须返回 `Unsupported`，不能让 syscall 睡在未订阅 source 上。
- 不引入 epoll core、anonymous epoll file、Linux UAPI/syscall、watch/ready state 或 nested epoll；不让
  foundation cutover 与 Stage 2 混成一次不可回滚变更。

### 交付与实现路线

#### Iomux bridge 删除

- `PollRequest` 只保留 interests 与可选 `PollRoute`：snapshot 没有 route，register 只由
  `IomuxWaitRound` 构造并携带 route。删除 `PollRequest::register(&LatchTrigger)`、trigger 字段/getter
  和“route + trigger”双携带形状；`is_register()` 只由 route 是否存在推导。
- `PollRegisterResult` 收窄为 `Ready(PollEvent)`、`Subscribed(PollEvent)` 与 `Unsupported`，删除
  `Armed`。`Subscribed` 同时证明 route 已发布并携带 publication point 的 current readiness；
  `Ready` 在 register scan 中只允许非空、无需睡眠的 snapshot-only result。
- `IomuxWaitRound` 继续唯一拥有 observer acceptance 与本轮 latch，source-facing request 只取得 route。
  ppoll/pselect 删除 `Armed` 分支，保留 unexpected snapshot-subscribe、empty register-ready、unsupported、
  register abort 和 final-scan 的 fail-closed mapping。

#### Eventfd poll route

- blocking read/write trigger queue 保持不变；只把 `EventFdPollTrigger` / `poll_triggers` 替换为
  source-local、interest-bearing `PollRoute` registry。register 在 eventfd state guard 内先 fallibly 构造
  已 prune stale entry 的 replacement，再原子替换 registry并读取 current counter predicate；分配失败在
  publication 前返回 `ENOMEM`，已发布 registry 不变。
- registry 使用 owner-local COW snapshot，避免 counter transition 为通知分配内存。read/write 在 state
  guard 内更新 counter、detach 对应 blocking-I/O triggers，并 clone 当前 poll-route snapshot；释放 guard
  后分别触发 blocking waiter、按 READABLE/WRITABLE interest 发布 route hint并 drop snapshot。live route
  不因一次 notification 删除；被替换的旧 registry 在 guard 外 drop。
- ready-at-subscribe 仍保存 route。eventfd counter/read/write/semaphore 语义、blocking I/O、status flag、
  copy 和 errno 不在本阶段改变。

#### Fanotify poll route

- 只迁移 group-fd poll registry；`FanReadTrigger`、mark registry、event queue、overflow/dead truth、read、
  ioctl 与 final-release owner 保持不变。`FanQueue::poll` / `FanGroup::poll` 只为 fallible route publication
  收窄成可返回 `SysError` 的 owner-local path，Linux-facing `FileOps::poll` 继续直接传播结果。
- poll registry 使用 source-local COW `PollRoute` snapshot。register 在同一 group mutex 内 fallibly prune /
  replace registry并读取 queue/dead predicate，分配失败前不发布；旧 snapshot 在 mutex 外 drop。empty-to-
  nonempty enqueue 选择 READABLE candidates，group-dead/HANG_UP 选择所有 candidates；notification 与 route
  drop 均在 group mutex 和 global fanotify registry mutex 外发生，live route 保留到 consumer retirement
  后的后续 owner cleanup。
- 不把 fanotify 的 sleepable mutex/COW 形状抽成所有 source 的通用 registry，也不改变 Stage 5 backlog、
  permission/FID/name/merge-order 或现有 fanotify LTP acceptance boundary。

#### Opened-description capability cutover

- 对 `open_fd*`、reservation commit、dup/dup3、fork/`CLONE_FILES`、unshare、close/close_range、cloexec、
  table replacement 与 exit cleanup 重做只读 publication/final-release audit。确认所有 publication 都在
  `Unpublished` 或仍有 live alias 时增加 ref，terminal `Live(1) -> Retired` 后不能重新 publication。
- 保留 Stage 0 的 `OpenedDescriptionCapability` / non-cloneable operation lease 与静态
  `FileDescOps::final_release`；本阶段不修改 `task/files.rs`，不新增 dynamic observer、epoll-side alive bit
  或长期 target strong hold。

### 审计与可观测性

- 全量分类 `FileOps::poll`、`PollRequest` constructor/accessor、`PollRegisterResult`、`LatchTrigger`、
  `poll_triggers` 与 `poll_routes`。cutover 后 source-facing bridge 的禁止搜索必须为零；eventfd/fanotify/
  timerfd 的 blocking-I/O trigger 命中单独登记，不能误删。
- 审计 eventfd 的 counter `0 <-> nonzero` / writable boundary 与 fanotify queue empty/nonempty、clear、dead
  transition，确认 predicate update 与 route snapshot selection 同 guard、notify/drop 在 guard 外，且 source
  行为不读取 callback 结果。
- route allocation failure 返回现有 `SysError::OutOfMemory`，capacity/unsupported 保持现有 warning/debug
  边界；不增加每次 notification 日志、diagnostic route id 或行为化计数器。
- 不新增 KUnit。Stage 0 已通过真实 pipe/timerfd/TTY production transition证明 observer、普通/noirq/
  预分配三类 context；本阶段用 enum/bridge 删除的编译闭包、剩余两 source 的 owner audit、全部既有 KUnit
  与组合 userspace/LTP runtime 证明吸收，不为两个同类 dynamic registry 复制测试状态机。

### Contract cutover 与失败原子性

本阶段在同一最终 integration checkpoint 执行两个 foundation cutover：

- `SUBSCRIPTION-CUTOVER`：Refine `IOMUX-POLL-001`、Replace `IOMUX-POLL-002`，保持
  `IOMUX-POLL-003` 与 `SCHED-LATCH-*`；更新 iomux current contract 与 owner index，使 source-facing
  protocol 只剩 non-owning route + current snapshot。
- `OPENED-DESC-CAPABILITY-CUTOVER`：Refine `OPENED-DESC-001/002`、Introduce
  `OPENED-DESC-LIVENESS-001`，保持 `OPENED-DESC-003`；更新 opened-description current contract 与 owner
  index，使 terminal lifecycle / capability 成为 effective foundation。

代码、current contract 与 transaction cutover evidence 是一个原子合入单元。实现期间允许工作树短暂出现
混合形状，但任何中间 commit / partial source migration 都不能单独合入有效分支或被 Stage 2 依赖。任一
source 未迁移、bridge 搜索未清零、build/runtime/review 未闭合或 contract 文本未同步时，两个 cutover 都
保持 Not Cut Over，旧 current contract 继续有效；不得只切换其中一个 foundation ID 集合。

### 验证与 review

1. 运行 source/caller/publication audit：证明仅 `IomuxWaitRound` 构造 register request，source-facing
   `LatchTrigger` / `Armed` / legacy poll registry 为零；snapshot-only source 分类没有漂移；opened-description
   publication/release caller 与 0D closure 一致。
2. 运行 `just fmt kernel --check`、`git diff --check` 与 `mdbook build docs`。
3. 串行运行
   `just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G`。该命令只证明 LA64 build，
   不冒充 LA64 runtime、SMP、hardware 或 LTP。
4. 临时把 tracked profile 精确设为 `iomux`、`eventfd`、`timerfd`、`fanotify`，运行一次
   `./scripts/run-user-test-rv64.sh <preliminary-rv64-sdcard-image> build/epoll-stage1-rv64.log`。同一 image
   必须完成 repository-owned build、全部 enabled KUnit、glibc/musl 两轮 group matrix 与正常关机；随后
   恢复 profile 并核对调用者选择的 master image 未写入。
5. runtime 证据按 owner 分层：iomux 记录 Stage 0 已知 timer-accuracy case 而不把它们归因于 route；
   eventfd 以 `eventfd01..05`、`eventfd2_01..03` 为既有 target，`eventfd06` AIO/overflow 保持原边界；
   timerfd 要求 tracked active cases 完成；fanotify 只要求 tracked active subset 与既有 RFC target 形成
   可归因矩阵，不把 Stage 5 backlog 失败算成 subscription regression。任何新 timeout、unsupported-register、
   panic/deadlock 或 source notification failure 都必须停止并归因，不能用历史失败掩盖。
6. 对完整 Stage 1 diff 做 architecture/concurrency review，重点检查 route lifetime、COW replacement、
   allocation failure、source guard-out notify/drop、ppoll/pselect final-scan、terminal retirement 与 contract
   原子性。finding 在 Stage 内修正并重跑受影响证据；不另建形式化 review gate。

### 停止条件

- eventfd/fanotify 只能通过 source 强持 consumer、同步 drain、guard 内 callback/最后 drop、notification-time
  fallible allocation或第二份 readiness truth 保持正确；
- 删除 trigger/`Armed` 后发现新的参与阻塞 source、source-specific escape hatch，或 snapshot-only source
  会在未订阅状态进入 schedule；
- opened-description publication audit 发现 terminal retirement 后可复活、需要 final close 主动进入 epoll，
  或 capability 必须长期强持 target；
- ppoll/pselect 的 final-scan、temporary-mask outcome、现有 source readiness/errno 或 fanotify accepted target
  必须改变才能完成迁移；
- 实现需要修改 scheduler/wait core、public ABI、RFC owner/Contract Impact/acceptance boundary，或扩大到 epoll
  core/nesting。命中这些条件时在 cutover 前停止，按影响进入 write-set expansion、Route Correction 或
  Target Renegotiation Gate，不能用兼容分支绕过。

### 退出条件

- eventfd/fanotify 已返回 `Subscribed(current)`，ready-at-subscribe 保留 route，所有 predicate transition
  guard-out notify且 live route 不按 callback 次数消费；pipe/timerfd/TTY 保持 Stage 0 owner-local边界。
- production source-facing `PollRequest::register(&LatchTrigger)`、trigger getter、`Armed` 与 legacy poll
  registry 为零；blocking-I/O `LatchTrigger` 均有明确 owner 分类。
- ppoll/pselect register-abort、schedule/finish/final-scan、temporary-mask 与 snapshot-only unsupported 语义
  无回退；opened-description terminal liveness audit闭合且 `task/files.rs` 无第二 truth。
- format/diff/docs、LA64 build、一次 RV64 closure wrapper、完整 review 与 profile/image restoration 全部闭合；
  Not Run 边界逐项记录，没有用 build/docs/RV64 结果外推 SMP、LA64 runtime、hardware、final harness 或
  epoll LTP。
- 两个 foundation cutover 在同一 transaction checkpoint 记录旧/新规则、验证与生效点；无未关闭
  Apollyon、Keter 或 Euclid。Stage 1 独立 Closed；Stage 2 是否已解析不属于本阶段 closure。

### Resolved Write Set Manifest

允许修改的 kernel 文件：

- `anemone-kernel/src/fs/iomux/mod.rs`
- `anemone-kernel/src/fs/iomux/wait.rs`
- `anemone-kernel/src/fs/api/iomux/ppoll.rs`
- `anemone-kernel/src/fs/api/iomux/pselect6.rs`
- `anemone-kernel/src/fs/eventfd.rs`
- `anemone-kernel/src/fs/fanotify/queue.rs`
- `anemone-kernel/src/fs/fanotify/group.rs`
- `anemone-kernel/src/fs/fanotify/file.rs`

允许修改的 code/contract 状态回写：

- `docs/src/rfcs/epoll/{index.md,invariants.md,implementation.md}`
- `docs/src/devlog/transactions/2026-07-26-epoll.md`
- `docs/src/contracts/iomux/{index.md,poll-wait.md}`
- `docs/src/contracts/task/{index.md,opened-description-lifecycle.md}`
- `docs/src/rfcs.md`
- `docs/src/devlog/transactions/index.md`
- `docs/src/devlog/2026-07-20_to_2026-08-02.md`

Validation-only 输入：

- `anemone-kernel/src/fs/{pipe.rs,timerfd.rs,file.rs,mod.rs}`、
  `anemone-kernel/src/device/tty/{file.rs,terminal.rs}`、`anemone-kernel/src/task/files.rs`
- `anemone-apps/user-test/ltp/groups/{iomux,eventfd,timerfd,fanotify}.txt`
- `anemone-apps/user-test/ltp/profile.txt`：只允许 closure wrapper 临时选择四组，完成后恢复调用者原内容；
  profile diff 不得进入 checkpoint
- `conf/rootfs/pretest-rv64.toml`
- 调用者显式选择的初赛 RV64 sdcard master；只由 wrapper 复制，不能作为公共运行接口写入文档

不得修改：

- `anemone-kernel/src/sched/**`、wait core、Signal owner、architecture trap/IPI、`Event`
- `anemone-kernel/src/task/files.rs`、`FileDescOps`、epoll core/UAPI/syscall、socket/nested epoll
- pipe/timerfd/TTY 与 snapshot-only source 的 production code；本阶段只审计和回归它们
- `anemone-apps/**` tracked source/group、rootfs/build orchestration、public ABI、register 与未列 current contract

若真实 owner boundary 需要触碰未列文件，worker 必须先报告文件、原因、contract/验证影响；批准后先更新
本 manifest 和 transaction，再继续。不能在已列文件内增加 source-specific compatibility branch 规避扩展。

## Stage 1 -> Stage 2 Implementation Resolution Gate（Completed 2026-07-26）

Stage 1 独立关闭后已执行一次只读 preflight：

- 入口为 `dev/drc/omega@abaae0bf`，worktree clean。重新读取 Stage 1 最终 diff、transaction、
  current `IOMUX-POLL-*` / `OPENED-DESC-*` / `SIGNAL-TEMP-MASK-*` contract、register、所有 live
  `FileOps::poll` owner、fd publication/reservation、anonymous-file 路径与 userspace/rootfs test owner。
- live subscription surface 已没有 source-facing `LatchTrigger` / `Armed` bridge。`PollObserver` 与
  `PollRoute::new` 仍是 iomux-private，而 epoll watch 与 epoll file 必须实现同一 observer protocol；
  Stage 2 只把它们扩大到 `crate::fs` 内可见，source 继续只能取得 `PollRoute`。
- `OpenedDescriptionLease` 当前只提供 `is_live()`。watch 又不能保存 `ProcFile` 或长期 `Arc<File>`，
  因此 Stage 2 必须在 `task::files` 增加 operation-local `poll()` 与 `FileOps` identity probe；该窄
  capability 不暴露 owner 私有状态，并在 target snapshot/epoll-target 拒绝后继续用 `is_live()` 完成
  commit-time recheck。这是已生效 `OPENED-DESC-LIVENESS-001` 的 consumer 接线，不重开 dynamic
  final-release observer。
- `FdReservation` 已提供 unpublished slot、commit 与 Drop rollback；anonymous inode/File 创建也已有
  owner。`epoll_create1` 可以先完整构造 file/description，再一次 publication，不需要新的 fd-table
  transaction 或 parallel allocator。
- asm-generic syscall number 固定为 `epoll_create1=20`、`epoll_ctl=21`、`epoll_pwait=22`、
  `epoll_pwait2=441`。RV64/LA64 `epoll_event` 均为 16 bytes、`data` offset 8；现有 typed user pointer
  会拒绝 unaligned address，所以 adapter 必须用 byte slice 完整校验/copy，再 unaligned decode/encode，
  不能误用 x86_64 packed 12-byte layout。
- 初赛 RV64 master image 在本次 audit 的 SHA-256 为
  `167183750331ea82fcd8de6abc3e0a89481367f07dd1495bcafcb4c25bdce5ab`；glibc/musl 两套目录都有
  相同 22 个 epoll executable。固定 LTP source 证明 `epoll_ctl04/05` 依赖 nested epoll，
  `epoll_wait05` 与 `epoll_pwait01..05` 依赖当前不存在的 `AF_UNIX socketpair` owner。开发者明确接受
  本阶段不运行五个 `epoll_pwait` LTP；这不删除 pwait/pwait2 target，其 ABI/sigmask/timeout 由 pipe-based
  focused test 证明。
- `epoll_create02` 的 glibc wrapper 在进入 kernel 前验证 `size > 0`，musl wrapper 则忽略 size并调用
  `epoll_create1(0)`；musl 变体不能由 kernel 修复，Stage 2 在 musl root 的 `disabled_cases` 精确排除
  该 case，并保留 libc 归因。
- 下节已经冻结完整 owner route、ABI/errno、ctl rollback、dirty/ready/harvest/copyout、epoll-file wait
  handoff、focused/LTP matrix、cutover、停止/退出条件与精确 manifest。解析没有改变 R0 target、owner、
  Contract Impact、visible semantics 或 acceptance boundary，因此 R0 不递增，也不新增 tracking issue。

## Stage 2 Ready：Epoll Core、ABI 与最终 Cutover

### 阶段成熟度与授权边界

- **Active / Checkpoint 2C Closed / 2D Authorized, Not Started。** Stage 0-1 已 Closed，`SUBSCRIPTION-CUTOVER` 与
  `OPENED-DESC-CAPABILITY-CUTOVER` 已 effective；上一节 resolution gate 已完成 live owner、ABI、LTP、
  test harness 与 current-contract preflight。
- 开发者先授权解析 Stage 2 implementation，并明确允许后续测试需要时把 `anemone-rs` 与
  `anemone-apps` 纳入写集；后续独立授权已分别关闭 2A 与 2B，当前目标授权继续覆盖2C与2D。
  2C已完成build-only closure；QEMU、LTP与`EPOLL-CUTOVER`只在2D激活后执行。
- Stage 2 保持一个原子 integration / acceptance unit，但实现拆成 2A-2D 四个有序 checkpoint。2A-2C 只形成
  不可独立合入的 stacked implementation evidence；在 2D 完成 kernel ABI、focused test、LTP、review、current
  contract 与 transaction write-back 前，任何 partial core、syscall handler 或单独 contract page 都不得合入
  有效分支或被其它功能依赖。
- 除非开发者明确一次授权多个 checkpoint，Stage 2 activation 只进入 2A；任一 checkpoint 关闭都不自动授权
  下一个。每次 activation、closure、validation 与 partial-code disposition 只追加到 transaction，不复制本文
  的 authoritative delivery/write-set 定义。

### 阶段内 checkpoint 路线

| Checkpoint | 目的 | 主要 write subset | 关闭边界 |
| --- | --- | --- | --- |
| 2A - Watch / Lifecycle Core | 先固定 instance、watch identity/generation、operation serialization、ADD/MOD/DEL replacement 与 logical retirement | `fs::epoll::{mod,watch,ready}` 的 dormant core、iomux visibility、opened-description lease 窄接口 | core 不接收用户指针、不开放 syscall；lifecycle/rollback review 与 RV64 build 通过 |
| 2B - Ready / Wait Protocol | 独立闭合 callback-safe dirty、refresh/harvest、LT/ET/ONESHOT、epoll-file route publication 与 wait handoff | `fs::epoll::{mod,watch,ready,file}` 及同 owner integration corrections | lost-wake / stale-delivery / copyout-policy obligation 可审查，2B concurrency review 无未关闭 finding；仍不开放 ABI |
| 2C - ABI / Focused Oracle | 接入四个 adapter、两架构 UAPI/syscall table、temporary-mask context 与 focused userspace oracle | kernel ABI/adapter、`task::sig::delivery`、`anemone-abi`、`anemone-rs`、`epoll-test` | 两架构 app/kernel build 与 ABI audit 通过；candidate handlers 只留在 stacked branch，不得独立合入 |
| 2D - Integration / Cutover | 接入 user-test/LTP/rootfs，运行组合 closure，完成 full-diff review 与唯一 contract cutover | harness/LTP/rootfs、前序 finding 修正、contract/RFC/transaction/navigation | focused + LTP matrix、iomux组合、LA64 build、review与docs全部闭合后执行唯一 `EPOLL-CUTOVER` |

2A-2C 的 checkpoint commit 只是回滚、review 与证据边界，不是 transitional contract。前序 checkpoint 的
production 文件可以在后续 checkpoint 为真实 finding 做局部修正，但不能借此绕过 manifest expansion 或把尚未
验证的 partial capability 当作当前事实。

### 全 Stage 2 受保护 owner/module 边界

- `fs::epoll` 是 policy/protocol owner，拆成 `mod.rs`、`watch.rs`、`ready.rs` 与 `file.rs`：
  `mod.rs` 组合 instance 与 operation surface，`watch.rs` 拥有 watch identity/generation/policy，
  `ready.rs` 拥有 bounded slot、dirty/ready/harvest，`file.rs` 拥有 anonymous `EpollFile` 与普通 pollability。
  这只是同一 owner 内按 lifecycle/ops 拆分，不增加 public API 或第二抽象层。
- `fs::api::iomux::epoll/{create,ctl,wait}.rs` 是 Linux ABI adapter：只在这里解析 syscall args、
  `epoll_event`、ctl opcode、flags、timeout/sigmask、errno 与 user copy。`fs::epoll` 只接收内部 event/policy
  类型，不读写用户指针，不保存 Linux-shaped bits。
- 每个 `Epoll` 拥有一个 sleepable operation mutex，串行 ADD/MOD/DEL、instance teardown、candidate refresh、
  harvest 与 copyout commit/rollback。task sleep 不持有该 mutex；source callback 与 epoll-file route notify
  也不获取它，否则 ctl 无法使已睡眠的 wait 可见新 readiness。
- watch table 的 key 是 `(OpenedDescriptionCapability identity, user fd key)`；fd reuse 不继承旧 entry，dup
  可以为同一 opened description 建立不同 fd-key entry。`task::files` 继续唯一拥有 publication、identity
  与 terminal liveness；watch 只持 non-owning capability，不保存 target `Arc<File>` 或 epoll-side alive bit。
- `EpollWatch` 实现 `PollObserver`，持 immutable slot/generation、fd key、internal interests、LT/ET/ONESHOT
  policy、user data、`accepting: AtomicBool`、target capability 与 `Weak<Epoll>`。source route 不拥有 watch；
  logical retirement 先清除 accepting/移出 current slot，晚到 callback 只能成为 stale recheck hint。
- watch slots 与 dirty bitmap 以现有 `MAX_FD_PER_PROCESS=1024` 为上界；slot table 中的 current generation
  是 user data/policy 的唯一当前映射。旧 callback 命中空 slot或已复用 slot最多触发一次额外 recheck，
  不能交付旧 generation 的 user data。
- epoll-file route registry 使用 owner-local fallible COW snapshot，live route 数以现有进程容量为界；
  callback 只在短 non-sleeping guard 内 clone 已发布 snapshot，guard 外 notify/drop。它不获取 operation
  mutex、不在 notification path 分配，也不进入 target snapshot。
- `PollObserver` 与 `PollRoute::new` 只扩大到 `crate::fs`，`PollRoute` 的 source-facing capability 与
  `notify/is_prunable` contract 不变。不得向 source 暴露 observer、epoll类型或 callback result。
- `OpenedDescriptionLease` 增加 feature-neutral 的 operation-local `poll(request)` 和
  `uses_file_ops(&'static FileOps)`（或等价窄命名）；lease 仍不可 clone且不暴露 `ProcFile`。后者只用于
  拒绝 epoll target，不允许把 `FileOps` identity 推广为 watch identity或 lifecycle truth。

### Checkpoint 2A - Watch / Lifecycle Core

**执行状态：** Closed / 2026-07-26。candidate实现、expansion report/批准、最终owner/lifecycle review与
验证证据见
[transaction checkpoint log](../../devlog/transactions/2026-07-26-epoll.md#stage-2-checkpoint-2a-closure---2026-07-26)；
本项关闭只证明dormant core可继续，不开放syscall、current contract或2B。

#### Ctl publication、replacement 与 retirement

- ADD 在 operation mutex 下取得 target fd、capture capability、reserve unused slot/generation并构造 watch；
  先让 target 用该 watch route执行 register poll。只有 `Subscribed(current)` 证明 persistent route 已安装；
  snapshot-only `Ready` 或 `Unsupported` 都映射 `EPERM`，不能把无 route 的 regular file 当成可持续 target。
  route/callback 可以早于 table publication，但 slot 只有在 liveness final recheck 与 duplicate recheck 都通过后
  才成为 current；commit后无条件把该slot标为dirty并提示epoll-file routes，使ready-at-ADD或早到hint对并发
  waiter可见。失败把 accepting 置 false并释放 slot，早到/晚到 hint均不能泄漏 user data。
- ADD 对同一 `(description identity, fd key)` 返回 `EEXIST`。target fd 无效返回 `EBADF`；epfd 不是 epoll、
  target 与 epfd 相同或 target 是任意 epoll file分别返回 `EINVAL`，后两类同时记录 nested/self rejection
  notice。首版不执行 cycle/depth admission。
- MOD 只处理 current key；不存在返回 `ENOENT`。它以新 generation/watch 执行完整 fallible subscribe与
  liveness recheck，成功后在 operation mutex 内一次替换 current slot，再 retire旧 watch；任一前置失败保持
  旧 interest/policy/user data/ready state不变。commit后把新generation标为dirty并提示epoll-file routes，
  MOD 同时承担 ONESHOT rearm；旧 callback不能启用新 generation。source fixed-capacity可能因原route尚live而
  拒绝replacement subscribe；该resource failure必须保持旧watch有效，不能先retire再赌新route成功。
- DEL 忽略 event pointer内容并允许 null pointer；不存在返回 `ENOENT`。在 operation mutex 内先移除 current
  table/ready映射并 logical-retire watch，再释放 slot；source route unlink/prune不是成功条件，DEL 返回后旧
  callback fail closed。
- watched target 的 final close 只推进 `task::files` terminal liveness，不进入 epoll、不获取 operation mutex、
  不要求主动唤醒 wait。ctl/refresh/harvest 取得短 lease并在 commit前重验；capability失效时惰性 retire watch。
  epoll file自身的最后 published close可使用创建时固定的 `FileDescOps::final_release` 进入本 instance teardown；
  teardown设置 closing、唤醒自身 wait routes并在 operation mutex下 retire全部 watch，不修改 watched target owner。
- 所有 allocation、target register与用户 copy validation发生在可回滚 publication之前；一旦 ADD/MOD/DEL 的
  current mapping commit，后续只执行 infallible logical retirement/guard-out drop。不得用“记录错误后继续”
  留下 half-published watch。

#### 2A write subset、验证与关闭

- write subset限于 `anemone-kernel/src/fs/{mod.rs,file.rs,iomux/{mod.rs,subscription.rs}}`、
  `anemone-kernel/src/fs/epoll/{mod.rs,watch.rs,ready.rs}` 与 `anemone-kernel/src/task/files.rs`。`ready.rs`
  在本 checkpoint 只提供 watch publication/retirement 所需的 slot、generation 与 dirty obligation；完整
  refresh/harvest policy 由 2B 关闭。
- 运行 `just fmt all --check`、`git diff --check` 与
  `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`；build 不冒充 runtime、SMP 或
  lifecycle race proof。
- review必须覆盖 target strong hold、identity第二truth、lease逃逸、ADD/MOD fallible subscribe顺序、DEL/final
  close retirement、slot/generation reuse与 teardown lock ordering。任何路径只有通过同步 cancel/drain、source
  强持watch、target长期strong hold或第二alive truth才能成立时，停止 Stage并进入既有反馈路由。
- 2A关闭只证明 dormant core的owner/lifecycle形状可继续；不开放syscall、不创建current contract、不授权2B。

### Checkpoint 2B - Ready / Wait Protocol

**执行状态：** Closed / 2026-07-26。callback-safe dirty/sequence、refresh/harvest、LT/ET/ONESHOT、
anonymous epoll file 与 route publication 已按独立授权完成；实现、review、验证和 KUnit 取舍见
[transaction checkpoint log](../../devlog/transactions/2026-07-26-epoll.md#stage-2-checkpoint-2b-closure---2026-07-26)。
本项关闭只证明 core protocol 可进入 ABI 接线；不开放 syscall、不切 current contract、不授权 2C。

#### Dirty、Ready、Harvest 与 Copyout 协议

- callback 先检查 watch accepting，再 upgrade `Weak<Epoll>`，对 slot执行原子 dirty-set，随后推进 monotonic
  notification sequence并提示 epoll-file routes。dirty bit只表示“该 slot 需要重检”，重复 notification可合并；
  callback 不获取 operation mutex、不分配、不读取 target、不修改 ready set或用户 policy。
- operation mutex 下的 refresh 原子取走 dirty slots，逐项校验 current generation、取得 target lease、执行
  snapshot poll并最终重验 liveness。实际 predicate只来自 target；ready bitmap/候选集合只保存可重检 identity。
  stale/retired target被移除；snapshot `Err` 保留slot的重检义务并使当前operation失败，不合成
  `EPOLLERR`。只有source明确返回 `PollEvent::ERROR` 才是用户可见error readiness，callback payload同样不能
  被解释为readiness。
- ready candidates使用 round-robin cursor选择最多 `maxevents` 项，避免持续 LT source饿死后续 slot。harvest
  claim从 ready集合暂时移出候选，但不会清除 claim后由 callback重新设置的 dirty bit。
- 每个 claim在交付前再次 snapshot target并按当前 interest生成 internal event；source实际 ERROR/HANG_UP始终
  报告，不依赖用户 interest。无当前 readiness的 candidate被消费；未来 notification可重新 dirty。
- ET 只消费当前 candidate，成功交付后不因仍 ready自动 requeue；claim/snapshot后到达的 dirty obligation保持，
  因而并发 edge不会被“already queued”吞掉。LT 成功交付后若 final snapshot仍 ready则放回 ready尾部；
  ONESHOT 只在对应 event成功 copyout commit后 disable，MOD 才能以新 generation rearm。
- core 返回不含用户指针的 RAII harvest batch，记录 claimed slots、internal events与原 ready/disable义务；syscall
  adapter持 operation guard把整批编码为 16-byte records并完成用户写入。完整 copyout成功后 batch commit LT
  requeue/ET consume/ONESHOT disable；validation或copyout失败则 Drop rollback全部 claim，不形成 partial
  policy commit。用户内存即使发生部分字节写入也不等于事件已消费，下次 wait仍能重新交付。
- output range在 harvest前按 `maxevents * 16` checked arithmetic整段验证；`maxevents <= 0` 返回 `EINVAL`，
  overflow/invalid range返回对应 `EINVAL/EFAULT`。core batch上限同时受 `maxevents` 与 1024-slot owner bound约束，
  不新增不可配置 magic capacity。

#### Epoll-file readiness 与 wait handoff

- epoll-file `FileOps::poll` 的 READABLE含义是“在当前 operation-serialized refresh后至少有一个可交付
  candidate”，不能直接等同 dirty bit、route callback次数或 ready bitmap未复核内容。
- snapshot poll取得 operation mutex、refresh并返回 current readability。register poll使用 notification
  sequence闭合 refresh与route publication窗口：读取 sequence、refresh、fallibly发布 epoll-file route、再读
  sequence；若变化则在 route已可见的前提下重新refresh。route发布后的 callback会通知新 route，因而不存在
  “refresh为空—尚未订阅—target通知”lost wake。
- `epoll_pwait/pwait2` 每轮先在 operation mutex下 harvest；无 event 时释放 mutex，只对 epoll file建立一个
  `IomuxWaitRound`，完成 subscribe/current snapshot后进入 scheduler wait。wake/timeout/signal/force后 retire
  round并重新 harvest；绝不为一次 wait重订阅全部 targets，也不 busy scan。
- 多个并发 waiter可以各自持有 epoll-file route，但每次 refresh/harvest/copyout commit仍由同一 operation mutex
  串行。closing使后续 operation fail closed并提示已发布 wait routes；sleep阶段不持 operation mutex。

#### KUnit、审计与可观测性

- 只允许一项可选 KUnit：直接驱动 production dirty/sequence/epoll-file route transition，确定性制造
  “refresh宣告empty—route publication—notification”交错，证明 obligation被当前refresh吸收或由已发布
  route保持可见。若必须复制 scheduler、伪造 callback接口或新增 test-only state，删除该 KUnit，改用常开
  assertion、source audit与focused bounded stress；不得为普通 getter/ctl/ABI复制 userspace测试。
- 常开 assertion覆盖 slot/generation唯一映射、ready/disabled互斥、batch只commit一次、retired watch不能
  行为化、operation guard覆盖policy commit。昂贵全表一致性扫描才允许 `debug_assert!`。
- 日志只放在 ABI兼容/拒绝点、resource exhaustion、unexpected typed poll result与fail-closed internal error；
  notification/harvest热路径不逐事件打印。diagnostic generation/slot不得反向驱动除stale校验外的policy。
- 2B审计 target strong hold、raw fd/inode/path identity、callback进入operation mutex/分配、ready-as-truth、
  source lock内notify/drop、final target close进入epoll，以及 epoll target 意外成功。

#### 2B write subset、验证与关闭

- write subset限于 `anemone-kernel/src/fs/{mod.rs,file.rs}`、
  `anemone-kernel/src/fs/epoll/{mod.rs,watch.rs,ready.rs,file.rs}`，以及为2A finding所需的同subset局部修正；不得
  进入 syscall adapter、Signal、userspace、LTP或contract文件。
- 运行 `just fmt all --check`、`git diff --check` 与
  `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`。若保留可选 KUnit，本 checkpoint
  必须证明它只驱动production transition且能够编译；实际QEMU执行并入2D repository wrapper，2B不得把
  Not Run写成runtime proof。
- 独立 concurrency review必须覆盖callback-safe dirty publication、claim后pending obligation、generation
  reuse、LT fairness、ONESHOT commit/rollback、epoll-file refresh/publication window、多个waiter与closing。
  dirty + sequence不能闭合 ET 或 empty-wait lost wake时立即停止，不能先进入adapter层掩盖协议失败。
- 2B关闭证明 core protocol可进入ABI接线；不开放syscall、不切contract、不授权2C。

### Checkpoint 2C - ABI / Focused Oracle

#### Linux ABI、flags 与 errno policy

- `epoll_create1` 只接受 `EPOLL_CLOEXEC`；未知 flags 返回 `EINVAL`。adapter 先 reserve fd，再完整创建
  `Epoll`、anonymous file、`FileDesc` 与静态 description ops，最后一次 commit；任一步失败都由 reservation
  rollback，userspace 不观察半发布 fd。
- `anemone_abi::fs::linux::epoll` 定义 16-byte `#[repr(C)]` event：`events` offset 0、padding 4 bytes、
  `data: u64` offset 8，并用 compile-time size/offset assertion锁住 RV64/LA64 layout。adapter按 bytes执行
  unaligned copyin/copyout；`data` 完全 opaque，不参与 kernel identity。
- 两个 architecture syscall表增加 asm-generic `20/21/22/441`。kernel只注册 `epoll_create1`、
  `epoll_ctl`、`epoll_pwait`、`epoll_pwait2`；libc wrapper继续承接无独立 asm-generic number的
  `epoll_create` / `epoll_wait`。
- 支持 `EPOLLIN`、`EPOLLOUT`、`EPOLLET`、`EPOLLONESHOT`。`EPOLLERR/EPOLLHUP` 输入位不构成 interest，
  但internal subscribe始终加入ERROR/HANG_UP interest，target snapshot为真时始终输出。`EPOLLPRI/EPOLLRDHUP`
  等当前无 source truth的已知 bits在 ctl时接受并记录 compatibility notice，不能伪造 readiness；source以后
  形成真实 internal bit时再删除该 notice。
- `EPOLLWAKEUP` 当前没有 suspend-visible效果，按带关键注释与 notice的静默兼容处理；移除条件是系统形成
  suspend/wakeup-source owner。`EPOLLEXCLUSIVE` 的并发可见语义不能静默兼容，返回 `EINVAL` 并记录 notice；
  未知 event bits同样 `EINVAL`。
- ctl opcode只接受 ADD/MOD/DEL；ADD/MOD要求可读的 16-byte event，DEL忽略 pointer。除前述特殊项外，
  fd/entry错误按 `EBADF`、`EINVAL`、`EEXIST`、`ENOENT`、`EPERM` 映射；不支持的 source resource/capacity
  failure保留 owner error。timerfd route固定容量16是既有 owner-local可观察上界，第17个 live route的
  `Unsupported -> EPERM` 不触发本阶段重写 foundation contract。
- `epoll_pwait` timeout按毫秒解析：负值按Linux compatibility视为无限（`-1`是canonical wrapper输入）、
  `0`为nonblocking、正值为relative duration。`epoll_pwait2` nullable `TimeSpec` 使用严格
  `tv_sec >= 0`、`0 <= tv_nsec < 1e9`校验，null为无限。
  两者在 mask非空时要求 `sigsetsize == size_of::<LinuxSigSet>()`，清除不可屏蔽的 SIGKILL/SIGSTOP并复用
  `TemporarySigMaskToken`；Signal owner增加 `TemporaryMaskWaitContext::EpollPwait` 字符串分类，不复制
  pending/reservation/restore truth。ready/timeout/error先 restore；signal/force经现有 classifier收口。

#### Focused userspace test

- 新增单一 `anemone-apps/epoll-test` app，复用 `anemone-rs` 的 typed Linux wrapper；不在 app内复制 raw
  syscall number、`epoll_event` layout或 runner。app按 case输出稳定 PASS/FAIL summary，失败以非零退出，
  `user-test` 通过现有 fork/exec/wait owner在 LTP前运行并要求 exit 0。
- ABI/errno组覆盖 create1 flags、16-byte layout、unaligned ADD/MOD event pointer、invalid epfd/target/op/event、
  duplicate ADD、missing MOD/DEL、same/nested epoll拒绝、snapshot-only/unsupported target `EPERM`、
  `maxevents`/bad output pointer与copyout fault rollback。
- pipe语义组覆盖 ready-at-ADD、LT重复交付、ET无重复且新 transition可再次交付、ONESHOT成功后disable与MOD
  rearm、ERR/HUP independent bits、user data replacement、coalescing、`maxevents` round-robin fairness。
- lifecycle组覆盖 dup epfd、dup target、close original alias仍live、最后 target close lazy retirement、fd number
  reuse不继承旧 user data、DEL/MOD与late callback、epoll instance final close teardown，以及 `poll(epfd)`
  empty/ready/consume后的readability。
- wait/race组用真实 pipe producer与bounded iteration覆盖 notification-vs-harvest、empty-vs-route publication、
  ET callback during harvest、LT requeue与多个waiter串行harvest；禁止 test-only callback/state-machine副本。
- pwait组不用 socket，覆盖 temporary sigmask + EINTR、millisecond zero/finite timeout、pwait2 null/finite/invalid
  timespec、mask非空时正确/错误 `sigsetsize` 与基本 pwait2 ready交付。它是本阶段对 pwait/pwait2 target的主要
  runtime oracle，不能因五个 socket-based LTP被排除而省略。

#### 2C write subset、验证与关闭

- write subset限于 kernel ABI/syscall adapter manifest、`anemone-kernel/src/task/sig/delivery.rs`、
  `anemone-abi/src/{fs.rs,syscall/{riscv.rs,loongarch.rs}}`、`anemone-rs/src/{sys/linux.rs,os/linux.rs}`、
  `anemone-apps/epoll-test/**`，以及为adapter暴露的真实finding所需的2A/2B局部修正。不得进入user-test、LTP、
  rootfs或current contract。
- 运行 ABI/layout、syscall number/table、unaligned byte-copy、errno、temporary-mask restore与user-pointer
  containment audit；运行 `just fmt all --check`、`git diff --check`、
  `just app build epoll-test --arch riscv64` 与 `just app build epoll-test --arch loongarch64`。随后串行运行
  `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G` 与
  `just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G`；不能把kernel/app build或static
  layout assertion写成runtime证据。
- candidate syscall table/handlers只服务当前stack的2D验证，不能作为2C独立提交对外生效、不能创建
  `EPOLL-*` current contract，也不能被其它功能依赖。ABI/focused oracle若要求改变R0 layout、flags/errno、
  pwait target或acceptance boundary，停止并进入Target Renegotiation Gate。
- 2C关闭只允许进入harness/integration；focused app此时完成构建但尚未形成runtime PASS，不授权2D。

### Checkpoint 2D - Integration / Cutover

#### LTP group 与 profile 角色

新建 `epoll` group，只执行不依赖 nested epoll或 socket的 14 项：

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
```

- `epoll_ctl04/05` 作为注释保留，明确依赖首版 target外的 nested epoll；`epoll_wait05` 注明依赖 socket
  persistent readiness。开发者已接受 `epoll_pwait01..05` 五项不做测试，同样只以注释记录 socketpair原因，
  不把它们计入 attempted、PASS、TCONF或closure分母。
- glibc执行 `epoll_create02`；musl root在 `LTP_ROOTS.disabled_cases` 增加精确的 `epoll_create02`，原因是
  musl wrapper在 syscall前丢弃 legacy size，kernel无法观察测试的 invalid input。该排除只限musl root，
  不能扩成两个 libc都跳过或伪造 TPASS。
- profile不增加组合别名。开发期可临时只选 `epoll`；最终只需一次 `epoll` + `iomux` 组合运行，证明
  epoll consumer与已完成 cutover的 poll/select在同一次启动共存。运行后恢复调用者原
  `anemone-apps/user-test/ltp/profile.txt`，profile diff不得进入 checkpoint。

#### 2D write subset、验证输入与进入边界

- write subset限于 `anemone-apps/user-test/{src/main.rs,src/ltp/config.rs,ltp/groups/epoll.txt}`、
  `conf/rootfs/pretest-{rv64,la64}.toml`、验证时临时且必须恢复的 `ltp/profile.txt`、前序finding所需的manifest内
  局部修正，以及最终contract/RFC/transaction/navigation surfaces。不得新增build wrapper、test framework、
  source production修改或未列contract。
- 2D进入前必须确认2A-2C分别Closed、candidate handlers仍未独立合入、三个`EPOLL-*` ID仍Not Effective，且
  current profile/master image baseline已记录。任一前序checkpoint存在未关闭finding或证据被后续diff失效，
  必须先回到对应subset修正并重跑，不得直接用最终组合run覆盖。

#### Contract cutover 与失败原子性

Checkpoint 2D执行Stage 2唯一 `EPOLL-CUTOVER`：

- 新建 epoll current-contract owner，单一 `protocol.md` 共同保存 `EPOLL-WATCH-001`、
  `EPOLL-READY-001`、`EPOLL-FILE-001`，因为三项由同一 instance owner、operation mutex、lifecycle与
  closure matrix共同变化/证明；不拆一条 ID 一个文件。
- contract正文引用 effective `IOMUX-POLL-*`、`OPENED-DESC-LIVENESS-001` 与
  `SIGNAL-TEMP-MASK-*`，明确各域local obligation；不复制 source predicate、task lifecycle或Signal truth。
- 同一 cutover开放四个 syscall handlers、更新 RFC/transaction/navigation并把三个 ID从 Not Effective
  切换为 Active。任一 code/build/runtime/review/docs条件失败时全部保持 Not Cut Over；不得只发布ABI、只发布
  core、把target内失败登记为limitation，或让新 contract描述尚未生效的代码。

#### 2D验证与 review

1. 运行 ABI/layout、syscall table、FileOps identity/lease、fd reservation/publication、source poll分类与禁止
   搜索 audit；固定 LTP source和两套image executable清单按transaction记录版本/哈希，不把私人绝对路径写成
   公共接口。
2. 运行 `just fmt all --check`、`git diff --check` 与 `mdbook build docs`。
3. 通过仓库入口分别运行 `just app build epoll-test --arch riscv64` 与
   `just app build epoll-test --arch loongarch64`，核对 `build/` exports；不以 app-local target目录作为证据。
4. 与其它 architecture build串行运行
   `just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G`。这只证明 LA64 build，不冒充
   LA64 runtime、SMP、hardware或LTP。
5. 临时选择 `epoll`、`iomux` profile，运行一次
   `./scripts/run-user-test-rv64.sh <preliminary-rv64-sdcard-image> build/epoll-stage2-rv64.log`。wrapper必须完成
   repository-owned rootfs/kernel build、全部enabled KUnit、`epoll-test`、glibc/musl组合matrix与正常关机；
   随后恢复profile并复核caller选择的master image未写入。
6. 逐case记录 focused summary、glibc/musl LTP attempted/PASS/FAIL/TCONF/disabled与明确排除；
   `epoll_pwait01..05` 不运行，musl `epoll_create02` 记为libc-wrapper disabled，nested/socket cases不冒充
   target regression。任何target内 FAIL、panic、deadlock、timeout、lost wake、stale user data或unexpected
   unsupported都在cutover前停止。
7. 对完整2A-2D stacked diff做 architecture/concurrency/ABI/resource review，重点检查operation mutex释放点、
   callback-safe dirty publication、generation reuse、ADD/MOD rollback、DEL/close retirement、ET pending、LT
   fairness、ONESHOT copyout commit、epoll-file publication window、unaligned UAPI与Signal restore。finding在
   原owner checkpoint subset内修正并重跑受影响证据；不另建形式化review gate。

#### 2D停止条件

- dirty bitmap + sequence不能在callback-safe、无分配、无operation-mutex条件下闭合 ET/empty-wait lost wake，
  或需要把callback payload提升为readiness truth；
- ADD/MOD/DEL只有通过source强持watch、同步cancel/drain、target长期strong hold、close-driven target callback
  或第二alive truth才能正确；
- epoll-file pollability必须开放nested epoll、引入跨instance graph owner或让wait重订阅全部targets；
- 真实 ABI/LTP证据要求改变16-byte layout、syscall numbers、首版flags/errno、pwait target或接受边界，而非
  修复adapter实现；
- fixed source capacity（包括timerfd 16 routes）无法作为owner-local resource failure诚实暴露，必须改写
  effective foundation contract；
- 实现需要修改scheduler/wait core、source production owner、socket、architecture trap/IPI、`Event`、
  public contract delta或未列文件。命中后先停止，按影响报告write-set expansion、Route Correction或
  Target Renegotiation Gate；不得用compat branch、日志或弱化测试绕过。

#### 2D退出条件

- create/ctl/pwait/pwait2、watch identity/liveness、ADD/MOD/DEL rollback、dirty/ready/harvest/copyout、
  LT/ET/ONESHOT、epoll-file pollability与teardown全部满足本节协议；source与Signal/current task contract
  无第二truth。
- focused app两架构build通过；RV64 closure中focused全部PASS，14项LTP对glibc/musl形成可归因matrix，
  accepted exclusions精确保持，iomux组合无新regression且正常关机；LA64 release build通过。
- profile和master image恢复/未写入；SMP>1、LA64 runtime、hardware、broad/final harness等未运行项逐项
  写明，不从docs/build/RV64外推。
- 完整diff无未关闭Apollyon、Keter或Euclid；三个current contract ID、RFC状态、transaction cutover证据与
  navigation在2D同一cutover生效。Stage 2 Closed后RFC R0才回到 Closed。

### Resolved Write Set Manifest

允许修改的 kernel/core 文件：

- `anemone-kernel/src/fs/mod.rs`
- `anemone-kernel/src/fs/file.rs`
- `anemone-kernel/src/fs/iomux/{mod.rs,subscription.rs}`
- `anemone-kernel/src/fs/epoll/{mod.rs,watch.rs,ready.rs,file.rs}`（新增）
- `anemone-kernel/src/task/files.rs`
- `anemone-kernel/src/task/sig/delivery.rs`

允许修改的 kernel ABI/syscall adapter：

- `anemone-kernel/src/fs/api/iomux/mod.rs`
- `anemone-kernel/src/fs/api/iomux/epoll/{mod.rs,create.rs,ctl.rs,wait.rs}`（新增）
- `anemone-abi/src/fs.rs`
- `anemone-abi/src/syscall/{riscv.rs,loongarch.rs}`

允许修改的 userspace/test/rootfs 文件：

- `anemone-rs/src/sys/linux.rs`
- `anemone-rs/src/os/linux.rs`
- `anemone-apps/epoll-test/{Cargo.toml,Cargo.lock,app.toml,src/main.rs}`（新增）
- `anemone-apps/user-test/src/main.rs`
- `anemone-apps/user-test/src/ltp/config.rs`
- `anemone-apps/user-test/ltp/groups/epoll.txt`（新增）
- `conf/rootfs/pretest-{rv64,la64}.toml`
- `anemone-apps/user-test/ltp/profile.txt`：只允许验证时临时选择 `epoll` + `iomux`；必须恢复且diff不得
  进入checkpoint

允许修改的 contract/RFC/transaction/navigation：

- `docs/src/rfcs/epoll/{index.md,invariants.md,implementation.md}`
- `docs/src/devlog/transactions/2026-07-26-epoll.md`
- `docs/src/contracts/epoll/{index.md,protocol.md}`（cutover时新增）
- `docs/src/{contracts.md,SUMMARY.md,rfcs.md}`
- `docs/src/devlog/transactions/index.md`
- `docs/src/devlog/2026-07-20_to_2026-08-02.md`

Validation-only 输入：

- `anemone-kernel/src/fs/{anonymous/mod.rs,pipe.rs,eventfd.rs,timerfd.rs}`、
  `anemone-kernel/src/fs/fanotify/{file.rs,group.rs,queue.rs}`、
  `anemone-kernel/src/device/{console.rs,tty/file.rs,tty/terminal.rs,block/devfs.rs,char/devfs.rs}`
- snapshot-only poll owners under `anemone-kernel/src/fs/{devfs,ext4,proc,ramfs}/**`；只做分类/audit，不修改
- `anemone-kernel/src/fs/api/iomux/{wait.rs,ppoll.rs,pselect6.rs}` 与所有 `task/files.rs` publication callers
- 固定 LTP epoll source、调用者明确选择的初赛 RV64 master image与其中glibc/musl executable；它们只作为
  只读验证输入，私人路径不进入公共命令接口
- repository `Justfile`、`scripts/xtask` 与 `scripts/run-user-test-rv64.sh`；只核对/使用现有入口，不修改

不得修改：

- `anemone-kernel/src/sched/**`、wait core、除已列context枚举外的Signal owner、architecture trap/IPI、`Event`
- pipe/eventfd/timerfd/fanotify/TTY/socket等source production code；Stage 2只消费已生效subscription contract
- `task::files` publication/refcount/final-release语义、`FileDescOps`形状或dynamic final-release observer；只增加
  lease窄操作面并使用既有static hook
- nested epoll、socket persistent readiness、register/current-limitations、LTP固定source与master image
- build orchestration、额外wrapper、通用test framework或未列current contract

若真实owner boundary需要触碰未列文件，worker必须先报告文件、原因、contract/ABI/验证影响；批准后先更新
本manifest并在transaction记录扩集，再继续。不能把未列source或owner逻辑塞进已列adapter规避扩展。

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
