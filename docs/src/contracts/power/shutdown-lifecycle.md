# System Power Shutdown Lifecycle 当前契约

**Contract ID：** `SYSTEM-POWER`
**状态：** Active
**Owner：** `power` terminal episode、跨 subsystem shutdown sequencing 与 machine-handler registry
**参与领域：** power / panic / filesystem / device / storage drivers / architecture bootstrap
**覆盖范围：** 当前 orderly power-off/reboot、panic/emergency handoff、最终 machine-action attempt
**不覆盖：** strong durability、userspace lifecycle、driver-local flush/quiesce 完整性或 firmware ABI
**实现位置：** `anemone-kernel/src/{power.rs,panic.rs,fs/mod.rs,fs/superblock.rs,device/mod.rs,arch/riscv64/bootstrap.rs}`
**依赖：** filesystem/device/driver owner-local lifecycle 与 best-effort `StopExecution` transport
**Pending Successor：** None
**最后核验：** 2026-07-26

本页是 System Power R0 cutover 后的 current authority。`power` 只拥有全局 terminal episode、静态
subsystem handoff 和最终 machine capability 选择；filesystem resident cache、device tree、driver
queue/hardware 与 task/mm 状态继续由各自 owner 独占。

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 当前行为用途 |
| --- | --- | --- | --- |
| terminal episode / executor / frozen intent / phase | `power` | subsystem facade invocation | first-winner 仲裁与单向 terminal progression |
| panic diagnostics 与 stop broadcast | panic owner | `power` emergency publication 和 machine-action capability | 诊断后跳过 ordinary plan |
| resident inode snapshot / filesystem commit | VFS 与 concrete filesystem | `power` 的单次 facade invocation | best-effort writeback 后 commit |
| device tree / driver-local flush 与 shutdown | device subsystem 与 concrete driver | `power` 的单次 facade invocation | child-before-parent traversal |
| power-off / reboot handler lists | `power` | platform/driver 注册的 boxed capability | 按 frozen intent 尝试 machine action |
| userspace task / mapping lifecycle | task/mm owner | system-power 不取得 capability | 不属于 shutdown episode |

## SYSTEM-POWER-EPISODE-001

**规则标题：** `power` 唯一发布不可回滚的 terminal episode。

**规则：** 第一次成功 publication 原子固定 executor CPU、`PowerOff | Reboot` intent 与初始 orderly
phase。后续 ordinary 请求不能替换 executor/intent、不能运行 subsystem plan，并在屏蔽本地中断后
不可返回停止。winning orderly executor 自身 panic 时，可以把同一 publication 原地切换到 emergency
并保留 intent；其它 CPU 的 panic、recursive emergency，以及 machine action 内的 panic 都只能本地
halt，不建立第二次 election、takeover 或 reset-to-running 路径。

episode phase 只由 `power` 的单一原子 truth 表达；handler list、panic owner 与 subsystem 不缓存或
推进另一份全局状态。`StopExecution` 只是 publication 后的 best-effort transport，不是 episode
commit point 或 remote completion barrier。

**违反表现：** power-off/reboot/panic 各自拥有 terminal flag；loser 重跑 callback；winner panic 改写
intent或选出第二 executor；halt 仍允许 scheduler/interrupt work 继续。

**验证 / Enforcement：** production `TerminalEpisode` focused KUnit 覆盖 first-publication、intent
冻结、orderly-to-emergency 与 recursive/machine terminal phase；RV64 KUnit 257/257 通过。RV64/LA64
release build 与全入口 source audit覆盖同一 production type，无 global reset hook。

**最初来源：** [System Power RFC R0](../../rfcs/system-power/index.md)。

**当前来源：** [2026-07-26 System Power transaction](../../devlog/transactions/2026-07-26-system-power.md)。

## SYSTEM-POWER-ORDERLY-001

**规则标题：** Orderly winner 执行显式、单次、fail-forward 的静态 shutdown plan。

**规则：** `power_off()` 与 `reboot()` 只提供 intent，并进入同一个 orderly executor。publication 后、
第一个 callback 前，winner 发起一次 best-effort `IpiPayload::StopExecution` broadcast；发送失败记录后
继续。全局 participant 是 `power` 中源码固定的 `filesystem -> device` function array，machine action
是数组后的终止点。callback 返回后无条件进入下一 step；没有 retry、rollback、timeout 或替代 executor。

filesystem owner 一次 snapshot anonymous + visible mount tree 的 superblock，并按 `Arc` identity 跨树
去重。每个 superblock 一次 snapshot `indexed + ghosts` resident inode，释放 cache lock 后逐项调用
`sync_inode`；单项失败记录并继续，随后仍调用该 filesystem 的 `sync_fs`。snapshot 后新 load/redirty
不追赶。device owner继续独占 child-before-parent traversal；storage flush 只由 concrete driver 在自己的
callback 中承担，`power` 不遍历 filesystem cache、block backend 或 device tree。

**违反表现：** runtime registry/priority 隐藏全局 participant；device 在 filesystem attempt 前关闭；
单项失败跳过后续 step/machine action；持 inode-cache lock 调 backend；重复 snapshot 追赶 dirty set。

