# net-udp 迁移实施计划

**状态：** R0 / Stage 0 Active / Checkpoints 0A-0B Closed / 0C Not Authorized / Stage 1-5 Outline
**最后更新：** 2026-07-29
**父 RFC：** [RFC-20260729-net-udp](./index.md)
**目标与不变量：** [net-udp 目标与不变量](./invariants.md)
**当前契约：** [Network current contracts](../../contracts/net/index.md)、
[Opened-description lifecycle](../../contracts/task/opened-description-lifecycle.md)、
[Poll wait / source registration](../../contracts/iomux/poll-wait.md)、
[Epoll protocol](../../contracts/epoll/protocol.md)、
[System Target](../../contracts/configuration/system-target.md)
**当前修订：** R0
**事务日志：** [2026-07-29 net-udp](../../devlog/transactions/2026-07-29-net-udp.md)

> 本文是R0的canonical实施顺序、stage maturity、probe、验证和resolved write set。R0已由public review接受，
> transaction已经建立；本轮独立授权覆盖的Stage 0 Checkpoint 0B已经关闭。Stage 0本身尚未关闭，当前停止并
> 等待0C独立授权；Stage 0不修改current contract。

## 1. 计划角色与 authority

本计划把[R0 accepted target](./index.md)和[目标与不变量](./invariants.md)转化为可滚动解析的
实施路径。它不得重新选择以下 target：initial domain 内唯一 global protocol `Stack`、domain-local logical
interface owner、Stack-owned Endpoint/binding truth、control-plane-owned route/source/interface policy、kernel
Socket-owned Linux ABI/readiness/error，以及五项 R0 用户可见决定。

本计划只冻结第一个可执行阶段。Stage 1-5 是 future Outline；其中列出的目录、模块和 contract gate 只是后续
resolution 输入，不是 write permission，也不是 concrete object graph。Stage N 必须先按自己的验证和退出条件
独立 Closed，之后才能运行只读的 `N -> N+1 Implementation Resolution Gate`。解析完成只让下一阶段达到
Ready，不自动进入 Active。

进入实现前必须：

1. R0由独立public review接受；Draft promotion本身不构成acceptance；
2. 建立独立transaction，并重新读取当时的live source、current contracts、register、branch/HEAD与dirty state；
3. 确认Stage 0的假设、命令和Resolved Write Set Manifest未漂移；如有漂移，先在本文重新解析并review；
4. transaction只记录activation preflight、批准事实和本文链接，不复制Ready定义或manifest；Stage 0仍须
   获得独立启动授权。

## 2. Live baseline 与首阶段选择

2026-07-29 的 source audit 得到以下 baseline：

- `anemone-kernel/src/net/worker.rs` 的每次 external netdev attach 都执行 `Stack::new()`；每个 worker 的
  `PumpCore` 独占一份 `Stack + provider + InterfaceId`。当前 production 因此仍是 per-netdev Stack wiring。
- `anemone-smoltcp-stack::Stack` 虽能保存多个 `InterfaceEntry`，但每个 entry 独立保存 `SocketSet`，`pump()`
  仍按一个 `InterfaceId + FrameProvider` 推进。该形状尚未形成 domain-wide Endpoint namespace。
- `FrameDevice` 固定报告 `smoltcp::phy::Medium::Ethernet`。当前 shared frame contract 没有 production
  IP-medium software-link capability，不能把 `lo` 伪装成 Ethernet provider。
- vendored smoltcp UDP egress 会先从 TX buffer dequeue，再用当前 `Interface` context 选择 source address；
  找不到 source 时记录 trace 并把 datagram 当作已处理而丢弃。把同一 `SocketSet` 依次交给多个 interface
  poll，既不能保证目标 interface 先观察 datagram，也不能满足本 RFC 的 send-success boundary。
- kernel 尚无本 RFC 五项 network syscall handler；generic syscall table 对未注册入口返回 `ENOSYS`。
- `ProcFile` publication lifecycle、静态 single `final_release` hook、`File::prv`、`PollRequest` 以及
  iomux/epoll snapshot-register-recheck protocol 已存在，应作为后续 socket consumer 的窄基线，不另建第二套
  fd、final-close 或 wait truth。
- current `SystemTarget` schema 只包含 Platform、root 与 initial-program selection，没有 network section；
  build resolver把它保存在 resolved selection 中，但尚未 materialize network typed input。
- register 中与本路径直接相关的 frame-path conformance issue 已关闭；没有可替代本 RFC target 或允许绕过
  current contracts 的 active network limitation。

首个高风险问题不是 syscall 参数布局，而是：在当前 smoltcp interface/socket egress 形状下，能否建立一个
Stack-level Endpoint owner，使同一个 endpoint namespace 安全跨多个 external interface 与 production-shaped
local software link推进，并在 admission 前固定 route/source/interface selection，而不复制 Endpoint truth或让
错误 interface dequeue datagram。

因此 Stage 0 只验证这一条架构主链。它不触碰 kernel、UAPI、build configuration、current contract 或 public
shared API；成功结果为 Stage 1 提供最窄可用内部 seam，失败结果精确说明下一阶段是否需要 vendored smoltcp
扩展、不同 engine-resource mapping，或 target review。先写 syscall/file object 只会把未决 topology 埋进 ABI
lifecycle，本计划不采用该顺序。

## 3. 实施原则

### 3.1 纵向结果，而不是按 crate 分层施工

后续每个语义阶段必须交付可验证的 cross-owner result。单独“完成 API types”“完成 smoltcp socket wrapper”或
“注册 syscall number”都不构成 stage closure。Stage 0 是例外意义上的 probe stage：它本身必须形成一个可判定
的 topology 结论，但不形成用户能力或 contract cutover。

### 3.2 Host-first，但 host 不替代 production

Stage 0 使用 deterministic host provider 和 software link 精确控制 interface order、capacity、recheck、source
selection 与 packet ownership。它只证明 protocol-owner 内部路线，不证明 kernel lock/wake、真实 fd/syscall、
VirtIO、QEMU、LA64 或 external deployment。后续 stage 必须分别提供 kernel/runtime 证据；Stage 0 PASS 不得
外推为 functional UDP。

### 3.3 Control plane 决定 selection，Stack 执行 admission

