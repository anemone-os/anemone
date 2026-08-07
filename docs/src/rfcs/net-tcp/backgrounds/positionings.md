# net-tcp RFC 定位共识

**状态：** Archived RFC Background / Superseded by Draft body
**最后更新：** 2026-08-05
**Canonical RFC：** [RFC-20260805-net-tcp](../index.md)
**主题：** TCP capability、现有网络架构封顶验证与后续扩展边界

## 文档目的

本文保存公共 Draft 形成前已经达成的元层面定位。它不再拥有 proposal、target、
Implementation Boundary、Contract Impact、acceptance 或执行事实；发生冲突时以父 RFC、
current contracts 与 live source 为准。本文不授权 implementation、probe、checkpoint、
transaction 或 contract cutover，并且不再随后续 review、implementation 或 closure 更新。

## Current Baseline

`net-tcp` 不再被理解为完成早期“三份网络 RFC 计划”的最后一项。`net-frame-path` 与
`net-udp` 关闭后，Socket Abstraction / Unix Socket、ICMP raw Socket 与 UDP Socket
Extension 又以真实异构 consumer 继续反馈并建设了共同 Socket 和 network 边界。因此，
本 RFC 的设计起点是届时的 live source、register 与已经生效的
[Network current contracts](../../../contracts/net/index.md)和
[Socket current contracts](../../../contracts/socket/index.md)，而不是早期网络
规划、历史 positioning 或某份 Closed RFC 的旧实现路线。

`net-tcp` 仍是一份独立的 sibling RFC。它不是 `net-frame-path` 或 `net-udp` 的子 RFC，
也不建立新的 network umbrella RFC、架构总 RFC、共享实施计划或聚合进度账本。既有
current contract 中未变化的规则只能作为 Dependencies；只有 live baseline 上真实发生
的语义变化才能形成 `Introduce`、`Refine`、`Replace`、`Remove` 或
`Scoped Exception`。

## 核心定位

`net-tcp` 首先是一份 TCP capability RFC：它应交付一组边界完整、可由普通用户程序
消费的连接型网络字节流能力，而不是只证明某个示例、测试或单一路径能够运行。

与此同时，`net-tcp` 是现有网络基础架构的封顶验证。TCP 是当前计划中对长期状态、
双向推进、异步结果、等待、生命周期与资源治理要求最完整的真实 network Socket
consumer。RFC 应利用这一真实压力检验既有 owner 拓扑、依赖方向、object fence、
cross-owner handoff 与扩展方式是否足以自然承载新的 transport family，并只对被真实
TCP 义务证明不足的共享边界作必要修订。

因此，本 RFC 的成功不是“在旧架构旁边接入一条 TCP 路径”，也不是“为了保持旧架构
不变而让实现适配任何代价”。它必须同时得到两个彼此可审查的结论：TCP target 已经
诚实交付；承载该 target 的共同架构没有依赖第二份行为真相、owner 穿透、私有表示
泄漏、family-specific 旁路或无退出条件的长期桥。

## “架构封顶”的含义

本定位所说的架构封顶，目标是在当前证据范围内稳定以下根基：

- network device、frame path、control plane、protocol Stack、kernel Socket、VFS/fd
  与 wait consumer 之间的责任拓扑和依赖方向；
- 每类行为事实只有一个自然 owner，其它参与方只持有窄 capability、operation-local
  value、snapshot 或 recheck route；
- Linux/POSIX 可见语义停留在 kernel Socket/ABI owner，具体协议机与设备私有对象不
  穿透各自 object fence；
- 新 family 和新能力默认通过既有边界增量扩展，共享抽象只由多个真实 consumer 的
  共同义务驱动；
- ordinary follow-up 默认 Preserve 这些根基。后续若要改变 owner、handoff、public
  surface、shared contract 或 acceptance，必须由新的 RFC 或 Target Renegotiation
  明确授权，而不能作为普通实现反馈静默发生。

封顶不等于冻结具体 Rust API、trait、类型、目录布局、锁、队列、worker、算法、容量值
或 smoltcp 私有实现。只要唯一 owner、外部语义和 shared contract 保持成立，这些仍是
可以随实现证据演进的 implementation preference。

