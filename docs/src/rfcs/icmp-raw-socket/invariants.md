# IPv4 ICMP Raw Socket 目标与不变量

**状态：** R0 Accepted Target / Not Effective
**最后更新：** 2026-08-03
**父 RFC：** [RFC-20260803-icmp-raw-socket](./index.md)
**适用修订：** R0

本文定义本 RFC 的target invariants与proof obligations，不是current contract、implementation plan或执行授权。完整
用户可见包络、Contract Impact、Implementation Boundary与acceptance见父RFC；当前effective规则仍以
`docs/src/contracts/`与live source为准。R0 acceptance与Checkpoint 1 activation均不使这些target rule提前生效。

Linux 6.6.32 的逐项errno、sockaddr/optlen、copy precedence与重复transition matrix是本文ABI policy的conformance
surface，不需要在公共Draft前穷举成第二份规范表。实现和focused tests可以继续解析它们；只有解析结果会改变target、
owner、failure/cleanup、可见语义或acceptance时才重新进入RFC review。

## 规则分类

- **Correctness Invariant：** 唯一owner、ABI containment、packet/data ownership、local-admission-before-delivery、
  lifecycle/stale isolation、lost-wake防护、bounded resource与cleanup；不能通过工程妥协降低。
- **Target Guarantee / Capability：** ICMP-only IPv4 raw UAPI、unicast/unfragmented数据面、selected options、完整raw RX
  bytes、双架构focused/LTP/ping floor；只能通过显式Target Renegotiation改变。
- **Implementation Preference：** concrete type、trait/enum/function family、opaque handle、Endpoint/engine topology、
  copy或shared immutable backing、lock、queue、buffer、helper、module/file与是否复用smoltcp raw/icmp mechanism；保持前
  两类边界时由实现自然决定。

Target Renegotiation可以修订有用能力包络，不能接受第二份owner truth、UAF、stale identity命中、lost wake、无界queue、
资源泄漏、ABI success bluff或以test-only path替代production path。

## 状态所有权与生命周期

| 状态 / 关系 | 唯一 Owner | 生命周期 / commit | 非 owner 允许持有 |
| --- | --- | --- | --- |
| semantic type与static ops association | general Socket front | create时一次确定，semantic final release后不再用于新operation | immutable descriptor projection |
| creation permission decision | current task credential owner + Socket ABI adapter | tuple解析后、Endpoint/fd publication前 | operation-local allow/deny result |
| TTL/TOS send-header policy | kernel ICMP raw family | create到semantic final release；每次send读取immutable snapshot | operation-local TTL/TOS snapshot |
| route/source/interface selection | initial-domain IPv4 control plane | 每次connect/send operation | immutable selection result |
| local/peer association、ICMP filter与Endpoint lifecycle | domain Stack raw owner | create commit到retire；association/filter transition owner-local commit | opaque identity、normalized mutation/query、typed outcome、point-in-time facts |
| local IPv4 destination admission | concrete interface/IP owner | 每份ingress packet的parse/admission window | admitted packet observation capability |
| per-Endpoint RX/TX storage与fanout/drop | domain Stack raw owner | queue admission到detach/dispatch/drop/retire | capacity/readable/writable snapshot |
| RX detached packet | Endpoint owner，detach后转为operation-local kernel transaction | dequeue/detach是唯一handoff | byte slice只在current owner lifetime内借用 |
| TX ICMP message与IPv4 packet | user-copy transaction，admission后转为Stack raw owner | copy、selection、header formation与queue commit | immutable request/policy values |
| smoltcp handle/`SocketSet`/private buffer（若使用） | concrete Stack owner | Stack-private construction到cleanup | 不跨crate暴露 |
| source route entries | raw Socket source registry | subscribe到consumer retirement/stale cleanup | non-owning `PollRoute` |
| blocking wait round | syscall/Socket wait consumer | attempt/register/final recheck到return/cancel | family只提供attempt/predicate，不持waiter |
| opened-description publication | `task::files` | `Unpublished -> Live(n) -> Retired` | transient fd/file capability与static final-release ctx |

不存在Socket、control plane、Stack、smoltcp、wait或driver共同拥有的mutable fact。opaque identity、selection、snapshot、
detached packet与notification都必须具有明确的单向handoff或只读用途，不能反向推进另一个owner的state。

## Target Invariants

### ICMP-RAW-TYPE-001 — Raw semantic type固定为IPv4 ICMP且创建publication-last

