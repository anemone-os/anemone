# RFC-20260603-IOCTL-LOOP

**状态：** Draft，已提升为公开 RFC 草案
**负责人：** doruche, Codex
**最后更新：** 2026-06-04
**领域：** syscall ABI / VFS / devfs / block device / mount / LTP
**事务日志：** [IOCTL Loop 事务日志](../../devlog/transactions/2026-06-04-ioctl-loop.md)
**开放问题：** None；design review 发现的 Keter/Euclid 项已吸收到本文、[不变量需求](./invariants.md) 和 [迁移实施计划](./implementation.md)，历史记录见 [Tracking Issues](./tracking-issues.md)。
**下一步：** 按 [Agent 编排建议](./backgrounds/agent-orchestration.md) 执行 Agent 0 只读前置审计；通过后进入 [迁移实施计划](./implementation.md) 的 UAPI 与 VFS ioctl 分发阶段。

## 摘要

本 RFC 记录 `ioctl(2)` 基础设施与 loop 设备的实现边界，目标是支撑 LTP 中依赖测试块设备、`losetup`/loop ioctl，以及 `mount -o loop` 的用例。

第一阶段不是完整补齐所有 Linux ioctl，而是建立一条可维护的 ioctl 分发路径，并让 loop 设备成为 VFS、devfs、块设备和 mount 之间的真实连接点。

## 背景

LTP 中大量 `.needs_device` 用例依赖测试框架创建临时块设备。Linux 用户态通常通过 loop 设备把普通 image 文件绑定成块设备，再对该块设备执行 mkfs、mount、umount 和释放动作。如果内核缺少 loop ioctl、块设备 size ioctl 或 `/dev/loopN`，这些用例会失败在测试基础设施阶段，而不是失败在目标 syscall 语义上。

Anemone 当前已有 mount、devfs、块设备注册和 ext4 block mount 的基础路径，但 `ioctl(2)` 还没有 VFS 级分发，loop 设备池也不存在。因此这个 RFC 的核心是先补齐设备协议入口，而不是把 `mount -o loop` 当成 kernel mount option 特判。

## 目标

需要达成的外部行为：

- `/dev/loop0` 等 loop 块设备存在，`stat` 能看到块设备类型和稳定 `rdev`。
- 空闲 loop 设备对 `LOOP_GET_STATUS` 返回 `ENXIO`，从而让 LTP 的 `tst_device acquire` 能发现空闲设备。
- `LOOP_SET_FD` 能把普通文件绑定成块设备，`LOOP_CLR_FD` 能释放绑定。
- 基本块设备 ioctl 至少覆盖 `BLKGETSIZE64`、`BLKGETSIZE`、`BLKSSZGET`，支撑 `blockdev`、mkfs、LTP 设备大小检查。
- `mount -t ext4 /dev/loopN <mnt>` 能通过当前 `MountSource::Block` 路径挂载。
- 用户态 `mount -o loop file.img <mnt>` 通过 mount 工具转换成 loop 绑定后能落到上述路径。
- 第一阶段不发布半成品 `/dev/loop-control`；loop discovery 走 `/dev/loopN` + `LOOP_GET_STATUS*` 老式扫描路径。后续一旦发布 `/dev/loop-control`，必须同时实现 `LOOP_CTL_GET_FREE`，并从同一份 loop 设备池返回空闲编号。

## 非目标

以下内容不属于 loop/ioctl 第一阶段闭环：

- 完整 mount option 语义，例如 `MS_BIND`、`MS_MOVE`、`MS_REMOUNT`、`MS_NODEV`、`MS_NOEXEC`、`MS_NOSUID`、`MS_NOATIME`。
- `statfs.f_flags`、`/proc/self/mounts`、`/proc/mounts` 的完整可观测性。
- loop 分区扫描、uevent、sysfs `/sys/block/loop*`、`LO_FLAGS_PARTSCAN` 的真实分区设备生成。
- `LO_FLAGS_DIRECT_IO` 的真实 direct I/O 语义。
- 加密 loop、autoclear 超出 mount/release 最小闭环的完整 close-last-fd 语义、loop resize 的所有边界。
- 新 mount API，例如 `fsopen`、`fsconfig`、`fsmount`、`move_mount`、`mount_setattr`。

这些项目属于后续范围或当前限制；只有当草案内部出现边界冲突、证明缺口或错误的成功语义时，才进入 `tracking-issues.md`。第一阶段不得用空实现伪装成已支持。

## 文档地图

Canonical：

