# JH7110 GMAC 实施路线

**状态：** Accepted / R3
**最后更新：** 2026-08-09
**父 RFC：** [RFC-20260808-jh7110-gmac](./index.md)
**当前修订：** R3

## 全局 Implementation Boundary

- **Target / non-goals：** 任意有限数量 JH7110 matching nodes 的 one-time boot driver、命名 IRQ、
  coherent DMA、per-node provider、稳定 `eth<N>` 与既有单接口 static IPv4；不实现板级
  clock/reset/syscon/PHY owner、runtime lifecycle、offload、多 queue 或多 IP。现有 boot-time
  external publisher（包括 VirtIO）必须迁移到同一 reservation handoff，避免按 provider 类型分叉。
- **Owner / handoff / failure / cleanup：** generic clock/reset providers 拥有 consumer admission transaction，
  已 enable clock 与已完成 reset 不回滚，reset failure 只隔离当前 node；per-node provider拥有 hardware
  progression；IRQ、logical、netdev、Stack、control plane 与 System Power 保持各自 owner。opaque reservation 与 frame/pump
  capability单向移交；failure 先撤销未发布 mapping/reservation，IRQ/DMA 未 quiesce时 retain 到
  reset/power-off。
- **Protected ABI / contract / acceptance：** SystemTarget schema、socket ABI、frame contract、VirtIO
  path与 shutdown order 不变。`IRQ-FLOW-001`、`NET-IFACE-DOMAIN-001`、`NET-ATTACH-001` 只在最终
  `JH7110-GMAC-CUTOVER` 原子 Refine。QEMU 永远不是本 RFC acceptance。
- **Validation claim：** 每 Gate 后只形成 source/build/targeted-test 检查；所有 Gate 完成后才运行
  VisionFive 2 完整验收。在此之前最终硬件 acceptance 事实保持 Not Run；Gate-specific board
  diagnostics 只作为诊断证据，不改变该边界。
- **Stop conditions：** 发现需要改变 target/owner/handoff/failure/cleanup/ABI/contract/acceptance，
  需要扩大 generic controller API/contract、让 GMAC 保存 clock/reset 状态、fence 被误当作 coherency
  保证，或不能保持 per-node isolation，
  必须停止并回 RFC review / Target Renegotiation。

Gate 0--3 是同一 RFC 实现的有序阶段，不是独立产品 release。用户后续若只授权某一个 Gate，完成其
post-gate check 后必须停止。本文当前只规划路线，不授权实现。

## Gate 0 — Per-node discovery、resource、IRQ 与 controller/firmware handoff

**状态：** Closed (Gate 0 implementation + board diagnostic)
**Purpose：** 建立每个 matching node 的独立 probe 基础、准确 DT resource 事实和可复用的单项 IRQ
selector；不启动 DMA、不发布 netdev。
**Prerequisites：** RFC target/owner/contract delta 已 review；实现 Gate 0 获得明确授权。
**Protected Boundary：** 不建立 GMAC bus/registry；clock/reset 只走 generic provider，不解析 raw ID、
不写 controller register、不保存其状态；不写 syscon/PHY；不硬编码 GMAC 数量、MMIO、IRQ 或 MAC；
不 publication/attach；IRQ flow/device cause owner不变。

**Deliverable：**

- 为 observed JH7110 compatible 注册一个 platform driver，使 DT 中每个 matching `okay` node 独立
  进入 probe；保留 DT path/origin 作为诊断 identity。
- 解析并验证单个 MMIO resource、`phy-mode = "rgmii-id"`、有效的 DT `local-mac-address` 以及
  target 所需的 frame/DMA
  register capability。任何节点单独失败，不影响后续节点 probe。
- 增加 crate-local FwNode/IRQ resource selector，以 index 或 `interrupt-names` name 截取一个完整
  specifier；现有 public `request_irq()` 保持不变，multi-interrupt caller 必须使用 crate-local selected
  path。JH7110 只选择 `macirq`，既有单中断 caller 显式选择 index 0。
- 建立 per-node IRQ private context 和 device-cause enable/ack/disable 形状，但 Gate 0 不实际注册 JH7110
  IRQ；当前 `request_irq()` 会 unmask 且没有 `free_irq`，真实调用必须等 Gate 1 rings、handler context
  与 cause clear 全部 ready。不能注册空 handler。
- 输出 bounded diagnostics：node path、MAC、MMIO range、selected interrupt name/index/specifier length、
  `phy-mode` 与 controller/firmware-handoff assumption。不得打印任意 DT property bytes 或敏感无关数据。
