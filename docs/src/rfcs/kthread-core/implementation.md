# KThread Core 迁移实施计划

**状态：** Accepted for Implementation，阶段 6 gate 已关闭
**最后更新：** 2026-06-17
**父 RFC：** [RFC-20260616-kthread-core](./index.md)
**不变量：** [不变量需求](./invariants.md)

本文按可提交、可 review、可回滚的阶段拆分 kthread-core 纠偏实现。协议和不变量以 [不变量需求](./invariants.md) 为准；本文只定义落地顺序、write set、审计范围、验证 floor 和停止边界。

补充约束：`KThreadService` 已从目标设计中移除。若某个实现 checkout 仍有 legacy service 模块、符号、注释或 consumer 依赖，它们只作为残留清理处理，不再作为未来抽象、兼容桥或阶段目标保留。

## 迁移原则

1. 每个阶段完成后都必须能独立构建，并能说明没有违反本文对应 gate。
2. 先收窄 core surface，再修改 topology 和 exit。不要在仍有 service / park 语义的状态下继续扩大 kthread contract。
3. `kthreadd` 只负责创建 ordinary kthread；它不是 service dispatcher、workqueue manager、stop owner 或 exit result owner。
4. active identity 由 task topology 的 `Tid/Tgid` 和 `ThreadGroupType` 拥有；lifecycle 由 strong control handle 拥有。
5. stop 是协作式协议，wake 是纯唤醒能力。business predicate、pending work、active victim、pressure state 均属于 consumer。
6. `TaskSchedState` 仍是 scheduler runnable / waiting / zombie 的唯一真相源；kthread-core 不手工做 runqueue placement。
7. 若某阶段发现需要改变 topology identity、exit 线性化点、handle lifetime、procfs visibility 或 user-signal fail-closed 语义，必须先更新 RFC 文本和 tracking issues，不能只在实现注释里决定。

## 全局阶段 Gate

通用 gate：

1. 实现开始前必须创建 transaction devlog，或由负责人明确允许先做一次 RFC 约束下的 preflight patch。事务日志建立后，每个阶段退出都要记录实际 write set、验证证据和残余风险。
2. 每个代码阶段至少运行 `git diff --check` 和 `just build`。涉及 boot、topology、exit 或 procfs visibility 的阶段还需要最小 boot / procfs smoke。
3. 阶段退出声明必须引用对应 tracking gate 的处理结果。只通过构建不能替代 owner-boundary、source-of-truth、exit path 和 procfs/signal/wait 审计。
4. Safe / Euclid 级命名、文件拆分、closure API、Linux procfs 展示兼容不得升级为阶段 blocker，除非它们实际改变不变量。
5. 不得新增 service/request/workqueue 兼容层来临时接住 consumer。consumer 必须用 explicit loop 和自身 predicate 表达业务状态。

建议 code owner split：

1. `task::kthread` 只暴露 spawn、handle、ctx、stop/wake/wait-exited 和 `AnyOpaque` entry API。第一版目标文件是 `mod.rs`、`spawn.rs`、`kthreadd.rs`、`entry.rs`、`control.rs`、`handle.rs`、`ctx.rs`；不要新增 `service.rs`、`registry.rs` 或 `utils.rs`。
2. `task::topology` 拥有 `ThreadGroupType`、active identity、procfs-visible membership、ordinary user process parent/wait membership 和 kthread parent display anchor。
3. `task::exit` 拥有 user-process exit 与 kthread exit 的分流，以及可共享的 scheduler zombie tail helper。
4. procfs、wait、job-control、signal、priority 和 resource-style user API 只读取 topology type / accessor，不导入 kthread private control state。
5. initcall level 表达初始化时刻，不表达 “是否会 spawn kthread”。`kthreadd` 手动初始化；consumer 归属各自子系统 init path。

## 阶段 0：工作流与基线 Preflight

目标：在动代码前固定实现边界，避免把 legacy shape 当成新设计事实。

前置条件：

1. 本 RFC 已完成文档层 review，可指导实现。
2. 负责人已确认是否直接创建 transaction devlog，或先做一次 RFC 约束下的 implementation preflight。

交付：

1. 创建或更新 transaction devlog，记录本文为 canonical implementation plan。
2. 记录 starting inventory：
   - `task::kthread` 当前 public surface。
   - 是否仍有 `KThreadService` / service 模块残留。
   - 是否仍有 park/unpark 状态、API、consumer 调用。
   - `kthreadd` 当前 TID 分配方式。
   - ordinary kthread 当前 topology binding。
   - entry shim 当前 exit path。
   - 当前 initcall levels 与手写 consumer 初始化位置。
3. 固定第一轮 write set，并声明后续 write set 扩展规则。
4. 若需要同一 owner 内文件拆分，先作为 split-only checkpoint 记录，不混入语义修改。

