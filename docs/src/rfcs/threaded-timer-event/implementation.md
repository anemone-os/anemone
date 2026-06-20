# Threaded Timer Event 迁移实施计划

**状态：** Active
**最后更新：** 2026-06-20
**父 RFC：** [RFC-20260620-threaded-timer-event](./index.md)
**不变量：** [不变量需求](./invariants.md)

本文只定义第一版 threaded timer event 的实现路线、gate、write set、验证 floor 和反馈边界。它不替代 `index.md` / `invariants.md` 的 accepted contract；实现期若发现目标、上下文合同、取消语义、wait-core 边界或 ABI 验收需要变化，必须先回到文档层更新 RFC canonical 文本。

## 迁移原则

- 先补 timer core 基础设施，再迁移 consumer。`timerfd` / `ITIMER_REAL` 不应先各自做局部 kthread workaround。
- 保持两个显式 API：IRQ lane 继续服务 IRQ-safe callback，threaded lane 服务 bounded process-context completion。
- schedule 阶段优先承担需要 process context 的资源准备；第一版不把 allocation failure 作为 recoverable ABI 错误处理。当前 heap / page allocator 是 noirq-capable，IRQ 到期投递路径可以执行简单、bounded allocation，但只能用于 event 投递和 worker wake，不能阻塞、reclaim、丢弃/合并 event 或映射用户态 errno。
- queue truth 留在 `time::timer`；kthread control 只提供 pure wake / wait capability，不存储业务请求状态。
- 本轮新增通用 `Late` initcall level；它只表达启动时刻，不表达“会创建 kthread”。threaded timer worker、inode shrinker 和 OOM killer 可以作为各自子系统的 late consumer 接入。
- `kthreadd` 继续由 boot path 手动初始化，不迁入 initcall；它是所有 ordinary kthread late consumer 的前置锚点。
- late initcall 之间不表达依赖顺序。若某个 consumer 需要依赖另一个 late consumer，必须显式建模或回到文档层。
- 第一版只迁移 `timerfd` 和 `ITIMER_REAL`。wait-core timeout 的调用面必须被旁路审计确认未被隐式迁移。
- 实现反馈可以优化阶段顺序、write set 和验证 floor，但不能引入物理取消、periodic core、worker pool、per-object merge、wait-core timeout 迁移或通用 workqueue 语义。

## 阶段 0：Preflight 与模块边界

前置条件：

- RFC `index.md` / `invariants.md` 已经记录第一版目标和非目标。
- 当前代码仍以 `anemone-kernel/src/time/timer.rs` 作为 soft timer owner。

交付：

- 审计 `schedule_local_irq_timer_event()` 调用面，确认第一批迁移只覆盖 `timerfd` 和 `ITIMER_REAL`。
- 审计 kthread / Event 能力：`KThreadBuilder::cpu()`、`KThreadHandle::wake()`、`KThreadCtx::wait_until()` 是否仍满足 per-CPU 常驻 worker + pure wake wait，并确认 IRQ 投递路径可以按 `cur_cpu_id()` 取 timer core-owned worker slot、断言 `slot.cpu == cur_cpu_id()` 后再 `wake()`。
- 审计当前 initcall level、linker section、macro accepted-level list 和 `bsp_kinit()` 手写 late consumer 启动位置，确认 `Late` 插入点是所有 CPU online 之后、用户态 `init` exec 之前。
- 做模块边界预检：如果 `time/timer.rs` 同时承载 IRQ lane、threaded lane、worker init、event type、queue diagnostics 后开始混合职责，允许同一 owner 内行为保持拆分为 `time/timer/{mod.rs, irq.rs, threaded.rs}` 或等价结构；拆分不得改变 public API 或 timer 语义。

审计：

```sh
rg -n "schedule_local_irq_timer_event|schedule_threaded_timer_event" anemone-kernel/src
rg -n "KThreadBuilder|KThreadHandle|wait_until|Event::new|publish|wake_enqueue|send_ipi_wait_result" anemone-kernel/src/task anemone-kernel/src/sched anemone-kernel/src/exception
rg -n "InitCallLevel|initcall\\(|__sinitcall|__einitcall|init_inode_shrinker|init_oom_killer|init_kthreadd" anemone-kernel/src anemone-kernel/crates/kernel-macros conf/arch
```

