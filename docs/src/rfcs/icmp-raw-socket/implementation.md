# IPv4 ICMP Raw Socket 实施路线

**状态：** R0 Closed / Checkpoint 1 Closed after Feedback Interlude / Checkpoint 2A Closed / Checkpoint 2B Closed / `ICMP-RAW-CUTOVER` Complete
**最后更新：** 2026-08-03
**父 RFC：** [RFC-20260803-icmp-raw-socket](./index.md)
**目标与不变量：** [目标与不变量](./invariants.md)
**当前修订：** R0
**执行授权：** 三个checkpoint与唯一contract cutover均已按各自授权关闭；不得进入后续gate

本文只长期保存跨 Socket、Network Stack 与 interface/IP owner 的实施顺序，不复制父 RFC 的 target、ABI matrix或
acceptance。R0已实现并关闭；Checkpoint 1 closure后的软件工程审查曾触发Review Hold，反馈间章修复该checkpoint内的
owner与composition偏差并通过独立复核。Checkpoint 2A随后关闭syscall不可达的Socket consumer与shared owner review；
Checkpoint 2B完成ABI publication、mandatory product evidence、final review与唯一cutover。当前停止，不进入后续gate。

本路线只有一个implementation stage、三个checkpoint和一个最终cutover。Checkpoint 1把当前最高风险的
post-admission packet seam及Stack raw owner闭合为syscall不可达的final-shape protocol capability；Checkpoint 2A在继续
拒绝raw tuple的前提下接入final-shape Socket consumer并独立review common front、wait与lifecycle；Checkpoint 2B才发布
Linux Socket ABI、完成全部产品证据并原子执行`ICMP-RAW-CUTOVER`。当前没有独立probe、transitional contract或transaction
devlog；Git/PR默认拥有执行证据，只有实际出现长期、多轮probe/renegotiation或多个独立cutover时才重新分类。

## Live Source Baseline 与路线选择

当前live source已经足以直接进入final-shape实现，不需要先建立探索性production probe：

- vendored smoltcp在`iface/interface/ipv4.rs`中先调用`raw_socket_filter()`，之后才执行local-destination admission；该顺序
  不能交付本RFC的raw packet。
- smoltcp `socket/raw.rs`当前从`IpRepr`重新emit RX header，会丢失TOS、ID、flags与IP options；它不能直接充当要求
  original-byte fidelity的RX owner。
- current general Socket front只有mandatory-destination datagram send、无datagram receive flags/packet-length outcome，
  common read/write仍形成stream-shaped request。ICMP raw应收敛真实共同义务，不能旁路成第二套FileOps或raw-only syscall
  path。
- current UDP consumer已经证明opaque Endpoint identity、typed request/outcome、point-in-time facts、invalidation hint、
  opened-description final release与shared wait/recheck可以保持owner fence；ICMP raw只复用这些协议形状，不复制UDP
  namespace、engine topology或datagram state。

实施采用以下路线：

| Surface | 确定路线 | 保持开放的实现偏好 |
| --- | --- | --- |
| RX ingress | interface/IP owner在IPv4 parse/checksum与local-destination admission后，通过callback-scoped `AdmittedIpv4Packet`交付`total_len`内original datagram与interface-owned destination classification；Stack ICMP raw owner执行ICMP/unicast/unfragmented policy与独立Endpoint fanout，ordinary ICMP继续处理原packet | 已闭合；不延长driver frame lifetime，不复制destination predicate；private raw engine仅用于egress |
| Raw Endpoint | domain Stack raw owner统一拥有monotonic identity、association/filter、bounded RX/TX、fanout/drop、facts、invalidation与retire；kernel只取得syscall不可达的role-scoped operation capability | Checkpoint 2A接入final-shape Socket consumer；若该路线撤销则一并删除temporary capability |
| TX egress | Stack按operation-local selection形成完整无option IPv4 packet并完成Endpoint admission；每interface private raw engine沿normal interface/neighbor/provider path发送，窄header snapshot只补回`Ipv4Repr`遗漏的TOS/ID/flags | per-interface protocol cursor只仲裁新admission，engine-owned packet优先且queue truth仍在各protocol owner |
| Socket front | static descriptor作为唯一semantic type witness；只扩展raw、UDP与Unix真实需要的family-neutral datagram/file-I/O和option dispatch | enum/function family、request/outcome名称与owner-local模块拆分 |

