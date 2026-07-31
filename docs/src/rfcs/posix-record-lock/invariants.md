# POSIX Record Lock 目标和不变量

**状态：** R0 Accepted / Stage 0 Closed / Stage 1 Ready / Checkpoint 1A Ready / Not Active / Not Cut Over
**最后更新：** 2026-07-31
**父 RFC：** [RFC-20260731-posix-record-lock](./index.md)
**适用修订：** R0

本文定义本 RFC 尚未 cut over 的 R0 contract delta、target invariants 与 RFC-local proof obligations。当前
已经生效的共享规则仍以 `docs/src/contracts/` 为准；R0 中的 `Introduce` 项在 `POSIX-LOCK-CUTOVER` 前继续
Not Effective，不能作为当前实现事实。

## 规则分类

- **Correctness Invariant：** 状态唯一 owner、并发、生命周期、cleanup、内存安全和 ABI 诚实性规则；违反即
  实现不正确，不能作为性能或工程妥协接受。
- **Target Guarantee / Capability：** R0 拟承诺的命令、file-kind、range、lifecycle、restart、architecture 与
  acceptance surface；新修订接受前不得降低。
- **Implementation Preference：** Rust 类型、helper、module、锁、container、scan/wake 策略、stage 与文件
  manifest；本轮不写成 invariant，由后续 rolling implementation resolution 解析。

## Contract Impact

所有 Introduce 项拟在同一个 `POSIX-LOCK-CUTOVER` 生效。该 gate 所在 stage 尚未解析；R0 acceptance、
`implementation.md` 创建、Ready resolution、代码落地或单项测试通过都不会让它提前生效。

