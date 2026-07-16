# DW-MSHC / SD Cold Discovery 不变量

**状态：** Accepted / Runtime Validation
**最后更新：** 2026-07-16
**父 RFC：** [RFC-20260716-dw-mshc-sd-cold-discovery](./index.md)

## 闭合条件

1. 一个 controller instance 只有一个 `IoRemap`、register layout、readiness 和 applied-IOS owner。
2. `MmcHostDevice` 是 host identity 与 hierarchy owner；host registry 只保存 weak lookup，不复制 caps。
3. firmware capability 只筛选候选协议；card kind、capacity 和 addressing 只来自成功的 protocol attach。
4. cold discovery 每个 boot-published host 只执行一次；不存在并列 rescan/card-presence 状态。
5. card 完整识别、选择、geometry 验证后才发布 child device 与 card-bus entry。
6. SD block geometry 只来自 immutable CSD snapshot；FIFO、resource、node name、host ID 都不能参与。
7. 任一 host transport failure 都使 controller fail closed；恢复 transport 后必须重建 card session。
8. `MmcRequest<'a>` buffer 不跨 `execute()` 返回点，不跨 task，也不保存 raw address。
9. 当前 revision range/generic match 是显式物理机兼容桥；MMIO/HCON/FIFO layout 仍 fail closed，
   扩大桥接范围前必须重新审查。
10. firmware MMIO/resource/layout 错误必须返回 probe error，不能通过 MMIO bounds assert panic；
    `post-power-on-delay-ms` 是用户批准的 trusted platform input，不在本阶段做数值上限校验。
11. VisionFive 2 rootfs 使用 whole-disk `mmcblk0` ext4；在支持多个 MMC endpoint 前，目标设备顺序必须稳定。

任一条件不成立时，只能把代码视为 review 中间态，不能把 RFC 标为 Accepted 或 Closed。

## 状态所有权

### Controller

`DwMshcController.inner` 是唯一行为 owner：

- `DwMshcRegs` 持有 `IoRemap`；所有 raw register/FIFO pointer 都是 lock 内短生命周期派生值。
- `DwMshcLayout` 是 probe 后稳定的行为 snapshot；其输入来自 VERID/HCON、firmware FIFO depth/
  override 和 resource length。
- `applied_ios` 只在完整 power/bus/clock transaction 成功后提交；failed partial IOS 不能成为 cache。
- readiness 只允许 `Probing -> Ready -> RecoveryRequired -> Recovering -> Ready/Offline`；card、host
  registry 和 block endpoint 不保存并列 ready bool。

`DwMshcHost` 保存 controller capability 与 slot 0 facade，不保存 MMIO pointer/readiness。当前
`SpinLock` 是同步执行 owner；它不是长期性能 abstraction。

### Discovery

cold discovery 局部变量拥有 probe order、OCR/RCA/CID/CSD 和未提交 attach phase。失败时这些状态
随调用返回消失；不得复制进 controller 或 registry。只有 protocol attach 完整成功后才能构造
immutable `MmcCardIdentity`。

### Card 与 block endpoint

`MmcCardDevice.identity` 是 card kind、RCA、CID、CSD、addressing 与 capacity 的唯一提交来源。
card 的 weak host handle 只是 command capability；`DeviceBase.parent` 才是 hierarchy truth source。

`SdMemoryBlockDev.total_blocks` 是从 immutable capacity 派生的稳定 geometry。Stage 2 没有 replacement
或 hotplug，因此允许缓存该值；字段必须保留 truth source 和无 stale 条件的注释。

## 硬件与 firmware 信任边界

- platform `Resource::Mmio` 的 base/length 在首次 `VERID/HCON` read 前验证；length 至少覆盖本实现
  访问的最高普通 register，并单独验证 FIFO override 的 exact-width access。
- `ptr_at()` 的 assert 只保护已验证 owner 内部不变量，不能承担外部 firmware input validation。
- 当前 runtime 证据只覆盖 JH7110 `VERID=0x290a`、`HCON=0x00c43cc1` 与 single-slot、32-bit FIFO。
- `snps,dw-mshc` 与 `0x240a..=0x2fff` revision range 是为了覆盖现有物理机 firmware 差异而接受的
  compatibility bridge，不是任意 DW-MSHC 支持声明。bridge 只放行到现有 MMIO length、single-slot、
  FIFO width/depth/offset gate，任何一项不匹配仍拒绝 probe。
