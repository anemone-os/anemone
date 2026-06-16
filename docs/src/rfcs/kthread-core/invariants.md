# KThread Core 不变量需求

**状态：** Canonical
**最后更新：** 2026-06-16
**父 RFC：** [RFC-20260616-kthread-core](./index.md)

本文定义 kthread-core 的状态所有权、procfs-visible topology、handle 能力、线性化点和禁止退化项。本文已作为 accepted contract 收口，但实现尚未开始；已确认的文档层 review findings 记录在 [Tracking Issues](./tracking-issues.md)，落地顺序见 [迁移实施计划](./implementation.md)。

## 闭合条件

第一阶段完成后必须同时满足：

1. `kthreadd` 的 TID/TGID 固定为 2；普通 TID allocator 从 3 开始，TID 2 由 `kthreadd` 专用 handle 单独拥有。
2. 每个 kthread 都是 procfs-visible singleton thread group leader，`tid == tgid`。
3. topology 能通过 `ThreadGroupType` 区分 ordinary user process 与 kernel thread；`kthreadd` 的特殊身份由 `Tid::KTHREADD` 派生。
4. kthread thread group 不加入普通 `ProcessGroup` / `Session` 成员集。
5. ordinary kthread 的 `PPid` 指向 `kthreadd`，`kthreadd` 自己的 `PPid` 输出 inert value；`pgrp/session` 类字段第一版输出 inert value。
6. wait/reap、job-control、process-group signal 和 user-visible signal 操作不能把 kthread 当普通用户进程处理。
7. direct `getpgid(pid)` / `getsid(pid)` 和 signal permission helper 的 `SIGCONT` same-session check 必须在调用 `pgid()` / `sid()` 前完成 kthread 分流。
8. lifecycle 由 strong `KThreadControl` / `KThreadHandle` 拥有。
9. scheduler runnable / waiting / zombie 仍只由 `TaskSchedState` 表示。
10. stop 是协作式协议：handle 请求停止并唤醒，entry 自己检查后退出。
11. wake 是纯唤醒能力，不代表业务 request truth。
12. kthread exit 不执行普通用户进程 cleanup、reparent、child-exited 或 wait/reap 协议，但必须执行或显式定义 task-local resource closeout。
13. kthread exit 后撤销 procfs-visible topology 与 procfs binding，不保留 user-visible zombie。
14. ordinary kthread 启用 kthread-aware binding、专用 exit 和 topology/procfs unpublish 必须同 gate 闭合；只有不改变 ordinary kthread 发布语义的 scaffolding 可以早于该 gate。
15. core 不包含 `KThreadService`、park/unpark、workqueue、request queue、worker pool、负载均衡或独立 kthread registry。

任一条件不满足时，只能声明为迁移中间态，不能作为 kthread-core contract 推广给新 consumer。

## 非目标

本文不定义：

1. 用户可写或可控制的 kthread ABI。
2. 普通用户进程 `wait4()` / `waitid()` reaping kthread。
3. 强杀、异步取消、抢占式停止。
4. park/unpark、freezer、runtime bind、bind mask、CPU hotplug。
5. workqueue、delayed work、flush/cancel work、worker pool、service discovery。
6. 独立 `KThreadRegistry`、`KThreadId` 或 exited tombstone namespace。
7. Linux 完整 procfs pgrp/session 展示兼容。
8. closure builder API 的第一阶段实现。

## 状态所有权

### Scheduler 状态

`TaskSchedState` 是 scheduler 状态的单一真相源：

- `Runnable`：task 可被 runqueue 调度。
- `Waiting`：task 正处于 wait core 管理的一轮等待。
- `Zombie`：scheduler 不应再次运行该 task。

kthread-core 不能缓存另一个 runnable/waiting/zombie 状态，也不能通过 `KThreadControl` 直接做 runqueue placement。

### Topology 状态

task topology 是 active PID/TGID identity 和 procfs-visible membership 的单一真相源。

要求：

