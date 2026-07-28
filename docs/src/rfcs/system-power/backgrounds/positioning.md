# System Power 定位共识

**状态：** Archived Positioning Background
**最后更新：** 2026-07-26
**范围：** system power / orderly shutdown / emergency shutdown

> 本文记录 `system-power` RFC Draft 形成前的定位共识与明确未决项。当前 public Draft 权威已经
> 转移到上级目录的 `index.md` 与 `invariants.md`；本文只作为背景，不是 current contract、
> accepted RFC target 或 implementation plan，也不表示任何实现阶段已经获得授权。

## 文档目的

本轮只固定 system shutdown 的 owner boundary、两类 shutdown 路径、orderly shutdown 的
分层依赖和全局编排形状。后续 Draft review 已在上级 `index.md` / `invariants.md` 闭合 episode winner、
`StopExecution`、单次 snapshot、无 timeout 和 machine-handler fallback；精确 Rust 类型仍是实现偏好。

## 当前定位共识

### `power` 是全局 shutdown episode 的唯一 owner

`power` 唯一拥有一次全局 shutdown episode 的发布、全局阶段推进和跨子系统顺序。其它
subsystem 可以拥有自己的局部 shutdown / quiesce 状态，但不得发布、缓存或推进另一份全局
shutdown phase truth，也不得越过 `power` 直接执行全局 machine action。

一次 episode 是单向、不可回滚的终止过程。定位阶段曾保留重复 orderly 请求、orderly/emergency
竞态与 reboot 仲裁；current Draft 已裁决为 first successful publication wins，winner 同时冻结 executor
和 machine intent，后续请求不可返回，只有 winning orderly executor panic 能在同一 episode 原地
切换 emergency。

### Orderly shutdown 保持固定的高层顺序

当前接受的高层顺序是：

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

这里固定的是全局方向和 owner handoff，不是已经闭合的逐项执行协议。其核心规则是：上游
producer 先停止产生新的 owner-local 状态，下游 provider 必须继续提供完成排空所需的能力；
数据按 filesystem page / inode、filesystem cache / journal、block / storage 的方向逐层下沉后，
provider 才能进入不可逆 shutdown。

因此不存在一个要求所有 subsystem 同时进入同一种 `quiesced` 状态的全局阶段。producer-side
quiesce 从依赖上游向下推进，provider teardown 则等待其 consumer 完成后再向设备推进。静态
全局 plan 直接编码这一固定顺序，不把它抽象成运行时 dependency graph。

### Orderly episode 只允许 fail-forward

全局 episode 一旦发布就不再回滚。已经完成或尝试过的 step 不会因为后续失败而反向恢复
admission、重启 worker、重新开放 provider，或撤销已经发布的 shutdown 状态。

每个可返回的 orderly step 最多执行一次。step 返回失败时，`power` 记录全局 step 和错误结果，
subsystem 记录 owner-local 诊断，然后全局 plan 继续执行后续 best-effort step，最终仍然尝试
machine action。后续 step 不能以此前所有 step 成功为前提；它们必须在可安全处理的剩余状态上
完成自己的有限 attempt。普通失败不触发重试、补偿事务或 orderly rollback。

callback 永不返回或无限等待时 orderly shutdown 会保持卡死，不提供 timeout/cancel/watchdog；winning
executor panic 则放弃 ordinary plan，在同一 episode 进入 emergency。这些是后续 Draft 已接受的失败
边界，不由 fail-forward 假装转换成普通返回错误。

### Emergency / panic shutdown 是独立路径

Emergency shutdown 不复用 orderly shutdown callback plan，也不以 orderly cleanup 成功为
前提。它不得等待普通 worker，不执行普通 subsystem callback，不要求完整 filesystem/storage
flush、reclaim 或 owner-local teardown。

Emergency 路径的目标是在不依赖可能已经损坏或被其它 CPU 持有的普通内核状态的前提下：

1. 确立唯一 emergency 执行者，其它 CPU 停止继续执行；
2. 关闭或屏蔽继续执行所必需的本地中断来源；
3. 只执行明确证明为 emergency-safe 的 mask / reset / machine action；
4. 尽快掉电，无法掉电时进入不可返回的停止状态。

定位阶段要求 emergency 不依赖普通 callback registry、可睡眠等待或可能由被停止 CPU 持有的普通锁。
后续 live-source review 确认现有 `StopExecution` IPI transport 会分配 message，因此 current Draft 接受
这一次 best-effort allocation：失败只记录并继续 machine action，不能成为 progress 前提；除此之外
不新增 allocator dependency。