- bridge 的保留依据是当前支持物理机的 firmware identity/revision 差异；单台 JH7110 `0x290a`
  观测不能代表全部支持机器，也不能反向放宽其它 layout invariant。所有支持机器提供稳定
  SoC-specific compatible，且 revision 具有资料审计、layout test 与目标 hardware smoke 后，删除
  generic match并收窄为实际 allowlist。
- 扩大当前 revision range、接受新的 HCON/FIFO shape 或新增 generic integration 前，必须有 databook
  或等价公开实现审计、layout test 与目标 hardware smoke。
- bus width、FIFO depth、clock rate、MMIO base 和 card kind 不得从当前板数值硬编码。
- `post-power-on-delay-ms` 由当前支持平台 firmware 保证合理，本实现直接 busy-wait该时长。若支持来源
  扩大到不受控 firmware，必须在继续 probe 前增加 fallible 上限校验，不能保留这一信任退让。

## 同步 request 与锁规则

1. `set_ios()`、`execute()`、`recover_transport()` 获取同一个 controller SpinLock。
2. lock 内不分配 heap、不注册 device、不获取 host/card/block registry lock，也不执行 protocol poll interval。
3. 启用 `spin_lock_irqsave` 时，clock update、command/data polling、PIO、DAT busy wait 和 recovery 都在
   IRQ-off 区间；所有 loop 必须受 `DW_MSHC_POLL_TIMEOUT_MS` 约束。
4. borrowed read/write slice 只在该同步临界区内访问；返回后 host 不再持有其地址。
5. block subsystem 的 per-endpoint I/O mutex 位于 controller SpinLock 外；controller path 不反向获取它。
6. 引入 IRQ handler、DMA、concurrent outstanding request、cancellation、SDIO interrupt 或 runtime
   rescan 前，必须先替换长 SpinLock 并修改 request ownership contract。

## Error 与恢复协议

- `ResponseTimeout` 表示已提交 command 没有 card response；只有 optional discriminator 或明确候选
  probe 可以把它解释为 non-match。
- `CommandTimeout` 表示 controller completion deadline；不得继续尝试其它协议。
- CRC、framing、FIFO、data timeout、short transfer 和 hardware lock 都是 transport failure。
- write buffer 尚未完全交给 FIFO 时出现 `DATA_OVER` 必须返回 `ShortTransfer`；data-phase HLE 必须
  映射为 `HardwareLocked`，不能因同时出现 completion bit而返回成功。
- `execute()` 发生任何 host error 后把 readiness 锁定为 `RecoveryRequired`；后续 IOS/request 必须失败。
- `recover_transport()` reset controller 并重放最后完整 IOS，但不保留 identified/selected session。
- discovery 只有在 recovery、post-power-on delay 和 CMD0 后才能继续；block endpoint 当前不做盲恢复，
  因为它无法在 endpoint I/O transaction 内重建并重新发布 card identity。

cleanup 或 failure path 应先撤销可撤销的内部状态，再断言 owner invariant；当前不可撤销的 boot-only
parent/bus/block publication必须发生在所有 fallible identity preparation 之后。

## SD protocol 不变量

### Candidate 与 attach

- `allowed_kinds` 只过滤 candidates。node name、MMIO address、bus width、removable 属性不能形成 kind。
- 允许 SDIO 时，CMD5 只作为不解析 R4 的 discriminator；任何 response 都 fail closed，不把 combo 降级。
- CMD8 valid R7 选择 v2 path；response timeout 选择 legacy path；malformed echo/voltage 拒绝 SD candidate。
- 每次 expected response timeout 后必须 recover transport 并重新发送 CMD0，不能复用迟到 response 状态。
- ACMD41 每轮都有 CMD55，整体受 init deadline 约束；只有 OCR power-up complete 且 voltage compatible
  才能继续 identity。

### Response 与 card status

- R2 在 host boundary 规范化为 `[127:96]..[31:0]`，CSD/CID bit extraction 只消费该顺序。
- R6 必须拒绝 zero RCA 和所有定义 error bit。
- R1 `check()` 必须至少拒绝 SD v9.10 card-status 中所有 error bit，包括
  `OUT_OF_RANGE`、`ADDRESS_ERROR`、`BLOCK_LEN_ERROR`、`ERASE_SEQ_ERROR`、`ERASE_PARAM`、
  `WP_VIOLATION`、`CARD_IS_LOCKED`、`LOCK_UNLOCK_FAILED`、`COM_CRC_ERROR`、`ILLEGAL_COMMAND`、
  `CARD_ECC_FAILED`、`CC_ERROR`、`ERROR`、`UNDERRUN`、`OVERRUN`、`CSD_OVERWRITE`、
  `WP_ERASE_SKIP`、`SWITCH_ERROR` 与 `AKE_SEQ_ERROR`。
