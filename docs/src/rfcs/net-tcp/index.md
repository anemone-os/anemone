# RFC-20260805-net-tcp

**状态：** Accepted / Stage 1 Closed / Stage 2 Ready / TCP Not Effective
**修订：** R0
**负责人：** doruche
**最后更新：** 2026-08-05
**领域：** network / socket / TCP / userspace ABI
**影响契约：** R0仍Pending Introduce `NET-TCP-ENDPOINT-001`、`NET-TCP-STREAM-001`、
`NET-TCP-LIFECYCLE-001`并Refine `SOCKET-ABI-001`；Stage 1已Refine `NET-CONTROL-PLANE-001`与
`NET-STACK-PUMP-001`
**执行记录：** P0 Positive / Closed；Stage 1 Closed；Stage 2 Resolved / Ready / Not Authorized；
`NET-PROTOCOL-PROGRESSION-CUTOVER` Effective；transaction None

## 文档状态

本文是 IPv4 TCP Socket capability 与现有网络架构封顶验证的公共 R0，也是本提案
唯一 canonical target source。本文固定已经接受的 target、non-goals、owner/handoff、
failure/cleanup、ABI、Contract Impact、acceptance 与 validation boundary；它不覆盖
[Network current contracts](../../contracts/net/index.md)或
[Socket current contracts](../../contracts/socket/index.md)，也不因发布实施计划而授权执行probe、
Stage 1 implementation、后续stage、checkpoint或contract cutover；各gate仍以自己的独立授权为准。

本 RFC 的规范性页面是正文、[目标与不变量](./invariants.md)与
[实施计划](./implementation.md)；公共 R0 形成前的
[定位共识](./backgrounds/positionings.md)仅作为冻结历史背景，不再维护。具体类型、模块、锁、
buffer、smoltcp mapping、progression effect/wake表示、worker拓扑和验证命令仍属于实施选择。
当前实施计划已经关闭kernel外crate-only TCP engine Probe Gate与Stage 1 execution gate，并把Stage 2解析为两个
分别授权的execution checkpoint。Stage 2先建立kernel窄TCP owner capability，再接入syscall-unreachable的general
Socket front；本Stage不注册TCP creation tuple、不发布handler、fd或部分UAPI，也不执行contract cutover。Stage 3--5
仍为只含目的、依赖、受保护边界与解析触发点的Outline，不创建tracking page或transaction。解析Stage 2不授权
CKPT 2A、CKPT 2B或后续Stage。

## 摘要

Anemone 已经拥有 boot-time IPv4 control plane、bounded frame/Stack progression、IPv4 UDP、
ICMP raw、general Socket front、Unix stream/seqpacket、opened-description final release 与
poll/select/epoll wait/recheck。当前尚无 kernel TCP Socket capability，production Stack 也
没有发布 TCP protocol resource。

本 RFC 提议交付一组普通用户程序可消费的 initial-domain IPv4 TCP 字节流能力：主动和
被动连接、blocking/nonblocking connect、accept、partial stream I/O、half-close、真实
asynchronous result/`SO_ERROR`、owner-defined readiness、bounded resource、pump 外 mutation 的
progression handoff 与 non-blocking final-release handoff。TCP事实留在Stack-side TCP owner；UDP、ICMP raw
与TCP各自的Stack-side protocol owner判断自己的committed mutation是否产生progression obligation，并经同一
owner-driven handoff交给既有worker。Linux ABI、fd、blocking、signal、errno与publication rollback仍留在
kernel Socket owner。

TCP 同时作为当前共同网络与Socket架构的封顶及反馈consumer。它不仅要证明现有共同边界可以承载TCP，
也要把TCP暴露出的自然shared Socket责任反馈回正确owner；不得为了维持当前框架形状，把本应共同拥有的
能力硬塞进TCP owner或family-local adapter。RFC只有在TCP capability与架构封顶两项
closure claim 同时成立时才能关闭：既不能用一个成功 HTTP 路径代替完整 TCP target，也不能
在 TCP 旁边建立第二套 Socket、wait、control-plane、frame-path 或 protocol progression
架构后仍宣称共同边界稳定。

## Current Baseline

当前 effective baseline 明确不覆盖 TCP：

- [Network Protocol Socket](../../contracts/net/protocol-socket.md)已经规定 cross-owner protocol
  capability 保持窄且非阻塞，Stack owner 独占 Endpoint/private engine，invalidation 只提示
  recheck；当前 consumer 是 UDP 与 ICMP raw。
- [Socket Front、ABI 与 Wait](../../contracts/socket/front-abi-wait.md)已经规定 immutable static
  descriptor、family-private envelope、Linux ABI containment 与各 operation 读取各自
  owner-defined predicate；当前 `SO_ERROR`、pending error 与 TCP 不在 effective surface。
- [IPv4 control plane](../../contracts/net/control-plane.md)唯一决定 local、connected-prefix 与
  default route/source/interface selection；[frame path](../../contracts/net/frame-path.md)唯一拥有
  bounded provider handoff 与 Stack progression。
- [Opened-description lifecycle](../../contracts/task/opened-description-lifecycle.md)以最后一个
  published fd slot 的 `Live(1) -> Retired`作为 semantic final release，并提供创建时固定的
  single final-release hook。
- [IOMUX poll wait](../../contracts/iomux/poll-wait.md)与
  [Epoll protocol](../../contracts/epoll/protocol.md)已经固定 snapshot/register/recheck、
  cancellation 与 final harvest；family source 只提供 current predicate 与 recheck route。

