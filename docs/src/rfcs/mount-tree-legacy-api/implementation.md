# mount tree legacy API 迁移实施计划

**状态：** Completed; accepted limitations tracked in register
**最后更新：** 2026-06-19
**父 RFC：** [RFC-20260604-mount-tree-legacy-api](./index.md)
**不变量：** [不变量需求](./invariants.md)
**背景：** [mount LTP 与 Linux 参考调查](./backgrounds/ltp-linux-reference-20260604.md)

## 迁移原则

- 先闭合 legacy mount API 的 mount tree transaction，再打开用户可见成功语义。
- syscall handler 只负责 ABI 参数、flag/data 读取、权限检查、路径解析模式和 errno 映射。
- `MountTree` 拥有 topology mutation；filesystem 后端只创建/复用 superblock 和解析私有 data。
- `PathRef = Mount + Dentry` 是正式位置模型，不替换、不旁路。
- mount flags 采用严格 allowlist；暂不支持且有可见语义的能力稳定拒绝并打日志。
- fstype alias 是 syscall 边界的临时 LTP 兼容桥；不得下沉到 mount tree、filesystem backend、mount attrs 或 data parser。
- util-linux `-o loop` 不由内核 mount parser 伪造；普通 image file 绑定 loop 设备属于 ioctl-loop / block 设备路径。
- 语义问题默认对齐 Linux；当实现代价高或依赖后续子系统时，稳定拒绝、记录限制并保留后续 gate。
- 每个阶段都必须有可验证 gate；未闭合 feature 不能伪成功。
- 反馈机制只能验证受控假设和优化路线，不能削弱目标、不变量或验收边界；probe 计划写在本文的 `Probe / Vertical Slice Gates`，执行结果进入 transaction devlog，不新建通用 feedback/probe 状态文件。

## 阶段 0：公开 RFC 协议关闭

前置条件：

- LTP / Linux 背景调查已落在 `backgrounds/`。
- 本公开 RFC 的文档层 review 已完成，且设计公式已收敛。

交付：

- `index.md` 明确 legacy mount API 第一版范围、非目标和接受边界。
- `invariants.md` 明确状态所有权、锁类型、线性化点和生命周期规则。
- `implementation.md` 拆出 staged gates。

审计：

- 确认草案没有把 new mount API、真实 namespace 或 `pivot_root` 偷偷纳入第一版。
- 确认当前 `NameSpace` 只被定义为待正名的 `MountTree`。

模块边界预检：

- 文档层只允许调整本 RFC；不得把私有草案路径写成公共 canonical 链接。
- 若 review 发现需要改变 RFC artifact 边界，先回到 RFC workflow 文档判断，不在本 RFC 内另建平行状态文件。

write set：

- `docs/src/rfcs/mount-tree-legacy-api/**`
- `docs/src/rfcs.md`
- `docs/src/SUMMARY.md`

可观测性：

- 文档层无运行态日志要求。

验证：

- `git diff --check`
- `mdbook build docs`

退出条件：

- 公开 RFC 已可作为 implementation canonical source；进入实现前必须建立 transaction devlog。

## 阶段 1：UAPI parser、flag 分层、legacy data 和 syscall alias

前置条件：

- 阶段 0 文档协议关闭。
- 公开 RFC 已成为 implementation canonical source。
- implementation transaction devlog 已建立，并与本 RFC 建立双向链接。

交付：

- 在 ABI 层补齐 legacy mount/umount 所需常量：`MS_BIND`、`MS_REC`、`MS_MOVE`、`MS_REMOUNT`、`MS_PRIVATE`、`MNT_DETACH`、`MNT_EXPIRE`、`MNT_FORCE`、`UMOUNT_NOFOLLOW` 等第一版需要识别的位。
- 将 raw `mountflags` 解析为 operation request 和 per-mount attrs，不再直接生成单一 `MountFlags`。
- 引入 `MountAttrFlags`，首批只闭合 `RDONLY`。
- 阶段 1 若为复用旧 new-mount call path 暂时把 `MountAttrFlags::RDONLY` 映射到既有 `MountFlags::RDONLY`，该映射只属于迁移桥；不得增长新语义，阶段 3 attr plumbing 必须删除 `MountFlags`。
- 预留 future `SuperBlockState` 边界，但不实现 sb-wide reconfigure。
- 为 legacy `data` 引入 `MountData` 传递对象；filesystem 后端可选择接受 `NULL` / 空 data 或拒绝非空 data。
- 将现有 `tmpfs`、`ext2`、`ext3`、`vfat` 到 `ramfs` 的 fstype normalization 显式收进 syscall adapter。`tmpfs` 可以作为 ramfs 兼容入口；`ext2` / `ext3` / `vfat` 是当前 LTP 兼容桥，必须记录日志、说明它不承诺真实 filesystem 语义，并给出退出条件。
- 归一化后的 VFS 请求只携带 normalized fstype 和 source policy；`MountTree`、`FileSystemOps::mount()`、`MountFlags`、`MountAttrFlags` 和具体 filesystem backend 不得观察原始 alias。
- `data` 中的 `loop` 或普通 image file 自动绑定请求不得伪成功。需要 loop 的用户态路径必须先通过 loop ioctl 得到 block device，再调用普通 mount。
- 对 unsupported flag、operation combination 和 data option 打清晰日志，区分 stable reject 与 harmless compat。

审计：

- 搜索 `MS_`、`MountFlags`、`FileSystem::mount(`、`FileSystemOps`，确认不再把 operation bits 存成 mount attrs。
- 搜索现有 `[NYI] mount: ignoring unsupported flags`，替换为严格 allowlist 语义。
- 搜索 fstype alias 表，确认它只存在于 `fs/api/mount/**` syscall adapter，不被 VFS helper、backend 或 public docs 当作真实 filesystem support。
- 搜索 `loop` / mount `data` 处理，确认没有内核 mount parser 自动创建 loop 设备或把普通 file source 当 block source。

反馈假设：

