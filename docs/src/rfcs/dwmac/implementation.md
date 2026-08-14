# DWMAC RFC 实施路线

**状态：** Accepted / R6 / Gate 1--2 Closed; Gate 3 Authorized / Active; RFC Not Cut Over
**最后更新：** 2026-08-15
**父 RFC：** [RFC-20260811-dwmac](./index.md)
**当前修订：** R6

本文保存本 RFC 的实现 Gate、probe、验证和停止边界；不冻结逐文件 write set，也不授权未审查的后续
Gate。事实调查属于 RFC 正文和 backgrounds，不单独占 Gate；Gate 1--3 必须各自产生可审查的实现，
source/hardware regression 是对应实现 Gate 的退出验证。只有 Gate 4 是不新增实现能力的最终闭合审查。

## 全局 Implementation Boundary

- **Target / non-goals：** 见 [RFC 正文](./index.md)；DTB 保持不变，DWMAC1000 R0 仅 capability-admitted enhanced/extended descriptor、32-bit DMA、CSR5 W1C 和 boot-time PHY P1。
- **Owner / handoff / failure / cleanup：** concrete DWMAC backend owns MAC/DMA/rings/device cause；Loongson irqchip owns `IrqSense`/`EDGE/POL`/flow；generic IRQ core owns dispatch；Route A firmware owns external clock/reset/pinctrl。DWMAC1000 Gate 2 creates one long-lived per-node hardware owner and characterizes it by polling with CSR7=0；a quiesced pass is retained by the bound platform device, while Gate 3 may only adopt it in place and perform the first IRQ request。Backing release requires proven TX/RX process-state quiescence；otherwise Gate 2 fail-stops。
- **Protected ABI / contract / acceptance：** existing JH7110 visible behavior、network/Stack/attach、current DTB、`eth<N>` success-order semantics and userspace ABI；target contract delta remains Not Cut Over until final Gate 4。
- **Validation claim：** source/KUnit/build can close software protocol; only 2K1000/JH7110 hardware can close core, handoff, PHY, electrical IRQ and traffic claims。
- **Stop conditions：** target/owner/ABI/DTB boundary changes；`ENHDESSEL`/enhanced mode fails or descriptor readback disagrees；Route A/PHY owner unresolved；32-bit address invariant requires allocator/bounce；`IrqSense` expectation becomes a broader generic ABI。

## Gate 1 - DWMAC owner migration and IRQ foundation

**Purpose:** 按 R2 保留的 owner 形状将现有实现收口为 `net::dwmac` shared layer、
`net::dwmac::dwmac4` concrete driver module 和 `net::dwmac::dwmac1000` registration module，同时实现
2K1000 irqchip `IrqSense` 表和 optional request expectation。该 Gate 只建立 DWMAC/IRQ 的共享前提，不接入
DWMAC1000 node，也不发布新的 netdev 能力。

**Prerequisites:** RFC 正文/backgrounds 已记录当前 DT、Linux 6.6.32、Loongson manual/PMON、IRQ/network
contract 和 JH7110 live-source baseline；尚未关闭的 Route A、PHY、DMA 和 normal-mode 硬件问题留给 Gate 2。

**Protected Boundary:** DTB 不变；DWMAC 不写 ICU；`IrqSense` table 是 concrete irqchip owner；JH7110
DWMAC4 register/descriptor/clock/reset/PHY behavior、public network ABI 和 current contracts 不变。

**Deliverable:**

- `net::dwmac` common frame/progression/publication adapter；JH7110-only clock/reset/PHY/DT glue 留在
  `net::dwmac::dwmac4`，common 不拥有 concrete register/descriptor truth。
- `net::dwmac::dwmac4` 自己拥有 `Driver`/match table/registration 并启用既有 JH7110 match；
  `net::dwmac::dwmac1000` 自己拥有 `Driver`/match table/registration，但 Gate 1 只做 fail-closed
  registration，不执行 DWMAC1000 hardware transaction。
- 2K1000 `IrqSense` table：12/13/14/15 `LevelLow`，44..48 edge/pulse；表同时决定 `EDGE/POL` 和
  controller flow，reserved/GPIO/MSI 不伪造成已支持能力。
- `request_irq(expected: Option<IrqSense>)` 与 `request_irq_selected` named-resource path；`None` 保持现有
  caller 行为，`Some` mismatch 在 mapping/descriptor/unmask 前失败且不写 controller。

