# ANE-CHG-20260729-minimal-global-membarrier

**Type:** Small Feature / Linux ABI Compatibility
**Status:** Completed
**Date:** 2026-07-29
**Authors:** EDGW, Codex
**Area:** syscall ABI / IPI transport / scheduler context switch

## Problem

Anemone 没有注册 asm-generic `membarrier(2)` syscall 283，libc 与 LTP 启动探测会打印
`unknown syscall number 283`。直接让所有命令返回成功会错误承诺 private expedited、
sync-core 和 rseq 语义；仅让 `QUERY` 返回 0 虽然 ABI 诚实，但不提供可用的内存屏障
能力。

本轮需要一个性能可以很差、但成功返回范围可证明的最小实现。它只公布无需注册状态的
`MEMBARRIER_CMD_GLOBAL`，把所有在线 CPU 作为目标，避免引入 Linux 的 per-mm、
per-runqueue 注册缓存与目标筛选状态。

## Scope

- 注册 RV64 / LA64 共用的 asm-generic syscall 号 283；
- 支持 `MEMBARRIER_CMD_QUERY` 与 `MEMBARRIER_CMD_GLOBAL`；
- `GLOBAL` 在调用 CPU 前后执行 full data-memory fence，并同步广播只执行 full fence 的
  IPI；
- scheduler 在每次 outgoing-to-incoming task execution boundary 执行 full fence，覆盖
  睡眠、换出和迁移与 IPI 的交错；
- 增加命令/flags 矩阵和同步 IPI transport KUnit。

本轮不支持 global/private expedited 注册、private expedited、sync-core、rseq、CPU-target
flag 或 registrations query；不实现 Linux 的 RCU grace-period、`nohz_full`、per-mm target
filter 或 CPU hotplug 等价语义。

## Solution

`sys_membarrier()` 是 Linux ABI 的唯一解析 owner。`QUERY` 只返回
`MEMBARRIER_CMD_GLOBAL` bit；非零 flags、未知命令和未支持命令统一返回 `EINVAL`。
`cpu_id` 在唯一支持的 flags=0 命令中按 Linux ABI 忽略。

`GLOBAL` 的 protocol owner 在 syscall 模块：先执行 `SeqCst` full fence，再通过现有同步
`broadcast_ipi()` 向所有其它在线 CPU 发送无状态 `MemoryBarrier` capability；IPI handler
只执行 `SeqCst` full fence 并发布已有 completion，调用方等待全部 completion 后执行第二个
full fence。IPI transport 继续唯一拥有 message queue、completion 与分配失败；syscall 将
allocation failure 映射为 `ENOMEM`，target-offline 竞态映射为 `EAGAIN`，失败调用不宣称完成
barrier。

仅广播 CPU fence 不能单独覆盖 task 在两个目标 CPU 各自 fence 之间迁移的交错。为保持
直接且不引入第二套 task/mm snapshot，统一 scheduler loop 在旧 task 已停止、下一 task 尚未
恢复之间无条件执行同一个 full fence。该成本是本轮明确接受的性能让步。

## Change

- RV64 / LA64 ABI 增加共用的 `SYS_MEMBARRIER=283`，syscall 模块增加独立 ABI adapter。
- `QUERY` 只返回 `MEMBARRIER_CMD_GLOBAL`；未支持命令、未知命令和非零 flags 返回
  `EINVAL`，flags=0 时忽略 `cpu_id`。
- sync owner 增加一个命名的 full data-memory barrier primitive；RV64 / LA64 release
  codegen分别落为 `fence rw, rw` 与 `dbar 16`。
- IPI transport 增加可广播、无状态的 `MemoryBarrier` payload；handler只执行 full fence后
  发布已有 completion。
- syscall 在同步 broadcast 前后执行 full fence并等待全部 completion；allocation failure映射
  `ENOMEM`，target-offline映射`EAGAIN`。
- scheduler统一切换点在 `local_pick_next()` 后、mapping/context restore前执行 full fence，
  mapping相同也不跳过。
- KUnit覆盖query bit、invalid command/flags、payload copy和live global rendezvous。

## Contract Impact / Cutover

| Contract ID | 变化 | Cutover 前 effective baseline | 新 effective 规则 | 生效证据 |
| --- | --- | --- | --- | --- |
| `MEMBARRIER-GLOBAL-001` | Introduce | None；syscall 283 未注册 | 仅 `GLOBAL` 成功建立调用方前后 fence、全 CPU 同步 fence IPI 与 task-switch fence 组成的 rendezvous | source audit；RV64 SMP=2 272/272 KUnit；双架构 release build / disassembly |

