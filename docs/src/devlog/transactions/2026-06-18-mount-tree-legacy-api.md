# 2026-06-18 - Mount Tree Legacy API

**状态：** Completed; accepted limitations tracked in register
**负责人：** doruche, Codex
**领域：** fs / VFS / mount / LTP
**权威计划：** [RFC-20260604-mount-tree-legacy-api](../../rfcs/mount-tree-legacy-api/index.md), [不变量需求](../../rfcs/mount-tree-legacy-api/invariants.md), [迁移实施计划](../../rfcs/mount-tree-legacy-api/implementation.md)
**当前阶段：** 阶段 7 LTP profile 收口和限制矩阵复核已完成；后续按 register/current limitations 分拆 follow-up gate

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

### 2026-06-19 - 阶段 2 启动与只读 explorer

**阶段：** 阶段 2 - `MountTree` owner、transaction lock 和 stack 语义。

**前置核验：**

- 当前分支为 `dev/drc/mount-legacy`。
- 阶段 1 已由提交 `5458c20` 关闭，后续 `38fe10b` 已记录 `MountFlags` 迁移桥反馈。
- 本轮启动前工作树干净；顶部另有用户/外部提交 `a2340ec fix: user preemption`，本阶段在其当前状态上继续，不回退该提交。
- `tracking-issues.md` 当前无 active design blocker；Gate P1/P2 仍归阶段 6，不阻塞阶段 2。

**只读 explorer：**

- 已按阶段 1 closeout 要求启动 `phase2-mounttree-explorer`（Poincare），任务仅限只读盘点当前 `NameSpace`、`Mount` placement、stack lookup、KUnit 基线和 `PathRef` 调用点。
- explorer 未获准编辑文件；阶段 2 实现仍由总控在批准 write set 内推进。

**阶段 2 write set：**

- `anemone-kernel/src/fs/mod.rs`
- `anemone-kernel/src/fs/mount.rs`
- `anemone-kernel/src/fs/namei.rs`
- `anemone-kernel/src/fs/path.rs` 仅限注释或可见性需要

**本轮停止条件：**

- 需要在阶段 2 write set 之外修改源码。
- `MountTree` 单一 placement lock 无法支撑 lookup generation retry，或读侧必须长期持有全树锁。
- target/source revalidation 需要跨锁保存不稳定 parent 指针。
- 实现必须改变 `PathRef = Mount + Dentry` 位置模型、superblock lifetime、bind/remount/move 成功语义或阶段顺序。

**下一步：** 在阶段 2 write set 内实现 `NameSpace -> MountTree` 正名、睡眠式 placement transaction lock、root/attached/detached placement state、topmost-visible stack、attach revalidation helper 和 namei generation retry；随后运行 `just fmt kernel`、`just build`、`git diff --check`，并等待/整合 explorer 与 review gate 结论。

### 2026-06-19 - 阶段 2 `MountTree` owner / stack closeout

**阶段：** 阶段 2 - `MountTree` owner、transaction lock 和 stack 语义关闭。

**源码变更：**

- `NameSpace` 已正名并收束为 `MountTree`。全局 VFS 仍保留 visible / anonymous 两棵树，但不再暴露 `RwLock<NameSpace>`；读侧 root、mount list、top child 和 generation snapshot 只短暂持有 inner spin lock。
- 普通 topology writer 使用 `tx_lock: Mutex<()>` 串行化，并在 `fs.mount()`、attach revalidation、detach、`remove_sb()` / `kill_sb()` 期间保持同一 writer gate。早期 anonymous root 初始化因 fs initcall 发生在 `Mutex::lock()` 合法条件之前，使用带注释的 spin-only first-root publish path；该路径只允许 root mount，且后续 ordinary writer 必须走 `tx_lock`。
- `Mount` 新增 `MountPlacement::{Root, Attached, Detached}`，并把 parent / mountpoint / children 改为 `MountTree` 写侧维护的 placement cache。普通调用方只能通过 `MountTree` 修改 topology；`top_child_at()` 反向扫描 children，提供 topmost-visible stack 语义并清理 stale weak refs。
- attach 先在 `fs.mount()` 前检查 target 当前可见，随后在 writer transaction 中再次 revalidate；被覆盖、detached 或 stale `PathRef` 作为 mount target 稳定返回 `EBUSY`。
- unmount 先在 writer transaction 内建立 plan，再做 last-view inode 检查和 eviction，最终在同一 `tx_lock` 下 detach 并执行 superblock list removal / `kill_sb()`，避免 last-view detach 后被并发 `sget()` 复用。
- `namei` 的 mount-follow 入口切到 `mount_stack_top_at()`，component resolution 使用 `(visible_generation, anonymous_generation)` token 做 retry，避免单个 XOR 标量抵消 placement change。
- detached mount root 的 `..` 不再 fallback 到当前 global root，避免旧 `PathRef` 成为第二个 topology lookup source。
- rename 的 mountpoint busy 检查改用 topmost-visible helper。
- KUnit 增加 root unmount reject、covered direct `PathRef` attach reject、path-level same-mountpoint topmost stacking、attach / detach generation bump；既有 direct stack KUnit 改为验证 topmost-visible unwind。

**review gate：**

- `phase2-mounttree-explorer`（Poincare，只读）确认旧 `NameSpace` 只是内部 mount tree container，当前 stack lookup 为 first-mounted-visible，且 sleeping lock 用在 boot/root path 可能触发停止条件。
- `phase2-reviewer`（Kepler，只读）提出 2 个 Apollyon 和 1 个 Keter：
  - Apollyon：全树 sleeping `Mutex` 被 anonymous root fs initcall 使用，会在 IRQ/preemption 未满足 `Mutex::lock()` 条件时 panic。处置：读侧 / root snapshot 改为 inner spin lock，普通 writer 保留 sleeping `tx_lock`，早期 first-root publish 单独注释并限制为 spin-only path。状态：Neutralized。
  - Apollyon：last-view unmount detach 后再 `kill_sb()` 可能被并发 `fs.mount()` / `sget()` 复用同一 superblock。处置：普通 mount 在 `tx_lock` 下执行 `fs.mount()`，unmount 持同一 `tx_lock` 穿过 detach、`remove_sb()` 和 `kill_sb()`。状态：Neutralized。
  - Keter：`mount_placement_generation()` 用 XOR 合并两棵树 generation，两个 bump 可能抵消。处置：公开 helper 返回 `(visible, anonymous)` tuple，namei 比较完整 token。状态：Neutralized。
- reviewer 同时确认阶段 2 源码 write set 未越界：只修改 `fs/mod.rs`、`fs/mount.rs`、`fs/namei.rs`；`path.rs` 未改。事务日志更新属于本阶段 bookkeeping。

