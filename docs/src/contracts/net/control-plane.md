# Network IPv4 Control Plane 当前契约

**Contract ID：** `NET-CONTROL-PLANE-001`
**状态：** Active
**Owner：** initial-domain `Ipv4ControlPlane`
**参与领域：** SystemTarget / kernel attach authority / logical-interface owner / domain Stack / external与local pump worker
**覆盖范围：** boot-time static IPv4 publication、local/connected/default route precedence、source/interface selection、Stack projection与owner-driven protocol progression handoff
**不覆盖：** Socket/Endpoint/UAPI、runtime address/route change、runtime detach/reuse、多个network domain或完整teardown
**实现位置：** `anemone-kernel/src/net/{mod.rs,domain/control_plane.rs,domain/stack,worker}`、`anemone-kernel/crates/anemone-smoltcp-stack/src/{stack,pump}`
**依赖：** [STM-TARGET-001](../configuration/system-target.md#stm-target-001--systemtarget-是-bootdeploy-contract)、[NET-IFACE-DOMAIN-001](./interface-domain.md#net-iface-domain-001--initial-domain拥有logical-interface-namespace)、[NET-STACK-PUMP-001](./frame-path.md#net-stack-pump-001--stack-instance唯一推进protocol-state)
**Pending Successor：** None；UDP consumer协议见Active [Network UDP Socket](./udp-socket.md)
**最后核验：** 2026-08-14；`TCP-LISTENER-INGRESS-CUTOVER` Effective

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| static external IPv4 deployment input | SystemTarget | resolved/generated immutable projection | 选择配置目标，不证明runtime topology |
| local address set、route precedence与source/interface policy | `Ipv4ControlPlane` | immutable selection result | operation-local route与source选择 |
| logical membership、ifindex/name/kind | `LogicalInterfaces` | immutable boot snapshot | 匹配配置interface，不拥有route |
| protocol mapping、address/default-route/AnyIP与listener-ingress projection、raw Stack | `DomainStack` | fixed pump/operation capability | 执行active selection或解析boot-static listener path |
| local packet handoff capacity与packet access | bounded local software link | fixed local pump port | protocol egress到后续normal ingress |
| protocol progression obligation | 对应Stack-side UDP、ICMP raw或TCP owner | move-only `ProtocolProgression` | 在mutation commit后标识affected interface，不携带work truth |
| worker admission、deadline与recheck projection | 对应`PumpControl` / worker | attach-private narrow request capability | 请求有限重查，不表示protocol work truth |

SystemTarget、logical registry、control plane、Stack和worker各自只有一份本层truth。generated Rust input、stable
mapping snapshot、wake edge和smoltcp route均不得反向成为第二个policy owner。

## NET-CONTROL-PLANE-001 — initial domain唯一决定IPv4 route/source/interface

**规则：** Initial domain在boot attach drain后一次性建立并发布IPv4 control plane。它无条件包含
`127.0.0.1/8` local interface；若selected SystemTarget声明一个static external IPv4 deployment，则配置的logical
interface必须恰好匹配一个已经committed的external member。control plane保存该member的immutable boot-lifetime
logical/protocol association；它不保存或选择protocol mutation wake policy。R0没有runtime detach或identity reuse，
因此该snapshot不允许stale。raw mapping仍只由`DomainStack`拥有。

每次selection按以下顺序作一次pure owner-local决定：配置的本地external address、整个`127/8` local destination、
external connected prefix、显式default route。前两类选择bounded local protocol port；self-external的默认source是
该external address，其它`127/8` destination默认source是`127.0.0.1`。connected/default route选择配置的external
protocol mapping与address。explicit source必须是当前domain允许的local source；external route只接受配置的external
address。缺少route或source时返回typed failure，不搜索其它NIC、不fallback，也不修改binding/address truth。

TCP `listen()`不是active selection operation，不伪造destination执行route/source/interface lookup。non-wildcard binding
仍先使用control-plane local-address set验证；随后`DomainStack`从已经committed的boot-static protocol topology解析适用
listener path：local必然存在，wildcard增加已配置external projection，loopback-specific只保留local，configured
external specific增加与该address对应的external projection。该projection view不建立第二套路由或地址policy，kernel
Socket也不提交listener interface。

`DomainStack`只安装control plane要求的CIDR、default route和local AnyIP projection并执行显式selection；它不遍历
private interface建立active route policy；listener path解析只消费它自己拥有的boot-static interface/protocol mapping。
AnyIP只允许已经选择到local port的packet经normal protocol ingress交付，不能让external ingress绕入local path。
UDP、ICMP raw或TCP owner在pump外提交真实protocol mutation时，必须在同一owner
transaction中产生move-only、`must_use`的`ProtocolProgression`；kernel attach composition只从其中取得affected
interface并解析既有local/external worker association，在attach guard外提交可合并request。control plane与Socket
caller均不参与该wake决策；capacity、work、route、deadline和lifecycle仍由各自owner重新判断。

**线性化与失败：** attach authority先完成logical membership与protocol mapping，再在同一boot activation中验证
配置interface、安装完整Stack projection、发布control plane，最后开启local worker pump admission。missing、duplicate
或mismatched interface在publication前fail closed并停止boot；不得发布partial control plane或alternate selector。
shutdown先关闭global attach admission并withdraw control-plane publication，再在锁外请求local和external worker
stop。queued wake/deadline不能恢复admission；DomainStack、local link与provider backing保持boot-persistent，runtime
join/reclamation不在本规则内。

**进展与有界性：** protocol mutation先在Stack guard内commit，再在guard外消费其progression obligation；request
delivery完成后operation可以返回，但不等待实际pump。local packet从protocol TX token commit到normal RX consume只由bounded local link持有；egress
transfer发生在当前protocol round之后时，必须请求后续有限round，不能让sleeping worker遗留已发布ingress，也不能以
unbounded repoll或busy-poll补偿。每轮复用global pump budget，达到repoll上限后yield；真正idle时睡眠，deadline与
explicit wake只触发predicate重查。

**违反表现：** control plane与Stack各自决定active route；listen伪造destination或由Socket选择单一listener interface；
Platform/rootfs/Preset复制network deployment；配置错配时自动选择另一NIC；self-external进入external provider；
socket-to-socket copy、direct packet injection或第二Stack代替
normal local ingress；selection或caller保存worker wake capability并按operation猜测protocol work；wake bit成为
capacity/work truth；mutation commit后无durable progression；shutdown后的late request恢复pump admission。

**验证 / Enforcement：** xtask tests覆盖closed schema、preset/tuple同一resolved target、generated projection和
clean provenance；host topology覆盖`127/8`、self-external、connected/default projection、bounded recovery与
production budget transfer后的later-round requirement；kernel KUnit覆盖route/source matrix、one-time publication、
missing/duplicate interface、park/in-flight/coalesced/late-stop request和真实DomainStack/local worker三类local
delivery。Stage 1 source/host proof确认`PumpWake`和caller `request_pump()`已经删除，UDP与ICMP raw都在Stack owner
commit后移交progression。RV64/LA64 focused运行均实际经过loopback/self-external local path，并以external UDP
guest/peer双marker证明external selection、provider ingress/egress与normal demux；两架构UDP `16/16`、ICMP raw
`10/10`、whitelist LTP `6/6`通过。RV64 orderly shutdown与wrapper exit 0；LA64完成
`filesystem -> network -> device -> PowerOff`顺序后因当前缺少实际power-off handler停在halt，wrapper由人工终止，
不能作为exit-0证据。TCP listener-ingress closure进一步以owner host proof及RV64/LA64双libc runtime证明boot-static
local/external projection没有改变active connect selection、self-external local delivery或provider ingress boundary。
hardware、`smp>1`、其它deployment与runtime reconfiguration仍Not Run。

**最初来源：** [Network UDP RFC R0](../../rfcs/net-udp/index.md)。

**当前来源：** [Network UDP transaction](../../devlog/transactions/2026-07-29-net-udp.md)的
`NET-UDP-CONTROL-CUTOVER`，以及[IPv4 TCP RFC Stage 1 closure](../../rfcs/net-tcp/implementation.md#639-stage-1-execution-result--closed)的
`NET-PROTOCOL-PROGRESSION-CUTOVER`；[TCP listener ingress publication RFC R0](../../rfcs/tcp-listener-ingress-publication/index.md)
的`TCP-LISTENER-INGRESS-CUTOVER`随后把listener publication从active selection中分离。

**当前 consumer closure：** `NET-UDP-FINAL-CUTOVER`使
[`NET-PROTOCOL-BOUNDARY-001`与`NET-SOCKET-WAIT-001`](./protocol-socket.md)以及
[`NET-SOCKET-ENDPOINT-001`与`NET-UDP-TRANSACTION-001`](./udp-socket.md)生效；`ICMP-RAW-CUTOVER`随后让
[ICMP raw Endpoint/transaction](./icmp-raw-socket.md)成为第二个真实protocol consumer。route/source/interface policy仍由
本页owner唯一拥有。
