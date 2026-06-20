# 2026-06-20 - Threaded Timer Event

**状态：** Completed
**负责人：** doruche, Codex
**领域：** time / timer / scheduler / kthread / timerfd / signal
**权威计划：** [RFC-20260620-threaded-timer-event](../../rfcs/threaded-timer-event/index.md), [不变量需求](../../rfcs/threaded-timer-event/invariants.md), [迁移实施计划](../../rfcs/threaded-timer-event/implementation.md)
**当前阶段：** 阶段 5 - 旁路审计与收口已关闭；第一版 threaded timer event 完成

## 范围

本事务跟踪 `threaded-timer-event` RFC 的第一版实现：

- 新增显式 threaded timer completion lane，并保留 IRQ timer lane；
- 通过通用 `Late` initcall 启动 per-CPU threaded timer worker；
- 迁移 `timerfd` 和 `ITIMER_REAL` 到 bounded process-context completion；
- 保持 wait-core timeout、物理取消、drain、worker pool、periodic timer core 和通用 workqueue 为非目标。

## 不变量

- timer IRQ 仍是 deadline 到期检测中心；threaded worker 只执行已到期投递的 completion。
- IRQ lane 和 threaded lane 必须由调用点显式选择。
- IRQ 投递路径不执行 threaded callback，不拿普通锁，不阻塞，不丢弃或合并 event。
- threaded callback 可以在 process context 获取普通锁，但只能执行 bounded timer completion。
- timer core 不提供 per-object identity、serializing、periodic core、physical cancel、drain 或 lifetime pinning。
- `timerfd` / `ITIMER_REAL` 的 generation / validness stale filtering 和对象状态真相源仍归各自 owner。
- `Late` initcall 只表达启动窗口，不表达 kthread / service policy；`kthreadd` 继续由 boot path 手动初始化。
- worker 未经批准不得越过分配的 write set；需要扩展时必须先上报 write-set expansion request 并等待批准。

## 阶段日志

### 2026-06-20 - 阶段 0/1 启动与文档闭合

**阶段：** 阶段 0 - Preflight 与模块边界；阶段 1 - Late Initcall 基础设施。

**变更：** 在代码实现前建立事务日志，并把实现前 active Euclid proof gaps 折回 canonical 文本。本步骤未启动 worker；阶段 0/1 由主控直接执行。

**当前代码落点：**

- Git 基线：分支 `dev/drc/threaded-timer`，阶段启动时工作树干净。
- soft timer owner 仍是 `anemone-kernel/src/time/timer.rs`，当前只有 IRQ queue、`schedule_local_irq_timer_event()` 和占位 `schedule_threaded_timer_event()`。
- `schedule_local_irq_timer_event()` 调用面只有 wait-core timeout、`timerfd` 和 `ITIMER_REAL`。第一版迁移清单因此保持为 `timerfd` / `ITIMER_REAL`；wait-core timeout 留在 IRQ lane。
- kthread core 已提供 `KThreadBuilder::cpu()`、strong `KThreadHandle::wake()` 和 `KThreadCtx::wait_until()`。`wake()` 只是 `Event::publish()` wake edge，business predicate 仍归 consumer；Gate P1 仍必须证明 timer core-owned per-CPU worker slot 在 IRQ 投递时按 `cur_cpu_id()` 本地 wake。
- 当前 initcall 只有 `Fs` / `Driver` / `Probe`；`bsp_kinit()` 手写调用 `task::kthread::init_kthreadd()`，随后等 `FINISH_SYNC_COUNTER` 确认所有 CPU online，再手写启动 inode shrinker 和 OOM killer。
- `kthreadd` 仍是 boot path 手动 invariant，不挂 initcall。

**文档闭合：**

