#import "../template/components.typ": *
#import "../template/figures.typ": *

= 任务、进程与执行上下文

#epigraph(attribution: [Butler Lampson, @lampson1983hints])[
  The interface between two programs consists of the set of assumptions that each programmer needs to make about the other program in order to demonstrate the correctness of his program.
]

#thesis[
  Anemone 中的 task 首先是执行上下文的汇合点：它保存身份、用户地址空间句柄、文件表句柄、credentials、signal context、内核栈和架构调度上下文；thread group、process group 与 session 则由中心化的 global task topology 维护，表达 Linux-visible 的进程拓扑关系。这个边界的关键是把内存对象一致性和更高一级的拓扑一致性分开：task 可以被调度、可以被唤醒、可以被 signal 选中，但 TID/TGID/PGID/SID membership 不散落在每个 TCB 里，runnable state 属于 scheduler，阻塞协议属于 wait-core。
]

如果把进程模型只看成 PID 表，很多边界都会变得含糊：`clone3` 像是在复制进程，`execve` 像是在替换地址空间，`exit` 像是在删除 task，signal 又像是在给某个整数发消息。Anemone 更有用的划分是：task 表达一个可运行控制流；内存对象、文件表和 credentials 等状态由各自 owner 保持对象一致性；global task topology 表达 thread group、process group 和 session 之间的系统级关系。它们共同回答“这个执行流是谁、和谁共享、能被谁等待或 signal”，但不回答“下一次在哪个 CPU 上跑”或“怎样安全睡眠”。

== Task 执行上下文

`Task` 的形状故意很厚。它包含 TID/TGID、创建者、用户地址空间、内核栈、文件状态、credentials、signal mask / pending / disposition、robust futex、exit code、vfork completion、FPU 使用状态、CPU usage 与 scheduler context。厚并不意味着它拥有所有语义；更准确地说，`Task` 是若干 owner 边界的汇合点，每个子状态仍有自己的协议。

#listing([`Task` 摘录：执行上下文汇合点中的 scheduler state 只是受限内部字段])[
```rust
pub struct Task {
    tid: NoIrqRwLock<TidRef>,
    tgid: Tid,
    kstack: KernelStack,
    usp: RwLock<Option<Arc<UserSpaceHandle>>>,
    fs_state: Arc<RwLock<FsState>>,
    files_state: RwLock<Arc<RwLock<FilesState>>>,
    cred: RwLock<CredentialSet>,
    sig_mask: NoIrqSpinLock<TaskSigMaskState>,
    sig_pending: NoIrqSpinLock<PendingSignals>,
    sched_ctx: MonoFlow<TaskContext>,
    sched_entity: SpinLock<SchedEntity>,
    sched_state: NoIrqRwLock<TaskSchedState>,
}
```
]

`sched_state` 的存在很容易误导读者，以为 task 是调度状态的 owner。当前代码反而把这条边界写得很明确：`TaskStatus` 只是 observation-only compatibility snapshot，用于 procfs、debug 和一次性状态观察；scheduler、wait、wake、enqueue 路径必须使用 `TaskSchedState` helper 或 wait-core transaction。换句话说，task 保存 scheduler 需要读写的状态槽，但槽内协议由 scheduler/wait-core 维护，不能被普通 task 逻辑随手当作状态机入口。

#invariant[
  在 Anemone 语境中，task 指可被调度的执行实体；process 指用户可见的 thread group 身份与资源共享边界；process group / session 是拓扑选择器。调度可运行性、wait identity 和 wake placement 不归 task topology 拥有。
]

这个设计把 topology、调度和观测拆成不同的状态来源。`Task::cpuid()` 表示当前实现中 task 的 owner CPU，`sched_entity` 由调度类解释，`TaskCpuUsage` 在 switch-in / switch-out / privilege change 时累积 user/kernel runtime；这些都服务调度和观测。但 process topology 不根据这些字段决定成员关系，signal target selection 也不直接操作 run queue。task 是被调度的对象，不是调度策略本身。

#book-figure(
  "../assets/figures/ch03/task-execution-boundary.png",
  [`Task` 提供身份和执行上下文，scheduler / wait-core 拥有 runnable 与 blocking 协议。],
  width: 100%,
)

== Global Task Topology

