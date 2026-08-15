# ANE-CHG-20260813-unix-peer-credentials

**Type:** Small Feature / local Unix Socket ABI cutover
**Status:** Completed
**Date:** 2026-08-13
**Authors:** doruche, Codex
**Area:** Unix IPC / Socket front / credentials / Linux ABI

## Problem / Context

Anemone的Unix stream/seqpacket已经拥有自然的paired connection、pathname admission与opened-description
lifecycle，但没有向本地IPC consumer发布稳定的对端进程身份。`SO_PEERCRED`所需信息应在连接建立时由Unix owner
固定；查询时再查task table、长期持有完整`Task`/credential object，或把Linux `struct ucred`塞进connection都会
把身份生命周期、ABI布局与connection state不自然地耦合。

本轮只交付高价值的已连接Unix能力，不复刻Linux credentials全家桶。tracked Linux 6.6.32的
`xref:linux-6.6.32:net/unix/af_unix.c#unix_listen`、
`xref:linux-6.6.32:net/unix/af_unix.c#unix_stream_connect`、
`xref:linux-6.6.32:net/unix/af_unix.c#unix_socketpair`与
`xref:linux-6.6.32:net/core/sock.c#sock_getsockopt`用于确认snapshot位置及`getsockopt` copy规则；runtime
acceptance按用户要求只使用Anemone apps，不跑host Linux oracle。

## Decision

- 支持`AF_UNIX + SOCK_STREAM/SOCK_SEQPACKET`的pathname connect/accept与unnamed socketpair上的
  `getsockopt(SOL_SOCKET, SO_PEERCRED)`，返回稳定`{tgid,euid,egid}`；
- socketpair在paired connection创建时为两侧采当前调用者身份；pathname在首次成功listen时采server身份，在
  connect admission时采client身份，accept不重新采样；
- connection唯一拥有两侧窄snapshot。peer退出、close或后续credential变化不刷新既有connection；
- unconnected、bound与listening Unix role返回`ENOTCONN`，其它family返回`ENOPROTOOPT`；
- 重复`listen()`只更新backlog，不刷新首次listen snapshot。该低价值Linux边角差异与非连接role选择作为
  accepted limitation登记；
- Unix datagram、`SO_PASSCRED`、`SCM_CREDENTIALS`、其它ancillary credentials/fd passing、supplementary groups、
  pidfd、PID/user namespace投影、live task lookup与动态refresh均不在本轮范围。

## Change / Implementation Boundary

Target是让stream/seqpacket connection owner保存两侧immutable peer-identity values，让listener只在pathname
handoff前保存首次listen的server snapshot；Socket front只分发typed option query，ABI adapter独占Linux
`struct ucred`布局、errno、optlen和copyout ordering。fd/opened-description、VFS namespace、task credential、PID
allocation、stream/record data plane与wait owner均保持不变。

pathname connect在既有admission commit gate内重新验证exact listener后才发布携带两侧snapshot的connection；任何
失败候选均未发布并自然释放。retirement不查task table，也不让snapshot失效。本轮除新增明确的Linux ABI
常量/布局外，不扩大kernel owner public API；不增加完整credential cache、第二份mutable identity truth、
production probe或跨owner cleanup。若需要namespace投影、
credential mutation通知、datagram/ancillary协议、改变acceptance或第二次semantic cutover，必须停止并升级RFC。

## Change

- `anemone-abi`定义Linux `SO_PEERCRED`与固定12-byte `UCred` layout，并用静态断言固定size/alignment/offset；
- general Socket option boundary增加normalized peer-credential query/value与role-dependent `NotConnected` outcome；
- Unix listener保存首次listen snapshot，stream/seqpacket connection保存两侧snapshot并按endpoint side查询；
- `getsockopt` adapter执行tgid可表示性检查、Linux layout encoding、short/zero optlen、value-first fault ordering及
  `ENOTCONN`/`ENOPROTOOPT`映射；
- owner-local inline KUnit覆盖stream/seqpacket两侧选择、非连接role、unsupported option及peer retirement后稳定性；
- Anemone `socket-test`覆盖stream/seqpacket socketpair与pathname、fork/setuid、accept/child exit后稳定性、ABI
  truncation/zero/null fault和unsupported family；`user-test --socket-test`提供只运行该仓库内app并关机的真实guest
  focused入口。

## Validation

- RV64 SMP1 release guest：635/635 KUnit通过；`UNIXTEST:SUMMARY:PASS:25`、
  `SEQPACKETTEST:SUMMARY:PASS:4`及socket-test其它UDP/raw/TCP/netlink suite通过；两条新增peercred case通过，最终
  orderly PowerOff；
- LA64 SMP1 release guest：635/635 KUnit及同源socket-test suite通过；guest完成orderly shutdown后进入该平台已知
  terminal halt，随后宿主只终止已完成验证的QEMU实例；
- RV64/LA64 release kernel、`socket-test`与`user-test`均通过repository entrypoint构建；
- tracked Linux 6.6.32只作源码审查；没有执行host Linux runtime oracle。
- independent final review发现并闭合fault-ordering空证明、ABI boundary措辞与external-source citation三个局部
  Euclid；强化后的16-byte optlen fault assertion随后在最新RV64/LA64 focused guest中通过。复审为0 Apollyon、
  0 Keter、0 Euclid，Architecture Friction Scan无残留。

**Not Run:** host Linux oracle、host-side socket tests、LTP、final harness、physical hardware、SMP>1。

## Contract Impact / Cutover

**Cutover:** `SOCKET-UNIX-PEERCRED-CUTOVER`

以下变化在上述双架构Anemone guest验收与独立final review完成后，与实现一起原子cut over：

| Contract ID | 变化 | 先前effective baseline | Effective rule |
| --- | --- | --- | --- |
| `UNIX-SOCKET-PEERCRED-001` | Introduce | 已连接Unix connection没有可查询的peer identity能力 | listener/connect/socketpair在自然handoff点采窄snapshot，connection唯一拥有两侧稳定身份 |
| `SOCKET-ABI-001` | Refine | option adapter没有`SO_PEERCRED` layout/copyout映射 | family返回normalized snapshot；adapter独占`struct ucred`、optlen、fault ordering与errno |

`UNIX-SOCKET-STATE-001`、`UNIX-SOCKET-LIFECYCLE-001`、`SOCKET-FRONT-001`与opened-description规则均为
Dependencies；本轮沿既有owner/handoff/retirement协议增加connection-owned value，不改变其规则。

## Remaining Risk / Links

- Accepted limitation：[Unix peercred edge semantics](../../register/current-limitations.md#ane-20260813-unix-peercred-edge-semantics)。
- Current contracts：[Unix Socket state/lifecycle](../../contracts/socket/unix-stream-lifecycle.md)、
  [Socket front/ABI/wait](../../contracts/socket/front-abi-wait.md)。
- RFC / transaction / issue / PR：None。
