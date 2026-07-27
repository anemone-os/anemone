# System Power 迁移实施计划

**状态：** Completed
**最后更新：** 2026-07-26
**父 RFC：** [RFC-20260726-system-power](./index.md)
**不变量：** [System Power 目标与不变量](./invariants.md)
**当前契约：** [`SYSTEM-POWER-ORDERLY-001`](../../contracts/power/shutdown-lifecycle.md#system-power-orderly-001)、
[`SYSTEM-POWER-EMERGENCY-001`](../../contracts/power/shutdown-lifecycle.md#system-power-emergency-001)、
[`SYSTEM-POWER-MACHINE-001`](../../contracts/power/shutdown-lifecycle.md#system-power-machine-001)
**当前修订：** `R0`
**事务日志：** [2026-07-26-system-power](../../devlog/transactions/2026-07-26-system-power.md)

> 本计划只有一个 `Ready` stage，不再设内部 checkpoint 或强制提交边界。交付按 owner 分组只为
> 便于 review；完整实现统一进入一次 validation/cutover gate，`Ready` 不自动授予执行或
> cutover 权限。

## 实施原则

- 只建立一个 terminal episode owner；power-off、reboot 与 panic 不各自维护状态。
- 使用现有 `StopExecution` IPI 做一次 best-effort broadcast，不把它升级成 synchronous barrier。
- 沿用现有同步、unit-return shutdown facade；局部 owner 自己记录失败，`power` 不引入通用 outcome、
  retry、rollback、timeout、cancel 或 watchdog framework。
- filesystem 取一次 owner-local resident snapshot，device 保持一次 owner-local traversal；不追赶
  snapshot 后的新对象或 redirty，也不重跑 traversal。
- 保留现有 device child-before-parent owner，不把 block/device tree 搬进 `power`。
- 保留两个 machine-action handler 列表作为 orderly/emergency 唯一共享的 capability registry，
  并让各自唯一的 halt handler 永久位于末尾。
- 一个 stage 同时完成代码、focused proof、双架构结论和文档 cutover；不能把部分 candidate 写成
  Effective。

## Promotion Preflight — 2026-07-26

- 在 branch `dev/drc/alpha`、HEAD `cf23d7a7f86ed0ce5893c73f63bd88480bc9b3e5` 上核验 live owner；
  promotion 本身不修改 kernel、app、config 或 register。
- `power.rs` 当前没有 episode publication：power-off/reboot 各自调用 `fs::on_shutdown()`、
  `device::shutdown()`，随后在持有对应列表锁时按注册顺序遍历 handler，穷尽后独立 spin halt。
- panic 当前关闭本地中断、best-effort 广播 `StopExecution`、打印诊断后调用 ordinary
  `power_off()`；`PANIC_OCCURRED` 不参与 executor/intent 仲裁。
- filesystem 当前只对 anonymous/visible mount tree 各自取 superblock snapshot并调用 `sync_fs`；
  device subsystem 已拥有一次 child-before-parent traversal。RISC-V bootstrap 注册 SBI power-off/
  cold-reboot handler，并在 SBI 返回时进入 `unreachable!()`；未发现 LA64 对等注册。
- 上述事实已作为不改变运行行为的 minimum baseline 提取到
  [System Power shutdown lifecycle](../../contracts/power/shutdown-lifecycle.md)。
  `SYSTEM-POWER-EPISODE-001` 仍为 Introduce target；其余三个 ID 按真实 delta 分类为
  Refine / Replace / Refine。Public Draft 未接受，因此 baseline contract 不设置 pending successor。

## 单一 Stage：Terminal Episode 与 Shutdown Cutover

**状态：** Closed

### 输入基线

- `power.rs` 的 `power_off()` / `reboot()` 各自执行 `fs::on_shutdown()`、`device::shutdown()`，随后遍历
  两份动态 machine handler `Vec`；没有 episode publication。
- panic handler 已异步广播 `StopExecution`，但随后调用 ordinary `power_off()`。
- VFS 已拥有 mount snapshot、superblock resident `indexed + ghosts` inode cache、`sync_inode` 与
  `sync_fs`；shutdown 目前只调用 `sync_fs`。
- device subsystem 已拥有 child-before-parent traversal；VirtIO block 在 driver shutdown 中 flush，SD
  memory 的同步命令路径没有待排空 queue，AHCI 与 DW-MSHC shutdown capability 仍不完整。
- RV64 bootstrap 已向两个动态 handler 列表注册 SBI power-off/cold-reboot capability；LA64 当前没有
  等价的普通 handler，DTS 中的 syscon 节点不等于已有驱动能力。当前 SBI handler 把
  `system_reset` 返回视为 `unreachable!()`，尚不支持“当前 handler 返回则继续 fallback”；两个列表
  为空时由遍历后的独立 spin loop 兜底，尚未把 halt 表达为列表内永久 fallback。

### Implementation Deliverables

#### Terminal episode 与 machine action

- 在 `power` 内建立最小 terminal publication：第一个成功 publication 同时固定 executor CPU 与
  `PowerOff | Reboot` intent。后续请求不替换 winner/intent、不运行 callback，并进入不可返回停止。
- `power_off()` 与 `reboot()` 只作为同一 orderly 入口的两种 intent；winner 在第一个 callback 前调用
  `broadcast_ipi_async(IpiPayload::StopExecution)` 一次，失败只打印并继续。
- 用源码显式固定的 ordinary function slice 取代隐式重复调用链。当前 participant 只有 filesystem 与
  device 两个 owner facade；数组顺序固定为 filesystem 后 device，machine action 是数组后的终止点。
- panic handler 不再调用 ordinary `power_off()`。没有 episode 时，当前 CPU 可以成为 emergency winner；
  winning orderly executor panic 时，同一 episode 原地切换为 emergency；其它 CPU 的 panic/请求只停止。
- winner 已在 emergency 内再次 panic 时直接 final halt，不重跑 diagnostics、stop broadcast 或
  machine fallback。
- emergency path 关闭本地中断、best-effort 停止其它 CPU、保留 panic diagnostics，然后跳过
  filesystem/device subsystem plan，按已冻结 intent 直接进入共享 machine-handler 列表执行入口。
  无先行 episode 的 panic 冻结 power-off intent；从 orderly 原地切换时保留原 intent。
- 接受现有 `StopExecution` transport 的 message allocation；allocation/send 失败必须继续 machine
  action attempt。本 stage 不为 emergency 新增平行架构 machine-action 通道、无锁列表或专用
  emergency-safe handler 类型。
- 保留 `PowerOffHandler` / `RebootHandler` 与两个动态列表。每个列表初始化时内建唯一 halt handler；
  普通注册函数把 platform/driver handler 插到 halt 之前，使 halt 永久保持末位，同时保持普通 handler
  的注册顺序。handler 返回就继续下一个，末尾 halt 永不返回。
- `power` 提供唯一的内部列表执行 helper，其中不包含 filesystem/device cleanup。orderly 在
  subsystem plan 完成后进入该 helper，emergency 跳过该 plan 后直接进入；两者均根据 publication
  已冻结的 intent 遍历同一对列表。列表不拥有 episode/step/intent，也不被竞争失败者或
  subsystem callback 访问。
- 将列表锁或 handler 自身导致 orderly/emergency 不前进保留为 accepted limitation；本 stage 不添加
  timeout、takeover、lock-free migration 或 handler emergency-safety 承诺。
- RV64 保留现有 SBI handler 注册形状；`system_reset` 返回时记录失败并返回列表，不再
  通过 `unreachable!()` 把可尝试下一 handler 的结果升级为 panic。LA64 若没有普通 power-off/reboot
  handler，两个 intent 都自然落到各自列表末尾 halt，不为形式对称增加未验证的 syscon 实现。

#### Filesystem writeback 与现有 device traversal

- VFS 一次性 snapshot anonymous + visible mount tree 的 superblock，并按 `Arc` identity 去重；同一
  superblock 本轮只处理一次。
- 每个 superblock 在自己的 resident inode cache 上取一次 `indexed + ghosts` snapshot，释放 cache lock
  后逐 inode 调用现有 `sync_inode`。单个 inode 失败打印并继续其余 inode，完成后仍调用该 superblock
  的 `sync_fs`；不 eviction、不 reclaim、不重复 snapshot。
- `fs::on_shutdown()` 保持 unit-return facade，负责上述 resident inode writeback 后的 filesystem
  cache/journal commit。snapshot 后新 load/redirty 的 inode 不纳入本轮，也不宣称 dirty frontier 收敛。
- `device::shutdown()` 保持现有 child-before-parent traversal。storage flush 仍由 concrete block driver 在
  自己的 shutdown callback 内完成；本 stage 不为当前没有 queue/flush contract 的 backend 扩大
  `BlockDev` ABI，也不把 driver stub 伪装为成功 capability。
- VirtIO block 的 flush 继续发生在其 device callback 内；SD memory 保持同步完成说明；AHCI/DW-MSHC
  继续明确打印 unsupported，并在最终 register/current-limitations 中登记或保留现有条目。

### Implementation Boundaries

- 不等待 `StopExecution` completion，不扫描 task/mm，不增加 freezer 或 task killer；
- 不允许 emergency 抢占另一个仍在运行的 orderly executor；只有 winner 自身 panic 能原地切换；
- 不新增 generic state machine crate、全局 subsystem callback registry、dependency graph 或 platform
  plugin API；
- 不建立常驻 writeback worker、全局 block flush trait、通用 error aggregation 或 shutdown admission
  errno；
- 不为 snapshot completeness 扫描 userspace mapping、强制 close file 或等待 task 退出；
- 不添加 timeout、takeover、lock-free migration 或 handler emergency-safety 承诺。remote CPU
  遗留锁、列表锁或 handler 自身导致的不前进属于 accepted limitation。

### Validation and Cutover Gate

完成以下验证，证据写入独立 transaction devlog，不把长日志复制进 RFC：

1. **Production logic proof：** focused KUnit 只直接执行 production type/helper，覆盖
   first-publication-wins、intent 不可替换与 winning orderly executor 原地切 emergency。若共享
   machine-handler helper 自然支持局部实例，再覆盖 intent 选表、handler 返回后继续与末尾
   halt fallback。不复制 test-only coordinator/list 模型，不新增 production global-state reset hook；
   若只能如此测试，则取消对应 KUnit，改用 source audit 与运行证据。source audit 覆盖全部
   power-off/reboot/panic/machine-action 入口、ordinary 静态 plan 顺序、列表末尾 halt、SBI 返回、
   `StopExecution` 失败路径，以及 superblock/inode 单次 snapshot 去重、lock-before-callback 释放
   和单 inode 失败后继续/最终 `sync_fs`。
2. **Static and architecture validation：** 运行 `just fmt kernel --check`，并串行执行
   `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G` 与
   `just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G`，避免共享
   `build/generated/kernel.lds` 被并发覆盖。显式 provider bind 是当前 build resolver 的完整输入；原
   validation 文本遗漏它们，修正只改变执行路线，不改变 target。LA64 若没有已注册 platform
   machine-action handler，只用
   build/source audit 证明两个 intent 均落到默认 halt，并记录 power-off/reboot `Not Cut Over`；
   只有存在或新增 handler 时才需要对应 QEMU/硬件运行 evidence。
3. **RV64 runtime：** 只执行两条直接对应 target 的路径：
   - orderly run 在可复用磁盘副本上写入标记，不调用 `fsync` 并保持 file open 直到 shutdown，
     观察 episode、filesystem -> device -> SBI action 与正常退出，重启同一运行副本后验证标记。
     该运行同时承担 writeback 与关机顺序证明，不扩大为强 durability。
   - emergency run 使用 `power.rs` / `panic.rs` 内 validation-only、KUnit-only 的可控 boot panic
     probe，证明没有 filesystem/device callback且 panic 进入同一 SBI handler 列表。该 probe 不新增
     syscall/debug ABI，取得证据后删除；结果不扩大为锁竞争或任意 handler 下的
     emergency-progress 证明。
4. **Cutover hygiene：** 执行 `git diff --check` 与 `mdbook build docs`，并审计 diff 不含
   validation-only panic probe、test profile 或磁盘副本。

全部 proof 满足后，才在同一 cutover patch 中建立并标记
`SYSTEM-POWER-EPISODE-001`、`SYSTEM-POWER-ORDERLY-001`、
`SYSTEM-POWER-EMERGENCY-001`、`SYSTEM-POWER-MACHINE-001` 的实际 architecture coverage，同步 RFC、
transaction、register/current-limitations 与公共导航。部分实现、单一架构或单次运行成功都不能让
任何 ID 单独提前生效。

## Resolved Write Set

生产代码：

- `anemone-kernel/src/power.rs`；
- `anemone-kernel/src/panic.rs`；
- `anemone-kernel/src/fs/mod.rs`；
- `anemone-kernel/src/fs/superblock.rs`；
- `anemone-kernel/src/arch/riscv64/bootstrap.rs`，仅用于使现有 SBI handler 符合“架构请求返回则
  继续后续 handler / 末尾 halt”的列表语义，不抽出 emergency 专用架构入口；
- 上述 owner 内能直接执行 production type/helper 的 focused KUnit（若存在）。

架构 machine 目录、`arch/mod.rs` 与 LA64 machine/bootstrap 本 stage 默认只读；现有 handler
继续通过两个共享列表提供能力，不为 emergency 额外适配或抽出平行架构入口。

本 stage 默认只读：`exception/ipi.rs`、task/mm/scheduler、mount/umount ABI、`BlockDev` trait、device tree
traversal、具体 filesystem/driver 实现、syscall ABI、apps/rootfs/test profile 与 platform DTS。若 live
implementation 证明必须修改这些 owner surface，先停止并回写扩展理由、精确文件、target/contract
影响和新增验证；不能以“顺手补齐 backend”扩大本 RFC。

validation-only write set：

- `anemone-apps/user-test/src/main.rs`，仅用于 orderly marker/open-fd/shutdown probe，运行后必须恢复；
- `anemone-kernel/src/power.rs` / `anemone-kernel/src/panic.rs` 中的 KUnit-only emergency boot panic
  probe，取得证据后必须删除；
- worktree-local 磁盘副本与 `build/**` 日志；不得修改或写挂载 shared master image。

`R0` 接受并创建 transaction 后，本 stage 的文档 write set 精确包括：

- `docs/src/rfcs/system-power/{index.md,invariants.md,implementation.md}`；
- `docs/src/contracts/power/{index.md,shutdown-lifecycle.md}`、`docs/src/contracts.md`；
- `docs/src/devlog/transactions/2026-07-26-system-power.md`、
  `docs/src/devlog/transactions/index.md`、`docs/src/devlog/2026-07-20_to_2026-08-02.md`；
- `docs/src/register/{open-issues.md,current-limitations.md}`，只写实际仍开放或已接受的 gap；
- `docs/src/rfcs.md` 与 `docs/src/SUMMARY.md`。

本 promotion 只建立 public RFC、minimum current baseline 与导航，不创建 transaction、修改 register
或启动上述 stage。

## Stage 停止条件

出现以下任一情况，停止当前 stage，不进入 cutover：

- 无法在不建立第二 executor/takeover 的前提下表达 winner panic；
- emergency 仍需 filesystem/device callback 或 worker wait，而不是直接进入共享 machine-handler helper；
- 实现需要平行架构 machine-action 通道、无锁列表或新 handler safety contract 才能工作；
- resident inode writeback 必须通过 eviction/reclaim 或持 cache lock 调 backend；
- 后续 step 只有在前序 step 成功时才可安全调用；
- 两个共享 handler 列表不能维持唯一末尾 halt，或 recursive-emergency/loser CPU halt 仍可能返回；
- 实现证据要求改变 owner、ABI、static-plan、single-snapshot、no-timeout 或 best-effort target。

前六项优先回到实现/owner review；最后一项必须走 Target Renegotiation Gate，不能把较弱行为静默写成
原 target closure。

## 完成定义

本 stage 只有在代码、focused proof、RV64 运行证据、LA64 明确 coverage、旁路审计和文档 cutover 全部
闭合时才是 `Completed`。callback 卡死、best-effort `StopExecution`、snapshot 后 redirty、unsupported
backend、machine-handler 列表/handler 导致的 emergency 不前进与 LA64 Not Cut Over 可以
作为明确限制存在；它们不应诱发额外阶段或推测性恢复机制。

## Completion Record — 2026-07-26

单一 stage 已按冻结 production/write-back manifest 完成，未命中停止条件。实现建立唯一 terminal
episode publication、静态 `filesystem -> device -> machine` plan、独立 emergency handoff、永久末尾
halt、resident inode best-effort snapshot writeback 与 SBI 返回后继续 fallback。最终 source audit、
双架构串行 build、257 项 RV64 KUnit、orderly 两次启动持久化 probe、emergency boot panic probe 与
文档/差异检查证据统一记录在
[transaction](../../devlog/transactions/2026-07-26-system-power.md)。

四个 contract ID 已作为一个 unit 完成 cutover。RV64 power-off 的 orderly/emergency machine action
有 QEMU runtime evidence；RV64 reboot 只有共享 production path、focused KUnit 与 SBI registration
source proof，未运行 reboot runtime；LA64 未发现普通 machine handler，power-off/reboot 均明确
`Not Cut Over` 并落到永久末尾 halt。callback/lock 不前进、best-effort `StopExecution`、snapshot 后
redirty 与 backend shutdown 缺口保留为 current limitations，不扩大为额外 stage。