live source已有可表达byte stream、connect/accept、shutdown与family option dispatch的general Socket capability；
production Stack现已启用private TCP protocol resource，并以one-shot Reset/Timeout cause、bounded listener/engine、
generation和deferred reclaim保存Stage 1 foundation，但尚未向kernel发布TCP operation。future Stage仍需在不泄漏
smoltcp object的前提下完成Linux backlog、完整cause mapping、stream与Socket lifecycle proof obligations。

R0 entry时UDP与ICMP raw TX由kernel family caller经control-plane selection携带的`PumpWake`手工请求推进；Stage 1
已经删除该路径。当前两类真实consumer都在Stack owner commit后返回move-only progression obligation，并由attach
composition请求既有worker；control plane只保留route/source/interface selection。future TCP producer必须加入同一
owner-driven handoff，同时保持各protocol effect policy独立，不能恢复caller wake或建立shared effect truth。

本 RFC 的 Linux 6.6.32 ABI 比较基线固定在
`xref:linux-6.6.32:net/ipv4/af_inet.c#inet_create`、
`xref:linux-6.6.32:net/ipv4/af_inet.c#inet_bind`、
`xref:linux-6.6.32:net/ipv4/af_inet.c#inet_listen`、
`xref:linux-6.6.32:net/ipv4/af_inet.c#inet_stream_connect`、
`xref:linux-6.6.32:net/ipv4/af_inet.c#inet_accept`、
`xref:linux-6.6.32:net/ipv4/af_inet.c#inet_getname`与
`xref:linux-6.6.32:net/ipv4/af_inet.c#inet_shutdown`，以及
`xref:linux-6.6.32:net/ipv4/tcp.c#tcp_poll`、
`xref:linux-6.6.32:net/ipv4/tcp.c#tcp_sendmsg`、
`xref:linux-6.6.32:net/ipv4/tcp.c#tcp_recvmsg`、
`xref:linux-6.6.32:net/ipv4/tcp.c#tcp_close`、
`xref:linux-6.6.32:net/ipv4/tcp.c#tcp_setsockopt`与
`xref:linux-6.6.32:net/ipv4/tcp.c#tcp_getsockopt`；一次性 pending error 交付参考
`xref:linux-6.6.32:include/net/sock.h#sock_error`。这些引用只固定外部可见行为的比较快照，
不决定 Anemone 的内部 owner、类型或实现形状。

## 目标

- 发布 `AF_INET + SOCK_STREAM + protocol 0/IPPROTO_TCP` 的 initial-domain IPv4 Socket；
- 提供 explicit/implicit bind、listen/backlog、accept/accept4、active connect、local/peer
  address query 与 port conflict/admission；
- 支持 blocking 与 nonblocking connect，并保留 started/in-progress/connected/failed 的真实
  owner outcome，向 Linux ABI 映射 `EINPROGRESS`、`EALREADY`、`EISCONN`及最终 errno；
- 提供 reliable ordered full-duplex byte stream、partial progress、backpressure、EOF、FIN、RST、
  shutdown 与 final close；
- 为 connect、accept、send、receive/EOF 分别读取 owner-defined predicate，共享既有
  `EAGAIN`分类和 snapshot/register/recheck/final-scan 协议；
- 让UDP、ICMP raw与TCP各自的Stack-side protocol owner判断pump外committed transition是否产生
  immediate protocol work或提前next deadline，并在返回前把可合并、不会丢失的progression obligation
  交给能够推进相关path的既有Stack worker；不依赖control-plane selection、无关IRQ/timer、后续流量或
  caller偶然轮询；
- 提供真实 consuming `SO_ERROR`、`SO_REUSEADDR` 与 `TCP_NODELAY`，不以恒零、success-no-op、
  guessed errno 或 caller-specific branch 伪装能力；
- 保持 user pointer、Linux errno、fd/task/wait 与 smoltcp handle/ring borrow/private state 各自
  停留在所属 object fence，通过有界 operation-local byte handoff 表达 partial progress；
- 让 Endpoint、listener slot、pending child、RX/TX storage、timeout/deferred reclaim 等重要
  资源具有唯一 owner 和 owner-local Kconfig/build policy；owner capacity exhaustion与不可信/过大
  syscall输入返回typed backpressure/rejection，已经通过边界校验的有界kernel allocation遭遇全局OOM时
  允许kernel-fatal panic；
- 以 TCP 真实 consumer审计并改进现有network/Socket owner topology：自然shared obligation回到共同
  Socket owner，TCP-local fact留在TCP owner；既不为保持现状制造family-specific hack，也不为单一需求
  预建没有真实复用义务的generic framework。

## 非目标

- IPv6、dual-stack、IPv4-mapped IPv6、runtime network reconfiguration、runtime hotplug/detach、
  multiple network domain 与其它 protocol engine；
- 完整 BSD Socket framework、dynamic protocol manager、generic Endpoint hierarchy、通用
  mutable option bag、ready-mask bus 或第二 runtime registry；
- `SO_REUSEPORT`、keepalive 全套、linger、socket timeout、dynamic `SO_RCVBUF/SO_SNDBUF`、
  low-water mark、OOB/urgent data 与用户可配置拥塞控制；
- `MSG_ERRQUEUE`、通用 asynchronous ICMP error queue、ancillary producer、timestamp、credentials、
  fd passing、`sendmmsg/recvmmsg`、zero-copy、sendfile/splice；
