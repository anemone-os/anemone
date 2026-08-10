# ANE-CHG-20260805-kernel-performance-observation

**Type:** Small feature / native developer ABI
**Status:** Completed
**Date:** 2026-08-05
**Authors:** doruche, Codex
**Area:** debug / performance observation / syscall / userspace tooling

## Problem / Context

内核缺少一个能在目标系统内发现指标、控制采集并读取跨CPU聚合结果的原生观测路径。临时日志或host侧计时不能提供
稳定的指标身份、禁用态开销边界或SMP数据面证据；直接把Rust结构体作为syscall image复制，又会继承现有typed-copy
soundness问题。

本轮只建立最小developer-facing observation substrate，并以printk record数量和record构造延迟作为首批真实producer。
它不引入PMU、采样器、reset/session、动态注册、用户自定义事件或Linux `perf_event_open`兼容承诺。

## Decision

- `debug::perf`唯一拥有全局recording gate、静态metric registry、per-CPU storage和聚合规则；producer只通过
  `declare_perf_metrics!`、counter、histogram和timer宏提交窄能力，不读取gate或直接更新storage。
- registry由`.perf_metrics` linker section组成并在boot时校验非空、名称唯一和布局边界。counter使用一个`u64`，
  histogram使用覆盖完整`u64`域的65个log2 bucket；更新与跨CPU读取使用relaxed atomics，溢出按wrapping语义处理。
- recording默认关闭。关闭态在参数求值和取performance clock之前返回；已经armed的timer仍完成一次sample，disable不
  取消in-flight observation。
- `perf_observe`是Anemone native syscall，位于`debug/perf/api/perf_observe.rs`，不因进入`perf` owner而缩写。
  它提供catalog query、gate get/replace和snapshot四种operation；query/get可发现，gate replace与snapshot要求有效
  `CAP_SYS_ADMIN`。
- catalog和snapshot是显式offset定义、native-endian的初始化byte image。kernel使用`UserWriteSlice<u8>`分段copyout，
  values成功写出后才最后发布snapshot header；不新增generic typed-copy调用。
- snapshot在`begin_ticks..=end_ticks`窗口内逐CPU、逐metric近似聚合，不承诺跨CPU原子切片。userspace按wrapping差值
  比较两个snapshot；`perfctl run`在所有普通错误路径恢复进入前的完整gate值，但kernel不建立session owner。

## Implementation Boundary

`debug::perf`拥有gate、registry、storage、timer完成规则和aggregation；printk只拥有两个pilot的语义与调用点；timekeeper
只提供raw tick与frequency窄能力；syscall adapter拥有validation、permission、errno和byte codec；`anemone-rs`拥有image
解析与typed userspace projection；`perfctl`只编排公开wrapper，不成为kernel state owner。

受保护边界是现有printk可见行为、关闭态不求值/不取时钟、单一gate/storage truth、feature-off无handler和metric产物、
显式权限与错误优先级，以及RV64/LA64共用同一ABI。若需要dynamic registry、reset/session、consistent snapshot、PMU、
公开producer registration、Linux perf ABI或第二份gate truth，必须重新分类；本轮未触发这些条件。

## Change

- 增加`SYS_PERF_OBSERVE = SYS_ANEMONE_START + 2`及catalog/descriptor/snapshot wire constants；kernel syscall文件和模块
  均命名为`perf_observe`。
- 增加compile-time `perf_observe` feature、双架构linker section、boot registry validation、per-CPU counter/
  histogram、timer guard和跨online CPU近似aggregation；tracked默认配置启用该feature，但runtime gate仍默认关闭。
- printk接入`debug.printk.records` counter与`debug.printk.record_latency` histogram；producer宏在feature-off时不
  解析metric或求值参数，在runtime-disabled时先检查gate。
- `anemone-rs`增加raw syscall wrapper、严格byte-image parser、typed catalog/snapshot与wrapping delta；增加
  `perfctl list|status|enable|disable|snapshot|run`。
- 增加owner-local KUnit和`perf-test`，覆盖ABI/error precedence、权限、catalog、snapshot、feature-off ENOSYS、
  full-`u64` histogram boundary、disabled macro、SMP remote aggregation和`perfctl` smoke；增加RV64/LA64 focused配置。
- 独立review发现并修正了remote oracle可被setup日志误满足、错误路径可能绕过gate restore、gate/raw update能力过宽、
  catalog测试假定恰好两个metric及histogram边界覆盖不足。最终workload固定为4096次，所有enabled window用
  `Result`收口后恢复gate，更新能力收窄到perf owner，测试允许catalog扩展并覆盖全部power boundary。

## Validation

- RV64 feature-on release、SMP=2：443/443 KUnit通过；`perfctl` list/status/snapshot/run smoke通过；固定CPU0 reader
  观察CPU0 workload得到records/latency samples `4124/4124`，观察CPU1 workload得到`4206/4206`，均高于4096
  oracle；guest完成orderly PowerOff。
- RV64 feature-off release、SMP=1：436/436 KUnit通过；
  `compile_disabled_macros_do_not_resolve_metrics_or_evaluate_arguments`通过；用户态观察`perf_observe`为ENOSYS，
  guest完成orderly PowerOff。
- feature-off ELF、link map、symbol和string审计确认没有`.perf_metrics` entry、syscall handler、pilot descriptor/
  storage/name或`RECORDING_ENABLED`；linker只可能保留相等的零长度section anchors。
- RV64 feature-on release反汇编确认timer先读取gate并在disabled branch返回，之后才执行`rdtime`；printk counter同样
  先读取gate并分支，之后才进入per-CPU update。
- LA64 feature-on release、SMP=1：448/448 KUnit通过；同一catalog/wrapper/snapshot/`perfctl` workflow通过，pilot
  得到`4096/4096`；guest完成orderly shutdown。该QEMU平台没有可用power-off handler，kernel按既有边界进入halt，
  随后才由QEMU escape结束host进程，完整日志包含两者。
- `perf-test`应用在RV64和LA64均完成release build；独立source/evidence review确认没有Apollyon、Keter、Euclid或
  blocking finding，并确认syscall文件与模块均保持`perf_observe`全名。
- **Not Run:** physical hardware、full LTP、final harness、真实应用benchmark、PMU、strict overhead/latency bound、
  counter exhaustion，以及观测功能带来的实际speedup。当前证据只证明功能、关闭态code shape和focused pilot，不把
  QEMU计时结果表述为性能结论。

## Remaining Risk / Links

- snapshot是relaxed-atomic的近似聚合：不同CPU和不同metric可能来自窗口内不同瞬间；begin/end ticks用于暴露窗口，
  不是一致性证明。并发producer、gate切换和counter wrap都可能影响相邻snapshot delta。
- gate是全局replacement state，不提供租约、引用计数或多caller session isolation。`perfctl run`保证自己的普通退出
  路径恢复旧值，但并发控制者必须自行协调。
- histogram保留raw monotonic tick bucket；catalog提供每架构frequency，工具或开发者负责换算，不能跨架构直接比较
  bucket index对应的wall time。
- [`ANE-20260805-USER-ACCESS-TYPED-COPY-SOUNDNESS`](../../register/open-issues.md#ane-20260805-user-access-typed-copy-soundness)
  仍保持Open；本轮通过显式初始化byte codec避免扩大该问题，不声称修复generic typed-copy边界。
- Runtime evidence保存在build-local日志中，不进入public source。RFC / current contract / transaction：None；本记录、
  代码、工具与Git commit共同保存本轮实现事实。