**验证：**

- `just fmt kernel`：通过。
- `just build`：通过；仍仅保留既有 `anemone-kernel/src/sync/mono.rs` 的 `AtomicBool` / `Ordering` unused import warning。
- `git diff --check`：通过。
- `mdbook build docs`：通过，输出到 `docs/book`。
- source audit `rg -n "NameSpace|RwLock<NameSpace>|\\.child_at\\(|add_child\\(|has_children\\(|ignoring unsupported.*mount|\\[NYI\\].*mount" anemone-kernel/src/fs anemone-kernel/src/main.rs`：无输出。
- source audit `rg -n "inner\\.lock\\(|Mutex cannot|mount_placement_generation|MountTree|MountPlacement|tx_lock" anemone-kernel/src/fs/mod.rs anemone-kernel/src/fs/mount.rs anemone-kernel/src/fs/namei.rs`：仅命中预期的新 `MountTree`、placement、generation 和 `tx_lock` 代码路径；未发现旧全树 `Mutex<MountTreeInner>` 或 `Mutex cannot` panic 字符串。
- 首次 rv64 e2e 尝试在旧全树 `Mutex` 形状下触发 KUnit 后 runtime panic：`Mutex cannot be locked when preemption is disabled` / `Mutex cannot be locked when interrupts are disabled`。该形状已按 reviewer finding 修复。
- 修复后运行 `./scripts/run-user-test-rv64.sh rootfsconfig-rv etc/sdcard-rv.img build/mount-legacy-phase2-rv64-final.log`：kernel 启动、anonymous root 初始化、runtime KUnit runner 通过 `Running 77 tests...` / `All tests passed!`，新增 mount KUnit 均通过；随后进入 LTP glibc profile。用户在 `execveat03` 附近手动关闭 QEMU，因此本条只作为 rv64 boot + KUnit smoke，不作为完整 LTP closeout。

**未运行 / 残余风险：**

- 未完成完整 rv64 LTP profile；未运行 la64 smoke。
- 未新增真实并发 churn probe；阶段 2 通过 writer gate 和 generation retry 固化顺序点，但并发压力验证留给后续更高风险 gate。

**阶段 2 关闭判断：**

- `MountTree` 已成为 topology 写侧 owner，`Mount` placement 不再由普通 VFS call site 直接发布。
- same-mountpoint stack 改为 topmost-visible，并由 KUnit 固定 attach、lookup 和 unwind 行为。
- attach target revalidation、lookup generation retry、root / attached / detached placement state 已落代码。
- review gate blocker 已全部 Neutralized，验证 floor 已满足阶段 2 closeout；阶段 3 尚未启动。

### 2026-06-19 - 阶段 3 前 `fs/mount/` split-only checkpoint

**阶段：** 阶段 2 关闭后的结构维护；阶段 3 remount attrs 尚未启动。

**反馈：**

- 用户指出当前 `MountTree` owner 逻辑挤在 `fs/mod.rs`，继续把阶段 3 remount attrs 和后续 bind / move / unmount 逻辑堆进去会让 `fs/mod.rs` 同时承担 VFS facade、mount tree owner、VFS ops 和测试职责。
- 用户批准按 split-only checkpoint 拆出 `fs/mount/` 模块，并补充测试不单独建 `tests.rs`；某个内容的 KUnit 应留在该内容所在文件的 `kunits` 模块下。

**结构维护边界：**

- 本 checkpoint 只做同一 mount owner 内的目录化拆分，不改变 public VFS facade、syscall ABI、mount lookup 语义、superblock lifetime、阶段顺序或阶段 3 remount 成功边界。
- 后续阶段 write set 已在 `implementation.md` 中从 `anemone-kernel/src/fs/mount.rs` 更新为 `anemone-kernel/src/fs/mount/**`。

**源码变更：**

- `anemone-kernel/src/fs/mount.rs` 拆为 `fs/mount/{mod.rs,data.rs,flags.rs,view.rs,tree.rs}`。
- `MountData` 和 legacy data KUnit 移入 `data.rs`。
- `MountAttrFlags` / `MountFlags` 迁移桥移入 `flags.rs`；阶段 3 仍负责删除 `MountFlags`。
- `Mount` / `MountPlacement` / `MountSource` 移入 `view.rs`。
- `MountTree` / `MountTreeInner`、attach / detach / unmount transaction 和 mount-stack KUnit 移入 `tree.rs`。
- `fs/mod.rs` 删除内联 `namespace` 模块，只保留 VFS singleton / facade、VFS ops 和非 mount-tree KUnit。

**验证：**

- `just fmt kernel`：通过。
- `just build`：通过；仍仅保留既有 `anemone-kernel/src/sync/mono.rs` 的 `AtomicBool` / `Ordering` unused import warning。
- `git diff --check`：通过。
- `mdbook build docs`：通过，输出到 `docs/book`。
- source audit `rg -n "mod namespace|namespace::MountTree|fs::namespace|NameSpace|RwLock<NameSpace>" anemone-kernel/src/fs anemone-kernel/src/main.rs`：无输出。
- source audit `rg -n "tests\\.rs|mod tests|pub mod tests" anemone-kernel/src/fs/mount`：无输出。
- source audit `rg -n "MountAttrFlags|MountFlags|MountTree|MountPlacement|MountData" anemone-kernel/src/fs/mount anemone-kernel/src/fs/mod.rs anemone-kernel/src/fs/api/mount anemone-kernel/src/fs/filesystem.rs`：命中均为预期的 mount module split、syscall parser 使用和 filesystem mount signature。

**未运行验证：** 本 checkpoint 未重新启动 QEMU / LTP；阶段 2 closeout 已记录用户中止前的 rv64 boot + runtime KUnit smoke。

### 2026-06-19 - 阶段 2 early-root API 反馈回写

**阶段：** 阶段 2 closeout 后的 implementation feedback；阶段 3 尚未启动。

**反馈：**

- 阶段 2 closeout 为绕过 anonymous fs initcall 早于 `Mutex::lock()` 合法窗口的问题，引入了 `mount_early_root` spin-only first-root publish path。
- 用户指出旧实现用 `can_sleep` / IRQ / preempt 状态推断是否进入 early-root path，看似复用，实则会把 panic、hwirq 或任意 no-sleep context 误导成 mount-tree writer bypass。

**源码更正：**

