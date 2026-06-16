# 2026-06-16 - KThread Core

**状态：** 活动中
**负责人：** doruche, Codex
**领域：** task / topology / procfs / kthread
**权威计划：** [RFC-20260616-kthread-core](../../rfcs/kthread-core/index.md), [不变量需求](../../rfcs/kthread-core/invariants.md), [迁移实施计划](../../rfcs/kthread-core/implementation.md)
**当前阶段：** 阶段 4 gate 已关闭，准备阶段 5 post-gate user-facing boundary closeout

## 范围

本事务跟踪已接受的 `kthread-core` RFC 实现：

- 固定 `kthreadd` TID/TGID 为 2，并引入 kthread-aware topology；
- 引入 strong `KThreadHandle` / `KThreadControl` 生命周期能力；
- 落地专用 kthread exit、task-local closeout 与 topology/procfs unpublish；
- 对 wait、job-control、signal、priority、resource 等用户可见 API 做 fail-closed 分流；
- 移除 legacy service 与 park/unpark core surface。

第一阶段不包含 workqueue、freezer、CPU hotplug、runtime bind、closure builder API、完整 Linux procfs pgrp/session 展示兼容或独立 kthread registry。

## 不变量

- `TaskSchedState` 仍是 runnable / waiting / zombie 的唯一真相源。
- active kthread 身份属于 topology `Tid/Tgid`；生命周期属于 strong `KThreadControl` / `KThreadHandle`。
- `kthreadd` 只拥有 create transaction、fixed TID anchor 和 ordinary kthread 的 parent display anchor。
- `wake()` 只是唤醒能力；consumer business predicate 仍属于 consumer。
- ordinary kthread `TaskBinding::KThread`、专用 exit、topology/procfs unpublish 和最小 user-facing API 分流必须在同一个语义 gate 闭合。
- worker 未经批准不得越过分配的 write set；需要扩展时必须先上报 write-set expansion request 并等待批准。

## 阶段日志

### 2026-06-16 - 阶段 0 前置检查启动

**阶段：** 阶段 0 - 工作流与基线前置检查。

**变更：** 在代码实现前建立事务日志。本步骤未启动 worker，也未修改内核代码。

**当前代码落点：**

- Git 基线：分支 `dev/drc/kthread`，相对 `origin/dev/kako/kthread` ahead 3 commits；前置检查开始时工作树干净。
- `task::kthread` 当前仍是历史形状：`anemone-kernel/src/task/kthread/{mod.rs,create.rs,service.rs}`。
- 公开 kthread surface 仍包含 `KThreadBuilder`、weak `KThreadRef`、`KThread`、`KThreadContext`、stop/park/wake/exited helper 和 `KThreadService`。
- `KThreadService` 与 service pending 后端仍存在于 `task/kthread/service.rs`。
- park/unpark 状态与 API 仍存在：`start_parked`、`should_park`、`parkme`、`park`、`unpark`、`Parking` 和 `Parked`。
- `Tid::KTHREADD` 尚不存在。普通 allocator 从 1 开始，因此 TID 2 还没有从 AP kinit 或其它普通分配路径中隔离出来。
- `kthreadd` 由 `Task::new_kernel()` 创建，并以 `TaskBinding::Leader` 发布，继承 init 的 PGID/SID。
- ordinary kthread 同样以 `TaskBinding::Leader` 发布，继承 `kthreadd` 的 PGID/SID，并进入普通 parent/children 与 process-group/session topology。
- ordinary kthread 的 task-local `Arc<KThread>` 在 topology publish 之后安装，仍存在 RFC 指出的 publish-before-attachment 窗口。
- `kthread_entry_shim` 恢复 typed `KThreadStart<A>`，调用 `finish_returned_entry()`，随后进入完整 `kernel_exit()`。
- `kernel_exit()` 目前只断言 kthread 已经过历史 finish path，随后仍执行 user-process cleanup。
- inode shrinker 与 OOM killer 已是 explicit loop，但仍持有 weak `KThreadRef`，并检查 park/unpark 状态。
- initcall 当前只有 `Fs`、`Driver`、`Probe`；inode shrinker 与 OOM killer 由 `bsp_kinit()` 在所有 CPU 完成本地初始化后手动启动。
- procfs root 已按 thread group 枚举；无 userspace 的任务读取 `/proc/<pid>/cmdline` 返回空；`/proc/<pid>/status` 已根据 `TaskFlags::KERNEL` 输出 `Kthread:`。但 `status` 和 `stat` 仍直接读取 user-process `parent_tgid()`、`pgid()` 和 `sid()`。
- `getpgid`、`getsid`、`setpgid`、`setsid`、wait、signal、priority 和 resource-style 路径仍假设 ordinary user-process topology，或需要在阶段 4 启用 ordinary `TaskBinding::KThread` 前补齐显式 kthread target 分类。

**第一轮 write set：**

第一位 implementation worker 只能处理阶段 1：

- `anemone-kernel/src/task/kthread/{mod.rs,create.rs,service.rs}`
- `anemone-kernel/src/fs/inode_shrinker.rs`
- `anemone-kernel/src/mm/oom.rs`

如果 worker 采用 RFC 目标拆分 `task/kthread/{mod.rs,spawn.rs,kthreadd.rs,entry.rs,control.rs,handle.rs,ctx.rs}`，拆分必须保持在同一 kthread owner boundary 内；阶段 1 不得改变 topology、TID allocation、exit 语义、procfs 或 user-facing API 行为。

**准备启动的 agent 列表：**

- `phase1-kthread-surface-worker`：阶段 1 代码 worker。只移除 service 与 park/unpark core surface，把生命周期收窄到 stop/wake/exited，并把 inode shrinker / OOM loop 改为 explicit stop + business predicate。
- `phase1-reviewer`：阶段 1 worker 返回后的只读 reviewer。检查历史 RFC 文本之外没有 service/park residual、没有 consumer 直接调用 `Task::new_kernel()`，并确认 wake 仍只是唤醒能力。
- `phase2-tid-topology-explorer`：阶段 1 关闭后为下一 gate 做准备的只读 explorer。只盘点 TID allocation、`Task::new_kernel()` 调用点、publish guard、`TaskBinding` 和 topology accessor 调用点，不编辑文件。

本条记录不批准也不启动任何阶段 2+ implementation worker。

**停止条件：**

- worker 需要编辑已分配 write set 之外的文件。
- 某阶段需要重新引入 service/request/workqueue、park/unpark、独立 registry 或外部 `Arc<Task>` 生命周期 handle。
- 某阶段需要让 ordinary kthread 加入 process group/session 或 ordinary wait/reap topology。
- ordinary kthread 会在专用 exit、topology/procfs unpublish 和最小 user-facing API 分流同 gate 闭合前发布为 `TaskBinding::KThread`。
- kthread exit 会复用完整 `kernel_exit()`，或在缺少 RFC 要求的 assert/comment 边界时跳过 task-local resource closeout。
- direct signal/job-control/wait/priority/resource 路径在阶段 4 启用前无法完成分类。

**验证：** 前置检查只执行源码读取、RFC 读取、register 读取和 git 状态检查。事务链接 patch 之后需要运行 `git diff --check`、新增文件 whitespace 检查和可选 `mdbook build docs`。

**下一步：** 等待总控批准后再启动阶段 1 worker。

### 2026-06-16 - 阶段 0 文档验证

**阶段：** 阶段 0 事务链接验证。

**变更：** 修正初始链接 patch 后的事务导航，确保新增条目仍是 `docs/src/SUMMARY.md` 事务列表下的同级项。未修改内核代码。

**验证：** `git diff --check` 无输出并成功退出。`git diff --no-index --check -- /dev/null docs/src/devlog/transactions/2026-06-16-kthread-core.md` 未产生 whitespace 诊断；该命令的非零退出码是新增文件与 `/dev/null` 比较时的正常 no-index difference 状态。`mdbook build docs` 成功，并写入 `docs/book`。