**分类：** Correctness Invariant / ABI Correctness / Target Guarantee。

**规则：** resolver唯一接受`AF_INET + SOCK_RAW + IPPROTO_ICMP`及`SOCK_NONBLOCK|SOCK_CLOEXEC`创建flags，并归一为
一份immutable ICMP raw semantic type/static descriptor。protocol number不是backend-private mutable value；R0不得因
内部mechanism能够过滤任意`IpProtocol`而暴露generic raw tuple、dynamic registry或per-instance protocol truth。

权限检查读取current task effective `CAP_NET_RAW`。检查失败返回`EPERM`，且不得创建live Endpoint、source route、
opened description或fd slot。权限成功后，Endpoint/source/file/fd所需的fallible preparation仍必须publication-last；
任一失败由唯一unpublished creation guard撤销本transaction创建的registration与resource。fd publication后只有
opened-description final release能够触发semantic retire。

**Owner：** Socket resolver/ABI adapter拥有tuple、flags与permission admission；static descriptor拥有semantic type
witness；Stack raw owner拥有Endpoint resource；opened-description owner拥有publication lifecycle。

**依赖：** `SOCKET-FRONT-001` Target Refine、`SOCKET-ABI-001` Target Refine、`OPENED-DESC-001..003`。

**违反表现：** protocol number保存为任意backend state；Socket同时保存descriptor与第二type tag；权限失败后留下
Endpoint；fd先发布再补权限/初始化；`Drop`或fd number成为retire truth；或为future protocol建立registry。

**Cutover / Proof：** resolver/type witness、capability allow/deny、每个fallible preparation point、fd rollback、
dup/fork/CLOEXEC/non-final close/final close的owner-local与双架构focused proof。

### NET-ICMP-RAW-ENDPOINT-001 — Association、filter、fanout与retire由Stack raw owner统一拥有

**分类：** Correctness Invariant / Shared Contract Introduce。

**规则：** domain Stack raw owner唯一拥有Endpoint identity namespace、semantic lifecycle、local/peer association、
ICMP filter、per-Endpoint bounded RX/TX storage、matching fanout、drop accounting、capacity facts与retire。kernel只持
role-scoped non-blocking operation capability和opaque identity；它不能反向取得private handle、queue、mapping或lock，
也不能缓存association/filter/readiness truth。

bind、connect、reconnect与disconnect在同一个raw owner内原子裁决旧新association。control plane可以为connect/send提供
operation-local source/route validation，但不拥有association；失败不得部分改写既有local/peer filter。每个matching
live Endpoint独立取得或丢弃自己的delivery；一个Endpoint满载、retire或filter拒绝不能改变其它Endpoint、普通ICMP、
UDP/TCP demux或原packet的处理。

semantic final release先阻止kernel取得新operation capability并withdraw source publication，再移交non-blocking
Endpoint retire。Stack retire先撤销active identity/association与新fanout admission，再清理owner-local storage和
private engine。late notification、旧opaque identity、重复retire或延迟backing release不能命中新generation或恢复
association。

**Owner：** domain Stack raw owner。

**依赖：** Refined `NET-PROTOCOL-BOUNDARY-001`、`OPENED-DESC-001..003`、`NET-CONTROL-PLANE-001`。

**违反表现：** kernel和Stack各存local/peer/filter；按fd number寻找Endpoint；关闭一个dup提前retire；full consumer阻塞
整个fanout；raw handled抑制普通ICMP；old cleanup命中新Endpoint；Endpoint持Task/File/waiter；或final release等待worker。

**Cutover / Proof：** create/rollback/retire、bind/connect transition、multiple consumer/filter/full/drop isolation、
dup/fork/final close、late hint、stale generation与network shutdown proof。

### NET-ICMP-RAW-INGRESS-001 — Local admission先于非独占raw delivery

**分类：** Correctness Invariant / Shared Contract Introduce / Target Guarantee。

**规则：** 只有已经通过对应interface IPv4 parse/checksum与local-destination admission的packet可以进入raw matching。
destination admission truth继续只由interface/IP owner拥有；outer Stack、raw Endpoint与kernel Socket不得复制assigned
address、broadcast、multicast group或route-based local predicate。R0 raw另外只接受unicast destination，即使IP owner因
其它consumer承认local broadcast或joined multicast，也必须在raw owner admission中按target排除。

