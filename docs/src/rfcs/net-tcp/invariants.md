# IPv4 TCP Socket 与网络架构封顶目标与不变量

**状态：** Accepted Target / Not Effective
**最后更新：** 2026-08-05
**父 RFC：** [RFC-20260805-net-tcp](./index.md)
**适用修订：** R0

本文只定义父RFC的target、shared-contract delta与correctness proof obligations。当前effective
Network、Socket、Opened-description、IOMUX与Epoll规则仍以`docs/src/contracts/`为准；本文始终是
target文档，不直接成为current contract。Stage 1只在`NET-PROTOCOL-PROGRESSION-CUTOVER`时把两项
Network Refine回写到对应current contract页面；TCP条款在`NET-TCP-CUTOVER`前均不生效。本文不授权实现。

## 规则分类

- **Correctness Invariant：** 唯一owner、handoff、并发、lifecycle、cleanup、memory/resource safety、
  wait无lost wake与ABI诚实性；不能通过工程妥协降低。
- **Target Guarantee / Capability：** R0的IPv4 TCP连接、字节流、option/error/readiness和双重closure
  claim；只能通过Target Renegotiation改变。
- **Implementation Preference：** concrete type、trait、function、Endpoint/engine mapping、lock、queue、
  buffer/chunk/cursor、timer、worker、smoltcp seam、module/file、stage与精确命令；本文不冻结。

## 状态所有权

| Fact / relation | 唯一 Owner | 生命周期 / commit | 非 owner允许持有 |
| --- | --- | --- | --- |
| semantic type与ops association | immutable static Socket descriptor | Socket creation一次确定到semantic final release | immutable projection |
| Linux tuple、flags、sockaddr、copy、errno与signal choice | Socket ABI adapter | 当前syscall/operation | normalized value与typed outcome |
| opened-description publication | `task::files` | `Unpublished -> Live(n) -> Retired` | transient lease与static final-release ctx |
| route/source/interface selection | initial-domain IPv4 control plane | 每次bind/connect/send需要的selection window | operation-local immutable result |
| Endpoint identity、local binding与port reservation | domain Stack TCP owner | create到retire，owner-local commit | opaque identity、query/outcome |
| endpoint role与listener admission | domain Stack TCP owner | unbound/bound/listening/connecting/connected/terminal | role-scoped operation capability |
| pending child与accepted-child handoff | listener/TCP owner，handoff后由当前receiver | admission到accept rollback或fd publication | current handoff capability |
| online TCP state、protocol timer与packet processing | Stack-private protocol engine owner | engine resource attach到deferred reclaim | narrow cause/state seam |
| pump外mutation产生的immediate/earlier-deadline effect与受影响progression domain | 对应domain Stack protocol owner（UDP、ICMP raw或TCP） | protocol commit到request handoff | opaque、可合并progression request |
| explicit-work admission、coalescing与bounded pump scheduling | 既有kernel attach/worker owner | active mapping到worker stop | stateless recheck capability |
| async connect/terminal outcome | domain Stack TCP owner | outcome产生到ordinary operation或`SO_ERROR` consume | snapshot/consuming outcome |
| RX/TX bytes、capacity、FIN/RST/shutdown | domain Stack TCP owner | connection commit到direction/protocol terminal | bounded prefix request/outcome |
| Socket source route与Linux readiness projection | kernel TCP Socket/source | source publication到withdraw | owner facts与non-owning`PollRoute` |
| blocking wait round | syscall/Socket wait consumer | attempt/register/recheck到return/cancel | family predicate/attempt |
| iomux/epoll watch与delivery | iomux/epoll各自owner | register到cancel/final harvest | source snapshot与recheck route |
| orphan/TIME_WAIT/deferred reclaim | domain Stack TCP owner | kernel visibility撤销后到resource reclaim | diagnostic snapshot，不参与kernel behavior |

这些fact没有共同mutable owner。opaque identity、selection、snapshot、handoff capability与notification
只服务明确operation或所有权转移；不得反向取得另一owner的private lock/object，也不得成为第二份
behavior truth。