在 `net-tcp` closure 前，这些根基和下文四层反馈都是后续 RFC 的默认审查起点，不是
已经接受且不可修订的 invariant。实现、consumer audit 或 validation 若证明另一条路线
更简单、更自然且同样保持唯一 owner、完整 failure/cleanup、ABI 诚实性与可验证性，RFC
可以据实调整 owner 边界、handoff 或 shared contract，并明确记录相应 delta；“默认”只
禁止把这些变化伪装成普通实现细节，不禁止有证据的架构修订。

封顶也不宣称当前证据覆盖所有未来网络形态。它首先约束当前 initial-domain、IPv4、
boot-persistent netdev、smoltcp-backed protocol Stack 与 Linux/POSIX Socket 路线。
IPv6、动态控制面、runtime hotplug/detach、多 network domain、其它协议机或更广平台
若提出新的根本义务，应按当时证据单独 review；不能由 `net-tcp` 的 closure 预先证明，
也不应因此削弱本次对当前根基的稳定化要求。

## TCP 对四层架构的定位级反馈

本节根据当前 live source、current contract 与 TCP 已知义务，记录未来 RFC 的默认架构
参考。它只记录讨论问题时应先采用的责任方向和反例，不固定最终 Rust surface、内部状态
拆分、smoltcp 扩展方式或 validation route。公共 RFC 仍须重新核验每项责任；若真实证据
支持不同但自洽的模型，可以在 RFC review 中替换本节默认值。

### 四层是 object fence，不是完整 owner 数量表

四层首先描述 protocol object 的可见性和 crate 依赖方向，而不是宣称整个 network runtime
只有四个 owner、要求每次 operation 依次穿过四层，或要求每层内部只有一个状态中心：

- `anemone-smoltcp` 默认只拥有线上 TCP 状态机、packet processing 与 protocol timer；
  若现有观察面不足以保留 connect failure、RST 或 timeout 等原因，可以增加窄的
  protocol-cause seam，但不把 Linux errno、fd、wait 或 readiness 下沉到该层。
- `anemone-net-api` 默认只增加 TCP 所需的 protocol-domain value、非阻塞 command、一次性
  snapshot、recheck-only invalidation 与有界 byte handoff 语义；不因为 TCP 建立 dynamic
  protocol manager、generic Endpoint hierarchy、ready-mask bus 或第二个 runtime registry。
- `anemone-smoltcp-stack` 默认作为 TCP Endpoint 与 concrete engine resource owner，组合
  Endpoint identity、binding/port admission、smoltcp object mapping、buffer/capacity、异步
  protocol result 与 deferred reclaim。一个 Endpoint 可以映射零个、一个或多个私有
  smoltcp resource；具体 mapping、listener engine pool 与内部状态拆分保持开放。
- `anemone-kernel` 顶层包含多个彼此不同的 owner domain。当前默认由 kernel network 侧负责
  control-plane selection、窄 Stack access、pump wake 与 invalidation routing，由 general/
  family Socket 侧负责 Linux ABI、opened-description/fd、blocking、wait/readiness、errno 与
  publication rollback。这里描述责任，不冻结现有 `net` / `fs/socket` 目录或 adapter 名称，
  也不把两者合并为一个综合 Socket/network truth。

`driver/net`、`device/net`、logical-interface、control-plane、worker、VFS/fd 与 iomux/epoll
仍是完整责任拓扑中的相邻 owner。它们没有被“四层”计数省略掉，也不因 TCP 自动改由四层
中的任一层重新拥有。

### 默认的 TCP owner 与 handoff 方向

- **Listener 与 accepted child：** 默认由 Stack-side TCP owner 统一拥有 binding/port
  reservation、listener protocol resources、backlog/pending-child admission 与 child
  Endpoint handoff；kernel Socket 只投影 accept predicate，并拥有 fd reservation、地址
  copyout 与 publication rollback。若实现证据要求拆分 Linux-only admission fact，RFC 必须
  给出两个事实的区别、线性化点和 cleanup，而不能建立两份 pending-child queue truth。
- **Active connect 与异步结果：** 默认由 TCP owner 返回 typed started/in-progress/connected/
  failed facts，kernel ABI 再映射首次 `EINPROGRESS`、重复 `EALREADY`、`EISCONN`、readiness
  与最终 errno。protocol failure cause 必须从实际识别它的 owner 无损传到 ABI boundary；
  只观察一个合并的 closed state 后猜测 `ECONNREFUSED`、`ETIMEDOUT` 或 `ECONNRESET` 不构成
  诚实实现。窄 smoltcp fork seam、Stack-side instrumentation 或其它等价路线保持开放。
