# 2026-06-20 - Threaded Timer Event

**状态：** Active
**负责人：** doruche, Codex
**领域：** time / timer / scheduler / kthread / timerfd / signal
**权威计划：** [RFC-20260620-threaded-timer-event](../../rfcs/threaded-timer-event/index.md), [不变量需求](../../rfcs/threaded-timer-event/invariants.md), [迁移实施计划](../../rfcs/threaded-timer-event/implementation.md)
**当前阶段：** 阶段 0/1

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
