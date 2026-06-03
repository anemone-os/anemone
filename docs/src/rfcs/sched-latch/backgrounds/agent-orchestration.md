# Sched Latch Agent 编排建议

本文记录 `sched-latch` 进入实现阶段时的 agent 编排方式。Canonical 协议仍以
[RFC 入口](../index.md)、[不变量需求](../invariants.md)、[迁移实施计划](../implementation.md)
和 [Tracking Issues](../tracking-issues.md) 为准；本文只说明如何按这些 gate 组织
worker、reviewer 和验收顺序。

## 编排原则

1. 不按“一个文档阶段一个 agent”机械拆分。拆分边界应对应协议边界和生产路径切换点。
2. 阶段 2 的 typed register API 与阶段 3 的 pipe source 强耦合，建议由同一实现 agent
   完成，但中间保留 checkpoint。
3. review agent 只放在有意义的 gate 上，不在每个微小实现步后立即审查。
4. 写入型 worker 只改自己的 write set；遇到必须越界的依赖，停止并回报总控。
5. 每个实现阶段退出都要更新事务日志：
   [2026-06-03 - Sched Latch](../../../devlog/transactions/2026-06-03-sched-latch.md)。
6. 只通过 `just build` 不能替代协议审计；阶段退出必须引用对应 tracking gate。
7. 尚未迁移的 source 可以继续走旧路径，但不能被当成 latch wait 的 armed source。
8. 不允许把 `PollWaiter` / `poll_waiters` 草稿扩展成新的 waitable poll 协议。

## 总控 Agent 使用方式

建议启动一个总控 agent 负责 orchestration，但不要让它自由决定新的协议拆分。
总控 agent 的权限边界是：

- 可以执行前置检查、代码搜索和构建级 gate。
- 可以启动只读 explorer / reviewer。
- 可以启动写入型 worker，但必须使用本文列出的 write set 和 worker 合同。
- 可以串行集成 worker diff。
- 可以更新事务 devlog。
- 不运行 QEMU / LTP，除非用户后续明确要求；rv64 / LTP 日志默认由用户提供。
- 不 push、不 force-push、不 reset hard、不清理未归属改动。
- 遇到停止条件时回报用户，不自行拍板。

总控第一轮不要一次性派发所有 worker。建议流程是：

1. 重新确认当前分支、工作区状态、RFC 文档和事务日志。
2. 派发 Agent 0 做 wait-core placement 前置审计。
3. 前置审计通过后派发 Agent 1，实现 `sched::latch` 原语。
4. 进行 Gate 1 review，确认可以进入 poll source 注册协议。
5. 派发 Agent 2，实现 typed `PollRequest` / `PollRegisterResult` 并迁移 pipe source。
6. 进行 Gate 2 review，确认可以让 `ppoll` 进入 latch schedule。
7. 派发 Agent 3，迁移 `ppoll` 并固定可复用的 iomux wait helper 边界。
8. 进行 Gate 3 review，确认 `pselect6` 必须复用的 outcome mapping 已闭合。
9. 派发 Agent 4，迁移 `pselect6`。
10. 派发 Agent 5 做旁路审计、构建 gate 和事务日志收口。

可直接给总控 agent 的启动 prompt：

```text
工作目录是仓库根目录。请作为 sched-latch 的总控 agent，阅读
docs/src/rfcs/sched-latch/index.md、
docs/src/rfcs/sched-latch/invariants.md、
docs/src/rfcs/sched-latch/implementation.md、
docs/src/rfcs/sched-latch/tracking-issues.md、
docs/src/rfcs/sched-latch/backgrounds/agent-orchestration.md 和
docs/src/devlog/transactions/2026-06-03-sched-latch.md。

目标是按 RFC gate 实现 sched-latch：建立 sched::latch 原语、定义 typed iomux
source register protocol、迁移 pipe source、迁移 ppoll、迁移 pselect6，并做旁路审计。

你可以启动子 agent，但必须按 agent-orchestration.md 的顺序、write set 和 review gate
分工，不允许 worker 越界修改。你不是独自在代码库里工作；不得 revert 用户或其他
agent 的改动。每集成一个阶段都要更新
docs/src/devlog/transactions/2026-06-03-sched-latch.md。

第一步只做前置检查、刷新当前代码落点和准备启动的 agent 列表。不要直接一次性启动
所有 worker。遇到停止条件时停止并向用户报告，不要自行拍板。
```

