# Opened-description Lifecycle 当前契约

**Contract ID：** `OPENED-DESC`
**状态：** Active
**Owner：** `task::files` 的 opened-description publication lifecycle
**参与领域：** task fd table / VFS opened file / fanotify / anonymous control fds
**覆盖范围：** opened-description identity、published fd-slot references、dup/fork sharing 与当前 final-release hook
**不覆盖：** VFS inode lifetime、transient `Arc<FileDesc>` borrow、epoll watch identity、尚不存在的动态 lifecycle observer registry
**实现位置：** `anemone-kernel/src/task/files.rs`
**依赖：** None
**Pending Successor：** [RFC-20260726-epoll R0](../../rfcs/epoll/invariants.md#contract-impact)，等待 `OPENED-DESC-CAPABILITY-CUTOVER`
**最后核验：** 2026-07-26

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| opened-description object 与 status flags | `ProcFile` | published `FileDesc` / transient strong borrow | dup/fork 共享同一 description 语义 |
| published fd-slot refcount | `ProcFile::description_refs` | fd-table mutation 返回待 release 的 `ProcFile` capability | 决定 semantic final release |
| fd number allocation / publication | `FilesState` | syscall 持 reservation 或 fd key | fd table 可见性与 reuse |
| 静态 final-release hook | opened-description 创建时固定的 `FileDescOps` | hook 只借用 file/access/suppression context | 最后一个 published slot 移除后执行一次 feature-neutral cleanup |

## OPENED-DESC-001 — Published slot refcount 是 final release 的唯一真相

**规则：** opened-description 的 semantic lifetime 由 published fd-table slots 计数，而不是 syscall-local `Arc<FileDesc>` borrow 或底层 `File` 内存 lifetime。每次 fd slot publication exactly-once acquire，每次 close、replacement、close-on-exec、table teardown 或 owner-approved handle replacement exactly-once release；只有 `description_refs` 的 `1 -> 0` 转换可以触发 final release。

**违反表现：** 临时 `get_fd()` clone 延迟 user-visible close、同一 slot double release、fd reuse 命中旧 description，或底层 `Arc<File>` drop 偶然成为 final-close truth。

**验证 / Enforcement：** `ProcFile::{acquire_description_ref,release_description_ref}`、`FilesState` publication/removal 与 task close/exit/dup paths 的常开 assertions 和 source audit。

**最初来源：** live `task::files` opened-description model；fanotify final-release transaction history。

**当前来源：** live `task::files` implementation，2026-07-26 源码核验。

## OPENED-DESC-002 — Dup/fork 共享 description，fd table 只拥有 publication

**规则：** dup 与非 `CLONE_FILES` fork 产生新的 published fd slots，但共享同一个 `ProcFile`；`CLONE_FILES` tasks 共享同一 `FilesState` publication set。关闭一个 slot 只释放一个 published reference，只要任一共享 slot 仍存在就不得触发 final release。fd number、inode、path 或 raw `File` pointer 均不能单独替代 opened-description identity。

**违反表现：** close 一个 dup fd 使其它 alias 被视为 final-closed、fork 后复制独立 status truth、共享 table 被一个 task exit 提前 drain，或 raw fd reuse 被当成同一 description。

**验证 / Enforcement：** `FileDesc::clone()`、`FilesState::fork()`、dup/close/exit 与 shared-table teardown audit；fd sharing regressions。

**最初来源：** live `task::files` fd sharing semantics。

**当前来源：** live `task::files` implementation，2026-07-26 源码核验。

## OPENED-DESC-003 — 当前 final-release callback 是创建时固定的单 hook

**规则：** 当前 `FileDescOps::final_release` 在 opened description 创建时固定，最多一个；最后 published reference 移除后，`task::files` 在不持有 fd-table write guard 的调用路径执行它。hook 只能使用窄 `OpenedFileFinalReleaseCtx`，不得访问 fd-table 私有锁或重定义 published-ref truth。该静态 hook 不提供运行期追加、多个 observer 组合或 cancellation 能力。

**违反表现：** 动态 feature 覆盖既有 hook、持 fd-table lock 回调外部 subsystem、hook 通过瞬时 borrow 数量决定 final release，或把当前单 hook 误写成可组合 registry。

**验证 / Enforcement：** `FileDescOps` construction sites、`release_description_ref()` 与所有 release call paths audit；现有 fanotify cleanup regression。

**最初来源：** live `FileDescOps::final_release` implementation。

**当前来源：** live `task::files` implementation，2026-07-26 源码核验。