write set：

- `docs/src/rfcs/threaded-timer-event/*`，仅用于实现反馈需要回写 RFC 文本时
- `anemone-kernel/src/time/timer.rs` 或同 owner 的 `anemone-kernel/src/time/timer/` 拆分文件
- 只读审计：`anemone-kernel/src/task/kthread/*` 其它文件、`anemone-kernel/src/sched/event.rs`
- 只读审计：`anemone-kernel/src/initcall.rs`、`anemone-kernel/crates/kernel-macros/src/initcall.rs`、`conf/arch/*/kernel.lds.in`、`anemone-kernel/src/main.rs`

验证：

- 文档层：`git diff --check`。
- 若本阶段只做文档或只读审计，不运行 build。

退出条件：

- 明确 timer core 是否需要拆分。
- 明确第一批迁移调用点清单。
- 明确 worker wake 复用 kthread pure wake，而不是新建第二套 wait primitive；timer core 通过自己维护的 per-CPU worker slot 在 IRQ 投递路径断言目标 worker 属于本 CPU。
- 明确 `Late` initcall 插入点和本轮迁移的 late consumer 清单。

## 阶段 1：Late Initcall 基础设施

前置条件：

- 阶段 0 已确认 `Late` 的语义窗口：`Fs` / `Driver` / `Probe` 完成，`kthreadd` 已手动初始化，所有 CPU online，用户态 `init` 尚未 exec。

交付：

- 新增 `InitCallLevel::Late`，语义注释说明它是通用晚期初始化阶段，不是 kthread / service 专用阶段。
- `#[initcall(late)]` 进入 macro accepted-level list。
- 两套 arch linker script 增加 `.initcall.late` section 起止符号。
- `link_symbols` 增加 `__sinitcall_late` / `__einitcall_late`。
- 在 BSP late boot 窗口调用 `run_initcalls(InitCallLevel::Late)`：位置保持在 `task::kthread::init_kthreadd()` 之后、`FINISH_SYNC_COUNTER` 确认所有 CPU online 之后、`mount_rootfs()` / `exec_init_proc()` 之前。
- 将 `fs::init_inode_shrinker()` 和 `mm::oom::init_oom_killer()` 从 `bsp_kinit()` 手写调用迁移为各自模块内的 `#[initcall(late)]` 包装入口。

审计：

- `kthreadd` 不挂 initcall。
- late initcall 函数不接收参数、不返回 recoverable boot error；初始化失败仍按当前 panic / assertion 边界处理。
- inode shrinker 和 OOM killer 迁移只改变启动分发方式，不改变 worker loop、threshold、wake 或 victim policy。
- late initcall 之间不得依赖相对顺序；timer worker 接入后也不得要求 inode shrinker / OOM killer 先后。

write set：

- `anemone-kernel/src/initcall.rs`
- `anemone-kernel/crates/kernel-macros/src/initcall.rs`
- `conf/arch/riscv64/kernel.lds.in`
- `conf/arch/loongarch64/kernel.lds.in`
- `anemone-kernel/src/arch/link_symbols.rs`
- `anemone-kernel/src/main.rs`
- `anemone-kernel/src/fs/inode_shrinker.rs`
- `anemone-kernel/src/mm/oom.rs`

验证：

- `just fmt kernel --check`
- `git diff --check`
- `just build`
- Gate P1 关闭前还必须提供至少一种运行证据：KUnit、boot smoke 或临时 self-check。临时 self-check 只能服务本 gate，证据进入 transaction devlog，收口前必须移除。

退出条件：

- `#[initcall(late)]` 可用。
- boot late 窗口仍启动 inode shrinker 和 OOM killer，且不再由 `bsp_kinit()` 手写逐个调用。
- `kthreadd` 初始化仍为手动 boot invariant，未被 initcall 隐藏。

