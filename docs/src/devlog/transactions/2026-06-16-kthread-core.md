# 2026-06-16 - KThread Core

**状态：** 活动中
**负责人：** doruche, Codex
**领域：** task / topology / procfs / kthread
**权威计划：** [RFC-20260616-kthread-core](../../rfcs/kthread-core/index.md), [不变量需求](../../rfcs/kthread-core/invariants.md), [迁移实施计划](../../rfcs/kthread-core/implementation.md)
**当前阶段：** 阶段 1 已关闭，等待阶段 2 准备

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

## 开放事项

- 阶段 2 worker 尚未启动。
- 阶段 2 需要先盘点 `Task::new_kernel()`、TID allocator、publish guard、`TaskBinding` 和 topology accessor 调用点。
- 阶段 3+ worker 要等阶段 2 关闭并把 review evidence 写入本事务后再准备启动。

## 收口

尚未关闭。