**下一步：** 等待总控批准后再启动阶段 1 worker。

### 2026-06-16 - 阶段 1 kthread surface 收窄

**阶段：** 阶段 1 - 收窄 kthread core surface。

**执行：** 启动 `phase1-kthread-surface-worker`，只分配阶段 1 write set。worker 返回后，主控本地复核 diff、重新运行 gate 验证，并启动只读 `phase1-reviewer`。未启动阶段 2+ worker。

**实际 write set：**

- `anemone-kernel/src/task/kthread/mod.rs`
- `anemone-kernel/src/task/kthread/create.rs`
- `anemone-kernel/src/task/kthread/service.rs`（删除）
- `anemone-kernel/src/fs/inode_shrinker.rs`
- `anemone-kernel/src/mm/oom.rs`
- 本事务日志、`kthread-core` RFC 状态、双周 devlog，以及受代码语义变化影响的 inode-shrinker / OOM killer RFC 文档同步。

**变更：**

- 删除 `KThreadService` 源文件与 module wiring。
- 删除 kthread core 的 park/unpark surface：`start_parked`、`park()`、`unpark()`、`should_park()`、`parkme()`、`Parking`、`Parked` 和 park-specific wait / transition。
- 将当前 `KThreadControl` 临时收窄为 `Running`、`StopRequested`、`Exited`；strong `KThreadHandle`、AnyOpaque create transaction 和 dedicated exit 仍留给后续阶段。
- `wait_until_woken()` 只表达 wake capability；entry 仍必须重查 stop 与 consumer 自己的 business predicate。
- inode shrinker 与 OOM killer loop 不再检查 park，只检查 `should_stop()` 和自身压力 / victim predicate。

**边界确认：**

- 没有修改 TID allocator，没有新增 `Tid::KTHREADD`。
- 没有修改 topology binding、`TaskBinding`、procfs、wait、signal、job-control、priority、resource-style API 或 exit path。
- ordinary kthread 仍是阶段 4 前的迁移中间态：`TaskBinding::Leader`、weak `KThreadRef`、typed start pointer 和完整 `kernel_exit()` 均未在本阶段纠偏。
- consumer 没有直接持有 `Arc<Task>`，也没有直接调用 `Task::new_kernel()`。
- worker 未提出 write-set expansion request。

**验证：**

- `git diff --check`：通过。
- `just build`：通过；仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning，本阶段未处理。
- scoped source audit：`rg "KThreadService|service.rs|should_park|parkme|start_parked|Parking|Parked|unpark" anemone-kernel/src/task/kthread anemone-kernel/src/fs/inode_shrinker.rs anemone-kernel/src/mm/oom.rs` 无输出。
- 全仓 source audit：命中历史 `RFC-20260614-kthread`、`kthread-core` rejected / implementation 文本、历史 devlog，以及 scheduler wait-core 的 `ParkState::Parked`；这些不属于 kthread core surface。inode-shrinker / OOM killer 当前 RFC 中的 legacy stop/park 描述已随本阶段同步为 stop-only。
- `phase1-reviewer` 结论：未发现 Apollyon、Keter、Euclid 或 Safe 代码问题；没有阻塞 Phase 1 gate 的问题。

**未运行：** QEMU、boot smoke、LTP / user-test。阶段 1 gate 只要求 build 与 source audit；runtime smoke 留给 topology / exit / procfs 相关阶段。

**残余风险：**

- strong `KThreadHandle`、AnyOpaque create transaction、专用 `kthread_exit()`、fixed `kthreadd` TID、kthread-aware topology、procfs unpublish 和 user-facing API fail-closed 分流仍未实现。
- 当前 external handle 仍是 weak `KThreadRef`；consumer 持有 weak handle 是阶段 3 前的中间态。
- ordinary kthread 仍调用完整 `kernel_exit()`，不得在阶段 4 gate 前切到 `TaskBinding::KThread`。

**结论：** 阶段 1 gate 已关闭。下一步只能准备阶段 2 fixed `kthreadd` TID 与 topology preflight；不得直接启动阶段 3+ 或 ordinary kthread binding enablement。

### 2026-06-16 - 阶段 2 write set 扩展批准

**阶段：** 阶段 2 - fixed `kthreadd` TID 与 topology preflight。

**执行：** 启动只读 `phase2-tid-topology-explorer`，盘点 TID allocator、`Task::new_kernel()` 调用点、publish guard、`TaskBinding`、topology accessor 和当前 kthread 文件落点。explorer 未修改文件。

**发现：**

- 当前代码尚未拆分为 RFC 目标形态；`init_kthreadd()` 与 ordinary kthread create path 仍位于 `anemone-kernel/src/task/kthread/create.rs`，仓库中没有 `task/kthread/kthreadd.rs`。
- Phase 2 必须触碰 `init_kthreadd()` 才能固定 `kthreadd` TID/TGID；若严格按目标文件名只允许 `kthreadd.rs`，worker 会无法实现该 gate。
- root/init 当前也通过普通 allocator 间接取得 TID 1；普通 allocator 改为从 3 开始后，需要一个 fixed init TID 路径或内部 task constructor 辅助。
- ordinary kthread create path 仍依赖 `kthreadd_tg.pgid()` / `sid()` 与 `TaskBinding::Leader` 兼容形态。Phase 2 不得把 ordinary kthread 切到 `TaskBinding::KThread`，也不得让 `kthreadd` 的实际 KThread publish 迫使 ordinary create path 越过 Phase 4 gate。

**批准的 Phase 2 implementation write set：**

- `anemone-kernel/src/task/tid.rs`
- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/task/topology/{mod.rs,thread_group.rs,parent_child.rs,process_group.rs}`
- `anemone-kernel/src/task/kthread/create.rs`
- `anemone-kernel/src/arch/{riscv64,loongarch64}/bootstrap.rs`
- `anemone-kernel/src/task/kthread/mod.rs`（仅限必要 re-export 或同 owner wiring）

本批准把 `task/kthread/create.rs` 作为当前未拆分 checkout 中 `kthreadd.rs` 的实际落点；它不批准 worker 修改 exit、procfs、wait、signal、job-control、priority、resource-style API，亦不批准 ordinary kthread binding enablement。

**Phase 2 worker 交付边界：**

- 增加 `Tid::KTHREADD == 2`。
- ordinary `alloc_tid()` 不返回 0、1 或 2。
- root/init 与 `kthreadd` 使用专用 fixed handle，不暴露通用 `reserve_tid(Tid)`。
- 落地 `ThreadGroupType`、`TaskBinding` rename / scaffolding、`ty()`、User-only accessor panic 和 shape assertions。
- ordinary kthread create path 仍不得切到 `TaskBinding::KThread`；若实现发现必须提前切换，必须停止并上报。
- 阶段 2 退出声明不得声称 ordinary kthread 已满足最终 topology / exit / procfs contract。

**补充批准：** 用户随后批准 Phase 2 worker 修改 clone 路径中与线程组类型直接相关的代码，因为强行在既有 write set 内兼容会固化不自然的 owner boundary。追加 write set：

- `anemone-kernel/src/task/api/clone/{mod.rs,clone.rs,clone3.rs}`

该批准只覆盖 Phase 2 topology type / binding rename / shape assertion 所需的 clone 调用点调整；不批准 clone 路径顺带修改用户可见 clone ABI、exit、procfs、wait、signal、job-control、priority 或 resource policy。

### 2026-06-16 - 阶段 2 worker 集成与暂停点

**阶段：** 阶段 2 - fixed `kthreadd` TID 与 topology preflight。

**执行：** 启动 `phase2-tid-topology-worker`，按批准 write set 落地 Phase 2 代码。worker 返回后，总控复核 diff、关闭 worker，并做一个局部修正：普通 TID allocator 从 3 开始后，容量从 `MAX_PROCESSES` 调整为 `MAX_PROCESSES - 3 + 1`，避免 `/proc/sys/kernel/pid_max` 仍输出 `MAX_PROCESSES` 时普通分配上界越过 pid_max。

**实际 write set：**

- `anemone-kernel/src/task/tid.rs`
- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/task/topology/mod.rs`
- `anemone-kernel/src/task/topology/thread_group.rs`
- `anemone-kernel/src/task/topology/parent_child.rs`
- `anemone-kernel/src/task/topology/process_group.rs`
- `anemone-kernel/src/task/kthread/create.rs`
- `anemone-kernel/src/task/api/clone/mod.rs`
- 本事务日志

