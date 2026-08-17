# IPv4 TCP Socket 当前契约

**Contract IDs：** `NET-TCP-ENDPOINT-001`、`NET-TCP-STREAM-001`、`NET-TCP-LIFECYCLE-001`
**状态：** Active
**Owner：** initial-domain Stack TCP owner唯一拥有Endpoint、association、stream与protocol lifecycle；kernel TCP Socket只拥有Linux ABI投影、blocking orchestration与opened-description integration
**参与领域：** IPv4 control plane / domain Stack TCP / Socket ABI / opened description / iomux / epoll / frame progression
**覆盖范围：** initial-domain IPv4 TCP bind/listen/connect/accept、可靠字节流、FIN/RST/shutdown、owner-defined readiness、publication与deferred reclaim
**不覆盖：** IPv6、runtime network reconfiguration、`SO_REUSEPORT`、keepalive/linger/timeout、OOB、error queue、ancillary、zero-copy、sock-diag、`/proc/net/tcp`或通用BSD Socket framework
**实现位置：** `anemone-kernel/crates/anemone-net-api/src/tcp.rs`、`anemone-kernel/crates/anemone-smoltcp-stack/src/tcp/`、`anemone-kernel/src/fs/socket/tcp/`
**依赖：** `NET-CONTROL-PLANE-001`、`NET-STACK-PUMP-001`、`NET-PROTOCOL-BOUNDARY-001`、`NET-SOCKET-WAIT-001`、`SOCKET-FRONT-001`、`SOCKET-ABI-001`、`SOCKET-WAIT-001`、`OPENED-DESC-001..003`、`IOMUX-POLL-001..003`、`EPOLL-WATCH-001`、`EPOLL-READY-001`
**Pending Successor：** None
**最后核验：** 2026-08-17

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 非 owner持有什么 | 行为用途 |
| --- | --- | --- | --- |
| local binding、port reservation、role与connection outcome | Stack TCP owner | opaque Endpoint identity、typed request/outcome | bind/listen/connect与query |
| logical listener、private ingress projection、pending/claimed child与aggregate backlog admission | Stack TCP owner | accept predicate与一次性child handoff capability | bounded passive-open与accept rollback |
| RX/TX bytes、有效buffer预算、FIN/RST、shutdown direction、async cause与option fact | Stack TCP owner | point-in-time facts与operation-local reservation | stream commit、admission、error与readiness |
| local-address validation及active route、source与interface selection | IPv4 control plane | operation-local immutable selection | bind validation与active egress的唯一policy decision |
| Linux tuple、sockaddr、flags、copy、errno与signal | Socket ABI adapter | normalized request/value与typed outcome | Linux-visible projection |
| fd publication与semantic final release | opened-description owner | static final-release hook与unpublished reservation | rollback、dup/fork与exactly-once retirement |
| engine slot、timer、TIME_WAIT与deferred reclaim | Stack TCP owner | opaque invalidation/progression obligation | protocol progression与bounded reuse |

opaque identity、snapshot、reservation和notification都不是并列状态真相。kernel不得取得smoltcp handle、ring、engine
slot或generation来推进TCP行为；Stack不得取得task、fd、user pointer或Linux errno。

## NET-TCP-ENDPOINT-001 — Endpoint、listener与connection outcome由Stack TCP owner统一拥有

**规则：** Stack TCP owner唯一拥有Endpoint identity、explicit/implicit bind、port reservation、role、logical
listener、private ingress projection、pending/claimed child、active connection outcome与accepted-child lifecycle。IPv4
control plane只提供local-address验证或active operation的route/source/interface selection；listener ingress path由
Domain Stack从已经committed的boot-static topology解析。Socket只投影Linux address、fd与blocking语义，不选择listener
interface，也不复制binding、listener queue、connected state或async cause。

`listen(backlog)`不执行active egress selection；wildcard binding投影到local与已配置external path，loopback-specific
只投影到local，configured external specific同时投影到local self-connect与对应external ingress。没有external
deployment时wildcard退化为local-only。每条private engine完成transport handshake只形成candidate；只有Stack TCP owner在
同一guard内把candidate转换为`Pending`时才消费aggregate backlog，`Pending + Claimed` slot phase是occupancy唯一真相。
repeated listen只更新同一个Linux-normalized limit；shrink不驱逐既有child，新candidate在occupancy回落前被拒绝。

