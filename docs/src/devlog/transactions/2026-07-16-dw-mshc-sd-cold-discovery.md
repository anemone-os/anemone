# 2026-07-16 - DW-MSHC / SD Cold Discovery

**Status:** Active / Runtime Validation
**Canonical Plan:** [RFC-20260716-dw-mshc-sd-cold-discovery](../../rfcs/dw-mshc-sd-cold-discovery/index.md)
**Owners:** EDGW_, Codex
**Area:** device model / MMC / DW-MSHC / SD Memory / block / VisionFive 2 rootfs
**Started:** 2026-07-16
**Last Updated:** 2026-07-16

## Scope

本事务记录已经存在的 DW-MSHC host/controller 基线、工作区 SD cold discovery/card bus/block
endpoint、VisionFive 2 `mmcblk0` rootfs 集成，以及 post-implementation code review、修复 gate 和
实机验证证据。accepted contract 以 RFC 为准；本文只记录执行事实，不重新定义协议。

## 当前状态

- host/controller 基线位于 commit `c2c377479607c66a70383e6afdbb7336ad3be237`。
- Stage 2 code、Kconfig、rootfs 与 block integration 位于 2026-07-16 未提交工作区。
- 两轮 review 的 correctness findings 已修复，canonical RFC 已同步用户对 firmware、字符串规则和
  rootfs input 的处置；RFC 处于 Accepted / Runtime Validation。
- 当前启用 `kunit` 的 VisionFive 2 dev build通过；focused KUnit测试体已编译但未运行。
- Stage 2 KUnit runtime、QEMU、VisionFive 2 card attach/read/write/rootfs 均未由 agent 运行。

## 2026-07-16 - 实现事实恢复与公共 RFC 提升

**Phase:** Document promotion / post-implementation review

**Source Baseline:** 审计范围是上一 commit 的 Stage 1 实现与完整工作区差异。私有实现材料只用于
恢复意图；公共 RFC 不引用其路径，且冲突处以当前代码与配置为事实来源。

**Observed Architecture:**

- `DwMshcController` 唯一持有 MMIO、layout、readiness 与 applied IOS；`MmcHostDevice`/registry
  没有复制行为状态。
- cold discovery 是 platform probe 尾部 one-shot 同步调用；没有 rescan worker 或全局 mutable core。
- `MmcCardDevice` 在 identity commit 后作为 host child 发布；card bus 只按 immutable kind 匹配。
- SD endpoint 以 checked CSD geometry发布 `mmcblkN`，multi-block caller buffer 顺序拆为单块命令。
- platform config 已从 `vda` 切到 whole-disk `mmcblk0` ext4；该事实已折回 RFC 目标和 rootfs gate。

**Review Findings:**

- `MMC-SD-001` / Apollyon：R1 error mask 漏掉 `UNDERRUN` 与 `OVERRUN`，可能 silent-success/data corruption。
- `MMC-SD-002` / Apollyon：过短 firmware MMIO resource 在首次 VERID/HCON read 时 assert panic。
- `MMC-SD-003` / Keter：连续 revision range与 generic compatible 的支持面大于 JH7110 `0x290a` 证据。
- `MMC-SD-004` / Euclid：MMC rootfs integration夹带全局 init argv 改变，缺少独立 contract/验证。

详细影响、修复方向和 canonical 回写位置见
[Tracking Issues](../../rfcs/dw-mshc-sd-cold-discovery/tracking-issues.md)。

**Agent Validation:** 当前 `kconfig` 选择 `visionfive2-rv64` dev，启用 `spin_lock_irqsave`、
`fs_ext4`、`kunit`。`just build` 通过，生成 `build/anemone.elf`、`build/anemoneImage-rv64` 与 disassembly。
cargo cache warning 不改变成功结果。

**Not Run:** Stage 2 focused KUnit runtime、QEMU、VisionFive 2 hardware attach、capacity、首末块 read、
destructive write/readback、whole-disk ext4 rootfs/init/reboot。

**Decision:** RFC 状态为 Draft / Review Hold。先执行 Gate R0 neutralization；任何实机 destructive
write 必须等待用户指定 disposable card 或明确 LBA。

## 2026-07-16 - RFC 结构与导航验证

**Phase:** Documentation validation

**Validation:** `git diff --check` 通过；新增 RFC/transaction 文件存在性、双向链接目标和
`SUMMARY.md` 层级已做 source audit，新增文档无 trailing whitespace。当前环境没有 `mdbook`
可执行文件，因此未运行 `mdbook build docs`，不把 mdBook build 记为通过。

**Boundary:** 本条只验证公开文档结构，没有改变 code-review finding，也没有运行新的 kernel build、
KUnit、QEMU 或 VisionFive 2 实机测试。事务继续保持 Active / Review Hold。