- 删除 `MountTree::tx_lock_can_sleep()` 和 ordinary root mount 的自动分流。`MountTree::mount_root()` 现在始终走普通 `tx_lock` transaction。
- early publish 收窄为 `MountTree::mount_early_pseudo_root()`，只固定发布 pseudo root、空 attrs 和空 `MountData`。
- VFS facade 只保留 `pub(in crate::fs) mount_early_anonymous_root()`，并由 anonymous fs initcall 唯一调用；该能力不再通过 crate-wide prelude 暴露给 syscall、panic 或其它子系统。
- 代码注释明确：early-root 是 anonymous root boot-only capability，不是 arbitrary no-sleep 或 panic context fallback。

**RFC 回写：**

- `index.md`、`invariants.md` 和 `implementation.md` 已把阶段 2 canonical 形状修正为普通 writer 使用 `tx_lock: Mutex<()>`，placement state 由 `inner: SpinLock<MountTreeInner>` 短临界区发布。
- `invariants.md` 新增禁止退化项：不得用 `can_sleep`、IRQ/preempt 状态或 panic 状态推断来绕过 `MountTree` writer gate；early-root publish 必须是显式、fs-private、boot-only capability。

**验证：**

- `just fmt kernel`：通过。
- `just build`：通过；仍仅保留既有 `anemone-kernel/src/sync/mono.rs` 的 `AtomicBool` / `Ordering` unused import warning。
- `git diff --check`：通过。
- `mdbook build docs`：通过，输出到 `docs/book`。
- source audit `rg -n 'tx_lock_can_sleep|mount_anonymous_root|mount_early_root\(' anemone-kernel/src -S`：无输出。
- source audit `rg -n 'mount_early_pseudo_root|mount_early_anonymous_root' anemone-kernel/src -S`：仅命中 `MountTree` fs-private early pseudo-root API、VFS fs-private anonymous wrapper 和 anonymous fs initcall 调用点。
- `just fmt kernel --check`：未通过，失败只来自已知生成文件 `anemone-kernel/src/kconfig_defs.rs` 和 `anemone-kernel/src/platform_defs.rs` 的格式差异；本次编辑文件已由 `just fmt kernel` 格式化。

### 2026-06-19 - 阶段 3 ordinary per-mount readonly remount closeout

**阶段：** 阶段 3 - ordinary per-mount readonly remount 和 attr plumbing。

**源码变更：**

- 删除源码中的 `MountFlags` 类型和 `MountAttrFlags -> MountFlags` 迁移桥；`Mount` 现在用 `AtomicU32` 承载 `MountAttrFlags` bitset，`attrs()` 以 acquire-load 读取，`set_attrs()` 以 release-store 发布。
- `Mount::ensure_writable()` 只读取当前 `PathRef.mount()` 上的 attrs；opened file description / `FileStatusFlags` / `FileOpStatusFlags` 不承载 mount readonly。
- `FileSystemOps::mount()` / `FileSystem::mount()` / `ramfs`、`devfs`、`procfs`、`ext4`、anonymous fs 后端 mount vtable 不再接收 per-mount attrs，只接收 `MountSource` 和 legacy `MountData`。
- `sys_mount()` 将 raw flags 分成 `NewMount` 和 ordinary `Remount`。普通 `MS_REMOUNT` 允许 `MS_RDONLY` 切换当前 live mount view 的 attrs；`MS_REMOUNT | MS_BIND`、其它 operation bits、unsupported attrs 和 unknown bits 继续稳定返回 `EINVAL`。
- ordinary remount 路径拒绝非空 legacy `data`，不打开 filesystem instance reconfigure，也不声明 sb-wide readonly。
- `MountTree::remount_attrs()` 在 writer transaction 内重验目标仍是当前 tree 中的 live mount root，且没有被更上层 mount 覆盖；重验失败不更新 attrs。
- KUnit 增加 ordinary remount ro/rw 语义覆盖：已打开 fd 在 remount ro 后写入返回 `EROFS`，目录项创建也返回 `EROFS`，remount rw 后旧 fd 可再次写入；普通目录 remount 被拒绝。

**review gate：**

- `phase3-mount-readonly-reviewer`（Bohr，只读）未发现 kernel 阶段 3 实现 blocker。
- reviewer 确认 `MountFlags` 已从源码类型删除，filesystem backend 不再接收 attrs，ordinary remount 只发布当前 mount-view attrs，`MS_REMOUNT | MS_BIND` 与非空 remount data 稳定拒绝，remount target revalidation 在 `MountTree` writer / inner transaction 下执行。
- reviewer 提出一个 Keter：`anemone-apps/user-test/ltp/profile.txt` 当前从 `all` 改成 `fs`，在阶段 3 write set 外。处置：该文件视为本轮临时本地验证状态，不纳入阶段 3 implementation commit。
- 残余测试缺口：未新增 detached / replaced old target view 的 targeted KUnit；未对所有 write-entry class 建立逐项 runtime matrix；mmap / writeback readonly coherence 仍按 RFC accepted limitation 处理。

**验证：**

- `just fmt kernel`：通过。
- `just build`：通过；仍仅保留既有 `anemone-kernel/src/sync/mono.rs` 的 `AtomicBool` / `Ordering` unused import warning。
- `git diff --check`：通过。
- source audit `rg -n "MountFlags" anemone-kernel/src anemone-abi/src`：无输出，证明旧迁移桥不再作为源码类型存在。
- source audit `rg -n "FileSystemOps.*MountAttr|fn\(MountSource, MountAttr|_flags: MountAttr|mount: \|source, flags|fs\.mount\([^\n]*attrs" anemone-kernel/src anemone-abi/src`：无 backend attrs leakage 命中。
- source audit `rg -n "ensure_writable|ReadOnlyFs|EROFS|truncate\(|fallocate|write_at|append_at_current_end|O_TRUNC" anemone-kernel/src/fs anemone-kernel/src/task/files.rs`：确认普通 write / pwrite / append、`truncate` / `ftruncate`、`fallocate` grow、`open(O_TRUNC)`、目录项修改、metadata 修改和 copy-backed `splice(pipe -> file)` 仍经过当前 mount readonly gate 或 FileDesc write gate。
- 首次 non-tty rv64 e2e 尝试在 rootfs `sudo virt-make-fs` 处失败，未进入 QEMU；随后使用 PTY 运行 `./scripts/run-user-test-rv64.sh rootfsconfig-rv etc/sdcard-rv.img build/mount-legacy-phase3-rv64.log`。
- rv64 runtime KUnit：`Running 80 tests...` / `All tests passed!`。新增 `test_mount_flags_accept_plain_remount`、`test_vfs_remount_rejects_plain_directory_target`、`test_vfs_remount_readonly_rechecks_existing_file_writes` 均通过。
- rv64 `fs` LTP profile 在当前本地 `profile.txt=fs` 下完整跑完：`attempted=46 passed=25 failed=21 infra_failed=0 skipped=0`。失败主要落在既有 tmpfs non-empty data / realpath / mknod / group lookup / path errno 等非阶段 3 主语义；本条只作为 boot + KUnit + broad fs smoke，不作为阶段 3 LTP closeout。

