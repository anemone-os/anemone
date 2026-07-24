# IOCTL 与 Loop 设备 Tracking Issues

**状态：** Closed；无 active design issue
**最后更新：** 2026-07-22
**父 RFC：** [RFC-20260603-IOCTL-LOOP](./index.md)
**事务日志：** [IOCTL Loop 事务日志](../../devlog/transactions/2026-06-04-ioctl-loop.md)

本文只跟踪 design review 后确认的 RFC 草案缺陷、证明缺口、边界冲突或需要回到草案修改的设计问题。

实现前已知缺口、当前基础设施状态、暂缓范围和阶段性交付项不写入本文；它们属于 [RFC index](./index.md) 的背景、非目标、风险，或 [迁移实施计划](./implementation.md) 的阶段内容。

RFC 关闭时没有 Apollyon、Keter 或 Euclid 遗留项。`SAFE-001` 是已接受且已登记到 register 的第一阶段外限制，不是 active RFC blocker。

## Apollyon

- 暂无。

## Keter

- 暂无。

## Euclid

- 暂无。

## Safe

### SAFE-001：不为 `ioctl_loop01` 伪造 loop sysfs / partscan 成功语义

**状态：** Accepted Limitation

**观察：** 修复 LTP runner 的 loop 模块元数据路径后，`ioctl_loop01` 已经越过 driver availability 检查并能通过 `tst_find_free_loopdev()` 找到 `/dev/loop0`。当前失败推进到下一层：测试环境缺少或无法运行 `parted`，随后 LTP 仍会读取 `/sys/block/loop0/loop/partscan` 并因 `ENOENT` 报 `TBROK`。

**结论：** 这不是单个文件缺失的小修。`ioctl_loop01` 后续还会要求 `LOOP_SET_STATUS` 接受 `LO_FLAGS_AUTOCLEAR | LO_FLAGS_PARTSCAN | LO_FLAGS_DIRECT_IO` 的输入、`LOOP_GET_STATUS` 至少回显 `AUTOCLEAR | PARTSCAN`、`/sys/block/loopN/loop/{partscan,autoclear,backing_file}` 反映状态；若 `parted` 可用，还会要求 `/dev/loopNp1` 与 `/sys/block/loopN/loopNp1` 可见。只伪造 `partscan` 文件会把失败点推到 flag 回显或 partition node，且会与 RFC 的“unsupported loop 功能不能伪成功”边界冲突。

**处理：** 本轮不实现 loop sysfs、partscan、partition node、direct I/O 或完整 autoclear release hook，也不加入兼容 shim 让这些 flag 成功污染状态。保持 `LO_FLAGS_PARTSCAN` / `LO_FLAGS_DIRECT_IO` 和未具备释放 hook 的 `LO_FLAGS_AUTOCLEAR` 失败或 unsupported，直到后续阶段按完整语义补齐。

**关联：** [当前限制](../../register/current-limitations.md), [RFC index](./index.md), [迁移实施计划](./implementation.md)

## Neutralized

### KETER-001：`/dev/loop-control` 的发布与 `LOOP_CTL_GET_FREE` 支持必须绑定

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的目标、方案和 `loop-control 发布边界` 明确：第一阶段不发布 `/dev/loop-control`；后续一旦发布，必须同阶段实现 `LOOP_CTL_GET_FREE`，并从同一份 loop 设备池返回空闲编号。
- [不变量需求](./invariants.md) 的闭合条件、身份与能力模型、禁止退化项明确：loop discovery 不能被半发布控制节点遮蔽。
- [迁移实施计划](./implementation.md) 的迁移原则、阶段 0、阶段 3、阶段 4 和验收条件明确：`LOOP_CTL_GET_FREE` 第一阶段只保留 UAPI 常量，控制节点不发布。

**原问题：** RFC 已把 `LOOP_CTL_GET_FREE` 列入 UAPI 常量，但没有把 `/dev/loop-control` 作为设备协议入口纳入身份、不变量和阶段验收。LTP `tst_find_free_loopdev()` 与 BusyBox `mount -o loop` 都会先尝试打开 `/dev/loop-control`；如果该节点存在但 `LOOP_CTL_GET_FREE` 返回 unsupported 或错误，用户态会直接判定找不到空闲 loop 设备，而不会再回退到扫描 `/dev/loopN` + `LOOP_GET_STATUS` 的老路径。

