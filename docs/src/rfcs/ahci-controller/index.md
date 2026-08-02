# AHCI Controller / ATA Block Device

**状态：** Terminated
**修订：** `Draft`（未接受即终止）
**负责人：** EDGW, Codex
**最后更新：** 2026-08-01
**领域：** AHCI / SATA / ATA / DMA / block
**开放问题：** [Tracking Issues](./tracking-issues.md)
**事务日志：** [AHCI Controller 事务日志](../../devlog/transactions/2026-07-23-ahci-controller.md)
**下一步：** None；本RFC不再推进，任何后续AHCI工作必须重新授权，不能恢复下列gate。

> 本RFC于2026-08-01按维护者决定终止。它从未进入Accepted或current-contract cutover，`Terminated`不表示
> 原接受边界已满足，也不把未运行证据改写为PASS。既有实现与历史证据保留；仍影响live code的安全缺陷和
> 可见限制继续由register拥有。原Draft中的partition-scan non-goal只约束当时不在AHCI driver内实现扫描，
> 不限制后来由generic block subsystem提供、AHCI当前已经消费的one-time primary MBR discovery。

## 背景

提交 `48f86615` 增加了一个面向 firmware-described generic AHCI HBA 的第一阶段驱动，并在
`d6875c69` 将其从 `driver/block/ahci` 移到 `driver/ahci`。后者是同一 driver owner 内的结构
移动，不改变 block ABI 或运行时行为；当前 canonical code path 是
`anemone-kernel/src/driver/ahci/`。

Loongson 2K1000 的设备树提供 `loongson,ls-ahci` 和 `generic-ahci` compatible、MMIO resource、
`dma-mask`，并由 firmware 声明 coherent DMA。驱动把一个已识别的 ATA disk 注册为 SCSI class
block device，当前命名规则因此产生 `sda`。

## 历史目标（未接受）

- 发现并初始化一个 AHCI 1.x HBA 的单一已实现 port。
- 通过 ATA `IDENTIFY DEVICE` 证明目标是支持 DMA、LBA、LBA48、512-byte logical sector 和
  `FLUSH CACHE EXT` 的 ATA disk 后，发布稳定的 immutable identity 与容量。
- 以 slot zero、单 PRD 和可复用 DMA bounce buffer 提供同步 LBA48 DMA EXT read/write。
- 在错误、链路丢失、短传输和 port offline 时 fail closed，并输出足够的寄存器诊断信息。
- 保持 platform firmware 负责 pinmux、clock、reset、PHY 和 DMA coherency setup；generic AHCI
  driver 不按板载 MMIO 地址猜测控制器身份。

## 历史非目标

原Draft不承诺：多 port 或多 outstanding command、NCQ、IRQ completion、异步 request queue、
ATAPI、port multiplier、runtime hotplug、partition scan、power management、真实 cache flush
持久化语义或 controller resource reclamation。上述能力需要重新审查 owner、生命周期和验证边界，
不能在当前同步 port lock 内旁路实现。

这里的`partition scan`只排除AHCI-owned scanner，不排除独立generic block能力。当前AHCI registration已通过
`register_block_disk()`消费generic primary MBR discovery；GPT、重扫和可变partition-table lifecycle仍不由
本历史Draft声明完成。

## 终止时的实现事实（非current contract）

### Platform probe

`AhciDriver` 只通过 platform bus 的 `generic-ahci` 和 `loongson,ls-ahci` compatible 匹配。probe
要求 firmware node 提供 `dma-mask`、coherent DMA（节点或父节点声明）和至少一个可用 physical
memory zone。AHCI register view 在首次访问前检查 host baseline mapping；实现还要求 AHCI 1.x、
`CAP.PI` 恰好只有一个 bit，并检查该 port 的完整 register window 落在 MMIO resource 内。

DMA aperture 是 firmware mask、HBA 32/64-bit address capability 与 allocator 可用物理内存的
交集。当前 allocator 没有按 controller mask 单独分配，因此只在全部可分配物理地址都能落入
effective mask 时接受 probe；否则 fail closed。

### Controller and port

`AhciController` 是 block facade 的唯一同步 owner，内部 `SpinLock<AhciPort>` 串行化一次完整
command/DMA transaction，并在当前 `spin_lock_irqsave` 配置下关闭本地中断。`AhciPort` 唯一拥有
MMIO register view、DMA metadata/bounce storage、supported capability snapshot、port identity 和
`Probing -> Ready -> Recovering -> Offline` readiness。