**阶段 3 关闭判断：**

- ordinary per-mount readonly remount 语义已可验证；`MS_REMOUNT | MS_BIND` 仍留给阶段 4 bind view 语义打开。
- `MountFlags` 不再作为源码类型存在；new mount、remount 和写入口均使用 `MountAttrFlags` / `Mount` atomic attrs 单一真相源。
- filesystem backend 不再接收或保存 per-mount attrs；真实 sb-wide reconfigure 仍按 future `SuperBlockState` / follow-up gate 处理。
- remount 成功返回前完成 target revalidation 和 attrs 发布；旧 fd 对同一 live mount view 的后续写能观察 remount 后 attrs。
- shared writable mmap / dirty writeback 未声明闭合，继续保留 RFC accepted limitation。

### 2026-06-19 - 阶段 4 启动与 write set 扩展

**阶段：** 阶段 4 - plain bind 和 recursive bind。

**只读 explorer：**

- `phase4-bind-explorer`（Averroes，只读）确认当前源码尚无 `MS_BIND` / `MS_REC | MS_BIND` / `MS_REMOUNT | MS_BIND` 成功语义；parser 仍把这些 operation bits 稳定拒绝。
- explorer 建议阶段 4 最小落点为 `fs/api/mount/**` parser 分流、`fs/mod.rs` 窄 facade、`fs/mount/**` bind/rbind clone owner、`fs/namei.rs` mount-root boundary；并指出 `PathRef::to_pathbuf()` 若不使用 bind root boundary，会让 `getcwd`、`/proc/<pid>/cwd`、`/proc/<pid>/fd` 等用户可见路径带出 source 原路径前缀。

**write set 扩展：**

- 用户批准把 `anemone-kernel/src/fs/path.rs` 纳入阶段 4 write set。
- 该扩展仅用于让 `PathRef::to_pathbuf()` 使用与 bind root 相同的 mount-root boundary；不得改变 `PathRef = Mount + Dentry` identity、task root/cwd owner、namei ownership 或公开 path API。
- `implementation.md` 阶段 4 write set 已同步记录该扩展。

**当前未提交外部改动：**

- `anemone-apps/user-test/ltp/profile.txt` 仍为本地验证 profile 改动，不纳入阶段 4 implementation commit。
- `anemone-kernel/src/sync/mono.rs` 已有与阶段 4 无关的 import 条件编译改动，不纳入阶段 4 implementation commit，除非后续用户明确要求。

**下一步：** 在批准 write set 内实现目录 bind、recursive bind、bind-remount readonly 和 bind root boundary；随后启动只读 review gate。

### 2026-06-19 - 阶段 4 plain / recursive bind closeout

**阶段：** 阶段 4 - plain bind、recursive bind 和 bind-remount readonly。

**源码变更：**

- `sys_mount()` 现在接受 `MS_BIND`、`MS_BIND | MS_REC` 和 `MS_REMOUNT | MS_BIND | MS_RDONLY`，仍稳定拒绝 `MS_BIND | MS_MOVE`、`MS_REMOUNT | MS_BIND | MS_REC` 等未闭合组合。
- bind syscall adapter 拒绝非空 legacy data、null source 和 file source / target；file bind 拒绝日志显式包含 `errno=ENOTDIR`，不声明 first-pass file bind 支持。
- `MountTree::bind_mount()` 在 writer transaction 内重验 source / target，plain bind 创建新的 mount view 并共享 source dentry / superblock，recursive bind 在发布前准备 subtree clone 并批量 attach，避免部分 visible mount view。
- bind-remount 复用 per-mount attrs 路径，只更新目标 bind view；source / sibling mount 不被污染。
- `namei` 和 `PathRef::to_pathbuf()` 使用 mount-root boundary 处理 bind root，避免 `..` 或路径渲染越过 target boundary 泄漏 source parent。

**review gate：**

- `phase4-bind-reviewer`（Ohm，只读）未发现 Apollyon；核心 bind/rbind/remount/fanotify/namei/path 语义无 blocker。
- reviewer 提出的 Keter 是当前工作树仍包含阶段 4 write set 外的 `anemone-apps/user-test/ltp/profile.txt` 和 `anemone-kernel/src/sync/mono.rs`。处置：这两处继续视为外部本地改动，不纳入阶段 4 implementation commit。
- reviewer 提出的 Euclid：事务日志顶部状态仍写阶段 4 尚未启动。处置：顶部状态已更新为阶段 4 已关闭、阶段 5 尚未启动。
- reviewer 提出的 Euclid：file bind 拒绝日志缺少稳定 errno。处置：non-directory source / target 日志均加入 `errno=ENOTDIR`，返回值仍为 `SysError::NotDir`。

**验证：**

- `just fmt kernel`：通过。
- `just build`：通过。
- `git diff --check`：通过。
- `mdbook build docs`：通过，输出到 `docs/book`。
- rv64 e2e `./scripts/run-user-test-rv64.sh rootfsconfig-rv etc/sdcard-rv.img build/mount-legacy-phase4-rv64.log`：此前已完整运行；runtime KUnit `Running 87 tests...` / `All tests passed!`。新增 bind KUnit 包括 plain bind view、plain bind 不 clone child、recursive bind clone child、bind-remount readonly sibling isolation、file source / target reject、stale source / target revalidation、bind root parent / path rendering target boundary。
- rv64 `fs` LTP profile smoke：`attempted=46 passed=25 failed=21 infra_failed=0 skipped=0`；失败仍主要落在既有 tmpfs non-empty data、realpath、mknod、group lookup、path errno 等非阶段 4 主语义，本阶段不把它作为 bind/rbind 失败信号。
- source audit 确认 `MountFlags` 未重新引入，fstype alias 仍停在 syscall adapter，filesystem backend 仍只接收 `MountSource` / `MountData`，fanotify key 仍按 `Arc<Mount>` / `Arc<SuperBlock>` 区分。

**阶段 4 关闭判断：**

- 目录 plain bind、recursive bind 和 bind-remount readonly 已有闭合成功语义与 KUnit 覆盖。
- file bind、propagation-dependent bind 和未闭合 operation 组合仍稳定拒绝并可观测。
- recursive bind 失败路径不会留下部分 visible mount view。
- 阶段 5 private move mount / limited propagation 尚未启动。