**原违反的不变量：** loop discovery 不能因为一个半发布的控制节点而遮蔽可用的 `/dev/loopN`；devfs 节点、loop 设备池和 ioctl 状态必须暴露同一份空闲/绑定事实。

### KETER-002：`FileOps::ioctl` API 缺少打开描述符上下文会破坏 mode/权限语义

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的方案和 `ioctl 边界` 定义了短生命周期薄 `IoctlCtx`：显式携带目标 fd 的值语义能力快照、用户空间访问 handle、受控 arg-fd lookup helper 和最小 credential/capability snapshot；用户指针读写继续使用现有 `user_access` API，不新增 ioctl 专用 copy helper。
- [不变量需求](./invariants.md) 的闭合条件和状态所有权明确：设备实现不得保存 raw fd number，也不得自行调用当前 task 文件表重新解释 ioctl 参数；`FileDesc`、`ProcFile`、`FilesState` 和完整 task 对象不得跨过 syscall/VFS ioctl 边界。
- [迁移实施计划](./implementation.md) 的阶段 1 改为 `FileOps::ioctl(&File, ctx)` 或等价形式，并把 `O_PATH`、打开模式快照、arg-fd lookup 和默认 `ENOTTY` 行为列为实现与审查重点。

**原问题：** 阶段 1 只要求 `FileOps::ioctl` 至少接收 `&File`、`cmd`、`arg`。但 Anemone 的读写权限和打开模式保存在 `FileDesc` / `ProcFile` 层，不在共享的 VFS `File` 内。loop ioctl 需要同时判断目标 loop fd 的打开模式、`O_PATH`/只读 fd 行为、`LOOP_SET_STATUS*` 是否允许修改状态，以及 `LOOP_SET_FD` 参数 fd 的可读/可写能力。如果只把裸 `&File` 交给设备实现，驱动要么拿不到这些事实，要么隐式回到 `current_task` 文件表做二次 syscall 层解析，形成隐藏边界。

**原违反的不变量：** `sys_ioctl()` 负责 fd 查找和最外层 ABI 边界，打开文件对象负责类型语义；这两个层次之间必须显式传递 fd capability，而不是让设备实现猜测或绕回全局 task 状态。

### KETER-003：loop flag 支持策略在草案内自相矛盾

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的 `loop flags 第一阶段策略` 给出 flag/字段策略表：`READ_ONLY` 支持，`AUTOCLEAR` 只有具备最小释放语义才支持，`PARTSCAN`、`DIRECT_IO`、加密字段和未知 bit 返回 `EINVAL` 或稳定 unsupported。
- [不变量需求](./invariants.md) 的 `loop flag 支持边界` 和禁止退化项明确：`GET_STATUS*` 只能返回已经具备最小语义的 bit。
- [迁移实施计划](./implementation.md) 的阶段 4 改为先完整校验再提交，禁止 `PARTSCAN` / `DIRECT_IO` / unknown bit 成功污染状态，并把 autoclear 的 release hook 作为支持前提。

**原问题：** 非目标和禁止退化项要求 partscan、direct I/O、configure、加密等暂缓功能不能“返回成功但不生效”；但阶段 4 又要求 `LOOP_SET_STATUS` / `LOOP_SET_STATUS64` 接受 `autoclear/partscan` 记录位。对 `LO_FLAGS_PARTSCAN`，如果没有分区扫描、`/dev/loopNpM` 和 sysfs 可观测性，单纯保存 flag 会让 LTP 与用户态把该功能误判为已支持。对 `LO_FLAGS_AUTOCLEAR`，如果只返回并记录 flag，却不定义 close-last-fd / umount 后自动释放的最小语义，也会让 `mount -o loop` 的清理模型和实际 busy/release 行为分裂。

**原违反的不变量：** unsupported loop 功能必须以稳定错误或明确兼容降级表达，不能以“状态里有 bit”伪装成语义生效。

### KETER-004：`IoctlCtx` 不得把 task/fd 层对象传入 VFS file ops

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的方案和 `ioctl 边界` 明确：`sys_ioctl()` 可以读取调用者 `FileDesc`，但必须把 `FileDesc` / `ProcFile` / 当前 task / 文件表压缩成值语义 `target_access` 后再进入 `FileOps::ioctl`。
- [不变量需求](./invariants.md) 的闭合条件、状态所有权和禁止退化项明确：VFS/设备层不得接收 `FileDesc`、`ProcFile`、`FilesState` 或完整 task/capability 对象。
- [迁移实施计划](./implementation.md) 的阶段 1 明确：`FileOps::ioctl` 形状是 `&File` + 薄 `IoctlCtx` 或等价结构，不是 `Arc<FileDesc>` 或 ioctl 专用 fd wrapper；`IoctlCtx::get_arg_fd()` 只返回窄化后的 `IoctlArgFile` / `BackingFileHandle` 和能力快照。

