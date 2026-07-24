# Unix job control 定位共识

**状态：** Background / 已归档；target 已提升为公开 RFC 草案
**最后更新：** 2026-07-20
**范围：** 协作式 Unix job control 基础设施的目标、语义承诺与体系边界

## 文档目的

本文记录 Anemone 重新设计 Unix job control 时已经形成的方向共识。

本文不是 RFC target authority、current contract、状态机或 implementation plan。它只负责保留新路线的目标、外部承诺、owner 边界、核心交付和非目标，避免后续设计重新滑向已经失败的全内核 stop barrier 路线。

当前公开 RFC target 以 [RFC 正文](../index.md)和[目标与不变量](../invariants.md)为准；如果本文与两者冲突，以 target 文档为准。implementation gate、write set 与精确验证命令仍按 RFC implementation plan 为准。

## 重新设计的定位

此前的 `signal-group-stop` 路线要求 task 在任意内核调用栈和任意 wait publication 上都能立即服从 scheduler execution barrier。该目标自然扩张到 signal final-consumption、scheduler hold/admission、wait-core、同步原语 cleanup 和 typed unwind，且在大量框架代码完成后仍未进入 production runtime 验证。

新 `unix-jobctl` 不是旧 RFC 的删减实现，而是替换其核心不变量：

- job control 是建立在 scheduler-core 现有能力之上的 task policy layer，不是新的 scheduler 状态机；
- stop 的核心承诺是阻止 task 再次进入用户态，不是在任意内核栈位置即时冻结 task；
- 内核允许 task 收口已经进入的 syscall、异常和普通 wait 路径；
- 不为缩短 stop latency 而侵入 scheduler runqueue、wait-core、`Mutex`、`Event`、`Latch` 或其它同步原语；
- Linux/POSIX 外部 ABI 是主要兼容目标，Linux 内部状态机和物理停止方式不是复制目标。

旧 RFC、旧实现分支和相关 blocker 只作为历史证据，不再作为新 RFC 的 current target 或实现前提。

## 核心外部语义

### Stopped 的默认承诺

一旦父进程能够通过 `wait4` / `waitid` 观察到 child ThreadGroup 的 `Stopped`，该 ThreadGroup 的任何成员都不得继续执行用户态指令，直到 `SIGCONT` 或支配性的 terminal event 生效。

这个承诺是用户态 execution barrier，不是内核栈 freeze：

- task 可以在已有内核调用栈中继续运行到 mandatory user-entry checkpoint；
- 已经阻塞在 ordinary wait 中的 task 保留原 predicate、timeout、source registration和result owner，不取消、不替换也不参与stop ack；
- ordinary wait在stopped期间满足或超时时，task先按原协议收口syscall，再在mandatory checkpoint park；后续恢复后保留原wait的真实结果，stop本身不制造`EINTR`，但独立ordinary Signal occurrence仍可按既有语义中断wait；
- stop completion 可以明显延迟，RFC 不承诺硬实时或固定上界；
- 不得为了更早发布 `Stopped` 而允许未登记的 post-`Stopped` 用户态执行。

反过来，如果stop request进入ThreadGroup transaction时已经没有exposed member，phase可以在同一transaction中直接从`Stopping`提交为`Stopped`并建立parent report。此时某些成员仍可能阻塞或运行在内核中；parent观察`Stopped`后，这些既有syscall、exception或ordinary wait仍可继续并产生正常内核可见副作用。这里承诺停止的是user execution，不是kernel execution。

signal sender 在 occurrence 被接受后即可返回，不等待 ThreadGroup 完成 stop。parent wait/report 才是 completion 的外部观察面。如果某个成员长期不能到达 mandatory checkpoint，ThreadGroup 可以长期停留在 `Stopping`，但正式 RFC 必须为这种状态提供有范围的诊断可观测性，不能让它与 signal 丢失或状态机卡死无法区分。

如果工程证据表明某个用户态入口无法在既定边界内封闭，必须停止当前设计并回到 RFC review。允许通过明确、可观测、有范围的 accepted limitation 缩小目标，但不得让实现静默退化成无边界 best-effort。

