# 开放问题

## ANE-20260809-VFS-DYNAMIC-POSITIVE-DENTRY-REVOCATION

**Type:** Issue
**Status:** Open / Deferred
**Severity:** Medium
**Area:** fs / VFS namei / dentry lifecycle / dynamic pseudo filesystem

**Symptom / Trigger:** generic namei会先返回parent下已经cache的positive dentry；未命中时则先调用
filesystem backend `lookup`取得inode，再由VFS materialize child dentry。当dynamic backend在两步之间撤销
name-to-object binding时，失效路径只能清除当时已发布的dentry，无法阻止失效前已返回的旧inode
在失效后迟到materialize。该旧dentry随后又可被cached-positive快路命中，而不重新进入backend
完成liveness / incarnation核验。

当前首个真实consumer是procfs `/proc/<tgid>`：`proc_root_lookup()`释放binding transaction并返回旧
`InodeRef`后，`invalidate_thread_group_binding()`可以撤销binding、unindex inode并遍历procfs mounts清理
root child，但generic namei仍可以在清理后重新发布该旧inode。这是当前VFS cached-positive
publication / revocation协议的缺口，不是procfs binding owner独有的实现错误；procfs只是首个动态
管理该类映射并暴露窗口的pseudo filesystem。

**Impact:** 旧binding由`Arc` / `InodeRef`保活，当前没有use-after-free证据；多数procfs inode
operation也会通过`binding.alive()` fail closed。但stale positive dentry仍可以使纯pathname / permission
路径观察到过期可见性，并在可复用ID或未来其它dynamic pseudo fs中引入新旧incarnation的cache
identity冲突。自然触发需要并发lookup与teardown交错，当前只有source-level时序证据，尚无
forced interleaving或runtime复现证据。

**Owner:** VFS core namei / positive-dentry publication and revocation protocol。动态pseudo filesystem只拥有
各自backend mapping与lifecycle truth，不分别拥有generic dentry cache正确性或各自建立平行失效协议。

**Decision / Current Boundary:** 本问题暂缓至后续独立VFS core工作。当前procfs以及后续新增的dynamic
pseudo filesystem暂不需为此增加owner-local特判、临时freshness状态或单独acceptance gate；也不得
为了绕开generic窗口而让namei依赖procfs私有binding表示。该暂缓不表示stale pathname语义
已成为current contract，也不得将现有source review外推为完整namespace linearizability证据。

**Last Verified:** 2026-08-09

**Exit Condition:** 由独立VFS core RFC/迭代定义dynamic backend lookup结果与cached positive dentry之间的
freshness / revocation handoff、唯一线性化点、迟到materialization、同名新旧incarnation、多个mount view
与cleanup责任；实现必须保持backend mapping truth与VFS dentry cache owner分离，不依赖各pseudo filesystem
特判。以procfs作为首个真实consumer完成deterministic interleaving、cached-hit、迟到publish、新旧identity
与multi-mount验证，并对当时所有dynamic pseudo filesystem consumer做闭包审计后移除本条目。