### 2026-06-19 - mount-legacy LTP group 整理

**阶段：** 阶段 7 验证入口预整理；不启动阶段 5 feature code。

**变更：**

- 新增 `anemone-apps/user-test/ltp/groups/mount-legacy.txt`，作为本 RFC 的专用 LTP group。启用集合覆盖 `mount01..07`、`umount01..03`、`umount2_01..02`、`fs_bind` 中不依赖真实 mount namespace 的 bind / move / rbind / regression 条目，以及 `fs_readonly` 的 `test_robind01..55`。
- `mountns01..04`、`fs_bind_cloneNS01..07`、new mount API (`fsopen` / `fsmount` / `fspick` / `open_tree` / `move_mount` / `mount_setattr`) 和 `pivot_root01` 作为注释锚点保留，不纳入当前可运行 group，避免把 RFC 非目标误分类为 legacy mount regression。
- `anemone-apps/user-test/src/ltp.rs` 注册 `mount-legacy` group，并给 case argument parser 增加最小 quote-aware 支撑：`fs_readonly` 的 `test_robind.sh -c "..."` 命令字符串会作为单个 argv 传入。该 parser 不做 shell 展开、不支持转义或混合拼接，避免把 runner 扩展成半个 shell。
- `profile.txt` 未修改；后续运行该 group 时仍由本地 profile 或用户临时切换选择。

**RFC 回写：**

- `implementation.md` 阶段 7 write set 已补充 `mount-legacy.txt` 和必要的 runner group 注册 / case-argument parsing 支撑。该扩展只服务验证入口，不改变阶段 5/6 feature write set 或 accepted semantics。

**验证：**

- `just fmt user-test`：通过；命令曾格式化 `src/main.rs` 的无关 import，已还原该 churn。
- `just xtask app build user-test --arch riscv64`：通过。
- `git diff --check`：通过。
- `mdbook build docs`：通过。
- 静态 group 检查：`mount-legacy.txt` 启用 155 条 case，保留 38 条注释 follow-up；未发现未注释的 cloneNS、mountns、new mount API、`pivot_root` 条目；启用行双引号成对。
- 未运行 QEMU / LTP；本条只整理 profile group 和 runner argv 支撑。

### 2026-06-19 - 阶段 5 private move / limited propagation implementation checkpoint

**阶段：** 阶段 5 - private move mount 和有限 propagation；未关闭。

**源码变更：**

- `sys_mount()` 接受 `MS_MOVE`、`MS_PRIVATE` 和 `MS_REC | MS_PRIVATE`。`MS_MOVE` 拒绝非空 legacy data、null source、非目录 source / target，以及与 attrs、bind、remount、recursive 或 propagation bit 的组合。
- `MS_PRIVATE` / `MS_REC | MS_PRIVATE` 在当前 private-only tree 下作为 already-private no-op 接受，但仍通过 `MountTree` writer gate 验证 target 是当前 live path；非空 data 稳定拒绝。
- `MS_SHARED`、`MS_SLAVE`、`MS_UNBINDABLE` 及其 `MS_REC` / mixed propagation 组合稳定返回 `EINVAL`，日志标明缺少 peer-group / master-slave support。
- `MountTree::move_mount()` 在同一个 `tx_lock` 和 inner placement transaction 内重验 source 是当前 live mount root、target 是当前 live path、防止 target 落在 source subtree 内，随后从旧 parent stack 移除同一个 `Arc<Mount>` 并插入新 target stack，最后 bump `placement_generation`。
- move 保留同一个 `Arc<Mount>` identity、per-mount attrs 和 child subtree；source 非 mount-root 返回 `EINVAL`，stale / covered source 或 target 返回 `EBUSY`。

**review gate：**

- `phase5-move-explorer`（Archimedes，只读）确认阶段 5 最小实现落点仍在 `fs/api/mount/**`、`fs/mount/**`、`fs/mod.rs`；建议补充 private no-op 的 `MountTree` validation、拆分 source non-root errno，以及增强 propagation reject 矩阵。处置：三项均已折回当前实现 / KUnit。
- `phase5-move-reviewer`（Dalton，只读）确认核心 move transaction、identity / attrs / subtree 保留、limited private propagation 和 write set 边界基本满足阶段 5。
- reviewer 提出一个 Keter：阶段 5 验证要求 KUnit 覆盖 “move 与 lookup generation retry 边界”，证明 successful lookup 只能返回 move 前或 move 后位置。当前测试只证明 move bump generation 和 move 后 lookup 正常；要制造 lookup 起止 generation 之间发生 move 的场景，需要在 `anemone-kernel/src/fs/namei.rs` 加 KUnit-only hook 或等价测试 seam，但阶段 5 write set 当前不包含 `fs/namei.rs`。
- reviewer 提出一个 Euclid：move log 在 move 后格式化旧 `source`，`PathRef::to_pathbuf()` 会按新 placement 渲染，可能让日志缺少 old target。处置：`MountTree::move_mount()` 现在在线性化前捕获 old / new target 文本，成功日志包含 `old_target`、`new_target`、moved mount identity、subtree size 和 stack depth。状态：Neutralized。

**验证：**

- `just fmt kernel`：通过。
- `just build`：通过；KUnit 编译进 kernel build。
- `git diff --check`：通过。
- source audit `rg -n "MountFlags" anemone-kernel/src anemone-abi/src`：无输出，旧迁移桥未复活。
- source audit `rg -n "MS_MOVE|MS_PRIVATE|MS_SHARED|MS_SLAVE|MS_UNBINDABLE|move_mount|mount move|mount propagation" anemone-kernel/src/fs anemone-abi/src/fs.rs`：命中仅在 ABI 常量、mount syscall adapter、VFS facade、mount tree owner、KUnit 和注释；未进入 filesystem backend。
- source audit `rg -n "tmpfs|ext2|ext3|vfat|mount_fs_name|FsAliasKind|normalize_fstype|ltp-temporary-bridge" anemone-kernel/src/fs/api/mount anemone-kernel/src/fs`：alias 表仍只在 syscall adapter / KUnit。
- source audit `rg -n "FileSystemOps.*MountAttr|fn\\(MountSource, MountAttr|_flags: MountAttr|fs\\.mount\\([^\\n]*attrs|MS_MOVE|MS_PRIVATE|MS_SHARED|MS_SLAVE|MS_UNBINDABLE" ...backend paths...`：无输出，backend 未观察 per-mount attrs 或 move/private operation flags。

**停止边界：**

