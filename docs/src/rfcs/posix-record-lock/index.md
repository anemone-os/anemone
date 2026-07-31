# RFC-20260731-posix-record-lock

**状态：** R0 / Accepted for Implementation / Stage 0-2 Closed / Stage 3 Outline / Not Cut Over
**修订：** R0
**负责人：** doruche
**最后更新：** 2026-08-01
**领域：** fs / VFS / task files / scheduler wait / signal restart / syscall ABI
**事务日志：** [2026-07-31 POSIX Record Lock](../../devlog/transactions/2026-07-31-posix-record-lock.md)
**影响契约：** 拟 Introduce `FILES-POSIX-OWNER-001`、`POSIX-LOCK-DOMAIN-001`、
`POSIX-LOCK-WAIT-001`、`POSIX-LOCK-LIFECYCLE-001`；Preserve `VFS-FILE-KIND-001`、
`FLOCK-DOMAIN-001`、`FLOCK-WAIT-001`、`FLOCK-LIFECYCLE-001`、`OPENED-DESC-001/002/003`、
`OPENED-DESC-RETIRE-001`、`OPENED-DESC-LIVENESS-001`、`SCHED-LATCH-001..003` 与
`SCHED-WAKE-001..004`。完整 R0 delta 见
[Contract Impact](./invariants.md#contract-impact)。
**开放问题：** 当前无 active Apollyon / Keter；本轮关闭记录见 [Tracking Issues](./tracking-issues.md#neutralized)。
后续 Outline 中尚未解析的类型、锁、容器、模块路径、stage、write set 与精确验证命令属于滚动
implementation resolution，不因缺失本身构成 finding。
**下一步：** Stage 0-2已独立关闭，Stage 2的2A/2B均Closed。下一动作只能是开发者另行授权的
`Stage 2 -> Stage 3 Implementation Resolution Gate`；Stage 3仍为Outline，本轮不解析或执行Stage 3，也不进行任何
semantic contract cutover。

## 文档状态

本文与 [目标和不变量](./invariants.md) 是 POSIX process-associated byte-range record lock 的公共 canonical
R0 target。2026-07-31 独立 review 已共同接受 target、Contract Impact、proof obligations 与首个完整 Ready
stage；随后建立 transaction，开发者明确授权 Stage 0 Active。R0 target 尚未 cut over，不是 current contract，
Stage 0 foundation 已独立关闭，但仍不是可独立合入的 POSIX record-lock capability。其后的独立只读
resolution gate已基于live source把Stage 1解析为两个独立checkpoint：1A行为保持地完成lock namespace
alignment，1B建立inode-associated POSIX range domain与focused proof。两者现均已关闭，Stage 1 Closed。
其后的独立只读gate把Stage 2解析为2A native ABI/binding/nonblocking-query与2B blocking wait/signal replay；
2A/2B现已分别独立关闭，Stage 2 Closed。current contract语义仍未更新，Stage 2 stacked candidate不是可独立合入
或对外声称的POSIX record-lock capability。

本目录自本次提升起是该提案的公共 canonical source；此前的私有工作稿不再承担共享链接、target 或计划权威。
此前 public promotion 只改变文档可见性和引用入口；本次 R0 acceptance、transaction bootstrap 与 Stage 0
activation 是其后的独立事件。三者均不更新 current contract。

此前的定位讨论已经冻结为[背景材料](./backgrounds/positionings.md)，只保留决策来路，不再作为维护面。
后续 review 应直接修正本文或 `invariants.md`；不得继续向 positioning 回写新 target，也不得让背景材料覆盖
本文。

## 摘要

本 RFC 提议为 Anemone 增加本机、本地 `S_IFREG` 文件上的 POSIX process-associated byte-range record lock。
首版支持 RV64 与 LA64 原生 64 位用户态的 `F_GETLK`、`F_SETLK`、`F_SETLKW` 和原生 `struct flock`，覆盖
range normalization、读写冲突、同 owner range replacement / split / merge、非阻塞失败、阻塞等待、
signal interruption，以及 close、fork、`CLONE_FILES` sharing、`close_range(UNSHARE)` 与成功 exec 的
可见生命周期。

POSIX lock holder 是 file-table sharing episode 拥有的稳定 capability；它只表达 same-owner identity 与
cleanup attribution。全部已授予区间、mode、conflict predicate 与 wait publication truth 只存在于对应
inode-associated VFS POSIX record-lock domain。fd slot、opened description、数值 PID、path 与 inode number
都不能替代 holder；notification 也不能替代 grant truth。

现有 whole-file `flock` 保持独立 conflict namespace。首版不实现 OFD lock、deadlock detection、mandatory locking、
remote propagation 或通用 file-lock framework，也不为这些未来能力预置 owner variant、backend hook、
waiter graph 或 lifecycle registry。

## 背景

### Anemone 当前事实

Stage 2 stacked candidate现已接入`F_GETLK`、`F_SETLK/F_SETLKW`、native `struct flock` copy boundary、
operation-local binding、blocking Event recheck与ordinary signal replay。由于Stage 3、双架构runtime/focused LTP
和`POSIX-LOCK-CUTOVER`仍未执行，这些代码不能写成current POSIX record-lock支持。现有
`anemone-apps/user-test/ltp/groups/fcntl.txt`仍注释掉`fcntl14` / `fcntl14_64`，不能把其它`fcntl` case或Stage 2 focused
suite当成focused LTP与最终双架构产品证据。

当前 task file state 已具备与本 target 相邻、但不能直接冒充 holder contract 的事实：

- task-owned `FilesState`承载file-table participation与sharing lifecycle；episode-owned `FileTable`拥有fd slot
  allocation/publication，`CLONE_FILES`共享同一live table，普通fork复制slot publication；
- `close_range(CLOSE_RANGE_UNSHARE)` 已能替换当前 task 的 table handle，成功 exec 会关闭 `FD_CLOEXEC`
  slots；
- `ProcFile` 是 opened file description，dup 与普通 fork 的 fd aliases 可以共享它；
- opened-description published-reference lifecycle、terminal retirement 与固定 flock cleanup handoff 已由
  [`OPENED-DESC`](../../contracts/task/opened-description-lifecycle.md) current contract 拥有。

POSIX record lock 不能复用 opened-description retirement：同一 file-table holder 通过两个独立 `open()`
得到的 fd 仍属于同一 POSIX owner，而关闭其中任一指向某 inode identity 的 fd 都必须释放该 holder 在该
inode 上的全部 POSIX locks。cleanup 因而属于 fd-table holder × inode handoff，不属于 final opened-description
release。

VFS 已由 [`VFS-FILE-KIND-001`](../../contracts/vfs/file-kind.md#vfs-file-kind-001--inode-kind-是唯一-file-type-truth)
统一拥有 immutable inode kind 与 Linux mode projection；eventfd、timerfd、epoll 与 fanotify group fd 使用
`Anon` kind，不再伪装成 `S_IFREG`。本 RFC 的 admission 必须读取该 truth，不能以 `FileOps` identity、path、
filesystem name 或 private downcast 建立第二份 file-kind policy。

本次 omega 合入的 UDP socket 使用独立 `InodeType::Socket` 与 Linux `S_IFSOCK` projection，并由
`fs::socket` owner持有自己的private syscall adapter/core边界。它不会进入本RFC的`S_IFREG` admission；新增
socket kind与adapter目录化都不改变POSIX holder、inode-associated grant domain或close cleanup target。

现有 [`FLOCK`](../../contracts/vfs/flock.md) 已拥有 opened-description-scoped whole-file flock
domain、cooperative wait 与 terminal holder cleanup。它为 recheck-notification 形状提供已验证的相邻经验，
但其 holder、whole-file grant、final-close cleanup 与 generic file-kind admission 都不是 POSIX record lock 的
可复用行为真相。

### Linux / POSIX 兼容参考

Linux asm-generic UAPI 把 `F_GETLK`、`F_SETLK`、`F_SETLKW` 定义为 5、6、7，并在原生 `struct flock` 中使用
`short l_type`、`short l_whence`、native `off_t` start/len 与 `pid_t l_pid`：
`xref:linux-6.6.32:include/uapi/asm-generic/fcntl.h#F_GETLK`、
`xref:linux-6.6.32:include/uapi/asm-generic/fcntl.h#struct flock`。

Linux generic POSIX path 以 `current->files` 作为 behavior owner，以 `current->tgid` 作为报告 PID，先把
`SEEK_SET/CUR/END` 与 signed length 规范化为绝对 range，再在 inode lock context 中裁决冲突、split 与 merge：
`xref:linux-6.6.32:fs/locks.c#flock64_to_posix_lock`、
`xref:linux-6.6.32:fs/locks.c#posix_test_lock`、
`xref:linux-6.6.32:fs/locks.c#posix_lock_file`。关闭任意 fd slot 时，Linux 用同一 files owner 删除该 inode
上的 POSIX locks，而不是等待 opened file description 最终释放：
`xref:linux-6.6.32:fs/open.c#filp_flush`、
`xref:linux-6.6.32:fs/locks.c#locks_remove_posix`。

这些引用固定比较基线，不要求 Anemone 复制 Linux `file_lock`、全局锁、链表、wait-for graph、filesystem
`lock` hook 或内部分配策略。

## 目标

- 为本地 `S_IFREG` 文件提供 POSIX process-associated byte-range advisory locks。
- 支持 RV64 / LA64 原生 64 位 `F_GETLK`、`F_SETLK`、`F_SETLKW` 与 `struct flock` ABI。
- 让 file-table sharing episode 拥有不可伪造的 holder identity，并明确 fork/share/unshare/exec/exit 边界。
- 让 inode-associated VFS POSIX domain 成为 grant ranges、mode、conflict 与 wait publication 的单一真相源。
- 支持 `F_RDLCK`、`F_WRLCK`、`F_UNLCK`、`SEEK_SET/CUR/END`、signed range、`l_len = 0` 到 EOF，
  以及 same-owner range assignment 的 replacement / split / merge。
- 让 `F_GETLK` 返回一个真实 conflicting range 或 `F_UNLCK`，并把 `l_pid` 限定为诊断报告而非 owner identity。
- 让 `F_SETLK` 对冲突返回 `EACCES` 或 `EAGAIN`；让 `F_SETLKW` 无 lost wake 地等待并接入 ordinary
  signal interruption / `SA_RESTART` replay。
- 关闭 holder 的任意相关 fd 时，同步提交该 holder 在目标 inode 上的全部 grant cleanup 与 recheck hint；
  不要求 producer 等待 waiter 运行或 local cleanup 完成。
- 保持 ordinary read/write advisory、local flock conflict namespace 独立以及 non-target file kind fail-closed。
- 以 focused kernel proof、`fcntl-test` 的 `posix-record-lock` suite、focused LTP group 与 RV64/LA64
  end-to-end runtime 形成彼此独立的最终证据。

## 非目标

- OFD record lock、lease、mandatory locking、remote/distributed lock propagation、recovery 或 backend RPC。
- 32 位 compat personality、独立 `F_GETLK64/F_SETLK64/F_SETLKW64` 命令面或 compat `struct flock64`
  translation。
- 目录、char/block device、pipe/FIFO、socket、VFS `Anon` control fd、`O_PATH` 或其它非本地 `S_IFREG`
  object。
- POSIX/OFD/flock 的统一 grant engine、通用 owner trait、owner enum、backend hook 或 dynamic cleanup registry。
- POSIX deadlock detection、wait-for graph 与 `EDEADLK` guarantee。
- waiter FIFO、公平性、无惊群、精确唤醒数量、固定复杂度或 throughput guarantee。
- close 驱动的强制 waiter cancellation、同步 waiter teardown 或 close/signal/restart 的唯一 race winner。
- ordinary read/write 的 mandatory enforcement。
- 把普通 kernel heap OOM 普遍转换为 `ENOLCK`；显式容量策略若后续确有需要，必须在 implementation
  resolution 中单独解析。

## 文档地图

RFC target：

- [目标和不变量](./invariants.md)
- [Tracking Issues](./tracking-issues.md)：当前无 active Apollyon / Keter
- [实施计划](./implementation.md)：Stage 0-2 Closed；Stage 3 Outline / Not Authorized
- [事务日志](../../devlog/transactions/2026-07-31-posix-record-lock.md)：Stage 0执行证据、Stage 1 resolution与
  Checkpoint 1A/1B closure，以及Stage 2 resolution和2A/2B closure evidence

Current contracts：

- [`VFS-FILE-KIND-001`](../../contracts/vfs/file-kind.md#vfs-file-kind-001--inode-kind-是唯一-file-type-truth)
- [`FLOCK-DOMAIN-001`、`FLOCK-WAIT-001`、`FLOCK-LIFECYCLE-001`](../../contracts/vfs/flock.md)
- [`OPENED-DESC-001/002/003`、`OPENED-DESC-RETIRE-001`、`OPENED-DESC-LIVENESS-001`](../../contracts/task/opened-description-lifecycle.md)
- [`SCHED-LATCH-001..003`](../../contracts/scheduler/latch-wait-round.md)
- [`SCHED-WAKE-001..004`](../../contracts/scheduler/wake-delivery.md)

背景材料：

- [背景材料索引](./backgrounds/index.md)
- [冻结的前置定位共识](./backgrounds/positionings.md)

公共外部源码证据：

- `xref:linux-6.6.32:include/uapi/asm-generic/fcntl.h#struct flock`
- `xref:linux-6.6.32:fs/locks.c#flock64_to_posix_lock`
- `xref:linux-6.6.32:fs/locks.c#posix_lock_file`
- `xref:linux-6.6.32:fs/locks.c#locks_remove_posix`

## 修订记录

2026-07-31 首次 target acceptance 形成 `R0`。Git 保存此前 Draft review 历史，不创建 `index-v1.md` 或
amendment 副本。

## Target Capability

### 原生 ABI 与 range normalization

首版只处理原生 `struct flock`：`l_type` 接受 `F_RDLCK/F_WRLCK/F_UNLCK`，`l_whence` 接受
`SEEK_SET/SEEK_CUR/SEEK_END`。`F_GETLK` 的输入 `l_type` 只接受 read/write query；`F_UNLCK` 只用于
`F_SETLK/F_SETLKW` 的 range assignment。

syscall ABI owner 先从 opened-description position 或 inode size owner 取得一次同步 snapshot，再把 signed
`l_start/l_len` 规范化为绝对、半开 range：

- `l_len > 0` 表示 `[start, start + len)`；
- `l_len < 0` 表示反向区间 `[start + len, start)`；
- `l_len = 0` 表示 `[start, EOF)`，其中 EOF 是持续开放的无穷上界，不冻结为规范化时的 file size；
- 规范化后的起点不能小于 0；base/start/end 算术溢出返回 `EOVERFLOW`，非法 whence/type 或越过零点的
  negative range 返回 `EINVAL`。

snapshot acquisition 与 grant commit 不是跨 owner 原子事务。并发 `lseek`、truncate、append 或 file-size
变化可以形成多个合法先后结果，但不能来自未同步读取。同一次 `F_SETLKW` invocation 完成 normalization 后，
notification/recheck 不重新解释 position 或 size；signal handler 返回后的 ordinary syscall replay 是新 invocation，
会重新 copy-in、lookup 与 normalization。

### Range assignment、冲突与查询

每个 holder 在一个 inode domain 上拥有分段 mode。对某 range 设置 read/write/unlock 是对该 holder 在该 range
上已有语义的整体替换：区间外语义保持，区间内改为请求 mode，由此产生 split、merge 或 mode replacement。
同 owner 的 ranges 不互相冲突，也不存在候选 grant；不同 owner 只按 overlap 与 mode 判断：read/read 不冲突，
只要任一侧是 write 就冲突。

`F_SETLK` 无冲突时提交完整 range assignment；冲突时不得部分修改调用 holder 的既有 ranges，并返回
`EACCES` 或 `EAGAIN`。`F_SETLKW` 使用相同 commit 规则，但在冲突时发布 operation-local wait 并循环重验；
unlock 不等待且对 holder 没有覆盖该 range 的部分是幂等 no-op。

`F_GETLK` 不修改 grant state。没有冲突时只把输出 `l_type` 改为 `F_UNLCK`，其它 input fields 保持不变；
存在冲突时返回任一真实 blocking segment，输出 `l_type`、`l_whence = SEEK_SET`、absolute `l_start`、
`l_len`（open-ended range 为 0）与诊断 `l_pid`。RFC 不承诺多个同时冲突 grant 的选择顺序。

### Holder 与生命周期

holder 是一份 file-table sharing episode capability，而不是 mutable lock collection：

- 同一 live fd table 的 `CLONE_FILES` participants 共享 holder；不同 fd、dup aliases 与 independent `open()`
  都通过同一 holder 进入 POSIX domain；
- 普通 fork 建立新的 table episode与新 holder，不继承父 holder 的 locks；复制出的 opened descriptions 不改变
  这一点；
- `close_range(UNSHARE)` 或成功 exec 只有在确实分离一个共享 table episode 时才建立新 holder；新 holder
  不继承旧 grants，旧 grants 留给仍参与旧 episode 的 sharers；
- table 未实际共享时，内部 container copy、`UNSHARE` no-op 或成功 exec 不得仅因实现重建对象而改变 holder；
- 成功 exec 在 holder 保持时保留 locks，但实际关闭的每个 `FD_CLOEXEC` fd 仍执行普通 close cleanup；
- 一个 sharer 退出只释放自己的 participation，不提前终结其他 sharers 的 holder；最后 participation/table
  teardown 必须通过 fd cleanup 收敛到没有该 holder 的 grant。

holder capability 只允许 same-owner comparison 与 cleanup attribution。它不保存 grants、ranges、waiters、
inode index 或 report PID，不暴露 table private lock，也不能由 fd number、TGID/PID、path、inode number、
raw pointer 或 ordinary storage refcount 伪造。

### Close handoff 与并发 operation

关闭 holder 内任意一个指向目标 inode identity 的 fd，会在 fd-table owner 的 slot-removal transaction 与 VFS
POSIX domain 之间触发窄 handoff：删除该 holder 在该 inode 上的全部 granted ranges，并提交一次 recheck
notification。cleanup 不等待 opened description terminal retirement；同一 opened description 的其它 fd 或
同一 holder 对该 inode 的其它独立 open 仍然存活，也不能保留这些 locks。

fd slot removal 与 range commit 必须共同保证两类收敛结果：operation 可以先建立有效 assignment，随后 close
cleanup 删除；或者 close handoff 先建立“本 binding 已关闭”的事实，仍依赖该 binding authority 的 operation
不得在 cleanup 后留下归属于同一 holder/inode 的持久 grant。该约束按 operation 实际使用的 binding 判断，
不以 syscall invocation 的开始时间建立 holder × inode epoch；同一 holder 通过另一个仍存活 binding 发起的
operation 可以在 cleanup 后独立线性化并重新取得 lock。

本文只冻结上述合法结果集合与共同最终状态。binding validation、domain mutation 及其私有重试或校正如何组成
serialization protocol，由首次同时接入 fd binding 与 domain mutation 的 Ready stage 根据 live source 解析；
不得因此把完整 fd-table guard 或 task private state暴露给 VFS，也不得把一次 close 扩张成阻止其它 live
binding 后续 assignment 的永久屏障。

本 RFC 不要求 close、signal 与 operation return 形成唯一全序。operation 可以在自己的 observation point 返回
普通结果、`EBADF` 或 `EINTR`/restart；唯一强结果是 close cleanup 的逻辑效果已提交，且已关闭 binding 不能
留下 late persistent grant。producer 不等待 waiter 运行、scheduler placement 或 operation-local resource cleanup。

### Wait、signal 与 ordinary restart

`F_SETLKW` 的 waiter publication 与 conflict/fd-binding predicate recheck 必须闭合 check-then-sleep 窗口。
grant mutation、unlock 与 close cleanup 只提交 recheck notification；notification 不转移 grant，不预选 waiter，
不决定 success/errno，也不保存 candidate state。

一个 blocking invocation 可以经历多轮先后相接的 wait，但同一 task 任一时刻只能拥有一个 active scheduler
wait round。上一轮必须 finish/cancel/retire 后才能开始下一轮；active round 内不得进入 sleepable lock slow path、
`Event::listen*()` 或其它 nested wait。waiter 自己拥有 listener/trigger 等 operation-local resources，并在普通
success、`EBADF`、signal 或 copy boundary failure 后清理。

signal 只有在 range assignment 尚未提交且 local wait resources 已清理后，才能让 syscall 返回 ordinary
restart carrier；没有 `SA_RESTART` 时用户看到 `EINTR`，允许 restart 时 signal finalizer 重放完整 `fcntl()`。
一旦 assignment 已提交，本次 syscall 返回 success，不能再改写成 interruption。replay 不保存旧 file、holder、
normalized range 或 snapshot，因而可以观察 handler 对 fd、用户内存、position 或 inode state 的修改。

### `l_pid` 报告边界

`l_pid` 是查询报告，不是 behavior identity。普通非共享 holder 的 conflicting range 报告提交该 range mode 的
thread-group ID；若一个 `CLONE_FILES` holder 横跨多个 thread group，则可以报告建立当前 conflicting segment
的任一 participant TGID。该值允许在 task 退出后 stale，也允许 same-owner replacement 后变化；它只能用于
`F_GETLK` copyout、日志与诊断，不能参与 conflict、cleanup、holder equality 或 lifecycle 决策。

### Admission 与 errno

首版 admission 只接受非 `O_PATH` fd 指向的本地 VFS `S_IFREG` object。有效 fd 但非目标 file kind 返回
`EINVAL`，不得成功但不生效；invalid fd 与 `O_PATH` 返回 `EBADF`。`F_SETLK/F_SETLKW` 的 read lock 要求
opened description 可读，write lock 要求可写，不满足时返回 `EBADF`；unlock 不要求对应 access mode。

ABI owner 使用以下 validation order：command decode；fd lookup / `O_PATH`；`struct flock` copy-in；type/whence
与 range normalization；VFS file-kind admission；set-operation access-mode validation；最后才允许 grant/query
或 wait publication。`F_GETLK` 的 copyout 失败返回 `EFAULT` 且不改变 grant state；所有 copy-in/validation
失败都必须发生在 grant/wait publication 之前。本 RFC 不为多个同时非法输入额外承诺超出该顺序的 errno
别名。

普通 heap OOM 沿用当前 kernel-fatal 工程边界，不伪造 `ENOLCK`。如果 implementation resolution 选择明确的
lock-record/operation 配额，其容量必须由 Kconfig 拥有、由 kernel compile-time assertion 验证合法性，资源
拒绝必须在 mutation 前返回 `ENOLCK`；不得以此为理由引入双重索引或预付通用 framework。

### Advisory 与 conflict namespace

POSIX record locks 不强制 ordinary read/write。现有 flock 与 POSIX record lock 使用两个独立 conflict
namespace，不共享 grants、waiters、cleanup 或 mode；这里的 namespace 是锁冲突域，不约束 Rust module 的物理
归类。未来 OFD lock 必须与 POSIX record lock 互相冲突，但该
能力到来前不预置 OFD owner variant。未来 RFC 必须读取当时的 live POSIX implementation，再决定如何形成
POSIX/OFD 的单一 record-lock conflict truth。

## 方案

### 状态与责任摘要

| 状态 / 责任 | 唯一 owner | 其它参与方边界 |
| --- | --- | --- |
| fd slot、binding、`FD_CLOEXEC` 与真实 table sharing topology | task file-state lifecycle owner | syscall 只取得 operation-local binding/capability |
| POSIX holder identity 与 participation episode | task file-state lifecycle owner | VFS 只接收 opaque holder capability |
| opened-description identity、position/status 与 target file | `ProcFile` / opened-file owner | normalization 只取同步 snapshot；holder 不由它派生 |
| inode kind 与 inode-associated identity | VFS inode owner | admission/domain 只读取，不缓存第二份 kind |
| holder × inode 的 granted ranges、mode、conflict 与 wait publication | inode-associated VFS POSIX domain | task/syscall/backend 不保存 grant mirror |
| close/unshare/exec/exit cleanup episode | task file-state protocol owner编排；VFS domain提交 range cleanup | producer只等待逻辑 cleanup与notification submission |
| active wait identity、completion 与 physical placement | scheduler wait core | POSIX waiter持窄、单轮 capability |
| copy-in/out、range normalization、access/errno 与 syscall completion | `fcntl` ABI operation owner | VFS core不读取用户指针、raw command或完整 task |
| `l_pid` diagnostic snapshot | grant segment内的报告字段 | 不参与行为决策，允许 stale |

### 内部边界

syscall adapter 把 raw `struct flock`、fd binding 与必要 snapshot 折成 normalized operation，再调用 VFS POSIX
facade。facade 接收 inode-associated target、opaque holder、operation-local binding validation capability 与
normalized range；它不得接收完整 `Task`、`FilesState`、`FileDesc`、用户指针或 fd-table private guard。

本段只冻结 capability boundary，不冻结 Rust 类型名、module layout、锁序、container、wait adapter 或 facade
签名。后续 implementation resolution 可以在同一 owner 内选择浅层目录化拆分，但不能借拆分把 flock 与
POSIX grants 合并、让 backend拥有 lock state，或建立 public extension framework。

## 接受边界

Draft 被接受为 `R0` 表示以下内容已经共同完成文档层裁决：首版 capability/non-goal、file-table holder 与
inode domain 的唯一 owner、range/ABI/lifecycle/wait/restart semantics、contract delta、最终 proof obligations、
feedback boundary，以及 `implementation.md` 中首个可执行阶段的完整 Ready definition 与 resolved manifest。
更远阶段可以保持 Outline，只需说明目的、依赖、受保护边界与后续解析触发点。

`implementation.md` 必须在 acceptance 前单独读取 live source、当前 diff、review 结论、runner/test assets 与
module pressure，解析首个完整 Ready stage、精确 write set、probe、命令，以及后续 Outline 和
`POSIX-LOCK-CUTOVER` 可达路径。R0 acceptance 不更新 current contracts、不启用 syscall，也不自动创建实现
事实或把 Ready 变成 Active；进入实现仍需建立 transaction 并取得独立启动授权。

以下变化必须回到本文 review 或 `Target Renegotiation Gate`，不能由 implementation preference 静默决定：

- holder 从 file-table sharing episode 改为 task/TGID/opened-description/fd 等其它 identity；
- grant、wait publication 或 cleanup 出现第二 owner，或 POSIX 与 flock 提前共用 conflict namespace；
- 改变 native ABI、file-kind admission、range/conflict、close/fork/exec/restart 或 `l_pid` visible boundary；
- 把 deadlock detection、OFD、remote backend、non-regular files 或 32-bit compat 纳入首版；
- 降低双架构 runtime、focused oracle、LTP 或 lifecycle/concurrency proof 的最终验收边界。

类型、helper、私有 module、容器、O(n) scan、wake-all、惊群与具体锁选择属于 implementation preference，只要
它们保持上述 target 且通过后续 Ready-stage review。若真实工程证据只能形成较弱能力，必须在 cutover 前停止，
提交证据并重新接受 target；不能把部分实现写成原 target 已关闭。

## 备选方案

### 直接扩展 `FlockDomain`

拒绝。flock holder 是 opened description、grant 是 whole-file、cleanup 在 terminal description retirement；
POSIX holder 是 file-table episode、grant 是 ranges、任意相关 fd close 都 cleanup。合并会制造 owner 与 lifecycle
双重解释。

### 先建立通用 file-lock framework

拒绝。首版没有 OFD、lease、remote 或 mandatory consumer，通用 owner enum、waiter hierarchy、backend hook
和 dynamic cleanup registry 都没有当前责任。未来 OFD 到来时，以真实的 mutual-conflict requirement 驱动最小
抽取。

### 以 TGID/PID 直接作为 holder

拒绝。数值 identity 可复用，不能表达 `CLONE_FILES` 跨 thread-group sharing，也不能表达 shared table 在
unshare/exec 后的 episode 分离。`l_pid` 只保留报告用途。

### 首版加入 deadlock detection

延期。纯 POSIX record-lock wait cycle detection 需要 waiter relation、cycle cleanup 与 `EDEADLK` proof，且不
参与无 cycle 程序的基础 grant correctness。首版对 cycle 可能持续阻塞，直到 signal 或外部变化打破等待；
验收必须把 deadlock cases 标为 excluded/TCONF/Not Run，而非 PASS。

## 风险

- **file-table lifecycle 当前缺少显式 holder episode truth。** 后续实现前必须从 live sharing/unshare/exec/exit
  path 解析单一 lifecycle surface；不得用 `Arc::strong_count()`、fd slot 数或 opened-description refcount
  反推行为 identity。
- **close 与在途 commit 跨 task/VFS owner。** 通过 binding-scoped validation、窄 cleanup handoff 与共同最终
  状态约束，不要求 producer同步取消 waiter，也不建立 holder × inode invocation epoch；具体验证、串行化与
  私有校正路线必须在首次同时接入 binding close 与 grant commit 的 Ready stage 解析并证明。
- **range replacement 容易产生 overlap/overflow 缺陷。** 由 normalized half-open/open-ended model、
  owner-local focused proof 与 userspace oracle共同覆盖，不让 raw UAPI range进入 VFS core。
- **wait source 可能触发 nested scheduler wait。** 首个接入 blocking wait 的 Ready stage 必须审计 begin 到
  retire 的完整调用链；若任何 post-begin path 会睡眠，停止并重排 owner boundary，不能放宽 wait-core
  assertion。
- **LTP `fcntl` family 混有 deadlock、mandatory 与 runner signal/wait 行为。** focused group 必须逐 case 分类；
  不能用整组 timeout 反推 record-lock core，也不能把明确非目标伪造成 PASS。

## 收口

当前 R0 target 已接受，transaction 已建立，Stage 0已按实现、验证与全零独立review关闭；执行事实只由
[transaction](../../devlog/transactions/2026-07-31-posix-record-lock.md)记录。Stage 0 contract cutover为
`None`；后续只读resolution把Stage 1拆为1A/1B。1A已完成行为保持的`fs::lock::flock` namespace alignment，
只对current contracts做locator-only更新；1B已完成inode-owned range domain、opaque holder接线与focused KUnit。
Stage 1 semantic cutover仍为`None`，全部prospective ID继续Not Effective；Stage 2已经独立解析为2A native
ABI/binding/nonblocking-query与2B blocking wait/signal replay。2A/2B现均已独立关闭，Stage 2 Closed；截至Stage 2
closure contract cutover继续为`None`，完整stacked candidate仍不是standalone/current支持。
最终 R0 implementation closure 至少需要：

- 新增 contract IDs 在 `POSIX-LOCK-CUTOVER` 原子写入 current contracts，Preserve IDs 经 source/review 证明
  未退化；
- focused kernel proof 覆盖 single grant truth、range assignment、closed-binding late-commit exclusion、其它
  live binding 的 post-cleanup assignment、fd reuse、close cleanup、no-lost-wake 与 no-nested-wait；
- `fcntl-test posix-record-lock` 覆盖 native ABI、range/query/conflict、holder/close/fork/exec、blocking、
  signal/restart 与 admissible races；
- focused LTP group 对 target cases 给出实际 PASS/TCONF/FAIL 分类，并把 deadlock/OFD/mandatory/compat 等
  非目标如实排除；
- RV64 与 LA64 分别形成真实 guest runtime，build 或单架构结果不能替代另一架构；
- full-diff review、register audit 与 transaction evidence 分别记录实现事实、Not Run 与剩余限制。

最终 stage 的命令、case 名单、重复轮数与日志判据尚未解析；它们必须由
[实施计划](./implementation.md) 的滚动 resolution gate 从 live source、runner 与固定测试资产冻结，不能从本
R0 target 正文推导 future write set 或执行授权。
