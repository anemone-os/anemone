# Sched Latch 迁移实施计划

**状态：** Gate Plan Closed
**最后更新：** 2026-06-03
**父 RFC：** [RFC-20260603-sched-latch](./index.md)
**不变量：** [不变量需求](./invariants.md)

本文只描述建议落地顺序、审计范围和验收动作；协议和不变量以 [Sched Latch 不变量需求](./invariants.md) 为准。

## 迁移原则

1. `Latch` 必须直接建立在 wait core 上，不通过 `Event` 转发。
2. 每个阶段都必须保持单轮次 wait identity、single-consumer、多 producer trigger 和 first completion wins。
3. 接入生产路径前，任何 wait completion 都必须已经包含 wait core logical completion 和 stale-safe placement。
4. `poll` / `select` 迁移期间可以继续保留旧 busy-poll 路径，但不能让同一个 fd wait 同时被两套 completion 协议处理。
5. wake 只是 hint；syscall 返回前必须重新检查实际 readiness。
6. source 队列 cleanup 不能成为正确性支柱；旧 trigger 必须在 wait identity 层 fail closed。
7. 只有所有参与阻塞判定的 source 都 ready、armed 或明确 unsupported/fallback，syscall 才能进入 latch schedule。
8. 关键分支要保留 wait id、task id、source、reason 和 placement 诊断信息。

## 全局阶段 Gate

本文没有保留“实现时再想”的协议空洞。每个阶段开始前必须满足本阶段前置条件；每个阶段退出前必须能用交付、审计和验证项证明没有违反 [不变量需求](./invariants.md)。

通用 gate：

1. 若发现改动会改变 wait identity、completion 线性化点、owner boundary、source register gate、source wake detach、cleanup 非正确性支柱、placement 责任或 syscall outcome mapping，必须先更新 RFC 文本，不能只在实现注释里决定。
2. 尚未迁移的 source 可以继续走旧路径，但不能被当成 latch wait 的 armed source，也不能让 `ppoll` / `pselect6` 睡在未 armed 状态上。
3. 每个阶段退出声明必须同时引用对应 tracking gate 的处理结果；只通过 `just build` 不能替代协议审计。
4. Safe / Euclid 级命名、模块路径、诊断字段形状和未来 epoll / 异步通知 / AND wait 扩展不得升级为阶段阻塞项，除非它们实际改变不变量。

工程指导：

1. 当前代码中的 `PollWaiter` / `poll_waiters` 是旧 busy-poll 时代的草稿形状，不是本计划的协议真相。实现可以分阶段删除、绕开或暂时封存它们，但不得基于它们扩展新的 waitable poll 协议。
2. `PollRequest` 的迁移目标是 snapshot-only 与 register 型请求边界清晰；不要继续保留“可选 waiter 字段但没有 typed register result”的半协议接口。
3. `ppoll` 与 `pselect6` 的 latch loop、register scan、finish 后 final scan 和 outcome mapping 应尽早共享 helper 或共享一套明确控制流。不要在两个 syscall 中复制两份可独立漂移的等待协议。

## 阶段 1：建立 `sched::latch` 原语

前置条件：

1. `sched::wait` 已经提供 `ActiveWait` / `WakeToken` / `wake_wait()` / `wake_active_wait()` / `finish_wait()` 或等价入口。
2. `wake_wait()` / `wake_active_wait()` 的 `Woke` 语义已经包含 post-commit stale-safe placement。
3. timeout、signal、force 的现有 wait core 入口不会绕过 stale-safe placement。

交付：

1. 新增 `sched::latch` 或等价子模块。
2. 定义 waiter-owned `Latch` 和 cloneable producer handle `LatchTrigger`。
3. `Latch::begin_current(interruptible: bool)` 内部创建 `ActiveWait`。
4. `Latch::make_trigger()` 或等价方法派生本轮 producer handle。
5. `LatchTrigger::trigger()` 通过 `wake_wait()` 完成本轮等待。
6. 为 wait core 增加 `WaitReason::Latch` 或等价明确原因，避免复用 `WaitReason::Event`。
7. `Latch` 提供 waiter-owned `cancel()`、`finish()` 和可选 `schedule_with_timeout()` 薄封装。
8. producer trigger 普通路径 no-return / fail-closed；内部 debug 日志记录 wake 诊断结果。
9. consumer cancel 使用 `LatchCancelReason` / `ConsumerCancelReason` 或等价受限 reason，不直接接受任意 `WaitReason`。

建议类型草案：