Stage 0 可以使用 test-owned immutable selection input，但不得让 Stack 根据 private interface iteration 建立
第二份 route/source policy。operation 进入 Stack 前必须已经携带选中的 logical egress identity 与 source address；
Stack只验证对应 private interface mapping仍有效、完成 Endpoint capacity/admission，并保证非目标 interface不会
consume该datagram。一次选择结果不得写回 wildcard/implicit binding。

### 3.4 Probe 不建立 shared API 或第二 owner

Stage 0新增的ordinary UDP owner type和operation method保持`anemone-smoltcp-stack`私有。独立integration test若
无法调用crate-private fixture，可以保留最窄的`#[cfg(feature = "host-test")] pub` validation facade；它只传递
protocol-domain input/outcome和test observation，不暴露smoltcp handle、buffer/queue identity或production
authority，并且不得进入kernel的`default-features = false`dependency。Stage 0不得修改`anemone-net-api`、复制
一份per-interface Linux Endpoint，或建立future TCP trait hierarchy。只有后续真实kernel consumer与stack
implementation共同证明某个value/capability必须跨crate后，对应Ready stage才可解析shared surface及其具体Rust编码。

### 3.5 Bounded resource 与 recheck 从第一条路径开始

external provider credit、Endpoint TX/RX storage和local software-link handoff都必须有有限capacity。normal full/
empty/exhausted返回可重查outcome，不得panic、busy-spin或用无界`VecDeque`吸收压力。notification只请求重查，
不得成为capacity、selection或delivery truth。

### 3.6 实现反馈不改写 target

Stage 0 若证明当前smoltcp public surface不够，只说明本次“无需vendored change”的route失败，不说明global
Stack、single Endpoint owner或send-success target失败。保持target的内部route修正写回本文与transaction；只有
证据要求移动owner、复制binding truth、允许success后source-drop、绕过normal ingress或降低acceptance floor时，
才停止并进入RFC review / `Target Renegotiation Gate`。

## 4. 阶段成熟度与滚动解析

- `Outline`：future stage，只冻结目的、依赖、受保护边界和解析触发点。
- `Ready`：交付、probe路线、审计、可观测性、验证、停止/退出条件、contract cutover与
  `Resolved Write Set Manifest`均已解析，但尚未获得执行授权。
- `Active`：公共RFC、transaction与用户/编排协议已经明确授权当前stage开始执行。
- `Closed`：当前stage按自己的review、验证与退出条件独立关闭；不因下一stage仍是Outline而保持Active。
- Stage N关闭后，单独运行只读`N -> N+1 Implementation Resolution Gate`；不得把解析下一stage作为
  Stage N closure的一部分。
- Ready/Active manifest冻结后才存在write-set expansion。future Outline的自然收窄、扩大、拆分或重排不是
  expansion，但改变target、owner、ABI、contract或acceptance boundary必须回到RFC review。

## 5. 阶段路线图

| Stage | 成熟度 | 概括目的 | 前置依赖 | Contract 状态 |
| --- | --- | --- | --- | --- |
| Stage 0 — Multi-interface UDP topology probe | Active；Checkpoints 0A-0B Closed；0C未授权 | 验证单一Stack-level Endpoint owner、显式egress selection、双Ethernet interface与bounded IP-medium local link能否在现有shared/vendored边界内闭合 | 公共R0接受与transaction activation | None；全部保持现状 |
| Stage 1 — Initial domain / global Stack walking skeleton | Outline | 把current per-netdev Stack wiring迁移为initial-domain唯一Stack与logical-interface/attach authority，保留现有frame traffic | Stage 0 Closed | 候选`NETDEV-LIFE-001`/`NET-ATTACH-001` Refine与`NET-IFACE-DOMAIN-001` Introduce；解析前均Pending |
| Stage 2 — Static control plane与production loopback | Outline | materialize SystemTarget network input，建立唯一IPv4 control plane、local route与bounded production `lo` | Stage 1 Closed | 候选`STM-TARGET-001` Refine与`NET-CONTROL-PLANE-001` Introduce；是否本stage cutover由1->2 gate决定 |
| Stage 3 — Endpoint/socket nonblocking vertical slice | Outline | 建立opaque Endpoint association、bind/port/send/receive transaction与五项syscall的nonblocking纵切 | Stage 2 Closed | 候选`NET-PROTOCOL-BOUNDARY-001`、`NET-SOCKET-ENDPOINT-001`、`NET-UDP-TRANSACTION-001`；partial code不自动生效 |
| Stage 4 — Blocking/iomux与datagram hardening | Outline | harden opened-description retire/close/dup/fork race，并接入blocking/signal、poll/select/epoll、copy-fault consume、capacity/writable与fragment gate | Stage 3 Closed | 候选`NET-SOCKET-WAIT-001`及相关functional gate；保持既有OPENED-DESC/IOMUX/EPOLL IDs |
| Stage 5 — External/dual-architecture closure | Outline | 完成remote external双向路径、双架构同源测试、RV64 agent-run、LA64 user-run、旁路删除与原子final cutover | Stage 4 Closed | 所有仍Pending ID在达到各自evidence floor后Effective或明确Not Cut Over |

Stage名称与数量在future resolution中可以保持target地调整。上表不预定具体Rust类型、逐文件write set、
capacity数值或精确命令；这些只在对应stage变为Ready时冻结。

## 6. Future Stage Outlines

### 6.1 Stage 1 Outline — Initial domain / global Stack walking skeleton

概括目的：

- 撤销每个external worker各自创建Stack的production语义路径，建立initial domain内唯一global Stack owner；
- 建立domain-local logical-interface membership/identity/name/kind owner，并让`lo`与external interface进入同一
  namespace；
- Refine external attach，使provider capability仍由原owner持有，而per-interface worker/port只取得global Stack
  的窄pump capability；
- 继续用现有frame/ICMP路径证明迁移未破坏`NET-FRAME-*`与`NET-STACK-PUMP-001`。

前置依赖：

- Stage 0按成功或负面证据独立Closed；
- `0 -> 1 Implementation Resolution Gate`已经选择可实现的engine topology，并重新读取current worker/
  provider/registry source。

受保护边界：

- 一个initial domain只有一个protocol Stack owner；任一时刻最多一个pump推进该Stack；
- `device/net`仍拥有external publication和provider handoff，provider仍拥有queue/DMA/IRQ/resource truth；
- logical identity/ifindex/name不得在old/new registry同时驱动行为；
- concrete `Stack`只留在initial-domain protocol composition/pump owner；per-interface worker/port只取得完成自身
  handoff所需的窄pump capability，future socket/syscall consumer不能由本stage获得raw `Stack`引用；
