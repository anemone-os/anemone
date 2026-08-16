# RFC-20260816-signalfd

**状态：** Accepted
**修订：** R0
**负责人：** doruche
**最后更新：** 2026-08-16
**领域：** signal / fs / iomux / syscall ABI
**影响契约：** `SIGNAL-FD-001`、`SIGNAL-FD-002`、`SIGNAL-FD-003`（Proposed Introduce）
**执行记录：** None

## 摘要

本 RFC 为 RV64 与 LA64 引入 native `signalfd4(2)`，让用户态可以从文件描述符同步消费当前 task-private 与
ThreadGroup-shared pending signal，并通过 blocking read、poll/select 与 epoll 等待其可读。signalfd mask 属于共享的
opened file description；实际 pending occurrence 继续只由现有 Signal owner 持有，signalfd 不建立私有 pending queue、
readable bit 或 creator-task snapshot。

R0 以主流、可直接使用且 ABI 诚实的 signalfd 能力为目标，同时保留实现空间。RFC 固定用户可见 ABI、状态 owner、
publication/recheck 顺序、failure/cleanup 边界和验证下限，不固定具体 route 类型、容器、锁形状、内部 helper 或文件布局。
偏僻 Linux 兼容语义、当前 Anemone 无法产生的 siginfo 类别、极端交错下更强的公平性或排序保证，以及需要扩建通用框架
才能获得的完整性，不自动扩大 R0；只要不破坏本文能力底线，可以在实现证据中明确记录后继续收口。

## 背景

RV64 与 LA64 当前 syscall 表在 `ppoll(73)` 与 `vmsplice(75)` 之间均未注册 74。Linux asm-generic ABI 只在该位置定义
`signalfd4`；两种 Anemone native architecture 不需要另造 legacy `signalfd` syscall number。固定参考见
`xref:linux-6.6.32:include/uapi/asm-generic/unistd.h#L198-L199`。

现有 Signal contract 已经明确 task-directed occurrence 进入 task-private pending、ThreadGroup-directed occurrence 进入
shared pending，并由 [`SIGNAL-PENDING-001/002`](../../contracts/signal/pending-routing.md) 约束 publication 与普通 task
notification。同步消费已有 `Task::fetch_specific_signal()` 一类 owner API，可以在不复制 pending truth 的前提下按集合取得
ordinary private 或 shared occurrence，并完成 POSIX timer dequeue handoff；已经交给 temporary-mask trap-return delivery 的
task-private reserved target 不再参加这类同步消费竞争。

signalfd 不能直接复用普通 signal notification 作为唯一 wake 来源。调用者通常先 block 目标 signal，当前普通 generation
路径对被 mask 的非强制 signal 不通知 task；若没有独立的 recheck capability，blocking read 或 poll 可能在 pending 已经发布
后永久睡眠。现有 [`IOMUX-POLL-001..003`](../../contracts/iomux/poll-wait.md) 已提供 snapshot/register/final-scan 模型，
但 Signal 不应为此依赖 fs-private `PollRoute` 表示。

Linux 6.6.32 的 file mask、caller-relative pending scan、blocking/batch read、reconfigure 与 flag 行为分别可由
`xref:linux-6.6.32:fs/signalfd.c#signalfd_poll`、`#signalfd_dequeue`、`#signalfd_read` 和 `#do_signalfd4` 核对。
这些外部实现只作为 ABI 与可见行为参考，不规定 Anemone 的内部数据结构。

Linux signalfd 的 epoll subscription 绑定执行 `EPOLL_CTL_ADD/MOD` 时的 signal domain；fd 随后经 fork 或传递进入其它
process 时，direct read仍按新caller消费，但继承的epoll watch不保证被新process的signal唤醒。R0接受同一边界：不为单个
caller-relative source改变现有epoll persistent-watch协议，也不把跨ThreadGroup epoll rebind伪装成已支持能力。

## 目标

- 在 RV64 与 LA64 注册 native `signalfd4(int fd, const sigset_t *mask, size_t sigsetsize, int flags)`，syscall number 为 74；
  libc `signalfd()` wrapper 可以通过该 syscall 获得能力，不增加 architecture-local legacy number。
