# Network Frame Path 迁移实施计划

**状态：** R1 / Stage 1 Closed；Stage 1 -> 2 Boundary Interlude Closed；Stage 2 Ready / Checkpoint 1 Not Started / Unauthorized
**最后更新：** 2026-07-27
**父 RFC：** [RFC-20260726-net-frame-path](./index.md)
**目标与不变量：** [Network Frame Path 目标与不变量](./invariants.md)
**当前契约：**
[`SYSTEM-POWER-ORDERLY-001`](../../contracts/power/shutdown-lifecycle.md#system-power-orderly-001)；
六个 proposed network IDs 尚未生效
**当前修订：** `R1`
**事务日志：** [2026-07-26 net-frame-path](../../devlog/transactions/2026-07-26-net-frame-path.md)
**平台验收范围：** RV64 QEMU virtio-mmio；LA64 / virtio-pci 不属于本修订的 build、runtime 或
cutover 要求

> 本文是 R1 的实施顺序、stage maturity、验证与 write-set 权威。用户已于 2026-07-26 接受 R0、
> 授权建立 transaction，并独立激活 Stage 1 Checkpoint 1。用户随后分别独立授权并关闭 Checkpoint 2
> 与 Checkpoint 3，并独立授权、关闭 Checkpoint 4 与 Stage 1。Stage 1 关闭后的 module-boundary review
> 又发现 concrete driver、通用 worker 与 validation vocabulary 之间的局部耦合；用户已授权在 Stage 1
> 与 Stage 2 之间完成本文定义的 Boundary Interlude。本授权不授予 contract cutover，也不自动解析或
> 进入 Stage 2。2026-07-27 的首条 RV64 saturation route 命中 failure signal后，用户接受 R1
> proof-boundary correction并授权本次docs-only route resolution；Checkpoint 1重新达到Ready，但未获实现授权。

## 1. 计划角色与 authority

四层架构是长期 owner、object fence 与依赖方向，不是四个依次完成的施工阶段。实施单位必须是可执行的
跨层结果；“API crate 完成”“smoltcp stack 完成”或“driver 完成”都不能单独构成 stage closure。

本计划采用一个纵向 walking skeleton 和两个后续加固阶段：

1. Stage 1 在同一个 stage 内建立 hostable semantic seam、真实 smoltcp consumer、真实 VirtIO frame
   provider、`device/net` publication 与 kernel attach/IRQ/worker wiring；
2. Stage 2 对 ownership、completion、exhaustion、recheck、budget、deadline 与公平性做确定性硬化；
3. Stage 3 完成多实例隔离、attach/lifecycle、System Power handoff、RV64 production acceptance 与
   `NFP-FINAL-CUTOVER`。

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
| Stage 2 — Bounded progress conformance | R1 Ready / Checkpoint 1 Not Started / Unauthorized | host deterministic exhaustion/completion/recheck、budget/deadline、公平性、link recovery 与 RV64 bounded production-path proof | 全部 Not Effective |
| Stage 3 — Multi-instance/lifecycle closure | Outline | 双实例隔离、attach rollback、shutdown handoff、RV64 final acceptance 与原子 cutover | `NFP-FINAL-CUTOVER` 后 Effective |

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

**状态：** R1 Ready / Checkpoint 1 Not Started / Unauthorized。2026-07-27 的 R0 RV64 burst route因未观察到
TX exhaustion而按failure signal停止并删除probe；该历史不重写。用户随后接受R1 proof-boundary correction并
授权本次docs-only resolution，不授权Checkpoint 1实现、runtime validation或Checkpoint 2/3。

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

**状态：** Ready / Not Started / Unauthorized。

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

**状态：** Not Activated / Unauthorized。Checkpoint 1未关闭，不得进入本checkpoint。

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

### 8.5 Checkpoint 3 — provider/recheck/worker closure与RV64 acceptance

**状态：** Not Activated / Unauthorized。Checkpoint 2关闭后必须独立复核其diff与Checkpoint 1 RV64 evidence；
本节为R1预解析结果，不授予执行权限。

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
- `anemone-kernel/src/driver/net/virtio/{device.rs,frame.rs,mod.rs}`；
- `anemone-kernel/src/net/{worker.rs,validation.rs}`；
- `conf/.defconfig`只允许Checkpoint 3依据证据调整已经存在的三个pump/repoll数值；不得增加schema字段。

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

## 9. Stage 3 Outline：Multi-instance、lifecycle 与最终 cutover

**目的：** 完成 `NETDEV-LIFE-001` 与 `NET-ATTACH-001`，把 host 多实例、publication/attach rollback、
link lifecycle、network-local shutdown cleanup、RV64 final production proof 和 current-contract cutover
闭合为一个原子结果。

**前置依赖：** Stage 2 Closed；ownership/progress/pump proof 已稳定；System Power current contract 与
live static plan 重新核验。

**受保护边界：**

- 多设备是 identity、credit、completion、mapping、link 与 failure isolation，不是容器能存两个元素；
- published/unattached 合法，attach failure 不注销 netdev、不回滚其它 active path；
- link-down 不重建 identity 或 mapping；
- network facade 只做 owner-local admission closure 与有限 best-effort cleanup；
- `power` 唯一拥有 global order，driver 只做 owner-local IRQ/queue/DMA/device quiesce；
- 无法证明不再访问的 resource 保留到 reset/power-off，不提前释放；
- LA64/virtio-pci、runtime hotplug、完整 teardown、socket/control-plane 仍非目标。

**预计证明方向：** host 双 provider/interface failure isolation、attach rollback、link recovery、shutdown
admission；RV64 active attach、双向收发、queue recovery、network-before-device callback 与不安全资源不
回收；最终 dependency/unsafe/旁路审计和文档 cutover。

**Resolution trigger：** Stage 2 关闭后根据 live owner graph、worker/token lifetime、System Power plan
与剩余 review findings 完整解析 Stage 3。只有 host、RV64、source audit 与 contract write-back 全部达到
floor，才执行 `NFP-FINAL-CUTOVER`；不得按 contract ID 或 checkpoint 部分 cut over。

## 10. 全局反馈分流

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

## 11. 全局完成定义

本 RFC 只有在 Stage 1 到 Stage 3 依次 Closed、`NFP-PROOF-001` 到 `NFP-PROOF-004` 都有可审计证据、
host 与 RV64 production proof 互补闭合、所有临时 probe/旁路按边界收口，并在一个原子 gate 更新六个
network contract 与 `SYSTEM-POWER-ORDERLY-001` Refine 后才完成。

任何 Stage 的 host PASS、RV64 单次 packet smoke、build success、container shape 或 source plausibility
都不能单独形成 closure。未进入 target 的 LA64/virtio-pci、socket/control-plane、hotplug 与完整 teardown
不得因实现顺手存在而被记录为本 RFC 能力。