代码、[current contract](../../contracts/membarrier/global-rendezvous.md) 和限制记录在本完成
checkpoint原子生效，没有 partial/transitional contract。

## Validation

- `just build --preset qemu-virt-rv64-release --bind smp=2 --bind memory=1G --disasm`
  通过；syscall前后、`MemoryBarrier` handler与scheduler切换点均观察到 `fence rw, rw`。
- `just build --preset qemu-virt-la64-release --bind smp=2 --bind memory=1G --disasm`
  通过；相同三个位置均观察到 `dbar 16`，其 `orwrw` ordering hint对应LA64
  `__smp_mb()`，不是TLB或instruction-stream fence。
- RV64 SMP=2 QEMU使用本轮kernel和现有`build/rootfs/visionfive2/rootfs.img`完成
  272/272 KUnit；
  `test_global_memory_barrier_rendezvous_completes` 实际等待第二个CPU处理同步IPI，命令矩阵和
  payload-copy测试通过。日志为`build/membarrier-rv64-smp2.log`。
- 上述fixture initial program在KUnit之后立即退出并触发既有`init task shall not exit` panic，
  随后正常PowerOff；因此本运行只证明KUnit和同步IPI，不记作完整boot/userspace regression。
- `just fmt kernel --check` 的本轮文件达到formatter期望，但命令整体仍停在未修改的vendored
  smoltcp既有差异；未把全kernel格式检查写成通过。
- `git diff --check`通过；当前主机没有`mdbook`命令，文档渲染门Not Run。
- 用户态raw syscall、glibc fallback、LTP membarrier、LA64 runtime、hardware、CPU hotplug、
  并发barrier stress和延迟测试Not Run。

## Tracking Issues

### CHG-001 - 跨 CPU rendezvous 不能形成同步 IPI 环

**Status:** Neutralized
**Severity:** Keter

**Issue:** 现有同步 IPI transport 通过忙等 completion 收口；若 MemoryBarrier handler 获取
调用方持有的锁、分配、调度或发起反向同步 IPI，会形成不可审计的 cross-CPU wait cycle。

**Resolution:** final handler保持无状态，只执行 full fence 后发布已有 completion；syscall
断言local interrupts enabled，不持有或访问scheduler/IPI owner lock。RV64 SMP=2 live
rendezvous完成且source audit未发现handler分配、调度、加业务锁或反向IPI，本问题关闭。

### CHG-002 - Rust fence 必须生成目标架构 full data fence

**Status:** Neutralized
**Severity:** Keter

**Issue:** `SeqCst` 只是源码意图；如果 RV64 / LA64 codegen 未生成 `fence rw,rw` / full
`dbar`，成功返回的 `GLOBAL` 不成立。`sfence.vma` 与 `fence.i` 不能替代该数据屏障。

**Resolution:** final action-local disassembly在syscall前后、远端handler与scheduler切换点分别
观察到RV64 `fence rw, rw`和LA64 `dbar 16`；LoongArch barrier hint定义确认`16/orwrw`是
full SMP ordering barrier，本问题关闭。

## Risk / Follow-up

- 每次 task switch 的 full fence 与每次 `GLOBAL` 的 all-CPU synchronous IPI 成本很高，属于
  本轮接受的性能代价。
- expedited、private、sync-core、rseq 与 registrations query 保持未支持并由
  [current limitations](../../register/current-limitations.md#ane-20260729-membarrier-global-only)
  跟踪；后续实现不得复用本轮 `QUERY` bit 冒充更强能力。
- 当前没有用户态raw syscall/LTP证据；若ABI wrapper或libc probe暴露参数/errno差异，应作为
  `MEMBARRIER-GLOBAL-001`范围内缺陷修复，不能改写成已接受限制。
- CPU hotplug 不在本轮范围；稳定在线拓扑以外的 target-offline 只返回 `EAGAIN`，不声称
  barrier 成功。

## Links

- Biweekly devlog: [2026-07-20 至 2026-08-02](../2026-07-20_to_2026-08-02.md)
- Current contract: [`MEMBARRIER-GLOBAL-001`](../../contracts/membarrier/global-rendezvous.md#membarrier-global-001--成功的-global-建立全-cpu-full-fence-rendezvous)
- Register / limitations: [Global-only membarrier](../../register/current-limitations.md#ane-20260729-membarrier-global-only), [Signal LTP infrastructure stage 1](../../register/current-limitations.md#ane-20260607-signal-ltp-infra-stage1)
- RFC / transaction: None
- External source evidence: None
- Issue / PR / commit: current workspace diff；no commit created
