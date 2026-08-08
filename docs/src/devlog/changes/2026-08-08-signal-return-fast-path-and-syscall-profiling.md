# ANE-CHG-20260808-signal-return-fast-path-and-syscall-profiling

**Type:** Small optimization / native developer ABI extension
**Status:** Completed
**Date:** 2026-08-08
**Authors:** doruche, Codex
**Area:** signal / user entry / debug / performance observation / syscall / userspace tooling

## Problem / Context

Signal在每次返回用户态前都会扫描task-private pending、ThreadGroup shared pending、mask、reserved delivery和job-control
phase；在没有Signal工作时，这条完整路径仍占据可见的trap-return成本。预生产研究已经证明，一份Signal-owned、只允许
stale-true的保守摘要可以安全消除空扫描，但研究分支的代码形状和诊断工具不是正式实现来源。

同时，既有`debug::perf`只能表达counter和histogram，不能低成本表达“完成次数与累计区间”，也不能按syscall区分
inclusive residence与task active kernel CPU。临时日志能够帮助调查，却不能提供可发现、可聚合且由`perfctl`稳定消费的
正式开发者诊断路径。

## Decision

- Signal新增一份task-local `SignalReturnWork`保守缓存。pending、mask、reservation和job-control phase仍是唯一语义
  真相；producer先发布owner事实，再以Release rearm，当前task以AcqRel exchange消费。stale `true`只产生额外慢扫描，
  `false`只证明本次可以跳过`handle_signals()`。shared pending在发布后重新snapshot current members并rearm；之后加入的
  member由初始armed状态覆盖，因此membership变化不会漏掉已经发布的shared work。
- mandatory user-entry arbitration、lifecycle和job-control final gate保持无条件执行。handler frame、reservation retirement、
  default stop等提前结束扫描的路径重新arm；private/shared pending、mask与temporary-mask owner以及job-control recheck负责
  各自的rearm。
- `debug::perf`增加`Elapsed` kind，wire values固定为`[completed_samples, wrapping_sum_ticks]`。kernel registry、
  `anemone-abi`、`anemone-rs`和userspace工具原子更新；这是developer-facing native ABI，不建立Linux兼容承诺。
- `#[syscall]`生成的唯一raw wrapper是system-wide syscall recording owner。一个guard成对记录wrapper inclusive residence与
  当前task active kernel CPU；restart按每次返回的handler invocation计数。`perf_observe`控制面以及成功路径不返回wrapper
  的exit、exit_group、execve、execveat和power shutdown不注册指标。
- `perfctl syscalls run`在原有全局gate与wrapping snapshot之上编排命令，按kernel CPU累计值排序并报告calls、residence、
  mean kernel CPU、wall比例和reaped-child CPU envelope；输出明确说明system-wide数据不与child CPU守恒。

## Implementation Boundary

Signal拥有保守摘要、rearm点和fast/slow counter；pending queue、mask、temporary restore、reserved delivery与job-control仍由
现有owner解释。`debug::perf`继续唯一拥有gate、registry、per-CPU storage、interval completion和近似聚合；syscall macro只
生成静态metric与raw-wrapper guard，不扩张`SyscallHandler`元数据。`anemone-rs`拥有wire解析与`getrusage` typed wrapper，
`perfctl`只编排公开ABI，不成为kernel session或state owner。

受保护边界是现有Signal pending/mask/action语义、USER-ENTRY final gate、现有syscall返回/errno/restart行为、单一perf gate和
storage真相、feature-off零producer行为以及snapshot的relaxed approximate语义。本轮不引入dynamic filter、session
inheritance、event trace、PMU或Linux perf ABI；这些是没有当前需求的non-goal，而不是为了规避RFC而压缩实现。

若实现需要第二份Signal状态真相、第二个recording gate/owner、改变现有user-entry/current contract，或出现未闭合的
并发与生命周期协议，才需要停止并重新分类；本轮没有触发这些条件。

## Change

- task新增初始armed的Signal return-work缓存，并在private/shared generation、job-control resume、mask与temporary-mask
  mutation、POSIX timer ordinary/job-control publication以及提前结束delivery scan处闭合rearm；增加`signal.user_entry.fast_skip`与
  `signal.user_entry.slow_scan` counter。
- perf registry、declaration/recording宏和timer owner增加`Elapsed`与单一`SyscallProfileGuard`；feature-off宏既不解析
  storage名称也不获取时钟。guard完成时重新取得当前task的accounting snapshot，不跨阻塞syscall持有task `Arc`。
- syscall proc macro默认生成`syscall.<name>.elapsed`和`syscall.<name>.kernel_cpu`；结构性不返回与观测控制面通过显式
  `profile = false`排除，并由registry KUnit验证pair与排除集合。
- `anemone-rs`严格解析新的kind/value count，并增加reaped-children `getrusage` wrapper；`perfctl`支持通用Elapsed输出和
  `syscalls run`报告，普通delta只显示非零metric，避免大catalog淹没诊断输出。
- `perf-test`验证Elapsed catalog形状、syscall pair/排除集合、`perfctl syscalls run`与gate恢复。focused acceptance
  Kconfig显式记录Debug但仍只向console打印Emerg，使原有printk pilot oracle与配置一致。

## Validation

- RV64 feature-on release、SMP=1：579/579 KUnit通过，包括真实mask/timer owner rearm、Elapsed wrapping sum、paired syscall
  completion、registry pair/exclusion与wire codec；`perf-test`的`perfctl run`、`perfctl syscalls run`、gate恢复及printk
  pilot通过，guest完成orderly PowerOff。
- RV64 feature-off release、SMP=1：kernel双pass release build通过，disabled macro KUnit路径成功编译；未运行feature-off
  guest。
- RV64 `perfctl`与`perf-test`release app build通过；Rust格式检查、`git diff --check`和mdBook构建通过。
- 独立代码审阅在最终commit前完成，finding与处置保存在Git diff/commit review事实中。
- **Not Run:** 性能A/B与接受阈值（按计划留给optimize分支）、SMP runtime、LA64、LTP、final harness、physical hardware、
  PMU、动态filter/session/event trace。当前证据证明功能和source-level单向保守性，不声称性能验收或多核运行证明。

## Remaining Risk / Links

- Signal缓存依赖所有eligibility/publication owner先提交真相再rearm；新增Signal mutation路径必须沿用该顺序。缓存不编码
  pending集合、mask或phase，不能反向参与Signal语义决策。
- syscall residence包含阻塞时间，task kernel CPU排除switch-out区间；system-wide relaxed snapshot可能在两个成对value
  更新之间取样，因此单个snapshot边界附近允许短暂的count差异。counter与sum均使用wrapping语义。
- [Kernel performance observation](./2026-08-05-kernel-performance-observation.md)定义既有gate、registry、snapshot与工具
  owner边界。本轮只在该边界内扩展kind和producer。
- [Signal pending routing](../../contracts/signal/pending-routing.md)、
  [temporary-mask delivery](../../contracts/signal/temporary-mask-delivery.md)与
  [user entry](../../contracts/task/user-entry.md)是未变化的依赖；本轮没有current contract cutover。