**R2 Validation:** kernel build、owner-local KUnit、variant-local Driver registration/compatible source audit、
IRQ `None`/match/mismatch，以及用户提供的 2K1000 双 node 实机日志。该日志必须同时证明两个 enabled node
都由 `dwmac1000` Driver 匹配，并按 Gate 1 设计在任何 DWMAC1000 hardware transaction/publication 前返回
`NotSupported`。RiscV/JH7110 hardware regression 允许明确记录为 Not Run；2K1000 masked-source
`EDGE/POL` readback 必须精确为 edge bits 44..48 与 active-low bits 12..15。VisionFive 2 双节点 DWMAC4
regression 不被取消，移交 Gate 3 exit 与最终 closure proof。

**Cutover:** None；owner migration 对 existing visible semantics 保持中性，IRQ target delta 保持 Not Cut
Over 到 Gate 4。

**R2 Closure:** Gate 1 已由用户授权关闭。关闭只证明 owner migration、variant-local Driver dispatch、IRQ
software foundation、ICU electrical programming readback 和 DWMAC1000 fail-closed boundary；不证明
DWMAC1000 register/descriptor/PHY/traffic 或 JH7110 runtime regression。Gate 2 未授权。

**Stop / Exit:** migration 需要改变 JH7110 visible behavior、public API、IRQ/PHY semantics 或建立 shared raw
register abstraction；variant-local Driver registration 不能保持 single compatible owner；`IrqSense` 不能由
concrete irqchip 单一拥有；expectation 需要变成 caller-owned configuration；任一 JH7110 hardware regression
未解释。停止并回 RFC review。

## Gate 2 - DWMAC1000 backend and bounded bring-up

**Purpose:** 实现 DWMAC1000 concrete backend、Route A admission 和 per-node PHY P1 transaction；先以不
publication 的 bounded hardware slice 验证寄存器、descriptor、DMA、MDIO 和 device-cause 协议，再把
probe 代码删除或吸收到 production backend。

**Prerequisites:** Gate 1 实现和 R2 validation closure 已完成。RFC 中 live MAC、memory/DMA、IRQ、Route A、
PHY ID/RGMII delay 和 enhanced-mode facts还必须足以支撑实现。缺失事实是本 Gate 的启动/停止条件，不是新的
事实 Gate。

**Protected Boundary:** DWMAC1000 backend 不改变 DWMAC4；不改变 DTB；不注册IRQ、active netdev、Stack
membership 或 `eth<N>`。Gate 2 context 是 Gate 3 必须原位采用的长期 concrete hardware owner，不形成 public
API、旁路 registry、临时 handler 或第二条 production path。platform bind success只发生在bounded
characterization passed且quiescence已证明后，但仍不声明IRQ或network capability。

**Deliverable:**

- `snps,dwmac-3.70a` compatible/version/capability admission；DWMAC internal DMA SW reset 和 timeout diagnostics。
- `ENHDESSEL=true` admission、`ATDS=1` enhanced/extended descriptor/32-byte stride ring；ring size与per-slot frame capacity由Kconfig在
  enhanced descriptor合法范围内配置。Gate 2分配Gate 3可采用的最终RX/TX rings/backing，只借用slot 0做proof；通过且quiesced后
  在同一backing上恢复完整RX device ownership、idle TX和末项EOR；`ENHDESSEL`同时是descriptor mode admission事实。
- full checked 32-bit DMA range for ring/base/frame/next；越界 before bind/IRQ/publication failure。
- Route A capability/readability handoff check；MDIO divider/read/write；按已确认 PHY ID 实现 P1 reset/fixup/
  autonegotiation/link snapshot，不能闭合 `rgmii-id` reset effect 时禁止 production reset。
- CSR7保持0且不调用`request_irq`；以RX/TX bounded loopback轮询descriptor与raw CSR5，分别证明RI/TI；
  Linux-class fatal/error constituent与运行期abnormal summary单独累计并使characterization fail。TU与ETI按Linux
  只作为nonfatal legal evidence记录/W1C，不依赖TX OWN readback的采样顺序；主动stop产生的TPS/RPS只作为legal
  cleanup evidence记录/W1C。AIS只有确实汇总本阶段允许的TU/ETI/TPS/RPS且没有其它fatal constituent时才随之恢复，
  AIS-alone始终失败。这些状态不代替RI/TI、descriptor、payload
  或quiescence proof；每次raw legal cause W1C后立即回读，只有最终bounded stopped drain仍存在的legal位计入
  uncleared cause。