```rust
pub struct Latch {
    task: Arc<Task>,
    active_wait: ActiveWait,
    // private owner-bound linear guard state
}

#[derive(Clone)]
pub struct LatchTrigger {
    task: Weak<Task>,
    token: WeakWakeToken, // or strong WakeToken with explicit source queue bounds
}

pub enum LatchCancelReason {
    PredicateReady,
    RegisterError,
    TimeoutZero,
    SignalPrecheck,
    SyscallError,
    Drop,
}

impl Latch {
    pub fn begin_current(interruptible: bool) -> Self;
    pub fn make_trigger(&self) -> LatchTrigger;
    pub fn cancel(&self, reason: LatchCancelReason);
    pub fn finish(self) -> WaitOutcome;
    pub fn schedule_with_timeout(&self, timeout: Option<Duration>) -> Duration;
}

impl LatchTrigger {
    pub fn trigger(&self);
}
```

审计：

1. 确认 `Latch` 字段私有，不实现 `Clone`。
2. 确认 `Latch` 是 `!Send` / `!Sync`，或所有 waiter-owned 方法都校验 `current_task == owner_task`。
3. 确认普通 source 无法取得 `WakeToken`、强 `Task` 引用、`WakeResult` 或 wait-core lifecycle API。
4. 确认 `cancel()` 不覆盖已经完成的 outcome。

可观测性：

1. begin / make trigger / trigger attempt / trigger stale / trigger woke / cancel / finish 都能关联 wait id 和 task id。
2. `WaitReason::Latch` 能在日志和 outcome 中与 `Event` 区分。
3. owner 校验失败、double finish、drop-without-finish 等异常有 debug assert 或 warning。

验证：

1. `just build`
2. 最小内核单测或 debug hook 覆盖 old trigger 到达已 finish wait 时只记录 stale / retired，不 panic。

退出条件：

1. `Latch` 本体不可 clone，不可作为普通跨 task 对象使用。
2. `LatchTrigger` 只暴露 no-return trigger capability。
3. 每个 `begin_current()` 后有 exactly-once finish/retire 策略。
4. weak/strong trigger 策略已经被记录为阶段 2 source 注册协议的前置约束。

## 阶段 2：定义 poll source 注册协议

前置条件：

1. 阶段 1 完成。
2. weak trigger 或 strong trigger + pruning 的资源策略已经确定。

交付：

1. 在 `fs::iomux` 中定义携带 `LatchTrigger` 的注册型 `PollRequest`。
2. 保留 `PollRequest::snapshot(interests)` 作为纯快照入口。
3. 新增 `PollRequest::register(interests, trigger)` 或等价构造。
4. 定义 typed register result，例如 `Ready(events) / Armed / Unsupported`。
5. 给 `PollRequest` 提供只读访问器，让 source 能判断是否需要注册 trigger。
6. 明确 source 侧协议：在 source state lock 下检查 readiness；未 ready 且支持注册时才保存 trigger。
7. 明确 source wake 协议：predicate update 与 trigger detach 在同一 source lock 临界区内线性化；释放 source lock 后逐个 trigger。
8. 移除、私有化或明确废弃 `PollWaiter` / `PollRequest::waiter` 草稿入口，避免 source 在新 typed register API 之外继续接入旧 `Event + AtomicBool` wait 形状。

可能形状：

```rust
pub enum PollRegisterResult {
    Ready(PollEvent),
    Armed,
    Unsupported,
}

pub struct PollRequest<'a> {
    interests: PollEvent,
    latch: Option<&'a LatchTrigger>,
}

impl<'a> PollRequest<'a> {
    pub const fn snapshot(interests: PollEvent) -> Self;
    pub const fn register(interests: PollEvent, latch: &'a LatchTrigger) -> Self;
    pub const fn interests(&self) -> PollEvent;
    pub const fn latch(&self) -> Option<&'a LatchTrigger>;
}
```

审计：

1. snapshot poll 不产生注册副作用。
2. register poll 只在 source 未 ready 时注册 trigger。
3. `Unsupported` / `NotRegistered` 路径不会让 syscall 继续进入 latch schedule。
4. source 不需要直接调用 wait core。
5. source 保存 trigger 后，即使 consumer 提前 finish，后续 trigger 也会 fail closed。
6. `fs::iomux` 不再对外 re-export 会被误认为新协议入口的 `PollWaiter` 草稿类型，或已经用注释和可见性把它限定为迁移残留。

可观测性：

1. source 队列 entry 至少能记录 wait id、source/debug id、interest 和注册点。
2. register result 的 `Ready`、`Armed`、`Unsupported` 能在 debug trace 中区分。
3. source wake detach 的 entry 数量和后续 trigger 结果可关联。

验证：

