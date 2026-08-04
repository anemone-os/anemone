# IPv4 UDP Socket 能力扩展目标与不变量

**状态：** Draft
**最后更新：** 2026-08-04
**父 RFC：** [RFC-20260804-udp-socket-extension](./index.md)
**适用修订：** Draft

本文只定义本 RFC 的 target/contract proof obligations。当前 effective UDP、Socket、
Network、Opened-description、IOMUX 与 Epoll 规则仍以 `docs/src/contracts/` 为准；本文
在 cutover 前不是 current contract。

## 规则分类

- Correctness Invariant：唯一 owner、并发、lifecycle、cleanup、内存安全和 ABI 诚实性；
  不得通过 target reduction 或实现便利降低。
- Target Guarantee / Capability：R0 承诺的 connected UDP 与单消息/vector ABI；只有
  Target Renegotiation 可以改变。
- Implementation Preference：类型、字段、锁、helper、队列算法、模块布局、内部 phase、
  capability 物理签名与测试命令；本文不冻结。

## 状态所有权

R0 需要的 protocol facts 保持正交，不建立一份由 Socket front 共同拥有的
“connection state”：

| Fact | Target-level state | 唯一 Owner | 非 owner 持有内容 |
| --- | --- | --- | --- |
| local binding | absent / committed local address and port | Stack UDP Endpoint | opaque identity、point-in-time query |
| peer association | absent / committed IPv4 peer | Stack UDP Endpoint | normalized request、point-in-time query |
| receive queue | admitted datagrams及peer metadata | Stack UDP Endpoint | detach/peek outcome |
| route/source/interface selection | operation-local selection | IPv4 control plane | 当前 operation 的 immutable result |
| Linux ABI与blocking choice | flags、sockaddr、copy cursor、errno mapping | Socket ABI/UDP family | typed semantic request/outcome |
| wait/watch/delivery | wait round、watch、delivery policy | syscall/iomux/epoll consumer | source snapshot、non-owning route |

binding、peer 与 queue 都属于同一个 Endpoint lifecycle，但它们不是同一事实。disconnect
只改变 peer；reconnect 不改变已完成 admission 的 queued datagram；final release 才撤销
整个 Socket/Endpoint association。

## Target Invariants

### UDP-EXT-OWNER-001 — Peer/filter truth只存在于Endpoint owner

**分类：** Correctness Invariant。

**规则：** Stack UDP Endpoint 唯一拥有 persistent peer association 与基于该 peer 的
ingress admission truth。kernel UDP Socket/front 不缓存 peer、connected bit、filter、
queue count 或由这些事实派生的 readiness；`getpeername`、send destination selection、
receive admission 与 poll 都通过窄 capability 读取 Endpoint current fact/outcome。

control plane 的 route/source/interface selection 只服务当前 connect/send operation；
它不能成为 Socket 与 Endpoint 之间的第二份 persistent route/peer truth。若实现为了
性能保留 route cache，必须另行证明其 source of truth、staleness 与 invalidation，且
不得由 cache 反向驱动 peer association。

**违反表现：** `UdpSocketFile` 与 Stack 各保存 connected peer；Socket 在 receive 时
用本地 peer 二次过滤；poll 缓存 ready mask；control-plane selection 被当作 persistent
association；stale route/peer 让旧 operation 命中新 Endpoint generation。

**Proof：** source audit、peer query/transition tests、late invalidation 与 endpoint reuse
isolation；R0 review 必须确认 cross-crate surface 没有暴露 Stack-private object。

### UDP-EXT-CONNECT-001 — Connect/reconnect只有一个Endpoint commit

**分类：** Correctness Invariant / Target Guarantee。

**规则：** IPv4 `connect` 的 semantic sequence 是：ABI 完整 normalize/copy peer ->
control plane 取得 operation-local route/source selection -> Endpoint owner 在一次 commit
中验证 live identity、完成必要的 implicit bind 并替换 peer association。commit 前失败
不改变 binding 或 peer；commit 后新 peer 立即成为后续 send default 与 ingress admission
事实。

