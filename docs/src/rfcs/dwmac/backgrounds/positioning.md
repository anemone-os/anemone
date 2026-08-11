# DWMAC 多后端与 Loongson 2K1000 背景定位

**状态：** Background / Archived Evidence / Not Normative
**最后更新：** 2026-08-11
**范围：** 将现有 JH7110 DWMAC 5.20 驱动整理为 DWMAC owner，并为 Loongson 2K1000
的 DWMAC 3.70a 收集硬件初始化、Linux 路径和候选实现边界

> 本文只保存事实、比较和定位讨论；正式 target、owner、handoff、failure、cleanup、contract
> impact、acceptance 和 Gate 以本 RFC 的 [`index.md`](../index.md) 及其 supporting pages 为准。
> 本文不是 current contract，也不提供单独的实现授权。

## 结论预览

2K1000 不能简单继承 JH7110 的 clock/reset/PHY 初始化代码，但也不能据当前 DTS 缺少
`clocks`/`resets` 就断言这些硬件不需要初始化。当前证据支持把初始化拆成三层：

1. **DWMAC 内部 DMA reset 与 MAC/DMA 编程必须由 DWMAC1000 backend 拥有。** Linux 对
   DWMAC1000 同样在每次 DMA engine 初始化时执行 `DMA_BUS_MODE_SFT_RESET (SWR)`，不依赖外部 reset
   controller 代替它。
2. **2K1000 外部 clock/reset/pinctrl 本轮先按路线 A 的 firmware handoff 尝试。** 当前 DTS 没有
   给两个 GMAC node 声明 clock 或 reset resource，Linux generic stmmac 也允许这些 resource
   缺失；但 Linux 同时存在 LS2K GMAC clock divider provider，说明 clock rate 是真实的 SoC
   集成事实，而不是可以从 DWMAC core 中推导的常量。路线 A 只是不新增 kernel resource
   provider，不是对硬件可达性的免验证豁免。
3. **PHY 需要 driver-owned 的最低初始化与 link resolution，不能默认永久继承 firmware 状态。**
   当前两个 port 分别使用 `rgmii` 和 `rgmii-id`。在不知道实际 PHY ID 和 Linux PHY driver
   fixup 前盲目执行 BMCR soft reset，可能清掉 RGMII delay 或其它 vendor 配置；反过来完全不
   reset/configure PHY，又无法保证冷启动、不同 bootloader 路径和 cable 状态下的 MAC speed/duplex
   一致。因此正式 RFC 前必须先完成 per-port PHY identification 和 reset-effect characterization。

本轮讨论方向是：DWMAC1000 backend 始终执行内部 DMA reset；外部 clock/reset/pinctrl 先按路线 A
消费 firmware handoff；PHY 采用 per-node、boot-time、按实际 PHY ID 选择 standard Clause 22 加最小
vendor fixup 的 P1 transaction。完整 runtime PHY/link framework 不是这个定位默认要求。

## 为什么这是新的 RFC

现有 [JH7110 GMAC RFC](../../jh7110-gmac/index.md) 已经 Closed，并且明确把通用 DWMAC family、
其它 SoC glue 和 generic PHY framework 列为非目标。将 `driver/net/dwmac` 改组为
`driver/net/dwmac`，同时引入 DWMAC4 与 DWMAC1000 两个 backend，会新增或重新解析：

- common frame/IRQ/publication 与 concrete hardware backend 的 owner 边界；
- DWMAC1000 descriptor、32-bit DMA addressability、interrupt cause 和 cleanup proof；
- 2K1000 firmware handoff 与 driver-owned initialization 的分界；
- PHY reset、RGMII delay、link resolution 和 MAC reconfiguration 的 handoff；
- JH7110 已生效行为在 owner migration 中的保护与回归证明；
- 2K1000 实机 probe、单端口/双端口验收和最终 cutover。

这些内容包含 owner migration、高风险 probe 和多个独立验证阶段，超过小迭代的两个 execution
checkpoint 边界。本文只建立定位目录；正式 RFC 是否接受以及采用哪个 target，留给后续
`index.md` review。

## 当前 Anemone 事实

### JH7110 driver 是 DWMAC4/5.20 加板级 glue

当前 [`dwmac`](../../../../../anemone-kernel/src/driver/net/dwmac/mod.rs) 在 probe 中：

- 按 DT name 请求 `stmmaceth`、`pclk`、`gtx`、`tx`、`ptp_ref`、`gtxc` clock；
- 请求 `stmmaceth` reset 并 deassert shared `ahb` reset；
- 读取并要求 DWMAC version `0x52` 与 40-bit DMA capability；
- 执行 DWMAC 内部 DMA software reset；
- 初始化 Motorcomm YT8521/YT8531 family PHY；
- 构造 DWMAC4 descriptor rings、注册 `macirq`，最后发布 `ReadyNetdev`。

这条路径是在板上读到全零 GMAC register 后补齐 clock/reset owner 得到的，见
[clock controller 小迭代](../../../devlog/changes/2026-08-08-clock-controller-framework.md)。因此
JH7110 的经验不是“所有 DWMAC 都需要相同 clocks/resets”，而是：在访问 core register 前，必须
能够说明谁拥有外部可达性；外部 reset 也不能替代 core 自己的 DMA reset。

当前 [`dwmac4/fwnode.rs`](../../../../../anemone-kernel/src/driver/net/dwmac/dwmac4/fwnode.rs) 与
[`dwmac4/phy.rs`](../../../../../anemone-kernel/src/driver/net/dwmac/dwmac4/phy.rs) 还包含 JH7110/VisionFive 2
专用事实：只接受 `rgmii-id`、要求 `local-mac-address`、直接扫描 PHY child，并按 Motorcomm
extended registers 配置 delay、drive strength 和 TX clock inversion。这些不能提升为 DWMAC
common 规则。

### 可共享的 owner 已经存在

现有 `device/net`、`FrameProvider`、boot-time attach、global Stack、worker、durable recheck 和
shutdown 顺序已经是共享能力。新的 DWMAC common 层应停在 frame/link capability 边界；它不能
拥有 concrete descriptor、register、DMA address、PHY vendor state、logical name、route 或 Stack
object。

这项定位依赖但不修改：

- [NETDEV-LIFE-001](../../../contracts/net/netdev-lifecycle.md)；
- [NET-BOUNDARY-001 / NET-FRAME-OWN-001 / NET-FRAME-PROGRESS-001](../../../contracts/net/frame-path.md)；
- [NET-ATTACH-001](../../../contracts/net/attach-lifecycle.md)；
- [IRQ-FLOW-001](../../../contracts/interrupt/index.md)。

## 2K1000 DTS 已知事实

仓库跟踪的 [`2k1000-board.dts`](../../../../../conf/platforms/2k1000-board.dts) 描述两个 enabled
Ethernet node：

