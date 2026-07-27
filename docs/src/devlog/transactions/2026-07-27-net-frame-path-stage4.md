# 2026-07-27 - Network Frame Path Stage 4

**Status:** Completed / R1 Stage 4 single checkpoint Closed
**Date:** 2026-07-27
**Owner:** doruche, Codex
**Canonical Plan:** [RFC-20260726-net-frame-path R1](../../rfcs/net-frame-path/index.md),
[Stage 4 Ready definition](../../rfcs/net-frame-path/implementation.md#10-stage-4-readypost-close-contract-conformance-correction)
**Canonical Revision:** R1
**Contract Impact:** None. `NET-BOUNDARY-001`、`NETDEV-LIFE-001`、`NET-FRAME-OWN-001`、
`NET-FRAME-PROGRESS-001`、`NET-STACK-PUMP-001`、`NET-ATTACH-001`与
`SYSTEM-POWER-ORDERLY-001`保持Effective；本事务不执行第二次cutover。

## Scope and authorization

用户把完成Stage 4设为本轮唯一GOAL，并要求持续推进到closure或命中停止合同。授权覆盖
[Stage 4 resolved manifest](../../rfcs/net-frame-path/implementation.md#106-resolved-write-set-manifest)内的单一
aggregate checkpoint、独立review、全部validation floor、canonical write-back与一个`frame-path:`commit。
本事务不得进入`net-udp`、`net-tcp`或runtime lifecycle工作；不得改变R1 target、owner、public API、shared
contract、ABI/visible semantics、platform scope或acceptance boundary，也不得越出冻结write set。

原[2026-07-26 transaction](./2026-07-26-net-frame-path.md)保持Completed且只读；它继续拥有Stage 1-3与
`NFP-FINAL-CUTOVER`历史证据，本事务只记录post-close conformance correction。

## 2026-07-27 - Stage 4 activation preflight

**Entry state:** `dev/drc/alpha@bdcab2f1`，tracked与untracked worktree均clean；没有用户dirty change与Stage 4
source/docs write set重叠。AGENTS/LOCAL、R1正文/invariants/implementation/tracking、register、原Completed
transaction、current Network/System Power contracts与live owners已重新读取；backgrounds未用于本次实现判断。

**Open findings and effective authority:** NFP-008、NFP-009为Keter，NFP-010为Euclid；register umbrella
`ANE-20260727-NET-FRAME-PATH-CONFORMANCE`保持Open。`NETDEV-LIFE-001`要求`device/net`唯一拥有publication
record与published/unattached capability；`NET-ATTACH-001`要求逐netdev attach、失败撤销transaction-local mapping
并保持published/unattached。六个Network ID与`SYSTEM-POWER-ORDERLY-001`均已Effective，本stage只恢复live
conformance，不更新current contract文本、来源或最后核验。

**Boot-order evidence:** `InitCallLevel::Late`只是filesystem/driver/probe、kthreadd与all-CPU local init之后的通用
窗口，不提供consumer间相对顺序。threaded timer在普通`Late` initcall中发布per-CPU worker，schedule入口在
worker未ready时会触发correctness assertion；network attach当前也是`Late` initcall。Stage 4只允许在BSP
`run_initcalls(InitCallLevel::Late)`完整返回后、用户态init exec前显式调用network activation，不修改initcall、
timer或其它Late consumer。

**Pending-owner and dependency evidence:** registry当前只保存`NetdevSnapshot`；concrete VirtIO driver state另存
`PublishedNetdev<VirtIONetProvider>`并由kernel net经driver-specific drain取得。live `worker::prepare<P>`已经保持
concrete provider、stack mapping、wake/time wiring与active publication的泛型data path，但失败会`forget`
provider而不能把同一capability交还registry。冻结文件足以把异构pending storage/drain移入registry，并用一次性
erased attach operation在边界恢复到generic `P`；不需要`dyn FrameProvider`、downcast、shared API、provider.rs
或driver-to-stack dependency。

**Cargo target graph and negative baseline:** `frame_path`是唯一显式`[[test]]`且声明
`required-features = ["host-test"]`的target；`bounded_progress`与`multi_instance`仍由Cargo自动发现。实际运行
`cargo test -p anemone-smoltcp-stack --no-default-features --no-run`复现13个`E0599`，均来自两个host-only target
调用被feature排除的helper；production library只出现既有vendored smoltcp warning。Stage 4将只补齐两条同形
metadata，不删除或弱化test source。

**Frozen execution boundary:** implementation与local KUnit只写Stage 4 10.6列出的文件；`anemone-net-api`、
`device/net/provider.rs`、smoltcp source/tests、VirtIO data plane、timer/initcall/generic runtime/power、build
orchestration、current contracts/current limitations与其它RFC保持只读。验证按canonical Stage 4 floor执行；任何
mandatory gate失败或出现manifest/owner/API/contract扩张都使本事务保持Active并立即停止。

**Activation:** Stage 4现为Active / 单一checkpoint；contract cutover为None。实现、review、runtime、closure与
NFP-008/009/010/register关闭尚未执行，不能预记为通过。

## 2026-07-27 - Stage 4 implementation and owner audit

**Registry-owned handoff:** `device/net` registry现在在同一publication transaction内提交stable record与异构
pending capability；duplicate/identity/name failure不会留下半边状态。类型擦除只存在于registry向attach authority
移交的一次性调用边界，concrete operation随即恢复`worker::prepare<P>`泛型路径；`PumpCore<P>`、frame token、
resource truth与data plane没有改成`dyn FrameProvider`，也没有引入downcast或provider-specific registry policy。

attach逐项消费当前drain snapshot。成功路径把concrete provider移入唯一worker core；失败路径先撤销本次
transaction建立的stack mapping，再把同一capability重新插回registry-owned pending retention。当前drain不会
重新读取这些回插项，因此没有隐式retry；一个entry失败也不阻塞后续entry。local registry KUnit使用两个不同
concrete provider覆盖异构retention、一个成功attach、一个失败attach回插与publication record隔离。

**Dependency and lifecycle boundary:** VirtIO driver删除driver-owned
`PublishedNetdev<VirtIONetProvider>` slot与driver-specific drain；driver state只保留shutdown所需的device `Weak`
capability。kernel `net`只从`device/net`取得窄one-shot attach input，不再import concrete VirtIO provider或driver
facade；VirtIO driver不依赖stack/worker。active publication仍位于mapping、worker、wake/time wiring全部成功之后，
shutdown admission、terminal retention、network-before-device与emergency bypass保持Stage 3行为。

**Post-`Late` activation and Cargo metadata:** network删除`#[initcall(late)]`，BSP boot coordinator在完整
`run_initcalls(InitCallLevel::Late)`返回后、用户态init之前显式调用attach。timer与其它`Late` consumer未修改，
也没有新增initcall priority/readiness API。`bounded_progress`与`multi_instance`补齐与`frame_path`相同的显式
`[[test]]`和`required-features = ["host-test"]`；三个长期host target源码与production feature graph均未删减。

**Review:** 独立只读software-engineering review逐项复核registry transaction、异构erasure、failure
reinsertion、dependency direction、post-`Late` order、active/shutdown lifecycle、Cargo target graph与resolved
manifest，结论为Apollyon 0、Keter 0、Euclid 0、Safe 0。完整aggregate diff没有改变R1 target、owner、public
API、shared contract、ABI/visible semantics、platform scope或acceptance boundary，也没有命中Stage 4停止条件。

## 2026-07-27 - Stage 4 validation and closure

**Host and production feature gates:** `cargo test -p anemone-net-api -p anemone-smoltcp-stack`通过：2个stack
unit、7个bounded-progress、9个frame-path、2个multi-instance与2个compile-fail doctest；三个长期integration
target均实际执行。`cargo test -p anemone-smoltcp-stack --no-default-features --no-run`与
`cargo check -p anemone-smoltcp-stack --no-default-features`均通过，确认host-only target由Cargo metadata排除且
production feature set可编译。

**Build and formatting:** canonical
`just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`在sandbox内复现既有lwext4
`Bad system call` / `SIGSYS`，沙箱外相同仓库命令通过。`just fmt kernel --check`仍只报告三处既有vendored
smoltcp baseline；所有Stage 4 authored Rust/TOML file均无新增formatter drift，没有为修复基线扩大write set。

**Fresh-disk RV64:** 运行
`./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/net-frame-stage4-rv64.log`；wrapper从显式
pretest master建立fresh runtime disk，以RV64 QEMU virtio-mmio、`smp=1`、`memory=1G`运行。260/260 KUnit通过，
包括registry conformance case；`eth0`完成publication与active attach，随行`sys` profile的glibc/musl合计4/4
通过，日志随后严格记录`filesystem -> network -> device -> PowerOff`并以QEMU exit 0正常结束。本stage没有
packet injection probe，因此本轮不形成新的traffic/completion证明；Stage 2/3历史证据保持原结论。

**Static/docs gate:** final Stage 4 net-path source audit确认没有`take_published_netdevs`、driver-owned published
slot、network `Late` initcall、`dyn FrameProvider`、downcast、provider-specific registry branch或manifest外tracked write；
`git diff --check`与`mdbook build docs`通过。build日志、runtime disk与其它`build/**`产物不进入checkpoint。

**Not Run / claim boundary:** `smp>1`、LA64 build/runtime、virtio-pci、physical hardware、final harness、完整
LTP、runtime hotplug/retry/restart、完整teardown、socket/control-plane与packet injection均Not Run或非目标。
本次RV64 single-core boot、随行`sys` profile、host test与build evidence不外推到这些层级。

**Closure:** NFP-008/009/010已由同一aggregate correction neutralize，register umbrella
`ANE-20260727-NET-FRAME-PATH-CONFORMANCE`同步关闭。Stage 4与R1 RFC回到Closed；六个Network ID和
`SYSTEM-POWER-ORDERLY-001`继续以既有current contract保持Effective，本transaction没有第二次contract
cutover，也没有修改current contract文本、来源或最后核验。事务状态设为`Completed`，最终交付由包含本页的
单一`frame-path: ...`commit保存；本轮到此停止，不进入`net-udp`、`net-tcp`、runtime lifecycle或任何后续gate。