## 阶段 2：Timer Core 双 Lane 与 Per-CPU Worker 基础设施

前置条件：

- 阶段 1 已提供 `Late` initcall 基础设施。
- 阶段 0 已确认 timer write set 和拆分策略。

交付：

- 在 timer core 内把 event 表达为显式 lane：IRQ event 和 threaded event 都仍按 deadline 进入本 CPU timer queue。
- 保留 `schedule_local_irq_timer_event()` 的既有语义：到期后在 IRQ context 执行 callback。
- 新增 threaded schedule API，建议形态是 `schedule_threaded_timer_event(expire, callback) -> ()`。资源准备发生在 schedule path；真实 OOM 按当前内核不可恢复错误处理，不映射为 `SysError::OutOfMemory` 或用户态 errno。
- schedule threaded event 时优先准备本 CPU threaded-ready queue 所需节点 / 容量；如果实现选择 IRQ 到期投递时分配节点，必须证明该 allocation 使用当前 noirq-capable heap / page allocator，路径简单、bounded，并且失败不进入用户可见 rollback、丢弃或合并 event。
- 新增 timer core 初始化入口，例如 `time::timer::init_threaded_timer_workers()`。
- 增加 `#[initcall(late)]` wrapper，在 timer core late init 中为每个 `0..ncpus()` 创建一个 pinned kthread，建议命名 `timer-thread/<cpu>` 或等价稳定 debug name。
- 每个 worker 使用 `KThreadBuilder::new(...).cpu(CpuId::new(cpu)).spawn(...)` 创建，handle 和创建请求 CPU 存在 timer core-owned per-CPU worker slot 中。该 slot CPU 是 timer owner 的本地性 proof，不扩大 `KThreadHandle` public surface。
- worker entry 使用 `KThreadCtx::wait_until(|| ready_queue_not_empty_for_this_cpu)` 等待；predicate 读取 timer core queue truth，`KThreadControl` 只提供 wake edge。
- timer IRQ 到期处理时只把 threaded event 移入本 CPU ready queue，按 `cur_cpu_id()` 取本 CPU worker slot，在 `wake()` 前断言 `slot.cpu == cur_cpu_id()`，然后唤醒 worker；断言失败表示 worker 发布/绑定 bug，不做 remote wake fallback。
- worker 被唤醒后 drain 本 CPU ready queue：在 no-IRQ / spin lock 下取出一批 event，释放 queue lock 后执行 callback。
- 增加 backlog 诊断计数或日志阈值；该诊断不得改变调度、丢弃或合并 event。
- callback 执行前后更新诊断计数；callback panic 按现有 kernel panic 语义处理，不吞掉错误。

审计：

- IRQ path 中不得出现复杂/阻塞 allocation、reclaim、普通 `Mutex::lock()`、blocking wait 或 callback execution。
- `schedule_local_irq_timer_event()` 调用面不需要同步迁移。
- ready queue capacity / event node 可以在 schedule path 准备，也可以通过当前 noirq-capable allocator 做简单分配；不能在 IRQ path 暴露 recoverable 资源不足分支，不能因资源不足丢弃或合并 event。
- worker queue lock 不得跨 callback。
- worker 不持有 timer core queue lock 进入对象 callback。
- worker 不需要 stop/drain/restart API；handle 只由 timer core 保存并用于 wake。
- timer core 不得读取 worker `Task`、sched state 或 kthread control 内部状态；若实现必须查询实际 placement，必须先回到 kthread-core RFC 更新 handle contract。
- `handle.wake()` 下游必须经源码审计证明在本地 worker wake 路径不走 remote IPI、blocking placement 或复杂分配。
- 若 worker 未初始化而 schedule threaded event，使用 assertion 暴露内核 bug，不返回普通错误。

模块边界预检：

- 如果 typed lane、ready queue、worker slot、worker loop、diagnostics 与 legacy IRQ timer 混在一个文件里导致职责不清，先做同 owner split-only checkpoint，再继续 feature code。
- 拆分只允许移动 timer owner 内部结构；不得把 `sched`、`kthread` 或 `timerfd` 私有状态塞进 timer core。

