# ANE-CHG-20260808-reset-controller-framework

**Type:** Small Iteration
**Status:** Implemented / Board validation pending
**Date:** 2026-08-08
**Area:** device reset resources / machine initialization / VisionFive 2

## Problem / Context

VisionFive 2 的两个 JH7110 GMAC 节点在 Anemone 中读取到的 DWMAC、DMA 和 feature 寄存器全部为零。
DT 已经描述 `reset-controller`、`resets` 与 `reset-names`，但 Anemone 只有 platform-device 注册，
没有 reset-controller owner。用户要求把 reset controller（`rstc`）作为独立 iteration 实现，不把
QEMU 当作 JH7110 验证，也不让 `machine_init()` 关注具体设备 reset ID。

## Decision

- `device::reset` 提供通用 `ResetController` trait、provider registry 和 `require_reset()`。
- 通用层只解析 consumer 的 variable-width `resets` phandle array、provider `#reset-cells` 和唯一
  `reset-names`；provider-specific cells 以 `ResetSpecifier` 交给 provider。
- RISC-V `MachineDesc` 在 platform discovery 前增加 reset-controller discovery hook。QEMU 保持 no-op；
  StarFive 只扫描 DT 中 compatible 为 `starfive,jh7110-reset` 的 provider 并注册它。
- `driver::rstc::jh7110` 按 DT `reg-names` 映射 SYS/STG/AON/ISP/VOUT 窗口，拥有 JH7110 reset ID
  namespace、RMW 锁和有界 assert/deassert status poll。寄存器布局不泄漏到 machine 或 GMAC。
- GMAC probe 在 capability/MMIO 诊断前按 DT 的 `ahb`、`stmmaceth` 名称调用 `require_reset()`；
  reset 失败先记录 `kerrln!`，节点独立失败。

## Change

新增 generic reset core、VF2 provider 和 machine discovery；GMAC Gate 0 工作区中已有的
`require_reset()` consumer wiring 不在本次独立 framework commit 内。没有实现 generic clock、syscon、
PHY、MDIO、DMA、IRQ registration、netdev publication 或 QEMU JH7110 设备，也没有在 machine 中硬编码
GMAC reset ID。

## Validation

- `just fmt kernel` 通过。
- `just build --preset visionfive2-rv64-release` 通过（包含 KUnit 编译）。
- QEMU 未用于 reset 或 JH7110 hardware claim；QEMU machine 不注册 reset provider。
- VisionFive 2 上的 reset status、GMAC capability、真实收发和 clock handoff 仍待用户上板验证。

## Remaining Risk / Links

- 板上若 reset deassert status poll 超时，需结合 CRG status/clock 寄存器日志判断固件 clock handoff；
  本 iteration 不添加 machine-wide “打开所有 reset”或设备特判回退。
- 当前 GMAC RFC 的 firmware reset-handoff 描述保持历史 R1；本 iteration 记录实际新增的 reset owner，
  不执行 RFC contract cutover。
- [JH7110 GMAC RFC](../../rfcs/jh7110-gmac/index.md)
- [VisionFive 2 DT](../../../../conf/platforms/visionfive2-board.dts)
- Linux 6.6.32 reset provider：`etc/linux-6.6.32/drivers/reset/starfive/`
