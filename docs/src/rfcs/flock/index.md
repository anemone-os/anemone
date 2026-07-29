# RFC-20260728-flock

**状态：** Accepted for Implementation / Stage 0 Closed / Stage 1 Ready
**修订：** R0
**负责人：** doruche
**最后更新：** 2026-07-29
**领域：** fs / VFS / task files / scheduler wait / syscall ABI
**事务日志：** [2026-07-29 Flock](../../devlog/transactions/2026-07-29-flock.md)
**影响契约：** Preserve `OPENED-DESC-001/002/003`、`OPENED-DESC-LIVENESS-001`、
`SCHED-WAKE-001..004`；拟 Introduce `OPENED-DESC-RETIRE-001`、`FLOCK-DOMAIN-001`、
`FLOCK-WAIT-001`、`FLOCK-LIFECYCLE-001`。完整 R0 delta 见
[Contract Impact](./invariants.md#contract-impact)。
**开放问题：** None；当前 design findings 均已 neutralize，历史与 2026-07-29 cooperative direction
correction 见 [Tracking Issues](./tracking-issues.md)。
**下一步：** Stage 1已解析为`Ready / Not Active`；等待开发者独立授权从Checkpoint 1A进入执行。最终RV64 /
LA64 runtime由开发者本人运行；全部新contract ID继续Not Effective，不得执行`FLOCK-CUTOVER`。

## 文档状态

本文是第一版本地 `flock(2)` 的公共 canonical R0 accepted target。它陈述 target、contract delta、
correctness boundaries 与 acceptance boundary；Checkpoint 0S 完成 `task::files` 行为保持型结构拆分，
Checkpoint 0A 建立private inode domain与cooperative-retirement，Checkpoint 0B 增加Linux syscall ABI、双层
userspace wrapper与focused consumer。Checkpoint 0C已用RV64 wrapper关闭KUnit、focused oracle、当前`sys`
profile与正常关机证据，并用LA64 build/rootfs composition关闭该架构的build floor；LA64 runtime仍Not Run。
当前纵切不能作为partial flock capability宣称，current contract也尚无对应effective rule。

2026-07-29 Draft review 已废弃此前 close-driven precise cancellation 方向。旧 implementation stages、
probe 与 manifest 不再有效；后续独立 implementation resolution 从 live source 形成当前 Stage 0 Ready。
本轮独立 R0 review 未发现 Apollyon、Keter 或 Euclid，开发者授权建立 transaction、激活 Stage 0 并依次完成
Checkpoint 0S、0A、0B、0C并关闭Stage 0。后续独立`0 -> 1 Implementation Resolution Gate`已把Stage 1解析为
`Ready / Not Active`：只补focused/LTP acceptance assets，由开发者运行两架构wrapper，再完成current-contract
交接。本轮resolution不修改register/current contract，也不执行`FLOCK-CUTOVER`。

R0 acceptance只接受target与contract delta，不把它们写成effective contract。Stage 0 activation、
Checkpoint 0S/0A/0B/0C closure与后续Stage 1 resolution已分别记录在transaction；Stage 1达到Ready仍不自动
进入Active。

## 摘要

本 RFC 提议为 Anemone 增加 Linux 风格、本机范围内的 whole-file advisory `flock(2)`。首版支持
`LOCK_SH`、`LOCK_EX`、`LOCK_UN` 与 `LOCK_NB`，以 opened file description 为 grant holder，以 local VFS
file identity 聚合同一对象上的冲突，并由 VFS-owned flock domain 单独拥有 grant、冲突判断、等待 predicate
与 cleanup。

final close 同步终结 opened-description liveness，并通过窄 handoff 删除该 holder 已存在的 grant；对已经发布
等待的 operation，retirement 只提交 fire-and-forget recheck notification。waiter 在自己的检查点重验
liveness、冲突与 signal，并自行退出和清理。close 不等待 waiter 运行，不精确裁决 retirement、signal、restart
与 ordinary operation 的胜负，也不为这些边角结果建立强制取消协议。

ordinary local filesystem backend 不感知 flock。NFS、CIFS、mandatory locking、lease、deadlock detection、
POSIX/OFD record-lock 统一以及通用 file-lock engine 均不在本 RFC 范围内。

## 背景

### Anemone 当前事实

当前树没有 `flock` syscall implementation；0A 只有private VFS flock grant/wait/cleanup substrate，尚无userspace
入口或effective cleanup contract。
`task::files` 已经提供本 RFC 所需的 opened-description baseline：

- `ProcFile` 是当前 opened file description；dup 与非 `CLONE_FILES` fork 发布新 fd slot，但共享同一
  `ProcFile`。
- `ProcFile::description_refs` 单独编码 `Unpublished -> Live(n) -> Retired`。published fd-slot release 而不是
  syscall-local strong borrow 决定 terminal retirement。
- `OpenedDescriptionCapability` / operation-local lease 允许比较 identity、短暂访问 target 并在 commit 前重验
  terminal liveness；它不允许 consumer 取得 lifecycle word 或延迟 retirement。
- `FileDescOps::final_release` 是 opened description 创建时固定的单 hook，不是可动态追加、组合或取消的
  teardown registry。
- current contract 尚无 terminal retirement 后进入 VFS 并删除 flock grant 的 mandatory handoff。

上述 effective 规则由
[Opened-description lifecycle current contract](../../contracts/task/opened-description-lifecycle.md) 拥有。
flock R0 不复制 published-ref truth，也不把现有单 hook 扩张成 feature registry。

VFS `File` 持有 `PathRef` 并能取得稳定 `InodeRef`；硬链接、不同 path 和不同独立 `open()` 可以到达同一 inode
identity。因而 fd number、path 或临时 `(dev, ino)` key 都不足以成为本地 flock 冲突域的行为真相。

### Linux 兼容参考

Linux asm-generic UAPI 定义 `LOCK_SH / LOCK_EX / LOCK_NB / LOCK_UN` 与 `__NR_flock = 32`：
`xref:linux-6.6.32:include/uapi/asm-generic/fcntl.h#LOCK_SH`、
`xref:linux-6.6.32:include/uapi/asm-generic/unistd.h#__NR_flock`。

Linux 6.6.32 的 generic path 把 opened file object 写入 flock owner，将 whole-file request 放入
inode-associated flock list，并把同一 file object 的重复请求识别为同一 owner：
`xref:linux-6.6.32:fs/locks.c#flock_make_lock`、
`xref:linux-6.6.32:fs/locks.c#flock_locks_conflict`、
`xref:linux-6.6.32:fs/locks.c#flock_lock_inode`。final file release 通过同一 owner 删除 flock grant：
`xref:linux-6.6.32:fs/locks.c#locks_remove_flock`。

同一 generic path 的 flock context / grant record 分配失败返回 `ENOMEM` 而非 `ENOLCK`：
`xref:linux-6.6.32:fs/locks.c#flock_lock_inode`。这只说明 `ENOLCK` 不是该版本 generic path 必须复制的行为；
Anemone 的通用内核堆 OOM 仍按本 RFC 的非目标处理。

Linux `sys_flock` 在 fd lookup 后持有 file reference，因此另一个线程的 `close()` 不会把阻塞中的调用变成
精确 close-driven cancellation。Anemone 不复制该内部 lifetime：syscall-local borrow 仍不延迟 semantic
final close；但本 RFC 也不再为 concurrent final close 建立比 Linux 更强的精确 `EBADF`、waiter teardown 或
restart identity 保证。普通 `SA_RESTART` 继续使用 syscall replay。

这些引用只固定 Linux ABI 与可见语义的比较基线，不要求 Anemone 复制 `struct file_lock`、链表、
`file_operations::flock` 或 waitqueue。Linux 自身也把 local flock 与 fcntl record locks 分开处理；历史说明见
`xref:linux-6.6.32:Documentation/filesystems/locks.rst#L43-L54`。

## 目标

- 提供本机范围内、whole-file、advisory 的 `flock(2)`。
- 支持 `LOCK_SH`、`LOCK_EX`、`LOCK_UN` 与 `LOCK_NB` 的 Linux-compatible 基本 ABI。
- 让 opened file description 成为 grant semantic holder，保持 dup/fork/exec/final-close 可见语义。
- 让 inode-associated、VFS-owned flock domain 成为 grant relation、mode、冲突 predicate 与 wait publication
  的单一真相源。
- 建立无 lost wake 的 blocking acquire / conversion，并接入 ordinary signal interruption 与 `SA_RESTART`。
- final close 同步删除既有 grant，并保证 retired holder 不能遗留持久 grant。
- retirement 通过 notification 请求 waiter 协作式重验，但不等待 waiter 执行或物理清理。
- 为所有有效、非 `O_PATH` 的 local VFS `File` 提供 generic default，不按 file kind 或读写 open mode建立
  首版白名单。
- 用 LTP、focused ABI oracle、owner-local tests、source audit 与多架构 runtime 共同证明 target，而不是把
  基础 LTP case 当成完整 owner/lifecycle 证明。

## 非目标

- NFS、CIFS、Ceph、FUSE 或其它 remote/distributed flock propagation、server owner 与 recovery。
- mandatory locking、lease、deadlock detection、FIFO、公平性或无惊群保证。
- POSIX record lock、OFD record lock、range split/merge、区间树或三类锁的统一冲突矩阵。
- 为推测性 remote consumer 预置 backend hook、registry 或双路径 dispatch。
- 建立通用 lock record、owner trait、waiter/grant state machine 或公共 file-lock framework。
- 让普通 read/write 因 advisory grant 被 VFS 强制拒绝。
- 为通用内核堆 OOM 提供可恢复的 `ENOLCK`，或为制造该错误路径引入专用容量模型和 production test control。
- 复制 Linux file reference 对并发 final close / in-flight syscall 的内部生命周期。
- 保证 concurrent final close 立即取消 blocked flock、等待 waiter 返回、完成 physical CPU placement，或在
  retirement、signal、restart 与 ordinary operation 之间提供唯一精确胜负。
- 跨 signal handler 保存原 opened-description identity；ordinary restart 可以重新执行 fd lookup，本文不额外
  防止 signal handler 关闭并复用 fd number。
- 把 `FileDescOps::final_release` 扩张为动态 observer registry，或增加 per-description flock callback、
  `has_flock` mirror 与其它 feature registration state。
- 迁移 `ProcFile` semantic owner、重定义 fd-table publication owner，或把本 RFC 的窄 flock retirement handoff
  扩张为通用 backend / feature lifecycle framework。
- 把 `implementation.md` 中为当前 Ready stage 冻结的类型、API、容器、锁、module、write set或测试命令提升为
  target guarantee；后续 stage仍通过resolution gate基于live evidence解析。

## 文档地图

RFC target：

- [目标与不变量](./invariants.md)
- [Tracking Issues](./tracking-issues.md)
- [Implementation Resolution 状态](./implementation.md)

Current effective baseline：

- [Opened-description lifecycle](../../contracts/task/opened-description-lifecycle.md)
- [Asynchronous wake delivery](../../contracts/scheduler/wake-delivery.md)

公共外部源码证据：

- `xref:linux-6.6.32:include/uapi/asm-generic/fcntl.h#LOCK_SH`
- `xref:linux-6.6.32:include/uapi/asm-generic/unistd.h#__NR_flock`
- `xref:linux-6.6.32:fs/locks.c#sys_flock`
- `xref:linux-6.6.32:fs/locks.c#flock_lock_inode`
- `xref:linux-6.6.32:fs/locks.c#locks_remove_flock`
- `xref:linux-6.6.32:Documentation/filesystems/locks.rst#L43-L54`

## 修订记录

| 修订 | 日期 | 语义摘要 | Review / transaction |
| --- | --- | --- | --- |
| R0 | 2026-07-29 | 接受 cooperative retirement、本地 generic whole-file flock、opened-description holder、inode-associated single grant truth、ordinary restart 与原子 `FLOCK-CUTOVER` acceptance boundary | [2026-07-29 Flock transaction](../../devlog/transactions/2026-07-29-flock.md) |

## Target Capability

### 操作与冲突

- `LOCK_SH`、`LOCK_EX`、`LOCK_UN` 是三个基本 operation；`LOCK_NB` 只修饰 acquire 或 conversion。
- operation 未包含、或同时包含多个基本 operation，或者带有未知 flag 时返回 `EINVAL`。
- 同一 file identity 可以存在多个不同 owner 的 shared grant；exclusive grant 与其它 owner 的 shared 或
  exclusive grant 冲突。
- 同一 opened-description owner 在同一 file identity 上最多持有一种 mode。重复请求相同 mode 是
  idempotent；请求另一 mode 是 conversion，不是第二份 grant。
- SH/EX conversion 使用 Linux/BSD 的非原子语义：旧 mode 先撤销，再竞争新 mode；期间其它 waiter 可以获得
  grant，`LOCK_NB` conversion 失败后旧 grant 可以已经丢失。
- `LOCK_UN` 只删除调用 fd 所属 opened-description owner 的 grant；owner 没有 grant 时不影响其它 owner。

### ABI、等待与并发 final close

- 无效 fd 与 `O_PATH` fd 返回 `EBADF`。其它 local VFS file 不因 inode kind、anonymous/control provenance
  或 fd 的读写 open mode 被拒绝。
- `LOCK_NB` 遇到冲突返回 `EAGAIN / EWOULDBLOCK`。
- blocking acquire / conversion 可由 signal 打断；不 restart 时返回 `EINTR`，允许 `SA_RESTART` 时重新执行
  syscall 并申请 normalized target mode。
- ordinary restart 不保存原 opened-description identity。signal handler 若关闭并复用 fd number，重放后的
  fd lookup 可以观察当前 fd-table 内容；本文不引入专用 restart carrier 改变该行为。
- concurrent final close 不具有唯一 errno 或完成顺序。operation 若先完成普通 domain outcome，可以返回普通
  结果，随后 cleanup 删除仍存在的 grant；operation 若在 commit 前观察到 `Retired`，停止修改 grant 并返回
  `EBADF`；signal 若先完成当前 wait，则走普通 `EINTR` / restart 路径。
- 上述 race 中的普通 success 不承诺 grant 在 concurrent final close 后继续存在，`EBADF` 也不承诺精确压过
  已经提交的 signal outcome。唯一强结果是 final close 后该 retired holder 不遗留持久 grant。
- 通用堆 OOM 不形成本 RFC 的可恢复 errno guarantee。若未来自然出现独立、可恢复的锁资源耗尽边界，必须由
  对应 RFC revision 定义 errno、已有 grant 结果与验证责任。

### Opened-description 可见语义

- flock owner 是 opened file description，不是 pid、task、fd number、fd-table slot、path 或 inode。
- dup/fork alias 共享同一 owner；任一 alias 都可以 conversion 或 unlock。
- 同一进程中的独立 `open()` 产生不同 owner，并在同一 file identity 上相互冲突。
- 关闭单个 alias 不释放仍由其它 published alias 持有的 grant；最后一个 published alias 的 terminal
  retirement 自动删除该 owner 的全部 flock grant。
- flock 跨 `execve()` 保留；`FD_CLOEXEC` 只有在 exec 关闭最后一个 alias 时才触发 terminal cleanup。
- syscall-local borrow 或 operation-local lease 不延迟 semantic final close，也不能让 retired owner 遗留
  持久 grant。

### Advisory、namespace 与 backend

- 普通 read/write 不参与 flock protocol 时，不因 grant 被拒绝。
- local flock 与 POSIX/OFD record locks 使用独立 namespace，不共享 grant 或 conflict state。
- ordinary local filesystem backend 不保存 grant、owner、waiter 或 cleanup state；syscall adapter 通过 VFS
  flock facade 进入 generic local domain。
- remote filesystem 若未来需要 RPC、server-side owner/recovery、remote errno、mandatory semantics 或与
  record locks 交互，必须建立 follow-up RFC，重新解析 owner、wait/cancel、cleanup 与 contract delta。

## 方案

### 状态与责任摘要

| 状态 / 责任 | 唯一 owner | 其它参与方边界 |
| --- | --- | --- |
| fd number、slot publication、fd-local flags | `task::files` 的 `FilesState / FileDesc` | syscall 不长期保存 fd number 作为 holder identity |
| opened-description identity、publication liveness、terminal retirement | `ProcFile` lifecycle owner | VFS 只持 opaque identity/liveness capability 或 retirement context |
| terminal retirement episode、窄 VFS handoff 与 static hook 顺序 | `ProcFile` lifecycle owner | VFS flock owner只完成grant cleanup与recheck submission |
| local file identity 与 inode lifetime | VFS inode owner | flock domain 以同一 inode identity 聚合冲突 |
| holder × file identity grant relation、mode 与 conflict predicate | inode-associated VFS flock domain | `ProcFile`、fd slot 与 syscall 不保存 mode mirror |
| operation、wait predicate、grant commit/removal 与 recheck source | VFS flock protocol owner | waiter只把notification当hint并自行重验 |
| wait identity、signal outcome 与 physical wake delivery | scheduler / wait owner | close不观察或等待placement结果 |
| Linux flag/errno 解析与 fd admission | syscall ABI adapter | adapter不拥有grant、waiter或lifecycle truth |

flock grant 是 `OpenedDescription × FileIdentity -> Mode` 关系。opened description 决定 alias sharing、
explicit unlock 与 terminal release；inode-associated domain 让同一 file identity 上的所有 holder 在唯一裁决点
相遇。domain 与 inode identity 关联不表示 inode 成为 grant holder，也不预先选择 state 字段落点。

syscall adapter 在 fd lookup 与 ABI validation 后，把 normalized request、VFS target 与 opaque
opened-description capability/context 交给 VFS facade。它不得把完整 `ProcFile`、`FileDesc`、fd number、task
state 或 fd-table private lock 下沉到 VFS。

### Cooperative retirement

terminal retirement 由 `ProcFile` lifecycle owner 编排。`FilesState` 在自己的 private lock 下撤销最后 slot
publication，释放 guard 后首次提交 `Live(1) -> Retired`，再 exactly-once 进入窄、不可失败、不可动态注册的
VFS flock retirement handoff。

VFS flock owner 删除该 holder 当前存在的 grant；没有 grant 时 removal 可以 no-op。无论是否删除 grant，
retirement 都必须为目标 flock domain 中已经发布的 waiter 提交一次 recheck notification，使它们有机会观察
`Retired`。notification 只是 progress hint，不转移 grant，不决定 syscall errno，也不要求 waiter 已经运行、
返回或完成物理资源清理。

handoff 在 grant cleanup 与 notification submission 后返回，随后才运行现有 creation-time
`FileDescOps::final_release`。这只是 flock-specific cross-owner cleanup，不是 backend hook、dynamic observer
registry 或可供 future feature 自然加入的通用 retirement framework。

### Grant-state ordering

future implementation 必须证明 retired holder 不会留下持久 grant，但不需要为整个 syscall outcome 建立精确
全序。允许的 grant-state 收敛只有两类：

1. operation 先完成 domain-owned grant mutation，retirement cleanup 随后删除该 holder 仍存在的 grant；
2. retirement cleanup 已进入 domain，后续 operation 在 commit 前观察 terminal liveness，不再建立 grant。

这项排序只保护 grant truth 与 final-close cleanup。operation 返回普通结果、`EBADF`、`EINTR` 或进入 ordinary
restart 的选择由它实际完成的 domain/liveness/signal observation 决定；RFC 不要求 close 等待该结果。

### Wait / wake

- waiter publication 与 predicate recheck 必须闭合 check-then-sleep lost-wake window。
- predicate 至少覆盖 conflict 与 holder liveness；notification 只请求重新检查，不能直接代表 grant transfer、
  success 或 `EBADF`。
- unlock、conversion old-mode removal与grant cleanup使多个shared waiter newly eligible时，必须让这些waiter
  最终获得recheck机会，但不承诺FIFO、公平性或无惊群。
- retirement 即使没有删除 grant，也要提交 recheck hint；否则已经发布 listener 的 operation 可能永远无法
  观察 `Retired`。
- waiter owner 负责 signal/normal return 后的 operation-local wait cleanup。retirement producer 不等待 waiter
  execution，也不维护第二份 active/completed truth。

### Cleanup 与调用方向

- final-close grant cleanup 是显式 semantic teardown，不依赖 `Drop`、memory last-drop 或 deferred disposal。
- fd-table mutation 先撤销 publication，再在 fd-table private lock 外进入 flock retirement handoff。
- VFS 不回取 task/fd private lock，也不读取或复制 `description_refs` lifecycle word。
- handoff 返回前完成 grant cleanup 与 recheck submission；task physical placement、waiter return 和普通
  operation-local resource destruction可以随后发生。
- 现有 static `FileDescOps::final_release` 保持 creation-time single hook；不得增加 dynamic observer registry、
  per-feature callback slot、`has_flock` mirror 或让 `FilesState` 识别 flock policy。

## 接受边界

本 RFC 的 `R0` semantic revision只接受target与contract delta，不执行docs-only cutover，也不把首个Ready
stage的implementation choices提升为target；但R0 review必须同时确认该stage提供可达路线。当前Draft review要求：

1. cooperative retirement、grant-state correctness、ABI admissible outcomes 与 acceptance boundary 自洽；
2. Contract Impact 覆盖所有 affected IDs，并保持 current effective / accepted target 分离；
3. tracking 中不再存在会改变 target、owner、ABI、contract 或 acceptance 的 Apollyon / Keter；
4. implementation preferences 未被提升为 target；R0 acceptance时`implementation.md`的Stage 0已完整解析为
   `Ready`，Stage 1保持`Outline`，后续独立resolution再依据Stage 0 live evidence将其展开；
5. `Ready`、R0 acceptance、transaction bootstrap 与 `Active` authority 保持分离。

2026-07-29 的独立 implementation-resolution 任务满足首个 `Ready` 要求；本轮后续独立 review 接受 R0，
transaction 已建立且开发者已明确授权 Stage 0 Active。Checkpoint 0S、0A 分别按独立授权关闭；0A closure不授权
后续checkpoint。

最终 implementation closure 的证据范围至少包括：

- LTP `flock01`、`flock02`、`flock03`、`flock04`、`flock06` 的基础 ABI 与 conflict regression；
- representative local files、`O_PATH` rejection 与 generic path source audit；
- dup、fork、exec / `FD_CLOEXEC`、hard link、independent open、conversion 与 unlock；
- SH/SH、SH/EX、EX/SH、EX/EX、blocking wake、multiple shared waiters 与 `LOCK_NB` failure；
- non-restart `EINTR` 与普通 `SA_RESTART` replay；
- single-alias close、final-close grant cleanup、concurrent operation 不遗留 retired-holder grant，以及
  no-grant blocked waiter 能通过 recheck hint 获得 progress；
- source/owner review 证明 waiter cleanup 没有成为 close completion 的同步依赖。

并发 final close acceptance 不断言唯一 errno、fd-reuse 隔离、waiter 已运行或固定 close-to-wake latency。
agent-run、developer-run 与 Not Run evidence 必须在 transaction 中分别记录；R0 不预先声称任何平台
runtime 已验证。

## 备选方案

### Linux-like operation pin

通过 syscall-local semantic hold 延迟 final retirement，可以让在途 flock 自然遵循 Linux file-reference
lifetime并减少 close-driven coordination。但这会改变 current `OPENED-DESC-001` / liveness baseline，并让所有
opened-description consumer重新证明borrow与final release，当前不采用。

### Flag-only retirement without notification

只设置 `Retired` 而不提交 recheck hint，看似更少机制，但 waiter 在最后一次 predicate check 后可能永久睡眠。
如果仍要求 blocked operation 最终观察 retirement，这不是可接受的 cooperative protocol。

### Precise close-driven cancellation

为 final close、signal、restart、listener cleanup 与 syscall return建立精确全序，可以提供更强的确定性，但
不是基础 flock ABI 所需，并显著扩大 owner、证明与 acceptance surface。2026-07-29 Draft review 已拒绝首版
采用该方向。

### Generic file-lock framework

flock、POSIX record lock 与 OFD record lock 的 holder、range、close cleanup 与 deadlock semantics不同。没有
第二个 concrete consumer 前不提取通用 engine。

## 风险

- retirement notification可能造成惊群；首版接受该性能代价，优先保持单一predicate与直接recheck协议。
- concurrent final close 的errno不是确定性保证；focused tests必须验证admissible outcome与无grant遗留，不能
  把某次race结果固化为ABI。
- fixed handoff若被误扩张会形成feature lifecycle registry；Contract Impact与禁止退化项将其限制为flock关系。
- future remote filesystem语义可能不同；必须通过follow-up RFC重新定义而不是复用local假设。

## 收口

当前为R0 / Accepted for Implementation；Stage 0已Closed，Stage 1为`Ready / Not Active`。RV64 Stage 0
runtime与LA64 build/composition evidence已经记录，但尚无contract cutover；`OPENED-DESC-RETIRE-001`与全部
`FLOCK-*` IDs保持Not Effective。Stage 1执行、开发者双架构runtime handoff与`FLOCK-CUTOVER`仍需后续独立授权。
