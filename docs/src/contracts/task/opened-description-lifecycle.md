# Opened-description Lifecycle 当前契约

**Contract IDs：** `OPENED-DESC-001..003`、`OPENED-DESC-TRANSFER-001`、`OPENED-DESC-RETIRE-001`、`OPENED-DESC-LIVENESS-001`
**状态：** Active
**Owner：** `task::files` 的 opened-description semantic lifecycle
**参与领域：** task fd table / VFS opened file / VFS flock / fanotify / anonymous control fds / Unix IPC
**覆盖范围：** opened-description identity、published/transfer semantic references、terminal lifecycle、non-owning liveness capability、dup/fork sharing、flock retirement handoff 与当前 final-release hook
**不覆盖：** VFS inode lifetime、epoll watch lifecycle、动态 lifecycle observer registry
**实现位置：** `anemone-kernel/src/task/files/`
**依赖：** None
**Pending Successor：** None
**最后核验：** 2026-08-17

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| opened-description object 与 status flags | `ProcFile` | published `FileDesc` / transient strong borrow | dup/fork 共享同一 description 语义 |
| `Unpublished -> Live(n) -> Retired` semantic lifecycle | `ProcFile::description_refs` | published slot或move-only transfer reference | 决定 semantic final release 与 terminal liveness |
| fd number allocation / publication | file-table episode内的`FileTable` | task持`FilesState` participation；syscall持reservation或fd key | fd table 可见性与reuse |
| transferable opened-description reference | `task::files` lifecycle owner | Socket/Unix只持opaque move-only bundle；receiver持install plan | exact capture、peek duplication、abort与receiver publication |
| non-owning identity/liveness capability | `ProcFile` lifecycle owner | consumer 持 opaque capability或 operation-local lease | 比较 identity、取得短 live target lease并在 commit 前验证 |
| flock terminal-retirement handoff | `ProcFile` lifecycle owner编排；VFS flock domain执行grant cleanup | fd table/transfer drop只释放semantic reference；VFS只接收窄retirement context | 最后semantic ref移除后终结holder的flock relation并提交recheck hint |
| 静态 final-release hook | opened-description 创建时固定的 `FileDescOps` | hook 只借用 file/access/suppression context | 最后一个published或transfer reference移除后执行一次feature-neutral cleanup |

<a id="opened-desc-001--published-slot-refcount-是-final-release-的唯一真相"></a>

## OPENED-DESC-001 — Semantic refcount 是 final release 的唯一真相

**规则：** opened-description 的 semantic lifetime 由 `description_refs` 唯一编码为 `0 = Unpublished`、`1..=usize::MAX - 1 = Live(n)`、`usize::MAX = Retired`，而不是 syscall-local `Arc<FileDesc>` borrow 或底层 `File` 内存 lifetime。每个published fd slot和move-only transfer reference都在同一refword上exactly-once acquire/release；close、replacement、close-on-exec、table teardown、transfer abort或receiver conversion不能建立并列liveness truth。首次最终 `Live(1) -> Retired` 是 semantic final release，旧 identity 此后不得重新 publication。

**违反表现：** 临时 `get_fd()` clone 延迟 user-visible close、同一slot/transfer double release、sender close提前retire已排队transfer、fd reuse命中旧description，或底层`Arc<File>` drop偶然成为final-close truth。

**验证 / Enforcement：** `ProcFile::{acquire_description_ref,release_description_ref}`、`OpenedDescriptionTransfer`、`FileTable` publication/removal与task close/exit/dup/transfer paths的常开assertions、owner-local KUnit和source audit。

**最初来源：** live `task::files` opened-description model；fanotify final-release transaction history。

