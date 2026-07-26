# System Power 目标与不变量

**状态：** Accepted Target / Implemented
**最后更新：** 2026-07-26
**父 RFC：** [RFC-20260726-system-power](./index.md)
**适用修订：** `R0`

> 本文保存 R0 accepted target、owner、跨 subsystem handoff 和 RFC-local proof obligations。
> R0 已实现；`SP-*` 仍是 RFC-local label，生效后的 current truth 使用 `SYSTEM-POWER-*` contract ID。

## 规则分类

- **Correctness Invariant：** 唯一 owner、单向 episode、orderly/emergency 隔离、handoff 顺序和
  no-rollback 等违反即导致双重真相源、不可安全终止或错误 teardown 的规则。
- **Target Guarantee / Capability：** 静态 orderly plan、同步 best-effort writeback、错误可观测和
  machine-action attempt 等本 Draft 提议交付的能力。
- **Acceptance / Scope Boundary：** 明确排除 userspace lifecycle、强 durability 和可逆 lifecycle
  等会改变 RFC 接受范围的能力；不能由实现阶段静默扩入。
- **Implementation Preference：** Rust 类型、callback signature、数组形状、snapshot 容器、内部
  helper、日志格式和文件拆分；这些不在本文冻结。

## Contract Impact

promotion preflight 从当时的 live `power`、panic、filesystem、device 与 RISC-V bootstrap owner 提取
minimum effective baseline；R0 cutover 按以下真实 delta 原子更新 current contract：