- 阶段 5 closeout 停在 review gate Keter：需要用户批准把 `anemone-kernel/src/fs/namei.rs` 纳入阶段 5 write set，仅用于 `#[cfg(feature = "kunit")]` 的 forced generation-retry test hook；或者明确接受以 source audit 替代该 KUnit 项。
- 在该决定前，不提交阶段 5 commit，不宣称 private move / limited propagation 阶段关闭。

### 2026-06-19 - 阶段 5 review gate closeout

**阶段：** 阶段 5 - private move mount 和有限 propagation 关闭。

**write set 扩展：**

- 用户批准将 `anemone-kernel/src/fs/namei.rs` 纳入阶段 5 write set，仅用于 `#[cfg(feature = "kunit")]` forced generation-retry hook / test seam。
- `implementation.md` 已同步记录该扩展边界：不得改变普通 namei 解析语义、mount-root crossing、task root/cwd owner 或 P2 detached-path accepted boundary。

**review finding 处置：**

- `phase5-move-reviewer` 的 Keter 要求证明 move 与 lookup generation retry 边界。处置：`namei.rs` 新增 KUnit-only `resolve_with_mount_retry_hook_for_kunit()`，在第一次 path walk 完成后、generation 比较前执行一次测试 hook；普通 `resolve*()` 路径只委托到同一 retry helper，不安装 hook。
- `test_vfs_move_mount_lookup_generation_retry_returns_new_state` 使用该 hook 在 `/kunit-vfs-move-retry-target/file` 首次 lookup 返回前移动 source mount。第一次 path walk 的结果会因 generation 变化被丢弃，retry 后返回 moved mount 的新位置，并确认旧 mountpoint 下同一文件不可见。
- move log Euclid 已在上一条 checkpoint 中 Neutralized：成功日志在线性化前捕获 old / new target 文本。
- runtime KUnit 首次暴露新增测试 cleanup 仍持有 `PathRef` 导致 unmount busy；处置为在新增 move/private KUnit 的 unlink / unmount 前显式 drop 相关 `PathRef`、mount identity 和 file refs，保持测试遵守当前 busy 语义。

**验证：**

- `just fmt kernel`：通过。
- `just build`：通过；KUnit 编译进 kernel build。
- `git diff --check`：通过。
- `mdbook build docs`：通过，输出到 `docs/book`。
- rv64 runtime KUnit：`./scripts/run-user-test-rv64.sh rootfsconfig-rv etc/sdcard-rv.img build/mount-legacy-phase5-rv64-kunit.log` 启动 QEMU 后运行 `Running 92 tests...` / `All tests passed!`。阶段 5 新增 `test_vfs_private_propagation_validates_live_target` 和 `test_vfs_move_mount_lookup_generation_retry_returns_new_state` 均通过。随后进入 LTP，用户按本轮验证边界关闭 QEMU；本条不作为 LTP closeout。
- source audit `rg -n "resolve_with_mount_retry_hook_for_kunit|after_first_walk" anemone-kernel/src/fs/namei.rs anemone-kernel/src/fs/mount/tree.rs`：只命中 KUnit-only hook、内部 one-shot hook 参数和对应阶段 5 KUnit。
- source audit `rg -n "MountFlags" anemone-kernel/src anemone-abi/src`：无输出。
- source audit `rg -n "MS_MOVE|MS_PRIVATE|MS_SHARED|MS_SLAVE|MS_UNBINDABLE|move_mount|mount move|mount propagation" anemone-kernel/src/fs anemone-abi/src/fs.rs`：命中 ABI、mount syscall adapter、VFS facade、mount tree owner、KUnit 和注释；未进入 filesystem backend。
- source audit `rg -n "tmpfs|ext2|ext3|vfat|mount_fs_name|FsAliasKind|normalize_fstype|ltp-temporary-bridge" anemone-kernel/src/fs/api/mount anemone-kernel/src/fs`：alias 表仍只在 syscall adapter / KUnit。
- source audit backend attrs / operation leakage pattern：无输出，backend 未观察 per-mount attrs 或 move/private operation flags。

**阶段 5 关闭判断：**

- private tree 下基础 `MS_MOVE` 可用，保留同一 `Arc<Mount>` identity、attrs 和 subtree，并在同一 transaction 中完成旧 stack 摘除、新 stack 插入和 generation bump。
- `MS_PRIVATE` / `MS_REC | MS_PRIVATE` 作为当前 private-only tree 的有限 no-op 成功语义闭合；target 仍通过 `MountTree` validation，非空 data 拒绝。
- `MS_SHARED`、`MS_SLAVE`、`MS_UNBINDABLE`、file/source 非目录、非法 move / bind / remount / attrs / propagation 组合均稳定拒绝，不伪成功。
- review gate 已关闭；阶段 6 尚未启动。

### 2026-06-19 - 阶段 6 umount2 flags、lazy detach 和 mounts 视图 closeout

**阶段：** 阶段 6 - `umount2` flags、pre-unmount cleanup 和 mounts 视图。

**源码变更：**

- `sys_umount2()` 将 raw flags 解析为 `{lazy, nofollow}`：`flags == 0` 走同步 unmount，`MNT_DETACH` 走 topology-only lazy detach，`UMOUNT_NOFOLLOW` 映射到 `ResolveFlags::UNFOLLOW_LAST_SYMLINK`。
- `MNT_FORCE` 继续稳定拒绝并记录 `force-unmount-not-supported`；`MNT_EXPIRE | MNT_FORCE`、`MNT_EXPIRE | MNT_DETACH` 继续返回 `EINVAL`，`MNT_EXPIRE` 自身作为 deferred protocol 稳定拒绝并记录 `expire-deferred`。
- `MountTree::lazy_unmount()` 在 writer transaction 中摘除目标 mount subtree，允许子 mount 随 subtree 一起从 visible tree 消失；旧 `PathRef` / fd 只保持对象 lifetime，不触发 final superblock cleanup。
- VFS facade 增加 `lazy_unmount()` 和 `visible_mounts_snapshot()`；procfs 只消费 snapshot，不获得 `MountTreeInner` 或 topology 修改权。
- `/proc/<tgid>/mounts` 从空 stub 改为 live 六列视图：`source target fstype options 0 0`。`/proc/mounts` 仍保持 `self/mounts` symlink。
- mounts view 按绑定 task root 渲染 target；当前 tgid 读自身时使用当前 task，外部 tgid 使用绑定 leader。无法在该 root 下表达的 mountpoint 会跳过并打日志，不 fallback 泄露全局路径。kthread binding 返回空内容，避免对 hanging fs state 调用 `Task::root()`。

**Gate P1 / P2 结果：**

