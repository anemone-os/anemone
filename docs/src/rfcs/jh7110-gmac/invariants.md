# JH7110 GMAC 目标与不变量

**状态：** Accepted / R3
**最后更新：** 2026-08-09
**父 RFC：** [RFC-20260808-jh7110-gmac](./index.md)
**适用修订：** R3

本文只定义本 RFC 的 target 与 proof obligations。当前 effective 规则以
[`docs/src/contracts/`](../../contracts.md) 为准；本页不能在最终板级验收和
`JH7110-GMAC-CUTOVER` 前覆盖 current contract。

## 规则分类

- **Correctness Invariant：** 唯一 owner、DMA visibility/ordering、并发、生命周期、cleanup 与 ABI 诚实性；
  不得通过 target renegotiation 降低。
- **Target Guarantee / Capability：** 本修订承诺的多节点、one-time init、稳定 identity 与单接口
  IPv4 能力；改变它必须进入 Target Renegotiation。
- **Implementation Preference：** register wrapper、内部类型、ring helper、文件布局和具体算法；
  不构成 target。

## Target Invariants

### JH-GMAC-001 — Matching node 各自形成 probe transaction

**分类：** Target Guarantee / Capability
**规则：** 每个 `status = "okay"` 且 compatible 匹配的 DT node 都按 platform discovery 顺序独立
进入一次 probe transaction。实现不得按 GMAC0/GMAC1 写死实例数组、基址、IRQ 或实例数量，也不得
建立 GMAC bus/global registry。
**Owner：** generic platform discovery/binding 拥有遍历与 bind；per-node driver transaction 只拥有
该 node 的 construction。
**违反表现：** 只 probe 第一个匹配节点；通过 MMIO 地址分支选择静态状态；一个 singleton provider
覆盖另一个 node；后一个节点读取前一个节点资源。
**Proof：** synthetic DT 多节点测试、source audit 与最终板级逐 node 日志。

### JH-GMAC-002 — Controller admission 与 firmware handoff 各有唯一 owner

**分类：** Target Guarantee / Capability
**规则：** driver 必须按本节点 DT name 通过 generic providers 请求所需 clock enable 与 reset
transaction；不得解析 raw provider ID、写 controller register 或缓存 controller 状态。Clock enable 是
boot-lifetime 单调 admission，后续失败不回滚；reset failure 只让当前 node fail closed，已完成 reset 不
重放或补偿。firmware/board environment 继续拥有 clock rate/mux、syscon/RGMII path 与 PHY 初始化；不能
证明 link 时必须报告 `Unknown`。
**Owner：** generic clock/reset providers 唯一拥有 register 与 transaction truth；firmware/board
environment 拥有 rate/mux、syscon/RGMII/PHY handoff；per-node provider 拥有 admission 后的 MAC/DMA
current truth。
**违反表现：** GMAC 直接 RMW controller；保存 `clock_enabled`/`reset_done` 第二份 truth；probe failure
关闭已启用 clock或重放已完成 reset；一个 node 的失败补偿另一个 node；没有 PHY/link 证据却发布 `Up`。
**Proof：** source audit确认所有 clock/reset 操作只走 generic providers，failure path 无 rollback/补偿；
provider owner-local tests、per-node probe isolation 与最终真实收发证明 handoff 足以支持 target。

### JH-GMAC-003 — IRQ selector 只选择资源，不解释 controller

**分类：** Correctness Invariant
**规则：** driver 以 name `macirq` 请求单项中断。FwNode/IRQ resource owner 将 name 唯一映射为
index，按 interrupt parent 的 `#interrupt-cells` 截取一个完整 specifier，再交 irqchip `xlate()`；
driver 不解析 PLIC cell，irqchip 不解析 `interrupt-names`。
**Owner：** FwNode/IRQ resource layer 拥有 selector resolution；irqchip 拥有 electrical/controller
translation；IRQ core 拥有 descriptor/dispatch；driver 拥有 device-side cause。
**违反表现：** 整个三项 `interrupts` 串进入 PLIC xlate；driver 硬编码 IRQ 7/78；name 不存在时
fallback index 0；mapping 失败后 source 已 unmask。
**Proof：** name/index/missing/duplicate/misaligned/out-of-range fixture tests、request caller audit 与
板级 `macirq` dispatch。

