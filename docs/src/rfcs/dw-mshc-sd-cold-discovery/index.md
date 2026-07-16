# RFC-20260716-dw-mshc-sd-cold-discovery

**状态：** Accepted / Runtime Validation
**负责人：** EDGW_, Codex
**最后更新：** 2026-07-16
**领域：** device model / MMC / DW-MSHC / SD Memory / block / VisionFive 2
**事务日志：** [2026-07-16-dw-mshc-sd-cold-discovery](../../devlog/transactions/2026-07-16-dw-mshc-sd-cold-discovery.md)
**审查问题：** `MMC-SD-001` 至 `MMC-SD-010` 均已 Neutralized；处置见 [Tracking Issues](./tracking-issues.md)。
**下一步：** 执行 VisionFive 2 只读发现、整盘 ext4 rootfs 启动和用户授权的受控读写 gate。

## 摘要

本 RFC 固化 Anemone 当前已经形成的两阶段 MMC 路径：DW-MSHC platform driver 从 firmware
resource 与属性构造 protocol-neutral `MmcHostDevice`；一次性冷启动 discovery 通过 SD command
response 提交 `MmcCardDevice(SdMemory)`；MMC card bus 再绑定 `SdMemoryBlockDriver`，发布
512-byte logical-block `mmcblkN`。VisionFive 2 平台的实际工作区还把根文件系统从 `vda`
切换到 whole-disk `mmcblk0` ext4，因此 rootfs 启动属于本 RFC 的真实集成范围，而不是继续沿用
实现前材料中的非目标。

本 RFC 以 `c2c377479607c66a70383e6afdbb7336ad3be237` 的 host/controller 基线和 2026-07-16
工作区实现为事实来源。两轮代码审查确认 owner 分层总体成立，并修复 R1 error mask、MMIO resource、
PIO early completion/data HLE 和 SDSC byte-address geometry 四类 correctness 缺口。revision range、
generic compatible 与 firmware power-on delay 保留为用户确认的物理机信任边界；init argv 与 rootfs
输入 finding按用户决定接受且不采取行动，内核字符串规则改为优先 `Box<str>` 而非禁止 `String`。
本文已完成 review blocker closure，进入 runtime validation；未运行的实机和 KUnit runtime 不写成通过。

## 背景

当前实现已经建立以下路径：

```text
PlatformDevice
  -> DwMshcController                 # MMIO/readiness/applied IOS owner
  -> MmcHostDevice                    # stable slot identity
     -> one-shot cold discovery       # temporary protocol state owner
        -> MmcCardDevice(SdMemory)     # immutable card identity
           -> SdMemoryBlockDev         # geometry and synchronous block facade
              -> block registry name mmcblkN
```

实现与早期方案相比有四项应以代码为准的收敛：

- controller 使用带明确退出条件的 `SpinLock<DwMshcInner>`，而不是 sleepable `Mutex`；启用
  `spin_lock_irqsave` 时，单次 polling/PIO transaction 位于 IRQ-off 区间。
- discovery 是每个 host 在 platform probe 尾部执行一次的直接调用；没有全局可变 `MmcCore`、
  rescan worker 或 card-presence 副本。
- `MmcHostError::ResponseTimeout` 与 controller `CommandTimeout` 已分开；预期 probe timeout
  必须先恢复 transport 并重新建立 CMD0 baseline，才能继续候选协议。
- VisionFive 2 平台已选择 `mmcblk0` 作为 rootfs source，并增加专用 whole-disk ext4 rootfs
  composition；这扩大了 runtime acceptance boundary。

当前 agent 侧证据只有 `visionfive2-rv64` dev build。实现前的 VisionFive 2 controller identity
运行结果说明两个节点都报告 `VERID=0x290a`、`HCON=0x00c43cc1`、32-bit/32-entry FIFO；本轮没有
保存 Stage 2 card attach、读写或 rootfs 启动的原始实机日志，不能把这些运行项写成已通过。

## 目标

- 保持 platform controller、MMC host、protocol discovery、card device 和 block endpoint 的分层。
- 每个 firmware controller node 只创建一个 MMIO owner；当前 JH7110 只接受 HCON single-slot。
- firmware 属性只形成 host capability 和候选协议集合，不形成 card identity。
- 用 SD Physical Layer Simplified Specification v9.10 定义的 response 与 card register 提交
  SDSC、SDHC 或 SDXC identity；SDUC fail closed。
- 在完整 identity、selection 和 512-byte geometry 验证后才发布 `MmcCardDevice` 与 `mmcblkN`。
- 让 whole-disk `mmcblk0` ext4 成为 VisionFive 2 当前 rootfs 集成路径，并用实机启动证据验收。
- 所有 polling 都有 Kconfig hard deadline；rare failure 日志保留 host/card/LBA/opcode/typed cause。
- 在 IRQ、DMA、异步、并发 request、SDIO interrupt 或 hotplug 进入前强制更换当前长 SpinLock
  与 borrowed-buffer contract。

## 非目标