## Agent 0：Wait-Core 前置审计

职责：只读审计阶段 1 前置条件是否已经满足，不改代码。

读取范围：

- `anemone-kernel/src/sched/wait.rs`
- `anemone-kernel/src/sched/mod.rs`
- `anemone-kernel/src/sched/event.rs`
- timeout、signal、force 当前进入 wait core 的调用点

检查项：

- `wake_wait()` / `wake_active_wait()` 的 `Woke` 语义是否包含 logical completion 和
  stale-safe physical placement。
- timeout、signal、force 是否没有绕过 stale-safe placement。
- `ActiveWait` / `WakeToken` / `WaitOutcome` 是否足以支撑 `Latch` facade。
- 仍然暴露给外部的 wait-core lifecycle API 是否会破坏 `sched::latch` owner boundary。

交付：

- 是否允许进入 Agent 1 的结论。
- 如果不允许，列出必须先修的 wait-core blocker。
- 对应 tracking gate：KETER-005。

停止条件：

- 非 producer completion 不能证明共享 stale-safe placement。
- `ActiveWait` 或 wait-core public surface 会让普通 fd source 绕开 `LatchTrigger`。

## Agent 1：`sched::latch` 原语

职责：实现阶段 1，不接入生产 iomux 路径。

write set：

- `anemone-kernel/src/sched/mod.rs`
- `anemone-kernel/src/sched/wait.rs`
- `anemone-kernel/src/sched/latch.rs` 或等价子模块
- 必要的最小 debug / kunit hook
- `docs/src/devlog/transactions/2026-06-03-sched-latch.md`

语义要求：

- 新增 waiter-owned `Latch`，字段私有，不实现 `Clone`。
- 新增 cloneable producer handle `LatchTrigger`。
- `Latch::begin_current(interruptible)` 内部创建一轮 `ActiveWait`。
- `Latch::make_trigger()` 派生本轮 trigger。
- `LatchTrigger::trigger()` 通过 `wake_wait()` 完成 `WaitReason::Latch`。
- producer 普通路径 no-return / fail-closed，不把 `WakeResult` 暴露给普通 source 分支。
- consumer cancel 使用受限 `LatchCancelReason`，不接受任意 `WaitReason`。
- 每个 begin 后有 exactly-once finish / retire 策略。
- 记录 weak trigger 或 strong trigger + pruning 的资源策略，作为 Agent 2 前置条件。

验证：

```bash
just build
```

额外证据：

- old trigger 到达已 finish wait 时只记录 stale / retired，不 panic。
- begin / trigger / cancel / finish 日志能关联 wait id 和 task id。

Gate 1 reviewer 检查：

- KETER-003：consumer capability 是 owner-bound linear guard。
- KETER-004：cancel / finish 不覆盖 winning outcome，且 begin 后 exactly-once retire。
- KETER-005：producer、timeout、signal、force、cancel 共享 wait-core placement 合同。
- KETER-006：trigger 生命周期策略已记录，能作为 source 注册协议前置约束。
- KETER-007：producer capability no-return、fail-closed、隐藏 `WakeToken`。

## Agent 2：Typed Register API + Pipe Source

职责：合并执行阶段 2 和阶段 3，但保留两个 checkpoint。

write set：

- `anemone-kernel/src/fs/iomux.rs`
- `anemone-kernel/src/fs/pipe.rs`
- 必要的 `FileOps::poll` 类型签名调整及其直接调用点
- `docs/src/devlog/transactions/2026-06-03-sched-latch.md`

Checkpoint A：typed source register protocol。

- `PollRequest::snapshot(interests)` 保持纯快照。
- `PollRequest::register(interests, &LatchTrigger)` 或等价构造携带 trigger。
- 新增 `PollRegisterResult::{Ready(PollEvent), Armed, Unsupported}` 或等价类型。
- source 能区分 snapshot 和 register 请求。
- 未 armed / unsupported source 不允许被 syscall 用作 latch schedule 条件。
- 移除、私有化或明确废弃 `PollWaiter` / `PollRequest::waiter` 草稿入口。

Checkpoint B：pipe source 迁移。

