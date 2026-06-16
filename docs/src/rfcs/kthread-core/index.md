# RFC-20260616-kthread-core

**状态：** 已接受，等待实现
**负责人：** doruche, Codex
**最后更新：** 2026-06-16
**领域：** task / topology / procfs / kthread
**事务日志：** None
**开放问题：** 当前无 active tracking gate；已关闭的 review findings 保留在 [Tracking Issues](./tracking-issues.md)。
**下一步：** 实现尚未开始；若要进入实现，先按 [迁移实施计划](./implementation.md) 创建 transaction / preflight 记录。

## 摘要

本文定义 Anemone 第一阶段 `kthread-core` 的纠偏设计。它接续现有公开 `RFC-20260614-kthread` 的实现事实，但不继续把那份追补 RFC 当作后续权威；旧 RFC 只能作为 historical baseline，记录当前 `kthreadd` 创建队列、typed entry、stop/park/service 等 legacy 形状。

新 core 的目标更窄：在现有 `Task` 执行能力之上提供 kernel thread 的创建、procfs-visible identity、协作式 stop、纯 wake 能力、strong handle、退出结果和专用退出路径。它不包含 service/request/workqueue、park/unpark、独立 kthread registry 或用户进程 job-control 语义。

## 背景

当前分支已有可运行实现：

- `task::kthread::{mod.rs, create.rs}` 提供 `kthreadd` 创建队列、typed start shim、`KThreadControl`、weak `KThreadRef` 和 legacy park/unpark 状态。若某个实现 checkout 仍有 `service.rs` 或 `KThreadService` 残留，它只作为待清理的 legacy surface，不作为未来设计输入。
- inode shrinker 和 OOM killer 已作为 kthread consumer 落地；两者都可以用显式 loop 表达业务状态，不需要 core 内置 service/request 层。
- procfs root 通过 thread group registry 枚举 `/proc/<pid>`，`/proc/<pid>/status` 已输出 `Kthread:`，无 userspace 的 `/proc/<pid>/cmdline` 已返回空。

主要长期风险不在 “能否跑通”，而在 owner boundary：

1. ordinary kthread 当前通过未区分的 `TaskBinding::Leader` 发布，实际进入普通 parent / process group / session / wait/reap 拓扑。
2. kthread entry 当前尾部仍调用普通 `kernel_exit()`，虽然已先完成 kthread finish path，但仍经过用户进程 cleanup 与 wait/reap 逻辑。
3. external handle 当前是 weak ref，不足以作为稳定 lifecycle capability。
4. service/request 曾被写进 legacy kthread contract；当前设计已决定直接去掉。后续实现只审计无残留、无重引入，不再把它作为迁移对象。
5. `kthreadd` 当前没有显式固定 TID 2；legacy 启动顺序下 AP kinit 可能先消耗 TID 2。

## 目标

- 复用现有 `Task` 的 kernel stack、context switch、scheduler state、wait core 和 runqueue placement。
- 让每个 kthread 都是 procfs-visible singleton thread group leader，且 `tid == tgid`。
- 引入显式 kthread-aware topology type，区分 ordinary user process 与 kernel thread；`kthreadd` 的特殊身份由 `Tid::KTHREADD` 派生。
- 固定 `kthreadd` 的 TID/TGID 为 2；普通 TID allocator 从 3 开始分配，TID 2 由 `kthreadd` 专用 handle 单独拥有，不依赖启动顺序。
- 让 ordinary kthread 的 `PPid` 指向 `kthreadd`，`kthreadd` 自己输出 inert parent；第一版 `pgrp/session` 类字段输出 `0` 或明确 inert value。
- kthread 不加入普通 `ProcessGroup` / `Session` 成员集，不参与 `setpgid` / `setsid` / `kill(-pgid)` / job-control。
- 提供 strong `KThreadHandle`，使 subsystem owner 可以 request stop、wake、wait exited 和读取退出 completion code。
- 保留 monomorphic function + `AnyOpaque` start argument entry API；kthread core 只搬运和 drop opaque payload，不负责 downcast。
- 定义专用 `kthread_exit()`，不复用完整 user-process `kernel_exit()`。
- ordinary kthread 启用 kthread-aware topology、专用 exit、topology/procfs unpublish 和最小 user-facing API 分流必须作为同一个语义 gate；实现可以提前落地 type、accessor、assert 等 scaffolding，但不能让 ordinary kthread 先进入 `TaskBinding::KThread` 后仍调用普通 `kernel_exit()` 或仍被 ordinary process API 管理。
- 删除 core 中的 `KThreadService` 和 park/unpark 协议；consumer 自己拥有 request / service / pressure / active-victim 等业务状态。

