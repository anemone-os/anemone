# Signal 临时 Mask Restore Agent 编排建议

本文记录 `signal-temp-mask-restore` 进入实现阶段时的 agent 编排方式。Canonical
协议仍以 [RFC 入口](../index.md)、[不变量需求](../invariants.md)、
[迁移实施计划](../implementation.md) 和 [Tracking Issues](../tracking-issues.md)
为准；本文只说明如何按这些 gate 组织 worker、reviewer 和验收顺序。

本 RFC 尚未进入实现阶段。真正开始实现前，总控 agent 必须先建立事务 devlog，并在
RFC 入口和事务日志之间建立双向链接。

## 编排原则

1. 不按“一个文档阶段一个 agent”机械拆分。拆分边界应对应 `TaskSigMaskState`
   storage、temporary token/helper、trap-return delivery、signal-owned classifier /
   handoff、`rt_sigsuspend` callsite、iomux typed outcome 和 `rt_sigtimedwait` 边界。
2. 阶段 1A 与阶段 1B 必须分开 review。1A 只迁移 current-mask storage 和普通
   current-mask API；1B 才建立 delayed-restore token contract。
3. 阶段 1A 不迁移 `ppoll` / `pselect6` 的旧 save / set / restore 语义，只登记为
   阶段 4 必删债务。不要把它们算作 delayed restore 已完成。
4. 阶段 2 的 signal delivery commit / cleanup 必须先闭合，再接入
   `rt_sigsuspend`、`ppoll` 或 `pselect6`。否则 callsite 会拿到 token，却没有可靠的
   trap-return 恢复落点。
5. `classify_temporary_mask_wait()` 和 stable delivery target reservation 归 signal
   子系统所有。syscall body、fs/iomux 和 wait-core worker 不得复制 pending queue、
   disposition、ignore/default/custom action 或 force wake policy。
6. `rt_sigsuspend` 和 iomux 迁移可以共用同一个 classifier，但先用 `rt_sigsuspend`
   证明 helper、reservation 和 `EINTR` carrier 路径，再迁移 `ppoll` / `pselect6`。
7. `rt_sigtimedwait` 是边界审计和本地语义修复项，不迁入 delayed restore helper。
8. review agent 只放在有意义的 gate 上：storage/API、token contract、delivery
   commit/cleanup、classifier handoff、`rt_sigsuspend`、iomux、`rt_sigtimedwait` 和
   最终旁路审计。
9. 写入型 worker 默认只改自己的 write set；若更合适的架构必须扩大范围，停止并向
   总控提交 write set 扩展申请，批准后再继续。
10. 每个实现阶段都要保持 `just build` 通过。smoke / LTP / QEMU 默认由用户验证，
    除非用户后续明确授权 agent 运行。

## 总控 Agent 使用方式

建议启动一个总控 agent 负责 orchestration，但不要让它自由决定新的协议拆分。
总控 agent 的权限边界是：

- 可以执行前置检查、代码搜索和构建级 gate。
- 可以启动只读 explorer / reviewer。
- 可以启动写入型 worker，但必须使用本文列出的 write set 和 worker 合同；需要扩大
  write set 时，先记录原因、范围、contract/gate 影响和批准结果。
- 可以串行集成 worker diff。
- 可以在实现开始后建立并更新事务 devlog。
- 不运行 QEMU / LTP，除非用户后续明确要求；rv64 / LTP 日志默认由用户提供。
- 不 push、不 force-push、不 reset hard、不清理未归属改动。
- 遇到停止条件时回报用户，不自行拍板。

总控第一轮不要一次性派发所有 worker。建议流程是：

1. 重新确认当前分支、工作区状态、RFC 文档和是否已建立事务日志。
2. 派发 Agent 0 做当前 signal / iomux / syscall ABI 前置审计。
3. 派发 Agent 1，完成阶段 1A `TaskSigMaskState` storage 和 ordinary current-mask API。
4. 进行 Gate 1 review，确认 direct `sig_mask` access 已分类，`ppoll` / `pselect6`
   legacy path 只作为阶段 4 债务残留。
