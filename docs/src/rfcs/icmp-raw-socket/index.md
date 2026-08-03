# RFC-20260803-icmp-raw-socket

**状态：** Accepted for Implementation / Checkpoint 1 Closed after Feedback Interlude / Checkpoint 2 Not Active
**修订：** R0
**负责人：** doruche
**最后更新：** 2026-08-03
**领域：** fs / Socket / IPv4 / ICMP / network Stack / iomux / epoll
**影响契约：** Refine `SOCKET-FRONT-001`、`SOCKET-ABI-001`、`NET-PROTOCOL-BOUNDARY-001`、
`NET-SOCKET-WAIT-001`；Introduce `NET-ICMP-RAW-INGRESS-001`、`NET-ICMP-RAW-ENDPOINT-001`、
`NET-ICMP-RAW-TRANSACTION-001`
**执行记录：** Git / PR；Checkpoint 1反馈间章已关闭且未创建transaction；Checkpoint 2与contract cutover仍未授权

## 文档状态

本文是 IPv4 ICMP raw Socket 的R0 accepted target，也是该提案唯一的 canonical target source。它不描述当前已经存在的
能力；当前 effective 规则仍以 [Socket contract](../../contracts/socket/index.md)、
[Network contract](../../contracts/net/index.md)、live source 与 register 为准。

R0固定用户可见能力包络、owner/handoff、failure/cleanup、correctness invariants、Contract Impact 与
acceptance。Linux 6.6.32 的逐项 errno、optlen、copy ordering 和重复状态转换矩阵属于本 target 的 ABI conformance
surface，可以在 review、实现和 focused oracle 中继续精化；它们不再作为开始撰写公共 Draft 的前置条件。若精化结果
要求改变本文的能力、owner、failure/cleanup、ABI policy、acceptance 或验证强度，则必须回到 RFC review 或 Target
Renegotiation，不能在实现中静默漂移。

本 RFC 因 ingress、fanout、packet ownership 与 opened-description lifecycle 存在非平凡 proof obligations，保留一份
[目标与不变量](./invariants.md)。实施准备进一步确认post-admission packet seam值得在完整UAPI接入前独立review，因此
新增一份[实施路线](./implementation.md)，采用一个implementation stage、两个checkpoint与唯一
`ICMP-RAW-CUTOVER`：第一个checkpoint只关闭syscall不可达的protocol owner/packet path，第二个checkpoint接入Socket、
完成产品验收并cutover。当前没有独立probe、transitional contract、多个cutover、tracking page或transaction；本轮在
Checkpoint 1 closure后追加一次反馈间章，仍必须停止，不得自动进入Checkpoint 2或`ICMP-RAW-CUTOVER`。

## 摘要

本 RFC 提议交付自然闭合的首版 IPv4 ICMP raw datagram Socket：具备 `CAP_NET_RAW` 的进程可以创建
`socket(AF_INET, SOCK_RAW, IPPROTO_ICMP)`，使用 bind/connect、send/receive、read/write、blocking/nonblocking、
poll/select/epoll，以及 `IP_TTL`、`IP_TOS` 与 `ICMP_FILTER` 完成日常 ICMP 工具所需的完整路径。RV64 与 LA64 上普通
BusyBox `ping 10.0.2.2` 是 mandatory acceptance floor，但 target 不被压缩成只够一个 ping 命令偶然工作的 syscall
子集。

该能力复用 current general Socket front、initial-domain IPv4 control plane、opened-description lifecycle 和
iomux/epoll wait protocol。kernel Socket owner独占 Linux ABI、权限与等待政策；control plane独占 route/source/interface
选择；domain Stack raw owner独占 Endpoint identity、association/filter、bounded storage、fanout、transaction commit与
retire；interface/IP owner独占 local-destination admission。实现不得把这些事实复制到 common Socket，也不得把
smoltcp private handle、queue 或 packet representation暴露给kernel。

R0只覆盖unicast、未分片 IPv4 ICMP。TX由用户提供ICMP message，kernel/Stack形成IPv4 header；RX向用户保留本地
admitted datagram在IPv4 `total_len`内的原始完整字节。`IP_HDRINCL`、任意IP protocol、broadcast/multicast、
`sendmsg/recvmsg`、ancillary data、error queue与production Echo responder保持非目标。