## 非目标

- 不实现强杀、异步取消或抢占式停止。
- 不实现 park/unpark、freezer、runtime bind、bind mask、CPU hotplug 或 scheduler class / priority 策略。
- 不实现 workqueue、worker pool、request queue、flush/cancel work、delayed work、service discovery 或负载均衡。
- 不引入独立 `KThreadRegistry` 或 `KThreadId`。
- 不让 kthread 保留 user-visible zombie，也不让普通用户进程 `wait4()` / `waitid()` reap kthread。
- 不追求第一版 Linux procfs 中 pgrp/session 字段的完整展示兼容；后续可小迭代补齐。
- 不整理 `bsp-kinit` / `ap-kinit` bootstrap task 模型，除非固定 `Tid::KTHREADD` 需要最小 TID allocator 支持。
- 不把 closure API 作为第一阶段 gate；closure builder 可作为后续 optional polish。

## 文档地图

Canonical：

- [不变量需求](./invariants.md)
- [迁移实施计划](./implementation.md)
- [Tracking Issues](./tracking-issues.md)

背景材料：

- [背景材料索引](./backgrounds/index.md)

## 方案

### 分层

第一阶段采用四个清晰边界：

1. `Task`：执行载体，拥有 kernel stack、scheduler context、`TaskSchedState`、runqueue placement 和 wait integration。
2. kthread-aware task topology：拥有 active PID/TGID identity、`/proc/<pid>` 枚举、`PPid` 展示和 active lifetime 可见性。
3. `KThreadControl` / `KThreadHandle`：拥有 lifecycle phase、wake event、exited event 和退出 completion code。
4. consumer / future workqueue：拥有业务 request truth、pending queue、pressure state、worker count、drain/flush/cancel 和调度策略。

`kthreadd` 属于 core，但它只拥有 creation transaction、fixed TID anchor 和 ordinary kthread 的 procfs parent anchor；它不是 service dispatcher、workqueue manager 或 lifecycle owner。

### 文件组织与命名

第一阶段 `task::kthread` 建议按 owner role 拆分：

- `mod.rs`：facade、public re-export 和模块说明。
- `spawn.rs`：`KThreadBuilder`、`KThreadPlacement` 和 public `spawn()` 入口。
- `kthreadd.rs`：`init()`、`submit()`、task entry `run()`、单个 request 处理 `spawn()`、`KTHREADD`、`SPAWN_QUEUE` 和 `SPAWN_WAKE`。
- `entry.rs`：`KThreadEntry`、`KThreadLaunch`、`AnyOpaque` start payload 和 monomorphic entry shim。
- `control.rs`：`KThreadControl`、`KThreadPhase`、`wake` / `exited` 事件和 lifecycle 线性化。
- `handle.rs`：public strong `KThreadHandle`。
- `ctx.rs`：`KThreadCtx`，即 entry 内部的窄运行环境能力。

不新增 `service.rs`、`registry.rs`、`utils.rs` 或内部 `KThread` 实体。`Task` 的 task-local `kthread` 字段直接保存 kthread 专用 attachment：`Arc<KThreadControl>` 加 one-shot launch slot；`KThreadHandle` 和 `KThreadCtx` 是同一 control block 的不同权限视图。

命名原则：

- 避免 `Value`、`Data`、`Context`、`Manager`、`State`、`utils`、`misc` 等低区分度名称，除非它们确实是精确实体名或已形成业务语义。
- `KThreadCtx` 保留 `Ctx`，因为它精确表示 entry 的运行环境能力。
- 不使用 `KThreadSnapshot` / `KThreadLabel`。第一阶段没有 post-exit 诊断身份需求，control 不保存 `tid`、`name` 或 `created_at`。

