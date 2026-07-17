# DW-MSHC / SD Cold Discovery Tracking Issues

**状态：** Closed / all findings neutralized
**最后更新：** 2026-07-16
**父 RFC：** [RFC-20260716-dw-mshc-sd-cold-discovery](./index.md)
**事务日志：** [2026-07-16-dw-mshc-sd-cold-discovery](../../devlog/transactions/2026-07-16-dw-mshc-sd-cold-discovery.md)

本文只跟踪会改变实现顺序、review gate、停止边界或验收判断的问题。实现进度和运行证据写事务日志；
修复必须折回 RFC canonical 文本。

## Apollyon

None open。

## Keter

None open。

## Euclid

None open。

## Safe

None。审查在剩余观察只涉及局部命名、单次 boot allocator 回收或理论扩展后停止。

## Neutralized

### MMC-SD-001：R1 漏判 `UNDERRUN` / `OVERRUN`

**原级别：** Apollyon

**状态：** Neutralized / fixed in code

SD R1 bit 18 `UNDERRUN` 与 bit 17 `OVERRUN` 原本不在 `SdR1Flags::ERRORS`，使 transport 成功时
card 明确报告的 transfer error 仍可能被 CMD17/CMD24/CMD13 消费路径当成成功。

`anemone-kernel/src/device/mmc/discovery/sd.rs` 现已增加两个 named flag并纳入唯一 error mask；
`r1_check_rejects_every_card_status_error` 逐位验证全部 19 个 error flag，并验证
`READY_FOR_DATA + TRAN` 正常成功。当前 `kunit` 配置下 `just build` 已证明测试体可编译；KUnit runtime
未运行，运行责任仍由 [implementation gate](./implementation.md) 与事务日志跟踪。

### MMC-SD-002：过短 MMIO resource 在 probe 中触发 assert

**原级别：** Apollyon

**状态：** Neutralized / fixed in code

firmware resource length 原本在首次 `VERID/HCON` read 前没有 fallible validation，过短 resource
会触发 `DwMshcRegs::ptr_at()` 的 bounds assert。`DwMshcRegs::new()` 现要求映射至少覆盖到
`IdmacBusMode` 的 32-bit 访问末端；失败返回 `LayoutError::RegisterWindowOutsideMapping`，controller
记录实际/所需长度并返回 `SysError::DriverIncompatible`。`ptr_at()` assert 保留为 owner 内部不变量。

`register_window_accepts_exact_and_rejects_short_mapping` 覆盖 `0x84` exact boundary 与 one-byte-short
拒绝；当前 `kunit` 配置下 `just build` 已通过，KUnit runtime 未运行。

### MMC-SD-003：revision 与 generic compatible 支持面大于证据

**原级别：** Keter

**状态：** Neutralized / user-approved compatibility concession

用户明确确认 `0x240a..=0x2fff` revision range 与 `snps,dw-mshc` generic compatible 是为了适配
现有物理机器作出的兼容退让，因此本轮不收窄代码。该退让只允许节点进入既有严格 layout gate：
missing/out-of-range VERID、unsupported FIFO width/depth、multi-slot HCON、过短 register window 与越界
FIFO access仍 fail closed；它不构成对任意 DW-MSHC register semantics 的支持声明。

