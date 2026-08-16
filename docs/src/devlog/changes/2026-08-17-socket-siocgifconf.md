# ANE-CHG-20260817-socket-siocgifconf

**Type:** Small feature / local Socket ABI refinement
**Status:** Completed
**Date:** 2026-08-17
**Authors:** doruche, Codex
**Area:** Socket front / network interface diagnostics / Linux ioctl ABI

## Problem / Context

`build/mc-vanilla-rv-1.log`中OpenJDK通过Socket fd调用`SIOCGIFCONF`枚举网络接口时收到`ENOTTY`，使Netty无法取得
可用接口列表。Linux 6.6.32在common `sock_ioctl()`中处理该命令，而不是把它交给UDP/TCP等protocol callback；
`dev_ifconf()`先读取包含嵌套用户指针的`struct ifconf`，再按IPv4 address输出完整`struct ifreq`记录，最后写回实际
字节数。NULL buffer只查询完整snapshot所需长度。

Anemone已有read-only netlink diagnostics使用的owner-normalized snapshot：`LogicalInterfaces`拥有membership/name/
ifindex，`Ipv4ControlPlane`拥有IPv4 address，`route_diagnostics().addresses`在network guard内取得这些事实并返回owned
request-local snapshot。因此本轮缺口是共同Socket ABI adapter，不能通过family downcast、新interface registry或读取
smoltcp私有状态修补。

Minecraft日志中的console-handler `Illegal seek`是独立的TTY输入查询缺口，不属于本轮target。

## Decision / Implementation Boundary

Target是在initial network domain的所有common Socket fd上发布read-only Linux-compatible `SIOCGIFCONF`。Socket front
识别raw command，读取LP64 `ifconf`，取得一次`route_diagnostics().addresses` owned snapshot，仅枚举实际拥有IPv4
address的logical interface，并编码zero-initialized LP64 `ifreq`。支持OpenJDK的NULL-buffer sizing与后续filled-buffer
调用；非NULL buffer只写入完整记录，先复制记录、成功后再写回`ifc_len`。用户copy fault返回`EFAULT`，已经完成的
copyout不回滚。

network owner仍唯一拥有membership、name、ifindex与IPv4 address；Socket front只持request-local snapshot并独占
`ifconf/ifreq/sockaddr_in`布局、嵌套用户指针、native/network byte order、capacity planning、errno与copy ordering。
该命令不进入`SocketOps::ioctl` family callback，也不建立cached interface/address state。

Non-goals是其它`SIOCGIF*`命令、IPv6、runtime interface mutation/attach/detach、network namespace、通用raw Socket
ioctl framework、endpoint-specific state，以及Minecraft TTY console问题。`NET-CONTROL-PLANE-001`与Netlink diagnostics
是保持不变的依赖。

若实现需要新建registry、移动owner、读取smoltcp/private provider state、扩大到runtime topology/namespace、改变
failure/copy ordering或降低source/KUnit/RV runtime验证强度，本小迭代停止并升级RFC。

## Change

- common Socket FileOps新增显式raw-command decode；`SIOCGIFCONF`进入Socket-front interface ABI adapter，既有
  `FIONREAD`仍进入typed family callback，`FIONBIO`与unknown command边界保持不变；
- 新的owner-local adapter按RV64/LA64共同LP64布局解析16-byte `ifconf`，从
  `route_diagnostics().addresses`取得owned snapshot，按capacity选择完整40-byte `ifreq`并编码NUL-terminated name、
  zero padding、native-endian `AF_INET`与network-order IPv4 address；
- NULL buffer忽略input length并报告完整snapshot长度；非NULL的negative/zero/short capacity返回零或完整record prefix，
  snapshot/length overflow在copyout前拒绝；records copy成功后才更新`ifc_len`；
- inline owner-local KUnit覆盖common decode regression、LP64 layout/offset、loopback/external encoding、NULL sizing、
  negative/zero/short/exact/extra/full capacity、no-partial-record与overflow rejection。

## Validation

- tracked Linux 6.6.32 source review（`91de249b6804473d49984030836381c3b9b3cfb0`）：common
  `sock_ioctl()`直接处理`SIOCGIFCONF`；`dev_ifconf()`先copy `ifconf`，NULL buffer计算总长度，非NULL buffer逐interface
  调用`inet_gifconf()`；后者只枚举IPv4 address、zero-initialize `ifreq`、只写完整record并在copy fault返回`EFAULT`；
- production path source audit：common Socket FileOps直接进入front adapter，不走family callback/downcast；唯一network
  输入是`route_diagnostics().addresses` owned snapshot；network guard在nested user copy前释放；records先于最终
  `ifc_len`写回；没有新增interface/address registry或cached truth；
- `just fmt kernel --check`通过；
- `just build --preset qemu-virt-rv64-release`通过，输出只有本轮未引入的既有warning；
- RV64 `smp=1` QEMU runtime执行705/705 registered KUnit，六个本轮Socket-front cases均PASS，并完成orderly
  PowerOff；最终日志为`build/socket-siocgifconf-rv64-final.log`；wrapper附带的signalfd 6/6 PASS只作为正常关机路径，
  不外推为本能力userspace oracle；
- `git diff --check`与`mdbook build docs`通过。

**Not Run:** LA64 build/runtime、Java、Minecraft、userspace C `SIOCGIFCONF` oracle、hardware、network LTP/full Socket
suite、IPv6、runtime network reconfiguration、network namespace、`smp>1`与压力测试。

## Contract Impact / Cutover

| Contract ID | 变化 | 先前effective baseline | Effective refinement |
| --- | --- | --- | --- |
| `SOCKET-ABI-001` | Refine | common Socket front不识别interface ioctl | front独占`SIOCGIFCONF`的LP64 layout、nested user pointer、capacity、errno与copy ordering，并消费normalized owned IPv4-address snapshot |
| `NET-IFACE-DOMAIN-001` | Refine | logical interface没有用户可见interface query | initial-domain logical identity可连同control-plane-owned IPv4 address经只读`SIOCGIFCONF`投影；logical owner不取得address或UAPI职责 |

本change record、实现、source review、RV64 build/runtime与最终审查共同形成一次原子cutover；effective正文只位于上述
current contract。

## Remaining Risk / Links

- Current contracts：[Socket Front、ABI 与 Wait](../../contracts/socket/front-abi-wait.md)、
  [Network Interface Domain](../../contracts/net/interface-domain.md)、
  [IPv4 Control Plane](../../contracts/net/control-plane.md)与
  [Read-only Netlink Diagnostics](../../contracts/socket/netlink-diagnostics.md)。
- KUnit证明production command decode、codec与capacity planner；普通KUnit没有ordinary userspace，本轮RV64 runtime
  也没有执行专门的userspace ioctl oracle，因此嵌套用户指针的success/fault执行仍只由production source audit支撑，
  不声称已观察Minecraft warning消失。
