# DWMAC RFC 实施路线

**状态：** Accepted / R3 / Gate 1 Closed; Gate 2 Authorized / Not Cut Over
**最后更新：** 2026-08-13
**父 RFC：** [RFC-20260811-dwmac](./index.md)
**当前修订：** R3

本文保存本 RFC 的实现 Gate、probe、验证和停止边界；不冻结逐文件 write set，也不授权未审查的后续
Gate。事实调查属于 RFC 正文和 backgrounds，不单独占 Gate；Gate 1--3 必须各自产生可审查的实现，
source/hardware regression 是对应实现 Gate 的退出验证。只有 Gate 4 是不新增实现能力的最终闭合审查。

## 全局 Implementation Boundary

- **Target / non-goals：** 见 [RFC 正文](./index.md)；DTB 保持不变，DWMAC1000 R0 仅 normal descriptor、32-bit DMA、CSR5 W1C 和 boot-time PHY P1。
- **Owner / handoff / failure / cleanup：** concrete DWMAC backend owns MAC/DMA/rings/device cause；Loongson irqchip owns `IrqSense`/`EDGE/POL`/flow；generic IRQ core owns dispatch；Route A firmware owns external clock/reset/pinctrl。DWMAC1000 Gate 2 creates one long-lived per-node owner before IRQ request；after IRQ commit the bound platform device retains that same owner/context/backing and Gate 3 may only adopt it in place。Backing release requires proven TX/RX process-state quiescence；otherwise retain through shutdown/power-off。
- **Protected ABI / contract / acceptance：** existing JH7110 visible behavior、network/Stack/attach、current DTB、`eth<N>` success-order semantics and userspace ABI；target contract delta remains Not Cut Over until final Gate 4。
- **Validation claim：** source/KUnit/build can close software protocol; only 2K1000/JH7110 hardware can close core, handoff, PHY, electrical IRQ and traffic claims。
- **Stop conditions：** target/owner/ABI/DTB boundary changes；normal mode fails；Route A/PHY owner unresolved；32-bit address invariant requires allocator/bounce；`IrqSense` expectation becomes a broader generic ABI。

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
PHY ID/RGMII delay 和 normal-mode facts还必须足以支撑实现。缺失事实是本 Gate 的启动/停止条件，不是新的
事实 Gate。

**Protected Boundary:** DWMAC1000 backend 不改变 DWMAC4；不改变 DTB；不注册 active netdev、Stack
membership 或 `eth<N>`。Gate 2 context 是 Gate 3 必须原位采用的长期 concrete owner，不形成 public API、
旁路 registry、临时 handler 或第二条 production path。platform bind success 只提交 owner lifecycle，不声明
bounded characterization passed。

**Deliverable:**

- `snps,dwmac-3.70a` compatible/version/capability admission；DWMAC internal DMA SW reset 和 timeout diagnostics。
- `ATDS=0` normal descriptor/16-byte stride ring；`ENHDESSEL` diagnostic only；normal OWN/length/end-ring tests。
- full checked 32-bit DMA range for ring/base/frame/next；越界 before IRQ/publication failure。
- Route A capability/readability handoff check；MDIO divider/read/write；按已确认 PHY ID 实现 P1 reset/fixup/
  autonegotiation/link snapshot，不能闭合 `rgmii-id` reset effect 时禁止 production reset。
- `Some(IrqSense::LevelLow)` request wiring；CSR5 W1C mask/read/write；RX/TX bounded loopback or equivalent
  hardware event without publication。
- IRQ request 前创建唯一 per-node owner；request commit 后把 MMIO、IRQ context/mapping、rings、DMA backing
  和 characterization result 保存在 bound device 上。成功和失败 characterization 都必须先 mask CSR7、
  stop MAC/DMA 并记录 TX/RX process-state；未证明 quiescence 时禁止释放 backing。
- strong-reference publication固定为 local owner -> IRQ descriptor private data -> request commit/unmask ->
  device `drv_state` -> bounded characterization；各处引用都是同一 owner。commit期间handler由descriptor保活，
  commit前失败可正常cleanup；commit后handler可在device state/bus bind前安全运行，所有characterization结果
  都以`Ok(())`完成bus bind。
- 严格 IRQ oracle：分别证明 RI、TI，abnormal cause 单独累计并使 characterization fail；每次 legal W1C 后
  累计 uncleared cause、拒绝 unchanged-status immediate repeat，并在 unmask 前通过 irqchip-owned observation
  证明 controller pending clear。Loongson concrete irqchip在每次实际`EnableSet`前读取并输出pending，包括
  request commit首次unmask与level-flow tail；不新增DWMAC pending API或generic flow semantic。

**Validation:** owner-local KUnit/source tests、kernel build、cold/warm/bootloader-used bounded hardware bring-up；
记录 version/capability、DMA reset deadline、DMA addresses、MDIO/PHY/RGMII state、CSR5 before/after、
`EDGE/POL`/pending readback 和 failure logs；Gate 1 的 exact `EDGE/POL` readback作为 baseline，不替代
Gate 2 device-cause/pending/unmask sequence。source/KUnit还必须覆盖 request 前普通 cleanup、request commit 后
success/failure owner retained、quiesce timeout retains backing，以及 platform bind不产生 netdev/publication。

**Cutover:** `DWMAC1000-CUTOVER` 仍 Not Cut Over；不写 current contracts。