## Target Invariants

### NET-TCP-ENDPOINT-001 — Endpoint role、binding、listener与connection outcome只有一个Stack owner

**分类：** Correctness Invariant / Shared Contract Introduce / Target Guarantee。

**规则：** domain Stack TCP owner唯一拥有Endpoint identity namespace、binding/port reservation、
current role、listener protocol resources、backlog/pending-child admission、active connect progression、
connected tuple、async protocol outcome与stale isolation。kernel TCP Socket只持opaque Endpoint capability、
point-in-time facts与typed command/outcome；它不保存connected bit、peer、pending-child queue、engine handle、
timeout result或第二份port reservation。

explicit bind、listen implicit bind与connect implicit bind都在control-plane selection后由TCP owner裁决。
commit前的address copy、route/source、port conflict或capacity失败不改变旧role/binding；commit后binding、
role与reservation作为同一owner state立即供后续query/admission读取。port 0 assignment也只由该owner发布。

`SO_REUSEADDR` intent与binding admission在同一TCP owner内组合。option必须可query/mutate并真实影响
Linux 6.6.32允许的overlapping reservation判断；它不能允许duplicate live listener、替代
`SO_REUSEPORT`、绕过4-tuple uniqueness或只作为兼容success。wildcard/specific、listener、TIME_WAIT与
rebind matrix由fixed oracle证明。

**Listener handoff：** TCP owner唯一维护pending child。accepted child移交kernel后，receiver负责fd
reservation、peer-address copy与publication；任一步骤失败都由当前handoff owner清理child及其protocol
resource，不能requeue已移交child、留下不可达connection或恢复旧listener generation。listener close先
撤销新admission，再处理owner-local pending child与deferred protocol cleanup。

**Owner：** domain Stack TCP owner；control plane只提供operation-local selection，kernel只拥有ABI与fd
publication。

**依赖：** `NET-CONTROL-PLANE-001`、`NET-PROTOCOL-BOUNDARY-001`、`OPENED-DESC-001..003`。

**违反表现：** Socket与Stack都保存connected/peer/error；listener engine与Endpoint各有pending queue；
accept fd失败后child泄漏或重排；close按fd number释放port；late timer/cleanup命中新generation；
`SO_REUSEADDR`恒成功但不参与admission；或Endpoint持Task/File/waiter。

**Cutover / Proof：** create/bind/implicit-bind/port-0/reuse conflict、listen/backlog、active connect、
accept rollback、listener close、dup/fork/final close、stale generation与capacity isolation的owner-local、
host和双架构guest proof。

### TCP-CONNECT-RESULT-001 — Started、completion、failure与`SO_ERROR`共享一个真实outcome source

**分类：** Correctness Invariant / ABI Correctness / Target Guarantee。

**规则：** active connect由TCP owner产生typed `started/in-progress/connected/failed`事实。kernel ABI把
首次nonblocking start映射为`EINPROGRESS`、仍在推进的重复connect映射为`EALREADY`、已连接映射为
`EISCONN`；blocking connect只在同一predicate上等待并最终读取同一outcome，不建立parallel connect
state或由wake payload决定结果。

protocol owner必须在原因仍可识别时保存至少以下区别：initial SYN被RST拒绝、transport timeout、
established connection被RST、本地route/source/admission失败。ABI分别映射`ECONNREFUSED`、
`ETIMEDOUT`、`ECONNRESET`与相应`ENETUNREACH/EADDR*`；不得在所有路径先合并为Closed再猜测。
smoltcp/private engine可以增加narrow protocol-cause seam，但不能拥有Linux errno、`SO_ERROR` optlen、
fd、wait或readiness。

`SO_ERROR`是对同一owner outcome的一次consuming query：pending error不存在时返回0；存在时由TCP owner
原子交付并清除该pending outcome，kernel adapter只处理Linux value/optlen/copy。ordinary connect/send/
receive与`SO_ERROR`的竞争只能有一个error consumer；另一路按consume后的current role/state返回Linux
oracle定义的结果，不能让Socket和Stack各持一份pending error。