**验证 / Enforcement：** source audit确认静态 literal、无条件循环推进、跨树去重和 lock-before-callback
释放。RV64 orderly 两次启动在 open fd、无 `fsync` 条件下观察 `filesystem -> device -> PowerOff machine`
顺序，第二次复用同一磁盘读回 marker；该证据只覆盖一次 ext4/VirtIO best-effort attempt，不是 strong
durability。RV64/LA64 release build 均通过。

**最初来源：** R0 前的直接 `filesystem -> device -> machine` baseline。

**当前来源：** [System Power RFC R0](../../rfcs/system-power/index.md) 与
[cutover transaction](../../devlog/transactions/2026-07-26-system-power.md)。

## SYSTEM-POWER-EMERGENCY-001

**规则标题：** Panic/emergency 跳过 ordinary subsystem plan并直接进入共享 machine helper。

**规则：** 没有 episode 时，panic CPU 可以以 `PowerOff` intent 成为 emergency executor；winning
orderly executor panic 时原地切换并保留已冻结 intent。winner 屏蔽本地中断、best-effort 广播
`StopExecution`、打印 panic/backtrace，然后直接调用与 orderly 相同的 machine-action helper。
allocation/send 失败只记录，不能主动跳过 machine-action attempt。

emergency 不调用 `power_off()` / `reboot()` ordinary entry，不运行 filesystem/device callback，不等待
worker，也不触发 reclaim/eviction。recursive emergency、非 executor panic 或 machine handler panic
直接进入最小本地 halt；当前有锁 handler registry 不承诺 panic-safe progress。

**违反表现：** panic 复用 ordinary shutdown；emergency 取得 filesystem/device 普通 lifecycle 能力；
IPI send 失败直接放弃 machine attempt；另建绕过共享列表的 architecture emergency 通道。

**验证 / Enforcement：** RV64 validation-only boot panic 显示 panic diagnostics 后直接出现共享
`PowerOff machine action`，没有 filesystem/device step，QEMU exit 0；probe 已删除。focused KUnit
覆盖 orderly winner 原地切换和 recursive emergency。IPI failure 后继续由 source control flow 证明，
不扩大为锁竞争或任意 handler 下的 progress 证明。

**最初来源：** [System Power RFC R0](../../rfcs/system-power/index.md)，替换 panic 复用 ordinary
power-off 的旧规则。

**当前来源：** [2026-07-26 System Power transaction](../../devlog/transactions/2026-07-26-system-power.md)。

## SYSTEM-POWER-MACHINE-001

**规则标题：** 两个共享 handler list 按 frozen intent 选择，并永久以唯一 halt 收尾。

**规则：** `power` 分别拥有 power-off 与 reboot capability list。普通注册只 append 到 ordinary handler
序列；每个 list 结构内各有且仅有一个不可注册、不可移除的 `HaltHandler`，在 ordinary handler 全部
返回后作为永久末项执行。ordinary handler 保持注册顺序；返回表示当前 capability 未终止机器，必须
继续下一项。orderly 与 emergency 都只通过同一个不包含 subsystem cleanup 的 helper，按 episode 已
冻结 intent 选择对应 list。

RV64 bootstrap 为两个 list 注册同一个 SBI capability；`system_reset` 返回时记录结果并正常返回 list，
从而继续下一 handler或末尾 halt。LA64 当前没有 ordinary machine handler，两个 intent 都自然到达
末尾 halt；这不构成 LA64 power-off/reboot capability。

**违反表现：** halt 位于 ordinary handler 前或 list 外；注册可插到 halt 后；handler 改写 episode；
orderly/emergency 使用不同 registry；SBI 返回被升级为 panic并重试 machine path。

**验证 / Enforcement：** list type/source audit证明结构性唯一末尾 halt和 handler-return continuation；
全仓 source audit只发现 RV64 SBI registration。RV64 orderly/emergency power-off QEMU 正常退出；reboot
只覆盖 frozen-intent KUnit、共享 helper 与 SBI registration source，未运行 runtime。LA64 release build
通过，但 power-off/reboot 均为 `Not Cut Over`。

**最初来源：** 既有双 handler list 与 RV64 SBI registration。

**当前来源：** [System Power RFC R0](../../rfcs/system-power/index.md) 与
[cutover transaction](../../devlog/transactions/2026-07-26-system-power.md)。

## 当前限制与 architecture coverage

- callback、remote lock、handler-list lock 或 handler 自身可以无限不前进；没有 timeout、takeover、
  cancellation、watchdog 或 emergency-safe handler guarantee。
- `StopExecution` 不等待 remote completion；一次 filesystem snapshot不追赶新 load/redirty，共享 writable
  mapping 和并发 userspace 不形成 stable dirty frontier。
- RV64 power-off 有 orderly/emergency QEMU evidence；RV64 reboot 未运行 runtime。LA64 power-off/reboot
  没有 ordinary machine capability并明确 `Not Cut Over`，末尾 halt 只保证本地不返回。
- VirtIO block callback尝试 flush；SD memory endpoint 没有异步待排空 queue；AHCI 与 DW-MSHC 仍明确
  报告 shutdown/quiesce unsupported。以上不能被描述为全 backend durability。

长期可扫描的退出条件见 [current limitations](../../register/current-limitations.md)。
