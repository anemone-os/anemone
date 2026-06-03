# Sched Wait Refactor 迁移实施计划

日期：2026-06-01

状态：RFC canonical 迁移实施计划。本文只描述落地顺序、审计范围和验收动作；协议和不变量以 [Sched Wait Refactor 不变量需求](./invariants.md) 为准。

## 0. 迁移原则

0. 实现过程中，如非必要，每个阶段不要强行为下一阶段预留抽象或接口，否则反而可能制造错误约束。
1. 每个阶段都必须保持需求文档中的 wait identity、唯一线性化点、requeue permit、park latch 和 stale-safe placement 不变量。
2. 阶段性交付允许临时兼容 wrapper，但不得新增绕过 wait core 的普通等待完成路径。
3. 实现过程可以调整类型名和模块路径，但不能改变 API 责任归属：逻辑完成属于 wait core，wake 成功后的 stale-safe placement 也由 wait core 触发。
4. 旧 `TaskStatus` 只能作为观察投影逐步保留，不能继续作为新等待协议的写入口，也不能作为调度内部协议判断的普通入口。
5. 每个阶段完成后都要重新分类旁路命中，不能只看是否编译通过。
6. `wake_wait()` / `wake_active_wait()` 一旦被生产等待路径调用，就必须已经同时具备逻辑完成、post-commit stale-safe placement 和 park latch 闭合；不得先上线一个“只改状态、不负责 placement”的半成品入口。
7. 每个阶段开始前先确认前置阶段的禁用/启用边界：旧路径可以继续完整保留，新路径也可以完整接管，但不能让同一个等待来源同时被两套 completion 协议处理。
8. 关键状态转换和异常分支必须保留 debug / trace 可观测性；日志要带 wait identity、task id、reason、mode 和 placement 结果，避免后续 race 复审只能靠猜。

## 1. 阶段 1：建立调度等待核心

交付：

1. 新增 `sched::wait` 或等价子模块。
2. 定义 `WaitState`、`WakeToken`、`WaitGuard`、`WaitReason`、`WakeMode`、结果类型。
3. 将 `Task` 的 `status` 扩展为枚举式 `TaskSchedState`。
4. 提供只读兼容接口 `task.status()`，避免一次性改动 procfs、debug 等观察路径；调度内部应使用 `TaskSchedState` helper 或事务。
5. 新增 `begin_wait()`、`cancel_wait()`、`finish_wait()` 的事务骨架。
6. 新增 `wake_wait()`、`wake_active_wait()` 的类型和私有实现骨架，但在阶段 2 完成前不得接入 Event、timeout、signal 等生产等待路径。
7. 让 `wake_active_wait()` 只作为 sched 内部受控 helper。
8. 若为了编译保留旧 `update_status_with()` 写入口，必须把它标记为迁移期兼容路径，并禁止新 wait core 通过它完成等待。
9. 在 wait core 的 debug/trace 边界预留统一记录点，至少覆盖 begin、wake attempt、wake success/stale/mode-blocked、cancel、finish/retire。

验收：

1. 旧代码仍能构建。
2. `TaskStatus` 观察者不需要理解 `WaitState`。
3. 调度、wait、wake、enqueue 路径不通过 `task.status()` 投影判断内部协议状态。
4. 新接口文档明确线性化点、调用条件、暂未接入生产路径的禁用边界，以及 placement 责任归属。
5. `WaitState` 不持有强 `Arc<Task>` / `Event` 回指。
6. 阶段结束时，旧等待路径仍完整走旧协议；新 wait core 不应形成“完成了逻辑 wake 但没有 placement”的可调用路径。
7. 日志字段不暴露 `WaitState` 内部可变结构，但能用稳定身份区分新旧等待轮次。

## 2. 阶段 2：新增 stale-safe wake placement

交付：

1. 新增 wake 专用 `wake_enqueue()`。
2. local wake path 支持 stale、pre-park current、park pending、already queued、enqueued 等可区分结果。
3. remote wake IPI handler 改走 stale-safe 入口。
4. 保留 `task_enqueue()` 的严格断言，限制其语义为非 wake-tail placement。
5. 将 `schedule()` 的 park latch / abort-park 逻辑接入 `TaskSchedState::Waiting { park, .. }`。
6. 将 `wake_wait()` / `wake_active_wait()` 的真实实现补齐为“task sched-state 事务提交后释放锁，再调用 `wake_enqueue()`”。
7. 在 wait core 内部记录或返回 `WakeEnqueueResult` 供诊断，但不把补充入队责任暴露给调用方。
8. 为 `wake_enqueue()` 的 `Stale`、`AlreadyCurrent`、`ParkPending`、`AlreadyQueued`、`Enqueued` 分支保留 debug/trace 点，至少在非 happy path 或 stress profile 下可打开。

验收：