### Exposure completion 方向

stop completion只跟踪live member中“无需再次经过mandatory gate就可能执行用户指令”的exposure：

- 已经进入内核或ordinary wait的成员是unexposed，不参与stop ack，也不阻止`Stopped`提交；
- 仍在用户态或已经越过最终gate的成员保持exposed，直到自然trap entry清除；第一版可以不主动kick，因而接受无固定stop latency；
- completion前exit或detach的成员由lifecycle路径删除其exposure；Stopping期间的新成员和新映像在首次user entry前观察live phase，不能新增绕行入口。

只有exposure为空后，才能提交parent-visible `Stopped`。并发完成的ordinary waiter不重新打开已提交的stop，也不需要向jobctl报告completion；mandatory user-entry checkpoint保证它只能收口内核路径，不能越过仍有效的stop进入用户态。

本文不固定exposure container、扫描算法、字段或锁，但明确拒绝participant set、ack counter和scheduler-state completion。未来若确有latency需求，只能增加“促使仍在user-running状态的task进入trap”的窄kick / IPI；目标已经进入ordinary wait时必须安全no-op，不能完成它后来建立的active wait。

### `Stopping` 被 `SIGCONT` 取消

`SIGCONT` 在 `Stopping` 尚未提交时取消当前 incomplete stop：

- 取消基于当前exposure relation的completion requirement，但保留各member当下的live exposure truth；同时释放已经进入jobctl park的成员。resume side effect本身不唤醒仍在原ordinary wait中的task；
- 不提交 `Stopped`，不产生 stopped report 或 `CLD_STOPPED`；
- 不产生 continued report 或 `CLD_CONTINUED`，因为此前没有完成并发布过可被继续的 `Stopped`；
- 后续 `SIGCONT` occurrence 是否 pending、被同步消费或交给 handler，仍按普通 Signal 语义处理；只有这条ordinary occurrence自己的Signal notification才可能按既有规则影响可中断wait。

只有从已经提交的 `Stopped` 恢复，才产生一次 parent-visible `Continued`。本路线明确不复制 Linux 对 incomplete group-stop 加 `SIGCONT` 合成 `CLD_STOPPED` 通知并另存 continued wait state 的历史 corner；这是为保持 phase、wait report 和 `SIGCHLD` 通知一致而接受的 scoped ABI deviation，必须由确定性竞态测试固定，不能在实现后再按偶然结果解释。

## 状态所有权与体系边界

### ThreadGroup 是 jobctl owner

每个 user ThreadGroup 是持久 job-control 状态的唯一 owner。它负责：

- stop/continue phase；
- 当前 stop reason；
- stop completion 的逻辑进度；
- parent-visible stopped/continued report；
- 提供给 procfs 等观察者的只读已发布状态。

具体状态机名称和数据结构由正式 RFC 决定。其它 subsystem 不得保存能够反向驱动 jobctl 的并列真相。

### ProcessGroup 只负责目标选择

ProcessGroup 只拥有成员关系和 process-group signal broadcast。`kill(-pgid, signo)` 向当次选择到的各个 ThreadGroup 分别发送信号，不建立 process-group-wide 原子 stop transaction，也不拥有统一 stop phase。

每个 ThreadGroup 独立完成 stop、continue 和 parent report。进程组成员变化不能把 jobctl 状态转移到 ProcessGroup。

### Scheduler 只提供既有执行能力

scheduler 继续拥有 Runnable、Waiting、Zombie、runqueue 和 ordinary wait。jobctl 可以使用现有 park/wake 能力，但不得增加 `Requested` / `Held`、runqueue suppression、通用 wait admission 或新的 scheduler stop 状态。

scheduler 不决定 ThreadGroup 是否已经 `Stopped`，也不拥有 parent-visible report。

### Signal 与 jobctl 的窄边界

Signal 继续唯一拥有 pending、mask、disposition 和普通 delivery：