**Owner：** Stack TCP owner拥有protocol outcome与consume；Socket ABI adapter拥有Linux mapping和copy；
wait consumer只拥有当前wait round。

**依赖：** Target Refine `SOCKET-ABI-001`、current `SOCKET-WAIT-001`与`NET-SOCKET-WAIT-001`。

**违反表现：** Socket缓存`connect_error`而Stack也保存failure；`SO_ERROR`恒零；RST和timeout都映射同一
errno；wake edge携带final errno；blocking与nonblocking各有状态机；query后错误仍重复交付；或cancel
一个waiter取消connect protocol。

**Cutover / Proof：** immediate failure、EINPROGRESS/EALREADY/EISCONN、success/failure completion、
RST/timeout/route failure、`SO_ERROR` zero/consume/race、poll/select/epoll completion与signal/cancel matrix。

### NET-TCP-STREAM-001 — Byte prefix、FIN/RST、shutdown与partial progress只有一个transaction owner

**分类：** Correctness Invariant / Shared Contract Introduce / ABI Correctness。

**规则：** TCP owner唯一拥有ordered RX/TX bytes、capacity、FIN/RST与direction shutdown facts。kernel
Socket不缓存buffer length、EOF、peer-close、writable threshold或private ring cursor；每次send/receive只
通过bounded operation-local source/sink与typed outcome交换已提交prefix。

send在owner admission前完成当前prefix的fallible user copy；owner只为实际接纳bytes返回success。receive
由owner先保留/分离当前可交付prefix，再把成功copy的bytes作为consume commit；具体copy/chunk/cursor可变，
但不能让user pointer进入Stack、让smoltcp ring borrow跨object fence、重复提交send或重复消费receive。
发生signal、fault、capacity或terminal condition时，已完成prefix优先按Linux partial-progress规则返回，
未提交suffix保持原owner状态。

receive reservation由TCP owner作为transaction owner发出；当前operation只持operation-local、
exactly-once resolve capability，只能请求同一owner commit已成功copy的prefix或rollback未提交
suffix。该capability不转移RX truth，不是opened-description liveness lease，也不能以
syscall-local `Arc`数量决定是否继续。

与final release并发时，必须先确定新reservation acquisition与resolve的线性化边界。
默认参考顺序是：final release先阻止新acquisition时，operation在user-visible copy前
取得typed stop outcome；reservation先成功时，当前operation保留resolve capability至
commit或rollback，final release不等待它。本顺序不承诺确定的close/receive race UAPI、
winner或errno。实现可选择更早的cancel线性化点，但必须发生在该operation未产生
user-visible copy之前，由TCP owner exactly-once rollback，且不能丢失或重复交付bytes。

已缓冲RX bytes必须先于EOF或terminal error交付。orderly FIN在buffer耗尽后产生read 0；RST不能伪装EOF，
established reset必须保留`ECONNRESET`。`SHUT_RD/WR/RDWR`、peer FIN、local close与protocol terminal
是不同事实；local write-disabled/broken stream按Linux oracle产生`EPIPE`与`SIGPIPE`，本次
`MSG_NOSIGNAL`只抑制signal，不改变errno或state。

`read/write/readv/writev`与connected stream的send/recv/message projection消费同一transaction；message
wrapper不能建立packet boundary。R0的`MSG_PEEK`不消费RX bytes；nonempty ancillary send稳定拒绝，receive
没有producer时输出empty control。exact zero/fault/name/header order由fixed Linux 6.6.32 oracle约束。

**Owner：** domain Stack TCP owner拥有stream/protocol facts和commit；Socket ABI拥有user copy、Linux flag/
errno/signal与message projection。

**依赖：** Target Refine `SOCKET-ABI-001`、`NET-FRAME-PROGRESS-001`与共享kernel I/O iovec bound。