- RFC 状态改为 `Accepted for Implementation`，并链接本事务日志。
- Gate P1 validation floor 补充 KUnit、boot smoke 或临时 self-check 中至少一种运行证据；临时 self-check 必须在 transaction devlog 记录证据并在收口前删除。
- Gate P1 / P2 / P3 补充 `Exit` 字段。
- `ITIMER_REAL` signal action commit point 折回 [不变量需求](../../rfcs/threaded-timer-event/invariants.md)：callback 在 itimer state lock 下确认 token 有效并生成 `SIGALRM` / rearm action 即 completion commit，释放锁后无条件执行该 action。
- 阶段 4 / Gate P3 validation floor 明确为本地 LTP `alarm02`、`alarm03`、`alarm05`、`alarm06`、`alarm07`、`getitimer01`、`getitimer02`、`setitimer01`、`setitimer02`，并要求源码审计或等价 smoke 覆盖 real-only、interval rearm、锁外 `recv_signal()` 和 stale no-op。

**模块边界预检：**

- 阶段 0 不需要拆分 `time/timer.rs`。当前 timer file 尚未混合 threaded lane、worker slot、ready queue 和 diagnostics；若阶段 2 继续向单文件塞入这些职责，允许先做同 owner split-only checkpoint。

**阶段 1 write set：**

- `anemone-kernel/src/initcall.rs`
- `anemone-kernel/crates/kernel-macros/src/initcall.rs`
- `conf/arch/riscv64/kernel.lds.in`
- `conf/arch/loongarch64/kernel.lds.in`
- `anemone-kernel/src/arch/link_symbols.rs`
- `anemone-kernel/src/main.rs`
- `anemone-kernel/src/fs/inode_shrinker.rs`
- `anemone-kernel/src/mm/oom.rs`

**停止条件：**

- `Late` 需要依赖 late consumer 之间的相对顺序。
- `kthreadd` 需要迁入 initcall。
- inode shrinker / OOM killer 迁移需要改变 worker loop、threshold、wake 或 victim policy。
- Phase 1 需要越过上述 write set，或需要把 initcall failure 变成 recoverable boot error。

**验证计划：** 阶段 1 代码落地后运行 `just fmt kernel --check`、`git diff --check` 和 `just build`；文档导航变更补充 `mdbook build docs`。

### 2026-06-20 - 阶段 1 Late initcall 基础设施

**阶段：** 阶段 1 - Late Initcall 基础设施。

**变更：**

- 新增 `InitCallLevel::Late`，注释说明它是通用晚期启动窗口，不是 kthread / service 专用阶段。
- `#[initcall(late)]` 加入 macro accepted-level list。
- rv64 / la64 linker script 增加 `.initcall.late` section 和 `__sinitcall_late` / `__einitcall_late` 符号。
- `link_symbols` 增加 late initcall 起止符号。
- `bsp_kinit()` 在 `task::kthread::init_kthreadd()` 和 `FINISH_SYNC_COUNTER` 之后、`mount_rootfs()` / `exec_init_proc()` 之前调用 `run_initcalls(InitCallLevel::Late)`。
- `fs::inode_shrinker` 和 `mm::oom` 的 worker init 迁移为各自模块内 `#[initcall(late)]` 入口。

**边界确认：**

- `kthreadd` 仍由 boot path 手动初始化，没有挂 initcall。
- inode shrinker / OOM killer 只改变启动分发方式，没有改变 worker loop、threshold、wake 或 victim policy。
- late initcall 之间没有新增显式或隐式顺序依赖；当前两个 consumer 仍各自只依赖 ordinary kthread 已可 spawn、所有 CPU online。
- 本阶段没有触碰 timer core、timerfd、itimer 或 wait-core timeout。

**验证：**

- `just fmt kernel --check`：未通过，但诊断只在既有 generated `anemone-kernel/src/kconfig_defs.rs` 和 `anemone-kernel/src/platform_defs.rs` 格式换行 / 尾随空白，不涉及本阶段修改文件；本事务未手改 generated 文件。
- `git diff --check`：通过。
- 新增事务文件 `git diff --no-index --check -- /dev/null docs/src/devlog/transactions/2026-06-20-threaded-timer-event.md`：无 whitespace 诊断；非零退出码是新增文件与 `/dev/null` 比较的正常 no-index difference 状态。
- `mdbook build docs`：通过，输出到 `docs/book`。
- `just build`：通过。
- source audit：`rg -n "init_inode_shrinker|init_oom_killer|#\\[initcall\\(late\\)\\]|InitCallLevel::Late|__sinitcall_late|__einitcall_late" anemone-kernel/src anemone-kernel/crates/kernel-macros conf/arch` 确认 late section、macro、`bsp_kinit()` hook 和两个 late consumer；没有 `init_kthreadd` late initcall。