5. 派发 Agent 2，完成阶段 1B `TemporarySigMaskToken` 与 helper contract。
6. 进行 Gate 2 review，确认 token 是线性、must-use、无 drop-time restore，且
   `restore != None` 时 ordinary mutation 规则闭合。
7. 派发 Agent 3，完成阶段 2 signal delivery 接入。
8. 进行 Gate 3 review，确认 handler frame commit 点、无 frame cleanup 点和
   `rt_sigreturn` current-mask restore 落点闭合。
9. 派发 Agent 4，实现 signal-owned classifier / stable handoff。
10. 进行 Gate 4 review，确认 `DeferToTrapReturnDelivery` 不是观察型 proof。
11. 派发 Agent 5，实现阶段 3 `rt_sigsuspend` syscall。
12. 进行 Gate 5 review，确认 `rt_sigsuspend` 不主动 `fetch_signal()`，不早恢复旧
    mask，并只通过 classifier 决定 defer。
13. 派发 Agent 6，完成阶段 4 `ppoll` / `pselect6` typed outcome 与 shared helper
    迁移。
14. 进行 Gate 6 review，确认 iomux 不再提前把 `Signal` / `Force` 压成
    `SysError::Interrupted`，且旧 save / set / restore path 已删除或替换。
15. 派发 Agent 7，完成阶段 5 `rt_sigtimedwait` 本地语义修复与边界审计。
16. 派发 Agent 8 做旁路审计、构建 gate、验证证据整理和事务日志收口。

可直接给总控 agent 的启动 prompt：

```text
工作目录是仓库根目录。请作为 signal-temp-mask-restore 的总控 agent，阅读
docs/src/rfcs/signal-temp-mask-restore/index.md、
docs/src/rfcs/signal-temp-mask-restore/invariants.md、
docs/src/rfcs/signal-temp-mask-restore/implementation.md、
docs/src/rfcs/signal-temp-mask-restore/tracking-issues.md、
docs/src/rfcs/signal-temp-mask-restore/backgrounds/agent-orchestration.md。

目标是按 RFC gate 实现 signal temporary mask delayed restore：先完成
TaskSigMaskState storage 和 ordinary current-mask API，再建立 TemporarySigMaskToken
helper contract，接入 trap-return delivery commit/cleanup，随后实现 signal-owned
classifier / stable handoff、rt_sigsuspend、ppoll/pselect6 shared helper 迁移，最后
审计并修复 rt_sigtimedwait 的本地 waited-set dequeue 边界。

你可以启动子 agent，但必须按 agent-orchestration.md 的顺序、write set 和 review gate
分工；未经批准不允许 worker 越界修改。你不是独自在代码库里工作；不得 revert 用户或其他
agent 的改动。实现开始前需要建立并维护对应事务 devlog。

第一步只做前置检查、刷新当前代码落点和准备启动的 agent 列表。不要直接一次性启动
所有 worker。遇到停止条件时停止并向用户报告，不要自行拍板。
```

## Agent 0：前置审计

职责：只读审计当前代码落点是否仍符合 RFC 假设，不改代码。

读取范围：

- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/task/sig/mod.rs`
- `anemone-kernel/src/task/sig/api/rt_sigprocmask.rs`
- `anemone-kernel/src/task/sig/api/rt_sigreturn.rs`
- `anemone-kernel/src/task/sig/api/rt_sigtimedwait.rs`
- `anemone-kernel/src/task/sig/api/mod.rs`
- `anemone-kernel/src/task/api/clone/mod.rs`
- `anemone-kernel/src/fs/proc/tgid/status.rs`
- `anemone-kernel/src/fs/api/iomux/wait.rs`
- `anemone-kernel/src/fs/api/iomux/ppoll.rs`
- `anemone-kernel/src/fs/api/iomux/pselect6.rs`
- `anemone-abi/src/syscall/riscv.rs`
- `anemone-abi/src/syscall/loongarch.rs`
- syscall dispatch 表所在文件。

检查项：

- `Task` 是否仍直接持有 `sig_mask: NoIrqSpinLock<SigSet>`。
- `Task::sig_mask()` / `Task::set_sig_mask()` 和 direct `task.sig_mask.lock()` 分布。
- `rt_sigprocmask`、`rt_sigreturn`、clone/fork、procfs status 是否仍直接读写 mask。
- `perform_signal_action()` 是否仍把当前 mask 写入 sigframe，并在同一段里安装 handler
  `sa_mask` / self-mask。
- `handle_signals()` 是否仍缺少 no-handler-frame temporary restore cleanup。
- `rt_sigtimedwait` 是否仍在 syscall body 中临时改 mask，并在
  `CurrentWaitOutcome::Signal | CurrentWaitOutcome::Force` 后直接进入 interrupted 分支。
- `ppoll` / `pselect6` 是否仍有旧 save / set / wait / restore path。
- `wait_for_iomux_ready()` 是否仍把 `LatchWaitOutcome::Signal | LatchWaitOutcome::Force`
  压成 `SysError::Interrupted`。
- `SYS_RT_SIGSUSPEND = 133` 是否仍未在目标 ABI 和 syscall dispatch 中接通。

交付：

- 是否允许进入 Agent 1 的结论。
- 如果不允许，列出必须先修的 RFC blocker。
- 当前代码路径与阶段 1A / 1B / 2 / 3 / 4 / 5 的对应表。
- 是否已有事务 devlog；若没有，提醒总控在实现开始前创建。

停止条件：

- 当前分支已经引入多层 temporary mask restore stack。
- 已有实现把 `rt_sigtimedwait` 迁入 delayed restore helper。
- 已有实现把 cleanup 语义下沉到 arch-specific trap-return 层。
- 已有实现让 `ppoll` / `pselect6` 在没有 signal-owned classifier 的情况下 defer
  restore。
- 当前代码已经改动 signal pending / disposition / delivery ownership，导致 RFC 的
  classifier / handoff 边界不再成立。

## Agent 1：阶段 1A Storage 与 Current-Mask API

职责：完成阶段 1A，只建立 `TaskSigMaskState` storage 和 ordinary current-mask API。

write set：

- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/task/sig/mod.rs`
- `anemone-kernel/src/task/sig/api/rt_sigprocmask.rs`
- `anemone-kernel/src/task/sig/api/rt_sigreturn.rs`
- `anemone-kernel/src/task/sig/api/rt_sigtimedwait.rs`
- `anemone-kernel/src/task/api/clone/mod.rs`
- 必要的 fork / procfs status snapshot 触点。
- 实现开始后对应的事务 devlog。

语义要求：

- 将 current mask 和 restore slot 合并为单一 `TaskSigMaskState { current, restore }`。
- 提供命名清楚的 current-mask API：snapshot、permanent mutation、sigframe restore、
  syscall-body-only temporary mutation。
- `rt_sigprocmask` 永久 mask 修改不绕开 `TaskSigMaskState`。
- `rt_sigreturn` 只通过 current-mask API 恢复 sigframe mask，不读取、不消费、不覆盖
  pending restore slot。
- clone / fork 只继承 `current`，不得复制 `restore`。
- procfs status 等观察点只 snapshot `current`，不得暴露 `restore`。
- `rt_sigtimedwait` 仍保留 syscall-body-only temporary path，不迁入 delayed helper。
- `ppoll` / `pselect6` 旧 save / set / restore path 只登记为阶段 4 必迁移债务，不在
  本阶段伪装成完成迁移。

验证：

```bash
just build
git diff --check
```

Gate 1 reviewer 检查：