Anemone 没有把 thread group、process group 和 session 当成每个 `Task` 私有维护的一串派生字段。相反，`task::topology` 维护一个全局 topology owner：`TaskTopologyInner` 同时索引 TID 到 task、TGID 到 thread group、PGID 到 process group、SID 到 session。task publish、unpublish 和 topology membership 变化在这个 owner 内线性化，读者因此可以把它理解成执行实体之上的第二层一致性域。

#listing([`TaskTopologyInner` 把执行实体和 Linux-visible 拓扑关系收束到同一个 owner])[
```rust
struct TaskTopologyInner {
    tasks: BTreeMap<Tid, TaskNode>,
    thread_groups: BTreeMap<Tid, Arc<ThreadGroup>>,
    process_groups: BTreeMap<Tid, Arc<ProcessGroup>>,
    sessions: BTreeMap<Tid, Arc<Session>>,
}

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
]

这里的区别不是“有没有全局表”这么表面。对象一致性回答的是某个 owner 内部的状态是否自洽：地址空间和 backing object 如何处理 fault，文件表和 opened file description 如何保持 fd 语义，credentials snapshot 如何进入权限检查。拓扑一致性回答的是另一个层级的问题：一个新 task 在 publish 后是否同时出现在 TID 索引、thread group membership、parent/children 关系、process group 和 session membership 中；一个退出中的 thread group 又是否只留下可等待、可回收而不会继续参与目标选择的拓扑状态。

#invariant[
  `Task` 可以保存 `tgid` 这类身份引用，但 thread-group membership、process-group membership、session membership 和 user/kernel topology shape 的真相源是 global task topology。执行上下文里的标志位只能服务快速判断或防御性检查，不能替代 publish-time shape assertion。
]

`TaskBinding` 因此不是一个创建参数的小集合，而是 publish 事务的入口。`UserLeader` 同时建立新的 thread group、parent/children 关系、process group membership 和 session membership；`Member` 只能加入已经存在并仍可加入的 thread group；内核执行实体也通过 topology type 形成自己的 shape，而不是临时复用 ordinary user process 的拓扑字段。这让用户可见的目标选择、`/proc` 枚举、wait/reap 和权限检查可以在 topology 边界读取同一个事实。

#book-figure(
  "../assets/figures/ch03/global-task-topology.png",
  [全局 task topology 维护线程组、进程组和会话的一致性，而不是把拓扑关系缓存进每个 TCB。],
  width: 100%,
)

== ThreadGroup、ProcessGroup 与 Session

Anemone 的 `ThreadGroup` 以 leader TID 作为 TGID，并保存成员集合、child-exited event、父进程终止信号、`ITIMER_REAL` state 和 thread-group 级生命周期。它提供的是 process 语义的核心：多个 task 可以共享地址空间、文件表、signal disposition 等资源，也可以作为一个用户可见进程被等待、被 kill 或被 `/proc` 展示。

`ThreadGroup::for_each_member()` 与 `ThreadGroup::get_members()` 的区别体现了 topology 的边界。前者在 topology membership 稳定时运行短闭包，并明确要求闭包不能做 signal delivery、wait-core wakeup、event publish 或 scheduling；后者只给 object consistency snapshot，让真正的 signal delivery 或 wakeup 在短锁窗口之外执行。这不是风格问题，而是 owner boundary：topology lock 负责成员关系，不负责在锁内完成任意跨子系统工作。

process group 和 session 进一步把 thread group 组织成 job-control 拓扑。`ProcessGroup::recv_signal()` 只是收集当前成员 thread group 并把 signal 交给 `ThreadGroup::recv_signal()`；process group 是选择器，不是 signal delivery 状态机 owner。Anemone 把 PGID/SID topology、process-group signal target selection 和 exited-child wait 的基本路径放在 topology 边界内；terminal job-control、foreground/background 规则和 stopped/continued reporting 则需要额外的 terminal / signal / wait contract。

#boundary[
  process group / session 不是完整 POSIX job-control 协议。Anemone 把 topology 和 exited-child wait 主路径收束住，但不把 terminal job-control、stopped / continued reporting、foreground/background 规则伪装成已经存在的完整协议。
]

== Clone、Exec 与 Exit 生命周期

`clone` / `clone3` 的职责不是“复制一个完整 Linux 进程”，而是把 Linux-visible flags 翻译成 Anemone 能表达的生命周期和共享关系。`clone3` 先读取并校验 `struct clone_args`，解析 exit signal、stack、TLS、parent/child TID 指针和 supported flags，再复用 `kernel_clone()`。需要新基础设施的 `CLONE_PIDFD`、`CLONE_INTO_CGROUP`、`set_tid` / `set_tid_size`、pid namespace 和 cgroup placement 不会被伪造成 task 私有状态；ABI adapter 只能表达已有 owner 能承载的生命周期语义。

这个取舍让 `clone3` 成为典型的 ABI adapter：它愿意接受 Linux 的结构体版本、flag 编码和若干兼容性检查，但不会把 pidfd、cgroup、namespace 这些尚未存在的 owner 假装成 task 私有字段。task lifecycle 的真实创建仍回到 Anemone 自己的 topology、address-space、files、credentials、signal 和 scheduler publication 顺序。

`execve` 则是另一个方向的生命周期事务：它不是创建新 task，而是在当前 thread group 语境下替换用户映像、更新命令行范围、提交可能的 credentials transition，并让后续 trap return 进入新的用户态上下文。这个提交点不能被 loader、path lookup 或 fd model 打散：只有新用户映像、参数环境和权限身份都准备好之后，task 才切换到新的执行上下文。

退出路径把 task 从 topology 中摘除，并在 thread-group 维度完成可等待状态。`wait4` 和 `waitid` 共用 exited-child scan / reap helper；`waitid` 只逐字段写 Linux waitid 可见的 `siginfo_t` 字段，避免把 Rust 侧临时结构布局或 padding 变成 ABI。`WNOWAIT` 是 peek，普通 waitid 执行 reap；多 waiter 竞争导致 reap 失败时丢弃本轮扫描状态并重试。这些细节支撑同一个判断：等待 child 是 thread-group/topology 与 wait syscall helper 的事务，不是裸 task 指针的生命周期猜测。

== Credentials 权限身份

每个 task 持有 `CredentialSet`，其中包含 real/effective/saved/fs uid/gid、supplementary groups 和 capabilities。syscall handler 负责校验身份转换，再更新 task-local credential set；VFS、exec、SysV shm 等路径通过 snapshot 消费 credentials，而不是长期持有 task 内部锁。

#listing([credentials 作为 task-local permission snapshot 被权限路径消费])[
```rust
pub struct CredentialSet {
    pub uid: Credentials<Uid>,
    pub gid: Credentials<Gid>,
    pub groups: Vec<Gid>,
    pub caps: CredCapabilities,
}