未修改 arch bootstrap、exit、procfs、wait、signal、job-control、priority 或 resource-style API。

**变更：**

- 增加 `Tid::KTHREADD == 2`。
- 普通 `alloc_tid()` 范围改为从 3 到 `MAX_PROCESSES`；TID 0、1、2 均不进入普通 allocator。
- TID 1 由内部 one-shot init handle 消费，TID 2 由 `alloc_kthreadd_tid()` 专用 one-shot handle 消费；未新增通用 `reserve_tid(Tid)`。
- `init_kthreadd()` 改用 fixed TID handle 创建 TID/TGID 2，并在发布前后 assert `tid == tgid == Tid::KTHREADD`。
- `TaskBinding::Leader` 重命名为 `TaskBinding::UserLeader`，新增 guarded `TaskBinding::KThread` scaffolding。
- 引入 `ThreadGroupType::{User, KThread}` 与 `ThreadGroup::ty()`。
- `ThreadGroupInner.pgid/sid` 改为 `Option<Tid>`；User-only `pgid()`、`sid()`、`parent_tgid()` 和 parent/child/process-group 操作在非 `User` 上 panic。
- `User` / `KThread` thread-group shape assertion 已覆盖 pgid/sid、children 和 singleton member；`Member` 只能加入 `User` thread group。
- clone 路径仅同步 `TaskBinding::UserLeader` 命名，没有改变 clone ABI 或用户可见策略。

**暂停原因：**

当前代码没有把 `kthreadd` 实际发布为 `TaskBinding::KThread`。原因不只是 Phase 3 的 task-local prepublish attachment 尚未落地：只要 `kthreadd` 先成为 `ThreadGroupType::KThread`，legacy ordinary kthread create path 仍以 `TaskBinding::UserLeader` 发布时就失去合法的 `parent_tgid` / `pgid` / `sid` 来源，并会迫使 ordinary kthread binding 提前进入 Phase 4 gate。按照 RFC，ordinary kthread `TaskBinding::KThread`、专用 exit、topology/procfs unpublish 和最小 user-facing API 分流必须同 gate 闭合，因此总控不能自行把这一步拍板为 Phase 2 内完成。

**当前 gate 结论：**

- fixed TID 与 topology scaffolding 已落地。
- ordinary kthread create path 未越过 Phase 2 boundary，仍是 `TaskBinding::UserLeader`。
- `TaskBinding::KThread` 目前是 fail-fast scaffolding；source audit 确认 `task/kthread` 还没有实际消费该 binding。
- `phase2-reviewer` 结论为 Keter：当前 diff 可作为 fixed TID + topology scaffolding checkpoint，但不能声明 Phase 2 full gate 完整关闭，因为 `kthreadd` 仍发布为 `TaskBinding::UserLeader`，不是 `ThreadGroupType::KThread`。
- 负责人已接受将 “`kthreadd` 实际 `ThreadGroupType::KThread` publish” 从 Phase 2 出口改为 Phase 4 同 gate；Phase 2 以 fixed TID anchor + topology scaffolding checkpoint 关闭。

**已运行验证：**

- `just fmt kernel`：通过。
- `git diff --check`：通过。
- `just build`：通过；仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning，本阶段未处理。
- source audit：`TaskBinding::KThread` 未被 `task/kthread` 消费；ordinary kthread create path 仍未切到 `TaskBinding::KThread`。
- `phase2-reviewer` 只读审查：未发现越界 write set 或 ordinary kthread 提前切换；确认 TID allocator 范围与 `/proc/sys/kernel/pid_max` 语义一致；确认 Keter 暂停项如上。

**未完成验证：**

- focused boot / procfs smoke 尚未运行；按 RFC 该 smoke 与实际 topology / procfs 可见性 gate 相关，当前尚未启用 `KThread` publish。

### 2026-06-16 - 阶段 2 gate 决策修正

**阶段：** 阶段 2 closeout 决策。

**决策：** 负责人接受将 `kthreadd` 的实际 `TaskBinding::KThread` publish 并入阶段 4 同 gate。阶段 2 不再要求 `kthreadd` type publish，只要求 fixed TID anchor、ordinary allocator 隔离、`ThreadGroupType` / `TaskBinding::KThread` scaffolding、User-only accessor panic 与 shape assertion 就绪。

**理由：** 当前 legacy ordinary kthread create path 仍需要从 `kthreadd` 的 ordinary `pgid/sid` 兼容形态发布 `TaskBinding::UserLeader`。若 Phase 2 单独把 `kthreadd` 切到 `KThread`，会迫使 ordinary kthread binding 提前越过 Phase 4 的专用 exit、topology/procfs unpublish 和 user-facing API 分流 gate。

**文档同步：** [RFC index](../../rfcs/kthread-core/index.md)、[不变量需求](../../rfcs/kthread-core/invariants.md) 和 [迁移实施计划](../../rfcs/kthread-core/implementation.md) 已同步：最终不变量不变；迁移顺序改为阶段 4 同 gate 启用 `kthreadd` 与 ordinary kthread 的 `TaskBinding::KThread`。

**结论：** 阶段 2 checkpoint 已关闭。下一步只允许准备阶段 3 strong `KThreadHandle` 与 create transaction；不得启动阶段 4 或任何 user-facing API 分流 worker。

### 2026-06-16 - 阶段 2.1 fixed TID constructor cleanup

**阶段：** 阶段 2 corrective cleanup。

**原因：** 阶段 2 checkpoint 中 `Task::new_kernel()` 通过 `tgid == Tid::INIT` 隐式决定是否消费 fixed init TID。这把 “创建 init leader” 和 “创建 init thread-group member” 混在同一个 generic constructor 内，AP kinit 还依赖 init handle 已被 BSP 消费后的 fallback 普通分配，形状不自然，也会污染阶段 3 create transaction 的 owner boundary。

**变更：**

- `Task::new_kernel()` 恢复为只使用普通 `alloc_tid()`，不再认识 `Tid::INIT` 或其它 fixed identity。
- `alloc_init_tid()` 改为显式 one-shot fixed handle，重复消费直接 panic。
- BSP bootstrap 通过 `Task::new_kernel_with_tid_handle(..., alloc_init_tid())` 显式创建 init leader。
- AP bootstrap 继续使用普通 `Task::new_kernel(..., Some(Tid::INIT), ...)` 加入 init thread group，因此自然获得普通 TID。
- `kthreadd` 继续使用 `alloc_kthreadd_tid()` 与 `new_kernel_with_tid_handle()`；fixed TID ownership 只出现在 fixed identity caller 上。

**边界：** 未改变 Phase 2 / Phase 4 gate 决策；`TaskBinding::KThread` 仍是未启用 scaffolding，`kthreadd` 与 ordinary kthread 实际 `KThread` publish 仍留到阶段 4 同 gate。

### 2026-06-16 - 阶段 3 前置检查与 worker 准备

**阶段：** 阶段 3 - strong `KThreadHandle` 与 create transaction。

**执行：** 总控重新阅读 RFC index、invariants、implementation、tracking issues、当前 register 条目和本事务日志；刷新当前代码落点，准备阶段 3 worker 列表。本步骤未启动 worker，也未修改内核代码。

**当前代码落点：**