TX mechanism只有同时保持TTL/TOS、selected-interface/source、neighbor/provider normal path、Endpoint attribution与bounded
progression时才可复用。若候选smoltcp mechanism会重新归零可见header policy、隐式重选route/source、把packet成功推迟到
provider完成，或要求kernel取得private handle/queue，则不得以adapter掩盖，应改用更窄的Stack-private emission path或
触发停止条件。

## 实现原则：让真实 Consumer 反馈 General Socket Framework

本RFC的重要目的之一，是让第三个异构Socket consumer检验current general Socket framework是否真正family-neutral。
因此，当ICMP raw暴露的问题属于Socket front/ABI/FileOps orchestration、static dispatch、datagram request/outcome、option
dispatch或shared wait/recheck的自然职责时，优先在该common owner内形成最窄、final-shape修正；不能因为修改common代码
比增加raw-local adapter更显眼，就把结构问题封装在ICMP raw family内部。

判断一项framework反馈是否可以在当前Implementation Boundary内自然闭合，使用以下边界：

- 调整由当前ICMP raw真实operation触发，表达family-neutral semantic dimension或修正现有stream/UDP-biased shape，并且
  保持UDP、Unix与现有File/iomux/epoll行为；它不需要另一个未来consumer证明才允许实施。
- common front只拥有descriptor、dispatch、normalized request/outcome、FileOps orchestration、blocking choice与
  wait/recheck等共同责任；association/filter、packet queue、TTL/TOS、readiness predicate和protocol lifecycle仍留在各自
  family/Stack owner，不能借framework修正上收为generic mutable state。
- 优先调整正确owner中的request、outcome、static ops或orchestration，而不是增加raw-only stream/datagram转换、concrete
  downcast、第二FileOps、旁路syscall/wait loop、duplicated option/readiness state或case-specific errno/copy adapter。
- 调整不得为TCP、任意raw protocol、message ABI、ancillary data、generic option bag或动态family registry预建surface。
  超出父RFC已列`SOCKET-FRONT-001`/`SOCKET-ABI-001` Refine，或改变public API、visibility/shared contract、ABI、owner、
  acceptance时，仍须停止并回到RFC review；“倾向修framework”不是扩大实现授权的豁免。

Checkpoint review必须把raw-local workaround作为明确审查项：若它只是在绕过一个能够由current common owner自然表达的
真实义务，应在checkpoint内移除并修正framework；若common化反而会吸收family truth或建立无consumer抽象，则保持
owner-local并记录选择依据。

## 全局 Implementation Boundary

- **Target / non-goals：** 完整服从父RFC的ICMP-only、IPv4、unicast、unfragmented R0；不得顺带交付任意protocol、
  `IP_HDRINCL`、broadcast/multicast、message ABI、error queue或production Echo responder。
- **Owner / handoff：** Socket ABI/credential owner解析Linux tuple、pointer、flags、errno与`CAP_NET_RAW`；kernel raw family
  拥有TTL/TOS与Linux wait projection；control plane拥有route/source/interface selection；interface/IP owner拥有local
  admission；Stack raw owner拥有Endpoint、association/filter、packet storage/fanout与TX/RX commit；opened-description
  owner唯一触发final release。
- **Failure / cleanup：** create在fd publication前由unpublished guard撤销本次Endpoint/source/file资源；TX在Stack
  admission前失败不留下packet；RX在detach前后各有唯一owner；publication后只有semantic final release撤销source并移交
  non-blocking retire，late identity/hint fail closed。
- **Protected ABI / contract：** 父RFC的tuple、sockaddr、flags、copy/consume、option、readiness与errno policy不得由实施
  路线改写；全部current contract在最终cutover前保持effective，不登记partial或transitional rule。