在local admission完成且原始packet仍可用的同一mutation window内，raw owner为每个matching Endpoint建立独立计费的
delivery ownership。delivery必须保留原始未分片IPv4 datagram在`total_len`内的字节，包括header length、TOS、ID、
flags、IP options与ICMP message；不得从`Ipv4Repr`或其它会丢字段的表示重建。copy与shared immutable backing均可，
但交给Socket receive前必须从Stack lock、`SocketSet`、smoltcp buffer和driver frame lifetime detach。

raw observation是non-exclusive。建立或丢弃raw delivery不能窃取、修改或标记原packet handled；普通ICMP processing继续
按自己的checksum/type/body规则推进。IPv4 fragment不进入R0 raw fanout或reassembly state。invalid IP header/checksum
不交付；已经通过IP admission的invalid ICMP checksum仍对raw可见，普通ICMP consumer可以独立拒绝。

**Owner：** interface/IP owner拥有destination admission；Stack raw owner拥有target-scope unicast/filter、fanout与
detached delivery；普通ICMP owner拥有自己的protocol处理。

**依赖：** `NET-FRAME-OWN-001`、`NET-FRAME-PROGRESS-001`、`NET-STACK-PUMP-001`。

**违反表现：** current pre-admission `raw_socket_filter()`顺序进入production；outer layer重算local predicate；
`Ipv4Repr`重建header；raw delivery借用driver/smoltcp lifetime；一个consumer满载丢掉原packet；raw filter抑制普通ICMP；
或fragment静默进入raw queue。

**Cutover / Proof：** original-byte vectors覆盖TOS/ID/flags/options、wrong-destination/broadcast/multicast/fragment rejection、
multiple consumer与ordinary ICMP coexistence；production path source audit与双架构真实ingress evidence。

### NET-ICMP-RAW-TRANSACTION-001 — TX/RX各有唯一packet handoff与commit boundary

**分类：** Correctness Invariant / ABI Correctness / Shared Contract Introduce。

**TX规则：** 用户输入只包含ICMP message。Socket ABI owner完成fallible user copy和destination normalization；control
plane唯一选择route/source/interface；kernel raw family提供本次immutable TTL/TOS snapshot；Stack raw owner形成无IP
option、非分片IPv4 header并在返回success前完成size、capacity与packet ownership admission。用户负责ICMP checksum；
kernel不修补或用ICMP type/body推进自身state。

copy、selection、MTU与capacity失败都发生在Endpoint admission commit前，不得留下partial packet或成功结果。
超过selected interface MTU减IPv4 header返回`EMSGSIZE`；R0不隐式fragment。admission commit是packet进入Stack raw owner的
唯一handoff；commit后的普通provider loss不反向改写send success，也不能让Stack因缺失已验证selection而静默drop。
zero-length message仍按同一transaction形成真实IPv4 packet。

**RX规则：** Endpoint queue中的完整datagram由Stack raw owner独占；dequeue/detach后由operation-local kernel transaction
独占。short buffer复制prefix并消费整个packet；`MSG_TRUNC`返回完整packet length。`MSG_PEEK`不执行detach，因此成功、
short copy与copy fault后均不消费。zero-length non-peek receive仍等待、detach并消费一份packet，通常返回0，
`MSG_TRUNC`返回完整长度。non-peek copy fault可以丢弃已detach packet，但不能requeue、重排或让Endpoint再次访问。

exact user-copy与errno precedence遵循Linux 6.6.32 oracle，但任何允许路线都必须保持“copy成功前不提交TX”与“RX packet
只有一个owner”的correctness boundary。peer address来自packet source；sockaddr port projection不得成为protocol或
association truth。

**Owner：** Socket ABI adapter、control plane、kernel raw policy与Stack raw owner各自拥有上述局部阶段；handoff后前一
owner不得继续访问mutable packet state。

**依赖：** Refined `SOCKET-ABI-001`、`NET-CONTROL-PLANE-001`、`NET-FRAME-OWN-001`。

**违反表现：** smoltcp full-IP TX buffer直接暴露为UAPI；kernel与Stack同时持mutable packet；无route仍成功排队；
provider失败追溯修改send结果；short receive只消费prefix；peek改变queue；copy fault requeue；或ICMP checksum被kernel
静默修补。

**Cutover / Proof：** zero/short/fault/peek/trunc矩阵、TTL/TOS snapshot race、route/source/destination override、MTU、
capacity/recovery、header bytes与provider path的owner-local和双架构focused proof。

### ICMP-RAW-ABI-001 — Linux可见矩阵由adapter与固定oracle共同约束

**分类：** ABI Correctness / Target Guarantee。