- Gate 0 的临时 probe 以明确 unsupported/not-ready 结果停在 IRQ registration、DMA 与 publication 前；
  该临时退出必须在 Gate 3 删除，不能成为长期 fallback。

**Post-gate check：**

- Source audit：证明 platform discovery 对每个 matching node 独立 probe，所有资源来自本 node；检查
  selector resolution、specifier cell slicing、mapping-before-unmask 与 device-cause责任。
- Targeted tests：synthetic DT 覆盖 0/1/多中断、name/index、duplicate/missing name、越界、属性长度、
  两个 matching node 与第一个 node 失败。
- Build：通过仓库 Justfile/xtask 的相关 RV64 kernel build 和格式检查；不以 bare Cargo 替代项目入口。
- Architecture Friction Scan：检查 singleton instance、raw PLIC parse、第二份 identity map、无退出条件
  probe 或扩大 FwNode API。Keter/Apollyon 阻止 Gate 0 关闭。
- **Evidence：** `just fmt kernel`、`just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`
  与 `just build --preset visionfive2-rv64-release` 通过；VisionFive 2 build 同时编译 `kunit`。QEMU virt
  没有 JH7110 GMAC，运行日志因其 rootfs 期望的 `mmcblk0p3` 不存在而停止，不能作为本 Gate 的 GMAC
  targeted test 或硬件证据。FwNode/IRQ owner-local KUnit 已在此前 Gate 0 build/test pass 中覆盖
  name/index、specifier slicing、多节点独立失败与仅 `local-mac-address` 解析。
- **Board diagnostic：** 用户运行修复后的 production build；被 `.gitignore` 忽略的用户私有日志
  `etc/log-board-rv-13.log` 记录了 GMAC0/GMAC1 各自的 capability probe：register snapshot 中
  `version=0x4152` 的低 8 位匹配 DWMAC 5.20，`hw_feature0..2` 非零，随后各自输出
  `local-mac-address`、独立 MMIO、`macirq` index 0/specifier length 4、`rgmii-id` 和单 RX/TX queue。
  该 snapshot 属于诊断运行；当前 production 成功路径只保留 bounded capability fields。probe 随后按
  Gate 0 设计返回 `NotYetImplemented`，表示 attach 仍延期；该诊断不是完整硬件 acceptance。
- **Hardware status：** Passed (Gate 0 board diagnostic)。用户确认同一 production build 已在
  VisionFive 2 上完成本 Gate 的 capability/resource 诊断；该结果只覆盖本 Gate 的 MMIO、DWMAC
  version/features、DT MAC、`macirq` selector 与 `rgmii-id` 事实，不声称真实 `macirq` dispatch、
  DMA、收发或完整 controller/firmware handoff 已验收。

**R3 route correction：** GMAC probe 通过 generic `require_clock()`/`require_reset()` 消费 DT 声明的
provider capability；clock/reset register 与 transaction truth 仍只在 machine/provider。用户接受
boot-lifetime no-rollback：已 enable clock 和已完成 reset 不撤销，reset failure 只隔离当前 node；GMAC
不保存状态或执行补偿。原 firmware-only handoff owner mismatch 已闭合，不构成 current-contract cutover。

**Cutover：** None。
**Stop / Exit：** Gate 0 source/build/targeted semantic checks、architecture friction scan 与本 Gate
board diagnostic 已完成；最终 hardware acceptance 与 current contract 保持 Not Run/未切换。本
checkpoint 没有自动授权 Gate 1；Gate 1 的后续 closure 由下节独立记录。

## Gate 1 — Coherent DMA 与 ring ownership

**状态：** Closed
**Purpose：** 建立可审计的 coherent DMA sync/order 语义、DMA addressability 与 bounded RX/TX ownership；
仍不发布 netdev。
**Prerequisites：** Gate 0 post-gate check 关闭；R3 controller admission/no-rollback semantics 已接受；
用户提供的板级 coherency proof 已接受；DMA address
width、descriptor/frame hardware alignment 与 ordering/fence semantics 已确定。
**Protected Boundary：** `NET-FRAME-OWN-001` 不变；coherent visibility 与 ordering fence 分离；没有
quiesce proof 不释放 backing；不借机实现 generic DWMAC/offload/multi-queue。

**Deliverable：**

- 为 DMA sync surface 建立 coherent visibility 与 ordering 的真实语义；JH7110 采用 coherent path，
  不添加未经硬件要求的 clean/invalidate。