- `SIGSTOP`、`SIGTSTP`、`SIGTTIN`、`SIGTTOU` 全部属于本 RFC；
- stop-class signal 在 generation time 清理所有成员和 shared ordinary pending 中的 `SIGCONT`，该 cleanup 不因 stop signal 最终 caught 或 ignored 而省略；
- `SIGSTOP`通过target、permission与lifetime校验后，在同一个ThreadGroup control transaction中直接请求stop；它不进入ordinary pending，不等待member fetch，也不使用generic force notification完成active wait；
- `SIGTSTP` / `SIGTTIN` / `SIGTTOU`保留ordinary pending、mask和live disposition语义；generation只捕获窄control-ordering identity，只有Signal最终选择`DefaultStop`且identity仍有效时才请求stop；
- task-directed或ThreadGroup-directed的stop一旦取得unconditional `SIGSTOP` authority或validated conditional `DefaultStop` authority，就调用同一个ThreadGroup stop engine，不产生thread-local stop，也不按signo分叉后续phase；
- caught、ignored或masked的条件性stop signal不得提前进入jobctl；全组成员屏蔽process-directed occurrence时它继续pending，generation后安装handler再解除mask时应执行live handler而不是按旧snapshot停止；
- task-directed 或 ThreadGroup-directed 的 `SIGCONT` 都在 generation time 先推进control-ordering identity、清理所有ordinary stop-class pending，再无条件对整个ThreadGroup发生一次resume side effect；该 side effect 不受 mask、ignore 或 custom handler 影响，且后续delivery不得重放；
- `SIGCONT` occurrence 后续是否 pending、同步消费或交给 handler，仍由普通 Signal 语义决定；
- `Stopping` / `Stopped` 期间再次形成的 default-stop occurrence 合并进当前 stop，不新建 parent report，也不替换已经确定的 first stop reason；
- generation-time opposite-class pending cleanup、`SIGSTOP` direct admission、条件性stop的pending publication / identity capture，以及并发`SIGCONT`使旧candidate失效的排序，属于正式 RFC 必须闭合的窄跨 owner 边界。

本路线不采用“generation时的disposition / mask snapshot决定后三种signal是否stop”的弱兼容方案。那会破坏generation后改变handler、ignore或mask的ordinary Signal语义。未来orphaned-pgrp suppression也只能位于条件性`DefaultStop` admission之前；`SIGSTOP`始终有效，共享stop engine本身不承担该policy。

temporary-mask classifier已经从ordinary queue中dequeue并移入task-private reserved delivery的occurrence不再受后来opposite-class cleanup撤销。reservation只提交task-local occurrence claim，不冻结live disposition或action；Signal owner仍通过handler frame、no-frame cleanup或no-return terminal path收口temporary-mask responsibility。精确规则见 RFC target。

该边界不得演化成通用 signal final-consumption 重构，不得为了 jobctl 引入跨所有 signal path 的 delivery protocol。

### Wait 与 SIGCHLD 是核心交付

parent-visible status 不是后续附属能力，而是本 RFC 的核心组成：

- `wait4(WUNTRACED/WCONTINUED)` 必须观察 stopped/continued child；
- `waitid(WSTOPPED/WCONTINUED)` 必须观察对应状态；
- `waitid(..., WNOWAIT)` 必须支持观察而不消费；
- `SIGCHLD` 必须支持 `CLD_STOPPED` / `CLD_CONTINUED` 及标准的 `SA_NOCLDSTOP` 等语义；
- child ThreadGroup 的 report 是 wait 状态真相源，`SIGCHLD` 只是通知；
- wait 层只筛选、观察或消费 report，不反向决定 jobctl phase。

ThreadGroup 的 phase 与 report 必须在同一 owner 线性化域内保持一致。report 是有界 coalesced state，不是无界事件队列：

- stopped report 只能在 ThreadGroup 仍为 `Stopped` 且该 report 尚未消费时返回；
- `WNOWAIT` 观察但不消费当前 report，在状态未改变时允许重复观察；
- consuming wait 只能由一个 waiter 消费对应 report，其它并发 waiter 必须重新扫描；
- `SIGCONT` 使尚未消费的 stopped report 失效，并且只有从已提交 `Stopped` 恢复时才建立一次 continued report；
- terminal exit 清除 stopped/continued report，并在后续 wait 选择中具有最高优先级；
- `SA_NOCLDSTOP` 或 ignored `SIGCHLD` 只抑制 signal notification，不删除 report，也不阻止 parent wait 被唤醒。