- `FIONREAD`、`/proc/net/tcp`、sock-diag/inet-diag、`TCP_INFO`、Fast Open、MPTCP、HTTP/2、
  proxy、SSH、Git LFS、认证、submodule 与应用层协议实现；
- 为 CAgent 的 `ss -tan` 诊断子测例扩大 TCP、procfs 或 netlink target；该子测例属于独立
  系统诊断/验收环境，不定义本 RFC 的 TCP capability；
- 固定 Rust API、trait、type、module、lock、queue、buffer、listener engine pool、smoltcp fork
  seam、progression effect/wake 表示、worker 拓扑、stage、checkpoint 或验证命令。

## 工程准则

### Socket framework feedback

本RFC把TCP视为Socket framework的真实capstone与反馈来源，而不是只要求TCP适配当前代码形状。若最自然、
单一真相源的实现需要把owner-neutral creation、ABI dispatch、wait/recheck、opened-description lifecycle或
其它共同能力放回general Socket owner，实施必须停止当前gate并把证据、受影响consumer、最小shared surface、
迁移路线与Contract Impact提交RFC review。不能为了不改shared framework而在TCP owner内保存Linux/Socket truth、
复制common state machine、增加family-local bypass或无退出条件adapter。

这条反馈准则也不授权推测性抽象。只有具体TCP义务证明现有边界不自然，且shared owner能够比TCP-local方案减少
重复truth或协议税时，才提出共同框架变化；TCP特有的listener、connection、stream、error、option与resource fact
继续留在TCP owner。尚未出现具体shared delta时，当前Contract Impact表保持上界；一旦证据要求扩大或改变它，
先review本文再实现，不得静默修改current contract。

### Allocation 与 OOM boundary

资源失败分为三类，不能混写为一条“所有allocation都可恢复”的承诺：

1. 来自syscall的length、count、backlog或组合大小先在ABI/owner admission边界完成overflow、上界与target policy
   校验；不可信或过大输入按Linux-compatible typed error拒绝，不能靠尝试巨额分配后触发OOM；
2. Endpoint、listener/child、RX/TX、timer/reclaim等owner-configured capacity full是正常可恢复结果，必须返回
   typed rejection/backpressure/drop并保留recheck，不得panic、busy-spin或以allocator偶然失败定义容量；
3. 对已经通过校验、处于显式资源上界内的kernel allocation，当前工程阶段不要求把global heap OOM转成完整
   rollback/errno协议；allocator OOM可以kernel-fatal并panic。该容许不取消单一owner、显式上界、commit前校验、
   stale isolation或普通failure cleanup义务，也不能被用来掩盖可由用户输入稳定触发的无界分配。

## R0 用户可见 ABI

以下是已经接受的 R0 envelope。它在实现、validation 与
`NET-TCP-CUTOVER`完成前不是 effective API；未选择的 option/flag 必须稳定拒绝。

### 创建、地址与本地端点

- `socket(AF_INET, SOCK_STREAM, 0 或 IPPROTO_TCP)`；type bits 接受
  `SOCK_NONBLOCK | SOCK_CLOEXEC`，其它 family/protocol 稳定拒绝；
- IPv4 `sockaddr_in`支持 wildcard、loopback、configured local 与 remote external；
  `bind`支持 port 0，`connect`/`listen`在需要时完成 implicit bind；
- `getsockname`与`getpeername`保留 Linux addrlen、short output、copy fault 和未连接结果；
- local address、route/source/interface selection 与 binding/port reservation 分属 control-plane
  和 TCP owner，任何一方都不复制另一方的 truth；
- `SO_REUSEADDR`是一份 TCP-owner option fact，query/mutation 与后续 bind admission 可观察，
  不能只为首次 bind 返回成功。overlapping live reservation 只有在 Linux 6.6.32 oracle 允许且
  相关 reservation 均 opt in 时才能放宽；它不允许 duplicate live listener、绕过完整
  4-tuple uniqueness 或模拟 `SO_REUSEPORT`。wildcard/specific、listener 与 TIME_WAIT/rebind
  组合按固定 oracle 验证。

### Listener、accept 与主动连接

- `listen(backlog)`把 Linux-normalized backlog交给 TCP owner，并受 owner-local build capacity
  上界约束；acceptance 配置对 `listen(10)`至少能够保存十个已经完成 admission、等待 accept
  的 child，超出容量形成 bounded rejection/backpressure，而不是 panic、busy-spin或无界增长；
- Stack-side TCP owner统一拥有 binding、listener protocol resource、pending-child admission 与
  child Endpoint；kernel Socket只投影 accept predicate并拥有 fd reservation、peer-address
  copyout和publication rollback；
- `accept`与`accept4(SOCK_NONBLOCK | SOCK_CLOEXEC)`支持 blocking/nonblocking、并发 pending child、
  peer address output 与 fd rollback。child handoff 后的 copy/fd publication失败必须清理该 child，
  不得留下不可达 connection、重复排队或 stale reservation；
- active connect支持 explicit/implicit local endpoint、loopback/self-external 与 remote external。
  首次 nonblocking start 返回`EINPROGRESS`，仍在推进时重复调用返回`EALREADY`，已连接返回
  `EISCONN`；blocking connect使用同一 owner outcome和wait/recheck，不建立第二状态机；
- handshake RST、transport timeout与本地 route/source/admission failure必须保留为可区分 outcome，
  分别映射 `ECONNREFUSED`、`ETIMEDOUT`与相应 `ENETUNREACH/EADDR*`。无法区分原因时不得从
  merged closed state猜 errno。

### Byte stream、message projection 与 shutdown