- **`SO_ERROR`：** protocol outcome 默认从 TCP owner 产生，Linux optlen/copy、errno mapping
  与 query/consume 行为由 Socket ABI owner 编排。具体 consuming handoff 可以随实现决定，
  但不能让 kernel Socket 与 Stack 同时保存可独立推进的 pending-error truth，也不能用恒零
  或 success-no-op 掩盖没有 producer 的状态。
- **Progress 与 wait：** TCP command 默认保持非阻塞；connect、accept、send 与 receive/EOF
  各自读取 owner-defined predicate。Stack transition 产生的 notification 只表示重新检查，
  不携带 ready mask 或最终 errno；blocking、signal/cancel、poll/select/epoll 和 final harvest
  继续由各自 kernel consumer 拥有。现有 source helper 是否扩展、TCP 是否使用 sibling
  family source，以及 invalidation 采用 typed drain、coalesced edge 或其它窄机制均不在此固定。
- **Close 与 protocol reclaim：** opened-description semantic final release 默认先撤销 kernel
  Socket source/publication，再以非阻塞 handoff 请求 TCP owner 推进 release。fd close、Endpoint
  对 kernel 的可见性撤销、FIN/RST、orphan/TIME_WAIT 与 smoltcp resource reclaim 不默认合并为
  一个瞬时动作；具体 deferred state、超时和回收策略由 RFC/实现证据决定。
- **Byte stream handoff：** user pointer 与 smoltcp ring borrow 默认止于各自 object fence；
  send/receive 通过有界、operation-local byte prefix 和明确 commit/consume boundary 表达
  partial progress。它不要求公共 mbuf、统一零拷贝 buffer 或固定 scratch allocation；owned
  chunk、bounded copy、cursor 或其它等价形状都可以在不泄漏 private lifetime 的前提下选择。
- **Resource governance：** Endpoint、listener slot、pending child、RX/TX storage 与 deferred
  reclaim 都必须有唯一 capacity owner，normal exhaustion 形成 typed backpressure/rejection，而
  不是 panic、busy-spin 或无界增长。重要 capacity 默认进入 owner-local Kconfig/build policy；
  具体项目、数值、共享/独立 pool 与 admission 算法不在定位阶段确定。

### 对既有共同边界的默认反馈

frame path、netdev、interface domain、IPv4 control plane、attach、bounded Stack pump、opened-
description、iomux 与 epoll 默认作为 Dependencies 复用，并由 TCP 增加真实 consumer proof；
不能仅因 TCP 到来就机械登记 `Refine`。TCP 应首先拥有自己的 Endpoint/lifecycle/stream
transaction 规则；只有 live implementation 证明现有共同语义确实缺少 async result、terminal
readiness、consuming error、deferred reclaim 或其它跨 consumer 义务时，才对相应 shared
contract 作最小修订。

general Socket/front 同样只扩展被 TCP 和既有 consumer 共同证明的 capability。TCP-local
状态、option policy、listener machinery 或 engine mapping 不因为调用入口相似就上收为通用
framework；反过来，若既有 static dispatch、wait/recheck 或 opened-description lifecycle 已能
自然表达目标，也不为维持旧 positioning 的术语而另建 TCP 专用旁路。

## RFC 设计边界

后续 `net-tcp` RFC 应遵循以下方向：

- TCP 自己拥有的事实和生命周期留在自然的 TCP owner，不提升为共同 Socket 或 network
  truth；既有 shared owner 能自然表达的义务直接复用；
- Contract Impact 从 live effective contracts 与 source 得出，不从本文、历史计划或
  对 TCP 的先验想象得出；
- 不把 `net-tcp` 扩张为完整 BSD Socket framework、网络子系统总重构或未来协议抽象工程；
- 不为了宣称架构稳定而保留不自然的 adapter、重复状态、双路径或 caller-specific
  例外；真实冲突必须回到 owner、contract 或 target review；
- 不把具体 implementation preference、验证命令或普通 corner case 提升成定位层 blocker；
  只有会改变 target、owner、handoff、failure/cleanup、ABI、shared contract、acceptance
  或 validation claim 的问题才阻止 RFC 接受。

如果 TCP 只能通过新增并列状态中心、绕过 general Socket/wait 边界、让 concrete Stack
取得 kernel object、让 kernel Socket 取得协议机私有对象，或建立没有退出条件的 TCP
专用桥才能落地，这不是普通实现困难，而是架构封顶尚未成立的证据。此时 RFC 必须停止
完成声明并重新解析受影响的 owner 或 shared contract，不能以局部成功路径代替架构闭包。