## 背景与 Current Baseline

[Socket Front、ABI 与 Wait contract](../../contracts/socket/front-abi-wait.md) 已经建立 static descriptor、opaque
family storage、Linux ABI containment、operation-owned predicate、shared wait/recheck 与 opened-description final
release。当前真实 consumer 是 IPv4 UDP 与 Unix stream；raw family应作为第三个真实 consumer扩展共同 surface，而不是
旁路为第二套 FileOps、wait loop 或 concrete-type downcast。

[IPv4 Control Plane contract](../../contracts/net/control-plane.md) 唯一拥有当前 initial domain 的 route、source address
与interface selection。[UDP Socket contract](../../contracts/net/udp-socket.md) 已经证明 kernel Socket 与 domain Stack
Endpoint 之间可以只交换 opaque identity、typed request/outcome、operation-local selection、point-in-time facts 与
invalidation hint。ICMP raw复用这些共同义务，但不复制UDP port namespace、datagram transaction或engine topology。

选择在TCP之前交付ICMP raw，是为了用一条边界较窄但真实经过Socket、protocol Stack、frame path与用户工具的vertical
slice，验证general Socket front的第三个异构consumer和UDP之后第二个Network Stack Endpoint family。它可以暴露并收敛
当前framework已经被真实raw consumer触发的问题，但不证明TCP的connect/listen/accept、重传、拥塞、pending error、
half-close、orphan close或协议时序已经准备完成，也不授权提前建立TCP operation surface或connection state。

vendored smoltcp已经启用`socket-raw`，但尚无production raw Endpoint。live ingress在完成IPv4 parse后先调用
`raw_socket_filter()`，之后才执行local-destination admission；raw receive又从`IpRepr`重新emit header，不能保留TOS、
ID、flags与IP option等原始字节。这一机制不能直接成为Linux raw Socket backend：R0必须把raw observation放到
local-destination admission之后，并从原始未分片IPv4 datagram建立detached delivery；具体 seam、object graph 与是否
复用smoltcp raw socket由实现决定。

Linux 6.6.32只作为 UAPI、errno 与 observable behavior 的固定参考，不决定 Anemone 内部对象图：

- `xref:linux-6.6.32:net/ipv4/af_inet.c#inet_create`：`SOCK_RAW`与`CAP_NET_RAW` admission；
- `xref:linux-6.6.32:net/ipv4/raw.c#raw_bind`、
  `xref:linux-6.6.32:net/ipv4/af_inet.c#inet_dgram_connect` 与
  `xref:linux-6.6.32:net/ipv4/af_inet.c#inet_getname`：association与address projection；
- `xref:linux-6.6.32:net/ipv4/raw.c#raw_sendmsg`、
  `xref:linux-6.6.32:net/ipv4/raw.c#raw_recvmsg` 与
  `xref:linux-6.6.32:net/ipv4/raw.c#raw_local_deliver`：TX/RX shape、copy与delivery；
- `xref:linux-6.6.32:net/ipv4/raw.c#raw_setsockopt`、
  `xref:linux-6.6.32:net/ipv4/raw.c#raw_getsockopt`、
  `xref:linux-6.6.32:net/ipv4/ip_sockglue.c#ip_setsockopt` 与
  `xref:linux-6.6.32:net/ipv4/ip_sockglue.c#ip_getsockopt`：`ICMP_FILTER`与IPv4 options。

当前tracked `socket-test`没有ICMP raw suite，curated LTP registry也不能证明本target。提升前调查到的raw相关stock case
要么依赖本RFC明确defer的`IP_HDRINCL`、message ABI或network namespace，要么依赖独立remote-host topology，或把尚未
接受的Socket family混入同一不可拆matrix。因此R0需要新增target-complete focused suite；curated `socket` LTP group只
承担真实libc与既有Socket family regression，不能代替raw-specific proof或真实packet round trip。

## 四层架构定位与 Object Fence

