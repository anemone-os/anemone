# IOCTL Loop Agent 编排建议

本文记录 `IOCTL Loop` 进入实现阶段时的 agent 编排方式。Canonical 协议仍以
[RFC 入口](../index.md)、[不变量需求](../invariants.md)、[迁移实施计划](../implementation.md)
和 [Tracking Issues](../tracking-issues.md) 为准；本文只说明如何按这些 gate 组织
worker、reviewer 和验收顺序。

## 编排原则

1. 不按“一个文档阶段一个 agent”机械拆分。拆分边界应对应 ABI 分发边界、block 设备生产路径切换点和 loop 状态机提交点。
2. 阶段 1 的 `IoctlCtx` / `FileOps::ioctl` 是后续所有设备 ioctl 的协议基础，必须先单独闭合并 review。
3. 阶段 2 的通用 `BLK*` ioctl 与 block private hook 强耦合，建议由同一实现 agent 完成。
4. 阶段 3 的 loop 设备池和阶段 4 的 loop 私有 ioctl 可以同一 agent 串行推进，但必须保留 checkpoint；设备池未闭合前不要实现 `LOOP_SET_FD`。
5. review agent 只放在有意义的 gate 上，不在每个小补丁后立即审查。
6. 写入型 worker 只改自己的 write set；遇到必须越界的依赖，停止并回报总控。
7. 每个实现阶段都要保持 `just build` 通过；功能阶段必须提供最小用户态或 LTP 风格验证证据。
8. LTP/QEMU 默认由用户验证，除非用户后续明确授权 agent 运行。
9. 第一阶段不发布 `/dev/loop-control`；任何 worker 都不得用半发布控制节点绕过 `/dev/loopN` discovery。
10. 暂缓功能必须返回稳定 unsupported 或参数错误，不能把 unsupported flag 写入状态后伪装成功。

## 总控 Agent 使用方式

建议启动一个总控 agent 负责 orchestration，但不要让它自由决定新的协议拆分。
总控 agent 的权限边界是：

- 可以执行前置检查、代码搜索和构建级 gate。
- 可以启动只读 explorer / reviewer。
- 可以启动写入型 worker，但必须使用本文列出的 write set 和 worker 合同。
- 可以串行集成 worker diff。
- 可以在实现开始后建立并更新事务 devlog。
- 不运行 QEMU / LTP，除非用户后续明确要求；rv64 / LTP 日志默认由用户提供。
- 不 push、不 force-push、不 reset hard、不清理未归属改动。
- 遇到停止条件时回报用户，不自行拍板。

总控第一轮不要一次性派发所有 worker。建议流程是：

1. 重新确认当前分支、工作区状态、RFC 文档和是否已建立事务日志。
2. 派发 Agent 0 做当前 ioctl / VFS / block / mount 前置审计。
3. 前置审计通过后派发 Agent 1，实现 UAPI 常量、结构体和 `IoctlCtx` / `FileOps::ioctl` 分发。
4. 进行 Gate 1 review，确认 ioctl ABI 边界没有把 task/fd 层对象泄露给 VFS/设备层。
5. 派发 Agent 2，实现通用 block ioctl 和 block private hook。
6. 进行 Gate 2 review，确认所有 block devfs 节点仍走统一 file ops。
7. 派发 Agent 3，实现静态 loop block device pool。
8. 派发 Agent 4，实现 loop 私有 ioctl 第一阶段。
9. 进行 Gate 3 review，确认 loop identity、lifecycle、flag 策略和锁序闭合。
10. 派发 Agent 5 做 loop mount 最小闭环验证准备、旁路审计和事务日志收口。

可直接给总控 agent 的启动 prompt：

```text
工作目录是仓库根目录。请作为 IOCTL Loop 的总控 agent，阅读
docs/src/rfcs/ioctl-loop/index.md、
docs/src/rfcs/ioctl-loop/invariants.md、
docs/src/rfcs/ioctl-loop/implementation.md、
docs/src/rfcs/ioctl-loop/tracking-issues.md、
docs/src/rfcs/ioctl-loop/backgrounds/agent-orchestration.md。

目标是按 RFC gate 实现 IOCTL Loop：建立 VFS ioctl 分发和 IoctlCtx，补齐通用块设备
BLK* ioctl，建立静态 loop 设备池，实现 LOOP_GET_STATUS* / LOOP_SET_FD /
LOOP_SET_STATUS* / LOOP_CLR_FD 等第一阶段 loop ioctl，并准备 mount -t ext4
/dev/loopN 的最小闭环验证。

你可以启动子 agent，但必须按 agent-orchestration.md 的顺序、write set 和 review gate
分工，不允许 worker 越界修改。你不是独自在代码库里工作；不得 revert 用户或其他
agent 的改动。实现开始后需要建立并维护对应事务 devlog。

第一步只做前置检查、刷新当前代码落点和准备启动的 agent 列表。不要直接一次性启动
所有 worker。遇到停止条件时停止并向用户报告，不要自行拍板。
```

