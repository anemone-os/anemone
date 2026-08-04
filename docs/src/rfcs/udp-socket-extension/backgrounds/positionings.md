# IPv4 UDP Socket 能力扩展定位共识

**状态：** Archived RFC Background / Superseded by Draft body
**最后更新：** 2026-08-04
**Canonical RFC：** [RFC-20260804-udp-socket-extension](../index.md)

本文保存公共 Draft 形成前的定位、取舍与问题收敛历史。它不再拥有 proposal、target、
Implementation Boundary、Contract Impact、acceptance 或执行事实；发生冲突时以父 RFC、
current contracts 与 live source 为准。本文不授权 implementation、probe、checkpoint、
transaction 或 cutover。

## 起点

讨论由 userspace DNS resolver 无法通过既有 UDP/Socket ABI 完成名称解析触发。最初即
明确不把主题定位为“实现 DNS”：resolver configuration、NSS、cache、retry 与 query
policy 属于 userspace/rootfs，kernel 只应提供普通 UDP/Socket capability。

DNS 被选为第一个强 consumer，因为它可以暴露 connected UDP、file-style I/O、blocking
与 poll/readiness 缺口；它不能成为唯一 oracle，也不能成为按 libc、applet 或固定
syscall trace 建立 kernel 特判的理由。

## 定位共识

讨论将范围从现有 unconnected `sendto/recvfrom` vertical slice 收敛为一个有限的 IPv4
UDP Socket envelope：

- connected peer association、reconnect、disconnect 与 peer query；
- connected/unconnected datagram、file-style 和 single-message/vector I/O；
- bind、implicit bind、route/source/interface selection、dup/fork、final release 与
  stale identity 的一致组合；
- owner-defined connect/send/receive predicates 与现有 wait/recheck；
- 对 general Socket front/ABI adapter 只做真实 UDP consumer 所需的最小反馈。

首版明确不追求完整 Linux UDP compatibility。`sendmmsg/recvmmsg`、IPv6、TCP、
broadcast/multicast、ancillary data、timeout/buffer/reuse policy、`SO_ERROR`、error queue、
netlink/ioctl 配置面与 kernel DNS 均被排除。

## 用户边界收敛

定位阶段接受的 R0 方向包括：

- `socket(AF_INET, SOCK_DGRAM, 0/IPPROTO_UDP)`，以及 creation flags
  `SOCK_NONBLOCK | SOCK_CLOEXEC`；
- `bind/getsockname/connect/getpeername`，允许 reconnect 与 `AF_UNSPEC` disconnect；
- `sendto/recvfrom`、connected `read/write/readv/writev`、single-message
  `sendmsg/recvmsg`；
- send flags `MSG_DONTWAIT | MSG_NOSIGNAL`，receive flags
  `MSG_DONTWAIT | MSG_PEEK | MSG_TRUNC`；
- unsupported flags、control message、option 与 shutdown 保持稳定拒绝，不引入
  success-no-op option 或永久零值 `SO_ERROR`。

批量 message syscall 被有意延后：它会引入跨多消息 partial completion、timeout、copy
fault 与 cancellation 边界，而 DNS 与首批普通 consumers 不提供相称义务。single-message
vector ABI 足以验证 datagram/copy/handoff，同时避免过早扩大 target。

## 状态模型收敛

定位阶段决定把状态关系写成 target-level owner model，而不是实现类型或字段：

| Fact | Owner | 已接受关系 |
| --- | --- | --- |
| local binding | Stack Endpoint | 与peer正交；disconnect不释放binding |
| peer association | Stack Endpoint | connect/reconnect原子替换；failure保持旧peer |
| queued datagram | Stack Endpoint | admission完成后不因后续peer变化回溯清理 |
| route/source/interface selection | control plane | operation-local，不形成第二份peer truth |

ingress peer filter 只约束 transition 后的新 admission；已入队 datagram 保持可读。
connected `sendto/sendmsg` 的显式 destination 只覆盖本次 send，不改变 persistent peer。
receive readiness 读取 admitted queue，send readiness 读取 bounded capacity；缺少 destination、
无 route 或 invalid address 是立即错误，不伪装成 wait。

这个模型被提升到父 RFC 的 [目标与不变量](../invariants.md)，但不冻结内部 state enum、
字段、锁、generation、queue implementation 或 cross-crate method shape。

## ABI 诚实性取舍

定位阶段区分三类输入：完整承诺的成功能力、稳定拒绝的非目标，以及用户可见效果确实
不变时才允许的窄 compatibility behavior。`MSG_NOSIGNAL` 在 UDP 没有 SIGPIPE producer
时属于后一类，但必须带关键注释、一次性诊断和未来 producer 出现时的退出条件。

会承诺新状态、error delivery、buffer policy、timeout 或 control-message effect 的 option
不能仅为 resolver initialization 而 success-no-op。真实 consumer 如果必须依赖当前非目标，
应回到 RFC review 扩大或修订 target，或保持该 consumer Not Supported / Not Cut Over。

## 提升时列出的 review 候选（历史）

以下事项在公共 Draft 提升时曾被保守地列为 review 候选；它们不是当前 blocker 清单，
后续 review 已将 ABI oracle、Contract Impact、validation 分层与实施路线选择折回父 RFC：

1. `sendmsg/recvmsg` 的 iovec bounds、header copyout ordering、fault 与 partial oracle；
2. glibc/musl resolver 的 exact call shape 与 socket-option dependency；
3. 从 live source 分类最终 Contract Impact；
4. mandatory consumer、双架构、external networking 与 Not Run matrix；
5. 实现是否真的需要 multiple checkpoints、probe 或 `implementation.md`。

公共 Draft 初次发布时，mandatory resolver exact call shape 是 implementation route 前最后
一个 target-level review 问题；canonical RFC 后续已用固定 source audit 选择 musl IPv4
resolver，并保持 glibc resolver Not Supported / Not Cut Over。具体状态只以 canonical RFC
为准。本文从此冻结为历史材料，不再随 review、implementation 或 cutover 更新。