本RFC继续扩展现有四层network model，不建立第五个raw专用架构层，也不把四层理解为依次完成的implementation stage。
`anemone-net-api`是kernel与concrete Stack共同依赖的semantic vocabulary，不是runtime中介；netdev、provider与driver
继续位于正交的frame plane，不因raw consumer存在而取得protocol Endpoint或Socket语义。

| Layer | 本 RFC 中的角色 | 不得取得或拥有 |
| --- | --- | --- |
| `anemone-kernel::net`与kernel Socket owner | Linux ABI/credential boundary、static descriptor与family orchestration、TTL/TOS snapshot、blocking/readiness解释、control-plane selection与opened-description integration | smoltcp handle/`SocketSet`、private packet queue、raw association/filter truth、protocol engine state或driver frame identity |
| `anemone-net-api` | kernel与concrete Stack确实共同需要的protocol-domain value、opaque identity、typed request/outcome、point-in-time facts与invalidation vocabulary | registry、lock、runtime owner state、fd/task/waiter、Linux errno/user pointer或smoltcp object identity |
| `anemone-smoltcp-stack` | concrete protocol resource root；拥有raw Endpoint identity namespace、lifecycle、bounded storage、fanout/drop、private engine mapping、operation commit与invalidation | fd/task/credential/user pointer、Linux wait/readiness policy或ABI representation |
| `anemone-smoltcp` | IPv4 parse/validation/emit、interface-local destination admission、普通ICMP processing与protocol progression；若复用raw/icmp mechanism，它仍保持Stack-private | Anemone Endpoint/kernel Socket identity、control-plane policy、opened-description或wait lifecycle |

四层依赖与object fence不要求backend形式可替换，也不要求先建立generic trait hierarchy。ordinary Socket operation不能
取得concrete Stack或smoltcp object，concrete Stack不能反向取得kernel object，API value不能沉淀为第二个runtime
owner。trait、`pub(crate)`或opaque type本身都不自动构成fence；边界必须由调用者实际只能取得的窄capability证明。

这里的`raw`分为两层语义：Linux `AF_INET + SOCK_RAW + IPPROTO_ICMP`的tuple、权限、flag、sockaddr、errno与option
policy只属于Socket ABI/credential boundary；Stack-facing raw ICMP Endpoint则是non-exclusive observation、bounded
fanout、association/filter、TX admission与detached RX packet的protocol-domain能力。`SOCK_RAW`数值、`CAP_NET_RAW`、
fd、user pointer、Linux wait政策或`IP_HDRINCL`状态不得下推到`anemone-net-api`或concrete Stack。

raw Endpoint identity、operation authority与semantic lifetime保持分离。opaque identity只服务owner lookup与stale
isolation，不是handle，不证明liveness，也不延长resource lifetime；kernel raw family只取得role-scoped、non-blocking
operation capability，不能拆出concrete Stack、private handle或queue；creation rollback与opened-description final
release分别拥有publication前后唯一的lifecycle transition，capability clone、token、临时引用与Rust `Drop`都不能成为
retire truth。

## 目标

- 交付 `AF_INET + SOCK_RAW + IPPROTO_ICMP` 的单一、静态 semantic type；创建时检查current task effective
  `CAP_NET_RAW`，缺失能力稳定拒绝且不发布fd或Endpoint。
- 支持 `SOCK_NONBLOCK`、`SOCK_CLOEXEC`、bind/connect/disconnect、local/peer address query、destination override，
  以及与这些association一致的RX local/peer filter。
- 支持 `sendto/recvfrom`及其`send/recv`调用形状、普通`read/write`、zero-length、short receive、copy fault和
  Linux-compatible flag语义。
- 支持default blocking、`O_NONBLOCK`/`SOCK_NONBLOCK`、`MSG_DONTWAIT`以及poll/select/epoll；readable、writable与
  wait route均由raw owner current facts投影，不缓存ready truth。
- 支持 `IP_TTL`、`IP_TOS` 与 `ICMP_FILTER` 的读写，并让每次send使用operation-local immutable option snapshot。
- TX只接收用户ICMP message，由control plane选择route/source/interface，由Stack raw owner形成无IP option、非分片
  IPv4 header并在返回success前完成bounded admission commit；ICMP checksum由用户形成。