report 的具体存储形状、多 waiter 消费线性化点和 notification coalescing 算法留给正式 RFC，但不得形成与当前 phase 冲突的陈旧 stopped report，也不得让 `SIGCHLD` 成为 wait truth。

### Procfs 是只读观察者

本 RFC 必须把已发布的 jobctl stopped 状态接入现有 procfs task state：

- `/proc/<tgid>/stat` 的 state 字段在 ThreadGroup 已提交 `Stopped` 时显示 `T`；
- `/proc/<tgid>/status` 的 `State` 在同一条件下显示 `T (stopped)`；
- `Stopping` 尚未形成已发布 stopped 状态，继续按底层 task state 投影；
- observable leader `Zombie` 继续优先投影 `Z`；binding / leader resolution失败边界不在本 RFC 改变。

procfs 通过 ThreadGroup 提供的窄只读 snapshot/query 获取状态，不直接读取内部汇合结构，不持有 jobctl 私有锁，不参与 completion、resume、report 或 lifecycle 决策；`status` 的 state character与 name由同一 snapshot序列化。

### Lifecycle 保持独立 owner

`SIGKILL` 和已经提交的 ThreadGroup exit 无条件支配 `Stopping`、`Stopped` 和 jobctl park：

- jobctl 只负责取消未完成 stop、释放已停止成员并关闭当前汇合责任；
- exit code、terminal signal consumption 和最终 lifecycle 仍由现有 Signal / ThreadGroup lifecycle owner 负责；
- 成员在 completion 前 exit 或 detach 时，lifecycle 路径必须解除其汇合责任；
- jobctl report 不得延迟或阻止 terminal exit。

普通可屏蔽的终止信号不具备同等支配权。已经 stopped 的 task 可以保持 stopped，直到 `SIGCONT` 后再按普通 signal 语义处理这些信号。

## Mandatory checkpoint closure

唯一必须形成 correctness proof closure 的 checkpoint 是用户态入口：

- ordinary syscall / exception / trap return-to-user；
- clone 后首次进入用户态；
- fresh task 或 exec 新映像首次进入用户态。

任何 user task 在执行用户态指令前都必须经过其中之一。其它 owner-local 安全点可以额外检查 jobctl 以降低 stop latency，但只能是优化，不能成为 correctness proof 的必要条件。

ordinary wait不是额外checkpoint或ack surface。它的predicate未满足时继续等待；满足、超时或source完成时按原协议注销registration并形成真实result，然后才进入上述mandatory user-entry closure。jobctl stop / resume side effect都不得直接完成这条wait。

可选 checkpoint 不得放在持有任意外层 lock、linear token、wait registration 或未收口资源事务的深层路径中。正式 RFC 必须按架构和入口路径验证 closure，而不是假设一个公共 return helper 已经覆盖所有情况。

所有 mandatory user-entry path 必须实现同一个逻辑顺序：

1. 先完成 Signal / terminal lifecycle arbitration，包括可能提交 default-stop request 或消费支配性 terminal signal；
2. 再进入 jobctl before-user-entry gate，关闭当前成员的 stop 责任并在仍有效的 `Stopping` / `Stopped` 上 park；
3. 只有 gate 明确允许后，才能执行 architecture user transition。

jobctl park 被 `SIGCONT`、`SIGKILL` 或 terminal lifecycle 唤醒后，必须回到第一步重新仲裁，不能从 park 直接进入用户态，也不能在未处理 terminal event 时再次 park。ordinary trap return、clone child、fresh task 和 exec 新映像可以使用不同物理入口，但必须复用这一逻辑契约；正式 RFC 必须逐条验证，而不能仅证明其中一个公共 helper。

## 核心范围

本 RFC 的核心交付包括：