- Git 基线：分支 `dev/drc/kthread`，前置检查开始时工作树干净；最新本地提交为 `b01cd3e kthread: clean up fixed tid construction`。
- `task::kthread` 当前仍只有 `mod.rs` 与 `create.rs` 两个实现文件；RFC 目标文件 `spawn.rs`、`kthreadd.rs`、`entry.rs`、`control.rs`、`handle.rs`、`ctx.rs` 尚未建立。
- `KThreadService` 与 park/unpark core surface 已删除；代码中不再有 `start_parked`、`park()`、`unpark()`、`should_park()`、`parkme()`、`Parking` 或 `Parked` kthread 状态。
- public surface 仍是阶段 3 前 legacy 形态：`KThreadBuilder`、generic `KThreadEntry<A>`、weak `KThreadRef`、内部 `KThread` object、`KThreadContext`、`KThreadRunState` 和 `KThreadSnapshot`。
- `Task` 仍保存 `SpinLock<Option<Arc<KThread>>>`；`KThread` 反向 weak 指向 `Task`，`KThreadRef` 只是 weak handle。
- ordinary kthread create path 仍通过 `KThreadStart<A>`、`KThreadStartPointer` 和 `ParameterList::new(&[start_arg])` 传递 typed start payload；entry shim 再恢复 leaked `Box<KThreadStart<A>>`。
- ordinary kthread 的 task-local kthread state 仍在 `TaskBinding::UserLeader` publish 之后安装；阶段 3 必须修正为 publish 前安装 task-local attachment，但不得启用实际 `TaskBinding::KThread` publish。
- `kthreadd` fixed TID/TGID 2 与普通 allocator 从 3 开始已落地；但 `kthreadd` 与 ordinary kthread 仍以 `TaskBinding::UserLeader` 发布，`TaskBinding::KThread` 仍只是未启用 scaffolding。
- `kthread_entry_shim` 在 entry return 后仍调用完整 `kernel_exit()`；这属于阶段 4 gate，阶段 3 不得提前改 exit path。
- inode shrinker 与 OOM killer 已是 explicit loop，但仍持有 `KThreadRef`；`wake_oom_killer()` 仍通过 `upgrade()` 获取弱引用后调用 `wake()`。

**阶段 3 implementation write set：**

- `anemone-kernel/src/task/kthread/mod.rs`
- `anemone-kernel/src/task/kthread/create.rs`（仅作为迁移来源与删除目标，不作为阶段 3 实现落点）
- 阶段 3 开始必须按 RFC 目标形态拆分源码：新增 `anemone-kernel/src/task/kthread/{spawn.rs,kthreadd.rs,entry.rs,control.rs,handle.rs,ctx.rs}`，并迁移当前 `create.rs` 内容；`create.rs` 只作为迁移来源，阶段 3 worker 不能继续把它作为长期实现文件保留。
- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/fs/inode_shrinker.rs`
- `anemone-kernel/src/mm/oom.rs`

**不批准的写集和行为：**

- 不修改 TID allocator、topology publish、`TaskBinding::KThread` 实际消费、procfs、wait、signal、job-control、priority、resource-style API 或 `task/api/exit`。
- 不让 `kthreadd` 或 ordinary kthread 在阶段 3 发布为 `TaskBinding::KThread`。
- 不把 `kthread_entry_shim` 改成专用 `kthread_exit()` 或调整 `kernel_exit()` guard；这些必须留给阶段 4 同 gate。
- 不新增 service/request/workqueue、park/unpark、独立 registry、`KThreadId` 或外部 `Arc<Task>` lifecycle handle。
- 不保留 `create.rs` 作为阶段 3 之后的 kthread create/kthreadd/entry/control 实现聚合文件；若拆分过程中发现必须临时保留兼容 shim，worker 必须停止并上报保留原因、删除条件和验证 gate。

**准备启动的 agent 列表：**

- `phase3-kthread-handle-worker`：阶段 3 代码 worker。只在批准 write set 内实现 strong `KThreadHandle` / `KThreadControl`、`KThreadTaskLocal` + `KThreadLaunch` task-local attachment、`AnyOpaque` entry payload、publish 前 attachment 安装和 create transaction commit 边界；同步更新 inode shrinker / OOM killer 持有 strong handle。若需要拆分 `task/kthread` 文件，必须保持同 owner 内拆分，且不得触碰阶段 4 gate。
- `phase3-kthread-handle-worker` 的源码组织要求：先把现有 `create.rs` 按 RFC 文件组织迁移到 `spawn.rs`、`kthreadd.rs`、`entry.rs`、`control.rs`、`handle.rs`、`ctx.rs`，再在这些文件中落地阶段 3 语义；完成后删除 `create.rs` 和对应 module wiring。
- `phase3-reviewer`：阶段 3 worker 返回后的只读 reviewer。检查 public API 不暴露 `Arc<Task>` / raw scheduler / topology mutation，`request_stop()` 与 already-exited stop 幂等，`wake()` 不表达业务 request，`wait_exited()` 只观察 external exited event，control 不保存 post-exit diagnostic identity，launch slot 只被 entry shim take 一次，publish 后没有可失败 rollback。
- `phase3-source-audit-explorer`：阶段 3 worker 合入后按需启动的只读 explorer。只做 source audit：外部 owner 不再依赖 weak-only `KThreadRef` 等待 exit result；没有内部 `KThread` object 作为 Task/topology/control 之外的第四实体；payload 不再经 `ParameterList` / raw pointer 传递。

本条记录不批准也不启动阶段 4+ implementation worker。阶段 3 首轮只能先启动 `phase3-kthread-handle-worker`；reviewer 与 explorer 必须等 worker 返回或总控需要并行只读审计时再启动。

**阶段 3 停止条件：**

- worker 需要编辑批准 write set 之外的文件。
- worker 认为必须启用 `TaskBinding::KThread`、改 exit path、改 procfs/unpublish 或改 user-facing API 才能完成阶段 3。
- worker 需要重新引入 weak-only lifecycle API 作为 public contract、外部 `Arc<Task>` handle、service/request/workqueue、park/unpark 或独立 registry。
- worker 无法删除 `create.rs`，或需要把阶段 3 新语义继续集中写进 `create.rs`。
- worker 无法在 publish 前安装 task-local kthread attachment，或无法证明 publish 后步骤只剩 infallible enqueue / success completion。
- worker 无法让 `wait_exited()` / `has_exited()` 保持 external exited completion 语义，而只能直接暴露 `phase == Exited(code)`。

**下一步：** 若总控继续推进，只启动 `phase3-kthread-handle-worker` 一名 implementation worker。worker 返回后总控本地复核 diff，再启动 `phase3-reviewer`。

### 2026-06-16 - 阶段 3 strong handle 与 create transaction

**阶段：** 阶段 3 - strong `KThreadHandle` 与 create transaction。

**执行：** 启动 `phase3-kthread-handle-worker`，只分配阶段 3 write set。worker 返回后，总控本地复核 diff，修正 `has_exited()` / `wait_exited()` 外部完成语义，然后启动只读 `phase3-reviewer`。未启动阶段 4+ worker。

**实际 write set：**

- `anemone-kernel/src/task/kthread/mod.rs`
- `anemone-kernel/src/task/kthread/create.rs`（删除）
- `anemone-kernel/src/task/kthread/control.rs`
- `anemone-kernel/src/task/kthread/ctx.rs`
- `anemone-kernel/src/task/kthread/entry.rs`
- `anemone-kernel/src/task/kthread/handle.rs`
- `anemone-kernel/src/task/kthread/kthreadd.rs`
- `anemone-kernel/src/task/kthread/spawn.rs`
- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/fs/inode_shrinker.rs`
- `anemone-kernel/src/mm/oom.rs`
- 本事务日志

**变更：**