1. `just build`
2. 编译期或单测覆盖 snapshot-only 和 register result 分类。

退出条件：

1. source 注册 API 包含 typed result。
2. 未 armed source 不允许被 `ppoll` / `pselect6` 用作阻塞条件。
3. cleanup 策略是 lazy pruning、显式 cleanup 或两者组合，并有资源上界说明。
4. 新的 fd source 只能面向 typed `PollRequest` / `PollRegisterResult` 实现注册；不得新增 `PollWaiter` 使用点。

## 阶段 3：迁移一个最小 source

建议先迁移 pipe poll，因为 pipe 已经有明确的 readable / writable predicate，且当前 `PollWaiter` 草稿不应作为真相源。

前置条件：

1. 阶段 2 完成。
2. pipe readiness predicate 和锁边界已确认。

交付：

1. 移除或绕开草稿性质的 `PollWaiter` 依赖。
2. 在 pipe rx / tx 内维护 source-local trigger 队列。
3. `pipe_rx_poll()` 在读端未 ready 时注册 READABLE trigger。
4. `pipe_tx_poll()` 在写端未 ready 时注册 WRITABLE trigger。
5. pipe write、pipe read、rx drop、tx drop 等状态变化路径在同一 pipe lock 临界区内完成 predicate update 与 trigger detach。
6. 释放 pipe lock 后 trigger detached 队列。
7. trigger 队列做 stale pruning，避免长期积累已 retired wait。
8. 清理当前 pipe rx / tx 中未真正参与协议的 `poll_waiters` 字段和无效加锁，或在同一阶段替换为明确的 latch trigger queue。

审计：

1. pipe readiness 返回仍以实际 buffer / peer count 为准。
2. pipe source 不在持有 pipe lock 时调用 `LatchTrigger::trigger()`。
3. peer close 可以唤醒等待 HANG_UP / ERROR 的 poll/select。
4. 非阻塞 pipe read/write 语义不因 poll 注册改变。
5. 旧 busy-poll path 在未迁移 syscall 上仍可工作，已迁移 path 不重复注册两套 waiter。
6. pipe 代码里不再留下看似有效但实际未使用的 poll waiter 队列；如果为了分阶段保留，必须在注释中标明它不参与 correctness 且不可新增调用点。

可观测性：

1. pipe register entry 能记录 wait id、source side、READABLE/WRITABLE/HANG_UP/ERROR interest。
2. pipe wake 能记录 detach 数量、trigger stale/retired/woke 结果和 pruning 行为。

验证：

1. `just build`
2. pipe read/write 基础用例。
3. 手工或单测覆盖 readable、writable、peer close、consumer finish 后 late trigger。

退出条件：

1. pipe source 注册窗口闭合。
2. pipe source cleanup 不参与 correctness。
3. pipe source 不直接进入 wait core lifecycle。
4. pipe poll 的等待队列命名只指向 latch trigger queue，不再混用旧 `PollWaiter` 术语。

## 阶段 4：迁移 `ppoll`

前置条件：

1. 阶段 3 完成，并至少有一个 source 能真实 armed。
2. 所有参与本阶段 `ppoll` 阻塞判定的 source 都能返回 `Ready / Armed / Unsupported`。
3. 对 `Unsupported` source 的 fallback 或拒绝策略已明确。

交付：

1. `sys_ppoll` 先应用 signal mask，再做 snapshot scan；有 ready 直接返回。
2. 若已有 unmasked signal，直接返回 `EINTR`，不创建 `Latch`。
3. 若 timeout 为 zero 或 deadline expired，直接返回 0，不创建 `Latch`。
4. 未 ready 且确实需要阻塞时创建 interruptible `Latch`。
5. 携带同一 `LatchTrigger` 对所有 fd 做 register scan。
6. register scan 中若发现 ready，`cancel(PredicateReady)` 并 `finish()` 当前 `Latch`，返回 ready。
7. register scan 中若发现 unsupported，`cancel(RegisterError)` 并 `finish()` 当前 `Latch`，按既定 fallback / error 策略处理，不能继续 schedule。
8. 所有非 ready source 都 armed 后，使用同一 `Latch` 与 timeout 竞争并 schedule。
9. wake / timeout / signal 后 finish 本轮，再重新 snapshot scan 决定返回值或继续下一轮。
10. 将 snapshot scan、register scan、finish 后 final scan 和 outcome mapping 拆成可被 `pselect6` 复用的 helper，或至少先固定一套共享内部接口，避免阶段 5 再复制 `ppoll` 控制流。

建议循环结构：