- 假设 legacy mount parser 可以在 syscall adapter 内完成 ABI flag 分类、fstype alias 和 `MountData` 读取，而不扩大 `MountTree` / backend public API。
- 失败信号：alias 或 raw `MS_*` 泄漏到 `MountTree`、backend 需要识别原始 Linux flag，或 unsupported flag/data 返回成功；此时停止阶段，回写 `implementation.md` 或 `invariants.md`。

模块边界预检：

- `fs/api/mount/**` 若同时堆积 copy-in、flag parser、fstype alias、source policy 和 backend dispatch，应先做同一 syscall owner 内的 split-only checkpoint。
- 拆分不得改变 public syscall behavior，不得把兼容 alias 移入 `fs/mount.rs`、`filesystem.rs` 或具体 backend。

write set：

- `anemone-abi/src/fs.rs`
- `anemone-kernel/src/fs/api/mod.rs`
- `anemone-kernel/src/fs/api/mount/**`
- 旧 `anemone-kernel/src/fs/api/mount.rs` / `umount.rs` 的目录化删除
- `anemone-kernel/src/fs/filesystem.rs`
- `anemone-kernel/src/fs/mod.rs`，仅限 `MountData` syscall-only 透传 helper / call-through，不得改变 topology owner 或 stack 语义
- `anemone-kernel/src/fs/mount/**`
- 必要的 filesystem backend mount signature 调整文件，如 `ramfs`、`devfs`、`procfs`、`ext4`。

可观测性：

- unsupported flag log 必须包含 raw flag、分类和拒绝原因。
- unsupported data log 必须包含 filesystem type 和 data 是否为空；不要打印未清洗的长用户字符串。
- fstype alias log 必须包含原始 fstype、normalized fstype、兼容原因和退出条件分类。

验证：

- `just build`
- mount parser KUnit 或最小 syscall 负例，覆盖 unknown flags、非法组合、非空 unsupported data、`-o loop` 不伪成功，以及 fstype alias 不进入 VFS 状态。

退出条件：

- 不再存在 unsupported `MS_*` 被忽略后成功的路径。
- `MS_RDONLY` 仍能作为 per-mount attr 传入 new mount。
- 现有 LTP alias bridge 被限定在 syscall boundary，且每个 alias 都有日志和退出条件；真实 filesystem support 不因 alias bridge 被误报。

## 阶段 2：`MountTree` owner、transaction lock 和 stack 语义

前置条件：

- 阶段 1 的 parser 和 flag 分层完成。

交付：

- 将当前内部 `NameSpace` 正名为 `MountTree`。
- 引入睡眠式 `tx_lock: Mutex<()>` 串行化普通 topology writer；用 `inner: SpinLock<MountTreeInner>` 集中维护 root、mount list、mountpoint stack、attachment registry 和 `placement_generation`。
- anonymous fs initcall 所需的 first-root publish 必须走显式 fs-private early-root API；不得用 `can_sleep`、IRQ/preempt 状态或 panic 状态推断进入特殊路径。普通 root mount 仍必须通过 `tx_lock`。
- 第一版采用单一 placement lock 加 generation retry，不引入 COW / snapshot-style publish。
- 将 `Mount` placement 拆成 root / attached / detached 三态，消除 `parent == None` 同时表示 root 和 detached 的二义性。
- 收窄 `Mount::add_child/remove_child` 等拓扑修改接口，使其只能由 `MountTree` transaction 调用。
- 将同一 mountpoint stack 改为 Linux-like topmost-visible。
- `mount_at` / root mount / unmount 通过 `MountTree` transaction 线性化。
- 为所有 attach 类操作提供锁内 revalidation helper：`NewMount` 重验 target，`BindMount` / `RecursiveBind` 重验 source 和 target，失败不得发布新 view。
- lookup 通过窄 API 短临界区读取栈顶 mount；若 path walk 期间 `placement_generation` 变化，必须 retry，成功返回的 `PathRef` 来自同一 generation。

审计：

- 搜索 `child_at`、`add_child`、`remove_child`、`mount_at`、`unmount(`，确认没有 syscall 或 filesystem 后端绕过 `MountTree`。
- 审计所有 mountpoint stack 读写都经过 `MountTree` placement API；旧 stack 和新 stack 不得分两次对读侧可见发布。
- 对现有 multi-mount stack KUnit 做语义更新：后挂载层可见，卸载栈顶后露出下一层。
- 明确反转当前 direct stack KUnit 的 first-visible 基线：`test_vfs_direct_multi_mount_same_mountpoint_visibility_switch` 和 `test_vfs_direct_multi_mount_stack_stress` 这类测试必须证明 newest/topmost visible，而不是 first-mounted visible。

反馈假设：

- 假设单一 placement lock 加 `placement_generation` retry 足以让 lookup 只观察 transaction 前或 transaction 后状态，不需要 COW snapshot 作为第一版发布协议。
- 失败信号：path walk 无法在 generation 变化后可靠 retry、attach revalidation 需要跨锁保存不稳定 parent 指针，或读侧必须长期持有全树锁；此时停止阶段并回写 `invariants.md`。

模块边界预检：

- 如果 `fs/mod.rs` 继续同时承担 `NameSpace`/`MountTree` owner、path walk glue 和 syscall-facing helper，应先做同一 fs owner 内的目录化或 split-only checkpoint。
- `fs/namei.rs` 只允许通过窄 API 读取 mountpoint stack；不得获得 `MountTreeInner` 私有锁或直接改 placement。
- early-root 特殊 API 只服务 anonymous fs initcall 的 pseudo root 发布，不得成为 syscall、panic 或其它 no-sleep context 的通用 mount 绕行。

write set：

- `anemone-kernel/src/fs/mod.rs`
- `anemone-kernel/src/fs/mount/**`
- `anemone-kernel/src/fs/namei.rs`
- `anemone-kernel/src/fs/path.rs` 仅在注释或可见性需要时触碰。

可观测性：

- mount attach/detach log 包含 operation、target path、fstype、attrs 和 stack depth。
- assertions 覆盖非 root mount 必须有 parent/mountpoint、root mount 不得有 parent/mountpoint。

验证：

- `just build`
- VFS KUnit：mount stack visibility、unmount top layer reveal、root mount 不可卸载、child mount busy。

退出条件：