1. wake 尾巴不再因为 task 已进入新一轮 `Waiting` 而 panic。
2. 新建任务、bootstrap 首次入队等严格路径仍保留断言。
3. 远端 IPI wake 到达时重新验证 task 当前 placement 条件。
4. pre-park wake 和 post-park wake 都能按需求文档闭合。
5. `WakeResult::Woke` 明确表示 wait core 已执行过一次 stale-safe placement；Event、timeout、signal 不需要也不允许补调裸 `task_enqueue()`。
6. 阶段结束后，wait core 的 wake API 才允许被后续阶段接入生产等待路径。
7. 发生 late wake tail、park abort 或 stale placement 时，日志能还原 wait identity 与当前 task sched-state 的关系。

## 3. 阶段 3：改造 Event

前置条件：

1. 阶段 2 已完成，`wake_wait()` 的 `Woke` 语义已经包含 post-commit stale-safe placement。
2. `schedule()` 已经具备 park latch / abort-park 逻辑。

交付：

1. `Listener` 增加 wait identity。
2. `Listener` 相等性改为基于 `Arc::ptr_eq` 语义的 wait identity。
3. `prepare_listener()` 拆成 begin wait 与 event register 两个职责。
4. `clean_listener()` 只清 listener，不再修改 task status。
5. `Event::publish()` 改为扫描 listener 并调用 `wake_wait()`。
6. exclusive quota 按成功完成 wait 计数。
7. 增加 `requeue_blocked_listener_if_current_armed()`，用于 mode-blocked listener 的回挂前再校验。
8. 在 publish 扫描、listener detach、mode-blocked requeue 成功/失败、stale listener 丢弃和 exclusive quota 消耗处保留 debug/trace 记录点。

验收：

1. `Event::publish()` 不再调用 `try_to_wake_up()`。
2. `Event` 不直接写 `TaskStatus`。
3. 旧轮次 cleanup 不能删除新轮次 listener。
4. mode-blocked listener 在 detached 窗口中遇到 cancel、timeout、signal 或 finish 时不会重新进入 event 队列。
5. `requeue_blocked_listener_if_current_armed()` 不引入额外 generation、reservation 或公开状态面。
6. futex、mutex、wait4、vfork_done 等现有 Event 用户语义不回退。
7. `Event::publish()` 的所有 `Woke` 分支只更新 quota / 诊断，不补做 placement。
8. Event 日志能区分 stale、mode-blocked、already completed/cancelled 和 successful wake，且能关联到同一 wait identity。

## 4. 阶段 4：改造 timeout 和 signal

前置条件：

1. Event 已经不再直接写 `TaskStatus` 或调用 `try_to_wake_up()`。
2. wait core wake API 已经是完整入口，不能只提交逻辑完成。

交付：

1. `schedule_with_timeout()` 的 timer callback 改为持有 token 并调用 `wake_wait()`。
2. 移除 timeout 正确性对独立 `AtomicBool validness` 的依赖。
3. signal notify 改为调用 `wake_active_wait()`。
4. `clock_nanosleep()`、`rt_sigtimedwait()` 等直接等待路径改用统一 wait core。
5. 主动 cancel 路径统一通过 `cancel_wait()`，包括 predicate ready、timeout zero、signal precheck 和错误返回。
6. timeout callback 晚到、signal 被 uninterruptible wait 阻止、force wake 和主动 cancel 的关键结果保留 debug/trace 记录点。

验收：

1. timeout、event、signal 竞争同一 `WaitState`。
2. 旧 timer callback 晚到时 stale return。
3. signal 只完成 interruptible wait，force wake 有显式 mode。
4. 普通等待路径不再直接写 `TaskStatus::Waiting` 后调用旧 timeout/notify 组合。
5. `notify()` / `try_to_wake_up()` 若仍保留，只能作为明确分类的兼容或非 wait 协议路径。
6. timeout、signal 和主动 cancel 的日志能说明它们竞争的是同一个 `WaitState`，而不是各自维护独立完成状态。

## 5. 阶段 5：旁路审计和收口

交付：

1. 审计所有 `update_status_with()` 调用。
2. 审计所有 `TaskStatus::Waiting` 写入点。
3. 审计所有 `notify()`、`try_to_wake_up()` 和 `task_enqueue()` 调用点。
4. 将旧 wake API 收缩为兼容 wrapper，或明确标记为只允许非 wait 协议路径使用。
5. 给关键入口补充 debug assert，检查 `TaskSchedState` 状态转换、`ParkState::Parked` 只能出现在 `Waiting` 上，并确认调度内部判断不依赖 `task.status()` 兼容投影。
6. 审计 debug/trace 覆盖面，确认关键分支都有可打开的诊断信息，且默认配置下不会在高频路径产生不可接受噪声。

验收：

