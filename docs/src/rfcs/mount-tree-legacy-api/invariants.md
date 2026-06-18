# mount tree legacy API 不变量需求

**状态：** Draft
**最后更新：** 2026-06-18
**父 RFC：** [RFC-20260604-mount-tree-legacy-api](./index.md)

## 闭合条件

- `PathRef = Mount + Dentry` 是唯一正式的位置表示；不得为 mount work 引入第二套 path identity。
- `MountTree` 是 mount topology 的唯一写侧 owner。
- `Mount` 是 attached view；bind/rbind/move/remount/unmount 处理的是 mount view，不直接等同于 superblock 生命周期。
- legacy `mount(2)`、`umount(2)`、`umount2(2)` 都通过同一套内部 mount transaction 修改拓扑或 per-mount attributes。
- syscall raw flags 必须分为 operation flags、per-mount attributes、future superblock state 和 filesystem type flags；不得继续把所有 `MS_*` 混在 `MountFlags` 里。
- syscall fstype compatibility alias 只能存在于 syscall adapter；它不是 `MountTree`、`Mount`、`SuperBlock` 或 filesystem backend 的状态。
- 暂不支持且有用户可观察语义的 flag、operation、data option 或组合必须稳定拒绝并打日志。
- `RDONLY` 首批是 per-mount enforcement，不是 sb-wide readonly 或 filesystem reconfigure。
- 同一 mountpoint 的 stack 语义必须是 Linux-like topmost-visible。
- 不完整 feature 不能返回成功。

## 非目标

- 本 RFC 不证明完整 Linux mount namespace、`NsProxy`、`CLONE_NEWNS` 或 namespace fd。
- 本 RFC 不证明完整 shared/slave propagation。
- 本 RFC 不证明 new mount API、detached mount fd 或 filesystem context fd。
- 本 RFC 不证明 `pivot_root(2)`。
- 本 RFC 不证明 readonly remount 与 shared writable mmap / dirty writeback / `msync` 的强一致性。

## 状态所有权

### `PathRef`

`PathRef` 持有 `Arc<Mount>` 和 `Arc<Dentry>`，表示一个具体位置。它对齐 Linux `struct path` 的核心形状，是长期设计，不是第一版权宜方案。

`PathRef` 不拥有拓扑修改权。持有旧 `PathRef` 只能保证内存对象仍然存活，不能保证该 mount view 仍从当前 `MountTree` 可达。

### `Mount`

`Mount` object identity 由 `Arc<Mount>` 表示，bind/rbind/move/remount/unmount 的可见语义都必须围绕这个 identity 判断。`Mount` 的 root dentry 和 superblock 定义它暴露的 view；placement state 只描述这个 identity 当前是否以及如何挂在 `MountTree` 上。

`Mount` placement 必须能区分三种状态：

- `Root`：当前 tree 的 root mount，没有 parent / mountpoint。
- `Attached`：挂在某个 parent mount 的某个 mountpoint stack 上。
- `Detached`：已从当前 tree 摘除，但 `Arc<Mount>` 仍可能因旧 `PathRef`、fd 或 observer mark 存活。

`parent == None` 不得同时表达 root 和 detached。若实现阶段暂时保留旧字段形状，必须用 `MountTree` 的 attachment registry 或等价状态消除该二义性。

`Mount` view 包含：

- `root` 是这个 view 暴露的根 dentry。bind mount 的 root 可以是 source mount 下的任意目录 dentry。
- `sb` 是 backing superblock。多个 mount view 可以共享同一个 superblock。
- `attrs` 是 per-mount attributes，如首批 `RDONLY`，第一版由 `Mount` 上的 atomic bitset / interior-mutable attrs 承载。
- `parent`、`mountpoint`、`children` 和 stack 关系是 `MountTree` 发布的 placement state。

`Mount` 可以保存 parent/child/stack 关系，但这些字段只能是 `MountTree` placement state 的表达或缓存。修改这些关系的接口只能由 `MountTree` transaction 调用。普通 syscall handler、filesystem 后端、inode/dentry 逻辑不得绕过 `MountTree` 改 mount topology，也不得把 `Mount` 局部字段当成独立真相源。

### `MountTree`

`MountTree` 是当前 visible mount tree owner。当前实现中的 `NameSpace` 应正名为该对象；它不是 Linux mount namespace。

