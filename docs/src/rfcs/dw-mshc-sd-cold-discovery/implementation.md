# DW-MSHC / SD Cold Discovery 迁移与验证计划

**状态：** Active / Runtime Validation
**最后更新：** 2026-07-16
**父 RFC：** [RFC-20260716-dw-mshc-sd-cold-discovery](./index.md)
**不变量：** [DW-MSHC / SD Cold Discovery 不变量](./invariants.md)
**Tracking Issues：** [DW-MSHC / SD Cold Discovery Tracking Issues](./tracking-issues.md)

## 当前基线

实现已经先于公共 RFC 存在：

- commit `c2c377479607c66a70383e6afdbb7336ad3be237` 提供 `device/mmc` host contract、weak host
  registry、DW-MSHC register/layout/controller 和 platform publication。
- 2026-07-16 工作区增加 card bus、one-shot SD discovery、immutable card identity、SD block endpoint、
  MMC Kconfig policy、VisionFive 2 `mmcblk0` rootfs 和专用 rootfs composition。
- 当前 active `kconfig` 为 `visionfive2-rv64` dev，启用 `spin_lock_irqsave`、`fs_ext4`、`kunit`。
- agent 已运行 `just build` 并成功生成 kernel/UBoot image；没有运行 Stage 2 KUnit runtime、QEMU 或
  VisionFive 2 实机 attach/read/write/rootfs。
- `MMC-SD-001` 与 `MMC-SD-002` 已修复；当前 `kunit` 配置下 `just build` 证明新增 focused test可编译。
- `MMC-SD-003` 作为用户批准的物理机兼容退让保留，`MMC-SD-004` 按用户决定接受且不采取行动。
- 第二轮 review 的 `MMC-SD-005` 至 `MMC-SD-007` 已修复并由 focused KUnit保护；`just build` 通过。
- `MMC-SD-008` 至 `MMC-SD-010` 分别按 trusted firmware、字符串规则调整和接受不处理完成处置。

post-implementation review finding 已折回 canonical contract。当前阶段只执行既有 R1-R4 runtime
validation，不扩大功能或把未运行项宣称为通过。

## Gate R0：Review blocker neutralization（Completed）

**状态：** Completed / runtime KUnit not run

**目标：** 消除错误成功与 firmware panic，并把已接受的硬件兼容退让固化为有边界、可退出的合同。

**Write set：**

```text
anemone-kernel/src/device/mmc/discovery/sd.rs
anemone-kernel/src/driver/mmc/dw_mshc/regs.rs
anemone-kernel/src/driver/mmc/dw_mshc/controller.rs
docs/src/rfcs/dw-mshc-sd-cold-discovery/*
docs/src/rfcs.md
docs/src/devlog/transactions/2026-07-16-dw-mshc-sd-cold-discovery.md
docs/src/devlog/transactions/index.md
docs/src/devlog/2026-07-06_to_2026-07-19.md
```

**交付：**

1. 为 R1 增加 `UNDERRUN` / `OVERRUN` named flags，并纳入唯一 `ERRORS` mask；focused test 枚举
   每个 error bit，证明 `check()` 全部拒绝，同时证明 `READY_FOR_DATA + TRAN` 不误判。
2. controller probe 在构造/读取 `DwMshcRegs` 前验证 MMIO length 覆盖 baseline register window；
   FIFO override 继续在 layout decode 中独立验证。external error 返回 `SysError`，不触发 `ptr_at` assert。
3. revision range 与 `snps,dw-mshc` 按用户决定保留为物理机兼容退让；canonical 文本记录保留原因、
   只进入严格 layout gate 的行为边界，以及稳定 SoC identity/audited allowlist 可用后的删除条件。
4. init argv Euclid按用户决定接受且不采取行动，不修改 `main.rs`，不增加跨平台专项验证 gate。

**验证 floor：**