## Agent 0：前置审计

职责：只读审计当前代码落点是否仍符合 RFC 假设，不改代码。

读取范围：

- `anemone-kernel/src/fs/api/ioctl.rs`
- `anemone-kernel/src/fs/file.rs`
- `anemone-kernel/src/device/block/mod.rs`
- `anemone-kernel/src/device/block/devfs.rs`
- `anemone-kernel/src/fs/api/mount.rs`
- `anemone-kernel/src/fs/ext4/mod.rs`
- `anemone-abi/src`

检查项：

- `sys_ioctl()` 是否仍只有 `FIONREAD` 特判和 unsupported fallback。
- `FileOps` 是否还没有 ioctl 方法，或已有方法是否满足 `IoctlCtx` 边界。
- block devfs file ops 是否是 `/dev/<block>` 的统一 read/write/seek 入口。
- `BlockDev` registry、`BlockDevClass::Loop`、devfs publish 和 `MountSource::Block` 是否仍能支持同一个 loop 对象身份。
- ext4 mount 是否仍要求 512 字节块大小。
- mount option、statfs、`/proc/self/mounts` 的弱实现是否只属于后续范围。

交付：

- 是否允许进入 Agent 1 的结论。
- 如果不允许，列出必须先修的 RFC blocker。
- 当前代码路径与 RFC 阶段的对应表。

停止条件：

- 当前代码已经引入与 RFC 不兼容的 ioctl 分发边界。
- block devfs 已分裂出多套生产 file ops，导致 `/dev/vda`、`/dev/ram0` 和 future `/dev/loopN` 不能共享通用 block 行为。

## Agent 1：UAPI + VFS ioctl 分发

职责：合并执行阶段 0 和阶段 1，但保留两个 checkpoint。

write set：

- `anemone-abi/src`
- `anemone-kernel/src/fs/api/ioctl.rs`
- `anemone-kernel/src/fs/file.rs`
- 必要的直接 `FileOps` 调用点
- 实现开始后对应的事务 devlog

Checkpoint A：UAPI 常量与结构。

- 增加 `BLKGETSIZE`、`BLKGETSIZE64`、`BLKSSZGET`。
- 增加第一阶段需要识别的 `LOOP_*` 常量。
- 增加 `loop_info`、`loop_info64`、`loop_config` 的 `repr(C)` 结构体。
- 增加 loop flags 的内部转换 helper；不得把 Linux UAPI 结构体作为长期设备状态。

Checkpoint B：VFS ioctl 分发。

- 新增短生命周期 `IoctlCtx` 或等价结构。
- `IoctlCtx` 携带 `cmd`、`arg`、目标 fd 能力快照、用户态 copy helper、受控 arg-fd lookup helper。
- `FileOps::ioctl(&File, ctx)` 或等价方法默认返回 `ENOTTY`。
- `sys_ioctl()` 处理 `O_PATH`、fd lookup、能力快照和 `FIONREAD` 兼容后，把其他命令交给打开文件对象。
- VFS/设备层不得接收 `FileDesc`、`ProcFile`、`FilesState`、当前 task 或完整 task capability 对象。

验证：

```bash
just build
```

额外证据：

- 现有 pipe `FIONREAD` 行为不退化。
- 普通文件、目录、procfs 文件未知 ioctl 返回稳定 unsupported，不再落到 `ENOSYS`。

Gate 1 reviewer 检查：

- `sys_ioctl()` 和 `FileOps::ioctl` 的 ABI / VFS 边界符合不变量。
- `IoctlCtx::get_arg_fd()` 只返回窄化 backing file handle 和能力快照，不允许设备保存 raw fd number。
- 目标 fd 的 `O_PATH`、读写能力和 file status flags 没有在设备层重新解释。
- Linux ABI 结构体没有进入长期内核状态。

## Agent 2：通用块设备 ioctl

职责：实现阶段 2。

write set：

- `anemone-kernel/src/device/block/mod.rs`
- `anemone-kernel/src/device/block/devfs.rs`
- 必要的 block driver 默认实现调整
- 实现开始后对应的事务 devlog

语义要求：

- 在统一 block devfs file ops 中处理 `BLKGETSIZE64`、`BLKGETSIZE`、`BLKSSZGET`。
- 新增 block private ioctl hook；通用 `BLK*` 先处理，未匹配命令再委托具体 `BlockDev`。
- 默认 private hook 返回 `ENOTTY`。
- 不改变 block read/write 当前块对齐约束。

验证：

```bash
just build
```

额外证据：

- 真实块设备 `BLKGETSIZE64` 能返回非零容量。
- `/dev/vda`、`/dev/ram0` 和 future `/dev/loopN` 不出现分叉 file-op 行为。

Gate 2 reviewer 检查：