### JH-GMAC-004 — Device cause 在 controller completion 前撤销

**分类：** Correctness Invariant
**规则：** handler 必须读取并确认本 node 的 MAC/DMA cause，清除或以设备协议认可方式撤销 cause，
提交 durable recheck predicate，再发 wake edge。handler 不推进 Stack、不持有 frame slice，也不以
空 handler 占位；controller eoi/complete 仍由 `IRQ-FLOW-001` 执行。
**Owner：** per-node provider 拥有 device cause/pending predicate；IRQ core/irqchip 拥有 controller
flow。
**违反表现：** eoi 后才清 cause；edge 丢失后没有 durable predicate；handler 直接调用 socket/Stack；
两个 GMAC 共用 pending bit。
**Proof：** register trace/source audit、owner-local cause tests 与板级持续中断/收发。

### JH-GMAC-005 — DMA backing 始终只有一个访问 owner

**分类：** Correctness Invariant
**规则：** descriptor 与 frame backing 在任一时刻只能处于 CPU-owned、device-owned 或 terminal-
retained 中的一种；ownership transfer 必须有单一 publication/completion point。CPU 不访问
device-owned mutable bytes，device 不取得尚未完成 CPU publication 的 descriptor/backing。
**Owner：** per-node provider 的 ring protocol；frame callback只在 CPU-owned窗口取得临时能力。
**违反表现：** submit 后继续修改 TX；completion 前读取 RX payload；同一 slot 同时在 free/posted
队列；error cleanup 释放 device-owned backing。
**Proof：** typed/explicit slot transitions、assertions、wrap/exhaustion/completion tests 与板级压力流量。

### JH-GMAC-006 — Coherent DMA visibility 与 ordering fence 各自成立

**分类：** Correctness Invariant
**规则：** JH7110 的硬件 coherency 负责 CPU/device 对 descriptor 和 frame backing 的 visibility；这不
替代 ordering fence、MMIO doorbell/completion ordering、ownership handoff 或 quiesce。CPU 向 device
移交前必须完成 descriptor/frame publication 的 ordering，device completion 后 CPU 必须以正确顺序
读取状态和 payload；不得用未经证明的 coherency 假设掩盖 ordering 或 ownership 错误。
**Owner：** architecture DMA capability 拥有 coherency 与 ordering primitive；per-node ring protocol
拥有调用位置、方向、ownership 与设备寄存器顺序。
**违反表现：** doorbell 先于 descriptor visibility；completion 读取早于必要的 CPU acquire；把硬件
coherency 当成 ownership/quiesce 证明；或在没有 coherency 证据时把 fence-only 路径写成硬件保证。
**Proof：** production sync/order call-site audit、source/assembly audit、descriptor/frame layout 与
DMA address-width tests，以及最终板级重复 RX/TX。硬件 coherency 证据与 Anemone ordering/ownership
证据必须分别可追溯。

### JH-GMAC-007 — Descriptor/frame hardware alignment 与 layout

**分类：** Correctness Invariant
**规则：** descriptor ring base、descriptor stride、frame backing、DMA address 字段和硬件访问范围必须
满足 DWMAC/JH7110 的对齐、长度和表示约束；40-bit DMA address 不能被截断，ring/descriptor layout
不得产生设备未定义的跨界访问。coherent target 不要求为了 cache-line isolation 添加额外 padding；
需要的对齐常量必须来自 architecture/device truth，而不是猜测。
**Owner：** DMA allocation/layout owner。
**违反表现：** ring 或 descriptor 未按硬件要求对齐；地址高位丢失；descriptor/frame 长度或 stride
超出硬件表示；通过 cache-line 假设掩盖 layout 错误。
**Proof：** compile-time/runtime layout assertions、40-bit address and boundary fixtures、descriptor
format audit 与 architecture/device register audit。

### JH-GMAC-008 — DMA address 必须可表示且 backing 生命周期覆盖 device

**分类：** Correctness Invariant
**规则：** 每个 descriptor/buffer 的 DMA address 必须来自 DMA-capable backing，并可由硬件地址字段
完整表示；转换失败在 device ownership 前 fail closed。IRQ 或 DMA engine 仍可能访问时，backing、
descriptor 和 IRQ private context 必须保留到 quiesce proof 或 reset/power-off。
**Owner：** DMA allocator/provider 拥有 backing；device protocol拥有 quiesce fact。
**违反表现：** truncate physical address；把普通虚拟地址写入 descriptor；shutdown/attach failure 后
Drop 仍可 DMA 的 region；IRQ context 弱引用失效后硬件仍 unmasked。
**Proof：** address-width tests、failure injection、lifetime source audit 与板级 shutdown markers。

