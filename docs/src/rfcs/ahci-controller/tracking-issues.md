# AHCI Controller Tracking Issues

## AHCI-001 - Probe failure can release live DMA owner

**等级：** Apollyon
**状态：** Open
**位置：** `anemone-kernel/src/driver/ahci/port.rs`、`mod.rs`

`AhciPort::initialize()` 会启动 FIS/command engine，随后 IDENTIFY、`parse_identify()`、minor
allocation 或 block registration 的失败出口可能直接丢弃 `AhciPort` / `AhciController`。当前没有
等价的 `Drop` cleanup；DMA metadata/bounce 或 MMIO mapping 释放后，HBA 仍可能访问旧地址，形成
设备 DMA 到已释放内存的风险。

**修复方向：** 为 probe rollback 和 driver shutdown 建立同一个 owner cleanup path：先停止 command
engine/FIS receive，确认停止或 fail closed，再释放 DMA/MMIO。对每个 post-start `?` 出口增加测试或
source audit；cleanup 路径先撤销发布状态，再使用 `assert!` 暴露违反的生命周期不变量。

## AHCI-002 - IDENTIFY capacity can reach an internal FIS assertion

**等级：** Apollyon
**状态：** Open
**位置：** `anemone-kernel/src/driver/ahci/ata.rs`、`fis.rs`

当前只把四个 LBA48 words 合成为 `u64` 并转换为 `usize`，没有拒绝 `sectors > 2^48`。恶意或
异常设备 response 进入后续 read 时会触发 `command_fis()` 的 `assert!(lba < 1 << 48)`，把设备输入
升级为 kernel panic。

**修复方向：** 在 identity commit 前 checked-validate `0 < sectors <= 2^48`，并补覆盖 zero、exact
 upper capacity boundary、out-of-range 和 `usize` conversion 的 focused KUnit；FIS builder 的 assertion
  只能保护内部已验证调用者，不能代替设备输入校验。

## AHCI-003 - Shutdown does not quiesce the controller

**等级：** Keter
**状态：** Open
**位置：** `anemone-kernel/src/driver/ahci/mod.rs`

`AtaDisk::quiesce()` 已存在，但 `AhciDriver::shutdown()` 当前只记录 notice 并跳过 quiesce。系统
shutdown 或 driver teardown 时 command engine、FIS receive 和 device cache policy 没有明确的停止
边界；这与生命周期不变量和后续 resource reclamation 相冲突。

**修复方向：** 从 platform driver state 取得唯一 `AtaDisk` owner，调用 quiesce，并在是否发送
`FLUSH CACHE EXT` 后固定可见语义。若本阶段明确接受不保证 durable write，必须在 block contract、
current limitations 和用户可见文档中一致声明；不能保留未说明的 silent no-op。

## AHCI-004 - Read timeout panic is a temporary diagnostic bridge

**等级：** Euclid
**状态：** Open
**位置：** `anemone-kernel/src/driver/ahci/port.rs`

read watchdog 在 `AHCI_READ_TIMEOUT_MS` 后直接 panic。它保留了 controller hang 的现场，但把可恢复
的 block failure 变成全系统 crash，并且与普通 command 的 `SysError::Timeout` 不一致。

**修复方向：** 在获得等价的 post-failure register capture 和 fail-closed recovery 后，改为返回明确
的 timeout/error；删除条件写在 implementation gate 中。runtime validation 期间必须把该行为作为
已知 fail-stop 诊断策略，而不是 PASS 证据。

## AHCI-005 - Runtime evidence is not yet available

**等级：** Safe
**状态：** Open
**位置：** validation boundary

agent 已运行 LoongArch `just build` 和 `git diff --check`，但尚未运行 KUnit runtime、AHCI hardware
probe、first/last-sector read、越界 read、用户授权 write/readback、shutdown 或 reboot 稳定性验证。

**关闭条件：** 完成 focused KUnit/source audit，并由用户在 2K1000 或等价 generic AHCI 平台归档
controller identity、capacity、读写和失败路径证据；未运行项目继续保持 Not Run。
