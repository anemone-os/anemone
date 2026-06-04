# 2026-06-04 - IOCTL Loop

**Status:** Active
**Owners:** doruche, Codex
**Area:** syscall ABI / VFS / devfs / block device / loop / mount / LTP
**RFC:** [RFC-20260603-IOCTL-LOOP](../../rfcs/ioctl-loop/index.md)
**Current Phase:** Agent 3 completed; Agent 4 ready

## Scope

本事务跟踪 `ioctl(2)` 基础设施和第一阶段 loop 设备闭环实现：先建立 VFS ioctl 分发与 `IoctlCtx`，再补齐通用 block `BLK*` ioctl、静态 loop 设备池和第一阶段 loop 私有 ioctl，最终准备 `mount -t ext4 /dev/loopN` 的最小验证闭环。

本事务覆盖：

- `sys_ioctl()` 到打开文件对象的 VFS ioctl 分发；
- `IoctlCtx` 的目标 fd 能力快照、用户态 copy helper 和受控 arg-fd lookup；
- 通用 block devfs `BLKGETSIZE64` / `BLKGETSIZE` / `BLKSSZGET`；
- block private ioctl hook；
- 静态 `/dev/loop0..loopN` 设备池；
- `LOOP_GET_STATUS*` / `LOOP_SET_FD` / `LOOP_SET_STATUS*` / `LOOP_CLR_FD` 等第一阶段 loop ioctl；
- loop discovery、bind、block size query、mkfs、mount、umount、release 的最小闭环准备与证据归类。

非目标：

- 不让 mount 层直接理解普通 image 文件或 `-o loop`。
- 第一阶段不发布 `/dev/loop-control`。
- 不实现 sysfs `/sys/block/loop*`、分区扫描、direct I/O、加密 loop 或完整 mount flag 语义。
- 不运行 QEMU / LTP，除非用户后续明确要求；rv64 / LTP 日志默认由用户提供。

## Invariants

- `sys_ioctl()` 只拥有 fd 查找、最外层 ABI 边界和兼容分支；设备私有协议不能长期堆在 syscall 全局分支。
- `FileOps::ioctl` 只能接收短生命周期 `IoctlCtx` 或等价上下文，不能接收 `FileDesc`、`ProcFile`、`FilesState`、当前 task 或完整 capability 对象。
- block devfs file ops 保持单一路径，通用 `BLK*` 先处理，未匹配命令再通过 block private hook 委托具体 `BlockDev`。
- `/dev/loopN`、block registry 和 mount source lookup 必须解析到同一个 loop device 对象。
- loop backing file 生命周期必须独立于用户态传入 fd 的后续关闭。
- loop 状态锁内只复制 `Bound` 快照，锁外执行 backing file I/O。
- unsupported loop flag 或暂缓功能必须返回稳定错误，不能写入状态后伪装成功。

## Handoff

**Last Updated:** 2026-06-04

**Current Branch:** `dev/drc/ioctl`

**Current HEAD:** `4807bc3` (`docs: wire ioctl-loop transaction log`)

**Canonical RFC:** [RFC-20260603-IOCTL-LOOP](../../rfcs/ioctl-loop/index.md), [Invariants](../../rfcs/ioctl-loop/invariants.md), [Implementation Plan](../../rfcs/ioctl-loop/implementation.md), [Tracking Issues](../../rfcs/ioctl-loop/tracking-issues.md)