- 建立满足 DWMAC 硬件约束的 descriptor/frame allocation、40-bit DMA address validation 和 Kconfig-owned
  bounded RX/TX ring capacity；重要 ring/alignment 常量不散落在 driver 内。
- 实现单 RX queue 与单 TX queue 的 owner transition、descriptor initialization、RX refill、TX submit、
  completion/reclaim、queue exhaustion/backpressure 和 error unwind。
- rings、handler context、device status clear 与 retention path 全部 ready 后，才以 Gate 0 的 selector
  注册 `macirq` 并启用 device-side cause；失败不得留下可访问已释放 backing 的 IRQ context。
- 在 production call sites 保持方向正确的 fence/MMIO ordering；descriptor 与 payload 仍按 device 实际
  读写方向进行 ownership handoff，不能因 coherent 而跳过 publication/completion ordering。
- IRQ context 只发布 completion/recheck事实；真实 frame progression 保持在 provider/worker side。
- Gate 1 仍在 netdev publication 前停止。所有已启动 DMA 在 probe退出前必须证明未启动、已 quiesce，
  或按 terminal retention 规则保留；不能假装释放。

**Post-gate check：**

- Source/ownership audit：逐 transition 检查 CPU/device owner、visibility/order handoff、descriptor ownership bit、
  doorbell/completion ordering、address truncation 与 failure cleanup。
- Targeted tests：production sync/order call sites 必须可记录或可审计；覆盖 descriptor/frame hardware
  alignment、ring wrap、full/empty、RX/TX completion、stale completion、40-bit address overflow、partial
  allocation 与 unwind/retention。
- Build/architecture audit：相关 RV64 build、格式检查和生成指令/architecture capability 审计；编译成功
  只证明路径存在，不证明板上 coherency/order correctness。
- Architecture Friction Scan：检查第二份 coherency truth、generic DMA API 为单 driver 过度扩张、
  test-only bypass 与隐含 quiesce。
- **Evidence：** VisionFive 2 RV64 release kernel build 与 xtask `fmt all --check` 通过；owner-local
  KUnit 覆盖 FwNode、IRQ cause、descriptor layout、40-bit address boundary、ring wrap/full、RX refill、
  TX reclaim 与 durable pending cause。RV64 production image 的反汇编确认 MMIO 路径生成 `fence w,o`
  与 `fence i,ir`。Gate 1 source/ownership review 确认 DMA engine 保持 stopped，IRQ 只在 rings/context/
  cause baseline ready 后注册，临时 probe error 前禁用 device cause，并由已注册 IRQ context retain
  backing；该抑制在 Gate 3 成功 publication path 删除。完整 516-case KUnit suite 另在既有
  backtrace/symtab fixture 失败；该 fixture 使用 discovery-pass 空 symbol table，不属于本 Gate 的 GMAC
  targeted evidence。
- **Hardware status：** Not Run。Gate 1 后不上板；硬件 coherency、DMA engine 和 ring traffic 仍未验收。

**Cutover：** None。
**Stop / Exit：** Coherency 前提、DMA address width/representation、ordering、ownership handoff 与
registered-IRQ retention 的静态/定向证据已通过，本阶段关闭。硬件 DMA/ring traffic 仍属于最终板级
验收，保持 Not Run；Gate 2 未获授权，不在本阶段自动进入。

## Gate 2 — Per-node provider 与稳定 identity

**状态：** Planned / Not Run
**Purpose：** 把 Gate 1 的 hardware core 组成独立 `FrameProvider`，建立 early logical reservation、
多 provider isolation 和现有 global Stack 所需的窄能力；仍不进入 production publication。
**Prerequisites：** Gate 1 post-gate check 关闭；ring/DMA/IRQ ownership 已在 source/tests 中闭合。
**Protected Boundary：** logical owner唯一分配 `eth<N>`；driver只携带 token；netdev/logical/protocol
identity不合并；每个 provider独立，不增加 GMAC registry 或 per-provider Stack。

**Deliverable：**

- 在 matching-node probe admission 先向 initial-domain logical owner取得 external reservation，随后才
  解析会失败的 MAC/resource/DMA；DT discovery order 成为 ordinal order。
- reservation 是线性、不可复制、必须 commit/abort 的 token。失败 abort 不发布 membership但不复用
  ordinal；provider/device/net 不保存第二份 path-to-name truth。