1. 没有 Event、timeout、signal、cancel 路径绕过 wait core。
2. wake tail 全部走 stale-safe placement。
3. 裸 `task_enqueue()` 不再出现在 wait/wake 完成尾巴中。
4. 所有保留旁路都有明确分类和理由。
5. `update_status_with()` 不再作为普通等待 begin/completion/cancel 写入口；保留命中必须能说明不是 wait 协议路径。
6. `TaskStatus` / `task.status()` 命中只剩观察投影、procfs/status、debug 输出或等价观察层；调度内部 runnable / waiting / zombie 判断使用 `TaskSchedState` helper 或事务。
7. 关键日志字段统一：至少包含 task id、wait identity、wait reason、wake mode、状态转换结果和 placement 结果；缺字段的记录点必须说明原因。

## 6. 阶段 6：验证和文档跟进

交付：

1. 增加最小并发回归测试或 debug stress hook，覆盖旧 wake 尾巴晚到新 wait 的交错。
2. 复跑已知触发 profile。
3. 更新 open issue / devlog，记录该 race 的处理状态和剩余限制。
4. 在 `progress-tracking.md` 或等价记录中保存每阶段旁路分类结果，避免后续实现者只看到“构建通过”而不知道剩余旧路径。
5. 保存一次带 debug/trace 的复现或 stress 日志摘要，证明关键观测点能支撑后续审查 late wake、stale placement、mode-blocked requeue 和 timeout/signal 竞争。

建议验证：

1. `just build`
2. `./scripts/run-user-test-rv64.sh rootfsconfig-rv etc/sdcard-rv.img build/event-waitstate-rv64.log`
3. 复跑曾触发 `processor.rs` `Runnable` 断言的 musl memory profile。
4. 如 la64 侧也能稳定复现相关 profile，再做一次 la64 smoke run。

## 7. 代码审计清单

实现过程中至少检查：

```text
rg -n "TaskStatus::Waiting" anemone-kernel/src
rg -n "update_status_with" anemone-kernel/src
rg -n "try_to_wake_up|notify\\(" anemone-kernel/src
rg -n "task_enqueue|local_enqueue|remote_enqueue" anemone-kernel/src
rg -n "listen_with_timeout|schedule_with_timeout" anemone-kernel/src
```

每个命中都要分类：

1. 纯观察路径。
2. 新任务发布或 bootstrap placement。
3. wait begin。
4. wait completion。
5. wait cancel。
6. stale-safe physical placement。
7. 需要迁移的旧旁路。

分类结果必须能反推需求文档中的不变量没有被破坏。

## 8. 可观测性清单

实现过程中必须能从 debug/trace 记录里回答：

1. 某次 wait 是哪一个 task 的哪一轮 `WaitState`，由哪个路径创建。
2. 某次 wake/cancel/timeout/signal 是否匹配当前 active wait，失败时是 stale、mode-blocked、already completed/cancelled，还是 task 已进入下一轮 wait。
3. `wake_wait()` 成功后对应的 `wake_enqueue()` 返回了哪一种 placement 结果。
4. `schedule()` 是否因为 park latch 发现 wait 已完成而 abort park。
5. `Event::publish()` 本轮扫描消耗了多少 successful exclusive quota，哪些 listener 被丢弃或回挂。
6. mode-blocked listener 回挂失败时，失败原因来自 task weak upgrade、wait identity mismatch、`WaitState` 非 `Armed`，还是 wake mode 已不再阻止。
7. timer callback 晚到、signal 被阻止、force wake 和主动 cancel 这些竞争路径各自看到的最终 wait outcome。

日志纪律：

1. 优先在 wait core、Event 边界、timer callback、signal wake 入口和 `wake_enqueue()` 结果点记录；不要在无状态循环里刷屏。
2. 默认构建可以只保留 debug 级别或 feature-gated trace；stress profile 应能打开足够详细的记录。
3. 记录 wait identity 时使用指针身份或等价稳定调试 id，不暴露可被外部误用为协议字段的新状态。
4. 日志只能辅助诊断，不能成为协议正确性依赖。

## 9. 停止边界

迁移期间继续追问的情况：

1. 改动会改变等待轮次身份、线性化点、状态转移、锁序或状态所有权。
2. 改动会改变 wake / cancel / timeout / signal 的可见语义。
3. 改动会改变 listener 离队、回挂、exclusive quota 或 stale-safe placement 的结果。
4. 改动会让 Event、timer、signal、runqueue 或 Task 之间出现双重真相源。
5. 改动会让 wait core 的 capability 边界退化成外部可伪造、可缓存或可滥用的普通状态。

可以停止 issue 查找的情况：

1. 需求文档已经明确协议边界，但没有规定具体代码形状。
2. 某个 API 尚未落地，但不改变当前阶段的不变量。
3. 实现路径选择不同，但不改变需求文档中的不变量。
4. 问题属于 P2，且不会影响当前迁移主线。
5. 问题属于 P3。