审计：

1. `service` 只允许作为已删除事实或残留清理目标出现，不能作为 future layer 出现在 implementation plan、comments 或 public API。
2. 当前 consumer 分类为 explicit loop，不作为 service worker 迁移对象。
3. `Task::new_kernel()` 直接使用点按 bootstrap task、`kthreadd`、ordinary kthread、clone/internal helper 分类。
4. `kthreadd` 不迁入 initcall；若 consumer 需要 initcall 化，只考虑通用 `Late` level。

验证：

1. 文档变更：`git diff --check`。
2. 若同时做 split-only patch：`just build`。

退出条件：

1. transaction / preflight 记录能回答：下一阶段要改哪些文件、为什么、验证什么。
2. 没有把 service、park/unpark、独立 registry 或 weak-only handle 写成未来 accepted contract。

## 阶段 1：收窄 kthread core surface

目标：删除会误导后续状态机的上层或未使用协议，使后续 handle/control 和 exit 改造只面对 stop、wake、exit。

前置条件：

1. 阶段 0 完成。
2. inode shrinker 和 OOM killer 已确认可以用 explicit loop 表达业务 state。

交付：

1. 确认 `KThreadService` 已删除；若仍有残留，删除模块、re-export、注释、文档引用和 build wiring。
2. 删除 park/unpark：
   - 删除 `start_parked`、`park()`、`unpark()`、`should_park()`、`parkme()`。
   - 删除 `Parking` / `Parked` 状态。
   - 删除 `wait_until_unpark_or_stop()` 和 park-specific state transition。
3. 将 `KThreadControl` 临时收窄为 stop、wake、exited/result 所需状态；strong handle 可在阶段 3 完整替换。
4. inode shrinker / OOM killer consumer loop 只检查 `should_stop()` 和自身业务 predicate。
5. `wait_until_woken()` 注释和语义不得再引用 service pending backend。

write set：

1. `anemone-kernel/src/task/kthread/{mod.rs,spawn.rs,kthreadd.rs,entry.rs,control.rs,handle.rs,ctx.rs}`
2. `anemone-kernel/src/fs/inode_shrinker.rs`
3. `anemone-kernel/src/mm/oom.rs`
4. 若 residual service 文件仍存在：`anemone-kernel/src/task/kthread/service.rs`

审计：

1. `rg "KThreadService|service.rs|should_park|parkme|start_parked|Parking|Parked|unpark"` 只允许命中历史 RFC、tracking issue 或明确的 rejected text。
2. consumer 不直接持有 `Arc<Task>`，不直接调用 `Task::new_kernel()`。
3. wake 仍只是唤醒 entry 重查 stop 和 consumer predicate。

验证：

1. `git diff --check`
2. `just build`
3. 最小 source audit：consumer loop 没有 service/park 依赖。

退出条件：

1. kthread core public surface 只剩 create、stop/request-stop、wake、wait-exited/result、context wait helper 和 `AnyOpaque` entry 方向。
2. service / park 不再对后续状态机和 lifecycle proof 造成约束。

## 阶段 2：固定 `kthreadd` TID 与 topology preflight

目标：先落地 `kthreadd` fixed TID identity、`ThreadGroupType` / accessor / shape assertion scaffolding，避免后续阶段继续依赖未区分的 topology。阶段 2 不是任何 kthread 的实际 `TaskBinding::KThread` publish gate；`kthreadd` 和 ordinary kthread 都不能在仍调用完整 `kernel_exit()`、仍缺少 task-local prepublish attachment 或仍缺少 user-facing API 分流的状态下切到 `TaskBinding::KThread`。所有实际 `KThread` publish 均并入阶段 4 同 gate。

前置条件：

1. 阶段 1 完成。
2. `Task::new_kernel()`、TID allocator 和 topology publish guard 的 owner boundary 已确认。

交付：

1. TID allocator：
   - 增加 `Tid::KTHREADD == 2` 或等价常量。
   - 普通 allocator 初始化范围从 3 开始；普通 `alloc_tid()` 永远不会返回 0、1 或 2。
   - TID 2 不进入普通 allocator；新增 `kthreadd` 专用 one-shot TID handle，不新增通用 `reserve_tid(Tid)`。
   - `init_kthreadd()` 只能通过该专用 handle 创建 TID/TGID 2。
   - 若专用 handle 已被消费，或 topology 中已有 TID 2 非 `kthreadd` task，必须 panic。
2. Topology type：
   - 引入 `ThreadGroupType::{User, KThread}`。
   - `init` / user process 使用 `User`。
   - `KThread` type 在阶段 2 只作为编译期 type / accessor / assertion preflight 出现；不得把 `kthreadd` 或 ordinary kthread create path 切到 `TaskBinding::KThread`。
   - `kthreadd` 的实际 `KThread` publish 留到阶段 4：届时必须在 publish 前安装 `KThreadTaskLocal { control, launch: None }`，`tid == tgid == Tid::KTHREADD`，thread group 只有 leader 自己。
   - `kthreadd` 的特殊身份由 `tgid == Tid::KTHREADD` 派生，不新增单独 topology type。