write set：

- `anemone-kernel/src/time/timer.rs` 或 `anemone-kernel/src/time/timer/{mod.rs, irq.rs, threaded.rs}`
- 只在需要公开 API 路径时调整 `anemone-kernel/src/time/mod.rs`
- 如阶段 1 尚未落地：`initcall` / linker / macro / `main.rs` late hook；否则本阶段不直接编辑 `main.rs`
- 只读审计：`anemone-kernel/src/task/kthread/*`、`anemone-kernel/src/sched/*`、`anemone-kernel/src/exception/ipi.rs`

可观测性：

- threaded schedule submit 计数。
- IRQ 到期投递计数。
- threaded-ready queue backlog high-water 或阈值日志。
- worker spawned count。
- worker wake count。
- worker drain count / callbacks executed。

验证：

- `just fmt kernel --check`
- `git diff --check`
- `just build`

退出条件：

- IRQ lane 行为保持。
- 每个 online CPU 有一个常驻 worker handle。
- IRQ 投递 threaded event 前能断言 timer core worker slot 属于本 CPU，worker 随后能在 process context 执行 callback。
- 搜索确认 IRQ 投递路径不执行 threaded callback、不做复杂/阻塞分配、不拿普通锁。
- `schedule_local_irq_timer_event()` 和 wait-core timeout 仍保持原路径。

## 阶段 3：Timerfd 迁移

前置条件：

- 阶段 2 已提供可执行 threaded callback 的基础设施。
- `timerfd` 当前 state/generation/missed-tick 逻辑已通过源码审计确认单一真相源仍在 `TimerFdState`。

交付：

- 将 `schedule_timerfd_callback()` 切换到 threaded schedule API。
- 调整 `timerfd_settime()` 的 publish 顺序，确保普通路径不会留下 armed-but-unscheduled：
  - 若 value 为 0，继续按 disarm 处理。
  - 若需要 future callback，必须让对象 armed state 的发布与 event 已提交到 timer core 的事实保持一致。
  - 替换旧 timer 时，不围绕 allocation-failure rollback 设计；但普通路径不能先取消旧 timer 再留下没有 queued event 的新 armed state。
- 保持 generation stale filtering。
- 保持 `account_due_expiration_locked()` 按 `now` 计算 missed expirations，周期 timer 不退化成 worker 一次只加一。
- 保持 trigger batch 在对象锁外触发 / drop。
- 替换旧 IRQ bridge 注释为 bounded threaded-completion 注释。

审计：

- `timerfd` callback 可以拿普通锁，但不得持 timer core queue lock。
- callback 中周期 rearm 使用 threaded API。allocation failure 不作为 ABI 错误返回；panic / assertion 边界之外，普通路径仍必须保证 rearm 后的对象 state 与 queued event 一致。
- `TFD_TIMER_CANCEL_ON_SET`、`CLOCK_BOOTTIME` stage-1 限制不变。

反馈假设：

- 可能发现 `timerfd_settime()` 当前先发布 state 再 schedule 的结构需要较大重排。只要保持普通路径的 state/event 一致和 missed-tick 语义，这属于实现计划内反馈；若必须把 allocation failure 纳入用户可见 settime 语义，停止并更新 RFC。

write set：

- `anemone-kernel/src/fs/timerfd.rs`
- 如 API error type 需要显式转换，可触碰 `anemone-kernel/src/time/timer.rs`

验证：

- `just fmt kernel --check`
- `git diff --check`
- `just build`
- 复用 LTP timerfd profile；重点覆盖 `timerfd01`、`timerfd02`、`timerfd_create01`、`timerfd_gettime01`、`timerfd_settime01`、`timerfd_settime02`。`timerfd04` 仍不因本 RFC 纳入第一版验收。

退出条件：

- `timerfd` 不再使用 IRQ timer callback。
- `timerfd` missed-tick、read readiness、poll readiness 和 stage-1 limits 未退化。
- 普通路径不会产生 armed-but-unscheduled。

