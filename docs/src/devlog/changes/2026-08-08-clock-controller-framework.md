# ANE-CHG-20260808-clock-controller-framework

**Type:** Small Iteration
**Status:** Implemented / Board Gate 0 validated
**Date:** 2026-08-08
**Area:** device clock resources / machine initialization / VisionFive 2

## Problem / Context

VisionFive 2 的 JH7110 GMAC 在解除 reset 后仍读到全零寄存器。Linux 的
`dwmac-starfive` 会在 probe 中 enable `tx`、`gtx`，通用 stmmac 会 enable
`stmmaceth`、`pclk`，而 Anemone 只有 clock-controller platform device，
没有 clock provider owner。

## Decision

- `device::clock_controller` 提供通用 `ClockController` trait、provider registry、
  variable-width DT `clocks`/`clock-names` 解析和 `require_clock()`。
- RISC-V machine 只扫描 DT 中 `starfive,jh7110-clkgen` 并注册 provider；QEMU 保持 no-op。
- `driver::clkc::jh7110` 按 `reg-names = "sys", "stg", "aon"` 映射窗口，使用 Linux
  JH7110 的合并 global clock-id namespace，并只对 gate/GDIV/GMUX 时钟执行 bit-31 RMW。
-  纯 divider/mux/inverter 保留 firmware 的 rate/mux 配置并以成功 no-op 完成 enable；不伪装成
  可写 gate。
- clock/reset provider 共享 SYS/STG/AON 的 CRG 映射，reset 只额外映射 ISP/VOUT，避免同一 MMIO
  窗口被两个 provider 重复 `ioremap`。
- reset provider 的 global reset-id lookup 使用 `#[repr(usize)] Window` 和布局数组直接索引，
  不再线性扫描 `ResetLayout`。
- GMAC probe 按 DT clock name 请求 gate，再执行已有 reset handoff；machine 不硬编码 GMAC
  或全局开启 clocks。

## Change

新增 generic clock core、VF2 provider 和 machine discovery；不实现 rate/mux 改变、clock disable、
PHY/MDIO/syscon、DMA、IRQ registration、netdev publication 或 QEMU JH7110 模拟。

## Validation

- `just fmt kernel` 通过。
- `just build --preset visionfive2-rv64-release` 通过（包含 KUnit 编译）。
- QEMU 未用于 JH7110 clock 或硬件 claim；VisionFive 2 上的 clock enable、GMAC capability
  读取已由用户实机日志验证成功；Gate 1 attach、DMA、IRQ 和真实收发仍未实现。

## Remaining Risk / Links

- provider 当前只实现 gate enable，不递归改 rate/mux；板上若父 clock 未由 firmware 保持运行，
  需要后续按 Linux clock tree补齐 provider-owned parent handoff。
- GMAC RFC 的 firmware handoff 描述保持历史 R1，本 iteration 不执行 RFC contract cutover。
- [Reset controller iteration](./2026-08-08-reset-controller-framework.md)
- [JH7110 GMAC RFC](../../rfcs/jh7110-gmac/index.md)
- Linux 6.6.32 clock gate/RMW reference：
  `xref:linux-6.6.32:drivers/clk/starfive/clk-starfive-jh71x0.c#jh71x0_clk_enable`；
  JH7110 descriptor tables are in
  `xref:linux-6.6.32:drivers/clk/starfive/clk-starfive-jh7110-sys.c`、
  `xref:linux-6.6.32:drivers/clk/starfive/clk-starfive-jh7110-stg.c` and
  `xref:linux-6.6.32:drivers/clk/starfive/clk-starfive-jh7110-aon.c`。