3. Binding API：
   - 用 `TaskBinding::{UserLeader, KThread, Member}` 表达 user-process leader、kernel thread leader、thread member。
   - 阶段 2 的 `KThread` binding 只能作为 fail-fast scaffolding，不允许有实际消费点。
   - `kthreadd` 和 ordinary kthread binding 切换均留到阶段 4，与专用 `kthread_exit()`、task-local closeout、topology/procfs unpublish 和最小 user-facing API 分流同一个 review gate。
   - `KThread` publish 不得继承 ordinary `pgid/sid`。
   - `KThread` 不插入 `ProcessGroup.members` 或 `Session.process_groups`。
   - ordinary kthread 的 procfs display parent 规则可以先定义 helper shape，但不得在阶段 2 对 active ordinary kthread 生效；实际启用留到阶段 4。
   - `kthreadd` 自己的 procfs display parent 输出 `0` 或等价 inert value。
4. Accessor：
   - `ThreadGroup::ty()` 返回 `ThreadGroupType`。
   - `pgid()`、`sid()`、`parent_tgid()` 默认假设 `User`，非 `User` 直接 panic。
   - procfs 自己通过 display helper 把 kthread parent/pgrp/session 映射成第一版 inert view；process API 不使用 procfs helper。
5. Shape assertions：
   - `User` 的 `pgid/sid` 必须存在。
   - `KThread` 的 `pgid/sid` 必须不存在。
   - `KThread` 的 `children_tgids` 必须为空。
   - `Member` 只能加入 `User` thread group，非 user 直接 panic。
   - kthread 与 user process 不发生相互转化；`TaskFlags::KERNEL` 作为创建期设置、之后不可变的快速缓存，只能与 publish-time `ThreadGroupType` shape assertion 一起使用。

write set：

1. `anemone-kernel/src/task/tid.rs`
2. `anemone-kernel/src/task/mod.rs`
3. `anemone-kernel/src/task/kthread/kthreadd.rs`
4. `anemone-kernel/src/task/topology/{mod.rs,thread_group.rs,parent_child.rs,process_group.rs}`
5. bootstrap call sites only if required by the TID reserve API.

审计：

1. `alloc_tid()` 普通路径不会返回 0、1 或 2。
2. `init_kthreadd()` assert `tid == tgid == Tid::KTHREADD`。
3. 阶段 2 diff 中 `kthreadd` 与 ordinary kthread create path 均未切到 `TaskBinding::KThread`；若为编译引入临时桥，注释必须说明它只持续到阶段 4。
4. `KThread` shape assertion 已覆盖无 `pgid/sid`、空 `children_tgids`、singleton member、task-local attachment present 和 `Member` 禁止加入 `KThread`。
5. `for_each_thread_group_from()` 对 `ThreadGroupType` scaffolding 仍能枚举 active thread group，供后续 procfs root 使用。

验证：

1. `git diff --check`
2. `just build`
3. focused smoke / log：`kthreadd` TID/TGID 为 2；AP kinit 或其它 kernel task 不消耗 2；普通分配起点为 3。
4. source audit：`kthreadd` 与 ordinary kthread create path 均没有在阶段 2 切到 `TaskBinding::KThread`；`TaskBinding::KThread` 只是未启用的 scaffolding。

退出条件：

1. `kthreadd` fixed TID anchor 已成为单一真相源；TID 2 不依赖启动顺序或普通 allocator。
2. topology type、process-only accessor 和 shape assertion 已就绪，可供阶段 4 一次性启用 `kthreadd` / ordinary kthread binding、专用 exit、unpublish 和 user-facing API gate。
3. `kthreadd` 与 ordinary kthread 仍处在迁移中间态；阶段 2 退出声明不得声称 kthread 已满足最终 topology contract。

## 阶段 3：strong `KThreadHandle` 与 create transaction

目标：用 strong lifecycle capability 替换 weak-only handle，并把 create success/failure 的 publish commit 边界、task-local attachment 和 control lifetime 固定下来。

前置条件：

1. 阶段 2 完成。
2. `task::kthread` 已无 service/park 状态。

交付：

1. 引入 public `KThreadHandle`，内部 strong 持有 `KThreadControl`。
2. 删除内部 `KThread` 实体。`Task` 的 task-local `kthread` 字段直接保存 kthread 专用 attachment；attachment 本身不使用 `Arc`，需要跨 handle、ctx 和 exit path 共享的只有 `Arc<KThreadControl>`。
3. `KThreadControl` 最小拥有：
   - `phase: SpinLock<KThreadPhase>`
   - `wake: Event`
   - `exited: Event`