## 阶段 4：ITIMER_REAL 迁移

前置条件：

- 阶段 3 已证明 threaded callback lane 可支持对象 timer。

交付：

- 将 `ThreadGroup::set_real_itimer()` 的 schedule path 切到 threaded timer API。
- 重排 `ITIMER_REAL` publish 顺序，确保替换旧 timer 时普通路径不发布不可触发的新 state。
- 保持 `validness` 或替换为等价 generation token，stale callback 必须 no-op。
- 保持 `SIGALRM` 投递走现有 signal / thread-group 路径；completion 应在 itimer state lock 内生成投递动作，释放 itimer state lock 后再调用 `recv_signal()`。
- interval rearm 仍由 thread-group itimer state 决定。
- 替换 IRQ-lock 临时注释为 bounded threaded-completion 注释。

审计：

- callback 运行在 process context 后，原 `NoIrqSpinLock` 是否仍需要保留由实现期评估；若换成普通 `Mutex` 会改变锁语义和 state owner，必须单独说明。第一版可以先保留 `NoIrqSpinLock`，只移出 IRQ context。
- `recv_signal()` 不应在 itimer state lock 内执行；若实现期证明必须持锁投递 signal，必须先回写锁序证明并关闭 tracking issue。
- 第一版不因 threaded schedule allocation failure 改变 `setitimer()` errno 行为；真实 OOM 属于 panic / assertion 边界，不能伪装成普通 ABI 成功或失败。
- 不引入 virtual/prof/POSIX timer。

write set：

- `anemone-kernel/src/task/itimer.rs`
- `anemone-kernel/src/time/itimer/api/setitimer.rs` 只在 errno / old-value 顺序需要显式处理时触碰

验证：

- `just fmt kernel --check`
- `git diff --check`
- `just build`
- 复用本地 LTP itimer / alarm case：`alarm02`、`alarm03`、`alarm05`、`alarm06`、`alarm07`、`getitimer01`、`getitimer02`、`setitimer01`、`setitimer02`。若当前 profile 没有独立 itimer 组，使用定向 profile 或 smoke 覆盖这些 case，并补充源码审计确认 real-only、interval rearm、锁外 `recv_signal()` 和 stale no-op。

退出条件：

- `ITIMER_REAL` 不再使用 IRQ timer callback。
- stale callback、interval rearm 和 `SIGALRM` 投递语义保持现有 stage-1 能力。
- 普通路径不会取消旧 timer 后发布假 armed state。

## 阶段 5：旁路审计与收口

前置条件：

- 阶段 3、4 已完成并通过 build floor。

交付：

- 搜索所有 `schedule_local_irq_timer_event()` 调用点，确认只剩 IRQ-safe 路径和 wait-core timeout。
- 搜索 `schedule_threaded_timer_event()` 调用点，确认只在第一版允许的 `timerfd` / `ITIMER_REAL` 使用，除非文档层批准新增 consumer。
- 搜索所有 `#[initcall(late)]` 调用点，确认 late consumer 只有本轮接受的 timer worker、inode shrinker、OOM killer 或经文档层批准的新增晚期服务。
- 审计 IRQ handler 到 threaded-ready 投递路径：无 callback execution、无复杂/阻塞 allocation、无普通 lock、无 blocking wait；若使用 allocation，确认仍在当前 noirq heap / page allocator contract 内。
- 审计 timerfd / itimer 注释，确认旧 IRQ bridge 说明已替换为 bounded threaded-completion 约束。
- 记录未迁移 wait-core timeout 的理由和后续触发条件。

审计命令：

```sh
rg -n "schedule_local_irq_timer_event|schedule_threaded_timer_event" anemone-kernel/src
rg -n "Stage-1 bridge|IRQ callback|threaded timer|bounded threaded" anemone-kernel/src/fs/timerfd.rs anemone-kernel/src/task/itimer.rs anemone-kernel/src/time/timer.rs
rg -n "#\\[initcall\\(late\\)\\]|InitCallLevel::Late|init_inode_shrinker|init_oom_killer|init_threaded_timer" anemone-kernel/src
```