**当前来源：** [Unix SCM_RIGHTS RFC R0](../../rfcs/unix-scm-rights/index.md#closure)的`UNIX-SCM-RIGHTS-CUTOVER`。

## OPENED-DESC-002 — Dup/fork 共享 description，fd table 只拥有 publication

**规则：** dup 与非 `CLONE_FILES` fork 产生新的 published fd slots，但共享同一个 `ProcFile`；`CLONE_FILES`
tasks各自持有`FilesState` participation，并共享同一episode-owned `FileTable` publication set。关闭一个slot只
释放一个published reference，只要任一共享slot仍存在就不得触发terminal retirement。fd number、inode、path、
raw `File` pointer、`Weak::upgrade()`成功或底层storage存活均不能单独替代opened-description
identity/liveness。

**违反表现：** close 一个 dup fd 使其它 alias 被视为 final-closed、fork 后复制独立 status truth、共享 table 被一个 task exit 提前 drain，或 raw fd reuse 被当成同一 description。

**验证 / Enforcement：** `FileDesc::clone()`、`FileTable::fork()`、dup/close/exit与shared-table teardown audit；fd sharing regressions。

**最初来源：** live `task::files` fd sharing semantics。

**当前来源：** live `task::files` sharing model；[Epoll Stage 1 foundation cutover](../../devlog/transactions/2026-07-26-epoll.md#stage-1-closure-and-foundation-cutover---2026-07-26)。

## OPENED-DESC-TRANSFER-001 — Transfer capability保持exact opened description并原子转换publication

**规则：** sender file-table episode按输入fd顺序解析exact live slots并捕获opaque、move-only semantic transfer references；close/reuse之后仍指向原opened description。bundle只允许窄file-kind admission、复制semantic references供`MSG_PEEK`、move-only handoff与drop，不暴露`ProcFile`、fd-table lock或任意VFS operation。receiver install plan先保留不可见fd slots，再在owner guard外准备共享同一description的descriptors；全部ABI copyout成功后才在一个file-table episode中all-or-none publication。transfer到published slot的转换不得经历semantic ref为零的terminal gap；plan drop先回滚reserved slots，再在guard外释放未安装references。

**违反表现：** Unix queue保存raw `ProcFile`或独立alive bit；capture受fd reuse影响；peek复制queue truth而不取得semantic references；copy fault留下partial fd publication；conversion先释放transfer再取得slot reference；fd-table guard内触发flock/VFS/Socket final release。

**验证 / Enforcement：** capture bundle、transfer duplication、`OpenedDescriptionInstallPlan` reservation/rollback/commit、transfer-prepared `FileDesc` publication assertions与owner-local KUnit；Socket ABI fault/CTRUNC/CLOEXEC guest matrix和完整handoff source audit。

**最初来源：** [Unix SCM_RIGHTS RFC R0](../../rfcs/unix-scm-rights/index.md#closure)的`UNIX-SCM-RIGHTS-CUTOVER`。

**当前来源：** 同上。

## OPENED-DESC-003 — 当前 final-release callback 是创建时固定的单 hook

**规则：** 当前 `FileDescOps::final_release` 在 opened description 创建时固定，最多一个；最后published或transfer semantic reference移除后，`task::files`在不持有fd-table/Socket/Unix owner guard的调用路径执行它。hook只能使用窄`OpenedFileFinalReleaseCtx`，不得访问fd-table私有锁或重定义semantic-ref truth。该静态hook不提供运行期追加、多个observer组合或cancellation能力。

**违反表现：** 动态 feature 覆盖既有 hook、持 fd-table lock 回调外部 subsystem、hook 通过瞬时 borrow 数量决定 final release，或把当前单 hook 误写成可组合 registry。

**验证 / Enforcement：** `FileDescOps` construction sites、`release_description_ref()`与published/transfer所有release call paths audit；opened-description transfer与现有fanotify cleanup regression。

**最初来源：** live `FileDescOps::final_release` implementation。

**当前来源：** [Unix SCM_RIGHTS RFC R0](../../rfcs/unix-scm-rights/index.md#closure)的`UNIX-SCM-RIGHTS-CUTOVER`。

## OPENED-DESC-RETIRE-001 — Terminal retirement 固定进入窄 VFS flock handoff

**规则：** `ProcFile` lifecycle owner单独编排terminal retirement。fd table或transfer owner先在自己的private guard内撤销publication/reservation并释放guard；最后一个published或transfer semantic reference触发首次`Live(1) -> Retired`后，owner exactly-once进入固定、窄、不可失败的VFS flock
retirement handoff。handoff删除该holder已经存在的grant，并无论是否找到grant都向目标flock domain提交一次
recheck notification；完成边界只包括grant cleanup与notification submission，不等待waiter运行、errno选择、
physical placement或operation-local cleanup。handoff返回后才运行现有creation-time
`FileDescOps::final_release`。该顺序不建立dynamic observer registry、backend hook或future feature extension
surface。

**违反表现：** fd-table guard内回调VFS；同一terminal episode重复或遗漏handoff；没有grant时跳过notification；
static hook先于flock cleanup；close等待waiter execution/placement；或把固定flock handoff扩张为可注册的通用
lifecycle framework。

**验证 / Enforcement：** `ProcFile::release_description_ref()`、全部fd publication/removal与transfer capture/install/drop caller、
`OpenedDescriptionRetirementCtx`与`fs::lock::flock::retire_flock()` source audit和常开lifecycle assertions；
owner-local KUnit、focused dup/fork/final-close/no-grant waiter/concurrent-close regressions。

**最初来源：** [RFC-20260728-flock R0](../../rfcs/flock/invariants.md#opened-desc-retire-001---terminal-episode-固定进入窄-vfs-flock-cleanup)。

**当前来源：** [Unix SCM_RIGHTS RFC R0](../../rfcs/unix-scm-rights/index.md#closure)的`UNIX-SCM-RIGHTS-CUTOVER`；固定flock handoff本身最初由[FLOCK-CUTOVER](../../devlog/transactions/2026-07-29-flock.md#flock-cutover---2026-07-29)建立。

## OPENED-DESC-LIVENESS-001 — Non-owning capability 只验证 terminal opened-description liveness

**规则：** `task::files` 提供 opaque、non-owning 的 `OpenedDescriptionCapability`，只允许比较同一 opened-description identity、尝试取得当前 operation 使用的 non-cloneable live target lease，并在 commit 前验证同一 identity 仍为 `Live(n)`。capability 不暴露 lifecycle word、published count、fd-table lock 或行为型 escape hatch；lease 不能逃出当前 operation，也不能延迟或撤销 `Live(1) -> Retired`。final close 不通过 capability 主动回调 consumer。

**违反表现：** consumer 长期强持 target 代替 semantic liveness、`Weak::upgrade()` 成为 alive truth、lease 允许 retirement 后 commit、capability 复制 fd-table 私有状态，或 final close 被迫进入动态 observer。

**验证 / Enforcement：** `OpenedDescriptionCapability` / `OpenedDescriptionLease` surface、publication/release caller audit、常开 lifecycle assertions与 owner-local KUnit；consumer commit 前 validation review。

**最初来源：** [RFC-20260726-epoll R0](../../rfcs/epoll/invariants.md#watch-identity-与-opened-description-liveness)。

**当前来源：** [Epoll Stage 1 foundation cutover](../../devlog/transactions/2026-07-26-epoll.md#stage-1-closure-and-foundation-cutover---2026-07-26)。
