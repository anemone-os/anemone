# 2026-06-18 - Mount Tree Legacy API

**状态：** Active
**负责人：** doruche, Codex
**领域：** fs / VFS / mount / LTP
**权威计划：** [RFC-20260604-mount-tree-legacy-api](../../rfcs/mount-tree-legacy-api/index.md), [不变量需求](../../rfcs/mount-tree-legacy-api/invariants.md), [迁移实施计划](../../rfcs/mount-tree-legacy-api/implementation.md)
**当前阶段：** 阶段 0 公开 RFC 协议关闭已完成；阶段 1 UAPI parser 尚未启动

## 范围

本事务跟踪 `mount-tree-legacy-api` RFC 的 staged 实现：

- legacy `mount(2)` / `umount(2)` / `umount2(2)` 的 flag parser、errno 和 stable reject 边界；
- `NameSpace` 到 `MountTree` 的 topology owner 迁移、transaction lock、mountpoint stack 和 lookup retry；
- ordinary per-mount readonly remount、plain bind、recursive bind、private move mount 和有限 propagation；
- `MNT_DETACH` / `UMOUNT_NOFOLLOW`、pre-unmount cleanup、`/proc/<tgid>/mounts` live view 和 LTP cleanup 支撑；
- 阶段 7 的 LTP profile 收口、accepted limitation 登记和 RFC / register / transaction closeout。

第一版不包含 new mount API、真实 mount namespace / nsproxy、`pivot_root(2)`、file bind mount、完整 shared/slave propagation、sb-wide ordinary remount reconfigure、`MS_NOEXEC` / `MS_NODEV` / `MS_NOSUID` 接入或 readonly mmap/writeback 强一致性。

## 不变量

- `PathRef = Mount + Dentry` 是唯一正式位置表示。
- `MountTree` 是 mount topology 的唯一写侧 owner；syscall handler 和 filesystem backend 不得绕过它修改拓扑。
- `Mount` 是 attached view；bind/rbind/move/remount/unmount 处理 mount view，不直接等同于 superblock 生命周期。
- `RDONLY` 第一版是 per-mount enforcement，不是 sb-wide readonly 或 filesystem reconfigure。
- syscall raw flags 必须分为 operation flags、per-mount attrs、future superblock state 和 filesystem type flags。
- fstype alias 只能停在 syscall adapter，不能下沉到 `MountTree`、`Mount`、`SuperBlock`、`FileSystemOps` 或 backend。
- 不完整 feature 不能返回用户可见成功；unsupported flag/data/operation 必须稳定拒绝并打日志。
- worker 未经批准不得越过分配 write set；需要扩大 write set 时必须先上报并在本事务日志记录批准边界。

## 阶段日志

### 2026-06-18 - 阶段 0 事务启动与公开 RFC 协议关闭

**阶段：** 阶段 0 - 公开 RFC 协议关闭。

**变更：** 在代码实现前建立事务日志，并把 RFC、事务索引、mdBook Summary 和当前双周 devlog 连接到同一条实现记录。未修改内核代码，未启动阶段 1 worker。

**文档层核验：**

- `docs/src/rfcs/mount-tree-legacy-api/index.md` 已限定第一版范围：legacy mount API、`MountTree` topology owner、per-mount readonly、unmount / proc mounts 和 staged LTP 兼容；new mount API、真实 namespace、`pivot_root` 等仍是非目标或 follow-up。
- `invariants.md` 已闭合 `PathRef`、`Mount`、`MountTree`、`SuperBlock`、flags、linearization point、placement lock、busy / cleanup 和禁止退化项。
- `implementation.md` 已按阶段 0-7 拆分，每阶段包含 write set、模块边界预检、可观测性、验证和退出条件。
- `tracking-issues.md` 当前为 Closed；无 active Apollyon / Keter / Euclid / Safe。lazy-detach final cleanup owner 和 detached / moved `PathRef` namei 语义已重分类为阶段 6 Gate P1/P2 受控反馈，不阻塞阶段 1。
- `register/current-limitations.md` 仍保留 readonly mmap/writeback、remount/bind/move mount 和 mount flag 组合未系统化的限制；阶段 0 不把这些限制声明为已解决。
- 本次前置检查开始时工作树干净；当前分支为 `dev/drc/mount-legacy`。

**阶段 1 默认 write set：**

- `anemone-abi/src/fs.rs`
- `anemone-kernel/src/fs/api/mount.rs`
- `anemone-kernel/src/fs/api/umount.rs`
- `anemone-kernel/src/fs/filesystem.rs`
- `anemone-kernel/src/fs/mount.rs`
- 必要的 filesystem backend mount signature 调整文件，如 `ramfs`、`devfs`、`procfs`、`ext4`

阶段 1 只允许处理 UAPI parser、flag 分层、legacy `MountData`、syscall alias 和 stable reject / log 边界。若 parser 无法留在 syscall adapter，或者 raw `MS_*` / fstype alias 必须泄漏到 `MountTree` 或 backend，worker 必须停止并上报。

**准备启动的 agent 列表：**

- `phase1-mount-parser-worker`：阶段 1 implementation worker。只处理 UAPI 常量、raw flag 分类、`MountAttrFlags`、`MountData`、fstype alias syscall boundary 和 unsupported flag/data stable reject。
- `phase1-reviewer`：阶段 1 worker 返回后的只读 reviewer。检查 raw operation bits、alias、unsupported data / flag 是否泄漏到 VFS owner 或 backend，以及是否存在 silent success。
- `phase2-mounttree-explorer`：阶段 1 关闭后再启动的只读 explorer。只盘点 `NameSpace`、`Mount` placement、stack lookup、KUnit 基线和 `PathRef` 调用点，不编辑文件。

本条记录不批准也不启动任何阶段 2+ implementation worker。

**停止条件：**

- worker 需要编辑分配 write set 之外的文件。
- unsupported mount flag、operation、data option 或 filesystem alias 命中后仍返回成功。
- fstype alias 或 raw Linux operation flags 需要进入 `MountTree`、filesystem backend 或 `MountFlags` 长期状态。
- 阶段 1 实现需要改变 `PathRef`、`MountTree` topology owner、superblock lifetime、readonly accepted boundary 或后续阶段顺序。
- 需要新增 new mount API、detached mount fd、mount namespace、`pivot_root`、file bind mount 或 propagation peer group 状态。

**验证：** 本阶段只做文档层核验和事务链接；导航 patch 后运行 `git diff --check`、新增文件 whitespace 检查和 `mdbook build docs`。

**下一步：** 完成文档验证后，等待总控启动阶段 1 worker。

### 2026-06-18 - 阶段 0 文档验证

**阶段：** 阶段 0 - 事务链接验证。

**变更：** 修正 `invariants.md` 状态为 Canonical，避免 accepted implementation worker 继续看到 Draft invariant。未修改内核代码。

**验证：**

- `git diff --check`：通过。
- `git diff --no-index --check -- /dev/null docs/src/devlog/transactions/2026-06-18-mount-tree-legacy-api.md`：无 whitespace 诊断；命令退出码为 1，是新增文件与 `/dev/null` 比较时的正常 no-index difference 状态。
- `mdbook build docs`：通过，输出到 `docs/book`。
- stale 状态搜索：首次发现本次修正前的 `invariants.md` Draft 标记；修正后复查无匹配。

**下一步：** 阶段 0 关闭。下一次实现动作只能启动阶段 1 `phase1-mount-parser-worker`，并按本事务日志记录的 write set / 停止条件推进。