初始化顺序固定为 HBA reset、停止旧 engine、关闭 port interrupt、写入 command-list/received-FIS
base、清除 W1C status、启用 FIS receive、必要时 COMRESET、等待 task-file ready、校验 ATA signature，
最后启动 command-list engine。每次 command 只使用 slot zero；完成后检查 command issue、port
interrupt、task-file、link presence 和 transferred-byte count。

### ATA identity and block ABI

IDENTIFY response当前验证DMA/LBA、LBA48/FLUSH CACHE EXT、512-byte logical-sector-size、nonzero capacity
与host `usize` conversion后提交为`AtaIdentity`，但没有拒绝`sectors > 2^48`；该live defect由
[register issue](../../register/open-issues.md#ane-20260723-ahci-probe-lifecycle-and-capacity)拥有。model、
serial、firmware是immutable `Box<str>` diagnostic snapshot，不参与I/O状态决策。`AtaDisk`保留identity
和controller capability，通过block registry注册SCSI-class `sdN` endpoint。

block API 的最小单位是 512 bytes；I/O length 必须非零且 512-byte 对齐，range 必须在 identity
capacity 内。大于单次 bounce buffer 的请求按连续 sector chunk 拆分；任何 chunk 失败都停止后续
I/O。

### Error and observability

port interrupt 按 host-bus fatal、host-bus data、interface fatal、task-file、overflow、interface
non-fatal 的固定优先级分类。错误会记录 HBA/port register snapshot，尝试停止并重启仍连接的 port，
然后映射到 `SysError`；link loss 或 recovery failure 使 port 永久 Offline。

普通 command 使用 kconfig timeout。ATA read 另有 slow-read warning；当前达到 read timeout 会
panic，以保留可复现 controller hang 的完整上下文。这是临时诊断策略，不是最终 block ABI，退出
条件见 [Tracking Issues](./tracking-issues.md)。

## 终止时未满足的接受边界

终止时只能把 code shape、fake/helper KUnit 和 Loongson 2K1000 firmware integration 作为实现事实，
不能把RFC标为Accepted或Closed；下列未满足项不再形成active gate：

- probe 的所有失败出口必须保证 engine 停止后才释放 DMA/MMIO owner；
- IDENTIFY capacity 必须先证明可编码进 48-bit FIS，并拒绝会触发内部 assertion 的设备响应；
- shutdown 必须明确 quiesce/flush 的可见语义，不能保留当前无操作 stub 作为长期合同；
- 已注册AHCI helper KUnit已随本次合流在RV64/LA64 runtime通过，但capacity upper-bound regression/source
  audit仍缺失；用户侧真实controller probe、first/last-sector read、越界read、授权介质write/readback和
  重启稳定性验证仍Not Run。

未运行的测试不记录为 PASS；destructive write 只能由用户指定 disposable media 或明确 LBA 后执行。

## 历史方案取舍

### 直接放回 block 目录

拒绝。AHCI platform matching、MMIO/DMA 生命周期和 controller state 属于 driver owner；block
目录只应承载 block subsystem 与 block-specific driver facade。`d6875c69` 的目录移动保持了
`BlockDev` registration 方向，同时让后续 controller lifecycle 不继续挤入 block module。

### 引入通用异步 storage queue

延期。当前只需要 boot-time probe 和同步单命令 I/O；引入 worker、IRQ completion 或 borrowed request
会扩大 request lifetime、shutdown 和 cancellation contract，必须另开 revision 或 follow-up RFC。

### 按 SoC 地址选择控制器

拒绝。MMIO address 只能是 firmware resource；driver 只接受 firmware compatible 和 audited AHCI
layout，2K1000 的 pinmux/clock/PHY 仍由 firmware 负责。

## 修订记录

| 修订 | 日期 | 变化 | 证据 |
| --- | --- | --- | --- |
| Draft | 2026-07-23 | 依据 `48f86615`、`d6875c69` 固化 AHCI 第一阶段实现事实与结构 owner；尚未接受进入实现 | [事务日志](../../devlog/transactions/2026-07-23-ahci-controller.md) |