- `PathRef` 仍是 `Mount + Dentry`。
- topology mutation 只能从 `MountTree` transaction 进入。
- stack 行为与 Linux-like topmost-visible 一致。
- lookup retry / generation 机制能证明 move 读侧只能观察 move 前或 move 后。

## 阶段 3：ordinary per-mount readonly remount 和 attr plumbing

前置条件：

- 阶段 2 的 `MountTree` transaction 已闭合。

交付：

- 实现 `MS_REMOUNT` 的 per-mount `RDONLY` 切换。
- `MountAttrFlags` 第一版由 `Mount` 上的 atomic bitset / interior-mutable attrs 承载，作为 per-mount attrs 的单一真相源。
- 删除 `MountFlags` 作为并列 per-mount flag 类型；`Mount` 直接持有 `MountAttrFlags` 或等价 attrs storage，`ensure_writable()` 只读取该单一真相源。
- `FileSystemOps::mount()` / `FileSystem::mount()` / filesystem backend mount vtable 不再接收 per-mount attrs。backend 只接收 source、legacy `MountData` 和未来明确属于 superblock / filesystem instance 的配置；`RDONLY` 不再作为 backend mount 参数传入。
- `MS_REMOUNT | MS_BIND` 在本阶段只完成 parser 分类和稳定拒绝路径，不声明 bind view readonly 语义闭合；bind view 存在后由阶段 4 打开成功语义。
- 普通 `MS_REMOUNT` 第一版只允许当前 mount view 的 `RDONLY` 切换作为 accepted
  limitation；代码注释必须说明这不是 sb-wide reconfigure。涉及 filesystem
  instance reconfigure 的 remount data / option / attr 必须稳定拒绝并打日志。
- remount 发布 attrs 前必须进入 `MountTree` transaction，重验目标 mount view 仍 attached，且仍是当前 `MountTree` 中该路径对应的目标 view；重验失败不得改 attrs 或返回成功。
- remount 成功路径在仍持有 placement lock 时用 release-store 或等价同步发布 attrs；用户可见写入口用 acquire-load 或等价同步读取当前 `PathRef.mount()` attrs。
- 已打开 fd 在 remount ro 后，后续 `write` / `ftruncate` / `fallocate` 等直接修改入口重新检查 `file.path().mount()`。
- mount readonly 不进入 `FileStatusFlags` / `FileOpStatusFlags`。opened file description 仍是 file status flags 唯一真相源；mount readonly 必须由当前 `PathRef.mount()` 在用户可见写入口重新检查。

审计：

- 搜索所有直接写入口，确认仍按当前 `PathRef.mount()` enforcement，而不是 inode/superblock 全局 readonly。
- 审计 remount target revalidation，确认旧 `PathRef` 指向 detached mount 时不会更新 attrs 后返回成功。
- 搜索 `MountFlags`，确认该类型和 `MountAttrFlags -> MountFlags` 迁移桥已删除；保留命中只能是历史文档或本阶段前的事务记录。
- 审计用户可见写入口矩阵：`write` / `writev` / `pwrite*`、`open(O_TRUNC)`、`truncate` / `ftruncate`、`fallocate` grow、目录项创建/删除/改名、metadata 修改、fanotify path-fd reopen 写入，以及 copy-backed `splice(pipe -> file)`。
- 搜索 `ensure_writable` 调用点，分类尚未覆盖的 mmap/writeback 限制。

反馈假设：

- 假设 ordinary `MS_REMOUNT` 的 first-pass 成功语义可以限定为当前 live mount view 的 `RDONLY` 切换，而不需要 sb-wide reconfigure。
- 失败信号：LTP/Linux 对照证明 ordinary remount 成功必须修改 filesystem instance state，或现有写入口无法按 `PathRef.mount()` 重新检查 attrs；此时停止阶段，回写 `index.md` / `invariants.md` 或登记 limitation。

模块边界预检：

- readonly enforcement helper 应位于 VFS/write-entry owner 能复用的位置；不要让 syscall parser 直接了解每个写入口。
- 若 `fs/mount.rs` 同时承载 topology transaction 和 attr enforcement 细节导致锁/状态边界混乱，应先拆出同一 owner 内的 attrs 子模块或窄 facade。

write set：

- `anemone-kernel/src/fs/mount/**`
- `anemone-kernel/src/fs/filesystem.rs`
- `anemone-kernel/src/fs/api/mount/**`
- 必要的 filesystem backend mount signature 调整文件，如 `ramfs`、`devfs`、`procfs`、`ext4` 和 anonymous fs。
- `anemone-kernel/src/fs/api/truncate/**`
- `anemone-kernel/src/fs/api/openat.rs`
- 现有直接写入口所在文件，仅限补齐 per-mount readonly 检查。
- `anemone-kernel/src/fs/api/splice/**` 仅在 audit 证明 copy-backed写入绕过现有 `FileDesc` / `File` writable gate 时触碰。
- 必要时更新 `docs/src/register/current-limitations.md`，保留 mmap/writeback 限制。

可观测性：

- remount log 明确 old attrs、new attrs、operation classification，以及 `MS_REMOUNT | MS_BIND` 在阶段 3 尚未打开成功语义。
- code comment 明确 `RDONLY` 是 per-mount view enforcement，不是 superblock reconfigure。

验证：

- `just build`
- KUnit 或 targeted user-test：ordinary remount ro 后当前 live mount view 写失败，rw 后恢复可写。
- 已打开 fd 在 remount ro 后写入返回 `EROFS`。

退出条件：

- ordinary per-mount readonly 语义可验证；`fs_readonly` 的 bind-remount 场景等阶段 4 关闭。
- `MountFlags` 不再作为源码类型存在；new mount、remount 和写入口均使用同一套 per-mount attrs 真相源。
- filesystem backend 不再接收或保存 per-mount attrs；真实 sb-wide reconfigure 仍按 future `SuperBlockState` / follow-up gate 处理。
- remount 成功返回前完成 target revalidation 和 attrs 发布；旧 fd 对同一 live mount view 的后续写能观察 remount 后 attrs。
- detached 或已被 move/replaced 的旧 target view 不会被 remount 偶然修改后返回成功。
- `MS_REMOUNT | MS_BIND` 尚不返回成功，或者只在阶段 4 bind view 语义闭合后打开。
- 未声称 shared writable mmap 或 dirty writeback 已闭合。