1. kthread 使用现有 `ThreadGroup` 容器，但必须带显式 `ThreadGroupType`。
2. ordinary kthread thread group 只有 leader 一个 member。
3. kthread thread group 不在 `ProcessGroup.members` 或 `Session.process_groups` 中出现。
4. kthread topology unpublish 发生在专用 exit 路径中，不等待用户态 reap。
5. `ThreadGroupType` 是行为状态，不是诊断字段；wait/job-control/signal/procfs 必须能按 type 分流。
6. `ThreadGroup::pgid()`、`sid()` 和 `parent_tgid()` 是 user-process accessor。非 `ThreadGroupType::User` 调用必须 panic；需要分流的 caller 必须先读取 `ty()`。
7. `ThreadGroupInner` 的 `pgid` / `sid` 可以保存为 `Option<Tid>`，但必须用 assert 维护形状：`User` 为 `Some`，`KThread` 为 `None`。
8. `children_tgids` 只属于 `User` wait/reap topology；`KThread` 必须保持空 children list。
9. kthread 与 user process 不发生相互转化。`TaskFlags::KERNEL` 是创建期设置、之后不可变的快速缓存，可用于避免热路径重复获取 topology lock；它不得独立突变，也不得绕过 `ThreadGroupType` 的 publish-time shape assertion。
10. `TaskBinding::KThread` 对 ordinary kthread 生效时，必须已经处在专用 exit 与 topology/procfs unpublish gate 内；提前阶段只能合入 enum、accessor、assertion、display helper 和 source audit 等 scaffolding，不能让 ordinary kthread 先脱离 ordinary PG/session/children 后仍走普通 `kernel_exit()`。

建议 publish binding：

```rust
pub enum TaskBinding {
    UserLeader {
        parent_tgid: Tid,
        pgid: Tid,
        sid: Tid,
        terminate_signal: Option<SigNo>,
    },
    KThread,
    Member,
}
```

`TaskBinding::Member` 只能加入 `ThreadGroupType::User`。向 `KThread` 加 member 必须 panic。`kthreadd` 与 ordinary kthread 都通过 `TaskBinding::KThread` 发布；`kthreadd` 的 singleton identity 由 reserved `Tid::KTHREADD` 断言维护。

### Accessor 分类

`pgid()`、`sid()` 和 `parent_tgid()` 的调用点必须分成三类：

1. User-only：ordinary process API 和权限判断，包括 direct `getpgid(pid)` / `getsid(pid)`、`setpgid()`、`setsid()`、ordinary wait/reap、job-control，以及 signal permission helper 的 `SIGCONT` same-session check。它们必须先确认目标是 `ThreadGroupType::User`，再调用 User-only accessor。
2. procfs display helper：只用于 `/proc/<pid>/status` 和 `/proc/<pid>/stat` 展示 kthread 的 inert parent、pgrp、session 字段。该 helper 不得被 syscall、signal permission 或 wait/job-control 路径复用为行为 truth。
3. fail-closed resolver：direct signal、priority / scheduler user API、pid/tgid resolver 或其它可能命中 kthread 的用户入口，必须在调用 User-only accessor 前按 `ThreadGroupType` 拒绝或跳过 kthread。

source audit 闭合前，不允许新增未分类的 `pgid()`、`sid()` 或 `parent_tgid()` 调用点。

### Priority / resource / scheduler resolver 分类

`setpriority()`、`getpriority()`、`prlimit64()`、nice / scheduler 用户入口，以及任何按 pid、tid、tgid、pgid、session 或 uid 枚举目标的 resolver，必须在同一 policy 层按 `ThreadGroupType` 分流：

1. direct target 命中 `KThread` 时，mutating API 必须 fail closed；read-only API 只有在语义上本来就是 inert readonly、且不会被误读为 user-process truth 时，才可以返回 inert 结果。
2. process-group、session、uid 和 broadcast 枚举必须在 materialize 结果前跳过 `KThread`。
3. stage-1 stub 也必须记录其 kthread target policy，不能因为暂未实现就留下未分类入口。
4. 该 policy 不得依赖 procfs inert display helper，也不得绕过 `ThreadGroupType` 的显式分流。

### KThreadControl 状态

`KThreadControl` 是 lifecycle 的单一真相源，最小拥有：

- `phase: SpinLock<KThreadPhase>`
- `wake: Event`
- `exited: Event`

