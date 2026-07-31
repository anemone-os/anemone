# File-table POSIX Lock 当前契约

**Contract ID：** `FILES-POSIX-OWNER` / `POSIX-LOCK-LIFECYCLE`
**状态：** Active
**Owner：** `task::files` file-table sharing lifecycle 与 fd-binding removal protocol
**参与领域：** task file-table episode / VFS POSIX record-lock domain / exec / close / exit
**覆盖范围：** POSIX holder identity、episode participation/split、operation-local fd binding与任意相关fd removal cleanup handoff
**不覆盖：** opened-description identity/lifetime、flock retirement、POSIX range/conflict truth、raw `fcntl(2)` ABI
**实现位置：** `anemone-kernel/src/task/files/episode.rs`、`anemone-kernel/src/fs/lock/posix.rs`
**依赖：** `OPENED-DESC-001/002/003`、`OPENED-DESC-RETIRE-001`、`OPENED-DESC-LIVENESS-001`、`POSIX-LOCK-DOMAIN-001`、`POSIX-LOCK-WAIT-001`
**Pending Successor：** None
**最后核验：** 2026-08-01

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| file-table sharing episode、participation与POSIX holder identity | `task::files::FilesState` lifecycle owner | task持episode participation；VFS operation持opaque holder clone | fork/share/unshare/exec/exit topology与same-owner equality |
| fd slot publication/removal | episode-owned `FileTable` | syscall持fd key或operation-local binding | binding liveness、close/replacement/CLOEXEC/teardown cleanup attribution |
| holder x inode range relation | 对应inode的VFS POSIX domain | task-files只传窄`PosixLockBinding` | range conflict、assignment、query与removal |
| slot-removal cleanup episode | task file-state protocol owner编排；VFS domain执行range cleanup | removal path在private guard外移交binding | 删除holder在目标inode上的全部旧grant并提交recheck hint |
| opened-description lifecycle与flock retirement | `OPENED-DESC-*` / `FLOCK-*`既有owner | POSIX binding只借用目标file identity | 保持POSIX holder与opened-description/flock identity分离 |

## FILES-POSIX-OWNER-001 — File-table sharing episode 是唯一 holder identity

**规则：** POSIX holder由task file-state lifecycle owner为一个真实file-table sharing episode创建和终结。
共享同一live table的participants使用同一holder；普通fork建立新holder且不继承grants。只有真实shared episode
在unshare或成功exec时split为不继承旧grants的新holder；unique table的container replacement、copy或no-op
unshare不得改变holder。单一sharer exit只detach participation，最后participant teardown才终结episode并清理
remaining bindings。

holder是opaque capability，只表达same-owner equality、operation authority与cleanup attribution；不得由fd、
TGID/PID、path、inode number、opened-description identity、raw pointer、`Arc` strong count或published-ref count
派生，也不得保存grant、range、waiter、inode index、report PID或fd-binding mirror。

**违反表现：** 普通fork继承父锁；`CLONE_FILES` sharers互相冲突；unique table重建导致holder变化；一个sharer
exit提前清除共享episode的grant；PID reuse命中旧owner；holder反向保存或驱动VFS range state。

**验证 / Enforcement：** `FilesState` attach/fork/split/detach、exec与`close_range(UNSHARE)` source audit和常开
lifecycle assertions；owner-local KUnit与focused fork/share/unshare/exec/exit regressions。

**最初来源：** [RFC-20260731-posix-record-lock R0](../../rfcs/posix-record-lock/invariants.md#files-posix-owner-001--file-table-sharing-episode-是唯一-holder-identity)。

**当前来源：** [POSIX-LOCK-CUTOVER](../../devlog/transactions/2026-07-31-posix-record-lock.md#posix-lock-cutover--2026-08-01)。

## POSIX-LOCK-LIFECYCLE-001 — 任意相关 fd removal 清除旧 grant 并排除 closed-binding late grant

**规则：** holder内任意live fd slot只要指向目标inode，从table publication移除时，task file-state protocol
owner就必须在private guard外exactly-once进入窄POSIX cleanup handoff。VFS domain同步删除该holder在该inode上的
全部granted ranges并提交recheck notification；cleanup不等待opened-description final release，也不因同一
opened description或holder仍有其它相关fd而保留旧grant。

close与range commit必须收敛为两种合法最终状态：assignment先提交时cleanup随后删除；cleanup先关闭本次binding
authority时，依赖该binding的in-flight operation不得在cleanup后留下persistent grant。同一holder通过另一个仍
live binding发起的operation可以在cleanup后重新assignment；本规则不建立holder-wide close epoch，也不规定
success、`EBADF`、signal outcome与close return的唯一全序。

**违反表现：** close任一相关fd后旧grant仍存在；等到`ProcFile` final release才cleanup；closed binding在cleanup
后重新插入grant；阻止其它live binding后续合法assignment；持fd-table guard回调VFS；close等待waiter运行；或
用holder-local `has_locks`/inode mirror决定是否handoff。

**验证 / Enforcement：** `PosixLockBinding` creation/revalidation、全部fd-removal caller、
`FilesState::finish_removed_binding(s)`与`retire_posix_locks()` source audit；owner-local late-commit/cleanup KUnit，
focused ordinary close、dup3、close-range、CLOEXEC、final teardown与64轮close/set race。

**最初来源：** [RFC-20260731-posix-record-lock R0](../../rfcs/posix-record-lock/invariants.md#posix-lock-lifecycle-001--任意相关-fd-close-清除既有-grants-并排除-closed-binding-late-grant)。

**当前来源：** [POSIX-LOCK-CUTOVER](../../devlog/transactions/2026-07-31-posix-record-lock.md#posix-lock-cutover--2026-08-01)。