- **Validation claim：** Checkpoint 1只可声明owner-local/host protocol proof；Checkpoint 2A只可增加kernel Socket、
  family-neutral request/outcome normalization、wait/opened-description与既有consumer的owner-local/build proof，raw guest
  ABI、LTP与ping保持Not Run；
  Checkpoint 2B必须取得父RFC要求的owner-local、双架构focused guest、双架构/双libc curated Socket LTP与双架构真实ping
  证据，证据层不能互相替代。
- **自然闭合：** 同owner import/re-export、module registration、新文件、定向测试、行为保持拆分及Kconfig schema接线可在
  checkpoint内自然完成；由真实raw consumer触发、且保持上述owner/ABI/contract边界的general Socket framework修正也
  属于自然闭合，不因触及common模块单独停顿。路径提示不是穷举write set。
- **全局停止条件：** 出现父RFC停止条件，或需要test-only production facade、第二份association/readiness/route truth、
  generic raw registry/manager、driver/frame lifetime泄漏、无退出条件的兼容桥、降低oracle/architecture claim时，必须在
  checkpoint完成声明前停止并回到RFC review / Target Renegotiation。

预计触及的owner区域包括`anemone-net-api` protocol vocabulary、`anemone-smoltcp-stack` raw owner与pump、vendored smoltcp
local-admission/emission seam、kernel `net` role capability、general Socket front/ABI/raw family、Kconfig与focused validation
assets；这些只是非穷举提示，不冻结内部API或文件布局。

## Checkpoint 1 — Protocol Owner、Post-admission Seam 与 Packet Transaction

**状态：** Closed after Feedback Interlude

**Purpose：** 在不发布Linux Socket ABI的前提下，关闭R0最危险的original-byte ingress、independent fanout、TX admission、
bounded progression与Endpoint lifecycle，使下一checkpoint消费一份已经经过owner-local证明的final-shape capability。

**Prerequisites：** 已满足：父RFC进入Accepted R0，维护者本轮单独授权Checkpoint 1；current frame path、control plane与
UDP contract继续作为effective baseline。

**Protected Boundary：** 不改变用户可见target/non-goals、control-plane/interface/Stack owner划分、frame ownership、
provider backpressure或current Socket/network contracts；不得提前注册`SOCK_RAW` tuple、发布fd或更新current contract。

### Deliverable

1. 建立ICMP raw scope的shared semantic vocabulary：opaque identity、limits、association/filter mutation/query、
   operation-local egress selection/policy、detached RX packet、typed error/facts/invalidation。它不包含Linux UAPI、fd/task、
   waiter、smoltcp handle或runtime registry。
2. 在domain Stack建立唯一raw Endpoint owner，覆盖monotonic/stale-safe identity、create/retire、local/peer association、
   `ICMP_FILTER`、独立per-Endpoint RX/TX packet与byte accounting、drop diagnostics、facts与invalidation。
3. 在local-destination admission之后建立窄RX observation seam。seam只在original datagram借用有效期内调用raw owner；
   raw owner先按R0 unicast/unfragmented、association与ICMP filter匹配，再为每个live Endpoint独立建立或丢弃delivery。
   ordinary ICMP processing必须继续使用原packet，raw full/drop/retire不能改变其结果。
4. 建立TX transaction：caller交付ICMP message、operation-local selection与TTL/TOS policy；Stack验证selected interface/source、
   header/MTU、Endpoint capacity和retire状态，形成无option、非fragment IPv4 packet并在返回success前commit packet owner。
   commit后由existing bounded pump、neighbor与provider路径推进，ordinary external loss不追溯修改send结果。
5. 将RX detach/peek和TX saturation/recovery所需facts接入owner invalidation，但不在本checkpoint建立Linux wait loop或poll
   mask。notification只携带recheck hint，owner guard外发布。
6. 由KernelConfig拥有Endpoint count、per-Endpoint RX/TX packet/byte storage与相关pump limit；拒绝散落magic number、
   unbounded queue和以allocator偶然失败代替capacity contract。
7. 建立kernel `net`侧role-scoped operation capability与observer routing，使后续raw family无需取得global Stack、private
   engine或lock。该surface在Checkpoint 2A前保持syscall不可达，并以Checkpoint 2A final-shape Socket consumer或路线撤销
   作为明确退出条件。