- 不引入socket UAPI、route table、Endpoint public API或complete teardown；
- current per-netdev path与new path不得同时处理production protocol mutation。

解析触发点：

- Stage 0 Closed后的只读preflight。该gate解析global Stack access window、provider/worker基数、logical-interface
  registry placement、attach rollback、temporary bridge删除点、candidate contract cutover、精确manifest与RV64
  frame-path regression。

预计范围：

- `anemone-kernel/src/{net,device/net}`、`anemone-smoltcp-stack`、focused KUnit/host tests与network current
  contracts。具体文件不是当前写入授权。

### 6.2 Stage 2 Outline — Static control plane与production loopback

概括目的：

- 扩展SystemTarget schema并materialize有限typed IPv4 deployment input；
- 建立唯一control-plane owner、connected/default/local route与source/interface selection；
- 建立first-class `lo`与bounded IP-medium software link，使local traffic经protocol egress和normal ingress；
- 证明self-external address走local software handoff而不是external backend hairpin。

前置依赖：

- Stage 1 Closed且logical-interface/global Stack owner已经稳定；
- live SystemTarget resolver/materializer与selected RV64/LA64 target重新审计。

受保护边界：

- SystemTarget只拥有deployment declaration；Platform/KernelConfig/Preset/rootfs不得复制IP/route truth；
- control plane唯一拥有address/route/selection policy，Stack只消费projection和operation-local selection；
- `lo`不伪造Device/Driver/Ethernet/IRQ/DMA，software link不拥有membership/address/Endpoint/readiness；
- missing/mismatched `eth0`不触发fallback、alternate selector或第二配置truth；
- 任何local queue都必须bounded、可重查并受pump budget约束。

解析触发点：

- Stage 1 Closed后的只读preflight。该gate解析schema、typed input、boot ordering、control-plane representation、
  local medium/queue、capacity/KConfig inventory、contract cutover与validation commands。

预计范围：

- `scripts/xtask/src/config/`与build materializer、`conf/system-targets/`、必要KernelConfig owner、kernel net/domain、
  stack local-link与双架构build。具体路径由1->2 gate冻结。

### 6.3 Stage 3 Outline — Endpoint/socket nonblocking vertical slice

概括目的：

- 建立Stack-owned opaque Endpoint lifecycle、domain-wide binding namespace、port0/implicit bind与完整R0 conflict
  matrix；
- 在`File::prv`建立kernel Socket private state与opaque Endpoint association，接入创建rollback和semantic final
  release trigger但不阻塞cleanup；
- 注册`socket`、`bind`、`sendto`、`recvfrom`、`getsockname`，先形成`SOCK_NONBLOCK`/`MSG_DONTWAIT`下真实
  loopback与external operation的纵切；
- 建立operation-local receive transaction和copyout前detach consume point。

前置依赖：

- Stage 2 Closed，control-plane selection和production loopback均可由narrow capability使用；
- live syscall/UAPI、FileOps、`ProcFile` final-release和user-copy owner重新审计。

受保护边界：

- fd number不成为Socket/Endpoint identity，dup/fork共享同一opened description；
- Stack不接收task/fd/user pointer/Linux errno，kernel不接收smoltcp handle/private queue；
- raw concrete `Stack`只由protocol composition/pump owner持有；kernel socket/syscall consumer只能取得Endpoint/UDP
  operation所需的窄capability surface，不能因concrete backend还有其它`pub` method而依赖它们；
- bind conflict、reservation和commit只在Stack；source selection不写回binding；
- send success前完成selection/admission；receive detach后copy fault不requeue；
- Stage 3从第一条Endpoint/File association起就完成creation rollback与opened-description semantic final release到
  Endpoint retire的基本handoff；不得把首次final-release接入后延到Stage 4，Stage 4只harden并发交错；
- unsupported family/type/protocol/flag稳定拒绝，不用日志或silent success替代功能。

解析触发点：

- Stage 2 Closed后的只读preflight。该gate解析exact sockaddr/errno/copy order、Endpoint identity/stale
  isolation、buffer inventory、module split、syscall registration、File private-state shape、final-release handoff、
  precise tests与candidate cutover；同时读取真实protocol composition、socket和syscall callsites，决定窄surface用
  trait、facade/newtype、free functions还是其它直接编码。若选择trait，必须由这个真实consumer边界和可验证fake/
  conformance用途证明，并只包含Endpoint/UDP operation；不得建立catch-all `ProtocolStack`或让consumer同时保留
  concrete `Stack`逃逸路径。`anemone-net-api`只有在该capability确实需要由stack实现、由kernel消费，且不能由
  kernel protocol-owner module内的private facade封闭时，才承载最小trait/value contract；不能只为预防误用而
  先建立共享抽象。

预计范围：

- `anemone-net-api`的最小真实cross-crate consumer surface、`anemone-smoltcp-stack` Endpoint operations、kernel
  socket/VFS/syscall/user-copy与architecture-neutral user tests。具体路径不是当前授权。

### 6.4 Stage 4 Outline — Blocking/iomux与datagram hardening

概括目的：

- 让default blocking、`O_NONBLOCK`、`SOCK_NONBLOCK`和`MSG_DONTWAIT`共享同一owner predicate；
- 把socket接入现有poll/select/epoll snapshot-register-final-recheck协议和signal/cancel路径；
- 关闭readable/writable、Endpoint saturation/recovery、provider backpressure、late edge、retire race与dup/fork/
  close交错；
- 完成zero/short/oversize、三类copy fault、concurrent receive与IPv4 first/later fragment显式拒绝。

前置依赖：

- Stage 3 Closed且nonblocking syscall/Endpoint lifecycle纵切已经稳定；
- live iomux/epoll/opened-description contracts与实现重新审计。

受保护边界：

- Endpoint fact、notification与Linux readiness/error保持分离；event/wake只请求重查；
- source registration和predicate publication遵守`IOMUX-POLL-001..003`，不另建socket wait core；
- writable只表示destination-independent general Endpoint admission，不镜像route/provider queue truth；
- user copy不持Stack/source private lock，copy fault不回滚已detach datagram；
- fragment必须在UDP header parsing/Endpoint lookup前拒绝，未启用reassembly不是充分证据。

解析触发点：

- Stage 3 Closed后的只读preflight。该gate解析source registry/lock、snapshot/outcome、capacity数值、fragment
  gate落点、signal restart policy、focused stress与contract cutover。