- 不实现 MMC/eMMC attach、eMMC block endpoint 或 EXT_CSD。
- 不实现 SDIO/SDIO-combo attach、function device、CIS 或 function IRQ。
- 不实现 runtime rescan、card detect、removal、unpublish 或 controller hot-remove。
- 不实现 CMD18/CMD25/CMD12 多块 transaction、scatter-gather、DMA 或 IDMAC。
- 不实现 4-bit切换、high-speed、1.8 V、UHS、DDR、HS200、HS400 或 tuning。
- 不实现 GPT/MBR partition scan；当前 rootfs 是 whole-disk ext4，不是 `mmcblk0pN`。
- 不在本 RFC 中直接编程 JH7110 clock/reset/pinctrl/syscon；继续使用 firmware handoff。
- `snps,dw-mshc` 与连续 revision range 只作为当前物理机兼容桥，不构成支持任意 DW-MSHC 的声明。
- 不在本 RFC 中继续修改或专项验证全局 init argv；只在 rootfs gate 观察 configured init 是否启动。

## 文档地图

Canonical：

- [DW-MSHC / SD cold discovery 不变量](./invariants.md)
- [迁移与验证计划](./implementation.md)
- [Tracking Issues](./tracking-issues.md)

执行事实：

- [DW-MSHC / SD cold discovery 事务日志](../../devlog/transactions/2026-07-16-dw-mshc-sd-cold-discovery.md)

## 方案

### Host 与 controller 边界

`DwMshcController` 唯一持有 `IoRemap`、register layout、readiness 和 applied IOS。
`DwMshcHost` 只是 slot facade；`MmcHostDevice` 承载 kernel-local identity 与 parent hierarchy，
host registry 只保存 weak association。MMIO address、FIFO depth/offset、bus width、clock range 和
allowed protocol kinds 全部来自 platform resource、firmware 与已实现能力的交集。

`MmcRequest<'a>` 保持同步 borrowed buffer。`execute()` 返回前必须完成 response capture 和所有
PIO copy；任何实现都不得把 slice 或 raw pointer 保存到另一个 task。controller request failure
将 readiness 锁定为 `RecoveryRequired`。transport recovery 会 reset controller、重新应用最后一次
完整 IOS，但会破坏 card session；只有 discovery owner 能在重新发送 CMD0 后继续识别。

### Discovery 与 identity commit

每个 host 在 platform probe 成功注册后执行一次 cold scan。支持的 SD Memory 路径为：

```text
OFF -> UP -> post-power-on delay -> ON -> CMD0
  -> optional CMD5 discriminator
  -> CMD8 or legacy timeout path
  -> CMD55/ACMD41 bounded loop
  -> CMD2 -> CMD3 -> CMD9 -> CSD/OCR validation
  -> CMD7 -> optional CMD16 for SDSC
  -> 25 MHz capped 1-bit/3.3 V/Legacy IOS
  -> publish immutable card identity
```

CMD5 有任何 response 时，本阶段不解析 R4，而是 fail closed 为 SDIO unavailable；CMD8
response timeout 只表示 legacy SD candidate，必须恢复 transport 后继续；controller timeout、CRC、
framing、FIFO 或 data error 不得当作候选不匹配。所有 R1/R6 card-status error 都必须在 protocol
层判错，不能只依赖 host transport completion。

### Card bus 与 block endpoint

`MmcCardDevice` 只有 immutable identity 和 weak host command capability。parent/children hierarchy
是 parent identity 的真相源；card bus 只按已提交的 `MmcCardKind` 匹配，不发送 discovery command。

`SdMemoryBlockDev` 的 geometry 只来自已验证 CSD：logical block 为 512 bytes，SDSC 使用 checked
byte address，SDHC/SDXC 使用 checked logical-block address。一个 block-layer multi-block buffer
按顺序拆成 CMD17 或 CMD24；CMD24 完成后必须等待 DAT busy release，再用 CMD13 确认
`READY_FOR_DATA + TRAN`。第 N 块失败时前 N-1 块可能已经完成，整体返回 `SysError::IO`。

### VisionFive 2 rootfs

当前平台在 kernel mount 阶段按 block registry name 查找 `mmcblk0`，并以 ext4 挂载 whole disk。
这要求进入 `mount_rootfs()` 前恰好有一个受支持的 SD Memory endpoint，或至少目标 SD card 稳定地
成为第一个 `BlockDevClass::Mmc` endpoint。若未来 MMC/eMMC 或第二张 SD card 也能发布 endpoint，
必须先引入稳定 root identity 或更新本 RFC；不得继续依赖偶然 probe 顺序。

## 接受边界

本 RFC 进入 Runtime Validation 的 review boundary 已满足：

- `MMC-SD-001`：R1 `UNDERRUN` / `OVERRUN` 不再被当成成功，并有 focused KUnit。
- `MMC-SD-002`：任何过短 MMIO resource 在首次 register access 前返回 probe error，不触发 assert。
- `MMC-SD-003`：`0x240a..=0x2fff` 与 generic compatible 是物理机兼容退让，只允许进入既有
  MMIO/HCON/FIFO fail-closed gate；支持机器具有稳定 SoC-specific identity 与 audited revision
  allowlist 后删除该桥。