## TCP syscall / ABI 定位级矩阵

下面是本轮定位共识形成的首轮 capability envelope，作为未来公共 RFC 的 R0 输入。它
描述普通用户程序需要的语义族，不是要求实现按文件、函数或固定 syscall trace 机械
逐项照单施工，也不表示这些规则已经写入 current contract。

| 领域 | 定位级能力 | 保护边界 |
| --- | --- | --- |
| 创建 | `socket(AF_INET, SOCK_STREAM, 0 或 IPPROTO_TCP)`；接受 `SOCK_NONBLOCK` 与 `SOCK_CLOEXEC` | 当前 initial-domain IPv4；IPv6 和其它 protocol 稳定拒绝 |
| 地址 | IPv4 `sockaddr_in`；wildcard、loopback、configured local、remote external；`getsockname`、`getpeername` | addrlen、短 output、copy fault 和未连接状态必须有诚实结果 |
| 本地端点 | explicit `bind`，包括 port 0 与地址/端口冲突；`connect` / `listen` 必要时 implicit bind | listener/backlog/port reservation 由 TCP owner 统一拥有；不引入 `SO_REUSEPORT` |
| 被动连接 | `accept`、`accept4` 及其 `SOCK_NONBLOCK` / `SOCK_CLOEXEC` flags；并发 pending child 与 fd rollback | accept predicate 与 connect predicate 分离；accepted child 在 publication 前失败必须清理 |
| 主动连接 | blocking 与 nonblocking `connect`；explicit/implicit local endpoint；loopback 与 external route | 首次 nonblocking attempt 为 `EINPROGRESS`，重复 attempt 为 `EALREADY`，完成结果通过 readiness 与 `SO_ERROR` 取得 |
| 字节流 | `read`、`write`、`readv`、`writev`；ordered/reliable/full-duplex stream；partial progress、backpressure、EOF、FIN、RST | 不把 byte stream 降格为 message/packet 语义；buffered bytes 必须先于 EOF/error 交付 |
| message projection | connected stream 的 `sendto` / `recvfrom`、`sendmsg` / `recvmsg`；single-message iovec；无 ancillary producer | connected send 的 name、receive address、control buffer 与 flags 遵守稳定 ABI；`sendmmsg` / `recvmmsg` 延后 |
| shutdown / close | `SHUT_RD`、`SHUT_WR`、`SHUT_RDWR`；connecting、listening、connected、half-closed、final-release lifecycle | close 不等待完整 Stack reclaim；final release 是 kernel lifecycle trigger，不能由 `Drop` 或 fd number 代替 |
| wait / readiness | blocking、`O_NONBLOCK`、`MSG_DONTWAIT`；既有 `poll` / `select` / `epoll` projection | listener readable、connect writable/error、receive readable/EOF、send writable、`RDHUP` / `HUP` / `ERROR` 分别由 owner fact 投影；不建立 TCP 私有 wait loop |
| descriptor query | `SO_TYPE`、`SO_DOMAIN`、`SO_PROTOCOL`、`SO_ACCEPTCONN` | descriptor 仍是 semantic type 唯一 witness，general Socket 不复制 family truth |
| TCP option | `SO_ERROR` 的真实 query，以及 `SO_REUSEADDR`、`TCP_NODELAY` 的真实 query/mutation | `SO_ERROR` 无 pending error 时才为 0；`SO_REUSEADDR` 参与 bind admission；`TCP_NODELAY` 映射 TCP owner 的 Nagle policy |
| signal / errno | `EAGAIN`、`EINTR`、`EINPROGRESS`、`EALREADY`、`EISCONN`、`ENOTCONN`、`ECONNREFUSED`、`ETIMEDOUT`、`ENETUNREACH`、`ECONNRESET`、`EPIPE` 等普通连接结果 | 失败、取消、peer close、pending error 和 copy fault 不得伪造成成功或互相覆盖；`MSG_NOSIGNAL` 只抑制 `SIGPIPE` |

### 不进入首轮矩阵