- 按 RFC 文件组织拆分 `task::kthread`：`mod.rs` facade、`spawn.rs` builder / placement、`kthreadd.rs` create transaction、`entry.rs` launch slot / entry shim、`control.rs` lifecycle owner、`handle.rs` strong public handle、`ctx.rs` entry ctx。
- 删除 `create.rs`，不再保留 kthread create / kthreadd / entry / control 聚合文件。
- 删除 public weak-only `KThreadRef` 和内部 `KThread` 实体；`Task` task-local kthread 字段改为直接保存 `KThreadTaskLocal`。
- 引入 `KThreadHandle`，public API 只暴露 `request_stop()`、`wake()`、`wait_exited()` 和 `has_exited()`；没有 public `stop()`、`Arc<Task>`、scheduler state 或 topology mutation capability。
- `KThreadControl` 只拥有 lifecycle phase、wake event、external exited event 和 external completion result；`KThreadPhase` 只有 `Running`、`StopRequested`、`Exited(i32)`。
- 总控集成修正：`has_exited()` / `wait_exited()` 观察 `external_result` + `exited` event，不直接把 internal `phase == Exited(code)` 当作 handle-visible completion。阶段 3 仍在 entry result 后立即 publish external completion；阶段 4 必须把该 publish 移到 task-local closeout 与 topology/procfs unpublish 之后。
- Entry API 改为 `KThreadEntry = fn(KThreadCtx, AnyOpaque) -> i32`；kthread core 只搬运 / drop opaque payload，不 downcast。
- ordinary kthread launch payload 改为 task-local `KThreadLaunch`，在 publish 前安装到 `KThreadTaskLocal`；entry shim 从 current task 的 launch slot `take()`，缺失或重复进入直接 panic。
- ordinary kthread task creation 不再通过 `ParameterList` 或 raw pointer 传递 start payload；kernel task entry 使用 `ParameterList::empty()`。
- create transaction 改名为 `SpawnRequest` / `SpawnReply` / `SpawnOutcome`、`kthreadd::submit()` / `run()` / `spawn()`，静态名改为 `KTHREADD` / `SPAWN_QUEUE` / `SPAWN_WAKE`。
- inode shrinker 与 OOM killer 改为持有 strong `KThreadHandle`；`wake_oom_killer()` 直接 clone strong handle 后 `wake()`，不再 weak upgrade。

**边界确认：**

- 未修改 TID allocator。
- 未修改 topology publish 语义；`kthreadd` 与 ordinary kthread 仍以 `TaskBinding::UserLeader` 发布，`TaskBinding::KThread` 仍是阶段 4 前未启用 scaffolding。
- 未修改 `task/api/exit`、procfs、wait、signal、job-control、priority 或 resource-style API。
- `kthread_entry_shim` 在完成 kthread result 后仍调用完整 `kernel_exit()`；专用 `kthread_exit()`、task-local closeout、topology/procfs unpublish 和 user-facing API 分流仍留给阶段 4 同 gate。
- 未重新引入 service/request/workqueue、park/unpark、独立 registry、`KThreadId` 或外部 `Arc<Task>` lifecycle handle。

**Review：** `phase3-reviewer` 只读审查未发现 Apollyon 或 Keter，结论为 Phase 3 gate can continue。reviewer 记录的 residual notes 是阶段 4 边界：external completion 当前仍紧随 entry result；`kthreadd` 仍发布为 `TaskBinding::UserLeader`，且尚未安装 `KThreadTaskLocal { launch: None }`，这与阶段 3 stop boundary 一致。

**验证：**

- `just fmt kernel`：通过。
- `git diff --check`：通过。
- 新增 `task/kthread/{control.rs,ctx.rs,entry.rs,handle.rs,kthreadd.rs,spawn.rs}` 对 `/dev/null` 的 `git diff --no-index --check` 均无 whitespace 诊断；非零退出码为新文件差异的正常状态。
- `just build`：通过；仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。
- source audit：`rg "KThreadRef|struct KThread\\b|ParameterList::new\\(\\&\\[start_arg|KThreadStart|KThreadStartPointer" anemone-kernel/src/task/kthread anemone-kernel/src/fs/inode_shrinker.rs anemone-kernel/src/mm/oom.rs` 无输出。
- source audit：`rg "mod create|create::|task/kthread/create|KThreadContext|KThreadSnapshot|KThreadRunState|\\.stop\\(\\)" anemone-kernel/src/task/kthread anemone-kernel/src/fs/inode_shrinker.rs anemone-kernel/src/mm/oom.rs anemone-kernel/src/task/mod.rs` 无输出。
- source audit：`test ! -e anemone-kernel/src/task/kthread/create.rs` 通过。
- phase 4 boundary audit：`TaskBinding::KThread` 只在 RFC/事务文本、topology scaffolding 和 kthread 注释中出现；`task/kthread` 未实际消费该 binding。

**未运行：** QEMU、boot smoke、LTP / user-test。阶段 3 gate 要求 build 与 source audit；procfs/exit/user-facing runtime smoke 留给阶段 4+。

**残余风险：**

- `kthread_entry_shim` 仍调用完整 `kernel_exit()`；阶段 4 必须落地专用 `kthread_exit()`、task-local closeout、topology/procfs unpublish 和最小 user-facing API 分流。
- `kthreadd` 与 ordinary kthread 仍是 `TaskBinding::UserLeader` 迁移中间态；实际 `TaskBinding::KThread` publish 留阶段 4 同 gate。
- `KThreadTaskLocal { launch: None }` 尚未安装到 `kthreadd`；阶段 4 启用 `kthreadd` `TaskBinding::KThread` 前必须补齐。

**结论：** 阶段 3 gate 已关闭。下一步只能准备阶段 4 `kthreadd` / ordinary kthread topology、专用 exit、topology/procfs unpublish 与最小 user-facing API 分流同 gate；不得把其中任一部分拆成可独立发布的中间态。

### 2026-06-17 - 阶段 4 前置检查与 worker 准备

**阶段：** 阶段 4 - `kthreadd` / ordinary kthread topology / exit / user-facing API gate。

**执行：** 总控重新阅读 `kthread-core` RFC index、invariants、implementation、tracking issues、当前 register 和本事务日志；刷新当前代码落点，准备阶段 4 worker 列表。本步骤未启动 worker，也未修改内核代码。

**当前代码落点：**

- Git 基线：分支 `dev/drc/kthread`，前置检查开始时工作树干净；最新本地提交为 `50c2e7a kthread: close phase3 handle gate`。
- 阶段 3 已关闭：`task::kthread` 已拆为 `mod.rs`、`spawn.rs`、`kthreadd.rs`、`entry.rs`、`control.rs`、`handle.rs`、`ctx.rs`，`create.rs` 已删除。
- `KThreadService`、park/unpark、weak-only `KThreadRef`、内部 `KThread` 实体、typed start pointer 和 `ParameterList` start payload 均已从 kthread core 消失。
- `KThreadHandle` 是 strong lifecycle capability；inode shrinker 与 OOM killer 已持有 strong handle。
- `kthreadd` fixed TID/TGID 2 与普通 allocator 从 3 开始已落地。
- `TaskBinding::KThread` 与 `ThreadGroupType::KThread` 仍只是 topology scaffolding；`kthreadd` 与 ordinary kthread 仍实际发布为 `TaskBinding::UserLeader`。
- `kthreadd` 尚未安装 `KThreadTaskLocal { launch: None }`；ordinary kthread 已在 publish 前安装 `KThreadTaskLocal { launch: Some(...) }`。
- `kthread_entry_shim` 仍在完成 kthread result 后调用完整 `kernel_exit()`；`kernel_exit()` 仍执行 user-process cleanup、fd closeout、ordinary topology detach、child-exited、reparent、`SIGCHLD`、wait/reap 和 scheduler zombie tail。
- procfs `status` / `stat` 仍直接读取 user-process `parent_tgid()`、`pgid()`、`sid()`；阶段 4 需要分离 procfs display helper 与 user-process accessor。
- `getpgid()`、`getsid()`、`setpgid()`、`setsid()`、wait、signal permission / kill paths、priority 和 resource-style user API 仍需要在启用 `TaskBinding::KThread` 前完成 source inventory 与最小 fail-closed 分流。