## 阶段 4：plain bind 和 recursive bind

前置条件：

- 阶段 2 的 mount view / stack 语义完成。
- 阶段 3 的 ordinary per-mount attr plumbing 已完成；`MS_REMOUNT | MS_BIND` 尚未声明成功闭合。

交付：

- `MS_BIND`：创建新的 `Mount` view，复用 source superblock，root 为 source directory dentry。
- `MS_REC | MS_BIND`：递归 clone source subtree 的 mount views。
- plain bind 不 clone child mounts。
- bind attach 必须在 `MountTree` transaction 内重验 source 仍 attached、target 仍 attached 且仍是当前目标 view。
- recursive bind 必须先明确 source subtree snapshot / retry 策略；attach 成功以整棵 cloned subtree 全部可见为线性化点，失败不得留下半棵可见 subtree。
- `MS_REMOUNT | MS_BIND` 在 bind view 存在后打开成功语义，只更新目标 bind view 的 attrs，不污染 source 或 sibling mount。
- 首批只支持目录 bind；file bind 稳定拒绝并打日志。
- shared/slave/unbindable propagation 未闭合前，相关成功语义稳定拒绝。
- 新 bind view 是新的 `Arc<Mount>` identity。fanotify mount mark 不自动从 source view 继承到 bind view，除非后续 fanotify RFC 明确改变该语义。

审计：

- 确认 bind mount 不调用 filesystem mount 创建新 superblock。
- 确认 unmount bind view 不 kill shared superblock 或影响 sibling mount。
- 确认 source / target lookup 到 attach transaction 之间的 TOCTOU 窗口被锁内 revalidation 关闭。
- 确认 recursive bind 失败不会留下半棵可见 subtree。
- 审计 fanotify `FanTarget::Mount` consumer：source mount mark、bind view mark 和 filesystem mark 的匹配边界不能因复用 superblock 或 dentry 混淆。

反馈假设：

- 假设 recursive bind 可以用 transaction 前 source subtree snapshot 加 generation/retry 关闭并发 move/detach 风险，而不引入长期 detached mount fd 或 new mount API object。
- 失败信号：rbind clone 需要跨 transaction 持有不稳定 parent 指针、失败回滚无法保证全有或全无可见性，或 bind-remount 需要改变 readonly accepted boundary；此时停止阶段，回写 `implementation.md` / `invariants.md` / `tracking-issues.md`。

模块边界预检：

- bind/rbind clone 逻辑应归 `MountTree` owner；syscall 层只提供 parsed request 和 resolved `PathRef`。
- 如果 clone 需要大量 subtree traversal helper，优先在 `fs/mount.rs` 同一 owner 内拆出 ops/state 子模块，不把 traversal 暴露给 backend 或 namei。

write set：

- `anemone-kernel/src/fs/mount/**`
- `anemone-kernel/src/fs/mod.rs`
- `anemone-kernel/src/fs/api/mount/**`
- `anemone-kernel/src/fs/namei.rs`，仅限 `#[cfg(feature = "kunit")]` forced generation-retry hook / test seam，不得改变普通 namei 解析语义或 task root/cwd owner。
- `anemone-kernel/src/fs/namei.rs` 仅限必要 lookup/stack 辅助。
- `anemone-kernel/src/fs/path.rs` 仅限让 `PathRef::to_pathbuf()` 使用与 bind root 相同的 mount-root boundary，不改变 path identity 或 task root/cwd owner。

可观测性：

- bind/rbind log 包含 source path、target path、recursive 与 clone mount count。
- bind-remount log 明确 target bind view、old attrs、new attrs 和 source/sibling 不受影响。
- unsupported file bind / propagation-dependent bind log 包含稳定拒绝 errno。

验证：

- `just build`
- KUnit：plain bind sibling view、rbind subtree clone、unmount bind 不 kill shared superblock。
- KUnit 或 targeted user-test：同一 inode 经 ro/rw sibling bind mount 访问，`remount,ro,bind` 后 ro view 写失败，source / sibling rw view 写成功。
- LTP smoke：`mount05`、`fs_readonly` 小 profile；若 `/proc/mounts` live view 尚未完成，本阶段结果只能用于判断 bind/remount 主语义，不能以 cleanup 失败判定 bind/remount 失败。

退出条件：

- bind/rbind 成功返回只发生在语义闭合的目录场景。
- `remount,ro,bind` 不污染 source/sibling mount。
- recursive bind 失败路径不会留下部分 visible mount view。

## 阶段 5：private move mount 和有限 propagation

前置条件：

- 阶段 4 的 bind/rbind 已闭合。

交付：

- `MS_PRIVATE` / `MS_REC | MS_PRIVATE`：有限接受，作为 private tree 下 bind/move 的基础。
- `MS_MOVE`：移动同一个 mount subtree，不 clone，不新建 superblock。
- `MS_MOVE` 在同一个 placement transaction 内重验 source/target、防环、从旧 stack 摘除并插入新 stack，最后 bump `placement_generation`。
- 防止把 mount 移动到自身 subtree 内形成拓扑环。
- `MS_SHARED`、`MS_SLAVE`、`MS_UNBINDABLE` 在完整 propagation RFC/gate 前稳定拒绝并打日志。
- `MS_MOVE` 与 `MS_BIND`、`MS_REMOUNT`、new mount attrs 等非法组合稳定拒绝。
- `MS_MOVE` 保留同一个 `Arc<Mount>` identity；fanotify mount mark 应跟随 moved view，而不是在 old/new mountpoint 间复制或丢失。

审计：

- 搜索 move path，确认 source subtree attrs、children、stack 内部顺序保留。
- 审计 path lookup 后进入 transaction 前的 TOCTOU 边界；必须在 transaction 内重新验证 source/target mount identity、attachment state 和 target stack。
- 审计 move 发布点，确认旧 stack 摘除和新 stack 插入不会被 lookup 读侧分开观察。
- 审计 fanotify mount mark identity 在 move 前后仍指向同一个 mount object。

