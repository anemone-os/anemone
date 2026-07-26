# 2026-07-26 - Network Frame Path

**Status:** Active / Stage 1 Checkpoint 1 Closed; Checkpoint 2 Not Activated
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