- `SIGSTOP` generation-time direct admission，以及另外三种signal在ordinary action selection得到`DefaultStop`后的conditional admission；
- 四种stop signal从`ThreadGroup::request_stop(reason)`语义边界开始共享同一phase、exposure、report、continue、lifecycle与user-entry gate；
- `SIGCONT` 的无条件 resume side effect 与普通 signal delivery；
- process-group signal broadcast 到各 ThreadGroup；
- 多线程 ThreadGroup 的协作式 stop completion 与 continue；
- mandatory user-entry checkpoint closure；
- stopped 状态下的 `SIGKILL`、group exit 和成员 detach；
- stopped/continued child report、`wait4`、`waitid`、`WNOWAIT` 和 `SIGCHLD`；
- `/proc/<tgid>/stat` 与 `/proc/<tgid>/status` 的 stopped 状态投影；
- 单线程、多线程 runnable/ordinary-wait 混合，以及 process-group broadcast 的 production runtime 验证。

POSIX/Linux 已明确且未触发工程冲突的语义不是 positioning 决策空间。正式 RFC 应直接将这些行为作为兼容义务，例如 fork child 不继承 pending stop signal、同一 ThreadGroup 的新线程不能逃过 group stop、exec 不能借新映像越过仍有效的 stop，以及 signal disposition/mask 的标准规则。

## 明确非目标

### 不重建全内核 stop barrier

本 RFC 不提供：

- 任意内核调用栈上的即时 task freeze；
- scheduler-wide stop admission、hold 或 runqueue suppression；
- ordinary wait 的撤销、替换或 stop-aware publication；
- `Mutex`、`Event`、`Latch` 和其它同步原语的 typed unwind；
- 通用 signal delivery/final-consumption 重构；
- 为追求内部 Linux 同构而引入的跨 owner protocol framework。

### TTY 不主导基础 jobctl

controlling tty、foreground/background process group、terminal access check、`tcsetpgrp` 类控制面和 terminal-generated job-control signal 属于后续 TTY/jobctl 集成。

本 RFC 仍完整支持 `SIGTSTP`、`SIGTTIN`、`SIGTTOU` 被普通 signal 路径选择为 default-stop 后的 ThreadGroup 行为，使未来 TTY 只需要成为 signal producer，而不需要重写 jobctl owner。

### Orphaned process-group policy 延后

本 RFC 不实现：

- POSIX orphaned process-group 判定；
- orphan group 中 `SIGTSTP` / `SIGTTIN` / `SIGTTOU` default-stop suppression；
- process group 新变为 orphaned 且包含 stopped job 时自动发送 `SIGHUP` 后再发送 `SIGCONT`。

这些能力需要 parent/session/pgrp topology transition、exit/reparent hook 和 jobctl observation，作为后续 topology/jobctl RFC 处理。`SIGSTOP` 始终有效。

在这些能力落地前，orphaned process group 中的 `SIGTSTP` / `SIGTTIN` / `SIGTTOU` 行为与 POSIX/Linux 不完全一致。该差异是明确的 scoped ABI limitation，不得包装成已经兼容；本 RFC cutover 时必须在 register 中保留或重分类，直到后续 topology/jobctl RFC 关闭。

### Ptrace job stop 不在范围内

本 RFC 不实现 ptrace stop、tracer wait status、job-control stop 与 ptrace stop 的转换或 `PTRACE_CONT` / `PTRACE_LISTEN` 交互。当前内核尚无 ptrace task-state model，`WUNTRACED` / `WSTOPPED` 在本 RFC 中只承诺 ordinary child job-control report。未来引入 ptrace 时必须单独定义两类 stop 的状态所有权和 wait precedence，不能复用 procfs display state 或把 ptrace state 塞进本 RFC 的 report。

## RFC target 的闭合结果

[目标与不变量](../invariants.md)已经闭合 positioning 阶段留下的 target 问题：三态 phase、membership-bound exposure completion、`SIGSTOP` direct admission、条件性`DefaultStop`与首选`ContinueEpoch`、ordinary pending cleanup与reserved-delivery claim finality、ordinary-wait preservation、parent report 的 current-state claim、Signal / lifecycle / wait / procfs owner 边界、clone / fork / exec / detach 义务、lock 方向，以及 mandatory user-entry proof obligations。