**结论：** 阶段 1 gate 已关闭。下一步进入阶段 2 Gate P1：Timer Core 双 Lane 与 Per-CPU Worker 基础设施。阶段 2 开始前应重新检查 `time/timer.rs` 是否需要同 owner split-only checkpoint，并按 Gate P1 要求准备运行证据。

### 2026-06-20 - 阶段 2 Timer Core 双 Lane 与 Per-CPU Worker

**阶段：** 阶段 2 - Timer Core 双 Lane 与 Per-CPU Worker 基础设施；Gate P1 - Threaded Lane Skeleton。

**变更：**

- 将 `anemone-kernel/src/time/timer.rs` 按同一 timer owner 内职责拆分为 `time/timer/{mod.rs, irq.rs, threaded.rs}`。拆分不改变 public API 名称：`schedule_local_irq_timer_event()` 继续由 `time::timer` re-export。
- `TimerEvent` 增加显式 lane：`Irq` 到期后仍在 timer interrupt 中执行 callback；`Threaded` 到期后只投递到本 CPU threaded-ready queue 并唤醒 worker。
- 新增 `schedule_threaded_timer_event(expire, callback) -> ()`。第一版不返回 recoverable allocation failure；若本 CPU worker 未初始化，用 assertion 暴露内核初始化不变量破坏。
- 新增 per-CPU `THREADED_READY_QUEUE` 和 `THREADED_WORKER` slot。ready queue 使用普通 `VecDeque<Box<dyn FnOnce() + Send + 'static>>`，外层由 `NoIrqSpinLock` 保护；根据用户裁定，本阶段允许 IRQ 到期投递路径执行该队列可能产生的普通内存分配，不采用 intrusive node。
- `#[initcall(late)] init_threaded_timer_workers()` 为每个 `0..ncpus()` 创建 `timer-thread/<cpu>` kthread，使用 `KThreadBuilder::cpu(CpuId::new(cpu))` pin 到目标 CPU，并把创建请求 CPU 与返回的 `KThreadHandle` 发布到 timer core-owned per-CPU worker slot。
- IRQ 到期投递 threaded event 时按 `cur_cpu_id()` 读取本 CPU worker slot，在 `wake()` 前执行 `slot.cpu == cur_cpu_id()` 断言；断言失败表示 timer worker 发布或 CPU 绑定不变量破坏，不做 remote fallback。
- worker 使用 `KThreadCtx::wait_until(ready_queue_not_empty)`，predicate 只读取 timer core ready queue truth；`KThreadControl` 只提供 pure wake edge。worker 每次从 ready queue 弹出一个 callback，释放 ready queue lock 后执行 callback。
- 新增诊断计数：threaded schedule submit、IRQ dispatched、worker wake、worker drain、callbacks executed、ready queue high-water、workers spawned。诊断字段不参与 timerfd / itimer 语义决策。
- 新增 KUnit `threaded_timer_callback_runs_outside_hwirq`，通过真实 `schedule_threaded_timer_event()` 安排 1ms callback，callback 断言不在 hwirq、IRQ enabled、preemption allowed，并通过 `Event` 唤醒测试方。

**边界确认：**

- 本阶段未迁移 `timerfd`、`ITIMER_REAL` 或 wait-core timeout。`schedule_local_irq_timer_event()` 调用面仍只有 wait-core timeout、`timerfd` 和 `ITIMER_REAL`。
- IRQ lane 行为保持：`TimerLane::Irq` 仍在 `on_timer_interrupt()` 中直接执行原 callback。
- threaded lane 不提供取消、drain、periodic core、per-object merge、per-object serializing 或 worker pool。
- timer core 不读取 worker `Task`、sched state 或 kthread control 内部状态；本地性 proof 只来自 timer core-owned slot CPU 字段和 `KThreadBuilder::cpu()` 创建请求。
- `handle.wake()` 下游源码审计：`KThreadHandle::wake()` 只调用 `KThreadControl::wake()`，后者是 `Event::publish()`；对本 CPU pinned timer worker，wait-core `wake_enqueue()` 进入 local placement，不需要 remote IPI。若 worker placement contract 未来改变，本地性断言会在 timer core 边界暴露。
- threaded-ready queue 使用普通 `VecDeque`，IRQ path 可能在 `push_back()` 时按 allocator 当前 noirq-capable contract 分配；这是本阶段按用户裁定接受的实现取舍，不引入 user-visible rollback、event drop 或 merge。