反馈假设：

- 假设 private tree 下的 `MS_MOVE` 可以在单一 placement transaction 中关闭防环、source/target revalidation 和 generation retry，不需要 shared propagation 状态。
- 失败信号：move 需要 peer group/master-slave 信息才能保证目标语义，或 lookup 可观察到半移动状态；此时停止阶段并把 shared propagation 升级为单独 RFC/gate，而不是在本阶段伪成功。

模块边界预检：

- `MS_MOVE` 应复用阶段 2 的 placement API；不得在 syscall 层直接摘除或插入 stack。
- 如果 move 防环需要跨 subtree 扫描 helper，该 helper 保持在 `MountTree` owner 内，不能让外部调用者依赖内部 parent/children 字段。

write set：

- `anemone-kernel/src/fs/mount/**`
- `anemone-kernel/src/fs/mod.rs`
- `anemone-kernel/src/fs/api/mount/**`

可观测性：

- move log 包含 old target、new target、moved mount identity 和 subtree size。
- propagation reject log 明确缺少 peer group/master-slave support。

验证：

- `just build`
- KUnit：basic move、move into own subtree rejected、move preserves subtree and attrs。
- KUnit：move 与 lookup generation retry 边界，证明 successful lookup 只返回 move 前或 move 后位置。
- LTP smoke：`mount06` 和 fs_bind move 基础用例；若 `/proc/mounts` live view 尚未完成，本阶段结果只能用于判断 move 主语义，不能以 cleanup 失败判定 move 失败。

退出条件：

- private tree 下基础 move 可用。
- `MS_MOVE` 保留同一个 `Arc<Mount>` identity，且读侧发布协议不暴露半移动状态。
- shared/slave/unbindable 不伪成功。

## 阶段 6：`umount2` flags、pre-unmount cleanup 和 mounts 视图

前置条件：

- 阶段 2 的 detach transaction 已闭合。
- 阶段 4/5 的 bind/move 已能产生 LTP cleanup 需要枚举的 topology。

交付：

- `flags == 0`：同步 unmount，busy 返回 `EBUSY`。
- `MNT_DETACH`：lazy detach，从 `MountTree` 摘除目标 subtree，旧引用自然释放；在 Gate P1 关闭前只声明 topology detach，不声明 final superblock cleanup 完整闭合。
- `UMOUNT_NOFOLLOW`：target 最后一跳不跟随 symlink。
- `MNT_FORCE`：稳定拒绝并打日志。
- `MNT_EXPIRE`：暂缓；非法组合如 `MNT_EXPIRE | MNT_FORCE`、`MNT_EXPIRE | MNT_DETACH` 返回 `EINVAL`。
- 接入 fanotify 等 observer 的 pre-unmount cleanup / mark-dead hook。没有该 hook 时，只能声明 mount tree detach 生效，不能声明 mount/filesystem mark 生命周期完整兼容。
- 最后一个 mount view 的 superblock cleanup 通过 inode-shrinker RFC 接受的显式 eviction path 编排；mount 层不重新定义 resident inode eviction。若 Gate P1 不能证明 cleanup owner 和 retry 路径，本阶段必须登记 limitation，不能让 lazy detach 假装完成 final `kill_sb`。
- `/proc/<tgid>/mounts` 提供最小 live view，输出 LTP cleanup 可消费的六列格式；`/proc/mounts` 继续作为 `self/mounts` symlink。
- mounts view 的 target path 按 task fs root 渲染：`/proc/self/mounts` 使用读取任务 root，`/proc/<tgid>/mounts` 使用绑定 task root。无法表达在该 root 下的 mountpoint 时省略或稳定标记为不可表达，不 fallback 泄露全局路径。

审计：

- 对照 LTP `umount01`、`umount02`、`umount03`、`umount2_01`、`umount2_02` 确认 errno matrix。
- 搜索 procfs mounts 实现，确认 `/proc/mounts` 只是 symlink，内容落在 `/proc/<tgid>/mounts`，且视图来自 `MountTree` snapshot，而不是 filesystem 后端拼接。
- 搜索 fanotify mount/filesystem mark target，确认 detach / kill 前后有清理或明确限制记录。
- 审计 final superblock cleanup 不在 `MountTree` transaction lock 下执行 filesystem sync / evict / kill；inode eviction 顺序服从 inode-shrinker RFC。

反馈假设：

- 假设 `MNT_DETACH` 的 topology detach 可以先闭合，final superblock cleanup owner、retry queue 和 fanotify mark-dead 通过 Gate P1 验证后再决定是否进入本 RFC 第一版。
- 假设 detached / moved `PathRef` 的 relative lookup 可以按 Gate P2 选择稳定边界，且绝不 fallback 到当前全局 root 或 stale parent。
- 失败信号：final cleanup 只能放在 `Drop` 或持 tree lock 的长临界区、observer cleanup 无法 fail closed、detached cwd/root 会跨回错误 parent；此时停止阶段，回写 `invariants.md` / `tracking-issues.md` 或登记 limitation。

模块边界预检：

- unmount flag parsing 留在 `fs/api/mount/umount.rs`；detach、busy、cleanup queue 和 mount snapshot 属于 `MountTree` / VFS owner。
- 若 pre-unmount hook 需要触碰 fanotify owner surface，先提交 write set 扩展申请并在 transaction devlog 记录批准边界。
- procfs mounts view 只消费 `MountTree` snapshot；不得让 procfs 后端拼接或修改 mount topology。

write set：

- `anemone-kernel/src/fs/api/mount/umount.rs`
- `anemone-kernel/src/fs/mod.rs`
- `anemone-kernel/src/fs/mount/**`
- `anemone-kernel/src/fs/proc/mounts.rs`
- `anemone-kernel/src/fs/proc/tgid/mounts.rs`
- `anemone-kernel/src/fs/fanotify/**` 仅在新增 pre-unmount cleanup hook 或 mark-dead 边界时触碰。
- `anemone-kernel/src/fs/superblock.rs` 仅在 unmount 需要复用或暴露 explicit eviction facade 时触碰；不得重写 inode-shrinker eviction 协议。