首轮不自动承诺 IPv6、`SO_REUSEPORT`、keepalive 全套（包括
`TCP_KEEPIDLE` / `TCP_KEEPINTVL` / `TCP_KEEPCNT`）、linger、socket
timeout、动态 `SO_RCVBUF` / `SO_SNDBUF`、low-water mark、OOB/urgent data、
`MSG_ERRQUEUE`、ancillary producer、`sendmmsg` / `recvmmsg`、`FIONREAD` /
inet-diag、`TCP_INFO`、Fast Open、MPTCP、sendfile/splice/zero-copy 或其它
高级 TCP UAPI。未知 option/flag 稳定返回 `ENOPROTOOPT` 或 `EOPNOTSUPP`，不能
用恒零、success-no-op 或 caller-specific branch 掩盖缺口。

### 矩阵的工程弹性

该矩阵的边缘允许适度工程调整，但保护层次不同：

- TCP 的可靠有序字节流、partial progress、生命周期、failure/cleanup、owner-defined
  predicate、wait/recheck 和 ABI 诚实性是 correctness floor，不能因工程代价而收缩。
- 具体 syscall wrapper、同一语义的 lowering、内部类型/模块/锁/队列和容量实现是
  implementation preference；只要不改变 owner、handoff、visible semantics 或
  current contract，可以在实现阶段自然调整。
- 公共 RFC target 接受前，新 consumer 若证明某个 deferred option/flag 或 message
  projection 是普通 TCP 的共同义务，可以据此扩张矩阵；反之，live consumer audit 若
  证明某个非核心成员没有真实义务，也可以把它收缩为稳定 rejection 或后续项。
- 公共 RFC target 接受后，任何改变 named mandatory item、shared owner、public ABI、
  errno、acceptance 或 validation claim 的收缩/扩张都必须停止并回到 RFC review /
  Target Renegotiation，不能把较弱实现写成原 target closure。

## 用户态验收锚点与依赖

定位阶段曾提出两个互补的首轮 acceptance 锚点：

- CAgent 的本地 IPv4 HTTP workload：保留 server/client、`SO_REUSEADDR`、bind/listen/
  accept、并发连接、send/recv/close 与日志判定；judge 的 glibc/musl 读取方式不改变
  TCP target。
- musl 用户态的 HTTPS consumer：`curl` / `wget` 获取固定对象并校验内容，
  `git clone --depth=1 https://...` 校验固定仓库的 commit 或文件。SSH、Git LFS、认证、
  submodule、HTTP/2、代理和 IPv6 不从这些场景外推。

DNS、TLS/CA、系统时间、随机源、工具二进制和 rootfs 部署是各自 owner 的依赖，不由
TCP protocol owner 解释。现有 UDP resolver contract 只把未截断 IPv4 A 查询的 musl
路径作为条件性 consumer；若实际 musl resolver 触发 DNS-over-TCP fallback，它必须
自然使用上述普通 stream 能力，不能为 resolver 建立专用 TCP 旁路。glibc resolver 因
`IP_RECVERR` / pending-error 依赖继续 Not Supported / Not Cut Over，不属于本轮 TCP
mandatory acceptance。

## 未来 RFC 的 closure claim

未来公共 RFC 可以把“当前网络架构进入稳定扩展期”作为 closure claim，但必须由本 RFC
的真实实现与验证共同支持。至少需要证明：

- TCP 的每项长期责任都能落入唯一且自然的 owner；
- TCP 没有建立第二套 Socket、wait、control-plane、frame-path 或 protocol progression
  架构；
- 对共享 contract 的修订具有真实跨 consumer 含义，而不是把 TCP 私有需求包装成公共层；
- 既有 UDP、ICMP raw 与 Unix Socket consumer 仍能在同一共同边界内成立；
- closure 明确列出尚未证明的部署与能力范围，不把 Not Run 或 non-goal 写成已经被架构
  覆盖。

满足这些条件后，后续普通网络工作应以增量扩展为默认路线。若其中任一条件不能成立，
`net-tcp` 仍可以继续讨论 TCP target，但不得同时宣称现有网络架构已经封顶。

## 冻结与提升

定位阶段未固定具体 TCP 状态转换、错误映射的全部细节、buffer 策略、协议算法、资源数值、
内部 API、模块拆分、implementation stage 或测试命令；上面的矩阵只记录当时的能力族、
保护边界与验收方向。

这些结论已经由父 RFC 结合 current contracts、live source 与后续 review 重新核验并转写为
canonical Draft。父 RFC 对 target、non-goals、owner、ABI、Contract Impact、acceptance 与
validation claim 的表述覆盖本文；本文从此冻结为历史背景，不再维护，也不自动授权实现。
