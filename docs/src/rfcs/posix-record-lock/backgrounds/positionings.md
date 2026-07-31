# POSIX Record Lock 前置定位共识

**状态：** Archived background / superseded by Draft body
**最后更新：** 2026-07-31

本文归档 POSIX record lock Draft 正文形成前已经收敛的大方向与架构判断，只用于说明决策来路，此后不再
维护。当前 target 以 [RFC Draft 正文](../index.md)与[目标和不变量](../invariants.md)为准；后续 review 或
implementation feedback 必须写回对应 canonical layer，不得继续修改本文形成并列权威。

本文不是 accepted target、current contract 或 implementation authority，不授予实现权限，也不冻结具体类型、
算法、锁、模块路径、write set、阶段划分或验证命令。

## 背景与目标方向

Anemone 已有本机 whole-file advisory `flock(2)`，下一步拟实现 `fcntl()` 的 POSIX process-associated
byte-range record lock。目标是在明确声明的首版范围内保持 Linux/POSIX 用户可观察语义，包括 record range、
读写冲突、`F_GETLK` 查询、非阻塞设置、阻塞等待以及 close/fork/exec 等生命周期效果；内部数据结构、等待方式
和模块组织不要求复制 Linux。

`flock` 早期设计曾把“逻辑效果应成立”扩张成 close 驱动的精确强制取消、同步 waiter 清理和所有竞态的唯一
结果。POSIX record lock 不重复这一方向。简单性、可证明性、owner 清晰和正确性优先于更强的内部及时性、
公平性或性能保证。

## 当前共识

### 1. 首版是 POSIX record lock，不是通用文件锁工程

- 首版只交付 POSIX process-associated record lock；现有 `flock` 保持独立，OFD record lock 留给后续工作。
- 不把“建立 Linux 风格通用 file-lock framework”设为首版目标、前置条件或接受项。
- 不预先建立通用 lock record、owner trait、grant hierarchy、waiter state machine、动态 lifecycle registry，
  也不为尚未实现的 OFD、lease、remote filesystem 或 mandatory locking 预留行为接口。
- 允许做同一 VFS owner 内的浅层模块整理，例如让不同锁族在目录上相邻；这种整理不表示它们共享 grant、
  owner、cleanup、waiter 或冲突状态。

### 2. `flock` 与 POSIX record lock 保持两个语义域

两者虽然都以 inode-associated VFS identity 聚合本地冲突，也都可以消费现有 notification/recheck 能力，
但 lock-owner identity、range、转换和 cleanup 语义不同：

- `flock` holder 是 opened file description，whole-file grant 在最后一个 alias close 时释放；
- POSIX record lock grant 归属于稳定的 process-associated lock-owner identity；同一 identity 在一个
  inode-associated VFS object 上拥有可 split/merge 的 byte ranges，并具有“同一 owner 关闭指向该 inode
  identity 的任意 fd 即释放相关 locks”的可见语义；
- 本地 `flock` 与 POSIX record lock namespace 相互独立，ordinary advisory I/O 不因二者被 VFS 强制拒绝。

因此首版 POSIX record lock 应有自己的 inode-associated domain，并由该 domain 单独拥有 POSIX grant、range、
conflict predicate 与 wait publication truth。lock-owner identity 只提供 same-owner 判断与 cleanup attribution，
不保存 grant、range、waiter 或 inode 索引。不得把 POSIX 状态塞进现有 `FlockDomain`，也不得把 flock 的
opened-description retirement handoff 扩张成通用 feature cleanup registry。

### 3. POSIX holder 是 file-table-scoped lock-owner capability

本文后续所称 POSIX `holder`，专指 POSIX record-lock owner identity：它把操作划分为 same-owner 或
different-owner，并授权针对相应 inode domain 提交该 owner 的 range mutation 或 cleanup。它不是 grant 状态的
存储 owner，不是 fd slot、opened file description、进程数值身份或 inode identity。