### Validation

- vendored smoltcp focused tests证明observation严格位于local admission之后、保留`total_len`内原始bytes、fragment不进入
  R0 raw path且ordinary ICMP继续处理。
- Stack owner/host tests覆盖multiple matching Endpoint、local/peer/ICMP filter、一个full consumer的drop isolation、
  original header options/TOS/ID/flags fidelity、create/retire/stale identity、RX detach/peek、TX zero-length/header/
  TTL/TOS/MTU、capacity saturation/recovery、bounded pump与normal provider path。
- shared API/dependency/source audit证明没有Linux representation、runtime owner、private smoltcp object、driver frame borrow、
  second destination predicate或test-only production dependency越界。
- 对受影响host/package build与tests以及RV64/LA64 kernel build取得实际结果；本checkpoint不声明guest syscall、任一
  architecture runtime、LTP或ping通过，这些保持Not Run。

### Closure Evidence

- `just test net-host`通过：ICMP raw focused owner/path tests、vendored smoltcp IPv4 interface tests，以及既有frame、bounded
  progression、multi-instance、multi-interface与UDP topology regression全部通过；no-default-features compile/check通过。
- `just test xtask`的74项测试、`just defconfig`、`just fmt kernel --check`与`git diff --check`通过；五个ICMP raw Kconfig
  参数均由generated config生成、静态约束并被kernel policy消费。
- `qemu-virt-rv64-release`与`qemu-virt-la64-release`在最终source上均完成discovery/final pass与symbol table验证。RV64首次
  sandbox尝试在lwext4 C build触发`Bad system call`，相同命令在sandbox外通过，因此该次失败只归类为环境限制。
- source/dependency audit确认shared API不含Linux UAPI、fd/task/waiter、runtime registry、private smoltcp handle或driver
  frame borrow；route/source/interface与local-destination admission仍由既有owner决定；invalidation在Stack commit并释放
  owner guard后发布；production dependency没有test-only facade。
- guest syscall、RV64/LA64 runtime、LTP与BusyBox ping：**Not Run**。这些属于Checkpoint 2B product evidence，不能由
  Checkpoint 1或Checkpoint 2A的host test、KUnit或kernel build替代。

### Post-closure Review Hold 与反馈间章

`f040dfc2`关闭Checkpoint 1后，针对该大commit的软件工程审查发现一项阻止closure的Keter：vendored smoltcp的generic
raw dispatch被改成post-admission且只接收unicast/unfragmented packet，导致interface/IP owner替ICMP raw owner决定R0
policy，并回归了generic raw的pre-admission与fragment reassembly语义。与此同时，ICMP raw private engine同时承担RX
observer与TX、Stack用平行protocol字段和双`Option`表达egress owner、net-api重复定义同构selection、kernel重复维护
observer route table；这些Euclid形状说明此前围绕UDP单一协议形成的composition还没有自然容纳第二个protocol。

本反馈间章执行R0-preserving Route Correction，不修改accepted target、owner划分、ABI、Contract Impact、acceptance或
验证强度：

1. interface/IP owner只提供post-admission original-byte observation及其destination classification snapshot；Stack ICMP
   raw owner重新独占ICMP、unicast、unfragmented、association/filter与fanout policy。generic smoltcp raw恢复原有
   pre-admission及fragment reassembly行为，ICMP raw private engine改为egress-only。
2. `StackPolicy`、`Protocols`、`InterfaceProtocols`、`StackInvalidations`与单一`ActiveEgress`集中静态protocol
   composition、interface attach/detach、ingress drain、egress prepare/complete与invalidation drain；kernel pump/rollback
   回到domain Stack common owner。没有引入动态registry、manager、trait hierarchy或shared readiness truth。
3. net-api用owner-neutral `Ipv4EgressSelection`替代UDP/ICMP raw重复类型；ICMP raw owner按
   `namespace`/`ingress`/`egress`拆分稳定职责；kernel用typed `RecheckRoutes<Id, Observer>`复用reverse-route storage规则，
   但保留各protocol identity与observer truth。