`KThreadPhase` 第一阶段形态：

```rust
enum KThreadPhase {
    Running,
    StopRequested,
    Exited(i32),
}
```

要求：

1. `Running -> StopRequested` 是 `request_stop()` 的线性化点。
2. `Running -> Exited(code)` 表示 entry 自然完成。
3. `StopRequested -> Exited(code)` 表示 entry 观察 stop 后完成。
4. `Exited(code)` 是终态；重复完成 exit result 必须 assert/panic。
5. `wake` 只用于唤醒 entry 重查 stop 和业务 predicate。
6. `Exited(code)` 只表示 internal result completion，不等价于 public `has_exited()` / `wait_exited()` 可见的 external exited completion。
7. `exited` 只用于 owner 等待 external exited completion；该 event 只能在 task-local resource closeout 与 topology/procfs unpublish 都完成后发布。
8. `has_exited()` / `wait_exited()` 只观察 `exited` event，而不是直接把 `phase == Exited(code)` 当作对外完成语义。
9. `KThreadControl` 不强持有 `Task`，避免 task/control 生命周期环。
10. 第一阶段 `KThreadControl` 不保存 `Tid`、`name` 或 `created_at`；active 诊断走 topology/task，退出后 handle 只承诺 completion code。

### KThread task-local attachment

`Task` 拥有 kthread 专用 task-local attachment。它不是独立 lifecycle owner，也不使用 `Arc`；需要跨 handle、ctx 和 exit path 共享的只有 `Arc<KThreadControl>`。

建议形态：

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

要求：

1. 所有通过 `TaskBinding::KThread` 发布的 task 在 topology/procfs 可见前必须已经安装 `KThreadTaskLocal`。
2. ordinary kthread 的 `launch` 初始为 `Some(KThreadLaunch)`；entry shim 第一件事必须 `take()`，重复进入或 launch 缺失必须 panic。
3. `kthreadd` 也安装 `KThreadTaskLocal`，用于满足 KThread publish invariant；但 `kthreadd.launch == None`，且第一阶段不暴露 public `KThreadHandle` 指向 `kthreadd`。
4. `launch` 是 one-shot startup storage，不参与 stop/exited/result、procfs 展示、signal、wait/reap 或 job-control。
5. `AnyOpaque` 的 downcast 只允许发生在 consumer 自己的 entry/helper 内；kthread core 只搬运和 drop opaque payload，不按 concrete payload type 分支。

### Kthreadd 状态

`kthreadd` 只拥有 create transaction：

1. create queue / completion 属于 `kthreadd` 创建协议。
2. `kthreadd` 可以在自身上下文中创建 ordinary kthread task、publish topology 并 enqueue。
3. `kthreadd` 不拥有 ordinary kthread stop/exited/result。
4. `kthreadd` 不保存 work item、service routing、pending queue、worker pool 或 load-balance state。

### 无独立 Registry

第一阶段不引入独立 `KThreadRegistry` / `KThreadId`。

理由：

1. active kthread 枚举由 topology 完成。
2. lifecycle wait/result 由 strong handle/control 完成。
3. create-in-flight 状态由 create queue/completion 完成。
4. 第一阶段不提供 global post-exit kthread diagnostic namespace。

后续若需要 crash dump tombstone、global shutdown enumeration、退出后 `tid/name` getter 或独立 debugfs 管理视图，必须另行设计，不能把它伪装成第一阶段 core 需求。

## Procfs 可见性

要求：

1. `/proc` root readdir / lookup 可以看到 active `kthreadd` 和 ordinary kthread。
2. `/proc/<pid>/status` 对 kthread 输出 `Kthread: 1`。
3. `/proc/<pid>/cmdline` 对无 userspace 的 kthread 返回空。
4. `/proc/<pid>/stat` / `status` 中，ordinary kthread 的 `PPid` 指向 `Tid::KTHREADD`，`kthreadd` 自己的 `PPid` 输出 `0` 或等价 inert value。
5. `/proc/<pid>/stat` / `status` 中 pgrp/session 类字段第一版输出 `0` 或等价 inert value。
6. kthread exit 后 `/proc/<pid>` binding 被撤销；不会留下 user-visible zombie。
7. kthread topology unpublish 与 procfs binding invalidation 由同一个 topology unpublish transaction 驱动；procfs 只提供 invalidation hook，不拥有 kthread lifecycle。
8. unpublish transaction 必须先让 lookup 能观察到 unpublishing/exited 状态，再移除 active membership 或释放旧 binding；之后 lookup 只能返回不可见结果，不能因旧 dentry/inode/binding 或残留 topology 反向重建 `/proc/<pid>`。