- `MMC-SD-004`：按用户决定接受且不采取行动；不修改 `main.rs`，也不以跨平台 init argv 专项验证
  阻塞 MMC review closure。rootfs runtime gate仍要求 configured init 实际启动。
- `MMC-SD-005`：PIO write 在 buffer 未完全交给 FIFO 前观察到 `DATA_OVER` 时返回
  `ShortTransfer`，不再继续写 FIFO 或消费旧完成位为成功。
- `MMC-SD-006`：data-phase HLE 纳入唯一 error mask并映射为 `HardwareLocked`，即使与
  `DATA_OVER` 同时出现也不能成功。
- `MMC-SD-007`：CSD v1 只接受 `READ_BL_LEN=9..=11`，且 SDSC committed capacity 的最后一个
  512-byte logical block必须具有可编码的 `u32` byte-address command argument。
- `MMC-SD-008`：用户选择信任支持平台提供的 `post-power-on-delay-ms`，本轮不增加上限；该属性错误
  可使 cold discovery 长时间 busy-wait，是接受的 platform firmware 风险，不扩大为通用输入承诺。
- `MMC-SD-009`：仓库内核规则改为长期不可变字符串优先 `Box<str>`、确需构造或既有 API 时允许
  `String`；现有 block registry naming不再构成本 RFC blocker。
- `MMC-SD-010`：rootfs gitignored host input finding按用户决定接受且不采取行动；R4 仍只接受明确记录的
  用户侧实际 image/runtime 证据，不把本机已有 staging 或旧 image 当作通过。
- agent build、focused KUnit/source audit 与用户侧 VisionFive 2 smoke 被明确区分，未运行项不写 PASS。

本 RFC 完成还要求用户侧真实硬件证明：两个 controller 仍能 probe；SD slot 能 attach 并产生
正确容量的 `mmcblk0`；首块、末块和越界 read 行为符合合同；使用专用测试卡或用户明确授权区域完成
write/readback；whole-disk ext4 rootfs 能进入 configured init。任何 card-status 漏判、容量不一致、
rootfs 设备漂移、watchdog/IRQ latency 问题或 transport error 后的错误继续 I/O 都是停止信号。

## 备选方案

### 直接在 DW-MSHC driver 发布 block device

拒绝。controller firmware identity 不是 card protocol identity，且容量、寻址与 card kind 都必须来自
command response。该方案会把 protocol 和 block policy 泄漏到 MMIO owner。

### 引入全局 `MmcCore`、worker 和 request queue

延期。当前只有 boot-time one-shot discovery 和同步单块 I/O；额外 owner 会扩大状态与 buffer
lifetime。IRQ/DMA/hotplug gate 到来时必须用新的 RFC 修订同步 contract，而不是让 borrowed request
跨 task。

### 根据 bus width、node name 或 MMIO address 选择 SD/eMMC

拒绝。这些字段只描述 integration，不是 card identity。它们可以约束 candidate，但不能替代
CMD5、CMD8、ACMD41、CID/CSD/EXT_CSD 等协议证据。

### 保持 `vda` rootfs，MMC 仅作为次要块设备

已被当前实现替换。若产品仍需要 VirtIO rootfs，应由 platform config 选择另一个 platform/profile，
不能让同一 `visionfive2-rv64` 配置同时假定 `vda` 与 `mmcblk0`。

## 风险

- 500 ms polling deadline 位于 IRQ-off SpinLock 内；故障 hardware 可造成显著 latency。当前只接受
  同步单块路径，任何 watchdog 或 latency 证据都会触发 worker/sleepable owner redesign。
- rootfs 依赖 card discovery；任何 attach 或 block publication failure都会在 mount 阶段 fail fast。
- current block endpoint 不做 session recovery。一次 host transport error 后 controller fail closed，
  card 仍在 device hierarchy 中但后续 I/O 返回错误；这不是 hotplug/removal 语义。
- 当前没有 Synopsys databook，register 行为依赖受控公开实现证据和有界 compatibility bridge；扩大
  现有 revision range 或放宽 HCON/FIFO gate 前必须重新审计 reserved/W1C/self-clear/FIFO 语义。
- `post-power-on-delay-ms` 是当前支持平台的 trusted firmware input；本阶段不设上限，错误值可能延长
  cold discovery busy-wait。若后续接收不受控 firmware，必须先恢复 fallible bound validation。
- whole-disk write smoke 会破坏介质；agent 不得自行选择 MBR、GPT、filesystem metadata 或未知 LBA。

## 收口

文档与 code-review blocker 已收口，RFC 状态为 Accepted / Runtime Validation。两轮 review 的
correctness findings 已修复；硬件与 firmware 信任退让、字符串规则和不处理项已记录明确处置。
Stage 2 实机 attach/read/write/rootfs 与 KUnit runtime 证据仍未运行或未归档，因此 RFC 尚未 Closed。