**验证：**

- `git diff --check`：通过。
- `just build`：通过。
- `just fmt kernel --check`：未通过，但诊断仍限于既有 generated `anemone-kernel/src/kconfig_defs.rs` 和 `anemone-kernel/src/platform_defs.rs` 格式换行 / 尾随空白；本阶段修改文件无 formatter 诊断。
- Gate P1 运行证据：`timeout 20s just xtask qemu --platform qemu-virt-rv64 --image build/anemone.elf` 启动当前 KUnit kernel，新增 `anemone_kernel::time::timer::threaded::kunits::threaded_timer_callback_runs_outside_hwirq...ok`，随后全量 KUnit 打印 `All tests passed!`。命令最终由外层 `timeout` 在用户态 benchmark 继续运行时发送 SIGTERM，非 KUnit / boot 失败。
- source audit：`rg -n "schedule_local_irq_timer_event|schedule_threaded_timer_event|threaded timer|#\\[initcall\\(late\\)\\]" anemone-kernel/src/time anemone-kernel/src/task anemone-kernel/src/sched anemone-kernel/src/fs/timerfd.rs` 确认 threaded API 只有 timer core 与新增 KUnit 使用，`timerfd` / `ITIMER_REAL` / wait-core timeout 仍在 IRQ lane。

**结论：** 阶段 2 Gate P1 已具备关闭证据。下一步进入阶段 3：迁移 `timerfd` 到 threaded schedule API，重点保持 generation stale filtering、missed-tick accounting、read / poll readiness 和普通路径不发布 armed-but-unscheduled 状态。

### 2026-06-20 - 阶段 3 Timerfd threaded completion

**阶段：** 阶段 3 - Timerfd 迁移；Gate P2 - Timerfd Vertical Slice。

**变更：**

- `anemone-kernel/src/fs/timerfd.rs` 将 `schedule_timerfd_callback()` 从 `schedule_local_irq_timer_event()` 切换到 `schedule_threaded_timer_event()`；`timerfd` 不再提交 IRQ timer callback。
- `timerfd` 注释改为 bounded threaded completion 约束：callback 不是后台任务，generation stale filtering、missed-tick accounting、trigger handoff 和 periodic rearm 仍由 timerfd state 拥有。
- `settime()` 和 periodic rearm 在释放 timerfd state lock 前提交对应 threaded event；普通路径不把 armed generation 暴露给 reader 后再补交 timer-core event。
- `read()` / `poll()` 增加 `refresh_due_expiration_locked()`：如果 reader/poller 在 threaded callback 获得 CPU 前观察到 timer 已过期，就在 timerfd state lock 下按当前时间推进 missed-tick accounting，并递增 generation 让已排队的旧 callback stale no-op。该路径只使用 timerfd object-owned generation，不给 timer core 增加 per-object identity、cancel 或 merge 语义。

**边界确认：**

- `timerfd` 的单一真相源仍是 `TimerFdState`：`schedule`、`generation`、`expirations`、read/poll triggers 和 stage-1 `TFD_TIMER_CANCEL_ON_SET` no-op 状态都未下沉到 timer core。
- `timerfd` callback 不持有 timer core ready queue lock；trigger batch 仍在对象锁下 detach、锁外 trigger/drop。
- `CLOCK_BOOTTIME` 和 `TFD_TIMER_CANCEL_ON_SET` stage-1 限制不因本阶段扩大；`timerfd04` 仍不纳入第一版验收。
- wait-core timeout 和 `ITIMER_REAL` 仍保留 `schedule_local_irq_timer_event()`，未被本阶段隐式迁移。

**验证：**