### Topology

kthread 复用 `ThreadGroup` 容器，但必须带显式 type。建议形态：

```rust
pub enum ThreadGroupType {
    User,
    KThread,
}
```

要求：

- `kthreadd` 和 ordinary kthread 都是 `KThread`，每个都是 singleton thread group leader。
- `kthreadd` 的特殊身份由 `tgid == Tid::KTHREADD` 派生，不编码为单独 topology type。
- kthread thread group 只包含 leader task 自己，不能添加 member。
- kthread thread group 不加入普通 process group / session 成员集。
- kthread 与 user process 不发生相互转化；`TaskFlags::KERNEL` 可以作为创建期设置、之后不可变的快速缓存使用，但不能脱离创建/publish 协议独立变更。
- `ThreadGroup::pgid()`、`sid()`、`parent_tgid()` 是 user-process accessor。非 `User` 调用必须 panic；调用者需要分流时先读取 `ty()`。
- `ThreadGroupInner` 的 `pgid` / `sid` 可以用 `Option<Tid>` 保存，但必须用 assert 维护：`User` 必须是 `Some`，`KThread` 必须是 `None`。
- `children_tgids` 只属于 `User` wait/reap topology；`KThread` 必须保持空 children list。
- `/proc/<pid>/status` 和 `/proc/<pid>/stat` 中，ordinary kthread 的 `PPid` 指向 `kthreadd`；`kthreadd` 自己输出 `0` 或明确 inert value。
- `pgrp/session/NSpgid/NSsid` 第一版输出 `0` 或明确 inert value。
- wait/reap、job-control、process-group signal、user-visible signal、priority / resource / scheduler user API 操作遇到 kthread type 必须 fail closed 或跳过；只读 API 若返回 inert view，必须明确不会被当作 user-process truth，具体 errno 可在实现期按 ABI 需要确定。

调用点必须按用途分类：

1. User-only：`getpgid(pid)`、`getsid(pid)`、`setpgid()`、`setsid()`、ordinary wait/reap、ordinary job-control 和 signal permission helper 中的 `SIGCONT` same-session check。它们在读取 `pgid()` / `sid()` / `parent_tgid()` 前必须确认目标是 `ThreadGroupType::User`；遇到 kthread 走 fail-closed resolver，不能调用 User-only accessor。
2. procfs display helper：只服务 `/proc/<pid>/status` 和 `/proc/<pid>/stat` 的 inert 展示值，不能反向作为 job-control、signal permission、waitability 或 lifecycle truth。
3. fail-closed resolver：direct signal、priority / resource / scheduler user API 或其它按 pid/tgid/uid 解析 task 的用户入口，必须在调用 User-only accessor 或 materialize target list 前按 topology type 分流。

ordinary kthread 的 `TaskBinding::KThread` 切换是语义 gate，不是单独可接受的中间态。允许提前合入的只有不会改变 ordinary kthread 外部行为的 scaffolding，例如 `ThreadGroupType` enum、只读 `ty()`、新 accessor 骨架、shape assertion、procfs display helper 预备代码和 source audit；一旦 ordinary kthread 真的用 kthread-aware binding 发布，同一 review gate 必须同时落地专用 `kthread_exit()`、kthread topology/procfs unpublish 和所有能命中 kthread 的 user-facing resolver 分流。

### `kthreadd` TID

`kthreadd` 的 TID/TGID 必须固定为 2。legacy code 目前不保证这一点：TID allocator 从 1 开始，BSP root task 消耗 TID 1，AP kinit 可能在 `init_kthreadd()` 前调用 `Task::new_kernel()` 并消耗 TID 2。

实现方向采用 allocator 范围隔离：

