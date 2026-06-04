# IOCTL 与 Loop 设备迁移实施计划

**状态：** Draft
**最后更新：** 2026-06-04
**父 RFC：** [RFC-20260603-IOCTL-LOOP](./index.md)
**不变量：** [不变量需求](./invariants.md)

本文按可提交、可验证的阶段拆分。每个阶段完成后应能独立构建，并给出最小验证证据。

## 迁移原则

- ioctl ABI 常量、结构体布局和用户指针读写只放在 syscall 或设备文件 ABI 边界。
- 设备长期状态使用 Anemone 内部类型，不直接保存 Linux UAPI 结构体。
- `sys_ioctl()` 不积累设备私有分支，除兼容已有 `FIONREAD` 外应通过打开文件对象分发。
- ioctl 分发必须显式传递目标 fd 的打开描述符能力快照；VFS/设备层不得接收 `FileDesc`、`ProcFile`、当前 task 或文件表对象，也不得自行回到当前 task 文件表重新解释 syscall 参数。
- loop 设备必须同时是 devfs 节点和真实 `BlockDev`，但 `/dev` 上仍走统一 block file-op 路径，避免 mount、stat、`BLK*` 和 `LOOP_*` 看到不同对象。
- 第一阶段不发布 `/dev/loop-control`；若后续发布该节点，必须同阶段实现 `LOOP_CTL_GET_FREE` 并接入同一份 loop 设备池。
- 每个阶段都保持 `just build` 可通过；功能阶段必须给出最小用户态或 LTP 风格验证证据。
- 暂缓项返回稳定 unsupported 或 busy 错误，不用空实现伪装成成功。
- loop 第一阶段不修改 mount 层来解析 `-o loop`，只提供用户态 mount 工具所需的 loop 设备协议。

## 阶段 0：UAPI 常量与结构准备

目标：把 Linux ioctl 需要的常量和结构体放在 ABI 边界，不把 magic number 散落到驱动实现里。

实现内容：

- 在 `anemone-abi` 中补充块设备 ioctl 常量：
  - `BLKGETSIZE`
  - `BLKGETSIZE64`
  - `BLKSSZGET`
- 补充 loop ioctl 常量：
  - `LOOP_SET_FD`
  - `LOOP_CLR_FD`
  - `LOOP_SET_STATUS`
  - `LOOP_GET_STATUS`
  - `LOOP_SET_STATUS64`
  - `LOOP_GET_STATUS64`
  - `LOOP_CHANGE_FD`
  - `LOOP_SET_CAPACITY`
  - `LOOP_SET_DIRECT_IO`
  - `LOOP_SET_BLOCK_SIZE`
  - `LOOP_CONFIGURE`
  - `LOOP_CTL_GET_FREE`
- 补充 `loop_info`、`loop_info64`、`loop_config` 的 `repr(C)` 结构体。
- 为 loop flags 建立内部转换 helper，不在 loop 状态对象里直接保存 Linux 结构体。
- `LOOP_CTL_GET_FREE` 只作为 UAPI 常量保留；第一阶段不发布 `/dev/loop-control`，因此没有控制设备 ioctl 成功路径。

验收条件：

- 结构体大小、字段顺序与 Linux UAPI 对齐。
- 当前已有 `FIONREAD` 行为不退化。
- 还不要求任何新 ioctl 成功。

## 阶段 1：VFS ioctl 分发

目标：让 ioctl 从 syscall 层进入打开文件对象，而不是继续在 `sys_ioctl()` 中写全局分支。

实现内容：

- 给 `FileOps` 增加 `ioctl` 方法：
  - 参数为 `&File` 加短生命周期 `IoctlCtx` 或等价结构，不是裸 `cmd` + `arg`，也不是 `Arc<FileDesc>`。
  - `IoctlCtx` 是薄请求参数包，至少包含 `cmd`、`arg`、目标 fd 访问能力快照、用户空间访问 handle、受控的 arg-fd lookup helper；不得实现成 ioctl 专用 fd wrapper。
  - 目标 fd 的 `OpenAccessMode`、`O_PATH` 和 `FileStatusFlags` 在 `sys_ioctl()` 中转换为值语义快照后进入上下文；上下文类型不得依赖 `task::files::FileDesc`。
  - 如果第一阶段需要权限输入，只能传不可变、最小化的 credential/capability snapshot；不得传完整 task 或允许设备代码重新获取 current-task 权限上下文。
  - 默认实现返回适合 Linux ioctl 的“不支持该 fd 的 ioctl”错误，例如 `ENOTTY`。