## 2026-07-16 - Review blocker closure

**Phase:** Gate R0 completed / runtime validation handoff

**User Disposition:** `MMC-SD-003` 的 revision range 与 generic compatible 是为适配现有物理机器保留的
兼容退让，不修改实现；canonical RFC 记录其 layout fail-closed 边界与退出条件。`MMC-SD-004` 按用户
决定接受且不采取行动，不修改 `main.rs`，也不增加跨平台 init argv 专项验证 gate。

**Implementation:** `SdR1Flags` 增加 bit 18 `UNDERRUN` 与 bit 17 `OVERRUN` 并纳入唯一 `ERRORS`
mask；focused KUnit逐位验证全部 R1 error flag，并验证 `READY_FOR_DATA + TRAN` 成功。`DwMshcRegs`
构造现在要求 MMIO mapping 至少覆盖到 `IdmacBusMode` 的 32-bit 访问末端（`0x84`）；过短 resource
返回 `RegisterWindowOutsideMapping`，controller 记录实际/所需长度并映射为 `DriverIncompatible`，
`ptr_at()` assert 继续只保护内部不变量。边界 KUnit覆盖 exact length 与 one-byte-short。

**Agent Validation:** 当前 `visionfive2-rv64` dev 配置启用 `kunit`；修复后的 `just build` 通过，
因此 production code 与两个新增 focused KUnit测试体均已编译。`just fmt kernel --check` 未通过，但报告
的差异全部位于本事务 write set 之外，本次修改的三个 MMC 文件没有 rustfmt diff；未改动无关文件。

**Not Run:** KUnit runtime、QEMU、VisionFive 2 controller/card attach、capacity、首末块 read、
destructive write/readback、whole-disk ext4 rootfs/init/reboot。未运行项不记为 PASS。

**Decision:** `MMC-SD-001` 至 `MMC-SD-004` 全部 Neutralized，RFC 从 Draft / Review Hold 转为
Accepted / Runtime Validation。事务保持 Active，继续执行 R1-R4；任何 destructive write仍须用户指定
disposable card 或明确 LBA。

## 2026-07-16 - 第二轮审查更正与处置闭环

**Phase:** Gate R0.1 completed / canonical correction

**Review Findings:** 第二轮审查发现三个 correctness defect：PIO write 在 buffer 未完全进入 FIFO 前
可忽略 premature `DATA_OVER`；data phase error mask 漏掉 HLE；CSD v1 接受保留
`READ_BL_LEN`，且 committed SDSC geometry 没有提前证明完整容量可由 `u32` byte-address command
argument覆盖。另有 firmware delay 无上限、canonical RFC 落后、内核 `String` 规则和 gitignored
rootfs host input四项 policy/documentation finding。

**User Disposition:** firmware delay finding不修改实现，当前支持平台信任 firmware；内核字符串规则从
禁止 `String` 调整为长期不可变所有权优先 `Box<str>`、确有构造/修改或既有 API 需求时允许短生命周期
`String`；rootfs host input finding接受且不采取行动。

**Implementation:** `write_data()` 在继续写 FIFO 前拒绝并清除 premature `DATA_OVER`，返回
`ShortTransfer`；`DATA_ERRORS` 纳入 `HARDWARE_LOCKED`，统一分类优先返回 `HardwareLocked`；CSD v1
只接受 `READ_BL_LEN=9..=11`，SDSC identity commit要求 capacity 不超过 `2^32` bytes。新增纯 helper
与 focused KUnit覆盖 early completion/HLE 组合、保留 block length和 SDSC addressability boundary。

**Canonical Correction:** RFC index、invariants、implementation、tracking issues和当前状态索引已同步上述
实现与用户处置。第二轮问题 5 因此 Neutralized；这次文档更正不改变 RFC 目标、不变量或 runtime
acceptance floor，也不把 build证据写成 KUnit/hardware runtime通过。

**Agent Validation:** 当前 `visionfive2-rv64` dev 配置启用 `kunit`、`fs_ext4` 和
`spin_lock_irqsave`；`just build` 通过，生成 `build/anemone.elf`、`build/anemoneImage-rv64` 与
disassembly。focused KUnit测试体已编译。第一次 build invocation只因工具 1 秒 timeout被终止，不是
编译失败；随后正常 68 秒构建通过。

**Not Run:** KUnit runtime、QEMU、VisionFive 2 controller/card attach、capacity、首末块 read、
destructive write/readback、whole-disk ext4 rootfs/init/reboot。未运行项不记为 PASS。

**Decision:** 第二轮问题 1、2、3 和 5 已修复；问题 4、6、7 按用户决定完成边界处置。
`MMC-SD-001` 至 `MMC-SD-010` 均已 Neutralized，RFC 与事务继续保持 Runtime Validation，R1-R4
实机 gate不变。