- RX只观察已经通过对应interface local-destination admission的unicast、未分片IPv4 ICMP packet；向用户保留IPv4
  `total_len`内的原始header、IP options与ICMP message，不从high-level representation近似重建。
- 为每个matching live Endpoint建立独立计费、有界且detached的delivery ownership；一个consumer满载只丢自己的
  delivery，不能抑制普通ICMP processing或影响其它Socket/protocol consumer。
- 复用current general Socket、opened-description与iomux/epoll协议，以双架构focused ABI、curated Socket LTP regression
  和真实QEMU router ping共同完成acceptance。

## 非目标

- 不支持任意IP protocol、dynamic raw descriptor、protocol registry、IPv6 raw Socket或packet socket。
- 不支持 `IP_HDRINCL`、用户自带IPv4 header、outbound IP options、隐式fragmentation、PMTU/error queue或
  `SO_ERROR` pending-error语义。
- 不支持broadcast、multicast、`SO_BROADCAST`、`IP_MULTICAST_IF`、`SO_BINDTODEVICE`或interface-name lookup；因此
  `ping -I`、broadcast/multicast ping不属于R0。
- 不支持 `sendmsg/recvmsg`、`sendmmsg/recvmmsg`、cmsg/ancillary data；这些属于future common Socket message ABI，
  不建立raw-only半套入口。
- 不支持`SO_RCVBUF`、`socketpair`、listen/accept或stream shutdown语义；connected raw不会取得peer-close、half-close、
  HUP/RDHUP或connection error状态机。
- 不引入production ICMP Echo responder；raw Socket只观察/产生packet，不能据此声明peer能够ping Anemone。
- 不承诺公网、DNS、physical hardware、`smp > 1`、其它NIC或full network LTP；这些未运行时保持独立Not Run。

## 用户可见能力边界

### 创建、地址与权限

唯一成功tuple为`socket(AF_INET, SOCK_RAW | flags, IPPROTO_ICMP)`，其中`flags`只允许`SOCK_NONBLOCK`与
`SOCK_CLOEXEC`。resolver把它归一为immutable ICMP raw semantic type，不在backend保存任意protocol number。
current task effective capability不含`CAP_NET_RAW`时返回`EPERM`；所有创建失败都发生在fd publication前，并撤销本次
raw registration、observer route与Endpoint resource。

`bind`、`connect`、`connect(AF_UNSPEC)`、`getsockname`与`getpeername`采用Linux 6.6.32兼容的IPv4 raw Socket行为。
bind约束local source/filter，connect设置default peer/filter但不建立connection lifecycle；sendto的显式destination可以
覆盖default peer。重复bind/connect、disconnect后source snapshot与`sockaddr_in.sin_port`的逐项errno/projection由
固定Linux oracle和focused tests收敛，不另建association truth。

### 发送

用户buffer是ICMP message，不含IPv4 header。成功send使用当前destination、control-plane selection与本次TTL/TOS
snapshot形成IPv4 packet；header不含IP options且不分片。用户负责ICMP checksum，kernel不因checksum/type/body内容
把message解释为自身控制状态。message超过selected interface MTU减IPv4 header时返回`EMSGSIZE`。

`MSG_DONTWAIT`与opened-description nonblocking共同决定本次是否等待；`MSG_NOSIGNAL`被接受，因为本target没有
`SIGPIPE` producer，但实现必须用关键注释和低噪声diagnostic保留这一兼容取舍。其它send flags明确拒绝。zero-length
ICMP message仍是一个真实packet transaction；success只在user copy、route/source/size/capacity检查与Endpoint admission
commit完成后返回，commit后的普通provider loss不反向改写结果。

### 接收

`recvfrom`返回detached完整IPv4 packet的prefix，并把peer投影为packet source address；raw Socket address中的port按
Linux oracle投影。short buffer通常返回已复制prefix并消费整个packet；`MSG_TRUNC`返回packet完整长度。
`MSG_PEEK`在成功、short copy和copy fault后均不消费。zero-length receive仍等待并观察一个packet：non-peek成功返回0
并消费，带`MSG_TRUNC`时返回完整长度。non-peek copy fault可以丢弃已经detach的packet，但不得requeue、重排或部分
恢复readiness。

