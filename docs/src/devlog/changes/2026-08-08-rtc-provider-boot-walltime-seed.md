# ANE-CHG-20260808-rtc-provider-boot-walltime-seed

**Type:** Small Feature / contract-bearing local cutover
**Status:** Completed
**Date:** 2026-08-08
**Authors:** doruche, Codex
**Area:** Device RTC / boot / timekeeper / clock tests

## Problem / Context

common timekeeper原先把realtime offset从零开始，因此没有后续`clock_settime()`时只能观察1970附近的占位日期。内核已有
Goldfish RTC MMIO读取，却只保存driver state，没有把设备能力交给RTC subsystem或timekeeper。与此同时，native clock、
realtime step与soft-timer oracle散落在`user-test`，其中“realtime等于monotonic”和恢复到`monotonic + 1s`的旧假设会
破坏真实boot calendar。

## Decision

- RTC provider capability、origin registry、selection、sealing与一次read属于`device::rtc`；concrete driver只构造并注册
  provider，不能拥有第二份registry或selection policy。
- machine只产生optional stable firmware origin preference；common boot coordinator在discovery后完成RTC finalization/read，
  再把epoch sample一次性交给timekeeper。
- boot seed建立`realtime = monotonic + offset`的初始offset，但不是runtime realtime step，不增加change sequence或通知request
  owner。缺少provider、preference miss、ambiguity、read/arithmetic failure都保留zero-offset fallback。
- `clock_tests`成为clock/time/timer oracle owner，正常成功路径补齐`anemone-rs` wrapper；历史synthetic
  `timer_signal.rs` bridge因真实POSIX timer已覆盖而删除，不移入clock app。
- 本轮只有Goldfish provider。LA64 QEMU虽通过`-rtc base=utc`暴露`loongson,ls7a-rtc`，但没有对应driver/provider；strict
  `clock_tests`在该平台失败是已明确接受的validation边界，不为app增加architecture/caller特判。

## Implementation Boundary

device RTC core唯一拥有provider origins、registry phase、selection与一次read；Goldfish provider唯一拥有MMIO mapping，driver
state与registry共享同一个`Arc`。machine preference不inspect registry；boot coordinator只传递policy与epoch sample；timekeeper
唯一拥有offset/change sequence和所有runtime calendar projection。用户态只经现有Linux clock/time/timer ABI观察结果。

不增加devfs/ioctl、新syscall、dynamic clock、RTC alarm/writeback、runtime resync/hotplug、fallback chain、provider query、
timekeeper-held provider handle或test-only kernel hook。本轮也不实现LS7A RTC driver。

## Change

- 新增boot-only `device::rtc` core及inline KUnit；Goldfish driver向其注册device-owned provider。
- machine boot policy携带optional preferred origin；RV64 QEMU选择`/soc/rtc@101000`，boot coordinator在physical/virtual
  discovery后finalize/read/seed，再进入普通timer与用户态启动。
- timekeeper增加checked boot seed commit，成功和失败都不进入`RealtimeStep`协议。
- `clock_read`、`clock_step`与`soft_timer`移入`clock_tests`；mutation suite保存并恢复初始offset；`user-test`只exec app。
- `anemone-rs`增加现有clock getres/nanosleep/adjtime、timerfd与get/setitimer normal-path wrapper；corner-case ABI检查仍可用raw
  syscall。

## Contract Impact / Cutover

| Contract ID | 变化 | Cutover前baseline | 新规则 | 生效证据 |
| --- | --- | --- | --- | --- |
| [`RTC-BOOT-SEED-001`](../../contracts/time/rtc-boot-seed.md#rtc-boot-seed-001--rtc-只在-boot-handoff-中建立一次-calendar-anchor) | Introduce | 没有RTC core registration/selection/boot handoff contract | device RTC core按stable origin封存、确定性选择、锁外一次read，并向timekeeper交付一个epoch sample | owner-local KUnit、RV64 boot log/runtime oracle、independent review |
| [`TIMEKEEPER-CLOCK-001`](../../contracts/time/clock-derivation.md#timekeeper-clock-001--所有-clock-读取来自一条整数推导链) | Refine | realtime offset只从零开始 | offset从零或一次validated RTC boot seed开始；运行期仍由timekeeper唯一拥有 | timekeeper KUnit、RV64 clock/coarse/mutation/restore oracle |

`TIMEKEEPER-STEP-001`是Dependency：boot seed明确不推进change sequence、不产生step token或通知runtime request owner。代码、
本记录与两项current contract在同一closure commit中原子生效。

## Validation

- `just app build --arch riscv64 clock_tests`与`just app build --arch loongarch64 clock_tests`通过。
- RV64 release kernel build通过；provider-bearing `scripts/run-user-test-rv64.sh`运行通过576/576 KUnit，日志记录Goldfish
  registration/selection/read和seed commit。guest realtime `1786117831555307700`落在host UTC bracket
  `[1786117762718197117, 1786117980006889218]`内；native clocks、realtime mutation/restore、soft timer与真实POSIX timer通过，
  最终输出`clock_tests: clock/time/timer checks passed`并orderly poweroff。
- `just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G`通过；这只证明LA64编译边界，不声明RTC runtime。
- 独立review确认device RTC owner、Goldfish shared `Arc`、seal/read锁边界、boot order、seed arithmetic/change sequence均无
  Apollyon/Keter；指出的synthetic signal owner污染通过删除bridge关闭。LA64 strict oracle failure由维护者明确接受，不增加
  app特判。
- `just fmt kernel`、`just fmt clock_tests`、`just fmt user-test`、`just fmt all --check`与`git diff --check`通过。
- 上述app/kernel/runtime证据在最终删除已被真实POSIX timer替代的synthetic `timer_signal` bridge前取得；该收口只删除
  module/call且不改变RTC、clock或POSIX timer路径，按维护者指示未重跑build/QEMU。
- **Not Run:** LA64 RTC runtime/strict `clock_tests`、LS7A RTC provider、full final harness、physical boards、RTC alarm/writeback/
  hotplug/runtime discipline、suspend correction与long-running stress。socket/LTP结果不作为本轮RTC acceptance。

## Remaining Risk / Links

- [RTC boot seed](../../contracts/time/rtc-boot-seed.md)与
  [clock derivation](../../contracts/time/clock-derivation.md)是effective shared semantics的唯一正文。
- LA64 QEMU平台确有LS7A RTC device；当前缺的是Anemone concrete provider。未来实现该driver时应复用`device::rtc` registration，
  增加stable machine preference及provider-bearing LA64 runtime evidence，而不是修改`clock_tests`绕过当前failure。
- full final harness与实机没有运行；当前walltime证据只覆盖RV64 QEMU Goldfish provider。
