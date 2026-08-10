#import "../components/figure.typ": code-block, report-figure

= 进程管理

== 不选择无栈协程的理由

Anemone 没有把进程管理建立在无栈协程上，而是让每个 `Task` 拥有独立的内核栈和上下文切换状态。调度器切换的是完整的内核执行上下文，而不是把一个系统调用拆成多段手工状态机。

我们调研往届内核时看到过不少内核都选择了无栈协程方案。它的优点是单个执行单元占用内存更小，部分路径上的切换成本也低；不过，就内核实现而言，它会带来三个实际问题。首先，内核调用栈被拆散后，panic、日志和调试信息很难直接还原阻塞点，必须额外引入结构化的 trace机制，这导致调试相当困难。第二，系统调用和页错误处理这类路径天然会跨越 VFS、内存管理、信号、等待队列等模块；如果每个阻塞点都改写成状态机，代码会在错误处理和资源回滚上变得分散，分析会比较困难。最后，无栈协程本质上是协作式模型，可以用时钟中断抢占用户态，但没办法实现内核态抢占。

Anemone 最后选择有栈调度，我们支持内核抢占——这带来了更高的实时性与响应能力。同时另外两个困难的解决也使得我们在分析数据/控制流时更简单。

== 进程与线程模型

Anemone 使用单一的 `Task` 表达可运行的执行上下文，而不是分成 `Process` 和 `Thread` 两套结构，这套设计来自Linux的LWP。这使得各种资源的共享和独占关系的表达力更强：我们可以使用 `Arc` 句柄在 `Task` 之间共享地址空间、文件系统上下文、文件描述符表和信号处置表；也可以在 `Task` 内部使用普通的 `Box` 或 `RwLock` 独占这些资源。通过这种细粒度的共享，我们可以实现 fork/clone/exec 的各种组合，支持多线程进程、单线程进程和内核线程。但客观而言，这样做也存在一个缺点，那就是并发与锁的分析难度会大大上升。这是一种权衡：我们选择了更强的表达力，代价是更复杂的并发分析，而不是更简单的模型。

用户态看到的进程、线程组、进程组和会话由 `TaskTopology` 维护；调度器看到的是 `Task`。这和 Linux LWP 的思路一致：线程和进程在执行上下文层面都是 task，差别来自它们共享或独占哪些资源，以及它们被挂到哪个拓扑关系里。

#code-block(
  ```rust
  pub struct Task {
      tid: NoIrqRwLock<TidRef>,
      creator: Option<Tid>,
      tgid: Tid,
      create_instant: Instant,

      kstack: KernelStack,
      name: NoIrqRwLock<Box<str>>,
      flags: NoIrqRwLock<TaskFlags>,
      usp: RwLock<Option<Arc<UserSpaceHandle>>>,

      cpuid: CpuId,
      nice: AtomicIsize,
      sched_ctx: MonoFlow<TaskContext>,
      sched_entity: SpinLock<SchedEntity>,
      fpu_used: AtomicBool,

      fs_state: Arc<RwLock<FsState>>,
      files_state: RwLock<Arc<RwLock<FilesState>>>,
      cred: RwLock<CredentialSet>,
      no_new_privs: AtomicBool,
      cpu_usage: NoIrqRwLock<TaskCpuUsage>,

      sig_disposition: Arc<NoIrqRwLock<SignalDisposition>>,
      sig_mask: NoIrqSpinLock<TaskSigMaskState>,
      sig_pending: NoIrqSpinLock<PendingSignals>,
      sig_altstack: NoIrqSpinLock<Option<SigAltStack>>,

      robust_list: SpinLock<Option<VirtAddr>>,
      exit_code: SpinLock<Option<ExitCode>>,
      vfork_done: Event,
      sched_state: NoIrqRwLock<TaskSchedState>,
      clear_child_tid: SpinLock<Option<VirtAddr>>,
      kthread: SpinLock<Option<KThreadTaskLocal>>,
  }
  ```.text,
  caption: [`Task` 作为 TCB 保存执行上下文、资源句柄、信号状态和退出状态],
  lang: "rust",
)

