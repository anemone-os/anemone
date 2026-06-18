# 2026-06-18 - Mount Tree Legacy API

**状态：** Active
**负责人：** doruche, Codex
**领域：** fs / VFS / mount / LTP
**权威计划：** [RFC-20260604-mount-tree-legacy-api](../../rfcs/mount-tree-legacy-api/index.md), [不变量需求](../../rfcs/mount-tree-legacy-api/invariants.md), [迁移实施计划](../../rfcs/mount-tree-legacy-api/implementation.md)
**当前阶段：** 阶段 1 UAPI parser 已关闭；阶段 2 尚未启动

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

### 2026-06-18 - 阶段 1 UAPI parser 本地实现

**阶段：** 阶段 1 - UAPI parser、flag 分层、legacy data 和 syscall alias。

**write set 扩展：**

- 用户批准将 `anemone-kernel/src/fs/api/mount.rs` 和 `anemone-kernel/src/fs/api/umount.rs` 收归到 `anemone-kernel/src/fs/api/mount/` 目录，用于 legacy mount / umount syscall 和未来 mount-family syscall adapter。
- `implementation.md` 已同步记录该扩展，并允许 `anemone-kernel/src/fs/api/mod.rs` 做模块声明调整。
- 阶段 1 实现需要把 legacy `MountData` 从 syscall adapter 传到 filesystem backend，因此增加 `anemone-kernel/src/fs/mod.rs` 的 syscall-only `mount_at_with_data` call-through。该 helper 只透传 `MountData`，未改变当前 `NameSpace` / future `MountTree` topology owner、stack visibility、bind/move/remount 或 unmount transaction 语义。

**源码变更：**

- `anemone-abi/src/fs.rs` 补齐 legacy mount/umount 第一版需要识别的 Linux UAPI 常量，包括 `MS_BIND`、`MS_REC`、`MS_MOVE`、`MS_REMOUNT`、propagation bits、`MS_SILENT` / `MS_VERBOSE`、`MNT_DETACH`、`MNT_EXPIRE`、`MNT_FORCE` 和 `UMOUNT_NOFOLLOW`。
- `anemone-kernel/src/fs/api/mount/{mod.rs,mount.rs,umount.rs}` 接管 mount-family syscall adapter。`sys_mount()` 现在在 syscall 边界完成 raw flag allowlist、operation bit stable reject、unsupported attr stable reject、harmless `MS_SILENT` 兼容日志、legacy `MountData` 读取、`loop` data option stable reject、fstype normalization 和 source policy。
- `tmpfs -> ramfs` 作为 ramfs 兼容入口保留；`ext2` / `ext3` / `vfat -> ramfs` 只作为 LTP 临时 bridge，命中时记录 raw fstype、normalized fstype、兼容原因和退出条件。归一化后的内部请求只携带 normalized fstype。
- `sys_umount2()` 现在识别 `MNT_FORCE`、`MNT_DETACH`、`MNT_EXPIRE`、`UMOUNT_NOFOLLOW`；未知 bit 和已知但未闭合语义的 flag 稳定返回 `EINVAL`，`MNT_EXPIRE | (MNT_FORCE | MNT_DETACH)` 也返回 `EINVAL`。
- `MountAttrFlags` 与 legacy operation bits 分离；阶段 1 只把 `RDONLY` 转成现有 `MountFlags::RDONLY`。`MountData` 提供 `Null` / `Text` 对象、`loop` option 检测和 backend 非空 data 拒绝 helper。
- `FileSystemOps::mount()` / `FileSystem::mount()` 接收 `MountData`。`ramfs`、`devfs`、`procfs`、`ext4` 目前只接受 `NULL` / 空 data，非空 data 记录 filesystem type、empty=false 和 contains_loop 后返回 `EINVAL`；anonymous fs 内部 mount 断言 data 为空。

**审计结果：**