首版采用 Linux-compatible file-table sharing topology 承载这份 identity：共享同一 live file table 的执行实体共享
同一 holder；普通 fork 形成新的 holder，且不继承父进程的 POSIX locks；确实共享的 file table 在 unshare 或成功
exec 分离后形成新的 holder，旧 grants 留在旧 sharing episode，新 holder 不继承它们；未发生实际共享时，
unshare 或 exec 不应仅因内部容器复制而改变 holder，exec 中实际关闭的 CLOEXEC fd 仍按普通 close 语义生效。
一个共享者退出只放弃自己的 file-table participation，不得提前终结仍由其它共享者使用的 holder。精确失败、
并发线性化与 teardown 顺序由 RFC 正文及 rolling implementation resolution 解析。

实现概念上需要一份窄、稳定、不可由用户数值伪造的 owner capability，候选名称为
`PosixRecordLockOwner`。`PosixLocks` 不适合作为该 capability 的名称，因为它暗示这里保存一组 mutable locks；
实际 locks 仍只存在于 inode-associated domain。不得用 fd number、TGID/PID 数值、path、inode number、
raw pointer 或 opened file description 冒充 owner truth，也不得用普通存储引用计数直接推导 file-table sharing
truth。

如果后续为此整理当前 `task::files`，首选命名方向是让较大的 task file-state / sharing-lifecycle 实体保留
`FilesState` 名称，并让当前只承担 fd slot allocation/publication 的内部状态改称 `FileTable` 或同等窄名称；
owner capability 由前者或等价 lifecycle surface 承载，不能在每个 fd slot 中重复。这只是责任与命名方向，
不冻结 Rust 类型、字段、包装层、锁、引用形状、identity equality、share accounting、模块路径或最终可见性。

`F_GETLK` 返回的 `l_pid` 是用户可见的报告信息，不是 conflict 或 cleanup identity。后续 RFC 需要单独说明
共享 file table 等边界下如何产生该报告值，且报告字段不得反向驱动 owner state machine。

### 4. 同一 task 不得形成 nested scheduler wait

- `F_SETLKW` 及其内部 helper 必须遵守 wait core 的 single-active-wait 不变量：同一 task 在任一时刻至多
  发布一个 active wait round。
- 一旦 `ActiveWait::begin()`、`Latch::begin_current()` 或后续等价 adapter 发布本轮 active wait，在该轮完成
  finish / cancel / retire 前，调用路径不得进入普通 sleepable lock、`Event::listen*()`、另一个 blocking helper
  或其它可能发布第二个 scheduler wait 的路径。当前 wait core 对这种 nested wait 使用常开断言并直接 panic；
  它不是潜在死锁、公平性退化或可以依赖 fallback 恢复的普通失败。
- notification/recheck 可以让同一个 blocking operation 经历多个先后相接的 wait round，但上一轮必须先完整
  retire，之后才能开始下一轮；“重验后继续等待”不得实现成 active round 内再嵌套一轮等待。
- 首次接入 blocking wait 的 Ready stage 必须审计 begin 到 retire 之间的完整调用链、锁和 source registration
  路径，证明其中不会再次阻塞。
  如果 live source 表明所选路线只能依赖 nested wait，必须停止并重新解析 caller/source owner 的实现边界，不能
  放宽 wait-core 断言、要求 wait core 兜底，或另造 waiter 状态掩盖该违规。

### 5. 采用协作式等待与 cleanup

- grant、unlock、range replacement 与 close-triggered release 的逻辑效果必须在其 owner serialization boundary
  内明确提交；不得以“协作式”为由允许 lost wake、双重真相源或 close 后遗留本应释放的 grant。
- state change 只需提交 recheck notification。notification 不是 grant transfer、syscall success、errno 或
  waiter completion 的真相。
- blocking operation 在自己的生命周期内分别重验 range conflict、调用 fd 仍指向本次操作采用的 opened
  description、该 description 的 inode-associated identity 与 signal outcome，并自行清理 operation-local wait
  resources。fd binding、opened-description identity 与 inode identity 不得合并为一个含糊的 `file identity`。
