# RFC-20260714-sched-dynamic-attributes

**状态：** Accepted for Implementation
**修订：** R0
**负责人：** doruche, Codex
**最后更新：** 2026-07-15
**领域：** scheduler / dynamic attributes / syscall ABI / IPI / affinity
**事务日志：** [2026-07-15-sched-dynamic-attributes](../../devlog/transactions/2026-07-15-sched-dynamic-attributes.md)
**开放问题：** None；已关闭问题见 [Tracking Issues](./tracking-issues.md)
**下层问题：** [KETER-WAIT-001：synchronous remote placement 不能组合进 cross-CPU IPI completion](../sched-wait-refactor/tracking-issues.md#keter-wait-001synchronous-remote-placement-不能组合进-cross-cpu-ipi-completion)
**下一步：** 由 [R0 事务日志](../../devlog/transactions/2026-07-15-sched-dynamic-attributes.md) 记录当前 handoff；后续 checkpoint 继续服从 [迁移实施计划](./implementation.md) 的阶段顺序、write set 与独立 review gate

## 摘要

本 RFC R0 定义 Anemone 第一版动态调度配置事务和对应 Linux syscall 观察面。已发布 task 的 policy、policy parameters、nice、`reset_on_fork` 与 affinity 不再允许通过 TCB 字段旁路修改；所有会影响调度行为的修改都转换为 scheduler-owned `SchedConfigPatch`，并在 task 的固定 owner CPU 上由 `RunQueue` transaction 串行提交。

远端 setter 使用现有 per-CPU IPI queue 传递 `Arc<SchedRequest>`。IPI transport 是异步的，但 syscall 对用户态保持同步：调用 task 创建新的 dormant `sched::oneshot::channel::<T>()`，只有结果尚未发布时，`recv_uninterruptible()` 才建立 wait 并 park；owner CPU 写入结果并完成 one-shot 后 syscall 再返回。wait-core `Force` 只结束当前 receive-local Latch round；receiver 在 channel 仍 empty 时内部 rearm，不把 Force 暴露成 channel error。所有 remote setter 在 request 发布前获取 `sched/api` 私有的全局 `Mutex<()>` `REMOTE_SCHED_REQUEST_GATE`，并持有到 `recv_uninterruptible()` 观察真正 terminal phase 后返回，使任意时刻最多只有一个仍持有开放 receiver、其 completion 仍可能进入 wait-core placement 的 remote scheduler request；该 gate 只约束 syscall producer graph，不是 RunQueue transaction lock，也不建立 multi-target 原子性。

getter 不发送 IPI。它们在 `SchedEntity` 的一致性域内获取 coherent `SchedConfig` snapshot，再投影为对应 Linux ABI；snapshot 可以在返回后立即变旧，不承诺与并发 setter 建立全局读屏障。

本 RFC R0 还定义 fixed-CPU 架构下的 affinity 兼容语义：真实保存 allowed CPU mask，但只有包含 task 当前固定 `cpuid` 的 mask 才能成功；需要迁移的请求返回 `EINVAL`。主动迁移、load balancing 和 CPU hotplug 不在第一版范围内。

## 文档权威与下层合同

本目录是 dynamic scheduler attributes R0 的公开 canonical source。R0 已接受进入实现；在对应 transaction 完成前，live code、已关闭的下层 RFC 与 register 继续拥有当前实现事实。本 RFC 不引用或依赖任何私有草案。

本 RFC R0 建立在以下当前合同上：

- `RunQueue`、Fair、Realtime 与 Idle 的状态变化只在固定 owner CPU 的 IRQ-off transaction 中发生。
- Fair / Stride 使用 `pass`、`placement_floor` 与 `(pass, enqueue_seq)` 作为 service history 和 ready order。
- Realtime 由共享 priority-first class 承载 FIFO/RR；`remaining_ticks` 是 RR budget，`rotation_due` 是 active RR execution segment 的 committed tail-placement obligation。
- processor-owned `PendingResched` 只表示需要一次 full pick，不携带 class-visible reason。
- wait core 拥有 wait identity、completion、parkability 与 stale-safe physical placement；现有 `Latch` 是其单轮 OR-wait adapter。
- wait core 当前 synchronous remote placement 不能组合进双向 cross-CPU IPI completion；根问题由 [KETER-WAIT-001](../sched-wait-refactor/tracking-issues.md#keter-wait-001synchronous-remote-placement-不能组合进-cross-cpu-ipi-completion) 跟踪，本 RFC 只以 remote request 全局串行 gate 限制自己的 producer graph。
- `Task::cpuid()` 在 task 发布后不可改变，是 owner CPU 的唯一真相源。
- owner-CPU noirq heap allocation 的风险由 [ANE-20260622-IRQ-OFF-HEAP-ALLOCATION](../../register/open-issues.md#ane-20260622-irq-off-heap-allocation) 跟踪；本 RFC R0 将其保持为 accepted limitation，不宣称 allocation-free。

本 RFC 是 [Sched Fair / Stride](../sched-fair-stride/index.md)、[Sched RT Class](../sched-rt-class/index.md) 与 [Sched Latch](../sched-latch/index.md) 的 accepted follow-up：它以 owner-CPU transaction 替代现有 weak nice setter，开放已发布 task 的动态 Fair/RT policy transaction，并复用 Latch 表达 remote request completion。R0 是这些动态属性边界的 accepted successor contract；在 transaction 完成前，已关闭 RFC 与 live code 仍拥有当前实现事实。

## 目标

- 建立一个 scheduler-owned、UAPI-independent 的稳定配置模型与原子 patch 模型。
- 删除 `AtomicNice`；只保留 typed `Nice`，并把唯一 nice truth 放入 `SchedEntity` 的 generic configured attributes。
- 允许已发布 task 在 Fair、RT/FIFO 与 RT/RR 之间动态切换，并动态修改 nice、RT priority、`reset_on_fork` 与 affinity。
- 保持 Fair pass、RT priority bucket、RR budget 与 rotation obligation 的既有 owner boundary。
- 将所有调度相关 UAPI 统一放在 `sched/api`，包括现有 `getpriority()` / `setpriority()`。
- 提供 Linux `sched_*`、priority 与 affinity syscall 的稳定 read-back、权限和错误边界。
- 使用 async IPI + parked one-shot completion 表达远端同步事务，不忙等。
- 保持 local setter 与 remote setter 共享同一个 `RunQueue` transaction。
- 为 current、queued、detached 与 zombie target 给出完整的 mutation matrix。
- 固定 clone/fork 的 configured attribute 继承与 `SCHED_RESET_ON_FORK` 语义。

## 非目标

- 不实现 task migration、跨 CPU load balancing、CPU hotplug 或 NUMA placement。
- 不实现 `SCHED_BATCH`、`SCHED_IDLE`、`SCHED_DEADLINE`、deadline bandwidth 或 RT bandwidth control。
- 不实现 `RLIMIT_NICE` / `RLIMIT_RTPRIO` 尚不存在的资源限制机制；在这些机制落地前使用明确的 `CAP_SYS_NICE` 收窄策略。
- 不实现 priority inheritance、PI boost 与用户配置 priority 的联合事务。
- 不把 synchronous IPI 改造成通用 sleeping RPC，也不引入第二个 per-CPU scheduler request queue。
- 不修复 wait core 的 synchronous remote placement contract；本 RFC 的全局 remote request gate 是带退出条件的 syscall-domain 局部约束。
- 不把 `sched::oneshot` channel 扩展成多 sender、多 receiver、interruptible receive、timeout、`try_recv` 或可复用 channel。
- 不重算 Fair 历史 pass，不为离开 Fair 的 task 保存 dormant Fair payload。
- 不保存离开 RR 后的 dormant RR budget 或 rotation state。
- 不修复 register 中已有的 IRQ-off heap allocation风险，也不把现有 IPI allocation 的 fatal OOM 改造成可恢复错误、预留消息或 rollback 协议。
- 阶段 0 只完成文档接受、live-source audit、transaction 与导航，不修改 kernel code，也不运行 runtime 测试；阶段 1 及后续实现严格服从 `implementation.md` 的 planned gates。

## 文档地图

Canonical R0 contract：

- [不变量需求](./invariants.md)
- [迁移实施计划](./implementation.md)
- [Tracking Issues](./tracking-issues.md)
- [背景材料索引](./backgrounds/index.md)

`tracking-issues.md` 只保留当前 RFC 自己拥有的设计问题和已经 neutralize 的 review 记录；共享 wait-core 根问题以公开 RFC tracker 为 canonical owner。Linux/LTP 的逐项证据由背景材料保存，不覆盖本页和不变量页的 proposed contract。

## 修订记录

| 修订 | 日期 | 状态 | 语义变化 | Review / 事务 |
| --- | --- | --- | --- | --- |
| R0 | 2026-07-15 | Accepted for Implementation | 初始 accepted contract：统一 `SchedConfig` / patch、owner-CPU reconfigure、persistent-phase one-shot、remote request gate、fixed affinity 与 Linux scheduler UAPI 子集。 | [R0 事务](../../devlog/transactions/2026-07-15-sched-dynamic-attributes.md) |

## 子系统 owner

数据寄存在 TCB 不决定语义 owner。`Task` 是与 task 同生命周期对象的聚合容器；调度配置的解释、修改和 runtime 迁移仍属于 scheduler。

| 组件 | Owner | 职责 |
|---|---|---|
| `Task` / task topology | task | task identity、lifetime、TID/thread-group/process-group/user selection，以及 scheduler storage shell |
| `SchedEntity` / `SchedConfig` / patch / snapshot | sched | configured attributes、class-private runtime、验证与唯一真相源 |
| `RunQueue` reconfigure transaction | sched | published task 的全部配置修改、membership、runtime transition 与 resched request |
| `sched/api` | sched | 全部调度 UAPI 的解析、目标选择编排、permission snapshot、ABI 投影、multi-target result folding，以及 remote request submission gate |
| `task/api/clone` | task | clone 生命周期；通过 sched 的 unpublished-task 接口请求继承或 reset 后的新实体 |
| `exception/ipi` | exception | request transport 与 `sched::handle_request()` 转发；不解释 patch 或修改 entity |

`task/sched.rs` 可以继续保存因 `Task` 私有字段所需的窄 storage bridge 和 observation accessor，但不能提供 `set_nice()`、replace policy 或其它 published-task mutation bypass。普通 crate caller 不能构造 entity mutation capability。

`getpriority()` / `setpriority()` 搬入 `sched/api`。`PRIO_PROCESS`、`PRIO_PGRP` 与 `PRIO_USER` 的 selector 和 target snapshot 只存在于 ABI adapter；scheduler core 不感知 Linux selector。

## 稳定配置模型

内部模型按 scheduler 语义命名，不镜像 Linux syscall：

```rust
struct SchedConfig {
    discipline: SchedDiscipline,
    nice: Nice,
    reset_on_fork: bool,
    affinity: CpuMask,
}

enum SchedDiscipline {
    Fair,
    Realtime {
        mode: RtMode,
        priority: RtPriority,
    },
}

enum RtMode {
    Fifo,
    RoundRobin,
}
```

`SchedConfig` 是 stable configured snapshot，不包含以下 class-private runtime：

- Fair pass、fresh/placed state、heap snapshot 或 enqueue sequence；
- RR remaining quantum 与 `rotation_due`；
- `on_runq`、current identity、pending resched 或 wait state。

`nice` 只有 Fair class 可以消费，但它是跨 discipline 持久保存的 generic configured attribute。RT task 可以修改和读取 dormant nice；切回 Fair 时继续使用该值。RT 与 Idle 不得根据 nice 改变行为。

`SchedEntity` 是 configured attributes、class-private payload 与 physical membership metadata 的唯一承载体。实现可以决定是否把 generic fields 物理嵌入一个 `SchedConfig` 子对象，但不能缓存第二份 policy、priority、nice 或 affinity。

## 语义 patch

所有 setter 最终汇合到一个 `ApplyConfigPatch` transaction：

```rust
struct SchedConfigPatch {
    discipline: DisciplineChange,
    nice: Option<Nice>,
    reset_on_fork: Option<bool>,
    affinity: Option<CpuMask>,
}

enum DisciplineChange {
    Keep,
    Replace(SchedDiscipline),
    ReconfigureParameters(SchedParameters),
}

enum SchedParameters {
    Fair,
    Realtime { priority: RtPriority },
}
```

- `Keep` 不改变 policy 或 policy parameters。
- `Replace` 安装一份完整、已 typed-validate 的 configured discipline。
- `ReconfigureParameters` 保持当前 discipline，仅更新与当前 policy family 相匹配的参数；不匹配返回 `EINVAL`。
- owner CPU 基于 transaction 开始时的最新 entity 解析 patch，不允许 syscall 先做 snapshot read-modify-write 后提交完整旧配置。
- exact no-op 必须被识别为 no-op，不得改变 runtime、membership、queue order 或 pending resched。

raw Linux `sched_param`、`sched_attr`、policy constants、selector 与 flags 不进入 scheduler core。

## 支持的 syscall 与 policy

第一版覆盖：

- `sched_setscheduler()` / `sched_getscheduler()`；
- `sched_setparam()` / `sched_getparam()`；
- `sched_setattr()` / `sched_getattr()`；
- `sched_get_priority_min()` / `sched_get_priority_max()`；
- `sched_rr_get_interval()`；
- `setpriority()` / `getpriority()`；
- `sched_setaffinity()` / `sched_getaffinity()`；
- 保留既有 `sched_yield()`，但它不是配置 transaction。

policy mapping 固定为：

| Linux policy | 内部 discipline | 结果 |
|---|---|---|
| `SCHED_OTHER` | `Fair` | 支持，RT priority 必须为 0 |
| `SCHED_FIFO` | `Realtime { Fifo, priority }` | 支持，priority 必须在 `1..=99` |
| `SCHED_RR` | `Realtime { RoundRobin, priority }` | 支持，priority 必须在 `1..=99` |
| `SCHED_BATCH` | 无 | 明确拒绝 |
| `SCHED_IDLE` | 无 | 明确拒绝 |
| `SCHED_DEADLINE` | 无 | 明确拒绝 |

未知 policy、非法 policy/parameter 组合和不支持的 `sched_attr` feature 不得静默映射到 Fair 或 RT。逐 syscall layout、size、pointer ordering、errno precedence 与 LTP/POSIX evidence 见 [Linux 6.6 Scheduler UAPI Matrix](./backgrounds/linux-6.6-sched-uapi-matrix.md)；下列规则是 R0 binding ABI boundary。

### R0 UAPI boundary

`sched_attr` 使用 Linux 6.6 layout：

- `SCHED_ATTR_SIZE_VER0 = 48`，`SCHED_ATTR_SIZE_VER1 = 56`，Anemone advertised known size 固定为 56；
- setter 的 `size == 0` 按 48；`size < 48`、`size > PAGE_SIZE` 或大于 56 的 nonzero unknown tail 返回 `E2BIG` 并尝试把 56 写回 `size`；
- 大于 56 的 zero tail 向前兼容，短于 56 的 known prefix 后补零；
- effective size小于56且携带util-clamp flag时，在target lookup前返回`EINVAL`，但这不表示R0支持该feature；
- getter 接受 `48..=PAGE_SIZE`，复制 `min(usize, 56)` bytes，并把输出 `size` 设为同一值；大于 56 的用户 tail 不修改。

R0 的 syscall-level `flags` 只接受 0；`sched_attr.sched_flags` 只接受 `SCHED_FLAG_RESET_ON_FORK`。reclaim、deadline overrun、keep-policy、keep-params、util-clamp 和未知 flags 全部返回 `EINVAL`。特别地，R0 不支持 `SCHED_FLAG_KEEP_PARAMS`：它需要新的 owner-side “replace policy while keeping latest parameters” semantic patch，不能由 syscall 读取 stale snapshot 拼出完整 config。

legacy reset encoding 只属于 `sched_setscheduler()`：

- policy 中的 `SCHED_RESET_ON_FORK` 被剥离后再解析 policy，flag 值进入 `reset_on_fork` patch；
- 不携带该 bit 表示请求清除 reset；无 privilege caller 不能清除已置位 flag；
- `sched_setparam()` 保持现有 reset；
- `sched_getscheduler()` 返回 policy OR legacy reset bit；
- `sched_setattr()` / `sched_getattr()` 使用 `SCHED_FLAG_RESET_ON_FORK`。

Field consumption固定为：

| Entry | Discipline patch | Nice patch | Reset patch |
|---|---|---|---|
| `sched_setscheduler()` | replace requested policy | preserve dormant nice | legacy bit 的 bool |
| `sched_setparam()` | reconfigure latest discipline parameters | preserve | preserve |
| `sched_setattr(OTHER)` | replace Fair | clamp 后的 `sched_nice` | attr reset bit 的 bool |
| `sched_setattr(FIFO/RR)` | replace RT mode/priority | preserve dormant nice；inactive attr nice忽略 | attr reset bit 的 bool |
| `setpriority()` | keep | clamp 后的 nice | preserve |
| `sched_setaffinity()` | keep | preserve | preserve |

在 supported policy 下，inactive deadline tuple 按 Linux 行为忽略且不回显；选择 `SCHED_DEADLINE` 或使用对应 feature flag 仍为 `EINVAL`。raw Linux fields 到上述 semantic patch 的转换只能存在于 `sched/api`。

`sched_get_priority_{min,max}()` 是 policy-domain 静态查询：

| Policy | Min | Max |
|---|---:|---:|
| FIFO / RR | 1 | 99 |
| OTHER / BATCH / IDLE / DEADLINE | 0 | 0 |
| unknown 或附带 reset bit | `EINVAL` | `EINVAL` |

对 unsupported setter policy 返回 0 不表示该 policy 可以安装。

`sched_rr_get_interval()` 从 coherent discipline snapshot 与 class-owned configured interval 投影，不读取 ready queue或 class-private runtime：

| Discipline | Observable interval |
|---|---|
| Fair/Stride | 一个 effective scheduler tick |
| RT/FIFO | zero timespec |
| RT/RR | configured full effective quantum |

RR 不返回 `remaining_ticks`，也不根据 peer、`rotation_due` 或当前 segment 改变。Fair 的一个 tick对应现有 fixed-tick Stride service unit；不得复制 Linux CFS 的 load-dependent queue observation。

## Getter snapshot 语义

单 task getter 在 `SchedEntity` 的一致性域内一次性复制 `SchedConfig`，释放 guard 后完成用户态投影：

- snapshot 中 policy、RT priority、nice、reset flag 与 affinity 必须来自同一个 entity 版本；
- owner transaction 必须以一次 entity configured-view publication 使 old config 切换为 new config，getter 不得观察逐字段 patch 中间态；
- getter 不发送 IPI，不等待 owner CPU，不读取 class queue；
- runtime state 不进入公开 snapshot；
- snapshot 返回后可以立即 stale；
- target lookup 使用 strong `Arc<Task>`，避免读取期间释放或 TID reuse。

`getpriority()` 的 target set 是 task-topology snapshot，各 target 的 nice 是独立 coherent snapshot；不提供跨 target 全局原子性。`setpriority()` 也按 target 顺序提交独立 owner transaction，保留 Linux multi-target partial-progress/result-folding 语义，不建立跨 CPU two-phase commit。

ABI projection固定为：

| Getter | Fair | RT/FIFO 或 RT/RR |
|---|---|---|
| `sched_getscheduler()` | OTHER + legacy reset bit | FIFO/RR + legacy reset bit |
| `sched_getparam()` | priority 0 | configured RT priority |
| `sched_getattr()` | policy OTHER、configured nice、priority 0、reset attr flag | policy FIFO/RR、nice 0、configured priority、reset attr flag |
| `getpriority()` | configured nice | dormant configured nice |

`sched_getattr()` 的 deadline/util fields固定为 0。RT dormant nice属于同一 coherent internal snapshot，但不是 RT active parameter，只通过 `getpriority()` 投影。

## Permission snapshot 与窄 permit

IPI hardirq handler 不获取 caller/target credential lock。syscall adapter 在提交前：

1. snapshot caller 与 target credentials；
2. 完成 target ownership、effective UID 与 `CAP_SYS_NICE` 等身份检查；
3. 生成不含 UID/capability 的窄 `SchedChangePermit`；
4. 将 patch 与 permit 一起提交。

第一版区分 unrestricted 与 non-escalating capability。没有 `RLIMIT_NICE` / `RLIMIT_RTPRIO` 时，same-owner non-escalating permit允许 numeric nice增大或不变、同一 RT mode 内 priority降低或不变、RT退出到Fair、设置reset和exact no-op；它不能降低numeric nice、进入RT、切换FIFO/RR、提高RT priority或清除已置位reset。`CAP_SYS_NICE` 生成unrestricted permit。

owner CPU 根据 transaction 开始时的最新 `SchedConfig` 判断具体 old -> new transition 是否落在 permit 内，因此并发 setter 不能通过 stale nice/priority snapshot 绕过 privilege boundary。owner-side denial返回typed internal error，由 `sched/api` 按入口映射：`setpriority()` nice escalation为 `EACCES`，其它scheduler transition denial为 `EPERM`。wrong-owner与affinity permission denial也为 `EPERM`。

credential authorization 采用提交时 snapshot 语义。请求发布后 target credential 变化不撤销已发布请求。scheduler core 不读取或解释 `CredentialSet`。

## `sched::oneshot` channel

新增 scheduler-layer dormant one-shot value channel。endpoint 创建本身不读取 current task、不发布 `TaskSchedState::Waiting`，也不建立 `Latch`；只有 receiver 真正发现 channel 仍 empty 并准备阻塞时，才为调用 `recv_uninterruptible()` 的 current task 建立一轮 wait：

```rust
let (sender, receiver) = sched::oneshot::channel::<T>();

sender.send(value)                  // consumes Sender<T>, returns Result<(), T>
receiver.recv_uninterruptible()     // consumes Receiver<T>, returns Result<T, RecvError>
```

v1 contract：

- `Sender<T>` 可以移动到远端 CPU，不能 clone；`send(self, T)` 最多发布一次。
- `Receiver<T>` 不能 clone，可以在调用 receive 前移动；`recv_uninterruptible(self)` 消耗 endpoint，并仅在该调用内部把实际 wait round 绑定到调用时的 current task。
- receive 不可被普通 signal 中断，无 timeout。
- `send(self, value)` 不等待；receiver 已关闭时返回 `Err(value)`，成功只证明 value 已发布，不保证 receiver 最终消费。
- channel phase 持久保存 payload/closed/consumed truth，因此 send-before-recv 不需要预先注册 waiter；receiver 后来以 Acquire 直接取得 value 或 closed。
- `RecvError` 在 v1 只包含 `SenderClosed`；wait-core outcome 不进入 channel error surface。
- `recv_uninterruptible()` 先做 terminal fast path；仍 empty 时才 begin `Latch`，随后在 channel 的 hardirq-safe state lock 下重查 phase并安装唯一 trigger。若 send/close 已在 begin 与注册之间发生，receiver 在锁外 drop 未安装的 trigger，cancel + finish 本轮 latch，再由 persistent terminal phase 决定 no-switch 返回；即使 Force 已抢先完成该 round，也不能覆盖 terminal result。
- sender 在同一个 channel state transaction 中写 payload或closed、Release 发布 phase并 detach 已注册 trigger；释放 channel lock 后才能 trigger，不能持 channel lock 进入 wait core。
- channel phase 只拥有 value、endpoint lifecycle和当前 receiver registration；wait core拥有 wait identity、completion、park/wake与stale-safe placement。registration 不是第二份 task wait truth。
- registered wait 返回后，receiver 在 channel lock 下 take 仍存在的旧 trigger，锁外 drop，再 finish 当前 Latch并重查 phase。terminal phase 直接返回；phase 仍 empty 时只允许 Latch outcome 为 `Force`，随后建立下一轮 Latch。其它 empty outcome 是常开断言级 bug。
- receiver 在 receive 前 drop 只关闭 endpoint并exactly-once drop pending payload，不存在要cancel的active wait；`recv_uninterruptible()` 内部创建的每一轮 Latch 都必须在 rearm 或返回前显式 finish。
- sender drop without send 持久发布closed；已注册waiter会被trigger，尚未receive的receiver稍后直接观察closed。
- 不提供多 producer、OR wait、timeout、interruptible receive、cross-round permit 或 channel reuse。

现有 cloneable `LatchTrigger` 继续服务 multi-producer readiness hint；`oneshot` 只在 receive-time registration 内保存一个 trigger，并通过不向外暴露它建立 single-producer value-transfer contract，不改变 `Latch` 的 OR-wait API。

one-shot 的阻塞 adapter 明确选择 receive-local `Latch`，不使用 `Event`。`Event::listen_uninterruptible()` 在 Force 后重试 predicate 的方向与本 channel 一致，但其 API 要求调用时不持有 lock/guard，而 remote setter 刻意跨 receive 持有唯一的 `REMOTE_SCHED_REQUEST_GATE`。`Event` 还维护独立的可复用 listener `VecDeque`、exclusive/non-exclusive 与 quota policy；首次 listener 注册可能在 `NoIrqSpinLock` 临界区扩容，并为只有一个 receiver 的 channel 增加第二个同步域。one-shot 已有 hardirq-safe phase lock，只需要同锁保存一个 bounded `Option<LatchTrigger>`，因此直接复用 Latch 更窄，也不扩大 Event 或 IRQ-off allocation contract。dormant constructor 本身不是选择 Latch 的理由；决定因素是单轮 capability、固定 registration slot 与 audited gate lock order。

## Scheduler request 与 IPI transport

`REMOTE_SCHED_REQUEST_GATE` 明确使用现有 `Mutex<()>`，不是一套新的 gate primitive：

```rust
static REMOTE_SCHED_REQUEST_GATE: Mutex<()> = Mutex::new(());
```

现有 `Mutex` 的 fast path 已使用 `AtomicBool` CAS，contended slow path 已由内部 `Event::listen_uninterruptible()` 等待并在 unlock 时唤醒一个 waiter；本 RFC 不重复实现一套 `AtomicBool + Event` gate。Mutex内部 Event 只发生在 request 发布和 Latch 建立之前的 lock acquisition；它与 one-shot 都不把 Force 暴露成普通返回，但取得 guard 后不再进入该 Event wait。该 `Mutex<()>` 是 `sched/api` 私有的全局、sleepable transaction permit，只覆盖一个 remote request 从发布到 `recv_uninterruptible()` 观察 terminal phase并返回的窗口；local setter、getter、unpublished child construction、IPI handler 与 `RunQueue` transaction 都不获取它。持有者在建立 one-shot `Latch` 前取得 guard，并刻意跨越全部 receive rounds 持有；receive 内不再获取 sleepable lock，handler/completion path 不获取 gate，且 receive 必须 finish 当前 Latch 后才返回并 drop guard，因此不会形成 Event/Latch nested wait。

gate 的精确协议作用是：全系统任意时刻最多存在一个仍持有开放 receiver、其 completion 仍可能调用 `LatchTrigger` 并进入 wait-core placement 的 remote scheduler request，从而排除两个 scheduler request handler 同时完成对方 wait 的双向边。正常 value/close completion 后 request 已 terminal；若 wait-core `Force` 先完成当前 round，receiver 只 detach旧 trigger、finish Latch、重查 phase并在 empty 时 rearm，不关闭 channel或释放 guard。因此 gate release 证明 channel 已经 terminal：value 对应 transaction result，`SenderClosed` 则必须同时证明唯一 sender及其未来 mutation/complete capability 已消失。request envelope 的 transport引用可以在 terminal 后短暂存在，但不能再拥有未消费的 execution body；文档和测试不得把该 gate 误述为所有时序下“物理上最多一个 request envelope in flight”。它不保护 `SchedConfig`，不替代 owner CPU serialization，也不把 multi-target `setpriority()` 变成全局 transaction。

remote setter 流程：

```text
resolve Arc<Task> + parse patch + derive permit
-> release credential / topology / entity / user-memory guards
-> acquire REMOTE_SCHED_REQUEST_GATE in task context
-> create dormant sched::oneshot::channel<Result<(), SchedError>>
-> build Arc<SchedRequest>
-> send_ipi_async(owner_cpu, IpiPayload::SchedulerRequest(request))
-> recv_uninterruptible()
-> release REMOTE_SCHED_REQUEST_GATE
-> return transaction result
```

gate 必须在 request 发布前获取，并持有到 `recv_uninterruptible()` 返回。`channel()` 是 dormant constructor，不建立 active wait；`send_ipi_async()` 成功后才调用 receive，receive内部才可能 begin latch。transport 失败路径直接关闭/释放 dormant endpoints，再释放 gate，不需要 cancel/finish 一个不存在的 wait round。禁止在 receive 已经建立 active wait 后获取任何 sleepable mutex。

`Arc<SchedRequest>` 的业务 body 只有一份：

```rust
struct SchedRequest {
    body: NoIrqSpinLock<Option<SchedRequestBody>>,
}

struct SchedRequestBody {
    target: Arc<Task>,
    patch: SchedConfigPatch,
    permit: SchedChangePermit,
    completion: Sender<Result<(), SchedError>>,
}
```

handler 在进入 transaction 前 `take()` body；`Some -> None` 是 request execution capability 的唯一消费点。`Arc` clone 只能延长 envelope lifetime，不能复制 body 或 sender；第二次 execute、空 body 或 double complete 都是常开断言级 kernel bug。

owner CPU：

```text
IPI queue pop
-> exception layer forwards request to sched
-> owner IRQ-off ApplyConfigPatch transaction
-> publish result
-> complete oneshot
```

硬性边界：

- 现有 per-CPU IPI queue 就是唯一 pending transport，不新增 scheduler mailbox。
- `IpiPayload::SchedulerRequest(Arc<SchedRequest>)` 不允许 broadcast。
- `Arc` 只解决 transport/request/task lifetime；single-use body slot 才是 execute/complete capability 的唯一真相源。
- 所有 remote scheduler request 共用一个 module-private `Mutex<()>` submission gate；gate 只在 task context、request 发布前获取，并持有到 `recv_uninterruptible()` 观察 terminal phase后返回。Force 不能关闭 receiver或释放 gate；任意时刻最多一个 published request 仍有开放 receiver并可能触发wait-core placement，IPI handler 和 owner transaction不得获取它。
- multi-target setter可以按每个remote target独立持有gate；不同target之间仍允许partial progress，gate不形成cross-target atomicity。
- exception layer 不解析 patch、permit、target state 或 transaction result。
- local owner request 直接调用同一个 transaction，不发送 self-IPI，也不创建无意义的 wait。
- async IPI 发送失败发生在 request 发布前；调用侧关闭 dormant receiver、释放 gate并返回 transport error，不产生 wait round。
- syscall 对用户态的线性化语义仍是同步的：成功返回证明 owner transaction 已提交，失败返回证明没有半提交状态。
- 除 `REMOTE_SCHED_REQUEST_GATE` 外，不持有 credential、topology、entity、user-memory 或其它 guard 跨越 IPI submit / one-shot wait。
- gate 的移除条件是 wait core 接受并实现 hardirq-safe cross-CPU remote placement contract，且双向 remote setter stress 在移除 gate 后通过；移除前不得把 wait-core Keter 标记为已修复。

## Target identity 与生命周期

syscall adapter 在提交前把 pid/TID 解析为 strong `Arc<Task>`。request 不在 owner CPU 重新按数字 TID 查表，因此：

- target 在等待期间不能被释放或被同号新 task 替代；
- published target 若在 transaction 前进入 zombie，setter 返回 `ESRCH`；
- getter 对已经解析成功但随后退出的 target 返回它取得 snapshot 时仍可观察的配置；
- kernel/idle task 的用户态 setter fail closed，不允许修改特殊 scheduler entity。

当前 `Task::cpuid()` 不可变，所以 v1 请求选定 owner 后不存在 migration forwarding。未来若引入 migration，必须单独扩展 request routing/owner handoff contract，不能在本 RFC 实现中预埋第二 owner truth。

## Owner-CPU physical role matrix

owner CPU 在 local IRQ disabled 的 transaction 内，以 processor current identity、`on_runq` 与 task sched state 将 target 分类：

| 物理角色 | transaction |
|---|---|
| Current | 旧 class 关闭需要结束的 active segment，安装配置/新 payload，再以新配置 attach current；必要时设置 `pending_resched` |
| Queued | 使用旧 payload 从旧 class queue detach，安装配置/新 payload，再按 reconfigure placement 进入目标 class queue |
| Detached | 不做 queue remove；修改配置并完成新 class 的 detached-runtime 初始化。包括 blocked 和 logically runnable 但 physical enqueue 尚未到达的窗口 |
| Zombie | 不修改，返回 `ESRCH` |

`TaskSchedState` 不能单独证明 physical membership。transaction 边界必须保持：

- Current 与 `on_runq` 互斥；
- Queued 只属于一个 class queue，且 `on_runq == true`；
- Detached 不属于任何 class queue；
- class payload、queue key/bucket 与 generic membership 一致。

current reconfigure 不能伪装成 yield、block、wake handoff 或普通 preempt；这些 lifecycle 入口具有不同的 runtime 和 placement 语义。实际 context switch 不在 IPI handler 中执行，只能通过现有 `pending_resched` + scheduler tail 完成。

## Discipline 与 runtime transition

discipline 切换直接构造 fresh class-private payload；不做跨 class runtime conversion，也不保存 dormant payload：

| 配置变化 | runtime 结果 |
|---|---|
| exact no-op | 不改变任何 runtime 或 membership |
| 仅 `reset_on_fork` / affinity | 保留全部 class runtime 与位置 |
| Fair nice 变化 | 保留 pass；后续 service transaction 使用新 nice |
| RT nice 变化 | 只保存 dormant nice，RT runtime 不变 |
| RR priority 变化 | 保留 remaining quantum；若 current 的 active segment 因 reconfigure 结束，则清除旧 `rotation_due` |
| FIFO priority 变化 | 只改变 RT priority |
| FIFO -> RR | fresh RR payload：full quantum，`rotation_due = false` |
| RR -> FIFO | fresh FIFO payload：丢弃 remaining quantum 与 rotation state |
| Fair -> RT | fresh RT payload：丢弃 Fair pass |
| RT -> Fair | fresh Fair payload：丢弃 RT runtime，由 Fair owner 按 placement floor 初始化 pass |

policy/class payload 只能在 task 已从旧 physical class role detach 后替换。queued task 不能在旧 heap/bucket 仍引用其旧 key 时直接换 payload；current task 不能在旧 class 仍记录 active identity 时直接换 payload。

## Queued placement

| queued 配置变化 | post-state placement |
|---|---|
| exact no-op、仅 reset flag、仅 affinity | 不 detach，保持原位置 |
| Fair nice | pass 与现有 heap entry 不变；nice 不是 heap key，不重新排队 |
| RT priority 提高 | 目标 priority bucket 尾部 |
| RT priority 降低 | 目标 priority bucket 头部 |
| FIFO <-> RR，priority 不变 | 同 priority bucket 尾部 |
| Fair -> RT | 目标 RT bucket 尾部 |
| RT -> Fair | fresh pass 取当前 Fair placement floor，并取得新的 enqueue sequence |

该规则防止通过重复修改参数在同 priority bucket 插队，并保持 Linux RT reconfiguration 的 raise-to-tail / lower-to-head 基线。

blocked RT -> Fair 也必须在 owner transaction 中从 Fair placement floor 建立合法 placed pass，使未来 `enqueue_woken()` 不会遇到 fresh/uninitialized payload。该 follow-up 会扩展现有 Fair RFC 的“只有 enqueue_new 初始化 pass”边界；扩展入口必须仍是 owner-CPU class transaction，不能成为普通 task setter。

## Current reschedule

current task 的 patch 若改变当前 class 实际消费的调度维度，transaction 完成后无条件设置 processor-owned `pending_resched`，由 scheduler tail 做一次 full pick：

- Fair current 的 nice 变化；
- RT current 的 priority 或 FIFO/RR mode 变化；
- 任意 discipline/class 切换。

以下变化不结束 active segment，也不请求 resched：

- exact no-op；
- reset flag；
- 包含 current fixed CPU 的 affinity 变化；
- RT current 的 dormant nice 变化。

full pick 可能最终仍选择原 current；该有限开销优先于在 IPI handler 复制 class candidate/preemption decision。

## Fixed-CPU affinity

affinity 是真实 configured state，不是无存储的成功模拟：

- 新用户 task 默认 affinity 为全部 online CPU；
- `SchedEntity` 保存唯一 effective affinity mask；
- 永久不变量为 `task.cpuid() in task.affinity`；
- `sched_getaffinity()` 返回保存的 effective mask；
- `/proc/<pid>/status` 的 `Cpus_allowed` / `Cpus_allowed_list` 使用同一 snapshot；
- fork/clone 继承 affinity，child fixed CPU 必须从继承 mask 中选择。

`sched_setaffinity()`：

1. 先复制 `min(len, KERNEL_CPU_MASK_BYTES)`；短输入零扩展，长输入高 tail 忽略，copy failure先于target lookup返回 `EFAULT`。
2. `pid == 0` 选择current；affinity入口的negative/missing pid按Linux返回 `ESRCH`。
3. lookup与permission后，将mask与当前online CPU domain取交集；超出kernel CPU domain的高bits不形成虚假CPU。
4. 交集为空，返回 `EINVAL`。
5. 交集不包含 target 当前 immutable `cpuid`，说明请求需要 migration；v1 返回 `EINVAL` 并提供可诊断日志。
6. 交集包含 current `cpuid`，由 owner transaction 保存完整交集并返回成功；不触发 migration 或 resched。

mask 中可以包含多个 CPU。允许集合不要求 scheduler 实际轮转 task；task 固定停留在集合内的 owner CPU 仍满足 affinity contract。

`sched_getaffinity()` 在 lookup 前要求 `len` 足以覆盖 kernel CPU domain且按 native `unsigned long` 对齐；raw syscall成功返回实际复制的 `min(len, KERNEL_CPU_MASK_BYTES)`，不是0。target lookup发生在copy-out前，因此missing target优先 `ESRCH`，只有存在target的坏输出地址返回 `EFAULT`。

## Fork / clone 与 reset-on-fork

没有 reset flag 时，child 继承 parent 的 configured discipline、nice 与 affinity，但所有 class-private runtime 都是 fresh：

- 不继承 Fair pass；
- 不继承 RR remaining quantum 或 `rotation_due`；
- 不继承 queue membership、current identity 或 pending resched。

设置 `SCHED_RESET_ON_FORK` 时采用 Linux 语义：

- parent config 与 parent flag 不因一次 fork 改变；
- parent 为 FIFO/RR 时，child 变为 Fair、nice 0；
- parent 为 Fair 且 nice < 0 时，child 保持 Fair、nice 置 0；
- parent 为 Fair 且 nice >= 0 时，child 继承 nice；
- child 继承 affinity，并从 mask 内选择 fixed CPU；
- child 自身的 reset flag 清零；
- 无 privilege 的 caller 不能清除已经置位的 reset flag。

上述构造发生在 child 尚未 publish 时，不使用 IPI 或 published-task transaction；但 fresh payload 必须由对应 class owner 构造，clone code 不复制或解释 private runtime。

## Failure atomicity 与 allocation boundary

所有可恢复、可能返回用户错误的步骤应尽量发生在 detach 旧 membership 之前：

- user pointer 与 UAPI size/flag 解析；
- policy/parameter/range validation；
- target/permission/permit validation；
- affinity normalization；
- request、oneshot 与首次 request IPI message allocation；
- 能够预先确定的 class transition preparation。

一旦 owner transaction detach 旧 physical role，就不得因普通用户输入返回错误并留下半提交 entity。若未来 class attach 引入显式 fallible allocation，必须在 detach 前 preflight，或提供在同一 IRQ-off transaction 内可证明的 rollback；不能把 partially detached task 交还 scheduler。

configured-view publication 之前必须完成所有可恢复、会返回普通 transaction error 的检查。publication 之后只允许 owner CPU 在 IRQ-off 下执行不可失败的 physical attach tail；该 CPU 在 tail 完成前不能运行 scheduler，one-shot completion 也只能在 tail 完成后发送。其它 CPU 的 getter 可以在 publication 后看到完整 new config，但 snapshot 不包含 transaction 中间 membership。既有 remote wake allocation 的 fatal OOM 不属于普通 transaction error。

R0 将 IRQ-off path 上的现有内存分配保持为 accepted limitation，但不把它描述为安全证明。既有 IPI message / queue allocation 在 remote wake completion 中失败时继续服从内核的 fatal OOM 边界，不转译为 syscall error，也不要求本 RFC 增加预留消息、rollback 或 allocation-free transport。Fair heap、RT bucket 与其它既有 allocation 风险继续由 register 跟踪；实现不能借本 RFC 扩大为任意日志格式化、task drop、sleepable lock 或无界工作。

## 可观测性

实现期日志/trace/assertion 至少应能回答：

- request id、caller task、target task、owner CPU 与 local/remote path；
- patch 的语义维度与 permit 类别，不打印无必要的 credential 内容；
- target physical role与 old/new discipline；
- queued detach/attach class、RT bucket placement 或 Fair pass initialization；
- current reconfigure 是否请求 full pick；
- one-shot send、closed、Force round cleanup/rearm、consume与endpoint close时的exactly-once payload drop；
- receive terminal fast path、send-before-receive、send-between-begin-and-register与send-after-registration路径；
- remote request gate 的 acquire、release、owner task、target CPU 与 contention；
- request transport failure、zombie rejection 与 migration-required affinity rejection；
- transaction 成功返回时不存在 duplicated/missing membership。

诊断字段和日志不得参与 transaction、placement、permission 或 completion 决策。

## 接受边界

R0 已按以下边界接受为 `Accepted for Implementation`：

1. `index.md` 与 `invariants.md` 对 owner、配置模型、oneshot、IPI、snapshot、permission、role matrix、runtime、placement、affinity 与 fork 语义一致。
2. [Linux 6.6 Scheduler UAPI Matrix](./backgrounds/linux-6.6-sched-uapi-matrix.md) 已补齐对应 syscall 的 flag、size、errno、copy ordering、read-back 与 testcase分类，且没有把 unsupported policy/flag 静默伪装为成功。
3. 文档层 review 没有未关闭的 Apollyon 或 Keter；Euclid 有明确 owner、验证与回写位置。
4. [迁移实施计划](./implementation.md) 给出阶段、write set、probe、验证 floor 与 register 回写路径；R0 transaction 已建立，后续 checkpoint 不得越过其 canonical gate。
5. 本页保持对 Fair dynamic nice/fresh placement、RT dynamic policy 与 Latch completion 的明确 follow-up 关系；R0 已成为这些动态边界的 accepted successor，live 行为仍以 implementation checkpoint 为准。

## 备选方案

### 使用 synchronous IPI

拒绝。当前 synchronous IPI 在 atomic completion flag 上 busy spin。setter syscall 虽然需要同步返回，但等待 task 可以 park，不应占用 CPU 等待远端 transaction。

### async IPI 后轮询 flag

拒绝。它只改变 API 名称，不改变 busy-spin 机制，也无法复用 wait core 关闭 trigger-before-park race。

### 单独增加 scheduler mailbox

拒绝。现有 IPI queue 已经是 per-CPU pending transport。第二个 mailbox 会引入 enqueue/kick/lost-kick 协议，第一版没有 batching/coalescing 需求。

### 在当前 RFC 修复 wait-core remote placement

拒绝。cross-CPU IPI completion 与 synchronous remote placement 的组合缺口由 wait-core owner 跟踪。本 RFC 只用一个 module-private 全局 submission gate 保证自己的 remote scheduler request 不会形成双向 handler 互等；不复制 wait state，不改变 placement contract，也不预埋通用 completion transport。

### channel constructor 预先建立 wait round

拒绝。one-shot payload/closed phase是持久状态，能够自然保存send-before-receive结果。为复用`Latch`而在`channel()`时提前发布`Waiting/PrePark`会让endpoint构造带有隐藏scheduler side effect，并把request构造和IPI submit都扩进pre-park窗口。wait round只在`recv_uninterruptible()`确认phase仍empty后建立，并通过注册前后重查关闭lost wake。

### 使用 `Event` 作为 one-shot receive adapter

拒绝。`Event::listen_uninterruptible()` 在 Force 后重试 predicate 的语义可以作为 one-shot rearm 的参考，但直接调用要求不持有 lock/guard，与 remote setter 跨 receive 持有唯一 submission gate 的 audited exception冲突。Event还维护独立的可复用 listener `VecDeque`、exclusive/non-exclusive 与 quota policy；注册会把潜在扩容带入 `NoIrqSpinLock` 临界区，并为single-consumer channel增加第二同步域。one-shot使用已有phase lock内的bounded trigger slot和receive-local Latch即可完成同样的terminal recheck，不修改Event contract，也不扩大IRQ-off allocation面。

### 自定义 `AtomicBool + Event` remote gate

拒绝。现有 `Mutex<()>` 本身已经提供CAS fast path、Event slow path、owner校验与RAII release；自定义gate只会复制锁状态、wait和unlock协议。除非未来出现现有Mutex无法表达且已由RFC接受的cancel、handoff或诊断语义，否则remote request直接使用`Mutex<()>`，不新增第二套同步原语。

### syscall 枚举进入 scheduler core

拒绝。`SetScheduler` / `SetParam` / `SetAttr` 会把 Linux UAPI 形状固化为 scheduler transaction。所有 UAPI 必须转换为 `SchedConfigPatch`。

### syscall 先读 snapshot 再提交完整 config

拒绝。并发 partial setter 会互相覆盖。owner 必须在 transaction 内把 patch 应用到最新 config。

### 让 task subsystem 拥有调度逻辑

拒绝。TCB storage 是 lifetime composition，不是 semantic ownership。task 只提供身份、拓扑与 storage bridge。

### affinity 只返回成功但不保存 mask

拒绝。该方案破坏 set/get read-back、fork inheritance 与 `/proc` 一致性。v1 保存真实 allowed mask，只拒绝需要 migration 的请求。

### 为每个 class 保存 dormant runtime payload

拒绝。它制造并列历史状态和切换歧义。discipline 切换使用 fresh target payload，generic nice 单独持久保存。

## 风险

- hardirq 中执行配置 transaction 会扩大 IPI handler 工作量；implementation plan 必须限制操作为 bounded owner transaction，并继承 register 的 noirq allocation 风险标注。
- 全局 remote request gate 会串行所有 remote setter，并在单个 request 永不完成时扩大阻塞面；这是为绕开 wait-core Keter 接受的临时 syscall-domain 约束，必须保留 owner/target/contended 诊断和明确移除条件。
- one-shot 的 payload state 与 wait-core completion 若混为一个状态机，会产生 unsafe publication 或 lost wake；不变量文档将两者分离。
- queued entity 在换 payload 前未 detach 会破坏 Fair heap snapshot或 RT bucket identity；必须使用 role-specific transaction。
- stale permission snapshot 若直接决定“是否提高优先级”会被并发 setter 绕过；narrow permit 必须在 owner 上对 latest config 生效。
- fixed affinity 若不保存 mask或 clone 不继承，会产生明显 read-back 偏差；两者属于同一配置合同。
- multi-target `setpriority()` 在中途失败时会有 partial progress；这是明确 ABI 边界，不能被误述为全局 transaction。

## 当前状态

R0 已接受并由对应 transaction 按 checkpoint 推进。阶段执行事实、review、验证、Not Run 项与当前 handoff 只由 [R0 事务日志](../../devlog/transactions/2026-07-15-sched-dynamic-attributes.md) 记录；本页不维护并列进度账本。