- 现有 VirtIO boot-time provider 迁移到同一 probe-admission reservation path；其 QEMU 可见行为仍是
  单个 `eth0`，不再保留“JH7110 提前 reservation、VirtIO attach 时 reservation”的双语义。
- 每个 node 实现独立 `FrameProvider` / `NetdevFrameProvider`，拥有自己的 rings、IRQ context、pending
  predicate、wake edge、frame capacity 和 current link/resource truth。
- 扩展 pending publication/attach handoff，使 opaque logical reservation 可与对应 provider 一起到达
  attach authority；`device/net` 不读取或分配 logical identity。
- host composition 以一个 global Stack、两个以上 fake providers 验证 pump/mapping isolation；一个
  provider blocked/failed 不阻止另一个 finite progress。
- Gate 2 只准备 ready capability；production `publish()`/active attach wiring 保持关闭。任何临时开关或
  placeholder必须有 Gate 3 删除条件。

**Post-gate check：**

- Source audit：从 probe admission 追踪 reservation 到 abort/未来 commit，确认没有 driver map、token
  Drop遗漏、成功顺序编号或 provider交叉引用。
- Targeted tests：至少三个 matching candidates，覆盖全成功、首个失败、中间失败、attach prepare失败、
  ordinal no-reuse、wrong-provider completion、独立 wake/ring 和 one-Stack multi-provider progression。
- Build/regression：相关 RV64 build、格式检查；既有 logical/netdev/frame tests 全通过。
- Architecture Friction Scan：检查第二份 lifecycle/identity truth、raw Stack泄漏、provider singleton、
  无 consumer abstraction 与临时 publication bypass。
- **Hardware status：** Not Run。Gate 2 后不上板；双 GMAC identity和独立 progression仍未验收。

**Cutover：** None。
**Stop / Exit：** 任何实现若要求 driver 分配/查询 `eth<N>`、为每个 GMAC 建 Stack、让失败节点释放
ordinal 或把 provider 放入全局 GMAC registry，必须停止。检查通过后关闭本阶段并等待 Gate 3 授权。

## Gate 3 — Production publication、attach 与 single IPv4 deployment

**状态：** Planned / Not Run
**Purpose：** 删除实施期占位/禁用路径，完成 ready netdev publication、现有 attach/worker/global Stack、
单接口 static IPv4 和 terminal shutdown 的 production wiring。
**Prerequisites：** Gate 2 post-gate check 关闭；所有 temporary probe/activation path 的退出条件已定位。
**Protected Boundary：** publication/active commit原子性、single IPv4/default route、无 fallback、
provider retention 和 `filesystem -> network -> device -> PowerOff` 不变；不扩大 runtime lifecycle。

**Deliverable：**

- Provider 在 queue/RX refill/IRQ/DMA-order/wake 全部 ready 后一次性发布 netdev，并携带对应 opaque logical
  reservation；删除 Gate 0--2 的 placeholder error、dormant switch 和 test-only activation branch。
- Attach authority 消费既有 reservation，建立 global Stack mapping、inactive worker、wake/time wiring，
  最后 commit logical member与active path并activate。rollback 保持 mapping 先于 reservation。
- 所有成功 GMAC 都形成独立 active L2 path；每个 worker只持自己的 provider与pump port。
- 现有 `StaticIpv4Deployment` 只匹配 configured `eth<N>`，只为它配置 address/prefix/default gateway；
  其它 active GMAC 保持无 IPv4。missing target/duplicate mismatch fail closed，不自动 fallback。
- RX/TX 正常 progression、bounded recheck、queue backpressure 和 device-cause处理接入 production worker。
- Network shutdown先关闭active pump；device shutdown随后对每个 provider禁止新 IRQ/DMA。无quiesce proof
  的 context/rings/backing retain到power-off。

**Post-gate check：**

- End-to-end source audit：从 DT node、reservation、resource/IRQ/DMA、publication、mapping、worker、IPv4
  selection 到 shutdown 逐 owner 核对；确认所有临时 probe/开关已删除。
- Targeted tests：publication/attach rollback、multi-provider active progression、configured `eth0`/`eth1`、
  missing target fail-closed、unselected no-L3、shutdown admission/retention 与既有 VirtIO regressions。
- Build：完整相关 RV64 kernel/target build与格式检查。可运行 QEMU existing-network regression，但必须
  标记 `regression-only`，不能作为 JH7110 acceptance。
- Architecture Friction Scan：检查 second truth、owner穿透、driver special-case control plane、隐含
  cleanup顺序、无退出条件 bridge以及通过弱化 test oracle 换取通过。