4. focused tests新增wrong destination、broadcast、multicast、non-ICMP、original options/TOS/ID/flags、fragment exclusion、
   ordinary ICMP coexistence与egress-only engine证明，并恢复generic raw receive与fragment reassembly regression。

反馈间章的当前证据为：`just test net-host`全部通过，包括10项ICMP raw tests、33项vendored smoltcp IPv4 tests和既有
frame/bounded/multi-instance/multi-interface/UDP topology回归；`just fmt kernel --check`与`git diff --check`通过；最终源码
的RV64/LA64 release build均完成discovery/final pass与symbol table验证。RV64 sandbox build再次在lwext4 C compile触发
`Bad system call`，完全相同命令在sandbox外通过。两套kernel build编译了新增KUnit case，最终LA64 ELF symbol table可见
该case，但未执行KUnit runtime；guest syscall、architecture runtime、LTP与BusyBox ping仍为**Not Run**。

独立change review确认没有遗留Apollyon、Keter或有证据的Euclid；dead-weak route pruning与当前两protocol的typed
invalidation tuple均判定为Safe。`mdbook build docs`通过，因此Review Hold已经释放，Checkpoint 1在本反馈间章后重新关闭；
当时按gate停止，并未自动进入Checkpoint 2A。

**Cutover：** None。所有current contracts保持不变；Checkpoint 1 closure只说明protocol capability具备进入
Checkpoint 2A Socket consumer review的条件。

**Stop / Exit：** 以下任一事实阻止Checkpoint 1 closure：original bytes只能通过pre-admission delivery、header重建、
frame lifetime延长或destination truth复制取得；fanout需要shared bottleneck决定所有consumer命运；TX需要绕过selected
interface/neighbor/provider path或把route/source truth下推；retire需要等待worker或依赖`Drop`；temporary shared surface无法
给出Checkpoint 2A consumer/撤销条件。Checkpoint 1关闭后必须停止并等待Checkpoint 2A单独授权。

## Checkpoint 2A — Syscall 不可达的 Socket Consumer Integration

**状态：** Closed

**Purpose：** 用Checkpoint 1 capability建立final-shape kernel ICMP raw Socket family，并在不注册raw tuple、不发布用户
可见fd的前提下收敛general Socket第三个异构consumer所需的共同operation、FileOps、wait与lifecycle形状。该独立安全状态
用于在Linux ABI publication前完成shared owner review，不形成current capability或contract cutover。

**Prerequisites：** Checkpoint 1已经关闭且review没有遗留Keter/Apollyon；维护者单独授权Checkpoint 2A；父RFC仍为同一
accepted revision。若Checkpoint 1证据触发target/owner/ABI/acceptance变化，必须先完成RFC review，不能直接进入本段。

**Protected Boundary：** 保持Checkpoint 1 owner/packet transaction、父RFC Linux-visible matrix和mandatory validation
floor；resolver必须继续拒绝`AF_INET + SOCK_RAW + IPPROTO_ICMP`，不得执行`CAP_NET_RAW` admission、用户fd publication或
raw guest ABI claim；不得为方便接入复制Endpoint association/filter/facts、建立raw-only FileOps/wait loop或提前切任何
contract ID。

### Deliverable

1. 建立kernel ICMP raw family：持有role-scoped Endpoint capability、source publication与TTL/TOS policy；提供
   bind/connect/disconnect/query、send/receive、option、facts projection与final-release handoff，不取得Stack-private state。
2. 让general Socket front只增长三个真实consumer需要的最窄surface：optional datagram destination、datagram receive
   `PEEK/TRUNC`与完整packet-length outcome、common read/write到family-neutral send/receive、family option query/mutation
   dispatch。若current common shape无法自然表达这些义务，直接在正确framework owner修正，不在raw family内增加
   translation/downcast/parallel FileOps；同时不得借机建立`sendmsg/recvmsg`、ancillary、generic option bag或future TCP
   surface。
3. 让common front与family之间只交换normalized address、request、outcome、option value与copy cursor；可增加直接调用
   production descriptor/family path的owner-local test，但不得为缺失public tuple建立test-only production facade或让family
   解析Linux bit、errno、user pointer、sockaddr/optlen representation。