**原问题：** 先前修复 KETER-002 时把“必须显式传递 fd capability”写成了 `IoctlCtx` 可携带 `Arc<FileDesc>`。这会把 `task::files` 层反向拉进 VFS file ops，破坏当前方向：`FileDesc` 包装 `fs::File`，而不是 `fs::FileOps` 依赖 `FileDesc`。

**原违反的不变量：** syscall 层拥有 fd 表和打开描述符解析；VFS file ops 拥有打开文件对象语义。两者之间只能传递窄化后的能力事实，不能传递 task/fd 层对象。

**补充理由：** 设备实现可以知道 ioctl `arg` 是 fd number，但 fd number、当前 task fd-table lookup 和可长期保存的 backing file handle 不是同一层对象。`FileDesc` 还携带 fd-local flags、共享 status flags mutator 和 syscall 访问包装；loop 绑定成功后需要保存的是经边界窄化并延长生命周期的 backing file handle，而不是调用者文件表对象。

### KETER-005：loop 私有 ioctl 不得分叉 `/dev` block-device file ops

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的方案和 `VFS 与设备边界` 明确：块设备 `/dev` 文件操作保持单一路径，通用 `BLK*` 先由 block devfs file ops 处理，未匹配命令再通过 block 子系统拥有的 private hook 委托给具体 `BlockDev`。
- [不变量需求](./invariants.md) 的闭合条件、状态所有权和禁止退化项明确：loop 不得为 `/dev/loopN` 发布绕过统一 block devfs file ops 的专属 file ops。
- [迁移实施计划](./implementation.md) 的阶段 2/3/4 明确：新增 block private ioctl hook；`/dev/loopN` 复用统一 block devfs file ops，只通过 private hook 接收 `LOOP_*`。

**原问题：** 草案说通用 `BLK*` 在 block file ops 中，而 loop 私有 ioctl 可在 loop “自身 file ops” 中实现。这会把 `/dev/vda`、`/dev/ram0` 和 `/dev/loopN` 的块设备行为拆成多套入口，导致 read/write、seek、`BLK*`、stat、mount lookup 和私有 ioctl 可能看到不同边界。

**原违反的不变量：** block 子系统拥有 `/dev` block-device 行为和通用 `BLK*` 语义；loop 私有协议只能作为 block device 的扩展 hook 接入，不能另起一套 devfs file ops。

### EUCLID-001：loop 的 VFS-backed block bridge 必须标成例外

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的 `loop 设备边界` 明确：loop 位于块设备子系统下是 VFS-backed bridge 例外，不能推广到普通物理块设备。
- [不变量需求](./invariants.md) 的身份与能力模型、禁止退化项明确：只有 loop 可以持有 ioctl 边界创建的窄 backing file handle；virtio、SCSI、ramdisk 等不得依赖 VFS 打开文件对象或 task fd 状态。
- [迁移实施计划](./implementation.md) 的阶段 3 明确：`device/block/loop.rs` 是例外桥接点，不是普通 block driver 依赖 VFS 的先例。

**原问题：** loop 放在 `device/block/loop.rs` 是合理的，因为它确实注册成 `BlockDev`；但它会调用 regular file 的 `read_at` / `write_at`。如果不写明这是 loop 特有桥接，很容易被后续物理 block driver 当成可以依赖 VFS 的先例。

### EUCLID-002：权限输入必须是不可变、最小 snapshot

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的 `ioctl 边界` 明确：必要时只传不可变 credential/capability 快照，第一阶段不传完整 task，也不允许设备代码重新获取 current-task 权限上下文。
- [不变量需求](./invariants.md) 的状态所有权明确：上下文只能包含不可变、最小 credential/capability snapshot。
- [迁移实施计划](./implementation.md) 的阶段 1 明确：如果第一阶段需要权限输入，只能传不可变、最小化的 snapshot。

**原问题：** `IoctlCtx` 中预留 credential/capability view 是可以接受的，但如果写成未来可扩展的“权限视图”而不加限制，会让设备代码拿到过宽的 task/current-task 权限入口。
