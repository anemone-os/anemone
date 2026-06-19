# RFC-20260604-mount-tree-legacy-api

**状态：** Accepted for Implementation
**负责人：** doruche
**最后更新：** 2026-06-19
**领域：** fs / vfs / mount
**事务日志：** [2026-06-18-mount-tree-legacy-api](../../devlog/transactions/2026-06-18-mount-tree-legacy-api.md)
**开放问题：** 无 active design blocker；lazy-detach cleanup ownership 和 detached-path namei semantics 已作为受控反馈 gate 写入 [迁移实施计划](./implementation.md)。
**下一步：** 阶段 5 private move mount 和有限 propagation 已按 [事务日志](../../devlog/transactions/2026-06-18-mount-tree-legacy-api.md) 关闭；下一实现动作是启动阶段 6 `umount2` flags、pre-unmount cleanup 和 mounts 视图。

## 摘要

本 RFC 把 Anemone 现有的 mount 能力正式化为 legacy mount API 的可审查架构。目标不是一次性复制 Linux mount namespace 的完整历史语义，而是先让 `mount(2)`、`umount(2)`、`umount2(2)`、bind mount、recursive bind、move mount、per-mount readonly remount、有限 propagation flag 和 `/proc/mounts` cleanup 视图共享同一套 mount tree transaction。

本 RFC 不提前实现或暴露 new mount API 的接口形状。`fsopen()`、`fsconfig()`、`fsmount()`、`open_tree()`、`move_mount()` 和 `mount_setattr()` 需要后续单独 RFC 推进。本 RFC 只要求当前状态模型不要在功能上堵死这些后续能力；不为了它们提前引入 detached mount fd、filesystem context fd 或类似 Linux `fs_context` 的生命周期接口。

## 背景

当前内核已有基础挂载树，但其边界还没有成为正式 contract：

- `anemone-kernel/src/fs/api/mount/mount.rs` 实现 `sys_mount()`，支持 pseudo source 和 block-device source。当前 syscall 边界还有临时 LTP 兼容桥：`tmpfs`、`ext2`、`ext3`、`vfat` 会归一化到 `ramfs`，且 `ramfs` 走 pseudo source；这不是 mount tree 或 filesystem backend 的长期框架语义。
- `anemone-kernel/src/fs/api/mount/umount.rs` 实现 `sys_umount2()`，当前已识别 legacy umount flag 并稳定拒绝尚未闭合的成功语义。
- `anemone-kernel/src/fs/mount/` 目录承载 mount view、legacy data、attrs flags 和 mount tree owner。`Mount` 目前仍持有迁移桥 `MountFlags`，但 `MountFlags` 只有 `RDONLY`；本 RFC 收口后不保留这层并列 flag 类型，`RDONLY` 应归入 `Mount` 的 per-mount attrs 单一真相源。
- `anemone-kernel/src/fs/mod.rs` 的 VFS singleton 持有 visible / anonymous 两棵 `MountTree`；它们不是 Linux mount namespace，真正的 per-task namespace / nsproxy 系统仍由后续 namespace RFC 引入。
- 当前同一 mountpoint 已允许多层挂载，并按 Linux-like topmost-visible stack 查找。
- `anemone-abi/src/fs.rs` 已导出 legacy mount / umount parser 第一版需要识别的 flag 常量。
- `anemone-kernel/src/fs/proc/mounts.rs` 已把 `/proc/mounts` 发布为 `self/mounts` symlink，但 `anemone-kernel/src/fs/proc/tgid/mounts.rs` 仍是空内容 stub。
- `docs/src/rfcs/inode-shrinker/` 已把 resident inode cache 回收收敛为显式 `try_evict_inode()` / `try_evict_all()` 协议；mount unmount 只能编排最后一个 view 后的 superblock cleanup，不能重新定义 inode eviction 顺序。
- `docs/src/rfcs/fanotify/` 已把 mount / filesystem mark 作为 `Arc<Mount>` / `Arc<SuperBlock>` identity consumer，并要求完整 umount 兼容接入 pre-unmount flush / mark-dead hook 后才能宣称闭合。

详细 LTP 和 Linux 参考调查见 [mount LTP 与 Linux 参考调查](./backgrounds/ltp-linux-reference-20260604.md)。

## 目标