- `git diff --check`：通过。
- `just build`：通过。
- `just fmt kernel --check`：未通过，但诊断仍限于既有 generated `anemone-kernel/src/kconfig_defs.rs` 和 `anemone-kernel/src/platform_defs.rs` 格式换行 / 尾随空白；本阶段修改文件无 formatter 诊断。
- source audit：`rg -n "schedule_local_irq_timer_event|schedule_threaded_timer_event" anemone-kernel/src` 确认 `timerfd` 只使用 threaded API；剩余 IRQ timer consumer 是 wait-core timeout 和 `ITIMER_REAL`，符合阶段 3 边界。
- source audit：`rg -n "Stage-1 bridge|IRQ callback|threaded timer|background job|read_refresh|poll_refresh|armed generation" anemone-kernel/src/fs/timerfd.rs anemone-kernel/src/task/itimer.rs anemone-kernel/src/time/timer` 确认 `timerfd` 旧 IRQ bridge 注释已移除，新的 bounded threaded completion / lazy read-poll accounting 注释存在。
- `./scripts/run-user-test-rv64.sh rootfsconfig-rv etc/sdcard-rv.img build/threaded-timer-phase3-timerfd-rv64.log`：脚本完成，kernel KUnit `All tests passed!`；timerfd LTP profile 中 glibc 和 musl 的 `timerfd01`、`timerfd02`、`timerfd_create01`、`timerfd_gettime01`、`timerfd_settime01` 均 PASS。`timerfd01` 的 50ms periodic sequential case 在两套 runtime / 两个 clock 上均返回 `got 3 tick(s)`。
- 同一 LTP run 中 `timerfd_settime02` 仍在测试 setup 阶段 `TBROK`：`tst_taint.c` 打开 `/proc/sys/kernel/tainted` 得到 `ENOENT`，因此没有进入 timerfd CANCEL_ON_SET race body；本轮没有证据显示 timerfd race 语义失败。

**实现期反馈：**

- `timerfd01` 首次 threaded run 暴露 `got 2 tick(s) expected 3`：read/poll 可以先看到已有 `expirations > 0` 并返回，导致尚未执行的 threaded callback 没有机会把下一次已到期 interval 纳入 read 结果。修复没有改变 accepted contract；它把 overdue accounting 保持在 timerfd state owner 内，并用 generation 过滤旧 callback。
- `timerfd_settime02` 的剩余失败属于 procfs/sysctl 观察面缺口，不是 timerfd code path。LTP 源码 `timerfd_settime02.c` 使用 `.taint_check = TST_TAINT_W | TST_TAINT_D`，LTP `lib/tst_taint.c` 固定读取 `/proc/sys/kernel/tainted`。

**Write-set expansion request：**

- 原阶段 3 write set 只允许 `anemone-kernel/src/fs/timerfd.rs`，必要时触碰 timer API；不包含 procfs。
- 为完成 Gate P2 的 `timerfd_settime02` runtime validation，需要批准一个最小 procfs/sysctl 扩展：在 `anemone-kernel/src/fs/proc/sys/kernel/` 下新增只读 `tainted` PDE，内容为 `0\n`，并在 `kernel/mod.rs` 注册。
- 该扩展不改变 threaded timer RFC accepted contract，不改变 timerfd ABI；它只补齐 LTP taint helper 所需的只读观察面。批准后验证 gate 为 `git diff --check`、`just build`、`./scripts/run-user-test-rv64.sh rootfsconfig-rv etc/sdcard-rv.img build/threaded-timer-phase3-timerfd-rv64.log` 复跑 timerfd profile，并把 `timerfd_settime02` 结果追加到本事务日志。

**结论：** 阶段 3 timerfd 代码迁移和 `timerfd01` / read-poll missed-tick 保护项已通过验证；Gate P2 尚不能关闭，因为 `timerfd_settime02` 的 runtime evidence 被 `/proc/sys/kernel/tainted` 缺失阻断。下一步等待 write-set expansion 裁定：若批准，先补最小 procfs tainted 观察面并复跑 timerfd profile；若不批准，将 `timerfd_settime02` 记为 procfs infra validation gap，阶段 3 不能声明完整关闭。