4. `KThreadPhase` 第一版只包含 `Running`、`StopRequested`、`Exited(i32)`。
5. `KThreadControl` 不强持有 `Task`，也不保存 `Tid`、`name` 或 `created_at`。
6. `KThreadCtx` 只持有窄 control/ctx capability，提供：
   - `should_stop()`
   - `wait_until(predicate)`
7. `KThreadHandle` public API 只提供：
   - `request_stop()`
   - `wake()`
   - `wait_exited() -> i32`
   - `has_exited()`
8. `wait_exited()` / `has_exited()` 只观察 external exited completion event；该 event 必须在阶段 4 的 task-local resource closeout 和 topology/procfs unpublish 完成后发布，不能直接把 `phase == Exited(code)` 当作对外完成语义。
9. 不提供 public `stop()`。同步 stop 由 caller 显式组合 `request_stop(); wait_exited()`。
10. `KThreadBuilder::spawn()` 返回 `KThreadHandle`。legacy `KThreadRef` 若保留一轮，只能是内部临时兼容残留，不作为 public lifecycle capability 对外暴露。
11. `KThreadBuilder` 使用 `KThreadPlacement::{Any, OnCpu(CpuId)}`；`cpu()` 只是 convenience。
12. `AnyOpaque` entry：
   - `pub type KThreadEntry = fn(KThreadCtx, AnyOpaque) -> i32`
   - kthread core 不 downcast `AnyOpaque`；consumer entry/helper 拥有 concrete payload 解释。
   - entry 返回的 `i32` 是 kthread-local completion code，不是 `SysError` / errno contract。
13. task-local attachment：
   - `KThreadTaskLocal { control: Arc<KThreadControl>, launch: SpinLock<Option<KThreadLaunch>> }`
   - `KThreadLaunch { entry: KThreadEntry, arg: AnyOpaque }`
   - `KThreadTaskLocal` 由 `Task` 拥有，不提供 `Arc<KThreadTaskLocal>` 给外部 owner。
   - ordinary kthread 的 `launch` 初始为 `Some(KThreadLaunch)`，entry shim 第一件事 `take()`；重复进入或缺失 launch 必须 panic。
   - `kthreadd` 也安装 `KThreadTaskLocal`，但 `launch == None`，且第一阶段不提供 public handle 指向 `kthreadd`。
14. create transaction：
   - `spawn()` 消费 `AnyOpaque`；失败时 drop payload，不返还给 caller。
   - 创建 unpublished task。
   - 初始化 `Arc<KThreadControl>` 和 `KThreadLaunch`。
   - 在 task 对 topology/procfs 可见前安装 `KThreadTaskLocal`。
   - publish 前 assert task-local control link 已安装；所有 `TaskBinding::KThread` publish 都必须满足该 precondition。
   - topology/procfs publish 是 commit 边界；publish 前失败可以 drop unpublished task、control 和 launch payload。
   - publish 后只能执行 infallible enqueue 和 success completion，不再出现 recoverable rollback。
   - kthread start payload 不通过 `ParameterList` 或 raw pointer 传递。
15. kthreadd transaction internal names：
   - `SpawnRequest`
   - `SpawnReply`
   - `SpawnOutcome`
   - `kthreadd::submit()`
   - `kthreadd::run()`
   - `kthreadd::spawn()`
16. `kthreadd.rs` 内部静态名：
   - `KTHREADD`
   - `SPAWN_QUEUE`
   - `SPAWN_WAKE`

write set：

1. `anemone-kernel/src/task/kthread/{mod.rs,spawn.rs,kthreadd.rs,entry.rs,control.rs,handle.rs,ctx.rs}`
3. `anemone-kernel/src/task/mod.rs`
4. `anemone-kernel/src/fs/inode_shrinker.rs`
5. `anemone-kernel/src/mm/oom.rs`

审计：

1. public API 不暴露 `Arc<Task>`、raw `TaskSchedState` 或 topology mutation 能力。
2. `request_stop()` 幂等；already-exited request stop 幂等成功。
3. `wake()` 不修改 stop/result/exited，不表达 business request。
4. `wait_exited()` 不持有 topology lock 等待 event。
5. control 不保存 post-exit diagnostic identity；active tid/name 查询只能来自 task/topology。
6. task-local launch slot 只被 entry shim take 一次；kthread core 不 downcast `AnyOpaque`。
7. publish 后没有可失败 rollback 点；enqueue 和 success completion 是 commit 后步骤。

验证：

