# 2026-07-26 - System Power

**Status:** Completed
**Date:** 2026-07-26
**Owner:** doruche, Codex
**Canonical Plan:** [RFC-20260726-system-power R0](../../rfcs/system-power/index.md),
[目标与不变量](../../rfcs/system-power/invariants.md),
[单一实施 stage](../../rfcs/system-power/implementation.md#单一-stage-terminal-episode-与-shutdown-cutover)
**Canonical Revision:** R0
**Contract Impact:** `SYSTEM-POWER-EPISODE-001` Introduce、`SYSTEM-POWER-ORDERLY-001` Refine、
`SYSTEM-POWER-EMERGENCY-001` Replace、`SYSTEM-POWER-MACHINE-001` Refine；四项只在 final cutover
作为一个 unit 生效

## Scope and authorization

用户于 2026-07-26 授权完成 RFC 的唯一 stage、执行其 review/validation/write-back 并交付一个
`system-power: ...` commit。R0 因此从 Public Draft 接受为 Accepted for Implementation，已解析的
Ready stage 同时激活；本授权不允许越出 canonical resolved write set、改变 owner/public API/shared
contract/ABI/visible semantics/acceptance，或在命中停止条件后继续 cutover。

## R0 acceptance and activation preflight

Preflight 在 `dev/drc/alpha`、promotion commit `a5c32d75` 上读取 AGENTS/LOCAL、R0 正文、
implementation、current power contract、register 与 live owner。RFC 明确不创建 tracking page，因为
当前没有 confirmed design finding。

Live source 仍与 promotion baseline 一致：`power_off()` / `reboot()` 没有 episode publication；panic
诊断后复用 ordinary power-off；VFS shutdown 只调用 `sync_fs`；device 保持 child-before-parent
traversal；RV64 SBI handler 把返回升级为 panic；LA64 没有对等普通 handler。现有 owner surface 足以在
冻结 manifest 内表达 R0：无需修改 IPI、task/mm/scheduler、BlockDev、device traversal、syscall ABI、
LA64 machine/bootstrap 或具体 filesystem/driver。

Stage 启动时未命中停止条件：winner panic 可由同一 episode 原地切换；emergency 可直接进入共享
machine-list helper；resident inode cache 已能在释放 superblock lock 后形成 `indexed + ghosts` snapshot；
后续 owner facade 都是有限 best-effort attempt；两个列表可把唯一 halt 固定在末尾。四个 contract ID
保持 pending，直至完整 validation/cutover gate 闭合。

## Execution log

### 2026-07-26 - Single stage activated

**Write-set lock:** production、validation-only 与 docs write set 以 canonical implementation 为唯一
authority。本事务只记录执行事实，不复制或扩张 manifest。

**Planned proof:** production helper KUnit/source audit；`just fmt kernel --check`；RV64 与 LA64 release
build 串行执行；RV64 orderly dirty-open-file persistence 与 validation-only emergency panic 两条 runtime；
最终 review、`git diff --check`、`mdbook build docs` 与旁路/临时资产审计。

**Activation-time cutover:** Not Cut Over。此时 current power contract 只增加 pending-successor
navigation，effective baseline 未改变；最终 cutover 见本页 closure 段。

### 2026-07-26 - Production implementation and review

**Production write set:**

- `power.rs` 以一个 `AtomicUsize` 同时编码 executor、frozen intent 与 episode phase。第一次
  publication 是唯一 election point；winning orderly executor 可以原地切 emergency，loser、recursive
  emergency 与 machine-handler panic 都屏蔽本地中断后 halt。
- orderly 入口在 callback 前 best-effort 广播 `StopExecution`，随后执行源码固定的
  `filesystem -> device` function array，再进入按 frozen intent选表的共享 machine helper。两个 list
  各自把不可注册的 `HaltHandler` 结构性固定为唯一末项；ordinary handler 返回就继续。
- panic 不再调用 ordinary `power_off()`；winner保留 diagnostics 与 stop broadcast，然后跳过全部
  filesystem/device callback，直接进入同一 machine helper。
- VFS 一次 snapshot anonymous + visible superblock 并跨树按 `Arc` identity 去重；每个 superblock
  通过 `sync_resident_inodes_best_effort()` 一次 snapshot `indexed + ghosts`，释放 cache lock 后逐 inode
  writeback，单项失败继续，最后仍调用 `sync_fs`。
- RV64 SBI power-off/cold-reboot request 返回时记录并返回 list，不再 panic/retry machine path。

Review 发现 loser ordinary requester 原先会在 interrupts enabled 条件下 spin；统一 halt primitive 已改为
先关闭本地中断，避免 timer/scheduler 继续 unrelated work。`SuperBlock` helper 经 review 从调用场景名
`sync_resident_inodes_for_shutdown` 收窄为行为名 `sync_resident_inodes_best_effort`：该 owner-local 方法
不推进全局 phase，名称显式保留吞掉单项失败并继续的语义。

最终旁路审计覆盖全部 `power_off`、`reboot`、panic、handler registration 与 machine callsite：只有
native shutdown syscall进入 orderly power-off，panic只经 emergency publication进入共享 helper，RV64
bootstrap 是唯一普通 machine capability registration；LA64 未发现对等 handler。`power` diff 未触碰
task/mm/scheduler、userspace mapping、IPI owner、`BlockDev`、device traversal、具体 driver、syscall ABI
或 LA64 machine/bootstrap。没有命中 canonical stage stop condition。

### 2026-07-26 - Validation evidence

**Focused production proof:** RV64 KUnit 运行 `257/257` 通过，其中三个新增 case 直接执行 production
`TerminalEpisode`，覆盖 first publication冻结 executor/intent、orderly winner原地切 emergency、
recursive emergency 与 machine-action terminal phase；没有 production global reset hook 或 test-only
coordinator/list model。handler-return continuation、永久末尾 halt 与 intent选表不适合在不调用 `!`
fallback 的局部实例中执行，按 implementation gate使用 production list/source audit闭合。

**RV64 orderly runtime:** validation-only user-test 首次在工作盘创建 marker，保持 fd open且不调用
`fsync`/close；QEMU exit 0，日志依次出现：

```text
SYSTEM-POWER-PROBE:ORDERLY:STAGED-OPEN-FD
system-power: CPU core #0 published orderly PowerOff episode
system-power: starting filesystem shutdown step
system-power: completed filesystem shutdown step
system-power: starting device shutdown step
system-power: completed device shutdown step
system-power: executor core #0 entering PowerOff machine action
```

直接复用 `build/runtime/pretest-rv64/disk-x0.img` 第二次启动后得到
`SYSTEM-POWER-PROBE:ORDERLY:VERIFIED`，并再次观察同一顺序与 QEMU exit 0。该 evidence 证明本次
ext4 resident writeback/commit、VirtIO flush/device traversal 与 SBI power-off attempt，不证明并发
userspace、snapshot 后 redirty、其它 backend 或 hardware strong durability。

**RV64 emergency runtime:** validation-only KUnit boot panic probe 输出
`SYSTEM-POWER-PROBE:EMERGENCY`、kernel panic/backtrace，随后直接输出共享
`PowerOff machine action` 并以 QEMU exit 0 结束；日志没有 orderly publication、filesystem 或 device
step。probe 和 user-test 修改均在最终 diff 前删除。winning orderly executor切换由 production KUnit
覆盖；IPI allocation/send failure 后继续 machine attempt由 panic source control flow覆盖，未做 allocator
failure injection，也不宣称列表锁或任意 handler 下的 emergency progress。

**Failure/source proof:** 全局 `OrderlyStep` 沿用 accepted unit-return facade，因此没有由 `power`
解释的 error outcome或可注入 rollback branch。owner-local source audit确认 broadcast `Err` 记录后继续、
每个 inode `sync_inode` error 记录后继续 snapshot、`sync_fs` error 记录后继续其它 superblock，driver
failure保持 local log/return；静态 plan 不读取这些结果，也没有 retry/reset。没有额外 runtime failure
injection，这一 proof边界不扩大为 callback 必然返回。

**Static/architecture:** canonical build 当前要求完整 provider inputs；implementation 中原先遗漏的 binds
已作为保持 target 的 validation-route correction 回写。以下命令串行通过，避免共享
`build/generated/kernel.lds` 并发覆盖：

```text
just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G
just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G
```

首次 RV64 sandbox build 曾在 lwext4 host build script命中 `SIGSYS / Bad system call`；同一仓库命令在
sandbox 外通过，因此归类为 host sandbox evidence。`just fmt kernel --check` exit 1，全部 diff path
位于新并入的 `anemone-kernel/crates/anemos/smoltcp/**` 既有格式漂移；本 stage 五个 changed production
文件没有 formatter diff。冻结 write set 不允许为此批量改写 smoltcp，故保留全局 baseline failure并
单独记录 changed-file clean proof。

### 2026-07-26 - Contract cutover and closure

Review、production proof、RV64 两条 runtime、双架构 build与临时资产清理满足单一 atomic gate后，
current contract 同时完成：

- Introduce `SYSTEM-POWER-EPISODE-001`；
- Refine `SYSTEM-POWER-ORDERLY-001`；
- Replace `SYSTEM-POWER-EMERGENCY-001`；
- Refine `SYSTEM-POWER-MACHINE-001`。

Architecture coverage 如实分层：RV64 orderly/emergency power-off 为 `Cut Over`；RV64 reboot 只有
frozen-intent KUnit、共享 helper与 SBI cold-reboot registration source proof，runtime `Not Run`；LA64
build通过但没有 ordinary handler，power-off/reboot 均为 `Not Cut Over` 并落到末尾 halt。callback/
remote lock不前进、best-effort `StopExecution`、snapshot 后 redirty、AHCI/DW-MSHC unsupported 与
machine-handler lock/progress 边界已写入 current limitations。

RFC、implementation、invariants、current contract、register、transaction index、biweekly devlog与
公共导航在同一 closure patch同步。`git diff --check`、`mdbook build docs` 与最终 commit/status audit的
结果记录在本 transaction 的 final validation 段；未运行 RV64 reboot、LA64 QEMU/hardware、physical
hardware、LTP 或 final harness。

### 2026-07-26 - Final validation

- `git diff --check`：通过。
- `mdbook build docs`：通过；仅有 search index size warning。
- production-only 最终串行 RV64/LA64 release build：通过，命令和 provider inputs见上文。
- final source/write-set audit：只有五个 production 文件与 canonical docs write set有 diff；
  `anemone-apps/user-test/src/main.rs`、LTP profile、emergency probe、IPI/task/mm/scheduler、`BlockDev`、
  device/driver owner、LA64 owner与 platform DTS均无 diff。磁盘副本和 `build/**` 日志未被 Git跟踪。
- `just fmt kernel --check`：未通过，exit 1；formatter diff paths全部位于
  `anemone-kernel/crates/anemos/smoltcp/**` 的 pre-existing baseline，changed production files clean。
  未以格式化为由扩大冻结 write set。
- Not Run：RV64 reboot runtime、LA64 QEMU/hardware machine action、physical hardware、LTP、final harness。

事务状态设为 `Completed`；RFC R0、唯一 stage 和四个 contract ID 同步关闭/生效，最终交付由包含本页
的单一 `system-power: ...` Git commit保存。