- 增加 `Tid::KTHREADD = Tid::new(2)` 或等价常量。
- 普通 TID allocator 初始化范围从 3 开始，确保普通 `alloc_tid()` 永远不会返回 0、1 或 2。
- TID 2 不在普通 allocator 管辖内；`init_kthreadd()` 必须通过 `kthreadd` 专用 one-shot handle 创建 TID/TGID 2，并 assert `tid == tgid == Tid::KTHREADD`。
- 第一阶段不提供通用 `reserve_tid(Tid)` API，避免把 fixed kernel TID 需求扩张成任意 caller 的 allocator surface。
- 不采用 “提前创建 kthreadd，靠启动顺序抢到 2” 的方案。

### 创建

ordinary kthread 默认异步提交给 `kthreadd` 创建。当前 legacy `create.rs` 的 create queue 和 completion 是可保留资产；typed start exactly-once reclaim 只作为历史实现事实，第一阶段 accepted contract 改为 task-local launch slot。

接受边界：

- `kthreadd` 可以是 creation owner。
- 同步创建不进入第一阶段范围；若未来需要，必须另行设计，且不能绕过本节的 publish / handle / control 不变量。
- create transaction 不赋予 `kthreadd` stop/exited/result ownership。
- create queue 不得扩张为 service queue、work queue、request dispatcher 或 load balancer。
- `spawn()` 消费 `AnyOpaque` start payload；失败时 create transaction 正常 drop payload，不返还给 caller。
- publish 是 create transaction 的 commit 边界；publish 前失败可以 drop unpublished task、control 和 launch payload，publish 后只能执行 infallible enqueue 与 success completion。

### 生命周期

第一阶段生命周期只有：

1. create：初始化 `KThreadControl`，创建底层 task，安装 task-local kthread attachment，发布 kthread-aware topology，返回 strong handle。
2. run：monomorphic entry shim 从 current task 的 launch slot `take()` 出 `KThreadLaunch`，构造 `KThreadCtx`，调用 entry。
3. request stop：handle 将 `KThreadPhase::Running` 线性化为 `StopRequested`，并 publish wake event。
4. wake：handle 只 publish wake event，不改变 lifecycle state，不代表业务 request truth。
5. exit：entry 返回或显式 `kthread_exit(code)`，先记录 internal result completion，再完成 task-local resource closeout，撤销 procfs-visible topology 与 procfs binding，发布 exited event，进入 scheduler zombie tail。
6. wait exited：handle 等待 control 中的 exited event 并读取 exit result；该 event 是 external exited completion，只能在 closeout 与 unpublish 完成后发布。

kthread 退出后 `/proc/<pid>` 立即不可见，不保留 user-visible zombie / tombstone。若有 owner 需要结果，它必须持有 strong handle。

exit contract 分三层：

1. user-process cleanup：clear-child-tid、robust futex、user mm / address-space 退出、ordinary child-exited、reparent、`SIGCHLD`、ordinary wait/reap 和 vfork completion。`kthread_exit()` 不执行这一层。
2. task-local resource closeout：释放只属于当前 task 的资源，尤其是未来可能需要 sleepable process context 的 opened-description final release、fanotify cleanup 或 fd-table closeout。`kthread_exit()` 必须复用或显式定义这一层。
3. scheduler zombie tail：只负责把当前 task 交给 scheduler tail，不包含 user-process cleanup。

第一阶段若继续禁止 kthread 拥有 fd-table，必须在 task-local closeout 处保留 assert 和注释：说明 kthread fd-table 为空是临时边界，且一旦允许 kthread 继承或打开 fd，就必须先把该 assert 替换为完整 task-local closeout helper，再允许相关 consumer 合入。

topology/procfs unpublish 的唯一 owner 是 kthread 专用 exit 路径中的 topology unpublish transaction。procfs 层可以提供 binding invalidation hook，但不能独立决定 kthread lifecycle。该 transaction 必须在同一协议内标记 kthread 正在 unpublish、使 procfs binding 失效、移除 active topology membership；任何 `/proc/<pid>` lookup 在看到 unpublishing/exited 标记后只能返回不可见结果，不能根据残留 topology 或旧 binding 反向重建目录。

### API

第一版使用 monomorphic function + `AnyOpaque` start argument：

```rust
pub type KThreadEntry = fn(KThreadCtx, AnyOpaque) -> i32;

impl KThreadBuilder {
    pub fn spawn(self, entry: KThreadEntry, arg: AnyOpaque) -> Result<KThreadHandle, SysError>;
}
```