**Completed:** RFC 已提升到公开文档；design review 发现的 Keter / Euclid 项已经吸收到 RFC、不变量需求和迁移实施计划；本事务日志、事务索引、双周 devlog 和 mdBook Summary 已建立链接。总控前置检查确认当前分支为 `dev/drc/ioctl`。Agent 0 只读前置审计已完成，未发现 Apollyon / Keter blocker，停止条件未触发。Agent 1 已完成 UAPI 常量与 loop ABI 结构、语义化 `UnsupportedIoctl -> ENOTTY` errno 映射、`IoctlCtx` / `FileOps::ioctl` VFS 分发和默认 unsupported ioctl 行为。Gate 1 review 发现的 `FIONREAD` / `O_PATH` Keter 已修复，复审后未保留 Apollyon / Keter blocker。Agent 2 已完成统一 block devfs `BLKGETSIZE64` / `BLKGETSIZE` / `BLKSSZGET` 和 block private ioctl hook；Gate 2 review 通过，未保留 Apollyon / Keter blocker。Agent 3 已完成 kconfig 控制的静态 loop block device pool，并保留统一 block devfs file ops 与默认 private ioctl unsupported 行为。

**In Progress:** 暂无。Agent 4 尚未启动。

**Open Blockers:** 暂无已确认 blocker。

**Next Action:** 进入 Agent 4 前先做 Gate 3 预检或按用户指令启动 Agent 4：实现 loop 私有 ioctl 第一阶段；不得发布 `/dev/loop-control`，不得绕过统一 block devfs file ops。

**Do Not Redo:** 不要把 loop 或 block 私有 ioctl 塞回 `sys_ioctl()`；不要把 `FileDesc` / `ProcFile` / `FilesState` / 当前 task 传进 VFS 或设备层；不要为 `/dev/loopN` 发布绕过统一 block devfs file ops 的专属 file ops；不要发布半成品 `/dev/loop-control`；不要改 mount 层直接解析普通 image 文件或 `-o loop`。

## Phase Log

### 2026-06-04 - 事务日志启动与 Agent 0 前置审计

**Phase:** orchestration / pre-audit

**Change:** 建立本事务日志，并把 [RFC-20260603-IOCTL-LOOP](../../rfcs/ioctl-loop/index.md)、事务索引、mdBook Summary 和当前双周 devlog 连接到同一条实现记录。

**Change:** 总控 agent 刷新当前落点：分支为 `dev/drc/ioctl`，HEAD 为 `99d4f30`；当前工作区干净；`sys_ioctl()` 仍只有 `FIONREAD` 特判；`FileOps` 尚无 `ioctl` 方法；block devfs 仍有统一 `BLOCK_DEV_FILE_OPS`；`BlockDevClass::Loop` 已存在但未实现 loop 设备池；ext4 mount 仍通过 `MountSource::Block` 且要求 512 字节 block size。

**Review:** 按 [Agent 编排建议](../../rfcs/ioctl-loop/backgrounds/agent-orchestration.md) 启动 Agent 0 做只读前置审计。Agent 0 不改代码，不启动后续 worker，只输出是否允许进入 Agent 1、必要 blocker 和当前代码路径与 RFC 阶段的对应表。

**Validation:** 本阶段为文档与只读审计启动；未修改生产代码，未运行构建、QEMU 或 LTP。

**Next:** 等待 Agent 0 审计结果；通过后再进入 Agent 1，不一次性启动后续 worker。

### 2026-06-04 - Agent 0 前置审计完成

**Phase:** Agent 0 / read-only audit

**Review:** Agent 0 只读审计结论为允许进入 Agent 1。审计未发现 Apollyon / Keter blocker：`sys_ioctl()` 仍只有 `FIONREAD` 特判与 unsupported fallback，没有形成与 RFC 不兼容的设备私有分发边界；`BLOCK_DEV_FILE_OPS` 仍是 block devfs 的统一生产 file ops，未出现 `/dev/vda`、`/dev/ram0` 与 future `/dev/loopN` 的分裂入口。

**Review:** 当前缺口均归入 RFC 计划阶段：`FileOps` 尚无 `ioctl` 方法属于 Agent 1；`anemone-abi` 尚缺 `BLK*`、`LOOP_*` 与 loop UAPI 结构属于 Agent 1 Checkpoint A；`BlockDev` 尚无 private ioctl hook 属于 Agent 2；loop 设备池与 loop 私有 ioctl 分别属于 Agent 3 / Agent 4。