| 项目 | `ethernet@40040000` | `ethernet@40050000` |
| --- | --- | --- |
| compatible | `snps,dwmac-3.70a`, `snps,arc-dwmac-3.70a` | 同左 |
| MMIO | `0x40040000 + 0x8000` | `0x40050000 + 0x8000` |
| IRQ | `macirq=0x0c`, `eth_wake_irq=0x0d` | `macirq=0x0e`, `eth_wake_irq=0x0f` |
| PHY mode | `rgmii` | `rgmii-id` |
| PHY | `phy-handle` 指向 MDIO child，Clause 22 address 0 | 同左 |
| pinctrl | node 未声明 | `pinctrl-0` 选择 GMAC1 pins |
| clock/reset | 未声明 | 未声明 |
| MAC property | `local-mac-address = fa:9c:5b:e6:27:68` | `local-mac-address = 3e:f2:46:6c:3c:f5` |

`soc` node 声明 `dma-coherent`，两个 GMAC node 的 `dma-mask` 写成全 64-bit；但 DWMAC1000 的
descriptor/base-register 表示仍需要单独证明有效 DMA address width，不能由这个 DT mask 自动推出。

当前 DTS 还存在以下证据缺口：

- 没有 `loongson,ls2k-clk` clock-controller node，也没有给 GMAC 传入 `stmmaceth` clock；
- 没有 external GMAC reset、AHB reset、PHY reset GPIO 或 reset delay；
- 没有实际 PHY compatible/ID 和 vendor delay properties；
- 当前 ICU node 使用 `#interrupt-cells = <1>`，所以 `interrupts = <0x0c 0x0d>` 只携带 hwirq，不能像
  Linux 的两 cell 描述一样在 DT 中携带 `IRQ_TYPE_LEVEL_LOW`；极性不能由 driver 根据 node ordinal 猜；
- 只有 GMAC1 显式描述 pinmux，尚未说明 GMAC0 是 dedicated pins 还是 firmware-owned mux；
- DT 表达的是固件描述，不证明每个 cold boot 路径都留下相同的 live hardware state。

因此，当前 DTS 足以发现两个 candidate node，但不足以单独关闭 hardware initialization target。

## 本轮已确认的定位输入

以下是本轮讨论已经给出的实现方向；它们仍需在正式 RFC 的 `index.md` 中转化为 accepted target，
但后续事实收集应以此为边界，不再回到 JH7110 的固定双实例假设：

1. **Per-node、无 driver 固定实例上限。** 所有匹配的 DWMAC node 都独立 probe、独立拥有 MMIO、
   rings、DMA、IRQ、PHY transaction、provider 和 failure state。这里的“无限个数”表示驱动不写死
   GMAC0/GMAC1 数组或数量上限；实际可见数量仍受内存、IRQ、DMA addressability 和 logical identity
   资源约束。
2. **严格遵守 DTS compatible。** 不为了区分 2K1000 额外制造 first compatible；DWMAC4 与 DWMAC1000
   分别按 live DT 的 compatible match。backend 可以在匹配后读取 capability 做 family admission，
   但不能按 MMIO base、IRQ 数值或 node ordinal 猜测实例。
3. **先走路线 A。** clock、external reset 和 pinmux 由 firmware handoff 提供；driver 先做只读
   capability/可达性检查，再执行自己拥有的 DWMAC 内部 DMA reset。若 register-zero、DMA reset timeout
   或 MDIO timeout 暴露 handoff 不成立，停止并重新讨论 provider owner，不在 DWMAC backend 内写 raw
   LoongArch clock/reset/pinctrl register。
4. **Active logical identity 使用成功顺序。** 通过 publication/attach 的 provider 按成功的 active
   logical reservation 顺序取得 `eth0`、`eth1`、`eth2`；不要求 `ethernet@40040000` 永远是 `eth0`，
   也不为失败 candidate 留固定 ordinal 空洞。physical node path、netdev identity、logical `eth<N>`
   和 Stack `InterfaceId` 仍是不同 identity domain。
5. **PHY 采用 P1。** 每个 node 通过 `phy-handle`/MDIO 识别 PHY，执行 boot-time Clause 22、必要的
   soft reset、vendor/board fixup、autonegotiation 和 link snapshot；不建立 global PHY registry，
   不把 JH7110 Motorcomm register table 当成通用 PHY 实现。runtime link renegotiation 是否增加，
   仍需后续 target 决定。
6. **MAC 由 boot-time live DT 提供。** 仓库 DTS 已按板级 `net list` 为两个 Ethernet node 写入
   `fa:9c:5b:e6:27:68` 和 `3e:f2:46:6c:3c:f5` 的 `local-mac-address`；boot 流程生成 live DT 时必须
   保留或注入对应属性。driver 将它作为 publication-time MAC fact 消费并校验，不从另一个 node 借用、
   随机生成或由 `eth<N>` 推导；Gate 2 记录实际 bounded bring-up 消费的 live DT 值。

## Linux 6.6 的 DWMAC1000 路径

### Core family 与 generic platform probe

Linux generic DWMAC platform driver直接匹配 `snps,dwmac-3.70a`，解析 standard stmmac DT
resource 后进入共同 stmmac core：
`xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/dwmac-generic.c#dwmac_generic_probe`。

`stmmac_platform.c` 把 `snps,dwmac-3.70a` 归入 legacy GMAC/DWMAC1000，而不是 DWMAC4：
`xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/stmmac_platform.c#stmmac_probe_config_dt`。
硬件接口表随后选择 `dwmac1000_dma_ops`、`dwmac1000_ops` 和 normal/enhanced descriptor ops：
`xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/hwif.c#stmmac_hwif_init`。

这说明“按照 Linux 标准协议接入 DWMAC1000”至少包含 DWMAC1000 MAC register map、legacy DMA
CSR、normal/enhanced descriptor admission、MDIO clock range 和 device-cause W1C；它不表示可以复用
DWMAC4 register/descriptor backend。

### 外部 clock 与 reset 是可选 resource

Linux DT path 尝试取得并 enable 名为 `stmmaceth` 的 main clock；取得失败时记录 warning 并继续。
`pclk`、`stmmaceth` reset 和 shared `ahb` reset 也都是 optional resource：
`xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/stmmac_platform.c#stmmac_probe_config_dt`。
若 reset resource 存在，stmmac probe 会 pulse `stmmaceth` reset、deassert `ahb` reset，并等待
10 microseconds；若资源不存在，这一步不会发生：
`xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/stmmac_main.c#stmmac_dvr_probe`。

这证明 Linux generic path 允许 firmware 留下可用的外部 clock/reset 状态，但不证明当前 2K1000
板上的 handoff 已经稳定。缺失 resource 只表示 kernel 没有对应控制能力；它不是“硬件没有 clock/reset”
的证据。

### DWMAC 内部 DMA reset 不是可选项

在建立 DMA channel/base address 前，Linux `stmmac_init_dma_engine()` 总是调用 backend reset。
DWMAC1000 的实现设置 `DMA_BUS_MODE_SFT_RESET` 并等待 self-clear：

- `xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/stmmac_main.c#stmmac_init_dma_engine`；
- `xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/dwmac_lib.c#dwmac_dma_reset`；
- `xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/dwmac1000_dma.c#dwmac1000_dma_init`。

因此候选 DWMAC1000 backend 必须自己执行、观察和超时诊断这个 reset。不能因为 U-Boot 曾经使用过
Ethernet，或未来增加 external reset provider，就跳过内部 DMA reset。