可观测性：

- sync unmount busy log 区分 child mount、superblock active refs、observer cleanup blocker、last view eviction / kill 失败。
- lazy detach log 明确 detached subtree size。
- `/proc/<tgid>/mounts` snapshot 可在 LTP cleanup 前后观察到变化，并记录路径无法在 task root 下表达时的处理。

验证：

- `just build`
- KUnit：lazy detach hides from new lookup、detached cwd/root 不 fallback 到全局 root 或 stale parent、NOFOLLOW symlink target。
- KUnit 或 targeted smoke：`/proc/mounts -> self/mounts` symlink 仍存在，`/proc/self/mounts` 输出六列，chroot/root 视角不泄露 root 外路径。
- LTP smoke：`umount01..03`、`umount2_01`、`umount2_02` 可支持子集。

退出条件：

- LTP cleanup 不再因为缺少 `/proc/mounts` 基础视图失败。
- `MNT_EXPIRE` 和 `MNT_FORCE` 不伪成功。
- fanotify mount/filesystem mark lifecycle、或者尚未闭合的 pre-unmount hook 限制，有明确文档归属。
- final superblock cleanup 不绕过 inode-shrinker explicit eviction 协议。
- Gate P1/P2 的结果已写入 transaction devlog；若未闭合，对应 limitation / open issue 已登记。

## 阶段 7：LTP profile 收口和限制登记

前置条件：

- 阶段 1-6 完成。

交付：

- 针对 `mount01..07`、`umount01..03`、`umount2_01..02`、`fs_readonly`、`fs_bind` private/bind/move 子集形成验证记录。
- 将仍未支持的 shared/slave/unbindable propagation、mount namespace cloneNS、new mount API、`pivot_root`、普通 `MS_REMOUNT` sb-wide reconfigure、NOEXEC/NODEV/NOSUID、readonly mmap/writeback、临时 fstype alias bridge、fanotify pre-unmount cleanup 缺口写入 RFC risk 或 register/current limitations。
- 更新公开 RFC/事务日志中的 closeout、validation 和 limitation 记录后再进入 implementation closeout。

审计：

- 区分 semantic kernel bug、unsupported feature、procfs cleanup blocker 和测试环境问题。
- 搜索所有 `[NYI] mount` / `mount:` log，确认每个都能归入已接受限制或后续 gate。
- 区分真实 filesystem support 和 syscall alias bridge 命中；不得把 `ext2` / `ext3` / `vfat -> ramfs` 兼容成功计为对应 filesystem 语义通过。
- 搜索 `MountFlags`，确认 RFC 收口后源码不再保留该迁移桥；若仍有源码命中，必须归类为 blocker，而不是 accepted limitation。

反馈假设：

- 假设第一版 legacy mount API 的支持矩阵可以按 semantic success、accepted limitation、unsupported feature 和环境 blocker 四类收口。
- 失败信号：LTP 结果无法和 `/proc/mounts` cleanup、alias bridge 或真实 mount semantics 分离；此时补背景证据或 transaction 记录，不直接改 RFC 接受边界。

模块边界预检：

- closeout 阶段只更新公开 RFC / transaction / register / profile 等必要文件；不得补写 feature code。
- 本地 LTP profile 只有在用户授权时调整，且必须区分 agent-run 与 user-run 验证。

write set：

- mount RFC / transaction docs。
- `docs/src/register/current-limitations.md` 如需登记公开限制。
- `anemone-apps/user-test/ltp/groups/mount-legacy.txt` 和必要的 `anemone-apps/user-test/src/ltp.rs` group 注册 / case-argument parsing 支撑。
- `anemone-apps/user-test/ltp/profile` 仅在需要调整本地验证 profile 且用户授权时触碰。

可观测性：

- 保留 LTP group 级日志路径、测试 profile 和 pass/fail/TCONF 分类。

验证：

- `just build`
- 用户或授权运行 `./scripts/run-user-test-rv64.sh <rootfs-config> <test-disk> <log>` 的 mount profile。

退出条件：

- 第一版 legacy mount API 的支持矩阵、未支持矩阵和验证证据全部可追踪。

## 旁路审计清单

- `rg -n "MS_|MNT_|UMOUNT_|MountFlags|MountAttr|MountOp" anemone-abi/src anemone-kernel/src`
- `rg -n "NameSpace|MountTree|mount_at|unmount\\(|child_at|add_child|remove_child" anemone-kernel/src/fs`
- `rg -n "ensure_writable|ReadOnlyFs|EROFS" anemone-kernel/src/fs anemone-kernel/src/task`
- `rg -n "proc.*mount|/proc/mounts|mountinfo" anemone-kernel/src/fs/proc`
- `rg -n "\\[NYI\\].*mount|ignoring unsupported.*mount|unsupported.*mount" anemone-kernel/src`
- `rg -n "tmpfs|ext2|ext3|vfat|mount_fs_name|MountData|loop" anemone-kernel/src/fs/api/mount anemone-kernel/src/fs`
- `rg -n "FanTarget::Mount|FanTarget::Filesystem|pre-unmount|mark-dead|flush.*mount" anemone-kernel/src/fs/fanotify anemone-kernel/src/fs`
- `rg -n "try_evict_inode|try_evict_all|PERSISTENT_SB|SHRINKABLE_ICACHE" anemone-kernel/src/fs`

允许保留的旁路必须满足三点：

- 不返回用户可见成功。
- 有日志或文档说明拒绝原因。
- 有明确后续 gate 或 current limitation。

## 可观测性清单

- mount/new attach：fstype、source class、target、attrs、stack depth。
- bind/rbind：source、target、recursive、clone count。
- remount：old attrs、new attrs、bind remount vs ordinary per-mount remount。
- move：old mountpoint、new mountpoint、subtree size。
- propagation：accepted private 或 rejected shared/slave/unbindable reason。
- unmount：sync/lazy、busy reason、detached subtree size。
- unsupported flag/data/operation：raw value、classification、stable errno。
- fstype alias：raw fstype、normalized fstype、compat bridge reason、exit condition。
- `/proc/<tgid>/mounts`：snapshot source、ordering rule、task-root rendering rule。
- pre-unmount cleanup：observer target class、mark-dead/flush result、late enqueue behavior。