### Subsystem 只拥有自己的局部 producer quiesce

每个 subsystem 只负责自己的局部 admission、timer / wake、worker 和 in-flight access：

- 停止本 owner 的新 admission；
- 撤销或停止本 owner 发起的新 timer / wake / background work；
- 以 owner-local 规则排空已经进入的访问；
- 发布本 owner 的局部 producer-quiesced 状态；该状态不表示其下游 provider 已经关闭。

这些动作只覆盖 kernel-owned producer 和 subsystem admission，不表示相同 owner 依赖的下游
provider 可以同时关闭。例如 filesystem writeback 完成前，block I/O、completion、IRQ、timer
和设备 queue 必须继续提供该路径所需的能力。

这不等于拥有一套全局 userspace lifecycle，也不授权 `power` 强制终止任意 kthread。orderly path 会在
callback 前广播一次 best-effort `StopExecution` IPI，尽可能停止其它 CPU 上的执行，但它不是同步
barrier，也不证明每个 userspace task 已完成退出。除此之外，内核不枚举、不冻结、
不终止、不等待用户任务，也不把 userspace 协作或退出作为本 RFC 的保证或 shutdown 完成条件。
subsystem 可以关闭自己的新请求入口，但这只是 owner-local admission 语义，不是 userspace
lifecycle 管理。subsystem 可以要求自己拥有的 worker 停止生产新工作并排空；全局 force-stop、
通用 task freeze 和以 shutdown 为由扩大 scheduler / kthread 控制面均不进入本 RFC。这同样
不构成 userspace 会继续运行到某个阶段或继续获得正常服务的保证；全局 episode 只定义
kernel-owned shutdown step。

已经建立的 shared writable mapping 可以绕过新的 VFS syscall admission 继续修改 resident page。
本 RFC 不为此 write-protect、revoke 或遍历用户映射，也不声称 orderly writeback 形成了不再变化
的全局 dirty frontier。

### Flush 按 filesystem 到 storage 的依赖方向推进

filesystem shutdown facade 负责一次同步、owner-local 的 best-effort writeback attempt：对 shutdown
时可达的 resident file data 和 inode metadata 发起写回，然后提交 filesystem 自己的 cache /
journal。该过程不通过 eviction 或 reclaim 间接完成，也不要求关闭仍被 writeback 使用的 storage
能力。

filesystem commit 完成后，block / storage 才排空已经提交的 I/O 并尝试 flush volatile device
cache；完成该层 attempt 后，device subsystem 才能关闭 queue、DMA、IRQ 和硬件执行。driver
shutdown 不应成为第一次把 filesystem dirty state 推向持久介质的阶段。

当前 RFC 只引入 shutdown 所需的同步 facade，不建立常驻 writeback worker、dirty aging / budget、
周期回写、异步提交或完整 clean / redirty 状态机。普通运行期 writeback、完整 `fsync` / `msync`
语义和后台 worker 若后续需要，应独立解析其 owner、并发、错误和 durability contract。

由于内核不管理 userspace 状态，也不封住 shared writable mapping 的直接写入，本 RFC 的 flush
target 是顺序明确、错误可观测的 best-effort attempt，而不是面对并发用户态修改时的完整
durability guarantee。一次 orderly shutdown 不以 dirty set 收敛为空作为完成条件，也不通过
无界重复 writeback 追逐并发修改。

### Driver shutdown 保持 owner-local

全局 `power` plan 只进入一次 device subsystem facade。device subsystem 内部继续拥有自己的
device tree、parent / child、bus、driver 和 shutdown 顺序；`power` 不复制或旁路这些机制。

concrete driver shutdown 只处理 owner-local 的 IRQ、queue、DMA、reset、硬件 quiesce 和安全
资源回收。driver 不拥有全局 shutdown phase，也不负责协调其它 subsystem、filesystem 或全局
worker lifecycle。

### 全局采用显式的静态 callback plan

Orderly shutdown 的全局参与者数量有限，顺序可以并且应当由 review 明确裁决。因此 `power`
直接拥有一份编译期固定、源码中显式可见的 callback 数组，作为全局 orderly shutdown plan。

该选择具有以下边界：

- 不为 filesystem/device 等全局 subsystem participant 提供运行时 callback 注册、unregister、数字
  priority、linker section 自动收集或隐式 link-order 排序；
- callback 数组的字面顺序就是全局 review surface；新增、删除或重排参与者必须直接修改该
  plan；
- 每个 callback 只暴露一个窄的 subsystem shutdown facade，不把 subsystem 私有 object、锁、
  registry 或 traversal 交给 `power`；
