# Network Attach Lifecycle 当前契约

**Contract ID：** `NET-ATTACH-001`
**状态：** Active
**Owner：** `anemone-kernel::net` attach transaction、active publication与network-local shutdown admission
**参与领域：** `device/net` / concrete stack / network worker / System Power / concrete NIC driver
**覆盖范围：** published netdev attach、failure rollback、active-path publication与orderly shutdown handoff
**不覆盖：** global terminal episode、driver-local reset/quiesce完整性、runtime detach/retry/restart或全部resource reclamation
**实现位置：** `anemone-kernel/src/{net,power.rs,device/mod.rs,driver/net}`
**依赖：** [NETDEV-LIFE-001](./netdev-lifecycle.md#netdev-life-001--boot-time-identity与publication是单向transaction)、[NET-FRAME-OWN-001](./frame-path.md#net-frame-own-001--frame-backing只有一个访问owner)、[NET-FRAME-PROGRESS-001](./frame-path.md#net-frame-progress-001--有界资源normal-backpressure与durable-recheck)、[NET-STACK-PUMP-001](./frame-path.md#net-stack-pump-001--stack-instance唯一推进protocol-state)、[SYSTEM-POWER-ORDERLY-001](../power/shutdown-lifecycle.md#system-power-orderly-001)、[SYSTEM-POWER-EMERGENCY-001](../power/shutdown-lifecycle.md#system-power-emergency-001)
**Pending Successor：** None
**最后核验：** 2026-07-27

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| published/unattached netdev | `device/net` | one-shot published capability | attach transaction input |
| stack mapping与`InterfaceId` | concrete stack instance | opaque ID | protocol interface identity |
| active-path records与shutdown admission | kernel attach authority | narrow `PumpControl` capability | publication、pump admission与terminal stop |
| global terminal episode与participant order | System Power | network facade invocation | `filesystem -> network -> device`顺序 |
| queue/IRQ/DMA shutdown attempt | concrete driver | device-local capability | network facade之后抑制hardware activity |

以上是不同owner fact的合法组合，不建立综合`NetdevLifecycle` cache。per-path active bit只是attach-authority
terminal gate的capability-local projection，不能重新打开shutdown admission。

## NET-ATTACH-001 — attach publication、rollback与best-effort shutdown

**规则：** kernel-side唯一attach authority逐个取得published netdev，建立stack-local mapping并准备worker、
wake与time wiring；全部成功后才在同一authority下发布active path并activate。publication前的mapping只属于
transaction-local fact。失败撤销该stack mapping并保持netdev published/unattached；一个netdev失败不回滚或
阻塞其它active path，不自动retry。

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

**违反表现：** active path在mapping/worker/wake/time准备前可见；失败注销netdev或污染另一path；kernel与stack
各缓存mapping；shutdown后重新activate/pump/arm timer；normal worker exit Drop未quiesce backing；driver反向
访问stack；network facade等待或持raw queue lock。

**验证 / Enforcement：** host双实例mapping rollback/failure isolation、registry/attach source audit；
Checkpoint 2 RV64 traffic/order证据证明active attach、bounded worker与network-before-device；Checkpoint 3
final exact-code boot在删除probe后通过260/260 remaining KUnit、active attach、network summary、严格
`filesystem -> network -> device -> PowerOff`与正常QEMU退出。source review覆盖owner-lock linearization、
stop/timer/Weak/retention与emergency bypass。

**最初来源：** [Network Frame Path RFC R1](../../rfcs/net-frame-path/index.md)。

**当前来源：** [Network Frame Path transaction](../../devlog/transactions/2026-07-26-net-frame-path.md)的
`NFP-FINAL-CUTOVER`。

## 跨领域局部义务

| Obligation | 参与方 | 必须完成的动作 | Handoff / 线性化点 | 失败 / Cleanup责任 |
| --- | --- | --- | --- | --- |
| attach publication | `device/net` / stack / attach authority | 移交published capability；建立mapping/wiring；最后publish active | authority lock下active record publication | 失败只撤销transaction-local mapping；provider不安全时retain |
| orderly network step | System Power / attach authority | `device`前调用唯一network facade；关闭admission并请求stop | `shutdown_started`在authority lock下首次置位 | callback返回后power fail-forward；不等待worker |
| terminal provider lifetime | worker / driver | worker停止新pump并retain core；driver随后抑制IRQ/做local attempt | worker观察stop；driver callback顺序在network之后 | 未证明quiesce时retain到reset/power-off |
| emergency | System Power | 跳过network/filesystem/device ordinary callback | episode切到emergency后直达machine helper | network不建立panic-safe cleanup path |

## 当前接受边界

- 不支持runtime hotplug/detach/retry/restart、完整teardown或shutdown reclamation/progress guarantee。
- shutdown summary证明admission/stop request与retention，不证明worker join、queue reset或resource释放。
- RV64 QEMU/smp=1是唯一production runtime acceptance；LA64、virtio-pci、hardware与`smp>1`均Not Run。