- `rg -n "\\[NYI\\].*mount|ignoring unsupported.*mount|unsupported.*mount" anemone-kernel/src`：无输出；旧 unsupported flag silent-ignore 路径已移除。
- `rg -n "tmpfs|ext2|ext3|vfat|mount_fs_name|FsAliasKind|normalize_fstype|ltp-temporary-bridge" ...`：alias 命中只在 RFC 文本和 `anemone-kernel/src/fs/api/mount/mount.rs` 中出现，未进入 VFS helper 或 backend。
- `rg -n "MS_|MNT_|UMOUNT_NOFOLLOW" anemone-kernel/src/fs anemone-abi/src/fs.rs`：raw Linux mount / umount flag 只在 ABI 常量、syscall parser 和 `MountAttrFlags` 注释 / KUnit 中出现。
- `rg -n "MountData|MountAttrFlags|mount_at_with_data|FileSystemOps|FileSystem::mount\\(|\\.mount\\(" anemone-kernel/src/fs anemone-abi/src/fs.rs`：`MountData` 只作为 syscall-to-backend parser input 透传；`mount_at_with_data` 只在 VFS syscall facade 和 mount syscall adapter 使用。
- `find anemone-kernel/src/fs/api -maxdepth 3 -type f | sort`：确认旧 `fs/api/mount.rs` / `fs/api/umount.rs` 已目录化为 `fs/api/mount/{mod.rs,mount.rs,umount.rs}`。

**本地验证：**

- `just fmt kernel`：通过。
- `just build`：通过；KUnit 编译进 kernel build，未运行 QEMU KUnit runner。构建仅保留既有 `anemone-kernel/src/sync/mono.rs` 的 `AtomicBool` / `Ordering` unused import warning。
- `git diff --check`：通过。
- `mdbook build docs`：通过，输出到 `docs/book`。
- 新增 `anemone-kernel/src/fs/api/mount/{mod.rs,mount.rs,umount.rs}` 的 `git diff --no-index --check -- /dev/null <file>`：无 whitespace 诊断；退出码 1 是新增文件与 `/dev/null` 存在差异的正常结果。

**review gate：** `phase1-reviewer`（Epicurus）已按只读任务启动，60 秒内尚未返回；未中止该 agent。阶段 1 closeout 等待 reviewer 结论或用户处理异常。

**未运行验证：** 未运行 QEMU、user-test、LTP mount profile 或 runtime KUnit runner。

**下一步：** 等待 `phase1-reviewer` 返回。若无 blocker，则补写阶段 1 closeout 并提交；若发现 raw flag / alias 泄漏、silent success 或 write set 越界，则先修正或停止上报。

### 2026-06-18 - 阶段 1 review closeout

**阶段：** 阶段 1 - review gate。

**reviewer：** `phase1-reviewer` / Epicurus，只读审查。

**结论：**

- 代码层无 blocking finding。reviewer 未发现阶段 1 越界打开 `MountTree` topology、stack、remount、bind 或 move 语义。
- reviewer 确认 `fs/mod.rs` 当前只是 `MountData` 透传 helper，没有改变 stack 可见性或 topology owner 行为。
- reviewer 确认 raw `MS_*` / `MNT_*` 只停留在 ABI 与 syscall parser；fstype alias 表只在 `anemone-kernel/src/fs/api/mount/mount.rs`，未下沉到 `MountTree`、`MountFlags`、`FileSystemOps` 或 backend 长期状态。
- reviewer 确认 unsupported mount flags、umount flags、non-empty data 和 `loop` data 都有 stable reject 路径。

**review finding 处置：**

- reviewer 提出一个 Keter：审查开始时 transaction devlog 顶部状态和阶段 1 默认 write set 仍停在阶段 0 记录，未反映用户批准的 `fs/api/mount/**`、`fs/api/mod.rs` 和 `fs/mod.rs` 扩展。
- 处置：本事务日志已追加“阶段 1 UAPI parser 本地实现”条目，记录用户批准的 syscall adapter 目录化、`fs/mod.rs` 的 `MountData` call-through 扩展、当前实现结果、审计和验证；`implementation.md` 的阶段 1 write set / 结构维护记录也已同步更新。阶段 0 条目中的“默认 write set”保留为当时启动记录，不再作为当前边界。
- 状态：Neutralized。

**KUnit / 负例覆盖：**