初次 connect 与 reconnect 使用同一 transaction。reconnect 到同一或不同 peer 均合法；
失败保持旧 peer。`connect(AF_UNSPEC)` 只清 peer，保留 committed local binding、port
reservation 与已入队 datagram；对 already-unconnected Endpoint 保持幂等。UDP connect
不进入 capacity wait，`O_NONBLOCK` 不产生 `EINPROGRESS` protocol。

Endpoint commit 完成后，fact invalidation/notification 必须在 owner guard 外发布；hint
只要求 consumer 重算，不携带 peer、errno 或 ready result。

**违反表现：** implicit bind 已发布但 peer commit 失败；reconnect 失败清除旧 peer；
disconnect 释放 local port；Socket 与 Stack 分两步发布 connected state；持有 Endpoint
guard 进入 route lookup、user copy、sleep 或 notification callback。

**Proof：** unbound/bound connect、same/different-peer reconnect、AF_UNSPEC disconnect、
route/source failure、port exhaustion/conflict、copy fault 与 concurrent query matrix。

### UDP-EXT-ADMISSION-001 — Peer filter只约束新的ingress admission

**分类：** Correctness Invariant / Target Guarantee。

**规则：** 每个 inbound datagram 在 Endpoint owner admission 时读取 current peer fact。
peer absent 时按普通 UDP binding/demux 规则接纳；peer present 时只有 matching peer 的新
datagram 可以进入 queue。admission commit 后，该 datagram 及其 peer metadata 已获得
独立于后续 peer transition 的 queue ownership。

connect、reconnect 与 disconnect 不回溯扫描、重分类或清空已经入队的 datagram。旧 peer
下已接纳的数据仍可被 receive/peek 读取，并继续产生 ordinary receive readiness；transition
只改变之后的 ingress admission。

**违反表现：** Socket receive 二次过滤队首；reconnect 清空 queue；queue item 不保留
实际 sender metadata；wrong-peer ingress 先入队再由 reader 丢弃；queue nonempty 与
readable predicate 使用不同 admission truth。

**Proof：** pre-connect queued datagram、connected wrong/right peer、reconnect 前后 queue、
disconnect 后新 ingress、peek/consume 与 poll/select/epoll readiness matrix。

### UDP-EXT-SEND-001 — Default peer与显式destination不制造第二份association

**分类：** Correctness Invariant / Target Guarantee。

**规则：** 每次 send operation 只形成一个 operation-local destination：显式 `sendto`
address / `sendmsg.msg_name` 优先；否则读取 Endpoint current peer；两者都不存在时返回
`EDESTADDRREQ`。显式 destination 在 connected Socket 上只覆盖本次 send，不改变 peer、
receive filter 或 `getpeername`。

destination 决定后，send 继续服从 current UDP transaction：完成 bounded user-copy、
size validation、route/source/interface selection、implicit bind（如仍需要）与 Endpoint
capacity admission后，一次提交完整 datagram。任何 commit 前 failure 不发布 payload；
commit 后 ordinary external loss 不回写 syscall result。单消息 vector send 不报告半个
datagram 的 partial success。

**违反表现：** explicit destination 永久改写 peer；connected send 复制 peer 到 Socket；
iovec 前缀已发送而后缀 fault；route/source failure 发生在 successful admission 后；
capacity retry 跨 sleep 保留 owner guard 或可重复 commit token。

**Proof：** connected/unconnected explicit/implicit destination、reconnect race、zero/maximum/
oversize、multi-iovec、fault at each segment、capacity saturation/retry 与 route/source error。

### UDP-EXT-RECEIVE-001 — Datagram detach、peek与copyout保持唯一owner

**分类：** Correctness Invariant / Target Guarantee。

**规则：** ordinary receive 的线性化点仍是 Endpoint 将队首完整 datagram 原子 detach 给
operation-local kernel transaction。detach 前 Endpoint 独占 payload；detach 后 kernel
transaction 独占。short scalar/vector buffer 只复制 prefix 但消费整个 datagram；
`MSG_TRUNC` 的 returned length 与 output flag 服从 R0 Linux ABI oracle。

