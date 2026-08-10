#import "../components/figure.typ": code-block

= IPC

截至初赛结束，Anemone 的 IPC 能力已经覆盖了信号、管道、共享内存和事件类文件对象。

== 信号机制

Anemone 实现了 Linux/POSIX 风格的信号 UAPI，支持 `rt_sigaction`、`rt_sigprocmask`、`rt_sigpending`、`rt_sigsuspend`、`rt_sigtimedwait`、`rt_sigreturn`、`kill`、`tkill`、`tgkill` 和 `rt_sigqueueinfo` 等一系列接口，可以为用户态程序兼容提供强大的支持。用户程序可以设置信号处理函数、修改信号屏蔽字、查询 pending 信号，也可以向进程或指定线程发送信号。

#code-block(
  ```rust
  pub enum SignalAction {
      Default(fn(SigNo)),
      Ignore,
      Custom(VirtAddr),
  }

  pub struct KSigAction {
      pub action: SignalAction,
      pub flags: SaFlags,
      pub restorer: VirtAddr,
      pub mask: SigSet,
  }

  pub enum SigInfoFields {
      Kill(SigKill),
      Rt(SigRt),
      Chld(SigChld),
      Fault(SigFault),
      Timer(SigTimer),
      TKill(SigKill),
      Ill(SigFault),
  }
  ```.text,
  caption: [信号动作、用户可见信号处理配置和 `siginfo` 携带的信息类型],
  lang: "rust",
)

信号动作按照默认处理、忽略和用户自定义 handler 三类保存。每个任务维护自己的信号屏蔽字，线程组和线程定向信号分别进入对应的待处理集合；同步异常、管道写端错误、定时器到期、子进程退出等内核事件也会转化为信号投递给目标任务。这样，普通进程控制和内核异常通知可以复用同一套信号处理路径。

当任务从内核态返回用户态前，内核会检查是否存在未被屏蔽的 pending 信号。如果目标信号使用默认动作，内核直接执行对应的终止、忽略等行为；如果用户注册了 handler，内核会在用户栈上构造信号上下文，保存原来的用户态寄存器和信号屏蔽字，并把返回地址设置到 `rt_sigreturn` 路径。用户态 handler 执行结束后通过 `rt_sigreturn` 回到内核，由内核恢复原始上下文继续执行。

== Pipe

Anemone 通过 `pipe2` 创建匿名管道，并返回读端和写端两个文件描述符。管道内部维护一段环形缓冲区，读端只允许读取，写端只允许写入；文件描述符可以通过 `fork` 继承，也可以被复制到其它 fd，从而让相关进程在同一条字节流上通信。

#code-block(
  ```rust
  struct Pipe {
      buf: Box<RingBuffer<u8, { PagingArch::PAGE_SIZE_BYTES }>>,
      rx_cnt: usize,
      tx_cnt: usize,
  }

  struct PipeRx {
      pipe: Arc<SpinLock<Pipe>>,
  }

  struct PipeTx {
      pipe: Arc<SpinLock<Pipe>>,
  }
  ```.text,
  caption: [Pipe 的核心缓冲区与读写端状态],
  lang: "rust",
)

管道读写遵循 Linux 常见语义：读端在缓冲区有数据时返回实际读取的字节数，所有写端关闭后返回 EOF；写端在读端关闭后返回 `EPIPE`，同时向当前任务投递 `SIGPIPE`。非阻塞 fd 在无法立即完成读写时返回 `EAGAIN`。Anemone 还实现了 `F_GETPIPE_SZ`、`F_SETPIPE_SZ` 和 `FIONREAD` 等常用控制接口，使用户程序可以查询管道容量和当前可读字节数。

== 共享内存

Anemone 实现了 System V 共享内存，提供 `shmget`、`shmat`、`shmdt` 和 `shmctl` 等 UAPI。共享内存段可以通过 `IPC_PRIVATE` 创建，也可以通过 key 查找已有对象；每个段都有独立的 shmid、权限信息、大小、创建者、最近操作进程和 attach 计数。`IPC_RMID` 会先把段标记为删除，等所有进程都 detach 后再释放对象。

用户调用 `shmat` 后，内核会把共享内存段接入当前进程地址空间，并在地址空间中记录 attachment。该映射可以在 `fork` 后继承，多个进程访问的是同一个共享 backing，因此一个进程写入的数据能够被其它 attach 者直接看见。共享页由共享内存对象按页管理，和普通匿名内存、文件映射一样经过用户地址空间和缺页处理路径安装到页表中。

除了 System V 接口，Anemone 也支持 `/dev/shm` 形式的文件型共享内存使用方式。比赛测例中，很多测试环境（比如LTP）会依赖 `/dev/shm` 的共享内存，用户态的 `shm_open`。我们在测试启动之前，在`/dev/shm`这里提前挂在一个tmpfs，这样`shm_unlink` 和 `mmap` 可以通过普通文件创建、删除和映射来建立共享对象，提供了更强的兼容支持。

== Event 类对象

Anemone 还实现了各类被抽象为fd的事件对象。例如：我们实现了`eventfd2`系统调用。eventfd用于在进程或线程之间传递轻量级事件计数。`eventfd` 文件内部维护一个 64 位计数器，用户态可以通过读写该 fd 消费或增加计数；创建时支持 `EFD_CLOEXEC`、`EFD_NONBLOCK` 和 `EFD_SEMAPHORE`。在 semaphore 模式下，每次读取只消费一个计数单位，用户可以把 eventfd 当作轻量通知或同步原语使用。
