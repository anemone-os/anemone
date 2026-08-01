# VFS File Kind 与 Linux Mode Projection 当前契约

**Contract ID：** `VFS-FILE-KIND`
**状态：** Active
**Owner：** VFS immutable inode kind 与 Linux mode projection
**参与领域：** VFS inode / anonymous filesystem / stat ABI / persistent filesystem boundary
**覆盖范围：** inode file-kind truth、ordinary anonymous control object、Linux `S_IFMT` projection
**不覆盖：** inode identity topology、uid/gid credentials、opened-description flags、file operations、record-lock policy
**实现位置：** `anemone-kernel/src/fs/{inode/{metadata.rs,object.rs},anonymous/mod.rs,eventfd/mod.rs,timerfd/mod.rs,epoll/file.rs,fanotify/file.rs,api/getdents64.rs,ext4}`
**依赖：** None
**Pending Successor：** None
**最后核验：** 2026-07-31

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| inode file kind | immutable `Inode::ty()` | syscall、filesystem 与 backend 只读取 kind | admission、operation dispatch 与 Linux type projection |
| permission bits | inode metadata owner | `InodeMode` 组合时读取 snapshot | Linux mode permission projection |
| ordinary anon-control backend state | eventfd / timerfd / epoll / fanotify owner | VFS inode 只持 immutable kind 与 backend ops | object-specific I/O、readiness 与 lifecycle |

## VFS-FILE-KIND-001 — Inode kind 是唯一 file-type truth

**规则：** 每个 inode 的 file kind 在构造时一次确定，并由 immutable `Inode::ty()` 唯一拥有。
`InodeMode` 必须从该 kind 与 inode permission 派生 Linux mode；syscall、filesystem backend、
record-lock admission 或其它 consumer 不得根据 `FileOps` identity、路径、filesystem name、private
downcast 或 provider allow/deny list 维护第二份 file-kind truth。

Linux-style ordinary anon-inode control object 使用 internal `Anon` kind。该 kind 不是 Linux
`S_IF*` type，向 `stat` / `statx` 投影时贡献零个 `S_IFMT` bits；当前 ordinary control fd 的
permission projection 为 `0600`。位于 anonymous namespace 不等于 `Anon`：pipe/FIFO、tty/console
及其它有真实 UAPI kind 的 object 必须保留各自类型。`Anon` 不得作为未知或损坏的持久 inode
fallback，也不得被 persistent filesystem 编码成 regular inode。

**违反表现：** ordinary anonymous control fd 被报告为 `S_IFREG`；typed anonymous object 丢失
`S_IFIFO` / `S_IFCHR` 等真实类型；consumer 通过 provider 特判覆盖 VFS kind；另存 reported kind
并驱动 admission；或持久文件系统把 `Anon` 静默编码为 regular inode。

**验证 / Enforcement：** `InodeType` 与 `InodeMode` construction/projection source audit；ordinary
control fd 与 typed anonymous object construction-site audit；Rust 穷尽匹配；ext4 encode rejection；
RV64/LA64 release build。用户态、KUnit 与 guest runtime 在初始 cutover 中 Not Run。

**最初来源：** [Anonymous inode UAPI kind 小迭代](../../devlog/changes/2026-07-31-anon-inode-uapi-kind.md)。

**当前来源：** [Anonymous inode UAPI kind 原子 cutover](../../devlog/changes/2026-07-31-anon-inode-uapi-kind.md#contract-impact--cutover)。