4. 将raw facts接入current Socket wait/recheck、poll/select/epoll和opened-description lifecycle。readable/writable各自重读
   raw owner predicate；notification不携带ready truth；dup/fork/non-final close不retire，final release先撤销source/route再
   non-blocking移交Endpoint retire。
5. 完成独立software-engineering review与Architecture Friction Scan，特别审查raw-local workaround、common front是否吸收
   family truth、opened-description cleanup顺序、wait route撤销与late hint stale isolation。Checkpoint 2A只关闭内部消费和
   review边界，不更新current contract、不关闭整个RFC，也不写register。

### Validation

- 重跑受影响的Checkpoint 1 owner/path proof，并补齐kernel raw family、common Socket normalized operation、wait/
  opened-description、creation rollback与final release的KUnit/source proof。
- UDP、Unix、iomux/epoll与network shutdown mandatory regression通过；source audit证明没有raw-only FileOps/wait loop、
  concrete downcast、second association/readiness/lifecycle truth或test-only production dependency。
- resolver/source proof确认raw tuple仍不可达；RV64/LA64 build与documentation validation通过。raw focused guest ABI、
  curated Socket LTP与BusyBox ping保持**Not Run**，不能由owner-local proof或build替代。

### Closure Evidence

- kernel ICMP raw family已经通过静态descriptor接入common Socket front：Stack继续唯一拥有Endpoint lifecycle、
  local/peer association、filter、queue与readiness facts；family只保存TTL/TOS和Linux raw peer-port投影，source只保存
  Endpoint capability、reverse registration与non-owning poll routes。creation rollback与semantic final release都先撤销
  source publication/route，再non-blocking移交Endpoint retire；late hint在retire后fail closed。
- common front新增optional datagram destination、peek与完整packet-length receive outcome、静态ByteStream/Datagram
  FileOps dispatch、normalized option query/mutation以及单次datagram send的opaque family snapshot。raw在第一次attempt固定
  destination、TTL/TOS与control-plane route/source/interface selection；`WouldBlock`重试只重新执行Stack admission，不跨
  wait持有mutex、control-plane/Stack借用、user cursor、ready truth或commit authority。UDP仍不开放read/write，Unix仍为
  byte stream，既有family行为保持。
- owner-local KUnit覆盖descriptor creation rollback/final release、bind/connect/disconnect与option snapshot、raw peer
  prefix/完整packet length、common Datagram FileOps、blocking retry期间并发option/reconnect不改变send snapshot，以及
  production `prepare_send`/`send_prepared`经`DomainStack::protocol_transition`到source reverse route和retire后的late-hint
  isolation。最终RV64 fresh QEMU执行394/394 KUnit并报告`All tests passed!`；同次既有UDP 16/16、Unix 23/23回归通过。
- `just test net-host`通过，包括11项ICMP raw Stack tests、33项vendored smoltcp IPv4 tests以及frame、bounded
  progression、multi-instance、multi-interface和UDP topology回归；no-default-features build/check通过。`just test xtask`
  74/74通过，`just fmt kernel --check`与`git diff --check`通过。
- 最终源码的`qemu-virt-rv64-release`与`qemu-virt-la64-release`均完成discovery/final pass和symbol table验证。RV64较早的
  sandbox build在lwext4 C compile触发`Bad system call`，相同canonical命令在sandbox外通过，因此该次失败只归类为
  validation environment limitation。
- 首轮独立software-engineering review发现blocking send重试会重新读取TTL/TOS与default peer的Keter；上述opaque
  operation snapshot及focused KUnit修复后，同一reviewer复核为Apollyon 0、Keter 0。仍有两项不阻断closure的Euclid：
  raw retry与真实Stack saturation/recovery分别证明，RX peek/detach owner与copy prefix/length/fault边界也分别证明，尚未由
  单个production-family case串起完整组合矩阵。当前源码路径和owner model无偏差；最小增强是在Checkpoint 2B focused
  guest oracle中串起真实capacity retry和peek/non-peek的zero/short/fault矩阵，不为此建立test-only production facade。