- current mask 和 restore slot 只有一个锁和一个真相源。
- `rg -n "sig_mask|set_sig_mask|TaskSigMaskState" anemone-kernel` 中的残留都已分类。
- ordinary current-mask API 的调用点能区分 snapshot、permanent mutation、sigframe
  restore 和 syscall-body-only temporary mutation。
- `ppoll` / `pselect6` legacy temporary path 被点名登记为阶段 4 债务，且没有被算作
  delayed restore 已完成。

## Agent 2：阶段 1B TemporarySigMaskToken 与 Helper Contract

职责：完成阶段 1B，固定 temporary mask 的线性 token 生命周期。

write set：

- `anemone-kernel/src/task/sig/mod.rs`
- 必要的 signal-internal helper module。
- 仅限为编译闭合调整调用方 API 名称，不提前迁移阶段 2 到 5 的语义。
- 实现开始后对应的事务 devlog。

语义要求：

- 提供：
  - `begin_temporary_sig_mask(new_mask) -> TemporarySigMaskToken`
  - `TemporarySigMaskToken::restore_now(self)`
  - `TemporarySigMaskToken::defer_to_signal_delivery(self)`
  - `sigmask_to_save_for_signal_frame() -> SigSet`
  - `signal_frame_committed_restore_mask()`
  - `restore_temporary_sig_mask_if_pending()`
- `TemporarySigMaskToken` 必须 `#[must_use]`、非 `Clone` / 非 `Copy`。
- token terminal method 消耗 `self`，且只有 `restore_now()` 与
  `defer_to_signal_delivery()` 两个终止态。
- token 记录 task / slot identity，终止时校验仍匹配。
- `Drop` 最多 assert/log active-token leak，不得恢复 mask、清空 restore slot 或选择
  defer。
- begin 时已有 `restore` 必须在安装新 mask 前 fail-closed 或触发内部 invariant。
- `restore != None` 时 ordinary permanent mask mutation 默认非法或不可达；唯一允许在
  pending restore window 内修改 `current` 的路径是 signal delivery 安装 handler mask。
- helper 不得在持有 mask state 锁时执行用户 copy 或 schedule。

验证：

```bash
just build
git diff --check
```

Gate 2 reviewer 检查：

- token 线性、must-use、不可复制，terminal method 消耗 `self`。
- 没有 drop-time restore 语义。
- nested temporary begin 不覆盖已有 restore slot。
- `restore != None` 时 ordinary mutation 的合法路径和非法路径都可由代码审查定位。

## Agent 3：阶段 2 Signal Delivery 接入

职责：完成 trap-return signal delivery 边界，让 delayed restore 真正闭合。

write set：

- `anemone-kernel/src/task/sig/mod.rs`
- `anemone-kernel/src/task/sig/api/rt_sigreturn.rs`
- 必要时调整 arch `SignalArchTrait` 调用点，但不得把 cleanup 语义下沉到 arch 层。
- 实现开始后对应的事务 devlog。

语义要求：

- `perform_signal_action()` 用户 handler 路径按顺序执行：
  - 先调用 `sigmask_to_save_for_signal_frame()` 选择 sigframe mask。
  - 再安装 handler `sa_mask` / self-mask。
  - 写入 sigframe 并准备 trapframe。
  - 只在 frame commit 点调用 `signal_frame_committed_restore_mask()`。
- frame 写失败或 trapframe 准备失败时，不能提前消费 restore slot。
- `handle_signals()` 在没有留下用户 handler frame 的返回用户态路径调用
  `restore_temporary_sig_mask_if_pending()`。
- default terminate 路径保持不返回；default ignore / explicit ignore 消费 signal 后由
  统一 cleanup 恢复旧 mask。
- `rt_sigreturn` 仍只恢复 sigframe 中保存的 mask 到 `current`，不得读取或清理 pending
  restore slot。

验证：

```bash
just build
```

建议 smoke：

