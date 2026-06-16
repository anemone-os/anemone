# 2026-06-16 - KThread Core

**状态：** 活动中
**负责人：** doruche, Codex
**领域：** task / topology / procfs / kthread
**权威计划：** [RFC-20260616-kthread-core](../../rfcs/kthread-core/index.md), [不变量需求](../../rfcs/kthread-core/invariants.md), [迁移实施计划](../../rfcs/kthread-core/implementation.md)
**当前阶段：** 阶段 2 checkpoint 已关闭，准备阶段 3

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

## 开放事项

- 准备阶段 3 worker，但不能直接启动阶段 4+ worker。
- 阶段 3 worker 必须继续保留 `TaskBinding::KThread` 为未启用 scaffolding；实际 `kthreadd` / ordinary kthread `KThread` publish 留到阶段 4。

## 收口

尚未关闭。