- polling前创建唯一per-node hardware owner；characterization pass/fail都先suppress device causes、stop MAC/DMA
  并记录TX/RX process-state。只有quiesced pass才把MMIO、rings、DMA backing、PHY snapshot和result保存在bound
  device上；quiesced failure返回普通probe failure，未quiesced result fail-stop且禁止释放backing。
- Gate 3从bound device原位取得同一owner，首次执行named `macirq` `Some(LevelLow)` request并增加IRQ context；
  不得重映射MMIO、重建rings/backing或复制CSR5 truth。pending trace、handler/W1C/level flow、异常注入和跨启动
  重复性移交Gate 3/final acceptance。

**Validation:** owner-local KUnit/source tests、kernel build和至少一次2K1000 bounded hardware bring-up；
记录 version/capability、DMA reset deadline、DMA addresses、MDIO/PHY/RGMII state、CSR7=0、CSR5 raw/W1C
readback、ring size/frame capacity、descriptor/payload、quiescence和failure logs；Gate 1的exact `EDGE/POL` readback作为IRQ baseline，不替代
Gate 3 request/handler proof。source/KUnit还必须覆盖quiesced pass retained、quiesced failure released、quiesce
timeout fail-stop，以及platform bind不产生IRQ/netdev/publication。

**Cutover:** `DWMAC1000-CUTOVER` 仍 Not Cut Over；不写 current contracts。

**Stop / Exit:** Gate 2 只有在本次运行的 Route A/PHY、enhanced/extended descriptor、32-bit DMA、RI/TI、CSR5 W1C readback
和 quiescence evidence 全部闭合后退出。cold/warm/bootloader-used矩阵、人工abnormal事件、IRQ request/handler
和逐次pending trace移交Gate 3/final acceptance。unexpected register family、DMA high address、CSR7非零、W1C
failure、reset/MDIO timeout或bounded characterization failure必须明确记录；quiesced failure返回普通failure，
未quiesced failure必须fail-stop，不得伪装成Gate 2 pass。若需要另一种descriptor格式/DMA32/bounce/DTB change，不能让device
`drv_state`提供唯一owner供Gate 3原位采用，或polling需要扩大DWMAC/generic IRQ API，停止并进行target/owner
review；不能把probe留成并列backend。

**Gate 2 Closure:** `etc/log-board-la-new-33.log` 暴露的 TU 时序 oracle 和越界 KUnit 断言已按 Linux 修正。
用户随后提供的 `etc/log-board-la-new-35.log` 在同一修复镜像上连续三次证明有线 node 的 enhanced/extended
descriptor、`ATDS=1`、32-bit DMA、resolved PHY link、`RI/TI`、合法 CSR5 W1C、RX/TX descriptor、68-byte
payload、`abnormal=0`、`uncleared=0`、quiescence 和 `BindRetained`；无链路 node 在 PHY deadline 独立失败。
三次 KUnit runner 均输出 `All tests passed!`，覆盖修正后的 4 GiB admission 与 TU/AIS classification。
Gate 2 现已 **Closed / Not Cut Over**：不注册 IRQ、不 publication、不更新 current contracts；该 closure 记录时 Gate 3 尚未授权，当前已进入 Gate 3。

### Probe P1 - Enhanced descriptor and device-cause slice

**Hypothesis:** 2K1000 DWMAC 3.70a reports `ENHDESSEL`, accepts Linux enhanced/extended descriptors with `ATDS=1`, and CSR5 low cause bits are W1C as Linux/PMON indicate。

**Protected Boundary:** 不 publish netdev、不修改 current contract、不沉淀 generic descriptor API、失败删除 probe path。

**Non-goals:** normal descriptor fallback、PTP runtime、runtime PHY/link、traffic performance、high-address allocation。

**Validation:** CSR7=0下one bounded enhanced/extended TX and RX OWN/status/payload transition；`ATDS=1`/32-byte stride readback；CSR5 read -> legal W1C write -> CSR5 readback；two-port独立polling sample if hardware permits。

**Failure Signals:** `ENHDESSEL=false`、`ATDS`未置位、stride/layout mismatch、OWN never clears、CSR5 cause persists、DMA address truncation。

