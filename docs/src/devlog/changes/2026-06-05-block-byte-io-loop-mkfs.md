# ANE-CHG-20260605-block-byte-io-loop-mkfs

**Type:** Investigation / Bugfix
**Status:** Follow-up
**Date:** 2026-06-05
**Authors:** doruche, Codex
**Area:** block devfs / loop / LTP compatibility

## Problem

`fanotify13` 的当前失败还没有进入 fanotify 语义本身。LTP setup 会先把后端文件绑定到
`/dev/loop0`，再对 loop 设备运行 `mkfs.ext2`；此前 rv64 日志在普通块特殊文件访问
阶段失败：

```text
mkfs.ext2: lseek(6272, 0): Invalid argument
```

根因是 block devfs 把后端 `BlockDev` 的块对齐契约暴露给 `/dev/<block>` 普通文件
ABI：`seek`、`read` 和 `write` 都要求 offset 与长度按 `block_size()` 对齐。Linux
块特殊文件的普通 buffered I/O 按字节 seek/read/write，设备末尾返回 EOF 或短 I/O；
对齐限制属于 direct I/O 等更窄路径。

这次问题需要记录 LTP setup 归因、Linux ABI 边界和局部实现策略；但范围仍限定在
block devfs 普通块特殊文件 byte I/O，不引入仓库级 accepted contract 或新的 RFC。

## Scope

本轮覆盖：

- `/dev/<block>` 的 `SEEK_SET`、`SEEK_CUR`、`SEEK_END` 固定大小文件语义，合法非块对齐
  offset 不再返回 `EINVAL`。
- `/dev/<block>` 普通 `read` / `write` 与 positioned I/O 支持非对齐 offset 和长度。
- 非对齐写在 block devfs 层执行 read-modify-write，后端 `BlockDev::{read_blocks,write_blocks}`
  仍只接收块对齐请求。
- 每个 block device 注册共享 `io_lock`，用于串行化 block devfs byte I/O 与会改变 loop
  可见容量或字节映射的 loop 控制面提交。
- 块特殊文件上的 `O_DIRECT` open / `F_SETFL` 以 `EINVAL` fail-closed。

本轮不覆盖：

- 不特殊处理 `fanotify13`、`mkfs.ext2` 或某个 loop 设备编号。
- 不削弱 `BlockDev` 后端块对齐契约。
- 不声明支持 `LO_FLAGS_DIRECT_IO`、`LOOP_SET_DIRECT_IO`、分区扫描或 loop sysfs。
- 不证明 mount/ext4 直接 `Arc<dyn BlockDev>` 路径与 block devfs byte I/O 的全设备混合并发一致性。
- 不改变 `SEEK_DATA` / `SEEK_HOLE` 的当前 `NotYetImplemented` 边界。

## Solution

选择在 block devfs owning surface 内增加 byte I/O adapter，而不是修改 `BlockDev` trait
或为 fanotify / mkfs 增加特殊路径。后端仍表达块设备传输能力；普通块特殊文件的
Linux byte ABI 收敛在 devfs file ops。

关键策略：

- `block_seek()` 只按设备总字节数约束目标 offset：负 offset、溢出和超过设备末尾失败，
  非块对齐 offset 可以成功。
- `block_read_bytes_locked()` / `block_write_bytes_locked()` 在持有每设备 `io_lock` 后
  重新读取设备大小，并在锁内裁剪请求。
- 非对齐头尾使用单块 bounce buffer，整块窗口按 `BLOCK_BYTE_IO_WINDOW_BYTES` 分窗，避免按
  完整用户请求范围分配临时 buffer。
- 写入端 `offset >= total_bytes` 返回 `NoSpace`；跨设备末尾的普通 write 返回短写。
- loop 的 `LOOP_SET_FD`、`LOOP_SET_STATUS*` 和 `LOOP_CLR_FD` 在提交状态变化时进入同一
  `io_lock`，锁序是 registry 短查找并释放 guard -> `io_lock` -> loop `state` lock。
- block private ioctl 分发前登记 transient `Arc<dyn BlockDev>` 引用；`LOOP_CLR_FD` 的
  external ref 判断扣除 devfs/ioctl 临时引用，避免把并发等待 `io_lock` 的 ioctl 误判成
  mount/ext4 等持久外部引用。

拒绝的替代方案：

- 只修 `lseek` 会让 `mkfs.ext2` 后续非对齐写继续失败。
- 让 `BlockDev` 接收 byte offset 会把普通文件 ABI 泄漏到所有块后端。
- 只依赖 `File.pos` 锁不能覆盖 positioned I/O、跨 fd I/O 或 loop 控制面重配置。
- 静默接受 `O_DIRECT` 会把当前 buffered helper 误声明为 direct I/O 支持。

## Change

代码变更：

- `anemone-kernel/src/device/block/mod.rs`
  - 新增 `BlockDevIoHandle`，持有 block device 弱引用、每设备 `io_lock` 和 private ioctl
    transient ref 计数。
  - `BlockIoctlCtx` 携带 `BlockDevIoHandle`，向 block-private ioctl 暴露 `with_io_lock()`
    与 `target_device_persistent_ref_count()`。
- `anemone-kernel/src/device/block/devfs.rs`
  - `/dev/<block>` file ops 改为 byte seek/read/write。
  - 新增有界分窗与 RMW helper。
  - generic `BLK*` ioctl 和 private ioctl 分发改走共享 handle；private ioctl 临时强引用
    用 transient guard 标记。
  - 增加 KUnit-style helper 测试，覆盖非对齐 seek/read/write、EOF read、EOF write 和短写。
