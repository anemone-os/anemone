# Named FIFO Open 与 Lifecycle 当前契约

**Contract IDs：** `PIPE-FIFO-OPEN-001`、`PIPE-FIFO-LIFECYCLE-001`
**状态：** Active
**Owner：** VFS open orchestration拥有final admission/fd publication；resident inode拥有typed weak rendezvous；Pipe session拥有participation、data plane、readiness与cleanup
**参与领域：** VFS pathname/open / resident inode / ext4 / ramfs / Pipe / opened description / iomux / epoll
**覆盖范围：** filesystem-backed `S_IFIFO` 的 read/write/read-write open、pending admission、stream I/O、readiness、alias/unlink与fresh-session lifecycle
**不覆盖：** FIFO node creation与on-disk metadata、legacy `readdir`、Unix Socket namespace、packet-mode pipe、splice family、per-user pipe accounting、crash-time session persistence或 identity-preserving syscall restart
**实现位置：** `anemone-kernel/src/fs/api/openat.rs`、`anemone-kernel/src/fs/inode/object.rs`、`anemone-kernel/src/fs/pipe/`、`anemone-apps/fcntl-test/src/named_fifo.rs`
**依赖：** `VFS-MAKE-NODE-001`、`VFS-FILE-KIND-001`、`OPENED-DESC-001..003`、`IOMUX-POLL-001..003`
**Pending Successor：** None
**最后核验：** 2026-08-03

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| pathname、resident identity、kind、mode与DAC facts | VFS/resident inode | open operation持 stable `PathRef` | namespace admission与alias identity |
| current live-session rendezvous | resident FIFO inode的typed weak anchor | Pipe open只借用anchor并取得strong session capability | 同inode并发first-open会合；不持久化session |
| bytes、capacity、reader/writer count与partner generation | Pipe session | endpoint/pending operation持exact participation capability | open predicate、I/O、EOF/EPIPE与fresh-session teardown |
| direct read transaction | Pipe session read-operation gate | 每次read只临时持有gate | 多endpoint copyout prefix稳定性与exact consumption |
| readiness与subscription registry | Pipe session | iomux/epoll只持non-owning route | source predicate、register/final recheck与notify hint |
| initial-no-writer poll exception | read endpoint的writer-generation snapshot | Pipe保持current writer generation truth | 仅抑制该endpoint初始READABLE/HUP projection |

## PIPE-FIFO-OPEN-001 — VFS admission 先于 Pipe participation

**规则：** `openat` 必须先reserve fd并取得stable path，再完成 final directory/type、requested DAC、
`NOATIME` 与 status admission。`O_PATH` 只形成 path capability，不取得 Pipe session。普通 filesystem inode
继续调用 backend open；只有通过上述 admission 的 `InodeType::Fifo` 才以 normalized read/write/read-write 与
nonblocking context 进入 Pipe owner。Pipe activation完成后，FAN_OPEN与fd publication保持既有顺序。

blocking read-only/write-only open 先以operation-local admission guard贡献exact pending participation，再用
Pipe-owned partner predicate和generation等待对端。partner 已经与本轮 admission 重叠过时，即使在 waiter
再次调度前离开，仍可完成本轮 open。`O_NONBLOCK | O_RDONLY` 无writer时立即成功；
`O_NONBLOCK | O_WRONLY` 必须在reader observation与writer publication同一 Pipe transaction内决定，无reader时
返回 `ENXIO`，且不得推进 count/generation或发布wake。`O_RDWR` 立即为两侧各贡献一次。

signal interruption必须在fd publication前精确撤销本轮 pending participation。现有
`RestartSyscall::Idempotent` 可以在无残留状态时重放整个 syscall；replay重新开始partner round，不携带取消轮次的
partner identity/generation，也不建立FIFO-private restart state。

**违反表现：** DAC/status失败创建session或唤醒partner；failed nonblocking writer让blocking reader成功；
`O_PATH`改变participant；pending guard重复retire；取消后留下phantom count；backend解析raw open flags；或 restart
保存第二份partner truth。

**验证 / Enforcement：** open ordering/source audit；anchor/admission/generation KUnit；ext4/ramfs
blocking-order、nonblocking `ENXIO`、`O_RDWR`、`O_PATH`、signal interruption focused runtime；glibc/musl
`open06`与相关 LTP matrix。LA64 runtime与DAC-specific wake oracle仍按change record标记 Not Run。

**最初来源：** [Named FIFO 小迭代](../../devlog/changes/2026-08-03-named-fifo.md)及其同一原子
source/review/build/RV64 runtime cutover。

**当前来源：** 同最初来源。

## PIPE-FIFO-LIFECYCLE-001 — Resident inode 只弱会合 live session

**规则：** resident `InodeKind::Fifo` 是该 inode 唯一的runtime rendezvous owner，且 `InodeKind` 不可
`Copy`/`Clone`；filesystem kind仍由纯 `InodeType::Fifo` projection读取。anchor只弱持current Pipe。并发first
open可以在锁外准备candidate，但只有一个empty candidate被安装；loser和allocation failure不得发布participant、
bytes、route或wake。ext4/ramfs backend `prv`与on-disk metadata不得保存 Pipe/session state。

live Pipe session唯一拥有buffer/capacity、reader/writer participation、partner generation、read transaction gate、
Event recheck hint和poll routes。read/write/read-write endpoint分别只贡献对应participation一次；dup、fork与
`CLONE_FILES`共享同一opened description，不复制endpoint。所有 reader 使用 session-wide transaction gate，
direct-user copy只消费已成功复制的prefix；buffered bytes先于EOF，no-reader write返回`EPIPE`并发布`SIGPIPE`。
`FIONREAD`、pipe-size fcntl与readiness读取同一 Pipe facts。

readiness predicate、route snapshot与registry publication在线性化 Pipe lock下完成，notify和旧registry drop在锁外。
read/write endpoint分别投影READABLE/HUP与WRITABLE/ERR；无writer时打开的nonblocking reader只在writer generation
仍未变化时抑制初始READABLE/HUP，一旦真实writer出现过就恢复ordinary EOF/HUP规则。read-write endpoint对两侧
predicate做union；双向subscription只有在两侧fallible preparation全部成功后才一起发布。

hard link与rename保留同一resident inode/anchor/session。unlink只撤销pathname；旧opened或pending capability继续
pin旧identity。相同pathname重新创建得到新inode/session。没有endpoint、pending或in-flight strong capability时，
Pipe可释放；下一次open得到empty、default-capacity session，不继承bytes、resize、generation或HUP/EOF历史。

**违反表现：** inode与Pipe各缓存一份participant/session truth；weak anchor阻止teardown；两个alias取得不同live
session；新inode命中旧session；multiple readers重复提交同一prefix；duplex route allocation留下half subscription；
notify在Pipe lock内回调；或fresh open继承旧bytes/capacity/generation。

**验证 / Enforcement：** owner/lock/drop/source audit；anchor reuse/stale replacement、atomic nonblocking admission、
initial-no-writer poll、read-write retirement、distinct-anchor与buffer/capacity KUnit；RV64 ext4/ramfs focused suite验证
readiness、capacity、exact two-reader consumption、dup/fork/final close、alias/unlink/recreate、fresh session与SIGPIPE；
glibc/musl focused LTP各7/7 case。ext4 remount-before-first-open、hardware、SMP>1和LA64 runtime仍为 Not Run。

**最初来源：** [Named FIFO 小迭代](../../devlog/changes/2026-08-03-named-fifo.md)及其同一原子
source/review/build/RV64 runtime cutover。

**当前来源：** 同最初来源。