- 同一 owner 只有在跨 subsystem handoff 必须全局可见时，才按静态 plan 暴露多个阶段专用的
  窄 facade；例如 producer quiesce 与后续 flush 不能被一个不透明 callback 混成不可 review
  的局部顺序；
- 普通 subsystem callback 不能推进全局 phase、插入新的全局 step、调用其它 phase，或直接
  执行 machine action；
- `power` 统一负责 step 边界、调用顺序和全局观测，subsystem 负责局部执行与诊断；
- emergency shutdown 不运行这份数组。

这是静态全局编排与动态 owner-local 编排的混合策略。例如，device callback 内部可以继续使用
复杂的 device / bus / driver shutdown 机制；filesystem callback 内部可以使用完全不同的
superblock 枚举、去重和 flush 规则。全局 plan 不试图把这些局部机制统一成通用 shutdown
framework。

后续 Draft review 明确区分了这份静态 subsystem plan 与既有 power-off/reboot handler 列表：后者只
提供最终 machine-action capability，不发布 episode 或推进 phase，因此继续保留动态注册。两个列表
各自永久以唯一 halt handler 收尾，普通 handler 按注册顺序插在 halt 之前。定位阶段曾要求 emergency
不访问列表；current public Draft 已明确取代该要求，接受 emergency 跳过 ordinary plan 后访问同一
列表，并把列表锁/handler 可能不前进列为 acceptance boundary。

当前只冻结上述机制选择，不冻结 callback 的具体函数签名、context、result 类型或数组拆分
方式。实现应保持最小形状，避免把显式静态计划重新包装成可配置 dependency graph。

## 明确非目标

- 全局冻结 userspace 或建立通用 process freezer；
- 枚举、等待、终止用户任务，或把 userspace 协作 / 退出作为 shutdown 前提；
- 强制终止任意 kthread，或建立通用 shutdown task-kill 机制；
- 为 shared writable mapping 建立 shutdown-only write-protect / revoke 机制；
- 建立普通运行期 writeback worker、dirty aging / throttling 或完整 `fsync` / `msync` 语义；
- 在并发 userspace mutation 下承诺完整 filesystem / storage durability；
- runtime suspend / resume；
- device hotplug、unbind 或 runtime removal；
- runtime restart / reactivation，以及为这些能力预建可逆生命周期；
- 用 system shutdown RFC 接管 subsystem / driver 内部的局部 shutdown 顺序；
- 让 emergency 路径追求完整 flush、reclaim 或普通资源回收。

## 定位阶段未决项的后续结论

本节只记录这些问题已被 current Draft 如何裁决，权威规则仍以上级文档为准：

1. callback 卡死即 orderly 卡死；不建立 timeout/cancel/watchdog；winner panic 原地进入 emergency；
2. orderly 保留 power-off/reboot handler 列表作为 capability 集合；每个列表永久以唯一 halt handler
   收尾，普通注册插在 halt 之前；current public Draft supersede 了“emergency 禁止访问列表”的早期
   结论，改为两条路径共用列表并明确接受其不前进风险；
3. 本轮不新增 subsystem admission gate，因此不发明统一 errno；以后由具体 owner 单独审核；
4. reboot 只是 publication 时冻结的 machine intent，不拥有第二套 episode 或 plan；
5. filesystem owner 取一次 resident snapshot，device owner 做一次 traversal，两者都不重跑；
6. RV64/LA64 machine coverage 分别记录 Cut Over 或 Not Cut Over，不阻塞共享 owner 实现。

## RFC Draft 就绪结论

当前 owner boundary、orderly/emergency 分路、静态全局 plan、分层 flush 依赖、userspace 非管理
边界和 fail-forward 规则已经形成 RFC-shaped Draft；后续 review 又闭合了上述原未决项，并把完整工作
解析为一个 Ready implementation stage。Draft 与 Ready 都不表示 target 已公开接受，也不授权实现。

## RFC 接受与单一 Ready stage 的闭合结果

- orderly/emergency 的单向状态转换和 first-winner 竞争裁决已经给出；
- 把已经接受的 producer quiesce、filesystem writeback / commit、storage drain / flush 和 device
  shutdown 顺序解析为明确的 step precondition、保留能力和完成条件；
- 明确静态 plan 中每个 subsystem facade 的职责和禁止行为；
- callback non-return / no-timeout、machine-action failure 和 final halt 已经明确；
- minimum effective baseline、proposed target、subsystem-local obligation 与单一 cutover gate 已经
  写入上级文档和 `implementation.md`。