- **Hardware status：** Not Run。Gate 3 post-gate check 完成后，四个实现 Gate 才共同满足进入板级验收
  的前置条件；本检查本身不关闭 RFC 或 contract。

**Cutover：** None。
**Stop / Exit：** source/build/tests/architecture review通过且 implementation tree 中没有临时路径后关闭
Gate 3。若任何硬件相关 claim 被写成已通过，先纠正为 Not Run。随后才允许进入最终板级验收。

## 最终板级验收

**状态：** Not Run
**Purpose：** 在全部 Gate 实现和 post-gate check 完成后，以 VisionFive 2 对同一 production build
一次性验证完整 target；这是唯一 JH7110 acceptance，不是第五个实现 Gate。
**Prerequisites：** Gate 0--3 均关闭；无 blocking Architecture Friction；production build无 probe占位、
coherency bypass或test-only activation；测试所需两路外部 peer 与可切换 SystemTarget 配置已准备。
**Protected Boundary：** 不为过板把未证明的 coherency 假设写成硬件保证、跳过 ordering/ownership proof、
硬编码 node/IRQ/MAC、给两个接口复制同一 IP，或降低 failure/shutdown oracle。

### Acceptance matrix

- **Fresh discovery：** 冷启动日志逐一列出所有 target nodes 的稳定 DT path、有效 DT MAC source（记录
  实际属性，当前预期为 `local-mac-address`）、独立 MMIO、name/index选择的 `macirq`、`rgmii-id` 与
  per-node provider identity。GMAC0/GMAC1 必须分别有证据；一个节点的事实不能代替另一个。
- **Stable identity：** 至少两次冷启动保持相同 DT-order -> `eth<N>` mapping。以受控失败 fixture/DT
  让较早节点失败时，后续节点保持原 ordinal且 configured missing interface fail closed，不发生 fallback。
- **Port A：** 使用现有单 `[network.ipv4]` 形状选择第一个 GMAC，与其外部 peer 完成双向流量；payload/
  counters证明 RX/TX 都跨越 ring wrap，并覆盖 queue pressure 后恢复。
- **Port B：** 只改变 `network.ipv4.interface` 选择第二个 GMAC，重复独立双向、ring wrap与queue pressure
  验证；另一接口仍 active L2但没有 IPv4/default route。
- **Coherent DMA：** 两个端口分别在多种 frame length 与重复 RX/TX 下没有 stale payload、重复/丢失
  completion、descriptor corruption、地址截断或 use-after-submit。证据必须同时覆盖硬件 coherency
  与 production path 的 fence/MMIO ordering；coherency 证明不替代 ownership/quiesce 证据。
- **Isolation：** 每个 IRQ、completion、wake、worker、mapping 与计数只推进对应 node；一个端口无流量、
  queue pressure或受控失败不阻塞/重编号另一个。
- **Control plane：** 每次启动只有 configured `eth<N>` 获得 address/prefix 和唯一 default route；其它
  active接口保持无L3；不存在自动 fallback或重复地址。
- **Shutdown：** orderly shutdown 日志保持 `filesystem -> network -> device -> PowerOff`；network stop后
  无新的 worker reactivation，device step抑制每个 GMAC 的 IRQ/DMA，未证明 quiesce 的 backing保持到
  reset/power-off。

QEMU、host fake DMA/order backend和KUnit结果作为前置/回归证据附带记录，但不能替代以上任一板级项。
若板级失败暴露 route correction，可在不改变 target/owner/acceptance 的前提下修复并重新执行完整 matrix；
若要求改变这些边界，进入 Target Renegotiation，不能只豁免失败项。

### Cutover 与 closure

全部 matrix 在同一可审查 production revision 上通过后，执行单个 `JH7110-GMAC-CUTOVER`：

- Refine `IRQ-FLOW-001`，纳入 name/index 单项 firmware interrupt resource selection；
- Refine `NET-IFACE-DOMAIN-001`，纳入 probe-admission reservation 与失败 no-reuse；
- Refine `NET-ATTACH-001`，纳入对 publication 携带 reservation 的消费与既有 rollback；
- 更新 RFC closure，记录 agent-run、user-run、Not Run、commit/PR 和仍开放的真正非目标；
- 对最终 diff 执行 Architecture Friction Scan。Euclid 可带证据收口；Keter/Apollyon 阻止 cutover/closure。

任一 acceptance item 缺失时三项 contract 全部保持旧规则，RFC 保持未关闭；不允许部分 cutover。