**Write-back:** evidence folds into Gate 2 implementation; failure stops Gate 2 and records target renegotiation.
Probe code is deleted or absorbed into the production backend before Gate 2 exits。

**Exit:** enhanced/extended probe proven and absorbed into the long-lived per-node backend owner；失败保持Not Cut Over。

## Gate 3 - Per-node production attach

**Purpose:** 在每个 production kernel 的同一次 platform `probe()` 中，原位采用 Gate 2 已写入 device
`drv_state`的 DWMAC1000 hardware owner，首次建立IRQ context并接入 existing
FrameProvider/worker/publication/attach path，完成
不固定实例数的 per-node production implementation。单端口 bring-up 只是该 Gate 内的第一段验证，不形成
独立 Gate 或 single-port-only production target。

**Required path:** Gate 3 是 Gate 2 成功后的必经实现路径，不提供 `gate3` Cargo/Kconfig feature，也不提供
Gate 2-only production kernel。Gate 2 characterization 通过后，probe 必须在同一 owner 上继续完成 IRQ request、
publication 和 attach；无法完成该 continuation 时，probe 失败并按本 Gate 的 cleanup/fail-stop 规则处理。

**Prerequisites:** Gate 2 backend/bounded bring-up closure；每个可接入 node 都保留唯一、disabled、
characterization-passed owner，Route A、PHY、MAC、DMA、enhanced/extended descriptor、CSR5 polling和quiescence evidence已关闭；
R2 延期的 DWMAC4 regression 必须在本 Gate 取得并保持 green。

**Protected Boundary:** failure node does not consume `eth<N>`；其它 matching node 独立继续 probe；DTB、
existing network ABI、global Stack、attach owner 和 DWMAC4 behavior 不变。

**Deliverable:**

- 每个 matching node 通过 private continuation 原位取得 Gate 2 owner，在其已有 PHY snapshot、rings和DMA
  backing上执行first/only IRQ request并建立FrameProvider、worker、publication和attach；不得重映射MMIO、再次
  request IRQ、重建ring/backing、替换一次性device `drv_state`或建立并列device-cause state。该路径不依赖另一次
  reprobe，也不假设 owner跨kernel reboot存活。
- active logical identity 只在成功 publication/attach 时按成功顺序连续分配；不按 MMIO、IRQ、alias 或
  candidate ordinal 固定 `eth<N>`。
- 同一 per-node path 支持两个当前 2K1000 node；不增加 GMAC0/GMAC1 特判、固定数组或 second-port adapter。
- 任一 node 的 pre-publication failure先撤销新增的 provider/worker publication attempt，并把同一 owner恢复为
  disabled retained state；只有 quiescence已证明时才可释放非 IRQ-retained capability，且不阻止其它 node成功。