- 已 pending 的 handler signal 能进入 handler，handler `ucontext.uc_sigmask` 是旧 mask。
- default ignore / explicit ignore 消费 signal 后不会携带临时 mask 返回用户态。

Gate 3 reviewer 检查：

- handler frame commit 点和 no-frame cleanup 点都能被代码审查明确定位。
- restore slot 没有在读取 `mask_to_save` 后提前清除。
- cleanup 位于 signal 模块内部，没有在 riscv64 / loongarch64 trap-return 层复制。

## Agent 4：Signal-Owned Classifier 与 Stable Handoff

职责：实现 delayed-restore callsite 共用的 signal-owned classification / handoff 边界。

write set：

- `anemone-kernel/src/task/sig/mod.rs`
- 必要的 signal pending / delivery target 内部结构所在文件。
- 必要的 typed decision / context 定义所在 signal-internal module。
- 实现开始后对应的事务 devlog。

语义要求：

- 提供 `classify_temporary_mask_wait(candidate, context)` 或等价 signal-owned API。
- classification 输入只承接 typed wait outcome 和 syscall context；不能让 callsite 自己解释
  pending queue 或 disposition。
- 返回 `DeferToTrapReturnDelivery` 前，必须已经为当前 task 建立 stable delivery target。
- private pending signal 必须标记 / 移入当前 task 的 reserved target，或采用等价机制。
- shared pending signal 必须在 signal 子系统内部完成 claim / move / reservation，使其不再
  能被同一 thread group 的其它 eligible member 竞争消费。
- reservation 必须与 `handle_signals()` 的实际 fetch / delivery 路径对接。
- 无法 reserve / handoff 时，不得返回 `DeferToTrapReturnDelivery`；只能返回 restore /
  fail-closed / no-return force 类 decision。
- `Force` 不得被降级成 ordinary `EINTR` proof。

验证：

```bash
just build
```

Gate 4 reviewer 检查：

- `DeferToTrapReturnDelivery` 不是纯观察型 proof。
- shared pending signal 没有“先看见、后再竞争”的窗口。
- reserved target 被 ignore/default no-handler-frame 消费时仍会走统一 cleanup。
- classifier 没有把 wait-core `Signal` / `Force` 直接等同于 defer。

## Agent 5：阶段 3 rt_sigsuspend Syscall

职责：补齐 `rt_sigsuspend` ABI、syscall 接入和 wait-core outcome mapping。

write set：

- `anemone-abi/src/syscall/riscv.rs`
- `anemone-abi/src/syscall/loongarch.rs`
- syscall dispatch 表。
- `anemone-kernel/src/task/sig/api/rt_sigsuspend.rs`
- `anemone-kernel/src/task/sig/api/mod.rs`
- 必要的 signal-owned classifier 调用点所在文件。
- 可选：`anemone-rs/src/sys/linux.rs` 和 `anemone-rs/src/os/linux.rs` wrapper，仅用于
  smoke ergonomics。
- 实现开始后对应的事务 devlog。

语义要求：

- 注册 `SYS_RT_SIGSUSPEND = 133`。
- `sigsetsize` 和用户 mask copy-in 在安装临时 mask 前完成；bad pointer / bad size
  不产生 token。
- copy-in 后清除 `SIGKILL` / `SIGSTOP`。
- 安装 temporary mask 后进入 wait-core；precheck 使用 `has_unmasked_signal()`，但它只
  是 wait precheck，不是 defer proof。
- typed wait candidate 必须交给 signal-owned classifier。
- 只有 classifier 已完成 stable delivery target reservation / handoff 并返回
  `DeferToTrapReturnDelivery` 后，才调用 `token.defer_to_signal_delivery()`。
- restore / fail-closed / ordinary error path 必须先 `token.restore_now()`，再映射
  errno / result。
- `rt_sigsuspend` 不主动 `fetch_signal()`，正常不会 success return。

验证：

```bash
just build
```