- focused KUnit/source tests：R1 mask、short MMIO；`just build` 编译测试体，runtime KUnit 单独记录。
- `rg` 确认 production revision range/generic match 与文档记录的 compatibility bridge 一致。
- `just build`，使用当前 VisionFive 2 config。
- `git diff --check`。

**停止条件：**

- 修复 Apollyon 需要放宽 MMIO、R1 或 layout correctness invariant；
- compatibility bridge 必须继续扩大 revision/HCON/FIFO 支持面；
- init argv 行为导致 configured init 在 runtime rootfs gate 无法启动。

**回写：** finding 状态写 `tracking-issues.md`；实际修改、测试和 build 追加 transaction。若兼容桥
需要扩大 revision/HCON/FIFO 或 integration 范围，先更新 RFC index/invariants，再继续。

## Gate R0.1：Second review correction（Completed）

**状态：** Completed / runtime KUnit not run

**Write set：**

```text
anemone-kernel/src/driver/mmc/dw_mshc/{controller.rs,regs.rs}
anemone-kernel/src/device/mmc/discovery/sd.rs
AGENTS.md
docs/src/rfcs/dw-mshc-sd-cold-discovery/*
docs/src/devlog/transactions/2026-07-16-dw-mshc-sd-cold-discovery.md
docs/src/devlog/{2026-07-06_to_2026-07-19.md,transactions/index.md}
docs/src/rfcs.md
```

**交付：**

1. write buffer 未完全提交时观察到 `DATA_OVER`，清除该 W1C cause并返回 `ShortTransfer`。
2. HLE 纳入 data error mask，纯分类 helper保证 `HARDWARE_LOCKED | DATA_OVER` 仍返回
   `HardwareLocked`。
3. CSD v1 拒绝保留 `READ_BL_LEN`，并在 identity commit 前验证完整 SDSC geometry 可由 `u32`
   byte-address覆盖。
4. 用户选择信任 `post-power-on-delay-ms`，不修改代码；字符串规则改为 `Box<str>` 优先而非禁止
   `String`；rootfs host input finding接受且不处理。

**验证 floor：** focused KUnit编译、`just build`、`just fmt kernel --check`、`git diff --check` 与
文档链接/source audit。KUnit runtime与实机保持后续 gate，未运行项不记为 PASS。

**退出条件：** 三个 correctness helper/test体可编译，source audit确认完成/error mask/geometry合同已折回
canonical invariants，用户处置完整进入 tracking/transaction。

## Gate R1：Host/controller contract closure

**前置：** Gate R0 review findings 全部 Neutralized。

**Write set：**

```text
anemone-kernel/src/device/mmc/host.rs
anemone-kernel/src/device/mmc/registry.rs
anemone-kernel/src/driver/mmc/dw_mshc/{fwnode.rs,regs.rs,controller.rs,mod.rs}
conf/.defconfig
scripts/xtask/src/config/kconfig.rs
```

**审计：**

- `IoRemap`、readiness、applied IOS 各只有一个 owner；registry 不复制 caps/controller state。
- response timeout 与 controller timeout 分流；所有 submitted host error latch `RecoveryRequired`。
- recovery 重放 last committed IOS，但注释明确 card session 已失效。
- request validation覆盖 opcode、IOS、block count/size、checked bytes、buffer length 与 FIFO word alignment。
- `SpinLock<DwMshcInner>` 恰好一个，带 IRQ/DMA/async/hotplug 前替换的退出条件。
- firmware clock、bus width、FIFO depth/data offset 与 protocol candidates 都在 publication 前验证。

**验证 floor：**

- clock-divider boundary、response/data flag matrix、layout/slot/FIFO tests。
- source audit：driver 内无板载 MMIO address target selection，无 private worker/queue/Mutex。
- `just build`。

**硬件只读 probe：** 用户运行并保存两个 controller 的 device/resource/VERID/HCON/FIFO/caps 日志；
本 gate 不要求 card I/O。observed revision 超出兼容桥或 layout 不满足现有 gate时立即停止并回到
R0/RFC review。