- 将当前 `NameSpace` 语义收敛并正名为 `MountTree`：它是 mount tree owner，不是 Linux namespace。
- 保留并正式化 `PathRef = Mount + Dentry` 的位置模型；不发明新的路径 location handle。
- 支持 legacy `mount(2)` 的操作分流：new mount、plain bind、recursive bind、move、per-mount remount、有限 propagation type change。
- 支持 `umount(2)` / `umount2(2)` 的基础 flag validation、busy 判断、lazy detach 和 `UMOUNT_NOFOLLOW`。
- 支持 `fs_readonly` 需要的 `mount -o remount,ro,bind`：readonly 是 per-mount view attribute，不污染 sibling bind mount 或 superblock 全局状态。
- 为 legacy `mount(2)` 的 `data` 参数提供后端可选 parser 入口，但不引入 new mount API 的 filesystem context 状态机。
- 将 syscall 边界的 fstype 兼容归一化限制为临时 LTP bridge，不让该 bridge 下沉到 `MountTree`、`FileSystemOps` 或 filesystem backend。
- 为 LTP cleanup 提供最小 `/proc/mounts` / `/proc/self/mounts` live view。
- 对暂不支持的 mount flag、operation、data option 和组合稳定拒绝并打日志；只有确认无用户可观察差异的兼容项才允许记录后忽略。

## 非目标

- 不在本 RFC 第一版实现 new mount API；相关接口和 fd object 后续单独 RFC。
- 不在本 RFC 第一版引入 Linux `nsproxy`、per-task mount namespace、`CLONE_NEWNS`、`unshare(CLONE_NEWNS)`、`setns()` 或 `/proc/<pid>/ns/mnt`。
- 不承诺完整 Linux shared subtree propagation；首批只有限接受 private 语义，shared/slave/unbindable 在闭合前稳定拒绝。
- 不支持 file bind mount；首批只支持目录 new mount、bind、rbind 和 move。
- 不实现 `pivot_root(2)`。
- 不承诺完整 filesystem-specific data parser；每个后端可以先只接受 `NULL` 或空 data。
- 不在内核 mount parser 中实现 util-linux `-o loop` 这类用户态选项；普通 image file 到 block device 的绑定属于 loop ioctl / block 设备路径，`mount(2)` 只接收已经存在的 block-device source。
- 不把 `ext2`、`ext3`、`vfat` 到 `ramfs` 的测试兼容 alias 设计成 VFS framework 能力；它只能停留在 syscall adapter，并带退出条件。
- 不承诺 `MS_NOEXEC`、`MS_NODEV`、`MS_NOSUID` 的跨 exec/devfs/credentials 接入；`MS_NOSYMFOLLOW` 可作为后续靠近 namei 的小阶段。
- 不承诺普通 `MS_REMOUNT` 的 sb-wide reconfigure；第一版只把当前 mount view 的
  `RDONLY` 切换作为 accepted limitation 处理，其它需要 filesystem instance
  reconfigure 的 remount data / option / attr 必须稳定拒绝并打日志。
- 不承诺 readonly remount 与 shared writable mmap、dirty writeback、`msync` 的 Linux 级强一致性。

## 文档地图

Canonical：

- [不变量需求](./invariants.md)
- [迁移实施计划](./implementation.md)
- [tracking issues](./tracking-issues.md)

背景材料：

- [背景材料索引](./backgrounds/index.md)
- [mount LTP 与 Linux 参考调查](./backgrounds/ltp-linux-reference-20260604.md)

## 设计公式

本 RFC 第一版采用以下已定公式：

- Legacy mount API 优先；new mount API 只保留功能余地，不提前暴露接口抽象。
- `NameSpace` 正名为 `MountTree`；真正 namespace 系统后续独立 RFC。
- `PathRef { mount, dentry }` 是长期位置表示，对齐 Linux `struct path` 的核心形状。
- bind mount 创建新的 `Mount` view，复用原 `SuperBlock`，source root 可以是任意目录 dentry。
- unmount 是 mount view 生命周期操作，不是 superblock 生命周期操作。
- readonly remount 首批只承诺 per-mount enforcement，不承诺 superblock reconfigure。
- syscall mount flags 采用严格 allowlist；暂不支持的可观察语义稳定拒绝并打日志。
- fstype compatibility alias 是 syscall 边界的临时兼容桥，不参与 mount tree 状态、不改变 backend source 语义，也不能作为 data parser fallback。
- mount op flags、per-mount attrs、superblock state 和 filesystem type flags 必须类型分离。
- 现有 `MountFlags` 只允许作为阶段迁移桥；阶段 3 attr plumbing 关闭后，`Mount` 直接持有 `MountAttrFlags` 或等价 attrs storage，`FileSystemOps::mount()` 不再接收 per-mount attrs。
- 同一 mountpoint 支持 stack，lookup 进入 Linux-like topmost mount。
- `MountTree` 普通 topology writer 使用 `tx_lock: Mutex<()>` 串行化；placement state 在 `inner: SpinLock<MountTreeInner>` 内短临界区发布/读取。early anonymous root 只能通过显式 fs-private boot API 发布，不得用 `can_sleep`、IRQ/preempt 状态或 panic 状态推断绕过 writer gate。
- 第一版 topology 发布采用单一 placement lock 加 `placement_generation` retry：move 在同一 transaction 内修改旧 stack 与新 stack，lookup 通过短临界区读取 stack，并在 generation 变化时重试。
- per-mount attrs 第一版由 `Mount` 上的 atomic bitset 作为单一真相源；remount 在确认目标 mount view 仍 attached 且仍是当前 `MountTree` 目标 view 后发布 attrs。
- unmount 的 view detach、observer cleanup 和 superblock resident inode eviction 必须分层；inode cache 回收服从已接受的 inode-shrinker 显式 eviction 协议。
- 如果 Linux 语义代价不高，优先对齐 Linux；若代价高或依赖后续子系统，则稳定拒绝、记录限制并保留后续 gate。