`AnyOpaque` 的 downcast 只允许发生在 consumer 自己的 entry/helper 内。kthread core 不按 concrete payload type 分支，不提供泛型 `spawn<T>()`，也不负责把 downcast 错误映射为 syscall errno。

`i32` 是 kthread-local completion code，不是 syscall errno contract，也不默认等价 `SysError`。core 只保存和返回它，不把它映射成 signal status、wait status、`ExitCode` 或 syscall errno。若某个 consumer 需要强类型 result，应在 consumer 自己的 handle 上封装。

`KThreadBuilder` 使用显式 placement：

```rust
pub enum KThreadPlacement {
    Any,
    OnCpu(CpuId),
}
```

`KThreadBuilder::cpu(cpu)` 只是 `placement(KThreadPlacement::OnCpu(cpu))` 的 convenience，不是内部存储模型。

`Task::new_kernel()` 仍可作为底层 task constructor，但 kthread start payload 不再通过 `ParameterList` 或 raw pointer 传递。`task::kthread` 必须在 task 对 topology/procfs 可见前安装 task-local kthread attachment：

```rust
struct KThreadTaskLocal {
    control: Arc<KThreadControl>,
    launch: SpinLock<Option<KThreadLaunch>>,
}

struct KThreadLaunch {
    entry: KThreadEntry,
    arg: AnyOpaque,
}
```

`KThreadTaskLocal` 本身不使用 `Arc`，它由 `Task` 拥有；需要跨 handle、ctx 和 exit path 共享的只有 `Arc<KThreadControl>`。`launch` 只服务 entry 启动一次，entry shim 第一件事必须 `take()` 出 `KThreadLaunch`，重复进入或缺失必须 panic。`kthreadd` 自己也安装 `KThreadTaskLocal` 以满足所有 `TaskBinding::KThread` task 的 publish invariant，但 `kthreadd.launch == None`，也不暴露 public `KThreadHandle`。

closure API 或泛型 owned payload API 可作为后续 optional 目标，但不进入第一阶段 correctness gate。

### Handle 与诊断身份

`KThreadHandle` 是 strong lifecycle capability。它不暴露底层 `Arc<Task>`，只暴露：

- `request_stop()`
- `wake()`
- `wait_exited()`
- `has_exited()`

不提供 public `stop()`，避免把协作式 request-stop 误读为同步或强制停止。若调用者需要同步停止，应显式执行 `request_stop(); wait_exited()`。

`KThreadPhase::Exited(code)` 只表示 entry result 已完成；public `has_exited()` / `wait_exited()` 观察 `exited` event，而不是直接把 phase 当成外部完成状态。该 event 必须晚于 task-local resource closeout 和 topology/procfs unpublish。

不引入独立 `KThreadRegistry` / `KThreadId`。active kthread 的行为身份由 topology 的 `Tid/Tgid` 承担；第一阶段不承诺退出后的 `tid()` / `name()` 诊断 getter。

### Initcall 边界

`kthreadd` 初始化是 boot invariant，不走 initcall：它必须在 `init` topology 可用之后、任何 ordinary kthread spawn 之前手动建立。

ordinary kthread consumer 由所属子系统自己的 init path 启动。initcall level 表达初始化时刻，不表达 “是否会 spawn kthread”。若需要把当前手写的 inode shrinker / OOM killer 初始化迁入 initcall，新增通用 `Late` level，而不是 kthread-specific level。`Late` 的语义是基础 filesystem / driver / probe 初始化完成、`kthreadd` 已存在、所有 CPU 已完成 local init、用户 init 尚未 exec。

## 接受边界

本文被接受意味着：