### PHY/MDIO 由 Linux PHY owner 重新接管

Linux 解析 `snps,dwmac-mdio` child 与 `phy-handle`，注册 MDIO bus，再由 phylink 连接 PHY：

- `xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/stmmac_platform.c#stmmac_dt_phy`；
- `xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/stmmac_mdio.c#stmmac_mdio_register`；
- `xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/stmmac_main.c#stmmac_init_phy`。

MDIO bus reset会消费 optional reset GPIO/delay；对 pre-GMAC4 core，没有 GPIO 时仍执行一次 dummy
MDIO write。PHY owner 随后 deassert hardware reset、执行 PHY driver 的 soft reset、fixup 和
`config_init`，并由 phylink 启动 link/autonegotiation：

- `xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/stmmac_mdio.c#stmmac_mdio_reset`；
- `xref:linux-6.6.32:drivers/net/phy/phy_device.c#phy_init_hw`；
- `xref:linux-6.6.32:drivers/net/phy/phy_device.c#genphy_soft_reset`。

Linux 的做法不是永久信任 bootloader PHY 状态，而是让真实 PHY driver 在 reset 后重新应用
vendor/board fixup。Anemone 如果暂不实现完整 PHY framework，也必须保留这个 correctness obligation；
特别是 `rgmii-id` delay 不能在 soft reset 后无 owner。

### Loongson source 只能提供集成线索

Linux 的 LS2K clock driver把 `gmac` 注册为从 DC PLL 派生的 divider clock，divider 位于 clock
controller resource 的 offset `0x28`、bit `22..27`：
`xref:linux-6.6.32:drivers/clk/clk-loongson2.c#loongson2_clk_probe`。这说明 2K1000 的 GMAC/MDIO
clock rate可由 SoC clock state决定，但当前 Anemone DTS 没有暴露这个 provider 给 GMAC consumer。

Linux `dwmac-loongson.c` 是 PCI glue，不是当前 platform DT node 的直接实现。它设置
`clk_csr=2`、store-and-forward、PBL 32 和 8x PBL，并自动发现 PHY：
`xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/dwmac-loongson.c#loongson_default_data`。
这些值可作为实机 probe 的比较项，不能直接提升成 2K1000 platform target；必须由 core version、
clock rate、DT/board source 和 runtime register evidence共同确认。

Linux binding同时认识 `loongson,ls2k-dwmac` 与 `snps,dwmac-3.70a`：
`xref:linux-6.6.32:Documentation/devicetree/bindings/net/snps,dwmac.yaml#compatible`。当前 Anemone
DTS 只使用 generic compatible。是否补充 Loongson-specific first compatible，应取决于是否确有
SoC glue，而不是为了限制 match 范围制造无行为差异的新字符串。

## 2K1000 初始化责任的候选划分

| 初始化事实 | 候选 owner | 当前判断 |
| --- | --- | --- |
| DWMAC3 register family/version admission | per-node DWMAC1000 backend | 必须 driver-owned；错误 family fail closed |
| DMA software reset、CSR、descriptor base、interrupt mask | per-node DWMAC1000 backend | 必须 driver-owned |
| GMAC clock enable/rate/mux | firmware（路线 A 首选）或未来 Loongson clock provider | 先消费 firmware handoff；失败信号触发路线 B review |
| external GMAC/AHB reset | firmware（路线 A 首选）或未来 reset provider | DTS 无资源；driver 不直接写 raw reset register |
| GMAC1 pinmux / GMAC0 pin state | firmware（路线 A 首选）或未来 pinctrl owner | 先按 per-node cold-boot probe 证明 handoff |
| MDIO CSR divider | DWMAC1000 backend，输入来自 clock fact | 必须显式形成；不能无依据复用 JH7110 divider |
| PHY identity | per-node PHY transaction | 必须从 `phy-handle`/MDIO 读取，不从 port ordinal 推导 |
| PHY hardware reset | board/PHY owner | DTS 无 GPIO；是否需要由实际 PHY/板级证据决定 |
| PHY soft reset、vendor fixup、RGMII delay | per-node PHY transaction 或未来 generic PHY owner | reset 与 fixup必须是同一 transaction，不能只做前半段 |
| current link、speed、duplex | PHY owner；MAC消费 snapshot/callback | R0 至少在 publication/start 前解析一次，不能伪造 link-up |
| runtime link renegotiation | 未来 PHY/link owner | 是否进入 R0 尚未决定，默认不由 positioning 偷渡 |

## 外部 clock/reset/pinctrl 的路线比较

### 路线 A：带证据的 firmware handoff

driver 不直接写 Loongson clock/pinmux/reset controller，只消费 firmware 留下的状态。进入 core setup
前必须证明：

- 两个 node 的 version/capability register 在 cold boot 上稳定可读且非零；
- DWMAC1000 internal DMA reset可以 self-clear；
- MDIO transaction 在明确的 CSR divider 下稳定完成；
- GMAC1 pinmux 与两个 port 的 RGMII clock path 在不同 bootloader 使用历史下保持可用；
- handoff 通过 notice 可观察，并写明被 kernel provider 替代后的删除条件。

这是本轮先尝试的路线。优点是最小，不为单个 consumer 扩张 clock/reset/pinctrl framework。缺点是
suspend/reboot、不同 firmware 和未初始化 cold path 的鲁棒性较弱。任何一次 register-zero、DMA
reset timeout 或 MDIO timeout 都是 handoff 失败信号，不允许退回 magic MMIO 写或假成功；失败后应
回到路线 B 的 provider owner 讨论。

### 路线 B：kernel-owned Loongson platform resources

补充/修正 DTS，并由 clock/reset/pinctrl provider拥有 controller register和 transaction；DWMAC driver
只请求窄 capability。这个方向更接近 JH7110 最终路径，也能消除 firmware 依赖，但必须先确认：

- LS2K GMAC clock 是 rate-only divider、gate，还是还包含 mux/reset；
- 两个 GMAC 是否共享同一 clock/reset transaction；
- pinmux controller 的现有节点是否有可复用 provider；
- enable/deassert 的顺序、失败 rollback 和 shutdown policy。

在这些事实未闭合前，不应把 raw clock/pinmux register 写入 DWMAC1000 backend。若路线 A 的 probe
失败，正式 RFC 应停下并把路线 B 提升为 owner decision，而不是在 backend 局部加入板级特判。

## PHY 路线比较

### 路线 P0：完全继承 firmware PHY 状态

只读 PHY link/speed/duplex，不 reset、不重启 autonegotiation。它适合最早的 characterization，但不适合作为
默认 production target：它无法证明 `rgmii-id` delay、vendor fixup、不同 cable 状态和不同 bootloader
路径下的可重复性，也没有 runtime link change owner。

### 路线 P1：per-node boot-time PHY transaction

先经 `phy-handle` 得到 PHY address，读取 ID/BMCR/BMSR；按确认的 PHY ID 选择 standard Clause 22
流程与最小 vendor fixup；只有能够在 reset 后重新建立 `phy-mode` 要求的 RGMII delay 时才执行 soft
reset；完成 autonegotiation/link snapshot 后配置 MAC speed/duplex。每个 port 独立失败，不建立全局 PHY
registry或 runtime link framework。

