# ANE-CHG-20260807-tcp-local-time-wait-admission

**Type:** Small Bug Fix / lifecycle-aware admission
**Status:** Completed
**Date:** 2026-08-07
**Authors:** doruche, Codex
**Area:** TCP / local delivery / active open / deferred reclaim / Socket ABI

## Problem / Context

Anemone的local TCP连接会在同一个interface `SocketSet`中持有client和server两个方向相反的smoltcp
engine。server先active close时，client Endpoint及其autobind port可以先完成释放，而server的反向engine仍在
deferred reclaim中处理TIME_WAIT。旧active-open admission只拒绝同方向exact 4-tuple，因此下一次connect可能过早
复用刚释放的client port。

此时新SYN的flow恰好匹配旧server engine的ingress tuple。smoltcp按同一`SocketSet`中的engine顺序做demux，旧engine
会先消费SYN，listener无法得到pending child；client只能按RTO重传，直到旧engine退出。在固定RV64单核复现中，该路径
表现为约15秒的connect阶梯；仅让active open跳过反向冲突port即可同时消除host lifecycle复现和端到端阶梯。

问题不在TIME_WAIT时长、RTO、listener、调度或HTTP response completion。本轮因此只修正TCP owner的连接准入和
reclaim reservation，不改变下层smoltcp demux策略。

## Decision

- Stack TCP owner继续保证所有interface上的live/deferred engine之间不存在同方向exact 4-tuple重复。
- 对active open额外检查candidate outgoing flow是否等于现有engine的ingress tuple。该反向冲突只在同一个
  `InterfaceId` / `SocketSet`内成立；不同interface不共享smoltcp engine demux，因此不互相保留反向tuple。
- connection Endpoint退出active role时，tuple随engine移动到`DeferredReclaim`，与interface和socket handle保持同一
  生命周期；engine完成TIME_WAIT及其它final protocol work并被移除时，reservation一并消失。
- `SO_REUSEADDR`不能绕过engine demux安全。显式选择冲突tuple时返回`EADDRINUSE`；隐式ephemeral选择跳过冲突port，
  所有候选耗尽时返回`EADDRNOTAVAIL`。
- 不改变TIME_WAIT/RTO、ephemeral range或选择顺序、listener/accept、local-link、preemption、capacity、非local路由、
  public API、shared contract或visible close语义。

## Implementation Boundary

Stack TCP Endpoint owner唯一拥有binding、connection tuple、listener child slot、deferred engine与active-open admission。
active `Connection`结束时，tuple从Endpoint role移动到对应`DeferredReclaim`，不在Endpoint与reclaim queue中复制；
admission只读取live owner state，不缓存第二份TIME_WAIT或port-availability truth。Stack orchestration只把已选择的
`InterfaceId`传给TCP owner；Socket ABI adapter只负责把owner error投影为Linux errno。

本轮保护现有global exact-tuple uniqueness、per-interface SocketSet ownership、listener handoff、release/cleanup顺序、
`SO_REUSEADDR`边界和current TCP contract。若需要改变跨interface规则、TIME_WAIT语义、smoltcp demux、public owner
surface、shared contract或acceptance强度，必须停止并重新分类；本轮没有触发这些条件。

## Change

- `prepare_connect`接收已经完成egress selection的`InterfaceId`，统一检查live connection、listener child和deferred
  engine的exact/reverse tuple冲突；exact比较保持global，reverse比较限定同一interface。
- retiring connection不再把tuple保存在`EndpointRole::Reclaiming`；`DeferredReclaim`携带该engine的interface、handle和
  optional tuple，成为final protocol lifetime内的唯一reservation truth。
- owner-local host regression构造server active-close的真实local connected pair，证明client Endpoint释放后server反向
  tuple仍被保留；显式reuse被拒绝，隐式connect从40000跳到40001并到达listener。独立helper test覆盖同interface反向
  冲突、跨interface反向允许和跨interface exact冲突。
- Socket ABI将implicit ephemeral exhaustion从`EAGAIN`修正为Linux active-open语义的`EADDRNOTAVAIL`，inline KUnit保护
  该映射。

## Validation

- TDD回归在修正前首先失败，因为deferred reclaim没有携带retiring connection tuple；实现后`just test net-host`全部
  通过，其中TCP owner `22/22`、smoltcp TCP `178/178`，shared frame/UDP/ICMP suites与no-default compile/check通过。
- `just build --preset qemu-virt-rv64-release --bind smp=8 --bind memory=8G`通过。
- `just build --preset qemu-virt-la64-release --bind smp=8 --bind memory=8G`通过。
- RV64 SMP=8、8 GiB final harness通过494/494 KUnit；未修改的赛方CAgent脚本完成10/10 pass，elapsed为
  2208--3333 ms且脚本返回0。取得目标证据后按环境约束从宿主侧终止QEMU；guest没有shutdown app，因此不声称
  orderly shutdown。
- `just fmt kernel`已执行；`git diff --check`与`mdbook build docs`通过。
- 独立change review确认Apollyon 0、Keter 0、代码侧Euclid 0；未发现第二份tuple truth、owner穿透、public API
  扩大、test-only production path或无退出条件bridge。唯一文档状态finding已在commit前修正。
- **Not Run:** RV64 SMP=1 production复验、LA64 runtime/final harness、physical hardware、full LTP、非local TCP定向
  runtime、ephemeral range全耗尽runtime与长期connection churn stress。研究期SMP=1反事实不计为本次omega production
  validation。

## Remaining Risk / Links

- [TCP Socket当前契约](../../contracts/net/tcp-socket.md)继续定义effective语义；本轮只修复其既有owner/lifecycle下的
  admission缺口，没有Contract Impact或cutover。
- host regression完整覆盖local close-order、deferred reservation、explicit reuse和implicit skip；非local TCP与
  跨interface规则主要由现有suite、owner helper regression和源码审计证明，没有新增端到端矩阵。
- admission避免新engine与现有demux claim冲突，但不改变TIME_WAIT持续时间、RTO退避或smoltcp的first-match行为；这些
  下层策略若独立变化，应重新验证本记录的owner假设。
