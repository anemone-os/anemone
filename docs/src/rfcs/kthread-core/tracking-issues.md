# KThread Core Tracking Issues

**状态：** Closed
**最后更新：** 2026-06-16
**父 RFC：** [RFC-20260616-kthread-core](./index.md)
**事务日志：** [2026-06-16-kthread-core](../../devlog/transactions/2026-06-16-kthread-core.md)

本文只跟踪 design review 后确认的 RFC 草案缺陷、证明缺口、边界冲突或需要回到草案修改的设计问题。

实现前已知缺口、当前基础设施状态、暂缓范围和阶段性交付项不写入本文；它们属于 [RFC index](./index.md) 的背景、非目标、风险，或 [迁移实施计划](./implementation.md) 的阶段内容。

## Apollyon

- 暂无。

## Keter

- 暂无。

## Euclid

- 暂无。

## Safe

- 暂无。

## Neutralized

### KETER-009：ordinary kthread topology gate 不能早于 user-facing API 分流 gate

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 的阶段 4 已改为 `ordinary kthread topology / exit / user-facing API gate`：ordinary kthread `TaskBinding::KThread`、专用 `kthread_exit()`、task-local closeout、topology/procfs unpublish 和最小 user-facing API 分流必须同 gate 合入。
- 阶段 4 现在把 procfs display、wait/reap、job-control、signal、priority、resource limit、scheduler user API 和 `pgid()` / `sid()` / `parent_tgid()` 调用点分类列为启用 ordinary kthread binding 的前置条件。
- 阶段 5 已降级为 post-gate closeout：只能补 source audit、errno/policy 记录和 smoke；若发现阶段 4 遗漏能命中 kthread 的 user-facing path，必须回到阶段 4 补 gate 或回滚 ordinary binding enablement。

**原问题：** 阶段 4 会先让 ordinary kthread 成为 `ThreadGroupType::KThread`，但阶段 5 才完成用户入口分流，制造 “KThread topology + 仍可被 ordinary user-facing API 管理或触发 User-only accessor” 的不可接受中间态。

**原违反的不变量：** ordinary kthread procfs-visible 后仍不能参与 ordinary process lifecycle；User-only accessor 必须在调用前完成 type 分流，procfs inert display 不能反向驱动 job-control、signal permission、waitability 或 lifecycle。

### KETER-001：kthread topology 切换必须与专用 exit / unpublish gate 绑定

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的目标和 topology 方案明确：ordinary kthread 启用 kthread-aware topology、专用 exit 和 topology/procfs unpublish 必须作为同一个语义 gate；提前只能合入不改变 ordinary kthread 发布语义的 scaffolding。
- [不变量需求](./invariants.md) 的闭合条件、topology 状态和禁止退化项明确：ordinary kthread 不能先脱离 ordinary PG/session/children 后仍调用普通 `kernel_exit()`。
- [迁移实施计划](./implementation.md) 的阶段 2 已改为 fixed TID 与 topology preflight；ordinary kthread `TaskBinding::KThread` 切换被推迟到阶段 4，与专用 exit、task-local closeout 和 topology/procfs unpublish 同 gate。

**原问题：** 阶段 2 直接切换 ordinary kthread topology 会制造 “KThread binding + 普通 `kernel_exit()`” 的不可接受中间态。

**原违反的不变量：** kthread topology 语义、exit 线性化和 procfs unpublish 必须同 gate 闭合。

### KETER-002：`kthread_exit()` 不能误删 task-local closeout

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的生命周期和 exit contract 已拆成 user-process cleanup、task-local resource closeout、scheduler zombie tail 三层。
- [不变量需求](./invariants.md) 的 exit 线性化和禁止退化项要求 `kthread_exit()` 跳过 user-process cleanup，但必须保留 task-local closeout；若第一阶段禁止 kthread fd-table，必须用 assert 和注释记录边界及退出条件。
- [迁移实施计划](./implementation.md) 的阶段 4 交付、审计和验证要求复用或显式定义 task-local closeout helper，并确认该 helper 不包含 wait/reap/reparent/job-control 语义。