`MountTree` 负责：

- root mount 记录；
- mount list / attach order；
- mountpoint stack 顺序；
- placement state 和 `placement_generation`；
- attach、detach、lazy detach、move、bind、rbind、remount；
- `/proc/mounts` snapshot；
- observer pre-unmount hook 的调用顺序；
- 后续 propagation 分发的唯一入口。

初始阶段所有 task 仍共享同一个 visible `MountTree`。真正的 per-task mount namespace、namespace 复制和 nsproxy 风格聚合由后续 namespace RFC 设计。

### `SuperBlock`

`SuperBlock` 是 filesystem instance。它拥有 inode cache、backing source、filesystem private state 和 future sb-wide reconfigure state。

unmount 当前 mount view 不等于 kill superblock。只有当没有任何 mount view 使用该 superblock，且 filesystem type 允许回收时，mount 层才可以进入 superblock cleanup。resident inode cache eviction 不由 mount tree RFC 重新定义；它必须调用已接受 inode-shrinker 协议中的显式 eviction path（`try_evict_inode()` / `try_evict_all()` 等价入口），保持 busy recheck、sync-before-remove、cache-lock removal 和 failure rollback 顺序。`FileSystemFlags::{KERNEL_FS, PERSISTENT_SB, SHRINKABLE_ICACHE}` 继续约束 filesystem lifetime 和 cache 回收能力。

### Flags

- `MountOpFlags` 是 syscall parser 的临时操作位，不能存入 `Mount` 作为长期状态。
- `MountAttrFlags` 是 per-mount view 状态，存在 `Mount` 上。第一版使用 atomic bitset / interior-mutable attrs 作为单一真相源；remount 以 release-store 或等价同步发布，所有用户可见写入口以 acquire-load 或等价同步从当前 `PathRef.mount()` 读取。
- `SuperBlockState` / future `SuperBlockFlags` 是 superblock 实例状态，不能用来替代 per-mount attributes。
- `FileSystemFlags` 是 filesystem type 属性，不得表达某次 mount 或某个 superblock 当前状态。
- `FileStatusFlags` / `FileOpStatusFlags` 是 opened file description 与 per-operation I/O ctx 的状态，不得承载 mount readonly、noexec、nodev、nosuid 或 fstype alias 语义。
- fstype alias、`-o loop` 等用户态兼容处理不是 mount attrs、不是 superblock flags，也不是 filesystem type capability；允许的 alias 必须停在 syscall adapter，并在进入 VFS transaction 前被归一化或拒绝。

## 身份与能力模型

`Mount` object identity 由 `Arc<Mount>` 表示。两个 bind view 即使共享同一个 `SuperBlock` 和 root dentry，也必须是不同 `Mount` 对象。

比较路径位置时，`PathRef` 的 mount 和 dentry 都参与位置身份。仅比较 inode 或 dentry 不能区分 sibling bind mount 的不同 per-mount attributes。

若实现 `/proc/mounts` 或 stack ordering 需要 mount id / attach sequence，该 id 只能由 `MountTree` 分配。除非后续 RFC 明确提升为 ABI，它只服务诊断、排序和 snapshot，不得反向驱动拓扑状态机。

fanotify mount mark 使用 `Arc<Mount>` identity，filesystem mark 使用 `Arc<SuperBlock>` identity。`MoveMount` 必须保留被移动 subtree 的 mount identity；`BindMount` / `RecursiveBind` 创建的新 view 是新的 `Mount` identity，不得自动继承 source mount mark，除非后续 fanotify RFC 明确改写该语义。lazy detach 后旧 identity 可以继续被已有 mark 或 event 引用，但必须通过 pre-unmount cleanup / mark-dead 防止新事件把 detached 或 killed target 当成 live target。

## 线性化点

所有 topology-changing operation 的线性化点都在 `MountTree` transaction 内：