1. `git diff --check`
2. `just build`
3. focused smoke：spawn、wake、request_stop、entry return、wait_exited、already-exited request_stop。
4. source audit：外部 owner 不再依赖 weak-only `KThreadRef` 等待 exit result。
5. source audit：无内部 `KThread` object 作为 Task/topology/control 之外的第四实体。
6. source audit：kthread payload 不经 `ParameterList` / raw pointer 传递，entry shim 从 current task 的 launch slot take start。

退出条件：

1. lifecycle owner 是 strong handle/control，不是 topology、`kthreadd`、scheduler state、weak ref 或内部 `KThread` object。
2. create failure、publish commit 和 entry start payload move/drop 协议有可复审证据，且不依赖 typed erased pointer reclaim。

## 阶段 4：kthreadd / ordinary kthread topology / exit / user-facing API gate

目标：让 `kthreadd` / ordinary kthread binding 切换、专用 `kthread_exit()`、task-local closeout、topology/procfs unpublish 和最小 user-facing API 分流在同一个 gate 闭合。ordinary kthread exit 不再调用完整 user-process `kernel_exit()`，但也不能漏掉 task-local resource closeout；任何 kthread 一旦发布为 `TaskBinding::KThread`，用户入口也不能再把它当 ordinary process 管理。

前置条件：

1. 阶段 3 完成。
2. topology 已能按 kthread type 执行 kthread-specific unpublish。
3. 阶段 2 只完成 preflight，`kthreadd` 与 ordinary kthread binding 均尚未切到 `TaskBinding::KThread`。
4. `pgid()`、`sid()`、`parent_tgid()` 调用点已有 source inventory；未分类调用点不能和 ordinary kthread binding 一起合入。

交付：

1. 在 `task::exit` 中拆出三层边界：
   - user-process cleanup：clear-child-tid、robust futex、user address space、ordinary thread-group exit、child-exited、reparent、`SIGCHLD`、wait/reap、vfork completion。
   - task-local resource closeout：只处理当前 task 拥有且不属于 user wait/reap topology 的资源释放；若第一阶段规定 kthread fd table 永远为空，必须在 kthread exit 前 assert 为空，并在注释中写明边界和移除条件。
   - scheduler zombie tail：只负责 task 进入 `Zombie`、schedule away、never return。
2. 在 `task::exit` 中拆出只表达 scheduler tail 的内部 helper，例如：
   - set current task sched state to `Zombie`
   - schedule away
   - never return
3. 新增 `kthread_exit(result: i32) -> !` 或等价 kthread entry tail。
4. `kthread_entry_shim` 在 entry 返回或 stop-before-entry 后调用专用 kthread exit tail，不调用完整 `kernel_exit()`。
5. 同一 gate 启用 `kthreadd` `TaskBinding::KThread`：
   - `kthreadd` publish 前安装 `KThreadTaskLocal { control, launch: None }`。
   - `kthreadd` 不继承 ordinary `pgid/sid`，不插入 `ProcessGroup.members` 或 `Session.process_groups`。
   - `kthreadd` 自己的 procfs display parent、pgrp 和 session 输出 inert value。
6. 同一 gate 启用 ordinary kthread `TaskBinding::KThread`：
   - ordinary kthread publish 不继承 ordinary `pgid/sid`。
   - ordinary kthread 不插入 `ProcessGroup.members`、`Session.process_groups` 或 parent `children_tgids`。
   - ordinary kthread 的 procfs display parent 是 `Tid::KTHREADD`，但这只是展示 helper，不形成 waitable parent。
7. 同一 gate 完成最小 user-facing API 分流；这是启用任何 kthread `TaskBinding::KThread` 的前置条件，不是后续阶段：
   - procfs root readdir / lookup 可以枚举 active `KThread`；`/proc/<pid>/status`、`stat` 和 `cmdline` 使用 kthread display helper 输出 inert parent、pgrp/session 和空 cmdline。
   - wait4/waitid 不会 match 或 reap kthread，且不能从 procfs display parent 推导 waitability。
   - `setpgid()`、`setsid()`、process-group mutation 对 kthread fail closed。
   - `getpgid(pid)` / `getsid(pid)` 直指 kthread 时先按 `ThreadGroupType` 分流，不能调用 User-only `pgid()` / `sid()`；返回 errno 或 inert policy 必须在本 gate 统一并记录。
   - `kill(-pgid)` 和 session / process-group 路径不会触达 kthread；`kill(-1)` broadcast 跳过 kthread。
   - `kill(tgid)`、`tkill(tid)`、`tgkill(tgid, tid)`、`rt_sigqueueinfo()` 直指 kthread 时 fail closed。具体 errno 在本 gate 按现有 signal helper 统一；若选择会改变 RFC contract，先更新 tracking issue。
   - signal permission helper 中 `SIGCONT` same-session check 必须在读取 target `sid()` 前完成 kthread 分流；procfs inert session 不得作为 permission truth。
   - priority、resource limit、scheduler user API 的 pid / task-id target 遇到 kthread 必须 fail closed 或记录为只读 inert policy；UID、process-group 或 broadcast 枚举必须跳过 kthread。
   - `pgid()`、`sid()`、`parent_tgid()` 的所有调用点必须分类为 User-only caller、procfs display helper 或 kthread fail-closed resolver；source audit 结果写入 transaction devlog 或阶段退出记录。
