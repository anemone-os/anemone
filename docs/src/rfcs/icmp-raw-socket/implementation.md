# IPv4 ICMP Raw Socket 实施路线

**状态：** R0 Accepted / Checkpoint 1 Active / Checkpoint 2 Not Active
**最后更新：** 2026-08-03
**父 RFC：** [RFC-20260803-icmp-raw-socket](./index.md)
**目标与不变量：** [目标与不变量](./invariants.md)
**当前修订：** R0
**执行授权：** 本轮只授权Checkpoint 1；Checkpoint 2与contract cutover均未授权

本文只长期保存跨 Socket、Network Stack 与 interface/IP owner 的实施顺序，不复制父 RFC 的 target、ABI matrix或
acceptance。R0已接受，本轮只激活Checkpoint 1；关闭后必须停止，不得自动进入Checkpoint 2。

本路线只有一个implementation stage、两个checkpoint和一个最终cutover。Checkpoint 1把当前最高风险的
post-admission packet seam及Stack raw owner闭合为syscall不可达的final-shape protocol capability；Checkpoint 2接入真实
Socket consumer、完成全部产品证据并原子执行`ICMP-RAW-CUTOVER`。当前没有独立probe、transitional contract或transaction
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
| RX ingress | 在interface/IP owner完成IPv4 parse/checksum与local-destination admission后，以callback-scoped borrowed original datagram调用Stack raw owner；raw fanout不返回handled并继续ordinary ICMP | callback/trait/function形状，copy或shared immutable backing |
| Raw Endpoint | domain Stack raw owner统一拥有identity、association/filter、bounded RX/TX、fanout/drop、facts与retire；kernel只取得role-scoped operation capability | registry/container、queue、lock、generation与private engine布局 |
| TX egress | control plane提供operation-local selection，Stack raw owner形成header并在success前完成bounded admission；后续仍走selected smoltcp interface、neighbor与normal provider path | 复用受约束的smoltcp raw mechanism或新增窄private emission capability |
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
- **Validation claim：** Checkpoint 1只可声明owner-local/host protocol proof；Checkpoint 2必须取得父RFC要求的owner-local、
  双架构focused guest、双架构/双libc curated Socket LTP与双架构真实ping证据，证据层不能互相替代。
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

**状态：** Active

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
   engine或lock。该surface在Checkpoint 2前保持syscall不可达，并以Checkpoint 2真实consumer或路线撤销作为明确退出条件。

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

**Cutover：** None。所有current contracts保持不变；Checkpoint 1 closure只说明protocol capability具备进入真实Socket
consumer review的条件。

**Stop / Exit：** 以下任一事实阻止Checkpoint 1 closure：original bytes只能通过pre-admission delivery、header重建、
frame lifetime延长或destination truth复制取得；fanout需要shared bottleneck决定所有consumer命运；TX需要绕过selected
interface/neighbor/provider path或把route/source truth下推；retire需要等待worker或依赖`Drop`；temporary shared surface无法
给出Checkpoint 2 consumer/撤销条件。Checkpoint 1关闭后必须停止并等待Checkpoint 2单独授权。

## Checkpoint 2 — Socket Consumer、产品验收与 `ICMP-RAW-CUTOVER`

**状态：** Not Active

**Purpose：** 用Checkpoint 1 capability交付完整ICMP raw Socket UAPI，收敛general Socket第三个异构consumer，完成全部
mandatory acceptance、最终review和唯一current-contract cutover。

**Prerequisites：** Checkpoint 1已经关闭且review没有遗留Keter/Apollyon；维护者单独授权Checkpoint 2；父RFC仍为同一
accepted revision。若Checkpoint 1证据触发target/owner/ABI/acceptance变化，必须先完成RFC review，不能直接进入本段。

**Protected Boundary：** 保持Checkpoint 1 owner/packet transaction、父RFC Linux-visible matrix和mandatory validation
floor；不得为方便接入复制Endpoint association/filter/facts、建立raw-only FileOps/wait loop或提前切任何contract ID。

### Deliverable

1. 在Socket resolver/credential adapter增加唯一ICMP raw tuple与`SOCK_NONBLOCK|SOCK_CLOEXEC` normalization；读取current
   task effective `CAP_NET_RAW`并在任何Endpoint/source/file/fd publication前稳定拒绝`EPERM`。static descriptor是唯一
   semantic type witness，backend不保存任意protocol number。
