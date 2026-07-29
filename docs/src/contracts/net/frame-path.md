# Network Frame Path 当前契约

**Contract ID：** `NET-BOUNDARY-001` / `NET-FRAME-OWN-001` / `NET-FRAME-PROGRESS-001` / `NET-STACK-PUMP-001`
**状态：** Active
**Owner：** cross-layer frame protocol；每条规则和每份runtime state的唯一owner见正文
**参与领域：** `anemone-net-api` / `device/net` / concrete frame provider / `anemone-smoltcp-stack` / kernel worker
**覆盖范围：** shared frame semantic surface、RX/TX ownership、bounded resource/recheck与stack pump
**不覆盖：** netdev publication/identity、attach/shutdown、endpoint/socket/fd/readiness、route/address control plane或完整teardown
**实现位置：** `anemone-kernel/crates/{anemone-net-api,anemone-smoltcp-stack}`、`anemone-kernel/src/{device/net,driver/net,net}`
**依赖：** 本页内部依赖按条目声明
**Pending Successor：** None
**最后核验：** 2026-07-29

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| shared semantic types | `anemone-net-api` | values、move-only token traits | 跨provider/stack表达同一handoff，不拥有runtime state |
| backing、queue token、DMA与completion | concrete frame provider | callback-scoped frame token、recheck edge | frame I/O与资源回收 |
| link/resource durable truth | concrete provider；规范化publication snapshot由`device/net`拥有 | snapshot + edge-only wake | worker醒来后重读owner fact |
| smoltcp object、`InterfaceId` mapping与deadline | initial-domain唯一`DomainStack`内的concrete Stack instance | narrow per-interface pump port、wake/work capability | 串行bounded protocol progression |
| worker admission与explicit work | kernel attach/worker owner | stateless wake capability | 安排pump，不复制provider或stack truth |

notification、wake edge、统计与diagnostic label都不是行为真相源。当前production source不保留
frame/IRQ/completion diagnostic mirror或single-NIC query。

## NET-BOUNDARY-001 — frame slice依赖方向与object fence

**规则：** `anemone-net-api`只定义kernel side与concrete protocol stack共同需要的opaque identity、frame
capability/outcome、link/interface facts、monotonic time与pump/recheck语义；它不依赖kernel或concrete
smoltcp object，也不拥有runtime registry。stack只依赖shared API与smoltcp，不接收task/fd/wait capability；
driver/device停在frame/link边界，不依赖endpoint、socket或protocol object。descriptor、DMA address、VirtIO
header、hardware queue token、driver backing、smoltcp object identity与Linux readiness不得越过各自object fence。

**Owner：** `anemone-net-api`拥有共享semantic surface；runtime state仍由provider、netdev、stack与attach
authority分别唯一拥有。

**违反表现：** driver公开实现`smoltcp::phy::Device`；shared API返回concrete kernel/smoltcp object；kernel
按smoltcp handle决策；API crate形成第二个runtime registry或解释Linux errno/readiness。

**验证 / Enforcement：** crate dependency/public-surface audit；host gate以真实stack和正式deterministic
provider完成ownership、exhaustion、deadline与双实例matrix；non-host base build无packet injection或endpoint
construction。

**最初来源：** [Network Frame Path RFC R1](../../rfcs/net-frame-path/index.md)。

**当前来源：** [Network Frame Path transaction](../../devlog/transactions/2026-07-26-net-frame-path.md)的
`NFP-FINAL-CUTOVER`。

## NET-FRAME-OWN-001 — frame backing只有一个访问owner

**规则：** 每个frame backing任一时刻只有一个有权访问或推进它的owner。provider以move-only token授予
一次callback-scoped consume：RX只在`consume`内暴露有效`&[u8]`，callback返回后回收；TX只在
`consume(len, ...)`内暴露有效`&mut [u8]`，submit commit后上层失去访问权。未consume token的`Drop`
取消reservation。completion commit前CPU不得访问device-owned backing；consumer释放前不得重新投递RX。
protocol callback期间不得持device-wide lock；paired RX/TX consume不得重入同一全局锁。

**Owner：** 每个concrete frame provider instance唯一拥有自己的backing、slot与queue-token lifecycle。

**违反表现：** use-after-submit、double recycle/completion、CPU与DMA并发修改同一bytes、callback后保存raw
slice、stack持descriptor token、unconsumed token泄漏credit，或普通frame用共享锁引用制造并列owner。

**验证 / Enforcement：** compile-fail token tests、host cancel/oversize/unwind/paired-consume与matching
completion tests；VirtIO slot/token assertions和每处unsafe begin/complete的identity/lifetime/sync/error proof；
Stage 3 Checkpoint 2 RV64真实双向traffic/completion/IRQ evidence与Checkpoint 3只删除validation的source audit。