procfs 展示字段不得反向驱动 job-control 或 lifecycle。

## 身份与能力模型

### Active identity

active kthread 的行为身份是 `Tid/Tgid`。

要求：

1. `kthreadd` 使用 `Tid::KTHREADD == 2`。
2. ordinary kthread 在 active lifetime 内由 topology 保证 TID/TGID 唯一。
3. `Tid/Tgid` 可用于 procfs lookup、debug log 和 active task lookup。
4. 退出后不得依赖 `Tid/Tgid` 重新 resolve kthread control block。

### Strong handle

`KThreadHandle` 是 subsystem 持有的 lifecycle capability。

必须允许：

1. `request_stop()`
2. `wake()`
3. `wait_exited()`
4. `has_exited()`

不得允许：

1. 直接访问底层 `Arc<Task>`。
2. 直接修改 `TaskSchedState`。
3. 直接 enqueue / dequeue task。
4. 直接 publish / unpublish topology。
5. 绕过 `KThreadControl` 设置 exit result。

handle clone 指向同一个 control block。所有 handle drop 后，kthread 仍可运行到退出；若没有 owner 持有 handle，则退出结果不需要全局保存。

### Entry ctx

`KThreadCtx` 是 entry 内部的窄运行环境能力。

必须允许：

1. `should_stop()`
2. `wait_until(predicate)`，睡在 kthread `wake` event 上，直到 `should_stop() || predicate()`

不得允许：

1. 暴露底层 `Arc<Task>`。
2. 伪造其他 kthread 的 stop checker。
3. 修改 topology 或其他 kthread control block。
4. 表达 service pending / request queue / drain state。

## 线性化点

### Kthreadd TID allocation

`Tid::KTHREADD` 不参与普通 TID allocator。普通 allocator 的起点必须是 3，因此任何普通 `alloc_tid()` 路径都不可能分配到 2。

要求：

1. 普通 `alloc_tid()` 不会返回 0、1 或 2。
2. `init_kthreadd()` 使用 `kthreadd` 专用 one-shot TID handle 创建 task。
3. 若专用 handle 已被消费，或 topology 中已有 TID 2 非 kthreadd task，必须 panic。
4. 第一阶段不提供通用 `reserve_tid(Tid)` API。

### Spawn

spawn 成功对 caller 可见时，必须已经完成：

1. `KThreadControl` 初始化。
2. `KThreadLaunch { entry, arg: AnyOpaque }` 建立，且不依赖 caller stack。
3. 底层 `Task` 创建。
4. task-local `KThreadTaskLocal` 安装完成，其中 control link 已可通过 task-local accessor 取得。
5. kthread-aware topology publish。
6. task 已 enqueue 或已进入严格定义的 will-enqueue 状态。
7. strong `KThreadHandle` 返回。

`spawn()` 消费 `AnyOpaque` start payload。失败路径不把 payload 返还 caller，而是通过普通 drop 释放尚未 publish 的 launch/control/task draft。

publish 是 create transaction 的 commit 边界。publish 前失败可以 drop unpublished task、control 和 launch payload；publish 后不能再出现需要 recoverable rollback 的步骤，只能执行 infallible enqueue 和 success completion。若 enqueue 前置条件不满足，应通过 assert/panic 暴露实现 bug，而不是把已经 publish 的 kthread 当作普通 spawn failure 回滚。

### Stop request

`request_stop()` 的线性化点是 `KThreadPhase::Running` 变为 `KThreadPhase::StopRequested`，或观察到已经 `StopRequested` / `Exited(_)`。

要求：