| Contract ID | 变化 | 当前规则 | Target 摘要 | 生效 Gate |
| --- | --- | --- | --- | --- |
| `FILES-POSIX-OWNER-001` | Introduce | None（尚未生效） | file-table sharing episode 唯一拥有 POSIX holder identity、participation 与 split semantics | `POSIX-LOCK-CUTOVER` |
| `POSIX-LOCK-DOMAIN-001` | Introduce | None（尚未生效） | inode-associated VFS POSIX domain 唯一拥有 granted ranges、mode 与 conflict truth | `POSIX-LOCK-CUTOVER` |
| `POSIX-LOCK-WAIT-001` | Introduce | None（尚未生效） | operation-local single-active wait、predicate recheck 与 notification-only progress | `POSIX-LOCK-CUTOVER` |
| `POSIX-LOCK-LIFECYCLE-001` | Introduce | None（尚未生效） | 任意相关 fd close 删除既有 holder × inode grants，并排除 closed-binding late grant | `POSIX-LOCK-CUTOVER` |
| `VFS-FILE-KIND-001` | Preserve | [当前规则](../../contracts/vfs/file-kind.md#vfs-file-kind-001--inode-kind-是唯一-file-type-truth) | admission 只消费 VFS kind，不建立 provider/file-ops 特判 | 全程 |
| `FLOCK-DOMAIN-001` | Preserve | [当前规则](../../contracts/vfs/flock.md#flock-domain-001--inode-associated-domain-是唯一-grant-truth) | flock grant domain 不接收 POSIX ranges、holder 或 cleanup state | 全程 |
| `FLOCK-WAIT-001` | Preserve | [当前规则](../../contracts/vfs/flock.md#flock-wait-001--wait-publication-与-predicate-recheck-闭合进度) | POSIX 与 flock 不共享 waiter、notification 或 progress truth | 全程 |
| `FLOCK-LIFECYCLE-001` | Preserve | [当前规则](../../contracts/vfs/flock.md#flock-lifecycle-001--retired-holder-不遗留或重获持久-grant) | POSIX 任意 fd cleanup 不改变 flock 的 terminal opened-description cleanup | 全程 |
| `OPENED-DESC-001/002` | Preserve | [当前规则](../../contracts/task/opened-description-lifecycle.md) | 不改变 opened-description published-ref 与 dup/fork alias truth；它不成为 POSIX holder | 全程 |
| `OPENED-DESC-003` | Preserve | [当前规则](../../contracts/task/opened-description-lifecycle.md#opened-desc-003--当前-final-release-callback-是创建时固定的单-hook) | POSIX cleanup 不占用或扩张 creation-time single final-release hook | 全程 |
| `OPENED-DESC-RETIRE-001` | Preserve | [当前规则](../../contracts/task/opened-description-lifecycle.md#opened-desc-retire-001--terminal-retirement-固定进入窄-vfs-flock-handoff) | 固定 flock retirement handoff 不扩张为 POSIX 或通用 lifecycle registry | 全程 |
| `OPENED-DESC-LIVENESS-001` | Preserve | [当前规则](../../contracts/task/opened-description-lifecycle.md#opened-desc-liveness-001--non-owning-capability-只验证-terminal-opened-description-liveness) | operation 可重验 binding target，但不复制 opened-description lifecycle | 全程 |
| `SCHED-LATCH-001..003` | Preserve | [当前规则](../../contracts/scheduler/latch-wait-round.md) | 每个 blocking round 只持一个 waiter-owned latch identity；trigger 只是重验 hint | 全程 |
| `SCHED-WAKE-001..004` | Preserve | [当前规则](../../contracts/scheduler/wake-delivery.md) | logical completion、physical placement 与 consumer result 继续分离 | 全程 |

`FILES-POSIX-OWNER-001` 不把整个 task file-state 领域批量整理成新 contract；它只提取本 target 所需的最小
sharing-episode identity、split 与 participation closure。`POSIX-LOCK-*` 不复制 `FLOCK-*`，也不把 RFC-local
ABI details 全部提升为跨 RFC current contract。

上述 Preserve 闭包要求后续 Ready resolution 与 cutover review 审计全部 fd-removal 路径，并证明 POSIX
any-fd cleanup 保持在 task-files / VFS 的独立窄 handoff 内：它不复用 opened-description static hook，不扩张固定
flock retirement，不与 flock 共享 grant、waiter、notification 或 cleanup state。具体 helper、调用顺序与内部
模块可以按 live source 解析，只要现有 opened-description 与 flock owner、可见语义和 terminal obligation 不变。

## Target Invariants

### FILES-POSIX-OWNER-001 — File-table sharing episode 是唯一 holder identity

**分类：** Correctness Invariant。

**规则：** POSIX holder 由 task file-state lifecycle owner 为一个真实 file-table sharing episode 创建和终结。
共享同一 live table 的所有 participants 使用同一 holder；普通 fork 使用新 holder且不继承 grants；只有真实
shared episode 在 unshare 或成功 exec 时才 split 为新 holder，新 holder不继承旧 grants。table 未共享时，
内部 container replacement、copy 或 no-op unshare 不得改变 holder。

holder 是 opaque、不可由用户数值伪造的 capability，只表达 same-owner equality、operation authority 与 cleanup
attribution。它不保存 grants、ranges、waiters、inode index、report PID 或 fd binding，不暴露 private table
lock，也不由 fd number、TGID/PID、path、inode number、opened-description identity、raw pointer、`Arc`
strong count 或 published-description refcount派生。

**Owner：** task file-state sharing lifecycle owner。

**依赖：** `OPENED-DESC-001/002`（Preserve，但两类 identity 不合并）。

**违反表现：** 普通 fork 继承父锁；`CLONE_FILES` participants 互相冲突；unique table 因对象重建丢锁；一个
sharer exit 提前终结所有 grants；数值 PID reuse 命中旧 owner；holder 缓存 inode/grant state。

**Cutover：** `POSIX-LOCK-CUTOVER`；此前当前 task file-state contract 不包含该 holder capability。

### POSIX-LOCK-DOMAIN-001 — Inode-associated domain 是唯一 grant 与 conflict truth

**分类：** Correctness Invariant。

**规则：** 每个本地 inode-associated identity 的 POSIX record-lock domain 唯一拥有
`Holder × AbsoluteRange -> Mode` 的 granted relation、same-owner range assignment、different-owner conflict
predicate 与 query snapshot。一个 holder在同一 byte 上至多有一种 mode；same-owner request 先以 assignment
语义替换目标 range，再按相邻相同语义自然 coalesce。fd slot、opened description、holder object、syscall
adapter、filesystem backend、waiter或notification不得保存 mode、candidate、conflict 或 range mirror。

硬链接、不同 path、重复 open 与不同 local filesystem provider只要到达同一 VFS inode identity，就进入同一
domain。inode identity 是 domain association，不是 holder identity。

**Owner：** inode-associated VFS POSIX record-lock domain。

**依赖：** `FILES-POSIX-OWNER-001`、`VFS-FILE-KIND-001`。

**违反表现：** path或fd绕过冲突；同一 byte出现同 owner 重复 mode；waiter预占 candidate grant；backend保存
自己的 record-lock list；独立 open 被误当成不同 POSIX owner；不同 inode 被 `(dev, ino)` 临时 key 错误合并。

**Cutover：** `POSIX-LOCK-CUTOVER`。

### POSIX-LOCK-WAIT-001 — Wait publication 只服务 predicate recheck

**分类：** Correctness Invariant。

**规则：** blocking operation 在 domain serialization 下完成 conflict/fd-binding predicate check 与 waiter
publication，闭合 check-then-sleep lost-wake window；等待期间不持有 domain guard。unlock、range replacement、
grant cleanup 与其它可能改变 eligibility 的 producer 在 guard 外提交 recheck notification。notification 不
转移 grant、不表示 syscall success/errno、不预选 waiter，也不保存 active/completed truth。

同一 invocation 可以顺序创建多轮 wait，但同一 task 任一时刻至多发布一个 active scheduler wait identity。
每轮必须在下一轮 begin 前 finish/cancel/retire；begin 到 retire 的路径不得进入普通 sleepable lock slow path、
`Event::listen*()` 或其它 nested active wait。operation owner 单独清理 listener/trigger/capability，producer只
等待 notification submission，不等待 waiter execution或physical placement。

**Owner：** POSIX domain拥有predicate、publication source与notification；scheduler wait core拥有wait
identity、logical completion与placement；calling operation拥有round lifecycle、final recheck与local cleanup。

**依赖：** `POSIX-LOCK-DOMAIN-001`、`SCHED-LATCH-001..003`、`SCHED-WAKE-001..004`。

**违反表现：** conflict check 后漏 wake；wake直接授予 range；多个 task 争用一个 waiter state；active round内
再次进入 scheduler wait；close 等待 task 运行；stale trigger完成未来 round；notification决定 errno。

**Cutover：** `POSIX-LOCK-CUTOVER`。

### POSIX-LOCK-LIFECYCLE-001 — 任意相关 fd close 清除既有 grants 并排除 closed-binding late grant

**分类：** Correctness Invariant。

**规则：** holder 内任意 fd slot 从 live binding 移除，只要它指向目标 inode identity，task file-state protocol
owner就必须在自己的 private guard外 exactly-once 进入窄 POSIX cleanup handoff。VFS domain同步删除该 holder
在该 inode 上的全部 granted ranges并提交 recheck notification；cleanup不等待 opened-description final release，
也不因同一 opened description 或 holder 仍有其它相关 fd而保留 grants。

close 与 range commit必须收敛为：有效 assignment先建立则 cleanup随后删除；cleanup先建立 binding closure，
则仍依赖该 binding authority 的 in-flight operation不得在 cleanup 后留下该 holder × inode 的 persistent grant。
该判断绑定于operation实际使用的fd binding，不绑定于invocation开始时间：同一holder通过另一个仍存活binding
发起的operation可以在cleanup后独立线性化并重新取得lock。binding validation、domain mutation与必要的私有
重试或校正如何组成serialization protocol由Ready stage解析；target不冻结具体检查时点、锁形状或helper。
该invariant约束grant最终状态，不规定普通success、`EBADF`、signal outcome与close return的唯一全序。

**Owner：** task file-state protocol owner编排 slot-removal/cleanup episode；VFS POSIX domain拥有range
removal；operation owner拥有binding revalidation与return；scheduler owner拥有wait completion。

**依赖：** `FILES-POSIX-OWNER-001`、`POSIX-LOCK-DOMAIN-001`、`POSIX-LOCK-WAIT-001`、
`OPENED-DESC-001/002/003`、`OPENED-DESC-RETIRE-001`、`OPENED-DESC-LIVENESS-001`、
`FLOCK-WAIT-001`与`FLOCK-LIFECYCLE-001`。

**违反表现：** close 一个相关 fd 后锁仍存在；final `ProcFile` release才解锁；另一个 independent open 保留同
holder locks；已关闭binding在cleanup后留下grant；把其它live binding的后续assignment错误阻止为holder-wide
close epoch；持fd-table guard回调VFS；close等待waiter退出；holder维护`has_locks`/inode mirror决定是否cleanup。

**Cutover：** `POSIX-LOCK-CUTOVER`。

### POSIX-LOCK-TARGET-001 — Native `struct flock` 与 absolute range semantics

**分类：** Target Guarantee / Capability。

**规则：** RV64与LA64原生64位用户态支持`F_GETLK/F_SETLK/F_SETLKW`和原生`struct flock`。ABI owner接受
`F_RDLCK/F_WRLCK/F_UNLCK`与`SEEK_SET/CUR/END`，从相应owner取得position/size snapshot后规范化为绝对
half-open range；positive length向高地址延伸，negative length向低地址延伸，zero length保持open-ended EOF
语义。最终起点必须非负；invalid type/whence或越过零点返回`EINVAL`，算术不可表示返回`EOVERFLOW`。

同一次 blocking invocation 的 recheck重用normalized range；ordinary signal restart是新 invocation并重新
copy-in/lookup/snapshot/normalize。VFS core不得读取raw `struct flock`、用户指针或relative whence。

**Owner：** `fcntl` syscall ABI operation owner；position与size snapshot分别来自opened-description/VFS inode
owner；absolute grant range属于POSIX domain。

**依赖：** `POSIX-LOCK-DOMAIN-001`。

**违反表现：** negative length被一律拒绝；`l_len=0`冻结为当前size；wait后按新position重新解释；overflow
wraparound；VFS core依赖ABI layout；32-bit compat被误报为首版支持。

**Cutover：** N/A；产品能力随`POSIX-LOCK-CUTOVER`生效。

### POSIX-LOCK-TARGET-002 — Range assignment、conflict 与 `F_GETLK`

**分类：** Target Guarantee / Capability。

**规则：** read/read compatible，write与其它owner的任一overlap mode冲突。`F_SETLK/F_SETLKW` 对目标 range
执行same-owner整体assignment，允许split/merge/mode replacement；冲突时不发生partial mutation，前者返回
`EACCES`或`EAGAIN`，后者等待。unlock是幂等assignment且不等待。

`F_GETLK`输入只接受read/write query且不改变grant。无冲突只写回`F_UNLCK` type并保持其余input fields；有
冲突返回任一真实blocking segment，以`SEEK_SET`、absolute start、zero-for-open-ended length与diagnostic
`l_pid`表达。多个冲突的选择顺序不是target guarantee。

**Owner：** POSIX domain；ABI copyout属于syscall owner。

**依赖：** `POSIX-LOCK-DOMAIN-001`、`POSIX-LOCK-TARGET-001`。

**违反表现：** same-owner ranges互相阻塞；写锁允许其它owner读锁overlap；conflict失败先改了旧range；unlock
删除其它owner lock；query返回候选/过期非冲突segment；no-conflict改写caller range fields。

**Cutover：** N/A；产品能力随`POSIX-LOCK-CUTOVER`生效。

### POSIX-LOCK-TARGET-003 — Fork/share/unshare/exec/exit topology

**分类：** Target Guarantee / Capability。

**规则：** `CLONE_FILES` sharers共享holder；普通fork新建holder且不继承locks。真实shared table在
`close_range(UNSHARE)`或成功exec分离时，新table获得新holder且不继承旧grants；旧grants继续只属于旧episode
remaining sharers。unique table不因内部replacement改变holder。holder保持的成功exec保留locks，但每个实际
关闭的CLOEXEC fd执行`POSIX-LOCK-LIFECYCLE-001`。单一sharer exit不提前清除其它participants共享holder；
最后episode teardown必须清空其grants。

**Owner：** task file-state sharing lifecycle owner。

**依赖：** `FILES-POSIX-OWNER-001`、`POSIX-LOCK-LIFECYCLE-001`。

**违反表现：** fork child query被视为same owner；CLONE_FILES child与parent冲突；unshare继承旧locks；unique
exec无故丢锁；一个sharer exit清空仍共享table的locks；最后table消失后遗留grant。

**Cutover：** N/A；产品能力随`POSIX-LOCK-CUTOVER`生效。

### POSIX-LOCK-TARGET-004 — ABI admission、validation 与 error boundary

**分类：** Target Guarantee / Capability。

**规则：** 首版只接受非`O_PATH` fd指向的本地VFS `S_IFREG`。invalid fd或`O_PATH`返回`EBADF`；其它
file kind返回`EINVAL`且不发布grant/wait。read lock需要readable opened description，write lock需要writable，
否则`EBADF`；unlock不要求对应access。unreadable/unwritable userspace flock storage返回`EFAULT`。

validation次序是command decode、fd/`O_PATH`、copy-in、type/whence/range、file-kind、set-operation access，
之后才可进入query/grant/wait。`F_GETLK` copyout failure返回`EFAULT`且不修改grant state。ABI adapter不得让
non-target operation静默成功，也不得用filesystem/provider/FileOps特判绕过`VFS-FILE-KIND-001`。

**Owner：** syscall ABI operation owner；file-kind truth属于VFS inode owner；access truth属于opened
description owner。

**依赖：** `VFS-FILE-KIND-001`、`POSIX-LOCK-TARGET-001/002`。

**违反表现：** anonymous control fd因mode bits误入；directory/pipe返回success no-op；O_PATH进入domain；
read-only fd取得write lock；validation failure留下range或waiter；backend决定UAPI errno。

**Cutover：** N/A；产品能力随`POSIX-LOCK-CUTOVER`生效。

### POSIX-LOCK-TARGET-005 — Signal、ordinary replay 与 admissible race outcomes

**分类：** Correctness Invariant 与 Target Guarantee / Capability。

**规则：** `F_SETLKW`的read/write assignment只有在尚未commit且operation-local wait resources已清理后才能
返回ordinary restart carrier。无restart时用户观察`EINTR`；允许`SA_RESTART`时完整重放`fcntl()`。assignment
一旦commit，本次invocation返回success，不能被later signal改写。

replay不保存旧fd target、holder、normalized range或snapshot，可观察signal handler对fd、用户内存、position
与inode state的修改。close/signal/notification/commit不需要固定唯一winner；允许普通result、`EBADF`或
`EINTR`/restart，也允许其它live binding在close cleanup后重新取得lock，只要
`POSIX-LOCK-LIFECYCLE-001`与wait cleanup共同最终状态成立。

**Owner：** operation owner决定commit与return；signal finalizer决定ordinary replay；wait core决定本轮
completion；domain只拥有grant truth。

**依赖：** `POSIX-LOCK-WAIT-001`、`POSIX-LOCK-LIFECYCLE-001`。

**违反表现：** commit后返回EINTR；restart carrier保存旧holder；handler复用fd却仍锁旧inode；signal path
遗留listener；close被迫等待errno决定；某一次调度winner被测试固化为ABI。

**Cutover：** N/A；产品能力随`POSIX-LOCK-CUTOVER`生效。

### POSIX-LOCK-TARGET-006 — `l_pid` 是显式诊断字段

**分类：** Correctness Invariant 与 Target Guarantee / Capability。

**规则：** conflicting grant segment携带只服务`F_GETLK`/日志的report TGID snapshot。普通holder报告建立当前
segment mode的thread-group ID；跨thread-group `CLONE_FILES` holder可以报告建立该segment的任一participant
TGID。report允许task exit后stale、same-owner reassignment后变化，且不得影响holder equality、conflict、
range merge/split correctness、cleanup、wait或lifecycle。

如果实现为coalesce相邻same-owner/same-mode segments而选择一个合法report值，该选择不能改变range语义；
RFC不承诺shared-holder diagnostic boundaries或多个conflict中的稳定选择顺序。

**Owner：** POSIX domain内grant segment的diagnostic field；behavior owner仍分别是file-table holder与domain。

**依赖：** `FILES-POSIX-OWNER-001`、`POSIX-LOCK-DOMAIN-001`。

**违反表现：** 用TGID判断same owner或cleanup；PID reuse接管旧locks；为保留report boundaries复制behavior
state；query返回与冲突segment无关的PID；诊断字段反向阻止coalesce或assignment。

**Cutover：** N/A；产品能力随`POSIX-LOCK-CUTOVER`生效。

### POSIX-LOCK-TARGET-007 — Advisory、conflict namespace 与首版 capability exclusion

**分类：** Target Guarantee / Capability。

**规则：** ordinary read/write不因POSIX grants被VFS强制拒绝；local flock与POSIX record lock不共享grant、
holder、waiter、cleanup或conflict state。首版不包含OFD、deadlock detection、mandatory/lease/remote、32-bit
compat、non-regular files、公平性或performance guarantee。形成record-lock cycle时可以持续阻塞，直到signal
或外部state change；不保证`EDEADLK`。

未来OFD lock必须与POSIX ranges互相冲突，因此届时必须经follow-up RFC读取live domain并形成单一
POSIX/OFD conflict truth；当前不得为该未来需求预置owner enum、generic record、wait graph或backend hook。

**Owner：** POSIX domain只拥有本RFC conflict namespace；flock继续由`FLOCK-DOMAIN-001`拥有。该语义边界不阻止
两个domain归入同一纯wiring Rust module namespace。

**依赖：** `POSIX-LOCK-DOMAIN-001`、`FLOCK-DOMAIN-001`。

**违反表现：** ordinary I/O被强制拒绝；flock与fcntl lock意外交互；deadlock case被记为target PASS；首版代码
出现无consumer的OFD/remote extension surface；非目标file kind因方便被静默纳入。

**Cutover：** N/A；产品能力随`POSIX-LOCK-CUTOVER`生效。

## RFC-local Invariants

### POSIX-LOCK-RFC-001 — Draft/R0 acceptance 与 effective cutover 分离

本文成为R0时共同接受target、contract delta、proof obligations与`implementation.md`中的首个完整Ready
stage；这仍不表示代码或current contract已经生效。`FILES-POSIX-OWNER-001`和`POSIX-LOCK-*`在
`POSIX-LOCK-CUTOVER`前均为Not Effective，current `fcntl` lock commands仍可保持NYI。acceptance不自动
激活stage，也不允许公共文档把target写成current fact；进入实现仍需transaction与独立启动授权。

### POSIX-LOCK-RFC-002 — Implementation resolution 是 R0 前置且计划权威唯一

target review形成finding后即可创建Draft `implementation.md`。独立resolution必须从live source解析首个完整
Ready stage、probe、stage order、精确manifest、validation与cutover可达路径；active Keter必须进入明确的
Ready / R0 stop conditions，并在阶段冻结前neutralize。该Ready definition是R0 acceptance的前置输入，
`implementation.md`是阶段计划与resolved manifest的唯一权威。
Ready、R0 acceptance、transaction bootstrap与Active authorization继续分离；不得从本正文的候选类型名、
owner表或source examples反推物理write set。

### POSIX-LOCK-RFC-003 — Wait-core single-active rule不可降级

首个接入blocking wait的Ready stage必须审计从active wait begin到retire的全部lock、registration与predicate
path。若路线只能在active round内获取可能sleep的锁或进入第二wait，停止并重排caller/domain boundary；不得
放宽release assert、建立source-local park truth或把panic改成fallback。

### POSIX-LOCK-RFC-004 — 自然数据结构优先，策略常量才进入Kconfig

普通heap OOM沿用kernel-fatal boundary，不为`ENOLCK`预先引入intrusive collection、pool或duplicate index。
只有implementation确实选择capacity/batch/threshold等策略值时才建立Kconfig；consumer kernel code必须用
compile-time assertion拥有non-zero/range/relationship等语义合法性，host schema/generator不得clamp或fallback。

### POSIX-LOCK-RFC-005 — 证据层不互相替代

kernel focused proof、userspace `fcntl-test`、focused LTP、RV64 runtime、LA64 runtime、source audit与full-diff
review分别证明不同claim。build不能替代runtime；一架构不能替代另一架构；基础ABI PASS不能替代holder/
close/concurrency proof；deadlock/OFD/mandatory/compat excluded不能记作PASS。

### POSIX-LOCK-RFC-006 — Background 不再接受 target write-back

`backgrounds/positionings.md`只保存正文形成前的共识来路。后续任何owner、ABI、contract、acceptance或
implementation feedback都写回`index.md`、本文、`implementation.md`、transaction、current contract或
register的对应层；不得继续维护positioning形成并列canonical source。

## 状态所有权

| 状态或事实 | 唯一 owner |
| --- | --- |
| fd number、slot binding、`FD_CLOEXEC` 与 table sharing topology | task file-state lifecycle owner |
| POSIX holder identity与sharing participation | task file-state lifecycle owner |
| opened-description identity、access mode与position | `ProcFile` / opened-description owner |
| inode kind、size snapshot与inode-associated identity | VFS inode owner |
| holder × inode granted ranges、mode、conflict、query与wait publication source | inode-associated VFS POSIX domain |
| fd-close cleanup episode | task file-state protocol owner编排；VFS POSIX domain提交range cleanup |
| active wait identity、logical completion与physical placement | scheduler wait core |
| operation-local listener、binding revalidation、copy/errno与return | 当前`fcntl` operation owner |
| `l_pid` report snapshot | domain grant segment中的纯诊断字段 |

不存在“task与VFS共同拥有holder/grants”、`ProcFile`保存POSIX ranges、fd table保存inode lock index、或waiter
保存candidate grant的合法状态。跨owner协议可以共同参与一个operation，但每份mutable truth与每个transition
只能有一个owner。

## 身份与能力模型

- `HolderCapability`（概念名）只允许same-owner equality与向domain授权operation/cleanup；最终Rust名称不冻结。
- `FdBindingCapability`（概念名）只服务本invocation确认其授权来自仍有效的原fd binding，并保持原opened
  description/inode关联；它不延迟close、不保存grant，也不在ordinary restart间存活。验证与domain mutation的
  具体组合由Ready stage解析，不由概念名预先冻结。
- inode-associated target来自VFS stable identity；path、hard-link spelling、`st_ino`数值或provider不能替代。
- wait trigger只指向一个scheduler wait round；stale/duplicate trigger不能完成未来round。
- diagnostic report TGID不属于holder capability，也不能从holder反向查询behavior identity。

完整`Task`、`FilesState`、`FileDesc`、fd-table guard、raw user pointer与raw `struct flock`不得跨过syscall/VFS
core boundary。窄capability允许实现访问必要truth，但不得暴露owner private state或形成行为型escape hatch。

## 线性化点

- **range assignment/query：** domain serialization protocol内建立最终conflict结果、有效binding authority与
  range mutation/query snapshot；具体内部检查和mutation顺序由Ready stage解析，`F_SETLK/F_SETLKW`不得在
  对外commit前暴露success。
- **wait publication：** domain predicate check与round trigger registration闭合的点；此后任一eligibility change
  必须能完成本轮或被final recheck观察。
- **fd close：** task file-state owner撤销slot binding的点；随后在private guard外完成POSIX cleanup handoff，
  close completion包含range cleanup与notification submission。
- **holder split：** task file-state sharing owner原子替换当前participant table episode的点；新holder不继承
  grants，旧episode state不被新holder驱动。
- **syscall interruption：** operation确认assignment未commit、retire当前wait resources后返回restart carrier的
  点；commit之后signal不能反转结果。

这些点不要求position snapshot、inode size、fd-table binding、domain mutation、signal delivery与scheduler
placement组成一个全局事务。RFC只约束每个owner的局部linearization与跨owner共同最终状态。

## 锁序与生命周期规则

- fd-table/private sharing guard内不得调用VFS POSIX domain或等待；先提交slot/share state并释放guard，再进入
  窄cleanup handoff。
- domain guard不得跨scheduler park持有；notification/publish/drop在guard外执行。
- commit需要的binding/holder revalidation不能通过VFS回取完整task或fd-table private lock制造反向owner依赖；
  精确capability/serialization路线由Ready stage解析。
- cleanup必须是显式semantic transition，不依赖`Drop`、memory last-drop、`Arc::strong_count()`或后台GC偶然
  修复。
- holder episode、grant domain、wait round与operation resources各自有exactly-once terminal path；diagnostic
  field与notification不会延长它们的semantic lifetime。
- assertion-backed cleanup路径先retire/unpublish/release，再用常开assert暴露bug，不能panic后遗留grant或
  active wait。

## 禁止退化项

- 把POSIX locks放入`FlockDomain`，或建立两者共享grant/waiter/lifecycle的通用framework。
- 用TGID/PID、fd number、opened description、path、inode number或ordinaryrefcount作为holder truth。
- 让holder、task fd state、backend或waiter缓存domain可直接推导的ranges/mode/conflict。
- 把`l_pid`、wait id、debug label、notification result等诊断/提示字段反向驱动状态机。
- 为close race建立producer-driven强制cancel、同步waiter teardown或唯一errno winner。
- 在active wait内获取sleepable lock、调用`Event::listen*()`或开始第二round。
- 把`l_len=0`转成当前EOF固定range，或在同一blocking invocation每轮重新normalize。
- 用silent success兼容non-target file kind、OFD/compat command或deadlock detection缺失。
- 为future OFD/remote/lease预置public hook、owner enum、callback registry或wait graph。
- 把未运行架构、excluded case、build/source audit写成behavioral PASS。

## 完成标准

R0 target最终关闭必须同时满足：

1. `FILES-POSIX-OWNER-001`、`POSIX-LOCK-DOMAIN-001`、`POSIX-LOCK-WAIT-001`与
   `POSIX-LOCK-LIFECYCLE-001`在同一`POSIX-LOCK-CUTOVER`原子写入current contracts；失败时全部保持
   Not Effective。
2. Preserve contracts经source/full-diff review证明未改变owner、effective semantics或public capability。
3. focused kernel proof覆盖owner equality、range assignment/conflict、closed-binding late-commit exclusion、
   commit-before-cleanup removal、其它live binding的post-cleanup assignment、fd reuse、no-lost-wake、
   single-active-wait与cleanup terminal state；证明结果集合即可，不预先规定内部serialization形状。
4. userspace oracle覆盖native ABI、normalization/query、read/write/unlock、fork/share/unshare/exec/close、
   signal/restart与admissible concurrent outcomes。
5. focused LTP只对首版target cases形成可审查分类；deadlock/OFD/mandatory/32-bit/non-regular等非目标保持
   excluded/TCONF/Not Run。
6. RV64与LA64各有真实guest runtime；任何缺失证据明确标为Not Run并阻止相应architecture closure。
7. transaction记录每个checkpoint、review、validation、cutover、correction与remaining gap；accepted limitation
   或新open defect按归属进入register，不留在tracker伪装成实现进度。

当前 R0 已共同接受 target 与[实施计划](./implementation.md)的首个 Stage 0 Ready definition，transaction 已
建立且 Stage 0已独立关闭。后续独立只读resolution gate保持本页target不变，并把Stage 1解析为Ready / Not
Active；checkpoint修正让1A只做行为保持的module alignment，1B才证明inode range domain，均不接入ABI、close或
wait。current-contract语义write-back仍只属于最终`POSIX-LOCK-CUTOVER`，Stage 0/1 semantic cutover均为`None`，
全部Introduce项继续Not Effective。