impl CredentialSet {
    pub fn has_cap_effective(&self, cap: Capability) -> bool {
        self.caps.effective().contains(cap)
    }
}
```
]

credentials 不决定 thread-group membership，也不决定 scheduler placement。它们影响的是 permission check、exec credential transition、`O_NOATIME` 这类 VFS 兼容语义、signal sender identity、SysV shm permission hook 等。把 credentials 放在 task 上，是因为权限身份要跟随执行上下文和 clone/exec 生命周期；把权限判断留给各 owner，是因为文件、内存对象和 IPC 对象仍各自拥有自己的资源状态。

== Signal delivery boundary

signal 横跨 topology、wait-core 和 trap-return 三个 owner boundary。target selection 依赖 task、thread group、process group 和 session topology；阻塞 syscall 的中断与唤醒依赖 wait-core outcome；最终用户态 handler delivery 又落在 trap-return 边界。把这三件事压成一个状态机，会遮蔽 owner boundary。

topology 侧的事实是：thread-group signal 可以在成员中选择目标 task，process-group signal 先按 topology snapshot 找到 thread groups，再交给 thread-group delivery；task 本地保存 pending set、mask 和 altstack。wait-core 负责 active wait、`rt_sigtimedwait` 与 interrupted wait 的边界；trap return 负责真正进入用户态 handler。

== TradeOff: 厚 task 与窄 owner boundary

这个边界让 Linux 兼容可以按 owner-backed slice 推进。`clone3` 的 Linux UAPI 解析仍落在 task lifecycle adapter 附近；child wait 仍落在线程组和 wait syscall helper 之间；credentials 进入权限路径，但不改写 task topology；signal 则在 target selection、wait interruption 和 trap delivery 三个 owner 之间拆开收敛。

代价是有些 Linux-visible 能力必须等到内部 owner 能表达时再进入系统。Anemone 的 task/process 设计不是少写了一个表，而是在避免把尚未拥有 owner 的语义塞进错误的位置；adapter 可以拒绝或标出边界，但不能制造 task 私有字段去伪装完整 Linux 子系统。