**违反表现：** Socket缓存RX count/EOF；stream被降格为datagram；user pointer或ring borrow越界；copy fault
重复send/receive；short receive丢弃未读suffix；final release在user copy后cancel并requeue同一
prefix；reservation无resolve owner；buffered bytes被EOF/error覆盖；RST返回0；
`MSG_NOSIGNAL`吞掉`EPIPE`；或message wrapper建立第二data plane。

**Cutover / Proof：** zero/short/vector/fault/partial、capacity saturation/recovery、peek、buffer-before-FIN/RST、
shutdown combinations、SIGPIPE/NOSIGNAL、concurrent send/receive、receive reservation/final-close竞争的
allowed-outcome/exactly-once proof与message projection matrix。

### TCP-WAIT-001 — Connect、accept、send与receive各自读取owner predicate

**分类：** Correctness Invariant / Target Guarantee。

**规则：** connect completion、pending child、TX admission、RX bytes/EOF/error分别由相应TCP owner fact
定义predicate。kernel Socket source只把一次snapshot投影为Linux readiness；共同Socket层只统一
`EAGAIN`分类、blocking choice、signal/SIGPIPE处理与snapshot/register/recheck/final-scan。不存在一份
shared `tcp_ready` truth。

notification由owner先commit fact并取得route snapshot，再在owner guard外发送；edge只提示重查，不
携带mask、errno、child、capacity或terminal truth。public writable可以是最低admission hint；需要更强
operation-specific条件时必须注册对应predicate，不能因public hint为ready而busy-retry。

listener readable、connect writable/error、RX bytes/EOF readable、TX writable、peer-FIN RDHUP、terminal
HUP与pending ERROR分别由current owner facts投影。blocking、`O_NONBLOCK`、creation-time
`SOCK_NONBLOCK`与per-call`MSG_DONTWAIT`读取同一predicate；per-call flag不修改description status。

signal、timeout、force、close或losing waiter只retire当前round/route。final release先withdraw source，
不等待waiter；late/repeated hint对retired generation fail closed。

**Owner：** TCP owner拥有facts；kernel TCP source拥有Linux projection与route publication；syscall/iomux/
epoll各自拥有wait/watch/final harvest。

**依赖：** `SOCKET-WAIT-001`、`NET-SOCKET-WAIT-001`、`IOMUX-POLL-001..003`、
`EPOLL-WATCH-001`、`EPOLL-READY-001`与`EPOLL-FILE-001`。

**违反表现：** ready-mask cache驱动behavior；register window lost wake；event payload直接返回；family
内部second wait loop/busy-poll；accept predicate复用receive predicate；connect complete覆盖error；取消
一个waiter撤销其它consumer；或old edge命中新Endpoint。

**Cutover / Proof：** 每项predicate的not-ready/ready/terminal matrix、snapshot/register race、multi-waiter、
signal/cancel、poll/select/epoll coexistence、final close与late hint proof。

### NET-PROTOCOL-PROGRESSION-HANDOFF-001 — 各protocol owner可靠移交pump外progression obligation

**分类：** Correctness Invariant / Shared Contract Refine。

**规则：** UDP、ICMP raw与TCP各自的domain Stack protocol owner必须判断一次pump外transition是否产生了
相关interface/path可观察的immediate protocol work，或让worker当前已知的next deadline提前。该effect判断
是protocol-owner-local policy，不共享为一份generic effect或readiness truth。若产生effect，protocol state
commit与progression request必须形成不可丢失的handoff：commit先对后续pump可见，随后在operation报告成功或
release handoff完成前，把一个可合并request交给能够推进该domain的既有attach/worker owner。request只表示
重新读取Stack state/deadline truth，不携带packet、deadline、errno、readiness、capacity或完成结果。

