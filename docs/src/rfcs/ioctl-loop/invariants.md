# IOCTL 与 Loop 设备不变量需求

**状态：** Draft
**最后更新：** 2026-06-04
**父 RFC：** [RFC-20260603-IOCTL-LOOP](./index.md)

## 闭合条件

- `ioctl(2)` 从 fd 到打开文件对象的分发路径闭合，设备私有命令不继续散落在 `sys_ioctl()` 全局分支中。
- loop 设备同时具备稳定 devfs 节点、块设备注册身份和 loop 私有 ioctl 状态，三者指向同一个设备对象。
- loop discovery 不被半发布的控制节点遮蔽；第一阶段不发布 `/dev/loop-control`，后续发布时必须绑定 `LOOP_CTL_GET_FREE` 和同一份 loop 设备池。
- ioctl 分发上下文显式传递目标打开描述符能力快照、当前用户空间访问 handle 和受控的 arg-fd 查找能力；VFS/设备层不得接收 `FileDesc`、`ProcFile`、当前 task 或文件表对象。
- 块设备 `/dev` file ops 保持单一路径：通用 `BLK*` 由 block 子系统处理，loop 私有 ioctl 只能通过 block 子系统拥有的私有 hook 接入。
- 绑定成功后，loop backing file 的生命周期独立于用户态传入 fd 的后续关闭。
- 空闲、绑定、busy、unsupported、参数非法和只读写入等结果用稳定 errno 表达。
- loop flag 必须按第一阶段支持表校验；unsupported flag 不能返回成功并出现在后续 `GET_STATUS*` 结果中。
- loop 第一阶段能完成 image file 到 block device 到 ext4 mount 的最小闭环。

## 非目标

- 不证明完整 Linux ioctl 覆盖率。
- 不证明完整 mount option、mount propagation 或新 mount API 语义。
- 不证明 sysfs `/sys/block/loop*`、分区扫描、direct I/O、加密 loop 或 autoclear close-last-fd 的完整语义。
- 不要求 loop 设备支持动态扩容、热插拔或 uevent。

## 状态所有权

`sys_ioctl()` 拥有 fd 查找、最外层命令号兼容和用户指针 ABI 边界；打开文件对象的 `FileOps::ioctl` 拥有命令分发后的文件类型语义。

`FileOps::ioctl` 的输入必须来自 `sys_ioctl()` 构造的短生命周期上下文。该上下文至少能表达：

- 目标 fd 的值语义能力快照，包括 `OpenAccessMode`、`O_PATH` 和 file status flags。
- 用户空间访问 handle，例如当前 syscall 模块惯用的 `UserSpaceHandle` / `usp`；具体读写继续使用现有 `user_access` API，不新增 ioctl 专用 copy helper。
- 对 arg-fd 的受控 lookup helper，用于 `LOOP_SET_FD` 这类协议；返回值必须是窄化后的 backing file handle 和能力快照。
- 必要时的不可变、最小 credential/capability snapshot。

设备实现可以按 ioctl 命令语义知道某个 `arg` 是 fd number，但不得保存原始 fd number，也不得自行调用当前 task 文件表来重新解释 syscall 参数。`FileDesc`、`ProcFile`、`FilesState` 和完整 task 对象不能跨过 syscall/VFS ioctl 边界进入 `FileOps` 或设备私有 ioctl。`O_PATH`、目标 fd 不可读写、backing fd 不可读写等 fd capability 错误必须在统一上下文规则下表达为稳定 errno。

该限制不是为了隐藏 fd 参数，而是为了区分一次性 fd number、当前 task fd-table lookup 和可被设备长期保存的 file handle。`FileDesc` 属于 task/files 层，包含 fd-local flags、共享 status flags mutator 和 syscall 访问包装；loop 绑定成功后只能保存经 helper 窄化并延长生命周期的 backing file handle，不能把调用者文件表或 `FileDesc` 变成设备状态的一部分。

块设备子系统拥有通用块设备查询语义，例如 `BLKGETSIZE64`、`BLKGETSIZE` 和 `BLKSSZGET`。loop 设备拥有 loop 私有状态机和 loop ioctl 语义，但必须通过同一个 `BlockDev` 对象暴露给 mount 层。

块设备 devfs file ops 是所有 `/dev/<block>` 节点的统一入口。它先处理通用 block read/write/seek 和 `BLK*` ioctl；未匹配的 ioctl 再通过 block 子系统定义的 private hook 委托给具体 `BlockDev`。loop 不得为 `/dev/loopN` 发布专属 file ops，也不得绕过通用 block devfs 行为。

loop 设备状态的单一真相源是每个 loop device 的内部状态对象。devfs 节点只发布入口，mount source lookup 只解析块设备身份，二者不能复制或缓存另一套 loop 绑定状态。

## 身份与能力模型

loop 设备身份由 block device id 和 loop number 共同确定。`/dev/loop0`、块设备注册表中的 `BlockDevClass::Loop` 项，以及 mount 层看到的 `DeviceId::Block` 必须解析到同一个设备实例。

第一阶段的 loop discovery 身份只通过 `/dev/loopN` 暴露。`/dev/loop-control` 若在后续阶段发布，必须是同一份 loop 设备池的控制视图；`LOOP_CTL_GET_FREE` 返回的编号必须与对应 `/dev/loopN` 的 `Unbound` 状态一致，不能发布只存在 devfs 节点而没有可用控制 ioctl 的半设备。

`LOOP_SET_FD` 的 arg 是调用者文件表中的 fd number，只能作为一次性输入能力。绑定成功后，loop 状态必须保存能延长 backing file 生命周期的内核句柄，不能保存 fd number，也不能依赖调用者 task 或文件表继续存在。