## 方案

### 状态模型

`PathRef` 继续表示一个具体位置：`Arc<Mount>` 加 `Arc<Dentry>`。它不是临时兼容层，也不应被中央 graph handle 替换。

`Mount` object identity 由 `Arc<Mount>` 表示。它的 root dentry 和 superblock 定义这个 view 暴露的对象；per-mount attributes 是 `Mount` 上的 atomic state；placement state（root / attached / detached、parent、mountpoint、child/stack 链接）由 `MountTree` 在 placement lock 下发布。`Mount` 字段可以缓存 placement，但不能成为绕过 `MountTree` 的第二套拓扑真相源。

`MountTree` 是唯一的拓扑 owner。attach、detach、bind、recursive bind、move、remount、stack-top 更新和 lazy detach 都必须通过 `MountTree` transaction。这个模型类似 task topology 的集中修改权：对象身份可以被外部持有，拓扑变更只能由 owner 线性化。

`SuperBlock` 表示 filesystem instance。bind mount 复用同一个 superblock；unmount 某个 mount view 不等于 kill superblock。只有最后一个使用该 superblock 的 mount view 被摘除、filesystem lifetime 允许回收、且 resident inode cache 按 inode-shrinker RFC 的显式 eviction 协议清理成功后，mount 层才可以调用 filesystem `kill_sb`。`KERNEL_FS` / `PERSISTENT_SB` 这类 lifetime 边界仍归 filesystem capability flag 表达。

### Flag 和参数分层

legacy `mount(2)` 的 raw flags 先解析成临时操作请求：

- `MountOpFlags`：`BIND`、`MOVE`、`REMOUNT`、`REC`、`PRIVATE` 等操作选择位，不长期存入 `Mount`。
- `MountAttrFlags`：`RDONLY` 等 per-mount view 属性，存在 `Mount` 上。
- `SuperBlockState` / future `SuperBlockFlags`：真正 sb-wide readonly、reconfigure、dirty/sync 等 filesystem instance 状态，第一版只定义边界。
- `FileSystemFlags` 继续只描述 filesystem type 能力和生命周期策略，如 `KERNEL_FS`、`PERSISTENT_SB`，不得混用为 mount 或 superblock 当前状态。

legacy `data` 参数由 syscall 层读取为可选 `MountData`，VFS 不解释 filesystem 私有 option。filesystem 后端可以接受、拒绝或兼容具体 data；不支持且有可见语义的 option 必须稳定失败并打日志。

阶段迁移期间可以短暂把 `MountAttrFlags::RDONLY` 映射到既有 `MountFlags::RDONLY` 以复用旧 call path，但这不是 accepted final state。阶段 3 关闭时必须删除 `MountFlags` 作为并列 per-mount flag 类型，且 filesystem backend 不得通过 `FileSystemOps::mount()` 观察 per-mount attrs；backend 只处理 source、legacy `MountData` 和未来明确属于 superblock / filesystem instance 的配置。

fstype normalization 在 syscall adapter 内完成。`tmpfs -> ramfs` 这类 alias 可以作为明确的兼容入口；`ext2` / `ext3` / `vfat -> ramfs` 属于当前 LTP 兼容桥，必须有日志、文档化边界和退出条件。归一化后的内部请求不得让 `MountTree`、`MountAttrFlags`、`FileSystemOps::mount()` 或具体 filesystem backend 知道原始 alias；过渡期存在的 `MountFlags` 也不得观察原始 alias，并必须随阶段 3 attr plumbing 删除。