invalid IPv4 header/checksum不交付。通过IP admission后的ICMP checksum、type与body保持raw-visible；普通ICMP处理可以
独立接受或拒绝，raw observation不能以`handled`抑制它。R0不向raw Socket交付IPv4 fragment，也不承担reassembly。

### Socket options 与 query

R0成功支持：

- `SOL_SOCKET`：`SO_DOMAIN`、`SO_TYPE`、`SO_PROTOCOL`、`SO_ACCEPTCONN`；
- `IPPROTO_IP`：`IP_TTL`、`IP_TOS`；
- `SOL_RAW`：`ICMP_FILTER`。

`IP_TTL`默认值由明确配置拥有，R0默认64；`IP_TOS`默认0。`ICMP_FILTER`按ICMP type bit过滤并在per-Endpoint计费与queue
admission前生效。exact optlen、值域、短copy/copyout与并发snapshot矩阵遵循固定Linux 6.6.32 oracle，并由focused ABI
tests证明；这不会授权通用mutable option bag。

`SO_RCVBUF`、`SO_BROADCAST`、`SO_BINDTODEVICE`、`IP_MULTICAST_IF`、`IP_HDRINCL`、`SO_ERROR`及其它未选择option返回
`ENOPROTOOPT`，不得因为某个用户程序忽略错误而success-no-op。

### Readiness

readable只由当前RX queue非空事实产生。writable表示live raw owner当前可以接受一次R0范围内的最小TX admission；它
不要求Socket已经connect，不缓存route/source，也不保证任意destination、length或request-specific selection成功。
RX/TX fact变化由owner先commit，再在owner guard外发布non-owning invalidation hint；blocking、poll/select与epoll均
执行snapshot/register/recheck/final-scan。

raw Socket没有peer terminal relation、half-close或pending error producer，因此不会因connect获得HUP/RDHUP/ERR状态机。
这不削弱iomux/epoll对其它真实source的mandatory HUP/ERR政策。

## Owner、Handoff 与 Cleanup

| Fact / capability | 唯一 Owner | 跨 owner handoff |
| --- | --- | --- |
| Linux tuple、flags、sockaddr、copy、errno、sockopt与`CAP_NET_RAW` admission | Socket ABI/credential adapter | normalized request/outcome/value |
| immutable ICMP raw semantic type与common File association | static Socket descriptor/front | opaque family-private storage |
| blocking choice、Linux readiness projection、TTL/TOS policy与final-release integration | kernel ICMP raw Socket family | current facts、operation-local policy与recheck capability |
| route、source address与interface selection | initial-domain IPv4 control plane | operation-local immutable selection |
| interface-local IPv4 destination admission | concrete interface/IP owner | admitted raw packet observation |
| Endpoint identity/lifecycle、local/peer/ICMP filter、bounded storage、fanout/drop与retire | domain Stack raw owner | narrow non-blocking operation capability |
| TX IPv4 header formation与RX original packet delivery | domain Stack raw owner | ICMP message + selection + policy / detached IPv4 packet |
| smoltcp handle、`SocketSet`、buffer与private packet representation（若使用） | concrete Stack owner | 不跨crate暴露 |
| NIC queue、DMA、IRQ与frame backing | netdev/provider/driver owner | callback-scoped frame capability |
| fd publication与semantic final release trigger | opened-description lifecycle owner | unpublished creation guard / static final-release hook |

kernel与Stack之间只交换ICMP-scope raw capability、opaque identity、normalized value/request、typed outcome、
point-in-time facts与invalidation hint。`anemone-net-api`可以承载双方确实共同需要的语义，但不能拥有runtime registry、
lock、queue、fd/task/waiter、Linux errno或smoltcp identity。

创建失败由unpublished preparation回滚；publication后只由opened-description final release撤销source publication并
移交Endpoint retire。关闭dup/fork的非最后alias、Rust `Drop`、raw fd number或临时引用都不能推进semantic retire。
late/repeated notification与stale opaque identity必须fail closed，不能恢复association或命中新generation。