accept predicate只由aggregate pending child fact产生；opaque child capability每次claim/take/cancel都重新校验实际
projection、slot与generation。child从listener移交给Socket后，peer-address copy或fd publication失败必须消费该handoff
并清理child，不能重新插队或留下不可达association。nonblocking connect保留started/in-progress/connected/failed真实
outcome；blocking connect等待并重读同一outcome，不建立第二状态机。handshake RST、transport timeout与local
selection/admission failure保持可区分typed cause，不从merged closed state猜测结果。

`SO_REUSEADDR`是TCP owner option fact，并参与后续bind admission；它不允许duplicate live listener、绕过完整4-tuple
uniqueness或模拟`SO_REUSEPORT`。capacity full、stale identity与invalid role均返回typed rejection/backpressure，不能
panic、busy-spin、fallback到private handle或建立第二registry。

每个idle、bound、listener或connection Endpoint还唯一拥有方向性有效send/receive预算；Kconfig固定ring容量只是
物理上限。accepted child在Stack handoff时复制listener当前预算，之后独立；共享opened description及`dup`只观察同一
Endpoint truth。engine中的capacity limit只是owner预算的协议投影，不能反向驱动Endpoint query或被其它层独立修改。

**违反表现：** Socket缓存local/peer/role或connect result；control plane拥有port reservation或为listen伪造destination
selection；per-interface projection分别维护backlog/queue/credit；engine completed state直接成为pending truth；copyout失败后
child仍可accept；timeout/RST合并后猜errno；或test/caller直接选择engine slot。

**验证 / Enforcement：** owner host tests与focused smoltcp TCP覆盖path matrix、aggregate backlog、capacity、cause、
generation reuse、pending-child rollback和all-projection deferred reclaim；kernel KUnit、长期`socket-test` TCP suite及
RV64/LA64 glibc/musl oracle覆盖tuple、bind、listen、local self-connect、hostfwd ingress、blocking/nonblocking
connect、accept/accept4、name/query、fault与fd rollback。

**最初来源：** [IPv4 TCP Socket RFC R0](../../rfcs/net-tcp/index.md)的`NET-TCP-CUTOVER`。

**当前来源：** 同上；Checkpoint 5A closure，以及
[TCP listener ingress publication RFC R0](../../rfcs/tcp-listener-ingress-publication/index.md)的
`TCP-LISTENER-INGRESS-CUTOVER`与focused Git/PR evidence；随后由
[TCP Socket Buffer Budgets小迭代](../../devlog/changes/2026-08-17-tcp-socket-buffer-budgets.md) Refine有效预算与accept继承。

## NET-TCP-STREAM-001 — 字节流commit、terminal precedence与readiness读取owner fact

**规则：** Stack TCP owner唯一拥有ordered full-duplex byte stream、RX/TX capacity、FIN/RST、shutdown direction、
pending async cause与`TCP_NODELAY`。send/receive transaction只提交已经成功copy且被owner接受或消费的prefix；operation-
local receive reservation必须exactly once resolve，不延长opened-description lifecycle。已缓冲receive bytes先于EOF或
terminal error交付；orderly FIN在buffer耗尽后返回0，established RST产生`ECONNRESET`，不得伪装成EOF。

普通`SO_SNDBUF/SO_RCVBUF`选择的有效预算限制后续send admission与advertised receive window，但不重分配固定ring。
缩小到current occupancy以下不得丢弃已排队bytes：send admission与advertised receive window保持关闭，直到队列排空
到新预算以下；此前窗口已授权的in-flight receive bytes仍可进入物理ring。增大必须经既有invalidation/progression使
blocked sender与receive-window重新检查。`getsockopt`报告owner实际采用的预算而非allocation大小；本契约不据此承诺
autotuning、sysctl/memcg accounting或内存回收。

connect、accept、send与receive/EOF分别读取对应owner-defined predicate。Socket、poll/select与epoll只使用point-in-
time fact和`snapshot -> register -> recheck/final scan`；invalidation只提示重算，不携带ready mask、errno或结果。
`O_NONBLOCK`、creation-time `SOCK_NONBLOCK`与per-call `MSG_DONTWAIT`读取同一predicate，per-call flag不修改opened-
description status。local write shutdown或其它broken-stream send返回`EPIPE`并产生`SIGPIPE`，仅本次
`MSG_NOSIGNAL`可以抑制信号。

TCP owner产生的pump外committed mutation必须在返回前形成move-only progression obligation，由既有Stack worker
handoff推进；TCP、UDP与ICMP raw共享handoff协议但各自决定自己的effect。不得依赖caller手工wake、无关IRQ/traffic、
periodic poll或第二deadline/worker truth。