这是本轮选择的最小方向。它复用 JH7110 的 timeout、fail-closed、link snapshot 和 owner-local MDIO
transaction 经验，但不复用 Motorcomm register table。实际 PHY ID 未确认前，不能编写 vendor 分支或
把未知 ID 静默当作 generic PHY。

### 路线 P2：generic PHY/MDIO/link framework

建立独立 PHY device identity、driver match、reset/fixup owner、runtime autonegotiation 和 link callback，
由 DWMAC backend只消费 link capability。这最接近 Linux phylink/PHY architecture，但 scope 和 lifecycle
明显更大。除非实机证据表明 P1 无法可靠支持两个 port，或仓库出现第二个真实 PHY consumer，否则不应
仅为“像 Linux”而提前建立完整 framework。

## 建议先收集的板级事实

正式 RFC review 前，建议先完成只读或最小、无 publication 的 characterization：

1. 分别记录两个 node 在 cold boot、warm reboot、bootloader 使用/不使用 Ethernet 后的 DWMAC version、
   DMA bus mode、MAC address registers 和 interrupt baseline。
2. 在不发布 netdev、不启动 DMA 的前提下，执行 internal DMA soft reset，记录 self-clear deadline 和
   reset 前后 MAC/MDIO register变化。
3. 通过 `phy-handle` 读取两个 port 的 PHY ID、BMCR、BMSR、advertisement、link partner ability 和
   vendor status；确认两个 port是否真的是同一 PHY family。
4. 对 `rgmii-id` port确认 soft reset是否清除 internal RX/TX delay，以及 Linux 对该 PHY ID 选择的
   driver/fixup；未闭合前不执行 production reset。
5. 确认有效 MAC 的来源：live DT injection、hardware UMAC register、NVM 或其它 firmware channel；
   不使用随机 MAC 掩盖缺口。
6. 确认 GMAC clock rate或可信 `clk_csr` 来源，并用 MDIO MDC frequency/transaction稳定性验证，不能
   只照抄 PCI glue 的 `clk_csr=2`。
7. 分别验证 GMAC0/GMAC1 pinmux、RGMII clock direction和 PHY power/reset在 cold boot 的状态 owner。

这些 probe 的结果应回写正式 RFC 的 target/implementation 或 supporting evidence。probe code必须有
删除/替换 gate，不能因为可以读取 register 就自然沉淀为 production API。

## 候选 DWMAC 模块边界

以下只表示 responsibility topology，不冻结文件布局或 trait 形状：

```text
driver/net/dwmac
  common
    frame token / bounded progression / publication adapter
    durable recheck edge
  dwmac4
    DWMAC4/5.20 registers, descriptors, DMA and device causes
    JH7110 clock/reset/PHY/DT glue
  dwmac1000
    DWMAC3 registers, normal/enhanced descriptors, legacy DMA and causes
    2K1000 firmware/resource/PHY/DT glue
```

common 层可以依赖窄的 queue/interrupt capability，但不得通过一个大而全的 `DwmacHardware` trait 暴露
所有 register operation。每个 backend继续唯一拥有自己的 MMIO、rings、descriptor ownership、DMA
addressability、current link/resource truth 和 shutdown attempt。JH7110 的行为保持型目录迁移不能与
DWMAC1000 semantic cutover混成一个无法回归的提交点。

## 候选 Gate 轮廓

正式 RFC 只保留三个实现 Gate 和一个最终闭合 Gate；事实收集属于 RFC 正文/backgrounds，不单独形成 Gate。
这里只记录目的，不构成执行授权：

1. **Gate 1：DWMAC owner migration 与 IRQ foundation 实现。** 将现有 JH7110 module 整理为 common +
   DWMAC4，保持既有行为；同时实现 2K1000 `IrqSense` source 表、`EDGE/POL` 配置和
   `request_irq` expectation。JH7110 regression 是本 Gate 的退出验证。
2. **Gate 2：DWMAC1000 backend 与 bounded bring-up 实现。** 实现 legacy normal descriptor、internal DMA
   reset、32-bit DMA admission、Route A、MDIO/PHY P1、IRQ wiring 和 CSR5 W1C；先用不 publication 的
   bounded slice 验证，probe 必须在 Gate 退出前吸收到 production backend。
3. **Gate 3：2K1000 per-node production attach 实现。** 把 DWMAC1000 接入 FrameProvider、worker、
   publication 和 attach；同一 per-node path 先完成单端口 bring-up，再完成双端口独立 progression/failure、
   success-order `eth<N>` 和收发。
4. **Gate 4：最终 acceptance 与 closure review。** 只复核 Gate 1--3 的实现和证据，处理 tracking issue、
   执行 Architecture Friction Scan，并在没有实现缺口时更新真实 current contracts 和关闭 RFC。

如果 Gate 2 不能关闭外部 clock/reset、PHY reset-after-fixup owner 或 normal descriptor，不应在 Gate 3
偷偷加入 fallback；应回到 RFC review 决定 Route B、P2、enhanced 或缩减 target。

## 仍待讨论的三个技术问题

### 7. descriptor family 到底是什么意思

#### 7.1 descriptor 是什么

网卡 DMA 不会调用 `send()`，也不理解 Anemone 的 `FrameProvider`。它只会从软件提供的一张内存表中逐项
读取固定格式的记录。每条记录就是一个 descriptor，通常包含 frame buffer 的 DMA 地址、buffer 长度、
frame 的开始/结束标志、中断/checksum/timestamp 控制位，以及 DMA 与 CPU 之间的 ownership/status。

一次 TX 的基本交接是：CPU 填地址和长度，写好控制字段，最后把 `OWN` 交给 DMA；DMA 发送完成后清掉
`OWN`，并在 status 中写入完成或错误信息。RX 则相反：CPU 先把空 buffer 和 `OWN` 交给 DMA；DMA 收到
frame 后写入 buffer、写回长度/status，再清掉 `OWN`；CPU 看到 `OWN=0` 才能读取 frame。`OWN` 的位置、
长度字段的位置和 ring 末尾标记的位置都是硬件协议，不能由 Rust 结构体名称决定。

#### 7.2 normal、enhanced、extended 的区别

Linux 的 `struct dma_desc` 是四个 little-endian 32-bit word，共 16 字节；但同样的四个 word 在不同模式下
不是同一张位表：

| 形状 | 内存大小 | 它解决什么问题 | 典型字段位置 | Linux 解释器 |
| --- | ---: | --- | --- | --- |
| normal | 16 字节 | legacy DWMAC100/1000 的普通收发 | RX/TX 的 `OWN/status` 主要在 `des0`；长度、segment、ring/chain 控制在 `des1`；地址使用 `des2`/`des3` | `ndesc_ops` |
| enhanced/alternate | 16 字节 | 更大的 buffer、不同的控制位布局以及 checksum/timestamp 选项 | TX 的 `OWN`、first/last、checksum、ring/chain 位移到 `des0`；buffer size 的位宽和位置也变了 | `enh_desc_ops` |
| extended | 32 字节 | 在 enhanced 基础上保存扩展 RX status 和 PTP timestamp | 前四字仍是 basic descriptor，`des4` 是扩展 status，`des6/des7` 保存 timestamp | `dma_extended_desc` |