**规则：** raw tuple、`SOCK_*`/`MSG_*` bits、`sockaddr_in` layout/addrlen、user pointer、sockopt level/name/optlen、
copyout与Linux errno只存在于Socket ABI adapter。family/Stack只接收normalized association、message、policy和typed
outcome，不接收raw user pointer、fd、Task或Linux errno。

R0支持`MSG_DONTWAIT`、send-side `MSG_NOSIGNAL`、receive-side`MSG_PEEK|MSG_TRUNC`。`MSG_NOSIGNAL`只因本target没有
`SIGPIPE` producer而成为静默兼容；代码必须注释ABI取舍、可见行为与移除条件，并提交低噪声diagnostic。其它flag明确
拒绝，不能generic forgiveness。

R0支持`SO_DOMAIN/SO_TYPE/SO_PROTOCOL/SO_ACCEPTCONN`、`IP_TTL`、`IP_TOS`与`ICMP_FILTER`。TTL/TOS/filter的storage与
snapshot只能服务已选择的真实operation；不得建立通用option bag。unsupported option稳定返回`ENOPROTOOPT`，不能因
BusyBox或其它程序忽略error而success-no-op。`IP_HDRINCL`明确unsupported；存在full-packet private mechanism不能改变
UAPI。

bind/connect/disconnect、local/peer query、port projection及option exact optlen/value/copyout使用固定
`xref:linux-6.6.32`与focused runtime收敛。RFC不复制穷举表；实现若要采用可见deviation，必须先通过target review，
不能把oracle mismatch降级为helper细节。

**Owner：** Socket ABI adapter拥有Linux representation/errno；kernel raw family拥有normalized policy；Stack raw owner
拥有protocol association/filter transaction。

**依赖：** `SOCKET-ABI-001` Target Refine。

**违反表现：** Stack解析Linux option/errno；Socket保存raw protocol tuple；unsupported option返回success；
`MSG_NOSIGNAL`扩张为所有unknown flag兼容；optlen fault后partial commit；或为实现方便修改oracle。

**Cutover / Proof：** RV64/LA64 focused guest ICMP raw suite承载Linux-compatible ABI/errno/copy/options矩阵；
RV64/LA64、glibc/musl curated `socket` LTP只证明stock libc与相邻Socket regression；source audit证明raw representation
不越过adapter。

### NET-SOCKET-WAIT-001 Target Refine — Raw predicate、invalidation与consumer wait保持分离

**分类：** Correctness Invariant / Shared Contract Refine / Target Guarantee。

**规则：** raw Endpoint owner唯一拥有RX queue、TX admission capacity、retire及其它readable/writable所需facts；kernel
raw source只从一次current snapshot投影Linux readiness。readable等价于当前RX queue非空。writable表示live Endpoint
当前可接受一个R0范围内的最小TX admission，不要求connected，不缓存route/source，也不承诺任意destination/length或
request-specific selection成功。

fact transition必须先由owner commit并取得observer route snapshot，再在owner guard外notify/drop。notification只提示
consumer重算，不携带mask、errno、capacity或lifecycle truth。Socket source按`SOCKET-WAIT-001`与`IOMUX-POLL-001..003`
执行snapshot/register/recheck/final scan；blocking、`O_NONBLOCK`/`SOCK_NONBLOCK`、`MSG_DONTWAIT`读取同一operation
predicate，per-call flag不修改opened-description status。

raw family不生产peer close、half-close、HUP/RDHUP或pending error；absence不能通过恒零`SO_ERROR`或ready bit伪装。
signal、timeout、force、close或losing waiter只retire当前round/route，不取消其它consumer。final release withdraw source
publication后不等待waiter；late/repeated hint对retired generation fail closed。

**Owner：** Stack raw owner拥有protocol facts；kernel raw source拥有Linux projection与route registry；Socket/iomux/epoll
各自拥有blocking round、watch和delivery policy。

**依赖：** current `SOCKET-WAIT-001`、`IOMUX-POLL-001..003`、`EPOLL-WATCH-001`、`EPOLL-READY-001`。

**违反表现：** common Socket或source缓存ready mask；notification payload直接返回；register window lost wake；
unconnected永久清除writable；family内部busy-poll/第二wait loop；一个waiter取消撤销其它route；或connect制造stream HUP。

**Cutover / Proof：** unconnected/connected readable-writable matrix、capacity saturation/recovery、snapshot/register race、
multi-waiter/signal/cancel、poll/select/epoll coexistence、final close与late hint proof。