- `read/write/readv/writev`提供 ordered reliable byte stream；send/receive只提交已经成功 copy
  并被当前 owner接受/消费的 prefix，允许 partial progress；
- 已缓冲 receive bytes先于 EOF或terminal error交付。orderly peer FIN在buffer耗尽后以read返回0
  表达；established RST产生`ECONNRESET`，不得伪装为EOF；
- connected stream支持普通 `send/recv`以及 `sendto/recvfrom`、single-message
  `sendmsg/recvmsg` projection。iovec使用当前共享kernel I/O `max_iovec_count`；message name、
  control、header copyout、zero length与fault precedence遵循固定Linux 6.6.32 oracle。R0没有
  ancillary producer，非空send control稳定拒绝，receive control输出为空；
- send flags支持`MSG_DONTWAIT | MSG_NOSIGNAL`，receive支持`MSG_DONTWAIT | MSG_PEEK`；
  `MSG_NOSIGNAL`只抑制本次send的`SIGPIPE`。`MSG_MORE`、`MSG_WAITALL`及其它未选择flag不进入R0；
- `SHUT_RD/SHUT_WR/SHUT_RDWR`作用于唯一direction/protocol owner。local write shutdown或其它
  Linux-defined broken-stream send返回`EPIPE`并产生`SIGPIPE`，除非本次`MSG_NOSIGNAL`抑制；
  exact repeated shutdown、pending bytes与terminal precedence服从固定Linux oracle；
- close不等待FIN/RST/TIME_WAIT或完整Stack resource reclaim；opened-description final release只
  触发一次kernel publication withdrawal与non-blocking protocol release handoff。

### Query、option、error 与 readiness

- descriptor query支持`SO_TYPE`、`SO_DOMAIN`、`SO_PROTOCOL`与`SO_ACCEPTCONN`；
- `TCP_NODELAY` query/mutation映射TCP owner的Nagle policy；不是Socket front中的通用option bag；
- `SO_ERROR`从产生真实async result的TCP owner取得并执行一次 consuming handoff。没有pending
  error时返回0；handshake RST至少为`ECONNREFUSED`，timeout至少为`ETIMEDOUT`，established RST
  至少为`ECONNRESET`。optlen/copy、重复query与ordinary operation消费顺序服从Linux 6.6.32，
  但kernel Socket与Stack不得各自保存可独立推进的pending-error truth；
- listener readable、connect complete writable/error、receive bytes/EOF readable、send capacity
  writable以及`RDHUP/HUP/ERROR`都由对应 owner fact投影；notification只表示recheck，不携带
  mask、errno或最终结果；
- `O_NONBLOCK`、creation-time`SOCK_NONBLOCK`与per-call`MSG_DONTWAIT`读取同一operation predicate；
  per-call flag不修改opened-description status；
- unknown option返回`ENOPROTOOPT`，unsupported flag返回`EOPNOTSUPP`；不得因某个consumer忽略
  error而success-no-op。

## Owner、Handoff、Failure 与 Cleanup

| Fact / capability | 唯一 Owner | 跨 owner handoff |
| --- | --- | --- |
| Linux tuple、sockaddr、flags、copy、errno、signal与optlen | Socket ABI adapter | normalized request/value与typed outcome |
| immutable semantic type、common FileOps与fd integration | general Socket front/static descriptor | opaque family-private storage与operation capability |
| route、source与interface selection | initial-domain IPv4 control plane | operation-local immutable selection |
| Endpoint identity、binding、port reservation、role、listener admission、pending child、connection outcome、RX/TX capacity与deferred reclaim | domain Stack TCP owner | opaque identity、non-blocking command、snapshot、child handoff与invalidation |
| online TCP state machine、packet processing与protocol timer | Stack-private smoltcp resource owner | 只向TCP owner提供protocol-domain cause/state seam |
| pump外mutation产生的immediate/earlier-deadline effect与受影响progression domain | 对应domain Stack protocol owner（UDP、ICMP raw或TCP） | commit后的opaque、可合并progression request |
| explicit-work admission、coalescing与bounded pump scheduling | 既有kernel attach/worker owner | stateless recheck request，不取得protocol truth |
| Linux readiness projection与source route publication | kernel TCP Socket/source | point-in-time facts与non-owning recheck route |
| blocking round、iomux watch与final harvest | syscall/iomux/epoll各自consumer | family attempt/predicate，不取得protocol state |
| fd publication与semantic final release | opened-description lifecycle owner | unpublished preparation/rollback与static final-release ctx |

connect handoff按“ABI normalize -> control-plane selection -> TCP owner admission/start -> outcome
publication”推进。user copy、route/source与local admission在TCP commit前失败不得留下partial
binding或connection；started之后的protocol outcome由TCP owner唯一保存并通过snapshot/consume
能力交付。wait cancellation只撤销当前consumer round，不取消或接管TCP owner state。

listen/accept按“listener owner admission child -> kernel reserve/copy -> fd publication”推进。
pending queue只有一份owner truth；kernel不得复制queue或提前发布child。accept失败由持有child
handoff的当前owner清理，late hint、旧listener generation或延迟protocol cleanup不得命中新
Endpoint。

send/receive通过bounded operation-local byte prefix跨object fence。user pointer和copy cursor不
进入Stack，smoltcp ring borrow/private handle不进入kernel Socket。handoff前失败保持owner state；
handoff后的partial commit只报告真实bytes，不通过重试重复提交或消费。