预计范围：

- kernel socket source、fs/iomux与epoll consumer、stack Endpoint snapshot/event、host race tests与普通用户态
  syscall tests。具体路径由3->4 gate冻结。

### 6.5 Stage 5 Outline — External/dual-architecture closure

概括目的：

- 在最终exact code上证明loopback、self-external local delivery和remote external ingress/egress是三条不同证据；
- 完成host deterministic matrix、RV64 QEMU `smp=1` agent-run与LA64 QEMU `smp=1` user-run；
- 删除probe-only control、temporary dual wiring、legacy ifindex projection与validation-only production seam；
- 原子更新所有达到gate的current contracts、RFC状态、transaction、register/limitations与最终claim。

前置依赖：

- Stage 4 Closed；所有in-target Keter/Apollyon已经neutralized或转成明确cutover stop condition；
- 开发者准备LA64 runtime输入并负责user-run evidence。

受保护边界：

- 同一份architecture-neutral test源码与case用于RV64/LA64；launcher、镜像和remote endpoint参数可以不同；
- LA64失败阻塞closure，未运行只记`Not Run`；RV64/host结果不得外推；
- local path不冒充external provider proof，QEMU不外推physical hardware/virtio-pci/`smp>1`；
- partial success不更新functional contract或把target内失败登记为accepted limitation。

解析触发点：

- Stage 4 Closed后的只读preflight。该gate冻结final case/marker/log/command、external harness、LA64 handoff、
  exact contract write-back、probe deletion与final resolved manifest。

预计范围：

- host/network tests、architecture-neutral user-test资产、两个SystemTarget/Platform wrapper输入、current contracts、
  RFC/transaction/register与validation outputs。具体文件不是当前授权。

## 7. Stage 0 Ready — Multi-interface UDP topology probe

### 7.1 阶段状态与 activation preflight

**成熟度：** Active；Checkpoints 0A-0B Closed。0C仍未授权，当前不得继续Stage 0执行。

进入Active前必须同时满足：

1. 本RFC target已经R0接受，本文仍是canonical implementation authority；
2. 已创建指向R0的transaction，并记录branch、HEAD、dirty worktree、Stage 0 manifest和独立启动授权；
3. 重新读取live `anemone-smoltcp-stack::{Stack,InterfaceEntry,pump,FrameDevice}`、Cargo features、host provider、
   vendored smoltcp UDP `dispatch`/`Interface::socket_egress`与current network contracts；
4. 确认baseline仍是per-entry `SocketSet`、Ethernet-only adapter和per-netdev production Stack；若任一事实漂移，
   先更新公共implementation并重新review，不以旧manifest开始；
5. 检查manifest内文件与用户dirty changes是否重叠；任何重叠先确认归属并做语义合并，不覆盖；
6. 确认现有crate-owned host gate和`just fmt kernel --check`仍可用。仓库当前没有通用`just test`命令，本stage
   沿用net-frame-path已经接受的native Cargo host-test入口，不为一次probe增加Just/xtask/script wrapper。

### 7.2 Gate P0 probe contract

**Hypothesis：** 在不修改vendored smoltcp、`anemone-net-api`和kernel的前提下，`anemone-smoltcp-stack`内部可以用
一个Stack-level Endpoint owner和一个domain-wide binding/identity语义，驱动至少两个Ethernet interface和一个
bounded IP-medium local software link；control-plane提供的operation-local `selected interface + source address`
能够在queue/admission前固定，使非目标interface不会dequeue datagram，并且local flow仍经protocol egress与normal
ingress。

**Protected Goal / Invariant：** `NET-UDP-DOMAIN-001`、`NET-UDP-LOOPBACK-001`、
`NET-PROTOCOL-BOUNDARY-001`、`NET-CONTROL-PLANE-001`、`NET-UDP-DATAGRAM-001`和
`NET-STACK-PUMP-001`。本probe不得以per-interface Linux Endpoint/binding truth、Socket fast path、unbounded queue、
success后source-drop、fake UDP encoder/decoder或第二route table换取PASS。

**Contract Impact：** None。所有current contracts保持Effective；全部R0 candidate保持Pending / Not Effective。

**Non-goals：** kernel functional integration、production cross-crate API、syscall/File/iomux、port0 allocator、
完整bind conflict matrix、exact Linux errno、SystemTarget/KConfig、production VirtIO、fragment gate、runtime
shutdown、QEMU或contract cutover。独立integration test所需的feature-gated validation facade只属于test seam，
不构成production API。

### 7.3 Checkpoint 0A — Candidate seam risk characterization

交付：

- 在新增`udp_topology` host target中构造一个真实smoltcp UDP socket、两个具有不同IPv4 prefix的deterministic
  Ethernet interface，以及一份明确指向第二interface的operation-local selection；这描述candidate topology，
  不是声称current production Stack已经具有共享UDP Endpoint并发生故障；
- 证明naive candidate把同一engine UDP resource暴露给错误interface先poll时，会进入vendored UDP
  source-selection/drop或wrong-interface egress风险；测试记录queue ownership与两个provider的TX observation，
  不依赖trace文本判定；
- 把风险归类为candidate engine egress-admission seam ordering，而不是current frame provider、route table或syscall
  baseline defect；
- reproduction只服务本checkpoint。Checkpoint 0B形成正确路径后，不能保留“期望错误行为”为长期PASS测试。

停止条件：

- 若vendored/current source变化使candidate risk不再存在，停止当前Ready route并重新审计actual egress
  semantics、更新Stage 0 definition后再review；不能继续按旧假设修改Stack；
- 若只能通过packet injection、raw socket或手工UDP packet构造重现，说明fixture没有覆盖真实Endpoint路径，
  0A不能关闭。