## 停止边界

继续追查：

- 成功返回但语义未闭合的 mount operation。
- readonly sibling mount 污染。
- bind unmount kill shared superblock。
- mount stack 可见性与 Linux-like topmost-visible 不一致。
- `/proc/mounts` 导致 LTP cleanup 无法找到或逆序卸载挂载点。
- syscall alias bridge 泄漏到 MountTree / backend，或让真实 filesystem coverage 被误报。
- unmount 没有处理 fanotify mount/filesystem mark lifecycle 却宣称完整兼容。

停止并记录为后续 gate：

- shared/slave/unbindable propagation matrix。
- `CLONE_NEWNS` / mount namespace cloneNS。
- new mount API。
- `pivot_root(2)`。
- file bind mount。
- 普通 `MS_REMOUNT` 的 sb-wide reconfigure、remount data / option / attr。
- `MS_NOEXEC` / `MS_NODEV` / `MS_NOSUID` 业务接入。
- readonly mmap/writeback coherence。
- 临时 `ext2` / `ext3` / `vfat -> ramfs` LTP alias 的删除或替换策略。
- fanotify pre-unmount cleanup hook 如果本阶段未实现，则记录为后续 gate。

## Probe / Vertical Slice Gates

默认不要为 probe / feedback 新建通用 `feedback.md`、`probe.md` 或 `experiments.md`。计划写在本节；执行结果写入 transaction devlog。只有证据包过长时，才在 `backgrounds/` 下增加具体命名的证据文件，并从本节链接。

### Gate P1 - Lazy-detach final cleanup owner

**Hypothesis:** `MNT_DETACH` 可以先由 `MountTree` 完成 topology detach；final superblock cleanup 由 VFS / `MountTree` owned cleanup queue 或等价 reaper 在 transaction lock 外重试 busy check、observer cleanup、inode eviction 和 `kill_sb`。
**Protected Goal / Invariant:** lazy detach 成功后新 lookup 不再看到 detached subtree；mount 层不得在 `Drop`、任意引用释放路径或持 `MountTree` transaction lock 的长临界区内执行 filesystem sync / evict / kill；inode eviction 顺序仍服从 inode-shrinker RFC。
**Minimum Write Set:** `anemone-kernel/src/fs/mount/**`、`anemone-kernel/src/fs/mod.rs`、`anemone-kernel/src/fs/api/mount/umount.rs`，以及必要时的 `anemone-kernel/src/fs/superblock.rs` narrow facade；若需要 fanotify hook，先申请 `anemone-kernel/src/fs/fanotify/**` write set 扩展。
**Non-goals:** 不引入 new mount API detached mount fd，不改变 inode-shrinker contract，不把 final cleanup 放进 `Drop`，不把 fanotify mark lifecycle 静默降级成日志。
**Validation Floor:** `just build`；source audit 证明 cleanup 不在 tree lock / `Drop` 中执行；KUnit 或 targeted smoke 证明 lazy detach hides from new lookup，旧引用只保持对象 lifetime；fanotify hook 若未实现，必须有 explicit limitation。
**Failure Signals:** cleanup 只能依赖最后一个 `Arc<Mount>` drop、需要在 tree lock 内执行 filesystem I/O、busy recheck 无法和 inode eviction rollback 对齐，或 observer late enqueue 不能 fail closed。
**Write-back:** execution facts 写 transaction devlog；stage order / write set / validation floor 变化写 `implementation.md`；cleanup owner 或 lifecycle invariant 变化写 `invariants.md` 并更新 `tracking-issues.md`；接受延期写 `current-limitations`。
**Exit:** 证据充分时升级为阶段 6 正式实现；证据不足则阶段 6 只声明 topology detach，并登记 final cleanup limitation / open issue。
**Evidence:** None for draft.

### Gate P2 - Detached and moved PathRef namei boundary

**Hypothesis:** detached / moved `PathRef` 可以保持对象 lifetime 和相对 lookup 的稳定边界，同时禁止 `..`、cwd/root 或 root crossing fallback 到当前全局 root 或 stale parent。
**Protected Goal / Invariant:** `PathRef = Mount + Dentry` 不被替换；detached mount 不重新出现在 `/proc/mounts` 或新 lookup；old cwd/root 行为不能绕过 `MountTree` placement state 或制造第二套 topology truth。
**Minimum Write Set:** `anemone-kernel/src/fs/namei.rs`、`anemone-kernel/src/fs/path.rs`、`anemone-kernel/src/fs/mount/**`，以及相关 VFS KUnit / targeted smoke。
**Non-goals:** 不实现完整 mount namespace、`pivot_root`、new mount API detached mount fd、file bind mount，且不改变 task root/cwd 的 owner boundary。
**Validation Floor:** `just build`；KUnit 或 targeted smoke 覆盖 move 后 cwd、lazy detach 后 cwd、detached mount root 的 `..`、relative lookup 和 root crossing；source audit 证明没有 fallback 到 global root / stale parent。
**Failure Signals:** detached cwd/root 必须依赖失效 parent 才能继续、`..` 行为无法用当前 `PathRef` + placement state 表达，或 Linux-compatible behavior 需要 mount namespace/root semantics 超出本 RFC。
**Write-back:** execution facts 写 transaction devlog；若只需调整 stage gate，更新 `implementation.md`；若选择改变 namei accepted semantics，更新 `index.md` / `invariants.md` 和 `tracking-issues.md`；若延期完整语义，登记 limitation / open issue。
**Exit:** 选择并验证稳定边界后升级为阶段 6 正式语义；否则保持第一版 limitation，不让弱 fallback 成为成功语义。
**Evidence:** None for draft.

## 实现期反馈记录