**原问题：** 只抽 scheduler zombie tail 会漏掉 task-local resource closeout；完整复用 `kernel_exit()` 又会带回 user-process wait/reap/reparent 语义。

**原违反的不变量：** kthread exit 必须剥离 user-process lifecycle，但不能丢失当前 task 自有资源生命周期。

### KETER-003：procfs binding invalidation 必须与 topology unpublish 同协议闭合

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的生命周期段明确：kthread 专用 exit 路径中的 topology unpublish transaction 是 topology/procfs unpublish 的唯一 owner，procfs 只提供 binding invalidation hook。
- [不变量需求](./invariants.md) 的 procfs 可见性和锁序规则明确：unpublish transaction 先暴露 unpublishing/exited 状态，之后 lookup 不得因旧 binding 或残留 topology 重建 `/proc/<pid>`。
- [迁移实施计划](./implementation.md) 的阶段 4 write set、审计和验证覆盖 `fs/proc/tgid/binding.rs`，并要求 source audit 证明 lazy binding 在 unpublishing/removed 状态下不会重建。

**原问题：** 仅移除 topology 不能保证 lazy procfs `<tgid>` binding、dentry 或 inode 不残留，也不能防止 lookup 反向重建。

**原违反的不变量：** procfs 可见性撤销必须与 active topology unpublish 共享唯一 owner 和锁序。

### KETER-004：spawn transaction 必须消除半初始化可见窗口与 start-token 双重所有权

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的文件组织、创建边界、生命周期和 API 章节明确：第一阶段使用 monomorphic `KThreadEntry = fn(KThreadCtx, AnyOpaque) -> i32`，kthread core 只搬运和 drop opaque payload，不 downcast；`Task` 在 topology/procfs publish 前安装 kthread task-local attachment，`kthreadd` 自己也安装 `launch == None` 的 attachment。
- [不变量需求](./invariants.md) 的 `KThread task-local attachment`、`Spawn` 和 entry payload 边界明确：`KThreadTaskLocal` 由 `Task` 拥有，本身不使用 `Arc`；`launch` 是 one-shot startup storage，entry shim 只能 take 一次；publish 是 create transaction commit 边界，publish 后不再出现 recoverable rollback。
- [迁移实施计划](./implementation.md) 的阶段 2 和阶段 3 明确：任何实际发布为 `TaskBinding::KThread` 的 task 都必须先安装 kthread task-local attachment；Stage 3 使用 `AnyOpaque` entry、task-local launch slot 和 publish-before-enqueue commit 顺序，不再通过 `ParameterList` / raw pointer 传递 payload，也不再需要 `KThreadStartToken` typed reclaim 状态机。

**原问题：** 计划要求 `KThreadControl` 初始化早于 task publish，但又写了 topology publish 成功后安装 task-local control link。当前 topology publish 一旦成功，task/thread group 就可能被 procfs、signal 或其它 lookup 观察。另一个窗口是 task enqueue 后 entry shim 可能已经恢复并 drop typed start payload，此时 create transaction 不应继续拥有可 reclaim token。

**原违反的不变量：** kthread 对外可见前必须已经具备 task-local control link；start payload ownership 必须由单一 owner 线性移动，不能让 create failure path 和 entry shim 同时拥有 reclaim 责任。