- source audit确认resolver仍拒绝局部测试值`AF_INET + SOCK_RAW + IPPROTO_ICMP`，没有公开raw tuple常量、
  `CAP_NET_RAW` admission、raw fd publication、raw-only FileOps/wait loop、concrete family downcast、第二份
  association/readiness/lifecycle truth或test-only production dependency。
- raw focused guest ABI、curated Socket LTP、BusyBox ping、LA64 runtime、hardware、`smp > 1`与final harness：
  **Not Run**。RV64 KUnit与build不能替代这些Checkpoint 2B product claims。
- `mdbook build docs`通过。Cutover为None；未更新current contract、register或transaction，Git commit拥有2A执行证据。

**Cutover：** None。所有current contracts保持effective；syscall不可达的final-shape Socket consumer及其review只构成进入
Checkpoint 2B ABI publication的前置证据。

**Stop / Exit：** common front若只能通过downcast、第二FileOps、raw-only wait loop或generic message framework接入，raw
family若需要复制association/readiness/lifecycle truth，owner-local proof若需要test-only production facade，或实现必须提前
注册tuple、发布fd、改变current contract/ABI/acceptance，必须停止。完成条件是final-shape internal consumer、既有consumer
回归与独立review共同闭合；关闭后必须停止并等待Checkpoint 2B单独授权。

## Checkpoint 2B — ABI Publication、产品验收与 `ICMP-RAW-CUTOVER`

**状态：** Closed / `ICMP-RAW-CUTOVER` Complete

**Purpose：** 在Checkpoint 2A已经证明shared Socket consumer形状后，发布完整ICMP raw Socket UAPI，闭合Linux-visible
matrix、全部mandatory product evidence、最终review和唯一current-contract cutover。

**Prerequisites：** Checkpoint 2A已经关闭且review没有遗留Keter/Apollyon；维护者单独授权Checkpoint 2B；父RFC仍为同一
accepted revision。若Checkpoint 2A证据要求改变target、owner、ABI、Contract Impact、acceptance或validation claim，必须
先完成RFC review，不能直接进入本段。

**Protected Boundary：** 保持Checkpoint 1 packet owner/transaction与Checkpoint 2A common front/wait/lifecycle owner边界，
完整服从父RFC Linux-visible matrix和mandatory validation floor；不得以partial publication、transitional contract或降低
任一architecture/evidence claim换取cutover。

### Deliverable

1. 在Socket resolver/credential adapter增加唯一ICMP raw tuple与`SOCK_NONBLOCK|SOCK_CLOEXEC` normalization；读取current
   task effective `CAP_NET_RAW`并在任何Endpoint/source/file/fd publication前稳定拒绝`EPERM`。static descriptor是唯一
   semantic type witness，backend不保存任意protocol number。
2. 在ABI adapter闭合Linux sockaddr、addrlen、flags、zero/short/fault、copy/consume、errno与sockopt optlen/value矩阵；
   `MSG_NOSIGNAL`兼容路径必须有关键注释、低噪声diagnostic、行为边界和移除条件，unsupported options稳定
   `ENOPROTOOPT`。
3. 新增target-complete `socket-test` ICMP raw suite、tracked curated `socket` LTP group与现有runner的ordinary
   BusyBox ping入口；validation asset拥有exact case table，RFC/implementation不复制第二份ABI oracle。
4. mandatory evidence全部满足后，完成final software-engineering review与Architecture Friction Scan，原子更新受影响current
   contracts、RFC status/revision/closure与必要导航。只有真实剩余defect/accepted gap进入register；不为成功路径创建
   limitation或transaction占位。

### Validation

- 重跑Checkpoint 1全部proof与Checkpoint 2A的kernel Socket、request/outcome normalization、wait/opened-description
  proof，以及UDP、Unix、iomux/epoll和network shutdown mandatory regression。