Task内只保存当前任务的“本地”状态。`tid` 和 `tgid` 标识当前执行上下文及其线程组；`kstack`、`sched_ctx`、`sched_entity` 和 `sched_state` 服务调度与上下文切换；`usp` 指向用户地址空间；`fs_state` 和 `files_state` 分别描述工作目录、根目录、umask 与文件描述符表；`cred` 和 `no_new_privs` 参与权限检查；信号处置表、信号屏蔽字、task-local pending signal、alt stack 和 robust futex list 由 signal、futex、exec 和 exit 路径使用。

从这里可以看出，我们完全没有将拓扑关系放在TCB里，它们在全局拓扑表（一个单例）里维护：

#code-block(
  ```rust
  struct TaskTopology {
      inner: NoIrqRwLock<TaskTopologyInner>,
  }

  struct TaskTopologyInner {
      tasks: BTreeMap<Tid, TaskNode>,
      thread_groups: BTreeMap<Tid, Arc<ThreadGroup>>,
      process_groups: BTreeMap<Tid, Arc<ProcessGroup>>,
      sessions: BTreeMap<Tid, Arc<Session>>,
  }

  static TOPOLOGY: TaskTopology = TaskTopology {
      inner: NoIrqRwLock::new(TaskTopologyInner {
          tasks: BTreeMap::new(),
          thread_groups: BTreeMap::new(),
          process_groups: BTreeMap::new(),
          sessions: BTreeMap::new(),
      }),
  };
  ```.text,
  caption: [`TaskTopology` 保存 task、thread group、process group 和 session 的全局索引],
  lang: "rust",
)

`tasks` 提供 TID 到 `Task` 的查找；`thread_groups`、`process_groups` 和 `sessions` 分别维护 Linux 可见的 TGID、PGID 和 SID 层次。拓扑读写通过同一把 `NoIrqRwLock` 线性化，涉及 session、process group 和 thread group 的内部修改时使用固定锁序：`TOPOLOGY -> Session.inner -> ProcessGroup.inner -> ThreadGroup.inner`。

为何不直接把拓扑关系维护在`Task`里，比如说`Task`内部有某个字段形如`Arc<ThreadGroup>`？这主要是为了避免竞态关系。一个task正式进入系统主要有两个阶段：关系发布和全局表注册。如果我们先发布关系，再注册到全局表，那么这中间就会存在一个窗口：其他task可以通过拓扑查到这个task，但是却无法在全局表里找到它；如果我们先注册到全局表，再发布关系，那么系统内就会出现一个孤立于进程树的task。无论哪种方式，都会导致拓扑关系的不一致。*综上所述，我们选择了在同一个拓扑写锁下完成关系发布和全局表注册，这样就保证了拓扑关系的一致性。*

#report-figure(
  image("../assets/global-task-topology.png", width: 100%),
  caption: [进程管理全局拓扑。上半部分是单个 `Task` 持有或引用的执行资源，下半部分是 `TaskBinding` 发布后进入的全局拓扑索引。],
)

`TaskBinding` 是新 task 发布到全局拓扑时的分类。`UserLeader` 创建新的用户线程组，并继承父进程的 PGID、SID 和终止信号；`Member` 把新 task 加入现有用户线程组；`KThread` 创建内核线程的 singleton thread group。发布过程在同一个拓扑写锁下完成全局索引更新、线程组更新、父子关系更新以及进程组/会话关系更新，避免用户态观察到半初始化的进程关系。

== 生命周期路径

进程创建从 `fork` / `clone` 进入。内核先为子任务准备独立的执行上下文，使它从系统调用返回时在子进程视角中看到返回值为 0；随后根据 clone 标志决定哪些资源共享、哪些资源复制。普通 fork 会产生新的线程组，子进程继承父进程的进程组和会话；线程创建则加入父进程已有的线程组。子任务只有在地址空间、文件描述符表、文件系统上下文、信号状态和 credentials 都初始化完成后，才会一次性发布到全局拓扑并进入调度队列。这样可以避免一个还没有完整资源视图的任务被其他 CPU 或 procfs 提前观察到。

