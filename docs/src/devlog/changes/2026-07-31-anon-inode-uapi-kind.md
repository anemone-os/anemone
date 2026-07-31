# ANE-CHG-20260731-anon-inode-uapi-kind

**Type:** Bugfix / ABI correction
**Status:** Completed
**Date:** 2026-07-31
**Authors:** doruche, Codex
**Area:** VFS / anonymous inode / stat ABI / eventfd / timerfd / epoll / fanotify

## Problem

Anemone 的 `InodeType` 原先不能表达 Linux ordinary anon-inode control object 的“已知但没有
`S_IFMT` file-type bits”语义。eventfd、timerfd、epoll 与 fanotify group fd 因此都以
`InodeType::Regular` 创建，`fstat()` / `statx()` 会把它们报告为 regular file，依赖
`inode.ty() == InodeType::Regular` 的 VFS admission 也可能错误接纳这些 control fd。

Linux 6.6.32 的 `alloc_anon_inode()` 把 `i_mode` 设为 `S_IRUSR | S_IWUSR`，没有设置任何
file-type bits；上述四类 control fd 均经 anon-inode helper 创建。证据位置为
`xref:linux-6.6.32:fs/libfs.c#alloc_anon_inode`、
`xref:linux-6.6.32:fs/anon_inodes.c#anon_inode_getfile`，以及各 consumer 的
`anon_inode_getfile()` / `anon_inode_getfd()` 调用点。

这是 VFS immutable inode kind 与 Linux mode projection 的局部 ABI 缺陷。目标、owner、持久化
边界与验证均能在一个 checkpoint 内关闭，不需要 staged RFC、probe 或 transitional contract。

## Scope

本轮只修正 ordinary anonymous control fd 的 VFS kind、permission 与 UAPI projection：

- eventfd、timerfd、epoll 与 fanotify group fd 使用 `InodeType::Anon`；
- `Anon` 向 Linux mode 投影零个 `S_IFMT` bits，ordinary control fd 的 mode 为 `0600`；
- pipe 保持 `Fifo`，boot tty / console 保持 `Char`；
- ext4 不得持久化 `Anon`，目录项 fallback 使用 `DT_UNKNOWN`。

本轮不改变 Anemone per-object anonymous inode identity，不复制 Linux singleton topology，不调整
uid/gid credential 语义，不改变 pipe、tty、console 或未来 memfd kind，也不实现 record lock。

## Solution

`InodeType` 新增内部 VFS kind `Anon`。它不是新的 Linux `S_IF*` type；
`InodeType::to_linux_mode_bits()` 对它返回零。`Inode::ty()` 仍在构造时一次确定，并继续作为
内部 admission 与 `stat` / `statx` mode projection 的唯一 file-kind truth，不增加 provider
特判、reported-kind cache 或第二份状态。

anonymous inode constructor 仅对 `Anon` 使用 owner read/write permission。位于 anonymous
namespace 本身不决定 kind，已有 typed object 继续保留显式类型与权限。ext4 encode 显式拒绝
`Anon`，load/open 分支标记为不可达，防止这个 internal-only kind 被误编码为 regular inode。

## Change

- `InodeType::Anon` 与 Linux mode 零 type-bit projection 已加入 VFS inode owner。
- eventfd、timerfd、epoll、fanotify group fd 的 construction site 已切换到 `Anon`。
- anonymous constructor 对 `Anon` 生成 `0600`，其它 typed anonymous object 保持原行为。
- `getdents64` 将意外出现的 `Anon` 投影为 `DT_UNKNOWN`；ext4 encode/load/open 边界显式拒绝或
  标记不可达。
- `VFS-FILE-KIND-001` 提取这一会被 record-lock admission 复用的 current rule；未增加 register
  条目或并列 contract 文本。

## Contract Impact / Cutover

| Contract ID | 变化 | Cutover 前 effective baseline | 新 effective 规则 | 生效证据 |
| --- | --- | --- | --- | --- |
| `VFS-FILE-KIND-001` | Replace | immutable `InodeType` 已是 file-kind truth，但 ordinary anon control fd 被构造为 `Regular`，向 UAPI 投影 `S_IFREG` | immutable kind 仍是唯一 truth；ordinary control fd 使用 `Anon`，投影 `0600` 且 `S_IFMT == 0`；typed anonymous object 保留真实 kind | construction/projection/persistence source audit，RV64/LA64 release build |

代码与 [`VFS-FILE-KIND-001`](../../contracts/vfs/file-kind.md#vfs-file-kind-001--inode-kind-是唯一-file-type-truth)
在本 commit 原子切换；current contract 是唯一 effective 正文。任一 source、build 或文档 gate 失败时
不提交该 checkpoint，旧 baseline 不会与新 contract 分开生效。

## Validation

- source audit 确认 `Anon::to_linux_mode_bits() == 0`；`stat` 与 `statx` 继续共享
  `InodeMode::to_linux_mode()`；四个 ordinary control fd construction site 全部使用 `Anon`。
- source audit 确认 pipe 使用 `Fifo`，boot tty / console 使用 `Char`；没有 provider exception、
  第二份 kind truth 或 ext4 regular-file fallback。
- `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G` 通过。首次 sandbox
  执行在 lwext4 C 编译阶段命中 host seccomp `SIGSYS`，同一仓库命令在 sandbox 外通过。
- `just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G` 通过。
- `just fmt kernel --check` 只报告未触达的既有 vendored smoltcp 格式漂移：
  `wire/sixlowpan/iphc.rs` 与 `wire/tcp.rs`；本轮没有写入这些文件。
- 用户态、KUnit、QEMU guest runtime、LTP 与 hardware 均 Not Run。这里不从 source/build 证据
  外推 runtime 结论；本轮也不增加只重复 immutable kind、纯 projection 与穷尽分支形状的测试。

## Tracking Issues

本轮没有剩余 local tracking issue。anonymous inode identity topology、credential ownership 与
record-lock 行为都明确位于 target 外，没有被重新分类为已解决能力。

## Risk / Follow-up

后续 record-lock RFC 应 Preserve `VFS-FILE-KIND-001` 并直接消费 VFS-owned kind，不得重新识别
eventfd、timerfd、epoll 或 fanotify provider。未来新增 ordinary anon control fd 时必须显式选择
`Anon`；memfd 等具有真实 regular-file 语义的对象不能仅因 anonymous construction 而使用该 kind。

## Links

- Biweekly devlog: [2026-07-20 至 2026-08-02](../2026-07-20_to_2026-08-02.md)
- Current contract: [VFS file kind 与 Linux mode projection](../../contracts/vfs/file-kind.md)
- Register / limitations: None
- RFC / transaction: None
- External source evidence: `xref:linux-6.6.32:fs/libfs.c#alloc_anon_inode`,
  `xref:linux-6.6.32:fs/anon_inodes.c#anon_inode_getfile`,
  `xref:linux-6.6.32:fs/eventfd.c#do_eventfd`,
  `xref:linux-6.6.32:fs/timerfd.c#timerfd_create`,
  `xref:linux-6.6.32:fs/eventpoll.c#do_epoll_create`,
  `xref:linux-6.6.32:fs/notify/fanotify/fanotify_user.c#fanotify_init`
- Issue / PR / commit: this change's Git commit