**Review:** mount 与 ext4 路径保持 RFC 边界：mount source 仍只解析为 `MountSource::Block`，没有直接理解普通 image 或 `-o loop`；ext4 仍要求 source 是 block device 且 block size 为 512 字节。当前未发现 `/dev/loop-control` 半发布。

**Validation:** 只读审计；未修改生产代码，未运行构建、QEMU 或 LTP。

**Next:** 可以进入 Agent 1，但必须保持 `sys_ioctl()` 只做 fd lookup、`O_PATH` / 能力快照、用户指针边界和 `FIONREAD` 兼容；VFS / 设备层只接收短生命周期 `IoctlCtx`，不得接收 `FileDesc`、`ProcFile`、`FilesState`、当前 task 或完整 capability / task 对象。

### 2026-06-04 - Agent 1 UAPI 与 VFS ioctl 分发完成

**Phase:** Agent 1 / Checkpoint A + Checkpoint B

**Change:** Checkpoint A 增加 Linux ioctl UAPI 常量与结构：`BLKGETSIZE`、`BLKGETSIZE64`、`BLKSSZGET`，第一阶段需要识别的 `LOOP_*` / `LOOP_CTL_GET_FREE`，以及 `repr(C)` 的 `loop_info`、`loop_info64`、`loop_config`。loop UAPI 结构只作为 ABI 数据包存在；新增的 `LoopFlags` helper 只做 known-bit 边界表达，不作为长期设备状态。

**Change:** Checkpoint B 建立 VFS ioctl 分发：`FileOps` 新增 `ioctl(&File, IoctlCtx)`，默认返回 `UnsupportedIoctl`；`sys_ioctl()` 保留 `FIONREAD` 兼容，对其他命令完成 fd lookup、`O_PATH` 过滤、目标 fd 能力快照和用户空间 handle 捕获后分发到打开文件对象。

**Change:** `IoctlCtx` 只携带 `cmd`、`arg`、`IoctlFileAccess` 值语义快照、`Arc<UserSpaceHandle>` 和受控 arg-fd lookup helper。helper 返回 `IoctlArgFile { Arc<File>, IoctlFileAccess }`，不暴露 `FileDesc`、`ProcFile`、`FilesState` 或 fd table，也不允许设备保存 raw fd number 作为长期状态。为支持后续 backing file 生命周期，`task::files` 内部把 `ProcFile.file` 改为 `Arc<File>`，但该 task/files 层对象没有进入 VFS ioctl API。

**Change:** 按总控补充约束，新增 `SysError::UnsupportedIoctl` 并仅映射到 Linux `ENOTTY`；未使用 `NotYetImplemented` / `ENOSYS` 作为默认 ioctl unsupported 语义，也未保留 `NotTty` 这类 Linux 历史命名。

**Boundary:** 未实现 block `BLK*` 成功路径，未新增 block private hook，未新增 loop 设备池，未实现任何 `LOOP_*` 成功路径，未发布 `/dev/loop-control`，未改 mount 层解析普通 image 或 `-o loop`。

**Validation:** `just build` 通过；构建期间仅出现既有 `anemone-kernel/src/sync/mono.rs` unused import warning。`git diff --check` 通过，无 whitespace 报告。未运行 QEMU / LTP。

**Next:** 进入 Gate 1 review。审查重点是 `IoctlCtx` 是否仍只包含窄能力事实和用户空间 handle、arg-fd helper 是否只返回 `Arc<File>` + 能力快照、`sys_ioctl()` 是否没有积累 block / loop 私有分支、普通文件 / 目录 / procfs 未知 ioctl 是否稳定落到 `ENOTTY` 映射。

### 2026-06-04 - Gate 1 review 通过

**Phase:** Gate 1 / ioctl ABI-VFS boundary review

**Review:** Gate 1 reviewer 发现一个 Keter blocker：`FIONREAD` 兼容分支在 `sys_ioctl()` 中先于统一 `O_PATH` 过滤执行，导致 `O_PATH` 目标 fd 的 `ioctl(FIONREAD)` 可能绕过 RFC 要求的 `EBADF` 边界。