- `sys_ioctl()`：
  - 获取 fd 对应的 `FileDesc`。
  - 对 `O_PATH` 目标 fd 返回 `EBADF`，不把 path-only handle 交给设备实现。
  - 保留或迁移 `FIONREAD` 行为，确保 pipe 现有用例不退化。
  - 从 `FileDesc` 派生 access snapshot 后丢弃 task/fd 层对象，构造 `IoctlCtx` 后对其他命令调用 `file.ioctl(ctx)`。
- 为所有现有 `FileOps` 补默认 ioctl 方法。
- 用户指针读写使用现有 `UserSpaceHandle` / `usp` 和 `user_access` API；不要新增 ioctl 专用 `copy_from_user` / `copy_to_user` helper，避免制造与其他 syscall 模块不一致的封装层。
- `IoctlCtx::get_arg_fd()` 或等价 helper 只用于命令定义要求 arg 是 fd 的场景，例如 `LOOP_SET_FD`。设备实现可以看到原始 `arg`，但 fd-table lookup 只能通过该 helper 完成；helper 返回窄化后的 `IoctlArgFile` / `BackingFileHandle` 和访问能力快照，不允许设备实现保存原始 fd number、`FileDesc` 或文件表引用。

验收条件：

- `just build` 通过。
- `FIONREAD` 现有 pipe 行为保持。
- 对普通文件、目录、procfs 文件执行未知 ioctl 返回稳定错误，不再返回 `ENOSYS`。

审查重点：

- 不要让 `FileOps` 直接依赖某个具体设备子系统。
- 不要让设备实现调用 `get_current_task().get_fd(raw_arg)` 来弥补上下文缺口。
- 不要把 `task::files::{FileDesc, ProcFile, FilesState}` 引入 `fs::file` 或设备 file ops。
- 不要把 `FileDesc` 作为简化参数直接下沉到设备层；它包含 task/files 层的 fd-local flags、共享 status flags 和访问包装，loop 绑定需要的是经 ioctl 边界窄化后的 backing file handle。
- 不要把 Linux ABI 结构体保存进长期内核状态。
- errno 要区分“命令不适用于该 fd”、“设备空闲”、“参数非法”和“功能暂缓”。

## 阶段 2：通用块设备 ioctl

目标：让 `/dev/vda`、`/dev/ram0` 这类现有块设备具备最小 Linux 块设备查询能力。

实现内容：

- 在块设备 devfs 文件操作中实现：
  - `BLKGETSIZE64`：返回设备总字节数。
  - `BLKGETSIZE`：返回 512 字节扇区数。
  - `BLKSSZGET`：返回逻辑块大小。
- 继续使用统一的 block devfs file ops，例如当前 `BLOCK_DEV_FILE_OPS`。所有块设备节点的 read/write、seek 和 ioctl 都从这条路径进入。
- 在 block 子系统内新增私有 ioctl 扩展点，例如 `BlockDev::ioctl(BlockIoctlCtx)` 默认实现，或独立 `BlockIoctlOps`。通用 `BLK*` 命令先由 block devfs file ops 处理；未匹配的命令再委托给具体 block device 的私有 hook。
- 继续保持当前块设备 read/write 的块对齐约束。
- 对非查询型块设备 ioctl 返回明确 unsupported 错误。

验收条件：

- 打开真实块设备后，`blockdev --getsize64` 或等价小测能读到非零大小。
- LTP `tst_get_device_size()` 依赖的 `BLKGETSIZE64` 能工作。
- 不影响 ext4 从 virtio-blk 启动和挂载。
- `/dev/vda`、`/dev/ram0`、`/dev/loopN` 不出现彼此分叉的 devfs file-op 行为；未知私有 ioctl 经 block hook 默认返回 `ENOTTY`。

## 阶段 3：loop 块设备池

目标：创建静态 loop 设备池，让 `/dev/loop0..loopN` 成为可注册、可 stat、可打开的块设备。

实现内容：

- 新增 `anemone-kernel/src/device/block/loop.rs`。
- 该文件是 loop 作为 VFS-backed block bridge 的显式例外：loop 可以依赖由 ioctl 边界生成的窄 `BackingFileHandle`，普通物理块设备不得因此依赖 VFS 打开文件对象或 task fd 状态。
- 启动期静态创建一组 loop 设备。第一阶段数量可固定，例如 8 或 16。
- 每个 loop 设备注册为 `BlockDevClass::Loop`，并发布到 devfs。
- 第一阶段只发布 `/dev/loop0..loopN`，不发布 `/dev/loop-control`。
- loop 状态至少包含：
  - `id`
  - `devnum`
  - `Unbound | Bound`
  - `Bound` 内的 backing file handle
  - offset
  - size limit
  - readonly
  - block size
  - display name
  - loop flags