- 2026-06-18：文档层反馈重分类；阶段 3/4 依赖环和 attach revalidation 已折回 canonical text，lazy-detach cleanup 与 detached-path namei 改为 Gate P1/P2；目标和不变量保持不变。
- 2026-06-18：阶段 0 关闭，事务日志 [2026-06-18-mount-tree-legacy-api](../../devlog/transactions/2026-06-18-mount-tree-legacy-api.md) 已建立；阶段 1 尚未启动。
- 2026-06-18：阶段 1 write set 按用户批准扩展为 `fs/api/mount/**` syscall owner 目录；`fs/mod.rs` 仅允许用于 `MountData` syscall-only 透传 helper，不打开阶段 2 topology owner 迁移。
- 2026-06-18：阶段 1 实现反馈确认现有 `MountFlags` 只剩 `RDONLY`，且已被 `MountAttrFlags` 覆盖为更准确的 per-mount attrs 表达；RFC 接受 `MountFlags` 作为阶段 1 迁移桥，但要求阶段 3 attr plumbing 删除该类型，并让 `FileSystemOps::mount()` 不再接收 per-mount attrs。
- 2026-06-19：阶段 2 closeout 后的结构反馈确认 `fs/mod.rs` 已同时承担 VFS facade、mount tree owner、VFS ops 和 KUnit，继续在其中堆叠阶段 3 remount attrs 会固化错误 owner boundary；阶段 3 前允许做 behavior-preserving `fs/mount/` 目录化 checkpoint。
- 2026-06-19：阶段 2 implementation feedback 确认 `can_sleep` / IRQ / preempt 状态推断不应作为 early-root publish 的分流条件。canonical 形状改为普通 root mount 永远走 `tx_lock`，anonymous initcall 通过显式 fs-private early pseudo-root API 发布 first root；panic 或其它 no-sleep context 不获得 mount-tree writer bypass。
- 2026-06-19：阶段 5 review gate 要求用 KUnit 覆盖 move 与 lookup generation retry 边界。用户批准将 `anemone-kernel/src/fs/namei.rs` 纳入阶段 5 write set，仅用于 `#[cfg(feature = "kunit")]` forced retry hook；普通 namei path walk 语义不变。
- 2026-06-19：阶段 6 Gate P1 只关闭 `MNT_DETACH` topology detach；final superblock cleanup owner、retry queue、fanotify mount/filesystem mark-dead hook 和 `MNT_EXPIRE` 两阶段协议登记为 [ANE-20260619-MOUNT-UNMOUNT-CLEANUP-STAGE1](../../register/current-limitations.md#ane-20260619-mount-unmount-cleanup-stage1)。同步 unmount 暂保留阶段 2 为避免 `sget()` 复用竞态而持有 writer gate 穿过 `try_evict_all()` / `remove_sb()` / `kill_sb()` 的实现，不把它声明为长期 P1 形态。Gate P2 通过 lazy-detach KUnit 验证 detached mount root 的 `..` 不 fallback 到全局 root 或 stale parent。
- 2026-06-19：阶段 7 closeout 确认 `mount-legacy` group 是评分 / 观测 profile，而不是只保留 whole-case PASS 的 first-pass profile。`fs_bind*`、`fs_bind_move*`、`fs_bind_rbind*` 和 `test_robind*` 继续保留启用；阶段 7 按 TPASS/TFAIL/TCONF 子结果、accepted limitation 和环境 blocker 分类，不通过缩小 group 换取干净汇总。
- 2026-06-19：阶段 7 把 shared/slave/unbindable propagation、mount flag/statfs matrix、fstype alias bridge、ROFS mmap/writeback 和 unmount cleanup residual 登记到 [current limitations](../../register/current-limitations.md)。`user-test` runner 现在把纯 LTP `TCONF` 退出码 `32` 计为 skipped；混合 `TCONF|TFAIL/TBROK` 仍按失败处理。

## Write Set 扩展记录

- 2026-06-18：用户批准将 `anemone-kernel/src/fs/api/mount.rs` 和 `api/umount.rs` 收归到 `anemone-kernel/src/fs/api/mount/`，该目录作为 legacy mount / umount syscall adapter 和未来 mount-family syscall 入口 owner。阶段 1 同步允许 `anemone-kernel/src/fs/api/mod.rs` 做模块声明调整。
- 2026-06-18：阶段 1 需要把 legacy `MountData` 从 syscall adapter 传到 filesystem backend，因此允许 `anemone-kernel/src/fs/mod.rs` 增加 syscall-only `mount_at_with_data` call-through；该扩展不允许改变 `NameSpace` / future `MountTree` topology、stack visibility、bind/move/remount 或 unmount transaction 语义。
- 2026-06-18：阶段 3 write set 扩展纳入 `anemone-kernel/src/fs/filesystem.rs` 和必要 backend mount signature 文件，用于删除 `MountFlags` 迁移桥并切断 filesystem backend 对 per-mount attrs 的观察；该扩展不允许打开 sb-wide remount reconfigure。
- 2026-06-19：用户批准阶段 3 前 `fs/mount/` split-only checkpoint。该结构维护只允许在同一 mount owner 内移动 `MountData`、`MountAttrFlags` / `MountFlags`、`Mount` view 和 `MountTree` owner，不改变 public VFS facade、syscall ABI、lookup 语义、superblock lifetime 或阶段 3 remount 成功边界。
- 2026-06-19：用户批准阶段 5 将 `anemone-kernel/src/fs/namei.rs` 纳入 write set，仅用于 `#[cfg(feature = "kunit")]` 的 forced generation-retry hook / test seam，证明 move 期间 successful lookup 只能返回 move 前或 move 后稳态；不得改变普通 namei 解析、mount-root crossing、task root/cwd owner 或 P2 detached-path accepted boundary。

## 结构维护记录

- 2026-06-18：`fs/api/mount.rs` / `fs/api/umount.rs` 拆为 `fs/api/mount/{mod.rs,mount.rs,umount.rs}`，属于同一 syscall owner 内的 split-only checkpoint；compat alias 仍限制在 mount syscall adapter。
- 2026-06-19：`fs/mount.rs` 拆为 `fs/mount/{mod.rs,data.rs,flags.rs,view.rs,tree.rs}`，属于同一 mount owner 内的 split-only checkpoint。测试不单独建 `tests.rs`：legacy data KUnit 留在 `data.rs`，mount tree / stack KUnit 留在 `tree.rs`。