enhanced 不是“多了四个 word”，extended 也不是“DWMAC4 的旧名称”。enhanced/alternate 改的是前四个
word 的位解释；extended 才是把 `des4..des7` 追加到 basic descriptor 后面。ring mode 和 chain mode
又是另一条正交选择：ring 通过固定 stride 和 `END_RING` 循环，chain 通过 chained/next-address 关系串联，
不能把它们当成 descriptor family。

最容易误解的是“结构大小一样所以可以复用”。normal 和 enhanced 虽然都是 16 字节，但如果按错误位表写
`des0/des1`，DMA 可能把长度看成 OWN/控制位，或者把普通字段当成 next-descriptor 地址。表现通常是 DMA
停在第一个 descriptor，或者向错误物理地址写数据，不是一个可以容错的 feature mismatch。

#### 7.3 Linux 如何决定 DWMAC1000 用哪一种

Linux 分两步判断：

1. DT 的 `snps,dwmac-3.70a` 先把 node 归入 legacy GMAC/DWMAC1000，`stmmac_hwif_init()` 因而选择
   DWMAC1000 的 MAC、DMA 和 descriptor callbacks：
   `xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/stmmac_platform.c#stmmac_probe_config_dt`；
2. DWMAC1000 的 `DMA_HW_FEATURE` 在 bit 24 报告 `ENHDESSEL`。Linux 的
   `dwmac1000_get_hw_feature()` 读取该位得到 `dma_cap.enh_desc`；如果 capability register 存在，
   `stmmac_hw_init()` 用硬件事实覆盖平台默认值：
   `xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/dwmac1000_dma.c#dwmac1000_get_hw_feature`；
   `xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/stmmac_main.c#stmmac_hw_init`。

`enh_desc` 生效后，Linux 的 `stmmac_dwmac1_quirks()` 选择 `enh_desc_ops`；若 core version 至少为 3.50，
还把 ring stride 改为 `sizeof(struct dma_extended_desc)`，即 32 字节，并启用扩展 status/timestamp 解释。
若 `DMA_HW_FEATURE` 读到全零，Linux 将其视为老硬件没有 capability register，而不是“所有 capability 都是
false”。

当前 JH7110 ring 中 `des0/des1` 存 64-bit frame address，`des2` 存控制，`des3` 存 OWN/status，这是
DWMAC4/5 的位布局（见 [`ring.rs`](../../../../../anemone-kernel/src/driver/net/dwmac/dwmac4/ring.rs)）。
DWMAC1000 normal descriptor 的地址和 OWN/status 位置不同，所以可以共享“descriptor ownership 的抽象约束”，
不能共享这四个 word 的具体读写函数。

#### 7.4 我们怎样关闭这个问题

Gate 2 的最小 backend/probe 证据不是“编译出一个 `Dwmac1000Descriptor`”，而是：

- 记录每个 matching node 的 core version 和 `DMA_HW_FEATURE` 原值；
- 明确选择 normal 或 enhanced 位表，并为 ring end、first/last、buffer size、OWN/status 写回建立测试；
- 如果选择 extended，证明 allocation、stride、`des4..des7` 和 timestamp ownership；否则把 PTP/extended
  明确列为 R0 non-goal；
- 用不 publication 的 loopback/probe 让 DMA 至少完成一个 TX 和一个 RX，证明 OWN 转换与 frame length
  写回符合选中的位表。

如果 capability 与已实现位表不一致，probe 必须在 netdev publication 前失败。不能用一个“兼容读写”把两套
协议混在一起。

#### 7.5 硬件支持 enhanced 时能否主动配置 normal

可以，但这需要把两个问题分开：

- **协议兼容性：可以。** Linux 的 DWMAC1000 文档明确说 driver 同时处理 normal 和 alternate descriptor。
  DWMAC1000 的 DMA Bus Mode 有 `ATDS`（Alternate Descriptor Size）选择位：`ATDS=0` 时 DMA 按 normal
  16-byte descriptor 解码，`ATDS=1` 时按 alternate/enhanced 解码。`DMA_HW_FEATURE.ENHDESSEL=1` 是“alternate
  能力存在”的 capability，不是“normal 被禁用”的声明。若 actual core/databook 没有 vendor 例外，支持
  enhanced 的 core 应仍能用 normal。
- **Linux 默认策略：不是这样。** Linux 读取 capability 后把 `plat->enh_desc` 覆盖为 `dma_cap.enh_desc`；
  对 `snps,dwmac-3.70a`，这会使 `stmmac_dwmac1_quirks()` 选择 enhanced ops，并在 ring mode 下通过
  `atds=1` 设置 `DMA_BUS_MODE_ATDS`。所以“选 normal”是 Linux 支持的模式，但不是 Linux 6.6 的默认
  capability policy。

还要纠正“extended capability”的说法：DWMAC1000 的 `DMA_HW_FEATURE` 没有一个独立的“extended descriptor
必选”位。Linux 通常在 `enh_desc=1` 且 Synopsys core version 至少为 3.50 时把 ring stride 扩成
`dma_extended_desc`，再结合 timestamp capability 提供 PTPv2。也就是说，extended 是 enhanced 基础上的
软件布局选择，不是看到一个 bit 就必须使用的第三种 DMA 协议。

本轮已经决定 DWMAC1000 的最小 R0 只使用 normal descriptor：

1. 读取并记录 `DMA_HW_FEATURE`；`ENHDESSEL=0` 时只能走 normal，`ENHDESSEL=1` 时仍明确选择 normal；
2. 将 `ATDS` 保持为 0，ring stride 固定为 16 字节，使用 normal 的 `des0..des3` 位表；
3. 不启用 extended/PTP，直到后续 target 真正需要 `des4..des7`；
4. 在不 publication 的 probe 中完成一个 normal TX 和 RX，确认 DMA 清 OWN、写回 length/status，并确认
   descriptor base 与 buffer address 都按 normal 规则访问；
5. 如果 `ENHDESSEL=1` 但 normal probe 失败，不把它静默切换成 enhanced，而是停止并确认 2K1000 的
   integration/databook 是否存在“alternate-only”约束，再决定是否增加 enhanced backend。

这个决定的收益是首个 DWMAC1000 ring 更小、没有 PTP 扩展字段，也不会把 JH7110 DWMAC4 的 descriptor
布局误复用；代价是 R0 暂不提供 enhanced 的大 buffer、扩展 status 和 PTP 能力。它仍遵守 Linux 定义的
normal descriptor 协议，但不复制 Linux 在 capability bit 为 1 时自动升级到 enhanced 的默认 policy。
如果 normal probe 证明 2K1000 实际为 alternate-only，这个 R0 不得偷偷切换；应停在 Gate 2，重新提交
target renegotiation 或 follow-up RFC。

### 8. 64-bit `dma-mask` 与 DWMAC1000 32-bit 地址表示如何相容

#### 8.1 三个地址概念不能混为一谈

这里至少有三个地址域：