验证：

- `just fmt kernel --check`
- `git diff --check`
- `just build`
- 复用 LTP timerfd profile。
- 复用 itimer / signal timer 相关 LTP 或已有 profile。

退出条件：

- 第一版 consumer 全部迁移。
- IRQ lane 和 threaded lane 调用面清晰。
- late initcall 调用面清晰，`kthreadd` 仍为手动 boot invariant。
- 文档层没有被实现反馈推翻的 accepted boundary。

## 旁路审计清单

- `schedule_local_irq_timer_event()`：允许保留在 wait-core timeout 和明确 IRQ-safe 短 callback；不允许 `timerfd` / `ITIMER_REAL` 继续使用。
- `schedule_threaded_timer_event()`：第一版只允许 timerfd / itimer consumer。
- `#[initcall(late)]`：第一版接受 timer worker、inode shrinker、OOM killer；`kthreadd` 不允许迁入 initcall。
- `on_timer_interrupt()`：不能执行 threaded callback；threaded 投递路径只允许当前 noirq allocator contract 下的简单、bounded allocation。
- timer worker loop：不能持 timer core queue lock 执行 callback。
- timerfd：必须保留 generation stale filtering、missed-tick accounting、trigger batch 锁外触发。
- itimer：必须保留 stale filtering、`SIGALRM` 投递路径和 real-only 范围。
  `SIGALRM` 投递不能在 itimer state lock 内执行，除非有显式锁序证明。

## 可观测性清单

第一版至少应提供能支撑 review 的内部观测点：

- threaded schedule submit count。
- IRQ threaded event dispatch count。
- worker wake count。
- worker callbacks executed count。
- ready queue high-water 或 backlog threshold warning。
- timerfd / itimer callback stale-drop debug 日志保持或等价可观测。

这些字段只服务诊断，不参与对象语义决策。

## 停止边界

必须停止并回到文档层的情况：

- 需要迁移 wait-core timeout 才能完成第一版。
- 需要物理取消、drain、periodic core、per-object merge 或 worker pool。
- 需要改变 `timerfd` missed-tick / poll / read 可见语义。
- 需要扩大 itimer 到 virtual/prof/POSIX timer。
- 需要把 allocation failure 纳入用户可见 ABI 或 rollback 合同才能保持对象状态一致。
- IRQ 投递路径无法在 noirq allocation / no ordinary lock 条件下成立。

可以作为 implementation feedback 继续推进的情况：

- `time/timer.rs` 需要同 owner split-only 结构维护。
- timerfd / itimer publish 顺序需要局部重排，但不改变用户可见语义。
- backlog 诊断字段或日志阈值需要调整。
- KUnit 是否值得添加一个小 queue/context 单元需要实现期判断。

## Probe / Vertical Slice Gates

### Gate P1 - Threaded Lane Skeleton

**Hypothesis:** timer IRQ 可以只投递 threaded event 并唤醒 per-CPU worker，worker 可在 process context 执行 callback，且不影响 IRQ lane。

**Protected Goal / Invariant:** IRQ handler 不执行 threaded callback、不做复杂/阻塞分配、不拿普通锁；IRQ 投递路径按 `cur_cpu_id()` 选择 timer core-owned worker slot 并通过 `slot.cpu == cur_cpu_id()` 断言本地性，不走 remote IPI / blocking placement；IRQ lane 行为保持；threaded event 只提供 one-shot completion。

**Minimum Write Set:** `time/timer` owner 文件和 timer core `#[initcall(late)]` wrapper。
若阶段 1 尚未完成，还包括 `initcall` / macro / linker / `link_symbols` / `main.rs` 的 `Late` 基础设施。

**Non-goals:** 不迁移 timerfd / itimer；不实现取消；不新增 worker pool。