`data` 中的 `loop`、普通 image file 自动绑定等 util-linux 行为不由内核 mount parser 伪造。用户态应先通过 loop ioctl 得到 `/dev/loopN`，再以 block-device source 调用普通 `mount(2)`。

### Legacy transactions

`NewMount` 创建或复用 superblock，然后在 `MountTree` transaction 内重验 target 仍 attached、仍是当前路径对应的目标 view，再 attach 新 mount view。superblock 创建和 filesystem data parser 不应在树锁内执行。

`BindMount` 创建新的 mount view，复用 source mount 的 superblock，root 是 source path 的 dentry。plain bind 不递归克隆 child mounts。进入 transaction 后必须重验 source mount 仍 attached、target 仍 attached 且仍是当前目标 view；失败不得插入新 view 或返回成功。

`RecursiveBind` 克隆 source subtree 的 mount views。首批不支持成功创建 unbindable mount；后续支持 `UNBINDABLE` 后，rbind 必须按 Linux 规则跳过 unbindable subtree。source subtree snapshot / retry 策略属于阶段 4 反馈假设：实现必须证明失败不会留下半棵可见 subtree，若真实接口需要改变 snapshot contract，必须回写本 RFC。

`MoveMount` 移动同一个 mount subtree，不 clone、不创建新 superblock。它在同一个 placement transaction 内重验 source/target、防环、从旧 stack 摘除并插入新 stack；成功返回后同一个 `Arc<Mount>` identity 从新位置可见。首批只支持 private tree 下的基础 move，并拒绝会产生拓扑环或依赖 shared/slave propagation 的情况。

`Remount` 首批只更新 per-mount attributes，重点是 `RDONLY`。发布 attrs 前必须在同一个 placement transaction 内重验目标 `PathRef` 的 mount view 仍 attached，且仍是当前 `MountTree` 中该路径对应的目标 view；attrs store 必须在该 transaction 释放前完成。旧 `PathRef` 指向 detached view 时不能被 remount 偶然改 attrs 后返回成功。`MS_REMOUNT | MS_BIND` 只影响目标 bind view；普通 `MS_REMOUNT` 可以走同一套 per-mount readonly 切换，但不得声称完成 Linux sb-wide reconfigure。

`ChangePropagation` 首批有限接受 `MS_PRIVATE` / `MS_REC | MS_PRIVATE`。`MS_SHARED`、`MS_SLAVE`、`MS_UNBINDABLE` 在 peer group / master-slave 状态闭合前稳定拒绝并打日志。

`Unmount` 默认执行同步 detach 和 busy 判断；`MNT_DETACH` 执行 lazy detach；`UMOUNT_NOFOLLOW` 影响 target 最后一跳 lookup；`MNT_FORCE` 和 `MNT_EXPIRE` 暂缓。`MNT_EXPIRE` 与 `MNT_DETACH` / `MNT_FORCE` 的非法组合仍应按 Linux/LTP 期望返回 `EINVAL`。

unmount transaction 还必须给 mount / filesystem identity consumer 留出明确 hook。fanotify 当前已经把 mount mark 和 filesystem mark 建模为 `Arc<Mount>` / `Arc<SuperBlock>` identity；完整 umount 兼容必须在 view detach 或 final superblock kill 前后接入 pre-unmount flush / mark-dead，确保 late enqueue 观察 dead target 后 fail closed。没有该 hook 时，mount RFC 不能声称 fanotify mark 生命周期完整闭合。

### `/proc/mounts`

当前 procfs 发布形状是 `/proc/mounts -> self/mounts` symlink；第一版 live view 应落在 `/proc/<tgid>/mounts` 的内容入口，`/proc/mounts` 继续复用 `self` symlink。该视图由 `MountTree` snapshot 生成，输出 source、target、fstype、options、dump/pass 六列。options 至少准确反映 `ro/rw` 和已闭合的 mount attrs；未支持 flag 不得出现在 options 中伪装成功。

路径渲染必须固定为 task fs-root 视角：`/proc/self/mounts` 使用读取任务的 root；`/proc/<tgid>/mounts` 使用被绑定 task 的 root。若某个 mountpoint 不能在该 root 下表达，第一版应省略或稳定标记为不可表达，不能 fallback 泄露全局路径。

第一版不承诺 `/proc/self/mountinfo`、mount id、parent id、propagation tag 或 per-task namespace 视图。

## 接受边界

本 RFC 已接受进入实现阶段，并且其目录即为当前共享评审和 implementation 的 canonical source。接受本 RFC 意味着 legacy mount API 可以按 [迁移实施计划](./implementation.md) 分阶段推进，并且每个成功返回的用户可见操作都必须满足 [不变量需求](./invariants.md)。执行事实、checkpoint、review 结论和验证证据记录在 [事务日志](../../devlog/transactions/2026-06-18-mount-tree-legacy-api.md)。