- [不变量需求](./invariants.md)
- [迁移实施计划](./implementation.md)
- [Tracking Issues](./tracking-issues.md)

背景材料：

- [背景材料索引](./backgrounds/index.md)
- [LTP IOCTL 测例覆盖面](./backgrounds/ltp-ioctl-coverage.md)
- [Agent 编排建议](./backgrounds/agent-orchestration.md)

## 方案

`sys_ioctl()` 只负责 fd 查找、用户指针 ABI 边界和最外层兼容。它可以直接读取调用者的 `FileDesc` 和当前用户空间 handle，但不能把 `FileDesc`、`ProcFile`、当前 task 或文件表对象继续传入 VFS/设备层；分发前必须把这些事实压缩成短生命周期、值语义的 `IoctlCtx`，再交给打开文件的 `FileOps::ioctl`。`IoctlCtx` 是薄请求参数包，不是 ioctl 专用 fd 封装；它只携带目标 fd 的访问能力快照、用户空间访问 handle，以及受控的 fd 参数解析能力，供 `LOOP_SET_FD` 这类 ioctl 使用。

块设备 `/dev` 文件操作保持单一路径。现有 block devfs file ops 负责块设备 read/write、通用 `BLK*` 查询命令，并在通用命令不匹配时调用块设备子系统拥有的私有 ioctl hook。loop 设备在块设备子系统中注册真实 `BlockDev`，loop 私有 ioctl 作为该 hook 的实现接入；`/dev/loopN` 不另起一套绕过通用 block `/dev` 行为的 file ops。

loop 设备持有 backing file 的长期内核句柄，将普通文件的 `read_at` / `write_at` 转换成块设备 `read_blocks` / `write_blocks`。mount 层继续只理解块设备 source；用户态 `mount -o loop` 仍由 mount 工具转换为 loop 绑定加普通 `mount(2)`。

## 当前基础设施判断

已有基础：

- VFS 已有 `mount(2)` 入口，并能把 source path 解析为 `DeviceId::Block` 后构造 `MountSource::Block`。
- 块设备子系统已有 `BlockDev`、块设备注册表、`BlockDevClass::Loop` 命名空间，以及 devfs 发布路径。
- devfs 已转向发布层模型，叶子节点可以由设备侧决定 `open` 后的 `FileOps`。
- ext4 mount 已经接受任意 `BlockDev`，只要求块大小为 512 字节。
- 普通文件对象已有 `read_at` / `write_at`，loop 后端可以持有 backing file 并做偏移换算。

主要缺口：

- `sys_ioctl()` 当前只有 `FIONREAD` 特判，没有通用文件或设备分发。
- `FileOps` 没有 `ioctl` 方法，设备无法在打开文件后接收命令。
- 通用块设备文件没有 `BLK*` ioctl。
- loop 设备不存在，`/dev/loop*` 和 loop 状态机都不存在。
- mount data/options 当前被忽略，mount flags 只处理 `MS_RDONLY`。
- `statfs()` 和 `/proc/self/mounts` 仍是弱实现，会影响 mount flag 类 LTP，但不是 loop 第一阶段阻塞项。

## 设计边界

### ioctl 边界

Linux ioctl 常量、结构体布局、用户指针读写应集中在 syscall 或设备文件 ABI 边界。`FileOps::ioctl` 可以按现有 vtable 风格接收 `&File`，但不能接收裸 `cmd`/`arg` 后自行回到当前 task 文件表，也不能接收 `Arc<FileDesc>` 或其他 task/fd 层对象；它接收由 `sys_ioctl()` 构造的薄 `IoctlCtx`：

- `cmd` / `arg` 原始 ABI 值。
- `target_access` 值语义快照，用于表达目标 fd 是否 path-only、可读、可写，以及 ioctl 需要的 file status flags。该类型归 syscall/VFS ioctl 边界所有，不依赖 `task::files::FileDesc`。
- 用户空间访问 handle，例如当前 syscall 模块惯用的 `UserSpaceHandle` / `usp`；ioctl 代码使用现有 `user_access` API 读写用户态 `loop_info`、`loop_info64`、`loop_config` 和块设备查询输出，不新增 ioctl 专用 `copy_from_user` / `copy_to_user` 封装。
- 受控的 arg-fd 解析 helper，用于 `LOOP_SET_FD` 从调用者文件表取得 backing file。设备实现可以按命令语义知道 `arg` 是 fd number，但 fd-table lookup 只能通过该 helper 完成；helper 只返回窄化后的 `IoctlArgFile` / `BackingFileHandle` 与访问能力快照，不暴露 `FileDesc` 或文件表，也不允许设备保存 raw fd number 作为长期状态。
- 必要时的不可变 credential/capability 快照；第一阶段不传完整 task，也不允许设备代码重新获取 current-task 权限上下文。