- pipe rx / tx 内维护 source-local trigger queue。
- `pipe_rx_poll()` 在读端未 ready 时注册 READABLE trigger。
- `pipe_tx_poll()` 在写端未 ready 时注册 WRITABLE trigger。
- pipe write、pipe read、rx drop、tx drop 等状态变化路径在同一 pipe lock 临界区内完成
  predicate update 与 trigger detach。
- 释放 pipe lock 后触发 detached trigger。
- trigger queue 有 lazy pruning 或显式 cleanup 说明。
- 清理 `poll_waiters` 残留，或明确标注为不可新增调用点的迁移残留。

验证：

```bash
just build
```

额外证据：

- pipe readable、writable、peer close、consumer finish 后 late trigger 的手工或单测证据。
- source register / wake 日志能关联 wait id、source side、interest、detach 数量和 trigger 结果。

Gate 2 reviewer 检查：

- KETER-001：register scan fail closed；半迁移 source 不会导致永睡。
- KETER-002：pipe wake-side predicate update + detach 的线性化点在同一 source lock 内。
- KETER-006：source 队列 cleanup 不参与 correctness，资源上界或 pruning gate 已说明。
- EUCLID-002 / EUCLID-003：source queue entry 和 trigger 结果有足够诊断身份。

## Agent 3：`ppoll` 迁移与共享 Helper 边界

职责：实现阶段 4，并为 `pselect6` 固定可复用的 wait loop / outcome mapping 边界。

write set：

- `anemone-kernel/src/fs/api/iomux/ppoll.rs`
- `anemone-kernel/src/fs/api/iomux/mod.rs`
- 新增或调整的 iomux 内部 helper 文件
- 必要的 `fs::iomux` 内部类型
- `docs/src/devlog/transactions/2026-06-03-sched-latch.md`

语义要求：

- 先应用 signal mask，再 snapshot scan。
- snapshot ready 直接返回，不创建 `Latch`。
- unmasked signal、zero timeout、deadline expired 不创建 `Latch`。
- 只有确实需要阻塞时才 begin latch。
- 使用同一 `LatchTrigger` 对所有 fd 做 register scan。
- register scan 发现 ready 时 `cancel(PredicateReady)` + `finish()`。
- register scan 发现 unsupported 时 `cancel(RegisterError)` + `finish()`，不能 schedule。
- 所有非 ready source 都 armed 后才 schedule。
- wake / timeout / signal 后先 finish，再 final snapshot scan，再映射 outcome。
- helper 边界不泄漏 Linux `pollfd` ABI；ABI copy-in/copy-out 留在 syscall 层。

验证：

```bash
just build
```

额外证据：

- `ppoll` pipe readable、writable、hangup、timeout、signal interrupt。
- consumer finish 后旧 pipe trigger 晚到。
- timeout 与 source trigger race 的代表 case。

Gate 3 reviewer 检查：

- KETER-001：未 armed source 不会进入 schedule。
- KETER-004：`ppoll` 所有 begin 后路径 exactly-once finish / retire。
- KETER-008：final scan 与 outcome mapping 固定为 `pselect6` 可复用边界。
- EUCLID-001：timeout-zero / signal precheck 不创建无意义 latch。

## Agent 4：`pselect6` 迁移

职责：实现阶段 5，复用 Agent 3 的 helper 或共享控制流。

write set：

- `anemone-kernel/src/fs/api/iomux/pselect6.rs`
- `anemone-kernel/src/fs/api/iomux/mod.rs`
- Agent 3 已建立的 iomux helper 文件
- `docs/src/devlog/transactions/2026-06-03-sched-latch.md`

语义要求：

- 不复制一套独立 wait loop。
- input/output/exception fdset scan 适配到同一类 snapshot/register/final scan helper。
- READABLE、WRITABLE、exception interest 的处理只在 fdset 转换层分开。
- wake 后重新 snapshot scan，并只把最终 ready fdset 写回用户态。
- signal mask 临时替换与恢复覆盖所有路径。
- timeout、signal、ready race 与 `ppoll` 使用同一套 outcome mapping。

验证：

```bash
just build
```

额外证据：

- `pselect6` readable、writable、timeout、signal interrupt。
- timeout 与 ready race 的代表 case。

Gate 4 reviewer 检查：