```text
loop:
  apply/confirm signal mask
  ready = snapshot_scan()
  if ready > 0: return ready
  if signal: return EINTR
  if deadline expired: return 0

  latch = Latch::begin_current(true)
  trigger = latch.make_trigger()
  result = register_scan(trigger)
  if result.ready > 0:
    latch.cancel(PredicateReady)
    latch.finish()
    return ready
  if result.unsupported:
    latch.cancel(RegisterError)
    latch.finish()
    return fallback_or_error

  rem = latch.schedule_with_timeout(remaining)
  outcome = latch.finish()
  ready = snapshot_scan()
  if ready > 0: return ready
  map outcome to timeout / signal / retry / error
```

审计：

1. `ppoll` 不再用 `yield_now()` 实现有 fd 的阻塞等待。
2. `ppoll` wake 后不直接信任 latch reason，必须重新 scan fd readiness。
3. timeout、signal、ready race 使用统一 outcome mapping。
4. signal mask 恢复路径覆盖所有 early return。
5. 空 fd / `nfds == 0` 路径继续走已有 timeout/signal 等待语义，或显式统一到 `Latch`。
6. helper 边界不泄漏 Linux `pollfd` 布局到 source 注册协议；Linux ABI 转换仍留在 syscall 层，内部 helper 只处理 Anemone 的 fd、interest、register result 和 final readiness。

可观测性：

1. 同一 wait id 下能关联 register result、source id、trigger result、finish outcome 和 final scan result。
2. unsupported/fallback 路径有日志说明未进入 latch schedule。

验证：

1. `just build`
2. `ppoll` pipe readable、writable、hangup、timeout、signal interrupt。
3. consumer finish 后旧 pipe trigger 晚到。
4. timeout 与 source trigger race 的代表 case。

退出条件：

1. `ppoll` 对所有 begin 后路径都 exactly-once finish/retire。
2. 未 armed source 不会导致阻塞。
3. final readiness scan 与 outcome mapping 已在 syscall 层注释或 helper 中固定。
4. `pselect6` 可以复用同一 latch loop / outcome helper，或阶段 4 已记录必须复用的内部接口。

## 阶段 5：迁移 `pselect6`

前置条件：

1. 阶段 4 完成。
2. `ppoll` 的 latch loop、register scan 和 outcome mapping 可以复用或抽象。

交付：

1. 复用 `ppoll` 的 latch loop，而不是复制一套等待协议。
2. 将 input/output/exception fdset 的 scan 抽象为同一类 register scan。
3. 对 READABLE、WRITABLE、exception interest 分别注册对应 trigger。
4. wake 后重新 snapshot scan 并更新用户 fdset。
5. 修正当前注释中的 busy polling 状态。
6. 如果 `pselect6` 需要 fdset 特有的输出处理，只把 fdset read/write 留在 syscall 层；等待 loop、register result 和 outcome mapping 必须继续复用阶段 4 的共享边界。

审计：

1. `pselect6` 不再用 `yield_now()` 实现有 fd 的阻塞等待。
2. signal mask 临时替换与恢复覆盖所有路径。
3. 返回给用户的 fdset 只包含最终 snapshot scan 的 ready 集合。
4. timeout、signal、ready race 与 `ppoll` 使用同一套判定规则。
5. `pselect6` 没有重新引入独立的 wait loop、独立 timeout/signal mapping 或独立 unsupported/fallback 解释。

可观测性：

1. fdset register scan 能记录 fd、interest、source id 和 register result。
2. final scan 能记录返回给用户的 fdset 摘要。

验证：

1. `just build`
2. `pselect6` readable、writable、timeout、signal interrupt。
3. timeout 与 ready race 的代表 case。

退出条件：

1. `pselect6` 复用同一套 `Latch` owner lifecycle。
2. `pselect6` 与 `ppoll` 不再拥有分裂的 outcome mapping。
3. 后续新增 iomux syscall 或 source 时，有明确 helper / trait 边界可复用，而不是复制 `ppoll` / `pselect6` 的私有扫描循环。

## 阶段 6：旁路审计和收口

前置条件：

1. `ppoll` 和 `pselect6` 已经接入 latch path。
2. 已迁移 source 的 cleanup / pruning 策略有日志或审计证据。

交付：

1. 审计所有 `PollRequest`、`PollWaiter`、`poll_waiters` 和 `yield_now()` 命中。
2. 审计所有 `WaitReason::Latch`、`LatchTrigger`、`sched::latch` 命中。
3. 审计所有 `wake_wait()` / `wake_active_wait()` 调用点。
4. 审计 source-local trigger queue、source wake detach 和 cleanup。
5. 将 `PollWaiter` 草稿删除，或标为未使用且不再暗示协议真相。
6. 更新 tracking issues、devlog 或 register，记录剩余限制。
7. 审计 `ppoll` / `pselect6` 是否仍存在重复的 latch loop、register scan 或 outcome mapping；重复只允许保留在 ABI copy-in/copy-out 和结果格式转换层。