## Gate R2：SD discovery vertical slice

**前置：** R1 hardware identity/layout probe 通过。

**Write set：**

```text
anemone-kernel/src/device/mmc/{card.rs,bus.rs,discovery/mod.rs,discovery/sd.rs,mod.rs}
anemone-kernel/src/driver/mmc/dw_mshc/{controller.rs,mod.rs}
conf/.defconfig
scripts/xtask/src/config/kconfig.rs
```

**假设：** 已验证的 synchronous host contract 能在实际 JH7110 SD slot 上完成 CMD0、CMD5 timeout
recovery、CMD8、ACMD41、CID/CSD/RCA/selection，而无需 protocol 层读取 private register。

**受保护不变量：** card kind 只来自 command response；每次 expected timeout 后恢复 transport/CMD0；
完整 identity 前不发布 device；capacity/addressing 只来自 CSD/OCR。

**最小交付：**

- fake host 覆盖 CMD5 response/timeout、CMD8 valid/malformed/legacy、ACMD41 ready/deadline/no-card。
- CSD v1/v2 capacity、OCR/CSD mismatch、zero/unaligned capacity、SDUC unsupported tests。
- CardId、parent/weak host 和 card-bus kind matching tests。
- source audit证明 cold discovery 只有 platform probe 尾部一个 call site，无 rescan API。

**用户侧 vertical slice：**

1. eMMC controller/no-card path 保留 host且不发布假 card。
2. SD controller attach 后日志给出 host/card/RCA/capacity/addressing。
3. capacity 与已知测试卡标称/外部工具结果一致。

**失败信号：** controller offline、unexpected generic timeout、CSD decode mismatch、combo/SDIO 被发布为
SdMemory、重复 card publication、watchdog/IRQ latency。命中后不进入 block gate；execution fact 写
transaction，若改变 identity/recovery contract则回写 RFC invariants/tracking issue。

## Gate R3：SD block endpoint

**前置：** R2 attach/capacity 在专用测试卡上通过。

**Write set：**

```text
anemone-kernel/src/device/block/mod.rs
anemone-kernel/src/device/mmc/card.rs
anemone-kernel/src/driver/{mod.rs,mmc/mod.rs,mmc/sd_memory.rs}
```

**交付与审计：**

- `BlockDevClass::Mmc` 分配 `mmcblkN`；endpoint/card/host/controller lifetime 链无 cycle。
- 512-byte logical geometry、checked range/address conversion和 zero/unaligned rejection。
- fake host 覆盖 CMD17、CMD24 + CMD13、SDSC/SDHC argument、multi-chunk order与首错停止。
- R1 error 包括 `UNDERRUN` / `OVERRUN` 时 block I/O 返回 `SysError::IO`。
- PIO write early `DATA_OVER` 返回 `ShortTransfer`，data HLE 返回 `HardwareLocked`，两者都使 endpoint
  fail closed。
- block/card publication failure 日志可区分；不得留下 capacity-zero placeholder。

**用户侧验证：**

- 非破坏性读取 LBA 0 与 last LBA，两次读取结果稳定；越界 read 返回错误。
- write/readback 只在用户指定的 disposable card 或明确 LBA 执行；记录 LBA、原内容备份、写入内容、
  readback 与恢复结果。
- 任一 host error 后验证 endpoint fail closed，不在未重建 session 的情况下继续命令。

**停止条件：** capacity/last-LBA 不一致、silent short transfer、card-status error 返回成功、未授权写、
或 IRQ-off latency 不可接受。

## Gate R4：VisionFive 2 whole-disk rootfs

**前置：** R3 只读和授权写路径通过。

**Write set：**

```text
conf/platforms/visionfive2-rv64.toml
conf/rootfs/visionfive2/*
```

**配置合同：**