- 首版 `F_SETLKW` 接入现有 ordinary signal restart。阻塞中的 read/write range assignment 只有在 grant 与
  range replacement 尚未提交、且 operation-local wait resources 已经清理后，才能返回 restart carrier；
  自定义 handler 带 `SA_RESTART` 时由现有 signal finalizer 完整重放 `fcntl()`，未带时向用户态返回 `EINTR`。
  一旦 range assignment 已经在线性化点提交，本次 syscall 必须返回成功，不能再被改写成 restart / `EINTR`。
- ordinary restart 是完整 syscall replay：重新执行用户态 `struct flock` copy-in、fd / holder lookup 与 range
  normalization，不跨 signal handler 保存旧 file capability、holder、normalized operation 或 snapshot。handler
  对 fd、用户内存、opened-description position 或相关 inode state 的修改可以被重放后的新 operation 观察；若
  后续 target 要求 identity-preserving restart，必须回到 RFC review，不能让 waiter 或 restart carrier 保存第二份
  owner truth。
- close、unlock 或其它 producer 不等待 waiter 实际运行、physical scheduler placement 或全部局部清理完成，
  也不建立 producer 驱动的强制取消协议。
- 并发 close、signal、restart 与 ordinary operation 不必具有一个人为规定的唯一胜负；RFC 应约束 Linux-visible
  合法结果集合和共同最终状态，而不是为精确竞态次序增加跨 owner 状态。

### 6. 首版不做死锁检测

首版 `F_SETLKW` 提供正常的阻塞等待与 signal interruption，但不构建 wait-for graph，也不承诺检测循环等待或
返回 `EDEADLK`。对于通过全局锁顺序或其它协议保证不会形成 record-lock 等待环的程序，deadlock detection
不参与正常成功路径，因此不减少其加锁、查询、解锁和等待能力。

这个判断不能扩张成“已经完整兼容 Linux/POSIX”。依赖内核 `EDEADLK` 作为恢复路径的程序仍缺少对应能力；
形成循环等待的调用可能持续阻塞，直到 signal 或外部状态变化打破等待。首版 RFC 和接受证据必须明确这一
可观察边界，不运行或不通过 deadlock-detection 测例时应记录为不在首版 target，而不能伪造成 PASS。

死锁检测是后续可独立增加的 target capability，不是 POSIX range/grant correctness 的前置架构。后续只有在
该能力被接受时，才解析最小 waiter relation、cycle detection、cleanup 与验证；首版不为它预付状态机成本。

### 7. OFD lock 到来时再决定 record-lock 内部抽象

OFD record lock 与 POSIX record lock 的 holder/lifecycle 不同，但 Linux-visible 语义要求两者在 record-lock
namespace 中互相冲突。这会形成未来真实的共享状态与证明需求。届时应读取 live POSIX implementation，再决定
把两者收敛为共同 record-lock domain，还是采用其它能维持单一 conflict truth 的内部形状。

当前阶段只保护这条可达路径，不预先加入 OFD owner variant、OFD cleanup、通用 record-lock enum 或跨锁族
framework。允许未来 RFC 对私有 POSIX domain 做有证据的抽取或重构；这种未来可修改性不是首版抽象的理由。

### 8. 外部 ABI、file-kind admission 与本地 VFS 边界明确，内部结构保持可替换

首版只承诺 RV64 与 LA64 原生 64 位用户态的 `F_GETLK`、`F_SETLK`、`F_SETLKW` 及原生
`struct flock` ABI，其中 range offset/length 按各架构原生 64 位 `off_t` 解释。首版不承诺 32 位 compat
syscall/personality，也不提供独立的 `F_GETLK64`、`F_SETLK64`、`F_SETLKW64` compat 命令面或
compat `struct flock64` translation；未来若需要 32 位用户态兼容，应作为新的 ABI target 单独解析、验证和接受，
不能从本轮原生 ABI 支持中推导出来。