验证：

1. `just build`
2. `./scripts/run-user-test-rv64.sh rootfsconfig-rv etc/sdcard-rv.img build/sched-latch-rv64.log`
3. LTP 中依赖 `poll` / `select` 睡眠可观测性的用例。
4. 已登记的 `ANE-20260531-IOMUX-INFINITE-WAIT-STAGE1` 相关场景。

退出条件：

1. 没有 fd source 直接写 task sched state。
2. 没有 fd source 在持有 source lock 时进入 wait core wake。
3. source 队列中 stale trigger 有清理策略和资源上界。
4. `Event` 没有成为 `Latch` 的实现依赖。
5. tracking issues 中没有 Still open plan gap，且所有对应 implementation gate 都有阶段退出证据。
6. `PollWaiter` / `poll_waiters` 不再作为可扩展协议面存在；若仍有残留，必须是不可达、私有或明确登记的待删迁移残留。
7. `ppoll` / `pselect6` 的共享 helper 或共享控制流已经成为后续 iomux 维护入口。

## 旁路审计清单

实现过程中至少检查：

```text
rg -n "PollRequest|PollWaiter|poll_waiters|yield_now\\(\\)" anemone-kernel/src/fs anemone-kernel/src/task
rg -n "WaitReason::Latch|LatchTrigger|sched::latch" anemone-kernel/src
rg -n "wake_wait\\(|wake_active_wait\\(" anemone-kernel/src
rg -n "task_enqueue|local_enqueue|remote_enqueue" anemone-kernel/src
```

每个命中分类：

1. snapshot-only poll。
2. register poll。
3. unsupported/fallback poll。
4. source-local trigger queue。
5. source wake predicate update + detach。
6. stale pruning / cleanup。
7. unrelated wait core caller。
8. 需要继续迁移的 busy-poll path。
9. 非 wait-tail placement。

分类结果必须能反推不变量文档中的 wait identity、source 注册窗口、cleanup 非正确性支柱和 placement 责任没有被破坏。

## 可观测性清单

实现过程中必须能从 debug/trace 记录里回答：

1. 某个 `Latch` 属于哪个 task、哪一轮 wait id。
2. 某个 source 注册结果是 ready、armed 还是 unsupported。
3. 某个 trigger 来自哪个 source 注册点和 interest。
4. trigger 结果是 woke、stale、already completed、already cancelled 还是 retired。
5. wake 成功后的 placement 结果是什么。
6. consumer finish 时 outcome 是 latch、timeout、signal、force、predicate ready、register error 还是 cancel/drop。
7. poll/select 最终返回前的实际 readiness scan 结果。
8. cleanup / pruning 是否有资源上界证据。

日志纪律：

1. 优先在 `sched::latch`、`fs::iomux`、source register/wake、timeout/signal completion 和 final scan 记录。
2. 不在无状态循环里刷屏；stress profile 可打开更详细 trace。
3. 记录 wait identity 时使用指针身份或等价稳定调试 id，不暴露可被外部误用为协议字段的新状态。
4. 日志只能辅助诊断，不能成为协议正确性依赖。

## 停止边界

迁移期间继续追问的情况：

1. 改动会改变 wait identity、completion 线性化点、状态所有权或 stale-safe placement 责任。
2. 改动会让 producer 直接操作 task 调度状态或 runqueue。
3. 改动会让 `Latch` 变成可复用事件源或跨轮次 permit。
4. 改动会改变 poll/select 的 register gate、source wake detach、cleanup 语义或 wake 后重扫语义。
5. 改动会引入 source lock 与 wait core lock 的新锁序。
6. 改动会让 `ppoll` 与 `pselect6` outcome mapping 分裂。

可以停止 issue 查找的情况：

1. 需求文档已经明确协议边界，但没有规定具体代码形状。
2. 类型名尚未最终确定，但 single-consumer / multi-producer / one-shot 边界不变。
3. 首版选择 weak trigger 还是 strong trigger，只要资源上界和 stale-safe 行为已记录。
4. 某个 source 尚未迁移，只要它没有接入半套 `Latch` 协议，也不会让 syscall 睡在未 armed source 上。
5. 问题属于 epoll、异步通知或 AND wait 扩展，不影响当前 OR primitive。
6. 问题属于 Safe 级 tracking item，且不会影响当前迁移主线。