**Implementation checkpoint (2026-08-15):** 上述 in-place adoption、首次 `Some(LevelLow)` request、owner-local
enhanced-ring queue、CSR5/MAC cause service 和既有 publication handoff 已实现；LoongArch concrete irqchip 也已在
每次 `LevelLow` unmask 前记录 controller-owned pending snapshot，为 first request 与 handler-tail trace 提供观测点。
Gate 3 hardware validation 尚未运行，因此本 checkpoint 不宣称 IRQ/traffic/lifecycle acceptance。详见
[ISSUE-007](./tracking-issues.md#issue-007--gate-3-irq-and-production-traffic-evidence)。

**Validation:** 先执行单端口 cold/warm boot、link snapshot、RX/TX/abnormal IRQ 和 shutdown，再在同一 Gate
执行双端口 cold/warm boot、concurrent traffic、success-order identity、one-node failure isolation、shutdown/
reboot；确认无 unchanged-status immediate IRQ repeat，并取得 R2 延期的 VisionFive 2 双节点 DWMAC4
descriptor/PHY/IRQ/frame path/shutdown regression。RiscV regression失败必须停止，不能以 R2 Gate 1 closure覆盖。

**Cutover:** production implementation 完成，但 target contracts 仍保持 Not Cut Over，等待 Gate 4 独立闭合
审查；不得把单端口中间结果描述为 RFC target closure。

**Stop / Exit:** owner missing/duplicated、second IRQ request、ring/backing rebuild、publication-before-admission、
incorrect MAC/name、PHY delay/link failure、IRQ storm/loss、cleanup orphan、second-port 特判，或任一 correctness
invariant/owner/handoff/ABI/acceptance/validation claim变化。停止并进入 Target Renegotiation/Follow-up RFC。

## Gate 4 - Final acceptance and closure review

**Purpose:** 对 Gate 1--3 已完成实现和证据做独立最终审查，确认 RFC target、correctness invariants、
architecture-friction scan、acceptance 和 current-contract delta 全部闭合。本 Gate 不新增功能实现。

**Prerequisites:** Gate 1--3 均以各自 source/build/KUnit/hardware evidence 退出；没有 probe、single-port-only
path、未解释 regression 或 blocking tracking issue。

**Protected Boundary:** 不以 closure review 修改 target、owner、handoff、failure、cleanup、ABI、acceptance
或 validation strength；不在文档中把未运行的硬件证据写成 Passed。

**Deliverable:** 独立 source/owner/lifecycle review、最终验证矩阵、tracking issue disposition、Architecture
Friction Scan，以及与真实实现一致的 current-contract 更新和 RFC closure 记录。

**Validation:** 复核 Gate 1--3 的命令与 hardware logs；抽查 compatible dispatch、enhanced/extended descriptor、32-bit
DMA、Route A/PHY transaction、IRQ type/W1C、per-node failure isolation、双端口 traffic/shutdown/reboot 和
JH7110 regression。发现实现缺口必须退回对应实现 Gate，不能在 Gate 4 内补代码后直接 closure。

**Cutover:** independent review 通过后，原子执行 `DWMAC-IRQ-CUTOVER`、`DWMAC1000-CUTOVER` 和
`DWMAC-FINAL-CUTOVER`，更新真实受影响的 current contracts 并关闭 RFC；否则保持 Not Cut Over。

**Stop / Exit:** 任一 correctness invariant、owner/handoff、ABI、acceptance 或 validation claim 未闭合或发生
变化；停止并进入 Target Renegotiation/Follow-up RFC，不批准 closure。

## Target Renegotiation

若 enhanced/extended descriptor、Route A、32-bit DMA、PHY P1 或 `IrqSense` evidence 不能成立，必须记录真实失败证据、已完成 slice、代码处置、受影响 target/contract/acceptance 和 options。可选路线是 Route Correction、Accepted Reduced Target、Follow-up RFC 或 Not Cut Over；agent 不能自行批准 reduced target。

## 附录 A：Linux 6.6 DWMAC1000 完整初始化基线

本附录固定后续实现的协议来源，避免继续根据单次 2K1000 日志逐个猜寄存器。代码基线为仓库跟踪的
Linux 6.6.32 `drivers/net/ethernet/stmicro/stmmac/`，主要入口是 `stmmac_platform.c`、
`stmmac_main.c`、`dwmac1000_dma.c`、`dwmac1000_core.c`、`dwmac_lib.c`、`norm_desc.c`、
`mmc_core.c`、`stmmac_pcs.h` 和 `stmmac_selftests.c`；YT8511 基线是
`drivers/net/phy/motorcomm.c`。本附录描述 Linux 的完整顺序以及 Gate 2 对该顺序的有界裁剪，不把
Linux netdev/IRQ/runtime 功能误写成 Gate 2 已实现能力。

### A.1 输入、能力与禁止猜测规则

1. platform 层从 DT 取得 MMIO、IRQ、PHY interface/handle、MAC、`snps,clk-csr`、PBL、burst、FIFO、
   `force_thresh_dma_mode`、`force_sf_dma_mode` 和可选 AXI 配置。`force_thresh_dma_mode` 与
   `force_sf_dma_mode` 同时出现时，Linux 明确让 threshold 优先并忽略 SF；实现不得自行选择另一优先级。
2. legacy PBL 默认值是 8；`snps,pbl`、`snps,txpbl`、`snps,rxpbl` 只接受 DT binding 定义的
   `1/2/4/8/16/32`。`pblx8`、fixed burst、mixed burst、AAL 只来自对应 DT flag。不存在的 property
   使用 Linux 默认，不从某次 CSR0 readback 反推 policy。
3. MDIO divider来自合法 `snps,clk-csr`/`clk_csr`，或者 Linux clock owner根据真实 CSR clock计算。
   Route A 尚无 clock provider时，只允许采用 firmware 已写入且编码合法的 divider handoff；不得固定为某个
   2K1000 观测值。
4. `DMA_HW_FEATURE` 非零时按 Linux 位定义解码 MII/GMII/half-duplex、hash/multiple address、PCS、MDIO、
   PMT、MMC/RMON、PTP、EEE、TX/RX checksum、RX FIFO class、channel count和enhanced descriptor。
   当前 target 只有在 `ENHDESSEL=true` 时才可 admission；其它 capability 可以裁剪，但不得篡改 capability
   fact。若该寄存器为零，Linux把它视为老硬件未实现 capability register；当前 exact 3.70 target必须 fail
   closed，不得猜测 enhanced 支持。
5. 速率和双工来自 PHY link resolution，只允许 Clause 22 的 10/100/1000 与 half/full 合法组合；MAC
   `PS/FES/DM` 不得按端口号、网线位置或某次日志固定。FIFO精确大小只来自 DT或硬件能表达的精确 capability。
   legacy `RXFIFOSIZE` 只有“是否大于 2048”这一类事实，不能据此猜成 4 KiB或其它字节数。
6. 当前支持范围固定单 queue、ring mode、Linux enhanced/extended 32-byte descriptor、`ATDS=1`、32-bit DMA、
   标准 MTU和YT8511的`rgmii`/`rgmii-id`。不满足这些 admission 的硬件/DT应在任何 IRQ/publication 前明确
   失败，不能回退 normal descriptor、切换 board-specific speed、猜测未知 PHY transaction 或截断高地址。

### A.2 Linux probe/open 的完整协议顺序

以下是 Linux 6.6 的因果顺序；同一编号内的纯软件 bookkeeping 可裁剪，但硬件写入的依赖关系不可重排成
基于日志的试错序列。

1. **platform resource ownership**：解析 DT，取得并启用 platform glue 所拥有的 clock/reset/pinctrl，建立
   MMIO、MDIO/phylink和IRQ resource。当前 Route A 没有这些 provider时，只消费 firmware handoff；失败必须
   回 owner review，DWMAC owner不得直接写 LoongArch raw clock/reset/pinmux寄存器。
2. **hardware interface/capability admission**：匹配 DWMAC1000 ops，读取 Synopsys version和
   `DMA_HW_FEATURE`，用硬件 capability覆盖 Linux platform 中的enhanced descriptor、TX/RX checksum、PMT等
   capability facts，并根据 RGMII interface与`PCSSEL`选择 legacy RGMII PCS。
3. **PHY attach/init**：Linux连接外部 PHY，运行 phylib初始化与vendor `config_init`，再由phylink启动
   autoneg和link state。YT8511的Linux entry只提供`config_init`，没有YT8511专用`soft_reset`或专用
   `read_status`：`config_init`按interface修改page `0x0c/0x0d/0x27`，设置RGMII delay、125 MHz输出和
   sleep时PLL保持。若Gate 2主动做Clause 22 reset/restart，它必须遵守generic phylib语义并重新执行
   `config_init`；不能把vendor私有寄存器 `0x11` 当成Linux YT8511 driver的规范读取接口。
4. **最终 DMA backing 先建立**：Linux在`stmmac_hw_setup()`前分配最终RX/TX descriptor和buffer，并初始化
   完整rings。enhanced/extended ring中每个RX descriptor携带合法buffer length和OWN，只有最后一项置EOR；
   TX初始为CPU-owned，只有最后一项置EOR。Gate 2可只借用slot 0做proof，但必须使用同一份最终backing，不能
   在通过后以另一份production ring替换owner。
5. **DMA software reset**：对现有CSR0置SWR并bounded等待硬件清零。reset发生在descriptor backing建立后、
   任何base/start写入前。SWR timeout是MAC/DMA/external handoff failure，不是“未插网线”的合法结果。
6. **DMA CSR0初始化**：从reset后的CSR0按Linux规则设置USP、TX/RX PBL、pblx8、fixed/mixed burst、AAL和
   descriptor mode；capability-admitted enhanced target必须置`ATDS=1`并以readback确认。只有DT提供`snps,axi-config`时才按Linux AXI配置写
   outstanding limit、BLEN和LPI相关位；没有该property时不凭硬件型号制造AXI数值。
7. **DMA channel/base初始化**：建立CSR3 RX base和CSR4 TX base。所有descriptor ring、descriptor内buffer、
   frame backing的半开区间都必须通过32-bit address admission；任何结束地址越过4 GiB都在启动前失败。
8. **MAC address与core baseline**：写MAC address 0；按Linux `GMAC_CORE_INIT = JD | PS | BE | DCRS`
   建立core baseline，并只按实际MTU决定2K/jumbo位。当前标准MTU裁剪必须显式保持2K/jumbo关闭。RX checksum
   由Linux `rx_ipc`按已选择能力设置并readback；Gate 2裁剪checksum时必须明确清IPC且验证，而不是继承firmware。
9. **MAC/PCS interrupt baseline**：Linux core init写legacy MAC interrupt mask；有PCS时会让PCS/RGMII cause
   进入host handler。Gate 2尚无handler且architecture local IRQ未开启，因此R5唯一允许的偏差是同时保持
   CSR7=0并mask/drain全部device MAC/MMC cause；Gate 3建立handler后必须恢复Linux host-IRQ语义，不能把
   Gate 2 suppression沉淀为production协议。
10. **先启用MAC RX/TX**：Linux在DMA operation mode之前设置MAC RE/TE。link-up随后依据PHY state更新
    `PS/FES/DM`、pause mode和flow-control，再确保MAC enabled。Gate 2必须在内部loopback前取得有效resolved
    link，并以该事实配置speed/duplex；网线未插导致link timeout可以使该node独立失败，但不能影响另一node。
11. **选择DMA operation mode**：Linux的选择是：`force_thresh_dma_mode`时TX/RX都使用threshold；否则
    `force_sf_dma_mode || tx_coe capability`时TX/RX都使用store-and-forward；否则TX使用默认64-byte
    threshold、RX使用store-and-forward。TX SF同时启用OSF；threshold模式必须清TSF并按Linux encoding设置
    TTC，不能无条件写SF。RX SF设置RSF。RX FIFO大小未知或小于4 KiB时Linux清EFC/RFA/RFD；只有DT给出至少
    4 KiB的真实FIFO大小时才按Linux的full-minus-1K/full-minus-2K阈值启用flow control。
12. **MMC初始化**：始终mask MMC RX/TX/IPC interrupts；硬件报告RMON时，Linux还以
    `RESET_ON_READ | COUNTER_RESET | PRESET | FULL_HALF_PRESET`初始化MMC control。只写mask而遗漏control
    不是完整Linux初始化。
13. **可选能力初始化或显式裁剪**：Linux会按capability/config继续初始化PTP counter、RX watchdog、PCS AN、
    TSO、split header、VLAN insertion、TBS、EEE等。Gate 2不实现这些runtime能力时，必须选择Linux已有的
   disabled branch或写规范定义的disabled baseline：enhanced descriptor不置checksum/timestamp/VLAN位，
    MAC IPC关闭，VLAN tag为0，PMT power-down/wake bits为0，LPI enable/automate bits清零，PTP TCR关闭，
    TSO/TBS/split-header不启用。RX watchdog使用Linux `riwt_off`式“不要编程”裁剪，不发明未知disable数值。
14. **PCS配置**：若`PCSSEL`与RGMII使Linux选择legacy PCS，Linux在启动DMA前对PCS AN control执行enable/restart。
    `snps,ps-speed`只在DT给出合法10/100/1000时参与core预配置；缺失时不得固定speed。Gate 2若因无IRQ而mask
    PCS cause，仍不能省略Linux要求的PCS control配置；必须记录AN control/status readback。
15. **启动顺序**：Linux先启动所有RX DMA channel，再启动所有TX DMA channel。Gate 2的单queue顺序必须是
    MAC RE/TE -> RX `SR` -> TX `ST`。empty netdev address list对应hash表清零、frame filter选择HPF并只用
    MAC address 0做perfect match；该filter必须在loopback packet可能进入RX前生效。
16. **TX commit**：Linux先写完enhanced descriptor的address、length、FS/LS和裁剪后的offload bits，执行DMA
    ordering，再最后发布OWN，随后写CSR1 poll demand。不得把含OWN的整descriptor作为一次普通结构体写提交给
    已运行DMA。
17. **MAC loopback characterization**：Linux offline MAC loopback只允许在valid carrier上运行，先置LM，再走
    正常TX/RX data path并等待packet，最后清LM。Gate 2用bounded polling替代IRQ/NAPI等待，但descriptor与
    payload语义不变：TX/RX OWN都完成、无descriptor error、RX为single frame、raw length与ACS/FCS策略一致、
    payload一致；一次TX OWN清除或CSR5 `TU/TPS/RPS`不能代替RX completion。
18. **cause处理与退出**：Linux handler读取CSR5、只W1C定义的cause，并分别处理MAC/PCS/MMC read-to-clear
    cause。R5在CSR7=0下直接轮询raw CSR5，每次只W1C `CSR5_W1C_MASK`并立即readback；RI/TI仍是当前RFC oracle，
    在没有新的RFC acceptance前不得静默删除。cleanup顺序固定mask causes -> stop RX DMA -> stop TX DMA ->
    disable MAC/LM -> bounded证明process stopped和legal cause清零；只有证明后才可释放backing。Linux netdev
    release中的IRQ/queue/free bookkeeping不适用于Gate 2，但硬件stop顺序和DMA lifetime约束不能裁掉。

### A.3 Gate 2 支持矩阵

| Linux能力/步骤 | Gate 2处置 | 必须留下的证据 |
| --- | --- | --- |
| current DT、compatible、MMIO、MAC、PHY、IRQ resource | 实现；每node独立admission | firmware/resource结构化日志 |
| clock/reset/pinctrl provider | Route A firmware handoff；不写SoC raw寄存器 | SWR、MDIO和cold/warm后续证据 |
| enhanced/extended rings、32-bit DMA、CSR3/CSR4 | 完整实现 | 全部半开区间、ATDS=1、32-byte stride、base readback |
| PBL/burst/AXI | 只按Linux DT/default；无AXI property则不猜 | DT来源、CSR0 readback |
| PHY/YT8511、10/100/1000、duplex | 支持当前YT8511与`rgmii`/`rgmii-id` | ID、vendor page readback、resolved link |
| TX/RX DMA mode | 按force flags与TXCOE capability选择 | mode来源、CSR6 readback |
| MMC/RMON | mask全部；有RMON时初始化control | capability、MMC control/mask readback |
| PCS RGMII | 按PCSSEL+interface执行Linux AN control | AN control/status、MAC mask策略 |
| checksum/TSO/PTP/EEE/PMT/VLAN/jumbo/multi-queue | 裁剪且显式disabled | capability与disabled register/descriptor readback |
| DMA IRQ/NAPI/netdev publication | Gate 2禁止；CSR7=0 polling | CSR7持续为0、无IRQ request/publication |
| MAC loopback proof | 支持；只在resolved link后执行 | RI/TI、双descriptor、length/error、payload、W1C |
| failure cleanup | RX -> TX -> MAC，quiescence后才释放 | process state、ST/SR/RE/TE、final CSR5 |

### A.4 Gate 2 硬件证据与重验

本附录中的 Linux-shaped software correction 已一次闭合：最终 backing 在 SWR 前建立；YT8511 只保留
Linux `config_init` 与 generic Clause 22 resolution；DMA mode、capability admission、MMC、PCS、optional
disabled baseline、MAC/DMA start sequence 和 enhanced/extended descriptor 已进入同一初始化 owner。当前
实现不保留 normal descriptor path，`ENHDESSEL=false` 在启动前 fail closed。

`etc/log-board-la-new-35.log` 已完成 Gate 2 重验；后续 closure review 只需保留以下证据边界：

1. `ethernet@40040000`记录`selected-desc=enhanced`、`descriptor-stride=32`、`atds=true`和低于4GiB的完整DMA
   backing；TX OWN发布后清零，RX OWN清零并写回`rx-len=68`、single-frame、无descriptor error且payload匹配。
2. CSR5记录`RI|TI`，ETI作为Linux statistical evidence保留，fatal abnormal为0；三次legal W1C后的
   `uncleared=0`，cleanup时DMA/MAC process均停止且owner为`BindRetained`。
3. 同次启动的`ethernet@40050000`在无线缆状态下于generic PHY deadline独立失败；它使用另一份DMA allocation，
   未释放或覆盖首个node的retained backing，也未隐藏failure。
4. `new-33` 暴露的 TU 分类时序依赖和 KUnit 边界样例错误已修复，并由 `new-35` 三次重复运行收口。
   Gate 3 IRQ request、handler/level-flow、双端口 production traffic、cold/warm/bootloader 矩阵和 JH7110
   regression 仍未授权或未运行。
