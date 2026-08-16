# ANE-CHG-20260817-socket-fionread

**Type:** Small feature / local Socket ABI refinement
**Status:** Completed
**Date:** 2026-08-17
**Authors:** doruche, Codex
**Area:** Socket front / UDP / ICMP raw / TCP / Unix IPC / Netlink / Linux ioctl ABI

## Problem / Context

共同Socket FileOps此前只保留`FIONBIO`所在的opened-description status通路，其自身`ioctl`始终返回`ENOTTY`，
因此常见的`FIONREAD`/`TIOCINQ`/`SIOCINQ`输入队列查询在所有Socket family上均不可用。Linux 6.6.32分别从
UDP/raw队首、TCP stream receive queue和Unix incoming queue投影该值；listener拒绝查询，Netlink不发布该能力。

直接在共同FileOps按family downcast，或把raw command、用户指针与`IoctlCtx`交给family，都会混淆Linux ABI owner
与协议状态owner。本轮需要增加的是各family显式选择的兼容能力面，而不是通用raw-ioctl framework。

## Decision / Implementation Boundary

Target是让Socket front识别数值相同的`FIONREAD`、`TIOCINQ`与`SIOCINQ`，并通过typed
`SocketIoctlRequest::ReadableBytes`分发到static `SocketOps::ioctl` callback。front唯一拥有raw command、Linux
`int`表示、`usize -> i32`检查、errno mapping与用户copyout；family callback只读取其既有queue/stream owner truth，
返回typed response或role/lifecycle rejection，不接收raw cmd、arg、用户指针、file/task或`IoctlCtx`。

各family语义为：UDP返回下一datagram payload长度，ICMP raw返回下一完整IPv4 packet长度，TCP与Unix stream返回
累计未读stream bytes，Unix seqpacket返回全部排队record payload总和。UDP用`Option<usize>`在同一owner fact中区分
空队列`None`与可读零长datagram`Some(0)`，不建立第二份readiness truth。TCP/Unix listener返回`EINVAL`；Netlink
不安装callback并继续返回`ENOTTY`。unconnected/bound但非listener的已支持family返回0。

本轮不实现`SIOCOUTQ`、`SIOCATMARK`、interface ioctl、Netlink queue query、通用family ioctl registry或cached byte
counter；不改变receive/peek/consume、poll readiness、queue capacity、wait、lifecycle、`FIONBIO`或其它unknown ioctl
行为。若实现需要移动queue owner、缓存派生计数、下放raw ABI或扩大public/shared contract，本小迭代停止并升级RFC。

## Change

- Socket front新增typed ioctl request/response/error与`SocketOps::ioctl` capability，FileOps只负责Linux decode、
  checked scalar conversion和copyout；
- UDP与ICMP raw owner snapshot携带队首长度，readiness从同一`Option`派生；TCP复用既有
  `TcpConnectionFacts::received_bytes()`；Unix stream读取incoming `VecDeque::len()`，seqpacket读取既有direction
  byte总量；Netlink capability保持absent；
- owner-local KUnit覆盖typed static dispatch、UDP零长datagram可读性、TCP idle/retired、Unix stream重复查询与
  partial read、seqpacket聚合与head consume；
- socket-test覆盖各family的empty/query/non-consuming/consume后变化、listener rejection、Netlink unsupported，
  并在UDP路径覆盖zero-length datagram、队首消费前后推进、bad output pointer、unknown ioctl与`FIONBIO`回归；
  UDP只读取队首而不聚合的结论由Endpoint owner的`.received.front()`源码审查建立，runtime不单独外推该判别。

## Validation

- tracked Linux 6.6.32 source review：`udp_ioctl`与raw `SIOCINQ`读取队首skb长度，TCP listener返回`EINVAL`否则
  使用`tcp_inq`，Unix stream/seqpacket聚合receive queue，Netlink不接受该命令；asm-generic与sockios别名均等于
  `FIONREAD`；
- `just app build --arch riscv64 socket-test`通过；
- `just build --preset qemu-virt-rv64-release`通过，既有相邻warning未由本轮引入；
- focused RV64 wrapper通过696/696 KUnit、UDP 17/17、UDP extension 10/10、UDP message 7/7、Unix 26/26、
  Unix seqpacket 5/5、ICMP raw 11/11、TCP 9/9与Netlink suite，并完成orderly PowerOff；运行日志为
  `build/socket-fionread-rv64.log`；
- runtime之后只把front的等价`FIONREAD`判断整理为显式raw-command到typed-request decode、补充ABI边界注释，并把
  当前facts实现不会产生的TCP `WrongRole`防御映射从retired修正为invalid-state；这些整理不改变已执行的request、
  callback、正常role或copyout路径，按source review与最终RV64 build闭合，未重复运行QEMU；
- `just fmt kernel --check`、`just fmt socket-test --check`、`git diff --check`与`mdbook build docs`作为最终
  closure检查执行。

**Not Run：** LA64 build/runtime、hardware、LTP、host Linux executable oracle、final harness、`smp>1`与压力测试。

## Contract Impact / Cutover

| Contract ID | 变化 | 先前effective baseline | Effective refinement |
| --- | --- | --- | --- |
| `SOCKET-ABI-001` | Refine | Socket FileOps对输入队列查询统一返回`ENOTTY` | front解码`FIONREAD` aliases并独占Linux scalar/copyout/errno；static `SocketOps::ioctl`把typed request分发给明确支持的family，queue/stream owner返回瞬时事实，Netlink与unknown ioctl保持`ENOTTY` |

`SOCKET-FRONT-001`、各family endpoint/stream/queue contract、`SOCKET-WAIT-001`与opened-description status contract
作为受保护依赖不变；本轮没有第二份queue/readiness truth，也没有改变`FIONBIO`。

## Remaining Risk / Links

- 查询是owner锁窗口内的瞬时snapshot；返回后并发I/O可以改变值，这与readiness/query既有recheck模型一致，不承诺
  与后续receive原子绑定。
- `usize`超过Linux signed `int`表示范围时当前按既有FIONREAD scalar policy返回`EFBIG`；已配置的队列上界远低于
  该范围。
- Current contract：[Socket Front、ABI 与 Wait](../../contracts/socket/front-abi-wait.md)。