建议 smoke：

- 旧 mask 屏蔽 `SIGUSR1`，`rt_sigsuspend` 临时解除屏蔽；handler 必须运行，handler
  返回后旧 mask 恢复。
- signal 在调用前已经 pending，syscall 不永久睡眠。

Gate 5 reviewer 检查：

- syscall body 没有早恢复旧 mask。
- syscall body 没有复制 pending queue、disposition 或 force policy。
- `Signal` / `Force` outcome 没有被直接映射为 defer。
- every token return path 都有且只有一个 terminal。

## Agent 6：阶段 4 ppoll / pselect6 迁移

职责：消除 iomux 第二套临时 mask 生命周期，并保留 typed wait outcome。

write set：

- `anemone-kernel/src/fs/api/iomux/wait.rs`
- `anemone-kernel/src/fs/api/iomux/ppoll.rs`
- `anemone-kernel/src/fs/api/iomux/pselect6.rs`
- signal-owned classifier 所在 signal 文件，优先 `anemone-kernel/src/task/sig/mod.rs` 或
  signal-internal module。
- 实现开始后对应的事务 devlog。

语义要求：

- `wait_for_iomux_ready()` 或替代 helper 返回 typed outcome，至少区分 ready、timeout、
  syscall/register error、signal candidate 和 force。
- `ppoll` / `pselect6` 的 sigmask path 使用 `begin_temporary_sig_mask` /
  `restore_now` / `defer_to_signal_delivery`。
- 删除或替换阶段 1A 登记的 legacy save / set / wait / restore path。
- ready、timeout、copy-out error 和 syscall/register error 先 `restore_now()`，再返回。
- signal candidate / force 只能交给 signal-owned classifier；不能在 fs/iomux 中复制
  signal delivery policy。
- `DeferToTrapReturnDelivery` 不是观察型 proof，仍要求 signal 子系统完成 reservation /
  handoff。
- no-sigmask path 不创建 token。
- errno / result mapping 发生在 token terminal 之后。

验证：

```bash
just build
```

建议 smoke：

- `ppoll` 带临时 mask 被 handler 打断，handler 运行，返回后旧 mask 恢复。
- `pselect6` 带临时 mask 被 handler 打断，handler 运行，返回后旧 mask 恢复。
- ready path 和 timeout path 不改变旧 mask。

Gate 6 reviewer 检查：

- `ppoll` / `pselect6` 不再有独立早恢复模型。
- `wait_for_iomux_ready()` 不再在 token-active 路径上提前把 `Signal` / `Force`
  ABI-map 成 `SysError::Interrupted`。
- copy-out error、ready、timeout、syscall/register error、signal candidate、force 每条
  token-active path 都有且只有一个 terminal。

## Agent 7：阶段 5 rt_sigtimedwait 边界审计

职责：确认 `rt_sigtimedwait` 保持 helper 外语义，并修复 signal / force wake 后的
waited-set dequeue 分类。

write set：

- `anemone-kernel/src/task/sig/api/rt_sigtimedwait.rs`
- 若需要超出该文件或改变 trap-return delivery 协议，上报 RFC scope 变更。
- 实现开始后对应的事务 devlog。

语义要求：

- `rt_sigtimedwait` 不使用 delayed restore helper。
- 临时 unmask / restore 局限在 syscall body。
- wait 被 signal / force 唤醒后，先按 waited set 重新尝试 dequeue matching signal。
- 若拿到 waited signal，恢复 mask 并返回 signal number。
- 若没有 matching waited signal，恢复 mask 后返回 `EINTR` 或处理 force。
- 未等待 signal interrupted path 不把恢复责任交给 `rt_sigreturn()`。
- 若发现它需要 trap-return delivery / delayed helper，停止并回到 RFC 修改 Scope。

验证：

```bash
just build
```

建议 smoke：