8. exit 线性化顺序：
   - complete control `exit_result`
   - run kthread task-local resource closeout，或 assert 第一阶段 fd table / task-local resources 为空
   - unpublish kthread procfs/topology binding
   - publish `exited` event
   - enter scheduler zombie tail
9. kthread topology/procfs unpublish owner：
   - `task::topology` 拥有唯一 kthread unpublish transaction，procfs `<tgid>` binding invalidation 只能通过该 transaction 调用的窄 helper 发生。
   - lookup / readdir 只以 topology published/alive 状态为可重建条件；一旦 unpublish transaction 标记 kthread 为 unpublishing 或 removed，procfs lookup 不得重建 `<tgid>` binding。
   - 锁序固定为 topology publish/unpublish lock -> procfs `<tgid>` binding transaction lock -> control/event -> sched-state；不得从 procfs binding 路径反向持锁后再进入 topology mutation。
10. kthread topology unpublish：
   - 标记该 kthread 不再可被 procfs lookup 重建。
   - invalidate 已存在的 procfs `<tgid>` binding / dentry / inode 可见入口。
   - 移除 task registry entry。
   - 移除 singleton kthread thread group。
   - 不触发 ordinary child-exited event。
   - 不 reparent children。
   - 不等待 user-space reap。
11. `kernel_exit()` 保留 guard：若 ordinary kthread 意外进入完整 user-process exit，panic 或 fail closed。

write set：

1. `anemone-kernel/src/task/api/exit/mod.rs`
2. `anemone-kernel/src/task/kthread/{mod.rs,entry.rs,control.rs,kthreadd.rs}`
3. `anemone-kernel/src/task/topology/{mod.rs,thread_group.rs,parent_child.rs}`
4. `anemone-kernel/src/fs/proc/tgid/binding.rs`
5. `anemone-kernel/src/task/mod.rs`
6. `anemone-kernel/src/fs/proc/root/file.rs`
7. `anemone-kernel/src/fs/proc/tgid/{cmdline.rs,mod.rs,stat.rs,status.rs}`
8. `anemone-kernel/src/task/api/wait/*`
9. `anemone-kernel/src/task/api/jobctl/{getpgid.rs,getsid.rs,setpgid.rs,setsid.rs}`
10. `anemone-kernel/src/task/sig/api/{mod.rs,kill.rs,tkill.rs,tgkill.rs,rt_sigqueueinfo.rs}`
11. `anemone-kernel/src/task/api/priority.rs`
12. resource / scheduler user API files if the source inventory shows pid, process-group, UID or broadcast target handling.
13. topology accessors and display helpers needed by these paths.

审计：

1. kthread exit 不执行 clear-child-tid、robust futex cleanup、user address-space cleanup、thread-group child-exited event、reparent、parent `SIGCHLD`、ordinary wait/reap 或 vfork completion。
2. task-local resource closeout 已复用 user exit 中可共享的 task-local helper，或以 assert/comment 明确第一阶段 kthread fd table 为空、资源边界和退出条件。
3. topology/procfs unpublish 后 `/proc/<pid>` lookup 不再找到该 kthread，且不能通过 lazy binding 重建。
4. control result 和 exited event 在 topology/procfs removal 后仍可由 strong handle 读取。
5. scheduler zombie tail 不包含 user-process cleanup 或 procfs binding mutation。
6. `kthreadd` 与 ordinary kthread 只在本阶段切到 `TaskBinding::KThread`，且切换 diff 与专用 exit / unpublish diff 同 gate review。
7. procfs display fields do not drive job-control, signal permission, waitability or lifecycle.
8. wait/job-control/signal/priority/resource/scheduler user paths have explicit kthread type handling or cannot reach kthread by construction.
9. `getpgid(pid)`、`getsid(pid)` 和 `SIGCONT` same-session permission check 不会在 kthread target 上调用 User-only accessor。
10. broadcast、UID 和 process-group operations 不能通过 PG/session membership 或 global task enumeration 管理 kthread。
11. `pgid()` / `sid()` / `parent_tgid()` 调用点分类结果写入 transaction devlog 或阶段退出记录。

验证：

