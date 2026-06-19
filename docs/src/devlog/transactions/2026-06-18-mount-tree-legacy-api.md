# 2026-06-18 - Mount Tree Legacy API

**状态：** Active
**负责人：** doruche, Codex
**领域：** fs / VFS / mount / LTP
**权威计划：** [RFC-20260604-mount-tree-legacy-api](../../rfcs/mount-tree-legacy-api/index.md), [不变量需求](../../rfcs/mount-tree-legacy-api/invariants.md), [迁移实施计划](../../rfcs/mount-tree-legacy-api/implementation.md)
**当前阶段：** 阶段 3 ordinary per-mount readonly remount 已关闭；阶段 4 尚未启动

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
