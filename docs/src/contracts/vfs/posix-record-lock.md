# VFS POSIX Record Lock 当前契约

**Contract ID：** `POSIX-LOCK-DOMAIN` / `POSIX-LOCK-WAIT`
**状态：** Active
**Owner：** inode-associated VFS POSIX record-lock domain
**参与领域：** VFS inode/file / task file-table episode / scheduler wait / signal restart / Linux `fcntl(2)` ABI
**覆盖范围：** local regular-file byte-range grant/conflict truth、query/assignment、blocking predicate recheck与notification-only progress
**不覆盖：** flock grant/wait/lifecycle、OFD locks、mandatory locking、deadlock detection、remote locks、32-bit compat、公平性或性能保证
**实现位置：** `anemone-kernel/src/fs/lock/posix.rs`、`anemone-kernel/src/fs/api/fcntl/posix_lock.rs`、`anemone-kernel/src/fs/inode.rs`
**依赖：** `FILES-POSIX-OWNER-001`、`POSIX-LOCK-LIFECYCLE-001`、`VFS-FILE-KIND-001`、`SCHED-LATCH-001..003`、`SCHED-WAKE-001..004`
**Pending Successor：** None
**最后核验：** 2026-08-01

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| POSIX holder identity与fd-binding liveness | task file-table episode owner | domain operation持`PosixLockBinding` | same-owner equality、commit前binding revalidation与cleanup attribution |
| holder x absolute range grant、mode与conflict predicate | 对应inode的`PosixLockDomain` | ABI/VFS caller只传normalized range、mode、report TGID与binding | `F_GETLK/F_SETLK/F_SETLKW`裁决、assignment与unlock |
| wait predicate与notification source | 对应`PosixLockDomain` | waiter持operation-local listener/active wait | 闭合check-then-sleep窗口并触发重新检查 |
| scheduler wait identity、logical completion与physical placement | scheduler wait core | POSIX operation只消费本轮result并执行final recheck | sleep/wake、signal interruption与ordinary replay |
| raw `struct flock`、fd admission与errno translation | `fcntl(2)` syscall adapter | VFS core不读取raw command、用户指针或relative whence | Linux ABI containment与native range normalization |
| local whole-file flock state | `FLOCK-*`既有domain | POSIX domain不持flock capability/state | 保持两个advisory conflict namespace独立 |

## POSIX-LOCK-DOMAIN-001 — Inode-associated domain 是唯一 grant 与 conflict truth

**规则：** 每个local VFS inode identity的POSIX record-lock domain唯一拥有
`Holder x AbsoluteRange -> Mode` grant relation、same-owner range assignment、different-owner conflict
predicate与query snapshot。一个holder在同一byte上至多有一种mode；same-owner request以assignment语义替换
目标range并自然split/merge/coalesce。fd slot、opened description、holder、syscall adapter、filesystem backend、
waiter与notification不得保存mode、candidate、conflict或range mirror。

硬链接、不同path、重复open或不同local filesystem provider只要到达同一VFS inode identity，就进入同一domain；
inode identity只决定domain association，不定义holder。`F_GETLK`的report TGID只服务诊断，不参与owner equality、
conflict、coalesce、cleanup或lifecycle决定。

**违反表现：** path/fd绕过冲突；同一byte出现same-owner重复mode；waiter预占grant；backend保存独立lock list；
独立open被误当成不同POSIX owner；temporary `(dev, ino)` key错误合并不同inode；diagnostic PID反向驱动behavior。

**验证 / Enforcement：** `PosixLockDomain`全部query/set/unlock/retire transition、inode association与raw-UAPI
containment source audit；常开canonicalization assertions、owner-local range/conflict/hard-link KUnit、19项focused
userspace oracle和双libc `fcntl14/fcntl14_64`。

**最初来源：** [RFC-20260731-posix-record-lock R0](../../rfcs/posix-record-lock/invariants.md#posix-lock-domain-001--inode-associated-domain-是唯一-grant-与-conflict-truth)。

**当前来源：** [POSIX-LOCK-CUTOVER](../../devlog/transactions/2026-07-31-posix-record-lock.md#posix-lock-cutover--2026-08-01)。

## POSIX-LOCK-WAIT-001 — Wait publication 只服务 predicate recheck

**规则：** blocking operation在domain serialization下完成conflict与fd-binding predicate check并发布waiter，
闭合check-then-sleep lost-wake window；等待期间不持有domain guard。unlock、range replacement、grant cleanup和
其它可能改变eligibility的producer在guard外提交recheck notification。notification不转移grant、不表示syscall
success/errno、不预选waiter，也不保存active/completed truth。

同一invocation可以顺序建立多轮wait，但同一task任一时刻至多发布一个active scheduler wait identity；每轮必须
在下一轮begin前finish/cancel/retire。active round不得进入普通sleepable lock slow path、第二个`Event::listen*()`
或其它nested wait。operation owner清理listener/trigger/binding capability并执行final recheck；producer只等待
notification submission，不等待waiter execution或physical placement。

**违反表现：** conflict check与park之间lost wake；wake直接授予range或决定errno；多个task共享waiter state；
active round内nested scheduler wait；close等待task运行；stale trigger完成未来round；跨park持domain guard。

**验证 / Enforcement：** `PosixLockDomain::set_binding()`的listener/predicate loop、guard-out publish、
`Event`/active-wait lifecycle与signal/restart source audit；owner-local no-lost-wake/cleanup KUnit及focused blocking、
close-while-waiting、`EINTR`、`SA_RESTART`与full replay regressions。

**最初来源：** [RFC-20260731-posix-record-lock R0](../../rfcs/posix-record-lock/invariants.md#posix-lock-wait-001--wait-publication-只服务-predicate-recheck)。

**当前来源：** [POSIX-LOCK-CUTOVER](../../devlog/transactions/2026-07-31-posix-record-lock.md#posix-lock-cutover--2026-08-01)。