首版 file-kind admission 只接受非 `O_PATH` fd 所指向的本地 `S_IFREG` 文件；这里的 `S_IFREG` 必须来自 VFS
拥有的、与用户可见 `stat` ABI 一致的 file-type truth，而不是 `FileOps` 指针、路径、filesystem name、私有对象
downcast 或 record-lock 自建的 allow/deny list。正确分类为本地 `S_IFREG` 的 filesystem object 无论来自持久存储、
内存或本地 pseudo filesystem，都进入同一首版语义；同一对象的重复 open 必须收敛到稳定的 inode-associated
identity，不能用 provider 差异制造并列 conflict truth。

2026-07-31 的 [anonymous-inode UAPI kind 小迭代](../../../devlog/changes/2026-07-31-anon-inode-uapi-kind.md)
已经关闭此前前置偏差：eventfd、timerfd、epoll、fanotify group fd 使用 VFS-owned `Anon` kind，向
`stat` / `statx` 投影 `0600` 且不带任何 `S_IFMT` bits；pipe 与 boot tty/console 仍分别保留 `Fifo` 与
`Char`。[`VFS-FILE-KIND-001`](../../../contracts/vfs/file-kind.md#vfs-file-kind-001--inode-kind-是唯一-file-type-truth)
现在是该 baseline 的 current authority。此修正不复制 Linux singleton inode topology，也不改变 Anemone 当前
per-object anonymous inode identity。公共 record-lock RFC 只需 Preserve 该 contract，并让 admission 消费同一
VFS truth；不得通过 hard-coded exception、`FileOps` identity 或另一份 cached kind 绕过 owner。

目录、char/block device、pipe/FIFO、socket、VFS `Anon` kind 的 anonymous control fd 以及其它非
`S_IFREG` object 均不在首版 target，必须在进入 record-lock domain、发布 grant 或 wait state 前拒绝；`O_PATH`
固定返回 `EBADF`。非目标 file kind 的具体 errno 与相对 copy-in、access-mode、range validation 的校验顺序仍由
RFC target / Ready stage 解析，不能把“首版不支持”实现成成功但不生效的静默兼容。

首版 record-lock domain 只拥有本机 inode-associated VFS state，内部 normalized operation 与 range core 不按
file kind 分叉。远端或分布式 backend 的 lock propagation、协调、恢复和故障语义全部排除，也不为其增加
VFS/backend hook、callback 或 extension surface。未来其它本地 file kind 或 backend 若需要 record lock，必须
通过独立 target 与 owner/contract gate 进入，不能从首版本地 `S_IFREG` 支持中推导或让 core 预付接口。

Linux ABI 结构、flag、range normalization、access-mode validation、errno 与 copy-in/copy-out 行为应被限制在
syscall ABI boundary；VFS core 接收 normalized operation、range、inode-associated target 与窄 lock-owner
capability。首版需要覆盖的用户可见方向包括 overlap conflict、同 owner range replacement/split/merge、
`l_len = 0` 到 EOF、合法的负向
range、`F_GETLK` conflict reporting、任意相关 fd close、fork/exec 与 signal-interrupted blocking wait。

首版在方向上采用“先规范化、再做区间赋值”的逻辑模型：每个 lock-owner identity 在一个 inode-associated
domain 上拥有自己的分段锁语义，对某个 range 设置 read/write/unlock，是对该 owner 在该 range 上原有语义的
整体替换；由此自然产生 same-owner split、merge 与 mode replacement。不同 owner identity 之间只按 range
overlap 与 read/write conflict predicate 判断冲突，blocking waiter 不成为已授予区间或候选 grant 的第二份真相。

`SEEK_CUR`、`SEEK_END` 等依赖 opened-description position 或 inode size 的请求，应从相应 owner 取得一次同步
snapshot，再解析成后续操作使用的绝对 range。snapshot acquisition 与 POSIX grant commit 是两个不同 owner 的
边界，不要求与并发 `lseek`、read/write、truncate 或 append 组成跨域原子事务；并发变化可以形成多个合法先后
结果，但不得来自未同步读取。blocking operation 一旦完成规范化，等待和重试期间不得重新按新的 position 或
size 解释请求；这里的“重试”指同一次 syscall invocation 内部的 notification/recheck，不包括 signal handler
返回后的 ordinary syscall replay。后者是新的 operation，可以重新 copy-in、lookup 与 normalization。
`l_len = 0` 的 open-ended 语义也不能退化成规范化当时的固定文件长度。

本文不决定使用线性 `Vec`、有序集合、区间树或其它容器，不决定一个还是多个内部锁，也不决定精确 waiter
representation、range 类型、snapshot API 或锁顺序。实现期可以为了清晰性接受 O(n) 扫描、wake-all、惊群或
其它有限性能让步；但具体选择只能在首个 Ready stage 基于 live source、实际测例和 proof surface 解析，不能在
positioning 阶段写成长期保证。

### 9. 普通堆分配沿用当前 OOM 边界

- 首版可以使用符合 owner 与生命周期边界的普通堆分配和自然容器；当前工程阶段允许把真实 heap OOM 保持为
  kernel-fatal boundary，不要求为了把它转换成 `ENOLCK` 等可恢复 errno 而普遍改用 fallible allocation。
- 不得仅为了 heap OOM 可恢复而引入 intrusive container、自定义 fallible collection、预分配对象池、重复索引或
  镜像状态，也不得因此扭曲自然的模块、owner、transaction 和 cleanup 形状。
- 这不豁免 allocation site、对象生命周期、攻击者可控增长和 cleanup 的审计。若实现选择明确的有限 lock-record
  配额或后端本身存在可恢复资源拒绝，可以单独解析其 `ENOLCK` 等 ABI 行为；这种显式策略边界不得反向改写
  普通 heap OOM 的工程原则。

### 10. 明显策略常量进入编译期配置

- 影响容量、扫描或唤醒批量、阈值以及其它资源/性能取舍的明显策略常量，应进入编译期配置，通常由 Kconfig
  拥有其取值和受审查的默认值，不应以难以发现的 magic number 固化在 record-lock 实现中。
- 配置值的语义合法性必须由消费它的 kernel 代码通过 compile-time static/const assertion 拥有，例如 non-zero、
  range、相互大小关系、power-of-two 或与实际数据结构容量的关系。不得把这些检查散落或转移到 Kconfig schema、
  `xtask`、常量生成器或其它 host-side wrapper，也不得由这些层执行 reject、clamp 或 fallback；非法配置应使
  kernel 编译失败。
- schema、`xtask` 和生成器只负责反序列化、默认值物化与常量生成。只有把文本转换为常量前无法绕开的表示/格式
  检查，例如 IP 地址的基本格式检查，才留在这些层；这类检查不得顺带拥有 kernel consumer 的语义约束。
- ABI 固定值、由类型或布局直接推导的边界以及不表达策略的局部实现常量不因此强制配置化。positioning 阶段
  不预先发明具体配置项；首个 Ready stage 应根据选定实现列出实际策略项、对应的 kernel compile-time assertions
  及非法配置的编译失败验证。

### 11. 最终验收采用多层独立证据

- 首版最终关闭不能只依赖某一类测试。适用的 kernel focused tests、专用 userspace oracle、LTP 与真实
  end-to-end runtime 各自证明不同边界；build、源码审计或一个架构的结果不能替代另一层未运行的证据。
- userspace focused oracle 采用 `fcntl-test` 作为面向 `fcntl()` ABI 的稳定测试容器，首个且当前唯一需要解析的
  suite 是 `posix-record-lock`。这个命名不表示内核需要通用 `fcntl` 或 file-lock framework，也不要求现在建立
  通用 test framework，或为 OFD lock、lease 等能力预留测试接口；后续能力被接受时再决定
  是否增加相应 suite 与交互验证。
- `posix-record-lock` suite 在方向上覆盖首版用户可见 target，包括 range/conflict/query、同 owner 区间变换、
  holder 与 close/fork/exec 生命周期、阻塞与 signal/cleanup，以及并发合法结果和共同最终状态。positioning 阶段
  不冻结具体 case 名单、命令行、输出协议、辅助 API、重复轮数、拓扑或接入方式。
- focused admission 证据必须证明本地 `S_IFREG` 进入同一 generic core、`O_PATH` 返回 `EBADF`，并以有代表性的
  非目标 file kind 证明拒绝发生在 grant/wait publication 之前。它不要求为每一种未来 fd 类型建立枚举式测试，
  但必须 Preserve `VFS-FILE-KIND-001`，不能另建 provider 特判把 anonymous control fd 纳入首版 PASS 面。
- LTP 是最终验收的必需组成，并应形成聚焦首版 target 的 `posix-record-lock` group；更宽的 `fcntl` 或 system
  测试承担回归作用，不能用其中无关能力的结果替代 focused closure。首版 target 内的失败必须阻止关闭；
  deadlock detection、OFD lock、lease、mandatory locking、32 位 compat 与非 `S_IFREG` file-kind coverage 等
  明确非目标应按实际结果记为 excluded、TCONF 或 Not Run，不能伪造成 PASS，也不能为了追求整组通过而反向
  扩大首版 target。
- 首版声称支持的各架构必须分别形成真实 guest runtime 证据，并需要能审查阻塞、唤醒和 cleanup 竞态的并发
  证据。精确架构/用户态组合、测试选择、多核拓扑、运行次数、仓库命令和最终日志判据留给 RFC 与相应
  implementation resolution gate 基于 live runner 和测例解析。

## 首版明确非目标

- OFD record lock、lease、mandatory locking、远端/分布式 lock propagation 与恢复；
- 32 位 compat syscall/personality、独立 `F_GETLK64` / `F_SETLK64` / `F_SETLKW64` 命令面与 compat
  `struct flock64` translation；
- 目录、char/block device、pipe/FIFO、socket、VFS `Anon` kind 的 anonymous control fd、`O_PATH` 以及
  其它非本地 `S_IFREG` object；
- POSIX/OFD/flock 三类锁的统一 framework 或统一 lifecycle；
- deadlock detection 与 `EDEADLK` 保证；
- waiter FIFO、公平性、无惊群、精确 waiter 数量或性能复杂度保证；
- close 驱动的立即强制取消、同步 waiter teardown 或所有竞态的唯一 errno；
- 普通 read/write 的 mandatory enforcement；
- 为未来能力预置 backend hook、callback registry、owner enum 或 public extension surface。

## 留给 RFC 阶段解析的问题

- 在上述 file-table sharing semantics 下，`FilesState` / `FileTable` 候选分层的最终类型与模块形状、owner
  capability 表示、task-sharing truth、handle replacement、并发同步、失败回滚和 teardown 细节；
- 任意 fd close 的 task/VFS handoff、close 与在途 `F_SETLK/F_SETLKW` 的合法结果集合及最终状态；
- RV64/LA64 原生 `struct flock` 的各架构布局与 copy boundary、range normalization、`SEEK_CUR/SEEK_END`
  snapshot acquisition、overflow、access-mode、`O_PATH`、用户指针和 errno 校验顺序；
- record-lock admission 消费 `VFS-FILE-KIND-001` 的具体窄接口；admission 不得维护第二份 file-kind truth，
  也不得把 anonymous inode identity topology 纳入本轮 target；
- same-owner range replacement、split/merge、conflict reporting 与 `l_pid` 的 RFC-local proof obligations；
- blocking wait 的 signal 与 grant commit 竞态、notification/recheck、operation-local cleanup、ordinary replay
  后重新 lookup / normalization 的合法结果集合及 focused `SA_RESTART` / `EINTR` 验证；
- 非目标 file kind 的具体 errno 与校验顺序、allocation/capacity 审计及显式有限资源拒绝的 ABI；
- 首个 executable stage、probe 需求、resolved write set、验证矩阵和 contract cutover；
- OFD follow-up 到来时 POSIX/OFD 共同 conflict truth 的抽象与迁移方式。

这些问题必须在 RFC 正文及其 rolling implementation resolution 中逐步关闭。本文不因列出问题而预先选择
具体类型、算法、文件路径或阶段实现，也不把 future Outline 缺少这些细节视为缺陷。
