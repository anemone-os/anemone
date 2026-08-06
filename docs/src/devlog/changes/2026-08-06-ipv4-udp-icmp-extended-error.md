# ANE-CHG-20260806-ipv4-udp-icmp-extended-error

**Type:** Small Feature / contract-bearing local cutover
**Status:** Completed
**Date:** 2026-08-06
**Authors:** doruche, Codex
**Area:** IPv4 / UDP / ICMP / Socket ABI / iomux

## Problem / Context

决赛环境中未修改的glibc BusyBox能够向numeric IPv4地址发包，却不能完成hostname lookup。glibc IPv4 UDP
resolver在`connect`前无条件启用`IP_RECVERR`，Anemone原先返回`ENOPROTOOPT`并终止nameserver attempt；AF_UNSPEC
路径还会用`sendmmsg(vlen=2)`发送并行A/AAAA查询。只让option返回成功、让`SO_ERROR`恒零或对libc/BusyBox做特判，
都会把没有producer和consuming semantics的表面兼容伪装成Linux ABI支持。

现有UDP Endpoint已经唯一拥有binding、peer、datagram queue与lifecycle，ICMP raw和UDP可以观察同一份normal admitted
IPv4 packet。本轮因此在该owner边界内加入真实、bounded的IPv4 ICMP extended-error channel，并在共同Socket message
adapter中加入逐条复用既有single-message transaction的最小`sendmmsg`。DNS policy、route/source selection、ordinary
datagram transaction、iomux/epoll wait protocol与opened-description final release均保持原owner。

## Decision

- `IP_RECVERR`是UDP Endpoint拥有的真实enable truth。启用后，normal IPv4 admission中的合法ICMP Destination
  Unreachable与Time Exceeded可以按quoted IPv4/UDP tuple匹配当前connected或unconnected Endpoint；malformed、
  fragmented、non-UDP、quote不足、tuple不匹配或disabled packet不改变Endpoint。
- Endpoint同时拥有bounded FIFO和一个ordinary pending-error slot。每个matching error更新pending slot；FIFO满或record
  allocation失败时丢弃新record但保留已有顺序和pending error。disable停止后续admission并清空FIFO，但不撤回已经发布的
  pending error；retire/final release清除全部error state并保持identity reuse isolation。
- ordinary send在新datagram commit前消费pending error；ordinary receive先交付已经queued的数据，数据为空时再消费。
  `SO_ERROR`与ordinary I/O竞争同一个consuming pending slot。ERROR predicate由pending slot或FIFO非空投影，继续沿既有
  `snapshot -> register -> recheck/final scan`协议通知poll/select/epoll。
- `recvmsg(MSG_ERRQUEUE)`从FIFO detach一个move-only record，再由Socket ABI adapter投影quoted UDP payload、original
  destination、offender、`SOL_IP/IP_RECVERR` cmsg、`sock_extended_err`与输出flags。short data/control分别设置
  `MSG_TRUNC`/`MSG_CTRUNC`；empty queue立即返回`EAGAIN`；detach后的copy fault消费该record且不requeue。
- Destination Unreachable codes 0..15及未知code按Linux errno family投影；Port Unreachable为`ECONNREFUSED`，
  Fragmentation Needed为`EMSGSIZE`并在可得时投影quoted MTU，Time Exceeded为`EHOSTUNREACH`。本轮不建立PMTU cache。
- `sendmmsg`在一个稳定opened description上顺序执行既有`sendmsg` transaction，逐条写回`msg_len`；首条失败返回errno，
  已有成功后遇到send/copyout failure返回完成数，partial stream message停止后续条目，`vlen`按Linux上限clamp到1024。
- userspace oracle只断言已发布的Linux UAPI，不冻结Endpoint内部queue priority、owner handoff、allocation path或未发布
  family能力。parser admission、move-only handoff、retire/reuse与owner-local data-before-pending策略由host/KUnit证明。

## Implementation Boundary

domain Stack UDP Endpoint唯一拥有enable truth、FIFO、pending slot、capacity、overflow与cleanup；protocol composition和UDP
owner解析admitted ICMP、验证quote并查找current Endpoint；Socket adapter唯一拥有Linux option、errno、sockaddr/cmsg、
message header与user-copy projection；UDP source只把fresh Endpoint facts投影为readiness。error record从FIFO detach后由
当前syscall transaction唯一拥有，Stack不持有task、fd、user pointer或Linux ABI buffer。

本轮不引入kernel DNS、resolver/caller特判、generic asynchronous-error/cmsg framework、shared mutable option bag、
historical egress journal、packet-generation correlation、PMTU cache、IPv6/ICMPv6、local-origin error、TCP/raw error queue、
`recvmmsg`或batch-owned state。标准ICMP quote只能按current tuple归属，不能提供close/port reuse前后的packet-exact
historical attribution。

## Change

- 增加protocol-domain `UdpErrorCause`、move-only `UdpErrorRecord`、error facts与per-Endpoint Kconfig capacity；concrete
  Stack通过production packet admission解析ICMP、限制quoted IPv4 `total_len`、匹配live Endpoint并维护FIFO/pending。
- UDP option和operation capability增加`IP_RECVERR`、consuming `SO_ERROR`、`MSG_ERRQUEUE` detach及ERROR projection；
  Socket ABI增加Linux constants/layout、cmsg/name/flags/errno与fault/truncation规则。
- 增加RV64/LA64 syscall 269和family-neutral `sendmmsg` loop；每条message仍由既有family `sendmsg` semantic transaction
  拥有，不增加batch queue、shared commit或family-specific fast path。