- P1 只关闭 lazy detach 的 topology detach；final superblock cleanup owner/reaper、fanotify mount/filesystem mark-dead hook 和 `MNT_EXPIRE` 两阶段协议未闭合，已登记为 [ANE-20260619-MOUNT-UNMOUNT-CLEANUP-STAGE1](../../register/current-limitations.md#ane-20260619-mount-unmount-cleanup-stage1)。
- 同步 unmount 仍保留阶段 2 的实现形状：为了避免 last-view detach 后并发 `sget()` 复用同一 superblock，`try_evict_all()`、`remove_sb()` 和 `kill_sb()` 仍在 `MountTree` writer gate 内执行。本阶段不把该路径宣称为长期 P1 cleanup owner。
- P2 通过新增 KUnit 覆盖：lazy-detached child mount root 的 `..` 保持在 detached root，不 fallback 到全局 root 或 stale parent；旧 `PathRef` 仍可访问 detached object lifetime 内的文件。

**KUnit / 行为覆盖：**

- umount parser KUnit 更新为接受 `MNT_DETACH`、`UMOUNT_NOFOLLOW` 和二者组合，继续拒绝 unknown、`MNT_FORCE` 与 `MNT_EXPIRE`。
- VFS KUnit 增加 lazy detach subtree：新 lookup 看不到 detached subtree，旧 refs 仍可访问，generation bump 生效。
- VFS KUnit 增加 final symlink nofollow：带 `UNFOLLOW_LAST_SYMLINK` 的 unmount 目标停在 symlink 自身并返回 `NotMounted`，普通路径仍可跟随 symlink 卸载 mountpoint。

**验证：**

- `just fmt kernel`：通过。
- `just build`：通过；KUnit 编译进 kernel build。
- 用户运行 KUnit：`All tests passed`。本条为用户回报的 runtime KUnit 结果，当前事务日志未记录独立日志路径。

**未完成验证：**

- 尚未运行 LTP mount profile；阶段 7 负责 profile 收口和结果分类。

### 2026-06-19 - 阶段 7 LTP profile closeout 和限制矩阵

**阶段：** 阶段 7 - LTP profile 收口和限制登记。

**profile 策略：**

- 用户明确要求保留当前 `mount-legacy` group 的宽覆盖：许多 whole-case FAIL 仍包含可计分 TPASS 子点，不能为了让 group 汇总更好看而删除 `fs_bind*`、`fs_bind_move*`、`fs_bind_rbind*` 或 `test_robind*`。
- `mount-legacy` group 顶部注释已改为评分 / 观测 profile 定义。new mount API、`pivot_root` 和真实 mount namespace / cloneNS cases 继续只作为注释锚点，不纳入本 legacy profile。
- `profile.txt=mount-legacy` 是本轮用户授权的本地验证 profile 状态；不把它解释成缩小比赛测例全集的长期策略。

**runtime 证据：**

- 证据来源为用户/本地阶段 6 日志 `build/mount-legacy-phase6-rv64.log`；本阶段未重新运行 QEMU / LTP。
- rv64 runtime KUnit：`Running 95 tests...` / `All tests passed!`。
- glibc `mount-legacy` profile 原始 runner 汇总：`attempted=155 passed=7 failed=148 infra_failed=0 skipped=0`。
- whole-case PASS：`mount01`、`mount05`、`mount06`、`umount01`、`umount02`、`umount03`、`fs_bind_regression_sh`。其中 `fs_bind_regression_sh` 有 19 个 TPASS，覆盖 private/unshared bind、rbind 和 move 的主语义。
- `mount03` 产生 8 个 TPASS 和 32 个 TFAIL：直接 readonly 写入 `EROFS` 和 remount 基础路径有证据，剩余失败归入 `statfs()` flag reporting 和 `MS_NODEV` / `MS_NOEXEC` / `MS_NOSUID` / atime flag matrix accepted limitation。
- `mount07` 产生 48 个 TPASS 和 12 个 TFAIL：默认 symlink 行为通过，`MS_NOSYMFOLLOW` remount 与 `ST_NOSYMFOLLOW` reporting 归入 mount flag matrix follow-up。
- `umount2_02` 通过 `MNT_EXPIRE | MNT_FORCE`、`MNT_EXPIRE | MNT_DETACH`、`UMOUNT_NOFOLLOW` symlink / mntpoint 子项；`MNT_EXPIRE` 两阶段 `EAGAIN` / second-success 协议仍归入 unmount cleanup limitation。
- `umount2_01` 在 lazy detach / remount 后 `open(mntpoint/file)` 得到 `ENOENT` 并 TBROK；该证据不足以声明 lazy detach 后 final persistence / cleanup 完整闭合，继续归入阶段 6 cleanup follow-up。

**失败分类：**

- `mount02` 的 `mknod() failed: ENOSYS` 是 mknod / devfs setup 设施缺口，不是 mount transaction 主语义失败。
- `mount04` 先证明 non-root `mount()` 返回 `EPERM`，随后在 `/proc/mounts` 非 root 读取 `EACCES` 和 testcase cleanup 中 SIGSEGV/TBROK；该结果归入 procfs permission / testcase cleanup 环境问题，不作为 mount permission 主语义回归。
- `fs_bind01..24`、`fs_bind_move01..22`、`fs_bind_rbind01..39` 保持启用用于收集 TPASS；whole-case FAIL 主要来自 `--make-rshared`、`--make-rslave`、`--make-runbindable` 和 propagation matrix 检查，归入 shared/slave/unbindable propagation limitation。
- `test_robind01..55` 当前全部 `TCONF: tests need a big block device(>=500MB)`，旧 runner 将纯 `TCONF` exit code `32` 计作 failed。`user-test` runner 已修正为纯 `32` 归入 skipped；若用同一日志条件重跑，预期会把 55 个 pure TCONF case 从 failed 移到 skipped，但这不是新的 runtime 结果。

**源码 / profile 变更：**

- `anemone-apps/user-test/src/ltp.rs` 增加 `LtpCaseOutcome::Skipped`，将纯 LTP `TCONF` 退出码 `32` 计入 `skipped`。LTP 退出码是 result bitmask，只有 pure `32` 是 skip；`TCONF | TFAIL` 或 `TCONF | TBROK` 仍按失败处理。
- `anemone-apps/user-test/ltp/groups/mount-legacy.txt` 顶部说明已明确该 group 是评分 / 观测 profile，不能因部分子功能未实现而缩窄当前启用集合。

**register / RFC closeout：**

- `ANE-20260528-ROFS-DIRECT-WRITE-STAGE1` 已更新为当前 per-mount readonly 状态：ordinary remount、bind-remount sibling isolation 和 bind/rbind/move 下 readonly 主语义已实现；shared writable mmap / writeback 与 `test_robind*` 大块设备环境仍为限制。
- 新增 `ANE-20260619-MOUNT-PROPAGATION-STAGE1` 记录 shared/slave/unbindable propagation、peer group / master-slave、unbindable subtree filtering 和 cloneNS 后续范围。
- 新增 `ANE-20260619-MOUNT-FLAG-MATRIX-STAGE1` 记录 `statfs()` mount flag reporting、`MS_NODEV`、`MS_NOEXEC`、`MS_NOSUID`、atime flags 和 `MS_NOSYMFOLLOW`。
- 新增 `ANE-20260619-MOUNT-FSTYPE-ALIAS-BRIDGE` 记录 `tmpfs` / `ext2` / `ext3` / `vfat -> ramfs` syscall-boundary LTP bridge 及退出条件。
- 阶段 6 已登记的 `ANE-20260619-MOUNT-UNMOUNT-CLEANUP-STAGE1` 继续覆盖 final superblock cleanup / retry queue、fanotify observer cleanup 和 `MNT_EXPIRE` 协议。

**阶段 7 关闭判断：**

- 第一版 legacy mount API 的已支持矩阵、未支持矩阵、环境 blocker 和 runtime 证据均已在 RFC / transaction / register 中有归属。
- `mount-legacy` group 保持宽覆盖以保留 TPASS 分数；后续优化应按 register 条目分拆 follow-up gate，而不是缩小该 group。

**验证：**

- `just fmt user-test`：通过；`main.rs` 的无关 import 排序 churn 已还原。
- `just xtask app build user-test --arch riscv64`：通过。
- `mdbook build docs`：通过，输出到 `docs/book`。
- `git diff --check`：通过。
- `just build`：通过。
- source audit `rg -n "MountFlags" anemone-kernel/src anemone-abi/src`：无输出。
- source audit `rg -n "\\[NYI\\].*mount|ignoring unsupported.*mount|unsupported.*mount" anemone-kernel/src`：无输出。
- source audit `rg -n "tmpfs|ext2|ext3|vfat|mount_fs_name|FsAliasKind|normalize_fstype|ltp-temporary-bridge" anemone-kernel/src/fs/api/mount anemone-kernel/src/fs`：仅命中 syscall adapter / KUnit 中的 alias bridge。
- `phase7-reviewer`（Galileo，只读）在用户澄清前建议缩小或拆分 profile，并要求分类矩阵 / register 一致性。处置：用户明确要求保持 broad group 以保留 TPASS 分数；phase 7 已把 group 定义改为评分 / 观测 profile，并完成分类矩阵与 register 更新。状态：Neutralized。

### 2026-06-22 - post-closeout mount placement lifecycle repair

**阶段：** post-closeout implementation feedback；不改变阶段 1-7 accepted contract。

**问题：**

- `MountPlacement::Attached` 保存 parent `Arc<Mount>` 和 mountpoint `Arc<Dentry>`。旧实现的 `mark_detached()` / `move_attached()` 在持有 `Mount.placement` spin lock 时直接替换 placement，旧 enum 的强引用会在锁内释放。
- detach / lazy detach / move 又发生在 `MountTreeInner` 的 `lock_irqsave()` 临界区内；若旧 mountpoint dentry 的最后一个强引用在这里释放，`Dentry::drop()` 会拿 parent dentry lock，形成隐蔽锁序和生命周期副作用。
- lazy detach 还会从 `MountTreeInner.mounts` 移除整棵 subtree。若被移出树的 `Arc<Mount>` 临时强引用在 inner 函数返回前释放，同样可能在 tree lock 内触发 `Mount` / root dentry destructor。

**源码变更：**

- `Mount::move_attached()` 和 `Mount::mark_detached()` 改为 `mem::replace()` 取出旧 placement，并返回 `OldPlacementRefs`；旧 parent / mountpoint 强引用不再在 `Mount.placement` 锁内 drop。
- `MountTreeInner::move_mount()` 返回 old placement refs；外层 `MountTree::move_mount()` 在释放 ordinary writer gate 后再 drop。
- `MountTreeInner::detach_mount()` 和 `lazy_detach_subtree()` 返回 `DetachedMountRefs`，把被移出 visible tree 的 mount 强引用与旧 placement refs 打包。外层 `unmount()` / `lazy_unmount()` 在释放 `tx_lock` 后再释放这些 refs。
- `OldPlacementRefs` / `DetachedMountRefs` 带 `#[must_use]` 和锁序注释，明确它们是必须带出 placement/tree locks 后释放的生命周期包。

**review gate：**

- `placement-lifecycle-reviewer`（Epicurus，只读）审查 `anemone-kernel/src/fs/mount/{view.rs,tree.rs}` 当前 diff，未发现 Apollyon / Keter / Euclid blocker。
- reviewer 确认 old placement refs 已从 `Mount.placement` 锁内外提，`move_mount()` / `unmount()` / `lazy_unmount()` 都绑定 carrier 并在 `tx_lock` 释放后显式 drop。
- reviewer 确认 `self.mounts.retain()` 仍会在 inner 锁内减少 tree-held refs，但 detached subtree 和 old placement refs 已被 carrier 保住，不应在 inner 锁内触发 final `Mount` / `Dentry` destructor。
- reviewer 残余建议：若后续需要更强 runtime 证据，可增加专门 KUnit；没有 lockdep 时，本次以结构性 review、source audit 和 build gate 作为主要证据。

**验证：**

- `just fmt kernel`：通过。
- `just build`：通过；kernel release + KUnit feature 编译通过。
- `git diff --check`：通过。
- `mdbook build docs`：通过，输出到 `docs/book`。
- source audit `rg -n "mark_detached\\(\\);|move_attached\\([^\\n]*\\);|lazy_detach_subtree\\(mount\\)\\?;|detach_mount\\(mount\\)\\?;|MountFlags" anemone-kernel/src/fs/mount anemone-kernel/src/fs/api/mount anemone-kernel/src/fs/namei.rs`：仅命中预期的新 carrier-return call sites；未发现旧 ignored return / `MountFlags` 复活。

**未运行验证：**

- 未运行 QEMU runtime KUnit、user-test 或 LTP mount profile。本修复不改变用户可见 mount semantics；重点验证是锁序 / 生命周期结构和 build gate。

**归属：**

- 这是 Gate P1 lifecycle feedback 下的实现缺陷修复，不改变 `MNT_DETACH` topology-only accepted contract，不关闭 final superblock cleanup owner / retry queue / fanotify mark-dead limitation。