- `anemone-kernel/src/device/block/loop.rs`
  - `LOOP_SET_FD`、`LOOP_SET_STATUS*`、`LOOP_CLR_FD` 的状态提交进入同一 `io_lock`。
  - `LOOP_CLR_FD` busy 判断改用扣除 transient 引用后的 persistent ref count。
- `anemone-kernel/src/fs/api/openat.rs`
  - 块特殊文件上带 `O_DIRECT` 的 open 以 `InvalidArgument` fail-closed。
- `anemone-kernel/src/fs/api/fcntl.rs`
  - 块特殊文件上通过 `F_SETFL` 设置 `O_DIRECT` 以 `InvalidArgument` fail-closed，并在拒绝前
    不修改共享 file status flags。

## Validation

Agent-run validation:

- `just fmt kernel` 通过。
- `just build` 通过；仅保留既有 `anemone-kernel/src/sync/mono.rs` unused import warning。
- `git diff --check` 通过。
- 实现包含 KUnit-style 覆盖，且 `just build` 以 `--features kunit` 编译通过；本轮未单独启动
  QEMU 执行 KUnit runtime。

Review result:

- 实现 subagent 完成代码修改。
- review subagent 未发现 Apollyon / Keter blocker。
- review subagent 发现 `LOOP_CLR_FD` 可能把并发 block ioctl 的临时 `Arc<dyn BlockDev>` 误判为
  external ref；主控已通过 transient ref 计数修复。

未运行：

- 未运行 rv64 `mkfs.ext2 /dev/loop0` smoke。
- 未重跑 `fanotify13` 或 setup-heavy LTP。
- 未验证 mount/ext4 与 block devfs byte I/O 的混合并发一致性。

## Tracking Issues

### CHG-001 - Loop control plane linearization

**Status:** Neutralized
**Severity:** Euclid

**Issue:** 会改变 block devfs 可见容量或字节映射的 loop ioctl 需要与 byte I/O 共用线性化点。

**Resolution:** `LOOP_SET_FD`、`LOOP_SET_STATUS*` 和 `LOOP_CLR_FD` 的状态提交均通过
`BlockIoctlCtx::with_io_lock()` 进入每设备 `io_lock`，再进入 loop `state` lock。用户 copy、
backing fd lookup 和 backing file I/O 不在 loop `state` lock 下执行。

### CHG-002 - `LOOP_CLR_FD` external reference accounting

**Status:** Neutralized
**Severity:** Euclid

**Issue:** 新增共享 I/O state 后，devfs/private ioctl 的临时强引用不能导致 `LOOP_CLR_FD`
错误返回 `EBUSY`，但 mount/ext4 等持久外部引用仍必须阻止清除 backing。

**Resolution:** `BlockDevIoHandle` 只在 registry 中保存弱引用；block private ioctl 分发时用
transient guard 标记临时强引用。`LOOP_CLR_FD` 使用 persistent ref count，扣除 devfs/ioctl
临时引用后再与 registry + dispatch 基线比较。

### CHG-003 - Bounce buffer bound

**Status:** Neutralized
**Severity:** Safe

**Issue:** 非对齐读写需要 bounce buffer，但不能按一次大用户 I/O 的完整覆盖区间分配临时内存。

**Resolution:** 非对齐 RMW 使用单块 bounce buffer；对齐整块窗口按 `16 KiB` 上限分窗。

### CHG-004 - Runtime validation gap

**Status:** Deferred
**Severity:** Euclid

**Issue:** 本轮尚未提供 rv64 loop `mkfs.ext2` smoke 或 setup-heavy LTP 证据，不能证明
`fanotify13` setup 已越过旧失败点。

**Resolution:** 保持本记录为 `Follow-up`。后续应至少验证后端文件绑定到 `/dev/loop0` 后，
`lseek(/dev/loop0, 6272, SEEK_SET)` 成功、offset `6272` 处普通写不因非对齐失败，并确认
`mkfs.ext2 /dev/loop0` 越过旧 `lseek(6272, 0)` 失败点。若随后失败，应按 mount、filesystem
或 fanotify 重新归类。

## Risk / Follow-up

- 当前仍不支持 loop direct I/O、partscan、autoclear release hook、loop sysfs 或 partition nodes。
- mount/ext4 直接持有 `Arc<dyn BlockDev>` 的路径仍不参与这次 block devfs byte I/O handle；
  若后续需要全设备混合并发一致性，应升级为 block 子系统统一 handle 设计。
- 只有构建和 helper 覆盖，缺少实际 rv64 `mkfs.ext2` 与 LTP setup 证据；因此本记录不标记为
  完成态。

## Links

- Biweekly devlog: [2026-05-25 至 2026-06-07](../2026-05-25_to_2026-06-07.md)
- Register / limitations: [ANE-20260604-IOCTL-LTP-STAGE1-GAPS](../../register/current-limitations.md#ane-20260604-ioctl-ltp-stage1-gaps)
- RFC / transaction: [RFC-20260603-IOCTL-LOOP](../../rfcs/ioctl-loop/index.md), [IOCTL Loop 事务日志](../transactions/2026-06-04-ioctl-loop.md)
- Issue / PR / commit: current workspace diff; commit pending