- platform root source 是 block registry name `mmcblk0`，fstype 是 ext4。
- image 是 whole-disk ext4，不包含本阶段无法发现的 partition table/root partition。
- rootfs architecture、BusyBox/musl loader、kernel image和 VisionFive 2 kernel architecture一致。
- mount 前只存在一个受支持 MMC endpoint；若多个 endpoint 可出现，立即停止并设计 stable root identity。

**用户侧验证：**

- 用仓库 rootfs wrapper/manifest构造专用测试 image；记录实际命令与目标介质。
- U-Boot 启动后看到 controller、card、`mmcblk0` publication，kernel 成功挂载 ext4 并执行 configured init。
- `/dev`、`/proc` 等当前 pseudo-mount 限制按现有 rootfs script 行为观察，不在本 gate 扩大 VFS scope。
- 重启至少一次，证明 `mmcblk0` assignment 与 rootfs 选择稳定。

**失败信号：** `mmcblk0` 指向错误 card、找不到 endpoint、superblock/geometry 不一致、configured init
无法启动，或必须引入 partition scan。命中后保持 RFC open，不用 hard-coded host ID/address 绕过。

## Gate R5：收口

**前置：** R0-R4 完成，所有 review findings Neutralized。

**收口动作：**

- 把 RFC 状态改为 Implemented / Closed，或若 runtime acceptance 失败则标记 Closed / Deferred。
- 更新 transaction 的最终 validation、remaining limitations 与 user-run evidence link。
- 把仍接受的长 SpinLock、无 hotplug、无 MMC/SDIO、无 shutdown quiesce、transport-error permanent
  fail-closed 等限制登记到 current limitations；真实 defect 进入 open issues。
- 更新 tracking issues；修复依据必须已折回 index/invariants/implementation，不能只写 Neutralized。
- 当前双周 devlog 追加收口摘要，不复制 transaction 流水账。

## Source audit

```sh
rg -n "SpinLock<DwMshcInner>" anemone-kernel/src/driver/mmc/dw_mshc
rg -n "Mutex|KThread|worker|WorkQueue" anemone-kernel/src/device/mmc anemone-kernel/src/driver/mmc
rg -n "16010000|16020000" anemone-kernel/src/device/mmc anemone-kernel/src/driver/mmc
rg -n "UNDERRUN|OVERRUN" anemone-kernel/src/device/mmc/discovery/sd.rs
rg -n "verid.*\.\.|verid <|verid >" anemone-kernel/src/driver/mmc/dw_mshc
rg -n "CMD18|CMD25|CMD12|ReadMultiple|WriteMultiple" anemone-kernel/src/device/mmc anemone-kernel/src/driver/mmc
```

搜索结果必须人工解释；future-worker 注释不是依赖，测试 fixture 地址不是 production target selection。

## 验证责任

| 验证 | 责任 | 当前状态 |
| --- | --- | --- |
| `just build` / source audit / focused KUnit | Agent | build passed；R0 focused KUnit compile passed，runtime未运行；其余待 R1-R3 |
| `git diff --check` / mdBook build | Agent | diff check passed；本机无 `mdbook`，未运行 mdBook build |
| Controller identity/layout | User on VisionFive 2 | 旧 baseline 已报告；本事务未归档原始日志 |
| SD attach/capacity/read | User on VisionFive 2 | 未运行 |
| Destructive write/readback | User-authorized disposable media | 未运行，agent 不自行执行 |
| Whole-disk ext4 rootfs/init/reboot | User on VisionFive 2 | 未运行 |

## Write-set 扩展规则

若修复需要修改通用 `BusType`、block unregister、device remove/unpublish、clock/reset/pinctrl owner、
partition scanner 或稳定 root-device selector，先停止并上报：新增 owner surface、受影响 contract、
所需文件和验证 gate。批准并更新本计划/transaction write set 后才能继续；不得在当前 MMC 文件内
制造兼容旁路。