- 新增repository-owned `udp-errqueue-oracle`，从同一C source构建host、RV64 glibc与RV64 musl consumer；guest deterministic
  case经loopback UDP egress、closed-port ICMP generation、normal admitted ingress、Endpoint publication直至
  poll/epoll、`SO_ERROR`与`MSG_ERRQUEUE`完成完整链路。
- owner-local host topology覆盖malformed/short/non-UDP/fragment quote、specific/wildcard lookup、raw fanout、FIFO/pending、
  retire/reuse与overflow；inline KUnit覆盖option/message/readiness/copy/lifecycle边界。Unix blocking harness对预期
  `nanosleep(EINTR)`重试，避免把signal interruption误报为UDP阻塞。
- 独立初审发现detached payload二次分配、quoted `total_len`边界不足和private-boundary coverage不足；修复后record以
  `into_parts()`移动payload，parser同时约束UDP header/payload并忽略声明长度后的尾随字节，host suite经production
  admission补齐owner协议。复审确认三个blocking finding全部关闭且没有新的correctness finding。

## Contract Impact / Cutover

| Contract ID | 变化 | Cutover前baseline | 新规则 | 生效证据 |
| --- | --- | --- | --- | --- |
| [`NET-SOCKET-ENDPOINT-001`](../../contracts/net/udp-socket.md#net-socket-endpoint-001--socket与endpoint保持owner-fence和单向association) | Refine | Endpoint拥有binding/peer/datagram queue与既有facts，UDP没有async-error state | Endpoint增加enable truth、bounded FIFO、pending slot、overflow、disable/retire cleanup与generation isolation | production-admission host proof、KUnit、RV64 deterministic chain、independent review |
| [`NET-UDP-TRANSACTION-001`](../../contracts/net/udp-socket.md#net-udp-transaction-001--bindconnectsend与receive各自只有一个commit-boundary) | Refine | bind/connect/send/ordinary receive transaction；ICMP async error不在effective surface | 增加admitted ICMP quote lookup/enqueue、pending consume与`MSG_ERRQUEUE` detach transaction | host packet-path proof、KUnit、dual-libc oracle |
| [`SOCKET-ABI-001`](../../contracts/socket/front-abi-wait.md#socket-abi-001--linux-abi止于family-neutral-adapter) | Refine | UDP不支持`IP_RECVERR`、`SO_ERROR`、error ancillary producer或`sendmmsg` | 增加真实UDP option/error projection与逐条复用single-message transaction的`sendmmsg` | ABI KUnit、host/RV64 dual-libc oracle、resolver product evidence |

`NET-PROTOCOL-BOUNDARY-001`、`NET-SOCKET-WAIT-001`、`SOCKET-FRONT-001`、`SOCKET-WAIT-001`、
`OPENED-DESC-001..003`、iomux、epoll与network control-plane规则均为Dependencies；本轮没有改变其owner或协议。代码、
三项current contract与本记录在同一closure commit中原子生效。

## Validation

- `just test net-host`通过，UDP topology `24/24`；host Linux
  `6.18.33.2-microsoft-standard-WSL2`上的production packet-chain oracle通过。
- RV64 glibc/musl oracle构建通过。决赛盘RV64 pretest通过`471/471` KUnit、UDP `16/16`、UDP extension `10/10`、
  UDP message `7/7`、Unix `23/23`、seqpacket `4/4`、raw ICMP `10/10`、TCP `8/8`、Rust Command `2/2`以及双libc
  UDP errqueue/TCP oracle，最后正常PowerOff。
- 决赛盘没有profile中的六个Socket LTP executable，该次结果是`attempted=0, skipped=6`，不作为final-image LTP
  PASS。以初赛盘运行同一RV64 wrapper后，curated Socket LTP在glibc `3/3`、musl `3/3`通过，汇总
  `attempted=6, passed=6, failed=0, infra_failed=0, skipped=0`并正常PowerOff；这只补足对应UAPI回归。
- 较早的RV64 final harness记录`/etc/resolv.conf`为`nameserver 10.0.2.3`，未修改glibc BusyBox对numeric
  `223.5.5.5`、`ping -4 dns.alidns.com`及default AF_UNSPEC hostname ping均通过。该证据早于review fixes；修复只改变
  record move、malformed quote admission与oracle边界，没有修改normal resolver path。final image没有shutdown app，
  测试结束后直接关闭对应QEMU。
- `just test xtask`通过`83/83`；`just fmt kernel --check`通过。review fixes后代码未再修改，因此公共文档cutover后不重复
  运行kernel、xtask、QEMU或runtime suite；一次ordinary RV64 release build完成discovery/final pass与6279项symbol
  verification。`mdbook build docs`、`git diff --check`与closure diff/residual-reference audit通过。
- 独立复审确认没有Apollyon、Keter或其它blocking finding；Architecture Friction Scan未发现第二份truth、owner穿透、
  private representation泄漏、无退出条件bridge、batch-owned state或测试/架构特判。
- **Not Run:** LA64 build/runtime/final harness、physical hardware、`smp > 1`、low-memory allocation-failure runtime、
  long queue-pressure stress与full network LTP。

## Remaining Risk / Links

- [UDP Socket当前契约](../../contracts/net/udp-socket.md)和
  [Socket ABI当前契约](../../contracts/socket/front-abi-wait.md)是effective语义的唯一正文；Closed RFC保持历史资料。
- bounded FIFO在capacity/allocation pressure下允许丢失新record，但pending slot与ERROR wake仍更新；本轮没有公开drop
  counter或lossless guarantee，low-memory和长期pressure运行仍Not Run。
- standard ICMP quote不能证明historical send generation；晚到error可能按current tuple归属复用后的Endpoint。external
  hostname ping还依赖决赛DNS与公网，deterministic loopback closed-port chain才是error-channel的稳定oracle。