- KETER-008：`ppoll` / `pselect6` 没有分裂 outcome mapping。
- EUCLID-001：timeout-zero / signal precheck 顺序一致。
- fdset 输出只来自 final snapshot scan。

## Agent 5：旁路审计与收口

职责：执行阶段 6，不再大改协议。

write set：

- 必要的残留清理文件
- `docs/src/devlog/transactions/2026-06-03-sched-latch.md`
- 如确有剩余限制，需要更新 `docs/src/register/current-limitations.md`

审计命令：

```bash
rg -n "PollRequest|PollWaiter|poll_waiters|yield_now\\(\\)" anemone-kernel/src/fs anemone-kernel/src/task
rg -n "WaitReason::Latch|LatchTrigger|sched::latch" anemone-kernel/src
rg -n "wake_wait\\(|wake_active_wait\\(" anemone-kernel/src
rg -n "task_enqueue|local_enqueue|remote_enqueue" anemone-kernel/src
```

每个命中分类：

1. snapshot-only poll。
2. register poll。
3. unsupported / fallback poll。
4. source-local trigger queue。
5. source wake predicate update + detach。
6. stale pruning / cleanup。
7. unrelated wait core caller。
8. 需要继续迁移的 busy-poll path。
9. 非 wait-tail placement。

验证：

```bash
just build
./scripts/run-user-test-rv64.sh rootfsconfig-rv etc/sdcard-rv.img build/sched-latch-rv64.log
```

默认情况下 QEMU / LTP 由用户执行；如果用户要求 agent 执行，再按仓库脚本运行。

最终退出条件：

- 没有 fd source 直接写 task sched state。
- 没有 fd source 在持有 source lock 时进入 wait core wake。
- source 队列中 stale trigger 有 cleanup / pruning 策略和资源上界。
- `Event` 没有成为 `Latch` 的实现依赖。
- `PollWaiter` / `poll_waiters` 不再作为可扩展协议面存在。
- `ppoll` / `pselect6` 的共享 helper 或共享控制流成为后续 iomux 维护入口。
- Tracking Issues 中所有 implementation gate 都有阶段退出证据。

## Review Gate 节奏

推荐只在以下边界启动 reviewer：

1. Gate 1：`sched::latch` 原语完成后。
2. Gate 2：typed register API + pipe source 完成后，进入 `ppoll` 前。
3. Gate 3：`ppoll` 完成后，进入 `pselect6` 前。
4. Gate 4：`pselect6` + 旁路审计后，最终收口。

不要在每个微小提交后启动 reviewer。对本迁移来说，过度碎片化会留下半协议在不同
agent 之间来回传递，反而降低审查质量。

## 停止条件

遇到以下情况，总控必须停止并让用户拍板：

- weak trigger 与 strong trigger + pruning 的资源策略无法闭合。
- unsupported source 的 fallback / error 策略会影响 `ppoll` / `pselect6` 返回语义。
- 需要改变 wait identity、completion 线性化点、owner boundary、source register gate、
  source wake detach、cleanup 非正确性支柱、placement 责任或 syscall outcome mapping。
- `ppoll` / `pselect6` 共享 helper 做不出来，只能复制两套可漂移的 wait loop。
- source lock 与 wait core lock 出现新的不确定锁序。
- worker 必须越过 write set 才能继续。

## Worker Prompt 模板

```text
你是 sched-latch 迁移的 worker。工作目录是仓库根目录。
请阅读 docs/src/rfcs/sched-latch/index.md、
docs/src/rfcs/sched-latch/invariants.md、
docs/src/rfcs/sched-latch/implementation.md、
docs/src/rfcs/sched-latch/tracking-issues.md 和
docs/src/rfcs/sched-latch/backgrounds/agent-orchestration.md。

你的任务是：<填入 Agent N 的职责>。

只允许修改以下 write set：
<填入 write set>

必须满足的 gate：
<填入对应 KETER/EUCLID gate>

不要修改无关文件，不要 revert 用户或其他 agent 的改动。遇到必须越界、协议无法闭合、
或停止条件命中时，停止并报告。完成后更新
docs/src/devlog/transactions/2026-06-03-sched-latch.md，并在最终回复中列出：
改动文件、满足的 gate、运行过的验证、未解决风险。
```
