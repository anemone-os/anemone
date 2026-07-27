# Network Frame Path 迁移实施计划

**状态：** R1 Accepted for Implementation / Stage 1-3 Historical Closed / Stage 4 Ready, Unauthorized /
`NFP-FINAL-CUTOVER` Effective
**最后更新：** 2026-07-27
**父 RFC：** [RFC-20260726-net-frame-path](./index.md)
**目标与不变量：** [Network Frame Path 目标与不变量](./invariants.md)
**当前契约：** [Network current contracts](../../contracts/net/index.md)中的六个ID与
[`SYSTEM-POWER-ORDERLY-001`](../../contracts/power/shutdown-lifecycle.md#system-power-orderly-001) Refine均Active
**当前修订：** `R1`
**事务日志：** [2026-07-26 net-frame-path](../../devlog/transactions/2026-07-26-net-frame-path.md)已Completed；
Stage 4激活时另建transaction
**平台验收范围：** RV64 QEMU virtio-mmio；LA64 / virtio-pci 不属于本修订的 build、runtime 或
cutover 要求

> 本文是 R1 的实施顺序、stage maturity、验证与 write-set 权威。用户已于 2026-07-26 接受 R0、
> 授权建立 transaction，并独立激活 Stage 1 Checkpoint 1。用户随后分别独立授权并关闭 Checkpoint 2
> 与 Checkpoint 3，并独立授权、关闭 Checkpoint 4 与 Stage 1。Stage 1 关闭后的 module-boundary review
> 又发现 concrete driver、通用 worker 与 validation vocabulary 之间的局部耦合；用户已授权在 Stage 1
> 与 Stage 2 之间完成本文定义的 Boundary Interlude。本授权不授予 contract cutover，也不自动解析或
> 进入 Stage 2。2026-07-27 的首条 RV64 saturation route 命中 failure signal后，用户接受 R1
> proof-boundary correction并授权docs-only route resolution。用户随后分别独立授权并关闭R1 Checkpoint 1、
> Checkpoint 2与Checkpoint 3；Stage 2已关闭。用户随后只授权完成`2 -> 3 Implementation Resolution
> Gate`与docs write-back。用户随后分别授权并关闭Stage 3全部checkpoint与原子contract cutover。2026-07-27
> post-close review发现三项in-target conformance defect；用户授权本次docs-only反馈修订，将单一Stage 4解析为
> Ready。本次授权不创建transaction、不修改源码，也不激活Stage 4。

## 1. 计划角色与 authority

四层架构是长期 owner、object fence 与依赖方向，不是四个依次完成的施工阶段。实施单位必须是可执行的
跨层结果；“API crate 完成”“smoltcp stack 完成”或“driver 完成”都不能单独构成 stage closure。

本计划原实现采用一个纵向 walking skeleton 和两个后续加固阶段：

1. Stage 1 在同一个 stage 内建立 hostable semantic seam、真实 smoltcp consumer、真实 VirtIO frame
   provider、`device/net` publication 与 kernel attach/IRQ/worker wiring；
2. Stage 2 对 ownership、completion、exhaustion、recheck、budget、deadline 与公平性做确定性硬化；
3. Stage 3 完成多实例隔离、attach/lifecycle、System Power handoff、RV64 production acceptance 与
   `NFP-FINAL-CUTOVER`。

Stage 4是post-close contract-conformance correction：它不增加能力或contract，只在一个checkpoint内修正
pending handoff owner、完整`Late`之后的network activation顺序与长期host test metadata。

Stage 内 checkpoint 只用于缩小 review 和验证批次。任一 checkpoint 的代码、测试或 commit 都不是
effective contract，也不能被后续 sibling RFC 当成 baseline。只有 Stage 3 的原子 cutover 可以让六个
network contract ID 与 `SYSTEM-POWER-ORDERLY-001` Refine 生效。

## 2. 实施原则

### 2.1 Host-first，但不由 host 单侧定型

Host tests 从 Stage 1 的第一个行为 checkpoint 开始使用，以可控 frame、completion、resource
exhaustion、link fact 与 monotonic time 把网络路径变成确定性的离散事件系统。但 host provider 只是一个
正式 frame contract 的 concrete test implementation，不是 production model 的替身。

Stage 1 关闭前，新增共享语义必须同时受到以下三方约束：

- 真实 `anemone-smoltcp-stack` consumer；
- test-only deterministic host frame provider；
- 基于 `VirtIONetRaw` 的 production frame provider。

若某个类型或方法只服务 host fixture、无法被 VirtIO provider 自然实现，或只反映 VirtIO descriptor/DMA
细节、无法被 host provider 诚实表达，它不得留在 `anemone-net-api`。Checkpoint 1/2 形成的 API 在
Checkpoint 3 完成前保持 provisional；Stage 1 stage-wide review 后才允许作为后续 stage 的内部基线。

### 2.2 Test control 不进入 production capability

注入 RX frame、强制 TX completion、改变 link、推进 manual clock、读取完整 backing/queue 和设置
validation-only address 都留在 test-only owner。它们不得成为 production frame trait、netdev registry
或 kernel attach API。

第一份 host provider 直接留在 `anemone-smoltcp-stack` 的 integration test 边界，不预建通用
`net-test-support` crate。只有 `net-udp` 或 `net-tcp` 出现第二个真实复用者后，才重新判断哪些 helper
值得提取。

### 2.3 Object fence 优先于局部便利

- `anemone-net-api` 不依赖 kernel 或 smoltcp，不拥有 runtime registry；
- `anemone-smoltcp-stack` 不依赖 kernel task/fd/wait object；
- driver 与 `device/net` 不依赖 smoltcp object、`InterfaceId` mapping 或 endpoint；
- kernel attach authority 不复制 descriptor/queue truth，也不保存 smoltcp private handle；
- frame token 不兼任 netdev、interface、descriptor 或 DMA identity。

如果必须通过 downcast、公开 raw descriptor/DMA、把 `SocketHandle` 放入 kernel object、让 driver 直接实现
公开的 `smoltcp::phy::Device`，或让 host fixture 使用生产侧不可实现的旁路才能完成纵切，立即停止当前
checkpoint。

### 2.4 重要 capacity 与 budget 由 KernelConfig 注入、kernel consumer 校验

production queue size、frame backing capacity 与 worker/pump budget 不以散落 literal 固定。Stage 1
只增加其 walking-skeleton 所需的最小 KernelConfig 项，并由 `conf/.defconfig` 提供初始值。

KernelConfig schema 与 xtask 只负责字段反序列化、默认值 materialization 和 Rust 常量生成，不拥有范围、
非零、幂次或跨字段关系等语义合法性。所有这类约束由消费常量的 kernel owner 使用 `const` /
`static_assert!` 在编译期检查；非法配置必须让 kernel 编译失败，不能由 xtask 提前拒绝、静默 clamp、选择
fallback 或推迟到运行期处理。

具体数值是 implementation policy，不是 RFC invariant。Stage 2 可以根据 host deterministic exhaustion 与
RV64 bounded production-path evidence 调整，
但必须继续满足 finite work、bounded live resource 与跨方向进展边界。

### 2.5 RV64-only production proof

本修订只要求 RV64 `virtio-net-device` / virtio-mmio。driver 仍消费现有 `SomeTransport<'static>` 或等价
窄 capability，不重新复制 MMIO transport owner；但 LA64 build、virtio-pci runtime、PCI interrupt
routing 与双 architecture parity 都不属于 acceptance floor，也不得在未运行时记为 coverage。

## 3. 证据轨道与结论边界

| 证据轨道 | 本 RFC 中负责证明什么 | 不能替代什么 |
| --- | --- | --- |
| host real-stack tests | frame ownership、Drop cancellation、completion、exhaustion/recheck、显式时间、bounded pump、双实例隔离 | IRQ、DMA、真实 kernel worker 与 shutdown |
| focused KUnit / source audit | registry/publication、attach rollback、IRQ context、锁边界、unsafe lifetime、object visibility | 真实 VirtIO queue 与 host-controlled完整状态空间 |
| RV64 QEMU | virtio-mmio、IRQ、DMA、RX/TX completion、active attach、worker wiring 与 orderly shutdown 顺序 | host 的确定性 exhaustion、budget 与多实例证明 |
| acceptance audit | 没有 endpoint/socket/control-plane 旁路、fake/fake 自证或 validation-only probe 遗留到 production surface | 任何未实际运行的行为 |

Host PASS 不替代 RV64 QEMU；单次 QEMU packet smoke 不替代 host ownership/progress proof；source/build
PASS 不替代 runtime。未运行项目必须写成 Not Run，不能用“代码应当支持”扩大结论。

### 3.1 Stage 1 host validation gate

`Stage 1 host validation gate` 是本文对 Stage 1 crate-level host test 集合的文档标签，不是新的
repository command。它直接使用 Cargo 原生 test harness：

```sh
cargo test -p anemone-net-api -p anemone-smoltcp-stack
```

这组测试不消费 KernelConfig、generated input、kernel target、rootfs 或 QEMU wiring，因此不在
`Justfile`、xtask 或 `scripts/` 中增加一次性 wrapper。测试实现长期留在各自 crate 的 unit / integration
test 边界；transaction 记录每次实际命令、结果和对应 proof scope。只有后续出现稳定 CI gate 或跨 RFC 的
真实重复编排需求时，才经独立 owner review 提升为 repository-level test entry。

## 4. 阶段成熟度与滚动解析

- `Outline` 只固定目的、依赖、受保护边界和 resolution trigger；不冻结具体类型、算法、文件或命令。
- `Ready` 表示交付、实现/probe 路线、审计、可观测性、验证、停止/退出条件、cutover 与
  `Resolved Write Set Manifest` 已完整解析，但尚未获得执行授权。
- `Active` 只能由 public RFC acceptance、transaction preflight 和独立用户/编排授权共同进入。
- `Closed` 要求本 stage 自己的 checkpoint、review、验证和退出条件全部满足；它不自动解析或激活下一
  stage。

Stage N 关闭后，单独运行只读 `N -> N+1 Implementation Resolution Gate`。该 gate 必须读取 live
source、Stage N 实际 diff、review findings、验证证据、module-boundary pressure、RFC target 与 current
contract，再把下一个 Outline 完整解析为 Ready。

## 5. 阶段路线图

| Stage | 成熟度 | 跨层结果 | Contract 状态 |
| --- | --- | --- | --- |
| Stage 1 — Four-layer walking skeleton | Closed | hostable seam、真实 stack/provider、VirtIO-Net、netdev publication、kernel attach/IRQ/worker、RV64 一次真实双向纵切 | 全部 Not Effective |
| Stage 1 -> 2 Boundary Interlude | Closed | same-owner module split、kernel-local provider/wake handoff、artifact-neutral validation seam 与 visibility 收窄 | 全部 Not Effective |
| Stage 2 — Bounded progress conformance | R1 Closed / Checkpoint 1-3 Closed | host deterministic exhaustion/completion/recheck、budget/deadline、公平性、link recovery 与 RV64 bounded production-path proof | 全部 Not Effective |
| Stage 3 — Multi-instance/lifecycle closure | Closed / Checkpoint 1-3 Closed | 双实例隔离、attach rollback、shutdown handoff、validation seam退出、RV64 final acceptance 与原子 cutover | `NFP-FINAL-CUTOVER` Effective |
| Stage 4 — Post-close contract conformance correction | Ready / Unauthorized / 单一checkpoint | `device/net` pending handoff、post-`Late` network activation、host-only test metadata与aggregate revalidation | 既有contract保持Effective；无cutover |

## 6. Stage 1 Ready：Four-layer walking skeleton

**状态：** Closed / Checkpoint 1-4 Closed（2026-07-26）；Stage 2 仍为未解析、未授权的 Outline

### 6.1 目的与退出形状

Stage 1 建立一条尽可能薄、但真实贯穿共享 API、smoltcp stack owner、frame provider、netdev publication
和 kernel attach/worker 的路径。它只证明架构能够站立，不试图在本 stage 完成全部 exhaustion、fairness、
multi-device 或 shutdown closure。

Stage 1 关闭时必须同时具备：

- host 上真实 stack + deterministic provider 的双向 frame 纵切；
- production `VirtIONetRaw` provider 对同一 shared semantics 的实现；
- boot-time netdev identity/publication 与 published/unattached 状态；
- kernel attach authority、single-pump owner、IRQ recheck、worker 和显式 monotonic time wiring；
- RV64 QEMU 中经真实 virtio-mmio、DMA、IRQ、stack pump 的一次 TX 与一次 RX；
- shared surface 不包含 endpoint、socket、Linux readiness/errno、driver backing 或 smoltcp object。

Stage 1 不修改 current contracts，不把部分 ID 提前 cut over，也不声称已关闭 Stage 2/3 的 proof
obligations。

### 6.2 前置条件与 activation preflight

进入 Checkpoint 1 前必须满足：

1. 公共 Draft 完成 review；
2. 当前 Draft target 被接受为 `R0 / Accepted for Implementation`，并建立独立 transaction；
3. transaction 记录 Stage 1 Ready 定义、activation authority 与当时 branch/HEAD/dirty state；
4. 重新读取 live `Cargo.toml`、`VirtIONetRaw`、`VirtIOHalImpl`、VirtIO bus/device/IRQ、kthread、timer、
   System Power 静态 plan 与 RV64 platform network args；
5. 检查 Stage 1 write set 与已有 dirty changes 的重叠。2026-07-26 drafting baseline 中
   `conf/.defconfig` 已有用户修改；activation 必须重新核验该事实，并在仍有重叠时由用户或 transaction
   preflight 明确其归属、在现有内容上做语义合并，不得覆盖；
6. `just --list`、`just build --help`、`just qemu --help` 与 wrapper 内容仍与本文命令一致；若入口漂移，
   先更新本文再激活。

### 6.3 Checkpoint 1 — Hostable seam 与 frame-token representation probe

**状态：** Closed（2026-07-26）；执行证据见
[transaction](../../devlog/transactions/2026-07-26-net-frame-path.md)。本状态不激活 Checkpoint 2。

**交付：**

- 新建 `anemone-net-api` 与 `anemone-smoltcp-stack` 两个 first-party workspace crate；
- `anemone-net-api` 默认 `no_std`，只定义 frame slice 共同需要的 opaque identity、monotonic
  time/duration、link/interface facts、frame acquisition/outcome 与 recheck/pump values；
- `anemone-smoltcp-stack` 的 production feature set 使用 `no_std + alloc`，kernel dependency 关闭默认
  feature；host tests 使用 crate 自己的 `std` test feature，不把 host OS time/network object带入
  production；
- stack 的默认 `host-test` feature 只为 package test 启用 `std` 与确定性 fixture；kernel 依赖使用
  `default-features = false`，kernel 的 `kunit` feature 只转发 stack 的 `kunit-probe`；base feature 只启用
  Ethernet/IPv4 interface 所需的 smoltcp 能力，ICMP endpoint 只由 `kunit-probe` 启用；
- 在 integration test 内建立 deterministic frame provider，拥有自己的 backing、credit、completion、
  link predicate 与 manual clock control。

**Representation probe：**

具体 Rust 表示尚未被 RFC 固定。本 checkpoint 以“associated token / owner-private erased slot”中的最小
可行形状表达 move-only RX/TX token，并用测试验证一次性 consume、callback-scoped slice 与 Drop
cancellation。成功形状必须同时满足：

- RX 只在 consume callback 暴露 `&[u8]`，TX 只在 `consume(len, ...)` 暴露 `&mut [u8]`；
- token 不可复制，未 consume 时 Drop 恢复原 owner credit；
- callback 返回后不存在可保存的借用；
- provider 可以保留 concrete backing identity，API 不需要 descriptor/DMA/queue identity；
- 接口能被后续 VirtIO provider 在不持有 protocol callback 期间的 device-wide lock 下实现；
- 若使用 unsafe/type erasure，safety comment 明确 slot identity、lifetime、唯一 owner 和取消路径。

本 probe 不比较通用 buffer framework、trait-object ecosystem 或 future transport。若候选表示需要公开
concrete buffer、共享 `Arc<Mutex<_>>` backing、让 Drop 调复杂 callback，或无法表达 RX consume 内取得
配对 TX token，删除 probe 代码并停止 Stage 1，回到 implementation/owner review。

**验证：**

- compile-time dependency audit 证明 `anemone-net-api` 不依赖 kernel/smoltcp；
- host tests 覆盖 RX consume/recycle、TX fill/submit、unconsumed RX/TX Drop、capacity rejection 与 callback
  后无法继续访问；
- [Stage 1 host validation gate](#31-stage-1-host-validation-gate)、`just fmt kernel --check`、
  `git diff --check`。

Checkpoint 1 关闭不冻结共享 surface，不自动进入 Checkpoint 2。

### 6.4 Checkpoint 2 — Real smoltcp owner 与 host vertical slice

**状态：** Closed（2026-07-26）；执行证据见
[transaction](../../devlog/transactions/2026-07-26-net-frame-path.md)。本状态不激活 Checkpoint 3。

**交付：**

- concrete stack instance 拥有 smoltcp `Interface` resource、`InterfaceId` namespace/mapping、显式
  deadline 与唯一 pump access；
- stack-local adapter 把正式 frame capability 适配为 smoltcp `Device` / `RxToken` / `TxToken`，但这些
  smoltcp 类型不出 crate；
- pump 接受调用者提供的 `anemone-net-api::Instant` 与 finite ingress/egress budget，调用
  `poll_ingress_single()`、bounded `poll_egress()` 和 `poll_at()`，返回 work remaining、immediate
  recheck 与 next deadline 的最小 outcome；
- deterministic provider 注入一份带 validation-only address/MAC 的 ICMP echo request，由真实 smoltcp
  interface 产生 response，再由 provider 观察 TX；该 fixture 只存在于 host test，不形成 address、ICMP
  endpoint 或 control-plane production API；
- 同一 test process 建立两个 stack/provider instance，至少证明 `InterfaceId`、frame credit、TX output
  与 manual time 不交叉。

**锁与 owner：**

- 同一 stack instance 任一时刻最多一个 pump；允许测试两个调用者竞争，但只有一个取得推进能力；
- provider 取得/完成 slot 时可以持 owner-local guard，protocol callback 期间不得持有覆盖 RX/TX 全局
  状态的锁；
- pump 不反向修改 provider registry 或 kernel state；
- manual clock 是输入值，不在 stack 内读取 host wall clock。

**验证：**

- [Stage 1 host validation gate](#31-stage-1-host-validation-gate) 覆盖真实 echo、finite ingress budget、
  egress outcome、next deadline、single-pump serialization 与双实例隔离；
- source audit 证明 public surface 无 smoltcp object、socket handle、host clock 或 test control；
- `just fmt kernel --check`、`git diff --check`。

若真实 smoltcp paired RX/TX 只能通过持 provider 全局锁进入 callback、泄漏 concrete token，或普通负载下
无法用 finite pump 推进，停止 Stage 1；不得用 fake protocol consumer 代替。

Checkpoint 2 关闭不自动进入 Checkpoint 3；shared surface 仍允许由 production provider evidence 修正。

### 6.5 Checkpoint 3 — VirtIO-Net frame provider 与 netdev publication

**状态：** Closed（2026-07-26）；执行证据见
[transaction](../../devlog/transactions/2026-07-26-net-frame-path.md)。本状态不激活 Checkpoint 4。

**交付：**

- 新建 driver-owned VirtIO-Net implementation，消费 `VirtIODevice::take_transport()` 返回的
  `SomeTransport<'static>` 并构造 `VirtIONetRaw<VirtIOHalImpl, SomeTransport<'static>, QUEUE_SIZE>`；
- owner-local RX/TX slot 保存 raw buffer、对应 queue token 与唯一 lifecycle；只有 begin/complete 使用
  unsafe，且每个 unsafe block 说明同一 buffer identity、device ownership window、sync point 和 failure
  cleanup；
- RX completion 在 `receive_complete()` 与必要 DMA sync 后才暴露有效 Ethernet frame；TX 在 header 与
  frame 一起 begin 后终止 CPU access，在 matching completion 后才 reclaim；
- queue full、RX empty、TX credit shortage 与 temporary link unavailable 返回 normal outcome；全局
  allocator OOM 保持已接受的 kernel-fatal 边界；
- 新建 `device/net` registry，单调分配 boot-local ifindex/name，发布 provider capability 与规范化
  MAC/MTU/link facts；driver 完成 queue、initial RX refill、IRQ request 和 provider state 后才 publication；
- probe failure 不发布 netdev。无法证明 device 已停止访问的 backing 保留到 reset/power-off，不为完整
  rollback 修改 generic bus；
- IRQ handler 只 ack/观察 driver hardware facts、更新 owner predicate 并发布 recheck edge，不调用 stack
  或 frame consume/submit。

**KernelConfig：**

在 `scripts/xtask/src/config/kconfig.rs` 与 `conf/.defconfig` 增加并生成 Stage 1 所需的最小配置：

- `virtio_net_queue_size = 64`；只接受 `4..=1024` 内的 2 的幂；
- `virtio_net_frame_capacity_bytes = 2048`；不得小于 `VirtIONetRaw` 要求的 1526 bytes；
- `net_pump_ingress_budget_frames = 32`；必须非零；
- `net_pump_egress_budget_steps = 32`；必须非零；
- `net_worker_repoll_rounds = 8`；必须非零。

`scripts/xtask/src/config/kconfig.rs` 只增加字段、默认值 materialization 与生成常量，不增加上述数值约束或
对应的 parser/generator rejection test。`anemone-kernel/src/driver/net/virtio.rs` 使用 `static_assert!` 检查
queue size 的范围/幂次和 frame capacity 下界；`anemone-kernel/src/net/worker.rs` 使用 `static_assert!` 检查
三个 budget/round 常量非零。生成的根 `kconfig` 与 `anemone-kernel/src/kconfig_defs.rs` 只由 repository
build/config 入口产生，不手改、不纳入 source write set。

**验证：**

- focused KUnit 覆盖 netdev identity/name 单调分配、duplicate publication 拒绝、publication-before-ready
  不可表达、slot transition/Drop cancellation 中可以直接执行的 owner-local helper；
- source audit 对照 `VirtIONetRaw::{receive_begin,receive_complete,transmit_begin,transmit_complete}` 和
  `VirtIOHalImpl::{share,unshare}`，证明 matching buffer/token 与 live mapping 上界；
- KernelConfig owner audit 证明 xtask 只传递配置，全部数值合法性由 consuming kernel module 的
  `static_assert!` 覆盖；默认配置的 kernel build 必须通过，非法值不得存在 runtime fallback；
- [Stage 1 host validation gate](#31-stage-1-host-validation-gate) 必须继续通过，证明 production 反馈没有让
  host contract 退化；
- `just fmt kernel --check`；
- `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`；
- `git diff --check`。

若 production provider 必须修改/fork `virtio-drivers`、公开 descriptor/DMA、建立镜像 queue truth，或
generic device lifecycle 必须扩大才能安全 publication，停止 Stage 1 并上报 target/owner/write-set
影响。Checkpoint 3 关闭不自动进入 Checkpoint 4。

### 6.6 Checkpoint 4 — Kernel attach、IRQ/worker/time wiring 与 RV64 vertical slice

**状态：** Closed（2026-07-26）；交付、stage-wide review、validation 与 write-back 已闭合；
Stage 2 resolution / activation 未获授权。

**交付：**

- 新建 `anemone-kernel::net` 唯一 attach authority；它 snapshot published netdev，向 concrete stack
  请求 `InterfaceId` mapping，准备 worker/wake/time wiring，全部成功后一次性发布 active path；
- published/unattached 是合法状态。单个 attach 失败撤销 stack-local partial mapping并保留 netdev；
  Stage 1 不做自动 retry；
- 一个 ordinary kthread 负责当前 concrete stack instance 的 pump。worker 消费 durable predicate：RX/TX
  completion、explicit work、link recheck、immediate repoll 或 deadline due；wake edge 只请求重读；
- hard IRQ 只通过 driver/provider recheck capability 唤醒 worker；不进入 smoltcp，不发布 readiness；
- kernel `Instant::now()` 在 attach owner 边界转换为 `anemone-net-api::Instant`；stack 不读取 kernel clock；
- next deadline 使用既有 threaded timer 安排 wake。stale timer callback 只造成额外 recheck，不成为第二份
  deadline truth，也不要求为本 RFC 增加通用 timer cancellation；
- worker 每轮最多执行 KernelConfig 指定的 immediate-repoll round，仍有 work 时显式 requeue/yield，不能
  busy-spin；
- active publication 后的 object graph 不让 kernel 持有 smoltcp private object，也不让 stack 持有
  `Task`、`KThreadHandle`、timer 或 IRQ object。

**RV64 validation probe：**

KUnit/test-only feature 可以在 concrete stack 私有边界建立 `10.0.2.15/24`、固定 test MAC 和 ICMP probe，
向现有 QEMU user backend gateway `10.0.2.2` 发起一次 echo。它必须经过 production attach、worker、stack
pump、VirtIO TX submit/completion、IRQ recheck、VirtIO RX completion 与 stack receive；测试只观察 probe
完成和 owner-local counters，不将 address、ICMP endpoint、packet injection 或 completion control 暴露到
`anemone-net-api`/`device/net`。非 KUnit build 不创建该 address 或 endpoint。

如果 QEMU user backend 不能稳定提供该闭环，当前 checkpoint 停止并记录 packet/IRQ evidence。允许在
implementation plan 中重新解析一个 test-only host Ethernet peer，但不得退化成 polling、driver-only
raw send/receive 或正式 control-plane API；路线改变后必须先更新本 Ready 定义和 write set，再继续。

**可观测性：**

- probe/publication：driver name、netdev name/ifindex、MAC、queue capacity、published/unattached；
- attach：netdev identity、opaque `InterfaceId`、active publication 或 owner-local失败；
- worker：process-context pump summary、budget exhaustion、immediate repoll、next deadline；
- provider：RX/TX completion count、queue-full/recheck count、live/high-water mapping；
- shutdown 不在 Stage 1 宣称完成，只保留 driver owner 能安全 quiesce/retain 的诊断。

高频 IRQ 不做普通格式化日志；counter/label 仅供诊断且不得驱动行为。Stage 1 关闭前删除临时逐包日志、
packet dump 和不受 `cfg(test)`/KUnit feature 约束的 probe control。

**验证：**

1. [Stage 1 host validation gate](#31-stage-1-host-validation-gate)；
2. `just fmt kernel --check`；
3. `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`；
4. `./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/net-frame-stage1-rv64.log`；
5. 日志必须证明 network KUnit/probe 的真实 TX、TX completion、IRQ recheck、RX completion、echo reply 与
   active attach；全量 KUnit仍打印 `All tests passed!`，并正常关机。后续 LTP 分数不属于本 stage 的网络
   语义 proof，但新增网络路径不得让 wrapper 无法完成；
6. source/dependency/unsafe audit；
7. `git diff --check` 与 `mdbook build docs`。

### 6.7 Stage-wide review、停止与退出条件

Stage 1 review 必须同时检查：

- `anemone-net-api` 被真实 stack 和两种 provider 共同约束，没有 test-only/hardware-only surface；
- frame backing 的 owner、begin/complete token 与 unsafe window 一一对应；
- callback 期间无 device-wide/provider-global guard；
- IRQ 只有 ack/fact/recheck，worker 每次醒来重读 durable predicate；
- active publication 晚于 mapping、worker、wake 与 time wiring；
- host test 使用真实 stack，RV64 probe 使用 production provider/worker，不存在 fake/fake 或 driver-only
  自证；
- 没有 endpoint/socket/readiness/control-plane production API；
- 没有为 LA64/virtio-pci、hotplug、第二种 NIC 或未来 transport预建抽象。

出现以下任一情况，停止 Stage 1：

- 无法在不泄漏 descriptor/DMA/backing identity 的条件下表达 frame capability；
- smoltcp paired token 要求在 protocol callback 期间持有 device-wide lock，或 TX exhaustion 会使 ingress
  在普通软件策略下永久失去重查机会；
- VirtIO begin/complete 无法证明同一 buffer/token identity 或 bounded live mapping；
- hard IRQ 必须直接运行 protocol callback、普通锁、blocking/reclaim 或无界工作；
- active path 只能依赖第二份 mapping/lifecycle truth；
- RV64 双向纵切只能靠正式 endpoint/control-plane、polling 或 driver-only bypass 完成；
- 实现需要修改 generic bus owner、`virtio-drivers`、System Power contract 或 RFC acceptance boundary。

前六项先进入 implementation/owner review；最后一项或任何 target/owner/acceptance 改变必须进入 RFC
review / Target Renegotiation Gate。不得用更强 fake、减少验证或保留临时旁路关闭 Stage。

Stage 1 的退出条件是四个 checkpoint 全部关闭、stage-wide review 无 Keter/Apollyon、所有验证达到上述
floor、临时 probe/日志已按边界收口，并在 transaction 记录真实证据与未运行项。Stage 1 关闭后六个
network contract 和 power Refine 仍全部 Not Effective。

### 6.8 Resolved Write Set Manifest

Stage 1 production/source write set：

- `Cargo.toml`、`Cargo.lock`；
- `anemone-kernel/Cargo.toml`；
- `anemone-kernel/crates/anemone-net-api/Cargo.toml`；
- `anemone-kernel/crates/anemone-net-api/src/lib.rs`；
- `anemone-kernel/crates/anemone-smoltcp-stack/Cargo.toml`；
- `anemone-kernel/crates/anemone-smoltcp-stack/src/lib.rs`；
- `anemone-kernel/crates/anemone-smoltcp-stack/src/adapter.rs`；
- `anemone-kernel/crates/anemone-smoltcp-stack/src/pump.rs`；
- `anemone-kernel/crates/anemone-smoltcp-stack/tests/frame_path.rs`；
- `anemone-kernel/src/main.rs`，仅声明 `net` module；
- `anemone-kernel/src/device/mod.rs`；
- `anemone-kernel/src/device/net/mod.rs`；
- `anemone-kernel/src/driver/mod.rs`；
- `anemone-kernel/src/driver/net/mod.rs`；
- `anemone-kernel/src/driver/net/virtio.rs`；
- `anemone-kernel/src/net/mod.rs`；
- `anemone-kernel/src/net/worker.rs`；
- `scripts/xtask/src/config/kconfig.rs`，仅增加字段、默认值 materialization 与生成常量，不增加语义合法性
  检验；
- `conf/.defconfig`。

Stage 1 validation-only write set：

- 上述 stack/kernel owner 文件中的 `cfg(test)`、KUnit 或显式 test feature probe；
- `build/**` host/build/QEMU outputs 与 `build/net-frame-stage1-rv64.log`；
- wrapper 自动创建的 worktree-local runtime disk copy。不得写入
  `etc/preliminary/images/sdcard-rv.img` master。

生成文件 `kconfig`、`anemone-kernel/src/kconfig_defs.rs`、`anemone-kernel/src/platform_defs.rs`、
`build/**` 不是 source write set；只能通过 repository owner 生成。

Stage 1 默认只读：

- `anemone-kernel/crates/anemos/smoltcp/**`，只消费现有 bounded poll API；
- `anemone-kernel/src/driver/virtio/{mod.rs,mmio.rs,pcie.rs}`；
- `anemone-kernel/src/device/bus/**`、generic `Device` / `Driver` owner；
- `anemone-kernel/src/exception/intr/**`；
- `anemone-kernel/src/task/kthread/**`、scheduler/wait core；
- `anemone-kernel/src/time/**`；
- `anemone-kernel/src/power.rs` 与 current power contract；
- syscall、VFS、fd/wait/iomux、apps、rootfs manifest、LTP profile；
- LA64 platform、PCIe transport 与 LA64 wrapper/config。

若需要修改默认只读 owner，先停止并上报原因、精确文件、owner/contract 影响、替代方案和新增验证；批准后
先更新本 manifest，再在 transaction 记录 expansion authority。不得为适配冻结 write set 而把状态塞进
错误 owner。

当前公共 Draft 的文档写集仅包括 RFC 本身及公共导航；进入 R0/transaction 后，Stage 1 transaction
的文档 write set 精确切换为：

- `docs/src/rfcs/net-frame-path/{index.md,invariants.md,implementation.md,tracking-issues.md}`；
- `docs/src/devlog/transactions/2026-07-26-net-frame-path.md`；
- `docs/src/devlog/transactions/index.md`；
- `docs/src/devlog/2026-07-20_to_2026-08-02.md`；
- `docs/src/rfcs.md`；
- `docs/src/SUMMARY.md`。

若 transaction 实际跨日期，必须在 activation 前把上面的 transaction/biweekly 文件名改为
真实 canonical path；这是 manifest resolution，不授权同时扩大生产 write set。Stage 1 不更新 current
network contract；register/current-limitations 只有在出现真实 issue/accepted gap 并获 write-set expansion
后才写。已退役的私有草案路径不得成为公共链接或执行事实权威。

## 7. Stage 1 -> 2 Boundary Interlude：Owner 与 validation boundary 整理

**状态：** Closed（2026-07-26）；Stage 2 仍未解析且未授权

### 7.1 反馈与边界判断

Stage 1 的 runtime、frame ownership 与 pump evidence 保持有效；本间章不重新打开或改写其历史 closure。
关闭后的 live module review 发现三项结构反馈：

- `anemone-net-api`、`anemone-smoltcp-stack` 与 VirtIO-Net implementation 已形成多个稳定职责，但部分职责
  仍集中在单个文件；继续向其中加入 Stage 2 progress logic 会固化混合边界；
- kernel `kunit` feature 直接向 stack crate 传播 `kunit-probe`，stack 的 feature、类型和方法因而理解具体
  test harness，而不是只表达 artifact-neutral validation capability；
- `anemone-kernel::net::worker` 直接依赖 `driver::net::virtio` 的 provider、wake trait 与 diagnostics，迫使
  concrete driver module crate-wide visible，并把 attach-owned wake capability命名为 VirtIO policy。

这些问题没有改变 R0 target：shared frame semantics、stack state owner、driver queue/DMA owner、kernel attach/
worker owner 与 IRQ edge-only 规则均保持不变。它们属于 Stage 1 实现反馈后的 route correction，不递增 RFC
revision，也不触发 current-contract cutover。

### 7.2 交付与 owner route

1. `anemone-net-api` 按 interface、time、frame 与 pump stable surface 拆成 private modules；crate root 保持
   现有 public re-export，consumer path 与 shared semantics 不变。
2. `anemone-smoltcp-stack` 分离 smoltcp adapter、interface/mapping owner、bounded pump 与 validation-only
   ICMP probe；probe feature/type/method 按 capability 命名，不出现 KUnit vocabulary。kernel 可以由自己的
   `kunit` feature启用该 validation seam，但 stack 不理解调用者使用的 harness。
3. `device/net` 定义 kernel-local frame-provider handoff：provider仍唯一拥有 durable recheck predicate，
   attach owner只提供不携带 task/driver state 的 narrow wake capability。该 handoff不进入
   `anemone-net-api`，host provider也不被迫实现 kernel scheduling policy。
4. worker通过上述 kernel-local provider port消费 concrete provider，不 import `driver::net::virtio`；
   `PumpControl`实现通用 wake capability，不实现 VirtIO-named trait。VirtIO module恢复 private，
   `driver::net`只保留 attach/validation实际需要的窄 facade。
5. VirtIO-Net按 registration/publication、raw device/IRQ、frame slot/token职责做 same-owner directory split。
   provider diagnostics保持只读诊断，不驱动 queue、worker或probe completion状态；KUnit-specific wait/assert
   留在 kernel `net` validation boundary。

本间章不增加第二种 production driver、trait-object/type-erased frame framework、通用 test-support crate、
endpoint/control-plane API、shutdown能力或 Stage 2 progress semantics。

### 7.3 Review、验证与停止条件

Review必须确认：

- root re-export与 frame/pump visible semantics未改变；新 module visibility只收窄不扩大；
- stack production feature graph仍为 `no_std + alloc`，非 validation build没有地址、ICMP endpoint或 probe；
- wake只发布 edge，provider recheck predicate仍是durable truth，worker醒来后仍重新读取；
- worker不依赖 concrete VirtIO module、descriptor/DMA/queue token或 driver-private lock；
- 文件拆分没有改变 unsafe begin/complete window、provider/slot drop order或 publication/attach顺序；
- validation completion与diagnostic counters仍分离，纯诊断字段不驱动 production行为。

验证 floor：

1. `cargo test -p anemone-net-api -p anemone-smoltcp-stack`；
2. `cargo check -p anemone-smoltcp-stack --no-default-features` 与启用 artifact-neutral validation feature 的
   no-default check；
3. `just fmt kernel --check` 与 focused changed-file format audit；
4. `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`；
5. RV64 wrapper重新证明 active attach、真实 TX/TX completion、IRQ recheck、RX completion、echo reply、
   全量 KUnit通过与正常shutdown；
6. dependency/visibility/unsafe/source audit、`git diff --check` 与 `mdbook build docs`。

若实现需要改变 `FrameProvider` shared semantics、把kernel wake加入`anemone-net-api`、让driver访问stack/task
内部状态、修改generic bus/IRQ/kthread/timer/power owner，或改变R0 acceptance boundary，立即停止并进入
RFC owner review。普通Rust module拆分或为既有handoff增加kernel-local窄trait不命中该停止条件。

### 7.4 Resolved Write Set Manifest

Production/source：

- `anemone-kernel/Cargo.toml`；
- `anemone-kernel/crates/anemone-net-api/src/{lib.rs,interface.rs,time.rs,frame.rs,pump.rs}`；
- `anemone-kernel/crates/anemone-smoltcp-stack/{Cargo.toml,src/lib.rs,src/adapter.rs,src/stack.rs,src/pump.rs,src/validation.rs}`；
- `anemone-kernel/src/device/net/{mod.rs,registry.rs,provider.rs}`；
- `anemone-kernel/src/driver/{mod.rs,net/mod.rs,net/virtio/mod.rs,net/virtio/device.rs,net/virtio/frame.rs}`；
- `anemone-kernel/src/net/{mod.rs,worker.rs,validation.rs}`。

Validation-only与文档：

- 上述owner文件中的`cfg(test)`、kernel KUnit与artifact-neutral validation feature；
- `docs/src/rfcs/net-frame-path/{index.md,implementation.md,tracking-issues.md}`；
- `docs/src/devlog/transactions/2026-07-26-net-frame-path.md`；
- `docs/src/devlog/2026-07-20_to_2026-08-02.md`；
- repository owner生成的`build/**`与wrapper runtime disk copy。

`Cargo.lock`仅在feature graph实际改变lock resolution时写入。vendored smoltcp、generic device/bus/IRQ、
task/kthread、timer/scheduler、power、apps/rootfs/LTP profile、LA64/PCIe与current contracts全部只读。

间章关闭后才开始独立的`1 -> 2 Implementation Resolution Gate`；本间章不是Stage 2 Ready定义，也不授予
Stage 2执行权限。

### 7.5 Closure

间章按原 R0 route 保持行为不变并于 2026-07-26 关闭：shared API 的 root re-export 与语义未变；stack
只理解 `icmp-validation-probe`，KUnit request/wait/assert 只存在于 kernel；`device/net` 的
`NetdevFrameProvider` / `RecheckWake` 是 kernel-local port，worker 不再命名 concrete VirtIO 类型；
`driver::net::virtio` 恢复 private，Stage 1 单 NIC diagnostic query 也只在 KUnit build 中存在并带 Stage 3
替换条件。same-owner directory split 没有改变 publication、attach、slot ownership、unsafe begin/complete
window 或 drop order。

验证证据见 transaction：host gate 的 1 个 stack unit、9 个 integration 与 2 个 compile-fail doctest通过；
base 和 `icmp-validation-probe` 两种 no-default check通过；RV64 release build通过；RV64 wrapper中 260/260
KUnit、真实 RX/TX completion、IRQ recheck、ICMP echo与正常 power-off通过。focused changed-file format audit、
dependency/visibility/unsafe/source audit、whitespace和文档构建通过；repository-wide formatter只报告三个既有
vendored smoltcp diff。没有命中停止条件，NFP-004/005/006 已 neutralize。

本 closure 不修改 current contract、R0 target/revision、ABI、visible semantics 或 acceptance boundary。
六个 network IDs 与 System Power Refine继续 Not Effective；Stage 2仍为 Outline / Not Resolved /
Unauthorized，必须由后续独立 resolution gate解析。

## 8. Stage 2 Ready：Bounded progress conformance

**状态：** R1 Closed / Checkpoint 1-3 Closed（2026-07-27）；`2 -> 3`resolution与Stage 3均未授权。
2026-07-27 的 R0 RV64 burst route因未观察到TX exhaustion而按failure signal停止并删除probe；该历史不重写。
用户随后接受R1 proof-boundary correction，并分别独立授权、关闭R1 Checkpoint 1、Checkpoint 2与Checkpoint 3；
Stage 2 closure不授权后续resolution gate或Stage 3。

R1 保持normal exhaustion/recovery target不变：host real-stack + deterministic provider负责确定性制造并证明
exhaustion；RV64负责真实VirtIO bounded completion/IRQ/reclaim。`queue-full == 0`只表示本次production workload
未发生failed admission，不再单独构成失败。

### 8.1 R1 resolution preflight 与 live gap

解析输入为 Stage 1 与 Boundary Interlude aggregate diff、R0 Checkpoint 1负证据、transaction中的host/RV64
evidence、四个VirtIO unsafe begin/complete window、NFP-004/005/006/007 neutralization、R1 target/current
contracts，以及live shared API、stack pump、VirtIO provider、kernel-local provider port、worker、validation
feature、KernelConfig和QEMU wrapper。

当前 owner route 可以在不改变shared semantic surface的前提下完成本阶段：

- `FrameProvider` associated token/outcome已经表达callback-scoped ownership、normal exhaustion和link
  unavailable；deterministic host provider可以在test owner内控制自己的有限credit、matching completion、
  link和recheck edge，不需要production test-control；
- concrete VirtIO provider以固定RX/TX slot、queue token和matching completion独占资源；每次admission前回收
  已完成TX是正常progress行为，不能为测试延迟；
- IRQ只ack并先提交durable recheck bit、再尝试wake；worker不读取queue truth；`Stack`的独占`&mut self`仍是
  唯一pump capability；
- R0的128-packet probe只证明累计workload大于credit，不保证concurrent outstanding超过credit。当前保留日志
  没有最终counter summary，故completion时序仍只作为工作假设；R1不把该假设升级为事实。

live code同时保留三项Stage 2必须闭合、但不改变R1 target的implementation gap：

1. adapter只把TX exhaustion记作blocked work；link unavailable尚未进入同一“等待owner fact变化”边界；
2. pump即使已被TX/link阻塞，仍会因due deadline或budget boundary返回immediate recheck；worker按
   `NET_WORKER_REPOLL_ROUNDS`重复pump、request/yield，可能在资源恢复前持续消耗CPU；
3. 每次pump固定ingress-first；持续ingress response可以先耗尽本轮TX credit，使queued egress在普通workload
   下长期失去software admission机会。

这些是原bounded-progress target内的implementation gap。R1只调整proof assignment，不改变owner、public API、
ABI/visible semantics、contract delta或六个network ID的Not Effective状态。

### 8.2 交付、边界与 checkpoint 顺序

Stage 2关闭时必须交付：

- host deterministic provider对token cancel/unwind、paired RX/TX、bounded exhaustion/completion、link
  down/up、recheck coalescing、budget/deadline与跨方向公平性的完整矩阵；
- stack pump在owner-blocked时不immediate repoll，在无需等待owner fact且达到budget时仍有限重查；
- concrete VirtIO provider的slot/queue/mapping上界、completion reclaim与durable recheck protocol审计；
- RV64 virtio-mmio真实bounded burst上的TX/RX completion、IRQ recheck、outstanding上界、mapping回落、有限
  worker action和正常关机证据；自然观察到exhaustion时还要证明其后恢复。

本阶段按以下checkpoint依次执行；每项都需要独立activation、review、validation、transaction write-back与
closure，前一项关闭不自动激活后一项：

1. Checkpoint 1 — deterministic exhaustion seam与RV64 production observability；
2. Checkpoint 2 — host ownership/progress/pump conformance；
3. Checkpoint 3 — VirtIO provider/recheck/worker closure与RV64 acceptance。

每次activation前，transaction必须记录branch/HEAD/dirty state、上一checkpoint closure、manifest与用户改动
重叠、live KernelConfig/feature graph、`just` help和RV64 wrapper是否漂移。发现非本stage dirty overlap时先确认
归属；发现owner/API/validation route漂移时重新解析对应checkpoint，不机械执行本文路径。

受保护边界：

- frame/DMA唯一owner、callback-scoped access、matching completion与callback期间无device-wide lock不得弱化；
- notification只是edge，queue/link/resource durable truth仍由provider拥有，deadline仍由stack拥有；
- hard IRQ不推进smoltcp、不触碰frame slot、不执行复杂callback/drop；
- allocator OOM仍是已接受的system-level fatal boundary，不能伪装为normal network backpressure；
- 不改变`anemone-net-api` public surface，不修改vendored smoltcp/`virtio-drivers`，不引入endpoint、socket/
  control-plane、通用buffer/pool/workqueue或第二种production provider；
- 多netdev attach/rollback、shutdown、System Power Refine与contract cutover继续属于Stage 3。

### 8.3 Checkpoint 1 — deterministic exhaustion seam与RV64 production observability

**状态：** Closed（2026-07-27）。Host deterministic exhaustion与fresh RV64 production-path evidence均通过；
执行、review与验证证据见[transaction](../../devlog/transactions/2026-07-26-net-frame-path.md)。本状态不激活
Checkpoint 2。

**假设与proof split：** shared frame/pump surface已经足够让真实stack与test-owned小容量provider确定性走过
`Ready -> Exhausted -> matching completion/recheck -> resumed submission`，同时现有RV64 validation seam可以在
不控制device completion的前提下证明真实VirtIO bounded completion/reclaim。两条证据必须分别通过，互不替代。

**Host deterministic probe：**

1. 按test owner最小拆分现有integration fixture：walking-skeleton/identity保留在`tests/frame_path.rs`，共享
   provider/clock/frame builder移入`tests/support/mod.rs`，新增`tests/bounded_progress.rs`承载本probe；不创建
   test-support crate或production API；
2. 使用真实`Stack::pump`、现有packet builder/socket路径与capacity为2的deterministic provider，预排3个可区分
   egress frame；test provider先不归还completion，pump egress budget至少允许尝试第三个frame；
3. 证明前两个frame提交、第三次admission返回normal exhaustion、live TX不超过2、未发送frame仍由stack/socket
   owner保留且无重复/丢失。记录当前`PumpOutcome`作为Checkpoint 2输入；Checkpoint 1不提前要求修正已知的
   blocked-immediate gap；
4. 无credit时再做一次有界调用，证明没有新submission或owner corruption；随后只归还一个matching completion，
   发布重复/coalesced recheck edge，再pump并证明第三个frame提交。最终归还全部completion，slot/live resource
   回到baseline；
5. host test-control只修改test provider自己的credit、completion、link和edge，不进入shared trait、kernel port或
   concrete VirtIO provider。

**RV64 production-path probe：**

1. 保留`icmp-validation-probe` feature与object fence；stack validation owner按调用者给出的有界burst准备ICMP
   metadata/payload，只导出enqueue/drain/reply completion fact，不导出`SocketHandle`、地址或packet injection；
2. kernel KUnit把workload固定为`2 * VIRTIO_NET_QUEUE_SIZE`个不同sequence（checked arithmetic）。该值只表达
   bounded validation workload，不声称能够强制并发outstanding超过credit，也不进入KernelConfig；
3. concrete provider只增加KUnit-only、owner-scoped diagnostic mirror：TX submit/completion、normal exhaustion、
   current/high-water outstanding、IRQ recheck与live/high-water mapping；worker只镜像action count、单次最大pump
   rounds和yield/request次数。字段声明必须注明纯诊断，不能驱动queue、pump、worker或probe completion；
4. stack报告全部reply完成后，KUnit用`yield_now()` + monotonic deadline的bounded predicate loop等待TX
   submit/completion匹配、outstanding归零、live mapping回到probe前persistent RX baseline；每个sequence必须只
   完成一次；
5. 在最终assertion前打印一次摘要，包含burst、reply、submit/completion、normal exhaustion、outstanding、IRQ、
   live/high-water mapping、worker action/round/yield和baseline，确保失败日志保留判定证据；
6. mandatory PASS不要求`normal exhaustion > 0`。若自然观察到exhaustion，diagnostic还必须证明first exhaustion
   后存在新的submission与matching completion；若为0，只能记录“本次未观察到”，host probe仍负责确定性proof。

**Checkpoint review：** 确认host fixture使用真实stack和正式frame contract；RV64仍走普通pump/provider/IRQ/
completion路径；diagnostics只镜像owner transition；没有为测试改变production capacity、harvest顺序、worker
policy或public surface；旧R0 probe没有被改写为PASS。

**禁止：** 不得延迟/吞掉真实completion、强制queue-full、降低production slot/queue capacity、改写slot
ownership、向generic provider port加入test-control、使用sleep决定正确性，或让diagnostics成为行为真相。

**验证与退出：** 运行host gate、两种feature check、focused/repository format、RV64 release build、一次RV64
wrapper、source/dependency/diagnostic-boundary audit、whitespace与mdBook。Checkpoint 1只有在host确定性观察并恢复
exhaustion，且RV64完成全部reply、matching completion、IRQ、outstanding/mapping回落、bounded worker action、
全量KUnit与正常关机后才能Closed。若host无法在不改变shared semantics的条件下制造exhaustion，或RV64真实路径
出现leak、timeout、panic、无界repoll/丢包，删除未完成probe并停止；RV64`normal exhaustion == 0`本身不停止。

成功后host support与RV64 validation seam保留给Checkpoint 2/3复用，并在Stage 3 final audit删除或由accepted
production control-plane替换。Checkpoint 1关闭后立即停止，不自动激活Checkpoint 2。

### 8.4 Checkpoint 2 — host ownership/progress/pump conformance

**状态：** Closed（2026-07-27）。Host ownership/progress/pump matrix与checkpoint-scoped review、validation、
write-back均已闭合；执行证据见
[transaction](../../devlog/transactions/2026-07-26-net-frame-path.md)。本状态不激活Checkpoint 3。

在Checkpoint 1最小fixture上扩展deterministic provider矩阵：

- RX/TX token未consume、oversize与callback unwind均恢复原owner状态；callback外slice不可逃逸的doctest继续
  通过；paired RX/TX callback不重入provider-global lock；
- RX ready但无TX credit保持原RX、返回`TransmitExhausted`；TX/RX completion只释放matching slot；达到
  capacity返回normal outcome且live resource不越界；
- 多次resource return/link change只需要coalesced edge，edge先于wait或重复到达都不丢durable predicate；
  link down不丢identity/frame，link up后同一owner恢复推进；这里只证明同一provider上的frame progress，
  不提前证明registry publication或Stage 3 lifecycle；
- ingress/egress budget分别在0之外的精确边界停止；future deadline不提前运行，due deadline在provider可推进时
  immediate，在TX/link blocked时只保留`work_remaining`并等待owner recheck；
- 连续ingress与queued egress同时存在时，两方向在有限pump次数内都取得一次推进机会；现有outer mutex
  serialization与双stack/provider隔离保持回归。

production stack route保持现有shared types：adapter将TX exhaustion与link unavailable都标记为owner-blocked；
pump只有在本轮未被owner-blocked且budget耗尽/deadline到期时返回`Recheck::Immediate`，blocked work仍以
`work_remaining=true`保留；每个interface保存一个owner-local、不可外泄的下轮优先方向，在每次pump后切换
ingress-first/egress-first，两个方向仍各自受`PumpBudget`约束。它不是第二份queue/deadline truth，也不保证
device saturation下无暂时停顿。

Checkpoint 2不调整默认KernelConfig数值。若host矩阵只能通过改变`PumpOutcome`字段/含义、shared
`FrameProvider` outcome、smoltcp fork或固定response reserve才能通过，立即停止并进入RFC review。

**Closure：** deterministic provider现覆盖RX/TX未consume、oversize、RX/TX callback unwind、paired consume、
matching completion、RX-under-TX-exhaustion、capacity bound、resource/link coalesced recheck、link recovery、
nonzero ingress/egress exact budget、future/due deadline与连续ingress + queued egress公平性。adapter把TX
exhaustion和link unavailable统一归为owner-blocked；pump只在未blocked时按budget/deadline返回Immediate，并在
blocked且deadline已到期时清除本次到期deadline，避免只读worker的deadline predicate立即重新进入。每个
interface的private `PumpOrder`只轮换下次software admission顺序，不缓存queue/link/resource/deadline truth。

独立只读review pass先发现blocked pump虽返回Idle、但保留due deadline仍会让worker outer predicate busy-repoll
这一Keter；上述due-deadline suppression修复后复审为Apollyon 0、Keter 0、Euclid 0。host gate、两种
no-default feature check与RV64 release build通过；repository formatter只保留Stage 1已有的三个vendored
smoltcp baseline diff。本checkpoint没有运行QEMU/runtime，也没有修改KernelConfig、production provider、worker、
shared API、current contract或acceptance。Checkpoint 3继续Not Activated / Unauthorized。

### 8.5 Checkpoint 3 — provider/recheck/worker closure与RV64 acceptance

**状态：** Closed（2026-07-27）。provider/recheck/worker closure、独立review、validation floor与两次
fresh-disk RV64 acceptance均已通过；执行证据见
[transaction](../../devlog/transactions/2026-07-26-net-frame-path.md)。本状态不运行`2 -> 3`resolution或
激活Stage 3。

concrete provider保持slot为唯一frame/queue-token owner，并只做以下局部硬化：

- 把当前recheck bit + weak wake收拢为driver-private owner-local latch；publish必须先commit predicate再wake，
  take只清除predicate。KUnit覆盖wake尚未安装、重复/coalesced publish、take后再次publish，证明wake丢失或
  合并不丢durable fact；该latch不进入`device/net` shared port或`anemone-net-api`；
- 对RX/TX slot transition、matching queue token、outstanding/live mapping增减与capacity上界保留release
  `assert!`；diagnostic snapshot继续只镜像owner transition，不反向决定production行为；
- completion harvest保持由provider在process context执行，工作量受固定slot/queue capacity约束；IRQ仍只
  ack、publish recheck和wake，不取得slot或协议callback；
- worker继续只把`Recheck::Immediate`作为同一wake cycle的repoll依据；owner-blocked`work_remaining`等待
  provider predicate，due time等待stack deadline。达到`NET_WORKER_REPOLL_ROUNDS`必须request、yield后重新竞争，
  不在一个worker action内无限循环。

复用Checkpoint 1 bounded burst与summary，把KUnit-only单NIC query重命名为Stage 2 conformance语义；它仍带
Stage 3多设备前删除/替换条件，不形成control-plane。以fresh runtime disk连续运行两次RV64：每次都要求全部
reply、matching completion、IRQ、outstanding/mapping回落、bounded worker action、全量KUnit和正常关机；若任次
自然观察到exhaustion，还必须证明其后恢复。两次均未观察到exhaustion不阻塞，因为该proof由host矩阵拥有。

Checkpoint 3可根据host与两次RV64 evidence调整现有`net_pump_*_budget` / `net_worker_repoll_rounds`数值，但不得
新增配置维度、把test burst写入KernelConfig或由xtask拥有合法性；所有non-zero/capacity关系仍由kernel
compile-time assertion验证。

### 8.6 Review、审计与可观测性

Stage-wide review至少逐项确认：

- `RxOwnership` / `TxOwnership`每条成功、normal exhaustion、Drop、callback unwind与completion路径只有一个
  owner；四个unsafe begin/complete block仍使用matching token与stable boxed backing，CPU/DMA窗口不重叠；
- outstanding/live mapping不超过固定RX+TX slot credit，TX completion后回落，persistent RX mapping不被误报
  成leak；
- adapter/pump没有把notification、diagnostic counter或priority cursor当作queue/link/deadline truth；
- IRQ/raw lock不跨protocol callback，wake publication顺序不丢predicate，worker clear与并发publish可重查；
- alternating phase只保证software admission公平，不承诺真实device saturation下同步双向进展；每次pump、
  每个worker action与completion scan都有静态上界；
- validation feature不出现在非KUnit production build，不公开endpoint/socket/control-plane或driver test-control；
- `anemone-net-api`、vendored smoltcp、generic IRQ/kthread/timer/scheduler、attach registry与power均无diff。

可观测性只保留本stage需要的KUnit snapshot与单次summary：burst/reply、pump action/round/yield、RX/TX
completion、natural exhaustion、outstanding、IRQ recheck、live/high-water mapping和最终baseline。禁止每packet、
每completion、每IRQ或每repoll日志。纯诊断字段必须在声明处标注，不参与production state decision；summary在
最终assertion前输出。

### 8.7 验证 floor

每个checkpoint按其范围运行子集；Stage 2最终至少需要：

1. `cargo test -p anemone-net-api -p anemone-smoltcp-stack`，其中host deterministic probe必须实际观察并恢复
   exhaustion；
2. `cargo check -p anemone-smoltcp-stack --no-default-features`与
   `cargo check -p anemone-smoltcp-stack --no-default-features --features icmp-validation-probe`；
3. `just fmt kernel --check`与所有Stage 2 changed Rust file的focused format check；若repository-wide命令仍
   失败，只能接受与Stage 1 baseline完全相同的三个vendored smoltcp formatter diff，任何新增diff都阻塞；
4. `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`；
5. Checkpoint 1运行一次RV64 wrapper；Checkpoint 3再以fresh runtime disk连续运行两次
   `./scripts/run-user-test-rv64.sh <sdcard-image> <log>`。调用者每次显式选择同一只读pretest master，wrapper复制
   为worktree-local runtime disk。每次都要求全量KUnit、bounded burst全部reply、matching completion、IRQ、
   outstanding/mapping回落、bounded worker action、无timeout/panic/busy-spin迹象和正常关机；natural
   exhaustion只在观察到时追加恢复义务；
6. dependency/public-surface、slot/mapping/unsafe、IRQ/wake/worker、deadline/fairness、validation-bypass与
   manifest audit；`git diff --check`与`mdbook build docs`。

RV64普通wrapper仍固定`smp=1`，因此本stage不声称SMP race coverage。LA64 build/runtime、virtio-pci、hardware、
final harness、LTP和socket/control-plane测试全部Not Run；现有signal/wait profile结果不参与network closure。

### 8.8 Contract、停止与退出条件

Stage 2 contract cutover为None。六个network IDs与`SYSTEM-POWER-ORDERLY-001` Refine在全部checkpoint通过后仍
Not Effective；不创建或修改network current contract。

除各checkpoint局部failure signal外，以下任一情况立即停止并上报manifest expansion或Target Renegotiation：

- 需要改变shared API、状态owner、public visibility、ABI/visible semantics或R1 acceptance boundary；
- 需要修改vendored smoltcp/virtio dependency、generic IRQ/task/kthread/timer/scheduler、attach lifecycle、power、
  apps/rootfs/LTP或QEMU platform/wrapper来制造PASS；
- queue/link/resource truth必须复制到worker，diagnostics必须驱动production，或callback必须持device-wide lock；
- host deterministic provider不能在正式frame contract内制造/恢复exhaustion，或host conformance仍出现panic、
  无界repoll、lost durable predicate、mapping leak或普通workload方向永久饥饿；
- RV64任次burst丢失completion/reply/IRQ、outstanding或mapping不回落、worker action越界、无法正常关机，或两次
  final run任一失败；`normal exhaustion == 0`本身不是停止条件；
- review仍有Apollyon/Keter/Euclid。

退出要求三个checkpoint全部Closed、所有矩阵与audit达到floor、probe/temporary query带明确Stage 3退出条件、
transaction追加完整evidence并同步lifecycle文档。随后Stage 2记为Closed并立即停止；不得自动运行`2 -> 3`
resolution gate、进入Stage 3或cutover contract。

### 8.9 Resolved Write Set Manifest

允许production/source写入：

- `anemone-kernel/crates/anemone-smoltcp-stack/src/{adapter.rs,pump.rs,stack.rs,validation.rs}`；
- `anemone-kernel/src/driver/net/mod.rs`，仅允许Checkpoint 3同步KUnit-only conformance query的
  crate-private re-export命名；
- `anemone-kernel/src/driver/net/virtio/{device.rs,frame.rs,mod.rs}`；
- `anemone-kernel/src/net/{worker.rs,validation.rs}`；
- `conf/.defconfig`只允许Checkpoint 3依据证据调整已经存在的三个pump/repoll数值；不得增加schema字段。

2026-07-27 Checkpoint 3 preflight发现conformance query的定义与调用均在原manifest内，但唯一crate-private
re-export位于`driver/net/mod.rs`。用户批准把该文件按上述单行命名同步加入manifest；扩展仍在同一driver owner
内，不改变public API、shared contract、ABI、visible semantics或acceptance，验证floor保持不变。

允许validation与test写入：

- `anemone-kernel/crates/anemone-smoltcp-stack/tests/frame_path.rs`；
- 新建`anemone-kernel/crates/anemone-smoltcp-stack/tests/{bounded_progress.rs,support/mod.rs}`；
- 上述owner文件中的crate test、KUnit、`icmp-validation-probe`实现与KUnit-only snapshot；
- repository owner生成的`build/**`和wrapper runtime disk copy。

允许文档写回：

- `docs/src/rfcs/net-frame-path/{index.md,invariants.md,implementation.md,tracking-issues.md}`用于获准的R1
  Target Renegotiation、route correction、manifest expansion与closure；
- `docs/src/devlog/transactions/2026-07-26-net-frame-path.md`逐checkpoint追加activation、probe、review、validation、
  Not Run与closure事实；
- `docs/src/rfcs.md`、transaction index与当前双周devlog只同步revision/stage lifecycle。

明确只读：`anemone-net-api`、vendored smoltcp、`virtio-drivers`/`Cargo.lock`、`anemone-kernel/Cargo.toml`、
`device/net/**`、`net/mod.rs`、generic device/bus/IRQ/task/kthread/timer/scheduler/power、xtask/Justfile/platform、
apps/rootfs/LTP、register/current limitations与current contracts。若测试证明kernel-local provider port本身必须
改变，先停止并申请逐文件扩展，不能借`device/net/provider.rs`绕过manifest。

## 9. Stage 3 Ready：Multi-instance、lifecycle 与最终 cutover

**状态：** Closed / Checkpoint 1-3 Closed（2026-07-27）/ `NFP-FINAL-CUTOVER` Effective。

### 9.1 Resolution preflight 与 live gap

`2 -> 3 Implementation Resolution Gate`在`dev/drc/alpha@7fad0f67`的clean worktree上读取R1 target、
invariants、tracking issues、Stage 2 aggregate diff/review/validation、System Power current contract与
register，并重新审计live netdev registry、stack mapping、attach publication、worker/timer、VirtIO provider、
driver shutdown、power static plan、KUnit feature graph、RV64 Platform与pretest wrapper。

Stage 2已稳定闭合frame ownership、normal exhaustion/recovery、bounded pump、deadline/fairness、driver-private
recheck与单NIC RV64 production path；Stage 3仍有以下live gap：

1. 现有host双实例case只证明output与manual time不交叉，尚未在同一矩阵闭合credit、matching completion、
   link、mapping rollback与一侧失败不影响另一侧；registry KUnit只覆盖identity/name与duplicate origin。
2. `worker::prepare()`已经在spawn失败时撤销stack-local mapping、保留provider backing，并让外层for-loop继续，
   但这些rollback/independence路径仍只有source形状，没有Stage 3组合证据。`NetdevSnapshot`中的link字段是
   publication-time snapshot，当前注释也尚未明确其允许stale且不能反向驱动pump。
3. `ACTIVE_PATHS`只拥有active publication，没有shutdown-admission truth或network facade；`PumpControl`只有
   activation/work predicate。threaded timer没有cancel API，System Power live plan仍是
   `filesystem -> device`。
4. network worker拥有`VirtIONetProvider`；provider又是IRQ注册后slot/backing与`VirtIONetDevice`的唯一durable
   strong owner。若shutdown简单`request_stop()`并让worker正常Drop，device尚未reset/quiesce时会释放仍可能被
   DMA/IRQ访问的resource，违反`NET-FRAME-OWN-001`与`NET-ATTACH-001`。本stage必须在terminal stop路径显式
   retention，而不能把普通kthread退出冒充安全teardown。
5. Stage 2的`icmp-validation-probe`、worker/provider diagnostic mirrors、single-active-path KUnit与
   `stage2_conformance_stats()`仍是有明确退出条件的validation seam。它们可用于Stage 3首次RV64 lifecycle
   evidence，但必须在`NFP-FINAL-CUTOVER`前删除；host-only validation control不因此删除。
6. `device::shutdown()`的旧注释声称network会释放全部buffer，与R1允许unsafe resource保留到reset/power-off
   的accepted boundary不符；cutover前必须校正为当前实际owner义务。

以上都是R1既定multi-instance/lifecycle/cutover范围内的implementation与proof gap，不改变target、owner、
shared API、ABI/visible semantics、platform scope或acceptance，因此保持R1，不新增tracking issue。

### 9.2 交付、边界与checkpoint顺序

Stage 3按以下三个checkpoint顺序执行；任一checkpoint关闭后立即停止，下一checkpoint必须独立授权：

1. **Multi-instance与attach conformance：** 用真实stack/provider host矩阵、registry KUnit与live source audit
   闭合identity、credit、completion、link、mapping与failure isolation；不接入shutdown或改current contract。
2. **Shutdown handoff vertical slice：** 由kernel attach owner建立唯一shutdown-admission truth与有限stop/
   retention route，把network facade接入System Power静态plan，并用保留的validation seam完成一次RV64有流量
   lifecycle proof；contract仍Not Effective。
3. **Validation exit、final acceptance与cutover：** 删除Stage 2临时ICMP/single-NIC/diagnostic seam，复跑final
   source/build/RV64 evidence，然后原子建立六个network current IDs、Refine
   `SYSTEM-POWER-ORDERLY-001`并关闭RFC/transaction。

全程保持以下边界：

- 多设备证明必须覆盖两个真实stack/provider owner domain；`Vec`形状、两个相等但分属不同stack namespace的
  `InterfaceId`值或无global singleton都不能单独形成PASS。
- published/unattached是registry中的合法durable fact；attach失败只撤销transaction-local stack mapping，
  不注销netdev identity，不回滚其它active path，不自动retry。
- provider current link truth继续经正式frame/device port读取；publication snapshot允许stale时必须显式标注，
  任何snapshot/notification/diagnostic都不能驱动queue或pump行为。
- shutdown admission由kernel attach owner拥有；`power`只拥有global order，driver只做owner-local IRQ/queue/
  device attempt，不反向访问stack/worker。
- cleanup只关闭新admission、请求现有bounded worker停止并抑制后续timer/repoll；不等待worker、不给callback加
  timeout、不建立通用drain/refcount/teardown framework。
- 未证明device/CPU不可再访问的provider、slot、DMA backing、stack与mapping必须保留到reset/power-off；不得
  为了得到漂亮Drop结果释放它们。
- LA64/virtio-pci、runtime hotplug/detach/restart、完整teardown、socket/control-plane与SMP shutdown progress
  仍非目标。

### 9.3 Checkpoint 1 — Multi-instance与attach conformance

**状态：** Closed（2026-07-27）。Host multi-instance、registry/attach conformance、独立review、validation与
write-back已闭合；Checkpoint 2仍Not Started / Unauthorized。

**Host real-stack matrix：** 在现有deterministic provider support上建立独立multi-instance test，不增加通用
test-support crate。两个`Stack`与两个bounded provider分别拥有MAC、link、credit、completion、manual time与
private mapping；矩阵必须证明：

- 一侧TX exhaustion、withheld completion或link-down时，另一侧仍可完成RX/TX与matching completion；
- 一侧resource/link recheck只推进自己的durable predicate，另一侧credit、output、time与pump observation不变；
- 撤销一侧transaction-local interface mapping后，该ID在原stack返回`UnknownInterface`且不复用，另一stack的
  mapping仍可推进；失败侧未发送frame、credit与ready RX仍归原owner；
- 两个stack-local `InterfaceId` namespace可以各自从0开始；测试不得把数值相等解释为同一可解引用identity。

**Registry/attach audit：** 扩展`device/net` local-registry KUnit，使用不同origin/facts证明boot-local
netdev ID、ifindex/name、publication snapshot与duplicate/failure结果互不污染；字段注释明确link只是在
publication point取得的stable snapshot、可以stale且不参与runtime pump决策。source audit逐条证明driver在
queue/RX refill/IRQ完成后才publish、registry record在provider移交后仍保留published identity、
`attach_published_netdevs()`按netdev独立继续、spawn失败只`remove_interface()`并保留unsafe provider。

本checkpoint不为难以注入的真实kthread spawn failure增加production factory/manager或generic fault-injection
API。host真实stack的mapping rollback/failure isolation与live kernel error control flow共同形成proof；若review
认为这不足以证明实际attach transaction，必须停止并申请一个KUnit-only、不会进入production capability的最小
failure seam，不能提前把closure写成PASS。

**Review/validation：** 运行host gate、base与现有`icmp-validation-probe`两种no-default feature check、
`just fmt kernel --check`、RV64 release build、dependency/public-surface/identity/mapping/failure-isolation audit、
`git diff --check`与`mdbook build docs`。为实际执行新增registry KUnit，再运行一次
`./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/net-frame-stage3-c1-rv64.log`；该轮只把
registry KUnit、单NIC active attach、既有Stage 2 traffic probe与正常关机作为回归证据，不冒充双实例hardware
proof。本checkpoint不修改power、worker lifecycle、current contract或Stage 2 validation seam。

**退出：** host矩阵、registry KUnit/source audit与独立review达到Apollyon 0/Keter 0/Euclid 0后Checkpoint 1
Closed并停止。任何需要global interface ID、shared lifecycle enum、driver->stack callback、generic device rollback
framework或shared API扩张的结果立即停止并返回RFC/manifest review。

实际执行新增独立`multi_instance` host矩阵，以两个真实`Stack`和两个bounded provider闭合exhaustion、
withheld completion、link、matching completion、recheck、manual time、mapping rollback与stack-local identity
namespace隔离；registry KUnit同步覆盖不同origin/facts、publication snapshot与duplicate failure isolation，
并明确link只是在publication point取得、允许stale且不驱动runtime pump。live source audit确认driver在queue/
RX refill、IRQ与notification准备后才publish，registry保留identity record，attach按netdev继续，spawn failure只
撤销stack-local mapping并保留unsafe provider。无需新增production或KUnit fault-injection seam。

Host gate、两种no-default feature check、RV64 release build、formatter changed-file audit与fresh-disk RV64
wrapper通过；wrapper记录261/261 KUnit、新registry case、单NIC active attach、128/128 reply、129/129 TX
completion、IRQ与mapping/outstanding回落，并正常`filesystem -> device -> PowerOff`。独立review为Apollyon 0、
Keter 0、Euclid 0、Safe 0；没有manifest、owner、shared API、ABI/visible semantics、acceptance或contract变化。
Checkpoint 1关闭后立即停止，Checkpoint 2未激活。

### 9.4 Checkpoint 2 — Shutdown handoff vertical slice

**状态：** Closed（2026-07-27）。Shutdown handoff、独立review、validation与write-back已闭合；Checkpoint 3
已由当前GOAL显式授权，但尚未激活。

**Attach-owner route：** 用`anemone-kernel::net`中一个owner-local lock保护active-path records与唯一
`shutdown_started` admission fact；不新增综合`NetdevLifecycle` enum。active publication与shutdown publication
都在该owner lock下线性化：

- 正常attach在mapping/worker/wake/time全部准备后、持owner lock确认shutdown尚未开始，再发布path并activate；
- shutdown先原子发布`shutdown_started`，snapshot每个path的窄`PumpControl` capability后立即释放owner lock；
- shutdown已开始后取得的published capability保持published/unattached，provider按既有unsafe-retention规则
  保留；已经prepare但尚未active的path只请求worker停止/retention，不再发布active；
- facade逐path关闭`active` admission、清除owner-local explicit work并对现有kthread发一次stop+wake request；
  不wait/join，不循环等待completion，不取得driver raw lock或stack object。

**Worker/timer/retention route：** worker在每个bounded pump round之间重新检查stop；观察到stop后不再request
repoll或arm新deadline。已排队的threaded timer没有cancel capability，只保留stateless wake；inactive/stopped
control不会重新activate或进入pump。worker返回前必须显式保留其`PumpCore`，并用关键注释说明：IRQ不可移除、
device尚无reset/quiesce proof，正常Drop会释放唯一provider/slot/DMA owner；只有未来runtime removal先阻止Weak
upgrade并证明queue/device quiesce后才能删除retention。cleanup先撤销admission，再做stop request，断言不能放在
撤销之前。

**System Power/driver handoff：** 把`power`的源码静态literal改为
`filesystem -> network -> device`，network step只调用上述唯一facade；emergency helper仍完全绕过ordinary plan。
VirtIO-Net driver shutdown继续只做owner-local interrupt suppression，不访问attach/stack/worker，也不声称已reset
queue或安全释放backing。同步校正`device::shutdown()`注释，明确unsupported quiesce下resource可以保留到terminal
reset/power-off。

**Observability与RV64 probe：** 每个path只打印一次shutdown摘要，至少给出name/ifindex、admission closed与
stop requested；不得逐IRQ/frame/completion/timer记录。保留Stage 2 KUnit-only ICMP burst与diagnostic mirror只为
本checkpoint运行一次fresh-disk RV64 wrapper，要求：active attach、128/128 reply、matching TX completion、RX/
IRQ evidence、outstanding/mapping baseline、bounded worker action，随后日志严格出现
`filesystem -> network -> device -> PowerOff`，network summary在VirtIO driver/device shutdown前。正常shutdown
不等于worker已join、queue已reset或resource已释放。

**Review/validation：** 运行host gate、两种no-default feature check、`just fmt kernel --check`、RV64 release
build与一次
`./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/net-frame-stage3-c2-rv64.log`。
wrapper会重建pretest rootfs并从只读master覆盖worktree-local runtime disk；只读master不得直接写挂载或传给
QEMU。source review覆盖admission/publication linearization、stop/pump/timer交错、terminal retention、Weak lifetime、
power literal order、emergency bypass与driver dependency direction。

**停止/退出：** 任何正常Drop provider/backing、shutdown wait/join/timeout、timer cancellation framework、
driver反向调用stack、network facade取得raw queue lock、active path在shutdown后重新activate、RV64 traffic或order
失败、panic/busy-spin都立即停止。通过后Checkpoint 2 Closed并停止；code已实现但六个network IDs与power Refine
仍Not Effective，Checkpoint 3未激活。

实际执行把active-path records与唯一`shutdown_started`放入同一attach-authority lock；attach与shutdown在该锁
下线性化，shutdown snapshot窄`PumpControl`后锁外逐path撤销admission、清除explicit work并只请求一次kthread
stop/wake。worker在bounded round之间检查stop，停止后不再repoll/arm deadline，并显式保留`PumpCore`到terminal
reset/power-off；排队timer仍只持stateless wake。Power静态plan改为
`filesystem -> network -> device`，emergency保持直接machine path，device注释不再声称unsupported quiesce会释放
network backing。

首轮RV64完整运行暴露shutdown摘要使用普通notice级别而未进入terminal evidence，checkpoint review将其作为阻塞
observability finding修正为terminal可见摘要后重跑。最终host gate、两种no-default feature check、RV64 release
build与fresh-disk wrapper通过；wrapper记录261/261 KUnit、128/128 reply、129/129 TX completion、IRQ 63、
outstanding/mapping回落至0/32、worker最多7轮，以及network摘要严格位于network step内和device step前。最终review
为Apollyon 0、Keter 0、Euclid 0、Safe 0；没有wait/join/timeout、timer framework、driver反向依赖、normal Drop、
manifest扩张或contract cutover。Checkpoint 2关闭后先形成独立commit，再激活Checkpoint 3。

### 9.5 Checkpoint 3 — Validation exit、final acceptance与`NFP-FINAL-CUTOVER`

**状态：** Closed（2026-07-27）；Checkpoint 2 closure commit `19b6e847`形成后激活，validation exit、
final exact-code evidence与原子cutover均已闭合。

**Temporary seam退出：** 删除只服务Stage 1/2 production probe的全部路径：

- kernel到stack的`icmp-validation-probe` feature forwarding、stack feature与`src/validation.rs`；
- kernel `net/validation.rs`、worker的probe request/event/action mirrors；
- VirtIO provider/device的Stage 2 diagnostic counters/snapshot、single-NIC query与crate-private re-export；
- stack interface中的validation probe field/method与相关conditional code。

删除只允许移除test-control、probe socket和diagnostic mirror，不得改变ordinary frame trait、stack pump、slot/
unsafe transition、recheck latch、worker budget/deadline或shutdown route。`host-test`、deterministic provider与host
real-stack tests继续保留，因为它们是长期crate conformance而非production control-plane。base smoltcp的
`socket-raw` compile anchor保持现状；acceptance audit必须确认non-host build没有构造或公开raw/ICMP endpoint。

**Final exact-code proof：** seam删除后重新运行host gate、base no-default check、formatter与RV64 release build；
再用fresh runtime disk运行
`./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/net-frame-stage3-final-rv64.log`。
final源码不再有packet-injection probe，因此该轮只要求全部remaining KUnit、netdev publication/active attach、
network shutdown summary、`filesystem -> network -> device -> PowerOff`与正常关机。Checkpoint 2紧邻源码的
真实traffic/completion/IRQ evidence与Checkpoint 3“只删除validation”的diff/source audit共同证明production
frame path没有被测试旁路替代；final boot不伪称再次覆盖packet traffic。

**Atomic contract cutover：** 所有source/host/RV64 evidence通过后，在同一checkpoint原子完成：

1. 新建`docs/src/contracts/net/index.md`；按稳定owner/proof surface新建`frame-path.md`、
   `netdev-lifecycle.md`与`attach-lifecycle.md`，登记六个Active network IDs及其当前实现/验证来源；
2. 原地Refine`SYSTEM-POWER-ORDERLY-001`为`filesystem -> network -> device`，记录本RFC与transaction为当前来源，
   保持episode、fail-forward、emergency与machine contract不变；
3. 更新contracts导航、RFC状态/修订记录、tracking、transaction、双周devlog与RFC/transaction索引；
4. 六个network IDs与power Refine要么全部Effective，要么全部保持Not Effective；不得分批cut over或留下
   Transitional contract。

若final evidence失败，或contract文本无法为任一state/handoff指出唯一owner、cleanup与验证，立即停止且不更新
任何current contract。若只发现target外accepted gap，先按普通review判断是否需要current limitations；R1
target内错误仍进open issue或Target Renegotiation，不能用limitation换取cutover。

**退出：** final review为Apollyon 0/Keter 0/Euclid 0，`NFP-PROOF-001`到`005`证据可逐项定位，所有临时
production validation seam已删除，host/build/RV64/docs floor通过，contract原子更新完成，RFC改为Closed、
transaction改为Completed。随后立即停止，不自动进入`net-udp`或`net-tcp`。

实际执行删除kernel/stack ICMP probe、worker request/event/action mirrors、VirtIO diagnostic counters/snapshot、
single-NIC query与feature forwarding；长期host conformance、ordinary frame/pump/slot/recheck/shutdown路径保持。
Host gate、base no-default check与RV64 release build通过；sandbox内lwext4 `SIGSYS`由沙箱外同一build通过确认
为环境噪声。final fresh-disk wrapper通过260/260 remaining KUnit、netdev publication/active attach、network
summary、严格`filesystem -> network -> device -> PowerOff`与正常退出；本轮不宣称packet traffic，production
traffic/completion/IRQ由紧邻的Checkpoint 2 evidence与只删除validation的diff/source audit证明未被旁路替代。
最终review为Apollyon 0、Keter 0、Euclid 0、Safe 0；六个network IDs与Power Refine已原子Effective。

### 9.6 Stage-wide review、审计与可观测性

Stage-wide review至少确认：

- registry、stack mapping、active publication与shutdown admission各有唯一owner；publication snapshot、wake edge、
  `InterfaceId` projection与diagnostic均不反向驱动状态机；
- 两个provider/stack实例的credit、completion、link、mapping、pump与failure互不交叉；attach failure不注销identity
  或影响另一path；
- shutdown owner lock不跨kthread/provider/driver callback；stop request不等待；worker在terminal exit retention前
  不Drop provider；driver disable与Weak upgrade不能释放backing；
- hard IRQ仍只ack/publish/wake，frame callback不持device-wide lock，slot/unsafe begin-complete proof与Stage 2
  一致；
- power static plan字面顺序唯一，ordinary failure继续fail-forward，emergency仍无network/filesystem/device
  callback；
- final non-host/nonKUnit source没有ICMP/raw socket construction、single-NIC assertion、packet injection、driver
  stats query或diagnostics-driven behavior；
- endpoint/socket/fd/wait/control-plane、hotplug、generic device lifecycle、LA64/PCI与SMP shutdown guarantee没有
  混入shared/public surface。

Stage 3只允许publication/attach/shutdown的一次摘要与既有owner断言；不增加per-frame/IRQ/completion日志、统计
registry或shutdown progress counter。Checkpoint 2的probe summary在Checkpoint 3删除。

### 9.7 验证floor与结论边界

1. Crate host gate：`cargo test -p anemone-net-api -p anemone-smoltcp-stack`。Checkpoint 1/2还运行base与
   `icmp-validation-probe`两种`--no-default-features` check；Checkpoint 3 feature删除后只运行base check。
   Cargo命令是已接受的crate-owned host gate，不新增Just/xtask/script wrapper。
2. `just fmt kernel --check`；任何新增formatter diff阻塞。Stage 2已记录的三个vendored smoltcp baseline若仍存在，
   只能原样记录，不能借Stage 3扩散。
3. 每个checkpoint至少运行
   `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`；若sandbox再次触发相同lwext4
   `SIGSYS / Bad system call`，只有同一repository命令在允许环境通过后才可分类为环境噪声。
4. Checkpoint 1、2与3分别运行9.3/9.4/9.5给出的RV64 wrapper与独立日志。wrapper固定`smp=1`、`memory=1G`、
   tracked pretest rootfs和调用者显式只读master；三轮都必须正常关机，且只按各自probe scope下结论。
5. dependency/public-surface、multi-instance/failure、publication/attach、slot/unsafe/Weak、IRQ/wake/timer/worker、
   shutdown retention、power/emergency、validation-bypass与resolved-manifest audit；`git diff --check`与
   `mdbook build docs`。

本stage不运行或证明`smp>1`、LA64 build/runtime、virtio-pci、hardware、final harness、完整LTP、runtime
hotplug/detach/restart、完整teardown或socket/control-plane。wrapper附带的当前`sys` profile只作环境回归，不能
扩大network closure。

### 9.8 Contract、停止与退出条件

Checkpoint 1/2 contract cutover均为None；Checkpoint 3只有在全部stage evidence通过后才执行单一
`NFP-FINAL-CUTOVER`。以下任一情况停止并上报manifest expansion、design finding或Target Renegotiation：

- 需要改变R1 target、owner、shared API、ABI/visible semantics、platform scope或acceptance；
- 需要修改vendored smoltcp/virtio dependency、generic IRQ/task/kthread/timer/scheduler/device lifecycle、apps/
  rootfs/LTP、QEMU Platform/wrapper或KernelConfig；
- attach/shutdown需要第二份lifecycle truth、global ID、driver->stack dependency、wait/join/timeout或generic
  teardown framework；
- host双实例只能靠fake/fake或shared test-control通过，任一instance失败会污染另一instance；
- shutdown后仍可进入pump/arm新timer，或worker stop会Drop未quiesce provider/backing；
- Checkpoint 2 traffic/order或Checkpoint 3 final exact-code boot失败；
- validation seam删除触碰ordinary behavior，或final source仍保留production packet injection/single-NIC truth；
- review仍有Apollyon/Keter/Euclid，或六个network IDs与power Refine不能原子cut over。

Stage 3只有三个checkpoint都Closed、临时seam退出、final proof与contract write-back完成后才Closed；不得把
Checkpoint 2的有流量PASS、Checkpoint 3无流量boot、docs build或source plausibility中的任一项单独当成closure。

### 9.9 Resolved Write Set Manifest

允许production/source写入：

- `anemone-kernel/src/device/net/registry.rs`；
- `anemone-kernel/src/net/{mod.rs,worker.rs}`，并在Checkpoint 3删除`net/validation.rs`；
- `anemone-kernel/src/driver/net/mod.rs`与
  `anemone-kernel/src/driver/net/virtio/{device.rs,frame.rs,mod.rs}`，仅用于Checkpoint 3删除Stage 2 diagnostic/
  single-NIC seam与同步terminal-retention注释；
- `anemone-kernel/src/{power.rs,device/mod.rs}`，仅用于network orderly step与honest shutdown注释；
- `anemone-kernel/Cargo.toml`，仅用于Checkpoint 3删除`icmp-validation-probe` feature forwarding；
- `anemone-kernel/crates/anemone-smoltcp-stack/{Cargo.toml,src/lib.rs,src/stack.rs}`，并在Checkpoint 3删除
  `src/validation.rs`；ordinary adapter/pump semantics保持只读。

允许test/validation写入：

- `anemone-kernel/crates/anemone-smoltcp-stack/tests/{frame_path.rs,support/mod.rs}`；
- 新建`anemone-kernel/crates/anemone-smoltcp-stack/tests/multi_instance.rs`；
- 上述owner文件中的local KUnit、Checkpoint 2临时probe使用与Checkpoint 3删除；
- repository owner生成的`build/**`、pretest rootfs与wrapper worktree-local runtime disk copy。

允许current contract/canonical docs写入：

- 新建`docs/src/contracts/net/{index.md,frame-path.md,netdev-lifecycle.md,attach-lifecycle.md}`；
- `docs/src/contracts/power/shutdown-lifecycle.md`、`docs/src/contracts.md`与`docs/src/SUMMARY.md`；
- `docs/src/rfcs/net-frame-path/{index.md,invariants.md,implementation.md,tracking-issues.md}`；
- `docs/src/devlog/transactions/2026-07-26-net-frame-path.md`、`docs/src/devlog/transactions/index.md`、
  `docs/src/devlog/2026-07-20_to_2026-08-02.md`与`docs/src/rfcs.md`。

明确只读：`anemone-net-api`、smoltcp adapter/pump与vendored smoltcp、`virtio-drivers`/`Cargo.lock`、
KernelConfig/xtask/Justfile、generic bus/IRQ/task/kthread/timer/scheduler/device/driver APIs、其它power contract IDs、
Platform/QEMU/wrappers、apps/rootfs/LTP、register/current limitations与其它RFC/current contracts。若实际实现需要
触碰只读边界，必须先停止，说明owner/contract/validation影响并申请逐文件扩展；不得先改后追认。

## 10. Stage 4 Ready：Post-close contract conformance correction

**状态：** Ready / Unauthorized / 单一checkpoint。当前只完成docs resolution；没有Stage 4 transaction、源码
修改或执行授权。激活时必须新建独立transaction，并且只激活本stage整体，不拆出额外probe/checkpoint。

### 10.1 反馈定性与交付边界

Stage 3 post-close review确认[NFP-008](./tracking-issues.md#nfp-008--network-activation依赖同级late-initcall的偶然顺序)、
[NFP-009](./tracking-issues.md#nfp-009--published-capability的pending-handoff绕过devicenet-owner)与
[NFP-010](./tracking-issues.md#nfp-010--长期host-conformance-target缺少required-features)。它们分别是boot
ordering、published capability owner与test target metadata的实现偏差；没有新能力、target reduction、ABI、
public API、platform scope、acceptance boundary或contract delta。

本stage在一个checkpoint内完成以下aggregate result：

1. `device/net` registry同时拥有publication record与异构pending handoff；concrete driver完成owner-local准备后
   只调用通用publish，不保存或导出`PublishedNetdev<VirtIONetProvider>`，kernel net不再发现或drain concrete
   driver state。
2. dynamic dispatch只位于registry向attach authority移交one-shot pending capability的边界。内部接口至少提供
   immutable publication snapshot与一次性consume/attach操作；其concrete implementation立即回到
   `worker::prepare<P>`等价的generic路径。`PumpCore<P>`、worker entry、frame token、queue/resource truth与data
   plane继续保留concrete `P`，不得引入`dyn FrameProvider`。
3. attach逐项drain；失败先撤销transaction-local stack mapping，再把同一个未attach capability交还
   registry-owned pending retention。本次drain不得立即重试，第一版仍没有自动retry、runtime unpublish或
   lifecycle framework。publication snapshot与capability由同一registry owner保持可达，不能只留下snapshot并
   `forget` provider。
4. network activation移除`#[initcall(late)]`，由BSP boot coordinator在
   `run_initcalls(InitCallLevel::Late)`完整返回后、用户态init exec前显式调用。threaded timer继续作为普通
   `Late` provider；不修改initcall macro、level、linker section或timer readiness API。
5. `frame_path`、`bounded_progress`与`multi_instance`三个长期host integration target都用显式`[[test]]`和
   `required-features = ["host-test"]`描述。后两项不是Stage 3遗留probe，不能删除；已经退出的
   `icmp-validation-probe`不恢复。

内部trait、erased wrapper与attach result的确切名称不构成新contract。实现preflight必须在下述冻结文件内选择
最窄形状，并证明registry只保存/移交opaque capability、不理解provider或smoltcp policy；这是同一checkpoint的
普通source设计，不建立独立probe gate。一次性dynamic call的性能不作为阻塞因素。

### 10.2 Activation preflight 与实施顺序

Stage 4激活前，新transaction必须记录branch/HEAD/dirty state、原Completed transaction、三个open finding、
current `NETDEV-LIFE-001` / `NET-ATTACH-001`、threaded timer的`Late`无相对顺序规则、Cargo target graph与
本stage resolved manifest。发现用户dirty change与source write set重叠时先确认归属；不得覆写或续写
2026-07-26 Completed transaction。

单checkpoint内按以下顺序实施并作为一个aggregate接受：

1. 先收拢`device/net` pending storage与one-shot erased attach route，删除VirtIO driver-owned pending slot、
   driver-specific drain和相应visibility；用local KUnit/source audit先确认异构publication、one-shot drain、
   failure reinsertion与record isolation。
2. 再把network attach改为boot coordinator的post-`Late`显式调用；保持attach内部逐netdev failure isolation、
   active publication与shutdown admission逻辑不变。
3. 最后补齐Cargo test metadata，运行本stage全部host/source/build/runtime验证与独立review；统一写回RFC、
   tracking、register和新transaction后才允许关闭。

以上顺序只约束同一checkpoint内的reviewability，不形成三个子checkpoint，不要求中间commit，也不产生中间
contract状态。任何步骤失败都使整个Stage 4保持Active/Not Closed。

### 10.3 Owner、failure 与 dependency audit

Stage-wide source review至少逐项确认：

- registry在同一个publication transaction中提交stable record与pending capability；duplicate/identity/name
  failure不留下任一半，snapshot仍明确允许link stale且不驱动runtime；
- pending collection可以同时保存至少两个不同concrete provider type；erasure不要求公共associated token、
  不把provider-specific branch、downcast、smoltcp object或driver discovery放入registry；
- drain只消费每个capability一次；attach success把concrete provider移入唯一worker core，attach failure撤销
  mapping并将capability重新交给registry，当前drain不循环重取；一个entry失败不阻塞或回滚其它entry；
- `anemone-kernel::net`只依赖`device/net`窄drain/attach input，不import concrete VirtIO provider或driver facade；
  VirtIO driver不依赖stack/worker并且drv state只保留owner-local shutdown capability；
- `run_initcalls(Late)`返回是network activation的源码可见前置；timer、inode shrinker、OOM等Late consumer之间
  仍无相对顺序假设，boot coordinator没有按provider名称重放通用initcall policy；
- active publication仍发生在mapping、worker、wake与time wiring全部成功后；shutdown_started、terminal
  retention、network-before-device与emergency bypass均保持Stage 3行为；
- host tests在default feature下实际运行，在no-default feature下被Cargo按metadata排除；test source、
  `host-test` helper与production feature graph没有被删减或旁路。

KUnit可以使用两个最小dummy concrete provider证明heterogeneous pending container与failure retention，但test
control不得进入production trait或shared API。observability沿用现有publication/active/shutdown摘要；本stage不
增加per-frame、IRQ、completion、dynamic-dispatch或initcall日志，也不增加长期diagnostic field。

### 10.4 验证 floor 与结论边界

Stage 4必须在同一最终源码上至少完成：

1. `cargo test -p anemone-net-api -p anemone-smoltcp-stack`，确认三个长期integration target实际执行；
2. `cargo test -p anemone-smoltcp-stack --no-default-features --no-run`，确认host-only target由Cargo metadata
   排除且production feature set可编译；另运行`cargo check -p anemone-smoltcp-stack --no-default-features`；
3. registry/owner/erasure/failure、dependency/public-surface、boot/post-`Late` ordering、attach/shutdown与
   feature/test-target source audit；
4. `just fmt kernel --check`与所有changed Rust/TOML file的focused formatter检查；任何新增formatter diff阻塞；
5. `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`；
6. 一次fresh-disk
   `./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/net-frame-stage4-rv64.log`，要求全部
   KUnit、netdev publication/active attach、network shutdown summary、严格
   `filesystem -> network -> device -> PowerOff`与正常退出；本stage没有packet injection probe，因此不把该轮
   boot写成新的traffic/completion证明，Stage 2/3历史证据保持原结论；
7. 独立software-engineering review达到Apollyon 0/Keter 0/Euclid 0；`git diff --check`与`mdbook build docs`。

RV64 wrapper仍只证明smp=1 virtio-mmio product path。LA64、virtio-pci、hardware、smp>1、final harness、完整
LTP、runtime hotplug/retry/restart、完整teardown与socket/control-plane均Not Run或非目标，不能从本次修正外推。

### 10.5 Contract、停止与退出条件

Stage 4 contract cutover为None。六个Network ID与`SYSTEM-POWER-ORDERLY-001`继续以现有current contract为
Effective authority；本stage只证明implementation重新conform，不更新其规则、状态、来源或最后核验，不创建
Transitional contract或pending successor。

以下任一情况立即停止整个Stage 4并回到RFC/owner review，不得增加兼容bridge后继续：

- heterogeneous handoff要求`dyn FrameProvider`、公开token type、provider-specific registry policy、downcast、
  driver-to-stack dependency、第二份publication/lifecycle truth或`anemone-net-api`变化；
- attach failure无法把capability交还registry owner，或只能通过自动retry、runtime lifecycle framework、
  generic device rollback、unsafe Drop已注册IRQ/DMA backing来处理；
- boot顺序需要新增initcall level/priority、linker排序、timer readiness/cancellation API，或修改其它Late consumer；
- test修正删除/弱化`bounded_progress`或`multi_instance`，恢复旧ICMP probe，或只用production build替代
  no-default test compile gate；
- 需要修改current contract、ABI/visible semantics、accepted target/platform scope/acceptance boundary，或超出
  resolved manifest；
- host/no-default/build/RV64任一mandatory gate失败，source audit仍见concrete driver drain/偶然Late顺序/
  capability loss，或review仍有Apollyon/Keter/Euclid。

退出要求单checkpoint aggregate diff、review与全部验证通过；新transaction记录activation、实现、证据、Not
Run与closure；NFP-008/009/010和register umbrella issue同步关闭；RFC状态回到Closed。随后立即停止，不自动进入
`net-udp`、`net-tcp`或任何runtime lifecycle工作。

### 10.6 Resolved Write Set Manifest

允许production/source写入：

- `anemone-kernel/src/device/net/{mod.rs,registry.rs}`；
- `anemone-kernel/src/net/{mod.rs,worker.rs}`；
- `anemone-kernel/src/driver/net/mod.rs`与`anemone-kernel/src/driver/net/virtio/mod.rs`，仅用于删除driver-owned
  pending storage/drain、同步publication handoff和收窄visibility；
- `anemone-kernel/src/main.rs`，仅用于post-`Late` network activation；
- `anemone-kernel/crates/anemone-smoltcp-stack/Cargo.toml`，仅用于长期host test target metadata。

允许local test与验证产物写入：

- `anemone-kernel/src/device/net/registry.rs`中的local KUnit；
- repository owner生成的`build/**`、pretest rootfs与wrapper worktree-local runtime disk copy。

允许canonical docs与新执行记录写入：

- `docs/src/rfcs/net-frame-path/{index.md,invariants.md,implementation.md,tracking-issues.md}`；
- `docs/src/register/open-issues.md`与`docs/src/rfcs.md`；
- 激活时新建`docs/src/devlog/transactions/2026-07-27-net-frame-path-stage4.md`，并同步transaction index、
  `docs/src/SUMMARY.md`与当前双周devlog。原`2026-07-26-net-frame-path.md`明确只读。

明确只读：`anemone-net-api`、smoltcp stack source与`tests/{frame_path.rs,bounded_progress.rs,multi_instance.rs,
support/**}`、vendored smoltcp、`virtio-drivers`/`Cargo.lock`、`device/net/provider.rs`、VirtIO frame/device data
plane、timer/initcall macro/linker实现、其它Late consumer、generic bus/device/IRQ/task/kthread/scheduler/power、
KernelConfig/xtask/Justfile/Platform/QEMU wrapper、apps/rootfs/LTP、current contracts/current limitations与其它RFC。
若正确实现需要触碰任一只读边界，必须先停止并上报逐文件扩展、owner/contract影响与验证变化；不得先改后追认。

## 11. 全局反馈分流

| 反馈影响 | 处理位置 | 当前 gate 行为 |
| --- | --- | --- |
| 内部类型、owner-local模块、test fixture、budget 调整 | transaction；必要时更新本文 | 保持 target 时可继续 |
| future Outline/Ready 顺序、write set、validation 或停止条件 | 本文 + transaction | 先更新权威计划再继续 |
| target invariant、owner、shared semantic surface、可见行为或 acceptance boundary | `index.md` / `invariants.md` / `tracking-issues.md` | 停止并进入 RFC review |
| effective contract | current contract + transaction cutover evidence | 仅 `NFP-FINAL-CUTOVER` 更新 |
| 已接受缺口 / in-target defect | current limitations / open issues | 不用兼容桥或缩小测试隐藏 |

工程证据可以触发 Target Renegotiation Gate，但实现者不能自行批准 reduced target。提案必须说明原目标
影响、correctness invariant、已完成代码处置、候选新语义、验证和剩余 gap 路由；接受前保持 Not Cut
Over。

## 12. 全局完成定义

本 RFC 只有在 Stage 1 到 Stage 3 的历史closure保持，Stage 4重新Closed、`NFP-PROOF-001` 到
`NFP-PROOF-005` 都有可审计证据、
host 与 RV64 production proof 互补闭合、所有临时 probe/旁路按边界收口，并在一个原子 gate 更新六个
network contract 与 `SYSTEM-POWER-ORDERLY-001` Refine，且post-close owner/order/test conformance defect全部
关闭后才完成。Stage 4不重复cutover，也不重写Stage 1-3证据。

任何 Stage 的 host PASS、RV64 单次 packet smoke、build success、container shape 或 source plausibility
都不能单独形成 closure。未进入 target 的 LA64/virtio-pci、socket/control-plane、hotplug 与完整 teardown
不得因实现顺手存在而被记录为本 RFC 能力。