- `fd == -1` 时创建 anonymous signalfd opened description；`fd >= 0` 时只允许重新配置已有 signalfd 的共享 mask。
- 支持 `SFD_CLOEXEC` 与 `SFD_NONBLOCK`，拒绝未知 flag；创建时分别投影为 fd flag 与 opened-description status flag，
  新opened description的基础access mode为`O_RDWR`。
- 从 signalfd mask 中清除 `SIGKILL` 与 `SIGSTOP`。signalfd 不自动修改 task signal mask，block/unblock 仍由用户态显式负责。
- read 以调用时 current task 与其 current ThreadGroup 为观察域，按现有 Signal owner 规则消费匹配的 ordinary
  private/shared pending occurrence；已经reserved给trap-return delivery的target保持原handoff，不绑定创建signalfd的task、
  thread group或process。
- 支持 blocking、nonblocking 与 whole-record batch read；首次取得一条记录后不为填满剩余 buffer 继续阻塞。
- 以固定 128-byte `signalfd_siginfo` 返回 Anemone 当前 typed `Signal` 已拥有的字段；未使用字段清零，不暴露内核未初始化字节。
- 为direct poll/select提供caller-relative readable predicate，并以register-before-rescan和guards-out hint关闭普通pending
  publication与入睡之间的lost-wake窗口。epoll exact scan在实际发生时仍读取current caller predicate，但可靠recheck/wake只
  保证执行`EPOLL_CTL_ADD/MOD`时的ThreadGroup；R0不保证继承或传递到其它ThreadGroup的既有watch报告该组signalfd readiness。
- `dup` 与 `fork` 继续共享 opened description、file mask 与 file status flags；各次 read/poll 仍按实际 caller 的 pending
  owner 扫描。多个 caller 竞争同一 shared occurrence 时只允许一个成功消费，不承诺额外公平性。
- 用一个连续 implementation unit 和唯一 `SIGNALFD-CUTOVER` 完成 syscall、file capability、Signal recheck 协议、
  current contract、测试与验证，不发布只有 create/reconfigure 而 read/readiness 不可用的部分 ABI。

## 非目标

- 不注册 native legacy `signalfd` syscall number，不提供 32-bit compat ABI。
- 不自动 block signalfd mask，不改变 `rt_sigprocmask`、temporary-mask、`rt_sigtimedwait` 或 trap-return action selection。
- 不实现 `/proc/<pid>/fdinfo` 的 `sigmask` 投影，也不为此扩展 procfs anonymous-fd introspection。
- 不为 signalfd 新增当前 Signal 不会产生的 `SigPoll`、`SigSys` 或完整 Linux fault subtype producer；对应
  `signalfd_siginfo` 字段保持零。
- 不改变 ignored-signal admission、standard-signal coalescing、realtime FIFO、POSIX timer slot或job-control generation的
  现有 owner 与语义；signalfd只消费现有 contract 实际发布的 occurrence。
- 不消费、取消或重新发布 temporary-mask `reserved_delivery` target，也不借signalfd改变当前同步消费在不同signum之间的
  selection priority；R0保证同一realtime signum的既有FIFO，不新建Linux cross-signum ordering框架。
- 不建立通用 event-source、generic signal observer、全局 callback bus或跨 subsystem wait framework；只有出现第二个真实
  consumer 和独立授权时才重新考虑泛化。
- 不修改现有epoll persistent-watch/ET causality contract，也不为fork或fd passing后的跨ThreadGroup signalfd watch建立
  rebind、反向holder tracking或全局广播；direct read与direct poll/select的caller-relative语义不受此限制。
- 不追求 Linux 内部 restart errno、waitqueue 类型、锁序或 allocation policy 的同形复制；用户可见 interruption 继续通过
  Anemone 现有 read/wait errno carrier 表达。
- 不要求穷尽所有调度交错、证明公平性，或为 concurrent reconfigure/read/poll/dup/fork/teardown 固定 Linux 的偶然竞争胜者。
- 不把特定上下文 allocation-free 或逐层 fallible allocation 作为 R0 目标。实现仍不得在不可睡眠上下文引入
  blocking/synchronous reclaim、普通锁、remote placement、复杂 callback/Drop 或日志格式化。