`MSG_PEEK` 只观察 current queue item，不转移 ownership、不消费 queue/capacity；success、
short 或 user-copy fault 后都保持该 datagram 可再次读取。ordinary non-peek receive 在
detach 后发生 payload、peer address、addrlen、`msghdr` output 或 control-length copyout
fault 时不 requeue、不重排，并按 ABI adapter 的 copy ordering 返回错误/可见 prefix。

zero-length datagram payload 是合法 message。zero-capacity scalar/vector receive 与
`MSG_TRUNC` 组合的精确 return/consume 结果必须由 Linux 6.6.32 focused oracle 固化；
不得因此改变 detach ownership 或引入 family-specific user-pointer handling。

**违反表现：** Endpoint 与 kernel 同时访问 detached payload；copy fault 后 requeue；
short receive 只消费 prefix；peek 暂时 detach 再补回；family owner 接收 raw user pointer
或写 `msghdr`；不同 receive API 使用不同 queue truth。

**Proof：** scalar/vector zero/short/exact/oversize、peek+truncate、payload/name/addrlen/header
fault、concurrent readers 与 readiness/capacity recovery matrix。

### UDP-EXT-ABI-001 — Linux message ABI止于共同adapter

**分类：** Correctness Invariant / Target Guarantee。

**规则：** raw `msghdr`、`iovec`、sockaddr、flag、user pointer、length、copyout ordering 与
Linux errno 止于 Socket ABI adapter。family-neutral layer形成 normalized destination、
bounded vector cursor、receive sink 与 flags；UDP family只返回 typed success/not-ready/
rejection，不解析 Linux bits、struct layout 或 raw pointer。

R0 send flags只包括`MSG_DONTWAIT | MSG_NOSIGNAL`；receive flags只包括
`MSG_DONTWAIT | MSG_PEEK | MSG_TRUNC`。UDP没有SIGPIPE producer时，`MSG_NOSIGNAL`
可以作为带一次性诊断的 compatibility no-op；该桥必须有注释说明producer假设与退出条件。
其它flag返回`EOPNOTSUPP`。

`sendmsg` 的非空 ancillary/control message 返回 `EOPNOTSUPP`。`recvmsg` 可接收 control
buffer capacity，但在没有 ancillary producer 时返回空 control region；这不建立 option、
control-message registry 或 future producer slot。unsupported option 返回 `ENOPROTOOPT`；
`SO_ERROR` 不返回永久零值。

single-message vector import 与普通 `readv/writev` 共用 kernel I/O
`max_iovec_count` Kconfig 参数，不允许 UDP family 保存或解释另一份 limit。R0 默认与
acceptance 配置为 1024，并与公开 `IOV_MAX` 一致；配置不得高于公开上限，低于 1024 的
reduced-capacity profile 不构成完整 R0 ABI evidence。超限 errno、overflow、field copyout、
output flags 与 fault ordering 由共同 Socket ABI adapter 按固定 Linux 6.6.32 oracle 投影。

**违反表现：** UDP family解析 `MSG_*`；adapter按 resolver/app 分支；unknown cmsg 被静默
忽略并成功；`SO_ERROR` 恒零；iovec fault 已提交 datagram；为 message ABI 建立 future
family registry 或通用 option bag。

**Proof：** layout/length/overflow/fault oracle、unsupported flags/control/options、mandatory
musl wrapper calls、glibc `IP_RECVERR` rejection boundary 与固定 source audit。

### UDP-EXT-WAIT-001 — 各operation只等待自己的EAGAIN predicate

**分类：** Correctness Invariant。

**规则：** receive/read 等待 Endpoint receive queue nonempty；send/write 只在当前 Endpoint
bounded admission capacity 返回 would-block 时等待。缺少 destination、invalid address、
无 route/source/interface、message too long 与 unsupported flag/option 都是立即结果，不能
转成 wait。connect/disconnect 本 R0 不进入 wait protocol。

blocking、`O_NONBLOCK`、`SOCK_NONBLOCK` 与 `MSG_DONTWAIT` 读取相同 owner predicate，
只改变 would-block 的 Linux projection。poll/select/epoll 复用 source 的 snapshot/register/
recheck，不承诺任意 destination、length 或 route 都可成功发送；notification 只提示重算。