### KETER-005：user-facing accessor 分流仍需覆盖 direct job-control 和 signal permission helper

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的 topology 调用点分类明确：`getpgid(pid)`、`getsid(pid)`、`SIGCONT` same-session permission check 等 User-only 路径必须先确认 `ThreadGroupType::User`，遇到 kthread 走 fail-closed resolver。
- [不变量需求](./invariants.md) 新增 accessor 分类，并禁止 procfs inert display helper 被 syscall、signal permission 或 job-control 路径当成行为 truth。
- [迁移实施计划](./implementation.md) 的阶段 4 已把 `getpgid.rs`、`getsid.rs`、signal permission helper 以及 `pgid()` / `sid()` / `parent_tgid()` 调用点分类纳入 ordinary kthread binding 同 gate；阶段 5 只做 post-gate closeout。

**原问题：** direct pid job-control 查询和 `SIGCONT` same-session permission helper 仍可能在 kthread target 上调用 User-only `pgid()` / `sid()` accessor。

**原违反的不变量：** procfs inert display 不能反向驱动 job-control 或 signal permission，User-only accessor 必须在进入前完成 type 分流。

### KETER-006：`kthreadd` TID 2 reserve 线性化点不能晚于 AP kinit

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的目标、方案和接受边界明确：普通 TID allocator 从 3 开始，TID 2 由 `kthreadd` 专用 one-shot handle 单独拥有。
- [不变量需求](./invariants.md) 的闭合条件和 `Kthreadd TID allocation` 明确：普通 `alloc_tid()` 永远不会返回 0、1 或 2，`init_kthreadd()` 只能消费 `kthreadd` 专用 handle。
- [迁移实施计划](./implementation.md) 的阶段 2 和阶段 6 明确：验证普通分配起点为 3，AP kinit、clone 和 ordinary kthread 不会竞争 TID 2。

**原问题：** 如果只在 `init_kthreadd()` 内调用 reserve，SMP bootstrap 中 AP kinit 可能在 `init_kthreadd()` 前通过普通 `Task::new_kernel()` 消耗 TID 2。

**原违反的不变量：** `kthreadd` fixed identity 不能依赖启动顺序；普通 allocator 不得与 fixed kernel TID 竞争。

### KETER-007：`TaskFlags::KERNEL` 是否制造第二套 kthread truth

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的 topology 方案和备选方案明确：kthread 与 user process 不发生相互转化，`TaskFlags::KERNEL` 是创建期设置、之后不可变的快速缓存。
- [不变量需求](./invariants.md) 的 topology 状态要求明确：该缓存可用于避免热路径重复获取 topology lock，但不得独立突变，也不得绕过 `ThreadGroupType` 的 publish-time shape assertion。
- [迁移实施计划](./implementation.md) 的阶段 2 和阶段 4/5 审计明确：signal/procfs/job-control 可通过稳定 accessor 使用 immutable `TaskFlags::KERNEL` cache，但创建/publish 路径必须保持它与 `ThreadGroupType::KThread` 一致。

**原问题：** review 曾担心 user-facing API 若读取 `TaskFlags::KERNEL`，会在 `ThreadGroupType` 之外制造第二套 kthread truth。

**原违反的不变量：** 行为状态必须有单一 owner；诊断或缓存字段不得反向驱动状态机。

### KETER-008：SMP / remote wake 验证 floor 未覆盖

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 的阶段 6 focused kthread smoke 已加入：普通 TID 分配从 3 开始、非 BSP CPU placement、跨 CPU `wake()` / `request_stop()` / `wait_exited()`，并要求确认 remote wake path 不因 target CPU offline 或 stale placement 失败。

**原问题：** 第一版验证 floor 只覆盖 TID、spawn/wake/stop 和 procfs，没有明确覆盖非 BSP placement、remote wake、CPU online 顺序和 stale placement。

**原违反的不变量：** kthread core 复用 `TaskSchedState`、wait core 和 runqueue placement；跨 CPU wake 与 owner CPU online 顺序属于第一阶段 runtime closure 的必要证据。

### EUCLID-001：同步创建入口是否需要保留

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的创建边界明确：同步创建不进入第一阶段范围；若未来需要，必须另行设计，且不能绕过本轮确定的 publish / handle / control 不变量。

