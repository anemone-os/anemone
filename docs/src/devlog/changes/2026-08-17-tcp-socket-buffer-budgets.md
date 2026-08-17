# ANE-CHG-20260817-tcp-socket-buffer-budgets

**Type:** Small feature / local Socket ABI refinement
**Status:** Completed
**Date:** 2026-08-17
**Authors:** doruche, Codex
**Area:** Socket front / Stack TCP / smoltcp TCP engine / Linux socket-option ABI

## Problem / Context

Socket front已有typed `SocketOps::query_option` / `mutate_option`分发，但`SO_SNDBUF`与`SO_RCVBUF`
仍按family在`setsockopt`中选择：Netlink拥有真实budget mutation，TCP及其它family不支持，且共同
`getsockopt`没有缓冲预算查询。TCP engine的收发ring固定由Kconfig配置；只回显用户请求会制造第二份容量真相，
而直接重分配live ring会扩大到非平凡的协议与生命周期迁移。

Linux 6.6.32把普通`SO_SNDBUF`/`SO_RCVBUF`视为hint：读取一个`int`，将请求加倍并限制在系统上限和
方向性最小值内，`getsockopt`返回owner实际采用的值；缩小不会丢弃已排队数据。Anemone需要在固定物理
backing内实现同形的有效预算，而不是承诺autotuning或live backing resize。

## Decision / Implementation Boundary

Target是由Socket front对`SOL_SOCKET`的`SO_SNDBUF`/`SO_RCVBUF`执行family-neutral typed dispatch，
并仅为TCP新增完整的query/mutation能力。front唯一拥有optlen、用户copy、Linux `int`表示、errno和typed
request形成；Stack TCP Endpoint唯一拥有每个socket的有效send/receive budget，并将它投影到send admission、
smoltcp receive window、readiness invalidation和`getsockopt`。Kconfig的TCP ring容量是物理上限，新增的方向性
最小预算也是Kconfig policy；有效预算不是第二份物理容量真相。

普通TCP hint按Linux形状将signed `int`的bit pattern进入无符号上限clamp、再加倍并限制到方向性最小值与物理上限；
因此零请求得到最小值，负请求得到上限。缩小到当前occupancy以下不丢数据：后续send admission关闭，advertised
receive window降为零，直到队列排空到新预算以下；TCP不能撤回此前已广告的窗口，已经获准的in-flight bytes仍可进入
物理ring并由用户读取。增大通过既有Stack invalidation/progression使waiter和receive-window重新检查。
idle、bound、listener和connection均可query/mutate；accepted child继承listener在handoff时的有效预算，之后独立。
共享opened description（包括`dup`）继续观察同一Endpoint truth。

既有Netlink正值、exact-budget mutation与`len >= sizeof(int)`行为保持不变；它仍不提供buffer query。
UDP、ICMP raw和Unix不安装该能力，通用分发后继续返回`ENOPROTOOPT`。本轮不实现`SO_SNDBUFFORCE`、
`SO_RCVBUFFORCE`、TCP autotuning、sysctl/memcg accounting、动态ring重分配、其它family能力或跨accept-handoff的
原子配置更新。

Failure只有既有用户copy/长度错误、unsupported family和retired Endpoint；有效hint总能在已验证的Kconfig范围内
clamp。cleanup继续由Endpoint/engine既有retirement owner负责，不新增资源。若实现需要第二份behavior-driving
容量、移动TCP owner、暴露smoltcp私有表示、改变Netlink既有语义、live ring迁移或新的多阶段生命周期协议，
本小迭代停止并升级RFC。

## Change

- Socket front新增共同`SO_SNDBUF/SO_RCVBUF` query与hint mutation分发；optlen、Linux scalar、负值的无符号
  上限clamp形状、copy fault和errno仍止于front。Netlink既有exact mutation保留独立typed variant并保持原行为。
- Stack TCP Endpoint增加send/receive有效预算作为唯一行为真相；Kconfig新增方向性最小值，既有固定ring容量仍是
  物理上限。connect、listener engine与accept handoff把预算投影到smoltcp engine，`dup`自然共享Endpoint。
- smoltcp TCP engine增加可选effective capacity limit：send admission与advertised receive window服从owner预算；
  缩小不移动或丢弃ring内数据，也不伪装成能撤回已授权的in-flight receive bytes；增大经既有protocol progression与
  endpoint invalidation重新求值。
- 增加owner host test、Socket ABI KUnit及`socket-test --sockbuf`两项runtime case；user-test可选转发一个
  socket-test suite参数，默认完整suite行为不变。
- 首轮RV64在既有signal KUnit `blocked_default_ignore_enters_private_and_shared_pending`中因依赖live current-task
  pending state触发断言；按用户明确授权只删除该KUnit，不修改signal production语义。后续两轮完整RV64 KUnit通过。

## Validation

已完成：

- public xref Linux 6.6.32固定commit `91de249b6804473d49984030836381c3b9b3cfb0` source review，覆盖
  `SO_SNDBUF/SO_RCVBUF` hint clamp/doubling、actual-value query、FORCE差异与wake行为；
- `just test net-host`：Stack owner `24/24`（含新增预算/继承/缩小/增大测试）、focused smoltcp TCP `178/178`，
  其余network host suite同轮通过；
- `just test xtask`：`109/109`；
- `just fmt kernel`、`just fmt socket-test`、`just fmt user-test`；
- `just fmt kernel --check`、`just fmt socket-test --check`、`just fmt user-test --check`与`git diff --check`；
- `./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/tcp-socket-buffer-rv64-final.log`：
  RV64 release build、KUnit `776/776`、`SOCKBUFTEST 2/2`、orderly shutdown及wrapper exit 0。
- 最终独立subagent review无Apollyon/Keter finding；两项Euclid（Linux `int`可表示性断言、receive-window shrink
  描述精度）均在提交前修正并复核。

`mdbook build docs`被HEAD既有的重复SUMMARY目标`./devlog/transactions/2026-08-11-pty-devpts.md`阻断；本轮新增
change-record条目唯一，未新增或恶化该重复。

**Not Run：** LA64 build/runtime（用户明确排除，未触及架构代码）、hardware、LTP、final harness、`smp>1`与
长时/并发压力测试。

## Contract Impact / Cutover

| Contract ID | 变化 | 先前effective baseline | Effective refinement |
| --- | --- | --- | --- |
| `SOCKET-ABI-001` | Refine | buffer mutation按Netlink family特判，buffer query不可用 | front通用解码并typed dispatch；支持能力及实际值由family owner返回 |
| `NET-TCP-ENDPOINT-001` | Refine | Endpoint拥有reuse/no-delay等option fact，listener handoff复制相关配置 | Endpoint增加方向性有效预算；accepted child在handoff继承后独立 |
| `NET-TCP-STREAM-001` | Refine | send capacity与receive window直接使用固定ring容量 | owner-local有效预算限制send admission和receive window，固定ring只作为物理上限 |

上述refinement已由代码、KUnit、socket-test、RV64 runtime、current contract与本记录原子cut over。

## Remaining Risk / Links

- TCP采用固定预分配ring，因此`getsockopt`报告的是真实有效预算，不是物理allocation大小；本轮不提供内存回收收益。
- accepted-child继承在Stack handoff中线性化；listener与已handoff child之间不提供配置原子组更新。
- Current contracts：[Socket Front、ABI 与 Wait](../../contracts/socket/front-abi-wait.md)、
  [TCP Socket](../../contracts/net/tcp-socket.md)。