1. 多次 request stop 幂等。
2. 已退出 kthread 上 request stop 幂等成功。
3. stop flag 对 entry 的 `should_stop()` 可见。
4. request stop 必须 publish wake event。
5. wake 不得绕过 wait core 或手工写 scheduler state。

### Wake

`wake()` 的线性化点是 publish `wake` event。

要求：

1. wake 不改变 stop/exited/result。
2. wake 不表达 pending work。
3. wake 不读取或修改 consumer business state。
4. wake 只保证 stop-aware wait 有机会重新检查 predicate。

### Exit

kthread exit 的线性化点是 `KThreadPhase` 变为 `Exited(code)`。

建议顺序：

1. entry 返回或调用 `kthread_exit(code)`。
2. 完成 `KThreadPhase::Exited(code)`，重复完成 assert/panic。
3. 执行 task-local resource closeout。
4. 撤销 procfs-visible topology binding，并使 procfs binding 失效。
5. publish `exited` event；这一步是 external exited completion 的可见点，`wait_exited()` 只等待它，不直接观察 `phase`。
6. 进入 scheduler zombie/schedule tail。

exit helper 分三层：

1. user-process cleanup：用户地址空间、clear-child-tid、robust futex、ordinary child-exited、reparent、parent `SIGCHLD`、ordinary wait/reap 和 vfork completion。
2. task-local resource closeout：当前 task 独占资源的释放，包括未来 fd-table / opened-description final release / fanotify cleanup 等需要 sleepable process context 的 closeout。
3. scheduler zombie tail：只设置 scheduler zombie 并 schedule away。

`kthread_exit()` 必须跳过 user-process cleanup，必须保留 task-local resource closeout，然后进入 scheduler zombie tail。第一阶段若禁止 kthread 拥有 fd-table，closeout helper 中必须有 assert 和注释记录该临时边界：kthread fd-table 为空、该假设只服务第一阶段、允许 kthread 继承或打开 fd 前必须把 assert 替换为完整 closeout。

专用 exit 路径不得执行：

- clear-child-tid
- robust futex cleanup
- user address-space cleanup
- thread-group child-exited event
- reparent orphan children
- parent `SIGCHLD`
- ordinary `wait4` / `waitid` reap protocol
- vfork completion

若实现复用底层 helper，helper 名称和注释必须区分 task-local resource closeout 与 scheduler tail，不能把二者合并成“非 user cleanup”黑盒。

## 锁序与生命周期规则

建议锁序：

1. topology unpublish / procfs binding lock
2. `KThreadControl` inner lock or atomics
3. wait source / `Event`
4. task sched-state lock

要求：

1. 不在持有 topology lock 时进入可能阻塞的 wait。
2. 不在持有 wait source lock 时手工 enqueue。
3. `wait_exited()` 不持有 topology lock 等待 event。
4. exit path 先执行 task-local resource closeout，再通过 topology unpublish transaction 撤销可见发布状态，最后进入 scheduler zombie tail。
5. topology unpublish transaction 是 procfs binding invalidation 的唯一 owner；procfs lookup 在该 transaction 标记 unpublishing/exited 后不得创建新 binding。
6. procfs binding invalidation hook 不得回调需要反向获取 topology owner lock 的路径；如果实现需要跨层调用，必须保持上述锁序并在代码注释中说明。
7. cleanup / drop 路径先释放订阅或撤销发布，再用 assert 暴露 bug，避免 panic 放大泄漏。

## Entry payload 边界

entry payload 必须满足：

1. public API 形态为 `KThreadEntry = fn(KThreadCtx, AnyOpaque) -> i32`。
2. public API 不暴露 raw entry pointer、`ParameterList` 或 erased payload pointer。
3. kthread start payload 不通过 `ParameterList` 或 raw pointer 传递。
4. entry shim 是 monomorphic 入口，只从 current task 的 launch slot `take()` 出 `KThreadLaunch`。
5. launch slot 只能被 take 一次；重复进入、缺失 launch 或在非 ordinary kthread 上运行 entry shim 必须 panic。
6. kthread core 不 downcast `AnyOpaque`；consumer entry/helper 拥有 payload concrete type 解释。
7. create failure 通过普通 Rust ownership drop 尚未 publish 的 `KThreadLaunch`，不需要 `KThreadStart<A>` / `KThreadStartToken` reclaim state machine。