TCP的connect start、successful send admission、receive consume/window reopening、shutdown/abort/final release、
listener或accepted-child cleanup是必须审计的producer集合；UDP/ICMP raw至少审计当前TX admission与所有能够
产生egress/deadline effect的mutation。是否请求推进由对应owner观察到的committed effect决定，不由syscall名称、
Socket caller或control-plane selection猜测。protocol owner从自己的Endpoint/selection/engine association确定
受影响progression domain；control plane只拥有route/source/interface selection，不携带mutation wake policy；
Socket不得缓存worker capability或保存第二份route truth。实现可以在有界可合并的前提下对更多transition
保守请求recheck，但不能依赖周期轮询、无关IRQ、已有timer、后续traffic或另一个caller碰巧推进。

实现可以证明当前in-flight pump一定观察本次commit并据此完成repoll/deadline更新，也可以发布新的request；
无论采用哪种方式，都不能让mutation落在worker依据旧状态决定park或arm较晚deadline之后。worker消费request
后仍只执行既有bounded pump，并由pump outcome决定immediate repoll与next deadline；caller等待到request
handoff完成即可，不等待packet发送、worker round、peer或resource reclaim。worker stop/admission closure先于
terminal cleanup时，晚到request必须fail closed且不能重新激活progression；shutdown后的保留/cleanup继续服从
既有network/System Power边界。

本规则不固定progression effect的Rust表示、wake carrier、request存储/去重、lock、per-interface或
domain-wide routing、worker数量或module layout。实现可以把handoff闭合在Stack transition boundary，也可以
使用等价的owner-local composition，只要上述owner、顺序、liveness、boundedness与stop语义可证明。不同协议
可以使用不同的owner-local effect判定，但不能保留caller-driven与owner-driven两套长期策略。

**Owner：** 对应domain Stack protocol owner拥有effect判断与affected progression domain；既有kernel
attach/worker owner拥有explicit-work admission、coalescing、wake、schedule与stop。control plane、Socket ABI
与wait consumer都不拥有该handoff。

**依赖：** Target Refine `NET-CONTROL-PLANE-001`与`NET-STACK-PUMP-001`、current
`NET-UDP-TRANSACTION-001`与`NET-ICMP-RAW-TRANSACTION-001`，以及target
`NET-TCP-ENDPOINT-001`、`NET-TCP-STREAM-001`与`NET-TCP-LIFECYCLE-001`。

**违反表现：** UDP/ICMP raw send仍由caller/control-plane手工request；TCP send或receive成功后只能等下一次
IRQ；close只改FIN/RST state却没有worker request；connect创建更早deadline但worker继续睡到旧deadline；
accepted child后续operation重新route selection才能wake；Socket按operation手工维护wake清单；request先于
commit被worker消费；wake edge成为protocol work truth；domain stop后late request恢复worker；或为此建立
第二套protocol pump/worker。

**Cutover / Proof：** 分层关闭，不把完整TCP producer matrix提前归入Stage 1：

- Stage 1建立production handoff与Stack TCP foundation，原子迁移UDP/ICMP raw真实producer，并覆盖
  commit/request/park race、in-flight pump、coalescing、earlier deadline、local/external progression domain、
  late stop request及source/host/双架构回归。只有这些义务整体成立时，才通过
  `NET-PROTOCOL-PROGRESSION-CUTOVER`原子Refine current `NET-CONTROL-PLANE-001`与
  `NET-STACK-PUMP-001`；失败时两项规则都保持旧语义。
- 后续Stage随真实TCP能力逐步接入connect、successful send、receive-window reopening、shutdown/abort/
  final release与listener/accepted-child cleanup producer，并证明每个producer使用同一owner-driven handoff，
  不恢复caller-driven wake或第二套progression truth。
- 最终`NET-TCP-CUTOVER`证明worker初始park且无无关provider edge/traffic时，完整TCP producer matrix都能触发
  相关bounded pump，并关闭TCP source/host/双架构proof；它不重复cut over Stage 1已经Effective的两项
  Network规则。

### NET-TCP-LIFECYCLE-001 — Kernel publication withdrawal与protocol reclaim明确分离