目标 fd 的通用打开模式错误，例如 `O_PATH` 对设备 ioctl 的 `EBADF`，应在 `sys_ioctl()` 或统一 helper 中处理；设备实现只处理自身类型语义。核心设备状态不要直接保存 Linux UAPI 结构体，而应转换成 Anemone 内部状态：

- loop number
- backing file handle
- byte offset
- size limit
- logical block size
- readonly/autoclear/partscan/direct_io 等内部 flags
- backing file display name

不直接传递 `FileDesc` 的理由不是设备层不能理解 fd 参数，而是 fd number、fd-table lookup 和可保存的 backing file handle 是三层不同语义。fd number 是调用者文件表中的一次性索引；`FileDesc` 是 task/files 层对象，携带 fd-local flags、共享 status flags mutator 和访问包装；loop 成功绑定后需要的是独立延长生命周期的 backing file handle。若设备层接收 `FileDesc` 或自行回到 `current_task` 文件表解析 `arg`，就会把 syscall fd 表策略下沉到 VFS/block 设备层，并让后续 close/reuse fd、mount/block I/O 路径和 errno 归类变得不稳定。

### loop-control 发布边界

第一阶段不发布 `/dev/loop-control`，即使 `LOOP_CTL_GET_FREE` 常量已经在 ABI 层保留。这样 LTP 和 BusyBox/util-linux 的 discovery 可以稳定回退到扫描 `/dev/loopN` 并对空闲设备执行 `LOOP_GET_STATUS*`，不会被一个存在但不可用的控制节点遮蔽。

后续若发布 `/dev/loop-control`，它必须是同一份 loop 设备池的控制入口，且至少满足：

- `LOOP_CTL_GET_FREE` 返回当前最低可用或策略定义的空闲 loop number。
- 返回结果与 `/dev/loopN` 的 `Unbound` / `Bound` 状态一致。
- 没有空闲设备时返回稳定错误，不能返回一个随后无法绑定的编号。
- 发布该节点的同一阶段必须加入 discovery 验收，不能只补 devfs 节点。

### loop flags 第一阶段策略

第一阶段 `LOOP_SET_STATUS*`、`LOOP_GET_STATUS*` 和后续可能的 `LOOP_CONFIGURE` 必须按下表处理 flags。任何未列出的 flag bit 都返回 `EINVAL`，不得写入 loop 状态。

| flag / 字段 | 第一阶段策略 | 对外语义 |
| --- | --- | --- |
| `LO_FLAGS_READ_ONLY` | 支持，作为内部 readonly 状态暴露；只读来源包括目标 loop fd/backing fd 能力和用户显式设置。 | 写入返回 `EROFS`；`GET_STATUS*` 返回该 bit。 |
| `LO_FLAGS_AUTOCLEAR` | 支持最小 mount/release 闭环语义；设置后，当设备不再被挂载或块设备层引用时执行等价 `LOOP_CLR_FD` 的延迟释放。 | 可以记录并返回该 bit；若当前阶段没有 busy 引用下降后的释放 hook，则 `SET_STATUS*` 必须拒绝该 bit，不能伪成功。 |
| `LO_FLAGS_PARTSCAN` | 第一阶段不支持。 | `SET_STATUS*` / `LOOP_CONFIGURE` 中出现该 bit 返回 `EINVAL`；不生成 `/dev/loopNpM`，不记录该 bit。 |
| `LO_FLAGS_DIRECT_IO` | 第一阶段不支持。 | `LOOP_SET_DIRECT_IO` 返回稳定 unsupported；`SET_STATUS*` / `LOOP_CONFIGURE` 中出现该 bit 返回 `EINVAL`。 |
| encryption / crypt name / init data | 第一阶段不支持。 | 非空或非 none 字段返回 `EINVAL`；不得污染内部状态。 |
| offset / sizelimit / file name | 支持。 | 提交前完整校验，成功后由 `GET_STATUS*` 反映。 |

### VFS 与设备边界

`sys_ioctl()` 应只负责取 fd、做最外层兼容分发，然后交给打开文件的 `FileOps::ioctl`。普通文件、pipe、procfs、字符设备、块设备都通过同一个 VFS 入口接入。

