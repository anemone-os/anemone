# Signal fd 当前契约

**Contract ID：** `SIGNAL-FD`
**状态：** Active
**Owner：** signalfd opened description；matching pending / dequeue 仍由 Signal owner 持有
**参与领域：** signal / fs / iomux / epoll / syscall ABI
**覆盖范围：** native `signalfd4(2)` 的 opened-description mask、caller-relative pending view、同步 dequeue、128-byte record、blocking read 与 readiness recheck
**不覆盖：** legacy `signalfd` syscall、32-bit compat、temporary-mask reserved delivery、普通 signal action、跨 ThreadGroup 继承的既有 epoll watch rebind
**实现位置：** `anemone-abi/src/fs.rs`、`anemone-kernel/src/fs/signalfd/`、`anemone-kernel/src/task/sig/`
**依赖：** `SIGNAL-PENDING-001/002`、`SIGNAL-TEMP-MASK-002`、`IOMUX-POLL-001..003`、`EPOLL-WATCH-001`、`EPOLL-READY-001`、`OPENED-DESC-002/003`、`OPENED-DESC-LIVENESS-001`
**Pending Successor：** None
**最后核验：** 2026-08-16

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| signalfd mask | signalfd opened description | read / poll 取得 coherent snapshot | matching predicate 与 reconfigure |
| task-private / group-shared pending occurrence | 对应 Signal pending owner | signalfd 使用窄 scan/dequeue capability | caller-relative read 与 readiness |
| dequeue 与 POSIX timer completion | Signal owner | signalfd 取得已完成 handoff 的 owned occurrence | exact-once record projection |
| ThreadGroup recheck registration | Signal 侧当前 ThreadGroup registry | read / iomux 提供 non-owning route capability | pending publication 后促使 live predicate 重扫 |
| 同一 opened description 的 mask-change recheck | signalfd opened description | read / iomux / epoll watch 提供 non-owning route capability | reconfigure 后提示已登记的活跃 consumer 重扫 |
| wait round / poll waiter identity | scheduler latch / iomux / epoll owner | signalfd 与 Signal 侧只持 non-owning wake capability | blocking、final scan 与 stale pruning |

route、wake token、poll hint和record buffer都不是pending或readiness的第二真相源。

## SIGNAL-FD-001 — Opened description 只拥有 mask，pending view 属于 current caller

**规则：** 一个signalfd opened description唯一拥有其shared mask；`dup`与`fork`得到的别名共享该mask，reconfigure只原子替换mask，
不修改已有`CLOEXEC`或`NONBLOCK`状态。mask始终清除`SIGKILL`与`SIGSTOP`。每次read或readiness scan按live coherent mask，
先观察current task private ordinary pending，再观察current ThreadGroup shared ordinary pending；不得缓存creator Task、creator
ThreadGroup、pending snapshot或readable bit。temporary-mask `reserved_delivery`已退出ordinary competition，不能被signalfd取得。
direct poll/select使用current caller观察域；epoll exact scan同样使用调用时live predicate，但persistent watch的主动recheck只保证
`EPOLL_CTL_ADD/MOD`注册时的ThreadGroup，跨ThreadGroup继承或传递的既有watch不获得rebind或ET dirty保证。

native `signalfd4`使用asm-generic syscall number 74与8-byte `sigsetsize`。创建在file、opened description、mask、status flags和fd
flags全部初始化后才发布slot；reconfigure在fd kind、用户输入与flags全部验证后才改变旧mask。错误类别为Linux-compatible
`EFAULT`、`EINVAL`、`EBADF`；只接受`SFD_CLOEXEC | SFD_NONBLOCK`。

**违反表现：** 同一opened description的别名看到不同mask、mask reconfigure偷偷改写fd flags、fd在半初始化状态可见、从creator
而非current caller取pending，或signalfd保存第二份pending/readiness truth。

**验证 / Enforcement：** `sys_signalfd4()`、`SignalFd` mask与file publication源码审计；owner KUnit的mask清理；
`anemone-apps/signalfd-test`的create/reconfigure、flags、invalid input、dup/fork与caller-relative epoll/private-pending覆盖；RV64
定向LTP `signalfd01`、`signalfd4_01`、`signalfd4_02`。

**最初来源：** [RFC-20260816-signalfd R0](../../rfcs/signalfd/index.md)。

