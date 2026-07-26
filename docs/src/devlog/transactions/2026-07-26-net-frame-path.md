# 2026-07-26 - Network Frame Path

**Status:** Active / R1 Stage 2 Ready / Checkpoint 1 Not Started / Unauthorized
**Date:** 2026-07-26
**Owner:** doruche, Codex
**Canonical Plan:** [RFC-20260726-net-frame-path R1](../../rfcs/net-frame-path/index.md),
[目标与不变量](../../rfcs/net-frame-path/invariants.md),
[Stage 2 Ready definition](../../rfcs/net-frame-path/implementation.md#8-stage-2-readybounded-progress-conformance)
**Canonical Revision:** R1
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

用户现已独立授权完成 Stage 1 Checkpoint 4，并要求持续推进至 checkpoint closure 或停止条件；本授权只覆盖
kernel attach、IRQ/worker/time wiring、RV64 双向 vertical slice、stage-wide review、validation 与 write-back，
不得自动解析或进入 Stage 2，仍不授予 current-contract cutover。

Stage 1 与 Boundary Interlude 关闭后，用户独立授权解析 Stage 2 implementation。本授权仅覆盖只读
`1 -> 2 Implementation Resolution Gate`与docs write-back，不授权Stage 2 Checkpoint 1或任何实现、runtime
validation、contract cutover。

用户于2026-07-27独立授权完成Stage 2 Checkpoint 1，并要求持续推进至checkpoint closure或停止条件；本授权
只覆盖RV64 saturation observability probe、checkpoint-scoped review/validation/write-back与单独commit，
不得自动进入Checkpoint 2，也不授予target、owner、shared API/current contract或resolved manifest扩展。

R0 Checkpoint 1按failure signal停止后，用户接受将proof boundary修正为host deterministic exhaustion与RV64
bounded production-path completion/reclaim的互补证据，并授权只更新文档、最后提交一个commit。本授权形成R1
Target Renegotiation与Checkpoint 1 route-correction docs gate；不授权任何实现、host/build/QEMU/runtime验证、
Checkpoint 1 activation、current-contract cutover或Stage 3工作。

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

### 2026-07-26 - Stage 1 Checkpoint 4 activated

Checkpoint 4 从 commit `3a36a858` 的干净 `dev/drc/alpha` 工作树独立激活。Preflight 重新读取
AGENTS/LOCAL、R0 正文、Stage 1 Ready 定义、tracking issues、register、current System Power contract、
Checkpoint 1-3 transaction 与 live provider/stack/device/kthread/timer/IRQ source；`just --list`、build/qemu
help 与 RV64 wrapper 仍匹配 canonical commands，preliminary RV64 master 仍是只读共享资源。

Live owner 已提供冻结范围内所需的窄能力：concrete driver 可通过既有 `Driver::for_each_device()` 取得每个
已绑定设备的一次性 published capability；`KThreadHandle::wake()` 与 predicate wait 只传递 edge；threaded
timer 接受 process-context one-shot callback，stale callback 可只造成额外 wake。generic bus/IRQ、kthread、
timer/scheduler、vendored smoltcp/virtio-drivers 与 power owner均无需修改，没有命中 Stage 1 停止条件。

**Write-set lock:** 只写 frozen manifest 内的 stack `src/{lib.rs,pump.rs}`、kernel `device/net`、
`driver/net`、`net/{mod.rs,worker.rs}`、必要 manifest/feature wiring 与 canonical docs。generic bus/IRQ、
task/kthread、time/timer、scheduler、power、apps/rootfs/LTP/LA64 owner继续只读；若真实 runtime 迫使修改这些
owner，必须停止并报告扩展。

**Planned proof:** Stage 1 host validation gate；focused source/dependency/unsafe review；RV64 release build；
RV64 wrapper 日志中的 active attach、真实 VirtIO TX/TX completion、IRQ recheck、RX completion 与 echo reply；
全量 KUnit `All tests passed!`、正常 shutdown、`git diff --check` 与 `mdbook build docs`。

**Activation-time cutover:** Not Cut Over。六个 network IDs 与 System Power Refine 全部保持 pending；Stage 2
未解析、未激活。

### 2026-07-26 - Checkpoint 4 implementation and stage-wide review

`anemone-kernel::net` 成为唯一 attach authority：它一次移动 concrete driver 暂存的 published capability，
创建 stack-local `InterfaceId` mapping，完成 worker、provider wake 与 time wiring，再在同一 registry 临界窗口
提交 active predicate 和 active entry。attach 前缺少 MAC 或 worker spawn 失败均不产生 active entry；后者先
撤销 stack-local mapping。R0 没有 IRQ removal/reset rollback，失败 provider/backing 因此按既有 boot-only
lifetime 保留到 power-off，不伪造可安全析构。

每条 active path 由一个 ordinary kthread 独占 `Stack + VirtIONetProvider + InterfaceId`。worker 先清 edge，
再重读 provider queue/recheck 与 stack deadline truth；有限 pump round耗尽后显式 requeue、yield。kernel
monotonic time只在 attach owner 转为 `anemone-net-api::Instant`。threaded timer callback只持 worker wake
capability；worker-local arm去重相同 deadline，旧 callback 只会触发一次无状态 recheck，不成为第二份
deadline truth。

`kunit-probe` feature只在 concrete stack 私有边界建立 `10.0.2.15/24` 与 ICMP socket；`SocketHandle`、地址、
endpoint和完成读取均不出 stack。kernel KUnit只请求一次 probe，并观察完成 fact与 provider-local diagnostic
counters。非 KUnit kernel不启用该 address/endpoint；shared API、netdev registry 与 production driver surface
没有获得 control-plane、socket、readiness 或 packet injection能力。

Stage-wide review 在最终形状前修正四项问题：active registry最初会在 active predicate 前短暂可见；普通
IRQ/work pump会为同一 future deadline重复挂 one-shot timer；IRQ wake读取一次安装的 handle/capability时仍会
取得 owner-local spin lock；RX callback已消费后若 refill异常，Drop会尝试 Reserved-only cancellation并形成
二次 panic。最终实现分别采用同一临界窗口 publication、worker-local deadline arm、`spin::Once` 的无锁
只读 capability publication，以及 refill 前提交 consumed 状态。复审后无剩余 Apollyon、Keter 或 Euclid；
没有强引用环、provider-global callback guard、第二份 queue/deadline/mapping truth，四个 VirtIO unsafe window
仍与同一 boxed backing和 matching begin/complete token一一对应。

### 2026-07-26 - Checkpoint 4 validation and Stage 1 closure

- `cargo test -p anemone-net-api -p anemone-smoltcp-stack`：最终源码通过；1 个 stack unit、9 个 integration
  tests 与 2 个 compile-fail doctests证明 real-stack pump、ownership/Drop、capacity、budget、time、双实例隔离
  与 callback borrow non-escape。
- `cargo check -p anemone-smoltcp-stack --no-default-features --features kunit-probe`：通过；kernel 使用的
  `no_std + alloc + test-only ICMP` feature graph成立。dependency audit显示 `anemone-net-api` 无 normal
  dependency，production stack只有 `anemone-net-api + smoltcp`；shared API/production signature source audit
  未发现 smoltcp object、socket/control-plane、host object或 unsafe。
- `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`：最终源码在 sandbox 外完整通过。
  checkpoint内同一命令在 sandbox内的 lwext4 C build曾触发 `SIGSYS / Bad system call`，归类为已知环境限制，
  不作为 kernel build失败。
- `./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/net-frame-stage1-rv64.log`：最终
  wrapper exit 0。KUnit运行 260 个 tests；network probe直接打印 active `eth0` / ifindex 1 /
  `InterfaceId(0)`，RX completion 2、TX submit/completion 2/2、IRQ recheck 2、queue-full 0、live/high-water
  mappings 32/33，随后打印 `All tests passed!`。这证明 echo reply经真实 VirtIO TX、TX completion、IRQ、RX
  completion、worker和 stack返回。user-test完成后依次记录 filesystem/device shutdown与 PowerOff machine
  action。
- wrapper中的既有 signal/wait LTP profile实际运行：attempted 120、passed 106、failed 10、skipped 4、
  infra_failed 0。该 profile结果未作为 network correctness或全量 LTP结论，也未因既有失败阻止 wrapper
  正常完成。
- `just fmt kernel --check`：已运行，exit 1；全部 formatter diff仅位于既有 vendored
  `anemone-kernel/crates/anemos/smoltcp/**`。本 checkpoint六个修改 Rust 文件的独立 `rustfmt --check`通过。
  `git diff --check`、Stage 1 aggregate diff whitespace check 与 `mdbook build docs` 均通过；mdBook只有 search
  index size warning。

Checkpoint 4 delivery、stage-wide review、validation 与退出条件全部闭合，Stage 1 为 **Closed**。没有触发
停止条件，也没有改变 R0 target、owner、public API/shared contract、ABI、visible semantics 或 acceptance。
RV64 SMP>1、LA64 build/runtime、virtio-pci、hardware、queue saturation/Stage 2 conformance、final harness 与
其它 LTP profile均 **Not Run**，不得从本次证据外推。六个 network IDs 与 System Power Refine继续
**Not Effective**；Stage 2 保持 **Outline / Not Resolved / Unauthorized**，本事务停止在 Stage 1 closure。

### 2026-07-26 - Stage 1 -> 2 Boundary Interlude activated

Stage 1关闭后的独立module-boundary review重新读取R0、Stage 1 aggregate diff、transaction/runtime evidence、
tracking issues、current contracts、live API/stack/device/driver/worker source与Cargo feature graph。Review确认
Stage 1行为与proof仍成立，但发现三项实现反馈：concrete VirtIO provider/wake/diagnostics通过crate-wide
module visibility进入通用worker；KUnit harness vocabulary跨入first-party stack crate；多个已有stable role
仍集中在少数文件并已开始推动错误visibility。分别记录为NFP-004 Keter、NFP-005与NFP-006 Euclid。

用户将该反馈授权为Stage 1与Stage 2之间的独立间章并要求完成。authoritative delivery、owner route、review、
validation、停止条件与resolved manifest见
[Boundary Interlude](../../rfcs/net-frame-path/implementation.md#7-stage-1---2-boundary-interludeowner-与-validation-boundary-整理)。
本次route preservation不改变R0 target、状态owner、shared semantics、ABI、visible behavior或acceptance，
因此不递增revision；Stage 1历史closure保持不变，Stage 2仍未解析且未授权。

**Activation baseline:** `dev/drc/alpha@044ac1ec`，工作树在文档解析前clean；live build入口仍为显式
`just build --preset ... --bind ...`与repository RV64 wrapper。六个network IDs与
`SYSTEM-POWER-ORDERLY-001` Refine全部保持Not Effective。

**Write-set lock:** production、validation与docs范围以interlude resolved manifest为准。vendored smoltcp、
generic device/bus/IRQ、kthread/timer/scheduler、power、apps/rootfs/LTP profile、LA64/PCIe与current contracts
保持只读。若实现要求进入这些owner或改变shared `FrameProvider`，必须停止并报告。

### 2026-07-26 - Stage 1 -> 2 Boundary Interlude closed

实现按 authoritative manifest 完成行为保持的 owner split。`anemone-net-api` 现在由 private
`interface/time/frame/pump` modules 组成并保持原 root re-export；`anemone-smoltcp-stack` 分离
adapter、interface mapping、bounded pump 与 ICMP validation，kernel feature只转发
`icmp-validation-probe`，两个 first-party crate中已无 KUnit vocabulary。

kernel-local `device/net::{NetdevFrameProvider, RecheckWake}` 现在拥有 provider/worker handoff：provider继续
唯一拥有 durable recheck predicate，wake只携带 stateless edge；generic worker只依赖该port并以 concrete
provider泛型化，不再import或命名VirtIO类型。`driver::net::virtio`恢复private，窄facade只导出一次性
published-netdev move与KUnit-only `stage1_probe_stats()`；后者明确是Stage 1单NIC evidence query，Stage 3
多设备验收前必须替换，不形成control-plane API。

`device/net`按registry/provider拆分，VirtIO-Net按registration/publication、raw device/IRQ与frame slot/token
拆分。复审确认四个unsafe begin/complete点的buffer/token identity、device ownership window与sync条件未变；
provider field drop order、publication failure retention、attach prepare-before-publish与IRQ edge-only行为未变。
diagnostic counters不驱动queue/worker/probe completion；非KUnit build不编译stats snapshot/query。

**Validation:**

- `cargo test -p anemone-net-api -p anemone-smoltcp-stack`通过：1个stack unit、9个integration、2个
  compile-fail doctest；
- base与`icmp-validation-probe`两种`--no-default-features` check通过；warning仅来自既有vendored smoltcp；
- canonical RV64 release build在sandbox外通过；sandbox内仍由已知lwext4 `SIGSYS / Bad system call`环境限制
  阻断，不记为代码失败；
- RV64 wrapper `build/net-frame-boundary-interlude-rv64.log`通过并正常关机：260/260 KUnit通过；真实纵切
  观测RX completion 2、TX submit/completion 2/2、IRQ recheck 2、queue-full 0、live/high-water mappings
  32/33；现有signal/wait profile为106/120、10 failed、4 skipped，与网络proof无关且未据此扩展结论；
- focused changed-file `rustfmt --check`通过；`just fmt kernel --check`仍只报告vendored smoltcp的三个既有
  diff；dependency/visibility/unsafe/source audit无剩余Apollyon、Keter或Euclid；`git diff --check`与
  `mdbook build docs`通过。

NFP-004、NFP-005与NFP-006已Neutralized。间章没有触发停止条件，也没有改变R0 target/revision、owner、
shared `FrameProvider` semantics、ABI、visible behavior或acceptance；current contracts继续Not Effective。
Stage 2保持**Outline / Not Resolved / Unauthorized**，本次没有运行resolution gate或进入Stage 2。

### 2026-07-26 - Stage 1 -> 2 Implementation Resolution Gate completed

**Authorization / entry:** 用户在Stage 1与Boundary Interlude独立关闭后明确要求解析Stage 2 implementation；
本gate只获docs-only resolution授权，没有Stage 2代码、Checkpoint 1 activation、runtime validation或contract
cutover权限。入口为`dev/drc/alpha@609f8fff`，worktree clean；Boundary Interlude commit之后没有tracked或
untracked dirty change。

**Preflight evidence:** 重新读取R0 target/invariants/tracking、System Power current contract、register、
Stage 1与Boundary Interlude aggregate diff/transaction evidence，以及live shared API、stack、VirtIO-Net、
kernel-local provider port、worker、validation、KernelConfig、RV64 Platform/wrapper和Just/xtask CLI。当前
provider固定拥有32个RX slot与32个TX slot；正常boot保持32个persistent RX mappings，Stage 1单echo evidence
只到live/high-water 32/33且queue-full 0。四个unsafe begin/complete window仍以matching queue token与stable
boxed backing闭合，IRQ只ack并提交recheck bit，worker只读kernel-local predicate。

live source确认三个Stage 2 implementation gap：adapter没有把link unavailable记作owner-blocked；pump在
TX/link blocked时仍可能因due deadline/budget返回immediate并持续repoll/yield；固定ingress-first可能在连续
response流中先耗尽每轮TX credit，使queued egress长期没有software admission机会。这些都属于R0已承诺的
bounded-progress conformance范围，不改变owner、shared API、ABI/visible semantics、contract delta或acceptance，
因此不增加tracking issue、不递增R0。

**Resolved route:** canonical
[Stage 2 Ready](../../rfcs/net-frame-path/implementation.md#8-stage-2-readybounded-progress-conformance)
冻结三个顺序checkpoint：

1. validation-only ICMP burst先在RV64真实路径证明queue saturation可观察、可completion/IRQ恢复；若只能靠
   暂停completion、伪造queue state或sleep-as-correctness，删除probe并停止；
2. host fixture按test owner拆分，闭合token cancel/unwind、matching completion、bounded exhaustion、link
   recovery、coalesced recheck、deadline/budget与alternating RX/TX admission；
3. driver-private durable recheck latch、slot/mapping assertions、finite worker repoll与两次fresh-disk RV64
   saturation acceptance收口。

authoritative stage同时冻结review/unsafe/wake/validation-bypass audit、observability、Not Run、contract None、
停止/退出条件与exact tracked write set。`anemone-net-api`、vendored smoltcp/virtio dependency、`device/net`、
generic IRQ/task/kthread/timer/scheduler/power、apps/rootfs/LTP、platform/wrapper、current contracts/register均只读。

**Contract / lifecycle:** 本gate没有改变R0语义。六个network IDs与`SYSTEM-POWER-ORDERLY-001` Refine继续
Not Effective；Stage 2 contract cutover为None，Stage 3仍是Outline。Stage 2现在是**Ready / Not Started /
Unauthorized**；Checkpoint 1没有激活，后续必须取得独立实现授权并按checkpoint边界推进。

**Validation boundary:** 本gate只执行read-only source/config/command审计与docs write-back。`just --list`、
`just build --help`、`just qemu --help`、`just fmt --help`和RV64 `--show-bindings`确认当前显式preset、provider
`smp`/`memory`与runtime disk绑定路线；没有运行cargo test/check、formatter、kernel build、QEMU、LA64、hardware、
LTP或final harness，Stage 2 saturation/recovery全部Not Run。`git diff --check`与`mdbook build docs`通过；
mdBook只报告既有large search-index warning，新增Stage 2 anchor与跨页链接命中生成HTML。

### 2026-07-27 - Stage 2 Checkpoint 1 activated

**Authorization / entry:** 用户独立授权完成Stage 2 Checkpoint 1并明确不得自动进入下一gate。入口为
`dev/drc/alpha@c9120890`，tracked/untracked worktree均clean；上一commit只完成Stage 2 docs resolution，
Checkpoint 1尚无实现或runtime evidence。本次没有用户dirty change与frozen manifest重叠。

**Preflight evidence:** 重新读取AGENTS/LOCAL、R0正文/invariants、Stage 2 Ready、tracking issues、register、
System Power current contract、当前transaction与live stack/provider/worker/validation owner。feature graph仍由
kernel `kunit`单向启用stack `icmp-validation-probe`；provider仍固定64-entry queue、32个RX slot、32个TX slot，
现有KUnit-only snapshot已包含RX/TX completion、TX submit、queue-full、IRQ recheck与live/high-water mapping，
生产代码不读取这些diagnostic counters。现有单echo seam尚不能产生saturation，正是本checkpoint获准闭合的
live gap。

`just --list`与build/qemu/fmt help未漂移；RV64 preset仍要求provider `smp`/`memory`，runtime仍显式绑定kernel
与可选disk；wrapper继续使用`smp=1`、`memory=1G`、tracked pretest rootfs，并把调用者提供的只读master复制到
`build/runtime/pretest-rv64/disk-x0.img`后运行。master存在且为4 GiB ordinary file。wrapper会重建
`build/rootfs/pretest-rv64/rootfs.img`并覆盖上述runtime disk；本次validation接受这些generated side effect。

**Activated route / boundaries:** 只在Stage 2 Checkpoint 1 manifest内把validation-only ICMP seam扩成
`2 * VIRTIO_NET_QUEUE_SIZE`有界burst，由普通pump/provider/IRQ/completion路径制造并恢复真实exhaustion；KUnit
只比较owner snapshot并用`yield_now()`与monotonic deadline等待durable predicates。禁止暂停/吞掉completion、
伪造queue state、sleep-as-correctness、diagnostics驱动production或generic provider test-control。shared API、
vendored dependencies、`device/net`、generic runtime owner、platform/wrapper、power、register/current contracts均
只读；Checkpoint 2未激活。

**Activation-time contract state:** Stage 2 cutover为None；六个network IDs与
`SYSTEM-POWER-ORDERLY-001` Refine继续Not Effective。

### 2026-07-27 - Stage 2 Checkpoint 1 stopped at saturation failure signal

**Implementation / probe:** 在frozen manifest内把validation-only ICMP seam临时扩成128个不同sequence的
有界burst（`2 * VIRTIO_NET_QUEUE_SIZE`），沿原`PumpControl`、普通stack pump、concrete provider、真实IRQ与
completion路径运行。KUnit在请求前后读取同一owner-scoped snapshot，并只用`yield_now()`与monotonic deadline
等待TX submit/completion匹配、live mappings回到probe前persistent RX baseline；没有暂停/吞掉completion、
伪造queue state、修改slot ownership、加入sleep-as-correctness或让diagnostics驱动production。

**Validation evidence:** host gate通过：1个stack unit、9个integration与2个compile-fail doctest；base与
`icmp-validation-probe`两种`--no-default-features` check通过。`just fmt kernel --check`对本checkpoint三个
Rust文件无diff，只保留Stage 1已记录的三个vendored smoltcp baseline diff。canonical RV64 release build在
sandbox内先被既有lwext4 C compile `SIGSYS / Bad system call`阻断，同一命令在sandbox外通过。

RV64 wrapper使用只读`etc/preliminary/images/sdcard-rv.img`的worktree-local副本并写入
`build/net-frame-stage2-c1-rv64.log`。保留日志直接证明KUnit触发
`ICMP burst did not observe normal VirtIO TX exhaustion` panic，随后走System Power emergency
`PowerOff machine action`；它没有打印最终counter summary，因此不能从持久证据断言精确submit/completion、
mapping数值或completion相对下一轮pump的时序。当前工作假设是TCG/virtio completion回收过快，但必须由后续
route-correction gate重新验证。全量260项KUnit未完成，本次也没有normal orderly shutdown；该日志是明确
负证据，不是Stage 2 saturation PASS。

**Failure disposition / stop:** 命中Checkpoint 1局部failure signal后，全部临时probe source已删除；最终
tracked source相对`c9120890`无diff，只保留本transaction与stage lifecycle write-back。没有尝试调小production
budget、暂停device、伪造completion、增加test-control或把QEMU proof降级为host-only。Checkpoint 1为
**Stopped / Not Closed**，Stage 2在此停止；Checkpoint 2未激活。后续若继续，必须先独立review真实saturation
route并更新authoritative implementation route，不能机械重跑当前burst。

**Contract / Not Run:** Stage 2 cutover仍为None；六个network IDs与`SYSTEM-POWER-ORDERLY-001` Refine继续
Not Effective。RV64 saturation/recovery acceptance、完整KUnit、两次fresh-disk final run、SMP、LA64、
virtio-pci、hardware、LTP、final harness与Stage 3均Not Run / Not Achieved。

### 2026-07-27 - R1 Target Renegotiation and Checkpoint 1 route correction

**Authority / evidence:** 用户明确接受docs-only proof-boundary correction并要求单commit。Gate读取R0 target/
invariants、Stage 2原Ready route、Checkpoint 1临时probe形状与保留日志、live pump/provider admission顺序、
KernelConfig的64-entry queue/32 TX slots/32 egress budget、Stage 1的32/33 mapping evidence、tracking issues、
current contracts与transaction。结论是累计128 packet不等于同时占用超过32个TX credit；provider每次admission
前回收completion是正确production progress，不能为测试推迟。保留日志只证明exhaustion assertion失败，不足以
固定TCG completion精确时序。

**Accepted R1 correction:** normal exhaustion必须保持normal、bounded且可由matching completion/recheck恢复，
owner/public API/ABI/visible semantics/contract delta均不变。确定性exhaustion proof改由真实stack + capacity-2
deterministic host provider以3-frame transaction完成；RV64改为bounded burst上的真实TX/RX completion、IRQ、
outstanding上界、mapping回落、bounded worker action与正常关机。RV64自然观察到exhaustion时必须追加恢复证据；
未观察到只记录事实，不再构成失败。`NET-FRAME-PROGRESS-001` cutover与新增`NFP-PROOF-005`已经折回canonical
invariants；NFP-007记录问题来源并由R1 neutralize。

**Resolved Checkpoint 1:** authoritative Stage 2现在按顺序包含：(1) host deterministic exhaustion seam + 一次
RV64 production observability；(2) host ownership/progress/pump完整矩阵与production gap修复；(3) provider/
recheck/worker closure与两次fresh-disk RV64 acceptance。Checkpoint 1明确冻结host capacity 2 / 3-frame
exhaustion-recovery transaction、128-sequence RV64 workload、diagnostic-only outstanding/worker summary、条件式
natural-exhaustion义务、review/validation/failure signal与原write set。Ready不授予执行权限。

**Contract / validation boundary:** R1没有cutover；六个network IDs与`SYSTEM-POWER-ORDERLY-001` Refine继续Not
Effective，current contracts/register/current limitations均未修改。R1 docs gate只执行source/doc审计、whitespace、
链接与mdBook验证；没有运行host test、cargo check、formatter、kernel build、QEMU、KUnit、SMP、LA64、hardware、
LTP或final harness。旧Checkpoint 1仍为历史Stopped / Not Closed，临时probe仍已删除；新R1 Checkpoint 1为
**Ready / Not Started / Unauthorized**，Checkpoint 2/3均未激活。