**违反表现：** Socket缓存buffer count、EOF、ready mask或pending error；RST-as-EOF；copy fault提交未复制bytes；
blocking路径维护私有phase；notification直接决定return；caller按TCP operation补wake；或`MSG_DONTWAIT`修改file status。

**验证 / Enforcement：** scalar/vector/message partial/fault、peek、FIN/RST/shutdown、SIGPIPE/NOSIGNAL、consuming
`SO_ERROR`、poll/select/epoll与multi-waiter KUnit/host proof；双架构双libc focused oracle、self/remote-external peer及
并发CAgent HTTP consumer。

**最初来源：** [IPv4 TCP Socket RFC R0](../../rfcs/net-tcp/index.md)的`NET-TCP-CUTOVER`。

**当前来源：** 同上；Checkpoint 5A closure与focused Git/PR evidence；随后由
[TCP Socket Buffer Budgets小迭代](../../devlog/changes/2026-08-17-tcp-socket-buffer-budgets.md) Refine有效预算的stream行为。

## NET-TCP-LIFECYCLE-001 — Socket publication与protocol reclaim分离

**规则：** opened-description owner的`Unpublished -> Live(n) -> Retired`决定fd可见性与semantic final release；最后一个
published alias只触发一次non-blocking TCP release handoff。temporary `Arc`、raw fd、Rust `Drop`、FIN/RST、peer close、
TIME_WAIT或engine reclaim都不能替代该触发点。TCP owner随后唯一拥有observer withdrawal、association terminal、orphan、
timer、TIME_WAIT和bounded deferred reclaim，并以generation隔离晚到packet、hint或旧capability。

close不等待FIN/RST/TIME_WAIT或完整engine reclaim。cleanup先撤销publication/admission与observer route，再释放对应
reservation/child/Endpoint；晚到或重复notification只触发fail-closed recheck，不能恢复retired Endpoint。valid bounded
internal allocation遭遇全局OOM可以kernel-fatal，但syscall输入上界与owner capacity full必须在commit前分别形成typed
rejection，不能用allocator失败代替容量协议。

**违反表现：** 关闭非最后alias推进protocol close；`Drop`或引用计数决定final release；Socket与Stack各自保存retired
truth；final close等待网络握手；generation reuse接收旧edge；或capacity full形成unbounded leak/panic。

**验证 / Enforcement：** dup/fork/CLOEXEC/final-close、accept publication rollback、receive/final-close race、late
invalidation、generation reuse、orphan/TIME_WAIT/deferred reclaim与shutdown-order host/KUnit/runtime evidence；source audit
确认每个cleanup transition只有一个owner。

**最初来源：** [IPv4 TCP Socket RFC R0](../../rfcs/net-tcp/index.md)的`NET-TCP-CUTOVER`。

**当前来源：** 同上；Checkpoint 5A closure与focused Git/PR evidence。

## 当前接受边界

- effective tuple为initial-domain IPv4 `AF_INET + SOCK_STREAM + 0/IPPROTO_TCP`；覆盖loopback、configured self-external与remote-external。boot-static listener path上限为local加至多一个configured external projection。
- 原TCP closure evidence覆盖owner host `20/20`、focused smoltcp TCP `178/178`、RV64 KUnit `466/466`、LA64 KUnit `471/471`、四份glibc/musl C consumer、双架构`socket-test` TCP `8/8`、remote peer stream/RST `4/4`、并发CAgent与shared Socket/UDP/ICMP raw/Unix回归。
- listener-ingress closure新增owner host `10/10`，并通过完整`just test net-host`与xtask `93/93`；RV64 `639/639`、LA64 `642/642` KUnit及双架构双libc focused consumer均通过wildcard/external-specific local self-connect与hostfwd ingress、loopback hostfwd负向case、raw sock-diag、`ss -tan`、remote stream/FIN/RST和CAgent回归。
- RV64 orderly shutdown且wrapper exit 0；LA64完成`filesystem -> network -> device -> PowerOff`后因无成功power-off handler停在halt并人工终止，不能作为wrapper exit-0证据。
- TCP socket-buffer预算增量通过public xref Linux 6.6.32 source review、完整`just test net-host`、xtask `109/109`、
  RV64 KUnit `776/776`与focused `SOCKBUFTEST 2/2`；LA64按本迭代授权Not Run。
- physical hardware、`smp > 1`、其它NIC/provider/deployment、runtime hotplug、多external interface、IPv6、full network LTP、完整final harness及压力/长时backlog均Not Run。