**关闭记录：** 2026-07-29，0A以真实smoltcp UDP socket enqueue/egress路径稳定复现wrong-interface-first会
消费共享engine queue并从错误provider发包；ARP输入只用于准备普通Ethernet neighbor，没有注入或手工构造UDP
packet。风险归类为candidate egress-admission seam，不是current per-netdev production缺陷。精确证据见
[transaction](../../devlog/transactions/2026-07-29-net-udp.md#2026-07-29---checkpoint-0a-implementation-source-audit-and-closure)。
预期错误行为测试按0A退出条件保留；0B形成正确路径时必须删除或改写，且本记录不授权进入0B。

### 7.4 Checkpoint 0B — Stack-private production-shaped candidate

交付：

- 仅在`anemone-smoltcp-stack`内部建立provisional Endpoint/selection/local-link shape。ordinary owner logic保持
  `no_std + alloc`可编译；注入frame、设置selection、读取完整queue/packet仅存在于`host-test` fixture；
- 同一个Stack拥有一个UDP Endpoint identity/binding owner。不得为两个Ethernet interface和local interface复制
  三份会独立bind/retire的logical Endpoint；private engine resource可以存在多个，但必须由一个Endpoint owner
  聚合并证明binding、capacity、receive order与retire没有并列truth；
- test-owned control plane在operation外选择`InterfaceId + source IPv4`。Stack验证opaque mapping与Endpoint
  admission；selection failure、stale/unknown interface、unsupported source、oversize和bounded TX full均在成功
  commit前返回typed stack-local outcome；
- 只有selected external interface的pump可以消费该datagram。先pump其它interface必须保持Endpoint queue、
  owner与success/failure fact不变；之后selected interface经真实smoltcp UDP egress形成frame；
- local interface使用stack-private IP-medium software device/port，不修改shared `FrameProvider`并不伪造Ethernet
  facts。egress进入有界software-link storage，后续bounded pump从同一storage进入同一Stack normal IP/UDP ingress；
  不允许直接调用目标Endpoint enqueue/process或从source Socket复制到peer Socket；
- 两个external interface与local link共享同一个pump serialization boundary；每次interface/local round有限，
  blocked provider或full local link不能让其它ready interface永久饥饿；
- provisional Endpoint retire/remove必须从同一owner撤销它聚合的全部private engine resource、pending datagram与
  local-link association；随后使用stale provisional identity或继续pump都不能deliver/drive已撤销resource。本probe
  只证明aggregate teardown和no parallel truth，不证明File final release、port reuse或并发close race；
- private smoltcp `SocketHandle`、`SocketSet`、`Interface`、IP-medium device和buffer不得从crate public surface导出。

本checkpoint不预先要求literal `EndpointId`、`SelectedEgress`、`LocalLink`或`DomainPump`名称。若一个private type
同时混合Endpoint lifecycle、route policy、provider resource与test control，应在同一checkpoint内缩窄职责，
不能用`Manager`/`State`总对象掩盖owner边界。

**关闭记录：** 2026-07-29，0B以一个Stack-private Endpoint owner聚合每个external/local interface的private
smoltcp UDP resource；binding conflict、TX phase、receive ordering gate与retire均由aggregate owner推进。
operation-local selection在commit前验证interface mapping、source、destination、MTU与Endpoint capacity；只有
selected engine获得datagram，wrong-interface-first保持owner queue不变。新增bounded IP-medium local port经
protocol egress、下一bounded round的normal ingress完成delivery，provider blocked与local-link full均不阻塞其它
ready interface/Endpoint永久推进。retire先撤销aggregate identity，再删除全部engine resource及带owner的pending
local packet；host facade只暴露protocol-domain value/outcome与test-only observation。0A expected-wrong test已改写为
positive matrix。source、review与validation证据见
[transaction](../../devlog/transactions/2026-07-29-net-udp.md#2026-07-29---checkpoint-0b-implementation-review-validation-and-closure)。
本记录不选择0C的保留/cleanup路线，也不授权进入0C。

### 7.5 Checkpoint 0C — Decision closure与probe cleanup

成功路线：

- 保留证明target所需的最小ordinary stack code和long-term deterministic tests；删除0A的错误期望、packet dump、
  unrestricted inspection与不再需要的host-only method；只有独立integration test仍需要时才保留窄
  `host-test` facade，并在定义旁说明它不进入kernel dependency，以及在fixture可回到crate-private或accepted
  production control-plane owner替代后删除；
- 在transaction记录concrete topology decision、为何不需要vendored/shared API变化、仍未证明的kernel/runtime边界；
- 把Stage 1 Outline所需的route input写回本文，但不在0C解析或激活Stage 1。

负面路线：

- 若0B无法在manifest内满足hypothesis，删除无法形成独立长期价值的candidate/probe-only code；有长期价值时保留
  最小stable characterization/regression，否则只在transaction保留cleanup前的exact command/output/source point，
  不能用文字判断替代可复现证据；
- transaction必须指出失败发生在何处：非目标interface不可避免地dequeue、single Endpoint只能复制成
  per-interface truth、local medium必须修改shared frame API、或现有smoltcp缺少必要filter/dispatch seam；
- target保持不变时将结果分类为Route Correction，并由`0 -> 1`gate比较“narrow vendored smoltcp seam”与
  “Stack-owned aggregate engine resources”等候选；Stage 0不得自行扩大manifest尝试第二方案；
- 若证据要求移动control-plane/binding owner、允许success后silent drop、Socket fast path或削弱normal-ingress
  acceptance，停止cutover并进入Target Renegotiation Gate。agent只能提出，不能批准。

### 7.6 审计

Stage 0必须执行并在transaction记录以下source audit：

- `Stack`内所有`SocketSet`/UDP socket/handle owner与lookup，确认没有跨crate handle、per-interface Linux
  Endpoint或按interface分裂的binding truth；
- 每个TX enqueue/dequeue、selected-interface check和provider/local-link transfer，确认commit前后只有一个
  datagram owner；
- vendored UDP `dispatch`的no-source drop分支，确认Stage 0成功路径不能在syscall-equivalent success后进入；
- 所有local-delivery入口，确认没有direct Endpoint process/enqueue、Socket-to-Socket copy、unbounded queue或
  synchronous unbudgeted reentry；
- Endpoint retire/remove及所有private engine handle/resource lookup，确认withdraw先于stale lookup失败，pending
  datagram/local-link association不会在aggregate owner之后继续deliver或形成第二lifecycle truth；
- Cargo feature/public surface/dependency audit，确认kernel的`default-features = false`dependency不获得host
  control，conditional public facade只暴露validation input/outcome而不暴露smoltcp object/handle/buffer/queue
  identity，stack不依赖kernel/task/fs，`anemone-net-api`未被修改；
- currentframe host tests，确认multi-instance isolation、frame ownership、bounded progress和deadline没有因
  global endpoint experiment退化。

允许保留的旁路只有`#[cfg(feature = "host-test")]`下的fixture injection/inspection。独立integration test需要时，
该fixture可通过窄`pub` facade调用，但必须由Cargo target的`required-features = ["host-test"]`隔离，且no-default/
kernel dependency不编译或导出test control。ordinary UDP owner logic不得依赖host-test-only fact。

### 7.7 反馈假设与停止条件

本stage要验证的具体假设只有一个：current stack/vendored seam是否足以支持single Endpoint owner下的显式
interface/source admission。以下任一项出现即停止当前route，不允许通过扩展scope或降低断言继续：

- 非目标interface会peek/dequeue/consume属于另一interface的datagram；
- missing source/route/interface只能在smoltcp dequeue后发现，operation已无法同步失败；
- 必须让kernel/control plane保存smoltcp handle、SocketSet、queue depth或binding mirror；
- 必须修改`anemone-net-api`、vendored smoltcp、kernel或current contract才能继续；
- local delivery只能用unbounded queue、direct Endpoint injection或socket fast path完成；
- logical Endpoint必须复制为per-interface binding/readiness/lifecycle owner，且无法由一个Stack owner证明聚合；
- pump需要并发`&mut Stack`、持provider全局锁跨protocol callback，或普通负载无法有限推进；
- host-only API越过显式validation facade、进入no-default/kernel dependency或暴露private representation，或测试
  只能靠fake protocol path自证。

发生上述failure时，结果先写transaction。保持target的scope/route变化写回本文；设计issue如影响owner、stage
order或acceptance，另建/更新tracking issue；改变target/contract/ABI进入RFC review；target内已实现但错误的行为
进入open issue，不能登记为accepted limitation。

### 7.8 Contract cutover

None。Stage 0无论成功或得到负面结论，都不修改`docs/src/contracts/`、不增加pending-successor生效状态，也不
宣称`NET-IFACE-DOMAIN-001`、`NET-CONTROL-PLANE-001`、`NET-PROTOCOL-BOUNDARY-001`、
`NET-SOCKET-ENDPOINT-001`、`NET-UDP-TRANSACTION-001`或`NET-SOCKET-WAIT-001`已实现。

Stage 0失败时current per-netdev Stack和所有现有contracts保持baseline。Stage 0成功的private ordinary code也只
是尚未接入kernel的implementation input；没有production activation、syscall或runtime evidence前不能cut over。

### 7.9 模块边界预检

当前`anemone-smoltcp-stack/src/stack.rs`已经混合interface mapping、SocketSet ownership与host validation control，
`pump.rs`混合per-interface scheduling与socket egress。Stage 0若继续把Endpoint transaction、local software device
和test control全部塞入这两个文件，会固化新的混合边界。

因此本stage允许在同一crate/同一Stack owner内新建`src/udp.rs`与`src/local_link.rs`：

- `udp.rs`只承载provisional UDP Endpoint/operation ownership与stack-private engine conversion；
- `local_link.rs`只承载bounded IP-medium packet handoff与smoltcp device adaptation；
- `stack.rs`保留interface/endpoint mapping和owner-level orchestration；
- `pump.rs`保留finite progression与outcome组合；
- `adapter.rs`保留smoltcp device/token adaptation，不取得route/Endpoint policy。

这是same-owner、private、behavior-preserving-or-probe-local的结构维护，不扩大public API或shared contract。
若实际证据需要不同文件名但仍满足上述职责与manifest目录边界，可在activation preflight中冻结更窄清单；若需要
移动owner surface、导出public API、修改shared/vendored crate或触碰kernel，必须停止并申请write-set expansion，
不能包装成模块整理。

### 7.10 Scope envelope

参与owner：

- concrete `anemone-smoltcp-stack::Stack`：private interface/Endpoint/engine resource与唯一protocol mutation；
- deterministic external provider：test-ownedframe/resource truth；
- test-owned control plane：immutable route/source/interface selection input；
- stack-private local software-link：in-flight IP packet/capacity；
- host test harness：只拥有case orchestration与observation。

本stage不创建kernel `NetworkDomain` object，不决定production lock/worker/mailbox，不定义Linux errno或readiness，
不决定port allocator、SystemTarget schema或KernelConfig数值。本stage新增的ordinary UDP owner类型和operation
方法保持crate-private。独立integration test允许增加最窄的`#[cfg(feature = "host-test")] pub` validation facade；
其参数和返回值只表达protocol-domain input/outcome与test observation，不得暴露smoltcp object/handle、buffer/
queue identity或production authority。它是conditional test surface，不是production API；Cargo `required-features`
和kernel `default-features = false`dependency必须共同证明它不进入kernel可见surface。

### 7.11 Resolved Write Set Manifest

允许production/provisional source写入：

- `anemone-kernel/crates/anemone-smoltcp-stack/Cargo.toml`，只用于启用UDP/IP-medium所需smoltcp feature与声明
  `udp_topology` host test target；
- `anemone-kernel/crates/anemone-smoltcp-stack/src/lib.rs`；
- `anemone-kernel/crates/anemone-smoltcp-stack/src/{stack.rs,pump.rs,adapter.rs}`；
- 计划新建`anemone-kernel/crates/anemone-smoltcp-stack/src/{udp.rs,local_link.rs}`。

允许test/validation写入：

- `anemone-kernel/crates/anemone-smoltcp-stack/tests/support/mod.rs`；
- 计划新建`anemone-kernel/crates/anemone-smoltcp-stack/tests/udp_topology.rs`；
- Cargo/formatter生成的untracked build output；这些不是source、contract或freshness authority。

允许执行期文档写回（R0接受后由transaction引用）：

- `docs/src/rfcs/net-udp/implementation.md`，只记录Stage 0反馈、Route Correction与Stage 1 resolution；
- `docs/src/rfcs/net-udp/{index.md,invariants.md,tracking-issues.md}`，只有对应影响分类确实需要时；
- 当前net-udp transaction与transactions index/biweekly devlog的必要执行事实；
- target内缺陷或target外accepted gap确有结论时，分别写register open issue或current limitation。

validation-only只读输入：

- `anemone-kernel/crates/anemos/smoltcp/src/{socket/udp.rs,iface/interface/mod.rs,iface/interface/ipv4.rs,phy/mod.rs}`；
- `anemone-kernel/crates/anemone-net-api/src/**`；
- `anemone-kernel/src/{net,device/net,driver/net}/**`；
- 现有`anemone-smoltcp-stack/tests/{frame_path.rs,bounded_progress.rs,multi_instance.rs}`；
- 本RFC target、current network/opened-description/iomux/epoll contracts和register。

明确禁止写入：

- vendored`anemone-kernel/crates/anemos/smoltcp/**`；
- `anemone-kernel/crates/anemone-net-api/**`；
- kernel、device、driver、VFS、syscall、iomux、task与architecture source；
- root `Cargo.toml`/`Cargo.lock`、Justfile、xtask、`conf/**`、anemone-apps、current contracts和public navigation，
  除非后续独立docs review明确批准；
- 与net-udp无关的tracked或private file。

若manifest内无法完成Stage 0，不得先修改再追认。扩展申请必须说明失败证据、拟新增文件/owner、对target/
contract/gate/验证的影响，以及批准后在本文和transaction的记录位置。

### 7.12 可观测性

Stage 0只增加test-owned observation：

- 每个interface/provider的TX submission与RX consumption；
- Endpoint queue ownership、selected interface/source与typed admission outcome；
- aggregate Endpoint retire后private engine resource、pending datagram和local-link association均已撤销；
- local-link当前occupied credit、full/recovery/recheck和normal-ingress completion；
- bounded pump round与哪个owner仍有work的case-local记录。

这些字段不得参与production行为决策，必须在定义旁标明diagnostic/test-only。Stage 0不增加production per-packet
log、global diagnostic registry或长期counter。owner-local correctness使用常开`assert!`；昂贵的test扫描可以使用
test assertion，不把`debug_assert!`用于release correctness。

### 7.13 验证

Stage 0先按0C选择positive或negative路线，再在对应final source上执行分支验证和共同gate。

Positive branch：

1. `cargo test -p anemone-smoltcp-stack --test udp_topology`，覆盖：
   - wrong-interface-first不consume，selected interface唯一egress；
   - 两个不同prefix/source的external selection与同Endpoint identity；
   - no-selection/stale-interface/no-source/oversize/TX-full在commit前typed fail；
   - local egress -> bounded link -> normal ingress -> same Endpoint namespace；
   - local-link full/recovery/recheck、external provider blocked isolation与finite pump；
   - no-source vendored drop分支不在accepted admission后发生；
   - aggregate Endpoint retire撤销全部private engine resource/pending handoff，stale provisional identity不能继续
     deliver或drive；这不外推为fd final release、port reuse或concurrent close证明；
2. final diff若保留ordinary stack code或修改kernel dependency会编译的smoltcp feature，运行repository-owned
   `just build --preset qemu-virt-rv64-release --bind smp=8 --bind memory=1G`。它只证明RV64 kernel compile integration，
   不形成runtime、syscall或network behavior证据。

Negative branch：

1. candidate cleanup前，以transaction记录的exact test name/filter重新运行0A characterization，记录exact command、
   output、vendored/source decision point和0B失败分类；不能只保留文字判断；
2. cleanup后，若保留具有长期价值的窄characterization/regression test，则在final source上运行并PASS；若删除全部
   candidate/target test code，则保留cleanup前的transaction evidence，并用下面共同gate证明baseline恢复。Positive
   `udp_topology` capability matrix对此结果标为`Not Applicable — negative decision`，不能记为PASS或`Not Run`；
3. source audit确认无法形成独立价值的candidate/probe-only ordinary code、feature和conditional public facade均已
   删除；有意保留的regression必须逐项说明proof scope和退出条件。若negative final diff仍保留kernel dependency会
   编译的ordinary code/feature，也必须运行上述repository-owned RV64 build；否则以final diff记录该build为
   `Not Applicable — cleaned to host-only/baseline surface`。

共同gate（在positive或negative final source上均执行）：

1. `cargo test -p anemone-net-api -p anemone-smoltcp-stack`，沿用现有crate-owned host conformance gate并确认
   frame-path三个长期integration target继续实际执行；
2. `cargo test -p anemone-smoltcp-stack --no-default-features --no-run`与
   `cargo check -p anemone-smoltcp-stack --no-default-features`，证明host target由feature metadata隔离且ordinary
   stack code保持`no_std + alloc`；
3. `just fmt kernel --check`；任何新增formatter diff阻塞本stage，既有baseline只可原样记录；
4. dependency/public-surface/SocketHandle/SocketSet/source-selection/local-link/Endpoint-retire/queue-bound/pump-budget
   source audit；
5. `git diff --check`，以及新文件逐个`git diff --no-index --check -- /dev/null <file>`。

Stage 0不运行rootfs、QEMU、LTP或双架构runtime。它们记为`Not Run`，不能以host PASS或compile PASS外推。若Stage 0
需要kernel runtime才能证明hypothesis，说明scope已经扩大，应停止并申请新的Ready definition，不把broad runtime
当作替代证据。

### 7.14 退出条件

Stage 0独立Closed需要同时满足：

- Checkpoint 0A已经用真实smoltcp UDP path刻画candidate seam risk，并明确它不是current production baseline
  defect；
- Checkpoint 0B得到满足全部protected invariant的positive result，或者得到可复现、可分类且已清理partial code的
  negative result；
- 最终source没有fake UDP path、unbounded queue、per-interface Linux Endpoint truth、host control leak、
  Socket-to-Socket copy、vendored/shared/kernel write或未记录scope expansion；
- branch-specific与共同host/no-default/conditional-build/format/whitespace gates按positive或negative final code
  通过，`Not Applicable`与`Not Run`没有冒充PASS；
- transaction记录result、code disposition、remaining uncertainty和feedback route；
- current contracts保持不变；conditional RV64 build只按compile evidence记录，runtime/UAPI/architecture behavior
  claims明确`Not Run`；
- Stage 0先Closed。Stage 1是否、何时达到Ready由后续独立resolution gate决定，不是本stage closure条件。

## 8. Stage 0 -> Stage 1 Implementation Resolution Gate

前置条件：

- Stage 0已按7.14独立Closed；
- transaction已有exact diff、validation、review与positive/negative topology evidence；
- 当前RFC target、Contract Impact和current contracts仍有效。

只读preflight：

- 读取live Stack/interface/socket/pump、kernel `net::{mod,worker}`、`device/net` registry/provider、Stage 0 diff、
  review findings、host evidence和module-boundary pressure；
- 若Stage 0 positive，审计candidate能否在不泄露smoltcp object的前提下由kernel global Stack access window与
  heterogeneous external provider自然调用；
- 若Stage 0 negative，比较至少以下target-preserving路线：为vendored smoltcp增加最窄egress-selection seam；
  由Stack聚合多个private engine resource但保持单一Endpoint/binding/capacity truth；调整pump admission而不复制
  route policy。不能只按已写代码沉没成本选择；
- 核对Stage 1 Outline的logical-interface owner、global Stack、attach rollback、old wiring removal和frame-path
  regression是否仍构成可达路径。

解析输出：

- 选择并说明Stage 1的concrete Stack access/provider/worker路线；
- 把Stage 1展开为完整Ready：交付/checkpoint、internal API、module split、audit、observability、validation、
  failure/exit、temporary bridge删除点、candidate contract cutover与exact Resolved Write Set Manifest；
- 只改变implementation preference、physical files、stage order或validation时更新本文并在transaction记录Route
  Correction；
- 需要修改`anemone-net-api`或vendored smoltcp时，必须在Stage 1 manifest中明确新增的窄surface、真实consumer、
  object fence、no-default proof和退出条件，不能从Stage 0自动继承权限；
- 改变global Stack、Endpoint/control-plane owner、send-success、normal-ingress、ABI或acceptance boundary时停止，
  进入RFC review / Target Renegotiation Gate。

授权边界：

- Stage 1完整定义和manifest冻结后才达到Ready；必须另行授权进入Active，不得由Stage 0 closure、positive probe
  或transaction记录自动启动。

## 9. 旁路审计清单

后续每个Ready stage都必须按实际scope细化本清单；至少持续检查：

- `Stack::new()`、`SocketSet::new()`、smoltcp socket construction与handle storage，确认production只有一个domain
  protocol owner且没有hidden per-netdev namespace；
- netdev ifindex/name lookup与logical-interface lookup，确认cutover后只有一份行为truth；
- route/source/interface selection入口，确认kernel Socket、Stack private iteration与provider没有第二policy；
- local-address、loopback与packet injection入口，确认没有backend hairpin、Socket fast path或host fixture进入
  production dependency；
- syscall、File private state、fd table与final-release callsites，确认不按fd/memory lifetime retire Endpoint；
- poll/register/notify paths，确认route publication/current predicate原子、notify在guard外且final recheck存在；
- UDP enqueue/dequeue/copyout/fragment paths，确认single owner、send success与receive consume linearization；
- `cfg(test)`/feature/log/diagnostic fields，确认validation-only state不驱动production behavior；
- temporary bridge/fallback/legacy owner，确认有日志/注释、唯一behavior authority和明确删除gate。

## 10. 可观测性清单

实现期可观测性服务于owner、handoff、failure与validation，不建立第二truth：

- boot/attach摘要：logical identity、external publication origin、Stack-private mapping仅以opaque/diagnostic形式
  关联；不打印或暴露driver backing/smoltcp handle；
- endpoint lifecycle：create/bind/retire/reject可使用opaque diagnostic ID，但ID不得驱动lookup之外的行为；
- admission failure：unsupported ABI、bind conflict、no route/source/interface、oversize、normal capacity与
  would-block应能区分；静默兼容或未支持flag必须按syscall准则注释并记录；
- wait：只记录predicate transition/recheck/cancel类别，不把wake count或event payload当ready proof；
- validation marker必须标注host、RV64 agent-run、LA64 user-run与Not Run，不从一个轨道生成另一轨道结论；
- production per-packet dump、长期queue mirror和unbounded日志不作为默认方案。

## 11. 停止边界

以下情况继续在implementation层解析，不重开target：private type/方法名、module placement、lock/actor/worker、
queue或allocator形状、port选择算法、capacity数值、test case拆分、vendored narrow seam与Stack-private aggregate
之间的路线选择，只要保持owner/ABI/contract/acceptance。

以下情况立即停止当前gate并上报：

- 需要第二domain/Stack、per-interface Endpoint namespace或双route/binding/readiness truth；
- 需要扩大或降低五项R0用户可见决定、first-version syscall/flag/copy/fragment行为；
- 需要改变`NETDEV-LIFE-001`/`NET-ATTACH-001`分类、SystemTarget owner、OPENED-DESC/IOMUX/EPOLL contract；
- 需要Socket fast path、success后silent drop、copy fault requeue、unbounded queue、busy-poll或无法退出的compat bridge；
- 需要把LA64 mandatory closure改成可选、把external proof替换为local/host proof，或把Not Run写成PASS；
- Ready/Active manifest需要越界且尚无批准。

若issue追查后只剩Safe级实现偏好，不继续为“更通用”而扩展设计；按当前Ready gate执行并把未来可能性留给
真实consumer或后续RFC。

## 12. 实现期反馈记录

- `2026-07-29`：Checkpoint 0A / Execution Fact。真实UDP path确认naive shared-engine topology缺少
  selected-interface admission seam；wrong-interface-first可消费queue并通过错误provider egress。结论保持R0，
  不是production baseline defect；0A characterization保留到0B正确路径形成时删除或改写。没有Route Correction、
  Target Renegotiation、Contract Impact、tracking issue或register写回；精确source/review/validation证据见
  [transaction](../../devlog/transactions/2026-07-29-net-udp.md#2026-07-29---checkpoint-0a-implementation-source-audit-and-closure)。
- `2026-07-29`：Checkpoint 0B / Positive Execution Fact。Stack-private aggregate Endpoint为每个interface维护
  private engine projection，但binding、admission、TX phase、receive order gate与retire只有一个owner；显式
  selection阻止wrong-interface dequeue，bounded IP-medium local port经normal ingress交付。结论保持R0且不修改
  shared/vendored/kernel/current contract。0C仍须独立决定candidate/facade/test的长期保留与cleanup，并在final
  source上重跑Stage 0 branch/common gates；精确证据见
  [transaction](../../devlog/transactions/2026-07-29-net-udp.md#2026-07-29---checkpoint-0b-implementation-review-validation-and-closure)。

## 13. Target Renegotiation Gates

当前没有proposed gate。只有真实接口、代码、测试或集成证据表明R0 target代价/可行性需要重新判断时才增加。
提案不等于批准；决定前当前stage保持停止且不得cut over。correctness invariant不能作为reduced target妥协项。

## 14. Write Set扩展记录

当前没有扩展。只记录Ready/Active Stage 0 manifest冻结后的批准扩展；future Outline的范围调整不记为扩展。

## 15. 结构维护记录

当前没有已执行的结构维护。Stage 0允许的same-owner private module boundary已在7.9与7.11预先解析；实际拆分、
验证与保留/删除结论由transaction记录。