1. **CPU 物理地址**：Anemone frame allocator 返回的 `PhysAddr`，例如 `0x9000_4000`；
2. **设备 DMA 地址**：没有 IOMMU 时通常与物理地址相同，DWMAC 用这个数访问内存；
3. **descriptor/register 能表达的地址**：legacy DWMAC1000 的 DMA base register 和 descriptor 地址
   字段只有 32 位。

`dma-mask` 只描述第二个域允许的最大地址。DTS 中 `dma-mask = <0xffffffff 0xffffffff>` 是一个 64-bit
全 1 mask，意思是平台声明设备可以访问到 64 位范围；它不是给 descriptor 自动增加高 32 位，也不是 allocator
一定会返回低地址的保证。`dma-coherent` 处理的是 cache visibility 假设，同样不会改变地址字段宽度。

#### 8.2 DWMAC1000 为什么真的只有 32 位地址

Linux DWMAC1000 的 RX/TX descriptor base 分别写入 CSR3/CSR4，并使用
`lower_32_bits(dma_rx_phy)`/`lower_32_bits(dma_tx_phy)`；normal descriptor 的 buffer address 写入
32-bit `des2`，ring 的第二 buffer 或 chain 的 next-address 使用 `des3`。它没有 JH7110 DWMAC4 的
base-low/base-high register 和 40-bit address capability。

如果真实 buffer 位于 `0x1_2345_6780`，强转成 `u32` 后只剩 `0x2345_6780`，DMA 会访问低地址的另一页。
这不是“偶尔收不到包”，而是潜在的任意内存读写。因此所有 descriptor、TX frame、RX frame 以及 chain/
next address 都必须满足：

```text
end = checked_add(address, covered_bytes)
end <= 0x1_0000_0000
```

检查必须覆盖整个 allocation，而不是只看起始地址。

#### 8.3 2K1000 当前内存事实和 Anemone allocator

2K1000 DTS 的 memory node 是两个区间：`0x0..0x10000000` 与 `0x90000000..0x100000000`，第二个区间
末端恰好是 4 GiB。当前 [`dma_alloc()`](../../../../../anemone-kernel/src/mm/dma.rs) 通过
`alloc_frames_zeroed()` 从全局 frame allocator 取得连续 folio；它没有 DMA mask 参数，也没有在返回值处做
32-bit boundary check。`DmaRegion::sync_for_device()`/`sync_for_cpu()` 当前只是内存 fence，用来建立 CPU 与
设备交接的 ordering，不能把高地址变成低地址。

所以当前板子有一个有利事实：按这份 memory DT 初始化的可用 RAM 都低于 4 GiB。但这仍不是 backend 的长期
contract，因为 allocator 是全局的，未来 memory DT 或其它平台可以加入高于 4 GiB 的 zone。DWMAC1000 必须
自己保留 32-bit admission invariant。

#### 8.4 Linux 怎么处理，我们应怎样处理

Linux 会根据 hardware/driver 的 host DMA width 设置 DMA mask；当 width 不超过 32 位时，RX page pool 使用
`GFP_DMA32`，避免得到设备不能寻址的页：
`xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/stmmac_main.c#stmmac_init_rx_buffers`。legacy
DWMAC1000 的 descriptor/base path 仍然只写 32 位。

Anemone 目前没有 `dma_alloc(mask)` 或 DMA32 zone API，因此正式 RFC 需要在两条路中选择：

- **当前最小路线：** 2K1000 memory zones 都低于 4 GiB；DWMAC1000 ring allocation 后显式检查
  `start + allocated_bytes <= 4 GiB`，每个 descriptor/frame offset 也做 checked arithmetic，失败时在
  MMIO base 写入和 netdev publication 之前返回资源/硬件不兼容错误；
- **证据要求时的路线：** 增加带地址上限的 DMA allocation，或者为高地址 frame 建立 bounce backing。bounce
  会增加 copy、buffer ownership 和 cleanup 状态，没有高地址证据时不应提前引入。

不能用“把 DTS 的 `dma-mask` 改成 32 位”代替上述工作。那只会改变描述，不能改变 allocator 返回的物理页，
更不能修复写入 descriptor 时已经发生的高位截断。

#### 8.5 R0 的明确实现策略

本轮对问题 8 采用一个有意收窄、但可观测的路线：不建设 DMA32 allocator，也不引入 bounce buffer；DWMAC1000
在 per-node 初始化时明确声明并检查 32-bit DMA 假设。

具体行为如下：

1. 每个 matching DWMAC1000 node 在开始 DMA backing 初始化时打印一次 warning，说明该 backend 假定板级可用
   内存和所有 DMA backing 都位于 4 GiB 以下。这个 warning 不是“硬件错误”，而是把当前板级事实和 allocator
   缺少 mask contract 的限制写入启动诊断；不能因为当前 DTS 的 RAM 看起来低于 4 GiB 就静默省略。
2. `dma_alloc()` 返回后，以完整的半开区间 `[start, end)` 检查分配结果：使用 checked arithmetic 计算
   `end = start + allocated_bytes`，并要求 `end <= 0x1_0000_0000`。检查的是整个 contiguous backing，
   不是只检查 `start`；恰好等于 `0x1_0000_0000` 的地址已经越过可表示的 32-bit 地址空间。
3. 将 descriptor base、每个 TX/RX frame address、ring/chain next address 写入 MMIO 或 descriptor 前，继续
   用同一 32-bit invariant 检查 `address + covered_bytes` 和 offset arithmetic，避免一个低地址 allocation
   因 offset 溢出而产生高地址字段。
4. 任意检查失败都让该 node 的 probe 返回失败，并在错误日志中报告起始地址、分配大小和越界端点；不得把地址
   强转成 `u32` 后继续运行，不得 publication、注册 `macirq` 或消耗 active `eth<N>` identity。其它 matching
   node 仍可独立继续 probe。

这条路线的含义是：当前 2K1000 内存小于 4 GiB 是 R0 的板级前提，但 driver 不把它当作未经检查的全局真相。
warning 提供可观测性，allocation check 提供实际 fail-closed 边界；未来若出现高于 4 GiB 的 memory node，
这个 node 会明确失败，而不是以截断地址形成一个看似已发布、实际会破坏内存的网卡。

### 9. 2K1000 ICU 的 IRQ flow 与 DWMAC W1C cause 如何配合

#### 9.1 先把三个“DMA/中断”概念分开

2K1000 的手册在同一张中断路由图里出现了两类名字相似、但不是同一个硬件的 DMA：

1. **DWMAC 内部 DMA engine。** 它位于每个 GMAC function 内，通过 DWMAC 的 DMA CSR（CSR5 是
   `DMA_STATUS`）报告 RX/TX 完成和异常。这个 status cause 由 DWMAC backend 读取和清除；它不是 ICU 的
   source number。
2. **SoC 独立 DMA controller。** 手册把 source `44..48` 标成 `dma`，并在 §9.1 说明这些 source 是脉冲
   触发。它们不是 Ethernet node 的 `macirq`，不能因为 DWMAC 也有一个 DMA engine 就把 GMAC IRQ 配成
   edge。