1. `git diff --check`
2. `just build`
3. focused smoke：entry return 后 `wait_exited()` 得到 result；退出后 `/proc/<pid>` 不可见；ordinary user process exit 不退化。
4. source audit：`rg "kernel_exit\\(" anemone-kernel/src/task/kthread anemone-kernel/src/task/api/exit` 确认 kthread entry 不调用完整 `kernel_exit()`。
5. source audit：procfs `<tgid>` lookup / binding creation 路径在 topology unpublishing 或 removed 状态下不会重建 kthread binding。
6. source audit：kthread exit 的 task-local closeout helper 不包含 wait/reap/reparent/job-control 语义。
7. procfs smoke：`/proc/2/status`、ordinary kthread `/proc/<pid>/status`、`stat`、`cmdline`。
8. job-control smoke/source audit：`getpgid(pid)`、`getsid(pid)`、`setpgid()`、`setsid()` 对 kthread target 不触发 User-only accessor panic。
9. signal smoke/source audit：direct signal fail-closed，`kill(-pgid)` / `kill(-1)` 不触达 kthread，`SIGCONT` same-session helper 在读取 target session 前完成 kthread 分流。
10. source audit：priority、resource limit、scheduler user API 的 direct target、process-group、UID 和 broadcast 枚举按 kthread policy 分流或跳过。

退出条件：

1. ordinary kthread binding、kthread exit path、task-local closeout 和 procfs/topology unpublish 在同一个 gate 闭合。
2. kthread exit path 与 user-process exit path 在语义层分离。
3. kthread 不产生 user-visible zombie / tombstone，也不能被 ordinary wait reaped。
4. kthread procfs-visible 但不被 ordinary user-facing process API signal、reap、regroup、session-manage、priority-manage 或 resource-manage。

## 阶段 5：post-gate user-facing boundary closeout

目标：在阶段 4 已闭合最小 user-facing API 分流后，补齐更广的 source audit、errno/policy 记录和 runtime smoke。阶段 5 不能再承担 “让 ordinary kthread `TaskBinding::KThread` 变安全” 的职责；若阶段 4 未完成最小分流，不允许进入本阶段。

前置条件：

1. 阶段 2 和阶段 4 完成，且阶段 4 的 ordinary kthread binding / exit / unpublish / 最小 user-facing API gate 已同 gate 合入。
2. topology accessor 已能区分 process-only fields 与 procfs display fields。

交付：

1. 复查阶段 4 的 procfs、wait/reap、job-control、signal、priority、resource limit 和 scheduler user API 分流，补齐遗漏的 smoke 和 source evidence。
2. 固化 errno / inert policy 记录；若某个 direct user API 的选择会改变 RFC contract，先更新 tracking issue。
3. 确认 User-only caller 只能从 ordinary user process topology 到达，或在调用前检查 `ThreadGroupType::User`。
4. 确认 procfs display helper 只能生成展示字段，不能被 job-control、wait/reap、signal permission、priority/resource 或 scheduler user API 复用。
5. 若发现阶段 4 遗漏任何能命中 kthread 的 user-facing path，必须回滚 ordinary kthread `TaskBinding::KThread` enablement 或回到阶段 4 补 gate；不得把它作为阶段 5 普通后续项。

write set：

1. `anemone-kernel/src/fs/proc/root/file.rs`
2. `anemone-kernel/src/fs/proc/tgid/{binding.rs,cmdline.rs,mod.rs,stat.rs,status.rs}`
3. `anemone-kernel/src/task/api/wait/*`
4. `anemone-kernel/src/task/api/jobctl/{getpgid.rs,getsid.rs,setpgid.rs,setsid.rs}`
5. `anemone-kernel/src/task/sig/api/{mod.rs,kill.rs,tkill.rs,tgkill.rs,rt_sigqueueinfo.rs}`
6. `anemone-kernel/src/task/api/priority.rs`
7. resource / scheduler user API files if the stage 4 source inventory shows pid, process-group, UID or broadcast target handling.
8. topology accessors and display helpers needed by these paths.

审计：

1. procfs display fields do not drive job-control or lifecycle.
2. signal paths do not import `task::kthread` private control state; they branch on `ThreadGroupType` or the immutable creation-time `TaskFlags::KERNEL` cache through stable accessors.
3. broadcast and process-group operations cannot observe kthread via PG/session membership.
4. wait code cannot infer waitability from procfs-visible parent display.
5. `getpgid(pid)`、`getsid(pid)` 和 `SIGCONT` same-session permission check 不会在 kthread target 上调用 User-only accessor。
6. `pgid()` / `sid()` / `parent_tgid()` 调用点分类结果写入 transaction devlog 或阶段退出记录。
7. priority、resource limit 和 scheduler user API 的 target policy 与阶段 4 gate 记录一致，没有新增可命中 kthread 的未分类路径。

验证：