**原问题：** 同步创建入口可能绕开 `kthreadd` create transaction，形成另一套 publish/rollback/control 路径。

### EUCLID-002：user signal 直指 kthread 的 errno

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 的阶段 4 明确：direct signal to kthread 必须 fail closed，具体 errno 在实现期按现有 signal helper 和 ABI 兼容口径统一；若选择会改变 RFC contract，先更新 tracking issue。

**原问题：** core contract 只应规定 kthread 不被普通 signal path 管理，不必在设计层提前指定所有 errno。

### EUCLID-003：consumer 初始化是否迁入 initcall

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的 initcall 边界和 [迁移实施计划](./implementation.md) 的阶段 6 明确：若 consumer 迁入 initcall，只新增通用 `Late` level，不新增 kthread-specific level。

**原问题：** initcall level 表示初始化时刻，不表示 “是否会 spawn kthread”。新增 kthread-specific initcall level 会把 kthread policy 写进通用初始化框架。

### EUCLID-004：`has_exited()` / `wait_exited()` 的外部完成语义不够明确

**状态：** Neutralized

**修复落点：**

- [不变量需求](./invariants.md) 的 `KThreadControl 状态` 明确区分 internal result completion 与 external exited completion：`KThreadPhase::Exited(code)` 只表示内部结果完成，`has_exited()` / `wait_exited()` 只能观察 `exited` event。
- [不变量需求](./invariants.md) 的 `Exit` 顺序明确：`exited` event 必须在 task-local resource closeout 与 topology/procfs unpublish 完成后发布，作为 external exited completion 的可见点。

**原问题：** 不变量把 kthread exit 的线性化点定义为 `KThreadPhase::Exited(code)`，但建议顺序中该状态早于 task-local resource closeout、topology/procfs unpublish 和 `exited` event publish。与此同时，public handle 暴露 `has_exited()` / `wait_exited()`，容易让 caller 误以为 “exited” 已经等价于 task 完整 closeout 且 `/proc/<pid>` 不可见。

**原违反的不变量：** public lifecycle handle 的完成语义不能早于 task-local closeout 和 procfs/topology 不可见性。

### EUCLID-005：priority/resource 类 user API 的 kthread target policy 需要纳入同一审计

**状态：** Neutralized

**修复落点：**

- [不变量需求](./invariants.md) 的 `Accessor 分类` 已把 priority / scheduler user API 和 pid/tgid resolver 纳入 fail-closed resolver 类。
- [不变量需求](./invariants.md) 新增 `Priority / resource / scheduler resolver 分类`，要求 direct mutating target 命中 kthread 时 fail closed，read-only 仅允许明确 inert readonly，process-group/session/uid/broadcast 枚举跳过 kthread，stage-1 stub 也必须记录 policy。

**原问题：** 阶段 5 已提到 priority / scheduler user API 需要按 type 跳过或 fail closed，但计划主体仍主要围绕 procfs、wait/job-control 和 signal 展开，没有把 resource-style pid target API 纳入同一个调用点分类规则。

**原违反的不变量：** 所有 user-facing resolver 必须共享 kthread target policy，不能让 priority/resource/scheduler API 绕过 topology type 分流。

### SAFE-001：closure builder API

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的 API 章节和 [不变量需求](./invariants.md) 的 entry payload 边界明确：`AnyOpaque` start argument 是第一阶段 accepted API；closure builder 或泛型 owned payload API 是 optional follow-up。

**原问题：** closure API 更 Rusty，但不影响第一阶段 correctness。

### SAFE-002：procfs pgrp/session Linux 展示兼容

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 和 [不变量需求](./invariants.md) 明确：第一版 kthread procfs pgrp/session 类字段输出 `0` 或等价 inert value，Linux 展示兼容后续小迭代处理。

**原问题：** 更接近 Linux 的展示语义不应阻塞第一阶段 core owner-boundary 修正。
