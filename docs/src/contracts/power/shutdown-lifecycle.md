# System Power Shutdown Lifecycle 当前契约

**Contract ID：** `SYSTEM-POWER`
**状态：** Active
**Owner：** `power` terminal invocation 与 machine-handler registry
**参与领域：** power / panic / filesystem / device / storage drivers / architecture bootstrap
**覆盖范围：** 当前 orderly power-off/reboot、panic 到 ordinary power-off 的 handoff、最终 handler traversal
**不覆盖：** 尚未生效的 global episode、strong durability、userspace lifecycle、driver-local flush/quiesce
**实现位置：** `anemone-kernel/src/{power.rs,panic.rs,fs/mod.rs,device/mod.rs,arch/riscv64/bootstrap.rs}`
**依赖：** 当前 filesystem、device tree、driver 与 IPI owner-local 规则；尚无对应 stable contract ID
**Pending Successor：** None；[RFC-20260726-system-power](../../rfcs/system-power/index.md) 仍为 Draft，
其 Introduce / Refine / Replace target 尚未接受
**最后核验：** 2026-07-26

本页从 live owner 提取 promotion 时已经生效的最小 baseline。它不把现状评价为理想方案，也不把
System Power RFC 的 pending target 提前写成当前事实。

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 当前行为用途 |
| --- | --- | --- | --- |
| power-off / reboot 入口与调用顺序 | `power` | subsystem facade | 依次执行 ordinary cleanup 与 machine action |
| panic diagnostics 与 stop broadcast | panic owner | `power_off()` 调用能力 | 诊断后复用 ordinary power-off |
| filesystem shutdown snapshot / sync | VFS 与 concrete filesystem | `power` 的单次 facade invocation | 对 mount snapshot 调 `sync_fs` |
| device tree traversal / driver callback | device subsystem 与 concrete driver | `power` 的单次 facade invocation | child-before-parent shutdown |
| power-off / reboot handler lists | `power` | platform/driver 注册的 boxed capability | 按注册顺序尝试 machine action |

当前没有 global episode、executor 或 frozen intent 状态；`PANIC_OCCURRED` 只记录 panic 已发生，
不参与 terminal arbitration。并发或重复 terminal 请求没有 current contract guarantee，不能从现状
推导 first-winner、single execution 或 takeover 语义。

## SYSTEM-POWER-ORDERLY-001

**规则标题：** Orderly power-off/reboot 直接串行执行现有 owner facade。

**规则：** `power_off()` 与 `reboot()` 当前各自按固定源码顺序调用 `fs::on_shutdown()`、
`device::shutdown()`，然后遍历与 intent 对应的 machine-handler list。Orderly 入口不发送
`StopExecution`，也没有 global publication、executor election、重试或 rollback。

`fs::on_shutdown()` 对 anonymous 与 visible mount tree 分别取得 mount/superblock snapshot，分别按
`Arc` identity 去重，并只调用 concrete filesystem 的 `sync_fs`；它不遍历 resident inode cache。
单个 `sync_fs` error 被记录后继续。`device::shutdown()` 独占 child-before-parent traversal，并对每个
bound device 调一次 driver `shutdown()`；`power` 不复制 device tree 或 driver-local ordering。

**违反表现：** 在没有 contract cutover 的情况下改变 fs -> device -> machine 方向；`power` 直接遍历
filesystem/device 私有 object；把现有 `sync_fs` 调用描述为 resident page writeback 或完整 durability。

**验证 / Enforcement：** `power_off()` / `reboot()`、`fs::on_shutdown()` 与
`device::shutdown()` source closure。2026-07-26 baseline extraction 未修改 runtime code，也未新增
QEMU、hardware 或 durability 证据。

**最初来源：** 现有 `power`、VFS 与 device shutdown 实现。

**当前来源：** [System Power RFC promotion preflight](../../rfcs/system-power/implementation.md#promotion-preflight--2026-07-26)。

## SYSTEM-POWER-EMERGENCY-001

**规则标题：** 当前 panic 在诊断后复用 ordinary power-off。

**规则：** panic handler 当前设置 `PANIC_OCCURRED`、关闭本地中断、best-effort 广播
`IpiPayload::StopExecution`，记录发送失败并继续打印 panic/backtrace，随后调用 ordinary
`power_off()`。因此 panic 会继续进入 `fs::on_shutdown()`、`device::shutdown()` 和 power-off handler
list；当前没有独立 emergency callback plan、executor arbitration 或 double-panic 收口。

**违反表现：** 把 panic 当前路径描述为跳过 ordinary cleanup；把 asynchronous stop broadcast 写成
remote completion barrier；从 `PANIC_OCCURRED` 推导唯一 executor 或 recursion control。

**验证 / Enforcement：** `panic.rs` 到 `power_off()` 的 source closure；未运行受控 panic runtime。

**最初来源：** 现有 panic handler 与 ordinary power-off implementation。

**当前来源：** [System Power RFC promotion preflight](../../rfcs/system-power/implementation.md#promotion-preflight--2026-07-26)。

## SYSTEM-POWER-MACHINE-001

**规则标题：** 两个动态 handler list 按注册顺序尝试，穷尽后在列表外 spin halt。

**规则：** `power` 当前分别持有 `SpinLock<Vec<Box<dyn PowerOffHandler>>>` 与
`SpinLock<Vec<Box<dyn RebootHandler>>>`。注册函数 append 到对应 list；terminal 入口持有
`lock_irqsave()` guard 遍历，handler 返回时继续下一个。列表穷尽后打印 emergency log，并在列表外
执行不返回的 spin loop；halt 不是 registered handler，也没有“永久末位”结构保证。

RISC-V bootstrap 当前向两个 list 各注册一个 SBI capability；若 `sbi_rt::system_reset()` 返回，handler
执行 `unreachable!()`，而不是正常返回到下一 handler。2026-07-26 source audit 未发现 LoongArch 对等
power-off/reboot handler 注册；这只证明当前 source coverage，不构成 hardware capability 结论。

**违反表现：** 把 list 描述为包含内建末尾 halt；声称 SBI 返回会继续 fallback；声称当前 list lock /
handler 具有 panic-safe progress；在未验证平台上把 DTS node 当成已注册 machine capability。

**验证 / Enforcement：** `power.rs` list/register/traversal 与 RISC-V bootstrap source audit；当前
contract extraction 未运行 RV64/LA64 QEMU 或硬件 power-off/reboot。

**最初来源：** 现有 `power` handler registry 与 RISC-V bootstrap registration。

**当前来源：** [System Power RFC promotion preflight](../../rfcs/system-power/implementation.md#promotion-preflight--2026-07-26)。