程序替换由 `execve` 完成。它不会创建新的进程身份，而是在当前线程组内装入新的程序映像：路径解析使用当前进程的根目录和工作目录，装载器建立新的用户地址空间、用户栈和入口点，并根据 setuid/setgid 和 file capability 重新计算执行后的权限状态。exec 成功后，旧程序的用户态执行现场被替换，自定义信号处理、alt stack、robust futex 状态以及带 close-on-exec 标记的文件描述符被清理；多线程进程执行 exec 时，线程组会做“dethread”，这将杀死所有其他线程，只保留调用 exec 的线程继续执行，而它若非leader，也会被提升为新的线程组 leader。

退出路径区分单个线程退出和整个线程组退出。单个线程退出时，内核要先完成用户态可见的退出副作用，例如清理 clear-child-tid、唤醒相关 futex、处理 robust futex list、关闭本线程组不再需要的文件描述符，并从线程组成员集合中移除当前线程，等等。而如果退出的是线程组最后一个成员，内核会把线程组变成可等待的 zombie 状态，记录退出码和资源使用情况，将孤儿子进程重新挂到 init 下，并通知父进程。

等待路径对应 `wait4` / `waitid` 等接口。父进程可以等待任意子进程、指定 TGID 的子进程，或指定进程组内的子进程。内核扫描父进程的子线程组，只有已经进入 zombie 状态的子进程才可以被回收。真正回收时，子线程组会从全局拓扑中移除，退出码和资源使用量返回给用户态。这个过程把“退出”和“回收”分开，使父进程仍然可以在 wait 之前观察到已退出但尚未回收的子进程。

这些基本动作，就构成了Anemone进程管理的核心生命周期路径。实际上这也是类Unix内核的通用做法。

== 资源继承与隔离

资源继承主要由 `clone` flag 决定。默认的 fork 形态会复制地址空间、文件系统上下文、文件描述符表和信号处置表；带共享 flag 的 clone 则让子 task 直接持有父 task 的对应 `Arc` 句柄。TCB 中的 `usp`（用户空间指针）、`fs_state`（文件系统状态）、`files_state`（打开的文件描述符） 和 `sig_disposition`（信号处置表） 都按这个规则初始化，因此共享关系在对象引用层面表达，而不是通过额外的进程结构体转发。这也是LWP机制的体现。

权限状态在 fork/clone 和 exec 边界上的处理不同。创建子进程时，子进程继承父进程的用户 ID、组 ID、能力集、`no_new_privs` 和调度优先级等属性；这符合“子进程先从父进程复制而来”的语义。exec 时则要重新检查被执行文件的权限属性：setuid/setgid 位可能改变 effective ID，file capability 可能改变能力集，`no_new_privs` 会禁止通过 exec 获得新的权限。换句话说，fork/clone 复制身份，exec 重新计算身份。

exec 的资源边界也不同于 clone。它保留进程身份、线程组关系、工作目录、根目录以及未设置 close-on-exec 的打开文件；同时替换用户地址空间，重置信号处置中的用户自定义处理函数，清除只属于旧程序映像的用户态辅助状态，并关闭需要在 exec 边界消失的文件描述符。这样，fork/clone 负责建立“新执行实体”，exec 负责把“同一个进程身份”切换到新的程序映像。

== ProcFs

ProcFs 是进程管理向用户态暴露状态的主要入口。在Anemone中，`/proc/<tgid>` 是按访问懒创建的，不是在 task 创建时预先注册：如果一个task对应的procfs条目未被访问，那么这个条目就根本不会被注册。这样做是因为很多时候很多task的procfs目录项不会被访问，预先注册会浪费内存，也会降低内核性能。

`/proc/<tgid>` 下的条目由一个静态数组`TGID_ENTRIES` 统一注册。当前我们已经实现了 `root`、`cwd`、`cmdline`、`environ`、`exe`、`fd`、`mounts`、`stat` 和 `status`等等。每个子 inode 都持有 `ThreadGroupBinding`，操作前，我们仍然会校验当前绑定是否合法——检查这个tgid是否仍然对应着此前的task，这样可以避免用户态访问已经退出的进程的procfs条目。

退出和回收路径负责使 procfs 视图失效。在这里我们明确区分了用户线程和内核线程。不论如何，失效操作会接触条目和task的绑定。因此，procfs 目录项的生命周期跟随 topology 的活跃性和 wait/reap 结果，而不是单独维护一套进程状态。通过这样的设计，我们可以保证 procfs 视图的正确性和一致性，同时避免了额外的资源管理开销。