**最初来源：** [Network Frame Path RFC R1](../../rfcs/net-frame-path/index.md)。

**当前来源：** [Network Frame Path transaction](../../devlog/transactions/2026-07-26-net-frame-path.md)的
`NFP-FINAL-CUTOVER`。

## NET-FRAME-PROGRESS-001 — 有界资源、normal backpressure与durable recheck

**规则：** RX/TX backing、descriptor/credit、同时存活DMA mapping与queued completion都有明确capacity；
queue full、RX empty、TX credit exhausted与link unavailable返回normal outcome，不得panic、busy-spin或隐式
建立无界resource。completion、resource return或link/resource fact变化必须保留durable predicate并发布可合并
wake edge；worker醒来后重读owner fact，不能把notification或diagnostic mirror当作truth。普通software budget
不得让ingress/egress一侧无限期饿死；真实device saturation可以暂时阻塞，但resource恢复后必须重新参与推进。

`virtio-drivers::Hal::share()`当前不可失败地分配bounce mapping；global allocator OOM仍可能kernel-fatal，
不属于frame/queue normal exhaustion。适度、受credit约束的per-operation allocation是当前接受边界；本规则
不要求统一pool、allocation-free路径或dependency fork。

**Owner：** provider拥有resource count、queue/completion与durable recheck truth；worker只消费edge并重读。

**依赖：** `NET-FRAME-OWN-001`。

**违反表现：** queue exhaustion触发fatal path；live resource超过credit上界或completion后泄漏；wake丢失
durable fact；镜像counter驱动admission；为消除适度allocation引入跨层DMA identity或第二套credit truth。

**验证 / Enforcement：** host deterministic provider精确制造exhaustion、matching completion、coalesced
recheck、恢复与ingress/egress fairness；VirtIO slot/capacity/source audit；Checkpoint 2 RV64记录bounded
outstanding/mapping、completion、IRQ与有限worker round。RV64未自然观察到exhaustion不替代host确定性proof。

**最初来源：** [Network Frame Path RFC R1](../../rfcs/net-frame-path/index.md)。

**当前来源：** [Network Frame Path transaction](../../devlog/transactions/2026-07-26-net-frame-path.md)的
`NFP-FINAL-CUTOVER`。

## NET-STACK-PUMP-001 — stack instance唯一推进protocol state

**规则：** 一个concrete stack instance独占其interface resources、private smoltcp objects、`InterfaceId`
mapping、protocol deadline与后续transport resources；同一instance任一时刻最多一个pump持推进能力，所有
protocol mutation通过该独占边界串行化。pump使用调用者提供的monotonic instant，ingress、egress与maintenance
都受finite budget约束，并返回work remaining/immediate recheck与next deadline。IRQ、timer、wake与kernel
attach authority只请求推进，不在owner外修改smoltcp object或发布Linux readiness。

**Owner：** concrete `anemone-smoltcp-stack::Stack` instance。

**依赖：** `NET-FRAME-OWN-001`、`NET-FRAME-PROGRESS-001`。

**违反表现：** 两个worker并发poll同一stack；IRQ进入smoltcp；kernel缓存smoltcp handle；ordinary worker
调用无界poll；deadline读取wall clock或另一条隐藏时间线。

**验证 / Enforcement：** host serialization、exact budget、deadline/owner-blocked、multi-instance与one-Stack/
two-provider tests；kernel source audit确认worker只持narrow per-interface port且每轮受静态budget/repoll上界约束，
domain Stack lock在round间释放；RV64 worker/timer wiring与normal shutdown evidence。

**最初来源：** [Network Frame Path RFC R1](../../rfcs/net-frame-path/index.md)。

**当前来源：** [Network Frame Path transaction](../../devlog/transactions/2026-07-26-net-frame-path.md)的
`NFP-FINAL-CUTOVER`。

**当前 enforcement 更新：** [Network UDP transaction](../../devlog/transactions/2026-07-29-net-udp.md)的
`NET-UDP-DOMAIN-CUTOVER`将production从per-netdev Stack迁移为initial-domain唯一Stack与per-provider narrow pump
port；本ID的单instance唯一推进语义保持不变。

## 当前接受边界

- production runtime proof只覆盖RV64 QEMU virtio-mmio单NIC；多实例隔离由两个真实host stack/provider
  domain证明。LA64、virtio-pci、hardware与`smp>1`均Not Run。
- 本页不提供socket/control-plane或完整teardown；host-test control不进入kernel dependency。
- runtime hotplug/detach/restart未实现；无法证明device/CPU不再访问的provider/backing保留到reset/power-off。