- `BlockDev::block_size()` 第一阶段固定为 512 字节，以满足当前 ext4 mount 限制。
- `read_blocks()` / `write_blocks()`：
  - 空闲设备返回设备未配置类错误。
  - 绑定后按 `offset + block_idx * block_size` 读写 backing file。
  - readonly 状态下写入返回 `EROFS`。
  - 超过 size limit 或 backing file 可见大小时返回短设备边界错误，而不是越界访问。

验收条件：

- `/dev/loop0` 可见且是块设备。
- `/dev/loop-control` 不存在；用户态 discovery 不会被半发布控制节点遮蔽。
- 空闲 `/dev/loop0` 打开成功，但读写和 mount 不应伪成功。
- 绑定状态下可以被 `MountSource::Block` 找到。

审查重点：

- loop 状态发布前必须完整初始化。
- `/dev/loopN` 不发布 loop 专属 file ops；它必须复用统一 block devfs file ops，并只通过 block private ioctl hook 接收 `LOOP_*`。
- 状态锁不能在持有时调用可能回到 VFS 的长路径 I/O，避免锁反转。
- backing file lifetime 必须独立于用户态传入 fd 的关闭。

## 阶段 4：loop ioctl 第一阶段

目标：覆盖 LTP `tst_device acquire` 和常见 `mount -o loop` 路径所需的最小 loop ioctl。

实现内容：

- `LOOP_GET_STATUS`：
  - 空闲设备返回 `ENXIO`。
  - 已绑定设备返回 legacy `loop_info`。
- `LOOP_GET_STATUS64`：
  - 空闲设备返回 `ENXIO`。
  - 已绑定设备返回 `loop_info64`。
- `LOOP_SET_FD`：
  - arg 解释为 backing file fd。
  - 通过 `IoctlCtx` 的受控 arg-fd lookup helper 解析 backing fd，不保存 raw fd number、`FileDesc` 或文件表引用。
  - 目标 loop fd 不能是 `O_PATH`，并且需要具备后续绑定语义所需的写能力；只读 loop 绑定只允许进入 readonly 状态。
  - backing fd 必须可读；如果 loop 绑定不是 readonly，还需要 backing fd 可写。
  - 设备已绑定时返回 `EBUSY`。
  - 记录 readonly 状态。
- loop 私有 ioctl：
  - 不在 `sys_ioctl()` 中分支，也不在 `/dev/loopN` 上注册专属 `FileOps`。
  - 统一 block devfs file ops 先处理通用 `BLK*`，未匹配命令通过 block private ioctl hook 委托到 loop 设备。
- `LOOP_SET_STATUS` / `LOOP_SET_STATUS64`：
  - 只接受当前阶段支持的字段：name、offset、sizelimit、readonly、具备最小释放语义的 autoclear。
  - `LO_FLAGS_PARTSCAN`、`LO_FLAGS_DIRECT_IO` 和未知 flag bit 返回 `EINVAL`，不能记录成状态。
  - 不支持的加密字段、crypt name、init data 必须返回 `EINVAL`，不能静默污染状态。
  - 所有字段先完整校验，再一次性提交到 `Bound` 状态。
- `LOOP_CLR_FD`：
  - 已空闲时返回 `ENXIO`。
  - 若设备仍被挂载或有活跃文件系统引用，返回 `EBUSY`。
  - 成功后清空绑定状态。
- `LOOP_SET_DIRECT_IO`：
  - 第一阶段返回稳定 unsupported，不改变状态。
- `LOOP_CONFIGURE`：
  - 第一阶段可以暂缓。若暂缓，应返回让 LTP 判断为 unsupported 的稳定错误，不要半初始化。
  - 若实现，必须复用 `LOOP_SET_FD` + `LOOP_SET_STATUS64` 的完整校验和 flag 策略，不能出现 configure 专属半成功路径。
- `/dev/loop-control` / `LOOP_CTL_GET_FREE`：
  - 第一阶段不实现，因为控制节点不发布。
  - 后续阶段若实现，必须从同一个 loop 设备池返回空闲编号，并加入 discovery 验收。

验收条件：

- LTP `tst_find_free_loopdev()` 能发现空闲 loop 设备。
- LTP `tst_attach_device()` 的 `LOOP_SET_FD` + `LOOP_SET_STATUS` 能成功。
- `tst_device acquire` 返回 `/dev/loopN`，`tst_device release` 能释放。
- 设置 `LO_FLAGS_PARTSCAN` 或 `LO_FLAGS_DIRECT_IO` 不会成功污染状态。
- 设置 `LO_FLAGS_AUTOCLEAR` 后，如果实现了引用下降释放 hook，则 busy 解除后能释放；否则该 bit 必须被拒绝。

## 阶段 5：loop mount 闭环验证