1. `git diff --check`
2. `just build`
3. procfs smoke：`/proc/2/status`、ordinary kthread `/proc/<pid>/status`、`stat`、`cmdline`。
4. job-control smoke/source audit：`getpgid(pid)`、`getsid(pid)`、`setpgid()`、`setsid()` 对 kthread target 不触发 User-only accessor panic。
5. signal smoke/source audit：direct signal fail-closed，`kill(-pgid)` / `kill(-1)` 不触达 kthread，`SIGCONT` same-session helper 在读取 target session 前完成 kthread 分流。
6. source audit：wait/job-control/signal/priority/resource/scheduler user paths have explicit kthread type handling or cannot reach kthread by construction.

退出条件：

1. kthread is procfs-visible but not ordinary process-manageable.
2. user-facing APIs cannot accidentally signal, reap, regroup, session-manage, priority-manage or resource-manage kthreads.

## 阶段 6：consumer closeout 与完整验证

目标：把 current consumer 和全局 source audit 收口，证明第一阶段 kthread-core 可作为后续 consumer 的基础设施。

前置条件：

1. 阶段 1 到阶段 5 完成。
2. tracking issues 中所有 Keter gate 均有实现位置和验证证据。

交付：

1. inode shrinker：
   - 持有 strong `KThreadHandle`。
   - explicit loop 只依赖 frame stats / VFS cache predicate。
   - 不依赖 service、park/unpark 或 raw task。
2. OOM killer：
   - 持有 strong `KThreadHandle`。
   - wake 只提示重查 frame pressure / active victim。
   - victim selection 跳过 kthread type，不只依赖 ad hoc task flags。
3. kthread core source audit：
   - 无 public `KThreadRef` lifecycle dependency。
   - 无 `KThreadService`。
   - 无 park/unpark state。
   - 无 independent registry / `KThreadId`。
   - ordinary kthread 只通过 builder/create protocol 创建。
4. 更新 tracking issues 和 transaction devlog，记录每个 gate 的闭合证据。
5. 如果把 consumer 初始化改成 initcall，新增通用 `Late` level，并把 inode shrinker / OOM killer 挂到各自子系统 late init；不得新增 kthread-specific initcall level。

write set：

1. `anemone-kernel/src/fs/inode_shrinker.rs`
2. `anemone-kernel/src/mm/oom.rs`
3. `anemone-kernel/src/task/kthread/*`
4. transaction devlog and public RFC status files, if implementation is already public.
5. 若引入 `Late` initcall：`anemone-kernel/src/initcall.rs`、linker script、macro accepted-level list 和 `main.rs` 调用点。

验证：

1. `git diff --check`
2. `just build`
3. boot smoke：`kthreadd`、inode shrinker、OOM killer worker 启动且无 panic。
4. focused kthread smoke：
   - fixed `kthreadd` TID 2
   - ordinary TID allocation starts at 3
   - spawn
   - wake
   - request_stop
   - entry return
   - wait_exited
   - already-exited request_stop
   - procfs visibility before exit and invisibility after exit
   - SMP smoke：所有 CPU online 后将 kthread placement 到非 BSP CPU，从另一 CPU 执行 `wake()` / `request_stop()` / `wait_exited()`，确认 remote wake path 不因 target CPU offline 或 stale placement 失败。
5. source audit：
   - `Task::new_kernel()` 使用点分类完成。
   - kthread exit 不调用完整 `kernel_exit()`
   - ordinary kthread 不在 PG/session membership 中
   - no service/park residual.
6. 若 boot/topology/exit 改动已进入公开 implementation branch，建议运行最小 user-test profile；完整 LTP 不作为第一阶段必须 gate，除非 transaction owner 明确要求。

退出条件：

1. 第一阶段 kthread-core implementation gates closed。
2. public RFC / transaction devlog / tracking issues 对当前实现状态一致。
3. 后续 workqueue、closure API、Linux procfs 展示兼容、freezer/park、CPU hotplug 均保持 follow-up，不污染 core contract。

## 停止边界

必须回到 RFC / tracking issues 的情况：

1. 需要让 kthread 进入 ordinary process group/session 才能推进实现。
2. 需要让 ordinary wait/reap 观察 kthread zombie。
3. 需要让 `kthreadd` 持有 ordinary kthread stop/result state。
4. 需要新增 independent registry / `KThreadId` 作为 active lookup 真相源。
5. 需要把 service/request/workqueue 重新放回 core。
6. 需要让 kthread exit 复用完整 `kernel_exit()`。
7. 需要外部 owner 持有 `Arc<Task>` 才能完成 stop/wake/wait-exited。

可以作为普通 implementation detail 继续推进的情况：

1. direct user signal to kthread 的具体 errno，只要 fail-closed 且在 signal helper 中一致。
2. 文件拆分、helper 命名、非行为诊断 getter 增减。
3. closure builder API 或泛型 owned payload API 作为第一阶段之后的 optional polish。
4. procfs pgrp/session 展示从 `0` 调整到更 Linux-compatible 的 inert value。