- `NewMount`：superblock 已在 transaction 外准备好；进入 transaction 后必须重验 target 仍 attached 且仍是当前目标 view。attach 成功后新 mount view 对 lookup 可见。
- `BindMount`：进入 transaction 后必须重验 source mount 仍 attached、target 仍 attached 且仍是当前目标 view；新 mount view 被插入目标 mountpoint stack 后成功。
- `RecursiveBind`：整棵 clone subtree 全部 attach 完成后成功；失败不得留下半棵可见 subtree。source subtree snapshot / retry 策略可由阶段 4 反馈 gate 用真实实现验证，但不能削弱“全有或全无可见性”和“锁内重验 source/target”的不变量。
- `MoveMount`：source subtree 从旧位置 detach 并 attach 到新位置作为一个 transaction。该 transaction 持有同一把 placement lock，必须在锁内重验 source 仍 attached、target 仍是当前目标 view、防止移动到自身 subtree、从旧 mountpoint stack 摘除 source、插入新 mountpoint stack，并在成功路径最后递增 `placement_generation`。外部 lookup 只能稳定观察到 move 前或 move 后；不能观察到半移动状态。
- `Remount`：发布 attrs 前必须在 `MountTree` transaction 内重验目标 `PathRef.mount()` 仍 attached，且仍是当前 `MountTree` 中该路径对应的目标 view。重验失败不得更新 attrs，也不得返回成功；具体 errno 在实现阶段按 Linux/LTP matrix 收敛。重验通过后，必须在仍持有 placement lock 时发布目标 mount view attrs，并在成功返回前完成发布；不改 sibling view。
- `Unmount`：sync detach 或 lazy detach 从 tree 中摘掉目标 view/subtree 后成功；旧引用存活属于内存对象一致性，不属于当前 tree 可达性。若目标或其 superblock 可能被 fanotify 等 observer 持有 mark，必须在宣称完整兼容前调用明确的 pre-unmount cleanup / mark-dead hook。
- `ChangePropagation`：首批 private 状态变更在 transaction 内完成；shared/slave/unbindable 未闭合前不得成功。

syscall handler 可以在线性化点前完成用户指针读取、权限检查、flag/data 解析、source/target path lookup、filesystem mount/data parser 和 superblock 创建。进入 `MountTree` transaction 后，不应执行可能递归进入 VFS lookup 或 filesystem mount I/O 的操作。

## 锁序与生命周期规则

### 锁类型

`MountTree` 写侧 transaction 使用睡眠式 `Mutex<MountTreeInner>`，第一版 topology 发布机制固定为单一 placement lock，而不是 COW / snapshot-style publish。当前 repo 的 `RwLock` 是 spin-based，不适合作为可能扩展的 mount topology transaction lock。

`MountTreeInner` 维护 mountpoint stack、attachment registry 和 `placement_generation`。所有 attach、detach、lazy detach、move、stack-top 更新和 remount target revalidation 都在同一把 placement lock 下线性化。move 不能先发布旧 stack 摘除、再异步发布新 stack 插入；旧 stack 和新 stack 的修改必须在同一个 transaction 中完成。

读侧 lookup 不应长期持有全局 tree lock。它捕获 `placement_generation`，通过窄 API 在短临界区读取 mountpoint stack / 栈顶 mount；若一次 path walk 期间 generation 变化，lookup 必须重试或返回可审计的 retry outcome。成功返回的 `PathRef` 必须来自同一 placement generation。若后续 `/proc/mounts` 或 propagation 需要稳定全树 snapshot，应提供显式 snapshot API，而不是让普通 lookup 进入长时间全局读锁。

### 锁序

推荐顺序：

1. 用户指针读取、raw flag/data parse。
2. 权限检查。
3. path lookup / source resolution。
4. filesystem data parser 和 superblock 创建或复用。
5. `MountTree` transaction lock。
6. transaction 内部短暂修改 `Mount` child/stack/list 结构。

禁止在持有 `MountTree` transaction lock 时执行：

- 用户内存访问；
- 可能阻塞的 block/filesystem mount I/O；
- 复杂 path lookup；
- 递归调用 legacy mount/unmount syscall path；
- filesystem 后端私自回调 mount topology mutation。
- fstype alias / data compatibility fallback 不能进入 transaction lock 内临时判定；这些都属于 syscall adapter 前置解析。

### 内存对象一致性 vs 拓扑一致性

内存对象一致性由 `Arc<Mount>`、`Arc<Dentry>`、`Arc<SuperBlock>` 保证。对象只要仍被引用，就不能被释放。

拓扑一致性由 `MountTree` 保证。某个 `Mount` 是否从当前 root 可达、某个 mountpoint 的栈顶是谁、move/unmount/remount target revalidation 是否已经生效，只能由 `MountTree` transaction 决定。

