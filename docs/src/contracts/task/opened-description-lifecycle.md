# Opened-description Lifecycle 当前契约

**Contract ID：** `OPENED-DESC`
**状态：** Active
**Owner：** `task::files` 的 opened-description publication lifecycle
**参与领域：** task fd table / VFS opened file / VFS flock / fanotify / anonymous control fds
**覆盖范围：** opened-description identity、terminal publication lifecycle、non-owning liveness capability、dup/fork sharing、flock retirement handoff 与当前 final-release hook
**不覆盖：** VFS inode lifetime、epoll watch lifecycle、动态 lifecycle observer registry
**实现位置：** `anemone-kernel/src/task/files/`
**依赖：** None
**Pending Successor：** None
**最后核验：** 2026-07-29

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| opened-description object 与 status flags | `ProcFile` | published `FileDesc` / transient strong borrow | dup/fork 共享同一 description 语义 |
| `Unpublished -> Live(n) -> Retired` publication lifecycle | `ProcFile::description_refs` | fd-table mutation 返回待 release 的 `ProcFile` capability | 决定 semantic final release 与 terminal liveness |
| fd number allocation / publication | file-table episode内的`FileTable` | task持`FilesState` participation；syscall持reservation或fd key | fd table 可见性与reuse |
| non-owning identity/liveness capability | `ProcFile` lifecycle owner | consumer 持 opaque capability或 operation-local lease | 比较 identity、取得短 live target lease并在 commit 前验证 |
| flock terminal-retirement handoff | `ProcFile` lifecycle owner编排；VFS flock domain执行grant cleanup | fd table只返回待release的`ProcFile`；VFS只接收窄retirement context | final published ref移除后终结holder的flock relation并提交recheck hint |
| 静态 final-release hook | opened-description 创建时固定的 `FileDescOps` | hook 只借用 file/access/suppression context | 最后一个 published slot 移除后执行一次 feature-neutral cleanup |

## OPENED-DESC-001 — Published slot refcount 是 final release 的唯一真相

**规则：** opened-description 的 semantic lifetime 由 `description_refs` 唯一编码为 `0 = Unpublished`、`1..=usize::MAX - 1 = Live(n)`、`usize::MAX = Retired`，而不是 syscall-local `Arc<FileDesc>` borrow 或底层 `File` 内存 lifetime。每次 fd slot publication exactly-once acquire，每次 close、replacement、close-on-exec、table teardown 或 owner-approved handle replacement exactly-once release；首次最终 `Live(1) -> Retired` 是 semantic final release，旧 identity 此后不得重新 publication。

**违反表现：** 临时 `get_fd()` clone 延迟 user-visible close、同一 slot double release、fd reuse 命中旧 description，或底层 `Arc<File>` drop 偶然成为 final-close truth。

**验证 / Enforcement：** `ProcFile::{acquire_description_ref,release_description_ref}`、`FileTable` publication/removal 与 task close/exit/dup paths 的常开 assertions 和 source audit。

**最初来源：** live `task::files` opened-description model；fanotify final-release transaction history。

**当前来源：** [Epoll Stage 1 foundation cutover](../../devlog/transactions/2026-07-26-epoll.md#stage-1-closure-and-foundation-cutover---2026-07-26)。

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

## OPENED-DESC-003 — 当前 final-release callback 是创建时固定的单 hook

**规则：** 当前 `FileDescOps::final_release` 在 opened description 创建时固定，最多一个；最后 published reference 移除后，`task::files` 在不持有 fd-table write guard 的调用路径执行它。hook 只能使用窄 `OpenedFileFinalReleaseCtx`，不得访问 fd-table 私有锁或重定义 published-ref truth。该静态 hook 不提供运行期追加、多个 observer 组合或 cancellation 能力。

**违反表现：** 动态 feature 覆盖既有 hook、持 fd-table lock 回调外部 subsystem、hook 通过瞬时 borrow 数量决定 final release，或把当前单 hook 误写成可组合 registry。

**验证 / Enforcement：** `FileDescOps` construction sites、`release_description_ref()` 与所有 release call paths audit；现有 fanotify cleanup regression。

**最初来源：** live `FileDescOps::final_release` implementation。

**当前来源：** live `task::files` implementation，2026-07-26 源码核验。

## OPENED-DESC-RETIRE-001 — Terminal retirement 固定进入窄 VFS flock handoff

**规则：** `ProcFile` lifecycle owner单独编排terminal retirement。fd table先在自己的private guard内撤销slot
publication并释放guard；首次`Live(1) -> Retired`后，owner exactly-once进入固定、窄、不可失败的VFS flock
retirement handoff。handoff删除该holder已经存在的grant，并无论是否找到grant都向目标flock domain提交一次
recheck notification；完成边界只包括grant cleanup与notification submission，不等待waiter运行、errno选择、
physical placement或operation-local cleanup。handoff返回后才运行现有creation-time
`FileDescOps::final_release`。该顺序不建立dynamic observer registry、backend hook或future feature extension
surface。

**违反表现：** fd-table guard内回调VFS；同一terminal episode重复或遗漏handoff；没有grant时跳过notification；
static hook先于flock cleanup；close等待waiter execution/placement；或把固定flock handoff扩张为可注册的通用
lifecycle framework。

**验证 / Enforcement：** `ProcFile::release_description_ref()`、全部fd publication/removal caller、
`OpenedDescriptionRetirementCtx`与`fs::lock::flock::retire_flock()` source audit和常开lifecycle assertions；
owner-local KUnit、focused dup/fork/final-close/no-grant waiter/concurrent-close regressions。

**最初来源：** [RFC-20260728-flock R0](../../rfcs/flock/invariants.md#opened-desc-retire-001---terminal-episode-固定进入窄-vfs-flock-cleanup)。

**当前来源：** [FLOCK-CUTOVER](../../devlog/transactions/2026-07-29-flock.md#flock-cutover---2026-07-29)。

## OPENED-DESC-LIVENESS-001 — Non-owning capability 只验证 terminal opened-description liveness

**规则：** `task::files` 提供 opaque、non-owning 的 `OpenedDescriptionCapability`，只允许比较同一 opened-description identity、尝试取得当前 operation 使用的 non-cloneable live target lease，并在 commit 前验证同一 identity 仍为 `Live(n)`。capability 不暴露 lifecycle word、published count、fd-table lock 或行为型 escape hatch；lease 不能逃出当前 operation，也不能延迟或撤销 `Live(1) -> Retired`。final close 不通过 capability 主动回调 consumer。

**违反表现：** consumer 长期强持 target 代替 semantic liveness、`Weak::upgrade()` 成为 alive truth、lease 允许 retirement 后 commit、capability 复制 fd-table 私有状态，或 final close 被迫进入动态 observer。

**验证 / Enforcement：** `OpenedDescriptionCapability` / `OpenedDescriptionLease` surface、publication/release caller audit、常开 lifecycle assertions与 owner-local KUnit；consumer commit 前 validation review。

**最初来源：** [RFC-20260726-epoll R0](../../rfcs/epoll/invariants.md#watch-identity-与-opened-description-liveness)。

**当前来源：** [Epoll Stage 1 foundation cutover](../../devlog/transactions/2026-07-26-epoll.md#stage-1-closure-and-foundation-cutover---2026-07-26)。
