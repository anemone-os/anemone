# 2026-07-26 - Network Frame Path

**Status:** Active / Stage 1 Checkpoint 1-3 Closed; Checkpoint 4 Not Activated
**Date:** 2026-07-26
**Owner:** doruche, Codex
**Canonical Plan:** [RFC-20260726-net-frame-path R0](../../rfcs/net-frame-path/index.md),
[目标与不变量](../../rfcs/net-frame-path/invariants.md),
[Stage 1 Ready definition](../../rfcs/net-frame-path/implementation.md#6-stage-1-readyfour-layer-walking-skeleton)
**Canonical Revision:** R0
**Contract Impact:** `NET-BOUNDARY-001`、`NETDEV-LIFE-001`、`NET-FRAME-OWN-001`、
`NET-FRAME-PROGRESS-001`、`NET-STACK-PUMP-001`、`NET-ATTACH-001` proposed Introduce；
`SYSTEM-POWER-ORDERLY-001` proposed Refine；全部只在 `NFP-FINAL-CUTOVER` 原子生效

## Scope and authorization

用户于 2026-07-26 确认公共 review 已完成，接受当前 Draft 为 `R0 / Accepted for Implementation`，
授权建立本 transaction，并独立激活 Stage 1 Checkpoint 1。本授权只覆盖 hostable seam 与 frame-token
representation probe；不自动进入 Checkpoint 2，不允许越出 Stage 1 frozen manifest、修改默认只读
owner、改变 target/owner/public API/shared contract/ABI/visible semantics/acceptance，或提前 cutover。

用户随后在同日独立授权 Stage 1 Checkpoint 2，要求完成 real smoltcp owner 与 host vertical slice，按
checkpoint 执行 review、validation、write-back 并单独提交。该授权不进入 Checkpoint 3，其余 frozen
manifest、默认只读 owner、target 与 cutover 边界保持不变。

用户随后再次独立授权完成 Stage 1 Checkpoint 3，并明确不得进入 Checkpoint 4；本次仍按 frozen manifest、
review/validation/write-back 与单 checkpoint commit 合同执行，不授予 target/contract/cutover 扩展。

## R0 acceptance and activation preflight

Preflight 在 `dev/drc/alpha`、promotion commit `c827e680` 的干净工作树上读取 AGENTS/LOCAL、R0 正文、
implementation、tracking issues、register、System Power current contract 与 live owners。启动时不存在当前
transaction；本页是 R0 独立执行记录。

Live source 仍满足 Stage 1 Ready baseline：根 workspace 只自动纳入 `anemos/*`，因此两个新 first-party
crate 需要显式 member；仓库内 smoltcp 为 0.13.1，其 `Device` 使用 associated RX/TX token、paired receive
和 callback-scoped consume；`VirtIODevice::take_transport()`、`VirtIOHalImpl`、VirtIO bus/IRQ、kthread、
threaded timer、System Power 静态 `filesystem -> device` plan 与 RV64 QEMU virtio-net args 均存在且保持
只读。`just --list`、build/qemu/fmt help 与 RV64 wrapper 仍匹配 canonical commands。

启动时工作树无 dirty change，故 `conf/.defconfig` 没有需要归属判定的重叠。现有 owner surface 足以在
冻结 manifest 内表达 associated token probe，无需 type erasure、unsafe、公开 backing、全局锁、generic
bus/virtio dependency/power contract 改动，也未命中 Stage 1 停止条件。

## Execution log

### 2026-07-26 - Stage 1 Checkpoint 1 activated

**Write-set lock:** production、validation-only 与 docs write set 以 canonical implementation 为唯一
authority。本 checkpoint 只使用其中的 workspace manifests、两个新 crate、Cargo.lock 与 canonical docs；
不以本事务复制或扩大 manifest。

**Planned proof:** host validation gate；compile-time dependency/feature audit；RX consume/recycle、TX
fill/submit、unconsumed RX/TX Drop、capacity rejection 与 callback borrow non-escape；
`just fmt kernel --check`、`git diff --check`、mdBook，以及 checkpoint-scoped architecture review。

**Activation-time cutover:** Not Cut Over。六个 network IDs 与 System Power Refine 均保持 pending；
Checkpoint 2 未激活。

### 2026-07-26 - Checkpoint 1 implementation and review

根 workspace 显式加入 `anemone-net-api` 与 `anemone-smoltcp-stack`；kernel 以
`default-features = false` 消费 stack，并只由 `kunit` 转发 `kunit-probe`。`anemone-net-api` 默认
`no_std`，定义 opaque `InterfaceId`、Ethernet/link/interface facts、monotonic time/duration、frame
capacity/acquisition outcome、pump/recheck values，以及 GAT associated RX/TX token。stack 的 production
feature set 是 `no_std + alloc`，默认 `host-test` 只为 package test 启用 std 与 deterministic host 路径。

smoltcp 0.13.1 的 `medium-ethernet` 会启用内部 socket enum，并拒绝零 concrete variant；base feature 因此
用 private `socket-raw` 作为最窄 compile anchor。stack 不公开或构造 raw endpoint，ICMP endpoint 仍只由
`kunit-probe` 打开；当 smoltcp 支持 socketless Ethernet interface，或后续 accepted production socket
feature 取代它时删除该 anchor。这是 feature-graph bridge，不改变 endpoint/control-plane 非目标。

deterministic provider 只存在于 stack integration test。RX/TX lane 各自唯一拥有 backing、credit、slot
state 与 completion；associated token 只借用对应 owner lane。RX consume 后 recycle，TX consume 后
submit、在 owner completion 前保持不可再取得；未 consume 的 RX/TX Drop 分别恢复 Ready frame 与
Available credit。超长 TX 在 callback 前返回 typed capacity error并恢复 credit。manual clock、link
predicate、frame injection、completion 与 counters 都留在 test owner，不进入 production capability。

Review 初版发现 `FrameProvider::interface_facts()` 会让 driver/provider 看起来拥有由 `device/net`
规范化的 publication snapshot。该 Keter 方向在提交前修正：shared API 仍定义 `InterfaceFacts` value，
provider trait 只暴露自身 frame capacity 与当前 link observation；后续 MAC/MTU/link normalization 与
publication 继续由 `device/net` 唯一拥有。修正后无 Apollyon/Keter/Euclid；无 unsafe、type erasure、
trait object、shared `Arc<Mutex<_>>` backing、provider-global callback guard、descriptor/DMA identity、
smoltcp object、endpoint/socket/readiness/control-plane production API。

### 2026-07-26 - Checkpoint 1 validation and closure

- `cargo test -p anemone-net-api -p anemone-smoltcp-stack`：通过。6 个 integration tests 覆盖 RX
  consume/recycle、TX fill/submit/completion gate、unconsumed RX/TX Drop、capacity rejection、link/manual
  time owner；2 个 compile-fail doctests证明 RX/TX callback borrow不能逃逸。
- `cargo check -p anemone-net-api --no-default-features` 与
  `cargo check -p anemone-smoltcp-stack --no-default-features`：通过，证明 production no_std feature set；
  `cargo check -p anemone-smoltcp-stack --no-default-features --features kunit-probe` 也通过。
- `cargo test -p anemone-smoltcp-stack --no-default-features`：通过，并按 test target 的
  `required-features = ["host-test"]` 跳过 host-only integration fixture，证明它不进入 production
  feature set。
- dependency audit：`cargo tree -p anemone-net-api --edges normal` 只有 API crate 自身；production stack
  只有 `anemone-net-api + smoltcp`，没有 kernel dependency。source audit 未发现 smoltcp/kernel object、
  host time/network object、test control 或 unsafe 泄漏到 `anemone-net-api`。
- `just fmt kernel --check`：已运行，exit 1；formatter diff 全部位于 frozen read-only 的既有
  `anemone-kernel/crates/anemos/smoltcp/**` 与 generated/baseline 文件，新建两个 crate无 formatter diff。
  未以格式化为由扩大 write set。
- `git diff --check`：通过。
- `mdbook build docs`：通过；仅有 search index size warning。

Checkpoint 1 delivery、representation probe、review 与 validation floor 已闭合；没有命中 Stage 1
停止条件。共享 surface 仍按 canonical plan 保持 provisional，不冻结为 current contract。六个 network
IDs 与 System Power Refine 继续 Not Effective；Stage 1 保持 Active，但 Checkpoint 2 未激活，本事务停在
Checkpoint 1 closure。

### 2026-07-26 - Stage 1 Checkpoint 2 activated

Checkpoint 2 从 commit `f804b370` 的干净 `dev/drc/alpha` 工作树独立激活。Preflight 重新读取 R0、Stage 1
Ready 定义、tracking issues、register、current System Power contract、Checkpoint 1 transaction 与 live
smoltcp/frame-provider source；`poll_ingress_single()`、bounded `poll_egress()`、`poll_at()`、associated
paired token 和 production `default-features = false` dependency 均仍符合 Ready baseline。

**Write-set lock:** 只写 frozen manifest 内的 stack crate manifest、`src/{lib.rs,adapter.rs,pump.rs}`、
`tests/frame_path.rs`、`Cargo.lock` 与 canonical docs。`anemone-net-api`、vendored smoltcp、kernel/device/
driver/config/power owner 全部保持只读。

**Activation-time cutover:** Not Cut Over。六个 network IDs 与 System Power Refine 全部保持 pending；
Checkpoint 3 未激活。

### 2026-07-26 - Checkpoint 2 implementation and review

`anemone-smoltcp-stack` 新增 crate-private `FrameDevice` adapter，将正式 GAT frame token 适配为 smoltcp
`Device`/RX/TX token；smoltcp 的 token、interface、socket 与 hardware-address 类型均不出 crate。adapter
只在 smoltcp 已按 provider capacity 建立 interface 后把不可失败的 TX consume 转交给正式 token；capacity
漂移由每次 pump 的普通 `assert!` 暴露。

`Stack` 唯一拥有 smoltcp `Interface`、private `SocketSet`、实例内 `InterfaceId` namespace/mapping 与
namespace cursor。所有改变协议状态的入口都要求 `&mut Stack`，该独占借用是本层唯一 pump capability；
future kernel attach/worker owner 才决定是否在外层放置锁、如何竞争、try-lock 后如何 requeue/yield，以及
如何同 IRQ/timer 协作。已取得 pump capability 的调用路径只通过一次性 token 借用 provider，不持
provider-global guard。frame capacity 是 smoltcp 所需的 attach-time stable snapshot，provider 仍是真相源，
pump 每次校验 snapshot 未 stale。

一次 pump 先执行单次 bounded maintenance，再分别按非零 ingress/egress budget 调用
`poll_ingress_single()` 与 `poll_egress()`，最后由真实 `poll_at()` 产生 deadline。budget 未确认队列已空、
deadline 已到或 provider 报告 blocked work 时，outcome 分别表达 work remaining、immediate recheck 与
next deadline；stack 不读取 host wall clock，也不访问 kernel/provider registry。

默认 `host-test` 额外只为 host fixture 启用 smoltcp auto echo reply；kernel 的 no-default dependency 不
包含该行为。IPv4 配置入口同样只在 `host-test` 编译，带明确退出条件，不形成 production address、ICMP
endpoint 或 control-plane API。fixture 先用真实 ARP request 建立邻居事实，再注入 ICMP echo request，
由真实 interface 产生 echo reply 并通过正式 provider TX token 观察。两个独立 stack/provider pair 与
并发 pump caller 分别验证隔离和串行化。

首轮 checkpoint-scoped architecture review 发现两个 Keter：stack-local `spin::Mutex`/`Busy` 把 runtime
admission policy 错放进协议状态 owner；进程全局 atomic `InterfaceId` allocator 又把实例 namespace truth
移出 concrete stack owner。提交前修正移除了 `spin` 与 `Busy`，将协议推进收窄为 `&mut Stack`，并把
namespace cursor 移入 `Stack`；host 并发 fixture 改由调用方的 `std::sync::Mutex` 验证外层 admission。
复审后无剩余 Apollyon、Keter 或 Euclid。实现没有 unsafe、type erasure、trait object、provider-global
callback guard、descriptor/DMA/backing identity、kernel object、公开 smoltcp object、production endpoint/
socket/readiness/control-plane surface，也未命中 Stage 1 停止条件。

### 2026-07-26 - Checkpoint 2 validation and closure

- `cargo test -p anemone-net-api -p anemone-smoltcp-stack`：通过；1 个 stack unit test、9 个 integration
  tests 与 2 个 compile-fail doctests 全部通过。新增证据覆盖真实 ARP + ICMP echo、单次 ingress 调用上限、
  blocked egress 的单次调用上限、due deadline、`&mut Stack` 独占推进、外层 runtime lock admission，
  以及两个 stack/provider pair 的 mapping、frame credit、TX output 与 manual time 隔离。
- `cargo check -p anemone-smoltcp-stack --no-default-features` 与
  `cargo test -p anemone-smoltcp-stack --no-default-features`：通过；production `no_std + alloc` 形状不含
  host auto echo/address control，host integration fixture按 `required-features` 跳过。
- production dependency audit：stack 只有 `anemone-net-api + smoltcp` normal dependencies，kernel
  继续以 `default-features = false` 消费；source/public-surface audit 未发现 smoltcp object、socket handle、
  host clock、test control 或 unsafe 泄漏到 production signature。
- `just fmt kernel --check`：已运行，exit 1；formatter diff 仍只位于 frozen read-only 的既有
  `anemone-kernel/crates/anemos/smoltcp/**` 与 generated `anemone-kernel/src/boot_defs.rs`，本 checkpoint
  修改文件没有 formatter diff，未扩大 write set。
- `git diff --check`：通过。
- `mdbook build docs`：通过；仅有 search index size warning。

Checkpoint 2 delivery、review 与 validation floor 已闭合；没有改变 R0 target、owner、ABI、visible
semantics 或 acceptance，也没有 current contract cutover。kernel build、RV64/LA64、QEMU、hardware、LTP
与 final harness 均 Not Run，且不属于本 checkpoint floor。Stage 1 保持 Active，shared surface 继续
provisional；Checkpoint 3 未激活，本事务停在 Checkpoint 2 closure。

### 2026-07-26 - Stage 1 Checkpoint 3 activated

Checkpoint 3 从 commit `262bfbca` 的干净 `dev/drc/alpha` 工作树独立激活。Preflight 重新读取 R0、Stage 1
Ready 定义、tracking issues、register、current System Power contract、Checkpoint 1-2 transaction 与 live
VirtIO/device/IRQ/Kconfig owner；`VirtIODevice::take_transport()`、`VirtIONetRaw` begin/complete、
`VirtIOHalImpl::share/unshare`、boot-only device lifecycle 与 repository build/QEMU 入口仍匹配 canonical
baseline，没有命中停止条件。

**Write-set lock:** 只写 frozen manifest 内的 kernel `main`/module declaration、`device/net`、
`driver/net`、`net/worker`、xtask KernelConfig owner、默认配置与 canonical docs。generic bus/IRQ、
`driver/virtio`、vendored `virtio-drivers`/smoltcp、power、scheduler/time、apps/rootfs/LTP/LA64 owner 全部只读。

**Activation-time cutover:** Not Cut Over。六个 network IDs 与 System Power Refine 全部保持 pending；
Checkpoint 4 未激活。

### 2026-07-26 - Checkpoint 3 implementation and review

新增 `device/net` boot registry，唯一拥有单调 `NetdevId`、ifindex、`ethN`、origin 与 immutable normalized
publication snapshot。`ReadyNetdev<P>` / `PublishedNetdev<P>` 保留 concrete typed provider capability；registry
只保存 snapshot，不保存 driver backing、queue、transport 或另一份 lifecycle truth。VirtIO driver 在 queue
建立、32 个 initial RX refill、IRQ request 与 notification enable 后才调用 publication；成功 capability 暂存于
driver state，Checkpoint 4 才可一次移动给 attach authority。

VirtIO provider 分别独占 32 个 RX 与 32 个 TX stable boxed slot。slot state 把 begin 返回的 queue token 与
原 buffer identity 保存在同一 owner；RX 只有 matching `receive_complete` / HAL unshare 后才进入 Ready，TX
只有 matching `transmit_complete` 后回到 Available。protocol callback 只持具体 slot 的独占借用，不持 raw
queue `SpinLock`、IRQ-off guard 或 provider-global lock。IRQ-shared object 只拥有 raw queue lock 与
edge-only atomic；handler 只 ack hardware interrupt并发布 recheck edge，不 consume/submit frame 或进入 stack。

Checkpoint-scoped review 的早期形状暴露两个 Keter 并在最终实现前修正：slot 若放入 IRQ-shared `Arc` 会迫使
callback 跨 raw lock/IRQ-off，IRQ/driver state 若持 strong `Arc<VirtIONetDevice>` 又可能让 raw queue 比 slot
backing 活得更久。最终 provider 是唯一 durable strong owner，IRQ 与 driver state 只持 `Weak`。R0 明确没有
runtime removal：pre-IRQ failure 依靠 struct field drop order 先 unset raw queues；IRQ 注册后成功 provider
存活到 power-off，publication failure则先抑制 queue notification再有意保留 provider/backing到 reset。
运行时 removal 若未来进入 target，必须先阻止 Weak upgrade并 quiesce/reset queue，不能沿用当前析构前提。

最终 failure-path audit 另修正了 publication failure 仍保持 notification enable 的缺口；现在失败 entry 不会
进入 registry，IRQ handler 保留安全 backing lifetime，但 queue notification先被关闭。复审后无剩余
Apollyon、Keter 或 Euclid；没有修改/fork dependency、公开 descriptor/DMA/backing、建立镜像 queue truth、
扩大 generic lifecycle，或进入 attach/worker/time wiring。

五项 KernelConfig 只在 xtask `Parameters`、materialization、generated constant template 与 `.defconfig` 增加。
数值语义不由 xtask parser 拒绝或 fallback：VirtIO consumer 用 `static_assert!` 检查 queue `4..=1024`、2 的幂
与 backing `>= 1526`，worker consumer检查 ingress/egress budget和 repoll round均非零。

### 2026-07-26 - Checkpoint 3 validation and closure

- matching buffer/token 与 mapping audit：四个 unsafe block仅包围
  `receive_begin/receive_complete/transmit_begin/transmit_complete`；每次 begin/complete 都使用 slot 保存的同一
  queue token 与同一 boxed slice。每个 direct queue request只有一个 shared slice，正常 live bounce mapping
  上界为 32 RX + 最多 32 TX，并在 matching completion unshare；pre-IRQ failure先 unset queue，IRQ 注册后的
  不可安全回收路径按上述 boot-only规则保留到 reset/power-off。
- publication/owner audit：registry 不保存 provider/backing；duplicate origin在 commit 前拒绝，identity/name
  单调；queue/refill/IRQ 先于 publication；IRQ 只 ack + edge；callback 路径不跨 raw/provider-global lock。
- `cargo test -p anemone-net-api -p anemone-smoltcp-stack`：通过；1 个 unit、9 个 integration 与 2 个
  compile-fail doctests 保持通过，production provider feedback没有退化 host contract。
- `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`：sandbox 内首次在 lwext4 C
  compiler触发环境性 `SIGSYS / Bad system call`；同一 canonical 命令在 sandbox 外对最终源码完整通过。
- focused RV64 KUnit smoke 使用从只读 preliminary master 新建的 worktree-local runtime copy与最终 kernel：
  259/259，包含 netdev identity/name/duplicate 与 RX/TX owner-local cancellation 两个新增用例。复用先前已被
  KUnit 修改的 runtime disk曾使既有 openat 用例因残留文件得到 `AlreadyExists`；fresh copy重跑通过，故归类为
  validation-media contamination。KUnit 后因该盘缺少 `/.anemone/init` 按既有 boot protocol panic并 PowerOff；
  本结果不是完整 boot、Checkpoint 4 wiring 或网络流量证据。
- `just fmt kernel --check`：已运行，exit 1；formatter diff仍只位于 frozen read-only 的既有 vendored
  smoltcp baseline，本 checkpoint 修改的 Rust 文件没有 formatter diff，未扩大 write set。
- `git diff --check`：通过。`mdbook build docs` 在本次 write-back 后通过。

Checkpoint 3 delivery、unsafe/owner/failure-path review 与 validation floor 已闭合，没有命中 Stage 1 停止条件，
也没有改变 R0 target、owner、public API/shared contract、ABI、visible semantics 或 acceptance。RV64
attach/worker/time wiring、真实双向网络流量、queue saturation、LA64、hardware、LTP 与 final harness均
**Not Run**，不得从 build/KUnit evidence外推。Stage 1 保持 Active，shared surface继续 provisional；六个
network IDs 与 System Power Refine均 Not Effective。Checkpoint 4 为 **Not Activated / Unauthorized**，本事务
停在 Checkpoint 3 closure。