- 不顺手修复 [`ANE-20260606-RT-SIGTIMEDWAIT-ASYNC-WAITED-SIGNAL-EINTR`](../../register/open-issues.md#ane-20260606-rt-sigtimedwait-async-waited-signal-eintr)
  或其它相邻 Signal LTP 问题。

## Owner 与协议边界

### 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| task-private pending occurrence | 目标 `Task` 的 Signal pending owner | signalfd只通过窄 scan/dequeue capability访问 | task-directed matching read/readiness |
| ThreadGroup-shared pending occurrence | 当前 `ThreadGroup` 的 Signal pending owner | signalfd只通过窄 scan/dequeue capability访问 | process-directed matching read/readiness |
| signalfd mask | signalfd opened description | read/poll取得一次coherent snapshot；fd table只持published slot | matching predicate与reconfigure |
| synchronous dequeue与timer completion | Signal owner | signalfd取得已经完成dequeue handoff的owned occurrence | record projection与exact-once消费 |
| signalfd recheck registration | Signal侧当前ThreadGroup recheck owner | blocking read/direct iomux或epoll ADD/MOD提供non-owning recheck capability | pending或mask变化后促使同域waiter重扫 |
| blocking wait round | scheduler wait/Latch owner | signalfd持有本轮wait capability | signal、force与matching occurrence竞争 |
| poll/epoll waiter identity | iomux/epoll owner | Signal侧只保存不拥有waiter的route capability | readable hint与stale pruning |

表中的 owner 与 capability 是语义边界，不规定结构体名称或存放位置。实现可以使用直接的 owner-local registry、弱能力、
token 或其它窄表示，只要不复制 pending/readable truth、不让 Signal 依赖 fs-private表示，也不让 route 反向拥有 opened
description、Task、ThreadGroup 或 wait round。

### Pending publication 与 recheck

普通、job-control 与 POSIX timer producer 只有在实际向 private/shared pending 发布 matching occurrence 时，才形成 signalfd
recheck 义务。producer 必须先提交 pending truth，再取得当次 route snapshot；所有 Signal pending、ThreadGroup topology/
lifecycle 与 timer owner guard 释放后，才能触发 recheck。hint 成功、失败、重复、晚到或 stale 都不能反向决定 pending 是否
存在，也不能消费 occurrence。

blocking read与direct iomux register scan必须先向current ThreadGroup的Signal侧注册本轮non-owning route，再按同一次
file-mask snapshot或实现等价的coherent规则重新扫描current task private与current group shared pending。epoll watch在
`EPOLL_CTL_ADD/MOD`时向当时的current ThreadGroup注册persistent route，后续exact scan仍只读取live predicate。只有对应
register/rescan或现有epoll coverage协议确认可以park时才允许入睡。wake后继续由live predicate/dequeue形成结果；route hint
本身不能伪造readable record。

R0使用Signal owner-local动态容器保存non-owning route，不引入固定容量或对应kconfig。注册路径必须在增长前清理stale entry，
不得让反复timeout/retry留下的tombstone无界累积；live duplicate hint可以保留，但不能成为第二份readiness truth。动态增长失败
必须让本次注册失败并阻止park，不能把未armed状态静默当作成功。route snapshot的具体容器、pruning批次以及blocking read与
iomux是否共享内部carrier仍属implementation preference。不允许strong route让已结束wait round或opened description延寿，
也不要求final release跨全部曾使用该fd的ThreadGroup同步扫描清理。

### Read、dequeue 与 opened-description sharing

每次 read 先取得 shared signalfd mask 的 coherent snapshot，再让 Signal owner尝试 current task private、随后 current group
shared ordinary pending；temporary-mask reserved target继续只由ordinary trap-return delivery取得。取得的 occurrence 在离开
Signal guard 后成为当前 read transaction 的唯一责任；record copyout完成或失败时不得重新发布为第二份 pending truth。
POSIX timer completion继续由现有 synchronous dequeue handoff负责，signalfd不回调timer私有容器。

mask reconfigure 与并发 read/poll 只要求各操作观察可线性化的 coherent mask，不规定具体锁或 winner。reconfigure 更新共享
opened-description mask后提示当前调用者的ThreadGroup重扫；R0不为其它曾经继承或接收同一opened description的
ThreadGroup维护反向持有者列表或跨group广播。其它group在后续scan或signal publication时观察新mask。

### Failure、interruption 与 cleanup

- 创建路径只有在 file、opened description、mask 和 fd flags 均完成初始化后才发布 fd；失败路径不得留下可见 slot、route
  或半初始化 anonymous inode。
- reconfigure 在完成 fd kind、用户输入与 flags validation 前不改变旧 mask；成功只替换 mask，不修改既有 CLOEXEC 或
  NONBLOCK 状态。
- blocking read在任何wake后都先按live mask尝试matching dequeue；只有确认没有matching occurrence时，才可把其它未屏蔽
  signal或force条件映射为现有wait/read用户可见errno。interruption不能消费不匹配occurrence，也不能在matching occurrence
  仍可取得时抢先返回`EINTR`。
- 动态route增长失败返回`ENOMEM`；若本轮已经建立其它局部publication，先撤销或失效后再返回。direct iomux/epoll source
  registration同样不得在route未发布时允许consumer park。
- record 必须按完整 128-byte 单元报告。batch 中已经成功 copyout 的记录形成 short success；faulting record 的精确
  consume/retain 选择优先服从自然、可审查的 direct-user read transaction。若与 Linux 的偏僻 copy-fault行为不同但不泄漏、
  不重复发布、也不破坏后续主路径，可以作为 accepted limitation 记录，不要求为回滚复制 pending truth。
- route teardown先撤销或失效publication capability，再释放本地storage。stale capability必须安全no-op/prunable；任何
  use-after-free、悬挂强引用、双重dequeue或cleanup无owner仍属于实现错误。

## ABI 与可见语义

### Syscall surface

R0 使用 Linux native signature和asm-generic number 74。RV64与LA64的native `sigsetsize`均必须等于现有Linux ABI
`SigSet`尺寸8 bytes；用户mask copyin失败、未知flags、无效fd和非signalfd fd分别返回现有Linux-compatible `EFAULT`、
`EINVAL`、`EBADF`和`EINVAL`类别。混合多个无效输入时，优先遵循固定 Linux 6.6.32 参考与仓库既有 syscall validation
习惯；除非真实 libc、LTP 或 product consumer依赖，
RFC不把每一种mixed-invalid precedence提升为长期 target invariant。

`SFD_CLOEXEC`与`SFD_NONBLOCK`定义及128-byte record layout固定参考
`xref:linux-6.6.32:include/uapi/linux/signalfd.h#L16-L52`。reconfigure仍校验flags，但不借此改写既有fd flags或status flags；
调用者使用普通fd操作修改后者。

### Read 与 readiness

- `count < 128`返回`EINVAL`；更大但非整倍数的buffer按可容纳的完整record数处理。
- empty nonblocking read返回`EAGAIN`；blocking read等待至少一个matching occurrence。
- 一次成功read返回128的正整数倍。取得第一条后，其余slot只消费当时可得的matching occurrence，不等待填满batch。
- 同一realtime signum保持现有per-signum FIFO；不同signum之间沿用Signal同步消费owner的selection order，本RFC不承诺
  Linux内部`next_signal()`的完整优先级同形。
- direct poll/select只把current caller观察域内至少一个matching pending occurrence投影为readable；最终scan以live predicate
  为准。epoll exact scan也读取current caller predicate，但watch recheck只保证注册时ThreadGroup；跨ThreadGroup继承/传递的
  既有watch不具备新域wake保证，且ET没有对应dirty hint时也不承诺主动扫描。
- signalfd不要求目标signal已经被block才允许read，但不替用户阻止普通handler/default action与signalfd竞争；标准用法由用户
  先block目标集合。

### `signalfd_siginfo` 投影

每条record先整体清零，再写入`ssi_signo`、`ssi_errno`、`ssi_code`和当前typed `Signal`真实拥有的variant字段：

- Kill/TKill：`ssi_pid`、`ssi_uid`；
- realtime queued signal：`ssi_pid`、`ssi_uid`、sigval低32 bits投影为`ssi_int`、完整64 bits投影为`ssi_ptr`；
- POSIX timer：`ssi_tid`、`ssi_overrun`，以及同样的`ssi_int`/`ssi_ptr` sigval投影；
- child status：`ssi_pid`、`ssi_uid`、`ssi_status`、`ssi_utime`、`ssi_stime`；
- fault/illegal instruction：`ssi_addr`以及Anemone未来在同一typed producer中自然拥有、且无需扩建框架即可投影的字段。

`ssi_fd/ssi_band`、`ssi_syscall/ssi_call_addr/ssi_arch`和扩展fault metadata在相应producer不存在时保持零。后续独立任务若
增加这些typed producer，可以按既有UAPI projection自然补齐；这不授权signalfd RFC预先建立占位state或generic union mirror。

## Contract Impact

| Contract ID | 变化 | 当前规则 | Target 摘要 | Cutover |
| --- | --- | --- | --- | --- |
| `SIGNAL-FD-001` | Introduce | None（signalfd尚未生效） | signalfd opened description唯一拥有shared mask；read/poll按current caller观察private/shared pending | `SIGNALFD-CUTOVER` |
| `SIGNAL-FD-002` | Introduce | None（signalfd尚未生效） | Signal owner完成matching dequeue与timer handoff，signalfd在guard外形成whole 128-byte record | `SIGNALFD-CUTOVER` |
| `SIGNAL-FD-003` | Introduce | None（signalfd尚未生效） | pending-before-hint、guards-out trigger、动态route注册失败时禁止park、register-before-rescan与live-predicate final scan；epoll recheck保持registration-ThreadGroup边界 | `SIGNALFD-CUTOVER` |

### Dependencies

- [`SIGNAL-PENDING-001/002`](../../contracts/signal/pending-routing.md)：private/shared pending ownership、publication与普通
  notification分离；signalfd不改变ignored admission或ordinary action selection。
- [`SIGNAL-TEMP-MASK-002`](../../contracts/signal/temporary-mask-delivery.md#signal-temp-mask-002--defer-必须先建立-task-private-delivery-handoff)：
  reserved target已经退出ordinary private/shared competition，signalfd不得取得该handoff。
- [`IOMUX-POLL-001/002/003`](../../contracts/iomux/poll-wait.md)：snapshot/register gate、source-owned route publication与
  final predicate scan。
- [`EPOLL-WATCH-001`、`EPOLL-READY-001`、`EPOLL-FILE-001`](../../contracts/epoll/protocol.md)：watch ownership、bounded
  exact scan与non-sleeping wait publication。
- [`OPENED-DESC-002/003`、`OPENED-DESC-LIVENESS-001`](../../contracts/task/opened-description-lifecycle.md)：dup/fork
  sharing、final release与non-owning liveness capability。
- [`VFS-FILE-KIND-001`](../../contracts/vfs/file-kind.md)：anonymous inode kind继续由VFS inode truth投影。
- [`KUNIT-EXEC` / `KUNIT-CONCURRENCY` / `KUNIT-PROOF` / `KUNIT-SHAPE`](../../contracts/kunit/execution-and-proof.md)：
  KUnit执行、并发准入、证明外推与production shape。

## Implementation Boundary

- **允许改变：** RV64/LA64 syscall常量与dispatch、signalfd UAPI、anonymous file/opened-description capability、Signal侧窄
  scan/dequeue/recheck surface、blocking read与iomux接入、owner-local KUnit、`anemone-apps/signalfd-test`、定向LTP profile、
  current contract与公共导航；同owner新文件、import/re-export、模块注册和行为保持型拆分可自然闭合。
- **必须保持：** private/shared pending唯一owner、ordinary disposition/action selection、temporary-mask与`rt_sigtimedwait`
  contract、job-control与POSIX timer owner、opened-description sharing、iomux/epoll私有表示边界、现有syscall number和本文
  ABI诚实性、验证矩阵。
- **实现自由：** 动态route容器的具体类型与pruning批次，token表示、mask storage位置、锁/原子表示、blocking read与poll
  carrier是否复用，内部helper、文件布局、模块拆分和适度有界allocation均由实现按最直接可审查的形状选择；RFC中的能力名
  不是待实现类型清单。动态容器、stale-before-growth与失败时禁止park是已接受边界，不得退化为固定隐式上限或unarmed sleep。
- **工程妥协：** 偏僻Linux语义、极罕见竞争的更强排序、公平性、allocation-free纯度或需要通用框架兜底的完整性，只要
  位于R0 target之外且不造成ABI谎言、内存/生命周期错误、第二份truth或主路径lost wake，默认记录为current limitation、
  open issue或Not Proven后继续；不为单个观察面扭曲owner和代码形状。
- **停止条件：** 若实现需要改变R0主能力、state/protocol owner、pending/readable真相源、public ABI、contract delta、
  acceptance或validation claim，或必须让Signal依赖fs-private类型、为测试增加production hook、发布create-only部分ABI，
  则在cutover前回到RFC review。该review用于选择直接route correction、明确reduced target或Not Cut Over，不自动授权
  扩建通用框架，也不要求补齐本文已排除的Linux完整性。

R0只使用一个连续implementation unit与唯一`SIGNALFD-CUTOVER`。普通commit和owner内部工作片不构成独立gate；除非后续
证据证明需要probe、不安全中间态、多个cutover或长期执行历史，否则不创建`implementation.md`或transaction。

## Acceptance 与 Validation

接受本 RFC 表示上述 target、owner/handoff、兼容边界、工程余地与验证矩阵可以进入实现；Draft或Accepted本身不发布
syscall，也不改变current contract。只有实现、验证、contract surface与架构摩擦扫描同时闭合时才执行
`SIGNALFD-CUTOVER`。

### 源码审查

- 审计RV64/LA64 syscall number、signature、flags、errno类别、anonymous file publication与reconfigure transaction。
- 审计ordinary private/shared、job-control ordinary pending和POSIX timer publication路径，确认所有实际发布matching
  occurrence的路径都满足pending-before-route-snapshot、guards-out trigger；不要求没有pending publication的路径伪造hint。
- 审计blocking read与direct iomux的register-before-rescan、wake/final-scan、signal/force interruption及route teardown，
  确认matching dequeue先于`EINTR`判断，dynamic route在增长前prune stale、失败时禁止park，且stale route不拥有participant
  lifetime。
- 审计epoll ADD/MOD subscription、LT/ET scan与fork/fd-passing边界，确认R0没有修改`EPOLL-*` contract，也没有把注册域之外的
  wake或ET dirty causality写成已支持能力。
- 审计private-before-shared ordinary dequeue、reserved-target exclusion、exact-once ownership、timer completion、batch/partial
  copyout与mask reconfigure；确认没有copy-fault rollback queue、creator-task snapshot或second pending/readable cache。
- 审计`cfg(kunit)`与测试consumer，确认production state/control flow/API不理解测试协议；实现收口执行Architecture
  Friction Scan。

### Owner-local KUnit

- 覆盖`signalfd_siginfo` size/alignment、zero initialization、mask清除`SIGKILL/SIGSTOP`和typed variant字段投影。
- 覆盖private-before-shared matching dequeue、reserved-target exclusion、同signum realtime FIFO和POSIX timer dequeue handoff的
  可直接owner-local部分；不为构造fixture复制production pending state。
- 覆盖route register/rescan顺序、dynamic growth前的duplicate/stale route pruning、注册失败不park、mask snapshot/reconfigure
  等纯状态或deterministic protocol。
- 普通KUnit不为本RFC启动live userspace task或用固定yield/sleep模拟交错；并发correctness仍以source owner/caller/lifetime/
  happens-before审查和product-path app为主。

### `anemone-apps/signalfd-test`

建立独立product-path app，至少覆盖：

- create/reconfigure、invalid `sigsetsize`/mask/flags/fd/wrong-file、CLOEXEC与NONBLOCK状态；
- empty nonblocking `EAGAIN`、blocked self-signal read、private与shared occurrence、已有typed siginfo字段；
- signal在blocking registration前已pending和registration后到达两类路径，不匹配未屏蔽signal的interruption，以及matching
  occurrence与Signal outcome竞争时final dequeue先于`EINTR`；
- realtime batch FIFO、short batch、mask reassignment，以及同组reconfigure把已经pending的signal加入mask后唤醒blocked
  read/direct poll；
- direct poll与同注册ThreadGroup epoll的level-readable/final scan；
- dup/fork后的shared mask和direct read/poll caller-relative pending view，不把opened description绑定到creator task/group；
  跨ThreadGroup继承/传递的既有epoll watch明确为Not Claimed，不以direct read/poll结果外推其wake或ET dirty语义。

app使用显式phase/predicate完成多task握手；timeout只作为failure bound，不以固定yield次数或wall-clock delay证明成功顺序。

### Build 与 runtime matrix

- RV64 default KernelConfig release build：Required。
- LA64 default KernelConfig release build：Required。
- RV64 KUnit boot：Required，记录实际suite与tuple。
- RV64 `signalfd-test`：Required。
- LA64 `signalfd-test`：Required。
- LA64 KUnit boot：Optional；未运行时记录`Not Run`，不得从RV64结果外推。

### LTP

RV64定向运行`signalfd01`、`signalfd4_01`、`signalfd4_02`，记录glibc/musl实际TPASS/TFAIL/TCONF与runner tuple。
LTP只作为兼容与评分证据；当前case不能覆盖完整read/poll协议，因此不能替代`signalfd-test`或源码审查。LA64 LTP为
Optional，未运行时明确记录`Not Run`，不阻塞R0 closure。

本RFC不要求full LTP、competition harness、physical hardware、长稳压力或穷尽并发interleaving proof。任何未运行项、
waiver和只由单一architecture形成的证据必须按实际范围报告。

## 风险与反馈

- **Masked signal没有普通task wake：** 若recheck route遗漏producer，blocking read/poll可能永久睡眠。source audit必须按
  实际pending publication path闭合，而不是只测一个self-signal happy path。
- **caller-relative语义与shared file：** opened description共享mask，但pending view属于current caller。缓存creator Task/
  ThreadGroup会形成错误truth；product app用dup/fork覆盖direct read/poll边界。epoll是显式例外：persistent watch只保证
  registration ThreadGroup recheck，不得把该限制误修成creator-bound pending view或全局watch广播。
- **route lifetime：** 一个fd可以先后被多个ThreadGroup direct poll。R0使用动态non-owning route容器并要求注册增长前清理
  stale entry，不要求反向持有者tracking；任何无界stale累积、strong lifetime cycle或悬挂访问仍必须修复。
- **copy-fault Linux差异：** 精确consume/retain edge可能受当前direct-user transaction形状影响。若主路径、whole-record
  success、无泄漏与exact-once仍成立，可以记录限制，不为回滚建立镜像pending queue。
- **框架能力边界：** 如果完整Linux边角要求修改generic iomux/wait/Signal framework，优先保留局部窄能力或明确记录不支持；
  只有R0主能力本身无法ABI诚实地交付时才回到target review。
- **LTP证明不足：** 三个直接case主要验证create/reconfigure/flags，不能据其PASS声明blocking、batch或readiness已闭合。

实现反馈若只改变内部route、容器、helper、文件布局、测试组织或其它implementation preference，直接在当前边界内修正，
不形成RFC revision。只有target、non-goals、owner/handoff、ABI、contract、acceptance或validation claim改变时才形成R0/R1
语义review。普通Safe观察默认不记录；target外且值得长期保留的能力缺口进入current limitations，target内未关闭的行为缺陷
进入open issues。

## 文档与证据

- Current baseline：[`SIGNAL-PENDING`](../../contracts/signal/pending-routing.md)、
  [`IOMUX-POLL`](../../contracts/iomux/poll-wait.md)、[`EPOLL`](../../contracts/epoll/protocol.md)、
  [`OPENED-DESC`](../../contracts/task/opened-description-lifecycle.md)。
- 邻接开放问题：[`rt_sigtimedwait` async waited-signal](../../register/open-issues.md#ane-20260606-rt-sigtimedwait-async-waited-signal-eintr)、
  [`Signal LTP remaining semantics`](../../register/open-issues.md#ane-20260607-signal-ltp-remaining-semantics)。
- 外部源码证据：`xref:linux-6.6.32:include/uapi/asm-generic/unistd.h#L198-L199`、
  `xref:linux-6.6.32:include/uapi/linux/signalfd.h#L16-L52`、`xref:linux-6.6.32:fs/signalfd.c#signalfd_poll`、
  `xref:linux-6.6.32:fs/signalfd.c#signalfd_read`、`xref:linux-6.6.32:fs/signalfd.c#do_signalfd4`。
- commit / PR / optional transaction：None。

## 修订记录

- **R0（2026-08-16，Accepted）：** 接受native `signalfd4` target、Signal/opened-description/recheck owner、单一cutover与验证
  矩阵；epoll采用Linux registration-ThreadGroup边界，不Refine现有`EPOLL-*` contract；recheck registry选择owner-local动态
  容器，固定stale-before-growth与注册失败禁止park。实现内部路线、文件布局、验证命令或证据补充不单独增加修订。

## Closure

Not Started。源码实现、current contract、Architecture Friction Scan、RV64/LA64 build、KUnit、`signalfd-test`和LTP均为
`Not Run / Not Cut Over`。Closure只在实际交付与验证完成后记录一次，并同时冻结本RFC；后续事实进入live source、current
contract、register或新的独立任务。