bind/connect与send在owner commit前失败不得部分改变既有association或交付packet。receive在Endpoint detach前由
Endpoint独占delivery，detach后由operation-local kernel transaction独占；retire、copy fault与concurrent receive不得
制造双重owner。精确prepare、copy、lock和notification表示由实现选择。

## Contract Impact

所有变化都只在最终`ICMP-RAW-CUTOVER`满足acceptance后写入current contract；Draft或部分实现不会提前生效。

| Contract ID | 变化 | 当前规则 | Target 摘要 | Cutover |
| --- | --- | --- | --- | --- |
| `SOCKET-FRONT-001` | Refine | [Active](../../contracts/socket/front-abi-wait.md#socket-front-001--general-front只拥有共同外壳) | 将ICMP raw加入第三个真实consumer，收敛family-neutral datagram/file-I/O与option dispatch；不建立registry、downcast或第二FileOps | `ICMP-RAW-CUTOVER` |
| `SOCKET-ABI-001` | Refine | [Active](../../contracts/socket/front-abi-wait.md#socket-abi-001--linux-abi止于family-neutral-adapter) | 增加raw tuple、权限、IPv4 sockaddr、flags、selected sockopts与datagram copy/consume投影，Linux representation仍止于adapter | `ICMP-RAW-CUTOVER` |
| `NET-PROTOCOL-BOUNDARY-001` | Refine | [Active](../../contracts/net/udp-socket.md#net-protocol-boundary-001--cross-owner-udp-capability保持窄且非阻塞) | 从UDP单一consumer扩展为UDP与ICMP raw共同证明的窄、非阻塞protocol capability；不外推generic Stack/Endpoint framework | `ICMP-RAW-CUTOVER` |
| `NET-SOCKET-WAIT-001` | Refine | [Active](../../contracts/net/udp-socket.md#net-socket-wait-001--protocol-factwake与linux-readiness保持分离) | 让UDP与ICMP raw各自读取owner predicate并共享fact/invalidation协议；不共享ready truth | `ICMP-RAW-CUTOVER` |
| `NET-ICMP-RAW-INGRESS-001` | Introduce | None | local-destination admission先于raw fanout，保留原始未分片IPv4字节且不抑制普通ICMP处理 | `ICMP-RAW-CUTOVER` |
| `NET-ICMP-RAW-ENDPOINT-001` | Introduce | None | raw Endpoint identity、association/filter、independent bounded fanout、lifecycle/retire与stale isolation | `ICMP-RAW-CUTOVER` |
| `NET-ICMP-RAW-TRANSACTION-001` | Introduce | None | non-`IP_HDRINCL` TX header/admission commit、detached RX consume、option snapshot、capacity与error boundary | `ICMP-RAW-CUTOVER` |

### Dependencies

- [`SOCKET-WAIT-001`](../../contracts/socket/front-abi-wait.md#socket-wait-001--operation-predicate由各自owner定义)：
  common Socket继续只拥有blocking choice与shared wait/recheck协议。
- [`NET-CONTROL-PLANE-001`](../../contracts/net/control-plane.md#net-control-plane-001--initial-domain唯一决定ipv4-routesourceinterface)：
  route/source/interface selection保持唯一owner。
- [`NET-BOUNDARY-001`、`NET-FRAME-OWN-001`、`NET-FRAME-PROGRESS-001`与`NET-STACK-PUMP-001`](../../contracts/net/frame-path.md)：
  object fence、frame ownership、bounded backpressure与Stack progression保持有效。
- [`OPENED-DESC-001..003`](../../contracts/task/opened-description-lifecycle.md)：fd publication、dup/fork sharing与
  static final-release hook保持有效。
- [`IOMUX-POLL-001..003`](../../contracts/iomux/poll-wait.md)与
  [`EPOLL-WATCH-001`、`EPOLL-READY-001`、`EPOLL-FILE-001`](../../contracts/epoll/protocol.md)：raw source作为普通
  source接入，不改变consumer protocol。

## Implementation Boundary

本文R0已经接受，本轮只在取得单独授权的Checkpoint 1内按以下边界实施：

- **允许改变：** Socket resolver/ABI/front中ICMP raw真实consumer所需的最窄surface；kernel raw family；shared
  protocol vocabulary；domain Stack raw owner；local-admission后的raw observation seam；Kconfig资源参数；focused
  tests、curated Socket LTP group与QEMU router ping入口。
- **必须保持：** user-visible capability/non-goals、`CAP_NET_RAW` boundary、control-plane selection owner、
  interface-local destination owner、Stack Endpoint/fanout owner、opened-description final release、operation-specific
  readiness、original RX bytes、ordinary ICMP/UDP/Unix/iomux/epoll行为与mandatory validation floor。
- **实现偏好：** concrete type、trait/enum/function family、opaque handle、Endpoint/engine topology、copy或shared immutable
  backing、lock、queue、buffer、helper、module/file布局及是否复用smoltcp raw socket；只要保持上述语义，可自然决定。
- **自然闭合：** owner-local import/re-export、module registration、同owner新文件、定向测试和行为保持拆分不需要逐文件
  gate；important capacity与default policy由Kconfig或既有配置owner拥有，不以内嵌magic number固化。

出现以下任一事实必须在完成声明或cutover前停止并回到RFC review / Target Renegotiation：

- 需要任意IP protocol、`IP_HDRINCL`、broadcast/multicast、message ABI或其它non-goal才能形成有用能力；
- 无法在local-destination admission后保留原始IPv4字节，除非复制destination truth、延长frame owner lifetime或暴露
  private object；
- 需要移动route/source/interface、destination admission、association/filter、packet queue或readiness truth的owner；
- 需要扩大shared public API、建立第二份Socket/FileOps/wait loop、generic registry/manager或无真实consumer抽象；
- 需要改变Linux-visible failure/copy/consume政策、能力包络、Contract Impact、acceptance或降低双架构验证claim；
- 实现暴露当前路线之外的真实probe、多cutover或不安全中间态，需要先修订`implementation.md`并明确exit condition。

工程证据可以提出适当扩展、Route Correction、Accepted Reduced Target、follow-up RFC或Not Cut Over，但correctness
invariant不能作为工程妥协项，新边界被review接受前不能把较弱实现写成R0完成。

## Acceptance 与 Validation

### Owner-local / host proof

- resolver、`CAP_NET_RAW`允许/拒绝、fd publication rollback与static type witness；
- bind/connect/disconnect、query、destination override、RX filter、失败原子性与single association truth；
- create/retire、multiple matching consumer fanout、ICMP filter、final release、late notification与stale isolation；
- local-admission-before-raw-delivery、original IPv4 byte fidelity、ordinary ICMP coexistence与fragment exclusion；
- bounded RX/TX capacity、drop isolation、recovery、pump budget与shutdown；
- TX copy/header/selection/admission commit、TTL/TOS snapshot、zero-length与MTU rejection；RX zero/short/
  `MSG_TRUNC`/peek/copy-fault/consume；
- blocking/nonblocking、snapshot/register/recheck/final-scan、poll/select/epoll、multi-waiter cancellation；
- UDP、Unix Socket、iomux/epoll与network shutdown regression。

### 双架构 focused guest ABI

`socket-test`新增明确限定为ICMP raw的suite，在RV64与LA64 guest中验证：

- create/permission/flags与fd lifecycle；
- address transition/query/destination override的Linux-compatible errno与sockaddr矩阵；
- `SO_DOMAIN/SO_TYPE/SO_PROTOCOL/SO_ACCEPTCONN`、`IP_TTL/IP_TOS/ICMP_FILTER`与unsupported options；
- send/receive/read/write的zero/short/invalid pointer/copy fault/message size/flag矩阵；
- blocking/nonblocking、poll/select/epoll以及dup/fork/CLOEXEC/final close。

exact case table是该target的validation asset，不需要在RFC正文复制；oracle若暴露target-level差异，按停止条件处理。

### Curated Socket LTP regression

tracked test infrastructure建立语义名为`socket`的curated LTP group，作为common Socket ABI与既有UDP/Unix family的
mandatory regression floor，而不是raw-specific proof。case按完整stock binary matrix准入，不能修改上游case、降低
errno oracle、只运行有利subcase或重复计分。初始候选`socketpair02`、`bind03`与`listen01`必须在实施时重新审计并在
RV64/LA64、glibc/musl root中实际分类；`TCONF`不计作能力覆盖。

### 真实 ping

RV64与LA64当前QEMU user-mode network均必须由现有runner成功执行普通BusyBox `ping 10.0.2.2`，证明真实raw Socket、
IPv4、VirtIO、external Stack path与QEMU router round trip。focused oracle另外证明`IP_TTL`实际改变header；ping成功
不能替代权限、copy、readiness、lifecycle或sockopt matrix。

公网数值IPv4、hostname/DNS、peer ping Anemone、physical hardware、`smp > 1`、其它NIC、full network LTP与final
harness均为独立optional claim；未运行时记录Not Run，不阻塞R0，也不能由build、host test或另一架构替代。

## 风险与反馈

- Checkpoint 1 closure后的审查曾发现post-admission policy owner错位：generic smoltcp raw承担了ICMP raw的
  unicast/unfragmented policy，并丢失其既有fragment reassembly语义。反馈间章已用callback-scoped admitted packet与
  interface-owned destination snapshot修复：interface/IP只决定local admission，Stack ICMP raw决定R0 policy，ordinary
  ICMP与generic raw语义各自保持。细节与证据见[实施路线](./implementation.md#post-closure-review-hold-与反馈间章)。
- raw Endpoint topology、copy/shared immutable backing与smoltcp raw/icmp mechanism仍保持开放。选择必须证明per-Endpoint
  charge、detach时刻、retire isolation与bounded progression，而不是证明某个候选类型“能跑”。
- 本RFC的重要目的之一是让ICMP raw作为第三个异构consumer向current general Socket framework提供真实架构反馈。
  common read/write当前带有stream-shaped request；若raw operation暴露的是Socket front/ABI/FileOps orchestration、
  static dispatch或shared wait/recheck的自然职责，应优先在正确common owner内收敛最窄family-neutral形状，而不是在raw
  family内部增加translation、downcast、第二FileOps、旁路wait或重复状态。该偏好不授权上收family truth、预建future
  TCP/message/registry surface，或越过本文已列Contract Impact与停止条件。
- [实施路线](./implementation.md)只用一个stage和两个真实checkpoint隔离post-admission protocol proof与最终Socket/
  contract cutover，不把普通commit顺序升级为gate。当前没有独立probe；若Checkpoint 1暴露必须先验证的高风险假设，
  应先修订该页写明Hypothesis、Protected Boundary、Failure Signal、Write-back与Exit，probe不自动沉淀为production API。

## 文档与证据

- [目标与不变量](./invariants.md)
- [实施路线](./implementation.md)
- [背景材料索引](./backgrounds/index.md)
- [RFC 前定位共识](./backgrounds/positionings.md)：已冻结的决策来路，不覆盖本文target
- current contracts：[Socket](../../contracts/socket/index.md)、[Network](../../contracts/net/index.md)、
  [Opened-description](../../contracts/task/opened-description-lifecycle.md)、
  [Poll wait](../../contracts/iomux/poll-wait.md)、[Epoll](../../contracts/epoll/protocol.md)
- external source registry：[公共引用规则](../../external-source-references.md)与`xref:linux-6.6.32`
- commit / PR：Checkpoint 1 execution与反馈间章；optional transaction：None

## 修订记录

| 修订 | 日期 | 状态 | 语义变化 | Review / 执行 |
| --- | --- | --- | --- | --- |
| R0 | 2026-08-03 | Accepted for Implementation | 初始accepted target：IPv4 ICMP-only raw Socket、明确owner/handoff、post-admission original-byte fanout、bounded Endpoint transaction与完整产品验收边界 | Checkpoint 1 closure后曾因owner偏差进入Review Hold；反馈间章完成R0-preserving Route Correction并通过独立复核，不递增修订 |

## Closure

R0已经接受；Checkpoint 1反馈间章已完成syscall不可达protocol owner与packet-path的Route Correction，独立复核无
Apollyon、Keter或有证据的Euclid，Review Hold已经释放。guest syscall、architecture runtime、LTP与ping均保持Not Run，
尚无contract cutover或current limitation变化。Checkpoint 2保持Not Active并等待单独授权。