目标：证明 loop 设备已经能服务 mount/mkfs 类测试设备需求。

最小验证：

- 创建普通文件作为 backing image。
- 绑定到 `/dev/loop0`。
- 对 `/dev/loop0` 执行 `BLKGETSIZE64`。
- 用可用 mkfs 工具格式化为当前内核支持的文件系统。
- `mount -t ext4 /dev/loop0 <mnt>` 成功。
- 在挂载点创建、读取、删除普通文件。
- umount 后 `LOOP_CLR_FD` 成功。

LTP 验证建议：

- 先跑依赖 `.needs_device` 但语义简单的用例。
- 将 `LTP_DEV_FS_TYPE` 显式设为当前内核能 mount 的文件系统，例如 `ext4`。LTP 默认 `ext2`，当前内核若不支持 ext2，会把 loop 闭环误报成文件系统不支持。
- 再引入 `mount01`、`umount01`、`ioctl04` 这类基础用例。
- 最后再评估 `ioctl_loop01..07`，其中 sysfs、partscan、direct_io、configure 相关分支可以作为后续项。

## 阶段 6：mount option 后续分支

目标：在 loop 闭环稳定后，单独推进 mount option 语义。

后续内容：

- `MS_REMOUNT` 修改已有 mount flags。
- `MS_BIND` / `MS_MOVE` 的 mount tree 语义。
- `MS_NODEV` 拒绝打开设备特殊文件。
- `MS_NOEXEC` 拒绝从该 mount 执行文件。
- `MS_NOSUID` 影响 suid/sgid 执行语义。
- `MS_NOATIME` / `MS_NODIRATIME` / `MS_STRICTATIME` 影响 atime 更新。
- `statfs.f_flags` 报告 mount flags。
- `/proc/self/mounts` 和 `/proc/mounts` 输出真实 mount 表。

这阶段不应和 loop 第一阶段混在一起提交。

## 旁路审计清单

实现过程中至少检查：

```text
rg -n "sys_ioctl|FIONREAD" anemone-kernel/src
rg -n "trait FileOps|impl .*FileOps" anemone-kernel/src
rg -n "BlockDev|BlockDevClass|DeviceId::Block" anemone-kernel/src
rg -n "MountSource::Block|sys_mount|MS_" anemone-kernel/src
rg -n "read_at|write_at" anemone-kernel/src
```

每个命中至少分类为：

- syscall ABI 分发入口。
- 普通文件或 VFS 默认行为。
- 块设备通用行为。
- loop 私有行为。
- mount source 解析和 mount flag 语义。
- 观察路径，例如 `stat`、`statfs`、`/proc/mounts`。
- 暂缓项或不属于第一阶段的旁路。

## 可观测性清单

实现过程中必须能从验证日志、debug 记录或断言中回答：

- `ioctl` 命令最终由哪个打开文件对象处理。
- 未支持命令返回的是 `ENOTTY` / `EINVAL` / `ENOSYS` 中哪一种，以及为什么。
- loop 设备处于 `Unbound` 还是 `Bound`，对应 backing file name、offset、sizelimit、readonly 和 block size 是什么。
- `LOOP_SET_FD` 成功后 backing file 是否独立于用户态 fd 关闭继续存活。
- `LOOP_CLR_FD` 失败时原因是空闲、仍 busy、参数错误，还是暂缓语义。
- `BLKGETSIZE64` 暴露的容量如何由 backing file size、offset、sizelimit 和 block size 计算。
- mount 失败时失败点是在 loop 绑定、mkfs、block source lookup、ext4 mount，还是 mount flag / filesystem type 支持范围。

## 停止边界

迁移期间继续追查的情况：

- 改动会改变 ioctl ABI 分发边界或把设备私有协议塞回 `sys_ioctl()`。
- 改动会改变 loop backing file 生命周期、busy 判断、状态线性化点或锁序。
- 改动会让 devfs 节点、块设备注册表和 mount source lookup 看到不同 loop 对象。
- 改动会把第一阶段明确暂缓的 sysfs、partscan、direct I/O 或完整 mount flag 语义伪装成已支持。
- LTP 失败无法归类为 loop/ioctl、文件系统类型、mount flags、sysfs 或测试环境问题。

可以停止实现形状争论的情况：

- 实现路径不同，但保持 `sys_ioctl()` 到 `FileOps::ioctl` 的分发边界。
- loop 状态类型命名不同，但满足不变量中的身份、状态所有权、生命周期和线性化规则。
- 某个 ioctl 暂缓，但返回值足以让 LTP 和用户态工具稳定识别为 unsupported。
- 问题属于后续 mount option、sysfs、分区扫描或 direct I/O 范围，且不会阻塞 loop mount 最小闭环。