lazy detach 后，旧 `PathRef` 可以继续持有 detached `Mount`。这不表示该 mount 仍在 `/proc/mounts` 或新 lookup 可见，也不表示 observer subsystem 可以继续把它当成 live target 产生新事件。

detached 或 moved `PathRef` 的 namei 细节进入阶段 6 的受控反馈 gate，但有一个不可削弱边界：从 detached root / cwd / old root 出发的 `..`、relative lookup 或 root crossing 不能静默 fallback 到当前全局 root，也不能依赖已经失效的 stale parent。实现必须在 gate 中选择“停在 retained root / stable error / RFC 回写后扩展语义”之一，并用 targeted KUnit 或 smoke 证明该选择。

### Busy 和 superblock lifetime

sync `umount` 必须拒绝仍有 child mount 的目标 view。若目标 view 是某 superblock 的最后一个 mount view，还必须按当前 filesystem inode/file/path 引用策略判断 superblock 是否 busy，并通过显式 inode eviction path 清理可回收 resident cache。mount 层不得在 `Drop`、任意引用释放路径或持 `MountTree` transaction lock 的长临界区内执行 filesystem sync / evict / kill。

bind mount 场景下，同一 superblock 的 sibling mount 不应使当前 view 的 unmount 误判 busy。superblock kill 只在没有任何 mount view 使用它时考虑。

`MNT_DETACH` 从 topology 摘除目标 subtree，但不强制 kill superblock 或撤销已有 `Arc` 引用。final superblock cleanup 的调度 owner 必须通过阶段 6 反馈 gate 证明；在该 gate 关闭前，只能声明 topology detach 已闭合，不能声明 lazy-detach 后最终 `kill_sb` / observer cleanup 已完整兼容。

## 禁止退化项

- 不得继续把 `MS_BIND`、`MS_MOVE`、`MS_REMOUNT`、propagation flags 当作 ignored flags 后返回成功。
- 不得把 bind mount 实现为重新 mount 一个新 superblock。
- 不得把 readonly bind remount 写入 superblock 或污染 sibling mount。
- 不得复用同一个 `Mount` object 挂到多个 parent/mountpoint。
- 不得用 `parent == None` 同时表达 root 和 detached。
- 不得把 move 拆成读侧可见的旧 stack 摘除和新 stack 插入两次发布。
- 不得在 remount 目标已经 detached 或不再是当前目标 view 时更新 attrs 后返回成功。
- 不得让 syscall handler 或 filesystem backend 直接改 mount topology。
- 不得在没有 namespace RFC 的情况下把当前 `MountTree` 伪装成 Linux mount namespace。
- 不得在没有 new mount API RFC 的情况下提前暴露 detached mount fd 或 fs context fd。
- 不得在未支持 `MS_NOEXEC`、`MS_NODEV`、`MS_NOSUID` 时返回成功并让用户态误判能力。
- 不得把 LTP fstype alias bridge 写入 `MountTree`、`Mount`、`SuperBlock`、`FileSystemFlags` 或 filesystem backend。
- 不得在 mount parser 内伪造 util-linux `-o loop` 自动绑定普通 image file。
- 不得让 `FileStatusFlags` / `FileOpStatusFlags` 反向驱动 mount attrs。
- 不得在 fanotify mount/filesystem mark 仍认为 target live 时宣称完整 unmount / filesystem kill 兼容。

## 完成标准

- RFC text 明确 legacy mount API 第一版范围和所有暂缓能力。
- `MountTree`、`Mount`、`PathRef`、`SuperBlock`、flags 和 data parser 的状态所有权闭合。
- 每个 implementation stage 都有 write set、可观测性、验证和退出条件。
- 不完整 feature 的 syscall 返回稳定 errno，并带可定位日志。
- `fs_readonly` 所需 per-mount readonly 行为不污染 sibling bind mount。
- `/proc/mounts -> self/mounts` 和 `/proc/<tgid>/mounts` 足以支撑 LTP cleanup，路径渲染使用明确 task root 视角，且不声称 mountinfo / namespace 语义。
- unmount cleanup 与 inode-shrinker eviction、fanotify mount/filesystem mark lifecycle 的边界可审计。
- 允许带入实现的反馈 gate 都有受保护目标、失败信号、验证 floor、回写位置和退出路径；反馈不能把必须满足的不变量降级成建议项。