UDP、ICMP raw与TCP各自的Stack-side protocol owner负责判断一次pump外transition是否使相关interface/path
出现immediate work，或使worker当前已知的next deadline提前。TCP的connect start、send admission、receive
consume/window reopening、shutdown/abort/final release以及listener/accepted-child cleanup是必须审计的
producer；UDP/ICMP raw至少审计当前TX admission与所有能够产生egress/deadline effect的mutation。是否请求
推进取决于对应owner观察到的committed effect，而不是syscall名称或一份shared protocol policy。owner先提交
protocol state，再把可合并progression request交给既有attach/worker owner；Socket caller与control plane
不得缓存worker capability、从Linux operation猜测protocol work或拥有mutation wake policy。

progression request只表示“重新读取Stack truth并推进相关domain”，不携带packet、deadline、errno、readiness或
完成truth。Stack state/deadline仍是唯一progression truth。实现可以用
当前in-flight pump覆盖已可见的commit，也可以发布新的request；但必须证明mutation不会落在worker依据旧
状态决定休眠/arm较晚deadline之后而失去推进。request delivery完成后caller不等待实际pump；worker继续以
bounded round读取Stack truth并决定immediate repoll与next deadline。具体effect类型、wake carrier、存储、
去重、锁和per-interface/domain-wide routing由实现选择，也允许在有界可合并的前提下保守请求更多recheck；
但三类protocol owner不共享effect/readiness truth，attach/worker仍独占admission、coalescing、wake、schedule与stop。

receive与final release竞争时，TCP owner发出receive reservation是operation-local的
exactly-once resolve capability，不是opened-description liveness或`Arc`延长。默认参考顺序是：
final release先阻止新reservation时，operation在user copy前按typed outcome停止；
reservation先成功时，当前operation继续copy并请求TCP owner commit或rollback，而
final release并行撤销publication并返回。该顺序不新增确定的close/receive竞争
UAPI、errno或调度保证；满足同一safety envelope的更早cancel线性化点可由实现选择。

semantic final release先撤销kernel Socket source/publication和新operation入口，再以non-blocking
handoff请求TCP owner release。TCP owner唯一决定FIN/RST、orphan/TIME_WAIT和engine resource reclaim；
release mutation若产生immediate work或提前deadline，同一次handoff必须满足上述progression obligation；
late cleanup不能释放复用port的新owner，final release也不能等待worker、timer、peer或
receive operation。旧generation物理状态只在outstanding capability已resolve，或其需要的
prefix/state已完全detach且无法再触及复用对象后才能安全复用。

## Contract Impact

本 R0 的 Contract Impact 上界来自current contract与live source audit。每项target rule在本表指定的
cutover前都不是effective。实现反馈可以在review中收窄本表；若具体Socket framework证据要求新增或Refine
其它shared ID，必须先停止、修订并接受本文Contract Impact，再重新授权实现，不能静默扩大。