- loop 私有 ioctl 只能通过 block private hook 接入，不能为 `/dev/loopN` 另发专属 file ops。
- `BLKGETSIZE` 的 512 字节扇区计算有溢出策略。
- `BLKSSZGET` 返回逻辑块大小，不混用文件系统 block size。

## Agent 3：loop 设备池

职责：实现阶段 3，不实现 loop 私有 ioctl 成功路径。

write set：

- `anemone-kernel/src/device/block/mod.rs`
- `anemone-kernel/src/device/block/loop.rs`
- 必要的设备初始化入口
- 必要的 devfs publish 调用点
- 实现开始后对应的事务 devlog

语义要求：

- 启动期创建静态 `/dev/loop0..loopN`，第一阶段不发布 `/dev/loop-control`。
- 每个 loop 设备注册为 `BlockDevClass::Loop` 对应的真实 `BlockDev`。
- loop 状态有单一真相源：`Unbound | Bound`。
- `read_blocks()` / `write_blocks()` 锁内只复制状态快照，锁外调用 backing file I/O。
- 空闲设备不能让 read/write/mount 伪成功。

验证：

```bash
just build
```

额外证据：

- `/dev/loop0` 可见并是块设备。
- `/dev/loop-control` 不存在。
- 空闲 `/dev/loop0` 可打开，但读写和 mount 返回可分类错误。

## Agent 4：loop ioctl 第一阶段

职责：实现阶段 4。

write set：

- `anemone-kernel/src/device/block/loop.rs`
- `anemone-kernel/src/device/block/mod.rs`
- 必要的 ioctl ABI conversion helper
- 实现开始后对应的事务 devlog

语义要求：

- `LOOP_GET_STATUS` / `LOOP_GET_STATUS64`：空闲返回 `ENXIO`，绑定返回 ABI 状态。
- `LOOP_SET_FD`：通过 `IoctlCtx` arg-fd helper 获取 backing file handle，绑定成功后 backing file 生命周期独立于用户态 fd。
- `LOOP_SET_STATUS` / `LOOP_SET_STATUS64`：先完整校验 offset、sizelimit、flags、加密字段，再一次性提交。
- `LOOP_CLR_FD`：空闲返回 `ENXIO`，busy 返回 `EBUSY`，成功后释放 backing file。
- `LOOP_SET_DIRECT_IO`：第一阶段返回稳定 unsupported。
- `LOOP_CONFIGURE`：若暂缓，返回稳定 unsupported；若实现，必须复用 `SET_FD + SET_STATUS64` 的完整校验。
- `LO_FLAGS_PARTSCAN`、`LO_FLAGS_DIRECT_IO`、未知 bit、加密字段不得保存进状态。

验证：

```bash
just build
```

额外证据：

- `/dev/loopN` 空闲 discovery 能通过 `LOOP_GET_STATUS* == ENXIO` 识别。
- `LOOP_SET_FD` 后 `BLKGETSIZE64` 能看到 backing image 容量。
- 设置 unsupported flags 不会污染后续 `GET_STATUS*`。

Gate 3 reviewer 检查：

- loop devfs 节点、block registry、mount source lookup 和 ioctl 状态指向同一个设备对象。
- loop backing file handle 没有保存 raw fd number 或 task/fd 层对象。
- loop 状态线性化点、busy 判断、锁序和 readonly 语义符合不变量。
- `/dev/loop-control` 没有半发布。

## Agent 5：最小闭环与旁路审计

职责：实现阶段 5 的验证准备、阶段 6 后续范围切分和文档收口。

write set：

- 必要的测试脚本或小型用户态 helper
- `docs/src/devlog/transactions/<date>-ioctl-loop.md`
- 必要时更新 `docs/src/register/current-limitations.md`

审计命令：

```bash
rg -n "sys_ioctl|FIONREAD" anemone-kernel/src
rg -n "trait FileOps|impl .*FileOps" anemone-kernel/src
rg -n "BlockDev|BlockDevClass|DeviceId::Block" anemone-kernel/src
rg -n "MountSource::Block|sys_mount|MS_" anemone-kernel/src
rg -n "read_at|write_at" anemone-kernel/src
```

交付：

- `just build` 结果。
- loop discovery、bind、`BLKGETSIZE64`、mkfs、`mount -t ext4 /dev/loopN`、file write/read、umount、`LOOP_CLR_FD` 的最小验证记录，或等待用户 LTP 日志的明确 handoff。
- LTP `.needs_device` 失败分类：ioctl/loop、filesystem type、mount flags、sysfs、partscan、direct I/O、procfs/statfs、测试环境。
- 事务日志收口，记录 checkpoint、review gate、验证证据和剩余限制。

停止条件：

- 验证失败无法归类，且可能表示 ioctl 或 loop 核心协议空洞。
- mount 层被改成直接理解普通 image 文件或 `-o loop`。
- 第一阶段暂缓功能被实现成“返回成功但没有语义”。