loop 的 backing file handle 是一个明确的 VFS-backed block bridge 例外。只有 loop 可以持有由 ioctl 边界创建的窄 backing file handle，并在锁外调用普通文件 I/O；virtio、SCSI、ramdisk 等物理或内存块设备不得依赖 VFS 打开文件对象或 task fd 状态。

用户态传入的 `loop_info`、`loop_info64`、`loop_config` 是 ABI 数据包，不是内部状态身份。内部状态应保存规范化后的 offset、sizelimit、block size、readonly、flags 和 display name。

## loop flag 支持边界

第一阶段仅支持能被最小 loop mount 闭环真实兑现的 flag：

- `LO_FLAGS_READ_ONLY`：支持；写路径返回 `EROFS`，`GET_STATUS*` 可观测。
- `LO_FLAGS_AUTOCLEAR`：只有在实现 busy 引用下降后的延迟释放 hook 时才支持；否则 `SET_STATUS*` 必须拒绝该 bit。
- `LO_FLAGS_PARTSCAN`：不支持；`SET_STATUS*` / `LOOP_CONFIGURE` 遇到该 bit 返回 `EINVAL`，不得记录。
- `LO_FLAGS_DIRECT_IO`：不支持；`LOOP_SET_DIRECT_IO` 返回稳定 unsupported，状态输入中出现该 bit 返回 `EINVAL`。
- 加密字段、crypt name、init data、未知 flag bit：不支持；返回 `EINVAL`，不得污染内部状态。

`GET_STATUS*` 只能返回已经具备上述最小语义的 bit。保存一个 unsupported bit 后再返回给用户态，视为违反不变量。

## 线性化点

`LOOP_SET_FD` 的线性化点是设备从 `Unbound` 转为 `Bound` 的状态提交。提交前不得让 read/write、BLK ioctl 或 mount 看到半初始化 backing file；提交后即使用户态关闭原 fd，设备也必须继续保持 backing file 存活。

`LOOP_SET_STATUS` / `LOOP_SET_STATUS64` 的线性化点是状态字段更新提交。更新前必须验证 offset、sizelimit、flags 和 unsupported 字段；不能先写入一部分再因后续字段失败返回错误。

`LOOP_CLR_FD` 的线性化点是设备从 `Bound` 转为 `Unbound` 的状态提交。若设备仍被挂载或存在活跃块设备引用，必须在线性化前返回 `EBUSY`，不能先清空 backing file。

通用 `BLKGETSIZE64` / `BLKGETSIZE` / `BLKSSZGET` 的线性化点是读取设备容量快照。对 loop 设备，容量快照必须来自同一份 `Bound` 状态，不能混合更新前后的 offset、sizelimit 和 block size。

## 锁序与生命周期规则

loop 状态锁只保护 `Unbound` / `Bound` 状态和字段快照。`read_blocks()` / `write_blocks()` 不得在持有 loop 状态锁时调用 backing file 的长路径 VFS I/O；必须先复制 backing file 句柄和必要参数，释放状态锁后再执行 I/O。

backing file 句柄的释放责任属于 loop 状态转换。`LOOP_CLR_FD` 成功后释放旧 backing file；失败时不得改变当前 backing file 生命周期。

块设备注册、devfs 发布和 mount source lookup 不得分别持有互相等待的锁后再调用 loop 状态转换。若需要 busy 判断，应优先通过短生命周期引用计数或块设备层状态查询完成。

所有容量计算必须使用 checked arithmetic。offset 超过 backing file size、sizelimit 非零、文件大小非 block size 对齐和 32-bit sector count 溢出都必须有明确错误或截断策略。

## 禁止退化项

- 在 `sys_ioctl()` 中长期维护 loop、block、tty、socket 等设备私有命令的大分支。
- 让 mount 层直接打开普通文件 image 来模拟 `-o loop`。
- 发布 `/dev/loop-control` 但不实现同源的 `LOOP_CTL_GET_FREE`。
- 让设备实现通过当前 task 文件表重新解析 ioctl arg fd。
- 把 `FileDesc`、`ProcFile`、`FilesState` 或完整 task/capability 对象传给 `FileOps::ioctl` 或 block private ioctl。
- 为 `/dev/loopN` 发布绕过统一 block devfs file ops 的专属 file ops。
- 让非 loop 块设备依赖 VFS backing file handle 或 task fd 状态。
- 保存用户态 fd number 作为 loop backing file 的长期引用。
- 空闲 loop 设备对 `LOOP_GET_STATUS` 返回成功。
- 对 unsupported loop 功能返回成功但不生效，例如 partscan、direct I/O、configure、autoclear 或加密字段。
- 在 loop 状态锁内执行 backing file read/write。
- 让 devfs 节点存在但无法通过块设备注册表被 mount source lookup 找到。

## 完成标准

- VFS ioctl 分发、通用块设备 ioctl、loop 设备池和 loop 基础 ioctl 都有独立构建与最小验证证据。
- `/dev/loopN` 空闲时能被 LTP loop discovery 识别，绑定后能通过 `BLKGETSIZE64` 暴露容量。
- image file 绑定 loop 后可以完成 mkfs、`mount -t ext4 /dev/loopN <mnt>`、文件读写、umount 和 `LOOP_CLR_FD`。
- tracking issues 中剩余项目能明确归类为 sysfs、partscan、direct I/O、mount option、procfs/statfs 可观测性或文件系统类型支持，不再阻塞 loop mount 最小闭环。