| Contract ID | 变化 | 当前规则 | Target摘要 | Cutover |
| --- | --- | --- | --- | --- |
| `NET-TCP-ENDPOINT-001` | Introduce | None | TCP Endpoint identity、bind/implicit-bind、listener/pending-child、active connect outcome、accepted-child handoff与stale isolation由Stack TCP owner统一拥有 | `NET-TCP-CUTOVER` Pending |
| `NET-TCP-STREAM-001` | Introduce | None | bounded byte-prefix send/receive、partial progress、buffer-before-EOF/error、FIN/RST/shutdown、SIGPIPE与owner-defined stream predicates | `NET-TCP-CUTOVER` Pending |
| `NET-TCP-LIFECYCLE-001` | Introduce | None | Socket publication/final release与FIN/RST/orphan/TIME_WAIT/deferred engine reclaim分离，并固定唯一cleanup owner | `NET-TCP-CUTOVER` Pending |
| `SOCKET-ABI-001` | Refine | [Active](../../contracts/socket/front-abi-wait.md#socket-abi-001--linux-abi止于family-neutral-adapter) | 增加IPv4 TCP tuple、stream message projection、真实`SO_ERROR`/`SO_REUSEADDR`/`TCP_NODELAY`、SIGPIPE/`MSG_NOSIGNAL`与typed async errno；不建立通用option/error bag | `NET-TCP-CUTOVER` Pending |
| `NET-CONTROL-PLANE-001` | Refine | [Active](../../contracts/net/control-plane.md#net-control-plane-001--initial-domain唯一决定ipv4-routesourceinterface) | selection只交付route/source/interface；protocol mutation wake policy迁到对应Stack-side owner，control plane不再把`PumpWake`作为operation selection的一部分 | `NET-PROTOCOL-PROGRESSION-CUTOVER` Effective |
| `NET-STACK-PUMP-001` | Refine | [Active](../../contracts/net/frame-path.md#net-stack-pump-001--stack-instance唯一推进protocol-state) | 增加各protocol owner在pump外commit后向相关既有worker可靠交付可合并progression request的共同义务；Stack state/deadline仍是truth，各protocol effect policy与具体wake/worker形状不固定 | `NET-PROTOCOL-PROGRESSION-CUTOVER` Effective |

### Dependencies

- [`SOCKET-FRONT-001`与`SOCKET-WAIT-001`](../../contracts/socket/front-abi-wait.md)：现有immutable
  descriptor、family-private envelope、operation-specific predicate与wait/recheck直接复用；普通
  TCP capability接线不构成shared rule变化。
- [`NET-PROTOCOL-BOUNDARY-001`与`NET-SOCKET-WAIT-001`](../../contracts/net/protocol-socket.md)：
  typed non-blocking capability、owner facts与recheck-only invalidation直接复用；TCP不共享ready truth。
- [`NET-CONTROL-PLANE-001`](../../contracts/net/control-plane.md)：route/source/interface selection保持
  initial-domain唯一owner；Stage 1 Refine后selection不再携带`PumpWake`，protocol owner通过共同handoff提交
  progression request。
- [`NET-BOUNDARY-001`、`NET-FRAME-OWN-001`与`NET-FRAME-PROGRESS-001`](../../contracts/net/frame-path.md)：
  object fence、frame ownership、bounded backpressure与durable provider recheck保持有效。
- [`OPENED-DESC-001..003`](../../contracts/task/opened-description-lifecycle.md)：fd publication、dup/fork
  sharing与semantic final release保持有效。
- [`IOMUX-POLL-001..003`](../../contracts/iomux/poll-wait.md)与
  [`EPOLL-WATCH-001`、`EPOLL-READY-001`、`EPOLL-FILE-001`](../../contracts/epoll/protocol.md)：TCP source
  作为普通source接入，不改变consumer-owned wait/watch/final harvest。

`NET-SOCKET-ENDPOINT-001`与`NET-UDP-TRANSACTION-001`继续只拥有UDP规则，不因名称或相邻实现
被扩张为TCP contract。若真实实现要求改变上列shared Dependencies的语义，必须在实施或cutover前
停止并回到RFC review，而不能把新增consumer机械写成Refine、悄悄修改current contract或为避免review
把自然shared obligation塞进TCP-local hack。

## Implementation Boundary

本文只定义实现授权必须遵守的语义边界；R0接受本身不授权实现。P0与Stage 1都曾取得各自独立授权并已关闭；
Stage 2已经解析为CKPT 2A/2B但尚未授权，Stage 3--5仍需分别解析和授权。

- **允许改变：** 与R0 target直接对应的TCP protocol vocabulary、domain Stack TCP owner、kernel TCP
  family/source、general Socket ABI/descriptor capability、ABI constants/wrappers、owner-local Kconfig，
  以及把UDP/ICMP raw从caller/control-plane手工pump request迁移到对应Stack-side protocol owner handoff
  所必需的窄composition、focused tests与同owner行为保持型模块拆分。TCP证据要求shared Socket framework
  调整时，允许先经本文Contract Impact review修订边界，再由后续独立授权实施；不得以TCP-local hack代替。
- **必须保持：** IPv4 initial-domain scope、control-plane selection owner、Stack/Socket object fence、
  opened-description final release、operation-specific readiness、三类protocol owner到既有worker的可靠
  progression handoff、各protocol effect policy与Stack progression truth分离、bounded frame/Stack progression、
  UDP/ICMP raw既有visible semantics、ABI/failure/cleanup诚实性、R0 non-goals与双架构acceptance floor；
  Stage 1只在完整production迁移与验证后通过`NET-PROTOCOL-PROGRESSION-CUTOVER`原子Refine上表指定的两项
  Network ID；后续Stage不得重复cut over或削弱这项effective handoff。
- **实现偏好：** concrete type/signature、Endpoint到engine的zero/one/many mapping、listener pool、lock、
  buffer/chunk/cursor、reservation/resolve capability表示、owner-local guard/counter、
  timeout/reclaim表示、smoltcp cause seam、progression effect/wake carrier、请求存储/去重、worker拓扑、
  module/file和验证命令；只要保持target，可由未来实现自然决定。

以下事实必须停止并回到RFC review / Target Renegotiation：

- mandatory普通TCP consumer需要改变named target/non-goal、owner/handoff、failure/cleanup、ABI、
  Contract Impact、acceptance或validation claim；
- natural implementation表明general Socket owner/public contract需要调整；这属于本RFC预期的architecture
  feedback，应提交最小shared修订与existing-consumer迁移方案，不能在TCP owner内绕过；
- 只能通过kernel取得smoltcp private object、Stack取得Task/File/fd/waiter、复制binding/pending-child/
  connection/error/readiness truth、绕过general Socket/wait或建立无退出条件的TCP专用桥才能实现；
- 无法保留`ECONNREFUSED/ETIMEDOUT/ECONNRESET`等target cause，只能从merged closed state猜测；
- 任一UDP、ICMP raw或TCP的pump外mutation只能依赖无关IRQ、已有timer、后续traffic、control-plane
  selection或caller-specific手工wake才能推进，或者必须把worker/route truth复制进Endpoint/Socket；
- `SO_REUSEADDR`、`SO_ERROR`、unknown option/flag或其它ABI只能用恒值、success-no-op、caller/test特判
  或降低oracle实现；
- 实现需要IPv6、`SO_REUSEPORT`、keepalive、linger、socket timeout、sock-diag/`ss`、error queue、
  ancillary、zero-copy或其它non-goal才能形成有用能力；
- TCP capability已经工作，但架构封顶仍依赖第二套truth/path或无法证明既有UDP、ICMP raw与Unix
  consumer继续成立。

验收盘中的musl BusyBox `wget`及可能提供的glibc `curl`/`git`只作为deployment
probe，不是mandatory closure evidence。hostname路径可以依赖UDP resolver/DNS，HTTPS还依赖
TLS/CA、时间、随机源与rootfs部署；这些环境未准备或超出R0不使工程进入Review Hold，
也不授权kernel扩大target或伪造unsupported ABI。实际尝试时按PASS/FAIL/Not Run并明确
依赖边界；probe PASS不替代repository-owned TCP evidence。

## Acceptance 与 Validation

### R0 target acceptance

接受R0代表review已经闭合本文target/non-goals、工程准则、owner/handoff/failure/cleanup、ABI、
Contract Impact、acceptance和validation claim。它不代表实现、runtime或cutover已经发生，也不自动授权实施。

### Stage 1 Network contract cutover

Stage 1的production handoff、UDP/ICMP raw原子迁移、Stack TCP foundation、owner/race proof、host/build验证与
RV64/LA64 focused UDP/ICMP raw local/external回归已经共同成立；`NET-PROTOCOL-PROGRESSION-CUTOVER`据此原子
Refine `NET-CONTROL-PLANE-001`与`NET-STACK-PUMP-001`并已Effective。该cutover只记录当前UDP/ICMP raw真实
consumer与shared production substrate；TCP target contracts继续Pending，尚未实现的TCP send、receive-window
reopening、shutdown与final-release producer没有被写成已验证。执行范围、命令、LA64 halt边界和Not Run见
[Stage 1 execution result](./implementation.md#639-stage-1-execution-result--closed)。

### Implementation closure 与 `NET-TCP-CUTOVER`

未来closure必须同时满足TCP capability与architecture capstone两组证据：

1. **TCP capability：**
   - owner-local protocol/Endpoint/listener/stream/error/resource proof与
     [目标与不变量](./invariants.md)全部成立；
   - oversized/overflowing syscall input在allocation/commit前typed拒绝，owner capacity saturation/recovery
     保持可恢复；source/review明确valid bounded internal allocation的global OOM可以kernel-fatal，未把它伪装成
     Socket errno或用来替代capacity/input policy；
   - worker已park且没有无关provider edge/traffic时，TCP connect start、send、receive-window reopening、
     shutdown/final release与accepted child后续operation仍可靠触发相关bounded pump并刷新deadline；迁移后的
     UDP/ICMP raw TX也由对应owner可靠触发，且保持原selection、transaction、failure与readiness语义；
   - repository-owned focused C/libc consumer覆盖blocking/nonblocking connect、`SO_ERROR`、
     bind/listen/accept/accept4、stream partial/EOF/RST/shutdown、SIGPIPE/`MSG_NOSIGNAL`、
     `SO_REUSEADDR`、`TCP_NODELAY`、poll/select/epoll、dup/fork/CLOEXEC/final-close、
     receive/final-close race safety与failure cleanup；
   - RV64与LA64 release、guest-local loopback/self-external以及remote-external TCP证据；
   - CAgent本地IPv4 HTTP server/client、`SO_REUSEADDR`、`listen(10)`、并发connection、send/recv/close
     transport marker。其`ss -tan`诊断子测例不属于本RFC mandatory TCP证据，不能触发target扩张；
   - 两架构验收盘中的`/musl/busybox wget`及可能提供的glibc `curl`/`git`
     只作为条件性deployment probe，不是mandatory通过项。它们使用hostname时可以先受
     UDP resolver/DNS能力限制。尝试时记录libc/binary identity、endpoint、resolver/TLS路径与
     PASS/FAIL/Not Run；只有将failure signal定位到R0 TCP operation时才记为本RFC实现缺陷，
     resolver、TLS或rootfs失败回到对应owner。缺失或超出target不阻塞
     `NET-TCP-CUTOVER`，成功也不替代repository-owned remote-external TCP evidence；
   - UDP、UDP extension、ICMP raw、Unix stream/seqpacket、Socket、iomux与epoll regression继续通过。
2. **Architecture capstone：**
   - 每项TCP长期fact、capacity、lifecycle和cleanup都落在唯一自然owner；
   - 没有第二套Socket、wait、control-plane、frame-path、protocol progression或runtime registry；
   - pump外mutation的progression producer、commit-to-request handoff、coalescing、stop与multi-interface
     routing可由owner/source audit和确定性race proof解释，不依赖caller逐路径补wake；
   - shared contract delta具有具体owner-neutral义务与真实consumer；TCP-local requirement没有伪装成通用framework，
     自然shared obligation也没有为维持旧框架而伪装成TCP-local adapter；
   - existing consumer继续在同一共同边界内成立，且closure明确区分已证明范围与Not Run。

两组是合取closure。若TCP syscall/workload通过但architecture capstone失败，RFC保持Review Hold或进入
Target Renegotiation / Not Cut Over；不得关闭RFC或宣称网络架构已经进入稳定扩展期。

physical hardware、`smp > 1`、其它NIC/platform、IPv6、full network LTP和上述non-goals默认Not Run，
不作为R0 closure前置。source/build/host proof不能替代双架构guest与真实userspace consumer，CAgent
loopback也不能替代repository-owned remote-external TCP evidence；条件性deployment probe不能
替代两者。

## 风险与反馈

- **Protocol cause loss：** 当前smoltcp public state不足以诚实生成全部target errno。未来实现必须形成
  narrow protocol-domain cause seam或等价owner-local evidence；该seam不能发布Linux errno、fd或ready mask。
- **Listener composition：** 当前单engine listener没有Linux backlog。多个engine resource、pause/admission或
  其它mapping均可选择，但pending-child truth、backlog admission、child cleanup和capacity owner必须唯一。
- **Deferred reclaim：** fd final close、FIN/RST、TIME_WAIT与engine reclaim不能错误合并为一个瞬时动作；
  timeout/orphan storage不得成为unbounded leak或阻塞final release。
- **Progression handoff：** protocol command可能在worker已park后制造immediate work或更早deadline；实现必须让
  commit与可合并request形成可靠handoff，把UDP/ICMP raw的caller-driven路径迁入对应owner，同时保留
  effect/wake/worker的工程选择，不把wake提升为protocol truth或建立shared effect policy。该production迁移属于
  Stage 1，不进入crate-only P0。
- **Deployment probes：** musl BusyBox `wget`与可能提供的glibc `curl/git`依赖
  resolver、TLS和rootfs环境；它们只做条件性尝试，结果必须区分TCP缺陷、target外
  dependency与Not Run，不能用source audit伪造execution，也不能取代mandatory evidence。
- **Architecture feedback：** 若TCP只能以owner穿透、第二truth、family-specific bypass或长期bridge落地，
  或最自然的shared Socket能力被迫留在TCP owner，这都是本RFC要收集并改进framework的target/contract反馈，
  不是普通实现困难；反之，没有具体共同义务时也不能借capstone之名扩大generic surface。
- **Allocation boundary：** owner capacity full与oversized/untrusted syscall input必须typed处理；valid bounded
  kernel allocation的global OOM在当前阶段可以panic。实现与验证不得把这三类失败互相替代。

当前[实施计划](./implementation.md)中的crate-only TCP engine Probe Gate与Stage 1均已Closed。P0证明窄async
cause与bounded multi-engine listener composition路线；Stage 1将其收敛为Stack-private production foundation，
迁移UDP/ICMP raw progression并执行唯一Network cutover。临时fixture与external validation asset已经删除。Stage 2
已解析为两个syscall-unreachable execution checkpoint并保持Ready / Not Authorized；Stage 3--5继续只表达future
Outline / Not Resolved / Not Authorized。

## 文档与证据

- [目标与不变量](./invariants.md)
- [实施计划](./implementation.md)（P0 Positive / Closed；Stage 1 Closed；Stage 2 Ready / Not Authorized；
  Stage 3--5 Outline / Not Authorized）
- [背景材料：历史定位共识](./backgrounds/positionings.md)（冻结，不再维护）
- Current baseline：[Network](../../contracts/net/index.md)、
  [Socket](../../contracts/socket/index.md)、
  [Opened-description](../../contracts/task/opened-description-lifecycle.md)、
  [IOMUX](../../contracts/iomux/poll-wait.md)、[Epoll](../../contracts/epoll/protocol.md)
- External ABI oracle：本页Current Baseline列出的固定`xref:linux-6.6.32`源码位置
- CAgent fixed source evidence：
  [`simple_llm_server.c`](https://github.com/oscomp/testsuits-for-oskernel/blob/b5ec6ef8497e1818cbdec3b54bb722f036e57972/cagent-test/simple_llm_server.c)、
  [`agent_lite.c`](https://github.com/oscomp/testsuits-for-oskernel/blob/b5ec6ef8497e1818cbdec3b54bb722f036e57972/cagent-test/agent_lite.c)与
  [`cagent_testcode.sh`](https://github.com/oscomp/testsuits-for-oskernel/blob/b5ec6ef8497e1818cbdec3b54bb722f036e57972/scripts/cagent_testcode.sh)
- implementation：[实施计划](./implementation.md)；P0 checkpoint commit；Stage 1 focused Git commit；transaction None

## 修订记录

2026-08-05接受初始target与Contract Impact为R0；同日P0 Positive / Closed，并在不改变R0 target、Contract Impact或
validation claim的前提下解析和关闭Stage 1 implementation gate。Stage 1仅执行R0已经接受的两项Network Refine。
同日把Stage 2解析为不发布syscall的CKPT 2A/2B internal integration gate；这只调整implementation route、stage
职责与validation placement，不改变R0 target、owner、ABI、Contract Impact或acceptance，因此修订号保持R0。
文本历史由仓库Git保存。

## Closure

P0 Positive / Closed / Not Cut Over。crate-only experiment证明：engine可在`Closed`合流前保存并单次交付
RST/timeout protocol cause，由Stack-side owner结合权威connection phase解释；多个真实TCP engine slot可形成
有界logical listener，并在full、take/cancel recovery与generation reuse下保持单一pending truth和stale isolation。
probe-only fixture与experimental API均在P0 exit删除。

Stage 1 Closed / Network Cut Over。production source以move-only `ProtocolProgression`形成Stack owner commit到
既有worker request的可靠handoff，删除`PumpWake`和caller `request_pump()`，并把UDP/ICMP raw原子迁入该路径；
Stack-private TCP foundation永久保留bounded Endpoint/listener/engine/generation/deferred-reclaim owner和窄
Reset/Timeout cause seam，但不发布TCP operation或UAPI。host/config/双架构build、RV64/LA64 focused UDP/ICMP raw
local/external regression、asset cleanup与独立review满足Stage 1 acceptance，详情见
[execution result](./implementation.md#639-stage-1-execution-result--closed)。`NET-PROTOCOL-PROGRESSION-CUTOVER`
已Effective；三项TCP Introduce与`SOCKET-ABI-001` Refine继续Pending。TCP UAPI/guest/CAgent/final harness、hardware、
`smp>1`与其它NIC/platform保持Not Run；LA64只证明完整shutdown顺序，不宣称wrapper exit 0。transaction未创建，
register没有新增当前问题。Stage 2已Resolved / Ready，但CKPT 2A/2B均Not Authorized；Stage 3--5保持Outline /
Not Resolved / Not Authorized。Stage 2不发布syscall，production TCP UAPI route留给Stage 5最终activation；本轮只完成
resolution并到此停止。