本 RFC 允许把少量受控不确定性带入实现反馈阶段，但反馈只能优化路线，不能削弱目标、不变量或验收边界。lazy-detach final cleanup 和 detached / moved `PathRef` 的 namei 细节必须按 [Probe / Vertical Slice Gates](./implementation.md#probe--vertical-slice-gates) 验证；若验证失败，应回写 RFC canonical 文本、登记 limitation / open issue，或停止当前 gate，而不是让弱语义静默成为实现结果。

以下变化必须回到本 RFC 或新增 follow-up RFC：

- 改变 `PathRef`、`Mount`、`MountTree`、`SuperBlock` 的状态所有权。
- 把 readonly remount 从 per-mount enforcement 升级为 sb-wide reconfigure。
- 引入真正 mount namespace、nsproxy、`CLONE_NEWNS` 或 namespace fd。
- 支持 shared/slave propagation 的完整 peer group / master-slave 传播。
- 支持 new mount API 或 detached mount fd。
- 支持 file bind mount、`pivot_root(2)`、idmapped mount 或 user namespace mount 权限。
- 让 syscall fstype alias bridge 下沉为 mount tree / backend 状态。
- 改变 unmount 与 inode-shrinker显式 eviction 协议、或绕过 fanotify 等 mount identity consumer 的 pre-unmount cleanup 边界。

## 备选方案

### 继续在 `sys_mount()` 中特判 flag

拒绝。它会把 bind、move、remount、propagation 和 data parser 混成不可复审路径，也会继续制造 unsupported flag 假成功。

### 第一版直接引入 Linux-like `MountNamespace` / `NsProxy`

拒绝。当前问题是 legacy mount tree transaction 未正式化；真正 namespace 系统涉及 task clone/unshare、root/cwd、procfs namespace fd、propagation 和权限，需要单独 RFC。

### 为 new mount API 提前设计 detached mount object

拒绝。当前只在功能上不堵死 new mount API；提前暴露接口形状容易形成错误抽象，后续反而难以实现 `fsopen()` / `fsconfig()` 状态机。

### 把 bind mount 做成复用同一个 `Mount`

拒绝。`parent`、`mountpoint`、`children` 和 per-mount attrs 都是 attached-view 状态。复用同一个 `Mount` 会让 topology、readonly 和 unmount 生命周期混在一起。

### 将 readonly remount 写入 superblock

拒绝作为第一版方案。`fs_readonly` 依赖 sibling bind mount 的 ro/rw 隔离；把 `RDONLY` 做成 superblock 全局状态会污染同一 superblock 的其他 mount view。

## 风险

- Mount stack 需要从当前 first-visible 改成 Linux-like topmost-visible；既有 KUnit 应同步修改，避免保留实现副作用。
- `MountTree` transaction 需要清晰区分内存对象一致性和拓扑一致性；lazy detach 后旧 `Arc<Mount>` 仍可能存活。
- `/proc/mounts` 顺序和路径显示会影响 LTP cleanup；实现前需要按 Linux/LTP 期望确认输出顺序、escaping 边界和 task root 视角。
- exact errno matrix 仍需实现前对照 LTP/Linux，尤其是 invalid flag combination、non-directory bind、already-busy unmount 和 unsupported data option。
- 普通 `MS_REMOUNT` 第一版只承诺 per-mount `RDONLY` 子集；如果实现接受需要
  sb-wide reconfigure 的 remount option / data / attr，就会把 limitation 变成
  用户可见假成功。
- 当前 readonly enforcement 不覆盖 shared writable mmap 和 dirty writeback，需要在 register 或 RFC risk 中保持明确限制。
- 当前 syscall fstype alias bridge 可能掩盖真实 filesystem support；实现阶段必须把 alias 命中与真实 filesystem mount 失败区分记录。
- fanotify mount/filesystem mark 会 pin `Mount` / `SuperBlock` identity；没有 pre-unmount cleanup hook 时，sync unmount / lazy detach / final kill 都不能宣称完整通知子系统兼容。

## 收口

进入实现前，应完成一次文档层 review，确认：

- `index.md`、`invariants.md`、`implementation.md` 对状态所有权、锁边界和 staged gates 一致。
- `tracking-issues.md` 只保留仍影响实现顺序或验收的开放问题。
- `current-limitations` 中与 readonly mmap/writeback、full mount namespace、new mount API 相关的限制不被本 RFC 偷偷声明为已解决。