**分类：** Correctness Invariant / Shared Contract Introduce。

**规则：** opened-description的`Live(1) -> Retired`是kernel semantic final release唯一trigger。TCP
final-release hook先阻止新operation并withdraw Socket source、observer route与Endpoint association，再以
non-blocking capability请求TCP owner release。关闭一个dup/fork alias、Rust`Drop`、raw fd number或临时
reference都不能触发或延迟semantic release。

TCP owner唯一决定FIN/RST、orphan、TIME_WAIT、timer与engine resource reclaim。kernel fd close、Endpoint
对kernel不可见、peer观察到FIN/RST和物理resource释放不是同一瞬时动作；final release不等待worker、timer、
peer或完整reclaim。release handoff后，latepacket/timer/hint/cleanup只能命中原generation，不能恢复kernel
association、释放复用port的新owner或把旧bytes/error交给新Socket。

publication withdrawal只终止新operation acquisition，不依赖opened-description borrow延长；final release
也不等待已发出的receive resolve capability。TCP owner必须保留旧generation中该capability仍可触及的
transaction state，直到它commit/rollback；或在reservation时把所需prefix/state完全detach到
不再访问Endpoint storage的owner-issued operation-local storage，commit/rollback authority仍属于
TCP owner。Endpoint identity退役、engine/backing slot物理复用和
port/TIME_WAIT admission是三个边界；只有旧capability无法再触及新owner state时才能复用相应
slot/backing，但不因此要求final release等待完整protocol reclaim。

network orderly shutdown先停止新protocol admission和kernel access，再请求owner-local bounded cleanup；
boot-persistent provider/Stack与System Power全局顺序继续由current contracts拥有。本R0不宣称runtime detach或
完整network teardown。

**Owner：** opened-description owner拥有final-release trigger；kernel TCP source拥有publication withdrawal；
domain Stack TCP owner拥有protocol release/reclaim。

**依赖：** `OPENED-DESC-001..003`、`NET-ATTACH-001`与System Power network shutdown boundary。

**违反表现：** close任一alias提前FIN/reclaim；final release等待TIME_WAIT或receive operation；
`Drop`成为semantic close；kernel保存orphan list；Stack回调fd table；late timer释放复用port；
withdraw后operation仍取得旧Endpoint；旧reservation命中已复用slot/backing的新owner；或shutdown
以unbounded drain阻塞terminal episode。

**Cutover / Proof：** unpublished rollback、dup/fork/CLOEXEC/non-final/final close、half-close/final-close组合、
listener pending child cleanup、receive reservation在final close前后的resolve/reclaim、orphan/TIME_WAIT、
late timer/hint、generation/slot/backing/port reuse与orderly shutdown proof。

### TCP-RESOURCE-001 — Listener、child、buffer、timer与reclaim全部有界

**分类：** Correctness Invariant / Target Guarantee。

**规则：** live Endpoint、listener engine/slot、half-open与completed pending child、per-connection RX/TX bytes、
queued protocol work、timer/orphan/TIME_WAIT与deferred reclaim都必须有唯一capacity owner和显式上界。影响
外部行为的重要capacity进入owner-local Kconfig/build policy；不得依赖散落magic number、allocator偶然失败、
unbounded queue或global TCP emergency list形成边界。

`listen(backlog)`经Linux-compatible normalization后形成当前listener admission limit，并受build capacity上界
约束。acceptance配置必须支持`listen(10)`下至少十个completed pending child；该要求不固定内部engine数量、
SYN queue算法或共享/独立pool。exhaustion必须typed拒绝、drop或backpressure并保留恢复recheck，不能panic、
busy-spin或阻塞unrelated UDP/ICMP/Unix consumer。