RFC同时给出一组首选最小数据结构作为 implementation baseline，但不把 Rust字段布局、container、私有 helper或等价 owner-local串行化误写成外部 contract。实现期可以依据源码与probe证据调整物理形状；只有 owner、target invariant、ABI、accepted limitation或跨 subsystem protocol发生变化时，才必须回到 RFC review。

第一条 vertical slice、物理 write set、runtime 命令和阶段停止条件已经由公开 RFC 的 `implementation.md` 维护，不由本背景文档冻结。

`Stopping × SIGCONT` 的取消语义、report invalidation / precedence、signal occurrence 作用域和 mandatory user-entry 逻辑顺序已经是公开 RFC Draft 的 target constraint，不再是 probe 可以改变的 implementation choice。若工程证据表明其中任一 target 无法成立，必须回到 RFC review，而不是在 transaction 中静默采用 Linux corner 或更弱语义。

新路线同时吸收旧`signal-group-stop`分支的工程教训：明确、常用且能owner-local实现的Linux / POSIX语义仍须兼容；偏僻corner或极难稳定复现的race如果只能通过不成比例的代价、跨owner协议或不稳定重构复制，应优先在已有安全边界fail-close，不能自然fail-close时诚实记录scoped deviation / limitation及退出条件。不得为未复现的corner随意修改稳定路径，也不得把该准则用于掩盖常用ABI缺口或削弱mandatory user-entry等核心不变量。

## 工程验收方向

新路线必须尽早进入 production runtime，而不是先建设完整跨 subsystem framework：

- 第一条 vertical slice 应贯通 signal、ThreadGroup jobctl、mandatory checkpoint、parent wait/report 和必要的 procfs projection；
- feature code 达到最小可运行闭包后应立即进入定向 user test 和 RV64 端到端验证；
- 单线程成功不能替代多线程 runnable + ordinary-wait 混合验证；
- 必须证明active ordinary wait在stop前后保留predicate、timeout、source registration和真实result，stop不产生`EINTR`；当exposure为空时，parent可以在该wait仍未完成时观察Stopped；
- 必须证明`SIGSTOP`不进入pending、不等待member fetch且不force-complete active wait；
- 必须覆盖后三种stop signal在masked pending、generation后改变disposition再unblock、caught / ignored和最终`DefaultStop`路径上的区别；
- 必须有确定性测试覆盖 incomplete `Stopping` 被 `SIGCONT` 取消且不产生 stopped/continued report，以及 completed `Stopped` 后 `SIGCONT` 产生一次 continued report；
- 必须区分`SIGCONT` resume side effect与ordinary occurrence notification：前者只释放jobctl parker，后者仍按普通Signal规则决定是否影响可中断wait；
- 必须覆盖ordinary opposite-class pending cleanup、pre-existing reservation存活、reserved `SIGCONT`在Stopped期间的frame / no-frame收口以及后续真实`SIGCONT`恢复后可能出现的额外handler观察，并证明任何delivery都不重放resume side effect；
- KUnit 可以证明局部状态转换，但不能替代真实 trap return、scheduler wait、signal generation 和 wait syscall 路径；
- 如果 vertical slice 需要重新侵入 scheduler-core、generic wait 或同步原语，必须停止并回到 RFC review，而不是继续扩大 framework。

精确测试集合、阶段顺序和 gate 属于后续 `implementation.md`，不在本文提前冻结。

## 提升结果

本定位共识已经提升为由 `index.md` 与 `invariants.md` 组成的公开 RFC Draft。状态机、owner、外部 ABI、target contract delta、accepted limitations 和 RFC-local proof obligations 由这两份文档维护。

公开 RFC 已包含 `implementation.md`；`tracking-issues.md` 没有创建，因为文档层 review 后没有仍需单独跟踪的 confirmed target blocker。本文只保留路线背景，不覆盖 RFC target。
