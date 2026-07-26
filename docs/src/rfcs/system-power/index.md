# RFC-20260726-system-power

**状态：** Closed
**修订：** `R0`
**负责人：** doruche
**最后更新：** 2026-07-26
**领域：** system power / panic / filesystem / storage / device lifecycle
**事务日志：** [2026-07-26-system-power](../../devlog/transactions/2026-07-26-system-power.md)
**影响契约：** [`SYSTEM-POWER-EPISODE-001`](../../contracts/power/shutdown-lifecycle.md#system-power-episode-001)（Introduce）；
[`SYSTEM-POWER-ORDERLY-001`](../../contracts/power/shutdown-lifecycle.md#system-power-orderly-001)（Refine）、
[`SYSTEM-POWER-EMERGENCY-001`](../../contracts/power/shutdown-lifecycle.md#system-power-emergency-001)（Replace）、
[`SYSTEM-POWER-MACHINE-001`](../../contracts/power/shutdown-lifecycle.md#system-power-machine-001)（Refine）
**开放问题：** None；callback 卡死、StopExecution best-effort、machine-handler 遍历可能
不前进与单次 snapshot coverage 是已接受边界
**下一步：** None；后续能力扩展或 limitation 收敛由独立 owner/RFC gate 承担

> 本目录保存 `system-power` R0 accepted target 与迁移历史。R0 已由单一 stage 完成实现；四个
> contract ID 的 effective current truth 统一由
> [System Power 当前契约](../../contracts/power/shutdown-lifecycle.md) 维护。

## 摘要

R0 接受前的内核把正常关机、重启和 panic 后的掉电收敛到相近的直接调用链，但没有一份由 `power`
唯一拥有的全局 shutdown episode，也没有把 producer quiesce、filesystem writeback、storage
flush、device shutdown 与 machine action 建模为明确的单向 handoff。panic 在停止其它 CPU 后仍会
进入普通 filesystem/device shutdown；正常 shutdown 则可能在 resident dirty page 尚未进入
filesystem block cache 时直接开始 device traversal。

R0 由 `power` 唯一拥有全局 terminal episode 和跨子系统顺序，建立彼此独立的 orderly 与
emergency 路径。第一个成功发布 episode 的发起者成为唯一 executor；orderly shutdown 先通过
`StopExecution` IPI 尽可能停止其它 CPU，再使用源码中显式、编译期固定的静态 callback plan，沿依赖
方向执行一次 filesystem page/inode writeback snapshot、filesystem commit、storage drain/flush、
device shutdown 和 machine action；可返回失败只记录并继续，episode 从不回滚。emergency 路径不
执行普通 subsystem callback，不等待普通 worker，在 stop/mask 与诊断后直接按已冻结
intent 进入同一个 machine-handler 列表执行入口。本阶段接受列表锁或 handler 自身阻塞使
emergency 不前进，不为此预建平行的架构 machine-action 抽象。

## 背景与当前基线

### 当前 power 路径没有 episode owner

`anemone-kernel/src/power.rs` 当前分别实现 `power_off()` 与 `reboot()`。两条路径都直接调用
`fs::on_shutdown()`、`device::shutdown()`，随后在各自的动态 `Vec<Box<dyn ...Handler>>` 中顺序尝试
machine handler；没有全局 episode 状态、重复请求仲裁、orderly/emergency 竞争裁决或统一的阶段
观测。

当前 native `SYS_POWER_SHUTDOWN` 使用 magic value 进入不返回的 `power_off()`。RISC-V bootstrap
动态注册 SBI power-off 和 cold-reboot handler；LoongArch 当前没有同等的通用 handler 注册事实。
本 RFC 不以这些偶然注册顺序定义新的全局 callback plan，也不在 Draft 阶段改变现有 syscall ABI。

### 当前 panic 错误复用 ordinary shutdown

`anemone-kernel/src/panic.rs` 当前关闭本地中断、异步广播 `StopExecution` IPI、打印 panic 和
backtrace，随后调用普通 `power_off()`。这使 panic 路径在其它 CPU 已经停止、本地中断关闭、普通锁
可能被遗留持有的条件下继续执行 filesystem sync、device traversal 和动态 machine-handler registry。

目标 emergency 路径必须切断对 filesystem/device ordinary plan 的复用，但不另建一套 machine
power-off/reboot 机制。panic 的诊断输出和 CPU stop primitive 保留原 owner；最终 machine action
与 orderly 共用两个 handler 列表及其末尾 halt。

### 当前 filesystem shutdown 没有闭合 resident page writeback

`fs::on_shutdown()` 当前分别枚举 anonymous 与 visible mount tree 的 superblock，并只调用 filesystem
`sync_fs`。ext4 的 `sync_fs` 只 flush lwext4 block cache；resident regular-file page 已经具有 dirty
tracking 和逐 inode `sync_all()`，ext4 inode sync 也会写 page data 与 metadata，但 shutdown 没有遍历
这些 resident inode。

block-device trait 当前只提供同步 read/write，没有统一 flush facade。VirtIO block 把 device cache
flush 放在 driver shutdown callback 中；SD memory 当前依赖同步 command completion；AHCI 的明确
cache-flush durability contract 和完整 shutdown 仍是已登记限制。因此现状不能被描述为已经提供完整
filesystem/storage durability。

### Device subsystem 已有局部遍历 owner

`device::shutdown()` 当前对 device tree 做 child-before-parent 的深度优先遍历，再调用各 device 的
driver `shutdown()`。该 traversal 是 device subsystem 的 owner-local 机制，本 RFC 保留这一边界。
具体 driver 的现状不一致：部分 callback 为空，VirtIO block 尝试 flush，AHCI 和 DW-MSHC 仍保留
明确的 staged limitation。

本 RFC 不让 `power` 复制 device tree、bus 或 driver ordering，也不把当前未实现的 driver cleanup
伪装成 system-power 已经提供的能力。

## 目标

- 由 `power` 唯一拥有一次全局 shutdown episode、全局阶段推进和跨 subsystem 顺序。
- 让第一个成功 publication 的发起者成为唯一 executor；重复请求不改变 executor 或 machine intent。
- 把 orderly 与 emergency/panic shutdown 建立为不复用 subsystem callback plan 的两条终止路径，
  同时共用唯一 machine-handler 执行入口。
- 在 ordinary callback 前发送一次 best-effort `StopExecution` IPI，尽可能停止其它 CPU 上的执行。
- 用显式、编译期固定、可源码 review 的静态 callback plan 编排少量全局 orderly participant。
- 固定 filesystem writeback/commit、storage drain/flush、device shutdown 与
  machine action 的依赖顺序。
- 让每个 subsystem 只拥有自己的 snapshot、queue、DMA、IRQ、traversal 与局部 callback，不形成
  第二份全局 phase truth。
- 为 shutdown 引入一次同步、owner-local snapshot 上的 filesystem resident page/inode writeback
  attempt，随后执行 filesystem commit 与 storage flush attempt。
- 对普通可返回失败执行 log-and-continue：每个 step 最多尝试一次，不重试、不补偿、不回滚，最后
  仍尝试 machine action。
- 保留现有 power-off/reboot handler 列表作为唯一 machine-action 能力集合；`power` 根据
  已冻结 intent 遍历对应列表，handler 返回就继续下一个。两个列表都永久以一个不
  返回的 `halt` handler 收尾，orderly 与 emergency 共用同一个内部执行入口。
- 明确接受 emergency 访问当前有锁列表或调用未证明 emergency-safe 的 handler 时可能不前进；
  无锁列表和 handler safety contract 不在本 RFC target 内。
- 不为 ordinary callback 建立 timeout、cancel、watchdog 或 rollback；callback 卡死会使 orderly
  shutdown 卡死。

## 非目标

- 枚举、冻结、等待或终止用户任务，建立 process freezer，或要求 userspace 协作/退出；
- 保证 userspace 在 shutdown 期间继续运行到某一阶段或继续获得正常服务；
- 强制终止任意 kthread，或建立通用 shutdown task-kill 机制；
- 为 shared writable mapping 建立 shutdown-only write-protect、revoke 或全局 dirty frontier；
- 建立普通运行期 writeback worker、dirty aging/throttling、周期回写或完整 clean/redirty 状态机；
- 顺带实现完整 `fsync`、`fdatasync`、`msync`、同步写 flag 或强 durability ABI；
- 在并发 userspace mutation、panic 或 emergency 路径下承诺完整 filesystem/storage durability；
- runtime suspend/resume、device hotplug、unbind、runtime removal 或 restart/reactivation；
- 接管 filesystem、storage、device、bus 或 driver 内部的 owner-local ordering；
- 让 emergency 路径执行普通 flush、reclaim、teardown 或资源回收；
- 在当前有锁 machine-handler 列表上承诺 panic/emergency 必然前进，或在本 RFC 中顺带
  引入无锁注册结构、emergency-safe handler 类型或平行架构 machine-action 通道；
- 为 filesystem/device 等全局 orderly participant 引入运行时 callback registration、数字 priority、
  linker-section 收集、隐式 link order 或可配置 dependency graph；本项不禁止保留既有 machine-action
  handler 列表。

## 文档地图

RFC target：

- [目标与不变量](./invariants.md)：R0 accepted target、owner、handoff、失败边界和 proof obligations；
- [迁移实施计划](./implementation.md)：已关闭的唯一 stage、resolved write set、验证与 cutover gate；
- `tracking-issues.md`：当前没有经过 design review 确认、需要独立状态跟踪的 finding，因此不创建。

Current contracts：

- [System Power shutdown lifecycle](../../contracts/power/shutdown-lifecycle.md)：四个已生效
  `SYSTEM-POWER-*` ID 的 current authority。

背景材料：

- [定位共识](./backgrounds/positioning.md)：正文形成前的方向讨论和 Draft readiness 结论。

公共外部源码证据：None。R0 只依赖当前仓库 source、register 与已经记录的 project facts。

## 方案

### `power` 拥有唯一 terminal episode

`power` 是全局 episode publication、全局 step progression、orderly/emergency 仲裁和最终 machine
action invocation 的唯一 owner。subsystem 只能拥有局部状态并返回局部 outcome；它不能发布、缓存
或推进另一份全局 shutdown phase，也不能从普通 callback 直接调用其它全局 step 或 machine action。

episode 一旦发布就是单向、不可回滚的终止过程。精确状态表示、重复 orderly 请求、orderly 与
emergency 竞争以及 reboot request 使用同一条 first-publication-wins 规则：第一个成功发布 episode 的
发起者成为唯一 executor，其 machine intent 同时冻结。后续请求不能替换 executor 或 intent，也不能
回到普通执行；它们只能进入不可返回停止。如果 orderly executor 自身在 callback 中 panic，同一
episode 原地切换为 emergency path，不创建第二次 election，也不恢复 orderly plan。
如果该 executor 已在 emergency path 内再次 panic，则跳过普通诊断与 fallback，直接进入 final halt，
避免递归形成第三条控制流。

### Orderly shutdown 使用静态、显式 plan

全局 orderly 顺序固定为：

```text
publish shutdown
  -> best-effort StopExecution broadcast
  -> kernel-owned producer quiesce
  -> filesystem page / inode writeback
  -> filesystem cache / journal commit
  -> block / storage drain and cache flush
  -> device shutdown
  -> machine action
```

上游 producer 先停止产生新的 owner-local state；下游 provider 在 consumer 完成排空前必须继续
提供所需 capability。不存在“所有 subsystem 同时进入同一种 quiesced 状态”的全局阶段。

`power` 直接拥有编译期固定的 callback plan，字面顺序就是 review surface。callback 只暴露窄的
subsystem facade；局部 registry、object、lock 和 traversal 留在 subsystem。只有跨 subsystem handoff
必须全局可见时，同一 owner 才暴露多个 phase-specific facade。

### 普通失败只允许 fail-forward

每个可返回的 orderly step 最多执行一次。step 返回失败时，`power` 记录 step 与 outcome，subsystem
记录 owner-local 诊断，然后继续执行后续 best-effort step。已经关闭的 admission、已经停止的 worker
和已经发布的 shutdown 状态不得恢复；不执行重试、补偿事务或 rollback。

后续 step 不能假定此前所有 step 成功，必须能在其支持的剩余状态上做有限 attempt。callback 永不
返回或无限等待时 orderly shutdown 就保持卡死；本 RFC 不提供 timeout、cancel 或 watchdog。callback
panic 由 panic handler 按 executor 规则把同一 episode 切到 emergency path，不把 panic 包装成普通
错误，也不继续剩余 ordinary callback。

### Filesystem 与 storage 逐层下沉

filesystem facade 对 shutdown 时可达的 resident file data 与 inode metadata 发起一次同步 writeback
attempt，然后提交 filesystem 自己的 cache/journal。该过程不通过 inode eviction、reclaim 或强制关闭
open file 间接完成，也不提前关闭 writeback 所需的 block I/O、completion、IRQ、timer 或 queue。

filesystem commit 返回后，block/storage 才排空已提交 I/O 并尝试 flush volatile device cache；随后
device subsystem 才进入 owner-local queue/DMA/IRQ/hardware shutdown。driver shutdown 不能成为第一次
把 filesystem dirty state 推向 storage 的阶段。

该 flush 是有序、错误可观测的 best-effort attempt，不承诺 dirty set 收敛为空。已经建立的 shared
writable mapping 可以继续直接修改 resident frame；本 RFC 不遍历或管理对应用户任务和映射，也不以
无界重复 writeback 追逐并发修改。

### Emergency path 不进入 ordinary subsystem plan

emergency path 首先确立唯一 emergency executor，使其它 CPU 停止继续执行，再关闭或屏蔽继续执行所需
的本地中断来源并保留 panic 诊断。它不执行静态 orderly subsystem callback plan，不等待普通
worker，不要求 filesystem/storage flush、reclaim 或 owner-local teardown。

`StopExecution` allocation/send 失败只记录并继续。随后 emergency 不调用包含 ordinary cleanup 的
`power_off()` / `reboot()` 入口，而是根据 episode 已冻结 intent 直接进入与 orderly 共用的
machine-handler 列表执行入口。无先行 episode 的 panic 以 power-off 作为发布 intent；从 orderly 原地
切换时保留原 power-off/reboot intent。

当前列表仍使用普通锁，handler 也没有 emergency-safe 类型保证。本 Draft 明确接受列表锁、
handler 内部锁或 handler 自身阻塞使 emergency 停滞；列表内的末尾 halt 只保证“前序遍历能继续时”
最终不返回，不构成 panic progress guarantee。无锁迁移和 handler safety contract 留给后续独立工作。

### Machine-action handler 列表是唯一能力集合

现有 `PowerOffHandler` 与 `RebootHandler` 列表继续由 `power` 持有。它们不发布 episode、不推进
ordinary step，也不决定 machine intent。`power` 只提供一个不执行 filesystem/device cleanup 的内部
machine-handler 执行入口，根据 publication 已冻结的 intent 选择其中一个列表并按注册顺序
尝试。orderly 在 subsystem plan 返回后进入它；emergency 跳过该 plan 后直接进入它。handler 正常
返回表示该 machine action 没有终止机器，随后继续下一个 handler。

两个列表始终各有且仅有一个内建 `halt` handler，并且它永久位于末尾。普通 platform/driver handler
必须插入这个 fallback 之前，因此既有 handler 之间仍保持注册顺序，而列表穷尽必然停在不可返回的
halt。该动态列表是 machine capability discovery，不是全局 subsystem callback plan 或第二份 phase
truth。除 emergency 内再次 panic 或竞争失败者使用的最小不可返回 CPU halt 外，本 RFC 不建立
绕过这两个列表的第二套 power-off/reboot capability。

### Userspace 不属于 shutdown lifecycle participant

内核不枚举、不冻结、不终止、不等待用户任务，也不把 userspace 协作或退出作为完成条件。subsystem
可以根据自己的局部 terminal state 关闭新 admission，但这只决定到达 subsystem 边界的请求结果，
不改变发起请求的 task lifecycle。

本 RFC 同样不保证 userspace 会继续得到调度或正常服务。orderly executor 在 callback 前广播一次
best-effort `StopExecution` IPI；该广播不是完成 barrier，发送失败只记录，已经停住的 CPU 也可能遗留
普通锁或 worker 状态并使后续 callback 卡死。当前 plan 不新增 subsystem admission ABI，因此不统一
发明 shutdown errno；以后若具体 subsystem 新增 admission gate，由该 owner 单独决定可见 outcome。

## 已闭合的设计决定

本 Draft 在进入 implementation review 前采用以下最小结论：

1. episode publication 是唯一线性化点；first publication 同时冻结 executor 与 power-off/reboot intent；
2. orderly 前使用异步 `StopExecution` broadcast 做 best-effort CPU stop，不等待停止确认；
3. callback 卡死就保持卡死，不建立 timeout/cancel/watchdog；winning executor panic 时同一 episode
   原地进入 emergency，其他竞争请求不可返回；
4. filesystem owner 只取一次 resident snapshot；device owner 保持一次 child-before-parent traversal，
   两者都不为追求 completeness 重跑；
5. orderly 与 emergency 共用两个 machine-action handler 列表，按冻结 intent 选择其一；每个列表
   永久以唯一的 `halt` handler 收尾，新增 handler 插在它之前；
6. 本轮不新增 admission gate 或统一 errno；具体 backend 不支持 flush/shutdown 时记录为当前限制；
7. emergency 只跳过 ordinary subsystem plan，然后进入共享 machine-handler 执行入口；列表锁或
   handler 使其不前进是明确接受限制，RV64 与 LA64 分别按真实能力和运行证据得出 Cut Over
   或 Not Cut Over。

## Contract Impact

R0 确认 system-power 会形成跨 `power`、panic、filesystem、storage 与 device 的共享 handoff。
promotion preflight 已把以下 live baseline 提取到
[System Power shutdown lifecycle](../../contracts/power/shutdown-lifecycle.md)：`power_off()` / `reboot()` 各自
直接执行 filesystem 与 device shutdown 后遍历动态 machine-handler `Vec`；panic 在广播
`StopExecution` 后错误复用 ordinary `power_off()`；filesystem 只枚举 mount snapshot 并调用
`sync_fs`；device subsystem 已拥有 child-before-parent traversal，部分 driver shutdown 为空或仅打日志。

- `SYSTEM-POWER-EPISODE-001` — **Introduce**：唯一 publication、first-winner、固定 intent、不可返回；
- `SYSTEM-POWER-ORDERLY-001` — **Refine**：保留 filesystem -> device -> machine 的现有方向，
  增加 best-effort CPU stop、resident inode snapshot writeback 与显式静态 subsystem plan；保留
  machine-action handler capability lists；
- `SYSTEM-POWER-EMERGENCY-001` — **Replace**：panic 不再复用 ordinary subsystem callback，完成
  best-effort stop/mask 与诊断后直接进入共享 machine-handler 执行入口；
- `SYSTEM-POWER-MACHINE-001` — **Refine**：保留动态 power-off/reboot handler 注册与支持平台
  的 SBI 语义；两个列表增加永久末尾 halt fallback，并作为 orderly/emergency 唯一共享的
  machine-action surface。未支持的 architecture/action 仍可由 halt 收口，并按真实能力记录
  limitation/coverage；emergency progress 不由当前有锁列表保证。

四个 ID 已在同一 stage 的最终 cutover gate 作为一个最小 contract unit 切换：建立
`SYSTEM-POWER-EPISODE-001` Active 条目，并原子更新其它三个 current baseline。
[目标与不变量](./invariants.md) 中的 `SP-*` 继续只作为 RFC-local target/proof label；current dependency
必须引用 `SYSTEM-POWER-*` contract ID。

## 接受边界

### R0 接受什么

R0 接受 owner boundary、两条 terminal path、静态 plan、分层 flush、fail-forward、userspace 非管理
边界及四个 contract ID 的原子 cutover unit。用户于 2026-07-26 授权完成唯一 stage，因此建立
transaction 并把已解析的 Ready stage 激活；接受本身不提前改变 current contract。

episode arbitration、callback non-return、machine handler fallback、reboot intent、single snapshot 与
minimum effective baseline 已闭合。若实现形成 confirmed design issue，再创建 `tracking-issues.md`，
不以预填空分类代替 review。

### Target 变化边界

以下变化必须回到 RFC review，不能留给 implementation preference：

- 让 `power` 之外的 owner 发布或推进全局 episode；
- 让 emergency 执行 orderly subsystem callback plan；
- 将 filesystem/device 等全局 participant 的静态 plan 改为运行时 registry、priority、link-order 或
  dependency graph；
- 让 machine handler 列表拥有 episode/step/intent，移除永久末尾 halt fallback，或为 emergency
  建立绕过列表的平行 power-off/reboot capability；
- 改变分层 flush 顺序，或允许 provider 在 consumer 排空前关闭；
- 从 fail-forward 改为 rollback/retry，或让普通失败跳过最终 machine action；
- 增加 userspace freezer、task-kill、shared-mapping revoke 或强 durability guarantee；
- 让 driver/device framework 接管全局 phase，或让 `power` 接管 device-local traversal。

具体 Rust 类型、callback signature、数组表示、snapshot 容器、内部 helper、日志格式和文件拆分属于
implementation preference，但不得改变上述 target。

## 备选方案

### 用运行时 registry、priority 或 dependency graph 编排全局 subsystem participant

拒绝。全局 participant 数量少且顺序需要人工 review；运行时注册、数字 priority、link-order 和通用
graph 会隐藏 owner decision，并为当前不存在的扩展需求增加状态与失败路径。既有 power-off/reboot
handler 列表只发现/排序最终 machine action capability，不属于这里拒绝的全局 participant registry。

### 所有 subsystem 先 quiesce，再统一 flush

拒绝。filesystem writeback 依赖仍然可用的 block I/O、completion、IRQ、timer 和 device queue；把
provider 与 producer 同时关闭会破坏 flush 可达性。目标改为上游停止生产、数据逐层下沉、provider
最后 teardown。

### Panic 复用 orderly shutdown

拒绝。panic 时其它 CPU、锁、allocator、worker 和中断状态不可信，filesystem/device subsystem
callback plan 没有 emergency-safety 证明。这不排除两条路径在普通 cleanup 之后共用最终
machine-handler 执行入口。

### 为 shutdown 引入常驻 writeback worker

拒绝进入本 RFC。一次 terminal writeback 可以同步执行；worker 不能解决 producer fence，还会要求
普通运行期 clean/redirty、budget、wake、stop 和错误传播协议。

### 冻结或终止 userspace 后再 flush

拒绝。用户任务状态不属于 system-power episode 的管理范围；本 RFC 接受并发修改下只提供有序
best-effort attempt，而不扩大 scheduler/task lifecycle。

## 风险

- 前序 step 失败后继续执行要求每个后续 facade 能安全处理不完整前态；该义务已进入单一 Ready stage
  的 source audit 与 failure-injection 检查。
- `StopExecution` 不是同步 barrier，可能留下 remote lock/worker；普通 callback 卡死也没有 timeout。
  两者均是明确接受的 orderly shutdown failure boundary，而不是实现应偷偷补齐的 watchdog 功能。
- machine handler 列表仍使用普通锁；若 `StopExecution` 恰好停住持锁 CPU，或 handler 自身
  依赖不再前进的普通状态，orderly 与 emergency 的列表遍历都可能卡死。该风险归入 accepted
  no-timeout / no-emergency-progress boundary；本 RFC 不为此增加平行架构操作或无锁容器。
- shared writable mapping 和未管理 userspace 使 flush 不具备稳定 dirty frontier；文档、日志和测试
  不能把 best-effort 写成 durability guarantee。
- 当前 block/driver flush 能力不一致；system-power 只能调用 owner 已提供的安全 capability，不能在
  本 RFC 中伪造 AHCI、MMC 或其它 backend 的 durability。
- emergency path 明确依赖现有 machine-handler registry 及其当前锁/handler 行为，因此只承诺
  best-effort machine-action attempt，不承诺 panic-safe progress。源码审计仍需证明 IPI
  allocation/send 失败不会主动跳过该 attempt。

## Revision Record

- `R0`（2026-07-26）：接受唯一 terminal episode owner、orderly/emergency 分离、静态 shutdown plan、
  resident inode best-effort snapshot writeback、共享 machine-handler list 与永久末尾 halt；同日由唯一
  stage 完成实现和四个 contract ID 的原子 cutover。

## 收口

R0 已实现并关闭，单一 stage 与 transaction 均为 `Completed`。production candidate 保持在冻结
write set 内；source audit 未发现 power-off/reboot/panic/machine action 旁路，也未让 `power` 接管
task/mm、filesystem cache 或 device tree 私有 traversal。257 项 RV64 KUnit、RV64 orderly 两次启动、
受控 emergency panic、RV64/LA64 串行 build 与文档/差异检查完成 closure proof。

`SYSTEM-POWER-EPISODE-001` 已 Introduce，`SYSTEM-POWER-ORDERLY-001` 已 Refine，
`SYSTEM-POWER-EMERGENCY-001` 已 Replace，`SYSTEM-POWER-MACHINE-001` 已 Refine；四项在同一 cutover
生效。RV64 power-off 有 orderly/emergency QEMU runtime evidence；RV64 reboot 未运行 runtime；LA64
没有 ordinary machine handler，两个 intent 均落到永久末尾 halt并明确 `Not Cut Over`。accepted
best-effort 与 architecture gap 已进入 current limitations，当前没有 confirmed tracking issue。