### 2026-06-20 - 阶段 3 write-set expansion 与 Gate P2 closeout

**阶段：** 阶段 3 - Timerfd 迁移；Gate P2 - Timerfd Vertical Slice closeout。

**裁定：**

- 用户批准阶段 3 最小 write-set 扩展：引入只读 `/proc/sys/kernel/tainted`，当前固定返回 `0\n`。
- 用户裁定 `timerfd_settime02` 当前被 user-test 60s per-case timeout 杀掉，暂时不是 Phase 3 主要问题，本阶段不用继续处理该 timeout。

**变更：**

- 新增 `anemone-kernel/src/fs/proc/sys/kernel/tainted.rs`，作为 procfs/sysctl stage-1 观察面返回 `0\n`。
- 在 `anemone-kernel/src/fs/proc/sys/kernel/mod.rs` 注册 `tainted` PDE。
- 该扩展不改变 threaded timer accepted contract，不改变 timerfd ABI；只解除 LTP taint helper 的 setup 阻断。

**验证：**

- `git diff --check`：通过。
- `git diff --no-index --check -- /dev/null anemone-kernel/src/fs/proc/sys/kernel/tainted.rs`：无 whitespace 诊断；非零退出码是新增文件与 `/dev/null` 比较的正常 no-index difference 状态。
- `just fmt kernel --check`：未通过，但诊断仍限于既有 generated `anemone-kernel/src/kconfig_defs.rs` 和 `anemone-kernel/src/platform_defs.rs` 格式换行 / 尾随空白；本阶段修改文件无 formatter 诊断。
- `just build`：通过。
- `./scripts/run-user-test-rv64.sh rootfsconfig-rv etc/sdcard-rv.img build/threaded-timer-phase3-timerfd-rv64.log`：复跑完成，kernel KUnit `All tests passed!`；glibc / musl 的 `timerfd01`、`timerfd02`、`timerfd_create01`、`timerfd_gettime01`、`timerfd_settime01` 均 PASS。`timerfd01` 的 sequential 50ms 分支在两套 runtime / 两个 clock 上均返回 `got 3 tick(s)`。
- 同一复跑中 `timerfd_settime02` 不再因 `/proc/sys/kernel/tainted` 缺失在 setup 阶段 `TBROK`；case 进入 fuzzy race body。glibc / musl 两次均被 user-test 的 60s per-case timeout 杀掉，结果为 `FAIL LTP CASE timerfd_settime02 : 124`，最终 summary 为 `attempted=12 passed=10 failed=0 infra_failed=2 skipped=0`。

**结论：** 阶段 3 Gate P2 按用户裁定关闭。`timerfd` 已迁移到 threaded completion；read/poll missed-tick 保护、generation stale filtering、trigger batch 锁外触发和 stage-1 `CLOCK_BOOTTIME` / `TFD_TIMER_CANCEL_ON_SET` 边界保持。`timerfd_settime02` 的剩余 60s timeout 记录为 runner/runtime 验证缺口，不作为本阶段 timerfd 语义 blocker。下一步进入阶段 4：`ITIMER_REAL` 迁移。

### 2026-06-20 - 阶段 4 ITIMER_REAL threaded completion

**阶段：** 阶段 4 - ITIMER_REAL 迁移；Gate P3 - ITIMER_REAL Migration。

**变更：**

- `anemone-kernel/src/task/itimer.rs` 将 `ThreadGroup::set_real_itimer()` 的 schedule path 从 `schedule_local_irq_timer_event()` 切换到 `schedule_threaded_timer_event()`；`ITIMER_REAL` 不再提交 IRQ timer callback。
- 保留 thread-group itimer state 作为单一真相源：`expire_at`、`interval` 和 `validness` 仍由 `RealITimer` 拥有，timer core 不解释对象 identity、generation 或 rearm 语义。
- `set_real_itimer()` 在 itimer state lock 下替换 state，并在解锁前提交 threaded event；普通路径不暴露 armed state 后再补交 timer-core event。
- threaded callback 在 itimer state lock 下确认 `validness` token 与当前 state 匹配；one-shot timer 完成后 disarm，periodic timer 在 state lock 下更新下一次 `expire_at` 并提交 successor event。
- callback 在 state lock 下只生成 `SIGALRM` action commit，释放 itimer state lock 后再调用 `ThreadGroup::recv_signal()`；`recv_signal()` 不再在 itimer state lock 内执行。
- 替换旧 IRQ-lock 临时注释为 bounded threaded-completion 注释：`ITIMER_REAL` callback 不是后台任务，stale filtering、interval rearm 和 signal action commit point 仍归 thread-group itimer state。

