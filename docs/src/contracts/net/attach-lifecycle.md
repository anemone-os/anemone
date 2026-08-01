# Network Attach Lifecycle 当前契约

**Contract ID：** `NET-ATTACH-001`
**状态：** Active
**Owner：** `anemone-kernel::net` attach transaction、active publication与network-local shutdown admission
**参与领域：** `device/net` / initial domain / domain Stack / network worker / System Power / concrete NIC driver
**覆盖范围：** published netdev到initial domain/global Stack的attach、failure rollback、active-path publication与orderly shutdown handoff
**不覆盖：** global terminal episode、driver-local reset/quiesce完整性、runtime detach/retry/restart或全部resource reclamation
**实现位置：** `anemone-kernel/src/{net,power.rs,device/mod.rs,driver/net}`
**依赖：** [NETDEV-LIFE-001](./netdev-lifecycle.md#netdev-life-001--boot-time-identity与publication是单向transaction)、[NET-IFACE-DOMAIN-001](./interface-domain.md#net-iface-domain-001--initial-domain拥有logical-interface-namespace)、[NET-FRAME-OWN-001](./frame-path.md#net-frame-own-001--frame-backing只有一个访问owner)、[NET-FRAME-PROGRESS-001](./frame-path.md#net-frame-progress-001--有界资源normal-backpressure与durable-recheck)、[NET-STACK-PUMP-001](./frame-path.md#net-stack-pump-001--stack-instance唯一推进protocol-state)、[SYSTEM-POWER-ORDERLY-001](../power/shutdown-lifecycle.md#system-power-orderly-001)、[SYSTEM-POWER-EMERGENCY-001](../power/shutdown-lifecycle.md#system-power-emergency-001)
**Pending Successor：** None
**最后核验：** 2026-07-31

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| published/unattached netdev | `device/net` | one-shot published capability | attach transaction input |
| unpublished logical reservation | initial-domain logical-interface owner | transaction token | publication前identity/ifindex/name reservation |
| global-Stack mapping与`InterfaceId` | domain Stack | narrow pump port / transaction-local mapping owner | protocol interface identity与bounded progression |
| active-path records与shutdown admission | kernel attach authority | narrow `PumpControl` capability | publication、pump admission与terminal stop |
| global terminal episode与participant order | System Power | network facade invocation | `filesystem -> network -> device`顺序 |
| queue/IRQ/DMA shutdown attempt | concrete driver | device-local capability | network facade之后抑制hardware activity |

以上是不同owner fact的合法组合，不建立综合`NetdevLifecycle` cache。per-path active bit只是attach-authority
terminal gate的capability-local projection，不能重新打开shutdown admission。

## NET-ATTACH-001 — attach publication、rollback与best-effort shutdown

**规则：** kernel-side唯一attach authority逐个取得published netdev。missing Ethernet address在分配logical
reservation或Stack mapping前失败。普通路径先在authority下取得未发布logical reservation，锁外在initial-domain
唯一global Stack建立mapping并准备wake、time wiring与inactive worker，最后在同一authority critical section提交
logical membership、active-path record并activate。publication前的reservation与mapping都只属于本次transaction。

worker只持自己的concrete provider与`ExternalPumpPort`；port只能在一次finite pump window内取得global Stack推进
能力，repoll round之间释放该能力。provider callback不得sleep或反向进入domain/attach owner。raw Stack、其它
interface mutation、route、Endpoint或provider lifecycle不通过port暴露。

普通worker spawn失败先撤销global-Stack mapping，再abort未发布logical reservation，并把同一provider capability
交回`device/net`保持published/unattached；一个netdev失败不回滚`lo`或其它active path，不自动retry。若terminal
shutdown在prepare后、publication前关闭admission，依同样顺序撤销mapping与reservation，再请求inactive worker
stop并retain provider到reset/power-off；该terminal retention不重新进入ordinary pending retry。

同一authority lock还拥有唯一`shutdown_started` admission truth。orderly shutdown在lock内先发布terminal
fact并snapshot窄`PumpControl`，随后锁外逐path关闭active、清explicit work并只发一次non-waiting kthread
stop/wake；不wait/join/timeout，不持owner lock进入worker/provider/driver callback，也不取得raw queue lock。
worker在每个bounded pump round间重查stop，观察后不再repoll或arm deadline。已排队timer只持stateless wake，
不能重新activate。

IRQ当前不可移除且device没有reset/quiesce proof；terminal worker退出前必须保留唯一`PumpCore`及其
provider/slot/DMA owner到reset/power-off。只有未来runtime removal先阻止IRQ Weak upgrade并证明queue/device
quiesce后才可Drop。driver shutdown只做owner-local IRQ/queue/device attempt，不反向协调stack/worker。

**失败边界：** cleanup不保证callback有界返回、全部访问退出或全部resource回收；不因此建立parallel lifecycle
truth、通用drain/refcount/timeout/teardown framework。预期platform NIC未进入active path仍是runtime acceptance
failure，不能由其它owner绕过authority。

**违反表现：** active path在reservation/mapping/worker/wake/time准备前可见；失败注销netdev、留下logical member/
Stack mapping或污染另一path；worker持raw Stack；authority与Stack lock反序；shutdown后重新activate/pump/arm
timer；normal worker exit Drop未quiesce backing；driver反向访问stack；network facade等待或持raw queue lock。

**验证 / Enforcement：** one-Stack/two-provider host matrix覆盖双interface progression、blocked-provider isolation、
wrong-provider isolation与mapping rollback；domain/registry KUnit覆盖reservation/commit/abort、identity no-reuse及
failure isolation。Stage 5 final RV64/LA64 fresh-disk运行均在真实VirtIO active attach后通过274/274 KUnit，并保持
严格`filesystem -> network -> device -> PowerOff` markers；RV64自然退出，LA64因当前无电源驱动在orderly halt后
由launcher通过QEMU monitor `quit`收尾。source review覆盖identity separation、raw-Stack containment、authority/
Stack lock order、rollback、finite round、stop/timer/Weak/retention与emergency bypass。

**最初来源：** [Network Frame Path RFC R1](../../rfcs/net-frame-path/index.md)。

**当前来源：** [Network UDP transaction](../../devlog/transactions/2026-07-29-net-udp.md)的
`NET-UDP-DOMAIN-CUTOVER`。

## 跨领域局部义务

| Obligation | 参与方 | 必须完成的动作 | Handoff / 线性化点 | 失败 / Cleanup责任 |
| --- | --- | --- | --- | --- |
| attach publication | `device/net` / initial domain / Stack / attach authority | 移交published capability；reserve logical identity；建立mapping/wiring；最后publish active | authority lock下logical member与active record publication | 失败依mapping -> reservation顺序撤销；ordinary返回provider，terminal retain |
| orderly network step | System Power / attach authority | `device`前调用唯一network facade；关闭admission并请求stop | `shutdown_started`在authority lock下首次置位 | callback返回后power fail-forward；不等待worker |
| terminal provider lifetime | worker / driver | worker停止新pump并retain core；driver随后抑制IRQ/做local attempt | worker观察stop；driver callback顺序在network之后 | 未证明quiesce时retain到reset/power-off |
| emergency | System Power | 跳过network/filesystem/device ordinary callback | episode切到emergency后直达machine helper | network不建立panic-safe cleanup path |

## 当前接受边界

- 不支持runtime hotplug/detach/retry/restart、完整teardown或shutdown reclamation/progress guarantee。
- shutdown summary证明admission/stop request与retention，不证明worker join、queue reset或resource释放。
- production runtime acceptance覆盖RV64 QEMU/virtio-mmio与LA64 QEMU/virtio-pci的`smp=1`单NIC路径。hardware、
  其它NIC/deployment与`smp>1`均Not Run。