### JH-GMAC-009 — 每个 node 的 progression 与 failure 相互隔离

**分类：** Correctness Invariant
**规则：** 每个 provider 只拥有本 node 的 registers、rings、pending predicate、wake edge 与 worker。
一个 node 的 IRQ、queue exhaustion、attach failure 或 shutdown attempt 不得读取、推进、清空或重编号
另一 node。global Stack 只通过每个 mapping 的 narrow pump port 串行化 protocol progression。
**Owner：** per-node provider 与 global DomainStack 各守自己的状态。
**违反表现：** static current-device pointer；共享 completion queue；GMAC0 IRQ 唤醒 GMAC1 worker；一个
attach rollback 撤销另一 mapping。
**Proof：** two-provider host matrix、wrong-provider/failure injection 与板级分别选择两个端口收发。

### JH-GMAC-010 — `eth<N>` 在 probe admission 单调消费

**分类：** Target Guarantee / Capability
**规则：** matching node 进入 probe 后，在可能失败的 MAC/resource/DMA construction 前从
`LogicalInterfaces` 取得 reservation。DT discovery order 决定 ordinal；commit 发布 membership，abort
不发布但永久消费 ordinal。本 RFC 范围内 identity 不复用。
**Owner：** initial-domain logical owner；driver/provider只携带不可复制 token。
**违反表现：** 按成功 attach 排序；GMAC0 失败后 GMAC1 变成 `eth0`；driver另存 path-to-name map；
abort 后重新分配相同 name。
**Proof：** multi-node fail-first/fail-middle tests、token Drop assertion 与至少两次板级冷启动日志。

本 RFC 同时要求现有 boot-time VirtIO publisher 迁移到相同的 probe-admission reservation handoff。
否则含有两种 provider 的系统会有两套 ordinal 线性化点；QEMU 中仍只有一个可见 `eth0`，但该
内部双语义不能作为长期实现保留。

### JH-GMAC-011 — Publication 与 active attach 各只有一个提交点

**分类：** Correctness Invariant
**规则：** queue、RX refill、IRQ、DMA/order 和 wake 全部 ready 后才 publish netdev。Stack mapping、
inactive worker、time/wake wiring 全部 ready 后，attach authority 才 commit logical reservation、记录
active path 并 activate。任何失败不得留下半 published member/mapping。
**Owner：** `device/net` 拥有 netdev publication；attach authority 拥有 active publication。
**违反表现：** Gate 0/1 probe 发布 netdev；publication 后才分配 RX；worker active 时 logical member
尚未 commit；rollback 留下 mapping。
**Proof：** failure injection、publication/attach source audit 与 active-path logs。

### JH-GMAC-012 — 单个 SystemTarget deployment 不产生 fallback

**分类：** Target Guarantee / Capability
**规则：** 所有成功 GMAC 可提交为 L2 external member；`network.ipv4.interface` 只匹配一个 committed
logical name，address/prefix/default gateway 只投影到该 mapping。其它接口保持无 L3 配置。目标接口
缺失或不匹配时 fail closed，不选择下一个 GMAC。
**Owner：** `Ipv4ControlPlane`。
**违反表现：** 给所有 GMAC 复制同一地址；多个 default route；eth0失败后自动把配置投给eth1；driver
直接配置 IP。
**Proof：** selection tests 与板级分别选择 `eth0`/`eth1` 的启动矩阵。

### JH-GMAC-013 — One-time lifecycle 不伪装 runtime recovery

**分类：** Target Guarantee / Capability
**规则：** target 只支持 boot construction、persistent active/published-unattached state 和 terminal
shutdown。link变化不重建 identity，失败不自动 retry，shutdown 后不重新启动 queue/worker。未具备
`free_irq`/device quiesce proof 时保留 provider backing 到 power-off。
**Owner：** provider、attach authority 与 System Power 各拥有自己的 terminal fact。
**违反表现：** cable replug触发重新 publish；后台 retry 改变 ordinal；shutdown timer/IRQ重新激活 worker；
用资源泄漏掩盖仍在运行的 DMA。
**Proof：** lifecycle source audit、shutdown injection 与板级 orderly shutdown。