- 完整执行父RFC的[双架构focused guest ABI、双架构/双libc curated Socket LTP与真实ping](./index.md#acceptance-与-validation)
  matrix；各evidence owner分别记录实际结果，任一层不能替代另一层。
- focused oracle独立证明`IP_TTL`改变真实header；stock LTP不修改上游case/oracle、不只运行有利subcase，`TCONF`不记作
  能力覆盖。
- RV64/LA64 build、source/dependency audit与documentation validation通过；父RFC列出的optional claim只有实际运行时才
  增加独立证据，否则保持Not Run。

### Closure Evidence

- final source的owner-local/host proof通过：`just test net-host`覆盖raw ingress、fanout、Endpoint、TX/RX、MTU、capacity、
  provider path及UDP topology；`just test xtask`为74/74；kernel、socket-test、user-test format和`git diff --check`通过。
- RV64与LA64 canonical wrapper均完成release discovery/final pass及symbol verification；guest KUnit分别输出
  `All tests passed!`，focused suite均为`RAWICMP:SUMMARY:PASS:10`。
- 两架构普通BusyBox `ping -c 1 10.0.2.2`均为1 transmitted / 1 received；focused header oracle另外证明TTL/TOS进入真实
  IPv4 header，并以canonical VirtIO backing推导的2002-byte ICMP message成功、2003-byte `EMSGSIZE`证明live MTU boundary。
- 两架构的glibc与musl均完整执行tracked `socketpair02`、`bind03`、`listen01`，每架构汇总
  `attempted=6 passed=6 failed=0 infra_failed=0 skipped=0`；没有修改stock binary、subcase selector或以`TCONF`替代覆盖。
- 两架构均完成filesystem、network与device orderly shutdown。LA64没有成功的machine poweroff handler，在所有mandatory
  marker出现后由QEMU monitor退出；该既有platform termination边界不替代或削弱network shutdown evidence。
- 首轮独立review发现raw copy ceiling、unsupported option fault precedence、`AF_UNSPEC`完整copy与transaction matrix四项
  blocking finding；修复并重新生成全部双架构证据后，同一reviewer复核为Apollyon 0、Keter 0，允许cutover。
- 最终Architecture Friction Scan保留一项非阻断Euclid：guest尚未把真实TX saturation、blocking retry与capacity recovery
  串成单个production-path case。owner-local proof已分别覆盖Stack saturation/recovery与immutable retry snapshot；最小后续
  增强是在guest观察`EAGAIN`、等待恢复并确认同一snapshot提交，不建立test-only production facade。

**Cutover：** 上述证据、停止条件与final review全部满足后，已原子执行`ICMP-RAW-CUTOVER`：Refine
`SOCKET-FRONT-001`、`SOCKET-ABI-001`、`NET-PROTOCOL-BOUNDARY-001`、`NET-SOCKET-WAIT-001`；Introduce
`NET-ICMP-RAW-INGRESS-001`、`NET-ICMP-RAW-ENDPOINT-001`、`NET-ICMP-RAW-TRANSACTION-001`。没有partial或transitional
contract；register与transaction均无成功占位变化。

**Stop / Exit：** Linux oracle若要求改变父RFC能力/errno/copy policy，ABI publication若暴露Checkpoint 2A owner/front/
wait/lifecycle边界无法保持，或任一mandatory architecture/validation claim需要降低，必须停止。完成条件是代码、tests、
全部mandatory evidence、contract write-back和RFC closure共同闭合；普通build或单次ping不能单独关闭本checkpoint。

## 证据与实施反馈路由

- 默认由Git/PR保存每个checkpoint的代码、review与validation evidence；只有执行实际变成长周期、多轮probe、多个cutover
  或target renegotiation历史时，才按需新增transaction devlog。
- 保持accepted target的concrete seam、engine、queue、request或module route correction直接更新本文，不递增RFC修订。
- target、owner、handoff、failure/cleanup、ABI、Contract Impact、acceptance或验证强度变化回写父RFC review；agent只能
  提交evidence和proposal，不能自行批准reduced target。
- current contract只在`ICMP-RAW-CUTOVER`更新；register只接收cutover后仍真实存在的open defect或accepted limitation。
- 每个checkpoint收口执行Architecture Friction Scan。没有具体摩擦或只剩Safe时不写占位章节；Euclid简短回写证据、影响
  和最小修正，Keter/Apollyon立即阻止checkpoint closure/cutover。