**当前来源：** [SIGNALFD-CUTOVER closure](../../rfcs/signalfd/index.md#closure)；cutover commit。

## SIGNAL-FD-002 — Signal owner完成dequeue后，signalfd只提交完整record

**规则：** matching dequeue由Signal owner按current task private、随后current group shared的顺序完成；同一realtime signum沿用
pending owner既有FIFO，POSIX timer沿用同步dequeue completion handoff。occurrence离开Signal guard后由当前read transaction唯一
持有，不重新发布、不建立rollback queue，也不回调timer私有容器。每个`signalfd_siginfo`固定128 bytes，先整体清零，再只投影
typed `Signal`真实拥有的字段；无producer支持的字段与reserved bytes保持零。

`count < 128`返回`EINVAL`，其它buffer只使用能容纳完整record的前缀。empty nonblocking read返回`EAGAIN`；blocking read等待
至少一条matching occurrence。成功返回128的正整数倍；取得第一条后只批量消费当时立即可得的记录，已经copyout的记录形成
short success。copy fault不得造成泄漏、重复publication或双重dequeue。

**违反表现：** shared先于private被取走、reserved target被普通scan取得、同一occurrence被两次读取、timer completion遗失、
返回partial record、未初始化字段泄漏，或copy-fault rollback复制pending truth。

**验证 / Enforcement：** Signal specific-dequeue、pending/timer handoff与`Signal::to_signalfd_siginfo()`源码审计；owner KUnit覆盖
private-before-shared、reserved exclusion、realtime FIFO、timer handoff、record layout/zero/typed projection；product app覆盖
private/shared batch、realtime queued FIFO、short batch与typed payload。

**最初来源：** [RFC-20260816-signalfd R0](../../rfcs/signalfd/index.md)。

**当前来源：** [SIGNALFD-CUTOVER closure](../../rfcs/signalfd/index.md#closure)；cutover commit。

## SIGNAL-FD-003 — Pending与mask先发布，non-owning recheck只提示final scan

**规则：** ordinary private/shared、job-control ordinary pending与POSIX timer producer必须先提交pending truth，再取得当前
ThreadGroup route snapshot；释放Signal pending、ThreadGroup topology/lifecycle与timer owner guard后才触发route。blocking read
与direct iomux先注册non-owning route，再执行final matching scan，只有注册成功且final scan确认empty时才允许park；epoll在
`EPOLL_CTL_ADD/MOD`时注册同类persistent route。wake、重复hint、晚到hint或stale entry都不能决定readiness或消费occurrence。

Signal registry在增长前清理stale entry，live duplicate可以替换但只承担资源卫生；动态增长失败必须返回错误并阻止park。
route不得强持Task、ThreadGroup、opened description或active wait lifecycle；blocking trigger可以暂存retired wait-token backing，
但只能形成stale no-op，并须在下一次registry增长前prune。共享opened description的mask reconfigure先提交新mask，再通过
description-local non-owning routes提示所有已登记的活跃consumer重扫；这份registry不保存fd holder、ThreadGroup membership、
pending或readiness truth。mask-change hint可令已登记的persistent epoll watch进入既有dirty rescan；Signal publication仍只
提示发生publication的ThreadGroup，后续signal arrival不获得跨组epoll rebind、wake或ET dirty保证。blocking
read在任意wake后先尝试live matching dequeue，只有确认没有matching occurrence时才把其它signal或force结果映射为errno。

**违反表现：** pending或mask尚未发布就触发hint、持owner guard回调、未armed仍park、register/rescan窗口lost wake、route
延长participant生命周期、hint直接形成用户可见readiness，或`EINTR`抢在仍可取得的matching occurrence之前返回。

**验证 / Enforcement：** 所有实际pending publication producer、`SignalFd::{read,poll}`、Signal与description-local route registry、
iomux/epoll subscription源码审计；owner KUnit覆盖publication/pruning与register行为；product app覆盖pre-pending、post-registration、
poll/epoll、跨别名reconfigure wake与epoll dirty rescan、matching-before-`EINTR`。

**最初来源：** [RFC-20260816-signalfd R0/R1](../../rfcs/signalfd/index.md)。

**当前来源：** [SIGNALFD-CUTOVER closure](../../rfcs/signalfd/index.md#closure)；cutover commit。

## 当前边界

本页不改变Signal pending、ordinary action、temporary-mask、POSIX timer、iomux或epoll各自的既有owner。signalfd只新增
opened-description mask、同步消费capability与窄recheck handoff；cross-ThreadGroup persistent epoll watch rebind、legacy syscall、
32-bit compat、完整Linux copy-fault偶然语义与当前不存在的siginfo producer不在当前能力内。