3. **GMAC sideband/PMT source。** 手册的 source table 明确把 `12` 写成 `Gmac0_sbd_int`、`13` 写成
   `Gmac0_pmt_int`、`14` 写成 `Gmac1_sbd_int`、`15` 写成 `Gmac1_pmt_int`。当前 DTS 的 `macirq` 取的
   正是 12/14；`eth_wake_irq` 取的是 13/15。

手册 §9.1 说明：独立 DMA 和 PCIe MSI 是脉冲源，GPIO 可以按需配置，其余普通 I/O source 是电平源；§9.3
定义 `INTEDGE=0` 为电平、`INTPOL=1` 为低电平。Linux 6.6 的 2K1000 DTS 对 12/14/13/15 也都显式写
`IRQ_TYPE_LEVEL_LOW`：`xref:linux-6.6.32:arch/mips/boot/dts/loongson/loongson64-2k1000.dtsi#L136-L161`。
手册转载版的固定链接是
[`Loongson 2K1000 用户手册 v1.2`](https://github.com/everlasting001/loongson/blob/9458fcf1fd5304984fd07eca1660532573613ba0/2K1000/Loongson2K1000_user_v1.2_202004.pdf)，§9.1--§9.3（印刷页 94--96）。

一帧收到以后，首先动作的是 DWMAC，不是 ICU：DMA 把 frame 写入 RX buffer，并在自己的 DMA status register
中置位 `RI`（receive interrupt）以及 `NIS`（normal summary）；发送完成对应 `TI`，总线错误等对应 abnormal
bits。DWMAC 的这些 status cause 使用 **W1C（write one to clear）**：软件读出 status 后，只向需要清除的 bit
写 `1`；写 `0` 保持该 bit。这个寄存器属于 DWMAC driver，ICU 不知道 `RI/TI/NIS/AIS` 的含义。

Linux legacy stmmac 直接读取 CSR5，并把 `intr_status & 0x1ffff` 写回 CSR5；源码注释明确说明向 CSR5 的
cause bit 写逻辑 1 会清除中断。Loongson PMON 对 2K1000 的 DMA status offset `0x14` 也采用“读取
`DmaStatus`、回写观察值、再读取确认”的路径，并将 bit `16..0` 列为 `NIS/AIS/RI/TI` 等 GMAC cause：
[`PMON synopGMAC_get_interrupt_type`](https://github.com/loongson-community/pmon-ls2k1000la/blob/41760e8f600fd5cbdd3f34499b19c76e143e5380/sys/dev/gmac/synopGMAC_Dev.c#L2590-L2608)。
PMON 的回写值过宽，不能原样作为 Anemone 的长期实现；它与 Linux 的 `0x1ffff` mask 共同证明了“设备 cause
由 DWMAC status W1C 清除”，而不是由 ICU ack 清除。

ICU 只负责把一个 SoC interrupt source 记录为 pending、路由到 CPU、允许或禁止再次投递。它的 `mask`、`ack`、
`eoi` 是 controller transaction，不会清除 DWMAC 的 `RI/TI`。因此“ack 了 ICU”和“清了网卡 cause”是两件
完全不同的事。

#### 9.2 一次 2K1000 DWMAC level 中断的时序

DTS 中每个 Ethernet node 有两个 interrupt specifier，并以 `macirq`、`eth_wake_irq` 命名。driver 只按名称选择
`macirq`，因此 GMAC0/GMAC1 分别得到 hwirq `0x0c`/`0x0e`；这些数值只是当前 firmware mapping，不是 DWMAC
ABI。当前 DTB 固定使用 `#interrupt-cells = <1>`，不能从 specifier 得到 polarity；本 RFC 不改 DTB，因此
12/14/13/15 的电气配置必须由 2K1000 irqchip 的完整 `IrqSense` source 表拥有。当前 Anemone 只把
44..48 配为 edge，
所以 12/14 的 controller flow 走 `IrqFlowType::LevelMaskEoi`，但还没有把它们配置为 Linux 所要求的
`POL=1`（level-low）。

这不是 DWMAC backend 的职责：DWMAC 只请求 named `macirq` 并处理自己的 CSR5；它不能根据 node ordinal、
MMIO base 或 IRQ 数字直接写 ICU polarity。2K1000 irqchip 的 source 12/13/14/15 表项为 `LevelLow`，由同一
表项导出 `EDGE=0、POL=1`；44..48 的 edge 配置也由各自表项导出。这是一项受 DTB one-cell 限制的 concrete
irqchip 兼容桥，必须在代码注释中写明退出条件：只有未来 DTB 能携带两 cell Linux interrupt flags 时，才能
改为逐 specifier 配置；当前不能伪装成 DT 已表达 sense。

一次正常 RX/TX 事件应按下面顺序发生：

1. DWMAC status 置位 `RI` 或 `TI`，并使 `macirq` 保持有效电平；ICU 看到 source pending，路由到 CPU。
2. IRQ core 根据 ICU 给出的 flow 先执行 controller `mask(hwirq)`。这只暂时阻止同一个 source 重入，
   不改变 DWMAC status。
3. `macirq` handler 读取 DWMAC status 和 interrupt-enable，取出真正启用且可 W1C 的 cause。当前 JH7110
   的 [`take_enabled_dma_causes()`](../../../../../anemone-kernel/src/driver/net/dwmac/dwmac4/regs.rs) 就只写回
   `status & enabled & W1C_MASK`，DWMAC1000 需要自己的 legacy status mask。
4. handler 在 IRQ flow 返回前向 DWMAC status 写入这些 cause bits。这个写入才让设备撤销 level；同时把
   cause 发布成 durable recheck/wake，供 worker 回收 TX 或消费 RX descriptor。
5. handler 返回后 IRQ core 执行 `eoi()` 和 `unmask()`。当前 2K1000 ICU 的 `eoi()` 是 no-op，因为设备
   driver 负责撤销 level；`unmask()` 重新允许 source 投递。
6. 如果第 3 步之后又产生新 frame，新的 cause 会在 unmask 后再次 pending；如果旧 cause 没清掉，level 会
   立即重投。前者是正确的新事件，后者通常是 handler bug。

Linux legacy DWMAC 同样先消费 device status，再在 DMA handler 中向 CSR5 的低 cause bits 写一清除：
`xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/dwmac_lib.c#dwmac_dma_interrupt`。这正是
`IRQ-FLOW-001` 要求 device cause 必须在尾部 `eoi/complete/unmask` 前撤销的具体原因。

#### 9.3 三种错误时序会造成什么

- **先 unmask、后 W1C：** ICU 重新开放时 DWMAC 仍保持 level，CPU 会立即再次进入 handler，形成中断风暴，
  并重复唤醒、重复扫描 ring。
- **只清 summary、不清 source bit：** 只写 `NIS/AIS` 而不清 `RI/TI` 或错误 bit，设备实际 cause 仍在，
  level 不会撤销。反过来对整个 status 写 `u32::MAX` 也不安全，因为 process-state/read-only 位不一定是 W1C；
  必须使用 DWMAC1000 明确允许写一清除的 mask。
- **把 level 当 edge：** ICU edge `ack` 可能只撤销 controller 中记录的脉冲；如果 device cause 还在而 driver
  没有按协议读/清，可能丢失一次状态或在下一次事件前得不到新 edge。把 edge 当 level 则会增加 mask/unmask
  transaction，但是否正确仍由真实 source 行为决定，不能由 MMIO base 或 hwirq 数值猜测。

#### 9.4 目前已知什么、还要证明什么

仓库中的 [`loongson_2k1000.rs`](../../../../../anemone-kernel/src/driver/intc/loongson_2k1000.rs) 已经给出
controller flow 侧事实：ICU 有 64 个 source，`0x0c`/`0x0e` 当前走 `LevelMaskEoi`，所有 source 固定路由到
INT3，level flow 的 `eoi` 本身不写 controller register；44..48 单独走 edge flow。仓库中的
[`IRQ-FLOW-001`](../../../contracts/interrupt/index.md) 给出 owner 边界：irqchip 选择 controller flow 和
电气配置，DWMAC driver 选择 device cause 的读取、清除和 publication 协议。

目前的确切缺口不是“无法知道 12/14 是什么”，而是“one-cell DTB 没有表达 polarity，而现有初始化把
`POLARITY` 整体写成 0”。Linux 6.6 与 2K1000 手册都要求 GMAC 12/14 为 level-low，即 `EDGE=0、POL=1`；
因此 2K1000 irqchip 需要增加完整 `IrqSense` source 表，并把 GMAC 12/13/14/15 都列为 `LevelLow`。不能让
DWMAC1000 backend 自己改 ICU，也不能把 44..48 的 SoC DMA edge 表项误用于 GMAC。

还要在板上证明两类事实：第一，irqchip 写入静态表后，12/14 的 `EDGE/POL` readback 确实为 `0/1`，idle
时 pending 为 0，产生 GMAC cause 后 pending 才变为 1；第二，DWMAC1000 的 RX、TX、abnormal cause 在
`mask -> handler/W1C -> unmask` 顺序下稳定。`eth_wake_irq` 是另一个 DT resource，当前路线只注册 named
`macirq`，不把 wake source 混入 DMA cause。

Gate 2 的 IRQ probe 应按两个端口分别记录：

1. ICU source 12/14 的 `EDGE`、`POLARITY`、route 和 pending readback；预期 `EDGE=0、POL=1`，而不是只记录
   `IrqFlowType::LevelMaskEoi`；
2. named `macirq` 的 node path、hwirq 和 dispatch flow；不得使用 `eth_wake_irq` 或 44..48 代替它；
3. RX/TX/异常事件的 CSR5 read、采用的 W1C mask、CSR5 清除后的 readback，以及 W1C 后到 unmask 前的 ICU
   pending readback；
4. 每类事件的 dispatch、durable recheck/wake 和 descriptor completion 计数。新事件导致的下一次 dispatch
   是允许的，旧 status 未清导致的立即重复不是。

完整 `IrqSense` 表和 request expectation 会 refinement shared IRQ request surface，因此必须进入
`IRQ-FLOW-001` contract review；它仍不改变 DWMAC common/backend owner。`POL=1` readback、两端口静态证据
和 W1C 顺序未闭合前，Gate 2 只能停在 bounded bring-up，不能进入 Gate 3 netdev publication。

#### 9.5 已确认的中断类型表与 request expectation

当前 DTB 保持 one-cell，不增加 trigger cell。2K1000 irqchip 必须依据手册 §9.1 和表 9-1 建立 owner-local
完整中断电气类型表。只有 `IrqTriggerType::{Edge, Level}` 的粗分类无法决定 `POL`，因此表项需要保留
trigger 与 polarity 的组合。本定位采用：

```rust
enum IrqSense {
    LevelHigh,
    LevelLow,
    EdgeRising,
    EdgeFalling,
}
```

2K1000 ICU 的寄存器映射由 actual `IrqSense` 唯一导出：

| `IrqSense` | `INTEDGE` | `INTPOL` | controller flow |
| --- | ---: | ---: | --- |
| `LevelHigh` | 0 | 0 | `LevelMaskEoi` |
| `LevelLow` | 0 | 1 | `LevelMaskEoi` |
| `EdgeRising` | 1 | 0 | `EdgeAck` |
| `EdgeFalling` | 1 | 1 | `EdgeAck` |

source 12/13/14/15 的表项是 `LevelLow`，因此表本身同时确定 GMAC 的 `EDGE=0、POL=1`；这项结论由
2K1000 手册的 source/寄存器定义和 Linux 6.6 的 `IRQ_TYPE_LEVEL_LOW` 集成描述共同支持。source `44..48`
是独立 DMA controller 的脉冲 source，按手册和当前寄存器基线形成 edge 表项。GPIO source `58..63` 的
类型可配置，在 GPIO irqchip 真正拥有 trigger selection 前不能把当前默认值提升为永久固定表项。PCIe MSI
同样不在当前 allocation target 内，不能仅凭 `Pcie*_int` source 名称猜测 MSI sense。

这张表是 `InterruptInfo` actual sense、`INTEDGE/INTPOL` 编程和 controller flow 的唯一真相源；不能再保留
一份独立 `trigger_type(irq)` range 特判和另一份 polarity mask。实现可以从同一个表派生 coarse
`IrqTriggerType`，但派生值不能反向驱动或覆盖表项。DWMAC backend 不复制这张表，也不直接写 ICU register。

IRQ 请求接口增加可选期望类型。接口形状为：

```rust
request_irq(device, expected: Option<IrqSense>, handler, private)
request_irq_selected(device, selector, expected: Option<IrqSense>, handler, private)
```

校验顺序固定为：firmware resource owner先按 name/index 选择单个 specifier；irqchip `xlate()` 再从 hwirq 和
owner-local 表得到 actual `InterruptInfo { hwirq, sense, flow }`；若 `expected` 为 `Some`，IRQ core 在
domain mapping、`IrqDesc` publication 和第一次 `unmask()` 之前比较 `expected == sense`。不一致时记录 device、
hwirq、expected 和 actual，并返回 `InvalidInterruptInfo`；不得留下 mapping、descriptor 或 enabled source。

`expected` 只是 caller 的 admission assertion，不是第二份类型真相：它不写 `EDGE/POL`，不选择 flow，不保存到
`IrqDesc`，也不能覆盖 irqchip 表。没有额外期望的既有 caller 传 `None`，保持当前行为；DWMAC1000 请求 named
`macirq` 时传 `Some(IrqSense::LevelLow)`，同时校验 level trigger 和 low polarity，在错误 DT source mapping
或错误手册表时 fail closed。

这项 shared request surface 变化需要在正式 RFC 中列为 `IRQ-FLOW-001` 的真实 refinement，并用定向测试证明：
`None` 保持既有成功路径；matching `Some` 成功；trigger mismatch 和 polarity mismatch 都在
publication/unmask 前失败；named multi-interrupt node 只校验选中的 `macirq`，不误读相邻
`eth_wake_irq`。

这三个问题不会改变本轮已确认的 1--6，但会决定 DWMAC1000 Gate 2 能否关闭并进入 Gate 3 netdev
publication：descriptor 位表、DMA 地址窗口和 device/controller 中断时序必须同时闭合，任何一项不确定都
应停在 bounded bring-up，而不是用 board-specific fallback 伪造成功。