块设备默认 `/dev` 文件行为由块设备子系统统一维护。`BLOCK_DEV_FILE_OPS` 或等价单一路径处理所有块设备节点的 read/write、seek、通用 `BLK*` ioctl 和私有 ioctl 分发。私有 ioctl 通过块设备子系统拥有的扩展点接入，例如 `BlockDev::ioctl(BlockIoctlCtx)` 或独立 `BlockIoctlOps`，默认返回 `ENOTTY`。

loop 设备不得为 `/dev/loopN` 发布另一套 file ops 来绕过通用 block `/dev` 行为。它只能注册为 `BlockDevClass::Loop`，并通过 block 子系统的私有 ioctl hook 实现 `LOOP_*`；mount、stat、read/write、`BLK*` 和 `LOOP_*` 都必须解析到同一个 block device 对象。

### loop 设备边界

loop 设备是文件到块设备的适配层。它不应反向修改 VFS mount 逻辑，也不应让 mount 层理解 `-o loop`。用户态 mount 工具负责把 `-o loop` 转换为：

1. 找到空闲 `/dev/loopN`。
2. 打开 backing file。
3. 对 `/dev/loopN` 执行 `LOOP_SET_FD` 和状态设置 ioctl。
4. 调用 `mount(2)`，source 为 `/dev/loopN`。

因此内核第一阶段只要提供真实可用的 loop 块设备和 ioctl 协议即可。

loop 位于块设备子系统下是一个明确的 VFS-backed bridge 例外：loop 可以持有由 ioctl 边界创建的窄 `BackingFileHandle`，并在锁外调用普通文件的 `read_at` / `write_at`。这个依赖不能推广到普通物理块设备；virtio、SCSI、ramdisk 等驱动不得依赖 VFS 打开文件对象或 task fd 状态。

## 接受边界

本 RFC 被接受意味着 ioctl 分发、块设备 size ioctl、loop 设备池和 loop 基础 ioctl 可以作为一个 staged feature 推进，目标是先闭合 LTP 测试设备基础设施和 `mount -o loop` 的用户态路径。

以下变化必须回到本 RFC 或新增 follow-up RFC：

- 改变 `sys_ioctl()` 与 `FileOps::ioctl` 的 ABI 分发边界。
- 让 mount 层直接理解 `-o loop` 或直接解析普通 image 文件。
- 改变 loop backing file 的生命周期所有权、busy 判断或状态线性化点。
- 把 sysfs、分区扫描、direct I/O 或完整 mount flag 语义提前并入第一阶段验收。

## 备选方案

- 在 `sys_mount()` 中特判普通文件 image：拒绝。Linux `mount -o loop` 的常见路径是用户态工具完成 loop 绑定，内核直接解析 image 会绕过 loop ioctl、块设备身份和 LTP `tst_device` 发现路径。
- 只伪造 `/dev/loopN` 节点：拒绝。LTP 和 mount 工具会通过 `LOOP_GET_STATUS`、`LOOP_SET_FD`、`BLKGETSIZE64` 等 ioctl 验证设备行为，节点存在但协议不可用会制造更难分类的假进展。
- 先补 mount flags：延期。`MS_NODEV`、`MS_NOEXEC`、`MS_REMOUNT` 等属于 VFS mount 语义，不是 loop 闭环的最小阻塞项。

## 风险

- errno 不兼容会让 LTP 把 unsupported、busy 和 empty loop 误分类。控制方式是在不变量和实施计划中固定 `ENOTTY`、`ENXIO`、`EINVAL`、`EBUSY`、`EROFS` 等边界；只有发现草案内部冲突时才升级为 tracking issue。
- loop 状态锁包住 backing file I/O 可能引入 VFS 与块设备锁反转。控制方式是在不变量中要求锁内只复制 `Bound` 快照，实际 I/O 在锁外执行。
- backing file 生命周期如果只保存 fd number，会在用户态关闭 fd 后变成悬空引用。控制方式是 loop 状态持有独立内核文件句柄。
- LTP 默认 `ext2` 可能掩盖 loop 进展。控制方式是在 loop 验证阶段显式设置 `LTP_DEV_FS_TYPE=ext4`，直到文件系统支持范围扩大。

## 收口

完成后需要在事务日志中记录：

- VFS ioctl 分发和 `FIONREAD` 回归验证结果。
- 块设备 `BLKGETSIZE64` / `BLKGETSIZE` / `BLKSSZGET` 验证结果。
- `/dev/loopN` 发现、绑定、mkfs、mount、umount、release 的最小闭环日志。
- LTP `.needs_device` 用例的通过/失败分类，以及仍属于 mount flags、sysfs、partscan、direct I/O 或文件系统类型支持的剩余限制。