保留原因是当前支持物理机的 firmware identity/revision 不足以安全使用 exact allowlist。删除条件是
支持机器都提供稳定 SoC-specific compatible，且各 revision 已有资料审计、layout test 与实机 probe
证据；满足后应收窄 match/revision allowlist。边界已折回 [RFC 接受边界](./index.md#接受边界) 与
[硬件信任边界](./invariants.md#硬件与-firmware-信任边界)。

### MMC-SD-004：rootfs 集成夹带全局 init argv 变化

**原级别：** Euclid

**状态：** Neutralized / accepted without action

用户决定本轮忽略该项，不修改 `anemone-kernel/src/main.rs`，也不把独立 init-argv contract 作为
MMC RFC review blocker。VisionFive 2 runtime rootfs gate仍需证明 configured init 能启动，但本 RFC
不要求额外恢复旧参数或做跨平台 argv 专项验证。

### MMC-SD-005：PIO write 可在 buffer 未提交完时接受 `DATA_OVER`

**原级别：** Apollyon

**状态：** Neutralized / fixed in code

`write_data()` 原本只在所有 bytes 写入 FIFO 后等待 `DATA_OVER`；若 controller 提前置位该完成位，
循环仍可能继续写 FIFO，最终把旧完成位消费为当前完整传输成功。实现现在每次写 FIFO 前先判 data
error，再拒绝 premature `DATA_OVER`，清除该 W1C cause并返回 `ShortTransfer`。纯 helper KUnit覆盖
early completion；当前 `kunit` 配置下 `just build` 已证明测试体可编译，KUnit runtime未运行。

### MMC-SD-006：data phase 漏判 hardware locked error

**原级别：** Apollyon

**状态：** Neutralized / fixed in code

`HARDWARE_LOCKED` 原本只在 command phase error mask 中，data polling 遇到 HLE 与 `DATA_OVER`
并存时可能把请求当作成功。`RawInterrupt::DATA_ERRORS` 现在包含 HLE，统一分类 helper优先返回
`HardwareLocked`，`check_data_errors()` 清除全部已观察 data error causes后失败。focused KUnit覆盖 HLE
单独出现及与 `DATA_OVER` 并存；当前 build 已编译测试体，runtime未运行。

### MMC-SD-007：SDSC identity 可提交超出 command argument domain 的 geometry

**原级别：** Apollyon

**状态：** Neutralized / fixed in code

CSD v1 decode 原本接受所有 4-bit `READ_BL_LEN`，包括 SD 规范保留值，并且只在单次 I/O 时检查
`lba * 512 -> u32`。实现现在只接受 `READ_BL_LEN=9..=11`，并在 identity commit 前要求完整 SDSC
capacity 不超过 `2^32` bytes，确保最后一个 512-byte logical block 的 start byte可编码。focused KUnit
覆盖保留 block length 与 addressability boundary；当前 build 已编译测试体，runtime未运行。

### MMC-SD-008：firmware power-on delay 没有上限

**原级别：** Keter

**状态：** Neutralized / user-approved trusted firmware boundary

用户选择信任当前支持平台 firmware 提供合理的 `post-power-on-delay-ms`，本轮不增加数值上限，也不
修改 busy-wait 实现。该决定只适用于当前受控 platform firmware；若支持来源扩大到不受控 firmware，
必须在 probe 前恢复 fallible bound validation。风险与退出条件已折回 RFC index、invariants 和
implementation。

### 第二轮问题 5：canonical RFC 落后于实现与处置

**原级别：** Keter

**状态：** Neutralized / canonical documents corrected

第二轮审查发现 RFC 仍只描述首轮 blocker closure，没有记录 PIO early completion、data HLE、SDSC
addressability，以及用户对 firmware、字符串规则和 rootfs input 的处置。`index.md`、`invariants.md`、
`implementation.md`、本页和 transaction 已同步为同一 accepted boundary；这项更正不宣称新增 runtime
证据，RFC 仍为 Accepted / Runtime Validation。

### MMC-SD-009：内核字符串规则与现有 naming owner 冲突

**原级别：** Euclid

**状态：** Neutralized / repository policy changed

用户放弃全面禁止内核运行时 `String` 的强约束。根 `AGENTS.md` 现在要求长期拥有且不再修改的字符串
优先使用 `Box<str>`；增量构造、就地修改或既有 owner API 明确要求时允许短生命周期 `String`。
本轮不做与 MMC 修复无关的批量迁移，现有 block registry naming不再构成 blocker。

### MMC-SD-010：rootfs composition 依赖 gitignored host input

**原级别：** Euclid

**状态：** Neutralized / accepted without action

用户决定忽略该 finding，本轮不修改 rootfs composition 或 host input 管理方式。R4 仍只接受明确记录的
实际 image 构造与 VisionFive 2 runtime 证据；本机已有 staging、gitignored input 或旧 image 不得被
写成可复现构建或测试通过。