上段的`exhaustion`专指owner-configured Endpoint/listener/child/buffer/timer/reclaim capacity full。syscall提供的
length、count、backlog或组合大小必须先完成overflow与target上界校验，过大或不可信输入在allocation/commit前返回
Linux-compatible typed error，不能靠OOM拒绝。已经通过这些校验且位于显式资源上界内的kernel heap allocation若
遭遇global allocator OOM，当前工程阶段允许kernel-fatal panic；本target不要求为它建立完整errno/rollback协议。
该OOM边界不允许用allocator failure代替capacity policy，也不削弱普通admission failure与cleanup证明。

RX/TX saturation、Endpoint exhaustion、listener child full、timer/reclaim full与stale identity需要可区分的
owner-local诊断；diagnostic counter/label不参与admission、readiness或cleanup decision。recovery必须来自owner
fact transition和durable recheck，而不是caller/worker偶然轮询。

**Owner：** domain Stack TCP owner拥有TCP资源及admission capacity；global Stack pump、provider与driver继续
拥有各自current budget/backpressure。

**依赖：** `NET-FRAME-PROGRESS-001`、`NET-STACK-PUMP-001`与owner-local Kconfig policy。

**违反表现：** unbounded listener pool；一个full listener阻塞整个Stack；capacity mirror驱动behavior；
TIME_WAIT泄漏；用allocator OOM/panic代替owner capacity或用户输入上界；release等待pool slot；diagnostic
counter决定writable；或为capacity暴露smoltcp handle/driver resource。valid bounded internal allocation的
global OOM直接panic本身不构成违反。

**Cutover / Proof：** endpoint/listener/child/buffer/timer/reclaim limits、listen(10)、mixed saturation/recovery、
oversized/overflowing input的pre-allocation rejection、global OOM policy source audit、cross-family isolation、
bounded pump、diagnostic-only audit与shutdown stress。

### TCP-ABI-001 — Linux representation、option、signal与stable rejection止于adapter

**分类：** ABI Correctness / Target Guarantee。

**规则：** raw family/type/protocol tuple、creation/per-call flags、sockaddr/addrlen、iovec/msghdr、user pointer、
sockopt level/name/optlen、Linux errno与signal choice只存在于Socket ABI adapter。TCP/Stack owner只接收
normalized address/command/option intent/byte transaction并返回typed outcome；不接收raw pointer、fd、Task、
Linux errno或poll mask。

R0发布`SO_REUSEADDR`、`SO_ERROR`、`TCP_NODELAY`与descriptor query。每项option由能够保持其行为invariant的
owner保存；general Socket front只做normalized dispatch，不建立mutable option/error bag。`MSG_NOSIGNAL`只
抑制本次TCP`SIGPIPE`；UDP/raw现有diagnostic compatibility与TCP真实signal producer保持分离。

unknown option稳定`ENOPROTOOPT`，unsupported flag稳定`EOPNOTSUPP`。`SO_REUSEPORT`、keepalive、linger、
socket timeout、dynamic buffer option、OOB、error queue、diag与advanced TCP UAPI均不得因consumer/test忽略
error而success-no-op。exact optlen/value/copyout、errno precedence、repeat operation与message header行为由
`xref:linux-6.6.32`和focused runtime共同固定；需要visible deviation时先回到target review。

**Owner：** Socket ABI adapter拥有Linux representation/mapping；Stack TCP owner拥有normalized protocol fact。

**依赖：** Target Refine `SOCKET-ABI-001`。

**违反表现：** Stack解析Linux option/errno；Socket缓存private TCP state；`SO_ERROR`恒零；unknown option成功；
SIGPIPE policy下沉smoltcp；copy fault后partial option commit；caller-specific branch；或为deployment tool
新增静默flag。

**Cutover / Proof：** tuple/address/flags/options/optlen/fault、connect errno、SIGPIPE/NOSIGNAL、descriptor query、
message projection与stable rejection的repository-owned C、focused oracle和双架构guest matrix。

### TCP-ARCH-CAP-001 — TCP capability与架构封顶是同一closure的两个合取条件

**分类：** Target Guarantee / Architecture Closure Boundary。