### JH-GMAC-014 — MAC fact 按 node 解析且不引入隐式 fallback

**分类：** Correctness Invariant
**规则：** 每个 matching node 必须从本节点的 `local-mac-address` 独立得到有效、非零、unicast 的
六字节 MAC fact。R1 不读取 `mac-address`、obsolete `address`、NVM、随机地址或其它 firmware
source；属性缺失或无效时，该 node fail closed，不借用另一 node，也不让逻辑 owner 推导地址。
**Owner：** per-node provider 拥有 publication-time Ethernet fact；firmware/DT 提供 source；logical
owner 只保存 opaque netdev association。
**违反表现：** GMAC0 借用 GMAC1 MAC；缺失属性时静默 random/NVM fallback；MAC snapshot 反向驱动
其它 node 的 queue 或 `eth<N>`；只凭一个节点日志宣称所有节点有效。
**Proof：** Gate 0 source audit、per-node invalid fixture 与最终冷启动日志。

## 状态所有权与生命周期

```text
DT matching node
  -> logical reservation (ordinal consumed)
  -> resource/MAC/IRQ/DMA construction
  -> provider Ready
  -> netdev Published
  -> Stack mapping + inactive worker Prepared
  -> logical membership + active record Committed
  -> worker Active
  -> network stop requested
  -> device IRQ/DMA suppression
  -> retained until reset/power-off
```

任一 publication 前失败沿相反方向撤销已经建立但尚未发布的资源；reservation 只能 abort，ordinal
不能回退。IRQ 注册后的 cleanup 以 device-side suppression 加 retention 取代不真实的资源回收承诺。

每个 RX/TX slot 至少表达以下 owner 转换；具体类型名称不是规范：

```text
RX: CPU-empty -> descriptor/order prepared -> Device-posted -> completion observed
    -> CPU acquire/order observed -> CPU-frame window -> CPU-empty

TX: CPU-free -> CPU-frame window -> descriptor/order prepared -> Device-submitted
    -> completion observed -> CPU-free
```

descriptor completion 字段本身也由 device 写回，CPU 必须在正确的 acquire/order 窗口读取。error/shutdown
只有在 device quiesce 已证明时才能把 device-owned slot 转回可释放状态；否则进入 terminal-retained。

## RFC-local Proof Obligations

- IRQ selector 必须以 synthetic DT 同时证明 variable `#interrupt-cells`、多个 specifier 与 name/index
  一致解析；不能只测 JH7110 的单 cell PLIC。
- DMA tests 必须经过 production sync/order call sites；只测试独立 helper 不证明 descriptor publication、
  completion acquire 与 ring 调用顺序。硬件 coherency 作为板级前提单独记录，不能由 host fake 代替。
- provider tests 必须至少包含两个节点、两个 IRQ context、两个 wake predicate 和两个 ring set，并
  注入第一个/中间节点失败。
- final acceptance 必须在同一实现上完成，不允许为板测加入会绕过 owner、ordering 或 coherency proof 的
  test-only production branch。
- Gate 间临时“匹配但不 publication”状态只服务本 RFC 实施，Gate 3 必须删除返回占位错误或 dormant
  activation switch；否则不得进入板级验收。
- QEMU/host 证据只证明既有路径 regression 或纯语义，不得写成 GMAC hardware evidence。

## 禁止退化项

- 为 GMAC 增加全局 current instance、共享 rings 或第二份 logical identity map。
- 让 driver 读取/修改 raw Stack、route、socket 或其它 provider state。
- 在没有硬件 coherency 证据时用 volatile access、compiler fence 或 CPU memory fence 冒充 coherency；
  或以 coherency 假设替代 ordering、ownership 和 quiesce proof。
- 用硬编码 IRQ/MMIO/MAC、成功 attach 顺序或 U-Boot 的 `eth0/eth1` 名称代替 DT/resource owner。
- 将 GMAC0 MAC 缺失、PHY handoff失败或双口不能收发改写为 accepted limitation；这些都在 target 内，
  必须失败并回到 review。
- 在任何 Gate 检查后提前更新 current contract、声称硬件通过或把 RFC 标为 Closed。