**违反表现：** connected bit直接成为 writable；route absence 永久清 writable；family
内部第二套 wait loop；跨 sleep 持有 peer/connect phase；callback payload直接决定 syscall；
per-call flag修改 opened-description status。

**Proof：** initial writable、unconnected write immediate error、capacity saturation/recovery、
receive peer-filter queue、multi-waiter/signal、poll/select/epoll、late/duplicate hint matrix。

### UDP-EXT-LIFECYCLE-001 — Peer state服从现有opened-description与Endpoint generation

**分类：** Correctness Invariant。

**规则：** peer association、binding、queue 与 capacity 都属于 active Endpoint identity。
dup/fork aliases共享同一 opened description 与 Endpoint；关闭非最后 alias 不清 peer、
binding或queue。semantic final release 先撤销 Socket source publication/association/observer
routes，再发起 non-blocking Endpoint retire；waiter不拥有或延长 Endpoint lifecycle。

Stack retire 先撤销 active identity、peer、binding reservation 与 new admission，再清理
queue/private engine。late cleanup、old operation、old invalidation 或 stale identity 不得
恢复 peer、释放新 port owner 或向新 Endpoint 交付旧 datagram。

**违反表现：** raw fd或`Drop`决定 disconnect/retire；one-alias close清 peer；final release
等待 worker；wait route持有 Endpoint strong lifecycle；旧 reconnect/send commit命中新
generation；retire 先清 buffer 后撤销 publication。

**Proof：** dup/fork/CLOEXEC/one-alias/final close、connect/send/receive 与 close race、port
reuse、late hint、duplicate retire 与 orderly network shutdown matrix。

## RFC-local Proof Obligations

- R0 Contract Impact 已固定为 Refine `NET-SOCKET-ENDPOINT-001`、
  `NET-UDP-TRANSACTION-001`、`SOCKET-ABI-001`；front、wait 与其它未变化 contract 只列
  Dependencies。实现若要求改变 shared front/wait rule，必须回到 RFC review。
- `sendmsg/recvmsg` 复用 shared iovec bound；overflow、field copy ordering、output
  `msg_flags`、name/control length 与 zero-capacity behavior 必须形成可重复 Linux 6.6.32
  focused oracle，但这些是 ABI adapter implementation/proof obligation，不是开放 target。
- mandatory resolver 已固定为 musl IPv4 `getaddrinfo` path：musl 1.2.0 的
  `socket/bind/sendto/poll/recvfrom`、musl 1.2.5 的
  `socket/bind/sendto/poll/recvmsg`，以及两版结果排序使用的
  `socket/connect/getsockname/close` 都落在 R0。acceptance 只接受未设置 TC 的 IPv4 A
  response；1.2.5 TCP fallback 不在 R0。
- glibc 2.35/2.38 在 UDP resolver 建立时强依赖 `IP_RECVERR`，因此明确 Not Supported /
  Not Cut Over。不得以 option success-no-op 宣称 glibc resolver PASS；要改变该结论必须回到
  RFC review 并扩展 error owner、queue/pending state、poll/ordinary I/O error projection 与
  `MSG_ERRQUEUE`/`SO_ERROR` ABI proof。
- owner-local proof、host oracle、RV64 guest、LA64 guest 与 external path claim 必须分开；
  DNS success 不替代 non-DNS UDP ABI coverage；final harness 与其它 Not Run 范围不是 R0
  closure 前置。
- 如果实现需要 probe、多个独立 cutover 或不安全中间态，先创建 `implementation.md`
  并写明 hypothesis/protected boundary/failure signal/exit；Draft 本身不授权该动作。

## 禁止退化项

- Socket/Endpoint 两份 peer/filter/queue/readiness truth；
- family ops接收task、fd、raw pointer、Linux flag或errno policy；
- 为 DNS/resolver/app/test 建立 caller-specific success path；
- unsupported option/control message 恒零或静默成功；
- copy fault后requeue、vector前缀datagram提交或跨sleep保留commit authority；
- 为 future TCP/IPv6/batch/ancillary data 建立 registry、option bag、connection state machine
  或第二层 dispatch；
- 通过降低双架构、external path、copy/fault或lifecycle evidence 来换取 cutover。