**Related:** [Positive dentry residency小迭代](../devlog/changes/2026-08-09-positive-dentry-residency.md),
[Kthread Core procfs可见性不变量](../rfcs/kthread-core/invariants.md#procfs-可见性),
[`ANE-20260801-VFS-CREATE-PUBLICATION-ATOMICITY`](#ane-20260801-vfs-create-publication-atomicity)
（后者记录common-create backend commit后的failure/rollback窗口，不由本动态撤销问题替代或自动关闭）。

**Workaround:** 无需在当前dynamic pseudo filesystem中建立局部workaround；在VFS core协议闭合前，
将该窗口保持为已知、低自然复现率但未证明不可达的并发正确性缺口。

## ANE-20260807-TIMERFD-CANCEL-ON-SET-ENROLLMENT

**Type:** Issue
**Status:** Open
**Severity:** Keter
**Area:** timerfd / realtime step / soft timer request / ABI lifecycle

**Symptom / Trigger:** `TimerFdCore`当前只把`TFD_TIMER_CANCEL_ON_SET`的change-sequence snapshot保存于
`TimerFdSchedule::Armed`中的realtime deadline。one-shot到期、解除arm或clock-change callback把schedule切回
`Disarmed`后，该登记事实随单次queue request一同消失；已消费一次`ECANCELED`后也没有独立的持久登记状态。
因此后续realtime step无法继续按Linux timerfd lifecycle观察该fd，除非用户再次执行带flag的有效
`timerfd_settime()`。此外，非absolute-realtime组合当前返回`EINVAL`，而不是接受调用但不启用cancel-on-set。

**Impact:** 当前实现把timerfd对象的clock-step enrollment生命周期与一次armed soft-timer request生命周期
合并为同一事实。这不会破坏普通relative/absolute timerfd、物理request取消、generation stale filtering或
missed-expiry accounting，但它使完整`TFD_TIMER_CANCEL_ON_SET`语义未达到
`TIMEKEEPER-STEP-001`已经声明的effective范围；现有Gate 3 closure和窄KUnit只能证明单次armed generation，
不能证明disarm、到期或一次`ECANCELED`后的持久登记。

**Owner:** `TimerFdCore`拥有enrollment与readable cancellation状态；realtime-step publisher和per-CPU soft
timer request service只参与无丢失step通知与一次request交接
**Last Verified:** 2026-08-07
**Exit Condition:** 为`TimerFdCore`建立独立于armed request的cancel-on-set enrollment，并闭合register、replace、
disarm、expiry、一次`ECANCELED`消费和last-close的撤销/保留规则；realtime step必须在不持timekeeper锁进入
timerfd owner的前提下无丢失通知所有live enrollment。修复非absolute-realtime flag组合的Linux-visible行为，
并以focused KUnit和双架构用户态oracle覆盖disarm、one-shot expiry、periodic rearm窗口、已消费`ECANCELED`、
replacement、close与并发step；随后更新`TIMEKEEPER-STEP-001`的核验来源并移除此条目。

**Related:** [Realtime Step当前契约](../contracts/time/realtime-step.md),
[Clock Timekeeping与POSIX Timers RFC](../rfcs/clock-timekeeping-posix-timers/index.md)

**Workaround:** 不要把当前cancel-on-set通过证据外推到单次armed generation之外。该问题不阻塞保持
public API、ABI、owner、handoff和visible semantics不变的timerfd结构维护；任何语义修复、contract closure或
完整conformance声明仍须先闭合上述跨owner协议。

## ANE-20260805-USER-ACCESS-TYPED-COPY-SOUNDNESS

**Type:** Issue
**Status:** Closed / neutralized by user-access typed-copy soundness cutover
**Severity:** Apollyon
**Area:** syscall / user access / typed copy / ABI representation

**Symptom / Trigger:** `UserReadPtr<T>::read()`当前只要求`T: Copy`，随后把任意用户字节写入
`MaybeUninit<T>`并`assume_init()`；`Copy`不保证所有bit pattern都是合法`T`。反向的
`UserWritePtr<T>::write()`同样只要求`T: Copy`，却把整个`T`表示作为字节读取；`Copy`不保证结构没有
未初始化padding。具体可达路径中，`anemone_abi::system::linux::SysInfo`在`pad`与`totalhigh`之间有
4字节隐式padding，结构末尾还有4字节padding，`sys_sysinfo()`会通过typed copy直接将该表示写给用户态。

**Impact:** copyin可能形成无效Rust值并触发undefined behavior；copyout可能读取未初始化padding并把
内核栈内容泄漏给用户态。只清零某个具体调用点或只修补`SysInfo`不能恢复generic safe API的健全性，
也不能证明其它ABI struct的bit validity与padding边界。

**Owner:** syscall user-access typed-copy boundary；ABI wire representation由`anemone-abi`共同参与
**Last Verified:** 2026-08-06
**Exit Condition:** typed copy按方向建立可由编译器检查的能力边界：copyin只接受任意输入bit pattern均为
合法值的类型，copyout只接受完整表示均已初始化且无隐式padding的类型；补齐受影响ABI struct的显式
padding或等价byte codec，并完成全部typed caller审计、双架构layout assertion与build/runtime验证。

**Related:** [User-access typed-copy soundness小迭代](../devlog/changes/2026-08-06-user-access-typed-copy-soundness.md)

**Resolution:** scalar copyin、scalar/slice copyout与typed slice copyin已分别由`FromBytes`、
`IntoBytes + Immutable`及两组能力的交集建立编译期边界；全部production typed caller已由方向性derive、显式
padding或唯一byte codec闭合。raw user address已改为无provenance的64-bit token，双架构layout/build、focused
RV64 runtime、全consumer lock审计与独立review通过；没有保留`T: Copy` fallback、逐类型unsafe marker或平行ABI
truth。Linux-visible ABI、errno、owner/handoff与current contract保持不变。

## ANE-20260801-LA64-SOFT-UNALIGNED-USER-MEMORY-CORRUPTION

**Type:** Issue
**Status:** Open
**Severity:** Apollyon
**Area:** LoongArch64 / user trap / software unaligned access / user memory

**Symptom / Trigger:** 在不支持硬件 Unaligned Access 的 2K1000 实机上，启用
`soft_unaligned_access` 后运行重负载任务，尤其是构建大型系统或用 GCC 编译超大源文件时，会在不同
输入文件和 GCC pass 中出现用户态坏地址、page fault 或 internal compiler error。故障点并不稳定，
LSX 上下文支持会改变其暴露位置但不能消除故障；当前证据表明软件非对齐访存路径存在未解决 bug，
可能破坏被模拟进程的用户态内存。

**Impact:** 软件模拟目前不能为长时间或内存密集型 workload 提供可靠性保证。轻量 workload、单个
测试或一次成功编译不能作为安全证据；继续把普通用户态镜像依赖在该路径上，可能产生静默错误、进程
崩溃或编译器 ICE。

**Owner:** LoongArch64 / user memory
**Last Verified:** 2026-08-01
**Exit Condition:** 定位并修复软件非对齐访存路径中的内存破坏原因，并在不使用
`-mstrict-align` 规避的 2K1000 实机上完成大型系统构建和超大文件编译压力验证，证明跨页、权限失败、
寄存器提交及重复异常处理不会破坏用户态内存。
**Related:** [开发日志：2026-07-20 至 2026-08-02](../devlog/2026-07-20_to_2026-08-02.md)

**Workaround:** 保留 `soft_unaligned_access` 仅作为既有非严格对齐二进制无法立即替换时的应急 fallback。
对于不支持硬件 Unaligned Access 的 CPU，磁盘/rootfs 中的工具链和用户态程序应尽可能使用原生以
`-mstrict-align` 构建的二进制，避免触发软件模拟路径。

## ANE-20260727-NET-FRAME-PATH-CONFORMANCE

**Type:** Issue
**Status:** Closed / neutralized by net-frame-path Stage 4
**Severity:** Keter
**Area:** network-device / attach lifecycle / boot ordering / host conformance

**Symptom / Trigger:** `net-frame-path` post-close review确认三项已交付target内偏差：network activation依赖
同级`Late` initcall的偶然link order；published capability的pending storage/drain落在concrete VirtIO-Net
driver而不是`device/net` owner；两个长期host integration target缺少`host-test` required feature，导致
no-default test compile gate失败。

**Impact:** timer service尚未ready时active network path即可见；publication record与pending capability形成
分裂owner并让attach依赖concrete driver discovery；production feature isolation缺少稳定compile regression gate。
六个Network contract与System Power contract仍保持Active，本项记录live implementation未完整符合current
contract，而不是接受限制或新target。

**Owner:** doruche
**Last Verified:** 2026-07-27
**Exit Condition:** [Net Frame Path Stage 4](../rfcs/net-frame-path/implementation.md#10-stage-4-readypost-close-contract-conformance-correction)
以新transaction完成单checkpoint修正：`device/net`拥有异构pending handoff且失败capability仍由registry保留，
network只在完整`Late`返回后激活，三个长期host target具备一致feature metadata；host/no-default/build/RV64、
source audit与独立review全部通过后关闭NFP-008/009/010。

**Related:** [Net Frame Path RFC](../rfcs/net-frame-path/index.md),
[Tracking Issues](../rfcs/net-frame-path/tracking-issues.md),
[Network current contracts](../contracts/net/index.md),
[Stage 4 transaction](../devlog/transactions/2026-07-27-net-frame-path-stage4.md)

**Resolution:** Stage 4将pending capability与publication record原子收归`device/net`，attach失败回插同一
capability；network只在完整`Late`返回后激活；三个长期host target均具有显式`host-test`metadata。host/default
与no-default gates、RV64 build、fresh-disk 260/260 KUnit/active attach/strict shutdown order、source audit与
独立Apollyon/Keter/Euclid/Safe全0 review通过。contract cutover为None，既有current contracts保持Effective。

## ANE-20260727-MM-COW-SHADOW-ANCESTRY-STACK-OVERFLOW

**Type:** Issue
**Status:** Open
**Severity:** Apollyon
**Area:** mm / uspace / COW fork / VMO shadow

**Symptom / Trigger:** 同一长寿父进程连续执行大量受保护 fork 时，`VmArea::fork()` 会反复把父 VMA
backing 替换为新的 `ShadowObject(parent = old backing)`。后续缺页由
`ShadowObject::resolve_frame()` 递归遍历 parent ancestry；fixed LTP `epoll01` 的 `epoll_ctl`
组合为每次受保护调用执行一次 fork，稳定把该 ancestry 推到 kernel stack guard page并触发
stack-overflow panic。2026-07-27 用户本地多次复现；核验日志中 257 项 KUnit 与 11 项 epoll focused
oracle 已先通过，backtrace 随后连续返回到 `ShadowObject::resolve_frame()` 的 parent 调用点。

**Impact:** 高频 sequential fork 可以不经过 epoll 行为路径而稳定使内核崩溃；任何长寿父进程累积的
COW shadow depth 都可能触发同类故障。递归解析还使 kernel stack 消耗随 ancestry 深度无界增长。

**Owner:** mm
**Last Verified:** 2026-07-27
**Exit Condition:** 由 MM owner 为 COW shadow ancestry 建立有界、可证明的迭代解析、压平或合并策略；
增加同一父进程连续大量 fork 后 parent/child 分别读写 COW 页的定向回归，并以 MM/KUnit、RV64/LA64
build及 runtime stress 证明不再递归耗尽 kernel stack且 COW 隔离保持。
**Related:** [Epoll R2 acceptance boundary](../rfcs/epoll/index.md),
[Epoll 2D runtime evidence](../devlog/transactions/2026-07-26-epoll.md#stage-2-checkpoint-2d-reactivation-runtime-stop---2026-07-27)

## ANE-20260723-AHCI-PROBE-LIFECYCLE-AND-CAPACITY

**Type:** Issue
**Status:** Open
**Severity:** Apollyon
**Area:** AHCI / DMA / block / device lifecycle

**Symptom / Trigger:** AHCI engine/FIS receive 已启动后，IDENTIFY、capacity parse、minor allocation
或 block registration 失败可能直接释放 `AhciPort` / DMA metadata；同时异常 IDENTIFY capacity
尚未在进入 LBA48 FIS 前完成上界校验。

**Impact:** HBA 可能继续 DMA 到已释放内存，或设备返回值触发内部 FIS assertion，造成内存破坏或
kernel panic。

原AHCI RFC已在Draft阶段Terminated；因为实现仍保留，本issue继续Open，但不再从原RFC产生active gate。
任何修复都必须由新的授权边界拥有并在完成后回写本register。

**Owner:** EDGW, Codex
**Last Verified:** 2026-07-23
**Exit Condition:** 所有 post-start failure path 先停止 engine/FIS receive 再释放 DMA/MMIO owner；
IDENTIFY 明确拒绝超出 LBA48 domain 的 capacity，并由 focused KUnit/source audit 证明。
**Related:** [AHCI Controller Tracking Issues](../rfcs/ahci-controller/tracking-issues.md#ahci-001---probe-failure-can-release-live-dma-owner), [AHCI Controller RFC](../rfcs/ahci-controller/index.md), [AHCI Controller 事务日志](../devlog/transactions/2026-07-23-ahci-controller.md)

## ANE-20260527-LTP-CHDIR01-DEVICE-POOL

**Type:** Issue
**Status:** Open
**Area:** user-test / LTP / device model

**Symptom / Trigger:** 在 rv64 白名单跑到 `chdir01` 时，`tst_device` 可能拿不到可用设备，随后测试以 `TBROK: Failed to acquire device` 结束。

**Impact:** 会把一次本应聚焦内核语义的白名单验证变成环境失败，遮蔽后续回归判断。

**Owner:** doruche
**Last Verified:** 2026-05-27
**Exit Condition:** 白名单运行时稳定提供足够的可用设备，或者 `chdir01` 不再依赖当前这套设备池约束。
**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

**Severity:** Low
**Workaround:** 重新整理设备占用后再跑，或在专门的验证环境中执行该用例。

## ANE-20260527-MMAP-MPROTECT-HEAP-FASTPATH-PERSISTENCE

**Type:** Issue
**Status:** Open
**Area:** mm / uspace / mprotect

**Symptom / Trigger:** 当前 heap 上的 `mprotect` 快路径只会直接改写现有 PTE；如果后续再触发缺页，回填路径仍会按 VMA 的原始 `prot` 重新建页，保护属性不会稳定保留。

**Impact:** 这会让 heap 区间的保护变更在“已有页”与“未来 fault 页”之间出现分裂，语义和 Linux 的连续区间保护预期不一致。

**Owner:** doruche
**Last Verified:** 2026-05-27
**Exit Condition:** 为 heap 保护变更补齐可持久化的范围级保护记录，或者在修改保护时把 heap 拆成可独立表达保护的 VMA，并重新验证 `mprotect` / fault 交互。

**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

**Severity:** Medium
**Workaround:** 避免把需要长期保留的权限切换依赖在当前 heap 快路径上。

## ANE-20260527-MADVISE-DONTNEED-LOCKED-SHARED

**Type:** Issue
**Status:** Open
**Area:** mm / madvise / mlock

**Symptom / Trigger:** `madvise(MADV_DONTNEED)` 目前只做 discard hint，不会区分页面是否已被 `mlock`，也不会按 shared / locked 约束返回 `EINVAL`；在 LTP `madvise02` 里，这会把本该拒绝的 locked/shared 场景放过去。

**Impact:** 白名单里 `madvise02` 的锁页/共享页拒绝语义仍然不对，和 Linux 预期有偏差。

**Owner:** doruche
**Last Verified:** 2026-05-27
**Exit Condition:** 补齐锁页状态账本和共享页语义后，让 `MADV_DONTNEED` 按真实页面状态返回 `EINVAL` 或执行对应的回收逻辑，并重新跑 `madvise02`。

**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md), [当前限制](./current-limitations.md)

**Severity:** Medium
**Workaround:** 暂时不要把 `madvise02` 这类锁页语义当成已收敛的能力。

## ANE-20260527-OPENAT-NOFOLLOW-OPATH-SYMLINK

**Type:** Issue
**Status:** Resolved
**Area:** fs / openat / readlinkat

**Symptom / Trigger:** `openat` 曾经没有真正支持 `O_NOFOLLOW`，`O_PATH | O_NOFOLLOW` 打开的 symlink 不能稳定保留“指向符号链接本体”的语义，导致 `readlinkat("", ...)` 这类空路径用例失败。

**Impact:** 已通过 fd/openat cleanup 收敛：final `O_NOFOLLOW` symlink 拒绝、`O_PATH | O_NOFOLLOW` symlink fd 保存和 `readlinkat(fd, "", ...)` 路径已经落地；剩余 `O_PATH` 后续能力按当前限制跟踪。

**Owner:** doruche
**Last Verified:** 2026-05-28
**Exit Condition:** 已完成。focused rv64 LTP 中 `readlinkat01` 的 glibc / musl 空路径分支通过。

**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md), [当前限制](./current-limitations.md)

**Severity:** Medium
**Workaround:** 无需针对该问题绕过；完整 `O_PATH` 能力仍见 `ANE-20260528-OPATH-STAGE1-CAPABILITIES`。

## ANE-20260801-VFS-CREATE-PUBLICATION-ATOMICITY

**Type:** Issue
**Status:** Open
**Area:** fs / VFS create / inode cache / dentry lifecycle

**Symptom / Trigger:** live common-create先由filesystem backend提交inode/dirent，随后VFS初始化部分owner metadata并
materialize inode cache/dentry。backend commit到cache/dentry完成之间存在跨owner failure window；当前touch/mkdir
已经使用该路线，它不是VFS Make Node R2新引入的问题。

**Impact:** 若post-backend步骤失败，完整端到端原子性可能需要在backend、inode cache、dentry cache与全部create
caller之间建立统一transaction/rollback。只在mknodat局部补偿会制造并列create protocol或让不同创建路径语义分裂。

**Owner:** VFS common-create protocol（待独立设计）；各filesystem只拥有backend-local commit/rollback

**Last Verified:** 2026-08-01

**Exit Condition:** 由独立RFC/迭代解析所有create call site、唯一transaction owner、backend commit与cache/dentry
publication线性化点、post-commit failure/rollback和并发lookup语义，并以touch/mkdir/mknodat等真实consumer共同验证。
若工程证据表明现有handoff已经不可失败，也可用source invariant与fault-path proof关闭，无需强行引入framework。

**Related:** [VFS Make Node R2](../rfcs/vfs-make-node/index.md)、
[MAKE-NODE-ATOMIC-001](../rfcs/vfs-make-node/invariants.md#make-node-atomic-001--backend-local-有序可见性与诚实-cleanup)、
[Stage 2 resolution](../devlog/transactions/2026-07-31-vfs-make-node.md#stage-1---stage-2-implementation-resolution-gate---2026-08-01)

**Severity:** Medium
**Workaround:** VFS Make Node R2只保证backend-local final metadata先于dirent publication、正常并发不可见
中间态与可实施cleanup，并要求复用既有common-create handoff且不新增比touch/mkdir更弱的失败路径；本问题不阻塞
该RFC cutover。lwext4自身strict failure/crash atomicity由独立accepted limitation承接，不与本跨owner问题混合。

## ANE-20260527-LTP-MKNOD-LEGACY-READDIR

**Type:** Issue
**Status:** Open
**Area:** fs / syscall ABI / user-test

**Symptom / Trigger:** 老白名单里的 `readdir21` 直接依赖 legacy `__NR_readdir` 入口；当前架构没有对应 syscall。

**Impact:** `readdir21` 仍会把旧白名单的一个用例卡在 legacy syscall 入口层，和 filesystem-backed FIFO 数据面无关。

**Owner:** doruche
**Last Verified:** 2026-08-03
**Exit Condition:** 补齐或明确拒绝 legacy `readdir` ABI，并重新运行 `readdir21`。VFS Make Node与named FIFO
已分别完成node creation和data-plane cutover；focused glibc/musl `read03` 均通过，不再属于本条open issue。

**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md),
[VFS Make Node R2](../rfcs/vfs-make-node/index.md)及其
[transaction](../devlog/transactions/2026-07-31-vfs-make-node.md)、
[Named FIFO小迭代](../devlog/changes/2026-08-03-named-fifo.md)（已关闭原合并条目的FIFO部分；legacy `readdir`仍独立Open）

**Severity:** Medium
**Workaround:** 暂时把 `readdir21` 从当前白名单隔离，或在 legacy syscall入口完成后再回归。

## ANE-20260528-EXEC-ETXTBSY-WRITER-ACCOUNTING

**Type:** Issue
**Status:** Open
**Area:** fs / execve / open-file accounting

**Symptom / Trigger:** LTP `execve04` 让子进程以 `O_WRONLY` 打开 `execve_child`，父进程随后 `execve("execve_child", ...)`；Linux 期望返回 `ETXTBSY`，当前内核仍允许执行，导致 `execve_child` 运行并输出 `execve_child shouldn't be executed`。

**Impact:** 缺少 executable-vs-writer 排斥语义，会让正在被写打开的文件仍可作为新程序映像执行，和 Linux 的 text file busy 语义不一致。

**Owner:** doruche
**Last Verified:** 2026-05-28
**Exit Condition:** 为 VFS/open-file-description 或 inode 增加系统性的写打开/可执行打开账本，补齐 `execve` 与 writable open/truncate/write 之间的排斥规则，并重新验证 `execve04`。

**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

**Severity:** Medium
**Workaround:** 暂时不要把 `execve04` 视为 exec 主路径回归；等 VFS busy 账本系统性实现后再纳入通过项。

## ANE-20260529-MUSL-MEMORY-MADVISE01-SCHED-ASSERT

**Type:** Issue
**Status:** Open
**Area:** sched / task / user-test / LTP

**Symptom / Trigger:** 使用 `./scripts/run-user-test-rv64.sh <preliminary-rv64-image> build/ltp-debug.log` 复跑 memory profile 时，glibc memory 组已经完整结束；切到 musl memory 后，在 `madvise01` 执行到 `MADV_DOFORK` 附近触发 `anemone-kernel/src/sched/processor.rs:131` 的 `assertion failed: task.status() == TaskStatus::Runnable`。

**Impact:** musl memory 组无法完整跑完，导致本轮只能确认 glibc memory 组的 mmap / mremap errno 修复结果；后续 musl memory 的剩余失败矩阵会被这个调度断言遮蔽。

**Owner:** doruche
**Last Verified:** 2026-05-29
**Exit Condition:** 定位该断言对应的 task 状态转移竞态或错误唤醒路径，保证 musl memory 组至少能跑完整组并正常关机，再重新评估 musl 侧 mmap / madvise 失败项。

**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

**Severity:** High
**Workaround:** 当前先用 glibc memory 组验证 mmap / mremap errno 修复；musl memory 组需要等 scheduler 断言修复后再作为完整回归依据。

## ANE-20260602-SHMAT1-SIGILL-MASKS-SEGV-HANG-REVALIDATION

**Type:** Issue
**Status:** Open
**Area:** mm / signal / SysV shm / user-test / LTP

**Symptom / Trigger:** `shmat1` 曾在只读 attach 后写入触发缺页异常，内核投递 `SIGSEGV` 后进入线程组退出路径并卡住，`build/shmat01-stuck.log` 后段表现为大量重复的 event publish。修正同步 fault 的 `SIGSEGV` 应投递给 faulting task 而不是线程组后，最新 rv64 多次复跑未再复现该卡死；但最新 `build/user-test-rv64.log` 里的 glibc / musl `shmat1` 都更早以 illegal instruction 退出，状态码为 132。

**Impact:** 当前 rv64 日志只能说明原来的 `SIGSEGV -> exit_group` 卡死没有再次出现，不能证明原始 `NotMapped -> SIGSEGV -> task exit` 路径已经被完整覆盖；`SIGILL` 132 会遮蔽 `shmat1` 对只读映射 fault 语义和退出收敛性的回归判断。

**Owner:** doruche
**Last Verified:** 2026-06-02
**Exit Condition:** 先定位并消除 rv64 `shmat1` 的 illegal instruction 132，或构造一个定向用例稳定覆盖只读 `shmat` 写 fault；确认同步 `SIGSEGV` 只投递到 faulting task，线程组退出能收敛，且不再出现重复 event publish 卡死。

**Related:** [Sched Wait Refactor 事务日志](../devlog/transactions/2026-06-01-sched-wait-refactor.md), [RFC-20260601-sched-wait-refactor](../rfcs/sched-wait-refactor/index.md), [当前限制：SysV shm LTP infra](./current-limitations.md#ane-20260529-sysv-shm-ltp-infra-stage1)

**Severity:** High
**Workaround:** 当前只把最新 rv64 结果视为“未复现卡死但被 SIGILL 遮蔽”；不要用旧 la64 日志判断本轮修复结果，也不要把该项标成已验证通过。

## ANE-20260606-RT-SIGTIMEDWAIT-ASYNC-WAITED-SIGNAL-EINTR

**Type:** Issue
**Status:** Open
**Area:** signal / wait-core / syscall ABI

**Symptom / Trigger:** `rt_sigtimedwait` 在 wait-core 返回 `Signal` 或 `Force` outcome 时，会先把结果分类为 interrupted，恢复旧 signal mask，然后返回 `EINTR`；该分支没有先尝试 dequeue waited set 中的 pending signal。如果 waited signal 在 syscall precheck 之后到达并完成当前 wait round，调用可能错误返回 `EINTR`，而不是消费该 signal 并返回 signal number / siginfo。

**Impact:** 这会破坏 `rt_sigtimedwait` 的同步等待语义，并影响 `sigtimedwait` / `sigwaitinfo` 以及依赖它同步收割信号的 libc、BusyBox 或 LTP 路径。该问题不是 `sigsuspend` delayed mask restore 的范围扩张理由：`rt_sigtimedwait` 仍应在 syscall body 内同步 dequeue waited signal 并恢复 mask，不应改造成 trap-return signal delivery / `rt_sigreturn()` 协议。

**Owner:** doruche
**Last Verified:** 2026-06-06
**Exit Condition:** `rt_sigtimedwait` 在 wait completion 后先按 waited set 尝试 dequeue matching signal，只有确认没有 waited signal 且存在其他未屏蔽 signal / force 条件时才返回 `EINTR` 或进入对应 fail-closed 路径；重新验证 waited signal 在 precheck 后到达的定向用例，以及 LTP `rt_sigtimedwait01` / `sigtimedwait01`。
**Related:** [Sched Wait Refactor 事务日志](../devlog/transactions/2026-06-01-sched-wait-refactor.md), [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

**Severity:** Medium
**Workaround:** 不要把当前 `rt_sigtimedwait` 的 async wake `EINTR` 结果当成已收敛 ABI；需要验证同步信号等待语义时，应使用覆盖 precheck-after-arrival 窗口的定向用例重新确认。

## ANE-20260607-SIGNAL-LTP-REMAINING-SEMANTICS

**Type:** Issue
**Status:** Open
**Area:** signal / syscall ABI / scheduler / user-test / LTP

**Symptom / Trigger:** `build/user-test-rv64.log` 的 signal profile 中，`tgkill03` 和 `rt_sigqueueinfo01` 已有明确窄修；剩余仍有若干非设施型缺口需要单独收敛：`tgkill02` 在 `RLIMIT_SIGPENDING=0` 且 realtime signal 被阻塞时，Linux/LTP 期望 `tgkill()` 返回 `EAGAIN`，当前仍成功；`rt_sigaction01` / `rt_sigaction02` 在 signal 64 边界分别表现为期望成功却得到 `EINVAL`、坏用户指针期望 `EFAULT` 却先被 signal 编号校验拦成 `EINVAL`；`kill02` 在 child setup 阶段 timeout 并 `TBROK`，当前判断更像 LTP busy-poll/setup 与 scheduler/preemption 可观察性问题，不能作为 kill syscall errno 语义失败直接处理。

**Impact:** 这些残余会继续拉低 signal profile 得分，并且会把三类问题混在一起：realtime pending queue/resource accounting、signal number ABI 上界与参数校验顺序、以及 LTP setup 运行时可调度性。若不分开处理，后续容易为单个 TFAIL 写出过宽的 signal 子系统改动。

**Owner:** doruche
**Last Verified:** 2026-06-07
**Exit Condition:** 分别补齐并验证：realtime signal queue 与 `RLIMIT_SIGPENDING` 的 `EAGAIN` 语义；rt signal 编号上界 / `NSIG` 与 bad pointer 校验顺序策略；`kill02` setup timeout 的调度或 runner 根因。随后复跑 signal profile，确认 `tgkill02`、`rt_sigaction01`、`rt_sigaction02` 和 `kill02` 被重新归类或通过。
**Related:** [Signal LTP tgkill/sigqueueinfo 小迭代记录](../devlog/changes/2026-06-07-signal-ltp-tgkill-sigqueueinfo.md), [当前限制：Signal LTP infra](./current-limitations.md#ane-20260607-signal-ltp-infra-stage1), [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

**Severity:** Medium
**Workaround:** 当前先不要把 `tgkill02` / `rt_sigaction01` / `rt_sigaction02` / `kill02` 当成同一个 signal delivery bug；优先按上述子问题分别构造或复跑定向用例。

## ANE-20260608-RISCV-FPU-TRAP-RETURN-UNSAFE-BOUNDARY

**Type:** Issue
**Status:** Fix landed; revalidation pending
**Area:** riscv64 / trap return / FPU / unsafe boundary

**Symptom / Trigger:** rv64 LTP `poll02`、`pselect01` 以及 `iozone -t 4` 等路径在 release 运行中会在用户态浮点指令附近收到 `SIGILL`，即使日志已经显示 lazy-FPU 路径曾为该 task 打印 `enabled fpu`。问题对插桩高度敏感：在 `utrap` 路径打日志会让原始 `SIGILL` 消失；普通 `let _ = trapframe.sstatus()` 不改变行为；`core::hint::black_box(trapframe.sstatus())` 又能让失败消失。2026-06-18 根因收敛为 riscv64 user-trap assembly 在 trapframe 入栈后直接调用 Rust，但 `RiscV64TrapFrame` 大小曾使 `$sp` 偏离 RISC-V C ABI 要求的 16 字节对齐；插桩只是改变了 UB 表面。修复已将 riscv64 trapframe 对齐、尺寸断言和 trap CSR 偏移护栏落地，并移除 `black_box` 止血；loongarch64 同步增加预防性 trapframe / FPU context 布局护栏。

**Impact:** 该现象说明 riscv64 用户返回、FPU lazy enable、trapframe 内存提交和 `sstatus.FS` CSR 恢复之间的 unsafe Rust / assembly 边界曾违反调用 ABI。`Task::fpu_used()` 只能证明 task 有 FPU 上下文，不能证明 trap entry 以满足编译器 ABI 假设的栈形态进入 Rust。若 hand-written assembly 破坏栈对齐，后端可基于合法 ABI 前提生成对该环境不安全的代码，从而把原本应继续执行的用户态浮点指令错误暴露为 `SIGILL`，并遮蔽 `poll02` / `pselect01` 以及其它 rv64 SIGILL 相关回归判断。

**Owner:** doruche
**Last Verified:** 2026-06-18
**Exit Condition:** 在移除 `black_box` 止血后，release rv64 `iozone -t 4` 已由用户确认不再触发同类 SIGILL；仍需复跑并确认 `poll02` / `pselect01` 以及 rv64 full/user-test 相关 profile 不再在已启用 FPU 后因同类浮点指令收到 `SIGILL`。确认后移除此开放问题，并把长期历史保留在开发日志中。
**Related:** [SHMAT1 SIGILL revalidation](#ane-20260602-shmat1-sigill-masks-segv-hang-revalidation)

**Severity:** High
**Workaround:** 已移除 `core::hint::black_box(trapframe.sstatus())` 止血；后续不要重新引入插桩或 opaque read 作为稳定器。若同类 SIGILL 再现，优先检查 arch trap entry 是否保持 Rust/C ABI 调用边界、trapframe 布局断言和汇编偏移护栏，而不是先假设用户二进制非法。

## ANE-20260616-LTP-POST-SUMMARY-HANG

**Type:** Issue
**Status:** Open
**Area:** user-test / LTP / task exit / wait-core / timer / loop cleanup

**Symptom / Trigger:** rv64 长 profile 运行 LTP 时，小概率在单个 case 已经打印完 LTP `Summary` 后卡住，runner 没有继续打印 `PASS LTP CASE ...` 或 `FAIL LTP CASE ...`，也不会进入下一个 case。当前已知例子是 `build/ltp-all-rv.log` 在 `ioctl05` 结束 summary 后停住；单独运行 `ioctl05` 暂未复现。

**Impact:** 一个偶发 case 卡住会阻塞后续 profile，导致本轮 LTP 得分信号和失败矩阵都被截断。该现象目前不能简单归类为 `wait4` / `waitid` 错过唤醒：LTP harness 的 summary 输出发生在 testcase cleanup 和最终退出之前，仍需要确认 child 是否已经真正退出。

**Owner:** doruche
**Last Verified:** 2026-06-16
**Exit Condition:** 补充父/子进程状态与 cleanup 阶段观测，证明卡住时 child 是停在 LTP cleanup（例如 loop device detach、timer sleep 或退出路径）还是已经退出但父进程 wait/reap 状态没有收敛；随后按真实根因修复 loop cleanup / timer wait / task exit / wait-core 唤醒或 reaping 语义，并确认长 LTP profile 不再需要 runner timeout 才能推进。
**Related:** [User-test LTP Pgrp Isolation](../devlog/changes/2026-06-07-user-test-ltp-pgrp-isolation.md), [RFC-20260601-sched-wait-refactor](../rfcs/sched-wait-refactor/index.md), [当前限制：IOCTL LTP stage-1 gaps](./current-limitations.md#ane-20260604-ioctl-ltp-stage1-gaps)

**Severity:** High
**Workaround:** `user-test` LTP runner 暂时使用 per-case timeout；超时后只对该 case 的独立进程组发 `SIGKILL`，把该 case 归为 runner 设施失败并继续执行后续 case。该绕过只保证 profile 能继续推进，不证明内核 wait / timer / cleanup 语义已修复。

## ANE-20260622-IRQ-OFF-HEAP-ALLOCATION

**Type:** Issue
**Status:** Open
**Area:** irq / scheduler / task lifecycle / timer / mm allocator

**Symptom / Trigger:** 单核、关抢占的 LTP 长 profile 仍可能在 case summary 或 `PASS/FAIL LTP CASE ...` 附近卡死。2026-06-22 审查中发现若干 hard IRQ 或 IRQ-off return-tail 路径仍会执行可能扩容的堆分配或 allocator side effect：例如 trap interrupt return 在重新开中断前调用 deferred task disposal，disposal 扫描时用 `Vec` 临时收集 task 并可能在日志中 clone task name；threaded timer 的 IRQ 到 worker ready queue 交接使用 `VecDeque::push_back()`。当时还存在frame allocation后的OOM threshold/wake反向调用；2026-08-05 periodic-sampling小迭代已删除该hook和global OOM wake handle，因此这条递归OOM side effect已消除。

**Impact:** 当前工程阶段允许简单、适度且有界的 IRQ-safe allocation；allocation 本身不再是禁止项，也不应为了消除它引入侵入式对象、镜像状态或额外 owner。递归OOM wake已不再是当前风险，但allocation周围仍可能存在blocking/reclaim protocol、普通锁或remote placement、日志格式化和复杂对象析构，把本应短小、不可睡眠、不可重入的上下文扩大成复杂工作；allocator内部使用noirq lock或开启`spin_lock_irqsave`仍不能单独证明这些剩余副作用安全。

**Owner:** doruche
**Last Verified:** 2026-08-05
**Exit Condition:** 对 hard IRQ handler、trap interrupt return tail、scheduler noirq path、timer IRQ lane 和 deferred task disposal 做一次 source audit；允许与一次有界操作绑定、同时存活数量受现有 credit/capacity 约束且不制造第二套状态 truth 的简单 IRQ-safe allocation，但必须移除或隔离 blocking/synchronous reclaim、普通锁、remote placement、task Drop、普通日志/name clone和复杂callback等剩余副作用。随后用定向source audit和长LTP profile复跑，确认post-summary hang不再由IRQ/off-tail的复杂allocator side effect或重入路径解释。
**Related:** [LTP post-summary hang](#ane-20260616-ltp-post-summary-hang), [fanotify tracking issues](../rfcs/fanotify/tracking-issues.md), [OOM periodic sampling](../devlog/changes/2026-08-05-oom-periodic-sampling.md)

**Severity:** High
**Workaround:** 当前把复杂allocator side effect与重入路径视为未收敛风险，不把所有IRQ/off-tail allocation一概禁止。保持对象模型直接，分配量和live count有界；避免blocking/reclaim、普通锁、remote placement、日志格式化和complex drop/callback。不要用noirq allocator、`spin_lock_irqsave`或偶然通过的LTP case作为关闭依据。