2. 建立kernel ICMP raw family：持有role-scoped Endpoint capability、source publication与TTL/TOS policy；提供
   bind/connect/disconnect/query、send/receive、option、facts projection与final-release handoff，不取得Stack-private state。
3. 让general Socket front只增长三个真实consumer需要的最窄surface：optional datagram destination、datagram receive
   `PEEK/TRUNC`与完整packet-length outcome、common read/write到family-neutral send/receive、family option query/mutation
   dispatch。若current common shape无法自然表达这些义务，直接在正确framework owner修正，不在raw family内增加
   translation/downcast/parallel FileOps；同时不得借机建立`sendmsg/recvmsg`、ancillary、generic option bag或future TCP
   surface。
4. 在ABI adapter闭合Linux sockaddr、addrlen、flags、zero/short/fault、copy/consume、errno与sockopt optlen/value矩阵；
   `MSG_NOSIGNAL`兼容路径必须有关键注释、低噪声diagnostic、行为边界和移除条件，unsupported options稳定
   `ENOPROTOOPT`。
5. 将raw facts接入current Socket wait/recheck、poll/select/epoll和opened-description lifecycle。readable/writable各自重读
   raw owner predicate；notification不携带ready truth；dup/fork/non-final close不retire，final release先撤销source/route再
   non-blocking移交Endpoint retire。
6. 新增target-complete `socket-test` ICMP raw suite、tracked curated `socket` LTP group与现有runner的ordinary
   BusyBox ping入口；validation asset拥有exact case table，RFC/implementation不复制第二份ABI oracle。
7. mandatory evidence全部满足后，完成final software-engineering review与Architecture Friction Scan，原子更新受影响current
   contracts、RFC status/revision/closure与必要导航。只有真实剩余defect/accepted gap进入register；不为成功路径创建
   limitation或transaction占位。

### Validation

- 重跑Checkpoint 1全部proof，并补齐kernel Socket/ABI/wait/opened-description的KUnit与source proof，以及UDP、Unix、
  iomux/epoll和network shutdown mandatory regression。
- 完整执行父RFC的[双架构focused guest ABI、双架构/双libc curated Socket LTP与真实ping](./index.md#acceptance-与-validation)
  matrix；各evidence owner分别记录实际结果，任一层不能替代另一层。
- focused oracle独立证明`IP_TTL`改变真实header；stock LTP不修改上游case/oracle、不只运行有利subcase，`TCONF`不记作
  能力覆盖。
- RV64/LA64 build、source/dependency audit与documentation validation通过；父RFC列出的optional claim只有实际运行时才
  增加独立证据，否则保持Not Run。

**Cutover：** 只有上述证据、停止条件与final review全部满足时，原子执行`ICMP-RAW-CUTOVER`：Refine
`SOCKET-FRONT-001`、`SOCKET-ABI-001`、`NET-PROTOCOL-BOUNDARY-001`、`NET-SOCKET-WAIT-001`；Introduce
`NET-ICMP-RAW-INGRESS-001`、`NET-ICMP-RAW-ENDPOINT-001`、`NET-ICMP-RAW-TRANSACTION-001`。任一mandatory claim缺失时
全部current contracts保持旧规则并记为Not Cut Over，不做partial cutover。

**Stop / Exit：** Linux oracle若要求改变父RFC能力/errno/copy policy，common front若只能通过downcast/第二FileOps或
generic message framework接入，wait/final release若需要第二lifecycle truth，或任一mandatory architecture/validation
claim需要降低，必须停止。完成条件是代码、tests、全部mandatory evidence、contract write-back和RFC closure共同闭合；
普通build或单次ping不能单独关闭本checkpoint。

## 证据与实施反馈路由

- 默认由Git/PR保存每个checkpoint的代码、review与validation evidence；只有执行实际变成长周期、多轮probe、多个cutover
  或target renegotiation历史时，才按需新增transaction devlog。
- 保持accepted target的concrete seam、engine、queue、request或module route correction直接更新本文，不递增RFC修订。
- target、owner、handoff、failure/cleanup、ABI、Contract Impact、acceptance或验证强度变化回写父RFC review；agent只能
  提交evidence和proposal，不能自行批准reduced target。
- current contract只在`ICMP-RAW-CUTOVER`更新；register只接收cutover后仍真实存在的open defect或accepted limitation。
- 每个checkpoint收口执行Architecture Friction Scan。没有具体摩擦或只剩Safe时不写占位章节；Euclid简短回写证据、影响
  和最小修正，Keter/Apollyon立即阻止checkpoint closure/cutover。