- waited signal 在 precheck 前 pending 时返回 signal number。
- waited signal 在 wait 已发布后到达时返回 signal number。
- 非 waited unmasked signal interrupt 返回 `EINTR` 且 mask 恢复。

Gate 7 reviewer 检查：

- `CurrentWaitOutcome::Signal | CurrentWaitOutcome::Force` 分支先尝试 waited-set dequeue。
- every return path 都标注并实际恢复 mask。
- 没有把 `rt_sigtimedwait` 迁入 `TemporarySigMaskToken` helper。

## Agent 8：验证、旁路审计与收口

职责：做最终旁路审计、最低验证和事务日志收口。

write set：

- 对应事务 devlog。
- 必要的双周 devlog 追加项。
- 测试 profile / user smoke 程序按 repo 现有测试布局选择。
- 只在发现局部遗漏时最小修改前序阶段代码。

旁路审计：

```bash
rg -n "sig_mask|set_sig_mask|TaskSigMaskState|TemporarySigMaskToken" anemone-kernel
rg -n "restore_now|defer_to_signal_delivery|begin_temporary_sig_mask|restore_temporary_sig_mask_if_pending" anemone-kernel
rg -n "perform_signal_action|handle_signals|rt_sigreturn" anemone-kernel/src/task/sig
rg -n "ppoll|pselect6|rt_sigtimedwait|rt_sigsuspend|wait_for_iomux_ready" anemone-kernel/src
rg -n "WaitOutcome::Signal|WaitOutcome::Force|CurrentWaitOutcome::Signal|CurrentWaitOutcome::Force|has_unmasked_signal|SysError::Interrupted" anemone-kernel/src
```

验证：

```bash
just build
git diff --check
```

建议由用户或获授权 agent 运行：

- `rt_sigsuspend01`
- `sigsuspend01`
- `rt_sigtimedwait01`
- `sigtimedwait01`
- signal group 中依赖 mask restore 的用例。

收口要求：

- transaction devlog 记录每阶段 agent-run / user-run / unrun validation。
- RFC status / 收口说明按实际实现状态更新。
- 剩余失败明确归类为非目标、环境限制、accepted limitation 或 follow-up RFC。
- 若有 accepted limitation，更新 register / current limitations，并链接回 RFC 或事务日志。

## Write Set 扩展申请格式

worker 发现更合适的实现需要越界时，必须停止并提交：

```text
申请扩展 write set：
- 当前 agent / 阶段：
- 现有 write set：
- 拟新增文件或模块：
- 为什么现有 write set 无法保持 accepted contract：
- 受影响的不变量或 RFC 条款：
- 需要新增或调整的验证 gate：
- 若不批准的可行降级方案：
```

总控批准后，应把扩展记录写入事务 devlog 或本编排文档的实现期附注，再让 worker
继续。

## 停止边界

应停止并回到 RFC review 的情况：

- 需要多层 temporary mask restore stack。
- 需要把 `rt_sigtimedwait` 纳入 delayed restore helper。
- 需要改变 `rt_sigprocmask` 永久 mask 语义。
- 需要引入完整 Linux restart errno / `restart_syscall`。
- 需要把 cleanup 语义下沉到 arch-specific trap-return 层。
- `classify_temporary_mask_wait()` 无法稳定 reserve / handoff private 或 shared pending
  signal，却仍需要 defer restore 才能通过测试。
- iomux typed outcome 迁移要求 wait-core API 做超出 RFC 的语义重构。

可以继续实现的情况：

- 命名调整但状态转换保持不变量。
- helper 放置模块调整但仍由 signal 子系统拥有。
- classifier 内部 reservation 采用等价机制，而不是文档示例中的具体字段名。
- smoke / LTP 暴露的是已列入本 RFC 的 delayed restore 路径缺口。

## 实现期附注

- 暂无。实现开始后，任何超出本计划默认 write set 的架构性扩展都必须记录原因、拟新增
  范围、受影响 contract、验证 gate 和批准来源。