1. kthread-core 是 `Task` 之上的 kernel-owned lifecycle layer，但 active identity 仍由 task topology 承担。
2. kthread 是 procfs-visible singleton thread group leader，带显式 kthread type。
3. kthread 不加入 ordinary process group / session，不参与 user-process wait/reap/job-control。
4. `kthreadd` TID/TGID 固定为 2；普通 allocator 从 3 开始，`kthreadd` 只拥有 creation transaction、fixed TID anchor 与 ordinary kthread parent anchor。
5. lifecycle owner 是 strong `KThreadControl` / `KThreadHandle`，不是 topology、`kthreadd`、scheduler state 或 registry。
6. kthread exit 走专用路径，不复用完整 user-process `kernel_exit()`；该路径保留 task-local resource closeout，并与 topology/procfs unpublish 及最小 user-facing API 分流属于同一个 semantic gate。
7. `wake()` 是纯 wake capability，业务 request truth 留在 consumer。
8. `KThreadService`、park/unpark 和独立 registry 必须从 core contract 中移除。

## 备选方案

### 继续使用未区分的 `TaskBinding::Leader`

拒绝。leader + singleton thread group 方向可以保留，但没有 type 会让 wait/reap、process group、session、broadcast signal 和 exit path 自然继承普通用户进程语义。

### kthread 加入普通 process group / session

拒绝。job-control 不是 core 需求。第一版 procfs 中 pgrp/session 使用 inert value，Linux 展示兼容可后续小迭代。

### 复用完整 `kernel_exit()`

拒绝。即使先完成 kthread finish path，完整 `kernel_exit()` 仍会执行用户进程 cleanup、thread-group exited/reap、child-exited、reparent、vfork completion 等语义。可以抽取底层 scheduler zombie tail，但不复用完整 user-process exit。

### 独立 kthread registry / KThreadId

拒绝进入第一阶段。active identity 已由 topology 承担，lifecycle 已由 strong handle/control 承担，create transaction 已由 `kthreadd` create queue 承担。剩余 debug dump 需求不足以制造第二套 truth source。

### weak external handle

拒绝作为 lifecycle capability。weak ref 可以作为局部 convenience，但 owner 持有 handle 时，control block 必须稳定存在并保留 exit result。

### `KThreadService` 属于 core

拒绝。service/request/pending/drain 是 consumer 或 future workqueue 的业务属性。当前 consumer 均可使用 explicit loop。

### park/unpark 属于 core

拒绝。第一版只保留 stop/wake/exit。拆除 park/unpark 可以简化状态机和证明义务。

### 靠启动顺序让 kthreadd 抢到 TID 2

拒绝。SMP bootstrap 中 AP kinit 可能先消耗 TID 2。必须通过 allocator reserved handle 显式保证。

### 运行时每次经 topology 判断 kernel task

拒绝作为必要条件。kthread 与 user process 不发生相互转化，`TaskFlags::KERNEL` 在创建/publish 协议内一次性设置后可以作为稳定缓存使用。该缓存不得独立突变，也不得替代 topology publish 过程中的 `ThreadGroupType` shape assertion。

## 风险

- topology type 触达 wait/reap、job-control、signal、procfs、exit 等多个面。控制方式是先做 module-boundary/topology preflight，再按 gate 修改。
- 去除 `KThreadService` 后，未来 workqueue 需要另建上层设施。控制方式是把 consumer loop 作为当前唯一 accepted 使用模式，不在 core 中预留 fake service。
- strong handle 可能延长 control block 生命周期。控制方式是 control block 不强持有 task，且第一阶段不保存 post-exit `tid`、`name` 或 `created_at` 诊断身份。
- kthread exit 后立即撤销 procfs 可能减少用户可见调试窗口。控制方式是 handle/control 侧保留 kernel-side exit result；需要 user-visible tombstone 时另行设计。

## 收口

本文已提升为公开 RFC，并成为后续 `kthread-core` 纠偏实现的 canonical source。实现尚未开始；进入代码阶段前需要：

1. 按 [迁移实施计划](./implementation.md) 建立 transaction devlog 或明确的 preflight 记录。
2. 在 transaction 中记录阶段 write set、验证 floor 和 source-audit 证据。
3. 保持旧 [RFC-20260614-kthread](../kthread/index.md) 作为 historical baseline；后续纠偏实现以本文为准。
4. 实现推进时同步更新 [Tracking Issues](./tracking-issues.md) 与 transaction devlog 的 gate 证据。
