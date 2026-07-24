# VFS Mount Admission 当前契约

**Contract ID：** `VFS-MOUNT-ADMISSION`
**状态：** Active
**Owner：** VFS mount-admission protocol
**参与领域：** filesystem registry / legacy mount syscall / filesystem backends / procfs mount projection / anemone-rs
**覆盖范围：** plain new mount 的 canonical fstype、no-device / block-device source admission、typed backend handoff 与 fstype alias containment
**不覆盖：** bind/move/remount/propagation、mount attrs、unmount cleanup、filesystem-private data、`/proc/filesystems`、省略 `-t` 的 probe、network/file/UUID/LABEL source
**实现位置：** `anemone-kernel/src/fs/{filesystem.rs,api/mount/mount.rs,proc,ramfs,devfs,ext4,anonymous}`、`anemone-rs/src/os/linux.rs`
**依赖：** None
**Pending Successor：** None
**最后核验：** 2026-07-24

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| canonical fstype identity | filesystem registry / filesystem type | syscall adapter 持 lookup 结果；procfs 只读投影 | registry lookup、mount ABI fstype 与展示 |
| filesystem source requirement | source-kind-tagged filesystem mount operation | syscall adapter 读取即时派生分类 | 将 raw source 收敛为 typed source |
| raw fstype alias 与 raw source 解析 | legacy mount syscall adapter | VFS/backend 只收到 canonical fstype 与 typed source | 用户 ABI admission、errno 与诊断 |
| superblock backing | filesystem backend / `SuperBlock` | mount view 持 `Arc<SuperBlock>` | backing identity 与 backend lifetime |

## VFS-MOUNT-ADMISSION-001 — Canonical filesystem identity

**规则：** 每个已注册 filesystem type 只有一个 canonical name。该 name 驱动 registry lookup、mount fstype 和 `/proc/<tgid>/mounts`（包括 `/proc/mounts` self view）的 fstype 列；no-device mount 的 source 列也使用 canonical name，block-device mount 的 source 列仍投影为 `dev(<devnum>)`。procfs 的 canonical name 是 `proc`；模块名、backend 术语和 compatibility alias 不得形成并列 identity。raw `procfs` 没有 legacy alias，按 unknown fstype 失败。

**违反表现：** 同一个 filesystem 以多个 registry key 注册；backend/internal name 泄漏为第二个可调用 ABI name；fstype 列偏离 registry identity；或保存 raw no-device source label作为并列展示真相源。

**验证 / Enforcement：** `FileSystemOps::name`、filesystem registry、procfs registration 与 `/proc/<tgid>/mounts` source audit；RV64 guest 使用 canonical non-null `proc` source/fstype 完成 `/proc` 初始化。block source projection保持 `dev(<devnum>)` 的 source audit。本规则不构成 `/proc/filesystems` 验证。

**最初来源：** 既有 filesystem registry unique-name baseline 与 Closed [mount-tree-legacy-api RFC](../../rfcs/mount-tree-legacy-api/index.md)。

**当前来源：** [mount fstype/source compatibility 小迭代](../../devlog/changes/2026-07-24-mount-fstype-source-compat.md)。

## VFS-MOUNT-ADMISSION-002 — Source-kind-owned admission

**规则：** filesystem type 必须用 source-kind-tagged mount operation 将 no-device / block-device requirement 与 backend callback input 绑定为同一份真相源；不得再保存独立、可能漂移的 requirement 字段。legacy syscall adapter 在进入 VFS mount transaction 前解析 raw source：no-device 接受 null 或任意合法 source label、丢弃 label 并形成 `MountSource::Pseudo`；block-device 要求 non-null path，解析为已注册 block handle 后形成 `MountSource::Block`。统一 dispatch 只把 mount data交给 no-device callback，只把 block handle与 mount data交给 block-device callback；backend、mount tree和 procfs不得重新解释 raw source。

只表达 `flags=0`、`data=NULL` plain new mount 的高层 userspace wrapper 必须要求 caller 提供 source。需要表达 nullable raw pointer 的底层 syscall-word adapter可以保留该能力，但 in-tree app不得用它恢复 filesystem-specific null-source workaround。

**违反表现：** syscall 以 fstype 字符串特判 source kind；backend 再次匹配 `MountSource`；no-device label进入 backing identity；block filesystem接受 null/non-block source；callback variant与 resolved source不匹配；或高层 app wrapper继续把 `None` 当 pseudo mount约定。

**验证 / Enforcement：** tagged callback registrations与所有 backend signature source audit；`FileSystem::mount()` 的普通 `assert!` 锁定 variant/source handoff；RV64 release build、255项 KUnit、迁移后的 `user-test` guest初始化以及 mount01..07定向回归。

**最初来源：** [mount fstype/source compatibility 小迭代](../../devlog/changes/2026-07-24-mount-fstype-source-compat.md)。

**当前来源：** 同最初来源。

## VFS-MOUNT-ADMISSION-003 — Syscall-only alias containment

**规则：** fstype compatibility alias 只能由 legacy mount syscall adapter归一化，且必须在进入 VFS transaction前变成 canonical name。filesystem registry、filesystem operation、`MountTree`、`Mount`、`SuperBlock` 和 backend不得保存 raw alias，也不得把 alias成功宣称为真实 filesystem coverage。当前保留的 scoring bridge只有 `tmpfs -> ramfs` 与 `ext2` / `ext3` / `vfat -> ramfs`；不存在 `procfs -> proc` alias。

**违反表现：** alias进入 registry/backend状态；`/proc/mounts` 展示 raw alias；alias命中被当作真实 tmpfs/ext2/ext3/vfat支持；或新增无理由、无日志、无退出条件的别名。

**验证 / Enforcement：** `normalize_fstype()` 与 alias log source audit；KUnit确认 canonical `proc`不走 alias、raw `procfs`不被归一化，并锁定现有 scoring alias；current limitation持续记录 scoring bridge边界。

**最初来源：** Closed [mount-tree-legacy-api RFC](../../rfcs/mount-tree-legacy-api/invariants.md) 的 syscall-adapter containment边界。

**当前来源：** [mount fstype/source compatibility 小迭代](../../devlog/changes/2026-07-24-mount-fstype-source-compat.md)完成 baseline提取；规则语义保持不变。