**阶段 4 首轮 write set：**

第一位 worker 只能做 source inventory，不编辑文件：

- `anemone-kernel/src/task/api/exit/mod.rs`
- `anemone-kernel/src/task/kthread/{mod.rs,entry.rs,control.rs,kthreadd.rs}`
- `anemone-kernel/src/task/topology/{mod.rs,thread_group.rs,parent_child.rs}`
- `anemone-kernel/src/fs/proc/tgid/binding.rs`
- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/fs/proc/root/file.rs`
- `anemone-kernel/src/fs/proc/tgid/{cmdline.rs,mod.rs,stat.rs,status.rs}`
- `anemone-kernel/src/task/api/wait/*`
- `anemone-kernel/src/task/api/jobctl/{getpgid.rs,getsid.rs,setpgid.rs,setsid.rs}`
- `anemone-kernel/src/task/sig/api/{mod.rs,kill.rs,tkill.rs,tgkill.rs,rt_sigqueueinfo.rs}`
- `anemone-kernel/src/task/api/priority.rs`
- source inventory 发现的 resource / scheduler user API files

**准备启动的 agent 列表：**

- `phase4-source-inventory-explorer`：只读 explorer。盘点 `pgid()` / `sid()` / `parent_tgid()` 调用点、procfs binding lazy creation / unbind 路径、wait/job-control/signal/priority/resource/scheduler user-facing target resolver，并输出最小实现切片、需要的实际 write set 和必须同 gate 闭合的调用点清单。不编辑文件。
- `phase4-exit-topology-procfs-worker`：阶段 4 首个 implementation worker，必须等待 source inventory 和总控批准后才能启动。负责 `kthreadd` / ordinary kthread `TaskBinding::KThread` publish、专用 `kthread_exit()`、task-local closeout、topology/procfs unpublish 和 procfs display helper 的同 gate 实现。
- `phase4-user-api-worker`：阶段 4 第二个 implementation worker，必须等待 source inventory 和总控批准后才能启动。负责 wait、job-control、signal、priority/resource/scheduler user-facing API 的最小 kthread fail-closed 分流。该 worker 的 write set 必须与 exit/topology/procfs worker 协调，不能抢先启用 `TaskBinding::KThread`。
- `phase4-reviewer`：阶段 4 implementation diff 返回后的只读 reviewer。检查 kthread exit 未执行 user-process cleanup，task-local closeout 边界与注释符合 RFC，procfs lookup 不会重建已 unpublish kthread binding，strong handle 的 external completion 晚于 closeout/unpublish，user-facing API 不会把 kthread 当 ordinary process 管理。

本条记录不批准也不启动任何阶段 4 implementation worker。下一步最多先启动 `phase4-source-inventory-explorer` 一名只读 explorer；implementation worker 必须等 source inventory 返回、总控确认 write set 后再启动。

**阶段 4 停止条件：**

- source inventory 发现阶段 4 最小 gate 需要编辑上述 write set 之外的文件，且该扩展尚未被批准。
- 任何 worker 试图把 `kthreadd` 或 ordinary kthread 提前发布为 `TaskBinding::KThread`，但专用 exit、topology/procfs unpublish 和最小 user-facing API 分流不能同 gate 闭合。
- worker 需要让 kthread 进入 ordinary process group/session、ordinary parent children list 或 ordinary wait/reap topology。
- worker 需要让 `kthread_exit()` 复用完整 `kernel_exit()`，或跳过 task-local resource closeout 且没有 RFC 要求的 assert/comment 边界。
- worker 需要让 user-facing API 通过 procfs inert display helper 推导 job-control、signal permission、waitability、priority/resource policy 或 lifecycle truth。
- worker 需要重新引入 service/request/workqueue、park/unpark、独立 registry、`KThreadId` 或外部 `Arc<Task>` lifecycle handle。

**验证计划：**

- source inventory 后先记录 `pgid()` / `sid()` / `parent_tgid()` 调用点分类和实际 write set。
- implementation diff 后至少运行 `git diff --check`、`just build`、`rg "kernel_exit\\(" anemone-kernel/src/task/kthread anemone-kernel/src/task/api/exit` source audit、procfs binding lazy rebuild source audit、wait/job-control/signal/priority/resource/scheduler user path source audit。
- 若代码 gate 合入，追加 focused smoke：entry return 后 `wait_exited()` 得到 result；退出后 `/proc/<pid>` 不可见；ordinary user process exit 不退化；`/proc/2/status`、ordinary kthread `status` / `stat` / `cmdline`；job-control 与 signal kthread target 不触发 User-only accessor panic。

**下一步：** 若总控继续推进，只启动 `phase4-source-inventory-explorer` 一名只读 explorer。explorer 返回后，总控更新事务日志并决定是否申请 write set 扩展或启动第一名 implementation worker。

### 2026-06-17 - 阶段 4 执行决策修正

**阶段：** 阶段 4 - `kthreadd` / ordinary kthread topology / exit / user-facing API gate。

**决策：** 用户明确指示不启动 explorer，直接开始实现。总控据此跳过只读 source inventory worker，不再等待 `phase4-source-inventory-explorer`。

**执行边界：**

- 首个 implementation worker 必须覆盖阶段 4 的完整 semantic gate：`kthreadd` 与 ordinary kthread `TaskBinding::KThread` publish、专用 `kthread_exit()`、task-local closeout、topology/procfs unpublish 和最小 user-facing API 分流。
- 不允许把 `TaskBinding::KThread` enablement 拆成独立中间态；如果 worker 无法在同一个 review gate 内闭合 exit / procfs / user-facing API，必须停止并上报。
- worker 初始 write set 仍按 [迁移实施计划](../../rfcs/kthread-core/implementation.md) 阶段 4 执行；若发现 priority、resource 或 scheduler user API 需要额外文件，必须提交 write-set expansion request，说明理由、文件、contract/gate 影响和建议验证。
- reviewer 只在 implementation worker 返回后启动；本轮不一次性启动所有 worker。

### 2026-06-17 - 阶段 4 implementation worker 启动

**阶段：** 阶段 4 - `kthreadd` / ordinary kthread topology / exit / user-facing API gate。

**执行：** 启动 `phase4-gate-worker`（Faraday）作为本阶段首个且当前唯一 implementation worker。总控不在 worker 的实现 write set 上并行编辑。

**分配任务：** worker 必须同 gate 实现 `kthreadd` 与 ordinary kthread `TaskBinding::KThread` publish、专用 `kthread_exit()`、task-local closeout、topology/procfs unpublish、external handle completion 延后，以及 wait/job-control/signal/priority/resource/scheduler user-facing API 的最小 kthread 分流。

**分配 write set：**

- `anemone-kernel/src/task/api/exit/mod.rs`
- `anemone-kernel/src/task/kthread/{mod.rs,entry.rs,control.rs,kthreadd.rs}`
- `anemone-kernel/src/task/topology/{mod.rs,thread_group.rs,parent_child.rs}`
- `anemone-kernel/src/fs/proc/tgid/binding.rs`
- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/fs/proc/root/file.rs`
- `anemone-kernel/src/fs/proc/tgid/{cmdline.rs,mod.rs,stat.rs,status.rs}`
- `anemone-kernel/src/task/api/wait/*`
- `anemone-kernel/src/task/api/jobctl/{getpgid.rs,getsid.rs,setpgid.rs}`
- `anemone-kernel/src/task/sig/api/{mod.rs,kill.rs,tkill.rs,tgkill.rs,rt_sigqueueinfo.rs}`
- `anemone-kernel/src/task/api/priority.rs`

**停止条件：** 若 worker 需要编辑上述 write set 之外的 resource / scheduler user API 文件，或无法同 gate 闭合 `TaskBinding::KThread`、专用 exit、unpublish 与 user-facing 分流，必须停止并上报 write-set expansion request；总控不得代为静默扩展。

### 2026-06-17 - 阶段 4 write-set expansion request

**阶段：** 阶段 4 - resource user API 分流。

**执行：** `phase4-gate-worker`（Faraday）按停止条件暂停，未修改代码。

**请求扩展：**

- `anemone-kernel/src/task/resource/api/prlimit64.rs`

**理由：** `prlimit64(pid, ...)` 是 resource-style user API。当前代码解析 target pid，但实际只使用 current task 的 uspace / limits；它没有解析目标 thread group，也无法在 direct target 命中 kthread 时 fail closed。

**contract / gate 影响：** 阶段 4 要求 resource-style user API 直指 kthread 时 fail closed，或明确登记为只读 inert policy。若不纳入 `prlimit64.rs`，启用 `TaskBinding::KThread` 后 resource user API 分流无法完整闭合。

**建议验证：**

- `git diff --check`
- `just build`
- source audit：`rg -n "prlimit64|ThreadGroupType::KThread|PrLimitTarget" anemone-kernel/src/task`

### 2026-06-17 - 阶段 4 write-set expansion 批准

**阶段：** 阶段 4 - resource user API 分流。

**决策：** 用户批准将 `anemone-kernel/src/task/resource/api/prlimit64.rs` 纳入阶段 4 write set。

**批准边界：**

- 仅限 `prlimit64(pid, ...)` 的 kthread target policy 与阶段 4 resource-style user API 分流。
- 不批准顺带扩展完整 resource-limit 语义、rlimit 写入支持、全局 resource accounting 或其它 resource API 重构。
- 阶段 4 worker 继续必须保持同 gate 闭合 `TaskBinding::KThread`、专用 exit、topology/procfs unpublish 和最小 user-facing API 分流。

### 2026-06-17 - 阶段 4 procfs root lookup expansion request

**阶段：** 阶段 4 - procfs binding invalidation / lazy rebuild boundary。

**执行：** `phase4-gate-worker`（Faraday）按停止条件再次暂停。worker 已做部分 topology/procfs scaffolding，但尚未启用 `TaskBinding::KThread`，因此没有形成 independently published intermediate state。

**请求扩展：**

- `anemone-kernel/src/fs/proc/root/inode.rs`

**理由：** `/proc/<tgid>` lazy lookup 的实际 binding 创建点在 `proc_root_lookup()`。当前路径先进入 `binding_tx(...)`，再调用 `get_thread_group(&tgid)`；而阶段 4 要求 kthread unpublish 由 topology owner 驱动，并按 topology -> procfs binding 的顺序 invalidation。若 root lookup 继续按 binding lock -> topology lookup 的顺序创建 binding，就无法证明 unpublish 与 lazy rebuild 之间没有锁序反转或重建窗口。

**contract / gate 影响：** 阶段 4 要求 procfs binding invalidation 与 topology unpublish 同协议闭合，且 unpublish 开始后 lookup / readdir 不得重建 kthread binding。若不纳入 `fs/proc/root/inode.rs`，只能 invalidation 已有 binding，不能完整约束 lazy lookup 创建路径。

**拟定范围：** 仅调整 `/proc/<tgid>` lazy binding creation / revalidation，使其遵守 kthread unpublish ordering 和 no-rebuild 语义；不扩展 procfs 功能，不改静态 PDE 语义。

**建议验证：**

- `git diff --check`
- `just build`
- source audit：`rg -n "binding_tx|get_thread_group|invalidate_thread_group_binding|proc_root_lookup" anemone-kernel/src/fs/proc anemone-kernel/src/task/topology`

### 2026-06-17 - 阶段 4 procfs root lookup expansion 批准

**阶段：** 阶段 4 - procfs binding invalidation / lazy rebuild boundary。

**决策：** 用户批准将 `anemone-kernel/src/fs/proc/root/inode.rs` 纳入阶段 4 write set。

**批准边界：**

- 仅限 `/proc/<tgid>` lazy binding creation / revalidation 的锁序和 no-rebuild 语义。
- 不批准顺带扩展 procfs 静态 PDE、目录功能、inode cache 策略或其它 procfs 行为。
- 阶段 4 worker 继续必须保持 topology-owned unpublish、procfs narrow invalidation hook、专用 kthread exit 和最小 user-facing API 分流同 gate 闭合。

### 2026-06-17 - 阶段 4 syscall current-task policy

**阶段：** 阶段 4 - user-facing API 分流。

**决策：** 用户指出 syscall 当前执行者不可能是 kthread，因为 kthread 不会触发 syscall。阶段 4 实现不得新增 “current task 是否 kthread” 的虚假防御性检查。

**执行边界：**

- syscall `current` 路径按 user task by construction 处理；`pid == 0`、`who == 0`、current process group/session 等 self/current selector 不需要检查 current 是否 kthread。
- 只在 direct pid/tgid/tid target、process-group/session/uid/broadcast 枚举或 signal permission target 可能命中 kthread 时做 `ThreadGroupType::KThread` 分流。
- 如果 touched code 中已有 current-kthread 防御，只能在与阶段 4 target policy 直接相关时收窄或删除；不得借机清理无关 syscall。

### 2026-06-17 - 阶段 4 worker 返回与总控/用户复核

**阶段：** 阶段 4 - implementation diff review。

**执行：** `phase4-gate-worker`（Faraday）完成阶段 4 implementation diff，并运行验证。总控本地复核 diff；用户随后确认已审过该 diff，结论为“基本对”，并明确不再启动 reviewer。

**实际 write set：**

- `anemone-kernel/src/task/api/exit/mod.rs`
- `anemone-kernel/src/task/kthread/{mod.rs,entry.rs,control.rs,kthreadd.rs}`
- `anemone-kernel/src/task/topology/{mod.rs,parent_child.rs}`
- `anemone-kernel/src/fs/proc/{mod.rs,root/inode.rs,tgid/binding.rs,tgid/stat.rs,tgid/status.rs}`
- `anemone-kernel/src/task/api/wait/mod.rs`
- `anemone-kernel/src/task/api/jobctl/{getpgid.rs,getsid.rs,setpgid.rs}`
- `anemone-kernel/src/task/sig/api/{mod.rs,kill.rs,tkill.rs,tgkill.rs,rt_sigqueueinfo.rs}`
- `anemone-kernel/src/task/api/priority.rs`
- `anemone-kernel/src/task/resource/api/prlimit64.rs`
- 本事务日志

`fs/proc/mod.rs` 仅用于 re-export topology-owned procfs invalidation hook，未扩展 procfs 行为。

**worker 结果：**

- `kthreadd` 和 ordinary kthread 改为 `TaskBinding::KThread` publish；`kthreadd` publish 前安装 `KThreadTaskLocal { launch: None }`。
- `kthread_entry_shim()` 调用专用 `kthread_exit(result)`，不再进入完整 `kernel_exit()`；`kernel_exit()` 对 kthread 直接 panic。
- `kthread_exit()` 顺序为 internal result completion、阶段 4 fd-table 空断言、topology/procfs unpublish、external completion publish、scheduler zombie tail。
- procfs root lazy lookup 改为 topology -> binding lock order；procfs `status` / `stat` 使用 display-only parentage helper。
- wait/job-control/signal/priority/prlimit 最小 kthread fail-closed / skip 分流已落地。

**验证：**

- worker：`just fmt kernel` 通过。
- worker：`git diff --check` 通过。
- worker：`just build` 通过；仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。
- 总控复跑：`git diff --check` 通过。
- 总控复跑：`just build` 通过；同样仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。
- source audit：`rg "kernel_exit\\(" anemone-kernel/src/task/kthread anemone-kernel/src/task/api/exit` 无 kthread entry 调用完整 `kernel_exit()`。
- source audit：kthread core legacy `KThreadService` / park / weak handle / typed start pointer 查询无输出。

**复核结果：** 用户复核后认为 diff 基本正确，不再安排 reviewer。总控 source audit 发现一个 Euclid：`SYS_GET_ROBUST_LIST` 是剩余 direct-TID user resolver，当前可通过指定 kthread TID 观察到有效但空的 robust-list 响应；需要对 `KThread` fail closed，或明确记录为只读 inert policy。

### 2026-06-17 - 阶段 4 futex robust-list expansion request

**阶段：** 阶段 4 - direct-TID user resolver 分流。

**请求扩展：**

- `anemone-kernel/src/task/api/futex/get_robust_list.rs`

**理由：** `get_robust_list(tid, ...)` 是按 task id 解析目标的 user-facing syscall。RFC 不变量要求 direct signal、priority / scheduler user API、pid/tgid resolver 或其它可能命中 kthread 的用户入口按 `ThreadGroupType` 拒绝或跳过 kthread。当前实现对指定 TID 直接 `get_task(&tid)`，未分流 kthread。

**contract / gate 影响：** 若不纳入该文件，阶段 4 user-facing fail-closed source audit 仍有一个 direct-TID resolver 漏口。该问题是 Euclid 而非 blocking Keter，但修复成本低且与阶段 4 gate 直接相邻。

**拟定范围：** 仅在 `SYS_GET_ROBUST_LIST` 目标解析后对 `ThreadGroupType::KThread` 返回 `NoSuchProcess`，不扩展 robust futex 语义，不修改 `set_robust_list` 或 `exit_robust_list`。

**建议验证：**

- `git diff --check`
- `just build`
- source audit：`rg -n "get_robust_list|ThreadGroupType::KThread|robust_list" anemone-kernel/src/task/api/futex anemone-kernel/src/task`

### 2026-06-17 - 阶段 4 futex robust-list expansion 批准

**阶段：** 阶段 4 - direct-TID user resolver 分流。

**决策：** 用户批准将 `anemone-kernel/src/task/api/futex/get_robust_list.rs` 纳入阶段 4 write set。

**批准边界：**

- 仅限 `get_robust_list(tid, ...)` 指定 TID target 命中 kthread 时的 fail-closed 分流。
- `tid == 0` 的 current-task selector 不需要 kthread 防御；kthread 不触发 syscall。
- 不批准扩展 robust futex 语义，不修改 `set_robust_list` 或 `exit_robust_list`。

### 2026-06-17 - 阶段 4 futex robust-list 窄修复

**阶段：** 阶段 4 - direct-TID user resolver 分流。

**实际 write set：**

- `anemone-kernel/src/task/api/futex/get_robust_list.rs`
- 本事务日志

**变更：**

- `get_robust_list(tid, ...)` 的指定 TID 分支在 `get_task(&tid)` 后检查目标 `ThreadGroupType`。
- 指定 TID 命中 `ThreadGroupType::KThread` 时返回 `NoSuchProcess`，与阶段 4 direct target fail-closed policy 对齐。
- `tid == 0` 的 current-task selector 未增加 current-kthread 防御；kthread 不触发 syscall。

**边界确认：**

- 未修改 `set_robust_list`。
- 未修改 `exit_robust_list`。
- 未扩展 robust futex 语义，也未改变 kthread exit 的 robust-list cleanup policy。

**验证：**

- `just fmt kernel`：通过。
- `git diff --check`：通过。
- `just build`：通过；仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。
- source audit：`rg -n "get_robust_list|ThreadGroupType::KThread|robust_list" anemone-kernel/src/task/api/futex anemone-kernel/src/task` 确认新增分流只落在 `get_robust_list` 指定 TID 路径；`set_robust_list` 和 `exit_robust_list` 未改。

**结论：** `SYS_GET_ROBUST_LIST` direct-TID kthread 漏口已按批准边界修复，可回到总控 source audit。

### 2026-06-17 - 阶段 4 current/self selector policy cleanup

**阶段：** 阶段 4 - user-facing API 分流。

**实际 write set：**

- `anemone-kernel/src/task/api/jobctl/{getpgid.rs,getsid.rs,setpgid.rs}`
- `anemone-kernel/src/task/api/priority.rs`
- `anemone-kernel/src/task/api/wait/mod.rs`
- `anemone-kernel/src/task/resource/api/prlimit64.rs`
- `anemone-kernel/src/task/sig/api/{kill.rs,mod.rs}`
- 本事务日志

**变更：**

- 去掉 current/self selector 上的“current task 是否 kthread”防御分支。
- `pid == 0`、`who == 0`、current process group / session 等 current selector 继续按 user task by construction 处理，不再做虚假 current-kthread 检查。
- 保留 direct pid/tid/tgid target 命中 kthread 的 fail-closed 分流，以及 broadcast / UID / process-group 枚举中对 kthread 的跳过。

**边界确认：**

- 未改变 `get_robust_list(tid, ...)`、`prlimit64(pid, ...)` 或 signal direct target 的 kthread policy。
- 未放宽 any direct target fail-closed 语义。

**验证：**

- `just fmt kernel`：通过。
- `git diff --check`：通过。
- source audit：`rg -n "current_tg\\.ty\\(|caller\\.ty\\(|get_current_task\\(\\)\\.get_thread_group\\(\\)\\.ty\\(|current.*ThreadGroupType::KThread|current.*ThreadGroupType::User" anemone-kernel/src/task/api anemone-kernel/src/task/sig/api anemone-kernel/src/task/resource/api` 不再命中 current/self selector 的 kthread 防御。

**结论：** current/self selector 的防御性编程已按阶段 4 policy cleanup 收窄；剩余 kthread 分流只保留在 direct target 和枚举路径。

### 2026-06-17 - 阶段 4 gate closeout

**阶段：** 阶段 4 - closeout。

**结论：** 阶段 4 gate 已关闭。`kthreadd` 与 ordinary kthread `TaskBinding::KThread` publish、专用 `kthread_exit()`、task-local closeout 边界、topology/procfs unpublish、external handle completion 延后，以及最小 user-facing API 分流已同 gate 落地。

**总控复核要点：**

- `kthread_entry_shim()` 不再调用完整 `kernel_exit()`；`kernel_exit()` 对 kthread 直接 panic。
- `kthread_exit()` 完成 internal result 后，先执行阶段 4 fd-table 空断言，再执行 topology/procfs unpublish，随后 publish external completion 并进入 scheduler zombie tail。
- `/proc/<tgid>` lazy lookup 的 binding 创建走 topology -> procfs binding lock order；procfs `status` / `stat` 使用 display-only parentage helper。
- wait/job-control/signal/priority/prlimit/get_robust_list 的 kthread 分流只作用于 direct target 或枚举路径；`pid == 0`、`who == 0`、current process-group/session 等 current/self selector 未新增 kthread 防御。

**验证：**

- `git diff --check`：通过。
- `mdbook build docs`：通过，写入 `docs/book`。
- `just build`：通过；仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。
- source audit：`rg -n "kernel_exit\\(" anemone-kernel/src/task/kthread anemone-kernel/src/task/api/exit` 确认 kthread entry 不调用完整 `kernel_exit()`。
- source audit：kthread core legacy `KThreadService` / park / weak handle / typed start pointer 查询无输出。
- source audit：`get_robust_list` 新增分流只落在指定 TID 路径；`tid == 0` current selector 未增加 kthread 防御。

**未运行：** QEMU、boot smoke、LTP / user-test。阶段 4 当前关闭在 source/build gate；runtime smoke 与更广的 user-facing policy 复查进入阶段 5。

## 开放事项

- 准备阶段 5 post-gate user-facing boundary closeout：补齐更广的 source audit、errno / inert policy 记录和 focused smoke。
- 阶段 5 若发现阶段 4 遗漏能命中 kthread 的 user-facing path，必须回到阶段 4 gate 补修并记录 correction，不能作为普通阶段 5 后续项处理。
- QEMU / procfs / job-control / signal smoke 尚未运行。

## 收口

尚未关闭。