**边界确认：**

- 本阶段未触碰 `time/itimer/api/setitimer.rs`；`setitimer()` errno / old-value 顺序未改变。
- `ITIMER_VIRTUAL`、`ITIMER_PROF`、POSIX timer、timer overrun 和 alarm clock 仍为非目标；非 real 分支仍由 syscall adapter 返回当前 stage-1 unsupported 结果。
- `NoIrqSpinLock` 暂时保留。迁移只改变 completion context，不在本阶段改 itimer state lock 类型或 owner boundary。
- 本阶段没有引入物理取消、drain、periodic timer core、per-object merge、worker pool 或 wait-core timeout 迁移。
- `schedule_local_irq_timer_event()` 全局搜索后只剩 wait-core timeout 和 IRQ API 定义；`timerfd` 与 `ITIMER_REAL` 均使用 threaded lane。

**验证：**

- `git diff --check`：通过。
- `just build`：通过。首次运行曾因 `sdcard-rv.img` 被 QEMU 临时锁住而停在 DTB 生成；重试后完整构建通过，Rust 编译 `anemone-kernel` 成功。
- `just fmt kernel --check`：未通过，但诊断仍限于既有 generated `anemone-kernel/src/kconfig_defs.rs` 和 `anemone-kernel/src/platform_defs.rs` 格式换行 / 尾随空白；本阶段修改文件无 formatter 诊断。
- source audit：`rg -n "schedule_local_irq_timer_event|schedule_threaded_timer_event" anemone-kernel/src` 确认 `schedule_local_irq_timer_event()` 调用面只剩 `sched/mod.rs` 的 wait-core timeout、IRQ API 定义和 re-export；`timerfd`、`ITIMER_REAL` 和 threaded timer KUnit 使用 `schedule_threaded_timer_event()`。
- source audit：`rg -n "Stage-1 bridge|IRQ callback|threaded timer|bounded threaded|background job|recv_signal\\(|validness" anemone-kernel/src/fs/timerfd.rs anemone-kernel/src/task/itimer.rs anemone-kernel/src/time/timer` 确认旧 `ITIMER_REAL` IRQ bridge 注释已移除，`recv_signal()` 调用位于 itimer state lock block 之后，stale token 和 bounded threaded completion 注释存在。
- `./scripts/run-user-test-rv64.sh rootfsconfig-rv etc/sdcard-rv.img build/threaded-timer-phase4-itimer-rv64.log`：使用临时定向 profile 运行 RFC Gate P3 指定 cases，运行后已恢复 profile 文件；kernel KUnit `All tests passed!`。
- 同一 LTP run 中 glibc / musl 的 `alarm02`、`alarm03`、`alarm05`、`alarm06`、`alarm07` 均 PASS。
- 同一 LTP run 中 glibc / musl 的 `getitimer02`、`setitimer02` 均 PASS。
- 同一 LTP run 中 `getitimer01` 和 `setitimer01` 的完整 case 在 glibc / musl 均报告 FAIL，但失败仅来自 `ITIMER_VIRTUAL` / `ITIMER_PROF` 非目标分支被 LTP 作为同一 case 的失败计入。`ITIMER_REAL` 分支证据通过：`getitimer01` 的 real 分支 `getitimer()`、`setitimer()`、interval snapshot 和 timer value range 均 TPASS；`setitimer01` 的 real 分支 `sys_setitimer()`、old-value interval snapshot 和 child `SIGALRM` 均 TPASS。
- 定向 LTP summary：glibc `attempted=9 passed=7 failed=2 infra_failed=0 skipped=0`，musl `attempted=9 passed=7 failed=2 infra_failed=0 skipped=0`；总计 `attempted=18 passed=14 failed=4 infra_failed=0 skipped=0`。4 个 failed 是 `getitimer01` / `setitimer01` 在两套 runtime 的 VIRTUAL/PROF 非目标分支聚合失败，不作为 Gate P3 blocker。