### ICMP-RAW-RESOURCE-001 — 所有Endpoint、packet与progression资源有界且故障隔离

**分类：** Correctness Invariant / Target Guarantee。

**规则：** live raw Endpoint数量、每Endpoint RX/TX packet count与byte storage、单次fanout/pump工作和drop accounting必须
有界。影响资源与用户行为的重要capacity由KernelConfig拥有并显式命名；不得使用散落magic number、无界queue或依赖
allocator偶然失败形成边界。

每个matching Endpoint在fanout中独立计费。某一Endpoint容量不足只增加其owner-local drop诊断并跳过该delivery，不得
占用其它Endpoint budget、阻塞普通ICMP processing或使protocol/frame owner丢失原packet。TX normal exhaustion返回
可恢复not-ready/typed failure并通过owner transition恢复writable；不得panic、busy-spin或依赖worker偶然轮询。

malformed IP、unsupported destination/source、RX full、TX full、retired/stale identity必须保留可区分的owner-local
diagnostic与ABI mapping。诊断字段只服务观测，不能反向驱动state machine或成为第二份capacity/lifecycle truth。

**Owner：** Stack raw owner拥有Endpoint storage/fanout/drop与protocol admission capacity；global Stack pump、provider与
driver继续拥有各自既有budget/backpressure。

**依赖：** `NET-FRAME-PROGRESS-001`、`NET-STACK-PUMP-001`、System Power network shutdown boundary。

**违反表现：** global/shared unbounded packet list；一个full Endpoint使fanout提前终止；drop counter决定readiness；
capacity recovery靠timer/busy loop猜测；普通OOM变panic而无既有kernel-fatal依据；或隐藏drop以维持ping成功。

**Cutover / Proof：** endpoint/packet/byte limits、mixed full/live consumer、TX saturation/recovery、bounded pump、drop
diagnostics与shutdown stress；重要config的schema/source audit。

### ICMP-RAW-ACCEPT-001 — Syscall、回归与真实网络三类证据不能互相替代

**分类：** Target Guarantee / Acceptance Boundary。

**规则：** R0 cutover必须同时具备：

1. owner-local/host correctness proof，覆盖owner、handoff、fanout、packet bytes、capacity、lifecycle与wait races；
2. RV64/LA64 focused guest ICMP raw suite，覆盖权限、ABI/errno/copy/options/readiness与fd lifecycle；
3. RV64/LA64、glibc/musl curated `socket` LTP regression，按完整stock binary matrix执行；
4. RV64/LA64普通BusyBox `ping 10.0.2.2`真实external Stack/VirtIO/QEMU router round trip。

custom focused oracle不能冒充stock LTP；LTP syscall PASS不能冒充真实packet path；ping成功不能覆盖权限、copy、option、
readiness、fanout或lifecycle。每个architecture/libc/claim分别记录实际运行者与结果；缺失mandatory层时保持对应Not Run
和Not Cut Over。

公网、DNS、peer ping Anemone、physical hardware、`smp > 1`、其它NIC、full network LTP与final harness不属于R0
mandatory floor；未运行不能写成PASS，也不阻塞R0。

**Owner：** 各test owner拥有自己的oracle/fixture；外层runner只拥有环境进入、fixture staging、进程/timeout和日志收集，
不取得ping、DNS、ICMP或LTP case语义。

**依赖：** 父RFC Acceptance 与 Validation。

**违反表现：** test-only injection或Socket-to-Socket fast path替代production path；修改上游LTP取得PASS；只运行有利
subcase；解析BusyBox自然语言作为唯一oracle；一架构替代另一架构；或用build证明runtime。

**Cutover / Proof：** `ICMP-RAW-CUTOVER`引用每类canonical execution evidence；Draft publication本身没有运行证据。

## RFC-local Review 与停止边界

以下事项是实现调查，不要求在Draft发布前冻结：post-admission seam的concrete函数/类型、copy与shared backing选择、
Endpoint topology、lock/queue/buffer、common request type名称、module/file layout、validation命令及是否需要滚动stage。

以下事项不是实现偏好：用户可见能力与non-goals、Linux-compatible ABI policy、owner/handoff、packet fidelity、
fanout isolation、failure/cleanup、Contract Impact和mandatory acceptance。真实证据要求改变其中任一项时，必须停止并由
RFC review决定Route Correction、Accepted Reduced Target、Follow-up RFC或Not Cut Over。