**规则：** `NET-TCP-CUTOVER`只有在父RFC的TCP capability acceptance与architecture capstone acceptance都
满足时才能执行。每项TCP长期fact必须落入上述唯一owner；TCP不能建立第二套Socket、wait、control-plane、
frame-path、protocol progression、registry或无退出条件的bridge；对shared contract的每项Refine都必须由真实
cross-consumer语义变化证明。

现有UDP、UDP extension、ICMP raw、Unix stream/seqpacket、opened-description、iomux与epoll consumer必须在同一
共同边界内继续成立。TCP-local option/listener/error/resource machinery不能因入口相似而提升为generic framework；
existing shared boundary能自然承载的义务也不能为保持历史定位术语而另建TCP旁路。

TCP同时是Socket framework的反馈consumer。若具体TCP路径证明owner-neutral creation、ABI dispatch、wait/recheck、
opened-description lifecycle或其它共同义务在current shared boundary中位置不自然，必须把最小framework修订、
existing-consumer迁移与Contract Impact送回父RFC review；不得把Linux/Socket truth塞进TCP owner以避免shared change。
没有第二个真实consumer或共同owner义务时，则不得把TCP-local machinery泛化为framework。

TCP syscall、CAgent HTTP或条件性deployment-tool workload通过但架构条件失败时，证据可以保留，RFC必须
Review Hold、Target Renegotiation或Not Cut Over；不得关闭RFC、更新current contract或宣称网络架构进入
稳定扩展期。
反之，source architecture audit不能替代真实TCP userspace与双架构runtime。

CAgent的`ss -tan`子测例、`/proc/net/tcp`与sock-diag不属于本target；它们失败不能推动target扩张。
验收盘中的musl BusyBox `wget`与可能提供的glibc `curl/git`只是条件性deployment
probe；resolver/TLS/rootfs超出R0时不阻塞closure，缺失execution诚实记Not Run。probe
PASS不能替代repository-owned remote-external、双架构guest或architecture-capstone proof。

**Owner：** RFC review/closure authority拥有合取claim；各runtime fact仍由其subsystem owner拥有。

**依赖：** 父RFC全部target、Contract Impact、acceptance与current Network/Socket contracts。

**违反表现：** 只因HTTP成功就cutover；TCP走private wait/Stack bypass；pump外mutation靠caller逐路径补wake；
为TCP扩大generic public API而无第二consumer；为避免必要shared framework修订而建立TCP-local hack；environment
未运行却写PASS；`ss`缺口进入TCP target；shared regression缺失仍声称capstone；或将implementation preference
误写为永久architecture invariant。

**Cutover / Proof：** 父RFC两组mandatory evidence、shared contract/source audit、existing consumer regression、
final independent review与明确Not Run boundary；只有两组同时通过才允许`NET-TCP-CUTOVER`。

## 禁止退化项

- 不得缓存可以从TCP owner直接取得的binding、peer、role、pending-child、error、buffer/capacity、EOF或ready fact；
- diagnostic owner id、generation、timer label、counter或debug state不得反向驱动protocol/ABI behavior；
- 不得把wake edge、timeout identity、fd number、`Arc` count、smoltcp handle或`Drop`提升为lifecycle truth；
- 不得让任一UDP、ICMP raw或TCP的pump外protocol commit依赖无关event、control-plane selection或
  caller-specific手工wake才能最终被既有worker观察；
- 不得为了consumer、architecture或test建立caller-specific branch、parallel path、无退出条件bridge或第二truth；
- 不得为保持current Socket framework形状，把owner-neutral common obligation或Linux/Socket truth塞进TCP owner；
- 不得让过大/不可信syscall输入通过无界allocation触发OOM来代替typed拒绝；valid bounded internal allocation的
  global OOM允许panic，不需要伪装为可恢复errno；
- 不得通过恒零`SO_ERROR`、success-no-op option、RST-as-EOF、guessed errno、较弱oracle或Not Run-as-PASS换取
  workload成功；
- 不得把implementation type、module、listener pool、buffer、lock、timer或命令误写成target invariant。
