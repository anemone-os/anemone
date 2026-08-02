# AHCI Controller Tracking Issues

**状态：** Terminated / Frozen Historical Snapshot

> 父RFC已在未接受状态终止，本页冻结为termination-time问题证据，不再维护current状态、active gate、修复
> 路线或关闭条件。AHCI-001/002对应的live safety defect只由
> [register open issue](../../register/open-issues.md#ane-20260723-ahci-probe-lifecycle-and-capacity)拥有；
> shutdown、timeout与验证边界继续由[current limitation](../../register/current-limitations.md#ane-20260723-ahci-stage1-scope)
> 拥有。未来若另行授权，只能建立新的实施边界并回写register，不能重新激活本tracking页。

## AHCI-001 - Probe failure can release live DMA owner

**等级：** Apollyon
**终止时快照：** Unresolved；live authority见[register issue](../../register/open-issues.md#ane-20260723-ahci-probe-lifecycle-and-capacity)
**位置：** `anemone-kernel/src/driver/ahci/port.rs`、`mod.rs`

`AhciPort::initialize()` 会启动 FIS/command engine，随后 IDENTIFY、`parse_identify()`、minor
allocation 或 block registration 的失败出口可能直接丢弃 `AhciPort` / `AhciController`。当前没有
等价的 `Drop` cleanup；DMA metadata/bounce 或 MMIO mapping 释放后，HBA 仍可能访问旧地址，形成
设备 DMA 到已释放内存的风险。

## AHCI-002 - IDENTIFY capacity can reach an internal FIS assertion

**等级：** Apollyon
**终止时快照：** Unresolved；live authority见[register issue](../../register/open-issues.md#ane-20260723-ahci-probe-lifecycle-and-capacity)
**位置：** `anemone-kernel/src/driver/ahci/ata.rs`、`fis.rs`

当前只把四个 LBA48 words 合成为 `u64` 并转换为 `usize`，没有拒绝 `sectors > 2^48`。恶意或
异常设备 response 进入后续 read 时会触发 `command_fis()` 的 `assert!(lba < 1 << 48)`，把设备输入
升级为 kernel panic。

## AHCI-003 - Shutdown does not quiesce the controller

**等级：** Keter
**终止时快照：** Unresolved；live authority见[current limitation](../../register/current-limitations.md#ane-20260723-ahci-stage1-scope)
**位置：** `anemone-kernel/src/driver/ahci/mod.rs`

`AtaDisk::quiesce()` 已存在，但 `AhciDriver::shutdown()` 当前只记录 notice 并跳过 quiesce。系统
shutdown 或 driver teardown 时 command engine、FIS receive 和 device cache policy 没有明确的停止
边界；这与生命周期不变量和后续 resource reclamation 相冲突。

## AHCI-004 - Read timeout panic is a temporary diagnostic bridge

**等级：** Euclid
**终止时快照：** Unresolved；live authority见[current limitation](../../register/current-limitations.md#ane-20260723-ahci-stage1-scope)
**位置：** `anemone-kernel/src/driver/ahci/port.rs`

read watchdog 在 `AHCI_READ_TIMEOUT_MS` 后直接 panic。它保留了 controller hang 的现场，但把可恢复
的 block failure 变成全系统 crash，并且与普通 command 的 `SysError::Timeout` 不一致。

## AHCI-005 - Hardware/runtime vertical-slice evidence is not available

**等级：** Safe
**终止时快照：** Partial / hardware Not Run；live authority见[current limitation](../../register/current-limitations.md#ane-20260723-ahci-stage1-scope)
**位置：** validation boundary

原基线只运行LoongArch `just build`和`git diff --check`。截至终止，本次合流的RV64/LA64完整KUnit runtime
各自通过10个已注册AHCI helper case；但`ata.rs`的IDENTIFY helper没有`#[kunit]`注册，capacity upper-bound
regression/source audit仍缺失。真实AHCI hardware probe、first/last-sector read、越界read、用户授权
write/readback、shutdown或reboot稳定性均Not Run。