**Validation Floor:** `just build`，加源码审计确认 `schedule_local_irq_timer_event()` 调用面未变，并确认 threaded worker wake 前有 `slot.cpu == cur_cpu_id()` 断言或等价证明。还必须提供 KUnit、boot smoke 或临时 self-check 中至少一种运行证据，证明 threaded callback 实际由 worker 执行且执行时不在 IRQ context；临时 self-check 的证据写入 transaction devlog，收口前删除。

**Failure Signal:** worker 无法常驻、IRQ 投递需要复杂/阻塞分配、`KThreadHandle::wake()` 不能从 IRQ 安全调用、timer core worker slot 无法证明本地性，或 wake path 可能走 remote IPI / blocking placement。

**Write-back Target:** 若失败只影响 init / wake route，更新本 `implementation.md`；若失败推翻 per-CPU worker / simple IRQ allocation contract，更新 `index.md` 和 `invariants.md`。

**Exit:** 成功后把运行证据、源码审计结论和任何临时 self-check 的删除记录追加到 transaction devlog，再进入 timerfd 迁移；失败时删除临时探针，按影响回写 `implementation.md`、`index.md` / `invariants.md` 或 `tracking-issues.md`，必要时登记 register / current limitations。

### Gate P2 - Timerfd Vertical Slice

**Hypothesis:** `timerfd` 能在不改变 missed-tick、read readiness、poll readiness 和 stage-1 limits 的前提下迁移到 threaded callback。

**Protected Goal / Invariant:** `TimerFdState` 继续拥有 schedule/generation/expiration truth；普通路径不产生 armed-but-unscheduled；trigger batch 锁外触发。

**Minimum Write Set:** `fs/timerfd.rs` 和 timer threaded API 调用形态调整。

**Non-goals:** 不补 time namespace、alarm clocks、`TFD_TIMER_CANCEL_ON_SET` ECANCELED、`timerfd04`。

**Validation Floor:** `just build` + 复用 LTP timerfd profile。

**Failure Signal:** 需要把 allocation failure 纳入 settime 可见语义、周期 missed-tick 退化、或 trigger/drop 边界被迫改变。

**Write-back Target:** 调整阶段 3 或不变量；若改变 ABI，停止并回 RFC review。

**Exit:** 成功后把 timerfd LTP / source-audit 证据追加到 transaction devlog，并确认没有遗留临时兼容桥；失败时回写阶段 3、相关不变量或 tracking issue，若涉及 ABI 改变则停止在 RFC review。

### Gate P3 - ITIMER_REAL Migration

**Hypothesis:** `ITIMER_REAL` 可以只迁移 completion context，保留 real-only stage-1 语义。

**Protected Goal / Invariant:** thread-group itimer state 继续拥有 `expire_at` / `interval` / stale token；普通路径不取消旧 timer 后发布假 state；`SIGALRM` 投递路径不扩大，且不在 itimer state lock 内执行。

**Minimum Write Set:** `task/itimer.rs`，必要时 `time/itimer/api/setitimer.rs`。

**Non-goals:** 不实现 virtual/prof/POSIX timer，不补 overrun。

**Validation Floor:** `just build` + 定向复用本地 LTP `alarm02`、`alarm03`、`alarm05`、`alarm06`、`alarm07`、`getitimer01`、`getitimer02`、`setitimer01`、`setitimer02`，或用等价 smoke 覆盖 stale no-op、interval rearm、real-only 和锁外 `SIGALRM` 投递；不新增专门用户态测试体系。

**Failure Signal:** 需要把 allocation failure 纳入 `setitimer()` 可见语义、signal path 需要 ABI 扩展、stale filtering 需要物理取消才能成立，或必须持 itimer state lock 投递 signal 且无法给出锁序证明。

**Write-back Target:** 调整阶段 4；若需要扩大 itimer 功能或取消语义，停止并回 RFC review。

**Exit:** 成功后把 itimer / signal 验证证据和锁序审计追加到 transaction devlog；失败时删除临时探针，回写阶段 4、`invariants.md` 或 tracking issue，若需要扩大 itimer 功能或取消语义则停止在 RFC review。