**Fix:** 总控窄修 `sys_ioctl()`，把目标 fd lookup、`IoctlFileAccess` 快照和 `O_PATH => EBADF` 过滤提升到命令分发前。`FIONREAD` 仍保留现有 pipe readable-bytes 兼容路径；其他命令继续构造短生命周期 `IoctlCtx` 后分发到 `FileOps::ioctl`。

**Review:** 修复后 Gate 1 未保留 Apollyon / Keter blocker。确认 `IoctlCtx` 只暴露 `cmd`、`arg`、目标 fd 能力快照、`Arc<UserSpaceHandle>` 和短生命周期 arg-fd lookup；helper 只返回 `Arc<File>` 与能力快照；`FileDesc`、`ProcFile`、`FilesState` 和 fd table 未进入 VFS / 设备 ioctl API。默认 unsupported ioctl 通过 `SysError::UnsupportedIoctl` 映射到 `ENOTTY`。

**Boundary:** 未实现 Agent 2+ 内容：没有 block `BLK*` 成功路径、没有 block private hook、没有 loop 设备池、没有任何 `LOOP_*` 成功路径、没有 `/dev/loop-control`，也没有 mount 层 `-o loop` 特判。

**Validation:** `just build` 通过；仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。`git diff --check` 通过，无 whitespace 报告。未运行 QEMU / LTP。

**Next:** 可以进入 Agent 2，write set 限于 block 子系统与事务日志。

### 2026-06-04 - Agent 2 通用块设备 ioctl 完成

**Phase:** Agent 2 / generic block ioctl

**Change:** 在 `BlockDev` trait 上新增 block private ioctl hook，默认返回 `SysError::UnsupportedIoctl`，因此现有 virtio-blk 和 ramdisk 不需要逐个补空实现；后续 loop 私有 `LOOP_*` 只能通过该 hook 接入。

**Change:** 统一 `BLOCK_DEV_FILE_OPS` 新增 ioctl 分发：先处理 `BLKGETSIZE64`、`BLKGETSIZE`、`BLKSSZGET`，未匹配命令再委托具体 `BlockDev::ioctl(BlockIoctlCtx)`。用户态写回继续使用 `IoctlCtx` 携带的 `UserSpaceHandle` 与现有 `UserWritePtr` API，未新增 ioctl 专用 copy helper。

**Change:** `BLKGETSIZE64` 返回设备总字节数；`BLKGETSIZE` 返回 512 字节扇区数，并用 checked arithmetic 处理 sector count 溢出；`BLKSSZGET` 返回当前 block device 的逻辑块大小。顺手把 block read/write/seek 和 devfs stat size 的容量乘法改为同一 checked helper，但没有改变现有块对齐约束。

**Boundary:** 未修改 `sys_ioctl()`、`FileOps::ioctl`、`anemone-abi`、mount、ext4、devfs 发布层或 loop 设备池；未新增 `/dev/loop-control`；未实现任何 `LOOP_*` 成功路径；未为 `/dev/loopN` 创建专属 file ops；未把 block/loop 私有 ioctl 塞回 syscall 层。

**Validation:** `just build` 通过；仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。`git diff --check` 通过，无 whitespace 报告。未运行 QEMU / LTP。

**Next:** 进入 Gate 2 review。

### 2026-06-04 - Gate 2 review 通过

**Phase:** Gate 2 / block devfs ioctl review

**Review:** Gate 2 未发现 Apollyon / Keter blocker。确认所有 block devfs 节点仍由统一 `BLOCK_DEV_FILE_OPS` 打开，`block_ioctl()` 先处理通用 `BLKGETSIZE64`、`BLKGETSIZE`、`BLKSSZGET`，未匹配命令再委托 `BlockDev::ioctl(BlockIoctlCtx)`；默认 private hook 返回 `SysError::UnsupportedIoctl`，继续映射到 Linux `ENOTTY`。