**Stop / Exit:** Gate 2 只有在 Route A/PHY、normal descriptor、32-bit DMA、RI/TI/abnormal CSR5、pending-clear/
unmask ordering、unchanged-repeat rejection 和 quiescence evidence 全部闭合后退出。unexpected register family、
DMA high address、W1C failure、IRQ sense mismatch、reset/MDIO timeout 或 bounded characterization failure 必须
明确记录，并在 IRQ commit 前安全释放或在 commit 后由 disabled bound owner 保留；不得伪装成 Gate 2 pass。
若需要 enhanced/DMA32/bounce/DTB change、IRQ retirement，不能让 device `drv_state`/IRQ descriptor唯一共享
长期 context，或 pending oracle 需要扩大 DWMAC/generic IRQ API，停止并进行 target/owner review；不能把
probe 留成并列 backend。

### Probe P1 - Normal descriptor and device-cause slice

**Hypothesis:** 2K1000 DWMAC 3.70a accepts legacy normal descriptor with `ATDS=0`, and CSR5 low cause bits are W1C as Linux/PMON indicate。

**Protected Boundary:** 不 publish netdev、不修改 current contract、不沉淀 generic descriptor API、失败删除 probe path。

**Non-goals:** enhanced/extended/PTP、runtime PHY/link、traffic performance、high-address allocation。

**Validation:** one bounded TX and RX OWN/status transition；CSR5 read -> legal W1C write -> CSR5 readback；IRQ mask/pending/unmask ordering；two-port source mapping if hardware permits。

**Failure Signals:** OWN never clears、length/status layout mismatch、CSR5 cause persists、unchanged-status immediate repeat、`EDGE/POL` mismatch、DMA address truncation。

**Write-back:** evidence folds into Gate 2 implementation; failure stops Gate 2 and records target renegotiation.
Probe code is deleted or absorbed into the production backend before Gate 2 exits。

**Exit:** normal probe proven and absorbed into the long-lived per-node backend owner, or Not Cut Over with a
disabled retained owner/code disposition。不得在 IRQ commit 后返回 unbound orphan。

## Gate 3 - Per-node production attach

**Purpose:** 在 Gate 3-enabled kernel 的同一次 platform `probe()` 中，原位采用 Gate 2 已写入 device
`drv_state`并交给 IRQ descriptor 的 DWMAC1000 owner，接入 existing
FrameProvider/worker/publication/attach path，完成
不固定实例数的 per-node production implementation。单端口 bring-up 只是该 Gate 内的第一段验证，不形成
独立 Gate 或 single-port-only production target。

**Prerequisites:** Gate 2 backend/bounded bring-up closure；每个可接入 node 都保留唯一、disabled、
characterization-passed owner，Route A、PHY、MAC、IRQ、DMA、normal descriptor 和 quiescence evidence 已关闭；
R2 延期的 DWMAC4 regression 必须在本 Gate 取得并保持 green。

**Protected Boundary:** failure node does not consume `eth<N>`；其它 matching node 独立继续 probe；DTB、
existing network ABI、global Stack、attach owner 和 DWMAC4 behavior 不变。

**Deliverable:**

- 每个 matching node 通过 private continuation 原位取得 Gate 2 owner，在其已有 PHY snapshot、rings、IRQ context 和
  DMA backing 上建立 FrameProvider、worker、publication 和 attach；不得再次执行 IRQ request、ring/backing
  construction、替换一次性 device `drv_state` 或建立并列 MMIO/device-cause state。该路径不依赖另一次
  reprobe，也不假设 owner跨kernel reboot存活。
- active logical identity 只在成功 publication/attach 时按成功顺序连续分配；不按 MMIO、IRQ、alias 或
  candidate ordinal 固定 `eth<N>`。
- 同一 per-node path 支持两个当前 2K1000 node；不增加 GMAC0/GMAC1 特判、固定数组或 second-port adapter。
- 任一 node 的 pre-publication failure先撤销新增的 provider/worker publication attempt，并把同一 owner恢复为
  disabled retained state；只有 quiescence已证明时才可释放非 IRQ-retained capability，且不阻止其它 node成功。

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

**Validation:** 复核 Gate 1--3 的命令与 hardware logs；抽查 compatible dispatch、normal descriptor、32-bit
DMA、Route A/PHY transaction、IRQ type/W1C、per-node failure isolation、双端口 traffic/shutdown/reboot 和
JH7110 regression。发现实现缺口必须退回对应实现 Gate，不能在 Gate 4 内补代码后直接 closure。

**Cutover:** independent review 通过后，原子执行 `DWMAC-IRQ-CUTOVER`、`DWMAC1000-CUTOVER` 和
`DWMAC-FINAL-CUTOVER`，更新真实受影响的 current contracts 并关闭 RFC；否则保持 Not Cut Over。

**Stop / Exit:** 任一 correctness invariant、owner/handoff、ABI、acceptance 或 validation claim 未闭合或发生
变化；停止并进入 Target Renegotiation/Follow-up RFC，不批准 closure。

## Target Renegotiation

若 normal-only、Route A、32-bit DMA、PHY P1 或 `IrqSense` evidence 不能成立，必须记录真实失败证据、已完成 slice、代码处置、受影响 target/contract/acceptance 和 options。可选路线是 Route Correction、Accepted Reduced Target、Follow-up RFC 或 Not Cut Over；agent 不能自行批准 reduced target。
