# 2026-06-01 - Sched Wait Refactor

**Status:** Active
**Owners:** doruche, Codex
**Area:** scheduler / event / timer / signal / wait core
**Canonical Plan:** [RFC-20260601-sched-wait-refactor](../../rfcs/sched-wait-refactor/index.md), [Invariants](../../rfcs/sched-wait-refactor/invariants.md), [Implementation Plan](../../rfcs/sched-wait-refactor/implementation.md)
**Current Phase:** phase 1 complete; phase 2 pending

## Scope

本事务跟踪 `Event` wake race 对应的 scheduler wait 重构，从迁移前计划审查开始，直到 wait identity、统一 wait completion、stale-safe placement、park latch、mode-blocked listener requeue、timeout/signal/cancel 旁路收口和验证证据全部闭合。

非目标：

- 不重写调度策略、调度类或时间片策略。
- 不一次性完成 futex PI、poll/epoll 完整语义或 Linux waitqueue 全功能兼容。
- 不通过放宽 `task_enqueue()` 断言掩盖竞态。

## Invariants

- 一轮等待必须有稳定 `WaitState` 身份，旧 wake token 不能完成新 wait。
- event wake、timeout、signal 和主动 cancel 必须竞争同一个 wait core 状态。
- 逻辑 wake/cancel 与 task sched-state 更新必须有唯一线性化点。
- wake 成功后的 physical placement 必须由 wait core 触发 stale-safe `wake_enqueue()`。
- `Event` 只维护 listener 队列和 exclusive 策略，不直接修改 task 调度状态。
- mode-blocked listener 回挂必须通过短寿命 permit 再校验。
- 关键状态转换和异常分支必须保留可打开的 debug/trace 观测点。

## Phase Log

### 2026-06-01 - 迁移前计划收口

**Phase:** pre-implementation planning

**Change:** 将原 `Event WaitState/WakeToken` 计划提升为公开 RFC 目录：[RFC-20260601-sched-wait-refactor](../../rfcs/sched-wait-refactor/index.md)。RFC 内部拆分为 canonical 不变量需求文档和迁移实施计划。实施计划明确阶段边界：第 1 阶段只建立 wait core 骨架；`wake_wait()` / `wake_active_wait()` 在阶段 2 补齐 stale-safe placement 和 park latch 前不得接入生产等待路径。

**Audit:** 复审重点是实施顺序是否会制造半套协议。结论是：必须避免“逻辑 wake 已完成，但 placement 仍由 Event/timer/signal 适配层补做”的中间态。计划已补充阶段前置条件、旁路审计和 `update_status_with()` 收口要求。

**Observability:** 实施计划新增可观测性要求：wait core、`wake_enqueue()`、Event publish、mode-blocked requeue、timeout/signal/cancel 关键分支都要保留 debug/trace 记录点。日志字段至少能关联 task id、wait identity、reason、mode、状态转换结果和 placement 结果。

**Validation:** 本阶段是文档与协议审查，未运行构建或 QEMU 验证。

**Next:** 开始阶段 1 前，先确认旧等待路径仍完整保留，新 wait core 骨架不会被生产路径误用；阶段 1 完成后把旁路分类结果记录到本事务日志或对应 progress 文件。

### 2026-06-01 - 阶段 1 wait core 骨架

**Phase:** phase 1 - scheduler wait core skeleton

**Change:** 新增 `sched::wait` 模块，建立 `WaitState`、`WakeToken`、`WaitGuard`、`BeginWait`、`WaitReason`、`WakeMode`、`WaitResult`、`WaitOutcome` 和 `WakeResult` 等 wait-core 类型。`WaitState` 保存本轮等待的稳定指针身份、状态、创建 task 和创建时间，不持有强 `Arc<Task>` 或 `Event` 回指。`WaitGuard` 不实现 clone，`WakeToken` 只暴露 wait identity 诊断和指针身份比较。

**Change:** 将 `Task` 的内部状态字段从旧 `TaskStatus` 换成 `TaskSchedState`。新状态区分 `Runnable`、带 `Arc<WaitState>` 和 `ParkState` 的 wait-core `Waiting`、迁移期 `LegacyWaiting` 以及 `Zombie`。`task.status()` 保持只读兼容投影，观察者仍只看到旧 `TaskStatus`。

**Change:** 增加 `begin_wait()`、`cancel_wait()`、`finish_wait()` 的阶段 1 事务骨架：它们通过 `Task::update_sched_state_with()` 在同一个 NoIrq 调度状态事务中写入或清理 wait-core `TaskSchedState::Waiting`，并在 debug 日志中记录 task id、wait identity、reason 和 outcome。

**Compatibility:** `Task::update_status_with()` 保留为迁移期兼容写入口，输入输出仍是旧 `TaskStatus`，但内部会投影到 `TaskSchedState::LegacyWaiting` / `Runnable` / `Zombie`。现有 Event、timeout、signal、exit 等生产路径继续完整走旧协议，没有被接到新 wait core。

**Compatibility:** `wake_wait()` 和 `wake_active_wait()` 只作为 `sched::wait` 内部受控骨架存在，返回 `WakeResult::DisabledUntilWakePlacement`，没有 re-export 给外部生产路径。阶段 2 完成 stale-safe `wake_enqueue()` 和 park latch 前，不允许把它们接入 Event、timeout、signal 或 cancel 路径，避免出现“逻辑 wake 已完成但 physical placement 未由 wait core 负责”的半套协议。

**Audit:** 搜索 `begin_wait()`、`cancel_wait()`、`finish_wait()`、`wake_wait()`、`wake_active_wait()` 的调用点，确认新 wait-core API 只在 `sched::wait` 内定义，未被 Event、timeout、signal 接入。旧旁路仍集中在 `try_to_wake_up()`、`notify()`、`schedule_with_timeout()` 和生产路径的 `update_status_with()` 调用上，后续阶段继续按计划迁移。

**Validation:** 运行 `just build` 通过。构建目标为当前配置的 LoongArch64 kernel release + `fs_ext4` + `kunit`，只剩既有 `sync/mono.rs` unused import warning。

**Next:** 进入阶段 2 前，需要新增 stale-safe `wake_enqueue()`，把 `schedule()` 的 park latch / abort-park 接入 `TaskSchedState::Waiting { park, .. }`，并把 `wake_wait()` / `wake_active_wait()` 补齐为“逻辑完成提交后由 wait core 执行一次 stale-safe placement”的完整入口。

## Open Items

- 阶段 2：补齐 stale-safe `wake_enqueue()`、park latch / abort-park 和完整 wake API。
- 阶段 3：迁移 `Event` listener identity、publish、exclusive quota 和 mode-blocked requeue。
- 阶段 4：迁移 timeout、signal 和主动 cancel。
- 阶段 5：审计旧旁路并收缩旧 wake API。
- 阶段 6：运行已知触发 profile，并保存带 debug/trace 的验证摘要。

## Closure

事务尚未收口。完成时需要记录最终验证命令、late wake / stale placement / mode-blocked requeue / timeout-signal 竞争的观测证据，以及剩余限制或 register 链接。