**Review:** `BLKGETSIZE` 使用 block size 的 512-byte unit 数计算 sector count，并通过 checked arithmetic 处理溢出；`BLKSSZGET` 返回 block device 的逻辑块大小；用户态写回经 `IoctlCtx.uspace()` 与现有 `UserWritePtr` 完成，没有新增 ioctl 专用 copy helper。

**Boundary:** 旁路搜索确认没有修改 `sys_ioctl()`、`FileOps::ioctl`、`anemone-abi`、mount、ext4 或 devfs 发布层；没有发布 `/dev/loop-control`；没有新增 loop 设备池或任何 `LOOP_*` 成功路径；没有为 future `/dev/loopN` 创建专属 file ops。

**Validation:** 总控复跑 `just build` 通过；仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。总控复跑 `git diff --check` 通过。未运行 QEMU / LTP。

**Next:** Agent 3 可以开始，但本轮未启动。

### 2026-06-04 - Agent 3 静态 loop 设备池完成

**Phase:** Agent 3 / loop block device pool

**Change:** 新增 `anemone-kernel/src/device/block/loop.rs`，启动期按 `LOOP_DEVICE_COUNT` 创建并注册静态 `/dev/loop0..loopN`。每个 loop 设备使用 major 7 与对应 minor，注册为 `BlockDevClass::Loop` 的真实 `BlockDev`，并通过既有 `publish_block_device()` 发布为 devfs block 节点；未发布 `/dev/loop-control`。

**Change:** loop 状态的单一真相源是 `LoopState::{Unbound, Bound}`。Agent 3 阶段所有设备初始为 `Unbound`：`block_size()` 固定 512 字节，`total_blocks()` 返回 0，`read_blocks()` / `write_blocks()` 返回 `SysError::NoSuchDevice`，避免空闲设备对 read/write/mount 伪成功。`Bound` 状态、backing file、offset、sizelimit、readonly、display name 和内部 flags 类型已预留给 Agent 4，但本阶段没有任何 `LOOP_*` ioctl 成功路径。

**Change:** 预留的 Bound I/O 形状满足锁序边界：loop 状态用短持有 `SpinLock` 保护，read/write/容量查询只在锁内复制 `LoopBoundSnapshot`，锁外再查询 backing file attr 或调用 backing file `read_at` / `write_at`。readonly 写入映射为 `ReadOnlyFs`，越界与溢出使用 checked arithmetic。

**Change:** 将 loop 设备数量接入 kconfig：`scripts/xtask/src/config/kconfig.rs` 新增 `loop_device_count` 参数并生成 `LOOP_DEVICE_COUNT`；`conf/.defconfig` 默认值为 8。按用户补充要求，顺手把 ramdisk 数量从内嵌常量迁到同一套 kconfig 参数：新增 `ramdisk_count`，生成 `RAMDISK_COUNT`，默认保持 16。

**Boundary:** 未修改 `sys_ioctl()`、`FileOps::ioctl`、mount、ext4 或 `anemone-abi`；未实现 Agent 4 的 `LOOP_GET_STATUS*`、`LOOP_SET_FD`、`LOOP_SET_STATUS*`、`LOOP_CLR_FD` 成功路径；未为 `/dev/loopN` 发布专属 file ops；未发布 `/dev/loop-control`。

**Validation:** `just build` 通过；仅有既有 `anemone-kernel/src/sync/mono.rs` unused import warning。`git diff --check` 通过。未运行 QEMU / LTP，因此 `/dev/loop0` 可见性、`/dev/loop-control` 不存在和空闲读写/mount errno 尚待运行态验证。

**Next:** 停在 Agent 4 之前。下一阶段应只通过 block private ioctl hook 实现 loop 私有 ioctl 第一阶段，并在进入 Gate 3 前补齐 loop discovery / bind / BLKGETSIZE64 / release 的运行态证据。
