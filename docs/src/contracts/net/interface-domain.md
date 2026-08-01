# Network Interface Domain 当前契约

**Contract ID：** `NET-IFACE-DOMAIN-001`
**状态：** Active
**Owner：** initial-domain logical-interface owner与domain Stack各自拥有的membership/protocol state
**参与领域：** `device/net` / kernel attach authority / domain Stack / IPv4 control plane
**覆盖范围：** initial-domain boot identity、logical-interface membership/identity/ifindex/name/kind、external reservation与global-Stack composition
**不覆盖：** IP address/route/source selection与local packet progression（由`NET-CONTROL-PLANE-001`拥有）、Endpoint/socket/UAPI、runtime detach/retry/reuse或多个network domain
**实现位置：** `anemone-kernel/src/net/{domain/mod.rs,domain/interfaces.rs,domain/stack.rs,mod.rs}`
**依赖：** [NETDEV-LIFE-001](./netdev-lifecycle.md#netdev-life-001--boot-time-identity与publication是单向transaction)、[NET-BOUNDARY-001](./frame-path.md#net-boundary-001--frame-slice依赖方向与object-fence)、[NET-STACK-PUMP-001](./frame-path.md#net-stack-pump-001--stack-instance唯一推进protocol-state)
**Pending Successor：** None；[NET-CONTROL-PLANE-001](./control-plane.md#net-control-plane-001--initial-domain唯一决定ipv4-routesourceinterface)与[UDP Socket contract](./udp-socket.md)均已接入本页logical/domain边界
**最后核验：** 2026-07-31

## 状态与身份所有权

| 状态 / 身份 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| initial-domain composition | kernel `InitialDomain` | boot-persistent owner capability | 组合但不混淆logical registry与唯一domain Stack |
| logical membership、identity、ifindex/name/kind | `LogicalInterfaces` | immutable snapshot / unpublished reservation | domain-local interface namespace |
| netdev identity、origin与publication facts | `device/net` | opaque association | external admission输入与诊断 |
| protocol `InterfaceId` mapping与raw Stack | `DomainStack`内唯一Stack instance | transaction-local mapping owner / narrow pump port | protocol progression |
| queue、DMA、IRQ与current resource/link truth | concrete provider | callback-scoped frame capability / recheck edge | external frame progression |

这些身份域互不等价、不可互换，也不存在从一个ID到另一个ID的common lookup。external logical snapshot可以保存
opaque `NetdevId`关联供诊断，但不得以它反推logical/protocol identity或provider resource truth。

## NET-IFACE-DOMAIN-001 — initial domain拥有logical-interface namespace

**规则：** production在boot attach drain前无条件建立一个persistent initial domain。该domain拥有一个
`LogicalInterfaces`和一个global `DomainStack`，两者保持不同真相源。`LogicalInterfaces`首先提交logical ID 0、
ifindex 1、name `lo`、kind `Loopback`；即使没有external pending capability，该membership也存在。该logical fact
不拥有IP address、route或packet progression；Stage 2由独立control-plane owner和DomainStack local mapping在不
复制membership truth的前提下使它成为functional production path。

external logical interface只能通过reservation -> commit/abort transaction进入membership。logical ID、ifindex和
`eth<N>` ordinal在reservation时单调消费；abort或失败不发布membership，也不复用已经消费的identity。commit是
membership的唯一发布点。runtime detach、retry、identity reuse和多domain lifecycle不在当前契约内。

initial domain还持有production唯一global protocol Stack composition。raw Stack只存在于`DomainStack`内；external
worker只能取得自己的opaque `ExternalPumpPort`，不能取得logical registry、其它mapping、route、Endpoint或raw
Stack mutation authority。该composition不把logical identity、protocol mapping和provider resource合并为综合状态。

**失败 / Cleanup：** missing MAC在reservation前失败。reservation后的ordinary attach failure先撤销未发布
Stack mapping，再abort reservation；terminal admission race还必须在此后停止inactive worker并retain provider。
unfinished mapping进入cleanup时先fail-close撤销未发布mapping，再暴露protocol bug；logical reservation没有已发布
resource可撤销，其identity按no-reuse规则保持已消费。一个失败不影响`lo`或其它committed external member。

**违反表现：** `device/net`或Stack同时保存ifindex/name/membership；从`NetdevId`或`InterfaceId`推导logical identity；
没有external device时不建立`lo`；abort后复用identity；logical registry保存address/route或驱动local packet；
worker取得raw Stack或其它interface mutation capability；失败留下可观察的logical member或Stack mapping。

**验证 / Enforcement：** owner-local KUnit覆盖boot `lo`、external reservation/commit/abort、monotonic no-reuse、
opaque netdev association与failure isolation；registry KUnit独立覆盖netdev identity/facts。one-Stack/two-provider host
matrix覆盖mapping isolation与removed ID fail-closed。source audit确认production `Stack::new()`只在`DomainStack`构造、
worker只持`ExternalPumpPort`、device/logical/protocol identity没有转换路径；RV64/LA64 final fresh-disk运行均实际
执行274/274 KUnit，通过一张真实VirtIO NIC的publication、active attach、functional local/remote path与orderly
shutdown markers。

**最初来源：** [Network UDP RFC R0](../../rfcs/net-udp/index.md)。

**当前来源：** [Network UDP transaction](../../devlog/transactions/2026-07-29-net-udp.md)的
`NET-UDP-DOMAIN-CUTOVER`。

## 当前接受边界

- `lo`的membership仍只由logical owner定义；其functional IPv4 path现由独立
  [control-plane contract](./control-plane.md)拥有，不把route或packet truth写回logical registry。
- external logical facts服务attach record、boot-time control-plane interface match与immutable association；logical
  owner仍不拥有address/route，且没有用户可见interface query或runtime configuration ABI。
- production runtime分别验证RV64 QEMU的一张virtio-mmio NIC与LA64 QEMU的一张virtio-pci NIC、functional
  local/remote path和`smp=1`。hardware、其它NIC/deployment与`smp>1`均Not Run。