- mount parser KUnit 覆盖 `MS_RDONLY | MS_SILENT` 成功、`MS_BIND` / `MS_REMOUNT | MS_BIND | MS_RDONLY` operation bit 拒绝、`MS_NOEXEC` unsupported attr 拒绝和 unknown bit 拒绝。
- fstype alias KUnit 覆盖 `tmpfs`、`ext2` 和真实 `ext4` normalization 边界。
- mount data KUnit 覆盖 `loop`、`rw, loop`、`loop=/tmp/disk.img` 检测和普通 `rw` 非 loop option。
- `MountData` KUnit 覆盖 backend 非空 data 拒绝为 `EINVAL`。
- umount parser KUnit 覆盖 zero flags、unknown flags、known-but-unsupported `MNT_DETACH` / `UMOUNT_NOFOLLOW` 和 `MNT_EXPIRE` invalid combinations。
- 这些 KUnit 随 `just build` 的 `--features kunit` 编译通过；本阶段未运行 runtime KUnit runner。

**阶段 1 关闭判断：**

- 不再存在 unsupported `MS_*` 被忽略后成功的路径。
- `MS_RDONLY` 仍能作为 per-mount attr 转成现有 `MountFlags::RDONLY` 进入 new mount。
- fstype alias bridge 只在 syscall adapter，且每个 alias 有日志和退出条件。
- `loop` data option 不伪成功；普通非空 data 由 backend helper 稳定拒绝。
- 没有启动阶段 2+ implementation worker；阶段 2 `MountTree` owner / stack 迁移仍未开始。

**post-review 验证：**

- `rg -n "MS_|MNT_|UMOUNT_|MountFlags|MountAttr|MountOp" anemone-abi/src anemone-kernel/src`：预期命中 ABI、mount syscall parser、`MountAttrFlags` / `MountFlags` 和既有 `msync` 常量；未发现 raw mount operation flag 下沉到 backend 或 topology owner。
- `rg -n "tmpfs|ext2|ext3|vfat|mount_fs_name|MountData|loop" anemone-kernel/src/fs/api/mount anemone-kernel/src/fs`：预期命中 syscall alias、`MountData` 透传、loop option 检测和既有非 mount parser 的普通 `loop` 代码；未发现 alias 表进入 VFS helper 或 backend。
- `git diff --check`：通过。
- `mdbook build docs`：通过。
- `just build`：通过；仍仅保留既有 `anemone-kernel/src/sync/mono.rs` unused import warning。

**下一步：** 阶段 2 只能在阶段 1 commit 后启动；若启动，应先运行只读 `phase2-mounttree-explorer`，不得直接修改 topology owner。

### 2026-06-18 - `MountFlags` 迁移桥反馈回写

**阶段：** 阶段 1 关闭后的 implementation feedback；阶段 2 尚未启动。

**反馈：**

- 当前源码中 `MountFlags` 已只剩 `RDONLY`，阶段 1 又引入了语义更准确的 `MountAttrFlags`。继续保留两者会在阶段 3 remount attr plumbing 时制造 per-mount attrs 双真相源风险。
- 用户确认 RFC 应记录：`MountFlags` 在本 RFC 收口后应被删除，不作为长期抽象保留。

**RFC 回写：**

- `index.md` 已明确 `MountFlags` 只允许作为阶段迁移桥；阶段 3 关闭后 `Mount` 直接持有 `MountAttrFlags` 或等价 attrs storage，`FileSystemOps::mount()` 不再接收 per-mount attrs。
- `invariants.md` 已新增禁止保留 `MountFlags` / `MountAttrFlags` 并列真相源的约束。
- `implementation.md` 已把删除 `MountFlags` 纳入阶段 3 交付、审计和退出条件，并把阶段 3 write set 扩展到 `anemone-kernel/src/fs/filesystem.rs` 以及必要 backend mount signature 文件。该扩展只用于切断 backend 对 per-mount attrs 的观察，不打开 sb-wide remount reconfigure。

**下一步：** 阶段 2 仍只能先启动只读 `phase2-mounttree-explorer`；阶段 3 worker 必须按更新后的 write set 删除 `MountFlags`，不得把该迁移桥延续到 RFC closeout。