- host transport completion 不能覆盖 card-status error；CMD17/CMD24/CMD13 只有 transport 与 R1
  同时成功才返回成功。

### Capacity 与 addressing

- CSD v1 且 OCR CCS clear、`READ_BL_LEN` 位于 `9..=11` 时才提交 SDSC byte addressing；capacity
  使用 checked `(C_SIZE + 1) * 2^(C_SIZE_MULT + 2) * 2^READ_BL_LEN`。
- CSD v2 且 OCR CCS set 才提交 SDHC/SDXC block addressing；capacity 使用 checked
  `(C_SIZE + 1) * 512 KiB`。
- capacity 必须非零并被 512 整除。CSD structure/addressing 组合不一致时 identity 无效。
- CSD v3/SDUC fail closed；不得截断为 SDHC。
- SDSC command argument 是 checked `lba * 512`；SDHC/SDXC argument 是 checked `lba`；
  转为 `u32` 失败时不提交 command。
- SDSC identity commit 前必须证明最后一个 512-byte logical block 的起始 byte address可编码为 `u32`；
  endpoint 不得发布一个自身 command domain 无法覆盖的 geometry。

## Block 与 rootfs 不变量

- `BlockDevClass::Mmc` 只负责 `mmcblkN` namespace；card ID、host ID、devnum、block name 相互独立。
- endpoint logical block size固定为 512，`total_blocks = capacity_bytes / 512` 使用 checked conversion。
- I/O 前验证 non-empty、512-byte alignment、checked range end 与 `end <= total_blocks`。
- multi-block buffer 只拆成顺序单块 CMD17/CMD24；不发送 CMD18/CMD25/CMD12。
- write 的前序 chunk 可以在后续失败前已经持久化；BlockDev 无 partial-count contract，日志必须给出
  card/LBA/opcode/typed cause。
- CMD24 后 controller 等 DAT busy release，protocol 再用 CMD13 确认 `READY_FOR_DATA` 和 `TRAN`。
- rootfs mount 发生在 physical discovery 后，直接按 block registry name 查找 `mmcblk0`；找不到时
  fail fast，不发布容量为零或未识别 card 的 placeholder。
- 当前 whole-disk ext4 rootfs 假定只发布一个受支持 SD endpoint。出现多个可发布 MMC endpoint 前，
  必须更新 root identity contract，不能让注册时序成为长期设备身份。

## 禁止退化项

- 让 controller 解析 OCR、RCA、CID、CSD 或 block addressing。
- 让 discovery/block driver 直接访问 DW private register。
- 在 host、card 或 endpoint 中复制 controller readiness、applied IOS 或可派生 card kind。
- 用 firmware capability、8-bit/4-bit wiring、node name或 address 冒充 card identity。
- 在已接受 compatibility bridge 之外继续接受 unknown VERID/HCON，只因为 register offsets 看起来相似。
- 用 `debug_assert!` 保护轻量 correctness invariant，或用 assert 替代 external config error。
- 在 response timeout 后不恢复 transport就继续下一个 command。
- 忽略 R1 `UNDERRUN` / `OVERRUN` 或其它 card-status error并返回成功。
- 在 write buffer 未完全提交时忽略 `DATA_OVER`，或在 data phase 忽略 HLE。
- 发布最后一个 LBA 无法形成 `u32` byte address 的 SDSC geometry。
- 为了 rootfs 启动而发布未完成 CSD/selection 的 block endpoint。
- 在没有 write authorization 的介质区域运行 destructive smoke。

## 完成标准

- focused KUnit 覆盖 R1 全 error mask、PIO early completion/data HLE、CSD v1/v2/invalid/SDUC、
  address/range overflow、timeout 分类、revision/resource gate 和 CMD17/CMD24/CMD13 sequence。
- source audit确认一个 controller owner、一个 cold-discovery call site、无 worker/queue/private Mutex、
  无板载地址/card-kind硬编码。
- `just build`、`git diff --check` 与 mdBook build 通过。
- 用户侧 VisionFive 2 证据覆盖 controller probe、SD attach/capacity、首末块只读、授权 write/readback 和
  whole-disk ext4 rootfs/init。
- 事务日志明确区分 agent-run、user-run 与未运行验证，并记录所有 accepted limitations。