entry 返回的 `i32` 是 kthread-local completion code，不是 syscall errno contract，也不默认等价 `SysError`。kthread-core 只保存和返回它，不解释其业务含义。

closure API 或泛型 owned payload API 是 optional follow-up，不改变第一阶段 `AnyOpaque` start argument contract。

## Consumer 约束

当前 consumer 必须遵守：

1. inode shrinker 使用 explicit loop，业务 truth 是 frame stats 和 VFS cache state。
2. OOM killer 使用 explicit loop，业务 truth 是 threshold、active victim 和 mm snapshot。
3. consumer 可以调用 `wake()` 提示重查，但不能把 wake event 当 pending truth。
4. consumer 不直接调用 `Task::new_kernel()`。
5. consumer 不持有 raw `Arc<Task>` 作为 kthread lifecycle handle。

## Initcall 边界

要求：

1. `kthreadd` 初始化是 boot invariant，必须手动放在 `init` topology 可用之后、任何 ordinary kthread spawn 之前。
2. `kthreadd` 不走 initcall。通用 initcall level 不应隐藏 fixed TID anchor、ordinary kthread parent anchor 和 spawn legality 的顺序约束。
3. ordinary kthread consumer 由所属子系统自己的 init path 启动。
4. initcall level 表达初始化时刻，不表达 “是否会 spawn kthread”。
5. 若需要把当前手写 consumer 初始化迁入 initcall，应新增通用 `Late` level，而不是 kthread-specific level。
6. `Late` 的语义是：基础 filesystem / driver / probe 初始化完成，`kthreadd` 已存在，所有 CPU 已完成 local init，用户 init 尚未 exec。

## 禁止退化项

以下模式会破坏本文：

1. kthread 通过未区分 type 的 `TaskBinding::Leader` 进入普通 user-process topology。
2. kthread 加入普通 process group / session 成员集。
3. ordinary kthread 先切到 kthread-aware binding，却仍调用完整 `kernel_exit()` 或缺少 topology/procfs unpublish。
4. kthread exit 跳过 task-local resource closeout，或把 fd-table 永远为空的阶段假设留成无 assert / 无退出条件的隐式事实。
5. procfs binding 能在 topology unpublish 后残留或被 lookup 反向重建。
6. procfs inert display helper 被 direct `getpgid` / `getsid`、signal permission 或 job-control 路径当成行为 truth。
7. kthread 保留 user-visible zombie，等待普通用户进程 reap。
8. external lifecycle handle 是 weak-only，导致 owner 无法稳定等待 exit result。
9. core 内建 `KThreadService`、pending backend、drain、worker count 或 request queue。
10. core 保留 park/unpark 状态机。
11. 新增独立 registry 或 `KThreadId` 作为 active identity truth。
12. `kthreadd` 变成 workqueue/service dispatcher。
13. `kthreadd` TID 2 依赖启动顺序碰巧成立。
14. `wake()` 被解释为业务 request truth。

## 完成标准

文档协议闭合需要：

1. 本文 topology、handle、exit、wake、unsafe entry、consumer 约束均已 review。
2. [Tracking Issues](./tracking-issues.md) 中没有未分类的 Keter 设计问题。
3. [迁移实施计划](./implementation.md) 明确阶段顺序、write set、验证 floor 和停止边界。

实现闭合需要：

1. `kthreadd` 稳定为 TID/TGID 2。
2. kthread-aware topology type 落地，且 kthread 不加入 PG/session。
3. `/proc` 能枚举 active kthread，status/cmdline/stat 符合本文第一版语义。
4. strong handle/control 替代 weak-only lifecycle handle。
5. `KThreadService` 和 park/unpark 从 core contract 与当前 consumer 中移除。
6. 专用 `kthread_exit()` 路径落地，源码审计确认不经过 user-process cleanup。
7. smoke 覆盖 spawn、wake、request_stop、entry return、wait_exited、already-exited stop 和 procfs visibility。