**结论：** 阶段 4 Gate P3 关闭。`ITIMER_REAL` 已迁移到 threaded completion；stale callback no-op、interval rearm、real-only 范围和锁外 `SIGALRM` 投递均有源码审计与定向运行证据。下一步进入阶段 5：旁路审计与收口。

### 2026-06-20 - 阶段 5 旁路审计与收口

**阶段：** 阶段 5 - 旁路审计与收口。

**审计结果：**

- `schedule_local_irq_timer_event()` 调用面只剩 `sched/mod.rs` 的 wait-core timeout、IRQ API 定义和 re-export；`timerfd` 与 `ITIMER_REAL` 不再使用 IRQ timer callback。
- `schedule_threaded_timer_event()` 调用面只在第一版允许的 `fs/timerfd.rs`、`task/itimer.rs` 和 timer core KUnit。
- `#[initcall(late)]` 调用面只有 `time::timer::threaded::init_threaded_timer_workers()`、`fs::inode_shrinker::init_inode_shrinker()` 和 `mm::oom::init_oom_killer()`；`kthreadd` 仍由 `bsp_kinit()` 手动初始化。
- `on_timer_interrupt()` 仍是 deadline 到期检测中心。`TimerLane::Irq` 在 IRQ context 执行 callback；`TimerLane::Threaded` 只调用 `threaded::enqueue_expired_threaded()` 投递 ready queue 并唤醒本 CPU worker。
- threaded ready queue 由 `NoIrqSpinLock` 保护；IRQ 投递路径只在 ready queue 中 `push_back()` callback、更新诊断 high-water、按 `cur_cpu_id()` 读取 timer core-owned worker slot，并在 `wake()` 前断言 `slot.cpu == cur_cpu_id()`。
- `KThreadHandle::wake()` 仍是 pure wake capability：只调用 `KThreadControl::wake()`，后者发布 `Event` wake edge；业务请求真相仍在 timer ready queue。
- timer worker 每次从 ready queue 弹出 callback 后释放 queue lock，再执行 callback；timer core 不持锁进入 `timerfd` 或 itimer 对象 callback。
- `timerfd` 注释保留 bounded threaded completion、generation filtering、missed-tick accounting、trigger handoff 和 periodic rearm 约束；旧 IRQ bridge 注释已移除。
- `ITIMER_REAL` 注释保留 bounded threaded completion、stale filtering、interval rearm 和 signal action commit point；`recv_signal()` 调用位于 itimer state lock block 之后。
- wait-core timeout 仍留在 IRQ lane。理由与 RFC 一致：它绑定 `WakeToken`、timeout callback、signal/force/cancel/source trigger race 和 `finish()` outcome mapping；若后续要迁移，必须另走 wait-core contract review。
- `ANE-20260616-LTP-POST-SUMMARY-HANG` 未被本 RFC 关闭。当前证据只能证明 `timerfd` / `ITIMER_REAL` threaded migration 闭合，不能证明 post-summary hang 根因属于本 RFC 范围。

**验证：**

- `git diff --check`：通过。
- `mdbook build docs`：通过。
- 未重新运行 `just build`、QEMU 或 LTP：阶段 5 只做源码审计和文档收口，未改内核代码；阶段 1-4 的 `just build`、KUnit、timerfd profile 与 itimer/alarm 定向 LTP 证据已在前文记录。
- `just fmt kernel --check` 在阶段 1-4 均只因既有 generated `kconfig_defs.rs` / `platform_defs.rs` 格式问题失败；阶段 5 未改 Rust 源码，未重新运行。

**结论：** 第一版 threaded timer event 完成。IRQ lane 与 threaded lane 调用面清晰，late initcall 调用面清晰，`timerfd` 与 `ITIMER_REAL` 已迁移到 bounded process-context completion，wait-core timeout 保持显式非目标；文档层没有发现需要回写 RFC accepted boundary 的实现反馈。