| Contract ID | 变化 | 当前规则 | Target 摘要 | 生效 Gate |
| --- | --- | --- | --- | --- |
| `SYSTEM-POWER-EPISODE-001` | Introduce | [Active](../../contracts/power/shutdown-lifecycle.md#system-power-episode-001) | 唯一 publication、first winner、固定 executor/intent 与单向 terminal episode | 2026-07-26 single-stage cutover |
| `SYSTEM-POWER-ORDERLY-001` | Refine | [Active](../../contracts/power/shutdown-lifecycle.md#system-power-orderly-001) | best-effort CPU stop、显式静态 plan、resident inode snapshot writeback 与 fail-forward handoff | 2026-07-26 single-stage cutover |
| `SYSTEM-POWER-EMERGENCY-001` | Replace | [Active](../../contracts/power/shutdown-lifecycle.md#system-power-emergency-001) | panic/emergency 跳过 ordinary subsystem plan，按冻结 intent 进入共享 machine helper | 2026-07-26 single-stage cutover |
| `SYSTEM-POWER-MACHINE-001` | Refine | [Active](../../contracts/power/shutdown-lifecycle.md#system-power-machine-001) | 两个列表永久以唯一 halt 收尾，并由 orderly/emergency 共用同一执行入口 | 2026-07-26 single-stage cutover |

四个 ID 已作为同一个 cutover unit 生效。`SP-*` 继续只标识本 RFC 的 target/proof，不得被后续 RFC
当作 effective contract ID；跨 RFC 依赖必须引用 current contract 的 `SYSTEM-POWER-*` ID。

## Target Invariants

### SP-EPISODE-001 — `power` 唯一拥有全局 terminal episode

**分类：** Correctness Invariant

**规则：** `power` 是全局 shutdown episode publication、全局 step progression、orderly/emergency
仲裁和 machine action invocation 顺序的唯一 owner。其它 subsystem 只能拥有局部 shutdown 状态、
capability 和 outcome，不得发布、缓存或推进另一份全局 phase truth。

episode 一旦发布即为单向、不可回滚的 terminal process；任何普通路径不得把全局状态恢复为运行态。
第一个成功 publication 的发起者成为唯一 executor，publication 同时冻结 power-off/reboot machine
intent。后续 orderly、reboot 或 emergency 请求不能替换 executor/intent、不能重跑 plan，也不能返回
普通执行。只有 winning orderly executor 自身 panic 时，才允许同一 episode 原地切换为 emergency；
该切换不是第二次 publication 或第二轮 election。executor 在 emergency 内再次 panic 时必须直接
final halt，不重跑诊断、stop broadcast 或 machine fallback。

**违反表现：** `power_off()`、`reboot()`、panic 或 subsystem 分别维护 episode；重复请求重跑 plan；
callback 直接推进下一全局 phase；machine action 可绕过全局 owner。

**依赖：** None。

### SP-PLAN-001 — Orderly plan 静态、显式且顺序固定

**分类：** Correctness Invariant + Target Guarantee

**规则：** orderly shutdown 的全局 participant 由 `power` 中编译期固定、源码显式可见的静态 callback
plan 编排。数组字面顺序是唯一全局 review surface：

```text
publish shutdown
  -> best-effort StopExecution broadcast
  -> kernel-owned producer quiesce / admission closure, if a listed owner provides one
  -> filesystem page / inode writeback
  -> filesystem cache / journal commit
  -> block / storage drain and cache flush
  -> device shutdown
  -> machine action
```

callback 只调用一个窄的 subsystem facade。filesystem/device 等全局 participant 的 runtime
registration/unregistration、数字 priority、linker section 自动收集、implicit link order 和 configurable
dependency graph 均禁止。既有 power-off/reboot handler 列表只提供最终 machine-action capability，
不属于全局 participant plan。

`StopExecution` broadcast 在第一个 ordinary callback 前只发起一次。它不是 synchronous completion
barrier；发送失败被记录后继续，已经停住的 CPU 可能遗留 lock/worker 并使后续 callback 卡死。本
target 不从该 primitive 推导“所有 userspace 已确认停止”或“ordinary callback 一定能完成”。

**违反表现：** filesystem/device participant 通过 initcall 顺序插入 plan；subsystem callback 隐式调用
另一 global step；review 无法从一处源码确认参与者和顺序；emergency 执行该 plan。machine-action
handler 保持注册顺序不构成本条违反。

**依赖：** SP-EPISODE-001。

### SP-HANDOFF-001 — Producer 先停，provider 在 consumer 排空后关闭

**分类：** Correctness Invariant

**规则：** 上游 producer-side quiesce 只能关闭新的 owner-local production 并排空已经进入的访问；
consumer 完成前，下游 provider 必须保留完成该 handoff 所需的 I/O、completion、IRQ、timer、queue
和其它 capability。不存在所有 subsystem 同时进入同一种 `quiesced` 状态的全局 barrier。

**违反表现：** filesystem writeback 前关闭 block admission、IRQ 或 queue；device reset 发生在 storage
flush 前；把 subsystem-local quiesced 当成全局 phase truth。

**依赖：** SP-PLAN-001。

### SP-FAIL-001 — 普通可返回失败只允许 fail-forward

**分类：** Correctness Invariant + Target Guarantee

**规则：** 每个可返回的 orderly step 最多执行一次。失败 outcome 被 `power` 和 local owner 记录后，
全局 plan 继续后续有限 best-effort attempt 并最终尝试 machine action。不得重试该 step，不得恢复
admission、重启 worker、重新开放 provider、执行补偿事务或回滚 episode。

后续 facade 不能把此前所有 step 成功作为内存安全或可调用性的前提；它必须能安全拒绝、跳过不支持
的局部动作，或在剩余状态上完成有限 attempt。

**违反表现：** flush 失败后重新进入运行态；局部失败中止 plan 并跳过 machine action；callback 被
自动重跑；rollback 与并发 terminal path 形成双重 owner。

callback non-return 或无限等待会使 orderly shutdown 保持卡死；不提供 timeout、cancel、watchdog 或
替代 executor。winning executor 的 callback panic 直接把同一 episode 切换到 emergency，并放弃剩余
ordinary callback；这不是普通返回失败，也不受 fail-forward 继续规则约束。

### SP-EMERGENCY-001 — Emergency 跳过 ordinary subsystem plan

**分类：** Correctness Invariant + Acceptance / Scope Boundary

**规则：** panic/emergency path 不执行任何 ordinary subsystem callback，不等待普通 worker，不要求
filesystem/storage flush、reclaim 或 owner-local teardown。没有 episode 时，panic 可以通过同一个
publication point 成为 executor；winning orderly executor panic 时原地切换；其它 CPU 或后续请求只
能不可返回停止。无先行 episode 的 panic 以 power-off 作为 publication intent；从 orderly 原地
切换时保留已冻结的 power-off/reboot intent。

现有 `StopExecution` IPI transport 会为 message 做 best-effort allocation；该 allocation/send 可以失败，
且失败不得主动跳过 machine-action attempt。停止与诊断后，emergency 根据已冻结 intent
直接进入与 orderly 共用的 machine-handler 列表执行入口，不调用包含 filesystem/device cleanup 的
ordinary `power_off()` / `reboot()` 入口。

本 target 不要求当前列表锁或注册 handler 具有 emergency-safe / lock-free 保证。列表锁、handler
内部锁、睡眠或其它不前进可以使 emergency 停滞；末尾 halt 只保证前序遍历能继续时的
不返回收口。无锁列表、handler safety contract 与平行架构 machine-action 通道均不属于本 RFC。
只有 emergency 内再次 panic 或竞争失败者可以直接进入最小 CPU halt，不把它扩展为第二套
power-off/reboot capability。

**违反表现：** panic 调用 ordinary `power_off()`；stop IPI 后取得 filesystem/device 普通锁；等待
kthread drain；emergency 运行 ordinary subsystem plan；为 emergency 建立绕过两个 handler 列表的平行
machine power-off/reboot 通道。

**依赖：** SP-EPISODE-001。

### SP-LOCAL-001 — Subsystem 只拥有局部 quiesce

**分类：** Correctness Invariant

**规则：** subsystem 只拥有自己的 admission、timer/wake、worker、in-flight access 和局部 terminal
state。它可以停止新的 kernel-owned production、撤销自己发起的新 work，并按 owner-local 协议排空
已经进入的访问；它不能拥有全局 episode 或协调其它 subsystem 的 global phase。

同一 owner 只有在跨 subsystem handoff 必须全局可见时，才向静态 plan 暴露多个 phase-specific 窄
facade。内部 traversal 和局部顺序仍由 subsystem 独占。

**违反表现：** filesystem callback 直接关闭 device；driver 推进 global phase；`power` 遍历 subsystem
private object/lock；局部 owner 保存 global phase mirror。

**依赖：** SP-EPISODE-001、SP-HANDOFF-001。

### SP-FLUSH-001 — Shutdown flush 是分层同步 best-effort attempt

**分类：** Target Guarantee

**规则：** orderly filesystem facade 对 shutdown 时可达的 resident file data 与 inode metadata 发起
一次 owner-local snapshot，并对 snapshot 做一轮同步 writeback attempt，再提交 filesystem
cache/journal；之后 block/storage 在 device owner 的一次 traversal 中排空已提交 I/O并尝试 flush
volatile device cache。snapshot 之后新出现或再次变脏的 filesystem 对象不追赶、不重扫。该路径不
通过 eviction、reclaim、强制 close 或无界重复 writeback 完成。

flush 必须错误可观测，但不承诺在并发 userspace mutation、shared writable mapping、backend 不支持
flush 或局部失败时形成完整 durability guarantee，也不以 dirty set 收敛为空作为完成条件。

**违反表现：** 只 flush filesystem block cache 却声称 page data 已写回；driver shutdown 才第一次
处理 dirty filesystem state；用无限循环追逐 concurrent dirty；把 unsupported backend 记录为成功。

**依赖：** SP-HANDOFF-001、SP-FAIL-001、SP-USER-001。

### SP-DEVICE-001 — Device shutdown 保持 owner-local

**分类：** Correctness Invariant

**规则：** 全局 plan 只进入 device subsystem facade；device subsystem 继续独占 device tree、
child/parent、bus/driver traversal 和局部 ordering。driver callback 只处理 owner-local IRQ、queue、DMA、
reset、hardware quiesce 与能够证明安全的回收，不拥有 global phase 或 filesystem/storage 协调。

无法证明安全释放的 resource 可以保留到 reset/power-off，不能为了“清理完整”提前释放仍可能被
hardware、IRQ 或 completion 访问的 backing。

**违反表现：** `power` 复制 device tree traversal；driver shutdown 反向调用 filesystem flush；
device callback 发布 global phase；未 quiesce DMA 就释放 backing。

**依赖：** SP-HANDOFF-001。

### SP-MACHINE-001 — 共享 handler 列表保留能力注册，永久以 halt 收尾

**分类：** Correctness Invariant + Target Guarantee

**规则：** `power` 继续拥有独立的 power-off 与 reboot handler 列表。episode publication 已冻结的
machine intent 唯一决定遍历哪一个列表；handler 不拥有 episode、step 或 intent。registered handler 按
注册顺序尝试，正常返回表示未终止机器，随后继续下一个。

每个列表始终各有且仅有一个内建 `halt` handler，并且该 handler 永久位于列表末尾且永不返回。
platform/driver handler 注册时必须插入末尾 fallback 之前，因此既有普通 handler 的相对注册顺序保持
不变。列表不提供 unregister、priority 或第二份 fallback ordering。

两条 terminal path 共用唯一个不包含 subsystem cleanup 的内部列表执行入口。orderly winner 在
subsystem plan 完成后访问；emergency winner 跳过该 plan 后直接访问。竞争失败者和 subsystem
callback 不得调用列表。本条不把列表锁或 handler 语义提升为 emergency-progress guarantee。

**违反表现：** 默认 halt 位于普通 handler 之前；后注册 handler 被放到 halt 之后；列表可以为空或
包含多个 halt fallback；handler 改写 intent/episode；orderly 与 emergency 分别使用两套 machine-action
capability 或不同的列表顺序。

**依赖：** SP-EPISODE-001、SP-PLAN-001、SP-EMERGENCY-001。

### SP-USER-001 — Userspace 不参与 kernel shutdown lifecycle

**分类：** Acceptance / Scope Boundary

**规则：** system-power 不枚举、不冻结、不终止、不等待用户任务，也不把 userspace 协作、退出或
继续获得调度作为 episode 的前提或保证。subsystem 可以关闭 owner-local admission，但这只决定到达
该边界的新请求 outcome，不改变 task lifecycle。

本 RFC 不遍历、write-protect 或 revoke shared writable mapping，不建立 userspace-wide dirty
frontier。admission 关闭后的具体 errno/outcome 必须由受影响 subsystem 与 ABI review 决定，不能由
global power state 猜测。

**违反表现：** `power` 等待用户进程退出；引入 freezer/task killer；用 user task count 判断 flush
完成；为了 strong durability 扫描或撤销全部 shared mapping。

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| global episode / executor / machine intent / current global step | `power` | 只读 snapshot 或 callback invocation context | terminal arbitration 与全局顺序 |
| subsystem admission / local quiesce | 对应 subsystem | 请求 capability、local outcome | 关闭新 production、排空已进入访问 |
| resident page data / inode metadata | filesystem / inode owner | writeback invocation | 文件数据与 metadata attempt |
| filesystem cache / journal | concrete filesystem | sync/commit invocation | 向 block layer 提交 filesystem state |
| block queue / storage flush capability | block/storage owner | filesystem I/O capability | 排空 I/O、flush volatile cache |
| device tree / driver binding order | device subsystem | 单一 device shutdown facade | child/bus/driver 局部 traversal |
| IRQ / queue / DMA / hardware state | concrete driver/controller | device-local callback context | mask、drain、reset、quiesce |
| power-off/reboot handler lists | `power` | platform/driver 注册 machine-action capability | orderly/emergency 按冻结 intent 顺序尝试，永久末尾 halt |
| recursive-emergency / loser CPU halt | `power` | 不提供 machine-action capability | 避免再次诊断或 election，只保证当前 CPU 不返回 |
| userspace task/mapping state | task/mm 原 owner | system-power 不取得 lifecycle capability | 明确不由 shutdown episode 管理 |

诊断用 step label、错误计数和 callback name 不得反向驱动 owner state；若实现缓存它们，必须明确只
服务 log/review，不能成为第二份 global phase truth。

## 跨 subsystem handoff

| Handoff | 前一 owner 的局部义务 | 后一 owner 必须保留/执行 | 失败规则 |
| --- | --- | --- | --- |
| publish -> producer quiesce | `power` 发布单向 episode | subsystem 关闭新 production 并有限排空 | 记录后继续，不回滚 publication |
| producer -> filesystem writeback | producer owner 返回 local outcome | filesystem 仍可访问 resident state 与 block I/O | 即使前序失败也做安全有限 attempt |
| page/inode -> filesystem commit | filesystem 写回可达 data/metadata | concrete fs flush cache/journal | 失败可见，继续 storage attempt |
| filesystem -> storage | filesystem commit attempt 已返回 | storage drain 已提交 I/O并尝试 cache flush | 不反向重跑 filesystem |
| storage -> device | storage attempt 已返回 | device 做 owner-local shutdown | 不把首次 filesystem writeback拖入 driver |
| device -> machine | device traversal attempt 已返回 | `power` 按冻结 intent 遍历对应 handler list | handler 返回则继续，末尾 halt 不返回 |
| emergency -> machine | emergency winner 跳过 subsystem plan | `power` 按冻结 intent 进入同一 handler list | best-effort attempt；锁/handler 停滞不触发 takeover |

具体 callback signature、unit/error 返回形状、snapshot 容器、handler 容器和共享列表执行 helper 的
Rust 表示属于 implementation preference；它们不得改变上述 owner、单次遍历与失败语义。

## 线性化与生命周期边界

- **Episode publication：** 唯一 commit point 以 first-successful-publication 冻结 executor 与 machine
  intent；失败者和所有后续请求进入不可返回停止。
- **Step attempt：** `power` 是 step started/returned/failed 的唯一全局记账者；subsystem 只发布局部
  outcome，不自行改变 current global step。
- **Fail-forward：** 普通 callback 返回 error 后，全局 owner 记录并推进下一 step；此前局部状态不
  rollback。
- **Emergency ownership：** 没有 episode 时 panic 可以成为 winner；只有 winning orderly executor
  panic 才能把同一 episode 原地切到 emergency。切换后不再启动 ordinary callback。
- **Terminal completion：** 成功 machine action 不返回；所有 machine action 失败后进入不可返回 halt。

ordinary callback 沿用各 owner 当前 lock/wait 规则；本 RFC 不新增 timeout/cancellation。emergency
路径不进入 ordinary subsystem callback/worker，但明确沿用当前 machine-handler registry、列表锁与
handler 本身的依赖。本 target 只承诺进入 best-effort machine-action attempt，不承诺 panic-safe progress。

## RFC-local Proof Obligations

### SP-PROOF-001 — 全局 owner 与旁路审计

所有 power-off、reboot、panic 和 platform machine-action 入口都必须被分类；不得存在能绕过 `power`
episode、直接重跑 orderly plan 或直接从普通 subsystem callback 执行 machine action 的旁路。

### SP-PROOF-002 — Orderly 顺序与 capability 保留

验证必须证明 filesystem page/inode writeback、filesystem commit、storage drain/flush、device shutdown
和 machine action 的观察顺序，并证明 consumer 完成前必要 provider capability 未被关闭。

### SP-PROOF-003 — Failure injection 不触发 rollback

对每个可返回 step 注入失败，必须观察到一次诊断、后续 step 继续、该 step 不重试、已发布状态不恢复，
并最终到达 machine action 或明确的 final halt evidence。

### SP-PROOF-004 — Dirty data 与 unsupported backend 结论诚实

至少覆盖 shutdown 前 resident dirty file data 的 writeback/commit/storage attempt，并区分 backend flush
成功、明确不支持和失败。测试只能证明对应 attempt 和可观测结果，不能从单次 remount 成功推出并发
userspace 或所有 hardware 上的完整 durability。

### SP-PROOF-005 — Emergency 不执行 ordinary callback

panic/emergency evidence 必须证明 ordinary static subsystem plan、filesystem sync、device
traversal、worker wait、ordinary cleanup entry 均未被调用；还必须覆盖 winning orderly executor
panic 的原地切换，以及 IPI allocation/send failure 后仍尝试进入共享 machine-handler 执行
入口。证据不得把进入该入口写成列表锁、handler 或最终 machine action 必然前进。

### SP-PROOF-006 — Userspace 非管理边界

source audit 必须确认 `power` 不枚举 task/mm、不等待用户退出、不调用 freezer/task-kill、不遍历或
revoke shared mapping。subsystem admission outcome 的验证不能被写成 userspace lifecycle 证明。

### SP-PROOF-007 — Machine handler fallback 与共享入口

验证必须证明 power-off/reboot 两个列表各自恰有一个永久末尾 halt handler，普通 handler 保持注册
顺序并插在 halt 之前；前序 handler 返回时会到达下一个，空平台能力也会到达 halt。orderly 和
emergency 必须按各自已冻结 intent 进入同一执行 helper 与同一列表顺序，emergency 不得转入
filesystem/device plan。测试只证明可控 fixture 中的遍历，不扩大为生产 handler 的 panic-safety 证明。

## 禁止退化项

- 不得为 orderly、reboot 和 panic 各维护一份 global episode 或 plan progress；
- 不得用 callback registry、priority、link order 或 dependency graph 取代显式静态 subsystem plan；
- 不得让 machine handler 列表拥有 episode/step/intent、移除唯一末尾 halt、把普通 handler 放到 halt
  之后，或为 emergency 建立平行的 machine power-off/reboot capability；
- 不得让普通 callback 调用其它 global step、推进 phase 或执行 machine action；
- 不得在 consumer 排空前关闭下游 provider capability；
- 不得把可返回失败升级为 rollback/retry，或静默跳过后续 machine action；
- 不得让 emergency 复用 ordinary subsystem callback、filesystem/device cleanup 或 worker wait；
- 不得把共享 handler 列表、列表锁或注册 handler 的当前状态写成 emergency-safe 或必然前进；
- 不得把 `StopExecution` broadcast 写成同步 barrier、remote-lock cleanup 或 userspace completion proof；
- 不得通过 eviction/reclaim、强制 close 或无界 writeback loop 实现 shutdown flush；
- 不得把 best-effort attempt、单一 backend 成功或单次 QEMU shutdown 写成完整 durability；
- 不得建立 userspace freezer、task killer、cooperation protocol 或 shared-mapping revoke；
- 不得让 `power` 接管 filesystem/storage/device 内部 object traversal，或让局部 owner缓存 global phase。

## 已闭合决定与实现自由度

episode winner、reboot intent、orderly-to-emergency 切换、无 timeout、单次 snapshot、共享
machine handler fallback、emergency 跳过 subsystem plan 与 contract cutover 已在本文闭合。实现阶段可以选择最小的
atomic encoding、静态 subsystem 函数数组、snapshot/handler 容器和日志形状，但不能新增 takeover、
timeout、retry、第二轮 snapshot、全局 participant registry 或平行 machine-action surface。当前没有需要
`tracking-issues.md` 独立
跟踪的 confirmed design finding。

## 完成标准

R0 acceptance 时的文档层完成标准为：

- `SP-EPISODE-001` 到 `SP-MACHINE-001` 以及 `SP-USER-001` 的 owner、handoff、failure 和禁止退化项
  通过 review；
- 上述设计决定、minimum effective baseline 与真实 Contract Impact 已闭合；
- target guarantee、correctness invariant、explicit non-goal 和 implementation preference 没有混写；
- 单一 Ready implementation stage 已解析；tracking 文件只在 confirmed finding 出现后创建。

RFC 最终 closure 至少要求：

- `SP-PROOF-001` 到 `SP-PROOF-007` 均有可审计 evidence；
- orderly 与 emergency 在支持 architecture 上各自达到其 acceptance floor；
- 每个受影响 contract ID 都有 effective、pending 或 Not Cut Over 结论；
- transaction、register/current limitations 与 RFC 状态完成一致收口；
- 未实现的 general writeback、strong durability、userspace lifecycle 和 reversible device lifecycle 仍
  保持在本 RFC target 之外。

上述 closure 已于 2026-07-26 完成。逐项 production/source/runtime evidence、architecture coverage 与
Not Run 边界见 [transaction](../../devlog/transactions/2026-07-26-system-power.md)；生效规则见
[System Power 当前契约](../../contracts/power/shutdown-lifecycle.md)。
